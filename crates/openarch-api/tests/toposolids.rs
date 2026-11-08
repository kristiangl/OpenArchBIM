//! Toposolids: Create from Sketch and from a points file, Add Point, contours in plans, Properties, types, sections,
//! elevations, 3D, IFC, moving, and saving.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn row(s: &mut Session, id: u64, label: &str) -> Value {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap_or_else(|| panic!("no {label}"))
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([[x0, y0, x1, y0], [x1, y0, x1, y1], [x1, y1, x0, y1], [x0, y1, x0, y0]].iter().map(|l| json!({ "type": "line", "a": { "x": l[0], "y": l[1] }, "b": { "x": l[2], "y": l[3] } })).collect::<Vec<_>>())
}

#[test]
fn a_sketched_toposolid_takes_points_and_shows_everywhere() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let ty = info["toposolid_types"][0]["id"].as_u64().expect("new projects have a toposolid type");
    assert_eq!(info["toposolid_types"][0]["name"], "Site - Earth 1000");
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let lz = info["levels"][0]["elevation_mm"].as_f64().unwrap_or(0.0);
    let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
    let id = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "offset_mm": -300, "curves": square(-10000.0, -10000.0, 10000.0, 10000.0) }))["id"].as_u64().unwrap();
    assert_eq!(row(&mut s, id, "Area")["raw"], 400.0);
    assert_eq!(row(&mut s, id, "Thickness")["raw"], 1000.0);
    // flat: no contours yet unless the top sits on one
    call(&mut s, "add_toposolid_point", json!({ "id": id, "point": [0, 0], "elevation": 2700, "base": "current_level" }));
    call(&mut s, "add_toposolid_point", json!({ "id": id, "point": [5000, 5000], "elevation": 400, "mode": "surface" }));
    assert_eq!(row(&mut s, id, "Points")["raw"], 2.0);
    assert_eq!(row(&mut s, id, "Elevation at Top")["value"], "Varies");
    assert!(s.handle("add_toposolid_point", &json!({ "id": id, "point": [20000, 0], "elevation": 0 })).is_err());
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let topo = &plan["toposolids"][0];
    assert_eq!(topo["id"], id);
    assert_eq!(topo["points"].as_array().unwrap().len(), 2);
    // from -300 up to 2700: lines at 0, 500 ... 2500, with 0 and 2500 primary (the survey point is at 0)
    let contours = topo["contours"].as_array().unwrap();
    assert_eq!(contours.len(), 6, "{contours:?}");
    assert_eq!(contours.iter().filter(|c| c["primary"] == true).count(), 2);
    // Type Properties: contour intervals
    let t = call(&mut s, "describe_any_type", json!({ "id": ty }));
    assert_eq!(t["family"], "Toposolid");
    call(&mut s, "patch_system_type", json!({ "id": ty, "patch": { "secondary_interval": 0 } }));
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    assert_eq!(plan["toposolids"][0]["contours"].as_array().unwrap().len(), 2);
    assert!(s.handle("patch_system_type", &json!({ "id": ty, "patch": { "primary_interval": 5 } })).is_err());
    // 3D, a section through the mound (cut with the earth's layer) and an elevation (seen, with its outline)
    let scene = call(&mut s, "scene", json!({}));
    assert!(scene["meshes"].as_array().unwrap().iter().any(|m| m["id"] == id && m["kind"] == "Toposolid"));
    let view = call(&mut s, "add_section", json!({ "level_id": level, "start": [0, -15000], "end": [0, 15000] }))["view_id"].as_u64().unwrap();
    let items = call(&mut s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().clone();
    let cut: Vec<&Value> = items.iter().filter(|i| i["id"] == id && i["cut"] == true).collect();
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0]["material"], "Earth");
    let top = cut[0]["polys"][0].as_array().unwrap().iter().map(|p| p[1].as_f64().unwrap()).fold(f64::NEG_INFINITY, f64::max);
    assert!((top - (lz + 2700.0)).abs() < 0.01, "the cut reaches the top of the mound: {top}");
    let south = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "South").unwrap()["id"].as_u64().unwrap();
    let items = call(&mut s, "elevation", json!({ "view_id": south }))["items"].as_array().unwrap().clone();
    assert!(items.iter().any(|i| i["id"] == id && i["category"] == "Topography" && !i["polys"].as_array().unwrap().is_empty()));
    assert!(items.iter().any(|i| i["id"] == id && !i["lines"].as_array().unwrap().is_empty()));
    // IFC
    let path = std::env::temp_dir().join(format!("openarch-topo-{}.ifc", std::process::id()));
    call(&mut s, "export_ifc", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(text.contains("IFCGEOGRAPHICELEMENT(") && text.contains(".TERRAIN.)"));
    // Move carries the boundary and the points; save and reopen keep them
    call(&mut s, "transform", json!({ "ids": [id], "kind": "move", "dx": 1000, "dy": 0 }));
    let moved = call(&mut s, "plan", json!({ "view_id": site }))["toposolids"][0]["points"].clone();
    assert_eq!(moved[0][0], 1000.0);
    let file = std::env::temp_dir().join(format!("openarch-topo-{}.oab", std::process::id()));
    call(&mut s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    std::fs::remove_file(&file).unwrap();
    assert_eq!(call(&mut again, "plan", json!({ "view_id": site }))["toposolids"][0]["points"], moved);
}

#[test]
fn a_points_file_makes_a_toposolid_in_shared_coordinates() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let path = std::env::temp_dir().join(format!("openarch-points-{}.csv", std::process::id()));
    std::fs::write(&path, "Pt,E,N,RL\n1,0,0,10.0\n2,30,0,11.5\n3,30,20,12.0\n4,0,20,10.5\n5,15,10,13.25\n").unwrap();
    let r = call(&mut s, "import_toposolid", json!({ "type_id": ty, "level_id": level, "path": path, "scale": 1000, "base": "survey_point" }));
    std::fs::remove_file(&path).unwrap();
    assert_eq!(r["read"], 5);
    let id = r["id"].as_u64().unwrap();
    assert_eq!(row(&mut s, id, "Area")["raw"], 600.0);
    assert_eq!(row(&mut s, id, "Points")["raw"], 5.0);
    // nothing in a line, nothing at all
    let bad = std::env::temp_dir().join(format!("openarch-points-bad-{}.csv", std::process::id()));
    std::fs::write(&bad, "x,y,z\n0,0,0\n1,1,0\n2,2,0\n").unwrap();
    assert!(s.handle("import_toposolid", &json!({ "type_id": ty, "level_id": level, "path": bad })).is_err());
    std::fs::write(&bad, "nothing here\n").unwrap();
    assert!(s.handle("import_toposolid", &json!({ "type_id": ty, "level_id": level, "path": bad })).is_err());
    std::fs::remove_file(&bad).unwrap();
}

#[test]
fn points_are_edited_and_simplified_and_a_plan_cuts_high_ground() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
    let id = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "curves": square(0.0, 0.0, 10000.0, 10000.0) }))["id"].as_u64().unwrap();
    // Modify Sub Elements sends the whole list, heights above the unmodified top
    let mut pts = vec![];
    for i in 0..=10 { for j in 0..=10 { pts.push(json!([i as f64 * 1000.0, j as f64 * 1000.0, 0.0])); } }
    pts.push(json!([4500.0, 4500.0, 3000.0]));
    call(&mut s, "set_toposolid_points", json!({ "id": id, "points": pts }));
    assert_eq!(row(&mut s, id, "Points")["raw"], 122.0);
    assert!(s.handle("set_toposolid_points", &json!({ "id": id, "points": [[-500.0, 0.0, 0.0]] })).is_err());
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let t = &plan["toposolids"][0];
    assert_eq!(t["points"][121], json!([4500.0, 4500.0, 3000.0]));
    // the mound rises through the plan's cut plane (1200 above the level): the ground there is cut, in its style
    let cut = &t["cut"];
    assert!(!cut["polys"].as_array().unwrap().is_empty() && !cut["lines"].as_array().unwrap().is_empty(), "{cut}");
    assert!(cut["style"]["line"].is_object());
    // Simplify keeps the peak
    let r = call(&mut s, "simplify_toposolid", json!({ "id": id, "keep_percent": 20 }));
    assert!(r["kept"].as_u64().unwrap() <= 25, "{r}");
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    assert!(plan["toposolids"][0]["points"].as_array().unwrap().iter().any(|p| p[2] == 3000.0));
    assert!(s.handle("simplify_toposolid", &json!({ "id": id, "keep_percent": 0 })).is_err());
    // flat ground under the plane is not cut
    call(&mut s, "set_toposolid_points", json!({ "id": id, "points": [] }));
    assert!(call(&mut s, "plan", json!({ "view_id": site }))["toposolids"][0]["cut"].is_null());
}
