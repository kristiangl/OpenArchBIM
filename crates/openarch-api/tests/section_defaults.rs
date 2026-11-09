use openarch_api::Session;
use openarch_model::annotation::AnnotationFamily;
use openarch_model::view::ViewKind;
use serde_json::json;

#[test]
fn supplied_section_artwork_is_the_new_project_default_and_custom_files_stay_custom() {
    let mut session = Session::new();
    let head = session.doc.annotation_families().into_iter().find(|(_, f)| f.name == "Section Head").unwrap().0;
    let tail = session.doc.annotation_families().into_iter().find(|(_, f)| f.name == "Section Tail").unwrap().0;
    // the supplied artwork (Kristian's Section Head and Section Tail, 7 Nov) as it is in the files
    for (id, name, data) in [
        (head, "Section Head", include_str!("../../openarch-model/families/Section Head.oaf")),
        (tail, "Section Tail", include_str!("../../openarch-model/families/Section Tail.oaf")),
    ] {
        let file: serde_json::Value = serde_json::from_str(data).unwrap();
        let mut expected: AnnotationFamily = serde_json::from_value(file["family"].clone()).unwrap();
        expected.name = name.into();
        let family = session.doc.annotation_family(id).unwrap();
        assert_eq!(family, &expected);
        let drawing = session.doc.symbol_drawing(family, false);
        assert!(!drawing.pointer.fills.is_empty(), "{name} has its arrow");
        assert!(session.doc.delete_annotation_family(id).is_err(), "default marker type protects its symbols");
    }
    let view_type = session.doc.default_view_type(ViewKind::Section).unwrap();
    let marker = session.doc.view_type(view_type).unwrap().marker_type.unwrap();
    assert_eq!((session.doc.view_marker_type(marker).unwrap().head, session.doc.view_marker_type(marker).unwrap().tail), (Some(head), Some(tail)));
    let plan = session.doc.views().into_iter().find(|(_, v)| v.view_kind == ViewKind::FloorPlan).unwrap().0;
    let drawing = session.handle("plan", &json!({ "view_id": plan })).unwrap();
    assert_eq!(drawing["section_family"]["id"], head);
    assert_eq!(drawing["section_tail_family"]["id"], tail);
    let level = session.doc.levels()[0].0;
    let section = session.handle("add_section", &json!({ "level_id": level, "start": [0,0], "end": [4000,0] })).unwrap()["id"].as_u64().unwrap();
    assert_eq!(session.doc.section_symbols(session.doc.section(section).unwrap()), (Some(head), Some(tail)));
    for transform in [
        json!({ "ids": [section], "kind": "move", "dx": 500, "dy": 300 }),
        json!({ "ids": [section], "kind": "rotate", "cx": 0, "cy": 0, "angle_deg": 37 }),
        json!({ "ids": [section], "kind": "move", "copy": true, "dx": 0, "dy": 500 }),
    ] {
        let result = session.handle("transform", &transform).unwrap();
        let id = result["ids"][0].as_u64().unwrap();
        assert_eq!(session.doc.section_symbols(session.doc.section(id).unwrap()), (Some(head), Some(tail)));
        assert_eq!(session.doc.section_symbols(session.doc.section(section).unwrap()), (Some(head), Some(tail)));
    }
    let mut custom = session.doc.annotation_family(head).unwrap().clone();
    custom.name = "My edited section head".into();
    session.doc.set_annotation_family(head, custom.clone()).unwrap();
    let path = std::env::temp_dir().join(format!("openarch-default-marker-{}-{}.oab", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    session.handle("save", &json!({ "path": path })).unwrap();
    session.handle("open", &json!({ "path": path })).unwrap();
    assert_eq!(session.doc.annotation_family(head), Some(&custom));
    assert_eq!(session.doc.section_symbols(session.doc.section(section).unwrap()), (Some(head), Some(tail)));
    std::fs::remove_file(path).unwrap();
}
