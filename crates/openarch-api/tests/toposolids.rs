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

#[test]
fn split_lines_fold_the_ground_spots_read_it_and_contours_are_labelled() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let lz = info["levels"][0]["elevation_mm"].as_f64().unwrap_or(0.0);
    let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
    let id = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "curves": square(0.0, 0.0, 10000.0, 10000.0) }))["id"].as_u64().unwrap();
    // a split line across the middle, then its ends raised: a ridge
    call(&mut s, "add_toposolid_split_line", json!({ "id": id, "a": [0, 5000], "b": [10000, 5000] }));
    let t = call(&mut s, "plan", json!({ "view_id": site }))["toposolids"][0].clone();
    assert_eq!(t["points"].as_array().unwrap().len(), 2);
    assert_eq!(t["lines"], json!([[0, 1]]));
    call(&mut s, "set_toposolid_points", json!({ "id": id, "points": [[0, 5000, 2000], [10000, 5000, 2000]], "lines": [[0, 1]] }));
    // a second line across the first: both are broken where they cross, at a new point on the ridge
    call(&mut s, "add_toposolid_split_line", json!({ "id": id, "a": [5000, 0], "b": [5000, 10000] }));
    let t = call(&mut s, "plan", json!({ "view_id": site }))["toposolids"][0].clone();
    assert_eq!(t["points"].as_array().unwrap().len(), 5, "{t}");
    assert_eq!(t["lines"].as_array().unwrap().len(), 4);
    assert!(s.handle("add_toposolid_split_line", &json!({ "id": id, "a": [-100, 0], "b": [5000, 5000] })).is_err());
    // a spot on the ridge reads its height; one on the slope reads the slope (2000 over 5000)
    let spot = call(&mut s, "add_spot", json!({ "view_id": site, "kind": "elevation", "target": [2000, 5000], "host": id, "surface": "top", "shoulder": [500, 400] }))["id"].as_u64().unwrap();
    let slope = call(&mut s, "add_spot", json!({ "view_id": site, "kind": "slope", "target": [2000, 2500], "host": id, "surface": "top", "shoulder": [500, 400] }))["id"].as_u64().unwrap();
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let find = |sid: u64| plan["spots"].as_array().unwrap().iter().find(|x| x["id"].as_u64() == Some(sid)).unwrap().clone();
    let e = find(spot);
    assert!((e["value"].as_f64().unwrap() - (lz + 2000.0)).abs() < 0.5, "{e}");
    assert!(find(slope)["value"].as_f64().map_or(false, |g| (g - 0.4).abs() < 1e-6), "{}", find(slope));
    // undo (past the spots) takes the second line away again, with the point where they crossed
    let lines = |s: &mut Session| call(s, "plan", json!({ "view_id": site }))["toposolids"][0]["lines"].as_array().unwrap().len();
    for _ in 0..6 { if lines(&mut s) == 1 { break; } call(&mut s, "undo", json!({})); }
    assert_eq!(lines(&mut s), 1);
    assert_eq!(call(&mut s, "plan", json!({ "view_id": site }))["toposolids"][0]["points"].as_array().unwrap().len(), 2);
    // Label Contours: a line up the slope labels the contours it crosses, in metres
    let label = call(&mut s, "add_contour_label", json!({ "view_id": site, "a": [3000, 100], "b": [3000, 4900] }))["id"].as_u64().unwrap();
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let texts: Vec<String> = plan["contour_labels"][0]["labels"].as_array().unwrap().iter().map(|l| l["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(texts, vec!["0.5", "1.0", "1.5"], "{plan}");
    assert_eq!(call(&mut s, "describe", json!({ "id": label }))["kind"], "ContourLabel");
    call(&mut s, "patch_contour_label", json!({ "id": label, "patch": { "decimals": 2, "primary_only": false } }));
    let first = call(&mut s, "plan", json!({ "view_id": site }))["contour_labels"][0]["labels"][0].clone();
    assert_eq!(first["text"], "0.50");
    // contours run east-west there, so the text does too
    assert!(first["angle"].as_f64().unwrap().abs() < 1e-6, "{first}");
    // it moves, and goes with its view's other annotations when the toposolid's category is hidden
    call(&mut s, "transform", json!({ "ids": [label], "kind": "move", "dx": 1000, "dy": 0 }));
    assert_eq!(call(&mut s, "plan", json!({ "view_id": site }))["contour_labels"][0]["a"], json!([4000.0, 100.0]));
    // saving and reopening keeps the lines and the label
    let file = std::env::temp_dir().join(format!("openarch-topo-lines-{}.oab", std::process::id()));
    call(&mut s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    std::fs::remove_file(&file).unwrap();
    let plan = call(&mut again, "plan", json!({ "view_id": site }));
    assert_eq!(plan["toposolids"][0]["lines"].as_array().unwrap().len(), 1);
    assert_eq!(plan["contour_labels"][0]["labels"].as_array().unwrap().len(), 3);
}

#[test]
fn floors_and_toposolids_excavate_the_ground_and_the_hole_follows_them() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let lz = info["levels"][0]["elevation_mm"].as_f64().unwrap_or(0.0);
    let id = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "offset_mm": 0, "curves": square(-10000.0, -10000.0, 10000.0, 10000.0) }))["id"].as_u64().unwrap();
    let volume = |s: &mut Session| row(s, id, "Volume")["raw"].as_f64().unwrap();
    assert!((volume(&mut s) - 400.0).abs() < 1e-6);
    let ft = &info["floor_types"][0];
    let th = ft["thickness_mm"].as_f64().unwrap();
    let floor = call(&mut s, "add_floor", json!({ "type_id": ft["id"], "level_id": level, "points": [[-2000, -2000], [2000, -2000], [2000, 2000], [-2000, 2000]] }))["id"].as_u64().unwrap();
    // standing on the ground it takes nothing out, so it is refused
    call(&mut s, "patch_floor", json!({ "id": floor, "patch": { "offset_mm": th } }));
    assert!(s.handle("excavate_toposolid", &json!({ "id": id, "by": [floor] })).is_err());
    assert!(s.handle("excavate_toposolid", &json!({ "id": id, "by": [id] })).is_err());
    // sunk 500: the ground over its footprint goes down to its underside
    call(&mut s, "patch_floor", json!({ "id": floor, "patch": { "offset_mm": -500 } }));
    assert_eq!(call(&mut s, "excavate_toposolid", json!({ "id": id, "by": [floor] }))["added"], 1);
    let pit = 16.0 * (500.0 + th) / 1000.0;
    assert!((volume(&mut s) - (400.0 - pit)).abs() < 0.01, "{}", volume(&mut s));
    let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
    assert_eq!(call(&mut s, "plan", json!({ "view_id": site }))["toposolids"][0]["excavated_by"], json!([floor]));
    // a section across shows the pit, the earth cut off by it
    let view = call(&mut s, "add_section", json!({ "level_id": level, "start": [0, -15000], "end": [0, 15000] }))["view_id"].as_u64().unwrap();
    let low = |s: &mut Session| {
        let items = call(s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().clone();
        let cut = items.iter().find(|i| i["id"] == id && i["cut"] == true).unwrap().clone();
        let n = cut["polys"][0].as_array().unwrap().len();
        // the top of the band: its first half
        cut["polys"][0].as_array().unwrap()[..n / 2].iter().map(|p| p[1].as_f64().unwrap()).fold(f64::INFINITY, f64::min)
    };
    assert!((low(&mut s) - (lz - 500.0 - th)).abs() < 1e-3, "{}", low(&mut s));
    // the hole follows the floor: moved clear of the section, the earth there is whole again
    call(&mut s, "transform", json!({ "ids": [floor], "kind": "move", "dx": 5000, "dy": 0 }));
    assert!((low(&mut s) - lz).abs() < 1e-3);
    assert!((volume(&mut s) - (400.0 - pit)).abs() < 0.01);
    call(&mut s, "undo", json!({}));
    // a parking lot: a toposolid sunk into the other takes out down to its own underside, here below the host's,
    // so a hole goes right through
    let lot = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "offset_mm": -200, "curves": square(5000.0, 5000.0, 9000.0, 9000.0) }))["id"].as_u64().unwrap();
    call(&mut s, "excavate_toposolid", json!({ "id": id, "by": [lot] }));
    assert!((volume(&mut s) - (400.0 - pit - 16.0)).abs() < 0.02, "{}", volume(&mut s));
    let mesh = call(&mut s, "scene", json!({}))["meshes"].as_array().unwrap().iter().any(|m| m["id"] == id);
    assert!(mesh);
    // saved and opened again it is still excavated
    let file = std::env::temp_dir().join(format!("openarch-topo-dig-{}.oab", std::process::id()));
    call(&mut s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    assert!((row(&mut again, id, "Volume")["raw"].as_f64().unwrap() - volume(&mut s)).abs() < 1e-6);
    let _ = std::fs::remove_file(&file);
    // Remove Excavation, and a deleted floor takes its hole with it
    assert_eq!(call(&mut s, "remove_excavation", json!({ "id": id, "by": [lot] }))["removed"], 1);
    assert!(s.handle("remove_excavation", &json!({ "id": id, "by": [lot] })).is_err());
    call(&mut s, "delete", json!({ "ids": [floor] }));
    assert!((volume(&mut s) - 400.0).abs() < 1e-6);
    call(&mut s, "undo", json!({}));
    assert!((volume(&mut s) - (400.0 - pit)).abs() < 0.01);
}

#[test]
fn sub_divisions_split_and_merge_through_the_api() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
    let id = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "offset_mm": 0, "curves": square(0.0, 0.0, 20000.0, 10000.0) }))["id"].as_u64().unwrap();
    call(&mut s, "add_toposolid_point", json!({ "id": id, "point": [5000, 5000], "elevation": 1500, "base": "current_level" }));
    // a road 200 below the ground, running off the edge: cut off there, and it takes its depth out of the ground
    let road = call(&mut s, "add_toposolid_subdivision", json!({ "host_id": id, "type_id": ty, "offset_mm": -200, "curves": square(-2000.0, 8000.0, 22000.0, 9000.0) }))["id"].as_u64().unwrap();
    assert_eq!(row(&mut s, road, "Area")["raw"], 20.0);
    assert_eq!(row(&mut s, road, "Sub-divide Height")["raw"], -200.0);
    assert!((row(&mut s, road, "Excavation Volume")["raw"].as_f64().unwrap() - 20.0).abs() < 0.05);
    assert!(s.handle("add_toposolid_point", &json!({ "id": road, "point": [3000, 8500], "elevation": 0 })).is_err());
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let list = plan["toposolids"].as_array().unwrap();
    assert_eq!(list.last().unwrap()["id"], road, "sub-divisions draw over their toposolid");
    assert_eq!(list.last().unwrap()["host"], id);
    // split down the middle: two toposolids, the road split with them
    let ids: Vec<u64> = call(&mut s, "split_toposolid", json!({ "id": id, "curves": [{ "type": "line", "a": { "x": 10000, "y": -500 }, "b": { "x": 10000, "y": 10500 } }] }))["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let hosts: Vec<Value> = plan["toposolids"].as_array().unwrap().iter().filter(|t| !t["host"].is_null()).map(|t| t["host"].clone()).collect();
    assert_eq!(hosts.len(), 2);
    assert!(hosts.contains(&json!(ids[0])) && hosts.contains(&json!(ids[1])));
    // saved and opened again, then merged back into one
    let file = std::env::temp_dir().join(format!("openarch-topo-split-{}.oab", std::process::id()));
    call(&mut s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    let _ = std::fs::remove_file(&file);
    assert_eq!(row(&mut again, road, "Host")["value"].as_str().unwrap().is_empty(), false);
    call(&mut again, "merge_toposolids", json!({ "id": ids[0], "other": ids[1] }));
    assert_eq!(row(&mut again, ids[0], "Area")["raw"], 200.0);
    let plan = call(&mut again, "plan", json!({ "view_id": site }));
    assert_eq!(plan["toposolids"].as_array().unwrap().iter().filter(|t| t["host"] == json!(ids[0])).count(), 2);
    assert!(again.handle("merge_toposolids", &json!({ "id": ids[0], "other": road })).is_err());
}

#[test]
fn graded_regions_report_cut_and_fill_and_property_lines_come_from_bearings() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
    let id = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "curves": square(0.0, 0.0, 10000.0, 10000.0) }))["id"].as_u64().unwrap();
    call(&mut s, "add_toposolid_point", json!({ "id": id, "point": [5000, 5000], "elevation": 1000, "mode": "surface" }));
    assert_eq!(call(&mut s, "describe", json!({ "id": id }))["graded"], false);
    call(&mut s, "grade_toposolid", json!({ "id": id, "perimeter_only": true }));
    let p = call(&mut s, "describe", json!({ "id": id }));
    assert_eq!(p["graded"], true);
    // the mound went with the inside points: a third of 100 m² by 1 m, all cut
    let cut = row(&mut s, id, "Cut")["raw"].as_f64().unwrap();
    assert!((cut - 100.0 / 3.0).abs() < 1e-3, "{cut}");
    assert!((row(&mut s, id, "Net Cut/Fill")["raw"].as_f64().unwrap() + cut).abs() < 1e-6);
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    assert!(!plan["toposolids"][0]["existing"].as_array().unwrap().is_empty(), "existing contours");
    assert!(s.handle("grade_toposolid", &json!({ "id": id })).is_err());

    // a lot from its survey: 20 m east, 30 m north, back west, and south again, with bearings typed as surveys give them
    let rows = json!([
        { "distance_mm": 20000, "bearing": "90°00'00\"" },
        { "distance_mm": 30000, "bearing": "N 0 E" },
        { "distance_mm": 20000, "bearing": 270 },
        { "distance_mm": 30000, "bearing": "180 00 00" },
    ]);
    let pl = call(&mut s, "add_property_line_table", json!({ "at": [0, 0], "rows": rows }))["id"].as_u64().unwrap();
    assert_eq!(row(&mut s, pl, "Area")["raw"], 600.0);
    let back = call(&mut s, "property_line", json!({ "id": pl }));
    assert_eq!(back["table"], true);
    assert_eq!(back["rows"][3]["bearing"], "180°00'00\"");
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let drawn = plan["property_lines"].as_array().unwrap().iter().find(|p| p["id"] == pl).unwrap().clone();
    assert_eq!(drawn["labels"][1]["text"], "0°00'00\"  30.000");
    assert_eq!(drawn["style"]["line"]["pattern"], "Dash dot dot");
    assert!(s.handle("add_property_line_table", &json!({ "at": [0, 0], "rows": [{ "distance_mm": 1000, "bearing": "N 95 E" }] })).is_err());
    assert!(s.handle("set_property_line_sketch", &json!({ "id": pl, "curves": square(0.0, 0.0, 1000.0, 1000.0) })).is_err());
    // a sketched one, open: no area until it closes
    let open = call(&mut s, "add_property_line", json!({ "curves": square(0.0, 0.0, 5000.0, 5000.0).as_array().unwrap()[..3].to_vec() }))["id"].as_u64().unwrap();
    assert_eq!(row(&mut s, open, "Area")["value"], "Not closed");
    call(&mut s, "set_property_line_sketch", json!({ "id": open, "curves": square(0.0, 0.0, 5000.0, 5000.0) }));
    assert_eq!(row(&mut s, open, "Area")["raw"], 25.0);
    call(&mut s, "patch_property_line", json!({ "id": open, "patch": { "name": "Lot 7", "label_segments": false } }));
    // saved and opened again
    let file = std::env::temp_dir().join(format!("openarch-graded-{}.oab", std::process::id()));
    call(&mut s, "save", json!({ "path": file }));
    let mut again = Session::new();
    call(&mut again, "open", json!({ "path": file }));
    let _ = std::fs::remove_file(&file);
    assert!((row(&mut again, id, "Cut")["raw"].as_f64().unwrap() - cut).abs() < 1e-6);
    assert_eq!(row(&mut again, open, "Name")["value"], "Lot 7");
    let plan = call(&mut again, "plan", json!({ "view_id": site }));
    assert!(plan["property_lines"].as_array().unwrap().iter().find(|p| p["id"] == open).unwrap()["labels"].as_array().unwrap().is_empty());
}

#[test]
fn a_contour_subcategory_turned_off_in_visibility_graphics_is_not_drawn() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
    let id = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "curves": square(0.0, 0.0, 10000.0, 10000.0) }))["id"].as_u64().unwrap();
    call(&mut s, "add_toposolid_point", json!({ "id": id, "point": [5000, 5000], "elevation": 6000, "mode": "surface" }));
    let kinds = |s: &mut Session| -> Vec<bool> { call(s, "plan", json!({ "view_id": site }))["toposolids"][0]["contours"].as_array().unwrap().iter().map(|c| c["primary"].as_bool().unwrap()).collect() };
    let before = kinds(&mut s);
    assert!(before.contains(&true) && before.contains(&false), "{before:?}");
    call(&mut s, "patch_view", json!({ "id": site, "patch": { "settings": { "overrides": { "model": { "Topography": { "subcategories": { "Primary Contours": { "visible": false } } } } } } } }));
    let after = kinds(&mut s);
    assert!(!after.is_empty() && after.iter().all(|p| !p), "{after:?}");
}
