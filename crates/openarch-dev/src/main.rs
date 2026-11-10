//! Dev server: serves the UI folder and the JSON API over HTTP, so the whole
//! app can be run and tested in an ordinary browser without the Tauri shell.
//!
//!   cargo run -p openarch-dev [-- --port 8765 --ui apps/desktop/ui]

mod native_dialog;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openarch_api::Session;
use tiny_http::{Header, Method, Response, Server};

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// The page's content security policy, the same as the desktop app's (`security.csp` in tauri.conf.json, which
/// also lets the app's own channel to the core through). Only the page's own scripts run, so text that came in
/// with a file (a name in an IFC file, say) can never run as script. Keep the two in step.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'";

/// Whether a request to the core comes from the dev server's own page. A browser names the page that made a
/// request in `Origin`, so this stops any other web page open in the same browser from driving the project
/// (which can read and write files). Tools that send no `Origin` (curl, test scripts) are let through.
fn own_page(req: &tiny_http::Request, port: &str) -> bool {
    match req.headers().iter().find(|h| h.field.equiv("Origin")) {
        None => true,
        Some(h) => {
            let origin = h.value.as_str();
            origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}")
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str, default: &str| {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string())
    };
    let port = arg("--port", "8765");
    let ui = PathBuf::from(arg("--ui", "apps/desktop/ui"));
    let server = Server::http(format!("127.0.0.1:{port}")).expect("bind");
    println!("OpenArch BIM dev server on http://127.0.0.1:{port}  (ui: {})", ui.display());
    let session = Mutex::new(Session::new());

    for mut req in server.incoming_requests() {
        let url = req.url().split('?').next().unwrap_or("/").to_string();
        if req.method() == &Method::Post && !own_page(&req, &port) {
            let _ = req.respond(Response::from_string("forbidden").with_status_code(403));
            continue;
        }
        if req.method() == &Method::Post && url == "/dialog" {
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            std::thread::spawn(move || {
                let out = serde_json::from_str::<serde_json::Value>(&body).map_err(|e| e.to_string())
                    .and_then(|v| native_dialog::pick(&v))
                    .unwrap_or_else(|e| serde_json::json!({ "err": e }));
                let header = Header::from_bytes("Content-Type", "application/json").unwrap();
                let _ = req.respond(Response::from_string(out.to_string()).with_header(header));
            });
            continue;
        }
        if req.method() == &Method::Post && url == "/rpc" {
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            let out = match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) => {
                    let method = v["method"].as_str().unwrap_or("").to_string();
                    let r = session.lock().unwrap().handle(&method, &v["params"]);
                    match r {
                        Ok(ok) => serde_json::json!({ "ok": ok }),
                        Err(e) => serde_json::json!({ "err": e }),
                    }
                }
                Err(e) => serde_json::json!({ "err": e.to_string() }),
            };
            let h = Header::from_bytes("Content-Type", "application/json").unwrap();
            let _ = req.respond(Response::from_string(out.to_string()).with_header(h));
            continue;
        }
        let rel = if url == "/" { "index.html" } else { url.trim_start_matches('/') };
        // keep requests inside the UI folder
        let ok = !rel.contains("..") && !rel.contains('\\');
        let path = ui.join(rel);
        match (ok, std::fs::read(&path)) {
            (true, Ok(bytes)) => {
                let h = Header::from_bytes("Content-Type", content_type(&path)).unwrap();
                let csp = Header::from_bytes("Content-Security-Policy", CSP).unwrap();
                let _ = req.respond(Response::from_data(bytes).with_header(h).with_header(csp));
            }
            _ => {
                let _ = req.respond(Response::from_string("not found").with_status_code(404));
            }
        }
    }
}
