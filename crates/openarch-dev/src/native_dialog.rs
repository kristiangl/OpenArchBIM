use serde_json::{json, Value};

#[cfg(windows)]
pub fn pick(request: &Value) -> Result<Value, String> {
    let kind = request["kind"].as_str().ok_or("missing dialog kind")?;
    if !["save", "open"].contains(&kind) { return Err("unknown dialog kind".into()); }
    let options = &request["options"];
    let mut dialog = rfd::FileDialog::new();
    if let Some(title) = options["title"].as_str() { dialog = dialog.set_title(title); }
    if let Some(path) = options["defaultPath"].as_str() {
        let path = std::path::Path::new(path);
        if let Some(name) = path.file_name() { dialog = dialog.set_file_name(name.to_string_lossy()); }
        if let Some(parent) = path.parent().filter(|p| p.is_dir()) { dialog = dialog.set_directory(parent); }
    }
    for filter in options["filters"].as_array().into_iter().flatten() {
        let name = filter["name"].as_str().unwrap_or("Files");
        let extensions: Vec<&str> = filter["extensions"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect();
        dialog = dialog.add_filter(name, &extensions);
    }
    if kind == "save" { return Ok(json!({ "ok": dialog.save_file() })); }
    if options["directory"].as_bool().unwrap_or(false) { return Ok(json!({ "ok": dialog.pick_folder() })); }
    if options["multiple"].as_bool().unwrap_or(false) { return Ok(json!({ "ok": dialog.pick_files() })); }
    Ok(json!({ "ok": dialog.pick_file() }))
}

#[cfg(not(windows))]
pub fn pick(_request: &Value) -> Result<Value, String> {
    Ok(json!({ "unsupported": true }))
}
