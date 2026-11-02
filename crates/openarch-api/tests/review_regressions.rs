use openarch_api::Session;
use openarch_model::section::{Section, SectionEnd};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn call(session: &mut Session, method: &str, params: Value) -> Value {
    session.handle(method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))
}

fn scratch_file(name: &str) -> PathBuf {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("openarch-review-{}-{stamp}-{name}", std::process::id()))
}

fn property_labels(properties: &Value) -> Vec<&str> {
    properties["groups"].as_array().unwrap().iter()
        .flat_map(|group| group["rows"].as_array().unwrap())
        .filter_map(|row| row["label"].as_str())
        .collect()
}

#[test]
fn section_ends_preserve_old_defaults_and_have_their_own_property_labels() {
    let mut session = Session::new();
    let info = call(&mut session, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let marker = session.doc.elevation_markers()[0].0;
    let properties = call(&mut session, "describe", json!({ "id": marker }));
    let labels = property_labels(&properties);
    assert!(!labels.contains(&"Section Head"));
    let elevation_type = properties["type_id"].as_u64().unwrap();
    let marker_type = session.doc.view_type(elevation_type).unwrap().marker_type.unwrap();
    let marker_properties = call(&mut session, "describe_any_type", json!({ "id": marker_type }));
    assert!(property_labels(&marker_properties).contains(&"Symbol"));

    let section = call(&mut session, "add_section", json!({
        "level_id": level, "start": [0, 0], "end": [4000, 0]
    }))["id"].as_u64().unwrap();
    let properties = call(&mut session, "describe", json!({ "id": section }));
    let labels = property_labels(&properties);
    assert!(!labels.contains(&"Section Head"));
    let section_type = properties["type_id"].as_u64().unwrap();
    let marker_type = session.doc.view_type(section_type).unwrap().marker_type.unwrap();
    let marker_properties = call(&mut session, "describe_any_type", json!({ "id": marker_type }));
    assert!(property_labels(&marker_properties).contains(&"Section Head"));
    assert!(property_labels(&marker_properties).contains(&"Section Tail"));

    let mut old = serde_json::to_value(session.doc.section(section).unwrap()).unwrap();
    for key in ["tail_family", "start_marker", "end_marker"] {
        old.as_object_mut().unwrap().remove(key);
    }
    let old: Section = serde_json::from_value(old).unwrap();
    assert_eq!(old.start_marker, SectionEnd::Head);
    assert_eq!(old.end_marker, SectionEnd::Tail);
    assert_eq!(old.tail_family, None);

    let head = session.doc.section_marker_family(None).unwrap().0;
    let tail = call(&mut session, "duplicate_annotation_family", json!({
        "id": head, "name": "Regression Tail"
    }))["id"].as_u64().unwrap();
    call(&mut session, "patch_section", json!({
        "id": section, "patch": { "tail_family": tail, "start_marker": "none", "end_marker": "head" }
    }));
    assert!(session.handle("delete_annotation_family", &json!({ "id": tail })).is_err());
    let path = scratch_file("section.oab");
    call(&mut session, "save", json!({ "path": path }));
    call(&mut session, "open", json!({ "path": path }));
    let saved = session.doc.section(section).unwrap();
    assert_eq!(saved.tail_family, Some(tail));
    assert_eq!(saved.start_marker, SectionEnd::None);
    assert_eq!(saved.end_marker, SectionEnd::Head);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sheet_annotations_use_paper_coordinates_and_follow_sheet_lifecycle() {
    let mut session = Session::new();
    let info = call(&mut session, "doc_info", json!({}));
    let sheet = call(&mut session, "add_sheet", json!({
        "number": "T101", "name": "Regression sheet", "title_block": info["title_blocks"][0]["id"]
    }))["id"].as_u64().unwrap();
    let line = call(&mut session, "set_detail_lines", json!({
        "view_id": sheet,
        "lines": [{ "curve": { "type": "line", "a": { "x": 20, "y": 30 }, "b": { "x": 100, "y": 30 } } }]
    }))["ids"][0].as_u64().unwrap();
    let note = call(&mut session, "add_text_note", json!({ "note": {
        "view": sheet, "at": { "x": 50, "y": 60 }, "text": "SHEET NOTE",
        "text_type": info["text_note_types"][0]["id"]
    } }))["id"].as_u64().unwrap();
    let corners = [[20, 40], [40, 40], [40, 50], [20, 50]];
    let edges: Vec<Value> = (0..4).map(|index| {
        let a = corners[index];
        let b = corners[(index + 1) % 4];
        json!({ "curve": {
            "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] }
        }, "subcategory": "Thin Lines" })
    }).collect();
    let region = call(&mut session, "add_detail_region", json!({
        "view_id": sheet, "edges": edges, "type_id": info["filled_region_types"][0]["id"]
    }))["id"].as_u64().unwrap();
    let drawing = call(&mut session, "sheet", json!({ "id": sheet }));
    let line_path = drawing["detail"]["strokes"].as_array().unwrap().iter()
        .find(|stroke| stroke["line"] == line).unwrap();
    assert_eq!(line_path["pts"][0], json!([20.0, 30.0]));
    assert_eq!(drawing["text_notes"][0]["text"]["x"], 50.0);
    assert_eq!(drawing["regions"].as_array().unwrap().len(), 1);
    let plan = info["views"].as_array().unwrap().iter()
        .find(|view| view["view_kind"] == "floor_plan").unwrap()["id"].clone();
    let viewport = call(&mut session, "place_view", json!({
        "sheet": sheet, "view": plan, "centre": [200, 200]
    }))["id"].clone();
    call(&mut session, "patch_viewport", json!({ "id": viewport, "patch": { "centre": [250, 220] } }));
    let moved = call(&mut session, "sheet", json!({ "id": sheet }));
    assert_eq!(drawing["detail"], moved["detail"]);
    assert_eq!(drawing["text_notes"], moved["text_notes"]);

    let copy = call(&mut session, "duplicate_sheet", json!({
        "id": sheet, "how": "with_detailing"
    }))["id"].as_u64().unwrap();
    let copied = call(&mut session, "sheet", json!({ "id": copy }));
    assert_eq!(copied["detail_lines"].as_array().unwrap().len(), 1);
    assert_eq!(copied["text_notes"].as_array().unwrap().len(), 1);
    assert_eq!(copied["regions"].as_array().unwrap().len(), 1);
    assert_ne!(copied["detail_lines"][0]["id"], line);

    let path = scratch_file("annotations.oab");
    call(&mut session, "save", json!({ "path": path }));
    call(&mut session, "open", json!({ "path": path }));
    assert_eq!(session.doc.text_notes(sheet)[0].1.at.x, 50.0);
    call(&mut session, "delete", json!({ "ids": [sheet] }));
    assert!(session.doc.get(line).is_none());
    assert!(session.doc.get(note).is_none());
    assert!(session.doc.get(region).is_none());
    assert_eq!(session.doc.text_notes(copy).len(), 1);
    call(&mut session, "undo", json!({}));
    assert!(session.doc.get(line).is_some());
    assert!(session.doc.get(note).is_some());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn annotation_files_round_trip_styles_and_failed_imports_roll_back() {
    let mut source = Session::new();
    let head = source.doc.section_marker_family(None).unwrap().0;
    call(&mut source, "patch_object_styles", json!({ "patch": { "annotation": {
        "Section Marks": { "subcategories": {
            "Regression Heavy": { "projection_weight": 5, "color": [20, 30, 40], "pattern": "Solid", "subcategories": {} }
        } }
    } } }));
    let mut family = call(&mut source, "annotation_family", json!({ "id": head }));
    family["name"] = json!("Portable Regression Head");
    family["lines"].as_array_mut().unwrap().push(json!({
        "curve": { "type": "line", "a": { "x": 0, "y": 0 }, "b": { "x": 2, "y": 0 } },
        "subcategory": "Regression Heavy", "layer": "body"
    }));
    let path = scratch_file("head.oaf");
    call(&mut source, "save_annotation_family_file", json!({ "id": head, "path": path, "family": family }));
    assert_ne!(source.doc.annotation_family(head).unwrap().name, "Portable Regression Head");

    let mut target = Session::new();
    let imported = call(&mut target, "import_family_file", json!({ "path": path }));
    assert_eq!(imported["annotation"], true);
    assert!(target.doc.symbol_subcategories(openarch_model::annotation::AnnotationKind::SectionMarker).contains(&"Regression Heavy".into()));
    call(&mut target, "undo", json!({}));
    assert!(!target.doc.symbol_subcategories(openarch_model::annotation::AnnotationKind::SectionMarker).contains(&"Regression Heavy".into()));

    let mut file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    file["family"]["lines"].as_array_mut().unwrap().last_mut().unwrap()["subcategory"] = json!("Unknown Style");
    std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();
    assert!(target.handle("import_family_file", &json!({ "path": path })).is_err());
    assert!(!target.doc.symbol_subcategories(openarch_model::annotation::AnnotationKind::SectionMarker).contains(&"Regression Heavy".into()));
    file["version"] = json!(999);
    std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();
    assert!(target.handle("family_file_info", &json!({ "path": path })).is_err());
    std::fs::remove_file(path).unwrap();
}
