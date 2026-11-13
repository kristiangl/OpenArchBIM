//! View filters through the API: rule and selection filters, what they match, and what a view that applies them
//! draws (overrides by the order of the list, Visibility and Enable).
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

struct Setup {
    s: Session,
    view: u64,
    big: u64,
    small: u64,
}

/// Two floors on the first level: one of 20 m² and one of 6 m².
fn setup() -> Setup {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let ft = info["floor_types"][0]["id"].as_u64().unwrap();
    let view = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() == Some(level)).unwrap()["id"].as_u64().unwrap();
    let big = call(&mut s, "add_floor", json!({ "type_id": ft, "level_id": level, "points": [[0, 0], [5000, 0], [5000, 4000], [0, 4000]] }))["id"].as_u64().unwrap();
    let small = call(&mut s, "add_floor", json!({ "type_id": ft, "level_id": level, "points": [[8000, 0], [11000, 0], [11000, 2000], [8000, 2000]] }))["id"].as_u64().unwrap();
    Setup { s, view, big, small }
}

fn floor(s: &mut Session, view: u64, id: u64) -> Value {
    call(s, "plan", json!({ "view_id": view }))["floors"].as_array().unwrap().iter().find(|f| f["id"].as_u64() == Some(id)).cloned().unwrap_or(Value::Null)
}

fn line(color: [u8; 3], weight: Option<u8>) -> Value {
    json!({ "projection_lines": { "color": color, "weight": weight, "pattern": null } })
}

#[test]
fn a_rule_filter_colours_and_hides_what_it_matches() {
    let Setup { mut s, view, big, small } = setup();
    // floors over 10 m²
    let rules = json!({ "name": "Big floors", "kind": "rules", "categories": ["Floors"], "root": { "node": "and", "of": [{ "node": "rule", "field": "Area", "op": "greater", "value": "10" }] } });
    assert_eq!(call(&mut s, "view_filter_matches", json!({ "filter": rules }))["ids"], json!([big]));
    let f = call(&mut s, "add_view_filter", json!({ "filter": rules }))["id"].as_u64().unwrap();
    assert!(s.handle("add_view_filter", &json!({ "filter": rules })).is_err(), "names are unique");
    let fields = call(&mut s, "filter_fields", json!({ "categories": ["Floors", "Walls"] }));
    let names: Vec<&str> = fields["fields"].as_array().unwrap().iter().filter_map(|f| f["name"].as_str()).collect();
    assert!(names.contains(&"Fire Rating") && names.contains(&"Type") && !names.contains(&"Slope"), "only what floors and walls share: {names:?}");

    let plain = floor(&mut s, view, big)["style"]["line"]["color"].clone();
    call(&mut s, "set_view_filter_list", json!({ "view_id": view, "filters": [{ "filter": f, "overrides": line([255, 0, 0], None) }] }));
    let red = floor(&mut s, view, big)["style"]["line"]["color"].clone();
    assert_ne!(red, plain);
    assert_eq!(floor(&mut s, view, small)["style"]["line"]["color"], plain, "the small floor doesn't match");
    let listed = call(&mut s, "view_filter_list", json!({ "view_id": view }));
    assert_eq!((listed["filters"][0]["name"].as_str(), listed["filters"][0]["enabled"].as_bool(), listed["locked"].as_bool()), (Some("Big floors"), Some(true), Some(false)));

    // the model changes and the filter follows: the small floor grows past 10 m²
    call(&mut s, "delete", json!({ "ids": [small] }));
    call(&mut s, "undo", json!({}));
    assert_eq!(floor(&mut s, view, small)["style"]["line"]["color"], plain);

    // Visibility off hides what matches: gone from the plan and from the 3D scene, the other floor stays
    call(&mut s, "set_view_filter_list", json!({ "view_id": view, "filters": [{ "filter": f, "visible": false }] }));
    assert!(floor(&mut s, view, big).is_null() && !floor(&mut s, view, small).is_null());
    // Enable off skips the filter but keeps it in the list
    call(&mut s, "set_view_filter_list", json!({ "view_id": view, "filters": [{ "filter": f, "visible": false, "enabled": false }] }));
    assert!(!floor(&mut s, view, big).is_null());
    assert_eq!(call(&mut s, "view_filter_list", json!({ "view_id": view }))["filters"].as_array().unwrap().len(), 1);

    // deleting the filter takes it out of the view; undo puts both back
    call(&mut s, "delete_view_filter", json!({ "id": f }));
    assert_eq!(call(&mut s, "view_filter_list", json!({ "view_id": view }))["filters"].as_array().unwrap().len(), 0);
    call(&mut s, "undo", json!({}));
    assert_eq!(call(&mut s, "view_filters", json!({}))["filters"][0]["used_in"], 1);
}

#[test]
fn the_higher_filter_wins_a_property_and_an_element_override_wins_over_both() {
    let Setup { mut s, view, big, small } = setup();
    let rules = json!({ "name": "Big floors", "kind": "rules", "categories": ["Floors"], "root": { "node": "and", "of": [{ "node": "rule", "field": "Area", "op": "greater", "value": "10" }] } });
    let a = call(&mut s, "add_view_filter", json!({ "filter": rules }))["id"].as_u64().unwrap();
    let b = call(&mut s, "add_view_filter", json!({ "filter": { "name": "Checked", "kind": "selection", "ids": [big, small] } }))["id"].as_u64().unwrap();
    let style = |s: &mut Session, id: u64| floor(s, view, id)["style"]["line"].clone();
    let plain_weight = style(&mut s, small)["weight"].clone();

    // A (red) above B (blue, weight 7): the big floor is red with weight 7, the small one blue with weight 7
    call(&mut s, "set_view_filter_list", json!({ "view_id": view, "filters": [{ "filter": a, "overrides": line([255, 0, 0], None) }, { "filter": b, "overrides": line([0, 0, 255], Some(7)) }] }));
    let (rb, rs) = (style(&mut s, big), style(&mut s, small));
    assert_ne!(rb["color"], rs["color"]);
    assert_eq!((rb["weight"].as_u64(), rs["weight"].as_u64()), (Some(7), Some(7)));
    assert_ne!(plain_weight, json!(7));
    let blue = rs["color"].clone();
    // the other way up, B wins the colour of both
    call(&mut s, "set_view_filter_list", json!({ "view_id": view, "filters": [{ "filter": b, "overrides": line([0, 0, 255], Some(7)) }, { "filter": a, "overrides": line([255, 0, 0], None) }] }));
    assert_eq!(style(&mut s, big)["color"], blue);
    // an override on the element itself wins over the filters
    call(&mut s, "set_element_overrides", json!({ "view_id": view, "ids": [big], "override": line([0, 128, 0], None) }));
    let green = style(&mut s, big);
    assert_ne!(green["color"], blue);
    assert_eq!(green["weight"].as_u64(), Some(7), "the weight still comes from the filter");
}

#[test]
fn templates_are_copied_stamped_on_once_and_put_on_temporarily() {
    let Setup { mut s, view, big, .. } = setup();
    call(&mut s, "patch_view", json!({ "id": view, "patch": { "settings": { "scale": 50 } } }));
    let t = call(&mut s, "create_view_template", json!({ "name": "Check", "view_id": view }))["id"].as_u64().unwrap();
    // the template hides big floors through a filter of its own
    let rules = json!({ "name": "Big floors", "kind": "rules", "categories": ["Floors"], "root": { "node": "and", "of": [{ "node": "rule", "field": "Area", "op": "greater", "value": "10" }] } });
    let f = call(&mut s, "add_view_filter", json!({ "filter": rules }))["id"].as_u64().unwrap();
    call(&mut s, "set_view_filter_list", json!({ "template_id": t, "filters": [{ "filter": f, "visible": false }] }));
    assert_eq!(call(&mut s, "view_filter_list", json!({ "template_id": t }))["filters"][0]["name"], "Big floors");
    let info = call(&mut s, "view_templates_info", json!({}));
    assert_eq!((info["templates"][0]["values"]["scale"].as_str(), info["templates"][0]["values"]["vg_filters"].as_str(), info["templates"][0]["used_by"].as_u64()), (Some("1 : 50"), Some("1 filter"), Some(0)));

    // the view as saved shows the big floor; with the template on temporarily it doesn't; then it does again
    assert!(!floor(&mut s, view, big).is_null());
    call(&mut s, "set_temp_view_template", json!({ "view_id": view, "template_id": t }));
    assert_eq!(call(&mut s, "temp_view_template", json!({ "view_id": view }))["template_id"].as_u64(), Some(t));
    assert!(floor(&mut s, view, big).is_null(), "the temporary template's filter hides it");
    assert_eq!(call(&mut s, "clear_temp_view_templates", json!({}))["cleared"], 1);
    assert!(!floor(&mut s, view, big).is_null());

    // stamped on once: the view takes the filter list and stays free of the template
    assert!(call(&mut s, "apply_view_template_once", json!({ "view_id": view, "template_id": t }))["taken"].as_u64().unwrap() > 5);
    assert!(floor(&mut s, view, big).is_null());
    assert_eq!(call(&mut s, "view_filter_list", json!({ "view_id": view }))["locked"], false);
    let copy = call(&mut s, "duplicate_view_template", json!({ "id": t }))["id"].as_u64().unwrap();
    assert_eq!(call(&mut s, "view_templates_info", json!({}))["templates"].as_array().unwrap().iter().find(|x| x["id"].as_u64() == Some(copy)).unwrap()["name"], "Check Copy 1");
}
