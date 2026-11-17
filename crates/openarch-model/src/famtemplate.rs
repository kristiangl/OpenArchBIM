//! The templates New Family starts a door or window from (door-window-spec.md sections 1, 2 and 8): the planes,
//! the labelled dimensions that hold them, the opening sketched on them, the parameters every such family carries
//! and its subcategories. Nothing here draws a door or window; the author does that on top in the family editor.

use std::collections::BTreeMap;

use openarch_params::{EvalError, Value};
use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::Id;
use crate::famgeom::*;
use crate::family::{Family, FamilyCategory, FamilySettings, HostKind, ParamDef, ParamKind, ParamValue, PlanSymbol, WALL_CLOSURE};

/// The family templates New Family offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Template {
    Door,
    Window,
    CurtainWallDoor,
}

impl Template {
    pub fn category(self) -> FamilyCategory {
        match self {
            Template::Window => FamilyCategory::Windows,
            Template::Door | Template::CurtainWallDoor => FamilyCategory::Doors,
        }
    }
}

/// The subcategories a door family draws with (spec section 1).
pub const DOOR_SUBCATEGORIES: [&str; 8] = ["Frame/Mullion", "Panel", "Glass", "Opening", "Plan Swing", "Elevation Swing", "Hidden Lines", "Trim"];
/// The subcategories a window family draws with (spec section 2).
pub const WINDOW_SUBCATEGORIES: [&str; 8] = ["Frame/Mullion", "Glass", "Sash", "Opening", "Sill/Head", "Plan Swing", "Elevation Swing", "Trim"];

fn param(name: &str, kind: ParamKind, instance: bool, group: &str, default: ParamValue) -> ParamDef {
    ParamDef { name: name.into(), param_kind: kind, instance, group: group.into(), formula: None, reporting: false, shared_guid: None, choices: vec![], discipline: "common".into(), tooltip: String::new(), locked: false, family_category: None, default }
}
fn length(name: &str, group: &str, v: f64) -> ParamDef {
    param(name, ParamKind::Length, false, group, ParamValue::Number(v))
}
fn text(name: &str, group: &str) -> ParamDef {
    param(name, ParamKind::Text, false, group, ParamValue::Text(String::new()))
}
fn material(name: &str) -> ParamDef {
    param(name, ParamKind::Material, false, "Materials and Finishes", ParamValue::Text(String::new()))
}
fn choice(name: &str, group: &str, choices: &[&str]) -> ParamDef {
    ParamDef { choices: choices.iter().map(|c| c.to_string()).collect(), ..param(name, ParamKind::Text, false, group, ParamValue::Text(choices[0].into())) }
}
/// A type parameter worked out from others; it cannot be typed into.
fn formula(name: &str, group: &str, f: &str, instance: bool) -> ParamDef {
    ParamDef { formula: Some(f.into()), reporting: true, instance, ..length(name, group, 0.0) }
}

/// The schedule parameters of spec section 8, in both templates. `panels` is the category a Panel Type swaps
/// between.
fn schedule_params(panels: FamilyCategory) -> Vec<ParamDef> {
    let mut v = vec![
        formula("Opening Width", "Dimensions", "[Width]", false),
        formula("Opening Height", "Dimensions", "[Height]", false),
        text("Outside Frame Finish", "Materials and Finishes"),
        text("Inside Frame Finish", "Materials and Finishes"),
        text("Outside Leaf Finish", "Materials and Finishes"),
        text("Inside Leaf Finish", "Materials and Finishes"),
        // Metroll's striker height
        length("Handle Height", "Construction", 1000.0),
        length("Handle Offset from Edge", "Construction", 60.0),
        ParamDef { param_kind: ParamKind::FamilyType, family_category: Some(panels), ..text("Door Panel Type", "Construction") },
        text("Door Stop & Closer", "Construction"),
        text("Handle & Lockset", "Construction"),
        text("Accessories", "Construction"),
        text("Hardware Set", "Construction"),
        param("U-value", ParamKind::Number, false, "Analytical Properties", ParamValue::Number(0.0)),
        param("SHGC", ParamKind::Number, false, "Analytical Properties", ParamValue::Number(0.0)),
        text("Glazing Type & Treatment", "Materials and Finishes"),
        text("Acoustic Rating", "Identity Data"),
        text("Fire Rating", "Identity Data"),
        text("BAL Rating", "Identity Data"),
    ];
    for n in ["Opening Type", "Opening Type Middle Transom", "Opening Type Sidelight Side 1", "Opening Type Transom Side 1", "Opening Type Sidelight Side 2", "Opening Type Transom Side 2"] {
        v.push(text(n, "Other"));
    }
    v
}

/// The standard identity data every family has.
fn identity_params() -> Vec<ParamDef> {
    let mut v: Vec<ParamDef> = ["Keynote", "Model", "Manufacturer", "Type Comments", "URL", "Description", "Assembly Description", "Assembly Code", "Type Mark", "OmniClass Number", "OmniClass Title"].iter().map(|n| text(n, "Identity Data")).collect();
    v.push(param("Cost", ParamKind::Number, false, "Identity Data", ParamValue::Number(0.0)));
    v
}

fn params_of(t: Template) -> Vec<ParamDef> {
    let mut v = match t {
        Template::Door | Template::CurtainWallDoor => vec![
            param("Sill Height", ParamKind::Length, true, "Constraints", ParamValue::Number(0.0)),
            length("Width", "Dimensions", 820.0),
            length("Height", "Dimensions", 2040.0),
            length("Thickness", "Dimensions", 40.0),
            material("Frame Material"),
            material("Door Material"),
            choice("Function", "Construction", if t == Template::CurtainWallDoor { &["Exterior", "Interior"] } else { &["Interior", "Exterior"] }),
            text("Construction Type", "Construction"),
        ],
        Template::Window => vec![
            length("Default Sill Height", "Constraints", 900.0),
            param("Sill Height", ParamKind::Length, true, "Constraints", ParamValue::Number(900.0)),
            formula("Head Height", "Constraints", "[Sill Height] + [Height]", true),
            length("Width", "Dimensions", 1200.0),
            length("Height", "Dimensions", 1200.0),
            material("Frame Material"),
            material("Glass"),
            material("Sash Material"),
            text("Construction Type", "Construction"),
        ],
    };
    // a curtain wall door fills its panel: no rough opening, trim or closure of wall layers
    if t != Template::CurtainWallDoor {
        let (w, h) = if t == Template::Window { (1200.0, 1200.0) } else { (820.0, 2040.0) };
        v.push(length("Rough Width", "Dimensions", w));
        v.push(length("Rough Height", "Dimensions", h));
        v.push(choice("Wall Closure", "Construction", &WALL_CLOSURE));
    }
    if t == Template::Door {
        v.push(length("Trim Projection Exterior", "Dimensions", 0.0));
        v.push(length("Trim Projection Interior", "Dimensions", 0.0));
        v.push(length("Trim Width", "Dimensions", 0.0));
    }
    v.extend(schedule_params(t.category()));
    v.extend(identity_params());
    // one name each: the schedule list repeats Fire Rating, which the identity data does not
    let mut seen = Vec::new();
    v.retain(|p| if seen.contains(&p.name) { false } else { seen.push(p.name.clone()); true });
    v
}

/// The body every family of `t` starts with: Left and Right held equal about the centre by a dimension labelled
/// Width, Top (Head for a window) held by Height above Ref. Level (Sill), Exterior and Interior on the placeholder
/// wall's faces, and an opening sketched on Left, Right, Ref. Level and Top, locked to them.
fn body_of(t: Template, params: &[ParamDef]) -> Result<FamilyBody, String> {
    let mut b = FamilyBody::template();
    b.cut_with_voids = true;
    let window = t == Template::Window;
    if window {
        if let Some(p) = b.planes.iter_mut().find(|p| p.id == PLANE_LEVEL) {
            p.name = "Sill".into();
        }
    }
    let values: BTreeMap<String, f64> = params.iter().filter_map(|p| Some((p.name.clone(), p.default.as_f64()?))).collect();
    let mut env = |n: &str| -> Result<Value, EvalError> {
        match n {
            "Host Thickness" | "Host Core Thickness" => Ok(Value::Num(PLACEHOLDER_WALL_MM)),
            "Host Core Offset" => Ok(Value::Num(0.0)),
            _ => values.get(n).map(|v| Value::Num(*v)).ok_or_else(|| EvalError::UnknownName(n.to_string())),
        }
    };
    let w = values["Width"];
    let left = b.add_offset_plane("Left", PLANE_LR, &fmt(-w / 2.0))?;
    let right = b.add_offset_plane("Right", PLANE_LR, &fmt(w / 2.0))?;
    let top = b.add_offset_plane(if window { "Head" } else { "Top" }, PLANE_LEVEL, &fmt(values["Height"]))?;
    let walls = t != Template::CurtainWallDoor;
    let faces = if walls { Some((b.add_offset_plane("Exterior", PLANE_FB, "[Host Thickness] / 2")?, b.add_offset_plane("Interior", PLANE_FB, "-[Host Thickness] / 2")?)) } else { None };
    for p in &mut b.planes {
        p.ref_kind = match p.name.as_str() {
            "Left" => RefKind::Left,
            "Right" => RefKind::Right,
            "Top" | "Head" => RefKind::Top,
            "Sill" => RefKind::Bottom,
            "Exterior" | "Interior" => RefKind::Weak,
            _ => p.ref_kind,
        };
    }
    // Left and Right equal about the centre, the whole labelled Width (one dimension does both); Height in the
    // front elevation
    let wd = b.add_dim(&[left, PLANE_LR, right], Some(SymView::Plan), &mut env)?;
    b.patch_dim(wd, &DimPatch { eq: Some(true), label: Some("Width".into()), ..Default::default() }, &mut env)?;
    let hd = b.add_dim(&[PLANE_LEVEL, top], Some(SymView::Front), &mut env)?;
    b.patch_dim(hd, &DimPatch { label: Some("Height".into()), ..Default::default() }, &mut env)?;
    let profile = vec![Loop::rect(Coord::on(left), Coord::on(right), Coord::on(PLANE_LEVEL), Coord::on(top))];
    let (start_lock, end_lock) = match faces {
        Some((ext, int)) => (Some(int), Some(ext)),
        None => (None, None),
    };
    b.add_form(Form {
        id: 0,
        name: "Opening".into(),
        shape: FormShape::Extrusion { plane: PLANE_FB, start: "-[Host Thickness] / 2".into(), end: "[Host Thickness] / 2".into(), start_lock, end_lock, profile },
        void: true,
        cuts_host: true,
        opening: true,
        cut_by: vec![],
        material: String::new(),
        material_param: None,
        subcategory: "Opening".into(),
        vis: Visibility::default(),
    });
    b.solve(&mut env)?;
    Ok(b)
}

fn fmt(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

/// A new family of template `t`, not yet in a project.
pub fn family_from(name: &str, t: Template) -> Result<Family, String> {
    let params = params_of(t);
    let body = body_of(t, &params)?;
    let subs: &[&str] = if t == Template::Window { &WINDOW_SUBCATEGORIES } else { &DOOR_SUBCATEGORIES };
    let settings = FamilySettings { subcategories: subs.iter().map(|s| s.to_string()).collect(), ..FamilySettings::default() };
    Ok(Family { name: name.into(), category: t.category(), host: HostKind::Wall, plan_symbol: PlanSymbol::None, params, built_in: false, catalog: vec![], settings, source_path: None, loaded_at: None, body })
}

impl Document {
    /// New Family from a door or window template, with one type. One undo step.
    pub fn new_family_from(&mut self, name: &str, t: Template) -> Result<Id, String> {
        if name.trim().is_empty() {
            return Err("a family needs a name".into());
        }
        if self.families().iter().any(|(_, f)| f.name == name) {
            return Err(format!("a family called '{name}' already exists"));
        }
        let (id, _) = self.add_family(family_from(name, t)?, vec![("Type 1".into(), Default::default())])?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;
    use crate::geom::Point;
    use serde_json::json;

    #[test]
    fn a_door_from_the_template_flexes_and_sits_in_its_wall() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let fid = d.new_family_from("Door Test", Template::Door).unwrap();
        let fam = d.family(fid).unwrap().clone();
        assert_eq!((fam.category, fam.host), (FamilyCategory::Doors, HostKind::Wall));
        // the spec's parameters, each once
        for n in ["Width", "Height", "Thickness", "Rough Width", "Rough Height", "Trim Projection Exterior", "Trim Projection Interior", "Trim Width", "Frame Material", "Door Material", "Function", "Wall Closure", "Construction Type", "Fire Rating", "Opening Width", "Handle Height", "Door Panel Type", "BAL Rating", "Type Comments", "Type Mark"] {
            assert_eq!(fam.params.iter().filter(|p| p.name == n).count(), 1, "{n}");
        }
        assert_eq!(fam.param("Door Panel Type").unwrap().param_kind, ParamKind::FamilyType);
        assert!(fam.settings.subcategories.contains(&"Plan Swing".to_string()) && fam.settings.subcategories.contains(&"Trim".to_string()));
        // dimensions: Width (EQ about the centre) and Height, and the opening locked to the four planes
        assert_eq!(fam.body.dims.len(), 2);
        assert!(fam.body.dims.iter().any(|x| x.eq && x.label.as_deref() == Some("Width")) && fam.body.dims.iter().any(|x| x.label.as_deref() == Some("Height")));
        let opening = fam.body.forms.iter().find(|f| f.opening).unwrap();
        let FormShape::Extrusion { profile, .. } = &opening.shape else { panic!() };
        assert!(profile[0].nodes.iter().all(|n| n.u.plane.is_some() && n.v.plane.is_some()));
        // placed: 820 x 2040 on the floor; a wider type grows about the centre
        let t = d.family_types().into_iter().find(|(_, t)| t.family == fid).unwrap().0;
        let inst = d.place_on_wall(t, w, Point::new(3000., 0.)).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(inst).cloned() else { panic!() };
        assert_eq!(d.instance_opening_size(&i), (820.0, 2040.0, 0.0));
        d.patch_family_type(t, &json!({ "values": { "Width": 920.0, "Height": 2340.0 } })).unwrap();
        assert_eq!(d.instance_opening_size(&i), (920.0, 2340.0, 0.0));
        let gaps = d.wall_cut_gaps(w, 1200.0);
        assert!((gaps[0].0 - 2540.0).abs() < 1e-6 && (gaps[0].1 - 3460.0).abs() < 1e-6, "{gaps:?}");
        // Exterior and Interior follow the wall it is in
        let solved = d.instance_solved(inst).unwrap();
        let s = solved.as_ref().as_ref().unwrap();
        let plane = |n: &str| s.planes[&fam.body.plane_named(n).unwrap()];
        let (wl, wr) = d.wall_extents(match d.get(w) { Some(Element::Wall(w)) => w, _ => panic!() });
        assert!((plane("Exterior") - (wl + wr) / 2.0).abs() < 1e-6 && (plane("Interior") + (wl + wr) / 2.0).abs() < 1e-6);
        // the template's Exterior plane is on the wall's exterior side, where the brick is
        let f = d.instance_frame(inst).unwrap();
        let ext = f.fam_world(0.0, plane("Exterior"));
        assert!(ext.y > 0.0, "the brick is to the left of a wall drawn along +x: {ext:?}");
    }

    #[test]
    fn a_window_from_the_template_sits_at_its_default_sill() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let fid = d.new_family_from("Window Test", Template::Window).unwrap();
        let fam = d.family(fid).unwrap().clone();
        assert!(fam.body.plane_named("Sill").is_some() && fam.body.plane_named("Head").is_some());
        assert!(fam.settings.subcategories.contains(&"Sash".to_string()) && fam.settings.subcategories.contains(&"Sill/Head".to_string()));
        let t = d.family_types().into_iter().find(|(_, t)| t.family == fid).unwrap().0;
        let inst = d.place_on_wall(t, w, Point::new(3000., 0.)).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(inst).cloned() else { panic!() };
        assert_eq!(d.instance_opening_size(&i), (1200.0, 1200.0, 900.0));
        assert_eq!(d.instance_param(&i, "Head Height"), Some(ParamValue::Number(2100.0)));
        d.patch_instance(inst, &json!({ "values": { "Sill Height": 1000.0 } })).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(inst).cloned() else { panic!() };
        assert_eq!(d.instance_param(&i, "Head Height"), Some(ParamValue::Number(2200.0)));
        assert_eq!(d.instance_param(&i, "Opening Width"), Some(ParamValue::Number(1200.0)));
        assert!(d.wall_cut_gaps(w, 1200.0).len() == 1 && d.wall_cut_gaps(w, 600.0).is_empty());
        // a curtain wall door has no wall closure or rough opening, and no wall faces
        let cw = family_from("CW Door", Template::CurtainWallDoor).unwrap();
        assert!(cw.param("Wall Closure").is_none() && cw.param("Rough Width").is_none() && cw.body.plane_named("Exterior").is_none());
        assert_eq!(cw.category, FamilyCategory::Doors);
        // a second family of the same name is refused
        assert!(d.new_family_from("Window Test", Template::Window).is_err());
    }
}
