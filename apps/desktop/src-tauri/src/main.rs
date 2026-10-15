#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use openarch_api::Session;

/// Every UI request goes through here. The logic lives in `openarch-api`, which
/// is also what the browser dev server calls, so both behave identically.
#[tauri::command]
fn rpc(state: tauri::State<Mutex<Session>>, method: String, params: serde_json::Value) -> Result<serde_json::Value, String> {
    state.lock().map_err(|_| "project state is unavailable".to_string())?.handle(&method, &params)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(Session::new()))
        .invoke_handler(tauri::generate_handler![rpc])
        .run(tauri::generate_context!())
        .expect("error while running OpenArch BIM");
}
