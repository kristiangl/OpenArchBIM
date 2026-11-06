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

/// A U-shaped stair of two straight runs with a landing between them.
fn u_stair(m: &mut M, ty: &str) -> u64 {
    let t = m.info["stair_types"].as_array().unwrap().iter().find(|t| t["name"] == ty).unwrap()["id"].as_u64().unwrap();
    let base = json!({ "type_id": t, "base_level": m.l1, "top_level": m.l2 });
    let n = call(&mut m.s, "stair_draft", base.clone())["desired_risers"].as_u64().unwrap();
    let len = (n / 2 - 1) as f64 * 250.0;
    let mut p = base.clone();
    p["pending"] = json!({ "start": [0, 0], "end": [0, len], "justification": "center", "width_mm": 1000 });
    let r1 = call(&mut m.s, "stair_draft", p)["pending"].clone();
    let mut p = base.clone();
    p["runs"] = json!([r1]);
    p["pending"] = json!({ "start": [1200, len], "end": [1200, -10000], "justification": "center", "width_mm": 1000 });
    let r2 = call(&mut m.s, "stair_draft", p)["pending"].clone();
    let mut p = base;
    p["runs"] = json!([r1, r2]);
    call(&mut m.s, "add_stair", p)["id"].as_u64().unwrap()
}

fn plan_items(s: &mut Session, view: u64, id: u64) -> Vec<Value> {
    call(s, "plan", json!({ "view_id": view }))["stairs"].as_array().unwrap().iter().filter(|x| x["id"] == id).cloned().collect()
}

#[test]
fn a_run_or_landing_picked_on_its_own_has_its_own_properties() {
    let mut m = model();
    let id = u_stair(&mut m, "Concrete Stair");
    let item = plan_items(&mut m.s, m.p1, id).remove(0);
    let parts = item["parts"].as_array().unwrap().clone();
    assert_eq!(parts.iter().filter(|p| p["kind"] == "run").count(), 2);
    assert_eq!(parts.iter().filter(|p| p["kind"] == "landing").count(), 1);
    // the second run: its rows, and a change through its target
    let d = call(&mut m.s, "stair_part", json!({ "id": id, "part": { "kind": "run", "index": 1, "storey": null } }));
    assert_eq!(d["title"], "Stair Run: Run 2");
    let t = d["target"].clone();
    assert_eq!(t["method"], "patch_stair_run");
    let mut args = json!({ "id": id, "patch": { "width_mm": 1200 } });
    for (k, v) in t["extra"].as_object().unwrap() { args[k] = v.clone(); }
    call(&mut m.s, "patch_stair_run", args);
    let e = call(&mut m.s, "stair_edit", json!({ "id": id }));
    assert_eq!(e["runs"][1]["width_mm"], 1200.0);
    assert_eq!(e["runs"][0]["width_mm"], 1000.0);
    // the landing
    let d = call(&mut m.s, "stair_part", json!({ "id": id, "part": { "kind": "landing", "index": 0, "storey": null } }));
    assert_eq!(d["kind"], "StairLanding");
    assert!(d.to_string().contains("Automatic") && d.to_string().contains("Thickness"));
}

#[test]
fn a_storey_of_a_multistory_stair_unpins_to_change_on_its_own() {
    let mut m = model();
    let id = u_stair(&mut m, "Timber Stair");
    let e1 = m.info["levels"].as_array().unwrap().iter().find(|l| l["id"] == m.l1).unwrap()["elevation_mm"].as_f64().unwrap();
    let e2 = m.info["levels"].as_array().unwrap().iter().find(|l| l["id"] == m.l2).unwrap()["elevation_mm"].as_f64().unwrap();
    let l3 = call(&mut m.s, "add_level", json!({ "name": "Level 3", "elevation": e2 + (e2 - e1), "make_plan": true }))["id"].as_u64().unwrap();
    call(&mut m.s, "patch_stair", json!({ "id": id, "patch": { "multistory_top": l3 } }));
    let info = call(&mut m.s, "doc_info", json!({}));
    let p2 = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == m.l2).unwrap()["id"].as_u64().unwrap();
    // Properties list the storey above, pinned
    let d = call(&mut m.s, "describe", json!({ "id": id }));
    let row = d["groups"].as_array().unwrap().iter().find(|g| g["name"] == "Multistory").unwrap()["rows"][0].clone();
    assert_eq!(row["raw"], true, "{row}");
    // pinned: Edit Stairs from Level 2 edits the stair itself
    assert!(call(&mut m.s, "stair_edit", json!({ "id": id, "level": m.l2 }))["storey"].is_null());
    assert!(m.s.handle("patch_stair_run", &json!({ "id": id, "storey": m.l2, "run": 0, "patch": { "width_mm": 1400 } })).is_err(), "a pinned storey is the stair's");
    // unpin it through its row, then give it runs of its own
    call(&mut m.s, "patch_stair", json!({ "id": id, "patch": { "storey_pinned": { m.l2.to_string(): false } } }));
    let e = call(&mut m.s, "stair_edit", json!({ "id": id, "level": m.l2 }));
    assert_eq!(e["storey"], m.l2);
    assert_eq!(e["base_level"], m.l2);
    let mut runs = e["runs"].as_array().unwrap().clone();
    runs[0]["width_mm"] = json!(1400);
    runs[1]["width_mm"] = json!(1400);
    call(&mut m.s, "set_stair_runs", json!({ "id": id, "storey": m.l2, "runs": runs, "landings": [] }));
    let base = call(&mut m.s, "stair_edit", json!({ "id": id }));
    assert_eq!(base["runs"][0]["width_mm"], 1000.0, "the stair below keeps its runs");
    // the plan of Level 2 sees the storey leaving it with its own width, and it says which storey it is
    let up = plan_items(&mut m.s, p2, id).into_iter().find(|x| x["label"] == "UP").unwrap();
    assert_eq!(up["storey"], m.l2);
    let ring = up["parts"].as_array().unwrap().iter().find(|p| p["kind"] == "run").unwrap()["ring"].clone();
    let xs: Vec<f64> = ring.as_array().unwrap().iter().map(|p| p[0].as_f64().unwrap()).collect();
    let w = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - xs.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!((w - 1400.0).abs() < 1e-6, "{w}");
    // its run's own Properties change the storey only
    let d = call(&mut m.s, "stair_part", json!({ "id": id, "part": { "kind": "run", "index": 1, "storey": m.l2 } }));
    assert_eq!(d["target"]["extra"]["storey"], m.l2);
    assert!(d.to_string().contains("(unpinned)"));
    call(&mut m.s, "patch_stair_run", json!({ "id": id, "storey": m.l2, "run": 1, "patch": { "width_mm": 1100 } }));
    assert_eq!(call(&mut m.s, "stair_edit", json!({ "id": id, "level": m.l2 }))["runs"][1]["width_mm"], 1100.0);
    // Move takes it along; IFC still has both storeys
    call(&mut m.s, "transform", json!({ "ids": [id], "kind": "move", "dx": 5000, "dy": 0 }));
    let e = call(&mut m.s, "stair_edit", json!({ "id": id, "level": m.l2 }));
    assert_eq!(e["runs"][0]["start"]["x"], 5000.0);
    let path = std::env::temp_dir().join(format!("openarch-unpinned-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains("'Storey 2, Run 2'"));
    // pinned again, it repeats the stair below
    call(&mut m.s, "set_stair_storey_pinned", json!({ "id": id, "level": m.l2, "pinned": true }));
    assert!(call(&mut m.s, "stair_edit", json!({ "id": id, "level": m.l2 }))["storey"].is_null());
    call(&mut m.s, "undo", json!({}));
    assert_eq!(call(&mut m.s, "stair_edit", json!({ "id": id, "level": m.l2 }))["storey"], m.l2);
}

fn railing_type(m: &M, name: &str) -> u64 {
    m.info["railing_types"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["id"].as_u64().unwrap()
}

/// Each railing mesh's vertex count and lowest point (m), by id.
fn railing_mesh(s: &mut Session, id: u64) -> (usize, f64) {
    let scene = call(s, "scene", json!({}));
    let mut n = 0;
    let mut lo = f64::INFINITY;
    for x in scene["meshes"].as_array().unwrap().iter().filter(|x| x["id"] == id) {
        let v = x["verts"].as_array().unwrap();
        n += v.len() / 6;
        for k in (0..v.len()).step_by(6) { lo = lo.min(v[k + 2].as_f64().unwrap()); }
    }
    (n, lo)
}

#[test]
fn handrails_end_in_their_termination_and_3d_follows_the_detail_level() {
    let mut m = model();
    let id = u_stair(&mut m, "Concrete Stair");
    let rail = railing_type(&m, "Balustrade 1000");
    let ids: Vec<u64> = call(&mut m.s, "add_stair_railings", json!({ "stair_id": id, "type_id": rail, "placement": "treads" }))["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let r = ids[0];
    let v3 = m.info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "three_d").unwrap()["id"].as_u64().unwrap();
    let detail = |s: &mut Session, d: &str| { call(s, "patch_view", json!({ "id": v3, "patch": { "settings": { "detail_level": d } } })); };
    // Medium: the Wall termination returns each end; with None the rail just stops
    let (wall, _) = railing_mesh(&mut m.s, r);
    call(&mut m.s, "patch_railing_type", json!({ "id": rail, "patch": { "handrail_1": { "termination": "none" } } }));
    let (none, _) = railing_mesh(&mut m.s, r);
    assert!(wall > none, "{wall} {none}");
    call(&mut m.s, "patch_railing_type", json!({ "id": rail, "patch": { "handrail_1": { "termination": "floor" } } }));
    let (floor, _) = railing_mesh(&mut m.s, r);
    assert!(floor > none);
    // Fine: rounder rails and the handrail's brackets; Coarse: one bar
    detail(&mut m.s, "fine");
    let (fine, _) = railing_mesh(&mut m.s, r);
    detail(&mut m.s, "coarse");
    let (coarse, _) = railing_mesh(&mut m.s, r);
    assert!(coarse < floor && floor < fine, "{coarse} {floor} {fine}");
    let stair_verts = |s: &mut Session| -> usize { call(s, "scene", json!({}))["meshes"].as_array().unwrap().iter().filter(|x| x["id"] == id).map(|x| x["verts"].as_array().unwrap().len()).sum() };
    let coarse_stair = stair_verts(&mut m.s);
    detail(&mut m.s, "medium");
    assert!(coarse_stair < stair_verts(&mut m.s), "a coarse stair is a plain flight");
    // Flip moves a stair railing's handrail to the outside
    let hand_x = |s: &mut Session| -> f64 {
        let plan = call(s, "plan", json!({ "view_id": m.p1 }));
        let it = plan["railings"].as_array().unwrap().iter().find(|x| x["id"] == r).unwrap().clone();
        it["polys"].as_array().unwrap().iter().flat_map(|p| p["pts"].as_array().unwrap().iter().map(|q| q[0].as_f64().unwrap()).collect::<Vec<_>>()).fold(f64::INFINITY, f64::min)
    };
    let before = hand_x(&mut m.s);
    call(&mut m.s, "patch_railing", json!({ "id": r, "patch": { "flipped": true } }));
    assert!(hand_x(&mut m.s) < before - 50.0, "the handrail goes outside the stair's edge");
}

#[test]
fn a_sketched_railing_stands_on_a_floor_and_keeps_its_arcs() {
    let mut m = model();
    let rail = railing_type(&m, "Balustrade 1000");
    let ft = m.info["floor_types"][0]["id"].clone();
    let floor = call(&mut m.s, "add_floor", json!({ "type_id": ft, "level_id": m.l1, "points": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]] }))["id"].as_u64().unwrap();
    call(&mut m.s, "patch_floor", json!({ "id": floor, "patch": { "offset_mm": 500 } }));
    let curves = json!([
        { "type": "line", "a": { "x": 500, "y": 500 }, "b": { "x": 3000, "y": 500 } },
        { "type": "arc", "start": { "x": 3000, "y": 500 }, "end": { "x": 5000, "y": 2500 }, "centre": { "x": 3000, "y": 2500 }, "ccw": true },
    ]);
    let r = call(&mut m.s, "add_railing", json!({ "type_id": rail, "level_id": m.l1, "curves": curves }))["id"].as_u64().unwrap();
    assert!(railing_mesh(&mut m.s, r).1.abs() < 1e-6, "on its level");
    call(&mut m.s, "patch_railing", json!({ "id": r, "patch": { "floor_host": floor } }));
    assert!((railing_mesh(&mut m.s, r).1 - 0.5).abs() < 1e-6, "on the floor's top");
    let d = call(&mut m.s, "describe", json!({ "id": r }));
    assert!(d.to_string().contains("\"Floor\""));
    assert!(d["warnings"].as_array().unwrap().is_empty(), "{}", d["warnings"]);
    // Edit Path gives the arc back as an arc
    let p = call(&mut m.s, "railing_path", json!({ "id": r }));
    assert!(p["curves"].as_array().unwrap().iter().any(|c| c["curve"]["type"] == "arc" || c["type"] == "arc"), "{p}");
    // a path running off the floor warns
    call(&mut m.s, "set_railing_path", json!({ "id": r, "curves": [{ "type": "line", "a": { "x": 500, "y": 500 }, "b": { "x": 9000, "y": 500 } }] }));
    let d = call(&mut m.s, "describe", json!({ "id": r }));
    assert!(d["warnings"].to_string().contains("off its floor"), "{}", d["warnings"]);
    // a railing on a stair cannot also stand on a floor
    let id = u_stair(&mut m, "Concrete Stair");
    let ids = call(&mut m.s, "add_stair_railings", json!({ "stair_id": id, "type_id": rail, "placement": "treads" }))["ids"].clone();
    assert!(m.s.handle("patch_railing", &json!({ "id": ids[0], "patch": { "floor_host": floor } })).is_err());
}
