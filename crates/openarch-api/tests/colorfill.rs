//! Colour fill schemes and legends for areas and rooms, and new area schemes.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

/// An 8000 x 5000 building split at x = 4000, with a Rentable plan and an area each side.
fn model() -> (Session, u64, u64, u64, Vec<u64>) {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let wt = info["wall_types"][0]["id"].clone();
    let mut walls = vec![];
    for (a, b) in [([0, 0], [8000, 0]), ([8000, 0], [8000, 5000]), ([8000, 5000], [0, 5000]), ([0, 5000], [0, 0]), ([4000, 0], [4000, 5000])] {
        walls.push(call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": level, "start": a, "end": b, "height_mm": 3000 }))["id"].as_u64().unwrap());
    }
    let rentable = info["area_schemes"].as_array().unwrap().iter().find(|x| x["name"] == "Rentable").unwrap()["id"].as_u64().unwrap();
    let plan = call(&mut s, "create_area_plans", json!({ "scheme_id": rentable, "levels": [level], "boundaries": true }))["ids"][0].as_u64().unwrap();
    call(&mut s, "add_wall_area_boundaries", json!({ "view_id": plan, "wall_ids": [walls[4]] }));
    let ids: Vec<u64> = call(&mut s, "place_all_areas", json!({ "view_id": plan, "tag": false }))["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    (s, level, plan, rentable, ids)
}

#[test]
fn area_plan_coloured_by_area_type_with_a_legend() {
    let (mut s, _, plan, _, ids) = model();
    call(&mut s, "patch_area", json!({ "id": ids[0], "patch": { "area_type": "office_area" } }));
    call(&mut s, "patch_area", json!({ "id": ids[1], "patch": { "area_type": "store_area" } }));
    // no scheme yet: no colours, and a legend cannot go in
    assert!(call(&mut s, "plan", json!({ "view_id": plan }))["color_fills"].as_array().unwrap().is_empty());
    assert!(s.handle("add_color_legend", &json!({ "view_id": plan, "point": [9000, 5000] })).is_err());
    let list = call(&mut s, "color_schemes", json!({ "id": plan }));
    assert_eq!(list["category"], "Areas (Rentable)");
    let scheme = list["schemes"][0]["id"].as_u64().unwrap();
    call(&mut s, "set_color_scheme", json!({ "id": scheme, "scheme": { "parameter": "Area Type", "title": "Tenancies" } }));
    call(&mut s, "set_view_color_scheme", json!({ "view_id": plan, "scheme_id": scheme }));
    let drawn = call(&mut s, "plan", json!({ "view_id": plan }));
    let fills = drawn["color_fills"].as_array().unwrap();
    assert_eq!(fills.len(), 2);
    assert_ne!(fills[0]["color"], fills[1]["color"], "each area type its own colour");
    let legend = call(&mut s, "add_color_legend", json!({ "view_id": plan, "point": [9000, 5000] }))["id"].as_u64().unwrap();
    let drawn = call(&mut s, "plan", json!({ "view_id": plan }));
    let l = drawn["color_legends"].as_array().unwrap().iter().find(|x| x["id"] == legend).unwrap().clone();
    assert_eq!(l["title"], "Tenancies");
    let labels: Vec<&str> = l["rows"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Office Area", "Store Area"]);
    // hide Store Area and make Office red: one fill, red
    let mut rows = call(&mut s, "color_schemes", json!({ "id": legend }))["schemes"][0]["rows"].as_array().unwrap().clone();
    for r in &mut rows { if r["value"] == "Office Area" { r["color"] = json!("#ff0000"); } else { r["visible"] = json!(false); } }
    call(&mut s, "set_color_scheme", json!({ "id": scheme, "scheme": { "rows": rows } }));
    let fills = call(&mut s, "plan", json!({ "view_id": plan }))["color_fills"].as_array().unwrap().clone();
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0]["color"], "#ff0000");
    // by range of Area: the office is 19.7 m², the store (to the outside faces of its exterior walls) 20.3 m²
    call(&mut s, "set_color_scheme", json!({ "id": scheme, "scheme": { "parameter": "Area", "by_range": true, "rows": [
        { "at_least": 0, "color": "#eeeeee" }, { "at_least": 10, "color": "#00ff00" }, { "at_least": 20, "color": "#0000ff" } ] } }));
    let drawn = call(&mut s, "plan", json!({ "view_id": plan }));
    let colors: Vec<&str> = drawn["color_fills"].as_array().unwrap().iter().map(|f| f["color"].as_str().unwrap()).collect();
    assert_eq!(colors, ["#00ff00", "#0000ff"]);
    let labels: Vec<String> = drawn["color_legends"][0]["rows"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap().to_string()).collect();
    assert_eq!(labels, ["10 m² to 20 m²", "20 m² or more"]);
    // a name a scheme cannot take, and a range on text, are refused
    assert!(s.handle("set_color_scheme", &json!({ "id": scheme, "scheme": { "parameter": "Department" } })).is_err());
    assert!(s.handle("set_color_scheme", &json!({ "id": scheme, "scheme": { "parameter": "Name", "by_range": true } })).is_err());
    // save and reopen; deleting the scheme leaves the view with none
    let file = std::env::temp_dir().join(format!("openarch-colorfill-{}.oab", std::process::id()));
    call(&mut s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    std::fs::remove_file(&file).unwrap();
    assert_eq!(call(&mut again, "plan", json!({ "view_id": plan }))["color_fills"].as_array().unwrap().len(), 2);
    call(&mut again, "delete_color_scheme", json!({ "id": scheme }));
    assert!(call(&mut again, "plan", json!({ "view_id": plan }))["color_fills"].as_array().unwrap().is_empty());
}

#[test]
fn floor_plan_rooms_coloured_by_department() {
    let (mut s, level, _, _, _) = model();
    let info = call(&mut s, "doc_info", json!({}));
    let fp = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == level).unwrap()["id"].as_u64().unwrap();
    let a = call(&mut s, "add_room", json!({ "level_id": level, "view_id": fp, "point": [2000, 2500] }))["id"].as_u64().unwrap();
    let b = call(&mut s, "add_room", json!({ "level_id": level, "view_id": fp, "point": [6000, 2500] }))["id"].as_u64().unwrap();
    call(&mut s, "patch_room", json!({ "id": a, "patch": { "department": "Admin" } }));
    call(&mut s, "patch_room", json!({ "id": b, "patch": { "department": "Clinical" } }));
    let list = call(&mut s, "color_schemes", json!({ "id": fp }));
    assert_eq!(list["category"], "Rooms");
    let scheme = list["schemes"][0]["id"].as_u64().unwrap();
    // a room scheme does not suit an area plan
    call(&mut s, "set_color_scheme", json!({ "id": scheme, "scheme": { "parameter": "Department" } }));
    call(&mut s, "set_view_color_scheme", json!({ "view_id": fp, "scheme_id": scheme }));
    let fills = call(&mut s, "plan", json!({ "view_id": fp }))["color_fills"].as_array().unwrap().clone();
    assert_eq!(fills.len(), 2);
    assert_ne!(fills[0]["color"], fills[1]["color"]);
    let rows = call(&mut s, "color_schemes", json!({ "id": fp }))["schemes"][0]["rows"].as_array().unwrap().clone();
    assert_eq!(rows.iter().map(|r| r["value"].as_str().unwrap()).collect::<Vec<_>>(), ["Admin", "Clinical"]);
    // a copy with its own name
    let copy = call(&mut s, "duplicate_color_scheme", json!({ "id": scheme, "name": "By Department 2" }))["id"].as_u64().unwrap();
    assert!(s.handle("duplicate_color_scheme", &json!({ "id": scheme, "name": "By Department 2" })).is_err());
    assert_ne!(copy, scheme);
}

#[test]
fn new_area_schemes_and_deleting_them_with_their_plans() {
    let (mut s, level, _, rentable, _) = model();
    let id = call(&mut s, "add_area_scheme", json!({ "name": "Lettable", "description": "Lettable area", "rules": "rentable" }))["id"].as_u64().unwrap();
    assert!(s.handle("add_area_scheme", &json!({ "name": "Lettable" })).is_err(), "names are unique");
    let plan = call(&mut s, "create_area_plans", json!({ "scheme_id": id, "levels": [level], "boundaries": true }))["ids"][0].as_u64().unwrap();
    call(&mut s, "add_area", json!({ "view_id": plan, "point": [2000, 2500] }));
    // with areas the rules stay; renaming is fine
    assert!(s.handle("patch_area_scheme", &json!({ "id": id, "name": "Lettable", "rules": "gross_building" })).is_err());
    call(&mut s, "patch_area_scheme", json!({ "id": id, "name": "NLA", "description": "", "rules": "rentable" }));
    let info = call(&mut s, "doc_info", json!({}));
    assert!(info["area_schemes"].as_array().unwrap().iter().any(|x| x["name"] == "NLA" && x["has_areas"] == true));
    // Gross Building cannot go; a scheme takes its plans with it
    let gross = info["area_schemes"].as_array().unwrap().iter().find(|x| x["built_in"] == true).unwrap()["id"].as_u64().unwrap();
    assert!(s.handle("delete_area_scheme", &json!({ "id": gross })).is_err());
    call(&mut s, "delete_area_scheme", json!({ "id": id }));
    assert!(s.handle("describe", &json!({ "id": plan })).is_err());
    let info = call(&mut s, "doc_info", json!({}));
    assert!(info["area_schemes"].as_array().unwrap().iter().all(|x| x["id"] != id && x["id"].as_u64().is_some()));
    assert!(info["area_schemes"].as_array().unwrap().iter().any(|x| x["id"] == rentable));
    call(&mut s, "undo", json!({}));
    assert!(s.handle("describe", &json!({ "id": plan })).is_ok(), "one undo brings the scheme and its plan back");
}
