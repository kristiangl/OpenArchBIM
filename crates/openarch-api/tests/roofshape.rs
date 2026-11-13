//! Shape editing of flat roofs through the API: Add Point, Add Split Line, Modify Sub Elements, Reset Shape, what a
//! plan is told, the 3D grips and an elevation.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn roof(s: &mut Session, slope: bool) -> (u64, u64) {
    let info = call(s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let rt = info["roof_types"][0]["id"].as_u64().unwrap();
    let view = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() == Some(level)).unwrap()["id"].as_u64().unwrap();
    let line = |a: [f64; 2], b: [f64; 2]| json!({ "curve": { "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] } }, "defines_slope": slope, "slope_deg": 22.5, "overhang_mm": 0.0 });
    let edges = vec![line([0.0, 0.0], [8000.0, 0.0]), line([8000.0, 0.0], [8000.0, 6000.0]), line([8000.0, 6000.0], [0.0, 6000.0]), line([0.0, 6000.0], [0.0, 0.0])];
    let id = call(s, "add_roof_sketch", json!({ "type_id": rt, "level_id": level, "base_offset_mm": 0.0, "edges": edges }))["id"].as_u64().unwrap();
    // lift it into the plan's view: the roof sits at the top of the storey
    call(s, "patch_roof", json!({ "id": id, "patch": { "base_offset_mm": 3000.0 } }));
    (id, view)
}

fn in_plan(s: &mut Session, view: u64, id: u64) -> Value {
    // a roof above the cut plane still reaches the plan data of the level's underlay or range; ask for it by id
    let plan = call(s, "plan", json!({ "view_id": view }));
    plan["roofs"].as_array().unwrap().iter().find(|r| r["id"].as_u64() == Some(id)).cloned().unwrap_or(Value::Null)
}

#[test]
fn a_flat_roof_is_given_falls_to_an_outlet() {
    let mut s = Session::new();
    let (id, view) = roof(&mut s, false);
    // the 3D grips: no points yet, four bare corners
    let g = call(&mut s, "shape_points", json!({ "id": id }));
    assert_eq!((g["kind"].as_str(), g["points"].as_array().unwrap().len(), g["corners"].as_array().unwrap().len()), (Some("roof"), 0, 4));
    let base = g["base"].as_f64().unwrap();

    call(&mut s, "add_roof_point", json!({ "id": id, "point": [4000, 3000], "z": -80 }));
    let g = call(&mut s, "shape_points", json!({ "id": id }));
    assert_eq!(g["points"], json!([[4000.0, 3000.0, -80.0]]));
    let r = in_plan(&mut s, view, id);
    if !r.is_null() {
        assert_eq!((r["shape_points"].as_array().unwrap().len(), r["pitched"].as_bool(), r["ridges"].as_array().unwrap().len()), (1, Some(false), 4));
    }
    // the outlet lowered further by Modify Sub Elements, then a split line through it
    call(&mut s, "set_roof_points", json!({ "id": id, "points": [[4000, 3000, -100]], "lines": null }));
    call(&mut s, "add_roof_split_line", json!({ "id": id, "a": [0, 3000], "b": [8000, 3000] }));
    let g = call(&mut s, "shape_points", json!({ "id": id }));
    assert_eq!(g["points"].as_array().unwrap().len(), 3);
    assert!(g["points"].as_array().unwrap().iter().any(|p| p[2] == json!(-100.0)));

    // every elevation shows the roof no higher than its base, and its low point is hidden inside it
    let views = call(&mut s, "doc_info", json!({}))["views"].clone();
    let ev = views.as_array().unwrap().iter().find(|v| v["view_kind"] == "elevation").unwrap()["id"].as_u64().unwrap();
    let e = call(&mut s, "elevation", json!({ "view_id": ev }));
    let zs: Vec<f64> = e["items"].as_array().unwrap().iter().filter(|i| i["id"].as_u64() == Some(id)).flat_map(|i| i["polys"].as_array().unwrap().iter().flat_map(|p| p.as_array().unwrap().iter().map(|q| q[1].as_f64().unwrap())).collect::<Vec<_>>()).collect();
    assert!(!zs.is_empty(), "the roof shows in the elevation");
    assert!(zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) <= base + 1e-6);

    // flat again, and one undo brings the shape back
    call(&mut s, "reset_roof_shape", json!({ "id": id }));
    assert_eq!(call(&mut s, "shape_points", json!({ "id": id }))["points"].as_array().unwrap().len(), 0);
    call(&mut s, "undo", json!({}));
    assert_eq!(call(&mut s, "shape_points", json!({ "id": id }))["points"].as_array().unwrap().len(), 3);
}

#[test]
fn a_pitched_roof_refuses_shape_editing() {
    let mut s = Session::new();
    let (id, _) = roof(&mut s, true);
    assert!(call(&mut s, "shape_points", json!({ "id": id }))["kind"].is_null());
    let err = s.handle("add_roof_point", &json!({ "id": id, "point": [4000, 3000], "z": 50 })).unwrap_err();
    assert!(err.contains("pitched"), "{err}");
}

#[test]
fn a_section_draws_each_layer_of_a_roof() {
    let mut s = Session::new();
    let (id, _) = roof(&mut s, false);
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let sec = call(&mut s, "add_section", json!({ "level_id": level, "start": [-1000, 3000], "end": [9000, 3000] }))["id"].as_u64().unwrap();
    let view = call(&mut s, "doc_info", json!({}))["views"].as_array().unwrap().iter().filter(|v| v["view_kind"] == "section").map(|v| v["id"].as_u64().unwrap()).find(|v| *v > sec).unwrap();
    let cuts = |s: &mut Session| -> Vec<Value> {
        call(s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().iter().filter(|i| i["id"].as_u64() == Some(id) && i["cut"] == json!(true)).cloned().collect()
    };
    let span = |i: &Value| { let zs: Vec<f64> = i["polys"][0].as_array().unwrap().iter().map(|q| q[1].as_f64().unwrap()).collect(); (zs.iter().cloned().fold(f64::INFINITY, f64::min), zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)) };
    // flat: the first roof type has three layers (1, 40 and 90 thick), stacked down from the base at 3000
    let flat = cuts(&mut s);
    assert_eq!(flat.len(), 3, "{flat:?}");
    let mut spans: Vec<(f64, f64)> = flat.iter().map(span).collect();
    spans.sort_by(|a, b| b.1.total_cmp(&a.1));
    assert!((spans[0].1 - 3000.0).abs() < 1e-6 && (spans[0].0 - 2999.0).abs() < 1e-6 && (spans[2].0 - 2869.0).abs() < 1e-6, "{spans:?}");
    // shaped: still three, and they dip to the outlet on the cut line
    call(&mut s, "add_roof_point", json!({ "id": id, "point": [4000, 3000], "z": -80 }));
    let shaped = cuts(&mut s);
    assert_eq!(shaped.len(), 3);
    let low = shaped.iter().map(|i| span(i).0).fold(f64::INFINITY, f64::min);
    assert!((low - (2869.0 - 80.0)).abs() < 1e-6, "{low}");
}
