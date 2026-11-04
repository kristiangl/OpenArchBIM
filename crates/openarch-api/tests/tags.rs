//! Door and window tags: Marks, Tag by Category, Tag All, following the element, copies, deletes and files.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

struct Model {
    s: Session,
    plan: u64,
    south: u64,
    wall: u64,
    door_type: Value,
    window_type: Value,
}

/// A 6 m wall along y = 0 on the ground level, its plan and the South elevation that looks at it.
fn model() -> Model {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let views = info["views"].as_array().unwrap();
    let plan = views.iter().find(|v| v["level_id"] == level && v["view_kind"] == "floor_plan").unwrap()["id"].as_u64().unwrap();
    let south = views.iter().find(|v| v["view_kind"] == "elevation" && v["name"] == "South").unwrap()["id"].as_u64().unwrap();
    let wall = call(&mut s, "add_wall", json!({ "type_id": info["wall_types"][0]["id"], "level_id": level, "start": [0, 0], "end": [6000, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
    let types = info["family_types"].as_array().unwrap();
    let door_type = types.iter().find(|t| t["category"] == "Doors").unwrap()["id"].clone();
    let window_type = types.iter().find(|t| t["category"] == "Windows").unwrap()["id"].clone();
    Model { s, plan, south, wall, door_type, window_type }
}

fn place(m: &mut Model, ty: &Value, x: f64) -> u64 {
    call(&mut m.s, "place_instance", json!({ "type_id": ty, "host_id": m.wall, "point": [x, 0] }))["id"].as_u64().unwrap()
}

fn mark(s: &mut Session, id: u64) -> String {
    let p = call(s, "describe", json!({ "id": id }));
    p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == "Mark").unwrap()["value"].as_str().unwrap().to_string()
}

fn tags(s: &mut Session, method: &str, view: u64) -> Vec<Value> {
    call(s, method, json!({ "view_id": view }))["element_tags"].as_array().unwrap().clone()
}

fn label_text(tag: &Value) -> String {
    tag["labels"].as_array().unwrap().iter().flat_map(|l| l["lines"].as_array().unwrap().iter().flat_map(|line| line.as_array().unwrap().iter().filter_map(|t| t.as_str().map(str::to_string)))).collect::<Vec<_>>().join(" ")
}

#[test]
fn doors_and_windows_number_their_marks_and_copies_take_the_next() {
    let mut m = model();
    let (dt, wt) = (m.door_type.clone(), m.window_type.clone());
    let d1 = place(&mut m, &dt, 1000.0);
    let d2 = place(&mut m, &dt, 2500.0);
    let w1 = place(&mut m, &wt, 5400.0);
    assert_eq!((mark(&mut m.s, d1), mark(&mut m.s, d2), mark(&mut m.s, w1)), ("1".into(), "2".into(), "1".into()));
    // a numbering someone starts carries on, prefix and padding kept
    call(&mut m.s, "patch_instance", json!({ "id": d2, "patch": { "mark": "D05" } }));
    let d3 = place(&mut m, &dt, 3500.0);
    assert_eq!(mark(&mut m.s, d3), "D06");
    let copied = call(&mut m.s, "transform", json!({ "ids": [d3], "kind": "move", "dx": 1000, "dy": 0, "copy": true }))["ids"][0].as_u64().unwrap();
    assert_eq!(mark(&mut m.s, copied), "D07");
}

#[test]
fn a_tag_reads_its_element_follows_it_and_goes_with_it() {
    let mut m = model();
    let dt = m.door_type.clone();
    let door = place(&mut m, &dt, 2000.0);
    let tag = call(&mut m.s, "add_tag", json!({ "view_id": m.plan, "element_id": door }))["id"].as_u64().unwrap();
    let drawn = tags(&mut m.s, "plan", m.plan);
    assert_eq!(drawn.len(), 1);
    assert_eq!(label_text(&drawn[0]), "1", "a door tag shows Mark");
    assert_eq!(drawn[0]["at"], json!({ "x": 2000.0, "y": 0.0 }), "no leader: the head sits on the door");
    // the label follows the Mark
    call(&mut m.s, "patch_instance", json!({ "id": door, "patch": { "mark": "G.01" } }));
    assert_eq!(label_text(&tags(&mut m.s, "plan", m.plan)[0]), "G.01");
    // the tag follows the door along its wall
    call(&mut m.s, "transform", json!({ "ids": [door], "kind": "move", "dx": 1000, "dy": 0 }));
    assert_eq!(tags(&mut m.s, "plan", m.plan)[0]["at"], json!({ "x": 3000.0, "y": 0.0 }));
    // moved alone, the tag keeps its new place relative to the door
    call(&mut m.s, "transform", json!({ "ids": [tag], "kind": "move", "dx": 0, "dy": 800 }));
    call(&mut m.s, "transform", json!({ "ids": [door], "kind": "move", "dx": -500, "dy": 0 }));
    assert_eq!(tags(&mut m.s, "plan", m.plan)[0]["at"], json!({ "x": 2500.0, "y": 800.0 }));
    // a pinned tag stays put
    call(&mut m.s, "patch_tag", json!({ "id": tag, "patch": { "pinned": true } }));
    assert!(m.s.handle("patch_tag", &json!({ "id": tag, "patch": { "offset": [0, 0] } })).is_err());
    // deleting the door deletes its tag; undo brings both back
    call(&mut m.s, "delete", json!({ "ids": [door] }));
    assert!(tags(&mut m.s, "plan", m.plan).is_empty());
    call(&mut m.s, "undo", json!({}));
    assert_eq!(tags(&mut m.s, "plan", m.plan)[0]["id"], tag);
}

#[test]
fn leader_orientation_family_and_properties() {
    let mut m = model();
    let wt = m.window_type.clone();
    let window = place(&mut m, &wt, 3000.0);
    let type_id = call(&mut m.s, "describe", json!({ "id": window }))["type_id"].clone();
    call(&mut m.s, "patch_family_type", json!({ "id": type_id, "patch": { "values": { "Type Mark": "W3" } } }));
    let tag = call(&mut m.s, "add_tag", json!({ "view_id": m.plan, "element_id": window, "options": { "leader": true, "leader_mm": 10, "orientation": "vertical" } }))["id"].as_u64().unwrap();
    let t = &tags(&mut m.s, "plan", m.plan)[0];
    assert_eq!(label_text(t), "1", "a window tag shows Mark, not Type Mark");
    assert_eq!(t["at"], json!({ "x": 3000.0, "y": 1000.0 }), "10 mm on paper at 1:100");
    assert_eq!((t["leader"].clone(), t["vertical"].clone()), (json!(true), json!(true)));
    let p = call(&mut m.s, "describe", json!({ "id": tag }));
    assert_eq!(p["title"], "Window Tag");
    let rows: Vec<String> = p["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
    for want in ["Leader Line", "Leader End", "Orientation", "Family", "Pinned", "Tagged"] { assert!(rows.contains(&want.to_string()), "{want} in {rows:?}"); }
    // Spacebar: one undo step back to horizontal
    call(&mut m.s, "flip_tags", json!({ "ids": [tag] }));
    assert_eq!(tags(&mut m.s, "plan", m.plan)[0]["vertical"], false);
    // a free leader end starts where the attached one was
    call(&mut m.s, "patch_tag", json!({ "id": tag, "patch": { "leader_end": "free" } }));
    assert_eq!(tags(&mut m.s, "plan", m.plan)[0]["end"], json!({ "x": 3000.0, "y": 0.0 }));
    // a door tag family cannot tag a window
    let dt = m.door_type.clone();
    let door = place(&mut m, &dt, 1000.0);
    let door_tag = call(&mut m.s, "add_tag", json!({ "view_id": m.plan, "element_id": door }))["id"].as_u64().unwrap();
    let door_family = call(&mut m.s, "describe", json!({ "id": door_tag }))["groups"][0]["rows"][3]["options"][0]["patch"]["family"].clone();
    assert!(m.s.handle("patch_tag", &json!({ "id": tag, "patch": { "family": door_family } })).is_err());
    // walls cannot be tagged yet
    assert!(m.s.handle("add_tag", &json!({ "view_id": m.plan, "element_id": m.wall })).is_err());
}

#[test]
fn tag_all_tags_only_untagged_elements_in_the_view_and_elevations_take_tags() {
    let mut m = model();
    let (dt, wt) = (m.door_type.clone(), m.window_type.clone());
    let d1 = place(&mut m, &dt, 1000.0);
    place(&mut m, &dt, 2500.0);
    place(&mut m, &wt, 4500.0);
    call(&mut m.s, "add_tag", json!({ "view_id": m.plan, "element_id": d1 }));
    let options = call(&mut m.s, "tag_all_options", json!({ "view_id": m.plan }));
    let untagged = |kind: &str| options.as_array().unwrap().iter().find(|o| o["kind"] == kind).unwrap()["untagged"].clone();
    assert_eq!((untagged("door_tag"), untagged("window_tag")), (json!(1), json!(1)));
    let r = call(&mut m.s, "tag_all", json!({ "view_id": m.plan, "kinds": [{ "kind": "door_tag" }, { "kind": "window_tag" }] }));
    assert_eq!(r["ids"].as_array().unwrap().len(), 2);
    assert_eq!(tags(&mut m.s, "plan", m.plan).len(), 3);
    // one undo step takes them all off; a second run after redo tags nothing
    call(&mut m.s, "undo", json!({}));
    assert_eq!(tags(&mut m.s, "plan", m.plan).len(), 1);
    call(&mut m.s, "redo", json!({}));
    assert!(call(&mut m.s, "tag_all", json!({ "view_id": m.plan, "kinds": [{ "kind": "door_tag" }, { "kind": "window_tag" }] }))["ids"].as_array().unwrap().is_empty());
    // the elevation tags its own, at the openings' centres
    call(&mut m.s, "tag_all", json!({ "view_id": m.south, "kinds": [{ "kind": "window_tag" }] }));
    let elev = tags(&mut m.s, "elevation", m.south);
    assert_eq!(elev.len(), 1);
    assert!(elev[0]["at"]["y"].as_f64().unwrap() > 900.0, "the window's centre is above its sill: {}", elev[0]["at"]);
}

#[test]
fn tags_copy_with_their_element_and_survive_save_and_reopen() {
    let mut m = model();
    let dt = m.door_type.clone();
    let door = place(&mut m, &dt, 1000.0);
    let tag = call(&mut m.s, "add_tag", json!({ "view_id": m.plan, "element_id": door, "options": { "leader": true } }))["id"].as_u64().unwrap();
    let copies = call(&mut m.s, "transform", json!({ "ids": [door, tag], "kind": "move", "dx": 2000, "dy": 0, "copy": true }))["ids"].clone();
    let drawn = tags(&mut m.s, "plan", m.plan);
    assert_eq!(drawn.len(), 2);
    let copied_tag = drawn.iter().find(|t| t["id"] != tag).unwrap();
    assert!(copies.as_array().unwrap().contains(&copied_tag["element"]), "the copied tag tags the copied door");
    assert_eq!(label_text(copied_tag), "2");
    let path = std::env::temp_dir().join(format!("openarch-tags-{}.oab", std::process::id()));
    call(&mut m.s, "save", json!({ "path": path }));
    let mut reopened = Session::new();
    call(&mut reopened, "open", json!({ "path": path }));
    std::fs::remove_file(&path).unwrap();
    let again = tags(&mut reopened, "plan", m.plan);
    assert_eq!(again.len(), 2);
    assert_eq!(again.iter().find(|t| t["id"] == tag).unwrap()["at"], drawn.iter().find(|t| t["id"] == tag).unwrap()["at"]);
}
