//! CAD drawings: Import CAD and Link CAD (DXF and DWG) into a floor plan, the drawing the page fetches, layers,
//! Properties, Move and Rotate, Reload, and save and reopen.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

/// A 10 x 6 room in metres on WALLS, a note on NOTES, and a contour at 12.5 on a layer the file has turned off.
const DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n6\n0\nENDSEC\n\
0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n3\n\
0\nLAYER\n2\nWALLS\n70\n0\n62\n1\n6\nCONTINUOUS\n\
0\nLAYER\n2\nNOTES\n70\n0\n62\n7\n6\nCONTINUOUS\n\
0\nLAYER\n2\nCONTOURS\n70\n0\n62\n-3\n6\nCONTINUOUS\n\
0\nENDTAB\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n\
0\nLWPOLYLINE\n8\nWALLS\n90\n4\n70\n1\n10\n0.0\n20\n0.0\n10\n10.0\n20\n0.0\n10\n10.0\n20\n6.0\n10\n0.0\n20\n6.0\n\
0\nTEXT\n8\nNOTES\n10\n1.0\n20\n1.0\n40\n0.3\n1\nLIVING\n\
0\nLWPOLYLINE\n8\nCONTOURS\n90\n2\n70\n0\n38\n12.5\n10\n0.0\n20\n3.0\n10\n10.0\n20\n3.0\n\
0\nENDSEC\n0\nEOF\n";

fn temp(name: &str, bytes: &[u8]) -> String {
    let dir = std::env::temp_dir().join(format!("openarch-cad-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p.to_string_lossy().to_string()
}

fn plans(s: &mut Session) -> (u64, u64) {
    let info = call(s, "doc_info", json!({}));
    let views = info["views"].as_array().unwrap();
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let on_level: Vec<u64> = views.iter().filter(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() == Some(level)).map(|v| v["id"].as_u64().unwrap()).collect();
    assert!(!on_level.is_empty(), "{views:?}");
    let other = views.iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() != Some(level)).map(|v| v["id"].as_u64().unwrap()).unwrap_or(0);
    (on_level[0], other)
}

#[test]
fn a_dxf_is_imported_drawn_moved_and_kept() {
    let mut s = Session::new();
    let (view, other) = plans(&mut s);
    let path = temp("room.dxf", DXF.as_bytes());
    let r = call(&mut s, "import_cad", json!({ "path": path, "view_id": view, "position": "origin" }));
    let id = r["id"].as_u64().unwrap();
    assert_eq!((r["format"].as_str(), r["unit"].as_str(), r["scale"].as_f64(), r["scale_from"].as_str()), (Some("DXF"), Some("m"), Some(1000.0), Some("file")));
    assert_eq!((r["counts"]["lines"].as_u64(), r["counts"]["texts"].as_u64(), r["linked"].as_bool()), (Some(2), Some(1), Some(false)));

    // the plan is told where it sits; the layer the file has off starts hidden
    let plan = call(&mut s, "plan", json!({ "view_id": view }));
    let c = &plan["cad_imports"][0];
    assert_eq!((c["id"].as_u64(), c["scale"].as_f64(), c["hidden"][0].as_str()), (Some(id), Some(1000.0), Some("CONTOURS")));
    assert_eq!(c["ring"][2], json!([10000.0, 6000.0]));
    if other != 0 {
        assert_eq!(call(&mut s, "plan", json!({ "view_id": other }))["cad_imports"].as_array().unwrap().len(), 0, "another level's plan doesn't draw it");
    }

    // the drawing as the page fetches it: about its middle (5, 3), the contour with its height
    let d = call(&mut s, "cad_drawing", json!({ "id": id }));
    assert_eq!((d["stamp"].as_str(), d["base"].clone()), (c["stamp"].as_str(), json!([5.0, 3.0])));
    let shapes = d["shapes"].as_array().unwrap();
    // the file's own layers come after the layer 0 every drawing has
    let walls = d["layers"].as_array().unwrap().iter().position(|l| l["name"] == "WALLS").unwrap();
    assert_eq!(shapes[0], json!([0, walls, null, 1, [-5.0, -3.0, 5.0, -3.0, 5.0, 3.0, -5.0, 3.0]]));
    assert_eq!(shapes[1][9], "LIVING");
    assert_eq!(shapes[2][5], json!([12.5]));
    assert_eq!(d["layers"][walls]["color"], json!([255, 0, 0]));

    // layers, Properties, and showing the contour layer
    let layers = call(&mut s, "cad_layers", json!({ "id": id }));
    assert_eq!(layers["layers"].as_array().unwrap().iter().map(|l| (l["name"].as_str().unwrap(), l["shown"].as_bool().unwrap())).collect::<Vec<_>>(), vec![("0", true), ("WALLS", true), ("NOTES", true), ("CONTOURS", false)]);
    call(&mut s, "patch_cad_import", json!({ "id": id, "patch": { "hidden_layers": [] } }));
    let props = call(&mut s, "describe", json!({ "id": id }));
    let rows: Vec<Value> = props["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect();
    assert_eq!(rows.iter().find(|r| r["label"] == "Layers").unwrap()["value"], "4 of 4 shown");
    assert_eq!(rows.iter().find(|r| r["label"] == "File Units").unwrap()["value"], "m");

    // Move and Rotate carry it
    call(&mut s, "transform", json!({ "kind": "move", "ids": [id], "dx": 1000.0, "dy": 2000.0 }));
    call(&mut s, "transform", json!({ "kind": "rotate", "ids": [id], "cx": 1000.0, "cy": 2000.0, "angle_deg": 90.0 }));
    let c = call(&mut s, "plan", json!({ "view_id": view }))["cad_imports"][0].clone();
    assert_eq!((c["at"].clone(), c["angle_deg"].as_f64()), (json!([1000.0, 2000.0]), Some(90.0)));
    let far = c["ring"][2].as_array().unwrap();
    assert!((far[0].as_f64().unwrap() + 5000.0).abs() < 1e-6 && (far[1].as_f64().unwrap() - 12000.0).abs() < 1e-6, "{far:?}");

    // an import has no file to read again
    assert!(s.handle("reload_cad", &json!({ "id": id })).is_err());

    // saved and opened again, the drawing is still there
    let file = temp("cad.oab", b"");
    std::fs::remove_file(&file).unwrap();
    call(&mut s, "save", json!({ "path": file }));
    let mut t = Session::new();
    call(&mut t, "open", json!({ "path": file }));
    let c2 = call(&mut t, "plan", json!({ "view_id": view }))["cad_imports"][0].clone();
    assert_eq!(c2["at"], c["at"]);
    assert_eq!(call(&mut t, "cad_drawing", json!({ "id": id }))["shapes"].as_array().unwrap().len(), 3);

    // pinned, it can't be deleted; unpinned, it can, and undo brings it back
    call(&mut s, "patch_cad_import", json!({ "id": id, "patch": { "pinned": true } }));
    assert!(s.handle("delete", &json!({ "ids": [id] })).is_err());
    call(&mut s, "patch_cad_import", json!({ "id": id, "patch": { "pinned": false } }));
    call(&mut s, "delete", json!({ "ids": [id] }));
    assert_eq!(call(&mut s, "plan", json!({ "view_id": view }))["cad_imports"].as_array().unwrap().len(), 0);
    call(&mut s, "undo", json!({}));
    assert_eq!(call(&mut s, "plan", json!({ "view_id": view }))["cad_imports"].as_array().unwrap().len(), 1);
}

#[test]
fn a_dwg_is_linked_to_one_view_and_reloaded() {
    let mut s = Session::new();
    let (view, _) = plans(&mut s);
    let path = temp("room.dwg", &openarch_cad::dwg_from_dxf(DXF).unwrap());
    let r = call(&mut s, "import_cad", json!({ "path": path, "view_id": view, "link": true, "current_view_only": true, "layers": "all", "at": [20000, 0] }));
    let id = r["id"].as_u64().unwrap();
    assert_eq!((r["format"].as_str(), r["linked"].as_bool(), r["scale"].as_f64()), (Some("DWG"), Some(true), Some(1000.0)));
    // its middle (5, 3) m went on the point given
    let c = call(&mut s, "plan", json!({ "view_id": view }))["cad_imports"][0].clone();
    assert_eq!((c["at"].clone(), c["hidden"].as_array().unwrap().len()), (json!([15000.0, -3000.0]), 0));
    let stamp = c["stamp"].as_str().unwrap().to_string();

    // the file changes (the note goes); Reload reads it again and keeps the drawing where it was
    let less = DXF.replace("0\nTEXT\n8\nNOTES\n10\n1.0\n20\n1.0\n40\n0.3\n1\nLIVING\n", "");
    std::fs::write(&path, openarch_cad::dwg_from_dxf(&less).unwrap()).unwrap();
    let r = call(&mut s, "reload_cad", json!({ "id": id }));
    assert_eq!(r["counts"]["texts"].as_u64(), Some(0));
    let c = call(&mut s, "plan", json!({ "view_id": view }))["cad_imports"][0].clone();
    assert_ne!(c["stamp"].as_str().unwrap(), stamp);
    assert_eq!(c["at"], json!([15000.0, -3000.0]));

    // what isn't a drawing is refused, and nothing is added
    let junk = temp("junk.dwg", b"AC1032 not really a drawing");
    assert!(s.handle("import_cad", &json!({ "path": junk, "view_id": view })).is_err());
    assert_eq!(call(&mut s, "plan", json!({ "view_id": view }))["cad_imports"].as_array().unwrap().len(), 1);
}

#[test]
fn contours_of_an_import_make_a_toposolid() {
    // three contours 10 m apart at 10, 11 and 12 m, a spot level, and a flat boundary on its own layer
    let contour = |y: f64, z: f64| format!("0\nLWPOLYLINE\n8\nCONTOURS\n90\n2\n70\n0\n38\n{z}\n10\n0.0\n20\n{y}\n10\n30.0\n20\n{y}\n");
    let dxf = format!("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n6\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{}{}{}\
0\nPOINT\n8\nLEVELS\n10\n15.0\n20\n5.0\n30\n10.8\n\
0\nLWPOLYLINE\n8\nBOUNDARY\n90\n4\n70\n1\n10\n-5.0\n20\n-5.0\n10\n35.0\n20\n-5.0\n10\n35.0\n20\n25.0\n10\n-5.0\n20\n25.0\n\
0\nENDSEC\n0\nEOF\n", contour(0.0, 10.0), contour(10.0, 11.0), contour(20.0, 12.0));
    let mut s = Session::new();
    let (view, _) = plans(&mut s);
    let path = temp("contours.dxf", dxf.as_bytes());
    let id = call(&mut s, "import_cad", json!({ "path": path, "view_id": view, "position": "origin" }))["id"].as_u64().unwrap();
    // turned a quarter, as a survey is to suit the plan: the ground goes with it
    call(&mut s, "transform", json!({ "kind": "rotate", "ids": [id], "cx": 0.0, "cy": 0.0, "angle_deg": 90.0 }));

    let h = call(&mut s, "cad_heights", json!({ "id": id }));
    let layer = |n: &str| h["layers"].as_array().unwrap().iter().find(|l| l["name"] == n).unwrap().clone();
    assert_eq!((layer("CONTOURS")["raised"].as_u64(), layer("CONTOURS")["lo"].as_f64(), layer("CONTOURS")["hi"].as_f64()), (Some(3), Some(10.0), Some(12.0)));
    assert_eq!((layer("BOUNDARY")["raised"].as_u64(), layer("BOUNDARY")["flat"].as_u64(), layer("LEVELS")["raised"].as_u64()), (Some(0), Some(1), Some(1)));

    call(&mut s, "ensure_toposolid_types", json!({}));
    let ty = call(&mut s, "doc_info", json!({}))["toposolid_types"][0]["id"].as_u64().unwrap();
    // flat layers alone give nothing
    assert!(s.handle("toposolid_from_cad", &json!({ "id": id, "layers": ["BOUNDARY"], "type_id": ty })).is_err());
    let r = call(&mut s, "toposolid_from_cad", json!({ "id": id, "layers": ["CONTOURS", "LEVELS", "BOUNDARY"], "type_id": ty, "base": "internal_origin" }));
    assert_eq!((r["read"].as_u64(), r["lo"].as_f64(), r["hi"].as_f64()), (Some(7), Some(10000.0), Some(12000.0)));
    let topo = r["id"].as_u64().unwrap();
    let props = call(&mut s, "describe", json!({ "id": topo }));
    let rows: Vec<Value> = props["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect();
    let row = |l: &str| rows.iter().find(|r| r["label"] == l).unwrap().clone();
    assert_eq!((row("Area")["raw"].as_f64(), row("Points")["raw"].as_f64()), (Some(600.0), Some(7.0)));
    // the drawing's (30, 20) is at (-20, 30) m once turned
    let t = call(&mut s, "plan", json!({ "view_id": view }))["toposolids"][0].clone();
    let ring = t["rings"][0].as_array().unwrap();
    assert!(ring.iter().any(|p| (p[0].as_f64().unwrap() + 20000.0).abs() < 1e-3 && (p[1].as_f64().unwrap() - 30000.0).abs() < 1e-3), "{ring:?}");
    // one undo takes the toposolid away and leaves the drawing
    call(&mut s, "undo", json!({}));
    let plan = call(&mut s, "plan", json!({ "view_id": view }));
    assert_eq!((plan["toposolids"].as_array().unwrap().len(), plan["cad_imports"].as_array().unwrap().len()), (0, 1));
}

#[test]
fn level_text_and_a_lot_boundary_shape_the_ground_from_an_import() {
    // contours at 10, 11 and 12 m, a lot drawn inside them, a level written as text, and a lot number that isn't one
    let contour = |y: f64, z: f64| format!("0\nLWPOLYLINE\n8\nCONTOURS\n90\n2\n70\n0\n38\n{z}\n10\n0.0\n20\n{y}\n10\n30.0\n20\n{y}\n");
    let dxf = format!("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n6\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{}{}{}\
0\nTEXT\n8\nLEVELS\n10\n15.0\n20\n5.0\n40\n0.3\n1\nRL 10.80\n\
0\nTEXT\n8\nLEVELS\n10\n15.0\n20\n15.0\n40\n0.3\n1\nLOT 123\n\
0\nLWPOLYLINE\n8\nBOUNDARY\n90\n4\n70\n1\n10\n5.0\n20\n2.0\n10\n25.0\n20\n2.0\n10\n25.0\n20\n18.0\n10\n5.0\n20\n18.0\n\
0\nENDSEC\n0\nEOF\n", contour(0.0, 10.0), contour(10.0, 11.0), contour(20.0, 12.0));
    let mut s = Session::new();
    let (view, _) = plans(&mut s);
    let path = temp("lot.dxf", dxf.as_bytes());
    let id = call(&mut s, "import_cad", json!({ "path": path, "view_id": view, "position": "origin" }))["id"].as_u64().unwrap();
    let h = call(&mut s, "cad_heights", json!({ "id": id }));
    let layer = |n: &str| h["layers"].as_array().unwrap().iter().find(|l| l["name"] == n).unwrap().clone();
    assert_eq!((layer("LEVELS")["level_texts"].as_u64(), layer("BOUNDARY")["closed"].as_u64(), layer("CONTOURS")["closed"].as_u64()), (Some(1), Some(1), Some(0)));

    call(&mut s, "ensure_toposolid_types", json!({}));
    let ty = call(&mut s, "doc_info", json!({}))["toposolid_types"][0]["id"].as_u64().unwrap();
    // without the text there are six contour ends; with it, seven points
    let r = call(&mut s, "toposolid_from_cad", json!({ "id": id, "layers": ["CONTOURS", "LEVELS"], "type_id": ty, "base": "internal_origin", "text_levels": true }));
    assert_eq!(r["read"].as_u64(), Some(7));
    call(&mut s, "undo", json!({}));
    // bounded by the lot: its area, and its corners at the height of the ground there (2 m up the slope is 10.2 m)
    assert!(s.handle("toposolid_from_cad", &json!({ "id": id, "layers": ["CONTOURS"], "type_id": ty, "boundary_layer": "CONTOURS" })).is_err());
    let r = call(&mut s, "toposolid_from_cad", json!({ "id": id, "layers": ["CONTOURS"], "type_id": ty, "base": "internal_origin", "boundary_layer": "BOUNDARY" }));
    let topo = r["id"].as_u64().unwrap();
    let props = call(&mut s, "describe", json!({ "id": topo }));
    let rows: Vec<Value> = props["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect();
    assert_eq!(rows.iter().find(|r| r["label"] == "Area").unwrap()["raw"].as_f64(), Some(320.0));
    let t = call(&mut s, "plan", json!({ "view_id": view }))["toposolids"][0].clone();
    let level = call(&mut s, "doc_info", json!({}))["levels"][0]["elevation_mm"].as_f64().unwrap_or(0.0);
    let corner = t["points"].as_array().unwrap().iter().find(|p| p[0] == json!(5000.0) && p[1] == json!(2000.0)).expect("a point at the lot's corner");
    assert!((corner[2].as_f64().unwrap() + level - 10200.0).abs() < 1e-6, "{corner:?}");
    // the contour ends outside the lot are gone
    assert!(!t["points"].as_array().unwrap().iter().any(|p| p[0] == json!(0.0)));
}
