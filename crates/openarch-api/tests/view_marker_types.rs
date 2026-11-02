use openarch_api::Session;
use openarch_model::{Element, Point};
use openarch_model::view::{ViewKind, ViewType};
use serde_json::{json, Value};

fn call(session: &mut Session, method: &str, params: Value) -> Value {
    session.handle(method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))
}

fn section(session: &mut Session, y: f64) -> u64 {
    let level = session.doc.levels()[0].0;
    session.doc.add_section(level, Point::new(0.0, y), Point::new(4000.0, y), false).unwrap()
}

#[test]
fn section_grips_keep_the_cut_plane_while_align_and_rotate_change_it() {
    let mut session = Session::new();
    let id = section(&mut session, 0.0);
    call(&mut session, "move_section_end", json!({ "id": id, "end": 1, "point": [6000, 2500] }));
    let s = session.doc.section(id).unwrap();
    assert_eq!(s.start, Point::new(0.0, 0.0));
    assert_eq!(s.end, Point::new(6000.0, 0.0));
    call(&mut session, "move_section_end", json!({ "id": id, "end": 0, "point": [9000, 1000] }));
    let s = session.doc.section(id).unwrap();
    assert!((s.length() - 100.0).abs() < 1e-6);
    assert_eq!(s.start.y, 0.0);
    call(&mut session, "undo", json!({}));

    call(&mut session, "align", json!({
        "ids": [id], "reference": [[1000, 2000], [5000, 6000]], "target": [[0, 0], [6000, 0]]
    }));
    let s = session.doc.section(id).unwrap();
    assert!(((s.end.y - s.start.y) - (s.end.x - s.start.x)).abs() < 1e-6);
    assert!((s.start.y - s.start.x - 1000.0).abs() < 1e-6);
    let (start, end) = (s.start, s.end);
    call(&mut session, "move_section_end", json!({ "id": id, "end": 1, "point": [10000, -5000] }));
    let s = session.doc.section(id).unwrap();
    let cross = (s.end.x - start.x) * (end.y - start.y) - (s.end.y - start.y) * (end.x - start.x);
    assert!(cross.abs() < 1e-5);
    call(&mut session, "transform", json!({ "ids": [id], "kind": "rotate", "cx": start.x, "cy": start.y, "angle_deg": 30 }));
    let s = session.doc.section(id).unwrap();
    let angle = (s.end.y - s.start.y).atan2(s.end.x - s.start.x).to_degrees();
    assert!((angle - 75.0).abs() < 1e-6);
}

#[test]
fn section_selection_edits_its_view_and_reusable_marker_types_control_all_views() {
    let mut session = Session::new();
    let a = section(&mut session, 0.0);
    let b = section(&mut session, 1000.0);
    let c = section(&mut session, 2000.0);
    let av = session.doc.section(a).unwrap().view.unwrap();
    let cv = session.doc.section(c).unwrap().view.unwrap();
    let view_type = session.doc.view_type_of(session.doc.view(av).unwrap()).unwrap();
    let marker_type = session.doc.view_type(view_type).unwrap().marker_type.unwrap();
    let original_tail = session.doc.view_marker_type(marker_type).unwrap().tail;
    let head = session.doc.section_marker_family(None).unwrap().0;
    let tail = call(&mut session, "duplicate_annotation_family", json!({ "id": head, "name": "Test Tail" }))["id"].as_u64().unwrap();
    let independent_tag = call(&mut session, "duplicate_view_marker_type", json!({ "id": marker_type, "name": "Independent Section Tag" }))["id"].as_u64().unwrap();
    let independent_view = call(&mut session, "duplicate_view_type", json!({ "id": view_type, "name": "Detail Section" }))["id"].as_u64().unwrap();
    call(&mut session, "patch_view_type", json!({ "id": independent_view, "patch": { "marker_type": independent_tag } }));
    call(&mut session, "patch_view", json!({ "id": cv, "patch": { "view_type": independent_view } }));
    call(&mut session, "patch_view_marker_type", json!({ "id": marker_type, "patch": { "tail": tail } }));
    assert_eq!(session.doc.section_symbols(session.doc.section(a).unwrap()).1, Some(tail));
    assert_eq!(session.doc.section_symbols(session.doc.section(b).unwrap()).1, Some(tail));
    assert_eq!(session.doc.section_symbols(session.doc.section(c).unwrap()).1, original_tail);

    let properties = call(&mut session, "describe", json!({ "id": a }));
    assert_eq!(properties["kind"], "View");
    assert_eq!(properties["target"]["id"], av);
    assert_eq!(properties["type_id"], view_type);
    call(&mut session, "patch_view", json!({ "id": av, "patch": { "name": "View edited through its section line" } }));
    assert_eq!(session.doc.view(av).unwrap().name, "View edited through its section line");
    let copy = call(&mut session, "transform", json!({ "ids": [c], "kind": "move", "copy": true, "dx": 1000, "dy": 1000 }))["ids"][0].as_u64().unwrap();
    let copy_view = session.doc.section(copy).unwrap().view.unwrap();
    assert_eq!(session.doc.view_type_of(session.doc.view(copy_view).unwrap()), Some(independent_view));
}

#[test]
fn elevation_marker_slots_share_the_selected_view_type_and_symbol() {
    let mut session = Session::new();
    let level = session.doc.levels()[0].0;
    let marker = session.doc.add_elevation_marker(level, Point::new(1000.0, 2000.0), 0.0, [true, true, false, false]).unwrap();
    let views: Vec<_> = session.doc.elevation_marker(marker).unwrap().slots.iter().flatten().copied().collect();
    let view_type = session.doc.view_type_of(session.doc.view(views[0]).unwrap()).unwrap();
    let tag = session.doc.view_type(view_type).unwrap().marker_type.unwrap();
    let new_type = session.doc.duplicate_view_type(view_type, "Interior Elevation").unwrap();
    let new_tag = session.doc.duplicate_view_marker_type(tag, "Interior Elevation Mark").unwrap();
    session.doc.patch_view_type(new_type, &json!({ "marker_type": new_tag })).unwrap();
    session.doc.patch_view(views[0], &json!({ "view_type": new_type })).unwrap();
    assert!(views.iter().all(|id| session.doc.view_type_of(session.doc.view(*id).unwrap()) == Some(new_type)));
    session.doc.set_marker_slot(marker, 2, true).unwrap();
    let added = session.doc.elevation_marker(marker).unwrap().slots[2].unwrap();
    assert_eq!(session.doc.view_type_of(session.doc.view(added).unwrap()), Some(new_type));
    let original = session.doc.marker_family(None).unwrap().0;
    let symbol = session.doc.duplicate_annotation_family(original, "Interior Elevation Symbol").unwrap();
    session.doc.patch_view_marker_type(new_tag, &json!({ "head": symbol })).unwrap();
    assert_eq!(session.doc.elevation_symbol(session.doc.elevation_marker(marker).unwrap()), Some(symbol));
    let section_tag = session.doc.default_view_type(ViewKind::Section).unwrap();
    assert!(session.doc.patch_view_type(new_type, &json!({ "marker_type": session.doc.view_type(section_tag).unwrap().marker_type })).is_err());
}

#[test]
fn legacy_instance_symbols_migrate_without_merging_different_drawings() {
    let mut session = Session::new();
    let a = section(&mut session, 0.0);
    let b = section(&mut session, 1000.0);
    let head = session.doc.section_marker_family(None).unwrap().0;
    let custom = session.doc.duplicate_annotation_family(head, "Legacy Custom Section Head").unwrap();
    let mut changed = session.doc.section(b).unwrap().clone();
    changed.family = Some(custom);
    let types: Vec<_> = session.doc.view_types().into_iter().map(|(id, t)| (id, t.clone())).collect();
    let markers: Vec<_> = session.doc.view_marker_types().iter().map(|(id, _)| *id).collect();
    session.doc.transact("Legacy fixture", |tx| {
        tx.update(b, Element::Section(changed))?;
        for (id, t) in types {
            let mut old = serde_json::to_value(t).unwrap();
            old.as_object_mut().unwrap().remove("marker_type");
            let old: ViewType = serde_json::from_value(old).unwrap();
            tx.update(id, Element::ViewType(old))?;
        }
        for id in markers { tx.remove(id)?; }
        Ok(())
    }).unwrap();
    session.doc.ensure_view_marker_types();
    assert_eq!(session.doc.section_symbols(session.doc.section(a).unwrap()).0, Some(head));
    assert_eq!(session.doc.section_symbols(session.doc.section(b).unwrap()).0, Some(custom));
    let view_a = session.doc.section(a).unwrap().view.unwrap();
    let view_b = session.doc.section(b).unwrap().view.unwrap();
    assert_ne!(session.doc.view_type_of(session.doc.view(view_a).unwrap()), session.doc.view_type_of(session.doc.view(view_b).unwrap()));
    let count = session.doc.view_marker_types().len();
    session.doc.ensure_view_marker_types();
    assert_eq!(session.doc.view_marker_types().len(), count);
}
