//! Areas: schemes, area plans with automatic boundaries, boundary lines, areas, tags, unplacing and schedules.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))
}

struct Model { s: Session, level: u64, gross: u64, rentable: u64 }

/// An 8000 x 5000 building (wall centrelines, 116 mm walls) split in two at x = 4000.
fn model() -> Model {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let wt = info["wall_types"][0]["id"].clone();
    for (a, b) in [([0, 0], [8000, 0]), ([8000, 0], [8000, 5000]), ([8000, 5000], [0, 5000]), ([0, 5000], [0, 0]), ([4000, 0], [4000, 5000])] {
        call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": level, "start": a, "end": b, "height_mm": 3000 }));
    }
    let schemes = info["area_schemes"].as_array().unwrap();
    let id = |name: &str| schemes.iter().find(|x| x["name"] == name).unwrap()["id"].as_u64().unwrap();
    Model { gross: id("Gross Building"), rentable: id("Rentable"), s, level }
}

#[test]
fn gross_building_plan_draws_its_boundary_on_the_outside_of_the_building() {
    let mut m = model();
    let plan = call(&mut m.s, "create_area_plans", json!({ "scheme_id": m.gross, "levels": [m.level], "boundaries": true }))["ids"][0].as_u64().unwrap();
    let drawn = call(&mut m.s, "plan", json!({ "view_id": plan }));
    assert!(drawn["area_boundaries"].as_array().unwrap().len() >= 4, "boundary lines round the outside");
    let area = call(&mut m.s, "add_area", json!({ "view_id": plan, "point": [2000, 2500] }))["id"].as_u64().unwrap();
    let a = row(&mut m.s, area, "Area")["raw"].as_f64().unwrap();
    // to the outside faces: 8116 x 5116 (the interior wall does not bound an area)
    assert!((a - 8.116 * 5.116).abs() < 0.01, "{a}");
    assert_eq!(row(&mut m.s, area, "Area Type")["value"], "Gross Building Area");
    assert_eq!(row(&mut m.s, area, "Area Scheme")["value"], "Gross Building");
    // a second area in the same boundary is redundant
    let dup = call(&mut m.s, "add_area", json!({ "view_id": plan, "point": [6000, 2500], "tag": false }))["id"].as_u64().unwrap();
    assert_eq!(row(&mut m.s, dup, "Status")["value"], "Redundant");
    // the tag shows the area
    let tags = drawn_tags(&mut m.s, plan);
    assert_eq!(tags.len(), 1);
    assert!(tags[0].contains("41.52 m²"), "{tags:?}");
    // the area plan is listed with its scheme
    let info = call(&mut m.s, "doc_info", json!({}));
    assert!(info["views"].as_array().unwrap().iter().any(|v| v["id"] == plan && v["view_kind"] == "area_plan" && v["area_scheme"] == m.gross));
}

fn drawn_tags(s: &mut Session, plan: u64) -> Vec<String> {
    call(s, "plan", json!({ "view_id": plan }))["area_tags"].as_array().unwrap().iter().map(|t| t["labels"].as_array().unwrap().iter().flat_map(|l| l["lines"].as_array().unwrap().iter().flat_map(|x| x.as_array().unwrap().iter().filter_map(|y| y.as_str().map(str::to_string)))).collect::<Vec<_>>().join(" ")).collect()
}

#[test]
fn rentable_boundaries_split_areas_types_unplace_and_schedules() {
    let mut m = model();
    let plan = call(&mut m.s, "create_area_plans", json!({ "scheme_id": m.rentable, "levels": [m.level], "boundaries": true }))["ids"][0].as_u64().unwrap();
    // Pick Walls on the middle wall: a Rentable boundary sits on its centreline
    let info = call(&mut m.s, "doc_info", json!({}));
    let _ = info;
    let walls: Vec<u64> = call(&mut m.s, "plan", json!({ "view_id": plan }))["walls"].as_array().unwrap().iter().map(|w| w["id"].as_u64().unwrap()).collect();
    let middle = *walls.last().unwrap();
    let b = call(&mut m.s, "add_wall_area_boundaries", json!({ "view_id": plan, "wall_ids": [middle] }))["ids"][0].as_u64().unwrap();
    let drawn = call(&mut m.s, "plan", json!({ "view_id": plan }))["area_boundaries"].as_array().unwrap().iter().find(|x| x["id"] == b).unwrap().clone();
    assert!(drawn["pts"].as_array().unwrap().iter().all(|p| p[0] == 4000.0), "on the centreline: {drawn}");
    let ids = call(&mut m.s, "place_all_areas", json!({ "view_id": plan }))["ids"].as_array().unwrap().clone();
    assert_eq!(ids.len(), 2, "one area each side of the middle boundary");
    let left = ids.iter().map(|v| v.as_u64().unwrap()).min().unwrap();
    assert_eq!(row(&mut m.s, left, "Area Type")["value"], "Building Common Area");
    call(&mut m.s, "patch_area", json!({ "id": left, "patch": { "area_type": "office_area", "name": "Tenancy 1" } }));
    assert_eq!(row(&mut m.s, left, "Area Type")["value"], "Office Area");
    // a Gross Building type is not a Rentable one
    assert!(m.s.handle("patch_area", &json!({ "id": left, "patch": { "area_type": "gross_building_area" } })).is_err());
    // the Rentable schedule lists both
    let schedule = call(&mut m.s, "add_schedule", json!({ "category": "Areas (Rentable)", "name": "Area Schedule (Rentable)" }))["id"].as_u64().unwrap();
    let mut def = call(&mut m.s, "schedule_table", json!({ "id": schedule }))["def"].clone();
    def["fields"] = json!([{ "name": "Number", "kind": "instance" }, { "name": "Name", "kind": "instance" }, { "name": "Area Type", "kind": "instance" }, { "name": "Area", "kind": "instance" }]);
    call(&mut m.s, "set_schedule", json!({ "id": schedule, "def": def }));
    assert_eq!(call(&mut m.s, "schedule_table", json!({ "id": schedule }))["rows"].as_array().unwrap().iter().filter(|r| r["kind"] == "item").count(), 2);
    // Delete in the plan unplaces the area and keeps it; one undo puts it back
    call(&mut m.s, "delete", json!({ "ids": [left], "unplace_rooms": true }));
    assert_eq!(row(&mut m.s, left, "Status")["value"], "Not Placed");
    assert_eq!(row(&mut m.s, left, "Name")["value"], "Tenancy 1");
    call(&mut m.s, "undo", json!({}));
    assert_eq!(row(&mut m.s, left, "Status")["value"], "Placed");
    // save and reopen
    let file = std::env::temp_dir().join(format!("openarch-areas-{}.oab", std::process::id()));
    call(&mut m.s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    std::fs::remove_file(&file).unwrap();
    assert_eq!(row(&mut again, left, "Area Type")["value"], "Office Area");
    // deleting the area plan takes its boundaries, areas and tags with it
    call(&mut again, "delete", json!({ "ids": [plan] }));
    assert!(again.handle("describe", &json!({ "id": left })).is_err());
}

fn area_of(s: &mut Session, id: u64) -> f64 { row(s, id, "Area")["raw"].as_f64().unwrap() }

#[test]
fn rentable_rules_follow_the_area_types_each_side_and_the_glass() {
    let mut m = model();
    let plan = call(&mut m.s, "create_area_plans", json!({ "scheme_id": m.rentable, "levels": [m.level], "boundaries": true }))["ids"][0].as_u64().unwrap();
    let walls: Vec<u64> = call(&mut m.s, "plan", json!({ "view_id": plan }))["walls"].as_array().unwrap().iter().map(|w| w["id"].as_u64().unwrap()).collect();
    call(&mut m.s, "add_wall_area_boundaries", json!({ "view_id": plan, "wall_ids": [walls[4]] }));
    // picking the same wall again adds nothing
    assert!(m.s.handle("add_wall_area_boundaries", &json!({ "view_id": plan, "wall_ids": [walls[4]] })).is_err());
    let ids: Vec<u64> = call(&mut m.s, "place_all_areas", json!({ "view_id": plan }))["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let (left, right) = (ids[0].min(ids[1]), ids[0].max(ids[1]));
    // office beside office at the centreline, the exterior walls to their inside faces
    assert!((area_of(&mut m.s, left) - 3.942 * 4.884).abs() < 0.001, "{}", area_of(&mut m.s, left));
    // a store measures to the outside faces of its exterior walls; the walls it shares with the office split there
    call(&mut m.s, "patch_area", json!({ "id": right, "patch": { "area_type": "store_area" } }));
    assert!((area_of(&mut m.s, right) - 4.058 * 5.116).abs() < 0.001, "{}", area_of(&mut m.s, right));
    assert!((area_of(&mut m.s, left) - 3.942 * 4.884).abs() < 0.001, "{}", area_of(&mut m.s, left));
    call(&mut m.s, "patch_area", json!({ "id": right, "patch": { "area_type": "office_area" } }));
    // a shaft: the boundary goes to the face of the wall on the office side
    call(&mut m.s, "patch_area", json!({ "id": left, "patch": { "area_type": "major_vertical_penetration" } }));
    assert!((area_of(&mut m.s, left) - 4.000 * 4.884).abs() < 0.001, "{}", area_of(&mut m.s, left));
    assert!((area_of(&mut m.s, right) - 3.884 * 4.884).abs() < 0.001, "{}", area_of(&mut m.s, right));
    // a floor-length window in the south wall: the boundary goes to the glass, at the middle of the wall
    let info = call(&mut m.s, "doc_info", json!({}));
    let ty = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Fixed: 2400 x 2100").unwrap()["id"].clone();
    call(&mut m.s, "place_instance", json!({ "type_id": ty, "host_id": walls[0], "point": [6000, 0] }));
    assert!((area_of(&mut m.s, right) - 3.884 * 4.942).abs() < 0.001, "{}", area_of(&mut m.s, right));
    // rules off: the line sits at its offset from the wall centre, toward the exterior
    let b = call(&mut m.s, "plan", json!({ "view_id": plan }))["area_boundaries"].as_array().unwrap().iter().find(|x| x["wall"] == walls[4]).unwrap()["id"].as_u64().unwrap();
    call(&mut m.s, "patch_area_boundary", json!({ "id": b, "patch": { "apply_rules": false, "offset_mm": 0 } }));
    assert!((area_of(&mut m.s, right) - 3.942 * 4.942).abs() < 0.001, "{}", area_of(&mut m.s, right));
}

#[test]
fn boundaries_follow_their_walls_and_stay_when_a_wall_goes() {
    let mut m = model();
    let plan = call(&mut m.s, "create_area_plans", json!({ "scheme_id": m.gross, "levels": [m.level], "boundaries": true }))["ids"][0].as_u64().unwrap();
    let drawn = call(&mut m.s, "plan", json!({ "view_id": plan }));
    assert_eq!(drawn["area_boundaries"].as_array().unwrap().iter().filter(|b| b["wall"].is_u64()).count(), 4, "one boundary on each exterior wall");
    let walls: Vec<u64> = drawn["walls"].as_array().unwrap().iter().map(|w| w["id"].as_u64().unwrap()).collect();
    let area = call(&mut m.s, "add_area", json!({ "view_id": plan, "point": [2000, 2500] }))["id"].as_u64().unwrap();
    // the north wall moves 1000 north: the area grows with it
    call(&mut m.s, "transform", json!({ "ids": [walls[2]], "kind": "move", "dx": 0, "dy": 1000 }));
    assert!((area_of(&mut m.s, area) - 8.116 * 6.116).abs() < 0.01, "{}", area_of(&mut m.s, area));
    // deleting it leaves its boundary where it was
    call(&mut m.s, "delete", json!({ "ids": [walls[2]] }));
    assert!((area_of(&mut m.s, area) - 8.116 * 6.116).abs() < 0.01, "{}", area_of(&mut m.s, area));
    call(&mut m.s, "undo", json!({}));
    let b = call(&mut m.s, "plan", json!({ "view_id": plan }))["area_boundaries"].as_array().unwrap().iter().filter(|b| b["wall"].is_u64()).count();
    assert_eq!(b, 4, "undo brings the wall and its boundary back together");
}