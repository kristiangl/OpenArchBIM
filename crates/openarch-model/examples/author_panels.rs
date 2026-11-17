//! Writes the starter door Panel families (door-window-spec.md section 6) to `families/Panels/` as .oaf files.
//!
//! A one-off authoring aid like `author_profiles`: the .oaf files are the source from then on, and running it again
//! overwrites them.
//!
//!     cargo run -p openarch-model --example author_panels
//!
//! A panel is a door leaf in its closed position, drawn in the front view: Width centred on Center (Left/Right),
//! from Ref. Level up to Height, Thickness centred on Center (Front/Back). A door nests it, picks it with its Door
//! Panel Type parameter and sets its Width, Height and Thickness. Its solids are hidden in plan, where the door
//! draws the open leaf and swing itself (spec section 7); they show in elevation, section and 3D.

use std::collections::BTreeMap;

use openarch_model::famgeom::*;
use openarch_model::family::{Family, FamilyCategory, FamilySettings, HostKind, ParamDef, ParamKind, ParamValue, PlanSymbol};
use openarch_model::faminst::{FamilyFile, FamilyFileType};

fn param(name: &str, kind: ParamKind, group: &str, default: ParamValue, tooltip: &str) -> ParamDef {
    ParamDef {
        name: name.into(),
        param_kind: kind,
        instance: false,
        group: group.into(),
        formula: None,
        reporting: false,
        shared_guid: None,
        choices: vec![],
        discipline: "common".into(),
        tooltip: tooltip.into(),
        locked: false,
        family_category: None,
        default,
    }
}
fn length(name: &str, v: f64, tooltip: &str) -> ParamDef {
    param(name, ParamKind::Length, "Dimensions", ParamValue::Number(v), tooltip)
}
fn text(name: &str, v: &str) -> ParamDef {
    param(name, ParamKind::Text, "Identity Data", ParamValue::Text(v.into()), "")
}
fn material(name: &str, v: &str) -> ParamDef {
    param(name, ParamKind::Material, "Materials and Finishes", ParamValue::Text(v.into()), "")
}

/// Shown everywhere but plan.
fn not_in_plan() -> Visibility {
    Visibility { plan: false, ..Visibility::default() }
}

struct P {
    body: FamilyBody,
    left: PlaneId,
    right: PlaneId,
    top: PlaneId,
}

impl P {
    fn new() -> P {
        let mut body = FamilyBody::template();
        let left = body.add_offset_plane("Left", PLANE_LR, "-[Width] / 2").unwrap();
        let right = body.add_offset_plane("Right", PLANE_LR, "[Width] / 2").unwrap();
        let top = body.add_offset_plane("Top", PLANE_LEVEL, "[Height]").unwrap();
        for (id, kind) in [(left, RefKind::Left), (right, RefKind::Right), (top, RefKind::Top)] {
            body.planes.iter_mut().find(|p| p.id == id).unwrap().ref_kind = kind;
        }
        P { body, left, right, top }
    }
    fn x(&mut self, name: &str, from: PlaneId, by: &str) -> PlaneId {
        self.body.add_offset_plane(name, from, by).unwrap()
    }
    /// A solid (or the glass) through the panel's depth from `start` to `end`, its outline and holes in the front
    /// view as (left, right, bottom, top) planes.
    fn solid(&mut self, name: &str, rects: &[(PlaneId, PlaneId, PlaneId, PlaneId)], start: &str, end: &str, glass: bool) {
        let profile = rects.iter().map(|&(l, r, b, t)| Loop::rect(Coord::on(l), Coord::on(r), Coord::on(b), Coord::on(t))).collect();
        self.body.add_form(Form {
            id: 0,
            name: name.into(),
            shape: FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_FB, start: start.into(), end: end.into(), profile },
            void: false,
            cuts_host: false,
            cut_by: vec![],
            opening: false,
            material: if glass { "Glass".into() } else { String::new() },
            material_param: Some(if glass { "Glass" } else { "Door Material" }.into()),
            subcategory: if glass { "Glass" } else { "Panel" }.into(),
            vis: not_in_plan(),
        });
    }
}

fn file(name: &str, description: &str, extra: Vec<ParamDef>, body: FamilyBody, glass: bool, types: Vec<(&str, Vec<(&str, f64)>)>) -> FamilyFile {
    let mut params = vec![length("Width", 820.0, "Set by the door"), length("Height", 2040.0, "Set by the door"), length("Thickness", 40.0, "Set by the door"), material("Door Material", "")];
    if glass {
        params.push(material("Glass", "Glass"));
        params.push(length("Glass Thickness", 6.0, ""));
    }
    params.extend(extra);
    params.push(text("Description", description));
    params.push(text("Type Comments", ""));
    let mut subcategories = vec!["Panel".to_string()];
    if glass {
        subcategories.push("Glass".into());
    }
    let family = Family {
        name: name.into(),
        category: FamilyCategory::Doors,
        host: HostKind::None,
        plan_symbol: PlanSymbol::None,
        params,
        built_in: false,
        catalog: vec![],
        settings: FamilySettings { subcategories, ..FamilySettings::default() },
        source_path: None,
        loaded_at: None,
        body,
    };
    let types = types.into_iter().map(|(n, vals)| FamilyFileType { name: n.into(), values: vals.into_iter().map(|(k, v)| (k.to_string(), ParamValue::Number(v))).collect::<BTreeMap<_, _>>() }).collect();
    FamilyFile { openarch_family: 1, family, types, uses: vec![] }
}

const LEAF: (&str, &str) = ("-[Thickness] / 2", "[Thickness] / 2");
const PANE: (&str, &str) = ("-[Glass Thickness] / 2", "[Glass Thickness] / 2");

fn flush() -> FamilyFile {
    let mut p = P::new();
    let (l, r, t) = (p.left, p.right, p.top);
    p.solid("Leaf", &[(l, r, PLANE_LEVEL, t)], LEAF.0, LEAF.1, false);
    file("Door Panel - Flush", "Flush door leaf.", vec![], p.body, false, vec![("Flush", vec![])])
}

/// A flush leaf with a glazed vision panel: Vision Width by Vision Height from Vision Sill, its centre Vision Offset
/// from the leaf's centre (positive towards the right).
fn vision() -> FamilyFile {
    let mut p = P::new();
    let (l, r, t) = (p.left, p.right, p.top);
    let vc = p.x("Vision Centre", PLANE_LR, "[Vision Offset]");
    let vl = p.x("Vision Left", vc, "-[Vision Width] / 2");
    let vr = p.x("Vision Right", vc, "[Vision Width] / 2");
    let vs = p.x("Vision Sill", PLANE_LEVEL, "[Vision Sill Height]");
    let vh = p.x("Vision Head", vs, "[Vision Height]");
    p.solid("Leaf", &[(l, r, PLANE_LEVEL, t), (vl, vr, vs, vh)], LEAF.0, LEAF.1, false);
    p.solid("Vision Glass", &[(vl, vr, vs, vh)], PANE.0, PANE.1, true);
    let extra = vec![
        length("Vision Width", 150.0, ""),
        length("Vision Height", 600.0, ""),
        length("Vision Sill Height", 1000.0, "From the bottom of the leaf"),
        length("Vision Offset", 0.0, "The vision panel's centre from the leaf's centre, positive to the right"),
    ];
    file("Door Panel - Flush Vision Panel", "Flush door leaf with a glazed vision panel.", extra, p.body, true, vec![("150 x 600", vec![]), ("300 x 900", vec![("Vision Width", 300.0), ("Vision Height", 900.0), ("Vision Sill Height", 800.0)])])
}

/// A stile and rail leaf glazed above Glass Sill Height: solid between the stiles below it (the bottom rail, or a
/// lower panel for a half glazed leaf), glass between the stiles and up to the top rail.
fn glazed() -> FamilyFile {
    let mut p = P::new();
    let (l, r, t) = (p.left, p.right, p.top);
    let gl = p.x("Glass Left", l, "[Stile Width]");
    let gr = p.x("Glass Right", r, "-[Stile Width]");
    let gs = p.x("Glass Sill", PLANE_LEVEL, "[Glass Sill Height]");
    let gh = p.x("Glass Head", t, "-[Top Rail]");
    p.solid("Stiles and Rails", &[(l, r, PLANE_LEVEL, t), (gl, gr, gs, gh)], LEAF.0, LEAF.1, false);
    p.solid("Glass", &[(gl, gr, gs, gh)], PANE.0, PANE.1, true);
    let extra = vec![length("Stile Width", 110.0, ""), length("Top Rail", 110.0, ""), length("Glass Sill Height", 200.0, "From the bottom of the leaf: the bottom rail, or the top of the lower panel")];
    file(
        "Door Panel - Glazed",
        "Stile and rail door leaf, glazed above Glass Sill Height. Aluminium Framed is a sliding or stacking door panel's narrow frame.",
        extra,
        p.body,
        true,
        vec![
            ("Full Glazed", vec![]),
            ("Half Glazed", vec![("Glass Sill Height", 1000.0)]),
            ("Aluminium Framed", vec![("Stile Width", 60.0), ("Top Rail", 60.0), ("Glass Sill Height", 100.0)]),
        ],
    )
}

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("families").join("Panels");
    std::fs::create_dir_all(&dir).unwrap();
    for f in [flush(), vision(), glazed()] {
        let path = dir.join(format!("{}.oaf", f.family.name));
        std::fs::write(&path, serde_json::to_string_pretty(&f).unwrap()).unwrap();
        println!("{}", path.display());
    }
}
