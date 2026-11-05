//! Stairs by component: the Stair tool's draft and Finish, riser balancing, automatic landings, Properties, plans
//! (cut mark, UP and DN), 3D, sections, Move, schedules and IFC.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

struct M { s: Session, l1: u64, l2: u64, p1: u64, p2: u64, ty: u64, info: Value }

fn model() -> M {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let mut levels: Vec<(u64, f64)> = info["levels"].as_array().unwrap().iter().map(|l| (l["id"].as_u64().unwrap(), l["elevation_mm"].as_f64().unwrap())).collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (l1, l2) = (levels[0].0, levels[1].0);
    let plan = |l: u64| info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == l).unwrap()["id"].as_u64().unwrap();
    let (p1, p2) = (plan(l1), plan(l2));
    let ty = info["stair_types"].as_array().unwrap().iter().find(|t| t["name"] == "Concrete Stair").expect("starter stair types")["id"].as_u64().unwrap();
    M { s, l1, l2, p1, p2, ty, info }
}

fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))["value"].clone()
}

/// A U-shaped stair drawn as the tool does: the first run up the y axis, then the second back down beside it.
fn u_stair(m: &mut M) -> u64 {
    let base = json!({ "type_id": m.ty, "base_level": m.l1, "top_level": m.l2 });
    let d = call(&mut m.s, "stair_draft", base.clone());
    let n = d["desired_risers"].as_u64().unwrap();
    let half = n / 2;
    // drawing the first run: its length holds half the risers
    let mut p = base.clone();
    p["pending"] = json!({ "start": [0, 0], "end": [0, (half - 1) as f64 * 250.0], "justification": "center", "width_mm": 1000 });
    let r1 = call(&mut m.s, "stair_draft", p)["pending"].clone();
    assert_eq!(r1["risers"].as_u64().unwrap(), half);
    // the second run, drawn longer than what remains, takes only the rest
    let mut p = base.clone();
    p["runs"] = json!([r1]);
    p["pending"] = json!({ "start": [1200, (half - 1) as f64 * 250.0], "end": [1200, -20000], "justification": "center", "width_mm": 1000 });
    let draft = call(&mut m.s, "stair_draft", p);
    let r2 = draft["pending"].clone();
    assert_eq!(r2["risers"].as_u64().unwrap(), n - half, "only the risers that remain");
    assert_eq!(draft["remaining"], 0);
    let mut p = base;
    p["runs"] = json!([r1, r2]);
    call(&mut m.s, "add_stair", p)["id"].as_u64().unwrap()
}

#[test]
fn a_u_stair_is_drawn_run_by_run_and_balances_its_risers() {
    let mut m = model();
    let id = u_stair(&mut m);
    let n: u64 = row(&mut m.s, id, "Actual Number of Risers").as_str().unwrap().parse().unwrap();
    let elev = |l: u64, info: &Value| info["levels"].as_array().unwrap().iter().find(|x| x["id"] == l).unwrap()["elevation_mm"].as_f64().unwrap();
    let h = elev(m.l2, &m.info) - elev(m.l1, &m.info);
    let r: f64 = row(&mut m.s, id, "Actual Riser Height").as_str().unwrap().parse().unwrap();
    assert!((r - h / n as f64).abs() < 0.06, "riser {r} for {h} over {n}");
    assert_eq!(row(&mut m.s, id, "Calculation Rules"), "Met");
    assert_eq!(row(&mut m.s, id, "Number of Runs"), "2");

    // the lower plan: cut, solid below and dashed above, UP
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p1 }));
    let st = plan["stairs"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
    assert_eq!(st["label"], "UP");
    assert!(!st["cut"].as_array().unwrap().is_empty(), "a cut mark");
    let lines = st["lines"].as_array().unwrap();
    assert!(lines.iter().any(|l| l["above"] == true) && lines.iter().any(|l| l["above"] == false));
    // the upper plan sees it whole, DN
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p2 }));
    let st = plan["stairs"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
    assert_eq!(st["label"], "DN");
    assert!(st["cut"].as_array().unwrap().is_empty() && st["lines"].as_array().unwrap().iter().all(|l| l["above"] == false));

    // 3D: concrete
    let scene = call(&mut m.s, "scene", json!({}));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|x| x["id"] == id && x["kind"] == "Stair" && x["material"] == "Concrete"));

    // a section across the first run cuts it
    let view = call(&mut m.s, "add_section", json!({ "level_id": m.l1, "start": [-1000, 800], "end": [3000, 800] }))["view_id"].as_u64().unwrap();
    let items = call(&mut m.s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().clone();
    assert!(items.iter().any(|i| i["id"] == id && i["category"] == "Stairs" && i["cut"] == true), "the section cuts the flight");
    // the south elevation sees it
    let south = m.info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "elevation" && v["name"] == "South").unwrap()["id"].as_u64().unwrap();
    let seen = call(&mut m.s, "elevation", json!({ "view_id": south }))["items"].as_array().unwrap().clone();
    assert!(seen.iter().any(|i| i["id"] == id && i["category"] == "Stairs"));

    // IFC: the stair, its two flights and its landing
    let path = std::env::temp_dir().join(format!("openarch-stairs-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(text.matches("=IFCSTAIR(").count(), 1);
    assert!(text.contains(".HALF_TURN_STAIR."));
    assert_eq!(text.matches("=IFCSTAIRFLIGHT(").count(), 2);
    assert!(text.contains(".LANDING."));
    assert!(text.contains("'Pset_StairCommon'") && text.contains("'NumberOfRiser'"));
}

#[test]
fn properties_change_the_riser_count_and_width_and_the_stair_moves() {
    let mut m = model();
    let id = u_stair(&mut m);
    let before: u64 = row(&mut m.s, id, "Actual Number of Risers").as_str().unwrap().parse().unwrap();
    // one riser fewer comes off the last run
    call(&mut m.s, "patch_stair", json!({ "id": id, "patch": { "desired_risers": before - 1 } }));
    assert_eq!(row(&mut m.s, id, "Actual Number of Risers"), json!((before - 1).to_string()));
    // a narrow run breaks the rules: a warning, not an error
    call(&mut m.s, "patch_stair", json!({ "id": id, "patch": { "run_width_mm": 800 } }));
    let p = call(&mut m.s, "describe", json!({ "id": id }));
    assert!(p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("under the minimum of 1000")), "{}", p["warnings"]);
    call(&mut m.s, "undo", json!({}));
    call(&mut m.s, "undo", json!({}));
    assert_eq!(row(&mut m.s, id, "Actual Number of Risers"), json!(before.to_string()));
    // Move takes the runs along
    call(&mut m.s, "transform", json!({ "ids": [id], "kind": "move", "dx": 5000, "dy": 0 }));
    let e = call(&mut m.s, "stair_edit", json!({ "id": id }));
    assert_eq!(e["runs"][0]["start"]["x"], 5000.0);
    // Stairs schedule
    let sched = call(&mut m.s, "add_schedule", json!({ "category": "Stairs", "name": "Stair Schedule" }))["id"].as_u64().unwrap();
    let mut def = call(&mut m.s, "schedule_table", json!({ "id": sched }))["def"].clone();
    def["fields"] = json!([{ "name": "Type", "kind": "type" }, { "name": "Actual Number of Risers", "kind": "instance" }, { "name": "Actual Riser Height", "kind": "instance" }]);
    call(&mut m.s, "set_schedule", json!({ "id": sched, "def": def }));
    let table = call(&mut m.s, "schedule_table", json!({ "id": sched }));
    assert_eq!(table["rows"].as_array().unwrap().iter().filter(|r| r["kind"] == "item").count(), 1, "{table}");
    // Edit Stairs: one run only, no landing
    let runs = e["runs"].as_array().unwrap()[..1].to_vec();
    call(&mut m.s, "set_stair_runs", json!({ "id": id, "runs": runs }));
    assert_eq!(row(&mut m.s, id, "Number of Runs"), "1");
    let p = call(&mut m.s, "describe", json!({ "id": id }));
    assert!(p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("does not reach its top")));
    // its type opens in Type Properties
    let t = call(&mut m.s, "describe_any_type", json!({ "id": m.ty }));
    assert_eq!((t["patch"].as_str(), t["duplicate"].as_str()), (Some("patch_stair_type"), Some("duplicate_stair_type")));
    call(&mut m.s, "patch_stair_type", json!({ "id": m.ty, "patch": { "rules": { "max_riser_mm": 150 } } }));
    let p = call(&mut m.s, "describe", json!({ "id": id }));
    assert!(p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("over the maximum of 150")));
}
