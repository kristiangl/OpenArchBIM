//! Traffic: line marking, parking bays and kerbs on the ground, in plans, 3D and Properties.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}
fn line(a: [f64; 2], b: [f64; 2]) -> Value { json!({ "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] } }) }

#[test]
fn parking_bays_markings_and_kerbs_sit_on_the_ground_and_show_everywhere() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
    let sq = [[-5000.0, -5000.0], [30000.0, -5000.0], [30000.0, 15000.0], [-5000.0, 15000.0]];
    call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "offset_mm": 250, "curves": (0..4).map(|i| line(sq[i], sq[(i + 1) % 4])).collect::<Vec<_>>() }));
    let row = call(&mut s, "add_parking_row", json!({ "level_id": level, "start": [0, 0], "end": [25000, 0], "wheel_stops": true }))["id"].as_u64().unwrap();
    let mark = call(&mut s, "add_line_marking", json!({ "level_id": level, "curves": [line([0.0, 12000.0], [25000.0, 12000.0])] }))["id"].as_u64().unwrap();
    let kerb = call(&mut s, "add_kerb", json!({ "level_id": level, "curves": [line([0.0, -1000.0], [25000.0, -1000.0])] }))["id"].as_u64().unwrap();
    let d = call(&mut s, "describe", json!({ "id": row }));
    let rows: Vec<Value> = d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect();
    assert_eq!(rows.iter().find(|r| r["label"] == "Bays").unwrap()["value"], "10");
    assert_eq!(rows.iter().find(|r| r["label"] == "On").unwrap()["value"], "Toposolid");
    call(&mut s, "patch_traffic", json!({ "id": mark, "patch": { "dashed": true, "paint": "yellow" } }));
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let t = plan["traffic"].as_array().unwrap();
    assert_eq!(t.len(), 3);
    assert_eq!(t.iter().find(|x| x["id"] == row).unwrap()["stops"].as_array().unwrap().len(), 10);
    assert!(t.iter().find(|x| x["id"] == mark).unwrap()["lines"].as_array().unwrap().len() > 5, "dashes");
    let three = call(&mut s, "scene", json!({}));
    let kinds: Vec<&str> = three["meshes"].as_array().unwrap().iter().filter_map(|m| m["kind"].as_str()).collect();
    for k in ["ParkingRow", "WheelStop", "LineMarking", "Kerb"] { assert!(kinds.contains(&k), "{k} in 3D: {kinds:?}"); }
    // the kerb stands on the toposolid: its lowest corner at the ground's top
    let kv = three["meshes"].as_array().unwrap().iter().find(|m| m["id"] == kerb).unwrap()["verts"].as_array().unwrap().chunks(6).map(|c| c[2].as_f64().unwrap()).fold(f64::MAX, f64::min);
    assert!((kv * 1000.0 - 250.0).abs() < 1.0, "{kv}");
    let file = std::env::temp_dir().join(format!("openarch-traffic-{}.oab", std::process::id()));
    call(&mut s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    let _ = std::fs::remove_file(&file);
    assert_eq!(call(&mut again, "plan", json!({ "view_id": site }))["traffic"].as_array().unwrap().len(), 3);
}
