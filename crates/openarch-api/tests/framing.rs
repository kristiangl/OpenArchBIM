//! Beam systems, trusses and columns at grids through the API: placing them, what a plan and a 3D view draw,
//! Properties, Drop Truss and the IFC export.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn rows(v: &Value) -> Vec<Value> {
    v["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect()
}

#[test]
fn a_beam_system_a_truss_and_columns_at_grids() {
    let mut s = Session::new();
    call(&mut s, "ensure_structural_types", json!({}));
    let info = call(&mut s, "doc_info", json!({}));
    let (l0, l1) = (info["levels"][0]["id"].as_u64().unwrap(), info["levels"][1]["id"].as_u64().unwrap());
    let types = info["structural_types"].as_array().unwrap();
    let ty = |name: &str| types.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("no type {name}"))["id"].as_u64().unwrap();
    let upper = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() == Some(l1)).unwrap()["id"].as_u64().unwrap();
    let structure = |s: &mut Session, view: u64| call(s, "plan", json!({ "view_id": view }))["structure"].as_array().unwrap().clone();

    // a system of joists every 600 across a 6 m by 4 m bay
    let sys = call(&mut s, "add_beam_system", json!({ "type_id": ty("200UB25.4"), "level_id": l1, "boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "direction_deg": 90, "rule": "fixed_distance", "spacing_mm": 600 }))["id"].as_u64().unwrap();
    let beams: Vec<Value> = structure(&mut s, upper).into_iter().filter(|x| x["id"].as_u64() == Some(sys)).collect();
    assert_eq!(beams.len(), 9);
    assert!(beams[0]["boundary"].is_array() && beams[1]["boundary"].is_null() && beams[0]["group"] == "system");
    let d = call(&mut s, "describe", json!({ "id": sys }));
    assert_eq!((d["category"].as_str(), rows(&d).iter().find(|x| x["label"] == "Number of Beams").unwrap()["raw"].as_f64()), (Some("Structural Framing"), Some(9.0)));
    call(&mut s, "patch_structural", json!({ "id": sys, "patch": { "rule": "fixed_number", "count": 4 } }));
    assert_eq!(structure(&mut s, upper).iter().filter(|x| x["id"].as_u64() == Some(sys)).count(), 4);
    assert!(rows(&call(&mut s, "describe", json!({ "id": sys }))).iter().any(|x| x["label"] == "Number of Lines"));
    let scene = call(&mut s, "scene", json!({}));
    assert_eq!(scene["meshes"].as_array().unwrap().iter().filter(|m| m["id"].as_u64() == Some(sys)).count(), 4);

    // a truss: one outline in plan, its members in 3D, then dropped to beams
    let truss = call(&mut s, "add_truss", json!({ "level_id": l1, "start": [0, 8000], "end": [9000, 8000], "height_mm": 1500, "panels": 6, "top_chord_id": ty("150 x 50 x 4.0 RHS"), "bottom_chord_id": ty("150 x 50 x 4.0 RHS"), "web_type_id": ty("150 x 50 x 4.0 RHS") }))["id"].as_u64().unwrap();
    assert_eq!(structure(&mut s, upper).iter().filter(|x| x["id"].as_u64() == Some(truss)).count(), 1);
    let d = call(&mut s, "describe", json!({ "id": truss }));
    assert_eq!(rows(&d).iter().find(|x| x["label"] == "Number of Members").unwrap()["raw"].as_f64(), Some(15.0));
    call(&mut s, "transform", json!({ "kind": "move", "ids": [truss], "dx": 0.0, "dy": 1000.0 }));
    assert_eq!(structure(&mut s, upper).iter().find(|x| x["id"].as_u64() == Some(truss)).unwrap()["start"], json!([0.0, 9000.0]));
    let path = std::env::temp_dir().join(format!("openarch-framing-{}.ifc", std::process::id()));
    call(&mut s, "export_ifc", json!({ "path": path.to_string_lossy() }));
    let f = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!((f.matches("=IFCBEAM(").count(), f.matches("=IFCMEMBER(").count()), (1, 1));
    let dropped = call(&mut s, "drop_truss", json!({ "id": truss }))["ids"].as_array().unwrap().len();
    assert_eq!(dropped, 15);
    assert!(s.handle("describe", &json!({ "id": truss })).is_err());
    call(&mut s, "undo", json!({}));

    // columns where two grids cross two others
    for (a, b) in [([0, -1000], [0, 5000]), ([6000, -1000], [6000, 5000]), ([-1000, 0], [7000, 0]), ([-1000, 4000], [7000, 4000])] {
        call(&mut s, "add_grid", json!({ "curves": [{ "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] } }] }));
    }
    let made = call(&mut s, "columns_at_grids", json!({ "type_id": ty("200UC46.2"), "level_id": l0, "top_level_id": l1 }))["ids"].as_array().unwrap().len();
    assert_eq!(made, 4);
    call(&mut s, "undo", json!({}));
    assert!(call(&mut s, "plan", json!({ "view_id": upper }))["structure"].as_array().unwrap().iter().all(|x| x["kind"] != "column"));
}
