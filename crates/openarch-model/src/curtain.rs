//! Curtain walls: a wall whose type is of the curtain kind. The type's grid rules divide the wall's face into cells
//! (U lines across its length, V lines up its height); each cell holds a panel (glazed, solid or empty) and mullions
//! run along the grid lines, interior and border types for each direction, joined by the type's join condition.
//! Everything is worked out from the wall's line, base and top each time it is drawn: the pieces are solids, each a
//! plan footprint swept between two heights, so plans, sections, elevations and the 3D view draw them alike.
//!
//! A curtain type keeps one layer as thick as its glazing zone so the rest of the program (extents, location line,
//! selection) treats the wall as it does any other; the layer is not drawn. Curtain walls take no part in wall
//! joins. With Automatically Embed on, a curtain wall standing in a basic wall cuts its opening in it.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::*;
use crate::geom::Point;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridLayout {
    #[default]
    None,
    FixedDistance,
    FixedNumber,
    MaximumSpacing,
    MinimumSpacing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Justify {
    #[default]
    Beginning,
    Center,
    End,
}

/// How grid lines are laid out in one direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridRule {
    #[serde(default)]
    pub layout: GridLayout,
    /// Fixed Distance, Maximum and Minimum Spacing.
    #[serde(default = "default_spacing")]
    pub spacing_mm: f64,
    /// Fixed Number: how many grid lines.
    #[serde(default = "default_number")]
    pub number: u32,
    /// Fixed Distance: where the lines are measured from, and how far they are shifted.
    #[serde(default)]
    pub justify: Justify,
    #[serde(default)]
    pub offset_mm: f64,
}

fn default_spacing() -> f64 {
    1500.0
}
fn default_number() -> u32 {
    2
}

impl Default for GridRule {
    fn default() -> Self {
        GridRule { layout: GridLayout::None, spacing_mm: default_spacing(), number: default_number(), justify: Justify::Beginning, offset_mm: 0.0 }
    }
}

impl GridRule {
    pub fn new(layout: GridLayout, spacing_mm: f64) -> GridRule {
        GridRule { layout, spacing_mm, ..GridRule::default() }
    }

    /// The interior grid lines along a length `len`, in order, none on or past either end.
    pub fn positions(&self, len: f64) -> Vec<f64> {
        let mut out: Vec<f64> = match self.layout {
            GridLayout::None => vec![],
            GridLayout::FixedDistance => {
                let d = self.spacing_mm;
                if !(d.is_finite() && d >= 1.0) {
                    return vec![];
                }
                let start = match self.justify {
                    Justify::Beginning => self.offset_mm,
                    Justify::End => len - self.offset_mm,
                    Justify::Center => len / 2.0 + self.offset_mm,
                };
                let k0 = ((0.0 - start) / d).floor() as i64;
                let k1 = ((len - start) / d).ceil() as i64;
                (k0..=k1).map(|k| start + k as f64 * d).collect()
            }
            GridLayout::FixedNumber => {
                let n = self.number.min(500) as f64;
                (1..=self.number.min(500)).map(|k| len * k as f64 / (n + 1.0)).collect()
            }
            GridLayout::MaximumSpacing | GridLayout::MinimumSpacing => {
                let s = self.spacing_mm;
                if !(s.is_finite() && s >= 1.0) {
                    return vec![];
                }
                let n = if self.layout == GridLayout::MaximumSpacing { (len / s).ceil() } else { (len / s).floor() }.clamp(1.0, 500.0) as usize;
                (1..n).map(|k| len * k as f64 / n as f64).collect()
            }
        };
        out.retain(|p| *p > 1.0 && *p < len - 1.0);
        out.sort_by(f64::total_cmp);
        out.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelKind {
    #[default]
    Glazed,
    Solid,
    /// No geometry: the cell is open, the mullions round it stay.
    Empty,
}

/// The system panel every cell takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelSpec {
    #[serde(default)]
    pub kind: PanelKind,
    #[serde(default = "default_panel_thickness")]
    pub thickness_mm: f64,
    /// Across the wall from its line, positive to its left (exterior).
    #[serde(default)]
    pub offset_mm: f64,
    #[serde(default = "default_glass")]
    pub material: String,
}

fn default_panel_thickness() -> f64 {
    25.0
}
fn default_glass() -> String {
    "Glass".into()
}

impl Default for PanelSpec {
    fn default() -> Self {
        PanelSpec { kind: PanelKind::Glazed, thickness_mm: default_panel_thickness(), offset_mm: 0.0, material: default_glass() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum MullionShape {
    /// Width 1 and Width 2 either side of the grid line, along the face; Thickness across the wall.
    Rectangular { width1_mm: f64, width2_mm: f64, thickness_mm: f64 },
    Circular { radius_mm: f64 },
}

/// A mullion type, by its parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MullionSpec {
    pub shape: MullionShape,
    /// Across the wall from its line, positive to its left (exterior).
    #[serde(default)]
    pub offset_mm: f64,
    #[serde(default = "default_aluminium")]
    pub material: String,
}

fn default_aluminium() -> String {
    "Aluminium".into()
}

impl MullionSpec {
    pub fn rectangular(width_mm: f64, thickness_mm: f64) -> MullionSpec {
        MullionSpec { shape: MullionShape::Rectangular { width1_mm: width_mm / 2.0, width2_mm: width_mm / 2.0, thickness_mm }, offset_mm: 0.0, material: default_aluminium() }
    }

    /// How far it reaches either side of its grid line, along the face.
    pub fn widths(&self) -> (f64, f64) {
        match self.shape {
            MullionShape::Rectangular { width1_mm, width2_mm, .. } => (width1_mm.max(0.0), width2_mm.max(0.0)),
            MullionShape::Circular { radius_mm } => (radius_mm.max(0.0), radius_mm.max(0.0)),
        }
    }

    /// How deep it is across the wall.
    pub fn depth(&self) -> f64 {
        match self.shape {
            MullionShape::Rectangular { thickness_mm, .. } => thickness_mm.max(0.0),
            MullionShape::Circular { radius_mm } => 2.0 * radius_mm.max(0.0),
        }
    }
}

/// Which mullions run through where vertical and horizontal ones cross.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinCondition {
    /// Treated as Vertical Grid Continuous until mullions can be joined one by one.
    #[default]
    NotDefined,
    VerticalContinuous,
    HorizontalContinuous,
    BorderAndVertical,
    BorderAndHorizontal,
}

/// Grid lines placed on one curtain wall (by hand, or read from a file): along its length from its start, and up
/// its height from its base. They add to the lines the type lays out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CurtainLines {
    #[serde(default)]
    pub u: Vec<f64>,
    #[serde(default)]
    pub v: Vec<f64>,
}

/// What makes a wall type a curtain wall type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CurtainSpec {
    /// Vertical grid lines (along the length) and horizontal ones (up the height).
    #[serde(default)]
    pub u: GridRule,
    #[serde(default)]
    pub v: GridRule,
    #[serde(default)]
    pub panel: PanelSpec,
    #[serde(default)]
    pub u_interior: Option<MullionSpec>,
    #[serde(default)]
    pub u_border1: Option<MullionSpec>,
    #[serde(default)]
    pub u_border2: Option<MullionSpec>,
    #[serde(default)]
    pub v_interior: Option<MullionSpec>,
    #[serde(default)]
    pub v_border1: Option<MullionSpec>,
    #[serde(default)]
    pub v_border2: Option<MullionSpec>,
    #[serde(default)]
    pub join: JoinCondition,
    /// Automatically Embed: standing in a basic wall, it cuts its opening there.
    #[serde(default)]
    pub auto_embed: bool,
}

impl CurtainSpec {
    /// How thick the glazing zone is: the deepest panel or mullion, each about its offset.
    pub fn thickness(&self) -> f64 {
        let panel = if self.panel.kind == PanelKind::Empty { 0.0 } else { self.panel.thickness_mm + 2.0 * self.panel.offset_mm.abs() };
        let mullions = [&self.u_interior, &self.u_border1, &self.u_border2, &self.v_interior, &self.v_border1, &self.v_border2]
            .iter()
            .filter_map(|m| m.as_ref())
            .map(|m| m.depth() + 2.0 * m.offset_mm.abs())
            .fold(0.0, f64::max);
        panel.max(mullions).max(1.0)
    }

    /// The one bookkeeping layer a curtain type carries.
    pub fn layer(&self) -> Layer {
        Layer::new(&self.panel.material, self.thickness(), LayerFunction::Structure, true)
    }
}

/// What a solid of a curtain wall is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CurtainPart {
    Panel,
    /// A mullion on a vertical grid line (U), or a horizontal one (V, a transom).
    Mullion { vertical: bool },
}

/// A piece of a curtain wall: a plan footprint (counter-clockwise) swept from `z0` to `z1`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CurtainSolid {
    pub part: CurtainPart,
    pub ring: Vec<Point>,
    pub z0: f64,
    pub z1: f64,
    pub material: String,
    /// A glazed panel (see-through in 3D).
    pub glazed: bool,
}

/// Distances along a wall path's points.
fn cumulative(pts: &[Point]) -> Vec<f64> {
    let mut c = vec![0.0];
    for w in pts.windows(2) {
        c.push(c.last().unwrap() + w[0].dist(&w[1]));
    }
    c
}

/// The quads covering the stretch `ua..ub` of a path, from `c0` to `c1` across it (left positive): one per facet
/// it crosses, each counter-clockwise.
fn band(pts: &[Point], cum: &[f64], ua: f64, ub: f64, c0: f64, c1: f64) -> Vec<Vec<Point>> {
    let at = |u: f64| -> (Point, Point) {
        let u = u.clamp(0.0, *cum.last().unwrap());
        let i = cum.partition_point(|c| *c <= u).clamp(1, pts.len() - 1);
        let (a, b) = (pts[i - 1], pts[i]);
        let seg = (cum[i] - cum[i - 1]).max(1e-9);
        let t = (u - cum[i - 1]) / seg;
        let d = Point::new((b.x - a.x) / seg, (b.y - a.y) / seg);
        (Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t), d)
    };
    let mut cuts = vec![ua];
    cuts.extend(cum.iter().copied().filter(|c| *c > ua + 1e-6 && *c < ub - 1e-6));
    cuts.push(ub);
    cuts.windows(2)
        .filter(|w| w[1] - w[0] > 1e-6)
        .map(|w| {
            let (pa, _) = at(w[0]);
            let (pb, _) = at(w[1]);
            let l = pa.dist(&pb).max(1e-9);
            let n = Point::new(-(pb.y - pa.y) / l, (pb.x - pa.x) / l);
            let off = |p: Point, c: f64| Point::new(p.x + n.x * c, p.y + n.y * c);
            vec![off(pa, c0), off(pb, c0), off(pb, c1), off(pa, c1)]
        })
        .collect()
}

impl Document {
    /// The curtain spec of a wall's type, when it is a curtain wall.
    pub fn curtain_of(&self, w: &Wall) -> Option<&CurtainSpec> {
        self.wall_type(w.wall_type)?.curtain.as_ref()
    }

    pub fn is_curtain_wall(&self, id: Id) -> bool {
        matches!(self.get(id), Some(Element::Wall(w)) if self.curtain_of(w).is_some())
    }

    /// A curtain wall's grid lines, along its length and up its height, the ends included.
    pub fn curtain_grid(&self, w: &Wall) -> Option<(Vec<f64>, Vec<f64>)> {
        let spec = self.curtain_of(w)?;
        let len = w.path().length();
        let (z0, z1) = self.wall_extent(w);
        let h = z1 - z0;
        if len < 1.0 || h < 1.0 {
            return None;
        }
        // the type's lines and the wall's own, inside its ends
        let merge = |rule: Vec<f64>, own: &[f64], end: f64| {
            let mut all: Vec<f64> = rule.into_iter().chain(own.iter().copied()).filter(|p| *p > 1.0 && *p < end - 1.0).collect();
            all.sort_by(f64::total_cmp);
            all.dedup_by(|a, b| (*a - *b).abs() < 1.0);
            std::iter::once(0.0).chain(all).chain(std::iter::once(end)).collect::<Vec<f64>>()
        };
        let none = CurtainLines::default();
        let own = w.curtain_lines.as_ref().unwrap_or(&none);
        Some((merge(spec.u.positions(len), &own.u, len), merge(spec.v.positions(h), &own.v, h)))
    }

    /// The pieces of a curtain wall: its panels and mullions as plan footprints swept between heights.
    pub fn curtain_solids(&self, w: &Wall) -> Vec<CurtainSolid> {
        let Some(spec) = self.curtain_of(w) else { return vec![] };
        let Some((us, vs)) = self.curtain_grid(w) else { return vec![] };
        let path = w.path();
        let pts = path.pts.clone();
        let cum = cumulative(&pts);
        let len = *us.last().unwrap();
        let (zb, zt) = self.wall_extent(w);
        let h = zt - zb;
        let (nu, nv) = (us.len() - 1, vs.len() - 1);
        let u_spec = |k: usize| if k == 0 { &spec.u_border1 } else if k == nu { &spec.u_border2 } else { &spec.u_interior };
        let v_spec = |j: usize| if j == 0 { &spec.v_border1 } else if j == nv { &spec.v_border2 } else { &spec.v_interior };
        // what each grid line's mullion takes up along the face (a border mullion stays inside the edge)
        let span = |p: f64, k: usize, last: usize, end: f64, m: &Option<MullionSpec>| -> (f64, f64) {
            match m {
                None => (p, p),
                Some(m) => {
                    let (w1, w2) = m.widths();
                    if k == 0 {
                        (0.0, (w1 + w2).min(end))
                    } else if k == last {
                        ((end - w1 - w2).max(0.0), end)
                    } else {
                        (p - w1, p + w2)
                    }
                }
            }
        };
        let uspan: Vec<(f64, f64)> = us.iter().enumerate().map(|(k, p)| span(*p, k, nu, len, u_spec(k))).collect();
        let vspan: Vec<(f64, f64)> = vs.iter().enumerate().map(|(j, p)| span(*p, j, nv, h, v_spec(j))).collect();
        let mut out = vec![];
        // panels: each cell between the mullions round it
        if spec.panel.kind != PanelKind::Empty && spec.panel.thickness_mm > 0.0 {
            let (c0, c1) = (spec.panel.offset_mm - spec.panel.thickness_mm / 2.0, spec.panel.offset_mm + spec.panel.thickness_mm / 2.0);
            for i in 0..nu {
                let (ua, ub) = (uspan[i].1, uspan[i + 1].0);
                if ub - ua < 1.0 {
                    continue;
                }
                for j in 0..nv {
                    let (za, zb2) = (vspan[j].1, vspan[j + 1].0);
                    if zb2 - za < 1.0 {
                        continue;
                    }
                    for ring in band(&pts, &cum, ua, ub, c0, c1) {
                        out.push(CurtainSolid { part: CurtainPart::Panel, ring, z0: zb + za, z1: zb + zb2, material: spec.panel.material.clone(), glazed: spec.panel.kind == PanelKind::Glazed });
                    }
                }
            }
        }
        // which mullions run through: (vertical, horizontal) for a border or an interior line
        let continuous = |border: bool| match spec.join {
            JoinCondition::NotDefined | JoinCondition::VerticalContinuous => (true, false),
            JoinCondition::HorizontalContinuous => (false, true),
            JoinCondition::BorderAndVertical => (true, border),
            JoinCondition::BorderAndHorizontal => (border, true),
        };
        // vertical mullions, on the U lines
        for (k, p) in us.iter().enumerate() {
            let Some(m) = u_spec(k) else { continue };
            let border = k == 0 || k == nu;
            let (ua, ub) = uspan[k];
            let (c0, c1) = (m.offset_mm - m.depth() / 2.0, m.offset_mm + m.depth() / 2.0);
            let pieces: Vec<(f64, f64)> = if continuous(border).0 { vec![(0.0, h)] } else { (0..nv).map(|j| (vspan[j].1, vspan[j + 1].0)).filter(|(a, b)| b - a > 1.0).collect() };
            for (za, zb2) in pieces {
                let rings: Vec<Vec<Point>> = match m.shape {
                    MullionShape::Circular { radius_mm } => {
                        let centre = band(&pts, &cum, *p, *p + 0.001, m.offset_mm, m.offset_mm + 0.001).first().map(|q| q[0]).unwrap_or(pts[0]);
                        vec![(0..16).map(|i| f64::from(i) * std::f64::consts::PI / 8.0).map(|t| Point::new(centre.x + radius_mm * t.cos(), centre.y + radius_mm * t.sin())).collect()]
                    }
                    MullionShape::Rectangular { .. } => band(&pts, &cum, ua, ub, c0, c1),
                };
                for ring in rings {
                    out.push(CurtainSolid { part: CurtainPart::Mullion { vertical: true }, ring, z0: zb + za, z1: zb + zb2, material: m.material.clone(), glazed: false });
                }
            }
        }
        // horizontal mullions (transoms), on the V lines
        for (j, _) in vs.iter().enumerate() {
            let Some(m) = v_spec(j) else { continue };
            let border = j == 0 || j == nv;
            let (za, zb2) = vspan[j];
            if zb2 - za < 1e-6 {
                continue;
            }
            let (c0, c1) = (m.offset_mm - m.depth() / 2.0, m.offset_mm + m.depth() / 2.0);
            let pieces: Vec<(f64, f64)> = if continuous(border).1 { vec![(0.0, len)] } else { (0..nu).map(|i| (uspan[i].1, uspan[i + 1].0)).filter(|(a, b)| b - a > 1.0).collect() };
            for (ua, ub) in pieces {
                for ring in band(&pts, &cum, ua, ub, c0, c1) {
                    out.push(CurtainSolid { part: CurtainPart::Mullion { vertical: false }, ring, z0: zb + za, z1: zb + zb2, material: m.material.clone(), glazed: false });
                }
            }
        }
        out
    }

    /// The outline a curtain wall covers in plan: its path offset by its extents either side.
    pub fn curtain_outline(&self, w: &Wall) -> Vec<Point> {
        let (l, r) = self.wall_extents(w);
        let path = w.path();
        let cum = cumulative(&path.pts);
        let quads = band(&path.pts, &cum, 0.0, *cum.last().unwrap_or(&0.0), -r, l);
        if quads.is_empty() {
            return vec![];
        }
        // right side forward, then the left side back
        let mut ring: Vec<Point> = quads.iter().map(|q| q[0]).collect();
        ring.push(quads.last().unwrap()[1]);
        ring.push(quads.last().unwrap()[2]);
        ring.extend(quads.iter().rev().map(|q| q[3]));
        ring
    }

    /// The basic walls a curtain wall is embedded in.
    pub fn embedded_in(&self, curtain: Id) -> Vec<Id> {
        self.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(i, _)| i).filter(|h| self.embed_cuts(*h).iter().any(|c| c.instance == curtain)).collect()
    }

    /// The opening a curtain wall with Automatically Embed cuts in basic wall `host`: along the host from its
    /// start, and its heights. None when it does not stand in that wall.
    pub(crate) fn embed_cuts(&self, host: Id) -> Vec<crate::family::Opening> {
        let Some(Element::Wall(hw)) = self.get(host) else { return vec![] };
        if self.curtain_of(hw).is_some() || hw.curve.is_some() {
            return vec![];
        }
        let (hl, hr) = self.wall_extents(hw);
        let (hz0, hz1) = self.wall_extent(hw);
        let hp = hw.path();
        let hlen = hp.length();
        let mut out = vec![];
        for (id, e) in self.iter() {
            let Element::Wall(cw) = e else { continue };
            let Some(spec) = self.curtain_of(cw) else { continue };
            if !spec.auto_embed || cw.curve.is_some() || id == host {
                continue;
            }
            let (cz0, cz1) = self.wall_extent(cw);
            if cz1 <= hz0 + 1.0 || cz0 >= hz1 - 1.0 {
                continue;
            }
            // parallel to the host and inside its thickness
            let (sa, sx, _) = hp.project(cw.start);
            let (ea, ex, _) = hp.project(cw.end);
            let reach = |x: f64| x <= hl + 1.0 && x >= -hr - 1.0;
            let (dx, dy) = (cw.end.x - cw.start.x, cw.end.y - cw.start.y);
            let (hx, hy) = (hw.end.x - hw.start.x, hw.end.y - hw.start.y);
            let sin = (dx * hy - dy * hx).abs() / (dx.hypot(dy) * hx.hypot(hy)).max(1e-9);
            if sin > 0.02 || !reach(sx) || !reach(ex) {
                continue;
            }
            let (u0, u1) = (sa.min(ea).max(0.0), sa.max(ea).min(hlen));
            if u1 - u0 < 1.0 {
                continue;
            }
            out.push(crate::family::Opening { instance: id, u0, u1, z0: cz0.max(hz0), z1: cz1.min(hz1) });
        }
        out
    }
}

/// The curtain wall types a new project starts with.
pub fn starter_types() -> Vec<(String, CurtainSpec)> {
    let mullion = MullionSpec::rectangular(50.0, 150.0);
    vec![
        ("Curtain Wall".to_string(), CurtainSpec::default()),
        (
            "Storefront".to_string(),
            CurtainSpec {
                u: GridRule::new(GridLayout::MaximumSpacing, 1200.0),
                v: GridRule::new(GridLayout::FixedDistance, 2100.0),
                u_interior: Some(mullion.clone()),
                u_border1: Some(mullion.clone()),
                u_border2: Some(mullion.clone()),
                v_interior: Some(mullion.clone()),
                v_border1: Some(mullion.clone()),
                v_border2: Some(mullion.clone()),
                join: JoinCondition::BorderAndVertical,
                auto_embed: true,
                ..CurtainSpec::default()
            },
        ),
        (
            "Exterior Glazing".to_string(),
            CurtainSpec {
                u: GridRule { layout: GridLayout::FixedDistance, spacing_mm: 1500.0, justify: Justify::Center, ..GridRule::default() },
                v: GridRule::new(GridLayout::FixedDistance, 3000.0),
                u_interior: Some(mullion.clone()),
                u_border1: Some(mullion.clone()),
                u_border2: Some(mullion.clone()),
                v_interior: Some(mullion.clone()),
                v_border1: Some(mullion.clone()),
                v_border2: Some(mullion),
                join: JoinCondition::VerticalContinuous,
                ..CurtainSpec::default()
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_rules_lay_out_their_lines() {
        let fixed = GridRule::new(GridLayout::FixedDistance, 1000.0);
        assert_eq!(fixed.positions(3500.0), vec![1000.0, 2000.0, 3000.0]);
        let end = GridRule { justify: Justify::End, ..fixed.clone() };
        assert_eq!(end.positions(3500.0), vec![500.0, 1500.0, 2500.0]);
        let centre = GridRule { justify: Justify::Center, ..fixed.clone() };
        assert_eq!(centre.positions(3500.0), vec![750.0, 1750.0, 2750.0]);
        let shifted = GridRule { offset_mm: 200.0, ..fixed };
        assert_eq!(shifted.positions(3500.0), vec![200.0, 1200.0, 2200.0, 3200.0]);
        assert_eq!(GridRule { layout: GridLayout::FixedNumber, number: 3, ..GridRule::default() }.positions(4000.0), vec![1000.0, 2000.0, 3000.0]);
        // 3500 at most 1200 apart: 3 panels; at least 1200 apart: 2 panels
        assert_eq!(GridRule::new(GridLayout::MaximumSpacing, 1200.0).positions(3600.0), vec![1200.0, 2400.0]);
        assert_eq!(GridRule::new(GridLayout::MinimumSpacing, 1200.0).positions(3500.0), vec![1750.0]);
        assert!(GridRule::default().positions(5000.0).is_empty());
    }

    #[test]
    fn a_storefront_has_panels_between_its_mullions() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let sf = d.wall_types().into_iter().find(|(_, t)| t.name == "Storefront").unwrap().0;
        let w = d.add_wall(sf, lvl, Point::new(0.0, 0.0), Point::new(3600.0, 0.0), 3000.0).unwrap();
        let Some(Element::Wall(wall)) = d.get(w).cloned() else { panic!() };
        let solids = d.curtain_solids(&wall);
        let panels: Vec<&CurtainSolid> = solids.iter().filter(|s| s.part == CurtainPart::Panel).collect();
        // 3600 at most 1200 apart: 3 bays; a transom at 2100: 2 rows
        assert_eq!(panels.len(), 6);
        let vertical = solids.iter().filter(|s| s.part == CurtainPart::Mullion { vertical: true }).count();
        let horizontal = solids.iter().filter(|s| s.part == CurtainPart::Mullion { vertical: false }).count();
        // 4 verticals run through; the head and sill borders run through too (border and vertical); the transom breaks into 3
        assert_eq!(vertical, 4);
        assert_eq!(horizontal, 2 + 3);
        // the first panel starts at the border mullion's face and stops at the first interior one
        let first = panels.iter().min_by(|a, b| a.ring[0].x.total_cmp(&b.ring[0].x).then(a.z0.total_cmp(&b.z0))).unwrap();
        let xs: Vec<f64> = first.ring.iter().map(|p| p.x).collect();
        assert!((xs.iter().cloned().fold(f64::INFINITY, f64::min) - 50.0).abs() < 1e-6);
        assert!((xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 1175.0).abs() < 1e-6);
        assert!((first.z0 - 50.0).abs() < 1e-6 && (first.z1 - 2075.0).abs() < 1e-6, "{} {}", first.z0, first.z1);
        // its outline in plan is the glazing zone
        let o = d.curtain_outline(&wall);
        assert_eq!(o.len(), 4);
        assert!(o.iter().all(|p| (p.y.abs() - 75.0).abs() < 1e-6));
    }

    #[test]
    fn a_storefront_in_a_brick_wall_cuts_its_opening() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let brick = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        let sf = d.wall_types().into_iter().find(|(_, t)| t.name == "Storefront").unwrap().0;
        let b = d.add_wall(brick, lvl, Point::new(0.0, 0.0), Point::new(8000.0, 0.0), 3000.0).unwrap();
        let c = d.add_wall(sf, lvl, Point::new(2000.0, 0.0), Point::new(4400.0, 0.0), 2400.0).unwrap();
        let cuts = d.wall_openings(b);
        assert_eq!(cuts.len(), 1);
        assert_eq!(cuts[0].instance, c);
        assert!((cuts[0].u0 - 2000.0).abs() < 1e-6 && (cuts[0].u1 - 4400.0).abs() < 1e-6 && (cuts[0].z1 - 2400.0).abs() < 1e-6);
        // the curtain wall does not join the brick wall, and plain glazing does not embed
        assert!(d.mid_join_hosts().is_empty());
        let cw = d.wall_types().into_iter().find(|(_, t)| t.name == "Curtain Wall").unwrap().0;
        d.add_wall(cw, lvl, Point::new(5000.0, 0.0), Point::new(6000.0, 0.0), 2400.0).unwrap();
        assert_eq!(d.wall_openings(b).len(), 1);
        // a door does not go in a curtain wall (curtain wall doors come with panels)
        let door = d.family_types().into_iter().find(|(_, t)| d.family(t.family).is_some_and(|f| f.category == crate::family::FamilyCategory::Doors)).unwrap().0;
        assert!(d.place_on_wall(door, c, Point::new(3000.0, 0.0)).is_err());
    }
}
