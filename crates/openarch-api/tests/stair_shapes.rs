//! Stairs, step 4 part 2: spiral (full-step and centre-ends) and winder (L and U) runs, drawn through the Stair
//! tool's draft, in plans, 3D, with railings and in IFC.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

struct M { s: Session, l1: u64, l2: u64, p1: u64, p2: u64, ty: u64, rail: u64 }

fn model() -> M {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let mut levels: Vec<(u64, f64)> = info["levels"].as_array().unwrap().iter().map(|l| (l["id"].as_u64().unwrap(), l["elevation_mm"].as_f64().unwrap())).collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (l1, l2) = (levels[0].0, levels[1].0);
    let plan = |l: u64| info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == l).unwrap()["id"].as_u64().unwrap();
    let ty = info["stair_types"].as_array().unwrap().iter().find(|t| t["name"] == "Concrete Stair").unwrap()["id"].as_u64().unwrap();
    let rail = info["railing_types"][0]["id"].as_u64().unwrap();
    M { s, p1: plan(l1), p2: plan(l2), l1, l2, ty, rail }
}

fn draw(m: &mut M, pending: Value) -> u64 {
    let base = json!({ "type_id": m.ty, "base_level": m.l1, "top_level": m.l2 });
    let mut p = base.clone();
    p["pending"] = pending;
    let d = call(&mut m.s, "stair_draft", p);
    assert!(!d["pending"].is_null(), "{d}");
    assert_eq!(d["remaining"], 0, "{d}");
    let mut p = base;
    p["runs"] = json!([d["pending"].clone()]);
    p["railing_type"] = json!(m.rail);
    call(&mut m.s, "add_stair", p)["id"].as_u64().unwrap()
}

#[test]
fn a_full_step_spiral_takes_every_riser_and_draws_in_plan_3d_and_ifc() {
    let mut m = model();
    let id = draw(&mut m, json!({ "kind": "spiral_full", "points": [[0, 0], [1500, 0]], "width_mm": 1000 }));
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p1 }));
    let st = plan["stairs"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
    assert_eq!(st["label"], "UP");
    assert!(!st["cut"].as_array().unwrap().is_empty(), "a cut mark across a riser");
    let lines = st["lines"].as_array().unwrap();
    assert!(lines.iter().any(|l| l["above"] == true) && lines.iter().any(|l| l["above"] == false));
    // every line within the spiral's ring
    for l in lines {
        for k in ["a", "b"] {
            let r = l[k][0].as_f64().unwrap().hypot(l[k][1].as_f64().unwrap());
            assert!((999.0..=2001.0).contains(&r), "{r}");
        }
    }
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p2 }));
    assert_eq!(plan["stairs"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap()["label"], "DN");
    let scene = call(&mut m.s, "scene", json!({}));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|x| x["id"] == id && x["kind"] == "Stair"));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|x| x["kind"] == "Railing"));
    let path = std::env::temp_dir().join(format!("openarch-spiral-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains(".SPIRAL_STAIR.") && text.contains(".SPIRAL.)"));
}

#[test]
fn centre_ends_spirals_and_winders_count_their_risers_from_what_is_drawn() {
    let mut m = model();
    let base = json!({ "type_id": m.ty, "base_level": m.l1, "top_level": m.l2 });
    // a quarter turn at radius 1500: 2356 along the walkline, nine treads and ten risers
    let mut p = base.clone();
    p["pending"] = json!({ "kind": "spiral_ends", "points": [[0, 0], [1500, 0], [0, 1500]], "width_mm": 1000 });
    let d = call(&mut m.s, "stair_draft", p);
    assert_eq!(d["pending"]["risers"], 10);
    assert_eq!(d["pending"]["shape"]["spiral"]["clockwise"], false);
    // drawn the other way round it turns clockwise
    let mut p = base.clone();
    p["pending"] = json!({ "kind": "spiral_ends", "points": [[0, 0], [1500, 0], [0, -1500]], "width_mm": 1000 });
    assert_eq!(call(&mut m.s, "stair_draft", p)["pending"]["shape"]["spiral"]["clockwise"], true);
    // an L winder over every riser, with its railings going round the outside of the corner
    let id = draw(&mut m, json!({ "kind": "winder_l", "points": [[0, 0], [0, 2000], [5000, 2000]], "width_mm": 1000, "style": "balanced" }));
    let rails: Vec<u64> = call(&mut m.s, "plan", json!({ "view_id": m.p1 }))["railings"].as_array().unwrap().iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(rails.len(), 2);
    let path = std::env::temp_dir().join(format!("openarch-winder-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains(".QUARTER_WINDING_STAIR.") && text.contains(".WINDER.)"));
    // a U winder
    let mut p = base;
    p["pending"] = json!({ "kind": "winder_u", "points": [[0, 0], [0, 1500], [1200, 1500], [1200, -3000]], "width_mm": 1000, "style": "single_point" });
    let d = call(&mut m.s, "stair_draft", p);
    assert!(d["pending"]["risers"].as_u64().unwrap() > 1);
    // Move takes a winder's corners along
    call(&mut m.s, "transform", json!({ "ids": [id], "kind": "move", "dx": 10000, "dy": 0 }));
    let e = call(&mut m.s, "stair_edit", json!({ "id": id }));
    assert_eq!(e["runs"][0]["shape"]["winder"]["corners"][0]["x"], 10000.0);
}
