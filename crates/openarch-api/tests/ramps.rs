//! Ramps: the Ramp tool's draft and Finish (with railings), the slope, run length and landing checks, plans, 3D,
//! sections, Properties, Type Properties and IFC.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))["value"].clone()
}

#[test]
fn an_accessible_ramp_is_drawn_in_two_runs_with_a_landing_and_railings() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let l1 = info["levels"].as_array().unwrap().iter().min_by(|a, b| a["elevation_mm"].as_f64().unwrap().total_cmp(&b["elevation_mm"].as_f64().unwrap())).unwrap()["id"].as_u64().unwrap();
    let p1 = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == l1).unwrap()["id"].as_u64().unwrap();
    let ty = info["ramp_types"].as_array().unwrap().iter().find(|t| t["name"] == "Accessible Ramp 1:14").unwrap()["id"].as_u64().unwrap();
    let rail = info["railing_types"][0]["id"].as_u64().unwrap();
    // 600 up at 1:14 needs 8400
    let base = json!({ "type_id": ty, "base_level": l1, "top_level": null, "unconnected_height_mm": 600 });
    let mut p = base.clone();
    p["pending"] = json!({ "start": [0, 0], "end": [4200, 0], "justification": "center", "width_mm": 1200 });
    let d = call(&mut s, "ramp_draft", p);
    assert_eq!(d["needed_mm"], 8400.0);
    let r1 = d["pending"].clone();
    // the second run, drawn past the top, stops where the ramp gets there
    let mut p = base.clone();
    p["runs"] = json!([r1]);
    p["pending"] = json!({ "start": [5700, 0], "end": [20000, 0], "justification": "center", "width_mm": 1200 });
    let d = call(&mut s, "ramp_draft", p);
    let r2 = d["pending"].clone();
    assert_eq!(r2["end"]["x"], 9900.0);
    assert_eq!(d["remaining_mm"], 0.0);
    let mut p = base;
    p["runs"] = json!([r1, r2]);
    p["railing_type"] = json!(rail);
    let made = call(&mut s, "add_ramp", p);
    let id = made["id"].as_u64().unwrap();
    assert_eq!(made["railings"].as_array().unwrap().len(), 2);
    assert_eq!(row(&mut s, id, "Slope"), "1:14.0");
    assert_eq!(row(&mut s, id, "Slope and Lengths"), "Met");
    let rid = made["railings"][0].as_u64().unwrap();
    let n: u64 = row(&mut s, rid, "Baluster Count").as_str().unwrap().parse().unwrap();
    assert!(n > 50, "balusters along the ramp at the pattern spacing: {n}");
    // plan: whole and UP, its slope written on each run
    let plan = call(&mut s, "plan", json!({ "view_id": p1 }));
    let rp = plan["ramps"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap().clone();
    assert_eq!(rp["label"], "UP");
    assert_eq!(rp["slopes"].as_array().unwrap().len(), 2);
    assert!(rp["lines"].as_array().unwrap().iter().all(|l| l["above"] == false));
    // 3D and a section across the first run
    let scene = call(&mut s, "scene", json!({}));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|m| m["id"] == id && m["kind"] == "Ramp"));
    let view = call(&mut s, "add_section", json!({ "level_id": l1, "start": [2000, -3000], "end": [2000, 3000] }))["view_id"].as_u64().unwrap();
    let items = call(&mut s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().clone();
    assert!(items.iter().any(|i| i["id"] == id && i["category"] == "Ramps" && i["cut"] == true));
    // IFC
    let path = std::env::temp_dir().join(format!("openarch-ramps-{}.ifc", std::process::id()));
    call(&mut s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains(".TWO_STRAIGHT_RUN_RAMP."));
    assert_eq!(text.matches("=IFCRAMPFLIGHT(").count(), 2);
    assert!(text.contains(".LANDING.") && text.contains("'Pset_RampCommon'") && text.contains(".GUARDRAIL."));
    // steeper: a taller ramp over the same runs warns; Type Properties
    call(&mut s, "patch_ramp", json!({ "id": id, "patch": { "unconnected_height_mm": 800 } }));
    let p = call(&mut s, "describe", json!({ "id": id }));
    assert!(p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("steeper than 1:14")), "{}", p["warnings"]);
    let t = call(&mut s, "describe_any_type", json!({ "id": ty }));
    assert_eq!(t["patch"], "patch_ramp_type");
    call(&mut s, "patch_ramp_type", json!({ "id": ty, "patch": { "max_slope_ratio": 10 } }));
    let p = call(&mut s, "describe", json!({ "id": id }));
    assert!(p["warnings"].as_array().unwrap().is_empty(), "1:10.5 is within 1:10: {}", p["warnings"]);
    // a ramp's railings go with it
    call(&mut s, "delete", json!({ "ids": [id] }));
    assert!(s.handle("describe", &json!({ "id": rid })).is_err());
}
