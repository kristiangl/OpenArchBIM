//! IFC export of what stands on the site: the ground, grids, kerbs, parking and line marking, and property lines;
//! and the grids, kerbs and property lines read back in.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn line(a: [f64; 2], b: [f64; 2]) -> Value {
    json!({ "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] } })
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([line([x0, y0], [x1, y0]), line([x1, y0], [x1, y1]), line([x1, y1], [x0, y1]), line([x0, y1], [x0, y0])])
}

fn export(s: &mut Session, name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("openarch-ifc-site-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    call(s, "export_ifc", json!({ "path": path.to_string_lossy() }));
    std::fs::read_to_string(&path).unwrap()
}

fn count(f: &str, kind: &str) -> usize {
    f.lines().filter(|l| l.contains(&format!("={kind}("))).count()
}

/// Every `#n` an entity refers to is an entity of the file.
fn check_references(f: &str) {
    let ids: std::collections::BTreeSet<&str> = f.lines().filter(|l| l.starts_with('#')).map(|l| &l[..l.find('=').unwrap()]).collect();
    for l in f.lines().filter(|l| l.starts_with('#')) {
        let body = &l[l.find('=').unwrap() + 1..];
        let mut rest = body;
        while let Some(i) = rest.find('#') {
            let tail = &rest[i..];
            let end = tail[1..].find(|c: char| !c.is_ascii_digit()).map_or(tail.len(), |e| e + 1);
            // a '#' inside a quoted name is not a reference: those used here have none
            assert!(end == 1 || ids.contains(&tail[..end]), "{} is missing, used by {l}", &tail[..end]);
            rest = &tail[end..];
        }
    }
}

#[test]
fn the_site_goes_out_with_its_ground_grids_kerbs_parking_and_property_lines() {
    let mut s = Session::new();
    call(&mut s, "ensure_toposolid_types", json!({}));
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    // a new project writes none of these
    let empty = export(&mut s, "empty.ifc");
    for kind in ["IFCGRID", "IFCGEOGRAPHICELEMENT", "IFCBUILDINGELEMENTPROXY", "IFCANNOTATION"] {
        assert_eq!(count(&empty, kind), 0, "{kind} in an empty project");
    }
    // one grid line alone is left out: a grid needs a line each way
    call(&mut s, "add_grid", json!({ "curves": [line([0.0, -2000.0], [0.0, 9000.0])] }));
    assert_eq!(count(&export(&mut s, "one.ifc"), "IFCGRID"), 0);
    // two parallel lines: the first goes to the other direction so the grid is a valid one
    call(&mut s, "add_grid", json!({ "curves": [line([6000.0, -2000.0], [6000.0, 9000.0])] }));
    let parallel = export(&mut s, "parallel.ifc");
    assert_eq!((count(&parallel, "IFCGRID"), count(&parallel, "IFCGRIDAXIS")), (1, 2));
    call(&mut s, "add_grid", json!({ "curves": [line([-2000.0, 0.0], [8000.0, 0.0])] }));

    call(&mut s, "add_toposolid", json!({ "type_id": info["toposolid_types"][0]["id"], "level_id": level, "offset_mm": -200, "curves": square(-8000.0, -14000.0, 22000.0, 14000.0) }));
    call(&mut s, "add_kerb", json!({ "level_id": level, "curves": [line([0.0, -3000.0], [9000.0, -3000.0]), line([9000.0, -3000.0], [9000.0, 1000.0])], "height_mm": 120, "width_mm": 180 }));
    call(&mut s, "add_kerb_ramp", json!({ "level_id": level, "top": [4000, -3000], "bottom": [4000, -4200] }));
    call(&mut s, "add_line_marking", json!({ "level_id": level, "curves": [line([0.0, -5000.0], [9000.0, -5000.0])] }));
    call(&mut s, "add_parking_row", json!({ "level_id": level, "start": [12000, -5000], "end": [20100, -5000], "bay_width_mm": 2700, "bay_length_mm": 5500, "wheel_stops": true, "accessible": true }));
    call(&mut s, "add_property_line", json!({ "curves": square(-8000.0, -14000.0, 22000.0, 14000.0) }));

    let f = export(&mut s, "site.ifc");
    check_references(&f);
    assert_eq!(count(&f, "IFCGEOGRAPHICELEMENT"), 1, "the ground");
    assert_eq!((count(&f, "IFCGRID"), count(&f, "IFCGRIDAXIS")), (1, 3));
    // the two lines up the page are the U axes and the one across is the V axis
    let grid = f.lines().find(|l| l.contains("=IFCGRID(")).unwrap();
    let lists: Vec<&str> = grid.split("(#").skip(1).collect();
    let axes_in = |list: &str| list.split(')').next().unwrap().matches('#').count() + 1;
    assert_eq!((axes_in(lists[lists.len() - 2]), axes_in(lists[lists.len() - 1])), (2, 1), "{grid}");
    // a kerb, a kerb ramp, a line marking and a parking row, each one element with its name
    assert_eq!(count(&f, "IFCBUILDINGELEMENTPROXY"), 4);
    for name in ["'Kerb'", "'Kerb Ramp'", "'Line Marking'", "'Parking Bays'"] {
        assert!(f.contains(name), "{name}");
    }
    // the parking row has its lines, its access symbol and its wheel stops as separate coloured meshes
    assert!(count(&f, "IFCSTYLEDITEM") >= 5, "{}", count(&f, "IFCSTYLEDITEM"));
    assert_eq!(count(&f, "IFCANNOTATION"), 1);
    assert!(f.contains("'Property Line'"));
    // every one of them is in a storey
    let held: String = f.lines().filter(|l| l.contains("=IFCRELCONTAINEDINSPATIALSTRUCTURE(")).collect();
    for kind in ["IFCGRID", "IFCGEOGRAPHICELEMENT", "IFCBUILDINGELEMENTPROXY", "IFCANNOTATION"] {
        for l in f.lines().filter(|l| l.contains(&format!("={kind}("))) {
            let id = &l[..l.find('=').unwrap()];
            assert!(held.contains(&format!("{id},")) || held.contains(&format!("{id})")), "{kind} {id} is in no storey");
        }
    }
    // and the file still opens here
    let path = std::env::temp_dir().join(format!("openarch-ifc-site-{}", std::process::id())).join("site.ifc");
    let back = call(&mut s, "open_ifc", json!({ "path": path.to_string_lossy() }));
    assert!(back["counts"].is_object(), "{back}");
    // with its grids, property line and kerb made again
    assert_eq!((back["counts"]["grids"].as_u64(), back["counts"]["property_lines"].as_u64(), back["counts"]["kerbs"].as_u64()), (Some(3), Some(1), Some(1)), "{back}");
    assert!(!back["skipped"].to_string().contains("IFCGRID"), "{back}");
    let info = call(&mut s, "doc_info", json!({}));
    let site = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan").unwrap()["id"].as_u64().unwrap();
    let plan = call(&mut s, "plan", json!({ "view_id": site }));
    let names: Vec<&str> = plan["grids"].as_array().unwrap().iter().filter_map(|g| g["name"].as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    let pl = &plan["property_lines"].as_array().unwrap()[0];
    assert_eq!(pl["lines"].as_array().unwrap().len(), 4, "four sides");
    let kerb = plan["traffic"].as_array().unwrap().iter().find(|t| t["kind"] == "kerb").expect("the kerb").clone();
    let d = call(&mut s, "describe", json!({ "id": kerb["id"] }));
    let rows: Vec<Value> = d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect();
    let raw = |label: &str| rows.iter().find(|r| r["label"] == label).and_then(|r| r["raw"].as_f64());
    assert_eq!((raw("Height"), raw("Width")), (Some(120.0), Some(180.0)), "{rows:?}");
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}
