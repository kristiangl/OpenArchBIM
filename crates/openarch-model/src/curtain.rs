//! Curtain walls: a wall whose type is of the curtain kind. The type's grid rules divide the wall's face into cells
//! (U lines across its length, V lines up its height); each cell holds a panel (glazed, solid or empty) and mullions
//! run along the grid lines, interior and border types for each direction, joined by the type's join condition.
//! Everything is worked out from the wall's line, base and top each time it is drawn: the pieces are solids, each a
//! plan footprint swept between two heights, so plans, sections, elevations and the 3D view draw them alike.
//!
//! Each wall can add grid lines of its own (the Curtain Grid tool, or a file), take segments out of any interior line
//! (the cells either side then share one panel), unpin panels and mullions from the type, swap an unpinned panel and
//! delete an unpinned mullion. These are kept on the wall by position (`CurtainLines`), so they hold while the grid
//! around them changes. Panels, mullions and grid lines are not elements: each has a part id (`part_id`) made from
//! its wall and its place in the grid, which selection and Properties use.
//!
//! A curtain type keeps one layer as thick as its glazing zone so the rest of the program (extents, location line,
//! selection) treats the wall as it does any other; the layer is not drawn. Curtain walls take no part in wall
//! joins. With Automatically Embed on, a curtain wall standing in a basic wall cuts its opening in it.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::*;
use crate::family::FamilyInstance;
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
    /// A basic wall type stretched to the cell, its layers across it.
    Wall,
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
    /// The basic wall type of a Wall panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_type: Option<Id>,
}

/// What a panel can be swapped for: a system panel, a basic wall type, or a door type (a door instance in the cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelChoice {
    System(PanelKind),
    Wall(Id),
    Door(Id),
}

fn default_panel_thickness() -> f64 {
    25.0
}
fn default_glass() -> String {
    "Glass".into()
}

impl Default for PanelSpec {
    fn default() -> Self {
        PanelSpec { kind: PanelKind::Glazed, thickness_mm: default_panel_thickness(), offset_mm: 0.0, material: default_glass(), wall_type: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum MullionShape {
    /// Width 1 and Width 2 either side of the grid line, along the face; Thickness across the wall.
    Rectangular { width1_mm: f64, width2_mm: f64, thickness_mm: f64 },
    Circular { radius_mm: f64 },
    /// Corner mullions, for where two curtain walls meet end to end (see `corner_rings`). At an end that meets no
    /// other curtain wall, or on an interior line, each draws as a rectangle of its sizes.
    /// Two arms `thickness_mm` thick wrapping the corner, `leg1_mm` along the first wall and `leg2_mm` along the second.
    LCorner { leg1_mm: f64, leg2_mm: f64, thickness_mm: f64 },
    /// Two equal arms.
    VCorner { leg_mm: f64, thickness_mm: f64 },
    /// A post on the corner, `depth1_mm` along the first wall and `depth2_mm` along the second.
    QuadCorner { depth1_mm: f64, depth2_mm: f64 },
    /// A block across the corner, `depth_mm` deep, whose sides sit square to each wall `centre_width_mm / 2` past
    /// the corner.
    TrapezoidCorner { centre_width_mm: f64, depth_mm: f64 },
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
            MullionShape::LCorner { leg1_mm: w, .. } | MullionShape::VCorner { leg_mm: w, .. } | MullionShape::QuadCorner { depth1_mm: w, .. } | MullionShape::TrapezoidCorner { centre_width_mm: w, .. } => (w.max(0.0) / 2.0, w.max(0.0) / 2.0),
        }
    }

    /// How deep it is across the wall.
    pub fn depth(&self) -> f64 {
        match self.shape {
            MullionShape::Rectangular { thickness_mm, .. } => thickness_mm.max(0.0),
            MullionShape::Circular { radius_mm } => 2.0 * radius_mm.max(0.0),
            MullionShape::LCorner { leg2_mm: d, .. } | MullionShape::VCorner { leg_mm: d, .. } | MullionShape::QuadCorner { depth2_mm: d, .. } | MullionShape::TrapezoidCorner { depth_mm: d, .. } => d.max(0.0),
        }
    }

    /// Its type's name: its shape and sizes.
    pub fn name(&self) -> String {
        let r = |x: f64| (x * 10.0).round() / 10.0;
        match self.shape {
            MullionShape::Rectangular { width1_mm, width2_mm, thickness_mm } => format!("Rectangular Mullion: {} x {}", r(width1_mm + width2_mm), r(thickness_mm)),
            MullionShape::Circular { radius_mm } => format!("Circular Mullion: {} mm radius", r(radius_mm)),
            MullionShape::LCorner { leg1_mm, leg2_mm, thickness_mm } => format!("L Corner Mullion: {} x {} x {}", r(leg1_mm), r(leg2_mm), r(thickness_mm)),
            MullionShape::VCorner { leg_mm, thickness_mm } => format!("V Corner Mullion: {} x {}", r(leg_mm), r(thickness_mm)),
            MullionShape::QuadCorner { depth1_mm, depth2_mm } => format!("Quad Corner Mullion: {} x {}", r(depth1_mm), r(depth2_mm)),
            MullionShape::TrapezoidCorner { centre_width_mm, depth_mm } => format!("Trapezoid Corner Mullion: {} x {}", r(centre_width_mm), r(depth_mm)),
        }
    }

    pub fn is_corner(&self) -> bool {
        !matches!(self.shape, MullionShape::Rectangular { .. } | MullionShape::Circular { .. })
    }

    /// The plan footprint of a corner mullion where two walls meet at `c`: `a` and `b` run from the corner along
    /// the first and second wall, `na` and `nb` are their left (exterior) sides. Convex pieces, counter-clockwise.
    pub fn corner_rings(&self, c: Point, a: Point, na: Point, b: Point, nb: Point) -> Vec<Vec<Point>> {
        let at = |p: Point, d: Point, t: f64, n: Point, o: f64| Point::new(p.x + d.x * t + n.x * o, p.y + d.y * t + n.y * o);
        // where the lines `o` off each wall cross
        let mitre = |o: f64| -> Option<Point> {
            let (pa, pb) = (at(c, a, 0.0, na, o), at(c, b, 0.0, nb, o));
            let det = a.x * (-b.y) - a.y * (-b.x);
            if det.abs() < 1e-6 {
                return None;
            }
            let (rx, ry) = (pb.x - pa.x, pb.y - pa.y);
            let t = (rx * (-b.y) - ry * (-b.x)) / det;
            Some(at(pa, a, t, Point::new(0.0, 0.0), 0.0))
        };
        let along = |p: Point, d: Point| (p.x - c.x) * d.x + (p.y - c.y) * d.y;
        let off = self.offset_mm;
        let ccw = |mut ring: Vec<Point>| {
            let area: f64 = (0..ring.len()).map(|i| ring[i].x * ring[(i + 1) % ring.len()].y - ring[(i + 1) % ring.len()].x * ring[i].y).sum();
            if area < 0.0 {
                ring.reverse();
            }
            ring
        };
        // an arm `t` thick, its outer face `o1` off the walls, reaching `leg` past the outer corner along wall `d`
        let arm = |d: Point, n: Point, o0: f64, o1: f64, leg: f64| -> Option<Vec<Point>> {
            let (m0, m1) = (mitre(o0)?, mitre(o1)?);
            let end = (along(m1, d) + leg).max(along(m0, d) + 1.0);
            Some(ccw(vec![m0, m1, at(c, d, end, n, o1), at(c, d, end, n, o0)]))
        };
        let legs = match self.shape {
            MullionShape::LCorner { leg1_mm, leg2_mm, thickness_mm } => Some((leg1_mm, leg2_mm, thickness_mm)),
            MullionShape::VCorner { leg_mm, thickness_mm } => Some((leg_mm, leg_mm, thickness_mm)),
            _ => None,
        };
        let rings: Option<Vec<Vec<Point>>> = match self.shape {
            MullionShape::LCorner { .. } | MullionShape::VCorner { .. } => {
                let (leg1, leg2, t) = legs.unwrap();
                let (o0, o1) = (off - t / 2.0, off + t / 2.0);
                (|| Some(vec![arm(a, na, o0, o1, leg1)?, arm(b, nb, o0, o1, leg2)?]))()
            }
            MullionShape::QuadCorner { depth1_mm, depth2_mm } => mitre(off).map(|m| {
                let p = |s: f64, t: f64| Point::new(m.x + a.x * s + b.x * t, m.y + a.y * s + b.y * t);
                let (h1, h2) = (depth1_mm / 2.0, depth2_mm / 2.0);
                vec![ccw(vec![p(-h1, -h2), p(h1, -h2), p(h1, h2), p(-h1, h2)])]
            }),
            MullionShape::TrapezoidCorner { centre_width_mm, depth_mm } => mitre(off).map(|m| {
                let (o0, o1) = (off - depth_mm / 2.0, off + depth_mm / 2.0);
                let (ta, tb) = (along(m, a) + centre_width_mm / 2.0, along(m, b) + centre_width_mm / 2.0);
                vec![ccw(vec![at(c, a, ta, na, o0), at(c, a, ta, na, o1), at(c, b, tb, nb, o1), at(c, b, tb, nb, o0)])]
            }),
            _ => None,
        };
        rings.unwrap_or_default()
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

/// What one curtain wall changes on its type's grid. Grid lines placed by hand (or read from a file): along its length
/// from its start, and up its height from its base; they add to the lines the type lays out. Segments taken out of
/// lines, mullions unpinned or deleted, and panels unpinned or swapped, each kept by where it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CurtainLines {
    #[serde(default)]
    pub u: Vec<f64>,
    #[serde(default)]
    pub v: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<SegmentAt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unpinned: Vec<SegmentAt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<SegmentAt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<PanelAt>,
    /// Where a vertical and a horizontal mullion meet and the wall says which runs through, not the join condition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub joins: Vec<JoinAt>,
}

/// The crossing of the vertical (U) line `u` along the wall and the horizontal (V) line `v` up it, and which mullion
/// runs through there: the vertical one, or the horizontal one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JoinAt {
    pub u: f64,
    pub v: f64,
    pub vertical: bool,
}

/// One segment of a grid line, between the lines that cross it: on the vertical (U) line `at` along the wall, or the
/// horizontal (V) line `at` up it; the segment holding `mid`, measured the other way.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SegmentAt {
    pub vertical: bool,
    pub at: f64,
    pub mid: f64,
}

/// A panel taken off the type (unpinned): the one covering the point `u` along the wall and `v` up it; its own
/// panel once it has been swapped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelAt {
    pub u: f64,
    pub v: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<PanelSpec>,
    /// While a grid edit runs, a door in the wall rides along as one of these so it keeps its cell: the door (0 for
    /// one to be placed) and its type. Never stored: the door keeps its own place (`HostRef::v_mm`).
    #[serde(skip)]
    pub door: Option<(Id, Id)>,
}

/// A panel, mullion or grid line of a curtain wall, by its place in the grid: a vertical (U) line `k`, a horizontal
/// (V) line `k`, the panel whose first cell is `i` along and `j` up, or the mullion on line `k` from segment `s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CurtainRef {
    Line { vertical: bool, k: usize },
    Panel { i: usize, j: usize },
    Mullion { vertical: bool, k: usize, s: usize },
}

/// Part ids sit above every element id and below the site point ids, under 2^53 so they survive JSON.
pub const PART_BASE: u64 = 1 << 51;

/// The id selection and Properties use for a part of curtain wall `wall`.
pub fn part_id(wall: Id, r: CurtainRef) -> Id {
    let (kind, a, b) = match r {
        CurtainRef::Line { vertical, k } => (if vertical { 0 } else { 1 }, k, 0),
        CurtainRef::Panel { i, j } => (2, i, j),
        CurtainRef::Mullion { vertical, k, s } => (if vertical { 3 } else { 4 }, k, s),
    };
    PART_BASE + (((wall << 3) | kind) << 24) + ((a as u64 & 0xfff) << 12) + (b as u64 & 0xfff)
}

/// The wall and the part a part id names.
pub fn part_of(id: Id) -> Option<(Id, CurtainRef)> {
    if !(PART_BASE..2 * PART_BASE).contains(&id) {
        return None;
    }
    let x = id - PART_BASE;
    let (head, a, b) = (x >> 24, ((x >> 12) & 0xfff) as usize, (x & 0xfff) as usize);
    let r = match head & 7 {
        0 => CurtainRef::Line { vertical: true, k: a },
        1 => CurtainRef::Line { vertical: false, k: a },
        2 => CurtainRef::Panel { i: a, j: b },
        3 => CurtainRef::Mullion { vertical: true, k: a, s: b },
        4 => CurtainRef::Mullion { vertical: false, k: a, s: b },
        _ => return None,
    };
    Some((head >> 3, r))
}

/// A run of mullion on one grid line: segments `s0..s1`, from `lo` to `hi` along the line (up a U line, along a V
/// line), joined where this direction runs through.
#[derive(Debug, Clone, PartialEq)]
pub struct MullionRun {
    pub vertical: bool,
    pub k: usize,
    pub s0: usize,
    pub s1: usize,
    pub lo: f64,
    pub hi: f64,
    pub pinned: bool,
}

/// A curtain wall's grid worked out: its lines (ends included), which segments and mullions are there, which cells
/// share a panel, and its mullion runs.
#[derive(Debug, Clone)]
pub struct CurtainLayout {
    pub len: f64,
    pub height: f64,
    pub us: Vec<f64>,
    pub vs: Vec<f64>,
    /// Laid out by the type (the edges count): these are pinned and stay where the type puts them.
    pub u_typed: Vec<bool>,
    pub v_typed: Vec<bool>,
    /// `[line][segment]`: whether the segment is there (not taken out).
    pub u_on: Vec<Vec<bool>>,
    pub v_on: Vec<Vec<bool>>,
    /// `[line][segment]`: whether a mullion stands on it.
    pub u_mull: Vec<Vec<bool>>,
    pub v_mull: Vec<Vec<bool>>,
    /// The cells (`i` along, `j` up) of each panel, its first cell first; and the panel of each cell.
    pub panels: Vec<Vec<(usize, usize)>>,
    pub cell_panel: Vec<Vec<usize>>,
    pub runs: Vec<MullionRun>,
    /// What each line's mullion takes up across it: along the wall for a U line, up it for a V line.
    pub uspan: Vec<(f64, f64)>,
    pub vspan: Vec<(f64, f64)>,
    /// `[U line][V line]`: where they cross, whether the vertical mullion runs through (else the horizontal one).
    pub through: Vec<Vec<bool>>,
    /// The wall's start and end, where each meets another curtain wall with a corner mullion between them.
    pub corners: [Option<CornerEnd>; 2],
}

/// The end of a curtain wall that meets the end of another, with a corner mullion between them.
#[derive(Debug, Clone)]
pub struct CornerEnd {
    pub other: Id,
    pub mullion: MullionSpec,
    /// How far the corner mullion reaches along this wall from the corner.
    pub reach: f64,
    /// This wall draws the corner mullion; the other leaves it out.
    pub draws: bool,
    pub rings: Vec<Vec<Point>>,
}

impl CurtainLayout {
    /// The mullion type on line `k`: a corner mullion where an end meets another curtain wall, else the type's.
    pub fn mullion<'a>(&'a self, spec: &'a CurtainSpec, vertical: bool, k: usize) -> Option<&'a MullionSpec> {
        let last = self.lines(vertical).len() - 1;
        if vertical && (k == 0 || k == last) {
            if let Some(c) = &self.corners[usize::from(k == last && k > 0)] {
                return Some(&c.mullion);
            }
        }
        spec.mullion_on(vertical, k, last).as_ref()
    }

    /// Whether a vertical and a horizontal mullion meet where U line `k` crosses V line `j`.
    pub fn meets(&self, k: usize, j: usize) -> bool {
        let (nu, nv) = (self.us.len() - 1, self.vs.len() - 1);
        let vertical = (j > 0 && self.u_mull[k][j - 1]) || (j < nv && self.u_mull[k][j]);
        let horizontal = (k > 0 && self.v_mull[j][k - 1]) || (k < nu && self.v_mull[j][k]);
        vertical && horizontal
    }

    /// The cell holding the point `u` along the wall and `v` up it, or the nearest one to a point off the wall.
    pub fn cell_at(&self, u: f64, v: f64) -> Option<(usize, usize)> {
        let inside = |p: f64, lines: &[f64]| p.clamp(lines[0] + 1e-6, lines[lines.len() - 1] - 1e-6);
        Some((seg_index(&self.us, inside(u, &self.us))?, seg_index(&self.vs, inside(v, &self.vs))?))
    }

    /// Panel `n` inside the mullions round it: along the wall from and to, and up it from and to. Cells that do not
    /// make a rectangle give the box round them.
    pub fn panel_rect(&self, n: usize) -> Option<(f64, f64, f64, f64)> {
        let cells = self.panels.get(n)?;
        let rects = self.panel_rects(n);
        if rects.len() == 1 {
            return rects.first().copied();
        }
        let (imin, imax) = (cells.iter().map(|c| c.0).min()?, cells.iter().map(|c| c.0).max()?);
        let (jmin, jmax) = (cells.iter().map(|c| c.1).min()?, cells.iter().map(|c| c.1).max()?);
        Some((self.us[imin], self.us[imax + 1], self.vs[jmin], self.vs[jmax + 1]))
    }

    /// The rectangles panel `n` fills inside the mullions round it: one when its cells make a rectangle, else one
    /// per cell.
    pub fn panel_rects(&self, n: usize) -> Vec<(f64, f64, f64, f64)> {
        let Some(cells) = self.panels.get(n) else { return vec![] };
        let (us, vs) = (&self.us, &self.vs);
        let (nu, nv) = (us.len() - 1, vs.len() - 1);
        let same = |i: usize, j: usize| cells.contains(&(i, j));
        let edges = |i: usize, j: usize| -> (f64, f64, f64, f64) {
            let left = if (i > 0 && same(i - 1, j)) || !self.u_mull[i][j] { us[i] } else { self.uspan[i].1 };
            let right = if (i + 1 < nu && same(i + 1, j)) || !self.u_mull[i + 1][j] { us[i + 1] } else { self.uspan[i + 1].0 };
            let low = if (j > 0 && same(i, j - 1)) || !self.v_mull[j][i] { vs[j] } else { self.vspan[j].1 };
            let high = if (j + 1 < nv && same(i, j + 1)) || !self.v_mull[j + 1][i] { vs[j + 1] } else { self.vspan[j + 1].0 };
            (left, right, low, high)
        };
        let (imin, imax) = (cells.iter().map(|c| c.0).min().unwrap(), cells.iter().map(|c| c.0).max().unwrap());
        let (jmin, jmax) = (cells.iter().map(|c| c.1).min().unwrap(), cells.iter().map(|c| c.1).max().unwrap());
        if cells.len() == (imax - imin + 1) * (jmax - jmin + 1) {
            vec![(
                (jmin..=jmax).map(|j| edges(imin, j).0).fold(f64::NEG_INFINITY, f64::max),
                (jmin..=jmax).map(|j| edges(imax, j).1).fold(f64::INFINITY, f64::min),
                (imin..=imax).map(|i| edges(i, jmin).2).fold(f64::NEG_INFINITY, f64::max),
                (imin..=imax).map(|i| edges(i, jmax).3).fold(f64::INFINITY, f64::min),
            )]
        } else {
            cells.iter().map(|(i, j)| edges(*i, *j)).collect()
        }
    }

    /// The panel covering cell `(i, j)`, as its part.
    pub fn panel_ref(&self, i: usize, j: usize) -> CurtainRef {
        let (i, j) = self.panels[self.cell_panel[i][j]][0];
        CurtainRef::Panel { i, j }
    }

    /// The run of mullion a segment belongs to.
    pub fn run_of(&self, vertical: bool, k: usize, s: usize) -> Option<&MullionRun> {
        self.runs.iter().find(|r| r.vertical == vertical && r.k == k && (r.s0..r.s1).contains(&s))
    }

    /// Which lines in one direction the type lays out.
    pub fn typed(&self, vertical: bool) -> &[bool] {
        if vertical {
            &self.u_typed
        } else {
            &self.v_typed
        }
    }

    /// The lines crossing a U line (the V lines) or a V line.
    pub fn across(&self, vertical: bool) -> &[f64] {
        if vertical {
            &self.vs
        } else {
            &self.us
        }
    }
    pub fn lines(&self, vertical: bool) -> &[f64] {
        if vertical {
            &self.us
        } else {
            &self.vs
        }
    }
}

const TYPED_LINE: &str = "This grid line comes from the curtain wall type: change the type's grid to move or remove it.";

/// The line within a millimetre of `at`.
fn line_index(lines: &[f64], at: f64) -> Option<usize> {
    lines.iter().position(|p| (p - at).abs() < 1.0)
}

/// The segment between two lines that holds `mid`.
fn seg_index(lines: &[f64], mid: f64) -> Option<usize> {
    (0..lines.len().saturating_sub(1)).find(|j| lines[*j] < mid && mid < lines[j + 1])
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

    /// The mullion type on grid line `k` of `last + 1`: a border one on the first and last, else the interior one.
    pub fn mullion_on(&self, vertical: bool, k: usize, last: usize) -> &Option<MullionSpec> {
        match (vertical, k == 0, k == last) {
            (true, true, _) => &self.u_border1,
            (true, _, true) => &self.u_border2,
            (true, _, _) => &self.u_interior,
            (false, true, _) => &self.v_border1,
            (false, _, true) => &self.v_border2,
            (false, _, _) => &self.v_interior,
        }
    }

    /// The one bookkeeping layer a curtain type carries.
    pub fn layer(&self) -> Layer {
        Layer::new(&self.panel.material, self.thickness(), LayerFunction::Structure, true)
    }
}

/// A row of the curtain panel schedule: its size (inside the mullions round it, mm), material and U-value (W/m²K).
#[derive(Debug, Clone, PartialEq)]
pub struct CurtainPanelRow {
    pub wall: Id,
    pub part: Id,
    pub family: String,
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub material: String,
    pub u_value: Option<f64>,
}

/// A row of the mullion schedule: a run of mullion, its type and length (mm).
#[derive(Debug, Clone, PartialEq)]
pub struct CurtainMullionRow {
    pub wall: Id,
    pub part: Id,
    pub name: String,
    pub length: f64,
    pub material: String,
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
    /// The panel or mullion it is (a panel over merged cells, or a mullion run, can be several solids).
    pub at: CurtainRef,
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
        self.curtain_layout(w).map(|l| (l.us, l.vs))
    }

    /// A curtain wall's grid worked out: the type's lines and the wall's own, the segments taken out, the cells that
    /// share a panel, and where each mullion runs.
    pub fn curtain_layout(&self, w: &Wall) -> Option<CurtainLayout> {
        let spec = self.curtain_of(w)?;
        let len = w.path().length();
        let (z0, z1) = self.wall_extent(w);
        let h = z1 - z0;
        if len < 1.0 || h < 1.0 {
            return None;
        }
        let none = CurtainLines::default();
        let own = w.curtain_lines.as_ref().unwrap_or(&none);
        let (ru, rv) = (spec.u.positions(len), spec.v.positions(h));
        // the type's lines and the wall's own, inside its ends
        let merge = |rule: &[f64], own: &[f64], end: f64| {
            let mut all: Vec<f64> = rule.iter().chain(own).copied().filter(|p| *p > 1.0 && *p < end - 1.0).collect();
            all.sort_by(f64::total_cmp);
            all.dedup_by(|a, b| (*a - *b).abs() < 1.0);
            std::iter::once(0.0).chain(all).chain(std::iter::once(end)).collect::<Vec<f64>>()
        };
        let (us, vs) = (merge(&ru, &own.u, len), merge(&rv, &own.v, h));
        let typed = |lines: &[f64], rule: &[f64]| -> Vec<bool> { lines.iter().enumerate().map(|(k, p)| k == 0 || k + 1 == lines.len() || rule.iter().any(|r| (r - p).abs() < 1.0)).collect() };
        let (u_typed, v_typed) = (typed(&us, &ru), typed(&vs, &rv));
        let (nu, nv) = (us.len() - 1, vs.len() - 1);
        // where something kept by position is now: (vertical, line, segment)
        let find = |sa: &SegmentAt| -> Option<(bool, usize, usize)> {
            let (lines, across) = if sa.vertical { (&us, &vs) } else { (&vs, &us) };
            Some((sa.vertical, line_index(lines, sa.at)?, seg_index(across, sa.mid)?))
        };
        let mut u_on = vec![vec![true; nv]; nu + 1];
        let mut v_on = vec![vec![true; nu]; nv + 1];
        for (vert, k, j) in own.removed.iter().filter_map(find) {
            // the edges keep every segment
            if vert && k > 0 && k < nu {
                u_on[k][j] = false;
            } else if !vert && k > 0 && k < nv {
                v_on[k][j] = false;
            }
        }
        // an end meeting another curtain wall takes the corner mullion between them
        let corners = [self.curtain_corner(w, false), self.curtain_corner(w, true)];
        let corner = |vertical: bool, k: usize, last: usize| if !vertical { None } else if k == 0 { corners[0].as_ref() } else if k == last { corners[1].as_ref() } else { None };
        let mullion_on = |vertical: bool, k: usize, last: usize| corner(vertical, k, last).map(|c| &c.mullion).or(spec.mullion_on(vertical, k, last).as_ref());
        let mut u_mull: Vec<Vec<bool>> = (0..=nu).map(|k| (0..nv).map(|j| u_on[k][j] && mullion_on(true, k, nu).is_some()).collect()).collect();
        let mut v_mull: Vec<Vec<bool>> = (0..=nv).map(|j| (0..nu).map(|i| v_on[j][i] && mullion_on(false, j, nv).is_some()).collect()).collect();
        for (vert, k, j) in own.deleted.iter().filter_map(find) {
            if vert {
                u_mull[k][j] = false;
            } else {
                v_mull[k][j] = false;
            }
        }
        let unpinned: Vec<(bool, usize, usize)> = own.unpinned.iter().filter_map(find).collect();
        // what each grid line's mullion takes up across it (a border mullion stays inside the edge)
        let span = |p: f64, k: usize, last: usize, end: f64, vertical: bool| -> (f64, f64) {
            if let Some(c) = corner(vertical, k, last) {
                // a corner mullion reaches along the wall from the corner
                let r = c.reach.clamp(0.0, end);
                return if k == 0 { (0.0, r) } else { (end - r, end) };
            }
            match mullion_on(vertical, k, last) {
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
        let uspan: Vec<(f64, f64)> = us.iter().enumerate().map(|(k, p)| span(*p, k, nu, len, true)).collect();
        let vspan: Vec<(f64, f64)> = vs.iter().enumerate().map(|(j, p)| span(*p, j, nv, h, false)).collect();
        // cells either side of a segment taken out share a panel
        fn root(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        let cell = |i: usize, j: usize| i * nv + j;
        let mut parent: Vec<usize> = (0..nu * nv).collect();
        for k in 1..nu {
            for j in 0..nv {
                if !u_on[k][j] {
                    let (a, b) = (root(&mut parent, cell(k - 1, j)), root(&mut parent, cell(k, j)));
                    parent[a] = b;
                }
            }
        }
        for j in 1..nv {
            for i in 0..nu {
                if !v_on[j][i] {
                    let (a, b) = (root(&mut parent, cell(i, j - 1)), root(&mut parent, cell(i, j)));
                    parent[a] = b;
                }
            }
        }
        let mut panels: Vec<Vec<(usize, usize)>> = vec![];
        let mut cell_panel = vec![vec![0; nv]; nu];
        let mut of_root: std::collections::HashMap<usize, usize> = Default::default();
        for i in 0..nu {
            for j in 0..nv {
                let r = root(&mut parent, cell(i, j));
                let n = *of_root.entry(r).or_insert_with(|| {
                    panels.push(vec![]);
                    panels.len() - 1
                });
                panels[n].push((i, j));
                cell_panel[i][j] = n;
            }
        }
        // where a U line `k` and a V line `j` cross, which runs through: the vertical (true) or the horizontal. One that
        // goes on both sides wins over one that stops there; otherwise the join condition says.
        let continuous = |border: bool| match spec.join {
            JoinCondition::NotDefined | JoinCondition::VerticalContinuous => (true, false),
            JoinCondition::HorizontalContinuous => (false, true),
            JoinCondition::BorderAndVertical => (true, border),
            JoinCondition::BorderAndHorizontal => (border, true),
        };
        // the wall's own say first, where both mullions are there
        let mut forced = vec![vec![None; nv + 1]; nu + 1];
        for ja in &own.joins {
            if let (Some(k), Some(j)) = (line_index(&us, ja.u), line_index(&vs, ja.v)) {
                forced[k][j] = Some(ja.vertical);
            }
        }
        let owner = |k: usize, j: usize| -> bool {
            let (below, above) = (j > 0 && u_mull[k][j - 1], j < nv && u_mull[k][j]);
            let (left, right) = (k > 0 && v_mull[j][k - 1], k < nu && v_mull[j][k]);
            if let Some(v) = forced[k][j].filter(|_| (below || above) && (left || right)) {
                return v;
            }
            let (vt, ht) = (below && above, left && right);
            if vt != ht {
                return vt;
            }
            continuous(k == 0 || k == nu).0 || !continuous(j == 0 || j == nv).1
        };
        let through: Vec<Vec<bool>> = (0..=nu).map(|k| (0..=nv).map(|j| owner(k, j)).collect()).collect();
        // mullion runs: up each U line, then along each V line
        let mut runs = vec![];
        for vertical in [true, false] {
            let (n_lines, n_segs) = if vertical { (nu + 1, nv) } else { (nv + 1, nu) };
            for k in 0..n_lines {
                let has = |s: usize| if vertical { u_mull[k][s] } else { v_mull[k][s] };
                // at node `t` along this line: whether this line runs through it, whether a crossing mullion is there,
                // and the crossing line and what its mullion takes up
                let mine = |t: usize| if vertical { owner(k, t) } else { !owner(t, k) };
                let other = |t: usize| {
                    if vertical {
                        (k > 0 && v_mull[t][k - 1]) || (k < nu && v_mull[t][k])
                    } else {
                        (k > 0 && u_mull[t][k - 1]) || (k < nv && u_mull[t][k])
                    }
                };
                let cross = |t: usize| if vertical { (vs[t], vspan[t]) } else { (us[t], uspan[t]) };
                let typed_line = if vertical { u_typed[k] } else { v_typed[k] };
                let mut s = 0;
                while s < n_segs {
                    if !has(s) {
                        s += 1;
                        continue;
                    }
                    let s0 = s;
                    while s + 1 < n_segs && has(s + 1) && mine(s + 1) {
                        s += 1;
                    }
                    let s1 = s + 1;
                    s = s1;
                    // an end it owns reaches the far face of the crossing mullion; else it stops at the near one
                    let lo = match (other(s0), mine(s0)) {
                        (false, _) => cross(s0).0,
                        (true, true) => cross(s0).1 .0,
                        (true, false) => cross(s0).1 .1,
                    };
                    let hi = match (other(s1), mine(s1)) {
                        (false, _) => cross(s1).0,
                        (true, true) => cross(s1).1 .1,
                        (true, false) => cross(s1).1 .0,
                    };
                    let pinned = typed_line && (s0..s1).any(|x| !unpinned.contains(&(vertical, k, x)));
                    runs.push(MullionRun { vertical, k, s0, s1, lo, hi, pinned });
                }
            }
        }
        Some(CurtainLayout { len, height: h, us, vs, u_typed, v_typed, u_on, v_on, u_mull, v_mull, panels, cell_panel, runs, uspan, vspan, through, corners })
    }

    /// The corner a curtain wall's start (or end) makes with the end of another curtain wall, when a corner
    /// mullion goes there: either wall's border mullion at that end, the first one's when both have one. The first
    /// is the wall that ends at the corner (or, when both start or both end there, the one starting further down
    /// and to the left); it draws the corner mullion.
    pub fn curtain_corner(&self, w: &Wall, at_end: bool) -> Option<CornerEnd> {
        let spec = self.curtain_of(w)?;
        // the corner, the way along the wall from it, and the wall's left
        let end = |w: &Wall, at_end: bool| -> Option<(Point, Point, Point)> {
            let pts = w.path().pts;
            let (p, q) = if at_end { (*pts.last()?, pts[pts.len().checked_sub(2)?]) } else { (*pts.first()?, *pts.get(1)?) };
            let l = p.dist(&q);
            if l < 1e-6 {
                return None;
            }
            let away = Point::new((q.x - p.x) / l, (q.y - p.y) / l);
            let fwd = if at_end { Point::new(-away.x, -away.y) } else { away };
            Some((p, away, Point::new(-fwd.y, fwd.x)))
        };
        let (c, a, na) = end(w, at_end)?;
        let mine = if at_end { &spec.u_border2 } else { &spec.u_border1 };
        for (id, e) in self.iter() {
            let Element::Wall(o) = e else { continue };
            let Some(ospec) = self.curtain_of(o) else { continue };
            for o_end in [false, true] {
                let Some((oc, b, nb)) = end(o, o_end) else { continue };
                // end to end, at an angle
                if oc.dist(&c) > 1.0 || (a.x * b.y - a.y * b.x).abs() < 0.05 {
                    continue;
                }
                let theirs = if o_end { &ospec.u_border2 } else { &ospec.u_border1 };
                let key = |w: &Wall| (w.start.x, w.start.y, w.end.x, w.end.y);
                let first = if at_end != o_end { at_end } else { key(w).partial_cmp(&key(o)) == Some(std::cmp::Ordering::Less) };
                let corner = |m: &Option<MullionSpec>| m.clone().filter(|m| m.is_corner());
                let mullion = if first { corner(mine).or(corner(theirs)) } else { corner(theirs).or(corner(mine)) }?;
                // the walls' left sides agree when one ends where the other starts; else the second one's is turned
                // to agree with the first's
                let agree = |n: Point| if o_end == at_end { Point::new(-n.x, -n.y) } else { n };
                let rings = if first { mullion.corner_rings(c, a, na, b, agree(nb)) } else { mullion.corner_rings(c, b, nb, a, agree(na)) };
                let reach = rings.iter().flatten().map(|p| (p.x - c.x) * a.x + (p.y - c.y) * a.y).fold(0.0, f64::max);
                return Some(CornerEnd { other: id, mullion, reach, draws: first, rings });
            }
        }
        None
    }

    /// Panel `n` of a curtain wall: whether it is pinned to the type, and the panel it shows.
    pub fn curtain_panel_state(&self, w: &Wall, lay: &CurtainLayout, n: usize) -> (bool, PanelSpec) {
        let spec = self.curtain_of(w).map(|s| s.panel.clone()).unwrap_or_default();
        let Some(cells) = lay.panels.get(n) else { return (true, spec) };
        let inside = |p: &PanelAt| cells.iter().any(|(i, j)| lay.us[*i] < p.u && p.u < lay.us[i + 1] && lay.vs[*j] < p.v && p.v < lay.vs[j + 1]);
        match w.curtain_lines.as_ref().and_then(|o| o.panels.iter().find(|p| inside(p))) {
            Some(p) => (false, p.panel.clone().unwrap_or(spec)),
            None => (true, spec),
        }
    }

    /// What panel `n` of a curtain wall is, for its type selector and schedules: the type's id (0 to 2 for the system
    /// panels, else a basic wall type or a door type), its family and its name.
    pub fn curtain_panel_type(&self, wall: Id, w: &Wall, lay: &CurtainLayout, n: usize) -> (Id, String, String) {
        let door = self.curtain_panel_door(wall, lay, n).and_then(|id| match self.get(id) {
            Some(Element::FamilyInstance(i)) => self.instance_family(i).map(|(f, t)| (i.type_id, f.name.clone(), t.name.clone())),
            _ => None,
        });
        if let Some(d) = door {
            return d;
        }
        let (_, panel) = self.curtain_panel_state(w, lay, n);
        match (panel.kind, panel.wall_type.and_then(|t| self.wall_type(t).map(|wt| (t, wt.name.clone())))) {
            (PanelKind::Wall, Some((t, name))) => (t, "Basic Wall".into(), name),
            (PanelKind::Solid, _) => (1, "System Panel".into(), "Solid".into()),
            (PanelKind::Empty, _) | (PanelKind::Wall, None) => (2, "Empty System Panel".into(), "Empty".into()),
            (PanelKind::Glazed, _) => (0, "System Panel".into(), "Glazed".into()),
        }
    }

    /// Every curtain panel and mullion in the project, for the schedules. Panels holding a door are left to the door
    /// schedule. A panel's U-value is worked out from its materials' conductivity plus the standard surface
    /// resistances of a wall (0.17 m²K/W); none when a material has no conductivity.
    pub fn curtain_schedule(&self) -> (Vec<CurtainPanelRow>, Vec<CurtainMullionRow>) {
        let (mut panels, mut mullions) = (vec![], vec![]);
        for (id, e) in self.iter() {
            let Element::Wall(w) = e else { continue };
            let (Some(spec), Some(lay)) = (self.curtain_of(w), self.curtain_layout(w)) else { continue };
            for (n, cells) in lay.panels.iter().enumerate() {
                if self.curtain_panel_door(id, &lay, n).is_some() {
                    continue;
                }
                let Some((l, r, lo, hi)) = lay.panel_rect(n) else { continue };
                let (_, family, name) = self.curtain_panel_type(id, w, &lay, n);
                let (_, panel) = self.curtain_panel_state(w, &lay, n);
                let layers: Vec<(String, f64)> = match panel.kind {
                    PanelKind::Empty => vec![],
                    PanelKind::Wall => panel.wall_type.and_then(|t| self.wall_type(t)).map_or(vec![], |t| t.layers.iter().filter(|l| !l.is_void).map(|l| (l.material.clone(), l.thickness_mm)).collect()),
                    _ => vec![(panel.material.clone(), panel.thickness_mm)],
                };
                let resistance: Option<f64> = layers.iter().map(|(m, t)| self.material(m).and_then(|m| m.physical.conductivity_w_mk).filter(|k| *k > 0.0).map(|k| t / 1000.0 / k)).sum();
                let (i, j) = cells[0];
                panels.push(CurtainPanelRow {
                    wall: id,
                    part: part_id(id, CurtainRef::Panel { i, j }),
                    family,
                    name,
                    width: r - l,
                    height: hi - lo,
                    material: layers.first().map_or(String::new(), |l| l.0.clone()),
                    u_value: resistance.filter(|_| !layers.is_empty()).map(|r| 1.0 / (r + 0.17)),
                });
            }
            for run in &lay.runs {
                let Some(m) = lay.mullion(spec, run.vertical, run.k) else { continue };
                let corner = if run.vertical && run.k == 0 { lay.corners[0].as_ref() } else if run.vertical && run.k + 1 == lay.us.len() { lay.corners[1].as_ref() } else { None };
                if run.hi - run.lo < 1.0 || corner.is_some_and(|c| !c.draws) {
                    continue;
                }
                mullions.push(CurtainMullionRow { wall: id, part: part_id(id, CurtainRef::Mullion { vertical: run.vertical, k: run.k, s: run.s0 }), name: m.name(), length: run.hi - run.lo, material: m.material.clone() });
            }
        }
        (panels, mullions)
    }

    /// The doors standing in a curtain wall's cells: each door, the point along and up the wall it holds, and its
    /// type.
    pub fn curtain_doors(&self, wall: Id) -> Vec<(Id, f64, f64, Id)> {
        self.iter()
            .filter_map(|(id, e)| match e {
                Element::FamilyInstance(i) => i.host.filter(|h| h.wall == wall).and_then(|h| Some((id, h.u_mm, h.v_mm?, i.type_id))),
                _ => None,
            })
            .collect()
    }

    /// The door filling panel `n` of a curtain wall.
    pub fn curtain_panel_door(&self, wall: Id, lay: &CurtainLayout, n: usize) -> Option<Id> {
        let cells = lay.panels.get(n)?;
        self.curtain_doors(wall).into_iter().find(|d| lay.cell_at(d.1, d.2).is_some_and(|c| cells.contains(&c))).map(|d| d.0)
    }

    /// The panel a door in a curtain wall fills, inside the mullions round it: along the wall from and to, up it from
    /// and to. None for a door that is not in a curtain wall's cell.
    pub fn curtain_door_cell(&self, inst: &FamilyInstance) -> Option<(f64, f64, f64, f64)> {
        let h = inst.host?;
        let v = h.v_mm?;
        let Some(Element::Wall(w)) = self.get(h.wall) else { return None };
        let lay = self.curtain_layout(w)?;
        let (i, j) = lay.cell_at(h.u_mm, v)?;
        lay.panel_rect(lay.cell_panel[i][j])
    }

    /// The pieces of a curtain wall: its panels and mullions as plan footprints swept between heights.
    pub fn curtain_solids(&self, id: Id) -> Vec<CurtainSolid> {
        let Some(Element::Wall(w)) = self.get(id) else { return vec![] };
        let Some(spec) = self.curtain_of(w) else { return vec![] };
        let Some(lay) = self.curtain_layout(w) else { return vec![] };
        let path = w.path();
        let pts = path.pts.clone();
        let cum = cumulative(&pts);
        let (zb, _) = self.wall_extent(w);
        let us = &lay.us;
        let nu = us.len() - 1;
        let doors = self.curtain_doors(id);
        let mut out = vec![];
        // panels: each inside the mullions round it; cells that share a panel reach the line between them. A door
        // draws itself.
        for (n, cells) in lay.panels.iter().enumerate() {
            if doors.iter().any(|d| lay.cell_at(d.1, d.2).is_some_and(|c| cells.contains(&c))) {
                continue;
            }
            let (_, panel) = self.curtain_panel_state(w, &lay, n);
            // a wall panel is its type's layers across the cell, exterior first; a system panel is one layer
            let layers: Vec<(String, f64)> = match panel.kind {
                PanelKind::Empty => continue,
                PanelKind::Wall => match panel.wall_type.and_then(|t| self.wall_type(t)).filter(|t| t.curtain.is_none()) {
                    Some(t) => t.layers.iter().map(|l| (l.material.clone(), l.thickness_mm)).collect(),
                    None => continue,
                },
                _ => vec![(panel.material.clone(), panel.thickness_mm)],
            };
            let total: f64 = layers.iter().map(|l| l.1.max(0.0)).sum();
            if total <= 0.0 {
                continue;
            }
            let (i0, j0) = cells[0];
            let at = CurtainRef::Panel { i: i0, j: j0 };
            for (ua, ub, za, zb2) in lay.panel_rects(n) {
                if ub - ua < 1.0 || zb2 - za < 1.0 {
                    continue;
                }
                let mut c1 = panel.offset_mm + total / 2.0;
                for (material, t) in &layers {
                    let c0 = c1 - t.max(0.0);
                    if c1 - c0 > 1e-6 {
                        for ring in band(&pts, &cum, ua, ub, c0, c1) {
                            out.push(CurtainSolid { part: CurtainPart::Panel, at, ring, z0: zb + za, z1: zb + zb2, material: material.clone(), glazed: panel.kind == PanelKind::Glazed });
                        }
                    }
                    c1 = c0;
                }
            }
        }
        // mullions: each run on its line; a corner mullion drawn by one of the two walls
        for r in &lay.runs {
            let Some(m) = lay.mullion(spec, r.vertical, r.k) else { continue };
            if r.hi - r.lo < 1e-6 {
                continue;
            }
            let (c0, c1) = (m.offset_mm - m.depth() / 2.0, m.offset_mm + m.depth() / 2.0);
            let at = CurtainRef::Mullion { vertical: r.vertical, k: r.k, s: r.s0 };
            let part = CurtainPart::Mullion { vertical: r.vertical };
            if r.vertical {
                let (ua, ub) = lay.uspan[r.k];
                let corner = if r.k == 0 { lay.corners[0].as_ref() } else if r.k == nu { lay.corners[1].as_ref() } else { None };
                let rings: Vec<Vec<Point>> = match (corner, &m.shape) {
                    (Some(c), _) if c.draws => c.rings.clone(),
                    (Some(_), _) => continue,
                    (None, MullionShape::Circular { radius_mm }) => {
                        let p = us[r.k];
                        let centre = band(&pts, &cum, p, p + 0.001, m.offset_mm, m.offset_mm + 0.001).first().map(|q| q[0]).unwrap_or(pts[0]);
                        vec![(0..16).map(|i| f64::from(i) * std::f64::consts::PI / 8.0).map(|t| Point::new(centre.x + radius_mm * t.cos(), centre.y + radius_mm * t.sin())).collect()]
                    }
                    (None, _) => band(&pts, &cum, ua, ub, c0, c1),
                };
                for ring in rings {
                    out.push(CurtainSolid { part, at, ring, z0: zb + r.lo, z1: zb + r.hi, material: m.material.clone(), glazed: false });
                }
            } else {
                let (za, zb2) = lay.vspan[r.k];
                if zb2 - za < 1e-6 {
                    continue;
                }
                for ring in band(&pts, &cum, r.lo, r.hi, c0, c1) {
                    out.push(CurtainSolid { part, at, ring, z0: zb + za, z1: zb + zb2, material: m.material.clone(), glazed: false });
                }
            }
        }
        out
    }

    /// Changes what a curtain wall keeps of its own grid, as one undo step. Its doors ride along as panels kept by
    /// position (`PanelAt::door`), so they keep their cells as lines come and go; one taken out goes, and one put in
    /// (door 0) is placed.
    fn edit_curtain(&mut self, wall: Id, label: &str, f: impl FnOnce(&CurtainLayout, &mut CurtainLines) -> Result<(), String>) -> Result<(), String> {
        let Some(Element::Wall(mut w)) = self.get(wall).cloned() else { return Err("not a wall".into()) };
        let lay = self.curtain_layout(&w).ok_or("not a curtain wall")?;
        let mut own = w.curtain_lines.clone().unwrap_or_default();
        let doors = self.curtain_doors(wall);
        own.panels.extend(doors.iter().map(|d| PanelAt { u: d.1, v: d.2, panel: None, door: Some((d.0, d.3)) }));
        f(&lay, &mut own)?;
        let kept: Vec<PanelAt> = own.panels.iter().filter(|p| p.door.is_some()).cloned().collect();
        own.panels.retain(|p| p.door.is_none());
        w.curtain_lines = (own != CurtainLines::default()).then_some(own);
        // the doors as they are to be
        let (l, r) = self.wall_extents(&w);
        let frame = crate::family::frame_for_wall(l + r, self.wall_core(&w).0);
        let mut changes: Vec<(Id, Option<FamilyInstance>)> = doors.iter().filter(|d| !kept.iter().any(|p| p.door.is_some_and(|k| k.0 == d.0))).map(|d| (d.0, None)).collect();
        for p in &kept {
            let (id, type_id) = p.door.unwrap();
            let host = Some(crate::family::HostRef { wall, u_mm: p.u, v_mm: Some(p.v) });
            let inst = match self.get(id) {
                Some(Element::FamilyInstance(i)) if id != 0 => FamilyInstance { type_id, host, ..i.clone() },
                _ => {
                    let mut i = FamilyInstance::new(type_id, w.base_level);
                    i.host = host;
                    if let Some(fam) = self.family_type(type_id).and_then(|t| self.family(t.family)) {
                        // built-in doors take the frame that suits the wall
                        if fam.param("Frame Type").is_some() && fam.param("Jamb Depth").is_some() {
                            i.values.insert("Frame Type".into(), crate::family::ParamValue::Text(frame.0.into()));
                            i.values.insert("Jamb Depth".into(), crate::family::ParamValue::Number(frame.1));
                        }
                        i.ifc.export_as = fam.category.ifc_class().to_string();
                        i.ifc.predefined_type = fam.category.ifc_predefined().to_string();
                    }
                    i
                }
            };
            if id == 0 || self.get(id) != Some(&Element::FamilyInstance(inst.clone())) {
                changes.push((id, Some(inst)));
            }
        }
        self.transact(label, |tx| {
            tx.update(wall, Element::Wall(w))?;
            for (id, inst) in changes {
                match inst {
                    None => tx.remove(id)?,
                    Some(i) if id == 0 => {
                        tx.add(Element::FamilyInstance(i));
                    }
                    Some(i) => tx.update(id, Element::FamilyInstance(i))?,
                }
            }
            Ok(())
        })
    }

    /// Adds a grid line to a curtain wall: vertical (a U line) `at` along it from its start, or horizontal (a V line)
    /// `at` up it from its base. With `only`, the line keeps just the segment holding that point (One Segment).
    pub fn add_curtain_line(&mut self, wall: Id, vertical: bool, at: f64, only: Option<f64>) -> Result<(), String> {
        self.edit_curtain(wall, "Curtain Grid", |lay, own| {
            let lines = lay.lines(vertical);
            let end = *lines.last().unwrap();
            if !(at > 1.0 && at < end - 1.0) {
                return Err("A grid line goes inside the curtain wall, not on its edge.".into());
            }
            if line_index(lines, at).is_some() {
                return Err("There is a grid line there already.".into());
            }
            if vertical {
                own.u.push(at);
            } else {
                own.v.push(at);
            }
            // what is kept on segments the new line crosses holds for both halves, and a panel kept by a point stays
            // with the half it is in
            let span = |x: f64| seg_index(lines, x).map(|j| (lines[j], lines[j + 1])).filter(|(a, b)| *a < at && at < *b);
            for l in [&mut own.removed, &mut own.unpinned, &mut own.deleted] {
                let mut halves = vec![];
                for s in l.iter_mut().filter(|s| s.vertical != vertical) {
                    if let Some((a, b)) = span(s.mid) {
                        s.mid = (a + at) / 2.0;
                        halves.push(SegmentAt { mid: (at + b) / 2.0, ..*s });
                    }
                }
                l.extend(halves);
            }
            for p in own.panels.iter_mut() {
                let x = if vertical { &mut p.u } else { &mut p.v };
                if let Some((a, b)) = span(*x) {
                    *x = if *x < at { (a + at) / 2.0 } else { (at + b) / 2.0 };
                }
            }
            if let Some(mid) = only {
                for w in lay.across(vertical).windows(2) {
                    if !(w[0] < mid && mid < w[1]) {
                        own.removed.push(SegmentAt { vertical, at, mid: (w[0] + w[1]) / 2.0 });
                    }
                }
            }
            Ok(())
        })
    }

    /// Takes a grid line placed on the wall off it again, with what was kept on it.
    pub fn remove_curtain_line(&mut self, wall: Id, vertical: bool, k: usize) -> Result<(), String> {
        self.edit_curtain(wall, "Delete Curtain Grid", |lay, own| {
            let at = *lay.lines(vertical).get(k).ok_or("no such grid line")?;
            if lay.typed(vertical)[k] {
                return Err(TYPED_LINE.into());
            }
            let list = if vertical { &mut own.u } else { &mut own.v };
            list.retain(|p| (p - at).abs() >= 1.0);
            for l in [&mut own.removed, &mut own.unpinned, &mut own.deleted] {
                l.retain(|s| !(s.vertical == vertical && (s.at - at).abs() < 1.0));
            }
            own.joins.retain(|j| (if vertical { j.u } else { j.v } - at).abs() >= 1.0);
            Ok(())
        })
    }

    /// Moves a grid line placed on the wall to `to`, between the lines either side of it. What is kept on it moves
    /// with it; what is kept in the segments and cells beside it stays with them.
    pub fn move_curtain_line(&mut self, wall: Id, vertical: bool, k: usize, to: f64) -> Result<(), String> {
        self.edit_curtain(wall, "Move Curtain Grid", |lay, own| {
            let lines = lay.lines(vertical);
            let at = *lines.get(k).ok_or("no such grid line")?;
            if lay.typed(vertical)[k] {
                return Err(TYPED_LINE.into());
            }
            if !(to > lines[k - 1] + 1.0 && to < lines[k + 1] - 1.0) {
                return Err("A grid line moves only as far as the lines either side of it.".into());
            }
            let list = if vertical { &mut own.u } else { &mut own.v };
            for p in list.iter_mut().filter(|p| (**p - at).abs() < 1.0) {
                *p = to;
            }
            let remid = |mid: f64| match seg_index(lines, mid) {
                Some(j) if j == k || j + 1 == k => (if j == k { to } else { lines[j] } + if j + 1 == k { to } else { lines[j + 1] }) / 2.0,
                _ => mid,
            };
            for l in [&mut own.removed, &mut own.unpinned, &mut own.deleted] {
                for s in l.iter_mut() {
                    if s.vertical == vertical && (s.at - at).abs() < 1.0 {
                        s.at = to;
                    } else if s.vertical != vertical {
                        s.mid = remid(s.mid);
                    }
                }
            }
            for p in own.panels.iter_mut() {
                if vertical {
                    p.u = remid(p.u);
                } else {
                    p.v = remid(p.v);
                }
            }
            for j in own.joins.iter_mut() {
                let x = if vertical { &mut j.u } else { &mut j.v };
                if (*x - at).abs() < 1.0 {
                    *x = to;
                }
            }
            Ok(())
        })
    }

    /// Takes segment `s` out of interior grid line `k`, or puts it back (Add/Remove Segments). Returns whether the
    /// segment is there now.
    pub fn toggle_curtain_segment(&mut self, wall: Id, vertical: bool, k: usize, s: usize) -> Result<bool, String> {
        let mut now = false;
        self.edit_curtain(wall, "Add/Remove Segments", |lay, own| {
            let n = lay.lines(vertical).len() - 1;
            if k == 0 || k >= n {
                return Err("The edges of a curtain wall keep all their segments.".into());
            }
            let across = lay.across(vertical);
            if s + 1 >= across.len() {
                return Err("no such segment".into());
            }
            let at = lay.lines(vertical)[k];
            let on = if vertical { lay.u_on[k][s] } else { lay.v_on[k][s] };
            if on {
                own.removed.push(SegmentAt { vertical, at, mid: (across[s] + across[s + 1]) / 2.0 });
            } else {
                own.removed.retain(|x| !(x.vertical == vertical && (x.at - at).abs() < 1.0 && across[s] < x.mid && x.mid < across[s + 1]));
            }
            now = !on;
            Ok(())
        })?;
        Ok(now)
    }

    /// Pins the mullion holding segment `s` of line `k` to the type, or unpins it.
    pub fn set_curtain_mullion_pinned(&mut self, wall: Id, vertical: bool, k: usize, s: usize, pinned: bool) -> Result<(), String> {
        self.edit_curtain(wall, if pinned { "Pin" } else { "Unpin" }, |lay, own| {
            let r = lay.run_of(vertical, k, s).ok_or("there is no mullion there")?.clone();
            if !lay.typed(vertical)[k] {
                return Err("A mullion on a grid line placed by hand is not pinned to the type.".into());
            }
            let (at, across) = (lay.lines(vertical)[k], lay.across(vertical));
            for x in r.s0..r.s1 {
                own.unpinned.retain(|q| !(q.vertical == vertical && (q.at - at).abs() < 1.0 && across[x] < q.mid && q.mid < across[x + 1]));
                if !pinned {
                    own.unpinned.push(SegmentAt { vertical, at, mid: (across[x] + across[x + 1]) / 2.0 });
                }
            }
            Ok(())
        })
    }

    /// Deletes the mullion holding segment `s` of line `k`. One pinned to the type has to be unpinned first.
    pub fn delete_curtain_mullion(&mut self, wall: Id, vertical: bool, k: usize, s: usize) -> Result<(), String> {
        self.edit_curtain(wall, "Delete Mullion", |lay, own| {
            let r = lay.run_of(vertical, k, s).ok_or("there is no mullion there")?.clone();
            if r.pinned {
                return Err("The mullion is pinned to the curtain wall type: unpin it first.".into());
            }
            let (at, across) = (lay.lines(vertical)[k], lay.across(vertical));
            for x in r.s0..r.s1 {
                own.deleted.push(SegmentAt { vertical, at, mid: (across[x] + across[x + 1]) / 2.0 });
            }
            Ok(())
        })
    }

    /// Pins the panel over cell `(i, j)` to the type (it takes the type's panel again), or unpins it.
    pub fn set_curtain_panel_pinned(&mut self, wall: Id, i: usize, j: usize, pinned: bool) -> Result<(), String> {
        self.edit_curtain(wall, if pinned { "Pin" } else { "Unpin" }, |lay, own| {
            let cells = lay.cell_panel.get(i).and_then(|c| c.get(j)).map(|n| &lay.panels[*n]).ok_or("there is no panel there")?;
            let (us, vs) = (&lay.us, &lay.vs);
            own.panels.retain(|p| !cells.iter().any(|(i, j)| us[*i] < p.u && p.u < us[i + 1] && vs[*j] < p.v && p.v < vs[j + 1]));
            if !pinned {
                let (i0, j0) = cells[0];
                own.panels.push(PanelAt { u: (us[i0] + us[i0 + 1]) / 2.0, v: (vs[j0] + vs[j0 + 1]) / 2.0, panel: None, door: None });
            }
            Ok(())
        })
    }

    /// Swaps the unpinned panel over cell `(i, j)`: for a system panel as thick as the type's, a basic wall type
    /// stretched to it, or a door of a door type sized by it.
    pub fn set_curtain_panel(&mut self, wall: Id, i: usize, j: usize, choice: PanelChoice) -> Result<(), String> {
        let base = match self.get(wall) {
            Some(Element::Wall(w)) => self.curtain_of(w).ok_or("not a curtain wall")?.panel.clone(),
            _ => return Err("not a wall".into()),
        };
        let wall_panel = match choice {
            PanelChoice::Wall(t) => {
                let wt = self.wall_type(t).filter(|t| t.curtain.is_none()).ok_or("A panel takes a basic wall type.")?;
                Some(PanelSpec { kind: PanelKind::Wall, thickness_mm: wt.thickness_mm(), material: wt.layers.first().map_or(base.material.clone(), |l| l.material.clone()), wall_type: Some(t), ..base.clone() })
            }
            PanelChoice::Door(t) => {
                let fam = self.family_type(t).and_then(|ft| self.family(ft.family)).ok_or("unknown door type")?;
                if fam.category != crate::family::FamilyCategory::Doors {
                    return Err("A panel takes a door type, not a window or other family.".into());
                }
                None
            }
            PanelChoice::System(_) => None,
        };
        self.edit_curtain(wall, "Change Panel", |lay, own| {
            let cells = lay.cell_panel.get(i).and_then(|c| c.get(j)).map(|n| &lay.panels[*n]).ok_or("there is no panel there")?;
            let (us, vs) = (&lay.us, &lay.vs);
            let inside = |p: &PanelAt| cells.iter().any(|(i, j)| us[*i] < p.u && p.u < us[i + 1] && vs[*j] < p.v && p.v < vs[j + 1]);
            if !own.panels.iter().any(|p| p.door.is_none() && inside(p)) {
                return Err("The panel is pinned to the curtain wall type: unpin it first.".into());
            }
            if let PanelChoice::Door(t) = choice {
                // a door already there takes the new type; else one goes in the panel's first cell
                match own.panels.iter_mut().find(|p| p.door.is_some() && inside(p)) {
                    Some(p) => p.door = p.door.map(|(id, _)| (id, t)),
                    None => {
                        let (i0, j0) = cells[0];
                        own.panels.push(PanelAt { u: (us[i0] + us[i0 + 1]) / 2.0, v: (vs[j0] + vs[j0 + 1]) / 2.0, panel: None, door: Some((0, t)) });
                    }
                }
                return Ok(());
            }
            own.panels.retain(|p| !(p.door.is_some() && inside(p)));
            let p = own.panels.iter_mut().find(|p| inside(p)).unwrap();
            p.panel = match choice {
                PanelChoice::System(kind) => (kind != base.kind).then(|| PanelSpec {
                    kind,
                    material: match kind {
                        PanelKind::Glazed => default_glass(),
                        PanelKind::Solid => default_aluminium(),
                        _ => base.material.clone(),
                    },
                    wall_type: None,
                    ..base.clone()
                }),
                _ => wall_panel,
            };
            Ok(())
        })
    }

    /// Says which mullion runs through at each crossing `(k, j)` of U line `k` and V line `j`: the vertical one
    /// (true) or the horizontal one.
    fn set_curtain_joins(&mut self, wall: Id, label: &str, nodes: &[(usize, usize, bool)]) -> Result<(), String> {
        self.edit_curtain(wall, label, |lay, own| {
            for &(k, j, vertical) in nodes {
                let (u, v) = (*lay.us.get(k).ok_or("no such grid line")?, *lay.vs.get(j).ok_or("no such grid line")?);
                if !lay.meets(k, j) {
                    return Err("No two mullions meet there.".into());
                }
                own.joins.retain(|x| !((x.u - u).abs() < 1.0 && (x.v - v).abs() < 1.0));
                own.joins.push(JoinAt { u, v, vertical });
            }
            Ok(())
        })
    }

    /// Toggles which mullion runs through where U line `k` crosses V line `j` (Toggle Mullion Join). Returns whether
    /// the vertical one runs through now.
    pub fn toggle_curtain_join(&mut self, wall: Id, k: usize, j: usize) -> Result<bool, String> {
        let Some(Element::Wall(w)) = self.get(wall) else { return Err("not a wall".into()) };
        let lay = self.curtain_layout(w).ok_or("not a curtain wall")?;
        let now = !*lay.through.get(k).and_then(|t| t.get(j)).ok_or("no such crossing")?;
        self.set_curtain_joins(wall, "Toggle Mullion Join", &[(k, j, now)])?;
        Ok(now)
    }

    /// Make Continuous: the mullion holding segment `s` of line `k` runs through the mullions it meets at its ends.
    /// Break at Join (`continuous` false): it stops at each mullion it runs through.
    pub fn set_curtain_mullion_continuous(&mut self, wall: Id, vertical: bool, k: usize, s: usize, continuous: bool) -> Result<(), String> {
        let Some(Element::Wall(w)) = self.get(wall) else { return Err("not a wall".into()) };
        let lay = self.curtain_layout(w).ok_or("not a curtain wall")?;
        let r = lay.run_of(vertical, k, s).ok_or("there is no mullion there")?.clone();
        let node = |t: usize| if vertical { (k, t) } else { (t, k) };
        let nodes: Vec<(usize, usize, bool)> = if continuous {
            [r.s0, r.s1].into_iter().map(node).filter(|(a, b)| lay.meets(*a, *b) && lay.through[*a][*b] != vertical).map(|(a, b)| (a, b, vertical)).collect()
        } else {
            (r.s0 + 1..r.s1).map(node).filter(|(a, b)| lay.meets(*a, *b)).map(|(a, b)| (a, b, !vertical)).collect()
        };
        if nodes.is_empty() {
            return Err(if continuous { "The mullion already runs through the mullions at its ends." } else { "The mullion runs through no other mullion." }.into());
        }
        self.set_curtain_joins(wall, if continuous { "Make Continuous" } else { "Break at Join" }, &nodes)
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
        let solids = d.curtain_solids(w);
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

    #[test]
    fn a_curtain_walls_own_grid_segments_and_panels() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let sf = d.wall_types().into_iter().find(|(_, t)| t.name == "Storefront").unwrap().0;
        let w = d.add_wall(sf, lvl, Point::new(0.0, 0.0), Point::new(3600.0, 0.0), 3000.0).unwrap();
        let wall = |d: &Document| match d.get(w) {
            Some(Element::Wall(x)) => x.clone(),
            _ => panic!(),
        };
        let count = |d: &Document, f: &dyn Fn(&CurtainSolid) -> bool| d.curtain_solids(w).iter().filter(|s| f(s)).count();
        let panels = |d: &Document| count(d, &|s| s.part == CurtainPart::Panel);
        assert_eq!(panels(&d), 6);
        // a line placed by hand: 4 bays
        d.add_curtain_line(w, true, 600.0, None).unwrap();
        let lay = d.curtain_layout(&wall(&d)).unwrap();
        assert_eq!(lay.us, vec![0.0, 600.0, 1200.0, 2400.0, 3600.0]);
        assert_eq!(lay.u_typed, vec![true, false, true, true, true]);
        assert_eq!(panels(&d), 8);
        assert!(d.add_curtain_line(w, true, 600.4, None).is_err(), "one there already");
        assert!(d.add_curtain_line(w, true, 0.5, None).is_err(), "on the edge");
        // a segment out of the type's line at 1200: the two lower cells share one panel, and the mullion there goes
        assert!(!d.toggle_curtain_segment(w, true, 2, 0).unwrap());
        assert_eq!(panels(&d), 7);
        let solids = d.curtain_solids(w);
        let merged = solids.iter().find(|s| s.part == CurtainPart::Panel && s.at == CurtainRef::Panel { i: 1, j: 0 }).unwrap();
        let xs: Vec<f64> = merged.ring.iter().map(|p| p.x).collect();
        assert!((xs.iter().cloned().fold(f64::INFINITY, f64::min) - 625.0).abs() < 1e-6 && (xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 2375.0).abs() < 1e-6, "{xs:?}");
        let on_1200 = |d: &Document| d.curtain_layout(&wall(d)).unwrap().runs.iter().filter(|r| r.vertical && r.k == 2).map(|r| (r.s0, r.s1)).collect::<Vec<_>>();
        assert_eq!(on_1200(&d), vec![(1, 2)]);
        assert!(d.toggle_curtain_segment(w, true, 0, 0).is_err(), "the edges keep their segments");
        // the line placed by hand moves, the type's does not; what was taken out of the type's line stays out
        assert!(d.move_curtain_line(w, true, 2, 1300.0).is_err());
        d.move_curtain_line(w, true, 1, 700.0).unwrap();
        assert_eq!(d.curtain_layout(&wall(&d)).unwrap().us[1], 700.0);
        assert_eq!(panels(&d), 7);
        assert!(d.move_curtain_line(w, true, 1, 1250.0).is_err(), "not past its neighbour");
        // part ids name the wall and the part
        for r in [CurtainRef::Line { vertical: true, k: 3 }, CurtainRef::Panel { i: 2, j: 1 }, CurtainRef::Mullion { vertical: false, k: 1, s: 4 }] {
            assert_eq!(part_of(part_id(w, r)), Some((w, r)));
        }
        assert_eq!(part_of(w), None);
        // a mullion of the type's is pinned: unpinned, it can go
        let verticals = |d: &Document| count(d, &|s| s.part == CurtainPart::Mullion { vertical: true });
        let before = verticals(&d);
        assert!(d.delete_curtain_mullion(w, true, 3, 0).is_err());
        d.set_curtain_mullion_pinned(w, true, 3, 0, false).unwrap();
        d.delete_curtain_mullion(w, true, 3, 0).unwrap();
        assert_eq!(verticals(&d), before - 1);
        // the panel next to it now reaches the line
        // a panel is pinned until unpinned; then it can be swapped, and pinning it takes the type's panel again
        assert!(d.set_curtain_panel(w, 0, 1, PanelChoice::System(PanelKind::Solid)).is_err());
        d.set_curtain_panel_pinned(w, 0, 1, false).unwrap();
        d.set_curtain_panel(w, 0, 1, PanelChoice::System(PanelKind::Solid)).unwrap();
        let solid = |d: &Document| count(d, &|s| s.part == CurtainPart::Panel && !s.glazed && s.material == "Aluminium");
        assert_eq!(solid(&d), 1);
        d.set_curtain_panel(w, 0, 1, PanelChoice::System(PanelKind::Empty)).unwrap();
        assert_eq!(panels(&d), 6);
        d.set_curtain_panel_pinned(w, 0, 1, true).unwrap();
        assert_eq!((panels(&d), solid(&d)), (7, 0));
        // One Segment: a transom across the first bay only
        d.add_curtain_line(w, false, 1000.0, Some(300.0)).unwrap();
        let lay = d.curtain_layout(&wall(&d)).unwrap();
        let k = lay.vs.iter().position(|v| *v == 1000.0).unwrap();
        assert_eq!(lay.v_on[k], vec![true, false, false, false]);
        assert_eq!(panels(&d), 8);
        // a line through the middle of a segment taken out leaves both halves out
        d.add_curtain_line(w, true, 3000.0, None).unwrap();
        let lay = d.curtain_layout(&wall(&d)).unwrap();
        assert_eq!(lay.v_on[k], vec![true, false, false, false, false]);
        d.undo().unwrap();
        // each change is one undo step; taking the hand-placed line off takes what was kept on it
        d.undo().unwrap();
        assert_eq!(panels(&d), 7);
        d.remove_curtain_line(w, true, 1).unwrap();
        assert!(d.remove_curtain_line(w, true, 1).is_err(), "now the type's line at 1200");
        assert_eq!(d.curtain_layout(&wall(&d)).unwrap().us, vec![0.0, 1200.0, 2400.0, 3600.0]);
    }

    #[test]
    fn doors_walls_joins_and_corners_in_curtain_walls() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let sf = d.wall_types().into_iter().find(|(_, t)| t.name == "Storefront").unwrap().0;
        let w = d.add_wall(sf, lvl, Point::new(0.0, 0.0), Point::new(3600.0, 0.0), 3000.0).unwrap();
        let lay = |d: &Document| match d.get(w) {
            Some(Element::Wall(x)) => d.curtain_layout(x).unwrap(),
            _ => panic!(),
        };
        let panel_at = |d: &Document, i: usize, j: usize| d.curtain_solids(w).into_iter().filter(|s| s.at == CurtainRef::Panel { i, j }).collect::<Vec<_>>();

        // a door in the first bay's lower panel: sized by the panel inside its mullions
        let door_type = d.family_types().into_iter().find(|(_, t)| d.family(t.family).unwrap().name == "Single-Flush" && t.name == "820 x 2040").unwrap().0;
        assert!(d.set_curtain_panel(w, 0, 0, PanelChoice::Door(door_type)).is_err(), "pinned");
        d.set_curtain_panel_pinned(w, 0, 0, false).unwrap();
        d.set_curtain_panel(w, 0, 0, PanelChoice::Door(door_type)).unwrap();
        let doors = d.curtain_doors(w);
        assert_eq!(doors.len(), 1);
        assert!(panel_at(&d, 0, 0).is_empty(), "the door stands in for the panel");
        let f = d.instance_frame(doors[0].0).unwrap();
        assert!((f.width - 1125.0).abs() < 1.0 && (f.height - 2025.0).abs() < 1.0, "{} x {}", f.width, f.height);
        assert!((f.z0 - 50.0).abs() < 1.0 && (f.origin.x - 612.5).abs() < 1.0, "{} {:?}", f.z0, f.origin);
        // a line across the cell: the door keeps to the half it is in, and takes its size
        d.add_curtain_line(w, true, 400.0, None).unwrap();
        let f = d.instance_frame(doors[0].0).unwrap();
        assert!((f.width - 750.0).abs() < 1.0 && (f.origin.x - 800.0).abs() < 1.0, "{} {:?}", f.width, f.origin);
        // pinned again, the panel takes the type's glass and the door goes; undo brings it back
        let (i, j) = lay(&d).cell_at(800.0, 1000.0).unwrap();
        d.set_curtain_panel_pinned(w, i, j, true).unwrap();
        assert!(d.curtain_doors(w).is_empty() && d.get(doors[0].0).is_none());
        d.undo().unwrap();
        assert_eq!(d.curtain_doors(w).len(), 1);
        // a door swapped for a system panel goes too
        d.set_curtain_panel(w, i, j, PanelChoice::System(PanelKind::Solid)).unwrap();
        assert!(d.curtain_doors(w).is_empty());
        d.undo().unwrap();

        // a basic wall stretched to the panel at the top of the last bay, one solid per layer, exterior first
        let bv = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        d.set_curtain_panel_pinned(w, 3, 1, false).unwrap();
        d.set_curtain_panel(w, 3, 1, PanelChoice::Wall(bv)).unwrap();
        let layers = panel_at(&d, 3, 1);
        assert_eq!(layers.iter().map(|s| s.material.as_str()).collect::<Vec<_>>(), vec!["Face Brick", "Cavity", "Timber Frame", "Plasterboard"]);
        assert!(d.set_curtain_panel(w, 3, 1, PanelChoice::Wall(sf)).is_err(), "not a curtain type");

        // joins: where line 2 (2400) crosses the transom, the vertical runs through; toggled, the transom does
        let k = lay(&d).us.iter().position(|u| *u == 2400.0).unwrap();
        let runs = |d: &Document| lay(d).runs.iter().filter(|r| r.vertical && r.k == k).map(|r| (r.s0, r.s1)).collect::<Vec<_>>();
        assert_eq!(runs(&d), vec![(0, 2)]);
        assert!(!d.toggle_curtain_join(w, k, 1).unwrap());
        assert_eq!(runs(&d), vec![(0, 1), (1, 2)]);
        // Make Continuous runs it through again, and over the foot, which the sill took before
        let foot = |d: &Document| lay(d).runs.iter().find(|r| r.vertical && r.k == k).unwrap().lo;
        assert_eq!(foot(&d), 50.0);
        d.set_curtain_mullion_continuous(w, true, k, 0, true).unwrap();
        assert_eq!((runs(&d), foot(&d)), (vec![(0, 2)], 0.0));
        // Break at Join stops it at the transom
        d.set_curtain_mullion_continuous(w, true, k, 0, false).unwrap();
        assert_eq!(runs(&d), vec![(0, 1), (1, 2)]);
        assert!(d.set_curtain_mullion_continuous(w, true, k, 0, false).is_err(), "nothing left to break");
        assert!(d.toggle_curtain_join(w, 1, 0).is_ok() && d.toggle_curtain_join(w, k, 1).is_ok());

        // a corner: a quad corner mullion at the storefront's far end, where a second one turns the corner
        let Some(Element::WallType(mut t)) = d.get(sf).cloned() else { panic!() };
        t.curtain.as_mut().unwrap().u_border2 = Some(MullionSpec { shape: MullionShape::QuadCorner { depth1_mm: 150.0, depth2_mm: 150.0 }, offset_mm: 0.0, material: "Aluminium".into() });
        d.transact("type", |tx| tx.update(sf, Element::WallType(t))).unwrap();
        let w2 = d.add_wall(sf, lvl, Point::new(3600.0, 0.0), Point::new(3600.0, 3000.0), 3000.0).unwrap();
        let l1 = lay(&d);
        let c1 = l1.corners[1].as_ref().expect("a corner at the end");
        assert!(c1.draws && c1.other == w2 && (c1.reach - 75.0).abs() < 1e-6);
        assert_eq!(*l1.uspan.last().unwrap(), (3525.0, 3600.0));
        let corner = d.curtain_solids(w).into_iter().find(|s| s.at == CurtainRef::Mullion { vertical: true, k: l1.us.len() - 1, s: 0 }).unwrap();
        let (xs, ys): (Vec<f64>, Vec<f64>) = corner.ring.iter().map(|p| (p.x, p.y)).unzip();
        assert_eq!((xs.iter().cloned().fold(f64::INFINITY, f64::min), ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)), (3525.0, 75.0));
        // the second wall leaves its start to the corner mullion, and its panels stop at it
        let Some(Element::Wall(x2)) = d.get(w2).cloned() else { panic!() };
        let l2 = d.curtain_layout(&x2).unwrap();
        assert!(l2.corners[0].as_ref().is_some_and(|c| !c.draws && c.other == w));
        assert_eq!(l2.uspan[0], (0.0, 75.0));
        assert!(d.curtain_solids(w2).iter().all(|s| s.at != CurtainRef::Mullion { vertical: true, k: 0, s: 0 }));
    }

    #[test]
    fn curtain_panels_and_mullions_are_scheduled() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let sf = d.wall_types().into_iter().find(|(_, t)| t.name == "Storefront").unwrap().0;
        let w = d.add_wall(sf, lvl, Point::new(0.0, 0.0), Point::new(3600.0, 0.0), 3000.0).unwrap();
        let door = d.family_types().into_iter().find(|(_, t)| d.family(t.family).unwrap().name == "Single-Flush" && t.name == "820 x 2040").unwrap().0;
        d.set_curtain_panel_pinned(w, 1, 0, false).unwrap();
        d.set_curtain_panel(w, 1, 0, PanelChoice::Door(door)).unwrap();
        let (panels, mullions) = d.curtain_schedule();
        // six cells, one holding a door, which the door schedule lists
        assert_eq!(panels.len(), 5);
        let first = panels.iter().find(|p| p.part == part_id(w, CurtainRef::Panel { i: 0, j: 0 })).unwrap();
        assert_eq!((first.name.as_str(), first.width, first.height, first.material.as_str()), ("Glazed", 1125.0, 2025.0, "Glass"));
        // 25 mm of glass at 1 W/mK, with the surface resistances
        assert!((first.u_value.unwrap() - 1.0 / (0.025 + 0.17)).abs() < 1e-9);
        let Some(Element::Wall(x)) = d.get(w) else { panic!() };
        assert_eq!(mullions.len(), d.curtain_layout(x).unwrap().runs.len());
        // the border verticals run the full height; the total run is the sum of the runs
        assert!(mullions.iter().any(|m| m.name == "Rectangular Mullion: 50 x 150" && m.length == 3000.0));
    }
}
