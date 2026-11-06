//! Stairs, step 5 part 1: supports, nosings and a sloped soffit on spiral, winder and sketched runs.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

#[test]
fn a_timber_spiral_draws_its_stringers_and_at_fine_its_nosings() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let mut levels: Vec<(u64, f64)> = info["levels"].as_array().unwrap().iter().map(|l| (l["id"].as_u64().unwrap(), l["elevation_mm"].as_f64().unwrap())).collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (l1, l2) = (levels[0].0, levels[1].0);
    let p1 = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == l1).unwrap()["id"].as_u64().unwrap();
    let ty = info["stair_types"].as_array().unwrap().iter().find(|t| t["name"] == "Timber Stair").unwrap()["id"].as_u64().unwrap();
    let base = json!({ "type_id": ty, "base_level": l1, "top_level": l2 });
    let mut p = base.clone();
    p["pending"] = json!({ "kind": "spiral_full", "points": [[0, 0], [1500, 0]], "width_mm": 1000 });
    let run = call(&mut s, "stair_draft", p)["pending"].clone();
    let mut p = base;
    p["runs"] = json!([run]);
    let id = call(&mut s, "add_stair", p)["id"].as_u64().unwrap();
    let lines = |s: &mut Session| -> Vec<Value> {
        let d = call(s, "plan", json!({ "view_id": p1 }));
        d["stairs"].as_array().unwrap().iter().filter(|x| x["id"] == id).flat_map(|x| x["lines"].as_array().unwrap().clone()).collect()
    };
    let radius = |l: &Value, k: &str| l[k][0].as_f64().unwrap().hypot(l[k][1].as_f64().unwrap());
    // the 45 stringers' outer faces, round the outside and the inside of the ring
    let ls = lines(&mut s);
    assert!(ls.iter().any(|l| (radius(l, "a") - 2045.0).abs() < 1.0), "outer stringer");
    assert!(ls.iter().any(|l| (radius(l, "a") - 955.0).abs() < 1.0), "inner stringer");
    assert!(!ls.iter().any(|l| l["kind"] == "nosing"));
    call(&mut s, "patch_view", json!({ "id": p1, "patch": { "settings": { "detail_level": "fine" } } }));
    assert!(lines(&mut s).iter().any(|l| l["kind"] == "nosing"));
    // 3D: the stringers in their timber
    let scene = call(&mut s, "scene", json!({}));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|x| x["id"] == id && x["material"] == "Timber Joist"));
    // an L winder with its stringers round the corner
    let mut p = json!({ "type_id": ty, "base_level": l1, "top_level": l2 });
    p["pending"] = json!({ "kind": "winder_l", "points": [[10000, 0], [10000, 2000], [15000, 2000]], "width_mm": 1000, "style": "balanced" });
    let run = call(&mut s, "stair_draft", p.clone())["pending"].clone();
    p["pending"] = Value::Null;
    p["runs"] = json!([run]);
    let w = call(&mut s, "add_stair", p)["id"].as_u64().unwrap();
    let scene = call(&mut s, "scene", json!({}));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|x| x["id"] == w && x["material"] == "Timber Joist"));
}
