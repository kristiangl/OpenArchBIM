//! Railings: on a new stair, Place on Stair, Sketch Path and Edit Path, plans, 3D, sections, Properties and Type
//! Properties, delete and copy with the stair, IFC.
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

fn ty(m: &M, list: &str, name: &str) -> u64 {
    m.info[list].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("no {name}"))["id"].as_u64().unwrap()
}

fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))["value"].clone()
}

/// A straight stair of all its risers, with a balustrade each side.
fn stair(m: &mut M) -> (u64, Vec<u64>) {
    let base = json!({ "type_id": ty(m, "stair_types", "Concrete Stair"), "base_level": m.l1, "top_level": m.l2 });
    let mut p = base.clone();
    p["pending"] = json!({ "start": [0, 0], "end": [0, 20000], "justification": "center", "width_mm": 1100 });
    let run = call(&mut m.s, "stair_draft", p)["pending"].clone();
    let mut p = base;
    p["runs"] = json!([run]);
    p["railing_type"] = json!(ty(m, "railing_types", "Balustrade 1000"));
    let r = call(&mut m.s, "add_stair", p);
    (r["id"].as_u64().unwrap(), r["railings"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect())
}

#[test]
fn a_new_stair_gets_a_balustrade_each_side_drawn_in_plan_3d_sections_and_ifc() {
    let mut m = model();
    let (st, rails) = stair(&mut m);
    assert_eq!(rails.len(), 2);
    let n: u64 = row(&mut m.s, rails[0], "Baluster Count").as_str().unwrap().parse().unwrap();
    assert!(n >= 2 * 14, "two balusters a tread: {n}");
    assert_eq!(row(&mut m.s, rails[0], "Host"), "Stair");
    assert_eq!(row(&mut m.s, rails[0], "Maximum Opening"), "Met");
    // the lower plan: solid where the rail is below the cut, dashed above it
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p1 }));
    let r = plan["railings"].as_array().unwrap().iter().find(|r| r["id"] == rails[0]).unwrap().clone();
    let polys = r["polys"].as_array().unwrap();
    assert!(polys.iter().any(|p| p["above"] == true) && polys.iter().any(|p| p["above"] == false));
    // 3D in aluminium
    let scene = call(&mut m.s, "scene", json!({}));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|x| x["id"] == rails[0] && x["kind"] == "Railing" && x["material"] == "Aluminium"));
    // a section across the flight cuts the balustrade too
    let view = call(&mut m.s, "add_section", json!({ "level_id": m.l1, "start": [-2000, 1000], "end": [2000, 1000] }))["view_id"].as_u64().unwrap();
    let items = call(&mut m.s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().clone();
    assert!(items.iter().any(|i| i["category"] == "Railings" && i["cut"] == true));
    // IFC: guardrails with their handrails and members, parts of the stair
    let path = std::env::temp_dir().join(format!("openarch-railings-{}.ifc", std::process::id()));
    call(&mut m.s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(text.matches(".GUARDRAIL.)").count(), 2);
    assert_eq!(text.matches(".HANDRAIL.)").count(), 2);
    assert!(text.matches("=IFCMEMBER(").count() >= 2 * n as usize);
    assert!(text.contains("'Pset_RailingCommon'"));
    // deleting the stair takes its railings; undo brings all back
    call(&mut m.s, "delete", json!({ "ids": [st] }));
    assert!(m.s.handle("describe", &json!({ "id": rails[0] })).is_err());
    call(&mut m.s, "undo", json!({}));
    assert_eq!(row(&mut m.s, rails[1], "Host"), "Stair");
}

#[test]
fn sketched_railings_and_copies_of_stairs_with_theirs() {
    let mut m = model();
    let rt = ty(&m, "railing_types", "Balustrade 1000");
    let line = |a: [f64; 2], b: [f64; 2]| json!({ "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] } });
    let curves = json!([line([0.0, 5000.0], [4000.0, 5000.0]), line([4000.0, 5000.0], [4000.0, 7000.0])]);
    let id = call(&mut m.s, "add_railing", json!({ "type_id": rt, "level_id": m.l1, "curves": curves }))["id"].as_u64().unwrap();
    assert_eq!(row(&mut m.s, id, "Length"), "6000");
    assert_eq!(row(&mut m.s, id, "Post Count"), "3");
    // Edit Path: one line only
    let back = call(&mut m.s, "railing_path", json!({ "id": id }));
    let one = json!([back["curves"][0]["curve"].clone()]);
    call(&mut m.s, "set_railing_path", json!({ "id": id, "curves": one }));
    assert_eq!(row(&mut m.s, id, "Length"), "4000");
    // Type Properties: no balusters, then the openings break the rule... with no infill there is nothing to check
    let t = call(&mut m.s, "describe_any_type", json!({ "id": rt }));
    assert_eq!(t["patch"], "patch_railing_type");
    call(&mut m.s, "patch_railing_type", json!({ "id": rt, "patch": { "balusters": { "spacing_mm": 300, "justification": "beginning" } } }));
    let p = call(&mut m.s, "describe", json!({ "id": id }));
    assert!(p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("opening")), "{}", p["warnings"]);
    call(&mut m.s, "patch_railing_type", json!({ "id": rt, "patch": { "balusters": null } }));
    assert_eq!(row(&mut m.s, id, "Baluster Count"), "0");
    call(&mut m.s, "undo", json!({}));
    call(&mut m.s, "undo", json!({}));
    // copy a stair and paste it: its railings come along on the new stair
    let (st, _) = stair(&mut m);
    call(&mut m.s, "copy", json!({ "ids": [st], "view_id": m.p1 }));
    let ids: Vec<u64> = call(&mut m.s, "paste", json!({ "mode": "point", "view_id": m.p1, "point": [8000, 3000] }))["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 3, "the stair and its two railings");
    let new_stair = ids.iter().copied().find(|i| call(&mut m.s, "describe", json!({ "id": i }))["kind"] == "Stair").unwrap();
    let plan = call(&mut m.s, "plan", json!({ "view_id": m.p1 }));
    let on_new = plan["railings"].as_array().unwrap().iter().filter(|r| ids.contains(&r["id"].as_u64().unwrap())).count();
    assert_eq!(on_new, 2);
    assert!(new_stair != st);
}
