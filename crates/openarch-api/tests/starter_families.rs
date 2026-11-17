//! door-window-spec.md step 5: every type of every starter door and window, placed in a wall, checked in plan at
//! each detail level, in elevation and section, in 3D, in the schedules and through IFC.
use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m}: {e}"))
}

fn section(s: &mut Session, level: u64, a: [f64; 2], b: [f64; 2]) -> u64 {
    let r = call(s, "add_section", json!({ "level_id": level, "start": a, "end": b }));
    r["view_id"].as_u64().or(r["id"].as_u64()).unwrap()
}

/// The elevation items of one element.
fn items_of(s: &mut Session, view: u64, id: u64) -> Vec<Value> {
    call(s, "elevation", json!({ "view_id": view }))["items"].as_array().unwrap().iter().filter(|i| i["id"].as_u64() == Some(id)).cloned().collect()
}

struct Placed {
    id: u64,
    label: String,
    door: bool,
    y: f64,
}

#[test]
fn every_starter_door_and_window_draws_in_every_view_schedules_and_exports() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].as_u64().unwrap();
    let plan = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() == Some(level)).unwrap()["id"].as_u64().unwrap();
    let wt = info["wall_types"].as_array().unwrap().iter().find(|t| t["name"] == "Brick Veneer").unwrap()["id"].as_u64().unwrap();
    let fams: Vec<Value> = info["families"].as_array().unwrap().clone();
    let wall_hosted = |fid: &Value| fams.iter().find(|f| &f["id"] == fid).is_some_and(|f| f["host"] == "wall");
    let types: Vec<Value> = info["family_types"].as_array().unwrap().iter().filter(|t| (t["category"] == "Doors" || t["category"] == "Windows") && wall_hosted(&t["family_id"])).cloned().collect();
    // a wall each, 3 m apart, placed from the wall's right (the -y side): the interior faces -y
    let mut placed: Vec<Placed> = vec![];
    for (k, t) in types.iter().enumerate() {
        let label = t["label"].as_str().unwrap().to_string();
        if label.starts_with("Curtain Wall Door") {
            continue;
        }
        let y = 3000.0 * k as f64;
        let w = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": level, "start": [0, y], "end": [8000, y], "height_mm": 3000, "join": false }))["id"].as_u64().unwrap();
        let id = s.handle("place_instance", &json!({ "type_id": t["id"], "host_id": w, "point": [4000, y - 300.0] })).unwrap_or_else(|e| panic!("{label}: {e}"))["id"].as_u64().unwrap();
        placed.push(Placed { id, label, door: t["category"] == "Doors", y });
    }
    // the curtain wall door in a storefront's first cell
    let sf = info["wall_types"].as_array().unwrap().iter().find(|t| t["name"] == "Storefront").unwrap()["id"].as_u64().unwrap();
    let cy = -3000.0;
    let cw = call(&mut s, "add_wall", json!({ "type_id": sf, "level_id": level, "start": [0, cy], "end": [3600, cy], "height_mm": 3000, "join": false }))["id"].as_u64().unwrap();
    let cwd = types.iter().find(|t| t["label"].as_str().unwrap().starts_with("Curtain Wall Door")).unwrap()["id"].as_u64().unwrap();
    s.doc.set_curtain_panel_pinned(cw, 0, 0, false).unwrap();
    s.doc.set_curtain_panel(cw, 0, 0, openarch_model::curtain::PanelChoice::Door(cwd)).unwrap();
    let cid = s.doc.curtain_doors(cw)[0].0;
    placed.push(Placed { id: cid, label: "Curtain Wall Door - Single Glazed: Glazed".into(), door: true, y: cy });
    assert!(placed.len() > 50, "{} types", placed.len());

    // nothing reports a problem solving
    for p in &placed {
        let solved = s.doc.instance_solved(p.id).unwrap();
        let sv = solved.as_ref().as_ref().unwrap_or_else(|e| panic!("{}: {e}", p.label));
        assert!(sv.warnings.is_empty(), "{}: {:?}", p.label, sv.warnings);
    }

    // plan at each detail level: every one draws, cut through or (a high window) dashed overhead; a door's open leaf
    // and arc are there at every level
    for level_name in ["coarse", "medium", "fine"] {
        call(&mut s, "patch_view", json!({ "id": plan, "patch": { "settings": { "detail_level": level_name } } }));
        let data = call(&mut s, "plan", json!({ "view_id": plan }));
        let inserts = data["inserts"].as_array().unwrap();
        for p in &placed {
            let ins = inserts.iter().find(|i| i["id"].as_u64() == Some(p.id)).unwrap_or_else(|| panic!("{} not in the {level_name} plan", p.label));
            let parts = ins["parts"].as_array().unwrap();
            assert!(!parts.is_empty(), "{} draws nothing at {level_name}", p.label);
            assert!(ins["display"] == "cut" || ins["display"] == "overhead", "{} {level_name}: {}", p.label, ins["display"]);
            assert!(!parts.iter().any(|x| x["sub"] == "Panel" && x["closed"] == true && x["rings"][0].as_array().is_some_and(|r| r.len() > 5)), "{} closed leaf in plan", p.label);
            if p.door && !p.label.starts_with("Garage") {
                assert!(parts.iter().any(|x| x["sub"] == "Plan Swing"), "{} {level_name}: no leaf in plan", p.label);
            }
            // a window's glazing line at Coarse and Medium; at Fine what the cut plane meets in section: its panes, or
            // its sashes (a double hung's meeting rails) or frame (a sill at the cut plane)
            if !p.door {
                let fine_ok = |x: &Value| level_name == "fine" && (x["sub"] == "Sash" || x["sub"] == "Frame/Mullion");
                assert!(parts.iter().any(|x| x["sub"] == "Glass" || fine_ok(x)), "{} {level_name}: no glass in plan", p.label);
            }
        }
    }

    // a section along the wall of one type of each family, 1 m in front on its exterior (+y) side looking at it,
    // shows the door or window as seen with its swing lines or arrows (Medium); a section across it cuts it
    call(&mut s, "patch_view", json!({ "id": plan, "patch": { "settings": { "detail_level": "medium" } } }));
    let family = |p: &Placed| p.label.split(": ").next().unwrap().to_string();
    let mut seen_families: Vec<String> = vec![];
    for p in &placed {
        if seen_families.contains(&family(p)) {
            continue;
        }
        seen_families.push(family(p));
        let along = section(&mut s, level, [8000.0, p.y + 1000.0], [0.0, p.y + 1000.0]);
        let seen = items_of(&mut s, along, p.id);
        if seen.is_empty() {
            panic!("{}: a section 1 m out on the exterior, looking at the wall, sees nothing", p.label);
        }
        assert!(seen.iter().any(|i| i["cut"] == false && !i["polys"].as_array().unwrap().is_empty()), "{} not seen in elevation", p.label);
        let opens = !(p.label.starts_with("Fixed") || p.label.starts_with("Louvre") || p.label.starts_with("Garage Door - Roller"));
        let lined = seen.iter().any(|i| !i["lines"].as_array().unwrap().is_empty());
        assert_eq!(lined, opens, "{} elevation lines", p.label);
        let x = if p.label.starts_with("Curtain") { 612.5 } else { 4000.0 };
        let across = section(&mut s, level, [x, p.y + 1500.0], [x, p.y - 1500.0]);
        let cut = items_of(&mut s, across, p.id);
        assert!(cut.iter().any(|i| i["cut"] == true), "{} not cut in section", p.label);
    }
    // swing lines draw dashed in their subcategory's pattern
    let hinged = placed.iter().find(|p| p.label.starts_with("Single Flush Door - Metal Frame")).unwrap();
    let along = section(&mut s, level, [8000.0, hinged.y + 1000.0], [0.0, hinged.y + 1000.0]);
    let swing = items_of(&mut s, along, hinged.id).into_iter().find(|i| i["sub"] == "Elevation Swing").expect("the swing lines");
    assert_ne!(swing["style"]["line"]["pattern"], "Solid");

    // 3D: every one has solids, and the windows and glazed doors glass
    for p in &placed {
        let (_, solid, glass) = openarch_model::mesh::instance_meshes(&s.doc, p.id).unwrap();
        assert!(!solid.is_empty(), "{} has no 3D", p.label);
        let glazed = !p.door || ["Glazed", "Sliding Glass", "Stacking", "Bifold", "Entry", "Curtain"].iter().any(|k| p.label.contains(k));
        assert_eq!(!glass.is_empty(), glazed, "{} glass in 3D", p.label);
    }

    // schedules list them all
    let (doors, windows) = (placed.iter().filter(|p| p.door).count(), placed.iter().filter(|p| !p.door).count());
    let sch = call(&mut s, "schedule", json!({}));
    assert_eq!((sch["doors"]["count"].as_u64(), sch["windows"]["count"].as_u64()), (Some(doors as u64), Some(windows as u64)));

    // IFC: an IfcDoor or IfcWindow each, and they come back (all but the curtain wall door, which the importer leaves
    // as glass in its cell for now)
    let text = s.doc.export_ifc("Starter families");
    let count = |k: &str| text.matches(k).count();
    assert_eq!((count("=IFCDOOR("), count("=IFCWINDOW(")), (doors, windows));
    let (_, report) = openarch_model::Document::from_ifc(text.as_bytes()).unwrap();
    assert_eq!((report.counts["doors"], report.counts["windows"]), (doors - 1, windows), "{report:?}");
    assert!(report.skipped.len() == 1 && report.skipped[0].contains("Curtain Wall Door"), "{:?}", report.skipped);
    assert_eq!(seen_families.len(), 21);
}
