//! Clipboard: Copy, Cut and the Paste variants, and Match Type Properties.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

struct M { s: Session, l1: u64, l2: u64, p1: u64, p2: u64, walls: Vec<u64>, door: u64 }

/// A 6000 x 4000 room of walls on the lowest level with a door in the south wall.
fn model() -> M {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let mut levels: Vec<(u64, f64)> = info["levels"].as_array().unwrap().iter().map(|l| (l["id"].as_u64().unwrap(), l["elevation_mm"].as_f64().unwrap())).collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (l1, l2) = (levels[0].0, levels[1].0);
    let plan = |l: u64| info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == l).unwrap()["id"].as_u64().unwrap();
    let (p1, p2) = (plan(l1), plan(l2));
    let wt = info["wall_types"][0]["id"].clone();
    let walls: Vec<u64> = [([0, 0], [6000, 0]), ([6000, 0], [6000, 4000]), ([6000, 4000], [0, 4000]), ([0, 4000], [0, 0])].iter()
        .map(|(a, b)| call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": l1, "start": a, "end": b, "height_mm": 2700 }))["id"].as_u64().unwrap()).collect();
    let ty = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Single-Flush: 820 x 2040").unwrap()["id"].clone();
    let door = call(&mut s, "place_instance", json!({ "type_id": ty, "host_id": walls[0], "point": [2000, 0] }))["id"].as_u64().unwrap();
    M { s, l1, l2, p1, p2, walls, door }
}

fn walls_on(s: &mut Session, view: u64) -> Vec<Value> {
    call(s, "plan", json!({ "view_id": view }))["walls"].as_array().unwrap().clone()
}
fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))["value"].clone()
}

#[test]
fn walls_and_their_door_paste_onto_another_level_and_at_a_picked_point() {
    let mut m = model();
    // copying the walls takes the door in them along
    let c = call(&mut m.s, "copy", json!({ "ids": m.walls, "view_id": m.p1 }));
    assert_eq!(c["count"], 5);
    let info = call(&mut m.s, "clipboard_info", json!({}));
    assert_eq!(info["origin"], json!([3000.0, 2000.0]));
    let ids = call(&mut m.s, "paste", json!({ "mode": "levels", "levels": [m.l2] }))["ids"].as_array().unwrap().clone();
    assert_eq!(ids.len(), 5);
    let up = walls_on(&mut m.s, m.p2);
    assert_eq!(up.len(), 4, "four walls on the upper level");
    let door2 = ids.iter().map(|v| v.as_u64().unwrap()).find(|i| call(&mut m.s, "describe", json!({ "id": i }))["kind"] == "FamilyInstance").unwrap();
    assert_ne!(row(&mut m.s, door2, "Mark"), row(&mut m.s, m.door, "Mark"), "the pasted door takes the next Mark");
    // one undo takes the paste away
    call(&mut m.s, "undo", json!({}));
    assert!(walls_on(&mut m.s, m.p2).is_empty());
    // Paste from Clipboard at a point: the middle of the copy goes there
    call(&mut m.s, "paste", json!({ "mode": "point", "view_id": m.p1, "point": [13000, 2000] }));
    let mut starts: Vec<f64> = walls_on(&mut m.s, m.p1).iter().map(|w| w["start"][0].as_f64().unwrap()).collect();
    starts.sort_by(f64::total_cmp);
    assert_eq!(starts, [0.0, 0.0, 6000.0, 6000.0, 10000.0, 10000.0, 16000.0, 16000.0]);
}

#[test]
fn a_door_cut_alone_pastes_into_the_wall_under_it() {
    let mut m = model();
    call(&mut m.s, "cut", json!({ "ids": [m.door], "view_id": m.p1 }));
    assert!(m.s.handle("describe", &json!({ "id": m.door })).is_err(), "cut removes it");
    // onto the north wall, 2000 north and 1000 east of where it was
    let ids = call(&mut m.s, "paste", json!({ "mode": "point", "view_id": m.p1, "point": [3000, 4000] }))["ids"].clone();
    let d = ids[0].as_u64().unwrap();
    let drawn = call(&mut m.s, "plan", json!({ "view_id": m.p1 }));
    let ins = drawn["inserts"].as_array().unwrap().iter().find(|i| i["id"] == d).unwrap().clone();
    assert_eq!(ins["at"], json!([3000.0, 4000.0]), "on the north wall");
    assert_eq!(row(&mut m.s, d, "Host"), "Basic Wall (".to_string() + &m.walls[2].to_string() + ")");
    // nowhere to host: refused, nothing added
    assert!(m.s.handle("paste", &json!({ "mode": "point", "view_id": m.p1, "point": [3000, 9000] })).is_err());
}

#[test]
fn view_specific_elements_paste_into_views_and_not_onto_levels() {
    let mut m = model();
    let info = call(&mut m.s, "doc_info", json!({}));
    let note = call(&mut m.s, "add_text_note", json!({ "note": { "view": m.p1, "at": { "x": 1000, "y": 1000 }, "text": "STORE", "text_type": info["text_note_types"][0]["id"] } }))["id"].as_u64().unwrap();
    call(&mut m.s, "copy", json!({ "ids": [note], "view_id": m.p1 }));
    assert!(m.s.handle("paste", &json!({ "mode": "levels", "levels": [m.l2] })).is_err());
    let ids = call(&mut m.s, "paste", json!({ "mode": "views", "views": [m.p2] }))["ids"].clone();
    let n = ids[0].as_u64().unwrap();
    let notes = call(&mut m.s, "plan", json!({ "view_id": m.p2 }));
    let found = notes.to_string().contains("STORE");
    assert!(found, "the note is in the upper plan");
    let _ = n;
    // model elements do not paste into views
    call(&mut m.s, "copy", json!({ "ids": [m.walls[0]], "view_id": m.p1 }));
    assert!(m.s.handle("paste", &json!({ "mode": "views", "views": [m.p2] })).is_err());
    // Same Place doubles them; Aligned to Current View puts them on the view's level
    call(&mut m.s, "paste", json!({ "mode": "same_place" }));
    assert_eq!(walls_on(&mut m.s, m.p1).len(), 5);
    call(&mut m.s, "paste", json!({ "mode": "current_view", "view_id": m.p2 }));
    assert_eq!(walls_on(&mut m.s, m.p2).len(), 1);
    // another project: the clipboard is not pasted there
    call(&mut m.s, "new_project", json!({}));
    let err = m.s.handle("paste", &json!({ "mode": "same_place" })).unwrap_err();
    assert!(err.contains("another project"), "{err}");
}

#[test]
fn match_type_gives_targets_the_source_type() {
    let mut m = model();
    let info = call(&mut m.s, "doc_info", json!({}));
    let other = info["wall_types"][1]["id"].as_u64().unwrap();
    call(&mut m.s, "set_wall_type", json!({ "id": m.walls[0], "type_id": other }));
    let r = call(&mut m.s, "match_type", json!({ "source": m.walls[0], "targets": [m.walls[1], m.walls[2], m.door] }));
    assert_eq!((r["changed"].as_u64(), r["skipped"].as_u64()), (Some(2), Some(1)), "the door is not a wall");
    let ty = |s: &mut Session, id: u64| call(s, "describe", json!({ "id": id }))["type_id"].clone();
    assert_eq!(ty(&mut m.s, m.walls[2]), json!(other));
    assert_ne!(ty(&mut m.s, m.walls[3]), json!(other));
}

