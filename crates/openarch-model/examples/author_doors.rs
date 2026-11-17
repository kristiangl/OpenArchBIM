//! Writes the starter hinged door families (door-window-spec.md section 6) to `families/Doors/` as .oaf files.
//!
//! A one-off authoring aid like `author_profiles`: each family starts from the Door template and adds what an author
//! would draw on it in the family editor. The .oaf files are the source from then on, and running this again
//! overwrites them.
//!
//!     cargo run -p openarch-model --example author_doors
//!
//! Layout, in the family's own axes (x across the opening, y across the wall with the exterior on +y, z up):
//! - Left and Right are the leaf edges (Width). The frame's soffits (metal) or stop faces (timber) are planes off
//!   them, the frame is a sweep of a starter Profile family round them, and the wall's hole (Rough Width by Rough
//!   Height) is the Opening void locked to the Wall planes behind the frame.
//! - The frame is centred on the wall's centreline, so the door stays put when the wall type changes. The leaf
//!   closes flush with the frame's interior face and swings to the interior.
//! - Leaves are nested Panel families picked by Door Panel Type. In plan the door draws the open leaf on a
//!   reference line turned by Swing Angle, with its swing arc; in elevation, dashed swing lines.

use std::collections::BTreeMap;

use openarch_model::famgeom::*;
use openarch_model::family::{Family, ParamDef, ParamKind, ParamValue};
use openarch_model::faminst::{FamilyFile, FamilyFileType};
use openarch_model::famtemplate::{family_from, Template};

fn length(name: &str, group: &str, v: f64, tooltip: &str) -> ParamDef {
    ParamDef {
        name: name.into(),
        param_kind: ParamKind::Length,
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
        default: ParamValue::Number(v),
    }
}

fn vis(coarse: bool, medium: bool, fine: bool) -> Visibility {
    Visibility { coarse, medium, fine, ..Visibility::default() }
}

#[derive(Clone, Copy, PartialEq)]
enum Frame {
    /// Metroll Square Bend: soffits Frame Clearance / 2 outside the leaf edges, the wall Wall Gap behind them.
    Metal,
    /// Rebated timber jamb: the leaf edges in the 12 deep rebates, stop faces 12 - Frame Clearance inside them,
    /// the 30 jamb and 10 packing behind.
    Timber,
}

struct D {
    fam: Family,
}

impl D {
    fn plane(&self, name: &str) -> PlaneId {
        self.fam.body.plane_named(name).unwrap_or_else(|| panic!("no plane {name}"))
    }
    fn off(&mut self, name: &str, from: PlaneId, by: &str) -> PlaneId {
        self.fam.body.add_offset_plane(name, from, by).unwrap()
    }
    fn param_mut(&mut self, name: &str) -> &mut ParamDef {
        self.fam.params.iter_mut().find(|p| p.name == name).unwrap_or_else(|| panic!("no parameter {name}"))
    }
    fn set(&mut self, name: &str, v: ParamValue) {
        self.param_mut(name).default = v;
    }
    fn formula(&mut self, name: &str, f: &str) {
        let p = self.param_mut(name);
        p.formula = Some(f.into());
        p.reporting = false;
    }
    fn add_param(&mut self, p: ParamDef) {
        self.fam.params.insert(self.fam.params.iter().position(|q| q.group == "Materials and Finishes").unwrap_or(self.fam.params.len()), p);
    }
    fn sym(&mut self, view: SymView, nodes: Vec<Node>, shapes: Vec<Option<SegShape>>, sub: &str, v: Visibility) {
        let bulges = vec![0.0; nodes.len().saturating_sub(1)];
        self.fam.body.add_sym(SymElement { id: 0, view, path: Loop { nodes, bulges, closed: false, shapes }, filled: false, masking: false, subcategory: sub.into(), vis: v });
    }
    fn line(&mut self, name: &str, start: Node, angle: &str, length: &str) -> u32 {
        let id = self.fam.body.take_id();
        self.fam.body.ref_lines.push(RefLine { id, name: name.into(), view: SymView::Plan, start, angle: angle.into(), length: length.into(), ref_kind: RefKind::Weak, dim_at: None });
        id
    }
}

fn n(u: PlaneId, v: PlaneId) -> Node {
    Node::new(Coord::on(u), Coord::on(v))
}

/// A hinged door: one leaf hinged on the left, or a pair meeting at the centre.
fn hinged(name: &str, frame: Frame, double: bool, panel: &str, function: &str) -> Family {
    let mut d = D { fam: family_from(name, Template::Door).unwrap() };
    let (left, right, top) = (d.plane("Left"), d.plane("Right"), d.plane("Top"));
    d.set("Door Panel Type", ParamValue::Text(panel.into()));
    d.set("Function", ParamValue::Text(function.into()));
    d.set("Opening Type", ParamValue::Text(if double { "Double hinged" } else { "Hinged" }.into()));
    d.set("Frame Material", ParamValue::Text(if frame == Frame::Metal { "Steel" } else { "Timber" }.into()));
    d.set("Door Material", ParamValue::Text("Timber".into()));
    d.add_param(ParamDef { param_kind: ParamKind::Angle, instance: true, ..length("Swing Angle", "Graphics", 90.0, "How far the leaf is drawn open in plan") });

    // the frame: its line round the opening, the wall's hole behind it, and where the closed leaf's interior face is
    let (profile, profile_type, profile_params, frame_line, wall_back, leaf_face) = match frame {
        Frame::Metal => {
            d.add_param(length("Frame Clearance", "Construction", if double { 6.0 } else { 3.0 }, "Metroll frames are 3 wider than the door for each leaf"));
            d.add_param(length("Back Opening", "Construction", 95.0, "The wall thickness the frame fits round: 95 or 114"));
            d.add_param(ParamDef { formula: Some("[Back Opening] + 38".into()), ..length("Frame Width", "Construction", 133.0, "Across the wall, architrave to architrave") });
            d.add_param(length("Wall Gap", "Construction", 15.0, "From the soffit back to the end of the wall inside the frame"));
            d.formula("Rough Width", "[Width] + [Frame Clearance] + 2 * [Wall Gap]");
            d.formula("Rough Height", "[Height] + [Frame Clearance] / 2 + [Wall Gap]");
            let params = BTreeMap::from([("Back Opening".to_string(), "[Back Opening]".to_string()), ("Frame Width".to_string(), "[Frame Width]".to_string())]);
            ("Metal Door Frame - Square Bend", "95", params, ("-[Frame Clearance] / 2", "[Frame Clearance] / 2"), "[Wall Gap]", "-[Frame Width] / 2")
        }
        Frame::Timber => {
            d.add_param(length("Frame Clearance", "Construction", 2.0, "Round the leaf, in the jamb's rebate"));
            d.add_param(length("Jamb Width", "Construction", 119.0, "Across the wall: 119 or 138"));
            d.formula("Rough Width", "[Width] + 56 + 2 * [Frame Clearance]");
            d.formula("Rough Height", "[Height] + 28 + [Frame Clearance]");
            let params = BTreeMap::from([("Jamb Width".to_string(), "[Jamb Width]".to_string())]);
            ("Timber Door Jamb - Rebated", "119 x 30", params, ("12 - [Frame Clearance]", "[Frame Clearance] - 12"), "40", "-[Jamb Width] / 2")
        }
    };
    let (fl, fr) = frame_line;
    let jl = d.off("Frame Left", left, fl);
    let jr = d.off("Frame Right", right, fr);
    let jh = d.off("Frame Head", top, fr);
    let wl = d.off("Wall Left", jl, &format!("-{wall_back}"));
    let wr = d.off("Wall Right", jr, wall_back);
    let wh = d.off("Wall Head", jh, wall_back);
    let face = d.off("Leaf Interior Face", PLANE_FB, leaf_face);
    let leaf_mid = d.off("Leaf Centre", face, "[Thickness] / 2");
    let mid_h = d.off("Mid Height", PLANE_LEVEL, "[Height] / 2");

    // the wall's hole is the rough opening
    let opening = d.fam.body.forms.iter_mut().find(|f| f.name == "Opening").unwrap();
    if let FormShape::Extrusion { profile, .. } = &mut opening.shape {
        *profile = vec![Loop::rect(Coord::on(wl), Coord::on(wr), Coord::on(PLANE_LEVEL), Coord::on(wh))];
    }

    // the frame: a sweep up the right jamb, across the head and down the left (the profile's opening side to the
    // left of the path), centred on the wall
    let path = Loop { nodes: vec![n(jr, PLANE_LEVEL), n(jr, jh), n(jl, jh), n(jl, PLANE_LEVEL)], bulges: vec![], closed: false, shapes: vec![] };
    let pu = ProfileUse { family: profile.into(), type_name: profile_type.into(), type_param: None, params: profile_params };
    d.fam.body.add_form(Form {
        id: 0,
        name: "Frame".into(),
        shape: FormShape::Sweep { plane: PLANE_FB, path, profile: vec![], profile_angle: 0.0, flip: false, profile_family: Some(pu) },
        void: false,
        cuts_host: false,
        cut_by: vec![],
        opening: false,
        material: String::new(),
        material_param: Some("Frame Material".into()),
        subcategory: "Frame/Mullion".into(),
        vis: Visibility::default(),
    });

    // the leaves, closed, with their interior faces on the frame's interior face
    let leaves: Vec<(PlaneId, &str)> = if double {
        let lc = d.fam.body.add_mid_plane("Left Leaf Centre", left, PLANE_LR).unwrap();
        let rc = d.fam.body.add_mid_plane("Right Leaf Centre", PLANE_LR, right).unwrap();
        vec![(lc, "Left Leaf"), (rc, "Right Leaf")]
    } else {
        vec![(PLANE_LR, "Leaf")]
    };
    let (panel_family, panel_type) = panel.split_once(" : ").unwrap();
    let leaf_width = if double { "[Width] / 2" } else { "[Width]" };
    for (centre, label) in &leaves {
        let id = d.fam.body.take_id();
        let params = BTreeMap::from([("Width".to_string(), leaf_width.to_string()), ("Height".to_string(), "[Height]".to_string()), ("Thickness".to_string(), "[Thickness]".to_string())]);
        d.fam.body.nested.push(Nested { id, family: panel_family.into(), type_name: panel_type.into(), type_param: Some("Door Panel Type".into()), name: (*label).into(), origin: [Coord::on(*centre), Coord::on(leaf_mid), Coord::on(PLANE_LEVEL)], rotation_deg: 0.0, work_plane: None, flip: false, mirror: false, params, vis: Visibility::default() });
    }

    // plan: each leaf open on a hinge line from its hinge on the interior face, turned by Swing Angle to the
    // interior; a line at Coarse and Medium, its section (Thickness thick) at Fine; the swing arc from the closed
    // leaf's latch edge round to the open leaf's end
    // (hinge plane, the closed leaf's direction from it in degrees, latch plane)
    let hinges: Vec<(PlaneId, f64, PlaneId)> = if double { vec![(left, 0.0, PLANE_LR), (right, 180.0, PLANE_LR)] } else { vec![(left, 0.0, right)] };
    for (hinge, base, latch) in hinges {
        let open = if base == 0.0 { "-[Swing Angle]".to_string() } else { "180 + [Swing Angle]".to_string() };
        // the leaf's thickness lies towards the exterior of its interior face, whichever way it is turned
        let across = if base == 0.0 { "90 - [Swing Angle]".to_string() } else { "90 + [Swing Angle]".to_string() };
        let h = d.line(if base == 0.0 { "Hinge" } else { "Right Hinge" }, n(hinge, face), &open, leaf_width);
        let tip = d.line("", Node::on_line(h, 1.0), &across, "[Thickness]");
        let heel = d.line("", Node::on_line(h, 0.0), &across, "[Thickness]");
        d.sym(SymView::Plan, vec![Node::on_line(h, 0.0), Node::on_line(h, 1.0)], vec![], "Plan Swing", vis(true, true, false));
        d.sym(SymView::Plan, vec![Node::on_line(h, 0.0), Node::on_line(h, 1.0), Node::on_line(tip, 1.0), Node::on_line(heel, 1.0), Node::on_line(h, 0.0)], vec![], "Plan Swing", vis(false, false, true));
        d.sym(SymView::Plan, vec![n(latch, face), Node::on_line(h, 1.0)], vec![Some(SegShape::Arc { centre: Node::on_line(h, 0.0), ccw: base != 0.0 })], "Plan Swing", Visibility::default());
    }

    // elevation: dashed swing lines from the hinge side's top and bottom to the latch side at mid height, in the
    // front and back views
    let swings: Vec<(PlaneId, PlaneId)> = if double { vec![(left, PLANE_LR), (right, PLANE_LR)] } else { vec![(left, right)] };
    for view in [SymView::Front, SymView::Back] {
        for &(hinge, latch) in &swings {
            d.sym(view, vec![n(hinge, PLANE_LEVEL), n(latch, mid_h), n(hinge, top)], vec![], "Elevation Swing", vis(false, true, true));
        }
    }
    d.fam
}

fn file(fam: Family, types: Vec<(String, Vec<(&str, f64)>)>) -> FamilyFile {
    let types = types.into_iter().map(|(name, vals)| FamilyFileType { name, values: vals.into_iter().map(|(k, v)| (k.to_string(), ParamValue::Number(v))).collect::<BTreeMap<_, _>>() }).collect();
    FamilyFile { openarch_family: 1, family: fam, types, uses: vec![] }
}

/// `w x h` types, with any extra values.
fn sizes(widths: &[f64], h: f64, extra: &[(&'static str, f64)], double: bool) -> Vec<(String, Vec<(&'static str, f64)>)> {
    widths
        .iter()
        .map(|&w| {
            let name = if double { format!("2 x {w:.0} x {h:.0}") } else { format!("{w:.0} x {h:.0}") };
            let mut v = vec![("Width", if double { w * 2.0 } else { w }), ("Height", h)];
            v.extend_from_slice(extra);
            (name, v)
        })
        .collect()
}

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("families").join("Doors");
    std::fs::create_dir_all(&dir).unwrap();
    let flush = "Door Panel - Flush : Flush";
    // Metroll stocked single leaves and doubles (spec section 12); retail timber jamb doors
    let metal_singles = [520.0, 620.0, 720.0, 770.0, 820.0, 870.0, 920.0];
    let mut files = vec![];
    let mut t = sizes(&metal_singles, 2040.0, &[], false);
    t.push(("820 x 2040 (114 frame)".into(), vec![("Width", 820.0), ("Height", 2040.0), ("Back Opening", 114.0)]));
    files.push(file(hinged("Single Flush Door - Metal Frame", Frame::Metal, false, flush, "Interior"), t));
    let mut t = sizes(&[620.0, 720.0, 770.0, 820.0, 870.0, 920.0], 2040.0, &[], false);
    t.push(("820 x 2040 (138 jamb)".into(), vec![("Width", 820.0), ("Height", 2040.0), ("Jamb Width", 138.0)]));
    files.push(file(hinged("Single Flush Door - Timber Frame", Frame::Timber, false, flush, "Interior"), t));
    files.push(file(hinged("Double Flush Door - Metal Frame", Frame::Metal, true, flush, "Interior"), sizes(&[520.0, 620.0, 720.0, 820.0, 920.0], 2040.0, &[], true)));
    files.push(file(hinged("Double Flush Door - Timber Frame", Frame::Timber, true, flush, "Interior"), sizes(&[620.0, 720.0, 820.0], 2040.0, &[], true)));
    let mut t = sizes(&[820.0, 870.0, 920.0], 2040.0, &[("Jamb Width", 138.0)], false);
    t.push(("820 x 2340".into(), vec![("Width", 820.0), ("Height", 2340.0), ("Jamb Width", 138.0)]));
    files.push(file(hinged("Glazed Hinged Door - Timber Frame", Frame::Timber, false, "Door Panel - Glazed : Full Glazed", "Exterior"), t));
    for f in files {
        let path = dir.join(format!("{}.oaf", f.family.name));
        std::fs::write(&path, serde_json::to_string_pretty(&f).unwrap()).unwrap();
        println!("{}", path.display());
    }
}
