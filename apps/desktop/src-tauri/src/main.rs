#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use openarch_api::Session;
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

type Answer = Result<serde_json::Value, String>;

/// The project lives on a worker thread of its own, so a long request never stops the window from drawing or
/// answering. Every UI request goes through `rpc_send`, which runs on the main thread and so queues requests in the
/// order the page makes them; the worker answers them in that order (as the browser dev server does), and the page
/// collects each answer with `rpc_wait`. The logic lives in `openarch-api`, which the dev server calls too.
struct Core {
    jobs: Mutex<mpsc::Sender<(String, serde_json::Value, oneshot::Sender<Answer>)>>,
    waiting: Mutex<HashMap<u64, oneshot::Receiver<Answer>>>,
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
        Core { jobs: Mutex::new(tx), waiting: Mutex::new(HashMap::new()), next: AtomicU64::new(1) }
    }
}

#[tauri::command]
fn rpc_send(core: tauri::State<Core>, method: String, params: serde_json::Value) -> Result<u64, String> {
    let ticket = core.next.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    core.waiting.lock().map_err(|_| "the project is unavailable".to_string())?.insert(ticket, rx);
    core.jobs.lock().map_err(|_| "the project is unavailable".to_string())?.send((method, params, tx)).map_err(|_| "the project has stopped".to_string())?;
    Ok(ticket)
}

#[tauri::command]
async fn rpc_wait(core: tauri::State<'_, Core>, ticket: u64) -> Answer {
    let rx = core.waiting.lock().map_err(|_| "the project is unavailable".to_string())?.remove(&ticket).ok_or("no such request")?;
    rx.await.map_err(|_| "the project has stopped".to_string())?
}

/// When the window was last asked to close and the page has not answered yet.
struct CloseRequest(Mutex<Option<Instant>>);

/// The page has the close request: it asks about unsaved work, then closes the program with `quit`.
#[tauri::command]
fn close_ack(state: tauri::State<CloseRequest>) {
    if let Ok(mut p) = state.0.lock() {
        *p = None;
    }
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
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
        // Closing the window asks the page first, which offers to save unsaved work. A page that has not answered
        // an earlier request within five seconds is not listening (it has stopped), so then the window just closes.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<CloseRequest>();
                let Ok(mut pending) = state.0.lock() else { return };
                if pending.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
                    return;
                }
                api.prevent_close();
                *pending = Some(Instant::now());
                let _ = window.emit("close-requested", ());
            }
        })
        .invoke_handler(tauri::generate_handler![rpc_send, rpc_wait, close_ack, quit])
        .run(tauri::generate_context!())
        .expect("error while running OpenArch BIM");
}
