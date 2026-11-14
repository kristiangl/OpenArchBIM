//! Walls that stop close to another wall join it: a T on its length, a corner at its end. An end with joins
//! disallowed, and a caller that asks for none, are left where they are.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn wall(s: &mut Session, view: u64, id: u64) -> Value {
    call(s, "plan", json!({ "view_id": view }))["walls"].as_array().unwrap().iter().find(|w| w["id"].as_u64() == Some(id)).unwrap().clone()
}

#[test]
fn near_misses_join() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let lvl = info["levels"][0]["id"].as_u64().unwrap();
    let wt = info["wall_types"][0]["id"].as_u64().unwrap();
    let view = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() == Some(lvl)).unwrap()["id"].as_u64().unwrap();
    let add = |s: &mut Session, a: [f64; 2], b: [f64; 2]| call(s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": a, "end": b, "height_mm": 2700 }))["id"].as_u64().unwrap();
    let a = add(&mut s, [0.0, 0.0], [8000.0, 0.0]);

    // stopping a little short of the long wall, a wall runs on to its line: a T
    let t = add(&mut s, [4000.0, 3000.0], [4000.0, 150.0]);
    assert_eq!(wall(&mut s, view, t)["end"], json!([4000.0, 0.0]));
    // at an angle it keeps its direction
    let k = add(&mut s, [1000.0, 2000.0], [1950.0, 100.0]);
    let e = wall(&mut s, view, k)["end"].clone();
    assert!((e[0].as_f64().unwrap() - 2000.0).abs() < 1e-6 && e[1].as_f64().unwrap().abs() < 1e-6, "{e}");

    // a corner: both walls come to where their lines meet, the long wall growing by 100
    let c = add(&mut s, [8100.0, 3000.0], [8100.0, 150.0]);
    assert_eq!((wall(&mut s, view, c)["end"].clone(), wall(&mut s, view, a)["end"].clone()), (json!([8100.0, 0.0]), json!([8100.0, 0.0])));
    // one undo takes the wall away and puts the long wall's end back
    call(&mut s, "undo", json!({}));
    assert_eq!(wall(&mut s, view, a)["end"], json!([8000.0, 0.0]));
    call(&mut s, "redo", json!({}));

    // too far to be meant, or told not to: left alone
    let far = add(&mut s, [6000.0, 3000.0], [6000.0, 600.0]);
    assert_eq!(wall(&mut s, view, far)["end"], json!([6000.0, 600.0]));
    let own = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [7000.0, 3000.0], "end": [7000.0, 200.0], "height_mm": 2700, "join": false }))["id"].as_u64().unwrap();
    assert_eq!(wall(&mut s, view, own)["end"], json!([7000.0, 200.0]));

    // a dragged end dropped near the long wall joins it too
    call(&mut s, "move_wall_end", json!({ "id": far, "end": 1, "point": [6000.0, 120.0] }));
    assert_eq!(wall(&mut s, view, far)["end"], json!([6000.0, 0.0]));

    // a corner other walls already share is not pulled about: the new end goes to it when it is in line
    let n = add(&mut s, [8090.0, -3000.0], [8090.0, -120.0]);
    let e = wall(&mut s, view, n)["end"].clone();
    assert_eq!((e, wall(&mut s, view, a)["end"].clone()), (json!([8100.0, 0.0]), json!([8100.0, 0.0])));
}
