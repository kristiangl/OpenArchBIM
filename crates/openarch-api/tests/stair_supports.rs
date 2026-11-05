//! Stairs, step 4 part 1: supports (stringers, carriages, middle supports), each run's own properties, and the
//! headroom check.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

struct M { s: Session, l1: u64, l2: u64, p1: u64, info: Value }

fn model() -> M {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let mut levels: Vec<(u64, f64)> = info["levels"].as_array().unwrap().iter().map(|l| (l["id"].as_u64().unwrap(), l["elevation_mm"].as_f64().unwrap())).collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (l1, l2) = (levels[0].0, levels[1].0);
    let p1 = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == l1).unwrap()["id"].as_u64().unwrap();
    M { s, l1, l2, p1, info }
}

fn ty(m: &M, name: &str) -> u64 {
    m.info["stair_types"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["id"].as_u64().unwrap()
}

/// A straight stair of every riser along the y axis.
fn straight(m: &mut M, t: u64) -> u64 {
    let base = json!({ "type_id": t, "base_level": m.l1, "top_level": m.l2 });
    let mut p = base.clone();
    p["pending"] = json!({ "start": [0, 0], "end": [0, 20000], "justification": "center", "width_mm": 1000 });
    let run = call(&mut m.s, "stair_draft", p)["pending"].clone();
    let mut p = base;
    p["runs"] = json!([run]);
    call(&mut m.s, "add_stair", p)["id"].as_u64().unwrap()
}

fn warnings(s: &mut Session, id: u64) -> Vec<String> {
    call(s, "describe", json!({ "id": id }))["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect()
}

#[test]
fn a_timber_stair_stands_on_stringers_and_each_run_has_its_own_properties() {
    let mut m = model();
    let timber = ty(&m, "Timber Stair");
    let id = straight(&mut m, timber);
    // 3D: the stringers in their own timber
    let scene = call(&mut m.s, "scene", json!({}));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|x| x["id"] == id && x["material"] == "Timber Joist"));
    // the plan draws the stringers' faces beside the run
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p1 }));
    let st = plan["stairs"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
    let xs: Vec<f64> = st["lines"].as_array().unwrap().iter().flat_map(|l| [l["a"][0].as_f64().unwrap(), l["b"][0].as_f64().unwrap()]).collect();
    assert!(xs.iter().any(|x| (x - 545.0).abs() < 1e-6) && xs.iter().any(|x| (x + 545.0).abs() < 1e-6), "stringer faces at 545: {xs:?}");
    // IFC: the supports as a stringer member
    let path = std::env::temp_dir().join(format!("openarch-supports-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains(".STRINGER."));
    // the run's own Properties: End with Riser off adds a tread; Exterior Support: Left moves the run over by the stringer
    let d = call(&mut m.s, "describe", json!({ "id": id }));
    let g = d["run_groups"][0].clone();
    assert_eq!(g["target"]["method"], "patch_stair_run");
    let len = |s: &mut Session| call(s, "describe", json!({ "id": id }))["run_groups"][0]["rows"].as_array().unwrap().iter().find(|r| r["label"] == "Run Length").unwrap()["raw"].as_f64().unwrap();
    let before = len(&mut m.s);
    call(&mut m.s, "patch_stair_run", json!({ "id": id, "run": 0, "patch": { "ends_with_riser": false } }));
    assert!((len(&mut m.s) - before - 250.0).abs() < 1e-6);
    call(&mut m.s, "patch_stair_run", json!({ "id": id, "run": 0, "patch": { "justification": "left_exterior" } }));
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p1 }));
    let st = plan["stairs"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
    let xs: Vec<f64> = st["lines"].as_array().unwrap().iter().flat_map(|l| [l["a"][0].as_f64().unwrap(), l["b"][0].as_f64().unwrap()]).collect();
    // the left stringer's outer face is on the location line: stringer, run and right stringer to the right of it
    assert!((xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 1090.0).abs() < 1e-6, "{xs:?}");
    assert!((xs.iter().cloned().fold(f64::INFINITY, f64::min) - 0.0).abs() < 1e-6, "{xs:?}");
    // a middle carriage in the type
    let rows = call(&mut m.s, "describe_any_type", json!({ "id": timber }));
    assert!(rows.to_string().contains("Middle Support"));
    call(&mut m.s, "patch_stair_type", json!({ "id": timber, "patch": { "supports": { "middle": { "kind": "carriage", "width_mm": 90, "total_depth_mm": 200 } } } }));
    let scene = call(&mut m.s, "scene", json!({}));
    // the carriage has no material of its own: its own mesh
    assert!(scene["meshes"].as_array().unwrap().iter().any(|x| x["id"] == id && x["material"].is_null()));
}

#[test]
fn headroom_under_a_floor_above_is_checked() {
    let mut m = model();
    let concrete = ty(&m, "Concrete Stair");
    let id = straight(&mut m, concrete);
    assert!(!warnings(&mut m.s, id).iter().any(|w| w.contains("headroom")));
    // a slab on the level above, over the whole stair with no opening for it
    let ft = m.info["floor_types"][0]["id"].clone();
    let floor = call(&mut m.s, "add_floor", json!({ "type_id": ft, "level_id": m.l2, "points": [[-2000, -2000], [2000, -2000], [2000, 6000], [-2000, 6000]] }))["id"].as_u64().unwrap();
    let w = warnings(&mut m.s, id);
    assert!(w.iter().any(|x| x.contains("headroom over run 1")), "{w:?}");
    // the check can be switched off in the type
    let t = ty(&m, "Concrete Stair");
    call(&mut m.s, "patch_stair_type", json!({ "id": t, "patch": { "rules": { "check_headroom": false } } }));
    assert!(!warnings(&mut m.s, id).iter().any(|x| x.contains("headroom")));
    call(&mut m.s, "undo", json!({}));
    call(&mut m.s, "delete", json!({ "ids": [floor] }));
    assert!(!warnings(&mut m.s, id).iter().any(|x| x.contains("headroom")));
}
