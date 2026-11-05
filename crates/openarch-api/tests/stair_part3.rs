//! Stairs, step 4 part 3: sketched runs and landings, multistory stairs, and plans at Coarse and Fine detail.
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

fn line(a: [f64; 2], b: [f64; 2]) -> Value {
    json!({ "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] } })
}

#[test]
fn a_sketched_run_and_a_sketched_landing_make_a_stair() {
    let mut m = model();
    let base = json!({ "type_id": ty(&m, "Concrete Stair"), "base_level": m.l1, "top_level": m.l2 });
    let n = call(&mut m.s, "stair_draft", base.clone())["desired_risers"].as_u64().unwrap();
    let half = n / 2;
    // a run sketched as two boundaries (each in two pieces) and its riser lines, bottom first
    let len = (half - 1) as f64 * 270.0;
    let mut edges = vec![
        json!({ "curve": line([0.0, 0.0], [0.0, len / 2.0]), "role": "boundary" }), json!({ "curve": line([0.0, len / 2.0], [0.0, len]), "role": "boundary" }),
        json!({ "curve": line([1100.0, len], [1100.0, 0.0]), "role": "boundary" }),
    ];
    for k in 0..half { edges.push(json!({ "curve": line([0.0, k as f64 * 270.0], [1100.0, k as f64 * 270.0]), "role": "riser" })); }
    let run = call(&mut m.s, "stair_sketch_run", json!({ "edges": edges }))["run"].clone();
    assert_eq!(run["risers"], half);
    assert_eq!(run["width_mm"], 1100.0);
    // one boundary only: refused
    assert!(m.s.handle("stair_sketch_run", &json!({ "edges": [{ "curve": line([0.0, 0.0], [0.0, 1000.0]), "role": "boundary" }] })).is_err());
    // its landing, sketched as a closed outline at the top of it
    let outline = call(&mut m.s, "stair_sketch_landing", json!({ "curves": [line([0.0, len], [2300.0, len]), line([2300.0, len], [2300.0, len + 1200.0]), line([2300.0, len + 1200.0], [0.0, len + 1200.0]), line([0.0, len + 1200.0], [0.0, len])] }))["outline"].clone();
    assert_eq!(outline.as_array().unwrap().len(), 4);
    // and a straight run back down the other side
    let mut p = base.clone();
    p["runs"] = json!([run]);
    p["pending"] = json!({ "start": [1750, len], "end": [1750, -10000], "justification": "center", "width_mm": 1100 });
    let r2 = call(&mut m.s, "stair_draft", p)["pending"].clone();
    let mut p = base;
    p["runs"] = json!([run, r2]);
    p["landings"] = json!([{ "after_run": 0, "outline": outline }]);
    let id = call(&mut m.s, "add_stair", p)["id"].as_u64().unwrap();
    let e = call(&mut m.s, "stair_edit", json!({ "id": id }));
    assert_eq!(e["landings"].as_array().unwrap().len(), 1, "the sketched landing comes back for Edit Stairs");
    assert!(e["runs"][0]["shape"]["sketched"]["risers"].as_array().unwrap().len() as u64 == half);
    let d = call(&mut m.s, "describe", json!({ "id": id }));
    assert!(d["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("reach")), "{}", d["warnings"]);
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p1 }));
    assert!(plan["stairs"].as_array().unwrap().iter().any(|s| s["id"] == id));
}

#[test]
fn a_multistory_stair_shows_on_every_level_and_plans_follow_the_detail_level() {
    let mut m = model();
    let timber = ty(&m, "Timber Stair");
    let base = json!({ "type_id": timber, "base_level": m.l1, "top_level": m.l2 });
    let mut p = base.clone();
    p["pending"] = json!({ "start": [0, 0], "end": [0, 20000], "justification": "center", "width_mm": 1000 });
    let run = call(&mut m.s, "stair_draft", p)["pending"].clone();
    let mut p = base;
    p["runs"] = json!([run]);
    let id = call(&mut m.s, "add_stair", p)["id"].as_u64().unwrap();
    // two more levels, and the stair up to the top one
    let e2 = m.info["levels"].as_array().unwrap().iter().find(|l| l["id"] == m.l2).unwrap()["elevation_mm"].as_f64().unwrap();
    let e1 = m.info["levels"].as_array().unwrap().iter().find(|l| l["id"] == m.l1).unwrap()["elevation_mm"].as_f64().unwrap();
    let l3 = call(&mut m.s, "add_level", json!({ "name": "Level 3", "elevation": e2 + (e2 - e1), "make_plan": true }))["id"].as_u64().unwrap();
    let l4 = call(&mut m.s, "add_level", json!({ "name": "Level 4", "elevation": e2 + 2.0 * (e2 - e1), "make_plan": true }))["id"].as_u64().unwrap();
    call(&mut m.s, "patch_stair", json!({ "id": id, "patch": { "multistory_top": l4 } }));
    let info = call(&mut m.s, "doc_info", json!({}));
    let plan3 = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == l3).unwrap()["id"].as_u64().unwrap();
    let drawn = call(&mut m.s, "plan", json!({ "view_id": plan3 }));
    let here: Vec<&Value> = drawn["stairs"].as_array().unwrap().iter().filter(|s| s["id"] == id).collect();
    assert!(here.iter().any(|s| s["label"] == "UP") && here.iter().any(|s| s["label"] == "DN"), "Level 3 sees the storey arriving and the one leaving");
    let path = std::env::temp_dir().join(format!("openarch-multistory-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains("'Storey 3, Run 1'"));
    // detail: Fine draws the nosings, Coarse leaves out the risers
    let kinds = |s: &mut Session| -> Vec<String> {
        let d = call(s, "plan", json!({ "view_id": m.p1 }));
        d["stairs"].as_array().unwrap().iter().filter(|x| x["id"] == id).flat_map(|x| x["lines"].as_array().unwrap().iter().map(|l| l["kind"].as_str().unwrap().to_string()).collect::<Vec<_>>()).collect()
    };
    assert!(kinds(&mut m.s).iter().any(|k| k == "riser") && !kinds(&mut m.s).iter().any(|k| k == "nosing"));
    call(&mut m.s, "patch_view", json!({ "id": m.p1, "patch": { "settings": { "detail_level": "fine" } } }));
    assert!(kinds(&mut m.s).iter().any(|k| k == "nosing"));
    call(&mut m.s, "patch_view", json!({ "id": m.p1, "patch": { "settings": { "detail_level": "coarse" } } }));
    assert!(!kinds(&mut m.s).iter().any(|k| k == "riser"));
}
