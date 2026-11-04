//! Families hosted by ceilings: lights, air terminals, smoke detectors and any other family made Hosted by: Ceiling.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))
}

struct Model { s: Session, level: u64, plan: u64, rcp: u64, left: u64, right: u64, info: Value }

fn type_of(info: &Value, family: &str) -> Value {
    info["family_types"].as_array().unwrap().iter().find(|t| t["family"] == family).unwrap_or_else(|| panic!("no {family}"))["id"].clone()
}

/// Two rooms side by side (0-4000 and 4000-8000 by 5000), a ceiling in each, and a ceiling plan.
fn model() -> Model {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let plan = info["views"].as_array().unwrap().iter().find(|v| v["level_id"] == level && v["view_kind"] == "floor_plan").unwrap()["id"].as_u64().unwrap();
    let rcp_type = info["view_types"].as_array().unwrap().iter().find(|t| t["view_kind"] == "ceiling_plan").unwrap()["id"].clone();
    let rcp = call(&mut s, "create_plan_views", json!({ "view_type": rcp_type, "levels": [level] }))["ids"][0].as_u64().unwrap();
    let wt = info["wall_types"][0]["id"].clone();
    for (a, b) in [([0, 0], [8000, 0]), ([8000, 0], [8000, 5000]), ([8000, 5000], [0, 5000]), ([0, 5000], [0, 0]), ([4000, 0], [4000, 5000])] {
        call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": level, "start": a, "end": b, "height_mm": 3000 }));
    }
    let ct = info["ceiling_types"][0]["id"].clone();
    let left = call(&mut s, "add_ceiling", json!({ "type_id": ct, "level_id": level, "point": [2000, 2500] }))["id"].as_u64().unwrap();
    let right = call(&mut s, "add_ceiling", json!({ "type_id": ct, "level_id": level, "point": [6000, 2500], "offset_mm": 2700 }))["id"].as_u64().unwrap();
    Model { s, level, plan, rcp, left, right, info }
}

fn elevation(s: &mut Session, id: u64) -> f64 {
    row(s, id, "Elevation from Level")["value"].as_str().unwrap().parse().unwrap()
}

#[test]
fn new_projects_have_ceiling_hosted_families_of_several_categories() {
    let m = model();
    let fams = m.info["families"].as_array().unwrap();
    for (name, cat) in [("Recessed Troffer 600 x 600", "Lighting Fixtures"), ("Downlight 90", "Lighting Fixtures"), ("Supply Air Diffuser 600 x 600", "Air Terminals"), ("Smoke Detector", "Fire Alarm Devices")] {
        let f = fams.iter().find(|f| f["name"] == name).unwrap_or_else(|| panic!("no {name}"));
        assert_eq!(f["category"], cat);
        assert_eq!(f["host"], "ceiling");
    }
}

#[test]
fn a_hosted_family_hangs_from_its_ceiling_follows_it_and_goes_with_it() {
    let mut m = model();
    let troffer = type_of(&m.info, "Recessed Troffer 600 x 600");
    let light = call(&mut m.s, "place_on_ceiling", json!({ "type_id": troffer, "level_id": m.level, "point": [2000, 2500] }));
    assert_eq!(light["ceiling"], m.left);
    let light = light["id"].as_u64().unwrap();
    assert_eq!(elevation(&mut m.s, light), 2400.0, "on the left ceiling's underside");
    assert!(row(&mut m.s, light, "Host")["value"].as_str().unwrap().starts_with("Ceiling : "));
    assert_eq!(row(&mut m.s, light, "Mark")["value"], "", "lights number only once someone starts a numbering");
    // the ceiling's height carries it
    call(&mut m.s, "patch_ceiling", json!({ "id": m.left, "patch": { "offset_mm": 2550 } }));
    assert_eq!(elevation(&mut m.s, light), 2550.0);
    // Offset from Host lowers it from the face
    call(&mut m.s, "patch_instance", json!({ "id": light, "patch": { "offset_mm": 100 } }));
    assert_eq!(elevation(&mut m.s, light), 2450.0);
    // ceiling plans show it; floor plans do not (it is above their cut plane)
    let in_view = |s: &mut Session, v: u64| call(s, "plan", json!({ "view_id": v }))["inserts"].as_array().unwrap().iter().any(|i| i["id"] == light);
    assert!(in_view(&mut m.s, m.rcp));
    assert!(!in_view(&mut m.s, m.plan));
    // a copy into the next room goes onto that room's ceiling
    let copy = call(&mut m.s, "transform", json!({ "ids": [light], "kind": "move", "dx": 4000, "dy": 0, "copy": true }))["ids"][0].as_u64().unwrap();
    assert_eq!(elevation(&mut m.s, copy), 2600.0, "the right ceiling is at 2700, less the 100 offset");
    // deleting a ceiling deletes what it hosts; undo brings both back
    call(&mut m.s, "delete", json!({ "ids": [m.right] }));
    assert!(m.s.handle("describe", &json!({ "id": copy })).is_err());
    call(&mut m.s, "undo", json!({}));
    assert_eq!(elevation(&mut m.s, copy), 2600.0);
    // IFC: a light fixture
    let path = std::env::temp_dir().join(format!("openarch-hosted-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains("IFCLIGHTFIXTURE("));
}

#[test]
fn sections_cut_ceilings_and_elevations_see_them() {
    let mut m = model();
    let troffer = type_of(&m.info, "Recessed Troffer 600 x 600");
    let light = call(&mut m.s, "place_on_ceiling", json!({ "type_id": troffer, "level_id": m.level, "point": [2000, 2500] }))["id"].as_u64().unwrap();
    // a section across both rooms, looking north through y = 2500
    let view = call(&mut m.s, "add_section", json!({ "level_id": m.level, "start": [-1000, 2500], "end": [9000, 2500] }))["view_id"].as_u64().unwrap();
    let items = call(&mut m.s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().clone();
    let cut: Vec<&Value> = items.iter().filter(|i| i["category"] == "Ceilings" && i["cut"] == true).collect();
    assert!(cut.iter().any(|i| i["id"] == m.left) && cut.iter().any(|i| i["id"] == m.right), "both ceilings are cut");
    // the left ceiling's band runs from its underside (2400) up through its 13 mm
    let zs: Vec<f64> = cut.iter().filter(|i| i["id"] == m.left).flat_map(|i| i["polys"][0].as_array().unwrap().iter().map(|p| p[1].as_f64().unwrap()).collect::<Vec<_>>()).collect();
    assert!((zs.iter().cloned().fold(f64::INFINITY, f64::min) - 2400.0).abs() < 0.5 && (zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 2413.0).abs() < 0.5, "{zs:?}");
    assert!(items.iter().any(|i| i["id"] == light), "the light under the ceiling shows in the section");
    // an elevation sees them
    let south = m.info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "elevation" && v["name"] == "South").unwrap()["id"].as_u64().unwrap();
    let seen = call(&mut m.s, "elevation", json!({ "view_id": south }))["items"].as_array().unwrap().clone();
    assert!(seen.iter().any(|i| i["category"] == "Ceilings"));
}

#[test]
fn other_categories_host_too_and_placing_is_checked() {
    let mut m = model();
    for family in ["Smoke Detector", "Supply Air Diffuser 600 x 600", "Downlight 90"] {
        let t = type_of(&m.info, family);
        let id = call(&mut m.s, "place_on_ceiling", json!({ "type_id": t, "level_id": m.level, "point": [6000, 1000], "rotation": 45 }))["id"].as_u64().unwrap();
        assert_eq!(elevation(&mut m.s, id), 2700.0, "{family} on the right ceiling");
    }
    let t = type_of(&m.info, "Smoke Detector");
    // outside every ceiling there is nothing to host it
    assert!(m.s.handle("place_on_ceiling", &json!({ "type_id": t, "level_id": m.level, "point": [9000, 1000] })).is_err());
    // a ceiling family is not placed free standing, and a door is not hosted by a ceiling
    assert!(m.s.handle("place_free_instance", &json!({ "type_id": t, "level_id": m.level, "point": [2000, 2000] })).is_err());
    let door = type_of(&m.info, "Single-Flush");
    assert!(m.s.handle("place_on_ceiling", &json!({ "type_id": door, "level_id": m.level, "point": [2000, 2000] })).is_err());
}
