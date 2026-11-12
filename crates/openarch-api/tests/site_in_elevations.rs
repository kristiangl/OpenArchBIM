//! The ground, kerbs, kerb ramps and wheel stops in elevations and sections.
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

/// The items of one category an elevation or section draws.
fn items(s: &mut Session, view: u64, category: &str) -> Vec<Value> {
    call(s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().iter().filter(|i| i["category"] == category).cloned().collect()
}

fn view(info: &Value, name: &str) -> u64 {
    info["views"].as_array().unwrap().iter().find(|v| v["name"] == name).unwrap_or_else(|| panic!("no view {name}"))["id"].as_u64().unwrap()
}

/// The extent of an item across the view and in height.
fn extent(item: &Value) -> (f64, f64, f64, f64) {
    let pts: Vec<[f64; 2]> = item["polys"].as_array().unwrap().iter().flat_map(|p| p.as_array().unwrap().iter().map(|q| [q[0].as_f64().unwrap(), q[1].as_f64().unwrap()])).collect();
    let f = |k: usize, pick: fn(f64, f64) -> f64, start: f64| pts.iter().map(|p| p[k]).fold(start, pick);
    (f(0, f64::min, f64::INFINITY), f(0, f64::max, f64::NEG_INFINITY), f(1, f64::min, f64::INFINITY), f(1, f64::max, f64::NEG_INFINITY))
}

#[test]
fn an_elevation_standing_on_the_ground_cuts_it() {
    let mut s = Session::new();
    call(&mut s, "ensure_toposolid_types", json!({}));
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let ty = info["toposolid_types"][0]["id"].as_u64().unwrap();
    // the four elevation markers of a new project stand 11 m out: this ground runs past the north, south and east
    // ones and stops short of the west one
    let topo = call(&mut s, "add_toposolid", json!({ "type_id": ty, "level_id": level, "offset_mm": -200, "curves": square(-8000.0, -14000.0, 22000.0, 14000.0) }))["id"].as_u64().unwrap();
    for name in ["North", "South", "East"] {
        let ground = items(&mut s, view(&info, name), "Topography");
        let cut: Vec<&Value> = ground.iter().filter(|i| i["cut"] == true).collect();
        assert!(!cut.is_empty(), "{name}: the ground is cut where the view stands on it: {ground:?}");
        assert!(cut.iter().all(|i| i["id"].as_u64() == Some(topo)));
        // the profile's top is the top of the ground, and it runs the width of the ground as this view sees it
        let (u0, u1, _, top) = cut.iter().map(|i| extent(i)).fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |a, e| (a.0.min(e.0), a.1.max(e.1), a.2.min(e.2), a.3.max(e.3)));
        assert!((top + 200.0).abs() < 1.0, "{name}: top at {top}");
        let wide = if name == "East" { 28000.0 } else { 30000.0 };
        assert!((u1 - u0 - wide).abs() < 1.0, "{name}: {} wide", u1 - u0);
    }
    // from outside the ground it is seen, not cut
    let west = items(&mut s, view(&info, "West"), "Topography");
    assert!(!west.is_empty() && west.iter().all(|i| i["cut"] != true), "{west:?}");
    // hidden in the view, it goes
    let south = view(&info, "South");
    call(&mut s, "hide_elements", json!({ "view_id": south, "ids": [topo] }));
    assert!(items(&mut s, south, "Topography").is_empty());
}

#[test]
fn kerbs_kerb_ramps_and_wheel_stops_show_in_elevations_and_sections() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let south = view(&info, "South");
    assert!(items(&mut s, south, "Hardscape").is_empty() && items(&mut s, south, "Parking").is_empty());
    // in front of the south elevation (its marker is at y = -11000, looking north)
    let kerb = call(&mut s, "add_kerb", json!({ "level_id": level, "curves": [line([-3000.0, -6000.0], [9000.0, -6000.0])] }))["id"].as_u64().unwrap();
    let ramp = call(&mut s, "add_kerb_ramp", json!({ "level_id": level, "top": [4000, -6000], "bottom": [4000, -7200] }))["id"].as_u64().unwrap();
    let row = call(&mut s, "add_parking_row", json!({ "level_id": level, "start": [12000, -5000], "end": [20100, -5000], "bay_width_mm": 2700, "bay_length_mm": 5500, "wheel_stops": true }))["id"].as_u64().unwrap();
    let hard = items(&mut s, south, "Hardscape");
    let of = |id: u64| hard.iter().find(|i| i["id"].as_u64() == Some(id)).unwrap_or_else(|| panic!("{id} is not drawn: {hard:?}")).clone();
    // the kerb: 12 m long and its height high, standing on the level, outlined once round (no line at each piece)
    let (u0, u1, z0, z1) = extent(&of(kerb));
    assert!((u1 - u0 - 12000.0).abs() < 1.0 && z0.abs() < 1.0 && (z1 - 150.0).abs() < 1.0, "kerb {u0} {u1} {z0} {z1}");
    let lines = of(kerb)["lines"].as_array().unwrap().len();
    // (the kerb is built in short pieces that follow the ground, so its top and bottom edges come in as many lines)
    assert!((4..=200).contains(&lines), "the kerb's outline has {lines} lines");
    assert!(of(kerb)["lines"].as_array().unwrap().iter().all(|l| { let (a, b) = (&l[0], &l[1]); let upright = (a[0].as_f64().unwrap() - b[0].as_f64().unwrap()).abs() < 0.1; !upright || a[0].as_f64().unwrap() < -2999.0 || a[0].as_f64().unwrap() > 8999.0 }), "no joint lines along the kerb");
    // the ramp, seen from its low end: as wide as it is, no higher than the kerb
    let (_, _, r0, r1) = extent(&of(ramp));
    assert!(r0.abs() < 1.0 && r1 <= 151.0 && r1 > 50.0, "ramp {r0} {r1}");
    // the wheel stops of the row, under Parking; its paint lies flat and is not drawn
    let stops = items(&mut s, south, "Parking");
    assert!(stops.len() == 1 && stops[0]["id"].as_u64() == Some(row), "{stops:?}");
    assert!(stops[0]["polys"].as_array().unwrap().len() >= 3);
    // behind the viewer nothing shows: the north elevation looks south from y = 11000 and reaches them, the east
    // one (x = 11000, looking west) has the row behind it
    let east = view(&info, "East");
    assert!(items(&mut s, east, "Parking").is_empty());
    assert!(items(&mut s, east, "Hardscape").iter().any(|i| i["id"].as_u64() == Some(kerb)));
    // a section looking at them shows them too, and one looking away does not
    let toward = call(&mut s, "add_section", json!({ "level_id": level, "start": [-4000, -9000], "end": [22000, -9000] }))["id"].as_u64().unwrap();
    let section_view = |s: &mut Session, id: u64| { let i = call(s, "doc_info", json!({})); i["views"].as_array().unwrap().iter().filter(|v| v["view_kind"] == "section").map(|v| v["id"].as_u64().unwrap()).find(|v| *v > id).unwrap() };
    let v1 = section_view(&mut s, toward);
    let seen = items(&mut s, v1, "Hardscape").len() + items(&mut s, v1, "Parking").len();
    call(&mut s, "delete", json!({ "ids": [toward] }));
    let away = call(&mut s, "add_section", json!({ "level_id": level, "start": [22000, -9000], "end": [-4000, -9000] }))["id"].as_u64().unwrap();
    let v2 = section_view(&mut s, away);
    let unseen = items(&mut s, v2, "Hardscape").len() + items(&mut s, v2, "Parking").len();
    assert!(seen.max(unseen) == 3 && seen.min(unseen) == 0, "one way sees the kerb, ramp and stops, the other none: {seen} and {unseen}");
    // hiding the kerb in a view takes it out of that view
    call(&mut s, "hide_elements", json!({ "view_id": south, "ids": [kerb] }));
    assert!(!items(&mut s, south, "Hardscape").iter().any(|i| i["id"].as_u64() == Some(kerb)));
}
