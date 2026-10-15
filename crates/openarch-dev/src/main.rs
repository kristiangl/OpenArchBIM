//! Dev server: serves the UI folder and the JSON API over HTTP, so the whole
//! app can be run and tested in an ordinary browser without the Tauri shell.
//!
//!   cargo run -p openarch-dev [-- --port 8765 --ui apps/desktop/ui]

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
                let _ = req.respond(Response::from_data(bytes).with_header(h));
            }
            _ => {
                let _ = req.respond(Response::from_string("not found").with_status_code(404));
            }
        }
    }
}
