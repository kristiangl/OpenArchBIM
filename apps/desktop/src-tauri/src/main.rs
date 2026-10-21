#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use openarch_api::Session;

/// Every UI request goes through here. The logic lives in `openarch-api`, which
/// is also what the browser dev server calls, so both behave identically.
#[tauri::command]
fn rpc(state: tauri::State<Mutex<Session>>, method: String, params: serde_json::Value) -> Result<serde_json::Value, String> {
    state.lock().map_err(|_| "project state is unavailable".to_string())?.handle(&method, &params)
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
        .manage(Mutex::new(Session::new()))
        .invoke_handler(tauri::generate_handler![rpc])
        .run(tauri::generate_context!())
        .expect("error while running OpenArch BIM");
}
