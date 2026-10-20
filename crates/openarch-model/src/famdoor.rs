//! The built-in framed flush doors as stored family bodies.
//!
//! These are made the way a user would make them in the family editor: reference
//! planes driven by the type's parameters, extrusions locked to those planes, a void
//! for the opening, and symbolic lines for the open leaf and its swing in plan. The
//! same parameters (`Frame Type`, `Rib Width`, `Jamb Depth`, ...) drive them, so every
//! type and every placed door keeps working. `frame.rs` is kept as the reference the
//! tests compare this against.
//!
//! Family axes: x along the wall, y across it (positive on the swing side), z up from
//! the floor. A timber frame is centred on the wall's core (`Host Core Offset`); a
//! metal frame on the wall's centre.
//!
//! Plan: the frame is cut by the plan's cut plane at Fine, outlined by a symbolic
//! envelope at Medium and left out at Coarse, as in the legacy symbol. The closed leaf
//! is a solid that shows in 3D and elevation only; in plan the open leaf and its arc are
//! symbolic lines.

use crate::famgeom::*;
use std::collections::HashMap;

const METAL: &str = "[Frame Type] == \"Pressed Metal\"";
const TIMBER: &str = "[Frame Type] == \"Timber\"";

/// tan(22.5 degrees): the bulge of a quarter circle.
const QUARTER: f64 = 0.414_213_562_373_095;

/// A rectangle of material in section: `(a0, a1, b0, b1)` as expressions. `a` runs
/// out from the reveal edge into the wall, `b` across the wall from the frame's centre.
type Piece = (String, String, String, String);

/// `sign` ("-" or "") applied to an offset expression, leaving a zero offset alone.
fn off(sign: &str, e: &str) -> String {
    if e.trim() == "0" {
        "0".into()
    } else {
        format!("{sign}({e})")
    }
}

fn neg(e: &str) -> String {
    format!("-({e})")
}

struct B {
    body: FamilyBody,
    /// Planes already made, by what they hang from and how far, so equal planes are shared.
    seen: HashMap<(PlaneId, String), PlaneId>,
}

impl B {
    fn new() -> B {
        B { body: FamilyBody::template(), seen: HashMap::new() }
    }

    /// A plane `by` from another, made once.
    fn plane(&mut self, name: &str, from: PlaneId, by: &str) -> PlaneId {
        let by = by.trim();
        if by == "0" {
            return from;
        }
        if let Some(id) = self.seen.get(&(from, by.to_string())) {
            return *id;
        }
        let mut name = name.to_string();
        if name.is_empty() {
            // named for what it hangs from and how far, so the editor's list reads as the sketch of the section
            let base = self.body.plane(from).map_or(String::new(), |p| p.name.clone());
            name = format!("{base} {}{}", if by.starts_with('-') { "" } else { "+ " }, by.replace(['[', ']'], "").replace("  ", " "));
        }
        let mut unique = name.clone();
        let mut k = 2;
        while self.body.plane_named(&unique).is_some() {
            unique = format!("{name} ({k})");
            k += 1;
        }
        let id = self.body.add_offset_plane(&unique, from, by).expect("door plane names are unique");
        self.seen.insert((from, by.to_string()), id);
        id
    }

    fn form(&mut self, name: &str, shape: FormShape, void: bool, sub: &str, vis: Visibility) {
        self.body.add_form(Form { id: 0, name: name.into(), shape, void, cuts_host: true, cut_by: vec![], opening: false, material: String::new(), material_param: None, subcategory: sub.into(), vis });
    }

    fn sym(&mut self, path: Loop, filled: bool, sub: &str, vis: Visibility) {
        self.body.add_sym(SymElement { id: 0, view: SymView::Plan, path, filled, masking: false, subcategory: sub.into(), vis });
    }
}

fn vis(when: Option<&str>, coarse: bool, medium: bool, fine: bool, plan: bool) -> Visibility {
    Visibility { coarse, medium, fine, plan, front_back: true, left_right: true, visible: true, when: when.map(|s| s.to_string()) }
}

fn piece(a0: &str, a1: &str, b0: &str, b1: &str) -> Piece {
    (a0.into(), a1.into(), b0.into(), b1.into())
}

/// The pieces of a pressed metal frame's section, split into those that stop under the
/// head (the rib) and the rest. Same shapes as `DoorFrame::section`, cut so none overlap.
fn metal_pieces() -> (Vec<Piece>, Vec<Piece>) {
    let t = "[Steel Gauge]";
    let rh = "[Rib Height]";
    let (rw2, w2) = ("[Rib Width]/2".to_string(), "[Jamb Depth]/2".to_string());
    let d2 = "([Jamb Depth]/2 + [Architrave Projection])".to_string();
    let d2t = format!("({d2} - {t})");
    let face = "[Architrave Width]";
    let nrh = format!("-{rh}");
    let nrht = format!("-{rh} + {t}");
    let rib = vec![
        piece(&nrh, &nrht, &neg(&rw2), &rw2), // the rib's crown
        piece(&nrht, "0", &neg(&rw2), &format!("-({rw2}) + {t}")), // its two sides
        piece(&nrht, "0", &format!("{rw2} - {t}"), &rw2),
    ];
    let mut rest = vec![];
    for side in [1.0, -1.0] {
        let m = |b0: &str, b1: &str| if side > 0.0 { (b0.to_string(), b1.to_string()) } else { (neg(b1), neg(b0)) };
        let (f0, f1) = m(&rw2, &d2t);
        rest.push(piece("0", t, &f0, &f1)); // recess floor
        let (l0, l1) = m(&d2t, &d2);
        rest.push(piece("0", face, &l0, &l1)); // architrave leg
        let (r0, r1) = m(&w2, &d2t);
        rest.push(piece(&format!("{face} - {t}"), face, &r0, &r1)); // its return onto the wall face
    }
    (rib, rest)
}

/// A timber frame's section: the jamb, and the planted stop (which stops under the head).
fn timber_pieces() -> (Vec<Piece>, Vec<Piece>) {
    let h2 = "[Jamb Depth]/2".to_string();
    let s0 = "-[Jamb Depth]/2 + [Stop Offset]".to_string();
    let s1 = format!("{s0} + [Stop Width]");
    let stop = vec![piece("-[Stop Height]", "0", &s0, &s1)];
    let jamb = vec![piece("0", "[Jamb Thickness]", &neg(&h2), &h2)];
    (stop, jamb)
}

/// The body of `Single-Flush` (`double` false) or `Double-Flush`.
pub fn flush_door_body(double: bool) -> FamilyBody {
    let mut b = B::new();
    let (lr, fb, level) = (PLANE_LR, PLANE_FB, PLANE_LEVEL);

    // ---- planes
    let reveal_l = b.plane("Reveal Left", lr, "-[Reveal Width]/2");
    let reveal_r = b.plane("Reveal Right", lr, "[Reveal Width]/2");
    let head = b.plane("Head Reveal", level, "[Reveal Height]");
    let centre = b.plane("Frame Centre", fb, "if([Frame Type] == \"Timber\", [Host Core Offset], 0)");

    // ---- the frame: the section swept up one jamb, across the head and down the other, mitred at the corners
    // The path runs on the reveal line in the front view, so `a` (out from the reveal edge into the wall)
    // is the sweep's `u` and `b` (across the wall from the frame centre) is its `v`.
    let sets: [(&str, Vec<Piece>, Vec<Piece>, &str); 2] = {
        let (mr, mj) = metal_pieces();
        let (ts, tj) = timber_pieces();
        [("Metal", mr, mj, METAL), ("Timber", ts, tj, TIMBER)]
    };
    for (kind, narrow, full, when) in sets {
        // cut by the plan's cut plane at Fine only; elevations and 3D show it at every detail level
        let fine_only = vis(Some(when), false, false, true, true);
        let profile: Vec<ProfileLoop> = narrow.iter().chain(full.iter()).map(|(a0, a1, b0, b1)| ProfileLoop::rect(a0, a1, b0, b1)).collect();
        let node = |x: PlaneId, z: PlaneId| Node::new(Coord::on(x), Coord::on(z));
        let path = Loop { nodes: vec![node(reveal_l, level), node(reveal_l, head), node(reveal_r, head), node(reveal_r, level)], bulges: vec![], closed: false };
        b.form(&format!("{kind} Frame"), FormShape::Sweep { plane: centre, path, profile, profile_angle: 0.0, flip: false }, false, "Frame/Mullion", fine_only);
        // the envelope at Medium detail
        let (y_out, a_in, a_out) = if kind == "Metal" { ("([Jamb Depth]/2 + [Architrave Projection])", "-[Rib Height]", "[Architrave Width]") } else { ("[Jamb Depth]/2", "-[Stop Height]", "[Jamb Thickness]") };
        let (ya, yb) = (b.plane("", centre, &neg(y_out)), b.plane("", centre, y_out));
        for (from, sign) in [(reveal_l, "-"), (reveal_r, "")] {
            let xa = b.plane("", from, &off(sign, a_in));
            let xb = b.plane("", from, &off(sign, a_out));
            b.sym(Loop::rect(Coord::on(xa), Coord::on(xb), Coord::on(ya), Coord::on(yb)), false, "Frame/Mullion", vis(Some(when), false, true, false, true));
        }
    }

    // ---- the leaf and its swing
    let leaf_b0 = "if([Frame Type] == \"Timber\", -[Jamb Depth]/2 + [Stop Offset] + [Stop Width], [Rib Width]/2)";
    let leaf_back = b.plane("Leaf Back", centre, leaf_b0);
    let pivot = b.plane("Leaf Pivot", leaf_back, "[Thickness]/2");
    let side_gap = if double { "[Frame Clearance Width]/3" } else { "[Frame Clearance Width]/2" };
    let leaf_len = if double { "[Width]/2" } else { "[Width]" };
    let edge_l = b.plane("Leaf Left Edge", reveal_l, &side_gap.to_string());
    let edge_r = b.plane("Leaf Right Edge", reveal_r, &neg(side_gap));
    let leaf_bottom = b.plane("Leaf Bottom", level, "[Leaf Bottom Gap]");
    let leaf_top = b.plane("Leaf Top", leaf_bottom, "[Height]");
    let mut leaf_loops = vec![];
    if double {
        let mid_l = b.plane("Leaf Meeting Left", lr, "-[Frame Clearance Width]/6");
        let mid_r = b.plane("Leaf Meeting Right", lr, "[Frame Clearance Width]/6");
        leaf_loops.push(Loop::rect(Coord::on(edge_l), Coord::on(mid_l), Coord::on(leaf_bottom), Coord::on(leaf_top)));
        leaf_loops.push(Loop::rect(Coord::on(mid_r), Coord::on(edge_r), Coord::on(leaf_bottom), Coord::on(leaf_top)));
    } else {
        leaf_loops.push(Loop::rect(Coord::on(edge_l), Coord::on(edge_r), Coord::on(leaf_bottom), Coord::on(leaf_top)));
    }
    b.form("Leaf", FormShape::Extrusion { start_lock: None, end_lock: None, plane: leaf_back, start: "0".into(), end: "[Thickness]".into(), profile: leaf_loops }, false, "Panel", vis(None, true, true, true, false));

    // open leaf at 90 degrees to the wall, and the arc it sweeps
    let open_end = b.plane("Open Leaf End", pivot, leaf_len);
    let hinges: Vec<(PlaneId, f64)> = if double { vec![(edge_l, 1.0), (edge_r, -1.0)] } else { vec![(edge_l, 1.0)] };
    for (hinge, inward) in hinges {
        let name = if inward > 0.0 { "Left" } else { "Right" };
        let inner = b.plane(&format!("Open Leaf {name} Face"), hinge, &if inward > 0.0 { "[Thickness]".to_string() } else { "-[Thickness]".to_string() });
        let closed_end = b.plane(&format!("Swing {name} End"), hinge, &if inward > 0.0 { leaf_len.to_string() } else { neg(leaf_len) });
        b.sym(Loop::rect(Coord::on(hinge), Coord::on(inner), Coord::on(pivot), Coord::on(open_end)), true, "Panel", vis(None, true, true, true, true));
        // from the closed end round to the open tip: anticlockwise at the left hinge, clockwise at the right
        let bulge = if inward > 0.0 { QUARTER } else { -QUARTER };
        b.sym(
            Loop { nodes: vec![Node::new(Coord::on(closed_end), Coord::on(pivot)), Node::new(Coord::on(hinge), Coord::on(open_end))], bulges: vec![bulge], closed: false },
            false,
            "Plan Swing",
            vis(None, true, true, true, true),
        );
    }

    // ---- the hole in the wall
    let rough_l = b.plane("Rough Left", lr, "-[Rough Width]/2");
    let rough_r = b.plane("Rough Right", lr, "[Rough Width]/2");
    let rough_top = b.plane("Rough Top", level, "[Rough Height]");
    b.form(
        "Wall Opening",
        FormShape::Extrusion { start_lock: None, end_lock: None, plane: fb, start: "-[Host Thickness]".into(), end: "[Host Thickness]".into(), profile: vec![Loop::rect(Coord::on(rough_l), Coord::on(rough_r), Coord::on(level), Coord::on(rough_top))] },
        true,
        "Opening",
        Visibility::default(),
    );
    b.body
}

/// The frame, glass and opening of a window, or the frame of a sliding door, on the opening
/// (`Rough Width` by `Rough Height`) from the floor or sill up. The frame is `Frame Width`
/// wide (at most a quarter of the opening) and `Frame Depth` deep (at most the wall).
fn frame_in_opening(b: &mut B, sill: bool) -> (PlaneId, PlaneId, PlaneId, PlaneId, PlaneId, PlaneId) {
    let (lr, fb, level) = (PLANE_LR, PLANE_FB, PLANE_LEVEL);
    let fw = "min([Frame Width], [Rough Width]/4)";
    let depth = "min([Frame Depth], [Host Thickness])";
    let left = b.plane("Opening Left", lr, "-[Rough Width]/2");
    let right = b.plane("Opening Right", lr, "[Rough Width]/2");
    let top = b.plane("Opening Top", level, "[Rough Height]");
    let inner_l = b.plane("Frame Inner Left", left, fw);
    let inner_r = b.plane("Frame Inner Right", right, &neg(fw));
    let head_under = b.plane("Head Underside", top, &neg(fw));
    let sill_top = b.plane("Sill Top", level, fw);
    let back = b.plane("Frame Back", fb, &format!("-({depth})/2"));
    // the frame is swept round the opening: inside the reveal line (`u` negative), `Frame Depth` deep
    let node = |x: PlaneId, z: PlaneId| Node::new(Coord::on(x), Coord::on(z));
    let nodes = vec![node(left, level), node(left, top), node(right, top), node(right, level)];
    let path = Loop { nodes, bulges: vec![], closed: sill };
    let profile = vec![ProfileLoop::rect(&format!("-({fw})"), "0", "0", depth)];
    b.form("Frame", FormShape::Sweep { plane: back, path, profile, profile_angle: 0.0, flip: false }, false, "Frame/Mullion", vis(None, true, true, true, false));
    // the hole in the wall
    let rough_top = top;
    b.form(
        "Wall Opening",
        FormShape::Extrusion { start_lock: None, end_lock: None, plane: fb, start: "-[Host Thickness]".into(), end: "[Host Thickness]".into(), profile: vec![Loop::rect(Coord::on(left), Coord::on(right), Coord::on(level), Coord::on(rough_top))] },
        true,
        "Opening",
        Visibility::default(),
    );
    (left, right, inner_l, inner_r, head_under, sill_top)
}

fn line(a: (PlaneId, PlaneId), c: (PlaneId, PlaneId)) -> Loop {
    Loop { nodes: vec![Node::new(Coord::on(a.0), Coord::on(a.1)), Node::new(Coord::on(c.0), Coord::on(c.1))], bulges: vec![], closed: false }
}

/// The body of the starter windows (`Fixed`, `Awning` and `Sliding` share it until a type of
/// sash is drawn for each): a frame all round, a pane of glass, the wall faces and the glazing
/// line in plan.
pub fn window_body() -> FamilyBody {
    let mut b = B::new();
    let (fb, _level) = (PLANE_FB, PLANE_LEVEL);
    let (left, right, inner_l, inner_r, head_under, sill_top) = frame_in_opening(&mut b, true);
    let glass_back = b.plane("Glass Back", fb, "-[Glazing Thickness]/2");
    b.form(
        "Glass",
        FormShape::Extrusion { start_lock: None, end_lock: None, plane: glass_back, start: "0".into(), end: "[Glazing Thickness]".into(), profile: vec![Loop::rect(Coord::on(inner_l), Coord::on(inner_r), Coord::on(sill_top), Coord::on(head_under))] },
        false,
        "Glass",
        vis(None, true, true, true, false),
    );
    // plan: the wall faces across the opening and the glazing line either side of the centre
    let (face_f, face_b) = (b.plane("Host Face Front", fb, "[Host Thickness]/2"), b.plane("Host Face Back", fb, "-[Host Thickness]/2"));
    let (glaze_f, glaze_b) = (b.plane("Glazing Front", fb, "[Glazing Thickness]"), b.plane("Glazing Back", fb, "-[Glazing Thickness]"));
    b.sym(line((left, face_f), (right, face_f)), false, "Frame/Mullion", vis(None, true, true, true, true));
    b.sym(line((left, face_b), (right, face_b)), false, "Frame/Mullion", vis(None, true, true, true, true));
    b.sym(line((left, glaze_f), (right, glaze_f)), false, "Glass", vis(None, true, true, true, true));
    b.sym(line((left, glaze_b), (right, glaze_b)), false, "Glass", vis(None, true, true, true, true));
    b.body
}

/// The body of `Sliding-Glass`: a frame and two glass panels running past each other.
pub fn sliding_door_body() -> FamilyBody {
    let mut b = B::new();
    let (lr, fb) = (PLANE_LR, PLANE_FB);
    let (_left, _right, inner_l, inner_r, head_under, _sill_top) = frame_in_opening(&mut b, false);
    let (floor, left, right) = (PLANE_LEVEL, b.plane("Panel Run Left", lr, "-[Rough Width]/2"), b.plane("Panel Run Right", lr, "[Rough Width]/2"));
    // the two panels: the rear one hangs from the left, the front one from the right, 30 thick and 50 apart
    let (rear_a, rear_b) = (b.plane("Rear Panel Back", fb, "-40"), b.plane("Rear Panel Front", fb, "-10"));
    let (front_a, front_b) = (b.plane("Front Panel Back", fb, "10"), b.plane("Front Panel Front", fb, "40"));
    let (mid_r, mid_l) = (b.plane("Rear Panel Edge", lr, "[Rough Width]/10"), b.plane("Front Panel Edge", lr, "-[Rough Width]/10"));
    b.form(
        "Rear Panel",
        FormShape::Extrusion { start_lock: None, end_lock: None, plane: rear_a, start: "0".into(), end: "30".into(), profile: vec![Loop::rect(Coord::on(inner_l), Coord::on(mid_r), Coord::on(floor), Coord::on(head_under))] },
        false,
        "Glass",
        vis(None, true, true, true, false),
    );
    b.form(
        "Front Panel",
        FormShape::Extrusion { start_lock: None, end_lock: None, plane: front_a, start: "0".into(), end: "30".into(), profile: vec![Loop::rect(Coord::on(mid_l), Coord::on(inner_r), Coord::on(floor), Coord::on(head_under))] },
        false,
        "Glass",
        vis(None, true, true, true, false),
    );
    // plan: the two panels as filled outlines
    b.sym(Loop::rect(Coord::on(left), Coord::on(mid_r), Coord::on(rear_a), Coord::on(rear_b)), true, "Panel", vis(None, true, true, true, true));
    b.sym(Loop::rect(Coord::on(mid_l), Coord::on(right), Coord::on(front_a), Coord::on(front_b)), true, "Panel", vis(None, true, true, true, true));
    b.body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;
    use crate::element::{Element, Id};
    use crate::geom::Point;
    use crate::symbol::{plan_symbol_at, SymbolGeom};
    use crate::view::DetailLevel;
    use serde_json::json;

    fn inside(ring: &[Point], p: Point) -> bool {
        let mut c = false;
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                c = !c;
            }
        }
        c
    }

    fn bbox(rings: &[Vec<Point>]) -> [f64; 4] {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for p in rings.iter().flatten() {
            b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
        }
        b
    }

    /// The frame rings of a plan symbol, from either kind of family.
    fn frame_rings(g: &SymbolGeom) -> Vec<Vec<Point>> {
        if g.parts.is_empty() {
            g.frame.clone()
        } else {
            g.parts.iter().filter(|p| p.subcategory == "Frame/Mullion" && p.closed).flat_map(|p| p.rings.clone()).collect()
        }
    }

    fn setup(wall_type: usize, family: &str, type_name: &str, flip: (bool, bool), body: bool, values: serde_json::Value) -> (Document, Id) {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[wall_type].0);
        // a diagonal wall, so the mapping from family to world is tested with a turn in it
        let w = d.add_wall(wt, lvl, Point::new(500., 300.), Point::new(5500., 3300.), 2700.).unwrap();
        // the shipped door is a body; the coded version it replaced is what an empty body falls back to
        let fid = d.families().into_iter().find(|(_, f)| f.name == family).unwrap().0;
        assert!(!d.family(fid).unwrap().body.is_empty(), "the shipped door has a stored body");
        if !body {
            d.set_family_body(fid, FamilyBody::default()).unwrap();
        }
        let t = d.family_types().into_iter().find(|(_, t)| d.family(t.family).unwrap().name == family && t.name == type_name).unwrap().0;
        let id = d.place_on_wall_flipped(t, w, Point::new(3000., 1800.), flip.0, flip.1).unwrap();
        if values.as_object().map_or(false, |o| !o.is_empty()) {
            d.patch_instance(id, &json!({ "values": values })).unwrap();
        }
        (d, id)
    }

    /// Both versions of a door side by side: the same hole, plan symbol at each detail
    /// level and 3D extents.
    fn compare(wall_type: usize, family: &str, type_name: &str, flip: (bool, bool), values: serde_json::Value) {
        let (old, oid) = setup(wall_type, family, type_name, flip, false, values.clone());
        let (new, nid) = setup(wall_type, family, type_name, flip, true, values);
        let tag = format!("{family} {type_name} wall {wall_type} flip {flip:?}");
        // the hole in the wall
        let size = |d: &Document, id: Id| match d.get(id) {
            Some(Element::FamilyInstance(i)) => d.instance_opening_size(i),
            _ => panic!(),
        };
        let (a, b) = (size(&old, oid), size(&new, nid));
        assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6 && (a.2 - b.2).abs() < 1e-6, "{tag}: hole {a:?} {b:?}");
        // the wall opening along the wall
        let wall = |d: &Document, id: Id| match d.get(id) {
            Some(Element::FamilyInstance(i)) => i.host.unwrap().wall,
            _ => panic!(),
        };
        let (oa, ob) = (&old.wall_openings(wall(&old, oid))[0], &new.wall_openings(wall(&new, nid))[0]);
        assert!((oa.u0 - ob.u0).abs() < 1e-6 && (oa.u1 - ob.u1).abs() < 1e-6 && (oa.z1 - ob.z1).abs() < 1e-6, "{tag}: opening");
        for detail in [DetailLevel::Coarse, DetailLevel::Medium, DetailLevel::Fine] {
            let (go, gn) = (plan_symbol_at(&old, oid, detail, Some(1200.0)).unwrap(), plan_symbol_at(&new, nid, detail, Some(1200.0)).unwrap());
            let tag = format!("{tag} {detail:?}");
            // the frame: the same area covered
            let (fo, fnw) = (frame_rings(&go), frame_rings(&gn));
            assert_eq!(fo.is_empty(), fnw.is_empty(), "{tag}: frame present");
            assert_eq!(fo.is_empty(), detail == DetailLevel::Coarse || !family.contains("Flush"), "{tag}: the frame of a flush door shows from Medium up");
            if !fo.is_empty() {
                let bb = bbox(&fo);
                let bn = bbox(&fnw);
                for k in 0..4 {
                    assert!((bb[k] - bn[k]).abs() < 1e-6, "{tag}: frame bounds {bb:?} {bn:?}");
                }
                let mut diff = 0;
                let mut total = 0;
                let (mut x, step) = (bb[0] + 0.37, 1.0);
                while x < bb[2] {
                    let mut y = bb[1] + 0.37;
                    while y < bb[3] {
                        let p = Point::new(x, y);
                        let (i, j) = (fo.iter().any(|r| inside(r, p)), fnw.iter().any(|r| inside(r, p)));
                        total += i as usize;
                        diff += (i != j) as usize;
                        y += step;
                    }
                    x += step;
                }
                // Medium draws outlines, Fine draws material: either way the two cover the same ground
                assert!(total > 100 && diff == 0, "{tag}: frame coverage differs at {diff} of {total} points");
            }
            // the open leaf
            let leaves = |g: &SymbolGeom| -> Vec<[f64; 4]> {
                let mut v: Vec<[f64; 4]> = if g.parts.is_empty() { g.polys.iter().map(|r| bbox(&[r.clone()])).collect() } else { g.parts.iter().filter(|p| p.subcategory == "Panel" && !p.cut).map(|p| bbox(&p.rings)).collect() };
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v
            };
            let (lo, ln) = (leaves(&go), leaves(&gn));
            assert_eq!(lo.len(), ln.len(), "{tag}: leaf count");
            for (p, q) in lo.iter().zip(&ln) {
                for k in 0..4 {
                    assert!((p[k] - q[k]).abs() < 1e-6, "{tag}: open leaf {p:?} {q:?}");
                }
            }
            // the swing: every point of the new arc lies on one of the old arcs, and the ends meet
            for part in gn.parts.iter().filter(|p| p.subcategory == "Plan Swing") {
                let ring = &part.rings[0];
                let on_an_arc = |p: &Point| go.arcs.iter().any(|a| (p.dist(&a.centre) - a.radius).abs() < 0.5);
                assert!(ring.len() >= 3 && ring.iter().all(on_an_arc), "{tag}: swing arc is off the old one");
                let ends = |a: &crate::symbol::Arc| {
                    let pt = |deg: f64| Point::new(a.centre.x + a.radius * deg.to_radians().cos(), a.centre.y + a.radius * deg.to_radians().sin());
                    (pt(a.from), pt(a.to))
                };
                let (first, last) = (ring[0], ring[ring.len() - 1]);
                assert!(
                    go.arcs.iter().any(|a| {
                        let (e0, e1) = ends(a);
                        (first.dist(&e0) < 0.5 && last.dist(&e1) < 0.5) || (first.dist(&e1) < 0.5 && last.dist(&e0) < 0.5)
                    }),
                    "{tag}: swing arc ends"
                );
            }
            assert_eq!(go.arcs.len(), gn.parts.iter().filter(|p| p.subcategory == "Plan Swing").count(), "{tag}: arcs");
            // straight lines: the same segments in each subcategory
            let seg = |a: Point, b: Point| {
                let (p, q) = if (a.x, a.y) <= (b.x, b.y) { (a, b) } else { (b, a) };
                [(p.x * 1e3).round(), (p.y * 1e3).round(), (q.x * 1e3).round(), (q.y * 1e3).round()]
            };
            let mut so: Vec<(&str, [f64; 4])> = go.lines.iter().map(|l| ("Frame/Mullion", seg(l[0], l[1]))).chain(go.glass.iter().map(|l| ("Glass", seg(l[0], l[1])))).collect();
            let mut sn: Vec<(&str, [f64; 4])> = gn.lines.iter().map(|l| ("Frame/Mullion", seg(l[0], l[1]))).collect();
            for p in gn.parts.iter().filter(|p| !p.closed && p.rings[0].len() == 2 && p.subcategory != "Plan Swing") {
                sn.push((if p.subcategory == "Glass" { "Glass" } else { "Frame/Mullion" }, seg(p.rings[0][0], p.rings[0][1])));
            }
            so.sort_by(|a, b| a.partial_cmp(b).unwrap());
            sn.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(so, sn, "{tag}: lines");
        }
        // 3D: the same extents in x, y and z
        let extent = |d: &Document, id: Id| {
            let (_, m, _) = crate::mesh::instance_meshes(d, id).unwrap();
            let mut b = [[f64::MAX, f64::MIN]; 3];
            for c in m.chunks(6) {
                for k in 0..3 {
                    let v = c[k] as f64 * 1000.0;
                    b[k] = [b[k][0].min(v), b[k][1].max(v)];
                }
            }
            b
        };
        let (eo, en) = (extent(&old, oid), extent(&new, nid));
        for k in 0..3 {
            assert!((eo[k][0] - en[k][0]).abs() < 0.01 && (eo[k][1] - en[k][1]).abs() < 0.01, "{tag}: 3D extent {eo:?} {en:?}");
        }
    }

    #[test]
    fn the_single_flush_door_matches_the_coded_one_in_a_stud_wall() {
        compare(0, "Single-Flush", "820 x 2040", (false, false), json!({}));
    }

    #[test]
    fn it_matches_flipped_either_way() {
        for flip in [(true, false), (false, true), (true, true)] {
            compare(0, "Single-Flush", "820 x 2040", flip, json!({}));
        }
    }

    #[test]
    fn it_matches_with_a_timber_frame_on_the_core_of_a_thick_wall() {
        compare(1, "Single-Flush", "920 x 2040", (false, false), json!({}));
        compare(1, "Single-Flush", "820 x 2340", (false, true), json!({}));
    }

    #[test]
    fn it_follows_the_frame_type_chosen_on_the_instance() {
        compare(0, "Single-Flush", "820 x 2040", (false, false), json!({ "Frame Type": "Timber" }));
    }

    #[test]
    fn the_double_door_matches_too() {
        compare(0, "Double-Flush", "2 x 820 x 2040", (false, false), json!({}));
        compare(1, "Double-Flush", "2 x 720 x 2040", (true, false), json!({}));
    }

    #[test]
    fn projects_saved_with_the_coded_doors_get_the_stored_bodies_on_opening() {
        let mut d = Document::new_project();
        let fid = d.families().into_iter().find(|(_, f)| f.name == "Single-Flush").unwrap().0;
        d.set_family_body(fid, FamilyBody::default()).unwrap();
        let t = d.family_types().into_iter().find(|(_, t)| t.family == fid && t.name == "820 x 2040").unwrap().0;
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[0].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let id = d.place_on_wall(t, w, Point::new(2000., 0.)).unwrap();
        let path = std::env::temp_dir().join(format!("openarch-door-upgrade-{}.oab", std::process::id()));
        d.save(&path).unwrap();
        let o = Document::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let fid2 = o.families().into_iter().find(|(_, f)| f.name == "Single-Flush").unwrap().0;
        assert!(!o.family(fid2).unwrap().body.is_empty());
        // the placed door and its type survive and still cut the same hole
        assert_eq!(o.wall_openings(w).len(), 1);
        assert!(o.instance_solved(id).is_some());
    }

    #[test]
    fn doors_saved_with_extruded_frame_pieces_get_the_swept_frame_on_opening() {
        let mut d = Document::new_project();
        for name in ["Single-Flush", "Fixed"] {
            let fid = d.families().into_iter().find(|(_, f)| f.name == name).unwrap().0;
            // the earlier body: a frame made of separate extrusions
            let mut old = d.family(fid).unwrap().body.clone();
            for f in &mut old.forms {
                if f.name.ends_with("Frame") {
                    f.name = f.name.replace("Frame", "Jambs");
                    f.shape = FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_LEVEL, start: "0".into(), end: "10".into(), profile: vec![Loop::rect(Coord::at(0.0), Coord::at(10.0), Coord::at(0.0), Coord::at(10.0))] };
                }
            }
            if name == "Fixed" {
                for f in &mut old.forms {
                    if f.name == "Jambs" {
                        f.name = "Frame".into();
                    }
                }
            }
            d.set_family_body(fid, old).unwrap();
        }
        d.ensure_starter_families();
        for name in ["Single-Flush", "Fixed"] {
            let fid = d.families().into_iter().find(|(_, f)| f.name == name).unwrap().0;
            assert!(d.family(fid).unwrap().body.forms.iter().any(|f| matches!(f.shape, FormShape::Sweep { .. })), "{name}");
        }
    }

    #[test]
    fn the_windows_match_the_coded_ones() {
        for ty in ["600 x 1200", "1800 x 1200", "2400 x 2100"] {
            compare(0, "Fixed", ty, (false, false), json!({}));
        }
        compare(1, "Awning", "900 x 600", (true, false), json!({}));
        compare(0, "Sliding", "1800 x 1200", (false, true), json!({}));
    }

    #[test]
    fn the_sliding_door_keeps_its_plan_and_gains_glass_panels() {
        compare(1, "Sliding-Glass", "2400 x 2100 Exterior", (false, false), json!({}));
        let (d, id) = setup(1, "Sliding-Glass", "1800 x 2100 Exterior", (false, false), true, json!({}));
        let (_, solid, glass) = crate::mesh::instance_meshes(&d, id).unwrap();
        assert!(!solid.is_empty() && !glass.is_empty(), "frame and two panes of glass");
    }

    #[test]
    fn the_frames_are_sweeps_driven_by_the_type() {
        for body in [flush_door_body(false), flush_door_body(true), window_body(), sliding_door_body()] {
            let sweeps = body.forms.iter().filter(|f| matches!(f.shape, FormShape::Sweep { .. })).count();
            assert!(sweeps >= 1, "every starter frame is a sweep");
            assert!(!body.forms.iter().any(|f| f.name.contains("Head") || f.name.contains("Jamb")), "no hand-built jamb or head pieces remain");
        }
    }
}
