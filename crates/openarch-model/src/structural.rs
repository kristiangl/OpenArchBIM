//! Structural columns and beams.
//!
//! A structural type is a section (a concrete or timber rectangle, a round, a steel universal column or beam, a
//! parallel flange channel, a square, rectangular or circular hollow section) and a material, for columns or for
//! framing. A column stands at a point from its base level (plus an offset) to a top level (plus an offset), or to
//! a height of its own when it has no top level, turned about its axis. A beam runs between two points at a level
//! (each end with its own offset, so it can slope), with its section set against the line by a justification (top
//! of beam on the line is the usual one, so the beam hangs under the level), offsets and a rotation, and a usage
//! (girder, joist, purlin, bracing).
//!
//! A beam's end that lands on a column, or on another beam's length, is joined to it: cut back to the support's
//! face plus a gap (the cutback). Two beam ends that meet at a corner are mitred. A join can be switched off at
//! either end, and an end can be extended. A column can lean (its top at another plan point), and its top can be
//! attached to a roof or a floor: it then stops at the underside and is cut to its slope.

use serde::{Deserialize, Serialize};

use crate::doc::apply_patch;
use crate::edit::Xform;
use crate::shape::Tin;
use crate::{Document, Element, Id, Point};

/// A cross-section, in millimetres. Across is the section's width (a beam's horizontal), up its depth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Section {
    Rectangle { b: f64, d: f64 },
    Round { d: f64 },
    /// Universal beam or column: depth, flange width, flange thickness, web thickness.
    I { d: f64, bf: f64, tf: f64, tw: f64 },
    /// Parallel flange channel, toes to the right.
    Channel { d: f64, bf: f64, tf: f64, tw: f64 },
    /// Square or rectangular hollow section: width, depth, wall thickness.
    RectangularHollow { b: f64, d: f64, t: f64 },
    CircularHollow { d: f64, t: f64 },
}

const ROUND_SIDES: usize = 24;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn circle(r: f64) -> Vec<[f64; 2]> {
    (0..ROUND_SIDES).map(|i| { let a = std::f64::consts::TAU * i as f64 / ROUND_SIDES as f64; [r * a.cos(), r * a.sin()] }).collect()
}

impl Section {
    /// Width and depth overall.
    pub fn size(&self) -> (f64, f64) {
        match *self {
            Section::Rectangle { b, d } | Section::RectangularHollow { b, d, .. } => (b, d),
            Section::Round { d } | Section::CircularHollow { d, .. } => (d, d),
            Section::I { d, bf, .. } | Section::Channel { d, bf, .. } => (bf, d),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        let (w, d) = self.size();
        let ok = |v: f64| v.is_finite() && v > 0.0 && v < 20_000.0;
        if !ok(w) || !ok(d) { return Err("A section's width and depth are more than zero.".into()); }
        match *self {
            Section::I { d, bf, tf, tw } | Section::Channel { d, bf, tf, tw } => {
                if !ok(tf) || !ok(tw) || tf * 2.0 >= d || tw >= bf { return Err("The flange and web are thinner than the section.".into()); }
            }
            Section::RectangularHollow { b, d, t } => { if !ok(t) || t * 2.0 >= b.min(d) { return Err("The wall is thinner than half the section.".into()); } }
            Section::CircularHollow { d, t } => { if !ok(t) || t * 2.0 >= d { return Err("The wall is thinner than half the section.".into()); } }
            _ => {}
        }
        Ok(())
    }

    /// The section as convex pieces (anticlockwise), about its centre: what the ends of a member are made of.
    pub fn parts(&self) -> Vec<Vec<[f64; 2]>> {
        match *self {
            Section::Rectangle { b, d } => vec![rect(-b / 2.0, -d / 2.0, b / 2.0, d / 2.0)],
            Section::Round { d } => vec![circle(d / 2.0)],
            Section::I { d, bf, tf, tw } => vec![
                rect(-bf / 2.0, d / 2.0 - tf, bf / 2.0, d / 2.0),
                rect(-tw / 2.0, -d / 2.0 + tf, tw / 2.0, d / 2.0 - tf),
                rect(-bf / 2.0, -d / 2.0, bf / 2.0, -d / 2.0 + tf),
            ],
            Section::Channel { d, bf, tf, tw } => vec![
                rect(-bf / 2.0, d / 2.0 - tf, bf / 2.0, d / 2.0),
                rect(-bf / 2.0, -d / 2.0 + tf, -bf / 2.0 + tw, d / 2.0 - tf),
                rect(-bf / 2.0, -d / 2.0, bf / 2.0, -d / 2.0 + tf),
            ],
            Section::RectangularHollow { b, d, t } => vec![
                rect(-b / 2.0, d / 2.0 - t, b / 2.0, d / 2.0),
                rect(-b / 2.0, -d / 2.0, b / 2.0, -d / 2.0 + t),
                rect(-b / 2.0, -d / 2.0 + t, -b / 2.0 + t, d / 2.0 - t),
                rect(b / 2.0 - t, -d / 2.0 + t, b / 2.0, d / 2.0 - t),
            ],
            Section::CircularHollow { d, t } => {
                let (o, i) = (circle(d / 2.0), circle(d / 2.0 - t));
                (0..ROUND_SIDES).map(|k| { let n = (k + 1) % ROUND_SIDES; vec![o[k], o[n], i[n], i[k]] }).collect()
            }
        }
    }

    /// The outline (anticlockwise) and, for a hollow section, the inside of its wall (anticlockwise too).
    pub fn rings(&self) -> (Vec<[f64; 2]>, Option<Vec<[f64; 2]>>) {
        match *self {
            Section::Rectangle { b, d } => (rect(-b / 2.0, -d / 2.0, b / 2.0, d / 2.0), None),
            Section::Round { d } => (circle(d / 2.0), None),
            Section::I { d, bf, tf, tw } => {
                let (f, h, w) = (bf / 2.0, d / 2.0, tw / 2.0);
                (vec![[-f, -h], [f, -h], [f, -h + tf], [w, -h + tf], [w, h - tf], [f, h - tf], [f, h], [-f, h], [-f, h - tf], [-w, h - tf], [-w, -h + tf], [-f, -h + tf]], None)
            }
            Section::Channel { d, bf, tf, tw } => {
                let (f, h) = (bf / 2.0, d / 2.0);
                (vec![[-f, -h], [f, -h], [f, -h + tf], [-f + tw, -h + tf], [-f + tw, h - tf], [f, h - tf], [f, h], [-f, h]], None)
            }
            Section::RectangularHollow { b, d, t } => (rect(-b / 2.0, -d / 2.0, b / 2.0, d / 2.0), Some(rect(-b / 2.0 + t, -d / 2.0 + t, b / 2.0 - t, d / 2.0 - t))),
            Section::CircularHollow { d, t } => (circle(d / 2.0), Some(circle(d / 2.0 - t))),
        }
    }

    /// The area of steel, concrete or timber in the section, mm².
    pub fn area_mm2(&self) -> f64 {
        self.parts().iter().map(|p| crate::geom::signed_area(&p.iter().map(|q| Point::new(q[0], q[1])).collect::<Vec<_>>()).abs()).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralUse {
    Column,
    Framing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralType {
    /// The family the type belongs to ("Universal Beam", "Concrete Rectangular Column").
    pub family: String,
    pub name: String,
    pub used_for: StructuralUse,
    pub section: Section,
    /// The structural material, by name.
    pub material: String,
    #[serde(default)]
    pub info: crate::element::TypeInfo,
}

fn default_height() -> f64 { 3000.0 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralColumn {
    pub structural_type: Id,
    /// Base Level.
    pub level: Id,
    pub at: Point,
    #[serde(default)]
    pub base_offset_mm: f64,
    /// Top Level; none makes the column `height_mm` high from its base.
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub top_offset_mm: f64,
    #[serde(default = "default_height")]
    pub height_mm: f64,
    /// Turned about its axis, anticlockwise.
    #[serde(default)]
    pub rotation_deg: f64,
    /// A slanted column: where its top is in plan. None stands it plumb over `at`.
    #[serde(default)]
    pub top_at: Option<Point>,
    /// A roof or floor the top is attached to: the column stops at its underside (plus the top offset) and is cut
    /// to its slope, wherever the top level is.
    #[serde(default)]
    pub top_attach: Option<Id>,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub pinned: bool,
}

impl StructuralColumn {
    /// A plumb column at a point, 3 m high until it is given a top level.
    pub fn new(structural_type: Id, level: Id, at: Point) -> StructuralColumn {
        StructuralColumn { structural_type, level, at, base_offset_mm: 0.0, top_level: None, top_offset_mm: 0.0, height_mm: 3000.0, rotation_deg: 0.0, top_at: None, top_attach: None, mark: String::new(), comments: String::new(), pinned: false }
    }
}

/// Where the section sits against the beam's line, up and down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZJustification {
    /// The top of the beam on the line: it hangs under the level.
    #[default]
    Top,
    Center,
    Bottom,
}

/// And across, looking from start to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum YJustification {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FramingUsage {
    Girder,
    #[default]
    Joist,
    Purlin,
    HorizontalBracing,
    VerticalBracing,
    Other,
}

impl FramingUsage {
    pub const ALL: [FramingUsage; 6] = [FramingUsage::Girder, FramingUsage::Joist, FramingUsage::Purlin, FramingUsage::HorizontalBracing, FramingUsage::VerticalBracing, FramingUsage::Other];
    pub fn label(self) -> &'static str {
        match self {
            FramingUsage::Girder => "Girder",
            FramingUsage::Joist => "Joist",
            FramingUsage::Purlin => "Purlin",
            FramingUsage::HorizontalBracing => "Horizontal Bracing",
            FramingUsage::VerticalBracing => "Vertical Bracing",
            FramingUsage::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beam {
    pub structural_type: Id,
    /// Reference Level.
    pub level: Id,
    pub start: Point,
    pub end: Point,
    /// Each end's height above the level.
    #[serde(default)]
    pub start_offset_mm: f64,
    #[serde(default)]
    pub end_offset_mm: f64,
    #[serde(default)]
    pub z_justification: ZJustification,
    #[serde(default)]
    pub y_justification: YJustification,
    #[serde(default)]
    pub z_offset_mm: f64,
    #[serde(default)]
    pub y_offset_mm: f64,
    /// Cross-Section Rotation about the beam's line.
    #[serde(default)]
    pub rotation_deg: f64,
    #[serde(default)]
    pub usage: FramingUsage,
    /// Whether each end joins what it lands on (a column, a beam's length, another beam's end).
    #[serde(default = "yes")]
    pub start_join: bool,
    #[serde(default = "yes")]
    pub end_join: bool,
    /// Each end made longer (or shorter, negative) than its join leaves it.
    #[serde(default)]
    pub start_extension_mm: f64,
    #[serde(default)]
    pub end_extension_mm: f64,
    /// The gap left between a joined end and its support's face. None is the usual gap for the material
    /// (10 for steel, none for concrete and timber).
    #[serde(default)]
    pub cutback_mm: Option<f64>,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub pinned: bool,
}

fn yes() -> bool { true }

impl Beam {
    /// A joist between two points with its top on the level.
    pub fn new(structural_type: Id, level: Id, start: Point, end: Point) -> Beam {
        Beam { structural_type, level, start, end, start_offset_mm: 0.0, end_offset_mm: 0.0, z_justification: ZJustification::Top, y_justification: YJustification::Center, z_offset_mm: 0.0, y_offset_mm: 0.0, rotation_deg: 0.0,
            usage: FramingUsage::Joist, start_join: true, end_join: true, start_extension_mm: 0.0, end_extension_mm: 0.0, cutback_mm: None, mark: String::new(), comments: String::new(), pinned: false }
    }
}

/// The gap a joined end of this material leaves by default.
pub fn default_cutback(material: &str) -> f64 {
    if material.to_lowercase().contains("steel") { 10.0 } else { 0.0 }
}

/// What a beam's end does: how much shorter it is than its line (less for an extension), the plane it is cut to
/// (a point and the outward normal; square with none) and what it joined.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EndTrim {
    pub cut_mm: f64,
    pub plane: Option<(V3, V3)>,
    pub joined: Option<Id>,
}

pub type V3 = [f64; 3];
fn dot(a: V3, b: V3) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn add(a: V3, b: V3, s: f64) -> V3 { [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s] }
fn cross(a: V3, b: V3) -> V3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn unit(a: V3) -> V3 { let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-12); [a[0] / l, a[1] / l, a[2] / l] }

/// A member's solid: where its section's centre starts, the section's across and up directions, its axis, and
/// its length (mm).
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub origin: V3,
    pub across: V3,
    pub up: V3,
    pub axis: V3,
    pub length: f64,
    /// The planes its ends are cut to (a point on the plane and its outward normal); square ends with none.
    pub start_cut: Option<(V3, V3)>,
    pub end_cut: Option<(V3, V3)>,
}

impl Member {
    fn at(&self, q: [f64; 2], along: f64) -> V3 {
        add(add(add(self.origin, self.across, q[0]), self.up, q[1]), self.axis, along)
    }

    /// A corner of the section at one end, on that end's cut plane.
    pub fn corner(&self, q: [f64; 2], end: bool) -> V3 {
        let (t0, cut) = if end { (self.length, self.end_cut) } else { (0.0, self.start_cut) };
        let Some((p, n)) = cut else { return self.at(q, t0) };
        let den = dot(self.axis, n);
        // a plane nearly along the member cuts nothing sensible
        if den.abs() < 0.15 { return self.at(q, t0); }
        let from = self.at(q, 0.0);
        self.at(q, dot([p[0] - from[0], p[1] - from[1], p[2] - from[2]], n) / den)
    }

    /// The same member square ended and shorter (or longer) at each end, then cut to the planes.
    pub fn trimmed(self, ends: &[EndTrim; 2]) -> Member {
        let (a, b) = (ends[0].cut_mm, ends[1].cut_mm);
        if self.length - a - b < 1.0 { return self; }
        Member { origin: add(self.origin, self.axis, a), length: self.length - a - b, start_cut: ends[0].plane, end_cut: ends[1].plane, ..self }
    }

    /// The solid as triangles: position and normal for each corner, in metres.
    pub fn mesh(&self, s: &Section) -> Vec<f32> {
        let mut out: Vec<f32> = vec![];
        let mut push = |p: V3, n: V3| out.extend([p[0] / 1000.0, p[1] / 1000.0, p[2] / 1000.0, n[0], n[1], n[2]].map(|v| v as f32));
        let back = self.start_cut.map_or([-self.axis[0], -self.axis[1], -self.axis[2]], |c| c.1);
        let front = self.end_cut.map_or(self.axis, |c| c.1);
        for part in s.parts() {
            for k in 1..part.len().saturating_sub(1) {
                // the far end faces along the axis, the near end back along it
                for q in [part[0], part[k], part[k + 1]] { push(self.corner(q, true), front); }
                for q in [part[0], part[k + 1], part[k]] { push(self.corner(q, false), back); }
            }
        }
        let (outer, inner) = s.rings();
        let mut side = |ring: &[[f64; 2]], outward: f64| {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
                let l = ex.hypot(ey).max(1e-12);
                // to the right of an anticlockwise edge is outside
                let nrm = unit(add(add([0.0; 3], self.across, ey / l * outward), self.up, -ex / l * outward));
                let (a0, b0, a1, b1) = (self.corner(a, false), self.corner(b, false), self.corner(a, true), self.corner(b, true));
                let quad = if outward > 0.0 { [a0, b0, b1, a0, b1, a1] } else { [a0, b1, b0, a0, a1, b1] };
                for p in quad { push(p, nrm); }
            }
        };
        side(&outer, 1.0);
        if let Some(inner) = inner { side(&inner, -1.0); }
        out
    }
}

/// A grey for a member by what it is made of, where the project has no such material.
fn fallback_colour(material: &str) -> [u8; 3] {
    let m = material.to_lowercase();
    if m.contains("steel") { [150, 156, 166] } else if m.contains("timber") || m.contains("wood") { [198, 166, 120] } else { [196, 196, 190] }
}

impl Document {
    pub fn structural_types(&self) -> Vec<(Id, &StructuralType)> {
        self.iter().filter_map(|(id, e)| match e { Element::StructuralType(t) => Some((id, t)), _ => None }).collect()
    }
    pub fn structural_type(&self, id: Id) -> Option<&StructuralType> {
        match self.get(id) { Some(Element::StructuralType(t)) => Some(t), _ => None }
    }
    pub fn structural_columns(&self) -> Vec<(Id, &StructuralColumn)> {
        self.iter().filter_map(|(id, e)| match e { Element::StructuralColumn(c) => Some((id, c)), _ => None }).collect()
    }
    pub fn beams(&self) -> Vec<(Id, &Beam)> {
        self.iter().filter_map(|(id, e)| match e { Element::Beam(b) => Some((id, b)), _ => None }).collect()
    }

    /// The starter types, for a project that has none. Steel sizes are the common Australian open and hollow
    /// sections (AS/NZS 3679.1, AS/NZS 1163), to the nearest tenth of a millimetre from the usual section tables.
    pub fn ensure_structural_types(&mut self) -> Result<(), String> {
        if !self.structural_types().is_empty() { return Ok(()); }
        use Section::*;
        use StructuralUse::*;
        let list: Vec<(&str, &str, StructuralUse, Section, &str)> = vec![
            ("Concrete Rectangular Column", "300 x 300", Column, Rectangle { b: 300.0, d: 300.0 }, "Concrete"),
            ("Concrete Rectangular Column", "450 x 450", Column, Rectangle { b: 450.0, d: 450.0 }, "Concrete"),
            ("Concrete Round Column", "450 dia", Column, Round { d: 450.0 }, "Concrete"),
            ("Universal Column", "150UC30.0", Column, I { d: 158.0, bf: 153.0, tf: 9.4, tw: 6.6 }, "Steel"),
            ("Universal Column", "200UC46.2", Column, I { d: 203.0, bf: 203.0, tf: 11.0, tw: 7.3 }, "Steel"),
            ("Square Hollow Section Column", "89 x 89 x 5.0 SHS", Column, RectangularHollow { b: 89.0, d: 89.0, t: 5.0 }, "Steel"),
            ("Square Hollow Section Column", "100 x 100 x 5.0 SHS", Column, RectangularHollow { b: 100.0, d: 100.0, t: 5.0 }, "Steel"),
            ("Circular Hollow Section Column", "114.3 x 4.8 CHS", Column, CircularHollow { d: 114.3, t: 4.8 }, "Steel"),
            ("Timber Post", "90 x 90", Column, Rectangle { b: 90.0, d: 90.0 }, "Timber"),
            ("Universal Beam", "200UB25.4", Framing, I { d: 203.0, bf: 133.0, tf: 7.8, tw: 5.8 }, "Steel"),
            ("Universal Beam", "250UB31.4", Framing, I { d: 252.0, bf: 146.0, tf: 8.6, tw: 6.1 }, "Steel"),
            ("Universal Beam", "310UB40.4", Framing, I { d: 304.0, bf: 165.0, tf: 10.2, tw: 6.1 }, "Steel"),
            ("Universal Beam", "360UB50.7", Framing, I { d: 356.0, bf: 171.0, tf: 11.5, tw: 7.3 }, "Steel"),
            ("Parallel Flange Channel", "200PFC", Framing, Channel { d: 200.0, bf: 75.0, tf: 12.0, tw: 6.0 }, "Steel"),
            ("Rectangular Hollow Section", "150 x 50 x 4.0 RHS", Framing, RectangularHollow { b: 50.0, d: 150.0, t: 4.0 }, "Steel"),
            ("Timber Beam", "240 x 45", Framing, Rectangle { b: 45.0, d: 240.0 }, "Timber"),
            ("Timber Beam", "190 x 45", Framing, Rectangle { b: 45.0, d: 190.0 }, "Timber"),
            ("Concrete Beam", "300 x 600", Framing, Rectangle { b: 300.0, d: 600.0 }, "Concrete"),
        ];
        self.transact("Structural types", |tx| {
            for (family, name, used_for, section, material) in list {
                tx.add(Element::StructuralType(StructuralType { family: family.into(), name: name.into(), used_for, section, material: material.into(), info: Default::default() }));
            }
            Ok(())
        })
    }

    fn typed(&self, id: Id, used_for: StructuralUse) -> Result<&StructuralType, String> {
        let t = self.structural_type(id).ok_or("unknown structural type")?;
        if t.used_for != used_for {
            return Err(if used_for == StructuralUse::Column { "That type is a framing section, not a column." } else { "That type is a column, not a framing section." }.into());
        }
        Ok(t)
    }

    /// The height of the underside of a roof or a floor over a plan point; None where it isn't over the point.
    pub fn underside_at(&self, host: Id, p: Point) -> Option<f64> {
        match self.get(host)? {
            Element::Floor(f) => {
                if !f.regions(20.0).iter().any(|r| crate::rooms::contains(r, p)) { return None; }
                match self.floor_shape(f) {
                    Some(tin) => { let bot = self.floor_bottoms(f, &tin); Tin { verts: tin.verts.iter().zip(&bot).map(|(v, &z)| [v[0], v[1], z]).collect(), tris: tin.tris.clone() }.z_near(p) }
                    None => Some(self.floor_surface(f)?.z(p) - self.floor_type(f.floor_type).map_or(0.0, |t| t.thickness_mm())),
                }
            }
            Element::Roof(r) => {
                let t = self.roof_type(r.roof_type)?.thickness_mm();
                let g = self.roof_geometry(r)?;
                if let Some((tin, bot)) = &g.shaped {
                    return Tin { verts: tin.verts.iter().zip(bot).map(|(v, &z)| [v[0], v[1], z]).collect(), tris: tin.tris.clone() }.z_at(p);
                }
                // the sloping face over the point: its plane there, less the roof's thickness
                for f in g.faces.iter().filter(|f| !f.vertical && f.poly.len() >= 3) {
                    let plan: Vec<Point> = f.poly.iter().map(|q| Point::new(q[0], q[1])).collect();
                    if !crate::curve::point_in_polygon(p, &plan) { continue; }
                    let mut n = [0.0; 3];
                    for i in 0..f.poly.len() { let (a, b) = (f.poly[i], f.poly[(i + 1) % f.poly.len()]); n[0] += (a[1] - b[1]) * (a[2] + b[2]); n[1] += (a[2] - b[2]) * (a[0] + b[0]); n[2] += (a[0] - b[0]) * (a[1] + b[1]); }
                    if n[2].abs() < 1e-9 { continue; }
                    let a = f.poly[0];
                    return Some(a[2] - (n[0] * (p.x - a[0]) + n[1] * (p.y - a[1])) / n[2] - t);
                }
                for cap in &g.caps {
                    let plan: Vec<Point> = cap.iter().map(|q| Point::new(q[0], q[1])).collect();
                    if crate::curve::point_in_polygon(p, &plan) { return Some(cap[0][2] - t); }
                }
                g.flat.iter().any(|reg| crate::rooms::contains(reg, p)).then_some(g.base_z - t)
            }
            _ => None,
        }
    }

    /// The roofs and floors over a column's top that it could be attached to, with the underside's height there.
    pub fn column_hosts(&self, c: &StructuralColumn) -> Vec<(Id, f64)> {
        let base = self.level(c.level).map_or(0.0, |l| l.elevation_mm) + c.base_offset_mm;
        let at = c.top_at.unwrap_or(c.at);
        self.iter().filter(|(_, e)| matches!(e, Element::Roof(_) | Element::Floor(_))).filter_map(|(id, _)| Some((id, self.underside_at(id, at)?))).filter(|(_, z)| *z > base + 1.0).collect()
    }

    /// Where an attached column's top is: the underside of what it is attached to, over its top point.
    fn attached_top(&self, c: &StructuralColumn, base: f64) -> Option<f64> {
        let z = self.underside_at(c.top_attach?, c.top_at.unwrap_or(c.at))? + c.top_offset_mm;
        (z > base + 1.0).then_some(z)
    }

    /// A column's base and top heights (absolute, mm).
    pub fn column_z(&self, c: &StructuralColumn) -> (f64, f64) {
        let base = self.level(c.level).map_or(0.0, |l| l.elevation_mm) + c.base_offset_mm;
        if let Some(z) = self.attached_top(c, base) { return (base, z); }
        let top = match c.top_level.and_then(|l| self.level(l)) {
            Some(l) => l.elevation_mm + c.top_offset_mm,
            None => base + c.height_mm,
        };
        (base, top)
    }

    /// The ends of a column's axis: its base, and its top (over the base unless it is slanted).
    pub fn column_ends(&self, c: &StructuralColumn) -> (V3, V3) {
        let (base, top) = self.column_z(c);
        let t = c.top_at.unwrap_or(c.at);
        ([c.at.x, c.at.y, base], [t.x, t.y, top])
    }

    pub(crate) fn check_column(&self, c: &StructuralColumn) -> Result<(), String> {
        self.typed(c.structural_type, StructuralUse::Column)?;
        if self.level(c.level).is_none() { return Err("unknown level".into()); }
        if c.top_level.is_some_and(|l| self.level(l).is_none()) { return Err("unknown top level".into()); }
        if ![c.at.x, c.at.y, c.base_offset_mm, c.top_offset_mm, c.height_mm, c.rotation_deg, c.top_at.map_or(0.0, |p| p.x + p.y)].iter().all(|v| v.is_finite()) { return Err("a column's position and offsets are numbers".into()); }
        if c.top_attach.is_some_and(|h| !matches!(self.get(h), Some(Element::Roof(_) | Element::Floor(_)))) { return Err("A column's top attaches to a roof or a floor.".into()); }
        let (base, top) = self.column_z(c);
        if c.top_at.is_some_and(|t| t.dist(&c.at) > (top - base) * 5.0) { return Err("A slanted column leans too far: a member that flat is a beam.".into()); }
        if top - base < 1.0 { return Err("The top of a column is above its base: check the levels and offsets.".into()); }
        if top - base > 1_000_000.0 { return Err("A column can be a kilometre high at most.".into()); }
        Ok(())
    }

    fn check_beam(&self, b: &Beam) -> Result<(), String> {
        self.typed(b.structural_type, StructuralUse::Framing)?;
        if self.level(b.level).is_none() { return Err("unknown level".into()); }
        if b.cutback_mm.is_some_and(|c| !(0.0..=1000.0).contains(&c)) { return Err("A cutback is between 0 and 1000.".into()); }
        if ![b.start.x, b.start.y, b.end.x, b.end.y, b.start_offset_mm, b.end_offset_mm, b.z_offset_mm, b.y_offset_mm, b.rotation_deg, b.start_extension_mm, b.end_extension_mm].iter().all(|v| v.is_finite()) { return Err("a beam's ends and offsets are numbers".into()); }
        if b.start.dist(&b.end).hypot(b.end_offset_mm - b.start_offset_mm) < 1.0 { return Err("A beam's ends are apart.".into()); }
        Ok(())
    }

    /// A vertical column at a point, from `level` up to `top_level` (or `height_mm` high with none).
    pub fn add_structural_column(&mut self, c: StructuralColumn) -> Result<Id, String> {
        self.check_column(&c)?;
        self.transact("Structural column", |tx| Ok(tx.add(Element::StructuralColumn(c))))
    }

    pub fn add_beam(&mut self, b: Beam) -> Result<Id, String> {
        self.check_beam(&b)?;
        self.transact("Beam", |tx| Ok(tx.add(Element::Beam(b))))
    }

    /// Properties of a column or a beam, by patch.
    pub fn patch_structural(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        match self.get(id).cloned() {
            Some(Element::StructuralColumn(old)) => {
                let new: StructuralColumn = apply_patch(&old, patch)?;
                if new == old { return Ok(()); }
                if old.pinned && new.pinned && new != (StructuralColumn { mark: old.mark.clone(), comments: old.comments.clone(), ..new.clone() }) && (StructuralColumn { mark: new.mark.clone(), comments: new.comments.clone(), ..old.clone() }) != new {
                    return Err("The column is pinned; unpin it first.".into());
                }
                self.check_column(&new)?;
                self.transact("Change column", |tx| tx.update(id, Element::StructuralColumn(new)))
            }
            Some(Element::Beam(old)) => {
                let new: Beam = apply_patch(&old, patch)?;
                if new == old { return Ok(()); }
                if old.pinned && new.pinned && (Beam { mark: new.mark.clone(), comments: new.comments.clone(), ..old.clone() }) != new {
                    return Err("The beam is pinned; unpin it first.".into());
                }
                self.check_beam(&new)?;
                self.transact("Change beam", |tx| tx.update(id, Element::Beam(new)))
            }
            Some(Element::StructuralType(old)) => {
                let new: StructuralType = apply_patch(&old, patch)?;
                if new == old { return Ok(()); }
                if new.used_for != old.used_for { return Err("A type stays a column or a framing section.".into()); }
                if new.name.trim().is_empty() { return Err("A type needs a name.".into()); }
                if self.structural_types().iter().any(|(i, o)| *i != id && o.family == new.family && o.name == new.name) { return Err(format!("{} already has a type called {}.", new.family, new.name)); }
                new.section.check()?;
                self.transact("Change structural type", |tx| tx.update(id, Element::StructuralType(new)))
            }
            _ => Err("not a structural column, beam or type".into()),
        }
    }

    /// A copy of a type under a new name, to give another size.
    pub fn duplicate_structural_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let t = self.structural_type(id).cloned().ok_or("not a structural type")?;
        let name = name.trim();
        if name.is_empty() || self.structural_types().iter().any(|(_, o)| o.family == t.family && o.name == name) { return Err("The name is empty or already used in that family.".into()); }
        self.transact("Duplicate structural type", |tx| Ok(tx.add(Element::StructuralType(StructuralType { name: name.into(), ..t }))))
    }

    /// Where a column's solid is.
    pub fn column_member(&self, c: &StructuralColumn) -> Member {
        let (p0, p1) = self.column_ends(c);
        let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let length = dot(d, d).sqrt();
        let axis = if length > 1e-9 { unit(d) } else { [0.0, 0.0, 1.0] };
        let (s, k) = c.rotation_deg.to_radians().sin_cos();
        // the section's frame leans with the column: the least turn that takes plumb to its axis
        let lean = |x: V3| {
            let v = [-axis[1], axis[0], 0.0];
            let sn = v[0].hypot(v[1]);
            if sn < 1e-9 { return x; }
            let v = [v[0] / sn, v[1] / sn, 0.0];
            let (vx, vd) = (cross(v, x), dot(v, x));
            [x[0] * axis[2] + vx[0] * sn + v[0] * vd * (1.0 - axis[2]), x[1] * axis[2] + vx[1] * sn + v[1] * vd * (1.0 - axis[2]), x[2] * axis[2] + vx[2] * sn + v[2] * vd * (1.0 - axis[2])]
        };
        // a slanted column's ends are cut level; an attached top to the slope of what it is under
        let slanted = c.top_at.is_some_and(|t| t.dist(&c.at) > 1e-6);
        let mut end_cut = slanted.then_some((p1, [0.0, 0.0, 1.0]));
        if let Some(host) = c.top_attach.filter(|_| self.attached_top(c, p0[2]).is_some()) {
            let (at, e) = (Point::new(p1[0], p1[1]), 40.0);
            let z = |dx: f64, dy: f64| self.underside_at(host, Point::new(at.x + dx, at.y + dy));
            if let (Some(xa), Some(xb), Some(ya), Some(yb)) = (z(-e, 0.0), z(e, 0.0), z(0.0, -e), z(0.0, e)) {
                end_cut = Some((p1, unit([-(xb - xa) / (2.0 * e), -(yb - ya) / (2.0 * e), 1.0])));
            }
        }
        Member { origin: p0, across: lean([k, s, 0.0]), up: lean([-s, k, 0.0]), axis, length, start_cut: slanted.then_some((p0, [0.0, 0.0, -1.0])), end_cut }
    }

    /// What each end of every beam does: joined to a column (cut back to its face), to another beam's length
    /// (cut back to its side), mitred with another beam's end, or left as drawn; and its extension.
    pub fn beam_trims(&self) -> std::collections::HashMap<Id, [EndTrim; 2]> {
        let list = self.framing();
        let trims = self.framing_trims(&list);
        let mut out = std::collections::HashMap::new();
        for ((id, _), t) in list.iter().zip(trims) {
            if matches!(self.get(*id), Some(Element::Beam(_))) { out.insert(*id, t); }
        }
        out
    }

    /// Every beam there is: the placed ones, then each beam system's (under the system's id).
    pub fn framing(&self) -> Vec<(Id, Beam)> {
        let mut out: Vec<(Id, Beam)> = self.beams().into_iter().map(|(id, b)| (id, b.clone())).collect();
        for (id, s) in self.beam_systems() { out.extend(self.system_beams(s).into_iter().map(|b| (id, b))); }
        out
    }

    /// `beam_trims` for a list of beams (see `framing`), in its order.
    pub fn framing_trims(&self, list: &[(Id, Beam)]) -> Vec<[EndTrim; 2]> {
        const TOL: f64 = 5.0;
        let beams: Vec<(usize, &Beam, &StructuralType, Member, V3, V3)> = list.iter().enumerate().filter_map(|(i, (_, b))| {
            let t = self.structural_type(b.structural_type)?;
            let (p0, p1) = self.beam_ends(b);
            Some((i, b, t, self.beam_member(b, &t.section), p0, p1))
        }).collect();
        let owner = |i: usize| list[i].0;
        let columns: Vec<(Id, &StructuralColumn, &StructuralType, V3, V3)> = self.structural_columns().into_iter().filter_map(|(id, c)| { let (a, b) = self.column_ends(c); Some((id, c, self.structural_type(c.structural_type)?, a, b)) }).collect();
        let mut out = vec![[EndTrim::default(); 2]; list.len()];
        for (id, b, t, m, p0, p1) in &beams {
            // a plumb member joins nothing
            if m.axis[0].hypot(m.axis[1]) < 1e-3 { continue; }
            let flat = m.axis[0].hypot(m.axis[1]).max(1e-6);
            let depth = t.section.size().1;
            let gap = b.cutback_mm.unwrap_or_else(|| default_cutback(&t.material));
            let mut ends = [EndTrim::default(); 2];
            for k in 0..2 {
                let (p, allow, ext, sign) = if k == 0 { (*p0, b.start_join, b.start_extension_mm, 1.0) } else { (*p1, b.end_join, b.end_extension_mm, -1.0) };
                // into the beam from this end, in plan
                let u = [m.axis[0] / flat * sign, m.axis[1] / flat * sign];
                let mut e = EndTrim { cut_mm: -ext, ..Default::default() };
                if !allow { ends[k] = e; continue; }
                let column = columns.iter().find_map(|(cid, c, ct, base, top)| {
                    if p[2] < base[2] - TOL || p[2] > top[2] + depth + TOL { return None; }
                    let f = ((p[2] - base[2]) / (top[2] - base[2]).max(1e-6)).clamp(0.0, 1.0);
                    let (cx, cy) = (base[0] + (top[0] - base[0]) * f, base[1] + (top[1] - base[1]) * f);
                    let (s, kk) = c.rotation_deg.to_radians().sin_cos();
                    let local = |x: f64, y: f64| [x * kk + y * s, -x * s + y * kk];
                    let (l, v) = (local(p[0] - cx, p[1] - cy), local(u[0], u[1]));
                    let (w, d) = ct.section.size();
                    // how far along the beam the column's face is
                    let exit = match ct.section {
                        Section::Round { .. } | Section::CircularHollow { .. } => {
                            let r = w / 2.0;
                            if l[0].hypot(l[1]) > r + TOL { return None; }
                            let lv = l[0] * v[0] + l[1] * v[1];
                            -lv + (lv * lv - (l[0] * l[0] + l[1] * l[1] - r * r)).max(0.0).sqrt()
                        }
                        _ => {
                            if l[0].abs() > w / 2.0 + TOL || l[1].abs() > d / 2.0 + TOL { return None; }
                            let mut te = f64::INFINITY;
                            for (li, vi, h) in [(l[0], v[0], w / 2.0), (l[1], v[1], d / 2.0)] {
                                if vi > 1e-9 { te = te.min((h - li) / vi); } else if vi < -1e-9 { te = te.min((-h - li) / vi); }
                            }
                            if te.is_finite() { te } else { 0.0 }
                        }
                    };
                    Some((*cid, exit.max(0.0)))
                });
                if let Some((cid, exit)) = column {
                    e.cut_mm += exit / flat + gap;
                    e.joined = Some(cid);
                    ends[k] = e;
                    continue;
                }
                let mut corners: Vec<(Id, V3)> = vec![];
                let mut tee: Option<(Id, f64)> = None;
                for (oid, ob, ot, om, q0, q1) in &beams {
                    if oid == id || om.axis[0].hypot(om.axis[1]) < 1e-3 { continue; }
                    let oflat = om.axis[0].hypot(om.axis[1]).max(1e-6);
                    let w = [om.axis[0] / oflat, om.axis[1] / oflat];
                    if (u[0] * w[1] - u[1] * w[0]).abs() < 0.1 { continue; }
                    // an end of the other beam on this end: a corner
                    let mut met = false;
                    for (q, ojoin, osign) in [(q0, ob.start_join, 1.0), (q1, ob.end_join, -1.0)] {
                        if (q[0] - p[0]).hypot(q[1] - p[1]) <= TOL && (q[2] - p[2]).abs() <= TOL {
                            met = true;
                            if ojoin { corners.push((owner(*oid), [om.axis[0] * osign, om.axis[1] * osign, om.axis[2] * osign])); }
                        }
                    }
                    if met || tee.is_some() { continue; }
                    // this end on the other beam's length
                    let (ow, od) = ot.section.size();
                    let rel = [p[0] - om.origin[0], p[1] - om.origin[1]];
                    let (along, len) = (rel[0] * w[0] + rel[1] * w[1], om.length * oflat);
                    if along < TOL || along > len - TOL { continue; }
                    let nrm = [-w[1], w[0]];
                    let off = rel[0] * nrm[0] + rel[1] * nrm[1];
                    if off.abs() > ow / 2.0 + TOL { continue; }
                    let oz = q0[2] + (q1[2] - q0[2]) * (along / len);
                    if (oz - p[2]).abs() > depth.max(od) + TOL { continue; }
                    let un = u[0] * nrm[0] + u[1] * nrm[1];
                    tee = Some((owner(*oid), ((un.signum() * ow / 2.0 - off) / un).max(0.0)));
                }
                if corners.len() == 1 {
                    let (oid, other) = corners[0];
                    let mine = [m.axis[0] * sign, m.axis[1] * sign, m.axis[2] * sign];
                    e.plane = Some((p, unit([other[0] - mine[0], other[1] - mine[1], other[2] - mine[2]])));
                    e.joined = Some(oid);
                } else if let Some((oid, exit)) = tee.filter(|_| corners.is_empty()) {
                    e.cut_mm += exit / flat + gap;
                    e.joined = Some(oid);
                }
                ends[k] = e;
            }
            out[*id] = ends;
        }
        out
    }

    /// Every column and beam as it is built: its id, kind ("StructuralColumn" or "Beam"), type and solid, with
    /// the beams cut at their joins.
    pub fn structural_members(&self) -> Vec<(Id, &'static str, &StructuralType, Member)> {
        let mut out = vec![];
        for (id, c) in self.structural_columns() {
            let Some(t) = self.structural_type(c.structural_type) else { continue };
            out.push((id, "StructuralColumn", t, self.column_member(c)));
        }
        let list = self.framing();
        let trims = self.framing_trims(&list);
        for ((id, b), ends) in list.iter().zip(&trims) {
            let Some(t) = self.structural_type(b.structural_type) else { continue };
            out.push((*id, "Beam", t, self.beam_member(b, &t.section).trimmed(ends)));
        }
        for (id, tr) in self.trusses() {
            for (_, ty, m) in self.truss_members(tr) {
                let Some(t) = self.structural_type(ty) else { continue };
                out.push((id, "Beam", t, m));
            }
        }
        out
    }

    /// A beam's line: its ends in space (the level plus each end's offset and the z offset).
    pub fn beam_ends(&self, b: &Beam) -> (V3, V3) {
        let z = self.level(b.level).map_or(0.0, |l| l.elevation_mm) + b.z_offset_mm;
        ([b.start.x, b.start.y, z + b.start_offset_mm], [b.end.x, b.end.y, z + b.end_offset_mm])
    }

    /// Where a beam's solid is: its section set against the line by the justifications, the y offset and the
    /// rotation.
    pub fn beam_member(&self, b: &Beam, s: &Section) -> Member {
        let (p0, p1) = self.beam_ends(b);
        let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let axis = unit(d);
        // across is level, to the right looking from start to end; up is square to both. A plumb member (a
        // truss's vertical web, dropped) has no right: east, turned by its rotation
        let flat = if axis[0].hypot(axis[1]) < 1e-6 { [1.0, 0.0, 0.0] } else { unit([axis[1], -axis[0], 0.0]) };
        let up0 = unit(cross(flat, axis));
        let (sn, cs) = b.rotation_deg.to_radians().sin_cos();
        let across = unit(add(add([0.0; 3], flat, cs), up0, sn));
        let up = unit(add(add([0.0; 3], up0, cs), flat, -sn));
        let (w, depth) = s.size();
        let dz = match b.z_justification { ZJustification::Top => -depth / 2.0, ZJustification::Center => 0.0, ZJustification::Bottom => depth / 2.0 };
        let dy = match b.y_justification { YJustification::Left => w / 2.0, YJustification::Center => 0.0, YJustification::Right => -w / 2.0 } + b.y_offset_mm;
        Member { origin: add(add(p0, up, dz), across, dy), across, up, axis, length, start_cut: None, end_cut: None }
    }

    /// The length of a column or a beam (mm) and its volume (mm³).
    pub fn structural_measures(&self, id: Id) -> Option<(f64, f64)> {
        let (t, length) = match self.get(id)? {
            Element::StructuralColumn(c) => { let (p, q) = self.column_ends(c); (c.structural_type, dot([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [q[0] - p[0], q[1] - p[1], q[2] - p[2]]).sqrt()) }
            Element::Beam(b) => { let (p, q) = self.beam_ends(b); (b.structural_type, ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt()) }
            _ => return None,
        };
        Some((length, length * self.structural_type(t)?.section.area_mm2()))
    }

    /// The colour a member draws in: its material's shading colour, or a grey by what it is made of.
    pub fn structural_colour(&self, t: &StructuralType) -> [u8; 3] {
        self.material(&t.material).map_or_else(|| fallback_colour(&t.material), |m| [m.graphics.shading_color[0], m.graphics.shading_color[1], m.graphics.shading_color[2]])
    }

    /// Every column and beam as a solid: its id, kind ("StructuralColumn" or "Beam"), colour and triangles.
    pub fn structural_meshes(&self) -> Vec<(Id, &'static str, [u8; 3], Vec<f32>)> {
        self.structural_members().into_iter().map(|(id, kind, t, m)| (id, kind, self.structural_colour(t), m.mesh(&t.section))).collect()
    }

    /// A beam's length as it is cut: its line less what its joins take, plus its extensions.
    pub fn beam_cut_length(&self, id: Id) -> Option<f64> {
        let Element::Beam(b) = self.get(id)? else { return None };
        let (p, q) = self.beam_ends(b);
        let ends = self.beam_trims().get(&id).copied().unwrap_or_default();
        Some(dot([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [q[0] - p[0], q[1] - p[1], q[2] - p[2]]).sqrt() - ends[0].cut_mm - ends[1].cut_mm)
    }

    /// A column's section in plan at a height (its outline, and the inside of a hollow one): where a plan's cut
    /// plane meets it, or its nearer end for a plan that sees it from above or below.
    pub fn column_plan(&self, c: &StructuralColumn, z: f64) -> Option<(Vec<Point>, Option<Vec<Point>>)> {
        let t = self.structural_type(c.structural_type)?;
        let m = self.column_member(c);
        let z = z.clamp(m.origin[2], m.origin[2] + m.axis[2] * m.length);
        let level = Member { end_cut: Some(([m.origin[0], m.origin[1], z], [0.0, 0.0, 1.0])), ..m.clone() };
        let place = |ring: Vec<[f64; 2]>| ring.into_iter().map(|q| { let p = if m.axis[2] > 0.15 { level.corner(q, true) } else { m.at(q, 0.0) }; Point::new(p[0], p[1]) }).collect::<Vec<_>>();
        let (outer, inner) = t.section.rings();
        Some((place(outer), inner.map(place)))
    }

    /// A beam in plan: the four corners of its width along its length, as its joins leave it (see
    /// `structural_members`).
    pub fn beam_plan(m: &Member, s: &Section) -> Vec<Point> {
        let (w, d) = s.size();
        // the widest it shows from above, whichever way it is turned
        let half = (w / 2.0 * m.across[0].hypot(m.across[1])).abs() + (d / 2.0 * m.up[0].hypot(m.up[1])).abs();
        if m.axis[0].hypot(m.axis[1]) < 1e-3 {
            // plumb: its section's box, seen from above
            return [[-w, -d], [w, -d], [w, d], [-w, d]].iter().map(|q| { let p = add(add(m.origin, m.across, q[0] / 2.0), m.up, q[1] / 2.0); Point::new(p[0], p[1]) }).collect();
        }
        let side = unit([m.axis[1], -m.axis[0], 0.0]);
        // each corner where its edge of the beam meets the end's cut plane, in plan
        let corner = |h: f64, end: bool| {
            let (t0, cut) = if end { (m.length, m.end_cut) } else { (0.0, m.start_cut) };
            let from = add(m.origin, side, h);
            let t = match cut { Some((p, n)) if dot(m.axis, n).abs() > 0.15 => dot([p[0] - from[0], p[1] - from[1], p[2] - from[2]], n) / dot(m.axis, n), _ => t0 };
            let q = add(from, m.axis, t);
            Point::new(q[0], q[1])
        };
        vec![corner(half, false), corner(half, true), corner(-half, true), corner(-half, false)]
    }

    /// A column or beam moved, turned, mirrored or copied.
    pub(crate) fn structural_transformed(e: &Element, x: &Xform, copy: bool) -> Option<Element> {
        let turn = match *x { Xform::Rotate { angle_deg, .. } => angle_deg, _ => 0.0 };
        match e {
            Element::StructuralColumn(c) => {
                let mut c = c.clone();
                c.at = x.apply(c.at);
                c.top_at = c.top_at.map(|p| x.apply(p));
                c.rotation_deg = match *x { Xform::Mirror { .. } => -c.rotation_deg, _ => c.rotation_deg + turn }.rem_euclid(360.0);
                if copy { c.pinned = false; c.mark.clear(); }
                Some(Element::StructuralColumn(c))
            }
            Element::Beam(b) => {
                let mut b = b.clone();
                b.start = x.apply(b.start);
                b.end = x.apply(b.end);
                if let Xform::Mirror { .. } = *x {
                    b.y_justification = match b.y_justification { YJustification::Left => YJustification::Right, YJustification::Right => YJustification::Left, c => c };
                    b.y_offset_mm = -b.y_offset_mm;
                    b.rotation_deg = -b.rotation_deg;
                }
                if copy { b.pinned = false; b.mark.clear(); }
                Some(Element::Beam(b))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Document, Id, Id, Id, Id) {
        let mut d = Document::new_project();
        d.ensure_structural_types().unwrap();
        let levels: Vec<Id> = d.levels().iter().map(|(i, _)| *i).collect();
        let col = d.structural_types().iter().find(|(_, t)| t.name == "300 x 300").unwrap().0;
        let ub = d.structural_types().iter().find(|(_, t)| t.name == "310UB40.4").unwrap().0;
        (d, levels[0], levels[1], col, ub)
    }

    fn extent(mesh: &[f32]) -> ([f64; 3], [f64; 3]) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in mesh.chunks(6) { for k in 0..3 { lo[k] = lo[k].min(v[k] as f64 * 1000.0); hi[k] = hi[k].max(v[k] as f64 * 1000.0); } }
        (lo, hi)
    }

    #[test]
    fn sections_have_the_area_of_their_parts() {
        assert_eq!(Section::Rectangle { b: 300.0, d: 600.0 }.area_mm2(), 180_000.0);
        let ub = Section::I { d: 304.0, bf: 165.0, tf: 10.2, tw: 6.1 };
        // two flanges and the web between them: about 5100 mm² for a 310UB40.4 (5210 in the tables, with its root radii)
        assert!((ub.area_mm2() - (2.0 * 165.0 * 10.2 + (304.0 - 20.4) * 6.1)).abs() < 1e-6);
        let shs = Section::RectangularHollow { b: 100.0, d: 100.0, t: 5.0 };
        assert!((shs.area_mm2() - (100.0 * 100.0 - 90.0 * 90.0)).abs() < 1e-6);
        assert!(Section::I { d: 100.0, bf: 50.0, tf: 60.0, tw: 5.0 }.check().is_err());
        assert_eq!(ub.rings().0.len(), 12);
    }

    #[test]
    fn a_column_stands_between_its_levels_and_a_beam_hangs_under_its_line() {
        let (mut d, l0, l1, col, ub) = setup();
        let (z0, z1) = (d.level(l0).unwrap().elevation_mm, d.level(l1).unwrap().elevation_mm);
        let c = d.add_structural_column(StructuralColumn { base_offset_mm: -200.0, top_level: Some(l1), ..StructuralColumn::new(col, l0, Point::new(1000.0, 2000.0)) }).unwrap();
        let meshes = d.structural_meshes();
        let (lo, hi) = extent(&meshes.iter().find(|m| m.0 == c).unwrap().3);
        assert!((lo[2] - (z0 - 200.0)).abs() < 0.01 && (hi[2] - z1).abs() < 0.01, "{lo:?} {hi:?}");
        assert!((lo[0] - 850.0).abs() < 0.01 && (hi[0] - 1150.0).abs() < 0.01 && (hi[1] - 2150.0).abs() < 0.01);
        assert_eq!(d.structural_measures(c).map(|m| m.0), Some(z1 - z0 + 200.0));
        // a beam type is not a column type, and a column needs height
        assert!(d.patch_structural(c, &serde_json::json!({ "structural_type": ub })).is_err());
        assert!(d.patch_structural(c, &serde_json::json!({ "top_offset_mm": -(z1 - z0) - 300.0 })).is_err());

        // a 310UB along x at the upper level: its top on the line, 304 deep, 165 wide
        let b = d.add_beam(Beam { usage: FramingUsage::Girder, ..Beam::new(ub, l1, Point::new(0.0, 0.0), Point::new(6000.0, 0.0)) }).unwrap();
        let (lo, hi) = extent(&d.structural_meshes().iter().find(|m| m.0 == b).unwrap().3);
        assert!((hi[2] - z1).abs() < 0.01 && (lo[2] - (z1 - 304.0)).abs() < 0.01, "{lo:?} {hi:?}");
        assert!((lo[1] + 82.5).abs() < 0.01 && (hi[1] - 82.5).abs() < 0.01 && lo[0].abs() < 0.01 && (hi[0] - 6000.0).abs() < 0.01);
        // centred, it sits half above the line; sloped, its far end rises
        d.patch_structural(b, &serde_json::json!({ "z_justification": "center", "end_offset_mm": 600.0 })).unwrap();
        let (_, hi) = extent(&d.structural_meshes().iter().find(|m| m.0 == b).unwrap().3);
        assert!(hi[2] > z1 + 600.0 && hi[2] < z1 + 600.0 + 160.0);
        assert!(d.add_beam(Beam { end: Point::new(0.0, 0.0), end_offset_mm: 0.0, ..match d.get(b) { Some(Element::Beam(x)) => x.clone(), _ => unreachable!() } }).is_err(), "no length at all");
        let (_, _, t, m) = d.structural_members().into_iter().find(|x| x.0 == b).unwrap();
        assert_eq!(Document::beam_plan(&m, &t.section).len(), 4);
        d.undo().unwrap();
        d.undo().unwrap();
        assert!(d.beams().is_empty());
    }

    #[test]
    fn beam_ends_join_columns_girders_and_each_other() {
        let (mut d, l0, l1, col, ub) = setup();
        // a 300 square column at each end of a 6 m steel beam: cut back 150 to each face and 10 more
        for x in [0.0, 6000.0] { d.add_structural_column(StructuralColumn { top_level: Some(l1), ..StructuralColumn::new(col, l0, Point::new(x, 0.0)) }).unwrap(); }
        let b = d.add_beam(Beam::new(ub, l1, Point::new(0.0, 0.0), Point::new(6000.0, 0.0))).unwrap();
        let x_of = |d: &Document, id: Id| { let (lo, hi) = extent(&d.structural_meshes().iter().find(|m| m.0 == id).unwrap().3); (lo[0], hi[0], lo[1], hi[1]) };
        let (x0, x1, ..) = x_of(&d, b);
        assert!((x0 - 160.0).abs() < 0.01 && (x1 - 5840.0).abs() < 0.01, "{x0} {x1}");
        assert!((d.beam_cut_length(b).unwrap() - 5680.0).abs() < 0.01);
        // no join at the start: it runs to its point; a longer end; a cutback of its own
        d.patch_structural(b, &serde_json::json!({ "start_join": false, "end_extension_mm": 50.0, "cutback_mm": 0.0 })).unwrap();
        let (x0, x1, ..) = x_of(&d, b);
        assert!(x0.abs() < 0.01 && (x1 - 5900.0).abs() < 0.01, "{x0} {x1}");
        d.undo().unwrap();

        // a joist landing on the girder's length stops at the girder's side (half of 165) and the gap
        let j = d.add_beam(Beam::new(ub, l1, Point::new(3000.0, 0.0), Point::new(3000.0, 4000.0))).unwrap();
        let (.., y0, y1) = x_of(&d, j);
        assert!((y0 - 92.5).abs() < 0.01 && (y1 - 4000.0).abs() < 0.01, "{y0} {y1}");
        let trims = d.beam_trims();
        assert_eq!((trims[&j][0].joined, trims[&j][1].joined), (Some(b), None));

        // two ends on one point are mitred: each beam reaches the outside corner and neither is cut short
        let a = d.add_beam(Beam::new(ub, l1, Point::new(3000.0, 4000.0), Point::new(6000.0, 4000.0))).unwrap();
        let (.., y1) = x_of(&d, j);
        let (ax0, ..) = x_of(&d, a);
        assert!((y1 - 4082.5).abs() < 0.01 && (ax0 - 2917.5).abs() < 0.01, "{y1} {ax0}");
        let (_, _, t, m) = d.structural_members().into_iter().find(|x| x.0 == a).unwrap();
        let plan = Document::beam_plan(&m, &t.section);
        let xs: Vec<f64> = plan.iter().map(|p| p.x).collect();
        assert!(xs.iter().any(|x| (x - 2917.5).abs() < 0.01) && xs.iter().any(|x| (x - 3082.5).abs() < 0.01), "{xs:?}");
    }

    #[test]
    fn a_column_leans_and_its_top_follows_a_floor() {
        let (mut d, l0, l1, col, _) = setup();
        let (z0, z1) = (d.level(l0).unwrap().elevation_mm, d.level(l1).unwrap().elevation_mm);
        // slanted: its top 1 m along x, both ends cut level
        let c = d.add_structural_column(StructuralColumn { top_level: Some(l1), top_at: Some(Point::new(1000.0, 0.0)), ..StructuralColumn::new(col, l0, Point::new(0.0, 0.0)) }).unwrap();
        let (lo, hi) = extent(&d.structural_meshes().iter().find(|m| m.0 == c).unwrap().3);
        assert!((lo[2] - z0).abs() < 0.01 && (hi[2] - z1).abs() < 0.01, "{lo:?} {hi:?}");
        assert!(lo[0] < -150.0 && hi[0] > 1150.0 && (hi[1] - 150.0).abs() < 0.01, "{lo:?} {hi:?}");
        let rise = z1 - z0;
        assert!((d.structural_measures(c).unwrap().0 - rise.hypot(1000.0)).abs() < 0.01);
        // its section where a plan cuts it half way up is half way along
        let (outline, _) = d.column_plan(match d.get(c) { Some(Element::StructuralColumn(x)) => x, _ => unreachable!() }, z0 + rise / 2.0).unwrap();
        let cx = outline.iter().map(|p| p.x).sum::<f64>() / outline.len() as f64;
        assert!((cx - 500.0).abs() < 0.01, "{cx}");
        assert!(d.patch_structural(c, &serde_json::json!({ "top_at": { "x": 50000.0, "y": 0.0 } })).is_err(), "too flat");
        // a move carries the top with the base
        d.transform_elements(&[c], crate::edit::Xform::Move { dx: 0.0, dy: 700.0 }, Default::default()).unwrap();
        assert_eq!(match d.get(c) { Some(Element::StructuralColumn(x)) => x.top_at, _ => None }, Some(Point::new(1000.0, 700.0)));

        // a plumb column under the upper level's floor: attached, it stops at the floor's underside
        let ft = d.floor_types()[0].0;
        let th = d.floor_type(ft).unwrap().thickness_mm();
        let ring = [Point::new(4000.0, -2000.0), Point::new(9000.0, -2000.0), Point::new(9000.0, 2000.0), Point::new(4000.0, 2000.0)];
        let f = d.add_floor(ft, l1, ring.to_vec()).unwrap();
        let p = d.add_structural_column(StructuralColumn { top_level: Some(l1), ..StructuralColumn::new(col, l0, Point::new(6000.0, 0.0)) }).unwrap();
        let pc = match d.get(p) { Some(Element::StructuralColumn(x)) => x.clone(), _ => unreachable!() };
        assert_eq!(d.column_hosts(&pc), vec![(f, z1 - th)]);
        d.patch_structural(p, &serde_json::json!({ "top_attach": f })).unwrap();
        let (_, hi) = extent(&d.structural_meshes().iter().find(|m| m.0 == p).unwrap().3);
        assert!((hi[2] - (z1 - th)).abs() < 0.01, "{hi:?}");
        // the floor moved up takes the column's top with it
        d.patch_floor(f, &serde_json::json!({ "offset_mm": 250.0 })).unwrap();
        let (_, hi) = extent(&d.structural_meshes().iter().find(|m| m.0 == p).unwrap().3);
        assert!((hi[2] - (z1 - th + 250.0)).abs() < 0.01, "{hi:?}");
        assert!(d.patch_structural(p, &serde_json::json!({ "top_attach": c })).is_err(), "a column is no host");
    }
}
