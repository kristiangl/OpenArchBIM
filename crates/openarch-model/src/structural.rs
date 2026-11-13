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
//! Step 1 of the notes in structural.md: no joins between members (a beam runs from point to point through
//! whatever is there), no slanted columns, beam systems or trusses yet.

use serde::{Deserialize, Serialize};

use crate::doc::apply_patch;
use crate::edit::Xform;
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
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub pinned: bool,
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
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub pinned: bool,
}

type V3 = [f64; 3];
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
}

impl Member {
    fn at(&self, q: [f64; 2], along: f64) -> V3 {
        add(add(add(self.origin, self.across, q[0]), self.up, q[1]), self.axis, along)
    }

    /// The solid as triangles: position and normal for each corner, in metres.
    pub fn mesh(&self, s: &Section) -> Vec<f32> {
        let mut out: Vec<f32> = vec![];
        let mut push = |p: V3, n: V3| out.extend([p[0] / 1000.0, p[1] / 1000.0, p[2] / 1000.0, n[0], n[1], n[2]].map(|v| v as f32));
        let back = [-self.axis[0], -self.axis[1], -self.axis[2]];
        for part in s.parts() {
            for k in 1..part.len().saturating_sub(1) {
                // the far end faces along the axis, the near end back along it
                for q in [part[0], part[k], part[k + 1]] { push(self.at(q, self.length), self.axis); }
                for q in [part[0], part[k + 1], part[k]] { push(self.at(q, 0.0), back); }
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
                let (a0, b0, a1, b1) = (self.at(a, 0.0), self.at(b, 0.0), self.at(a, self.length), self.at(b, self.length));
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

    /// A column's base and top heights (absolute, mm).
    pub fn column_z(&self, c: &StructuralColumn) -> (f64, f64) {
        let base = self.level(c.level).map_or(0.0, |l| l.elevation_mm) + c.base_offset_mm;
        let top = match c.top_level.and_then(|l| self.level(l)) {
            Some(l) => l.elevation_mm + c.top_offset_mm,
            None => base + c.height_mm,
        };
        (base, top)
    }

    fn check_column(&self, c: &StructuralColumn) -> Result<(), String> {
        self.typed(c.structural_type, StructuralUse::Column)?;
        if self.level(c.level).is_none() { return Err("unknown level".into()); }
        if c.top_level.is_some_and(|l| self.level(l).is_none()) { return Err("unknown top level".into()); }
        if ![c.at.x, c.at.y, c.base_offset_mm, c.top_offset_mm, c.height_mm, c.rotation_deg].iter().all(|v| v.is_finite()) { return Err("a column's position and offsets are numbers".into()); }
        let (base, top) = self.column_z(c);
        if top - base < 1.0 { return Err("The top of a column is above its base: check the levels and offsets.".into()); }
        if top - base > 1_000_000.0 { return Err("A column can be a kilometre high at most.".into()); }
        Ok(())
    }

    fn check_beam(&self, b: &Beam) -> Result<(), String> {
        self.typed(b.structural_type, StructuralUse::Framing)?;
        if self.level(b.level).is_none() { return Err("unknown level".into()); }
        if ![b.start.x, b.start.y, b.end.x, b.end.y, b.start_offset_mm, b.end_offset_mm, b.z_offset_mm, b.y_offset_mm, b.rotation_deg].iter().all(|v| v.is_finite()) { return Err("a beam's ends and offsets are numbers".into()); }
        if b.start.dist(&b.end) < 1.0 { return Err("A beam's ends are apart in plan: a vertical member is a column.".into()); }
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
        let (base, top) = self.column_z(c);
        let (s, k) = c.rotation_deg.to_radians().sin_cos();
        Member { origin: [c.at.x, c.at.y, base], across: [k, s, 0.0], up: [-s, k, 0.0], axis: [0.0, 0.0, 1.0], length: (top - base).max(0.0) }
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
        // across is level, to the right looking from start to end; up is square to both
        let flat = unit([axis[1], -axis[0], 0.0]);
        let up0 = unit(cross(flat, axis));
        let (sn, cs) = b.rotation_deg.to_radians().sin_cos();
        let across = unit(add(add([0.0; 3], flat, cs), up0, sn));
        let up = unit(add(add([0.0; 3], up0, cs), flat, -sn));
        let (w, depth) = s.size();
        let dz = match b.z_justification { ZJustification::Top => -depth / 2.0, ZJustification::Center => 0.0, ZJustification::Bottom => depth / 2.0 };
        let dy = match b.y_justification { YJustification::Left => w / 2.0, YJustification::Center => 0.0, YJustification::Right => -w / 2.0 } + b.y_offset_mm;
        Member { origin: add(add(p0, up, dz), across, dy), across, up, axis, length }
    }

    /// The length of a column or a beam (mm) and its volume (mm³).
    pub fn structural_measures(&self, id: Id) -> Option<(f64, f64)> {
        let (t, length) = match self.get(id)? {
            Element::StructuralColumn(c) => { let (b, t) = self.column_z(c); (c.structural_type, t - b) }
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
        let mut out = vec![];
        for (id, e) in self.iter() {
            match e {
                Element::StructuralColumn(c) => {
                    let Some(t) = self.structural_type(c.structural_type) else { continue };
                    out.push((id, "StructuralColumn", self.structural_colour(t), self.column_member(c).mesh(&t.section)));
                }
                Element::Beam(b) => {
                    let Some(t) = self.structural_type(b.structural_type) else { continue };
                    out.push((id, "Beam", self.structural_colour(t), self.beam_member(b, &t.section).mesh(&t.section)));
                }
                _ => {}
            }
        }
        out
    }

    /// A column's section in plan where it stands (its outline, and the inside of a hollow one).
    pub fn column_plan(&self, c: &StructuralColumn) -> Option<(Vec<Point>, Option<Vec<Point>>)> {
        let t = self.structural_type(c.structural_type)?;
        let m = self.column_member(c);
        let place = |ring: Vec<[f64; 2]>| ring.into_iter().map(|q| { let p = m.at(q, 0.0); Point::new(p[0], p[1]) }).collect::<Vec<_>>();
        let (outer, inner) = t.section.rings();
        Some((place(outer), inner.map(place)))
    }

    /// A beam in plan: the four corners of its width along its line.
    pub fn beam_plan(&self, b: &Beam) -> Option<Vec<Point>> {
        let t = self.structural_type(b.structural_type)?;
        let m = self.beam_member(b, &t.section);
        let (w, d) = t.section.size();
        // the widest it shows from above, whichever way it is turned
        let half = (w / 2.0 * m.across[0].hypot(m.across[1])).abs() + (d / 2.0 * m.up[0].hypot(m.up[1])).abs();
        let side = unit([m.axis[1], -m.axis[0], 0.0]);
        let (a, e) = (m.origin, add(m.origin, m.axis, m.length));
        Some([add(a, side, half), add(e, side, half), add(e, side, -half), add(a, side, -half)].iter().map(|p| Point::new(p[0], p[1])).collect())
    }

    /// A column or beam moved, turned, mirrored or copied.
    pub(crate) fn structural_transformed(e: &Element, x: &Xform, copy: bool) -> Option<Element> {
        let turn = match *x { Xform::Rotate { angle_deg, .. } => angle_deg, _ => 0.0 };
        match e {
            Element::StructuralColumn(c) => {
                let mut c = c.clone();
                c.at = x.apply(c.at);
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
        let c = d.add_structural_column(StructuralColumn { structural_type: col, level: l0, at: Point::new(1000.0, 2000.0), base_offset_mm: -200.0, top_level: Some(l1), top_offset_mm: 0.0, height_mm: 3000.0, rotation_deg: 0.0, mark: String::new(), comments: String::new(), pinned: false }).unwrap();
        let meshes = d.structural_meshes();
        let (lo, hi) = extent(&meshes.iter().find(|m| m.0 == c).unwrap().3);
        assert!((lo[2] - (z0 - 200.0)).abs() < 0.01 && (hi[2] - z1).abs() < 0.01, "{lo:?} {hi:?}");
        assert!((lo[0] - 850.0).abs() < 0.01 && (hi[0] - 1150.0).abs() < 0.01 && (hi[1] - 2150.0).abs() < 0.01);
        assert_eq!(d.structural_measures(c).map(|m| m.0), Some(z1 - z0 + 200.0));
        // a beam type is not a column type, and a column needs height
        assert!(d.patch_structural(c, &serde_json::json!({ "structural_type": ub })).is_err());
        assert!(d.patch_structural(c, &serde_json::json!({ "top_offset_mm": -(z1 - z0) - 300.0 })).is_err());

        // a 310UB along x at the upper level: its top on the line, 304 deep, 165 wide
        let b = d.add_beam(Beam { structural_type: ub, level: l1, start: Point::new(0.0, 0.0), end: Point::new(6000.0, 0.0), start_offset_mm: 0.0, end_offset_mm: 0.0, z_justification: ZJustification::Top, y_justification: YJustification::Center,
            z_offset_mm: 0.0, y_offset_mm: 0.0, rotation_deg: 0.0, usage: FramingUsage::Girder, mark: String::new(), comments: String::new(), pinned: false }).unwrap();
        let (lo, hi) = extent(&d.structural_meshes().iter().find(|m| m.0 == b).unwrap().3);
        assert!((hi[2] - z1).abs() < 0.01 && (lo[2] - (z1 - 304.0)).abs() < 0.01, "{lo:?} {hi:?}");
        assert!((lo[1] + 82.5).abs() < 0.01 && (hi[1] - 82.5).abs() < 0.01 && lo[0].abs() < 0.01 && (hi[0] - 6000.0).abs() < 0.01);
        // centred, it sits half above the line; sloped, its far end rises
        d.patch_structural(b, &serde_json::json!({ "z_justification": "center", "end_offset_mm": 600.0 })).unwrap();
        let (_, hi) = extent(&d.structural_meshes().iter().find(|m| m.0 == b).unwrap().3);
        assert!(hi[2] > z1 + 600.0 && hi[2] < z1 + 600.0 + 160.0);
        assert!(d.add_beam(Beam { end: Point::new(0.0, 0.0), ..match d.get(b) { Some(Element::Beam(x)) => x.clone(), _ => unreachable!() } }).is_err(), "no length in plan");
        let plan = d.beam_plan(match d.get(b) { Some(Element::Beam(x)) => x, _ => unreachable!() }).unwrap();
        assert_eq!(plan.len(), 4);
        d.undo().unwrap();
        d.undo().unwrap();
        assert!(d.beams().is_empty());
    }
}
