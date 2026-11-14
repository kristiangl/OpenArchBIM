//! Structural columns and beams through the API: placing them, what a plan, a 3D view and an elevation draw,
//! Properties and types, Move, schedules and the IFC export.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn rows(v: &Value) -> Vec<Value> {
    v["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect()
}

#[test]
fn a_column_and_a_beam_are_placed_drawn_described_and_exported() {
    let mut s = Session::new();
    call(&mut s, "ensure_structural_types", json!({}));
    let info = call(&mut s, "doc_info", json!({}));
    let (l0, l1) = (info["levels"][0]["id"].as_u64().unwrap(), info["levels"][1]["id"].as_u64().unwrap());
    let rise = info["levels"][1]["elevation_mm"].as_f64().unwrap() - info["levels"][0]["elevation_mm"].as_f64().unwrap();
    let types = info["structural_types"].as_array().unwrap();
    let ty = |name: &str| types.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("no type {name}"))["id"].as_u64().unwrap();
    let plan_of = |level: u64| info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() == Some(level)).unwrap()["id"].as_u64().unwrap();
    let (ground, upper) = (plan_of(l0), plan_of(l1));

    let col = call(&mut s, "add_structural_column", json!({ "type_id": ty("200UC46.2"), "level_id": l0, "at": [0, 0], "top_level_id": l1 }))["id"].as_u64().unwrap();
    let beam = call(&mut s, "add_beam", json!({ "type_id": ty("310UB40.4"), "level_id": l1, "start": [0, 0], "end": [6000, 0], "usage": "girder" }))["id"].as_u64().unwrap();
    assert!(s.handle("add_structural_column", &json!({ "type_id": ty("310UB40.4"), "level_id": l0, "at": [0, 0] })).is_err(), "a beam section is not a column");

    // the ground plan cuts the column: its I outline of twelve corners; the beam is up at the next level, out of range
    let structure = |s: &mut Session, view: u64| call(s, "plan", json!({ "view_id": view }))["structure"].as_array().unwrap().clone();
    let g = structure(&mut s, ground);
    let c = g.iter().find(|x| x["id"].as_u64() == Some(col)).expect("the column in the ground plan");
    assert_eq!((c["kind"].as_str(), c["display"].as_str(), c["outline"].as_array().unwrap().len()), (Some("column"), Some("cut"), 12));
    assert!(!g.iter().any(|x| x["id"].as_u64() == Some(beam)));
    // the upper plan shows the beam that hangs under its level: 165 wide, 6 m long
    let u = structure(&mut s, upper);
    let b = u.iter().find(|x| x["id"].as_u64() == Some(beam)).expect("the beam in its own level's plan");
    let ys: Vec<f64> = b["outline"].as_array().unwrap().iter().map(|p| p[1].as_f64().unwrap()).collect();
    assert!((ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 82.5).abs() < 1e-6, "{ys:?}");

    // 3D: both solids; an elevation: both, under their categories
    let scene = call(&mut s, "scene", json!({}));
    let kinds: Vec<&str> = scene["meshes"].as_array().unwrap().iter().filter(|m| [Some(col), Some(beam)].contains(&m["id"].as_u64())).filter_map(|m| m["kind"].as_str()).collect();
    assert!(kinds.contains(&"StructuralColumn") && kinds.contains(&"Beam"), "{kinds:?}");
    let south = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "elevation" && v["name"] == "South").unwrap()["id"].as_u64().unwrap();
    let items = call(&mut s, "elevation", json!({ "view_id": south }))["items"].as_array().unwrap().clone();
    assert!(items.iter().any(|i| i["id"].as_u64() == Some(col) && i["category"] == "Structural Columns"));
    assert!(items.iter().any(|i| i["id"].as_u64() == Some(beam) && i["category"] == "Structural Framing"));

    // Properties: length from level to level, the type by name; a type change and an offset through the palette's patches
    let d = call(&mut s, "describe", json!({ "id": col }));
    let r = rows(&d);
    assert_eq!(d["category"], "Structural Columns");
    assert_eq!(r.iter().find(|x| x["label"] == "Length").unwrap()["raw"].as_f64(), Some(rise));
    assert_eq!(r.iter().find(|x| x["label"] == "Type").unwrap()["value"], "Universal Column: 200UC46.2");
    call(&mut s, "patch_structural", json!({ "id": col, "patch": { "structural_type": ty("300 x 300"), "base_offset_mm": -300 } }));
    let d = call(&mut s, "describe", json!({ "id": col }));
    assert_eq!(rows(&d).iter().find(|x| x["label"] == "Length").unwrap()["raw"].as_f64(), Some(rise + 300.0));
    assert_eq!(structure(&mut s, ground).iter().find(|x| x["id"].as_u64() == Some(col)).unwrap()["outline"].as_array().unwrap().len(), 4);
    let bd = call(&mut s, "describe", json!({ "id": beam }));
    assert_eq!(rows(&bd).iter().find(|x| x["label"] == "Structural Usage").unwrap()["value"], "Girder");
    // the type's own properties, and a size changed there reaches the member
    let t = call(&mut s, "describe_any_type", json!({ "id": ty("300 x 300") }));
    assert_eq!(rows(&t).iter().find(|x| x["label"] == "Width").unwrap()["raw"].as_f64(), Some(300.0));
    call(&mut s, "patch_structural", json!({ "id": ty("300 x 300"), "patch": { "section": { "b": 400.0 } } }));
    let xs: Vec<f64> = structure(&mut s, ground).iter().find(|x| x["id"].as_u64() == Some(col)).unwrap()["outline"].as_array().unwrap().iter().map(|p| p[0].as_f64().unwrap()).collect();
    assert!((xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 200.0).abs() < 1e-6, "{xs:?}");

    // Move carries both; undo brings them back
    call(&mut s, "transform", json!({ "kind": "move", "ids": [col, beam], "dx": 1000.0, "dy": 500.0 }));
    assert_eq!(structure(&mut s, ground).iter().find(|x| x["id"].as_u64() == Some(col)).unwrap()["at"], json!([1000.0, 500.0]));
    assert_eq!(structure(&mut s, upper).iter().find(|x| x["id"].as_u64() == Some(beam)).unwrap()["end"], json!([7000.0, 500.0]));
    call(&mut s, "undo", json!({}));

    // IFC: a column and a beam, and the file opens again
    let path = std::env::temp_dir().join(format!("openarch-structural-{}.ifc", std::process::id()));
    call(&mut s, "export_ifc", json!({ "path": path.to_string_lossy() }));
    let f = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!((f.matches("=IFCCOLUMN(").count(), f.matches("=IFCBEAM(").count()), (1, 1));
    assert!(f.contains(".COLUMN.") && f.contains(".BEAM."));

    // the beam's start is on the column: cut back to its face (200 after the type's width became 400) and 10 more
    let d = call(&mut s, "describe", json!({ "id": beam }));
    assert_eq!(rows(&d).iter().find(|x| x["label"] == "Cut Length").unwrap()["raw"].as_f64(), Some(6000.0 - 210.0));
    let b = structure(&mut s, upper).iter().find(|x| x["id"].as_u64() == Some(beam)).unwrap().clone();
    let x0 = b["outline"].as_array().unwrap().iter().map(|p| p[0].as_f64().unwrap()).fold(f64::INFINITY, f64::min);
    assert!((x0 - 210.0).abs() < 1e-6, "{x0}");
    call(&mut s, "patch_structural", json!({ "id": beam, "patch": { "start_join": false } }));
    assert_eq!(rows(&call(&mut s, "describe", json!({ "id": beam }))).iter().find(|x| x["label"] == "Cut Length").unwrap()["raw"].as_f64(), Some(6000.0));
    call(&mut s, "undo", json!({}));
    // slanted through Properties, and a slanted one placed: the plan gives its line
    call(&mut s, "patch_structural", json!({ "id": col, "patch": { "top_at": { "x": 800.0, "y": 0.0 } } }));
    let c = structure(&mut s, ground).iter().find(|x| x["id"].as_u64() == Some(col)).unwrap().clone();
    assert_eq!(c["axis"], json!([[0.0, 0.0], [800.0, 0.0]]));
    assert!(rows(&call(&mut s, "describe", json!({ "id": col }))).iter().any(|x| x["label"] == "Top Attached To"));
    call(&mut s, "undo", json!({}));

    // deleted and undone
    call(&mut s, "delete", json!({ "ids": [col, beam] }));
    assert!(structure(&mut s, ground).is_empty());
    call(&mut s, "undo", json!({}));
    assert_eq!(structure(&mut s, ground).len(), 1);
}
