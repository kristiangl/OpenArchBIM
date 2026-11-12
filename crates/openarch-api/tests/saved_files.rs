//! Project files saved by earlier builds still open, hold what they held, draw, and save again.
//!
//! `tests/fixtures/projects` has one project per build, named by the date and commit that saved it, each with a
//! `.json` beside it saying what it held when it was saved: the format version, how many elements of each kind,
//! and the area of each room. Add a file from each installer build (see the README there). A file is never
//! replaced or re-saved: the point is that the build of today reads the bytes an old build wrote.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projects");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())).flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "oab")).collect();
    found.sort();
    found
}

fn kinds(s: &Session) -> BTreeMap<String, u64> {
    let mut count = BTreeMap::new();
    for (_, e) in s.doc.iter() {
        *count.entry(e.kind().to_string()).or_insert(0) += 1;
    }
    count
}

fn area(s: &mut Session, room: u64) -> String {
    let d = call(s, "describe", json!({ "id": room }));
    d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap()).find(|r| r["label"] == "Area").unwrap()["value"].as_str().unwrap().to_string()
}

/// Draws every view the project has, so a file that opens but cannot be shown is caught too.
fn draw_everything(s: &mut Session, name: &str) {
    let info = call(s, "doc_info", json!({}));
    for v in info["views"].as_array().unwrap() {
        let id = v["id"].as_u64().unwrap();
        for method in ["plan", "elevation", "schedule_table"] {
            // a view answers the command for its own kind and refuses the others; a panic is what must not happen
            if let Err(e) = s.handle(method, &json!({ "view_id": id, "id": id })) {
                assert!(!e.contains("internal error"), "{name}: {method} of {}: {e}", v["name"]);
            }
        }
    }
    for sheet in info["sheets"].as_array().unwrap() {
        call(s, "sheet", json!({ "id": sheet["id"] }));
    }
    assert!(!call(s, "scene", json!({}))["meshes"].as_array().unwrap().is_empty(), "{name}: nothing in 3D");
}

#[test]
fn projects_saved_by_earlier_builds_open_draw_and_save_again() {
    let files = fixtures();
    assert!(files.len() >= 2, "the saved projects are missing from tests/fixtures/projects");
    let scratch = std::env::temp_dir().join(format!("openarch-saved-files-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let held: Value = serde_json::from_str(&std::fs::read_to_string(file.with_extension("json")).unwrap_or_else(|e| panic!("{name} has no .json beside it: {e}"))).unwrap();
        // work on a copy: the saved file itself is never written to
        let copy = scratch.join(&name);
        std::fs::copy(&file, &copy).unwrap();

        let mut s = Session::new();
        call(&mut s, "open", json!({ "path": copy.to_string_lossy() }));
        assert_eq!(call(&mut s, "doc_info", json!({}))["dirty"], false, "{name}: opening made a change to save");

        // everything it held is still there (a build may add defaults of its own, never take away)
        let now = kinds(&s);
        for (kind, n) in held["kinds"].as_object().unwrap() {
            let have = now.get(kind).copied().unwrap_or(0);
            assert!(have >= n.as_u64().unwrap(), "{name}: it held {n} {kind}, now {have}");
        }
        // and measures what it measured
        let rooms = held["room_areas"].as_object().unwrap();
        for (room, was) in rooms {
            assert_eq!(&area(&mut s, room.parse().unwrap()), was.as_str().unwrap(), "{name}: room {room}");
        }
        draw_everything(&mut s, &name);

        // saved by this build and opened again, it is the same project
        let before = kinds(&s);
        call(&mut s, "save", json!({ "path": copy.to_string_lossy() }));
        let mut again = Session::new();
        call(&mut again, "open", json!({ "path": copy.to_string_lossy() }));
        assert_eq!(kinds(&again), before, "{name}: saving and opening again changed what it holds");
        for (room, was) in rooms {
            assert_eq!(&area(&mut again, room.parse().unwrap()), was.as_str().unwrap(), "{name}: room {room} after saving again");
        }
        draw_everything(&mut again, &name);
        // an edit still works on it, and can be undone
        let info = call(&mut again, "doc_info", json!({}));
        let (level, wall) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][0]["id"].as_u64().unwrap());
        call(&mut again, "add_wall", json!({ "type_id": wall, "level_id": level, "start": [50000, 0], "end": [54000, 0], "height_mm": 2700 }));
        assert!(call(&mut again, "undo", json!({}))["done"].is_string(), "{name}: undo");
        assert_eq!(kinds(&again), before, "{name}: undo did not put it back");
    }
    std::fs::remove_dir_all(scratch).ok();
}

/// Saving over the project's file keeps the file as it was as a numbered backup, as many as the project asks for.
#[test]
fn saving_from_the_program_keeps_backups_and_the_number_is_the_projects() {
    let dir = std::env::temp_dir().join(format!("openarch-api-backups-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Job.oab");
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    assert_eq!(info["max_backups"], 5);
    let (level, wall) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][0]["id"].as_u64().unwrap());
    let first = call(&mut s, "save", json!({ "path": path.to_string_lossy() }));
    assert!(first["backup"].is_null() && first["backups"] == 0, "{first}");
    for n in 1..=7 {
        call(&mut s, "add_wall", json!({ "type_id": wall, "level_id": level, "start": [0, n * 1000], "end": [4000, n * 1000], "height_mm": 2700 }));
        let saved = call(&mut s, "save", json!({}));
        assert!(saved["backup"].as_str().unwrap().ends_with(&format!("Job.{n:04}.oab")), "{saved}");
        assert_eq!(saved["backups"], n.min(5), "{saved}");
    }
    // five kept: 0003 to 0007
    assert!(!dir.join("Job.0002.oab").exists() && dir.join("Job.0003.oab").exists() && dir.join("Job.0007.oab").exists());
    // the number is changed in the project, which then has something to save, and it comes back with the file
    assert_eq!(call(&mut s, "doc_info", json!({}))["dirty"], false);
    call(&mut s, "set_max_backups", json!({ "count": 2 }));
    assert_eq!(call(&mut s, "doc_info", json!({}))["dirty"], true);
    assert!(s.handle("set_max_backups", &json!({ "count": 500 })).is_err());
    assert_eq!(call(&mut s, "save", json!({}))["backups"], 2);
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": path.to_string_lossy() }));
    assert_eq!(call(&mut again, "doc_info", json!({}))["max_backups"], 2);
    // a backup is a project like any other
    call(&mut again, "open", json!({ "path": dir.join("Job.0008.oab").to_string_lossy() }));
    // Save As to a new name backs nothing up
    let other = call(&mut again, "save", json!({ "path": dir.join("Other.oab").to_string_lossy() }));
    assert!(other["backup"].is_null(), "{other}");
    std::fs::remove_dir_all(dir).ok();
}
