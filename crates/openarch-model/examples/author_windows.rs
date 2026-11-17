//! Writes the starter window families (door-window-spec.md section 6) to `families/Windows/` as .oaf files.
//!
//! A one-off authoring aid like `author_doors`: each family starts from the Window template and adds what an author
//! would draw on it. The .oaf files are the source from then on, and running this again overwrites them.
//!
//!     cargo run -p openarch-model --example author_windows
//!
//! Width by Height is the frame size, as Wideline orders them, from the Sill plane (the family's origin level) to
//! Head. The frame is a closed sweep of the Aluminium Window Frame profile on Frame Offset from the wall's
//! centreline (spec section 2), and the wall's hole is the stud opening, frame + 60 each way. Sashes are drawn in
//! the family (Sash subcategory, Medium and Fine), glass shows at every level, and the opening sashes have dashed
//! swing lines or slide arrows in the front and back views. New windows sit with their heads at 2100.

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

fn n(u: PlaneId, v: PlaneId) -> Node {
    Node::new(Coord::on(u), Coord::on(v))
}
fn nf(u: PlaneId, du: f64, v: PlaneId, dv: f64) -> Node {
    Node::new(Coord::from(u, du), Coord::from(v, dv))
}

/// A rectangle in the front view: (left, right, bottom, top).
type Rect = (PlaneId, PlaneId, PlaneId, PlaneId);

struct W {
    fam: Family,
    centre: PlaneId,
    inner: Rect,
    mid: PlaneId,
}

impl W {
    fn plane(&self, name: &str) -> PlaneId {
        self.fam.body.plane_named(name).unwrap_or_else(|| panic!("no plane {name}"))
    }
    fn off(&mut self, name: &str, from: PlaneId, by: &str) -> PlaneId {
        self.fam.body.add_offset_plane(name, from, by).unwrap()
    }
    fn mid_plane(&mut self, name: &str, a: PlaneId, b: PlaneId) -> PlaneId {
        self.fam.body.add_mid_plane(name, a, b).unwrap()
    }
    fn param_mut(&mut self, name: &str) -> &mut ParamDef {
        self.fam.params.iter_mut().find(|p| p.name == name).unwrap_or_else(|| panic!("no parameter {name}"))
    }
    fn set(&mut self, name: &str, v: ParamValue) {
        self.param_mut(name).default = v;
    }
    fn add_param(&mut self, p: ParamDef) {
        self.fam.params.insert(self.fam.params.iter().position(|q| q.group == "Materials and Finishes").unwrap_or(self.fam.params.len()), p);
    }
    fn extrude(&mut self, name: &str, plane: PlaneId, half: &str, rects: &[Rect], sub: &str, material: Option<&str>, v: Visibility) {
        let profile = rects.iter().map(|&(l, r, b, t)| Loop::rect(Coord::on(l), Coord::on(r), Coord::on(b), Coord::on(t))).collect();
        self.fam.body.add_form(Form {
            id: 0,
            name: name.into(),
            shape: FormShape::Extrusion { start_lock: None, end_lock: None, plane, start: format!("-{half}"), end: half.into(), profile },
            void: false,
            cuts_host: false,
            cut_by: vec![],
            opening: false,
            material: if sub == "Glass" { "Glass".into() } else { String::new() },
            material_param: material.map(|m| m.to_string()),
            subcategory: sub.into(),
            vis: v,
        });
    }
    /// A pane, and its glazing line in plan at Coarse and Medium (Fine shows the pane in section).
    fn glass(&mut self, name: &str, plane: PlaneId, r: Rect) {
        self.extrude(name, plane, "[Glass Thickness] / 2", &[r], "Glass", Some("Glass"), Visibility::default());
        self.sym(SymView::Plan, vec![n(r.0, plane), n(r.1, plane)], "Glass", vis(true, true, false));
    }
    /// A sash: its stiles and rails Sash Face wide round its glass, Sash Depth deep on `plane`.
    fn sash(&mut self, name: &str, plane: PlaneId, (l, r, b, t): Rect) {
        let gl = self.off(&format!("{name} Glass Left"), l, "[Sash Face]");
        let gr = self.off(&format!("{name} Glass Right"), r, "-[Sash Face]");
        let gb = self.off(&format!("{name} Glass Bottom"), b, "[Sash Face]");
        let gt = self.off(&format!("{name} Glass Top"), t, "-[Sash Face]");
        self.extrude(name, plane, "[Sash Depth] / 2", &[(l, r, b, t), (gl, gr, gb, gt)], "Sash", Some("Sash Material"), vis(false, true, true));
        self.glass(&format!("{name} Glass"), plane, (gl, gr, gb, gt));
    }
    fn sym(&mut self, view: SymView, nodes: Vec<Node>, sub: &str, v: Visibility) {
        let bulges = vec![0.0; nodes.len().saturating_sub(1)];
        self.fam.body.add_sym(SymElement { id: 0, view, path: Loop { nodes, bulges, closed: false, shapes: vec![] }, filled: false, masking: false, subcategory: sub.into(), vis: v });
    }
    /// Dashed swing lines in the front and back views: from the hinge side's two corners to the handle side's middle.
    fn swing(&mut self, a: Node, apex: Node, b: Node) {
        for view in [SymView::Front, SymView::Back] {
            self.sym(view, vec![a, apex, b], "Elevation Swing", vis(false, true, true));
        }
    }
    /// A slide arrow in the front and back views from (`x`, `z`) + `from` to + `to`, along x or (`up`) along z.
    fn arrow(&mut self, x: PlaneId, z: PlaneId, from: f64, to: f64, up: bool) {
        let s = (to - from).signum();
        let at = |along: f64, side: f64| if up { nf(x, side, z, along) } else { nf(x, along, z, side) };
        for view in [SymView::Front, SymView::Back] {
            self.sym(view, vec![at(from, 0.0), at(to, 0.0)], "Sash", vis(false, true, true));
            self.sym(view, vec![at(to - s * 60.0, 40.0), at(to, 0.0), at(to - s * 60.0, -40.0)], "Sash", vis(false, true, true));
        }
    }
}

/// The window every starter starts as: the template, the frame swept round it, the stud opening and the planes
/// inside the frame.
fn window(name: &str, profile_type: &str, opening_type: &str) -> W {
    let mut fam = family_from(name, Template::Window).unwrap();
    fam.params.iter_mut().find(|p| p.name == "Rough Width").unwrap().formula = Some("[Width] + 60".into());
    fam.params.iter_mut().find(|p| p.name == "Rough Height").unwrap().formula = Some("[Height] + 60".into());
    let mut w = W { fam, centre: PLANE_FB, inner: (PLANE_LR, PLANE_LR, PLANE_LEVEL, PLANE_LEVEL), mid: PLANE_LEVEL };
    for (k, v) in [("Frame Material", "Aluminium"), ("Sash Material", "Aluminium"), ("Glass", "Glass"), ("Opening Type", opening_type)] {
        w.set(k, ParamValue::Text(v.into()));
    }
    w.add_param(length("Frame Depth", "Construction", 52.0, "Across the wall"));
    w.add_param(length("Frame Face", "Construction", 30.0, "The frame's sightline"));
    w.add_param(length("Frame Offset", "Construction", 0.0, "The frame's centre from the wall's centreline, positive to the exterior"));
    w.add_param(length("Glass Thickness", "Construction", 6.0, ""));
    let (left, right, head) = (w.plane("Left"), w.plane("Right"), w.plane("Head"));
    // the stud opening: Wideline's frame + 60 each way, the frame central in it
    let wl = w.off("Wall Left", left, "-30");
    let wr = w.off("Wall Right", right, "30");
    let wh = w.off("Wall Head", head, "30");
    let ws = w.off("Wall Sill", PLANE_LEVEL, "-30");
    let opening = w.fam.body.forms.iter_mut().find(|f| f.name == "Opening").unwrap();
    if let FormShape::Extrusion { profile, .. } = &mut opening.shape {
        *profile = vec![Loop::rect(Coord::on(wl), Coord::on(wr), Coord::on(ws), Coord::on(wh))];
    }
    w.centre = w.off("Frame Centre", PLANE_FB, "[Frame Offset]");
    let params = BTreeMap::from([("Frame Depth".to_string(), "[Frame Depth]".to_string()), ("Frame Face".to_string(), "[Frame Face]".to_string())]);
    let pu = ProfileUse { family: "Aluminium Window Frame".into(), type_name: profile_type.into(), type_param: None, params };
    // round the frame with its inside on the path's left: up the right jamb, across the head, down the left, along the sill
    let path = Loop { nodes: vec![n(right, PLANE_LEVEL), n(right, head), n(left, head), n(left, PLANE_LEVEL)], bulges: vec![], closed: true, shapes: vec![] };
    w.fam.body.add_form(Form {
        id: 0,
        name: "Frame".into(),
        shape: FormShape::Sweep { plane: w.centre, path, profile: vec![], profile_angle: 0.0, flip: false, profile_family: Some(pu) },
        void: false,
        cuts_host: false,
        cut_by: vec![],
        opening: false,
        material: String::new(),
        material_param: Some("Frame Material".into()),
        subcategory: "Frame/Mullion".into(),
        vis: Visibility::default(),
    });
    w.inner = (w.off("Inner Left", left, "[Frame Face]"), w.off("Inner Right", right, "-[Frame Face]"), w.off("Inner Sill", PLANE_LEVEL, "[Frame Face]"), w.off("Inner Head", head, "-[Frame Face]"));
    w.mid = w.off("Mid Height", PLANE_LEVEL, "[Height] / 2");
    w
}

fn sash_params(w: &mut W) {
    w.add_param(length("Sash Face", "Construction", 35.0, "The sash's stiles and rails"));
    w.add_param(length("Sash Depth", "Construction", 20.0, "Across the wall"));
}

fn fixed() -> Family {
    let mut w = window("Fixed Window", "Fixed", "Fixed");
    let (c, i) = (w.centre, w.inner);
    w.glass("Glass", c, i);
    w.fam
}

/// Two sashes side by side on two tracks: the left fixed on the exterior track, the right sliding left on the
/// interior one, meeting with Sash Overlap.
fn sliding() -> Family {
    let mut w = window("Sliding Window", "Sliding", "Sliding");
    sash_params(&mut w);
    w.add_param(length("Sash Overlap", "Construction", 30.0, "Where the sashes meet"));
    w.add_param(length("Track Spacing", "Construction", 26.0, "Between the sashes' centres across the frame"));
    let (il, ir, is, ih) = w.inner;
    let c = w.centre;
    let ext = w.off("Exterior Track", c, "[Track Spacing] / 2");
    let int = w.off("Interior Track", c, "-[Track Spacing] / 2");
    let meet = w.off("Meeting Left", PLANE_LR, "-[Sash Overlap] / 2");
    let meet_r = w.off("Meeting Right", PLANE_LR, "[Sash Overlap] / 2");
    w.sash("Fixed Sash", ext, (il, meet_r, is, ih));
    w.sash("Sliding Sash", int, (meet, ir, is, ih));
    let sc = w.mid_plane("Sliding Sash Centre", meet, ir);
    let mid = w.mid;
    w.arrow(sc, mid, 150.0, -150.0, false);
    w.fam
}

/// One sash filling the frame, top hung (awning, swing lines to the bottom) or side hung on the left (casement,
/// swing lines to the right).
fn hung(casement: bool) -> Family {
    let mut w = if casement { window("Casement Window", "Awning", "Casement") } else { window("Awning Window", "Awning", "Awning") };
    sash_params(&mut w);
    let (il, ir, is, ih) = w.inner;
    let c = w.centre;
    w.sash("Sash", c, (il, ir, is, ih));
    if casement {
        let mid = w.mid;
        w.swing(n(il, is), n(ir, mid), n(il, ih));
    } else {
        w.swing(n(il, ih), n(PLANE_LR, is), n(ir, ih));
    }
    w.fam
}

/// Two sashes one above the other: the top on the exterior track, the bottom on the interior sliding up, meeting
/// with Sash Overlap at mid height.
fn double_hung() -> Family {
    let mut w = window("Double Hung Window", "Double Hung", "Double hung");
    sash_params(&mut w);
    w.add_param(length("Sash Overlap", "Construction", 30.0, "Where the sashes meet"));
    w.add_param(length("Track Spacing", "Construction", 26.0, "Between the sashes' centres across the frame"));
    let (il, ir, is, ih) = w.inner;
    let c = w.centre;
    let ext = w.off("Exterior Track", c, "[Track Spacing] / 2");
    let int = w.off("Interior Track", c, "-[Track Spacing] / 2");
    let mid = w.mid;
    let mb = w.off("Meeting Bottom", mid, "-[Sash Overlap] / 2");
    let mt = w.off("Meeting Top", mid, "[Sash Overlap] / 2");
    w.sash("Top Sash", ext, (il, ir, mb, ih));
    w.sash("Bottom Sash", int, (il, ir, is, mt));
    let bc = w.mid_plane("Bottom Sash Centre", is, mt);
    w.arrow(PLANE_LR, bc, -150.0, 150.0, true);
    w.fam
}

/// The most blades a louvre window draws; Blade Count says how many show.
const BLADES: usize = 16;

/// Glass louvre blades, Blade Height high at Blade Pitch (closed, each overlapping the one below), tilted with the
/// bottom edge out, spanning the frame. As many show as fit the frame's inside.
fn louvre() -> Family {
    let mut w = window("Louvre Window", "Fixed", "Louvre");
    w.set("Glass", ParamValue::Text("Glass".into()));
    w.add_param(length("Blade Height", "Construction", 102.0, "Wideline: 102 blades span to 750, 152 to 900"));
    w.add_param(ParamDef { formula: Some("[Blade Height] - 13".into()), ..length("Blade Pitch", "Construction", 89.0, "Closed, each blade overlaps the one below by 13") });
    w.add_param(ParamDef { param_kind: ParamKind::Number, formula: Some("floor(([Height] - 2 * [Frame Face] - [Blade Height]) / [Blade Pitch]) + 1".into()), default: ParamValue::Number(0.0), ..length("Blade Count", "Construction", 0.0, "How many blades fit") });
    let (il, ir, is, _) = w.inner;
    let c = w.centre;
    // the glazing line in plan at Coarse and Medium, whichever blade the cut plane meets
    w.sym(SymView::Plan, vec![n(il, c), n(ir, c)], "Glass", vis(true, true, false));
    for k in 0..BLADES {
        let b = w.off(&format!("Blade {} Bottom", k + 1), is, &format!("{k} * [Blade Pitch]"));
        let t = w.off(&format!("Blade {} Top", k + 1), b, "[Blade Height]");
        // a blade's section in a side view (y across the wall, z up), extruded from frame jamb to frame jamb
        let nodes = vec![nf(c, 9.0, b, 0.0), nf(c, 3.0, b, 0.0), nf(c, -9.0, t, 0.0), nf(c, -3.0, t, 0.0)];
        let profile = vec![Loop::polygon(vec![nodes[0], nodes[1], nodes[2], nodes[3]])];
        w.fam.body.add_form(Form {
            id: 0,
            name: format!("Blade {}", k + 1),
            shape: FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_LR, start: "-[Width] / 2 + [Frame Face]".into(), end: "[Width] / 2 - [Frame Face]".into(), profile },
            void: false,
            cuts_host: false,
            cut_by: vec![],
            opening: false,
            material: "Glass".into(),
            material_param: Some("Glass".into()),
            subcategory: "Glass".into(),
            vis: Visibility { when: Some(format!("[Blade Count] > {k}")), ..Visibility::default() },
        });
    }
    w.fam
}

fn file(fam: Family, types: Vec<(String, Vec<(&'static str, f64)>)>) -> FamilyFile {
    let types = types.into_iter().map(|(name, vals)| FamilyFileType { name, values: vals.into_iter().map(|(k, v)| (k.to_string(), ParamValue::Number(v))).collect::<BTreeMap<_, _>>() }).collect();
    FamilyFile { openarch_family: 1, family: fam, types, uses: vec![] }
}

/// `W x H` types with their heads at 2100.
fn sizes(list: &[(f64, f64)], extra: &[(&'static str, f64)], suffix: &str) -> Vec<(String, Vec<(&'static str, f64)>)> {
    list.iter()
        .map(|&(w, h)| {
            let mut v = vec![("Width", w), ("Height", h), ("Default Sill Height", 2100.0 - h)];
            v.extend_from_slice(extra);
            (format!("{w:.0} x {h:.0}{suffix}"), v)
        })
        .collect()
}

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("families").join("Windows");
    std::fs::create_dir_all(&dir).unwrap();
    // Wideline frame sizes (spec section 12): heights 600, 860, 1030, 1200, 1370, 1460, 1800, 2050; widths 610,
    // 850, 1210, 1570, 1810, 2170, 2410, 2650
    let files = vec![
        file(fixed(), sizes(&[(610.0, 1200.0), (850.0, 1200.0), (1210.0, 1200.0), (1810.0, 1200.0), (1210.0, 2050.0), (1810.0, 2050.0)], &[], "")),
        file(sliding(), sizes(&[(850.0, 860.0), (1210.0, 1030.0), (1570.0, 1030.0), (1210.0, 1200.0), (1810.0, 1200.0), (2170.0, 1200.0), (2410.0, 1200.0), (1810.0, 1460.0)], &[], "")),
        file(hung(false), sizes(&[(610.0, 600.0), (850.0, 600.0), (1210.0, 600.0), (850.0, 860.0), (1210.0, 860.0), (1210.0, 1030.0), (1210.0, 1200.0)], &[], "")),
        file(double_hung(), sizes(&[(610.0, 1200.0), (850.0, 1200.0), (850.0, 1460.0), (1210.0, 1460.0), (850.0, 1800.0)], &[], "")),
        // no supplier figures found: the awning grid, taller (spec section 12)
        file(hung(true), sizes(&[(610.0, 1030.0), (610.0, 1200.0), (850.0, 1200.0), (610.0, 1460.0)], &[], "")),
        {
            let mut t = sizes(&[(610.0, 600.0), (610.0, 900.0), (610.0, 1200.0)], &[], "");
            t.extend(sizes(&[(850.0, 900.0), (850.0, 1200.0)], &[("Blade Height", 152.0)], " (152 blades)"));
            file(louvre(), t)
        },
    ];
    for f in files {
        let path = dir.join(format!("{}.oaf", f.family.name));
        std::fs::write(&path, serde_json::to_string_pretty(&f).unwrap()).unwrap();
        println!("{}", path.display());
    }
}
