//! Temporary Hide/Isolate, Hide in View, Reveal Hidden Elements and Override Graphics in View by element.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

struct M { s: Session, plan: u64, walls: Vec<u64>, door: u64, note: u64 }

/// A 6000 x 4000 room of walls with a door in the south wall and a text note.
fn model() -> M {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let mut levels: Vec<(u64, f64)> = info["levels"].as_array().unwrap().iter().map(|l| (l["id"].as_u64().unwrap(), l["elevation_mm"].as_f64().unwrap())).collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    let l1 = levels[0].0;
    let plan = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"] == l1).unwrap()["id"].as_u64().unwrap();
    let wt = info["wall_types"][0]["id"].clone();
    let walls: Vec<u64> = [([0, 0], [6000, 0]), ([6000, 0], [6000, 4000]), ([6000, 4000], [0, 4000]), ([0, 4000], [0, 0])].iter()
        .map(|(a, b)| call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": l1, "start": a, "end": b, "height_mm": 2700 }))["id"].as_u64().unwrap()).collect();
    let ty = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Single-Flush: 820 x 2040").unwrap()["id"].clone();
    let door = call(&mut s, "place_instance", json!({ "type_id": ty, "host_id": walls[0], "point": [2000, 0] }))["id"].as_u64().unwrap();
    let note = call(&mut s, "add_text_note", json!({ "note": { "view": plan, "at": { "x": 1000, "y": 1000 }, "text": "STORE", "text_type": info["text_note_types"][0]["id"] } }))["id"].as_u64().unwrap();
    M { s, plan, walls, door, note }
}

/// What the plan draws: walls, doors and windows, and whether the note shows.
fn drawn(m: &mut M) -> (usize, usize, bool, Value) {
    let p = call(&mut m.s, "plan", json!({ "view_id": m.plan }));
    let walls = p["walls"].as_array().unwrap().len();
    let inserts = p["inserts"].as_array().map_or(0, |a| a.len());
    let note = p["text_notes"].to_string().contains("STORE");
    (walls, inserts, note, p)
}

#[test]
fn temporary_hide_and_isolate_change_only_the_drawing() {
    let mut m = model();
    let (w, i, n, p) = drawn(&mut m);
    assert_eq!((w, i, n), (4, 1, true));
    assert_eq!(p["display"]["temporary"], false);

    // Hide Element
    call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "hide_elements", "ids": [m.door] }));
    let (w, i, n, p) = drawn(&mut m);
    assert_eq!((w, i, n), (4, 0, true));
    assert_eq!(p["display"]["temporary"], true);
    // not an undo step, and the view itself is unchanged
    assert!(m.s.handle("undo", &json!({})).is_ok());
    assert_eq!(drawn(&mut m).1, 0, "undo leaves the temporary hide alone");
    call(&mut m.s, "redo", json!({}));

    // Isolate Element: the one wall only
    call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "isolate_elements", "ids": [m.walls[1]] }));
    let (w, i, n, p) = drawn(&mut m);
    assert_eq!((w, i, n), (1, 0, false));
    assert_eq!(p["display"]["isolated"], true);

    // Reset, then Isolate Category (walls): no door, no note
    call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "reset" }));
    assert_eq!(drawn(&mut m).0, 4);
    call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "isolate_categories", "ids": [m.walls[0]] }));
    let (w, i, n, _) = drawn(&mut m);
    assert_eq!((w, i, n), (4, 0, false));

    // Hide Category (doors) only
    call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "reset" }));
    call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "hide_categories", "ids": [m.door] }));
    let (w, i, n, _) = drawn(&mut m);
    assert_eq!((w, i, n), (4, 0, true));

    // Apply Hide/Isolate to View makes it stick, as one undo step
    call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "reset" }));
    call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "hide_elements", "ids": [m.note] }));
    let st = call(&mut m.s, "temp_hide", json!({ "view_id": m.plan, "op": "apply" }));
    assert_eq!(st["temporary"], false);
    let (w, i, n, _) = drawn(&mut m);
    assert_eq!((w, i, n), (4, 1, false));
    call(&mut m.s, "undo", json!({}));
    assert!(drawn(&mut m).2, "undo brings the note back");

}

#[test]
fn hide_in_view_and_reveal_hidden_elements() {
    let mut m = model();
    // Hide in View > Elements, then > Category (walls)
    call(&mut m.s, "hide_elements", json!({ "view_id": m.plan, "ids": [m.door] }));
    let cats = call(&mut m.s, "hide_categories", json!({ "view_id": m.plan, "ids": [m.walls[0]] }));
    assert_eq!(cats["categories"], json!(["Walls"]));
    let (w, i, n, _) = drawn(&mut m);
    assert_eq!((w, i, n), (0, 0, true));

    // Reveal Hidden Elements: shown again, in the reveal colour, and listed
    let st = call(&mut m.s, "reveal_hidden", json!({ "view_id": m.plan, "on": true }));
    assert_eq!(st["reveal"], true);
    let (w, i, _, p) = drawn(&mut m);
    assert_eq!((w, i), (4, 1));
    let revealed: Vec<u64> = p["display"]["revealed"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert!(revealed.contains(&m.door) && m.walls.iter().all(|w| revealed.contains(w)));
    assert!(!revealed.contains(&m.note));
    assert!(p["walls"].to_string().contains("#d200a0"), "walls drawn in the reveal colour");

    // Unhide in View > Category and Elements
    call(&mut m.s, "hide_categories", json!({ "view_id": m.plan, "ids": [m.walls[0]], "hide": false }));
    call(&mut m.s, "hide_elements", json!({ "view_id": m.plan, "ids": [m.door], "hide": false }));
    let p = drawn(&mut m).3;
    assert!(p["display"]["revealed"].as_array().unwrap().is_empty());
    call(&mut m.s, "reveal_hidden", json!({ "view_id": m.plan, "on": false }));
    let (w, i, n, p) = drawn(&mut m);
    assert_eq!((w, i, n), (4, 1, true));
    assert!(!p["walls"].to_string().contains("#d200a0"));
}

#[test]
fn override_graphics_by_element() {
    let mut m = model();
    let over = json!({ "projection_lines": { "color": [255, 0, 0], "weight": 6 }, "cut_lines": { "color": [255, 0, 0], "weight": 9 }, "halftone": true });
    call(&mut m.s, "set_element_overrides", json!({ "view_id": m.plan, "ids": [m.walls[0], m.walls[1]], "override": over }));
    let got = call(&mut m.s, "element_overrides", json!({ "view_id": m.plan, "ids": [m.walls[1]] }));
    assert_eq!((got["set"].clone(), got["override"]["cut_lines"]["weight"].clone()), (json!(true), json!(9)));
    let p = drawn(&mut m).3;
    let walls = p["walls"].as_array().unwrap();
    let red = walls.iter().filter(|w| w.to_string().contains("#ff0000")).count();
    assert_eq!(red, 2, "two walls overridden");
    // reset clears it, one undo step each
    call(&mut m.s, "set_element_overrides", json!({ "view_id": m.plan, "ids": [m.walls[0], m.walls[1]], "override": null }));
    assert!(!drawn(&mut m).3["walls"].to_string().contains("#ff0000"));
    call(&mut m.s, "undo", json!({}));
    assert!(drawn(&mut m).3["walls"].to_string().contains("#ff0000"));
}
