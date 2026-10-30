#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use openarch_api::Session;
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

type Answer = Result<serde_json::Value, String>;

// ---- the log ---------------------------------------------------------------------------------
// A plain text file in the app's log folder (Help > Open Log Folder). Every core call is written with how long it
// took, plus any call still running after a few seconds, page errors and unhandled promise rejections, and the
// window's close requests. A freeze leaves the last call and the close request in the file, which is what a report
// needs. Times are UTC, so no time zone database is needed.

/// A core call that runs longer than this is written to the log while it is still going.
const SLOW_CALL: Duration = Duration::from_secs(5);
/// The page beats every 2 s; this long without a beat is written down as a stuck page.
const PAGE_SILENT: Duration = Duration::from_secs(8);
/// The log is moved aside once at start-up past this size, so the file cannot grow without end.
const LOG_LIMIT: u64 = 8 * 1024 * 1024;

struct Log {
    dir: PathBuf,
    file: Mutex<Option<File>>,
    /// The page's last heartbeat (`ui_alive`), and whether its silence has been written down.
    alive: Mutex<(Option<Instant>, bool)>,
}

impl Log {
    fn open(dir: PathBuf) -> Log {
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("openarch.log");
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_LIMIT) {
            let old = dir.join("openarch.log.1");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&path, old);
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        let log = Log { dir, file: Mutex::new(file), alive: Mutex::new((None, false)) };
        log.line(&format!("OpenArch BIM {} opened the log, times are UTC", env!("CARGO_PKG_VERSION")));
        log
    }

    fn line(&self, text: &str) {
        let Ok(mut file) = self.file.lock() else { return };
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{} {text}", stamp_now());
            let _ = f.flush();
        }
    }
}

/// The time of day in UTC, as 2026-10-23T04:12:33.123Z.
fn stamp_now() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (y, mo, day) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, (rem % 3600) / 60, rem % 60, d.subsec_millis())
}

/// Days since 1 Jan 1970 to a calendar date (Howard Hinnant's civil_from_days).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Shows a folder in the system's file manager.
fn open_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program).arg(dir).spawn().map(|_| ())
}

/// A core call that has been sent and not answered yet.
struct Call {
    method: String,
    started: Instant,
    warned: bool,
}

/// The project lives on a worker thread of its own, so a long request never stops the window from drawing or
/// answering. Every UI request goes through `rpc_send`, which runs on the main thread and so queues requests in the
/// order the page makes them; the worker answers them in that order (as the browser dev server does), and the page
/// collects each answer with `rpc_wait`. The logic lives in `openarch-api`, which the dev server calls too.
struct Core {
    jobs: Mutex<mpsc::Sender<(String, serde_json::Value, oneshot::Sender<Answer>)>>,
    waiting: Mutex<HashMap<u64, oneshot::Receiver<Answer>>>,
    running: Mutex<HashMap<u64, Call>>,
    next: AtomicU64,
}

impl Core {
    fn start() -> Core {
        let (tx, rx) = mpsc::channel::<(String, serde_json::Value, oneshot::Sender<Answer>)>();
        std::thread::spawn(move || {
            let mut session = Session::new();
            for (method, params, reply) in rx {
                let _ = reply.send(session.handle(&method, &params));
            }
        });
        Core { jobs: Mutex::new(tx), waiting: Mutex::new(HashMap::new()), running: Mutex::new(HashMap::new()), next: AtomicU64::new(1) }
    }
}

/// Writes any core call still running after a few seconds to the log, so a freeze names the call it was in.
fn watch_slow_calls(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let mut slow = Vec::new();
        {
            let core = app.state::<Core>();
            let calls = core.running.lock();
            if let Ok(mut running) = calls {
                for (ticket, call) in running.iter_mut() {
                    if !call.warned && call.started.elapsed() >= SLOW_CALL {
                        call.warned = true;
                        slow.push(format!("call {ticket} {} still running after {:.0} s", call.method, call.started.elapsed().as_secs_f64()));
                    }
                }
            }
        }
        for line in slow {
            app.state::<Log>().line(&line);
        }
        // the page beats every 2 s; silence for longer means its script has stopped (the window cannot redraw either)
        let log = app.state::<Log>();
        let silent = match log.alive.lock() {
            Ok(mut a) => match a.0 {
                Some(t) if !a.1 && t.elapsed() >= PAGE_SILENT => {
                    a.1 = true;
                    Some(t.elapsed().as_secs_f64())
                }
                _ => None,
            },
            Err(_) => None,
        };
        if let Some(secs) = silent {
            let last = app.state::<Core>().running.lock().ok().and_then(|r| r.values().map(|c| c.method.clone()).next());
            log.line(&format!("the page has not answered for {secs:.0} s (its script is stuck){}", last.map(|m| format!("; core call still running: {m}")).unwrap_or_default()));
        }
    });
}

#[tauri::command]
fn rpc_send(core: tauri::State<Core>, log: tauri::State<Log>, method: String, params: serde_json::Value) -> Result<u64, String> {
    let ticket = core.next.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    core.waiting.lock().map_err(|_| "the project is unavailable".to_string())?.insert(ticket, rx);
    core.running.lock().map_err(|_| "the project is unavailable".to_string())?.insert(ticket, Call { method: method.clone(), started: Instant::now(), warned: false });
    let queued = core.jobs.lock().map(|jobs| jobs.send((method.clone(), params, tx)).is_ok()).unwrap_or(false);
    if !queued {
        if let Ok(mut waiting) = core.waiting.lock() { waiting.remove(&ticket); }
        if let Ok(mut running) = core.running.lock() { running.remove(&ticket); }
        log.line(&format!("call {ticket} {method} could not be queued: the project has stopped"));
        return Err("the project has stopped".to_string());
    }
    Ok(ticket)
}

#[tauri::command]
async fn rpc_wait(core: tauri::State<'_, Core>, log: tauri::State<'_, Log>, ticket: u64) -> Answer {
    let rx = core.waiting.lock().map_err(|_| "the project is unavailable".to_string())?.remove(&ticket).ok_or("no such request")?;
    let answer = rx.await.map_err(|_| "the project has stopped".to_string())?;
    if let Ok(mut running) = core.running.lock() {
        if let Some(call) = running.remove(&ticket) {
            let ms = call.started.elapsed().as_secs_f64() * 1000.0;
            match &answer {
                Ok(_) => log.line(&format!("call {ticket} {} {ms:.1} ms ok", call.method)),
                Err(e) => log.line(&format!("call {ticket} {} {ms:.1} ms failed: {e}", call.method)),
            }
        }
    }
    answer
}

/// When the window was last asked to close and the page has not answered yet.
struct CloseRequest(Mutex<Option<Instant>>);

/// The page has the close request: it asks about unsaved work, then closes the program with `quit`.
#[tauri::command]
fn close_ack(state: tauri::State<CloseRequest>, log: tauri::State<Log>) {
    if let Ok(mut p) = state.0.lock() {
        *p = None;
    }
    log.line("the page answered the close request");
}

#[tauri::command]
fn quit(app: tauri::AppHandle, log: tauri::State<Log>) {
    log.line("quit");
    app.exit(0);
}

/// A page error or an unhandled promise rejection, sent by the UI so it lands in the same log as the core calls.
#[tauri::command]
fn ui_log(log: tauri::State<Log>, level: String, message: String) {
    let text: String = message.chars().take(4000).collect();
    log.line(&format!("page {level}: {}", text.replace('\n', " | ")));
}

/// The page's heartbeat. After a silence it also notes that the page came back.
#[tauri::command]
fn ui_alive(log: tauri::State<Log>) {
    let was = log.alive.lock().ok().map(|mut a| {
        let first = a.0.is_none();
        let back = a.1.then(|| a.0.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0));
        *a = (Some(Instant::now()), false);
        (first, back)
    });
    match was {
        Some((true, _)) => log.line("the page is running (it checks in every 2 s)"),
        Some((_, Some(secs))) => log.line(&format!("the page answered again after {secs:.0} s")),
        _ => {}
    }
}

/// Shows the folder that holds the log, so a frozen session's log can be sent on.
#[tauri::command]
fn open_log_folder(log: tauri::State<Log>) -> Result<String, String> {
    let _ = std::fs::create_dir_all(&log.dir);
    open_dir(&log.dir).map_err(|e| format!("cannot open {}: {e}", log.dir.display()))?;
    Ok(log.dir.display().to_string())
}

/// Linux: WebKitGTK's DMA-BUF renderer shows a blank window with the NVIDIA proprietary driver
/// (common on Wayland). Turn it off when that driver is loaded, unless the person set it themselves.
#[cfg(target_os = "linux")]
fn linux_webview_workarounds() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() && std::path::Path::new("/proc/driver/nvidia").exists() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    linux_webview_workarounds();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Core::start())
        .manage(CloseRequest(Mutex::new(None)))
        .setup(|app| {
            // The log sits in the app's own folder (%LOCALAPPDATA%\dev.oab.app\logs on Windows).
            let dir = app.path().app_log_dir()
                .or_else(|_| app.path().app_data_dir().map(|p| p.join("logs")))
                .unwrap_or_else(|_| std::env::temp_dir().join("openarch-logs"));
            app.manage(Log::open(dir));
            watch_slow_calls(app.handle().clone());
            // the page draws the one top bar; macOS keeps its own window buttons, over the bar's left end
            #[cfg(target_os = "macos")]
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_decorations(true);
                let _ = w.set_title_bar_style(tauri::TitleBarStyle::Overlay);
            }
            Ok(())
        })
        // Closing the window asks the page first, which offers to save unsaved work. A page that has not answered
        // an earlier request within five seconds is not listening (it has stopped), so then the window just closes.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<CloseRequest>();
                let log = window.state::<Log>();
                let Ok(mut pending) = state.0.lock() else { return };
                if pending.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
                    log.line("the page did not answer the close request in 5 s, so the window closes");
                    return;
                }
                log.line(if pending.is_some() {
                    "window close requested again while the page is still deciding"
                } else {
                    "window close requested, asking the page about unsaved work"
                });
                api.prevent_close();
                *pending = Some(Instant::now());
                let _ = window.emit("close-requested", ());
            }
        })
        .invoke_handler(tauri::generate_handler![rpc_send, rpc_wait, close_ack, quit, ui_log, ui_alive, open_log_folder])
        .run(tauri::generate_context!())
        .expect("error while running OpenArch BIM");
}
