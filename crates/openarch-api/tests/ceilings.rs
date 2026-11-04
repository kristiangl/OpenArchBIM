//! Ceilings: automatic and sketched placement, Properties, types, room volumes, schedules, IFC, plans and files.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))
}

struct Model { s: Session, level: u64, plan: u64, rcp: u64, types: Vec<u64>, thickness: f64 }

/// Four walls round a 4000 x 5000 rectangle (centrelines), 3000 high, with a ceiling plan of their level.
fn model() -> Model {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let plan = info["views"].as_array().unwrap().iter().find(|v| v["level_id"] == level && v["view_kind"] == "floor_plan").unwrap()["id"].as_u64().unwrap();
    let rcp_type = info["view_types"].as_array().unwrap().iter().find(|t| t["view_kind"] == "ceiling_plan").unwrap()["id"].clone();
    let rcp = call(&mut s, "create_plan_views", json!({ "view_type": rcp_type, "levels": [level] }))["ids"][0].as_u64().unwrap();
    let wt = info["wall_types"][0]["id"].clone();
    for (a, b) in [([0, 0], [4000, 0]), ([4000, 0], [4000, 5000]), ([4000, 5000], [0, 5000]), ([0, 5000], [0, 0])] {
        call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": level, "start": a, "end": b, "height_mm": 3000 }));
    }
    // the first starter wall type: 13 + 90 + 13
    let thickness = 116.0;
    assert_eq!(info["wall_types"][0]["name"], "Internal Stud 90");
    let types = info["ceiling_types"].as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap()).collect();
    Model { s, level, plan, rcp, types, thickness }
}

#[test]
fn automatic_ceiling_fills_the_walls_at_its_height_and_ignores_separators() {
    let mut m = model();
    assert_eq!(m.types.len(), 2, "new projects have plasterboard and suspended grid ceilings");
    // a room separator across the middle does not split a ceiling
    call(&mut m.s, "set_room_separators", json!({ "level_id": m.level, "curves": [{ "type": "line", "a": { "x": 0, "y": 2500 }, "b": { "x": 4000, "y": 2500 } }] }));
    let id = call(&mut m.s, "add_ceiling", json!({ "type_id": m.types[0], "level_id": m.level, "point": [2000, 1000] }))["id"].as_u64().unwrap();
    let area = row(&mut m.s, id, "Area")["raw"].as_f64().unwrap();
    let t = m.thickness;
    let expected = (4000.0 - t) * (5000.0 - t) / 1e6;
    assert!((area - expected).abs() < 0.01, "area {area}, expected {expected}");
    assert_eq!(row(&mut m.s, id, "Height Offset From Level")["raw"], 2400.0);
    // outside the walls there is nothing to fill
    assert!(m.s.handle("add_ceiling", &json!({ "type_id": m.types[0], "level_id": m.level, "point": [9000, 9000] })).is_err());
    // above the walls' tops there is nothing to fill either
    assert!(m.s.handle("add_ceiling", &json!({ "type_id": m.types[0], "level_id": m.level, "point": [2000, 1000], "offset_mm": 3500 })).is_err());
}

#[test]
fn sketched_ceiling_properties_types_and_reflected_ceiling_plan() {
    let mut m = model();
    let sq = |x0: f64, y0: f64, x1: f64, y1: f64| json!([[x0, y0, x1, y0], [x1, y0, x1, y1], [x1, y1, x0, y1], [x0, y1, x0, y0]].iter().map(|l| json!({ "type": "line", "a": { "x": l[0], "y": l[1] }, "b": { "x": l[2], "y": l[3] } })).collect::<Vec<_>>());
    let id = call(&mut m.s, "add_ceiling", json!({ "type_id": m.types[1], "level_id": m.level, "curves": sq(500.0, 500.0, 2500.0, 3500.0), "offset_mm": 2700 }))["id"].as_u64().unwrap();
    assert!((row(&mut m.s, id, "Area")["raw"].as_f64().unwrap() - 6.0).abs() < 1e-6);
    assert!((row(&mut m.s, id, "Perimeter")["raw"].as_f64().unwrap() - 10000.0).abs() < 1e-6);
    assert_eq!(row(&mut m.s, id, "Thickness")["raw"], 15.0);
    call(&mut m.s, "patch_ceiling", json!({ "id": id, "patch": { "mark": "C1", "comments": "Tiles", "ceiling_type": m.types[0], "offset_mm": 2550 } }));
    assert_eq!(row(&mut m.s, id, "Thickness")["raw"], 13.0);
    assert_eq!(row(&mut m.s, id, "Mark")["value"], "C1");
    // the reflected ceiling plan draws it with its type's grid; the floor plan does not
    let rcp = call(&mut m.s, "plan", json!({ "view_id": m.rcp }));
    assert_eq!(rcp["ceilings"].as_array().unwrap().len(), 1);
    assert!(call(&mut m.s, "plan", json!({ "view_id": m.plan }))["ceilings"].as_array().unwrap().is_empty());
    call(&mut m.s, "patch_ceiling", json!({ "id": id, "patch": { "ceiling_type": m.types[1] } }));
    assert_eq!(call(&mut m.s, "plan", json!({ "view_id": m.rcp }))["ceilings"][0]["grid"], json!([600.0, 1200.0]));
    // the type's grid edits in Type Properties; a basic ceiling has no layers and no thickness
    call(&mut m.s, "patch_system_type", json!({ "id": m.types[1], "patch": { "grid_across": 300 } }));
    assert_eq!(call(&mut m.s, "plan", json!({ "view_id": m.rcp }))["ceilings"][0]["grid"], json!([300.0, 1200.0]));
    let basic = call(&mut m.s, "duplicate_system_type", json!({ "id": m.types[0], "name": "Basic Ceiling" }))["id"].as_u64().unwrap();
    call(&mut m.s, "patch_system_type", json!({ "id": basic, "patch": { "layers": [] } }));
    call(&mut m.s, "patch_ceiling", json!({ "id": id, "patch": { "ceiling_type": basic } }));
    assert_eq!(row(&mut m.s, id, "Thickness")["raw"], 0.0);
    // Edit Boundary replaces the sketch as one undo step
    call(&mut m.s, "set_ceiling_sketch", json!({ "id": id, "curves": sq(0.0, 0.0, 1000.0, 1000.0) }));
    assert!((row(&mut m.s, id, "Area")["raw"].as_f64().unwrap() - 1.0).abs() < 1e-6);
    call(&mut m.s, "undo", json!({}));
    assert!((row(&mut m.s, id, "Area")["raw"].as_f64().unwrap() - 6.0).abs() < 1e-6);
}

#[test]
fn a_room_bounding_ceiling_caps_room_volume_and_ceilings_schedule_export_move_and_save() {
    let mut m = model();
    let room = call(&mut m.s, "add_room", json!({ "level_id": m.level, "point": [2000, 2500], "tag": false }))["id"].as_u64().unwrap();
    call(&mut m.s, "set_room_settings", json!({ "settings": { "boundary": "finish", "volumes": true } }));
    let open = row(&mut m.s, room, "Volume")["raw"].as_f64().unwrap();
    let ceiling = call(&mut m.s, "add_ceiling", json!({ "type_id": m.types[0], "level_id": m.level, "point": [2000, 2500] }))["id"].as_u64().unwrap();
    let area = row(&mut m.s, room, "Area")["raw"].as_f64().unwrap();
    let capped = row(&mut m.s, room, "Volume")["raw"].as_f64().unwrap();
    assert!((capped - area * 2.4).abs() < 0.05, "volume {capped} against {}", area * 2.4);
    assert!(open > capped);
    call(&mut m.s, "patch_ceiling", json!({ "id": ceiling, "patch": { "room_bounding": false } }));
    assert!((row(&mut m.s, room, "Volume")["raw"].as_f64().unwrap() - open).abs() < 1e-6);
    // a Ceilings schedule lists it
    let schedule = call(&mut m.s, "add_schedule", json!({ "category": "Ceilings", "name": "Ceiling Schedule" }))["id"].as_u64().unwrap();
    let mut def = call(&mut m.s, "schedule_table", json!({ "id": schedule }))["def"].clone();
    def["fields"] = json!([{ "name": "Type", "kind": "type" }, { "name": "Area", "kind": "instance" }, { "name": "Height Offset From Level", "kind": "instance" }]);
    call(&mut m.s, "set_schedule", json!({ "id": schedule, "def": def }));
    let table = call(&mut m.s, "schedule_table", json!({ "id": schedule }));
    assert_eq!(table["rows"].as_array().unwrap().iter().filter(|r| r["kind"] == "item").count(), 1);
    // IFC: an IfcCovering of type CEILING
    let path = std::env::temp_dir().join(format!("openarch-ceilings-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains("IFCCOVERING(") && text.contains(".CEILING.)"), "no ceiling in the IFC file");
    // Move carries the boundary; save and reopen keep it
    call(&mut m.s, "transform", json!({ "ids": [ceiling], "kind": "move", "dx": 100, "dy": 0 }));
    let before = call(&mut m.s, "plan", json!({ "view_id": m.rcp }))["ceilings"][0]["rings"].clone();
    let file = std::env::temp_dir().join(format!("openarch-ceilings-{}.oab", std::process::id()));
    call(&mut m.s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    std::fs::remove_file(&file).unwrap();
    assert_eq!(call(&mut again, "plan", json!({ "view_id": m.rcp }))["ceilings"][0]["rings"], before);
    // deleting it frees the room
    call(&mut again, "delete", json!({ "ids": [ceiling] }));
    assert!(call(&mut again, "plan", json!({ "view_id": m.rcp }))["ceilings"].as_array().unwrap().is_empty());
}
