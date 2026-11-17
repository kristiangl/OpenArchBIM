//! Writes the starter Profile families (door-window-spec.md sections 5 and 12) to `families/` as .oaf files.
//!
//! A one-off authoring aid: the .oaf files it writes are the source from then on, opened and changed in the family
//! editor like any other family. Running it again overwrites them.
//!
//!     cargo run -p openarch-model --example author_profiles
//!
//! Every profile is drawn as a plan jamb section of the left jamb: x along the wall, the wall on the -x side and the
//! opening on +x; y across the wall, the exterior on +y; the origin on the wall centreline at the line the family's
//! opening plane sits on (the soffit of a door frame, the outer edge of a window frame).

use std::collections::BTreeMap;

use openarch_model::famgeom::*;
use openarch_model::family::{Family, FamilyCategory, FamilySettings, HostKind, ParamDef, ParamKind, ParamValue, PlanSymbol};
use openarch_model::faminst::{FamilyFile, FamilyFileType};

fn length(name: &str, v: f64, tooltip: &str) -> ParamDef {
    ParamDef {
        name: name.into(),
        param_kind: ParamKind::Length,
        instance: false,
        group: "Dimensions".into(),
        formula: None,
        reporting: false,
        shared_guid: None,
        choices: vec![],
        discipline: "common".into(),
        tooltip: tooltip.into(),
        locked: false,
        family_category: None,
        default: ParamValue::Number(v),
    }
}
fn text(name: &str, v: &str) -> ParamDef {
    ParamDef { param_kind: ParamKind::Text, group: "Identity Data".into(), default: ParamValue::Text(v.into()), ..length(name, 0.0, "") }
}

/// A profile family under construction: its body and the points of its one closed ring.
struct P {
    body: FamilyBody,
}

impl P {
    fn new() -> P {
        P { body: FamilyBody::template() }
    }
    fn x(&mut self, name: &str, by: &str) -> PlaneId {
        self.body.add_offset_plane(name, PLANE_LR, by).unwrap()
    }
    fn y(&mut self, name: &str, by: &str) -> PlaneId {
        self.body.add_offset_plane(name, PLANE_FB, by).unwrap()
    }
    /// The closed ring through `pts`, each a point `off` from its x and y planes.
    fn ring(&mut self, pts: &[((PlaneId, f64), (PlaneId, f64))]) {
        let nodes = pts.iter().map(|&((px, dx), (py, dy))| Node::new(Coord::from(px, dx), Coord::from(py, dy))).collect();
        self.body.add_sym(SymElement { id: 0, view: SymView::Plan, path: Loop::polygon(nodes), filled: false, masking: false, subcategory: String::new(), vis: Visibility::default() });
    }
}

fn file(name: &str, description: &str, params: Vec<ParamDef>, body: FamilyBody, types: &[(&str, &[(&str, f64)])]) -> FamilyFile {
    let mut params = params;
    params.push(text("Description", description));
    params.push(text("Manufacturer", ""));
    params.push(text("Type Comments", ""));
    let family = Family {
        name: name.into(),
        category: FamilyCategory::Profiles,
        host: HostKind::None,
        plan_symbol: PlanSymbol::None,
        params,
        built_in: false,
        catalog: vec![],
        settings: FamilySettings::default(),
        source_path: None,
        loaded_at: None,
        body,
    };
    let types = types.iter().map(|(n, vals)| FamilyFileType { name: (*n).into(), values: vals.iter().map(|(k, v)| (k.to_string(), ParamValue::Number(*v))).collect::<BTreeMap<_, _>>() }).collect();
    FamilyFile { openarch_family: 1, family, types, uses: vec![] }
}

/// Metroll pressed metal frame, Square Bend (spec section 12): the soffit at the origin, a stop rib in the middle
/// standing proud of it into the opening, 40 deep architrave legs on the faces, 19 returns to the wall faces and
/// 10 lips that the wall fits between. 1.1 steel. `stop` false is the slider frame, without the rib.
fn square_bend(stop: bool) -> FamilyFile {
    const T: f64 = 1.1;
    const LIP: f64 = 10.0;
    let mut p = P::new();
    let back = p.x("Back", "-[Leg Depth]");
    let ext = p.y("Exterior Face", "[Frame Width] / 2");
    let int = p.y("Interior Face", "-[Frame Width] / 2");
    let ext_wall = p.y("Exterior Wall Face", "[Back Opening] / 2");
    let int_wall = p.y("Interior Wall Face", "-[Back Opening] / 2");
    let s = PLANE_LR;
    let mut pts = vec![((back, LIP), (ext_wall, 0.0)), ((back, 0.0), (ext_wall, 0.0)), ((back, 0.0), (ext, 0.0)), ((s, 0.0), (ext, 0.0))];
    let mut inner = vec![((back, LIP), (ext_wall, T)), ((back, T), (ext_wall, T)), ((back, T), (ext, -T)), ((s, -T), (ext, -T))];
    if stop {
        let rib = p.x("Stop Face", "[Stop Depth]");
        let se = p.y("Stop Exterior", "[Stop Width] / 2");
        let si = p.y("Stop Interior", "-[Stop Width] / 2");
        pts.extend([((s, 0.0), (se, 0.0)), ((rib, 0.0), (se, 0.0)), ((rib, 0.0), (si, 0.0)), ((s, 0.0), (si, 0.0))]);
        inner.extend([((s, -T), (se, -T)), ((rib, -T), (se, -T)), ((rib, -T), (si, T)), ((s, -T), (si, T))]);
    }
    pts.extend([((s, 0.0), (int, 0.0)), ((back, 0.0), (int, 0.0)), ((back, 0.0), (int_wall, 0.0)), ((back, LIP), (int_wall, 0.0))]);
    inner.extend([((s, -T), (int, T)), ((back, T), (int, T)), ((back, T), (int_wall, -T)), ((back, LIP), (int_wall, -T))]);
    // out along the outside of the sheet, back along its inside
    inner.reverse();
    pts.extend(inner);
    p.ring(&pts);
    let mut params = vec![
        length("Back Opening", 95.0, "The wall thickness the frame fits round, between its back lips"),
        length("Frame Width", 133.0, "Across the wall, architrave face to architrave face"),
        length("Leg Depth", 40.0, "The architrave legs on the wall faces, from the soffit back"),
    ];
    if stop {
        params.push(ParamDef { formula: Some("[Back Opening] - 42".into()), ..length("Stop Width", 53.0, "The stop rib across the soffit") });
        params.push(length("Stop Depth", 15.0, "How far the stop rib stands proud of the soffit"));
    }
    let (name, desc, types): (&str, &str, &[(&str, &[(&str, f64)])]) = if stop {
        ("Metal Door Frame - Square Bend", "Metroll Square Bend pressed metal door frame, jamb and head. Origin on the soffit at the wall centreline; the frame is ordered 3 wider per leaf than the door.", &[("95", &[("Back Opening", 95.0), ("Frame Width", 133.0)]), ("114", &[("Back Opening", 114.0), ("Frame Width", 152.0)])])
    } else {
        ("Metal Door Frame - Square Bend Slider", "Metroll Square Bend slider frame (no stop). Origin on the soffit at the wall centreline.", &[("95", &[("Back Opening", 95.0), ("Frame Width", 133.0)]), ("114", &[("Back Opening", 114.0), ("Frame Width", 146.0)])])
    };
    file(name, desc, params, p.body, types)
}

/// A rebated timber door jamb: Jamb Width across the wall by Jamb Thickness, with the door's rebate cut from the
/// opening face on the interior side. The origin is the stop face (the clear opening).
fn timber_jamb() -> FamilyFile {
    let mut p = P::new();
    let back = p.x("Back", "-[Jamb Thickness]");
    let reb = p.x("Rebate Face", "-[Rebate Depth]");
    let ext = p.y("Exterior Face", "[Jamb Width] / 2");
    let int = p.y("Interior Face", "-[Jamb Width] / 2");
    let rl = p.y("Rebate Line", "-[Jamb Width] / 2 + [Rebate Width]");
    let s = PLANE_LR;
    p.ring(&[((s, 0.0), (ext, 0.0)), ((back, 0.0), (ext, 0.0)), ((back, 0.0), (int, 0.0)), ((reb, 0.0), (int, 0.0)), ((reb, 0.0), (rl, 0.0)), ((s, 0.0), (rl, 0.0))]);
    let params = vec![
        length("Jamb Width", 119.0, "Across the wall"),
        length("Jamb Thickness", 30.0, ""),
        length("Rebate Width", 38.0, "Across the wall: the door's thickness and a little"),
        length("Rebate Depth", 12.0, "Into the jamb from the stop face"),
    ];
    file("Timber Door Jamb - Rebated", "Rebated timber door jamb (retail Meranti sizes). Origin on the stop face at the wall centreline; the rebate is on the interior side.", params, p.body, &[("119 x 30", &[("Jamb Width", 119.0)]), ("138 x 30", &[("Jamb Width", 138.0)])])
}

/// An aluminium frame member as its outline: Frame Face (the sightline) into the opening from the outer edge at
/// the origin, Frame Depth across the wall, centred on it.
fn aluminium(name: &str, desc: &str, types: &[(&str, &[(&str, f64)])], depth: f64, face: f64) -> FamilyFile {
    let mut p = P::new();
    let inner = p.x("Inner Edge", "[Frame Face]");
    let ext = p.y("Exterior Face", "[Frame Depth] / 2");
    let int = p.y("Interior Face", "-[Frame Depth] / 2");
    let s = PLANE_LR;
    p.ring(&[((s, 0.0), (int, 0.0)), ((inner, 0.0), (int, 0.0)), ((inner, 0.0), (ext, 0.0)), ((s, 0.0), (ext, 0.0))]);
    let params = vec![length("Frame Depth", depth, "Across the wall"), length("Frame Face", face, "The sightline: into the opening from the outer edge")];
    file(name, desc, params, p.body, types)
}

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("families").join("Profiles");
    std::fs::create_dir_all(&dir).unwrap();
    let files = vec![
        square_bend(true),
        square_bend(false),
        timber_jamb(),
        // no manufacturer section drawings found (spec section 12): common residential depths, sightlines to check
        aluminium(
            "Aluminium Window Frame",
            "Aluminium window frame member as an outline. Origin on the frame's outer edge (the Width line) at its centre across the wall. Depths are common residential values; sightlines to be checked against a manufacturer drawing.",
            &[("Sliding", &[]), ("Awning", &[]), ("Fixed", &[]), ("Double Hung", &[])],
            52.0,
            30.0,
        ),
        aluminium(
            "Aluminium Sliding Door Frame",
            "Aluminium sliding, stacking or bifold door outer frame as an outline. Origin on the frame's outer edge at its centre across the wall. Wideline stacking frame 162 deep with the fly screen track, bifold outer frame 100.",
            &[("Sliding Door", &[("Frame Depth", 100.0)]), ("Stacking Door", &[("Frame Depth", 162.0)]), ("Bifold Door", &[("Frame Depth", 100.0)])],
            100.0,
            45.0,
        ),
    ];
    for f in files {
        let path = dir.join(format!("{}.oaf", f.family.name));
        std::fs::write(&path, serde_json::to_string_pretty(&f).unwrap()).unwrap();
        println!("{}", path.display());
    }
}
