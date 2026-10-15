//! Frames of the built-in doors, built from the door type's parameters, and the
//! sizes that follow from them.
//!
//! Two frames are made. A pressed metal frame has a double rebate section: a centre
//! rib with a recess for the leaf on each side, and the architrave pressed into the
//! profile. It is the usual frame for internal and thin walls. A timber frame is a
//! rebated jamb built into the opening with no architrave, used where the wall is
//! thick (external masonry and veneer walls). Custom families carry no frame.
//!
//! The section is drawn in (a, b) coordinates. `a` runs from the reveal edge (the
//! clear opening the leaf sits in) outwards into the wall, negative into the
//! opening. `b` runs across the wall, centred on the frame, positive on the side the
//! door swings to. Jambs sweep the section up the opening and the head sweeps it
//! across the top, so one set of parameters drives the plan symbol at every detail
//! level and the 3D frame. A frame from a family editor sweep will replace `section`
//! with a stored outline; the callers only use `section`.

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::family::{FamilyCategory, PlanSymbol};

/// A rectangle of material in section: `[a0, a1, b0, b1]`.
pub type Piece = [f64; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    PressedMetal,
    Timber,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DoorFrame {
    pub kind: FrameKind,
    /// Clear opening between the jamb rebates, and its height from the floor.
    pub reveal_w: f64,
    pub reveal_h: f64,
    /// Depth across the wall: the back opening of a metal frame, the width of a
    /// timber jamb.
    pub depth: f64,
    /// Metal: sheet thickness, rib size, architrave width along the wall face and how
    /// far it stands proud of the wall.
    pub gauge: f64,
    pub rib_w: f64,
    pub rib_h: f64,
    pub face: f64,
    pub proud: f64,
    /// Timber: jamb thickness, the stop's size and where it starts from the back face.
    pub jamb_t: f64,
    pub stop_w: f64,
    pub stop_h: f64,
    pub stop_off: f64,
    /// Leaf size: width of each leaf, height and thickness.
    pub leaf_w: f64,
    pub leaf_h: f64,
    pub leaf_t: f64,
    pub leaves: usize,
    /// Gap between the leaf and the reveal at each side, and under the leaf.
    pub side_gap: f64,
    pub bottom_gap: f64,
    /// How far the frame's centre lies to the left of the wall's centre (a timber
    /// frame is centred on the wall's core).
    pub shift: f64,
}

impl DoorFrame {
    /// Overall depth of a metal frame over its architrave.
    fn overall(&self) -> f64 {
        self.depth + 2.0 * self.proud
    }

    /// The material in section. Metal: the centre rib as a hat, a recess floor on
    /// each side, and on each wall face the architrave leg standing proud with its
    /// return. Timber: the jamb and the planted stop.
    pub fn section(&self) -> Vec<Piece> {
        match self.kind {
            FrameKind::PressedMetal => {
                let (t, rw, rh) = (self.gauge, self.rib_w, self.rib_h);
                let (d2, w2) = (self.overall() / 2.0, self.depth / 2.0);
                let face = self.face.max(2.0 * t);
                let mut v = vec![
                    [-rh, -rh + t, -rw / 2.0, rw / 2.0], // the rib's crown
                    [-rh, 0.0, -rw / 2.0, -rw / 2.0 + t], // its two sides
                    [-rh, 0.0, rw / 2.0 - t, rw / 2.0],
                ];
                for side in [-1.0f64, 1.0] {
                    let m = |a0: f64, a1: f64, b0: f64, b1: f64| if side < 0.0 { [a0, a1, -b1, -b0] } else { [a0, a1, b0, b1] };
                    v.push(m(0.0, t, rw / 2.0, d2)); // recess floor
                    v.push(m(0.0, face, d2 - t, d2)); // architrave leg
                    v.push(m(face - t, face, w2, d2 - t)); // its return onto the wall face
                }
                v
            }
            FrameKind::Timber => {
                let h2 = self.depth / 2.0;
                let s0 = -h2 + self.stop_off;
                vec![[0.0, self.jamb_t, -h2, h2], [-self.stop_h, 0.0, s0, s0 + self.stop_w]]
            }
        }
    }

    /// The smallest rectangle that holds the section.
    pub fn envelope(&self) -> Piece {
        match self.kind {
            FrameKind::PressedMetal => [-self.rib_h, self.face.max(2.0 * self.gauge), -self.overall() / 2.0, self.overall() / 2.0],
            FrameKind::Timber => [-self.stop_h, self.jamb_t, -self.depth / 2.0, self.depth / 2.0],
        }
    }

    /// Half the width across the outside of the frame, measured from the centre of the opening.
    pub fn outer_half(&self) -> f64 {
        self.reveal_w / 2.0 + self.envelope()[1]
    }

    /// How far the frame reaches back from the reveal into the wall (for the head).
    pub fn reach(&self) -> f64 {
        self.envelope()[1]
    }

    /// Where the closed leaf's back face sits across the wall: against the rib or stop.
    pub fn leaf_b0(&self) -> f64 {
        match self.kind {
            FrameKind::PressedMetal => self.rib_w / 2.0,
            FrameKind::Timber => -self.depth / 2.0 + self.stop_off + self.stop_w,
        }
    }

    /// Hinge positions along the opening, with the leaf length. A single leaf
    /// hinges at the wall start side unless `flip_hand`; a pair hinges at both jambs.
    pub fn hinges(&self, flip_hand: bool) -> Vec<f64> {
        let edge = self.reveal_w / 2.0 - self.side_gap;
        if self.leaves == 2 {
            vec![-edge, edge]
        } else if flip_hand {
            vec![edge]
        } else {
            vec![-edge]
        }
    }

    /// Opening in the wall: the frame's outside, reveal plus the frame's material.
    pub fn wall_opening(&self) -> (f64, f64) {
        let m = match self.kind {
            FrameKind::PressedMetal => self.gauge,
            FrameKind::Timber => self.jamb_t,
        };
        (self.reveal_w + 2.0 * m, self.reveal_h + m)
    }
}

impl Document {
    /// The frame of a door instance, or `None` for a door without one (custom families).
    pub fn door_frame(&self, id: Id) -> Option<DoorFrame> {
        let Some(Element::FamilyInstance(inst)) = self.get(id) else { return None };
        let (fam, _) = self.instance_family(inst)?;
        if fam.category != FamilyCategory::Doors {
            return None;
        }
        let kind = match self.instance_param(inst, "Frame Type")?.as_text()? {
            "Pressed Metal" => FrameKind::PressedMetal,
            "Timber" => FrameKind::Timber,
            _ => return None,
        };
        let n = |name: &str| self.instance_param(inst, name).and_then(|v| v.as_f64());
        let leaves = if fam.plan_symbol == PlanSymbol::DoubleSwing { 2 } else { 1 };
        let clear_w = n("Frame Clearance Width")?;
        let shift = match (kind, inst.host.and_then(|h| self.get(h.wall))) {
            (FrameKind::Timber, Some(Element::Wall(w))) => self.wall_core(w).1,
            _ => 0.0,
        };
        Some(DoorFrame {
            kind,
            reveal_w: n("Reveal Width")?,
            reveal_h: n("Reveal Height")?,
            depth: n("Jamb Depth")?,
            gauge: n("Steel Gauge")?,
            rib_w: n("Rib Width")?,
            rib_h: n("Rib Height")?,
            face: n("Architrave Width")?,
            proud: n("Architrave Projection")?,
            jamb_t: n("Jamb Thickness")?,
            stop_w: n("Stop Width")?,
            stop_h: n("Stop Height")?,
            stop_off: n("Stop Offset")?,
            leaf_w: n("Width")? / leaves as f64,
            leaf_h: n("Height")?,
            leaf_t: n("Thickness")?,
            leaves,
            side_gap: clear_w / (leaves as f64 + 1.0),
            bottom_gap: n("Leaf Bottom Gap")?,
            shift,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;

    fn t_named(d: &Document, family: &str, name: &str) -> Id {
        d.family_types().into_iter().find(|(_, t)| d.family(t.family).unwrap().name == family && t.name == name).unwrap().0
    }

    /// A 6 m wall of the given type index and the single 820 x 2040 type.
    fn setup(wall_type: usize) -> (Document, Id, Id) {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[wall_type].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let t = t_named(&d, "Single-Flush", "820 x 2040");
        (d, w, t)
    }

    fn inst(d: &Document, id: Id) -> &crate::family::FamilyInstance {
        match d.get(id) { Some(Element::FamilyInstance(i)) => i, _ => panic!() }
    }

    #[test]
    fn a_thin_wall_gets_a_pressed_metal_frame_sized_by_formula() {
        let (mut d, w, t) = setup(0); // Internal Stud 90: 13 + 90 + 13 = 116
        let id = d.place_on_wall(t, w, Point::new(2000., 100.)).unwrap();
        let f = d.door_frame(id).unwrap();
        assert_eq!((f.kind, f.depth), (FrameKind::PressedMetal, 112.0));
        // leaf 2040 x 820 x 35 in a 2055 x 824 reveal, and the hole adds the sheet each side
        assert_eq!((f.reveal_w, f.reveal_h, f.leaf_t), (824.0, 2055.0, 35.0));
        let (ow, oh, _) = d.instance_opening_size(inst(&d, id));
        assert!((ow - 826.4).abs() < 1e-9 && (oh - 2056.2).abs() < 1e-9, "{ow} {oh}");
        // the calculated values cannot be set, but their inputs can
        assert!(d.patch_family_type(t, &serde_json::json!({ "values": { "Rough Width": 900.0 } })).unwrap_err().contains("formula"));
        d.patch_family_type(t, &serde_json::json!({ "values": { "Frame Clearance Width": 6.0 } })).unwrap();
        assert_eq!(d.door_frame(id).unwrap().reveal_w, 826.0);
    }

    #[test]
    fn a_thick_wall_gets_a_timber_jamb_on_its_core() {
        let (mut d, w, t) = setup(1); // Brick Veneer
        let id = d.place_on_wall(t, w, Point::new(2000., 0.)).unwrap();
        let f = d.door_frame(id).unwrap();
        assert_eq!(f.kind, FrameKind::Timber);
        assert!(TIMBER_JAMBS_OK.contains(&f.depth), "{}", f.depth);
        // no architrave: the whole section stays inside the jamb depth
        let e = f.envelope();
        assert!(e[2] >= -f.depth / 2.0 - 1e-9 && e[3] <= f.depth / 2.0 + 1e-9);
        // and it is centred on the core, not on the wall body
        let Some(Element::Wall(wall)) = d.get(w) else { panic!() };
        assert!((f.shift - d.wall_core(wall).1).abs() < 1e-9);
        assert!(f.shift.abs() > 1.0, "{}", f.shift);
        // the hole is the reveal plus the jamb each side
        let (ow, _, _) = d.instance_opening_size(inst(&d, id));
        assert!((ow - (f.reveal_w + 2.0 * f.jamb_t)).abs() < 1e-9);
    }

    const TIMBER_JAMBS_OK: [f64; 3] = crate::family::TIMBER_JAMBS;

    #[test]
    fn changing_the_frame_type_changes_the_hole() {
        let (mut d, w, t) = setup(0);
        let id = d.place_on_wall(t, w, Point::new(2000., 0.)).unwrap();
        d.patch_instance(id, &serde_json::json!({ "values": { "Frame Type": "Timber" } })).unwrap();
        let (ow, oh, _) = d.instance_opening_size(inst(&d, id));
        assert!((ow - (824.0 + 84.0)).abs() < 1e-9 && (oh - (2055.0 + 42.0)).abs() < 1e-9, "{ow} {oh}");
        assert_eq!(d.door_frame(id).unwrap().kind, FrameKind::Timber);
    }

    #[test]
    fn a_double_set_is_two_leaves_with_a_gap_between() {
        let (mut d, w, _) = setup(0);
        let t = t_named(&d, "Double-Flush", "2 x 820 x 2040");
        let id = d.place_on_wall(t, w, Point::new(3000., 0.)).unwrap();
        let f = d.door_frame(id).unwrap();
        assert_eq!((f.leaves, f.leaf_w, f.reveal_w), (2, 820.0, 1648.0));
        assert!((f.side_gap - 8.0 / 3.0).abs() < 1e-9);
        let h = f.hinges(false);
        assert!((h[0] + (824.0 - f.side_gap)).abs() < 1e-9 && (h[1] - (824.0 - f.side_gap)).abs() < 1e-9);
    }

    #[test]
    fn the_depth_follows_the_wall_and_a_poor_fit_is_reported() {
        let (mut d, _, t) = setup(0);
        let (lvl, stud, concrete) = (d.levels()[0].0, d.wall_types()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(stud, lvl, Point::new(0., 5000.), Point::new(6000., 5000.), 2700.).unwrap();
        let id = d.place_on_wall(t, w, Point::new(2000., 5000.)).unwrap();
        assert_eq!(d.instance_param(inst(&d, id), "Jamb Depth").unwrap().as_f64().unwrap(), 112.0);
        assert!(d.frame_fit(id).is_none());
        d.patch_instance(id, &serde_json::json!({ "values": { "Jamb Depth": 92.0 } })).unwrap();
        assert!(d.frame_fit(id).unwrap().contains("shallower"));
        d.patch_instance(id, &serde_json::json!({ "values": { "Jamb Depth": 192.0 } })).unwrap();
        assert!(d.frame_fit(id).unwrap().contains("deeper"));
        // a metal frame forced into a thick concrete wall is reported
        let w2 = d.add_wall(concrete, lvl, Point::new(0., 8000.), Point::new(6000., 8000.), 2700.).unwrap();
        let id2 = d.place_on_wall(t, w2, Point::new(2000., 8000.)).unwrap();
        d.patch_instance(id2, &serde_json::json!({ "values": { "Frame Type": "Pressed Metal", "Jamb Depth": 112.0 } })).unwrap();
        assert!(d.frame_fit(id2).unwrap().contains("shallower"));
    }

    #[test]
    fn the_section_keeps_the_leaf_in_the_recess() {
        for kind in ["Pressed Metal", "Timber"] {
            let (mut d, w, t) = setup(0);
            let id = d.place_on_wall(t, w, Point::new(2000., 0.)).unwrap();
            d.patch_instance(id, &serde_json::json!({ "values": { "Frame Type": kind } })).unwrap();
            let f = d.door_frame(id).unwrap();
            let e = f.envelope();
            // the leaf clears the rib or stop and the swing side of the frame
            assert!(f.leaf_b0() + f.leaf_t < e[3] - 1e-9, "{kind}");
            assert!(f.leaf_b0() > e[2], "{kind}");
            for p in f.section() {
                assert!(p[0] >= e[0] - 1e-9 && p[1] <= e[1] + 1e-9 && p[2] >= e[2] - 1e-9 && p[3] <= e[3] + 1e-9, "{kind} {p:?}");
            }
        }
    }

    #[test]
    fn the_3d_frame_and_leaf_have_the_right_extents() {
        let (mut d, w, t) = setup(0);
        let id = d.place_on_wall(t, w, Point::new(2000., 300.)).unwrap();
        let (_, frame, glass) = crate::mesh::instance_meshes(&d, id).unwrap();
        assert!(glass.is_empty());
        let bounds = |m: &[f32], k: usize| {
            let v: Vec<f32> = m.chunks(6).map(|c| c[k]).collect();
            (v.iter().cloned().fold(f32::MAX, f32::min) as f64 * 1000.0, v.iter().cloned().fold(f32::MIN, f32::max) as f64 * 1000.0)
        };
        let f = d.door_frame(id).unwrap();
        // x: reveal 824 wide, metal and architrave beyond it on both sides
        let (x0, x1) = bounds(&frame, 0);
        assert!((x0 - (2000.0 - 412.0 - f.face)).abs() < 0.5 && (x1 - (2000.0 + 412.0 + f.face)).abs() < 0.5, "{x0} {x1}");
        // z: from the floor to the top of the head
        let (z0, z1) = bounds(&frame, 2);
        assert!(z0.abs() < 0.01 && (z1 - (2055.0 + f.face)).abs() < 0.5, "{z0} {z1}");
        // the leaf sits above the floor gap and is 2040 high
        let leaf_top = 20.0 + 2040.0;
        assert!(frame.chunks(6).any(|c| (c[2] as f64 * 1000.0 - leaf_top).abs() < 0.01));
    }
}
