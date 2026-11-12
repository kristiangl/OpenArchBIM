//! Edits that are refused or warned about: a level moved past a stair, wall heights, a wall drawn over another, and
//! a wall with doors made into a curtain wall.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn row(s: &mut Session, id: u64, label: &str) -> String {
    let d = call(s, "describe", json!({ "id": id }));
    d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no row {label}"))["value"].as_str().unwrap_or("").to_string()
}

#[test]
fn a_level_cannot_be_moved_to_or_below_the_base_of_a_stair_or_ramp_that_rises_to_it() {
    let mut s = Session::new();
    call(&mut s, "ensure_stair_types", json!({}));
    call(&mut s, "ensure_ramp_types", json!({}));
    let info = call(&mut s, "doc_info", json!({}));
    let (ground, upper) = (info["levels"][0]["id"].as_u64().unwrap(), info["levels"][1]["id"].as_u64().unwrap());
    let base = json!({ "type_id": info["stair_types"][0]["id"], "base_level": ground, "top_level": upper });
    let n = call(&mut s, "stair_draft", base.clone())["desired_risers"].as_u64().unwrap();
    let mut p = base.clone();
    p["pending"] = json!({ "start": [1000, 1500], "end": [1000, 1500.0 + (n - 1) as f64 * 250.0], "justification": "center", "width_mm": 1000 });
    let run = call(&mut s, "stair_draft", p)["pending"].clone();
    let mut p = base;
    p["runs"] = json!([run]);
    let stair = call(&mut s, "add_stair", p)["id"].as_u64().unwrap();
    let riser = row(&mut s, stair, "Actual Riser Height");

    // raising the level is fine: the stair follows
    call(&mut s, "patch_level", json!({ "id": upper, "patch": { "display_elevation_mm": 3600.0 } }));
    assert_ne!(row(&mut s, stair, "Actual Riser Height"), riser);
    call(&mut s, "undo", json!({}));
    // below the base, or onto it, is refused and nothing changes
    for to in [-500.0, 0.0] {
        let err = s.handle("patch_level", &json!({ "id": upper, "patch": { "display_elevation_mm": to } })).unwrap_err();
        assert!(err.contains("stair") && err.contains("base"), "{err}");
    }
    assert_eq!(row(&mut s, stair, "Actual Riser Height"), riser);
    // and the base level cannot be lifted past the top either
    let err = s.handle("patch_level", &json!({ "id": ground, "patch": { "display_elevation_mm": 9000.0 } })).unwrap_err();
    assert!(err.contains("stair"), "{err}");
    // with the stair gone the level is free to move
    call(&mut s, "delete", json!({ "ids": [stair] }));
    call(&mut s, "patch_level", json!({ "id": upper, "patch": { "display_elevation_mm": -500.0 } }));
}

#[test]
fn wall_heights_have_a_limit_and_a_wall_over_another_is_warned_about() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let (level, ty) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][1]["id"].as_u64().unwrap());
    let wall = |s: &mut Session, a: [f64; 2], b: [f64; 2], h: f64| s.handle("add_wall", &json!({ "type_id": ty, "level_id": level, "start": a, "end": b, "height_mm": h }));
    let first = wall(&mut s, [0.0, 0.0], [6000.0, 0.0], 2700.0).unwrap();
    assert!(first["warning"].is_null(), "{first}");
    let id = first["id"].as_u64().unwrap();
    // a kilometre is the most
    assert!(s.handle("set_wall_height", &json!({ "id": id, "height_mm": 1.0e9 })).unwrap_err().contains("at most"));
    assert!(wall(&mut s, [0.0, 9000.0], [6000.0, 9000.0], 2.0e6).unwrap_err().contains("at most"));
    call(&mut s, "set_wall_height", json!({ "id": id, "height_mm": 1.0e6 }));
    call(&mut s, "undo", json!({}));

    // walls beside, across and end to end with the first are not warned about
    for (a, b) in [([0.0, 500.0], [6000.0, 500.0]), ([3000.0, -2000.0], [3000.0, 2000.0]), ([6000.0, 0.0], [9000.0, 0.0]), ([6000.0, 0.0], [6000.0, 4000.0])] {
        let w = wall(&mut s, a, b, 2700.0).unwrap();
        assert!(w["warning"].is_null(), "{a:?} to {b:?}: {w}");
    }
    // one drawn right over it is made, and warned about; so is one over part of it, in either direction
    for (a, b) in [([0.0, 0.0], [6000.0, 0.0]), ([5000.0, 0.0], [2000.0, 0.0])] {
        let w = wall(&mut s, a, b, 2700.0).unwrap();
        assert!(w["id"].is_u64() && w["warning"].as_str().is_some_and(|t| t.contains("overlaps")), "{a:?} to {b:?}: {w}");
    }
    // one over it but on the level above is its own wall
    let above = s.handle("add_wall", &json!({ "type_id": ty, "level_id": info["levels"][1]["id"], "start": [0, 0], "end": [6000, 0], "height_mm": 2700 })).unwrap();
    assert!(above["warning"].is_null(), "{above}");
}

#[test]
fn a_wall_with_a_door_in_it_cannot_become_a_curtain_wall() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let ty = |name: &str| info["wall_types"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["id"].as_u64().unwrap();
    let door = info["family_types"].as_array().unwrap().iter().find(|t| t["name"] == "820 x 2040").unwrap()["id"].as_u64().unwrap();
    let wall = call(&mut s, "add_wall", json!({ "type_id": ty("Brick Veneer"), "level_id": level, "start": [0, 0], "end": [6000, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
    let placed = call(&mut s, "place_instance", json!({ "type_id": door, "host_id": wall, "point": [2000, 0] }))["id"].as_u64().unwrap();
    let err = s.handle("set_wall_type", &json!({ "id": wall, "type_id": ty("Curtain Wall") })).unwrap_err();
    assert!(err.contains("1 door or window") && err.contains("curtain wall"), "{err}");
    // another solid type is fine, and with the door gone so is the curtain wall
    call(&mut s, "set_wall_type", json!({ "id": wall, "type_id": ty("Concrete 200") }));
    call(&mut s, "delete", json!({ "ids": [placed] }));
    call(&mut s, "set_wall_type", json!({ "id": wall, "type_id": ty("Curtain Wall") }));
    // between curtain wall types nothing is asked
    call(&mut s, "set_wall_type", json!({ "id": wall, "type_id": ty("Storefront") }));
}
