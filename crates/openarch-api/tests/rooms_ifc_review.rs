//! Regressions from the review of the IFC schedule property sets, rooms and revisions work.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))
}

/// Four walls round a 4000 x 5000 rectangle, drawn on `location_line`.
fn enclosure(location_line: &str) -> (Session, u64, u64, Vec<u64>) {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let view = info["views"].as_array().unwrap().iter().find(|v| v["level_id"] == level && v["view_kind"] == "floor_plan").unwrap()["id"].as_u64().unwrap();
    let mut walls = vec![];
    for (a, b) in [([0, 0], [4000, 0]), ([4000, 0], [4000, 5000]), ([4000, 5000], [0, 5000]), ([0, 5000], [0, 0])] {
        let w = call(&mut s, "add_wall", json!({ "type_id": info["wall_types"][0]["id"], "level_id": level, "start": a, "end": b, "height_mm": 3000, "location_line": location_line }));
        walls.push(w["id"].as_u64().unwrap());
    }
    (s, level, view, walls)
}

#[test]
fn wall_centre_boundary_measures_to_the_wall_centre_not_the_location_line() {
    let (mut s, level, view, walls) = enclosure("finish_face_interior");
    let t: f64 = row(&mut s, walls[0], "Thickness")["value"].as_str().unwrap().parse().unwrap();
    let room = call(&mut s, "add_room", json!({ "level_id": level, "view_id": view, "point": [2000, 2500] }))["id"].as_u64().unwrap();
    let finish = row(&mut s, room, "Area")["raw"].as_f64().unwrap();
    call(&mut s, "set_room_settings", json!({ "settings": { "boundary": "centre", "volumes": false } }));
    let centre = row(&mut s, room, "Area")["raw"].as_f64().unwrap();
    // the walls lie wholly on one side of the drawn lines: inside them or outside
    let inside = (finish - (4000.0 - 2.0 * t) * (5000.0 - 2.0 * t) / 1e6).abs() < 0.01;
    let expected = if inside { (4000.0 - t) * (5000.0 - t) } else { (4000.0 + t) * (5000.0 + t) } / 1e6;
    assert!((centre - expected).abs() < 0.01, "centre {centre}, expected {expected}, finish {finish}, thickness {t}");
}

#[test]
fn delete_in_a_model_view_unplaces_a_room_and_a_schedule_delete_removes_it() {
    let (mut s, level, view, _) = enclosure("wall_centreline");
    let room = call(&mut s, "add_room", json!({ "level_id": level, "view_id": view, "point": [2000, 2500] }))["id"].as_u64().unwrap();
    call(&mut s, "patch_room", json!({ "id": room, "patch": { "name": "Office" } }));
    let r = call(&mut s, "delete", json!({ "ids": [room], "unplace_rooms": true }));
    assert_eq!(r["unplaced"], 1);
    assert_eq!(row(&mut s, room, "Status")["value"], "Not Placed");
    assert_eq!(row(&mut s, room, "Name")["value"], "Office");
    assert!(call(&mut s, "plan", json!({ "view_id": view }))["room_tags"].as_array().unwrap().is_empty());
    // one undo step puts it back with its tag
    call(&mut s, "undo", json!({}));
    assert_eq!(row(&mut s, room, "Status")["value"], "Placed");
    assert_eq!(call(&mut s, "plan", json!({ "view_id": view }))["room_tags"].as_array().unwrap().len(), 1);
    // a delete without the flag (a schedule's Delete Row) removes the room
    call(&mut s, "delete", json!({ "ids": [room] }));
    assert!(s.handle("describe", &json!({ "id": room })).is_err());
}

#[test]
fn schedule_sets_keep_numeric_looking_text_as_labels_skip_blanks_and_carry_unrounded_areas() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].clone();
    let wall = call(&mut s, "add_wall", json!({ "type_id": info["wall_types"][0]["id"], "level_id": level, "start": [0, 0], "end": [4123.4567, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
    call(&mut s, "patch_wall", json!({ "id": wall, "patch": { "mark": "101" } }));
    let schedule = call(&mut s, "add_schedule", json!({ "category": "Walls", "name": "Pset_Walls" }))["id"].as_u64().unwrap();
    let mut def = call(&mut s, "schedule_table", json!({ "id": schedule }))["def"].clone();
    def["fields"] = json!([{ "name": "Mark", "kind": "instance" }, { "name": "Comments", "kind": "instance" }, { "name": "Area", "kind": "instance" }]);
    def["ifc"] = json!({ "enabled": true });
    call(&mut s, "set_schedule", json!({ "id": schedule, "def": def }));
    let sets = call(&mut s, "ifc_schedule_properties", json!({ "schedules": true }));
    let props = sets[0]["properties"].as_array().unwrap();
    let mark = props.iter().find(|p| p["name"] == "Mark").unwrap();
    assert_eq!(mark["unit"], "text");
    assert_eq!(mark["value"], "101");
    assert!(!props.iter().any(|p| p["name"] == "Comments"), "a blank text value is left out");
    let area = props.iter().find(|p| p["name"] == "Area").unwrap()["value"].as_f64().unwrap();
    assert!((area - 4.1234567 * 2.7).abs() < 1e-6, "{area}");
    // the schedule itself still shows the rounded value
    let table = call(&mut s, "schedule_table", json!({ "id": schedule }));
    assert_eq!(table["rows"][0]["cells"][2]["text"], "11.13 m²");
    let path = std::env::temp_dir().join(format!("openarch-review-psets-{}.ifc", std::process::id()));
    call(&mut s, "export_ifc", json!({ "path": path, "schedules": true }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains("IFCPROPERTYSINGLEVALUE('Mark',$,IFCLABEL('101'),$)"), "{text}");
    assert!(!text.contains("IFCPROPERTYSINGLEVALUE('Mark',$,IFCREAL"));
}
