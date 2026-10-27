//! Permanent dimensions (Annotate > Dimension).
//!
//! A dimension belongs to one view. Each witness line is a reference to a line on a model
//! element (`RefSpec`, the same names the temporary dimensions use: a face, layer or centre of a
//! wall, an edge or centre of an opening, the end of a wall, a grid), so the dimension follows the
//! element when it moves and the value is always measured, never stored. A chain of three or more
//! references has one segment between each pair, and each segment keeps its own text, lock and
//! the choice of which end stays still when its value is typed.
//!
//! Dimensions are also constraints. A locked segment keeps its distance: when one of its
//! elements moves the other follows. A chain in equality mode keeps its inner references evenly
//! spaced between the first and last. Both are applied after every edit (`Document::settle`),
//! and an edit that cannot be satisfied (a pinned element in the way, two locks that disagree) is
//! refused whole.
//!
//! The round kinds measure a curve or an angle instead of a chain of distances. Radial and diameter
//! take one arc (a curved wall face), angular takes two straight lines, arc length takes an arc and
//! the two lines that bound the part of it to measure. They have one text, no segments to chain, and
//! cannot be typed into or locked yet. For them `line_point` is an offset from the centre of the
//! circle (the apex for an angular dimension), so the dimension travels with the geometry.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::edit::{EditOptions, Xform};
use crate::element::*;
use crate::geom::Point;
use crate::tempdim::{unit, OpeningRef, RefSpec, TempDimSettings};
use crate::view::ViewKind;

/// Two lines count as parallel when their directions differ by less than this (about 0.5 degrees).
pub const PARALLEL_TOL: f64 = 0.0087;
const TOL: f64 = 1e-3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimKind {
    /// True distance between parallel references, drawn parallel to them.
    #[default]
    Aligned,
    /// Distance along the view's X or Y axis only.
    Linear,
    Angular,
    Radial,
    Diameter,
    ArcLength,
}

impl DimKind {
    /// Aligned and linear dimensions chain: three or more references give a segment between each pair.
    pub fn is_chain(self) -> bool {
        matches!(self, DimKind::Aligned | DimKind::Linear)
    }

    pub fn min_refs(self) -> usize {
        match self {
            DimKind::Aligned | DimKind::Linear | DimKind::Angular => 2,
            DimKind::Radial | DimKind::Diameter => 1,
            DimKind::ArcLength => 3,
        }
    }
}

/// What an equal chain shows on its segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqDisplay {
    /// The measured value of each segment.
    #[default]
    Value,
    /// "EQ" on every segment.
    Eq,
    /// One text such as `4 @ 2500 = 10000`.
    Formula,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TickStyle {
    /// Short oblique stroke, the usual architectural tick.
    #[default]
    Tick,
    Arrow,
    Dot,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Millimetres,
    Centimetres,
    Metres,
    FeetInches,
}

/// How a length is written (Format Units).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LengthFormat {
    #[serde(default)]
    pub unit: LengthUnit,
    /// Decimal places (not used for feet and inches).
    #[serde(default)]
    pub decimals: u8,
    #[serde(default = "yes")]
    pub suppress_trailing_zeros: bool,
    /// Write the unit symbol after the number (mm, cm, m).
    #[serde(default)]
    pub symbol: bool,
    /// Smallest fraction of an inch shown, as a denominator (16 gives 1/16").
    #[serde(default = "sixteenth")]
    pub fraction: u32,
}

fn yes() -> bool {
    true
}
fn sixteenth() -> u32 {
    16
}

impl Default for LengthFormat {
    fn default() -> Self {
        LengthFormat { unit: LengthUnit::Millimetres, decimals: 0, suppress_trailing_zeros: true, symbol: false, fraction: 16 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AltPlacement {
    /// In brackets after the primary text.
    #[default]
    Right,
    /// Stacked under the primary text.
    Below,
}

/// Dual dimensioning: a second set of units shown with the first.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AltUnits {
    pub format: LengthFormat,
    #[serde(default)]
    pub placement: AltPlacement,
}

/// How dimensions look (a dimension type). Sizes are on paper, so they scale with the view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionType {
    pub name: String,
    #[serde(default)]
    pub tick: TickStyle,
    #[serde(default = "d_tick")]
    pub tick_size_mm: f64,
    /// Gap left between a witness line and the element it points at.
    #[serde(default = "d_gap")]
    pub witness_gap_mm: f64,
    /// How far a witness line runs past the dimension line.
    #[serde(default = "d_ext")]
    pub witness_extension_mm: f64,
    #[serde(default = "d_text")]
    pub text_size_mm: f64,
    /// Distance from the dimension line to the text.
    #[serde(default = "d_text_off")]
    pub text_offset_mm: f64,
    /// Fill behind the text so lines under it do not run through.
    #[serde(default)]
    pub text_background: bool,
    #[serde(default)]
    pub format: LengthFormat,
    #[serde(default)]
    pub alt: Option<AltUnits>,
    /// Decimal places of an angle, in degrees.
    #[serde(default = "d_angle_decimals")]
    pub angle_decimals: u8,
    /// What leads the text of radial, diameter and arc length dimensions.
    #[serde(default)]
    pub symbols: RoundSymbols,
    /// A labelled dimension shows its value after the parameter's name ("Corridor Width = 1800").
    #[serde(default = "yes")]
    pub show_label_value: bool,
}

/// The symbols written before the value of the round dimensions (Radius, Diameter and Arc Length Symbol).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundSymbols {
    #[serde(default = "d_radius_symbol")]
    pub radius: String,
    #[serde(default = "d_diameter_symbol")]
    pub diameter: String,
    #[serde(default = "d_arc_symbol")]
    pub arc_length: String,
}

fn d_radius_symbol() -> String {
    "R".into()
}
fn d_diameter_symbol() -> String {
    "\u{00D8}".into()
}
fn d_arc_symbol() -> String {
    "\u{2312}".into()
}
fn d_angle_decimals() -> u8 {
    2
}

impl Default for RoundSymbols {
    fn default() -> Self {
        RoundSymbols { radius: d_radius_symbol(), diameter: d_diameter_symbol(), arc_length: d_arc_symbol() }
    }
}

fn d_tick() -> f64 {
    2.0
}
fn d_gap() -> f64 {
    1.5
}
fn d_ext() -> f64 {
    2.0
}
fn d_text() -> f64 {
    2.5
}
fn d_text_off() -> f64 {
    0.8
}

impl DimensionType {
    pub fn standard() -> DimensionType {
        DimensionType {
            name: "Linear - 2.5 mm".into(),
            tick: TickStyle::Tick,
            tick_size_mm: d_tick(),
            witness_gap_mm: d_gap(),
            witness_extension_mm: d_ext(),
            text_size_mm: d_text(),
            text_offset_mm: d_text_off(),
            text_background: false,
            format: LengthFormat::default(),
            alt: None,
            angle_decimals: d_angle_decimals(),
            symbols: RoundSymbols::default(),
            show_label_value: true,
        }
    }
}

/// One witness line: the line it measures from and where on it the person clicked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimRef {
    pub spec: RefSpec,
    /// Where the line was picked; the witness line leaves the element here. The middle of the line when unset.
    #[serde(default)]
    pub pick: Option<[f64; 2]>,
}

/// The text, lock and edit direction of one segment.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SegmentFmt {
    /// Keeps this distance whatever moves.
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub above: String,
    #[serde(default)]
    pub below: String,
    /// Words shown in place of the number (V.I.F., EXISTING). Never another number.
    #[serde(default)]
    pub value_text: Option<String>,
    /// Where the person dragged the text to, as an offset from its usual place, mm in the model.
    #[serde(default)]
    pub text_shift: [f64; 2],
    /// Which end stays still when the value is typed: 0 the first reference, 1 the second.
    #[serde(default)]
    pub anchor: u8,
    /// The global parameter that labels this segment. A driving one sets and holds its length; a reporting one reads it.
    #[serde(default)]
    pub param: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub view: Id,
    #[serde(default)]
    pub kind: DimKind,
    pub dim_type: Id,
    pub refs: Vec<DimRef>,
    /// The direction measured along. For aligned dimensions it only decides which way is positive;
    /// for linear ones it is the axis.
    #[serde(default)]
    pub dir: [f64; 2],
    /// A point the dimension line passes through.
    #[serde(default)]
    pub line_point: [f64; 2],
    /// One entry per segment (the gaps between consecutive references).
    #[serde(default)]
    pub segments: Vec<SegmentFmt>,
    /// Inner references kept evenly spaced.
    #[serde(default)]
    pub equality: bool,
    #[serde(default)]
    pub eq_display: EqDisplay,
    /// Where each reference was when the constraints last held; a difference from now is what
    /// the solver reacts to.
    #[serde(default)]
    pub solved: Vec<f64>,
    /// Name of the parameter this dimension stands for (Label).
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    /// Angular: which way along each reference the angle's arms run (+1 or -1). The cursor's
    /// quadrant when the dimension was placed chooses the acute or the supplementary angle.
    #[serde(default = "plus_plus")]
    pub sides: [i8; 2],
    /// Angular: measure the angle the long way round (the reflex angle).
    #[serde(default)]
    pub reflex: bool,
    /// Radial and diameter: draw a cross at the centre of the circle.
    #[serde(default)]
    pub centre_mark: bool,
}

fn plus_plus() -> [i8; 2] {
    [1, 1]
}

impl Dimension {
    fn constrained(&self) -> bool {
        self.equality || self.segments.iter().any(|s| s.locked || s.param.is_some())
    }

    fn sync_segments(&mut self) {
        let want = if self.kind.is_chain() { self.refs.len().saturating_sub(1) } else { 1 };
        self.segments.resize(want, SegmentFmt::default());
    }

    /// Take reference `k` out; the segments either side of it become one.
    pub fn remove_ref(&mut self, k: usize) {
        let n = self.refs.len();
        if k >= n {
            return;
        }
        self.refs.remove(k);
        if self.solved.len() == n {
            self.solved.remove(k);
        }
        if self.kind.is_chain() && self.segments.len() + 1 == n {
            if k == 0 {
                self.segments.remove(0);
            } else if k == n - 1 {
                self.segments.pop();
            } else {
                self.segments.remove(k);
                self.segments[k - 1] = SegmentFmt::default();
            }
        }
        if self.refs.len() < 3 {
            self.equality = false;
        }
    }

    /// Put a reference in at `k`; the segment it lands in splits in two.
    pub fn insert_ref(&mut self, k: usize, r: DimRef) {
        let n = self.refs.len();
        let k = k.min(n);
        self.refs.insert(k, r);
        if self.solved.len() == n {
            self.solved.clear(); // measured again on the next settle
        }
        if self.kind.is_chain() && self.segments.len() + 1 == n {
            if k == 0 {
                self.segments.insert(0, SegmentFmt::default());
            } else if k == n {
                self.segments.push(SegmentFmt::default());
            } else {
                self.segments[k - 1] = SegmentFmt::default();
                self.segments.insert(k, SegmentFmt::default());
            }
        }
    }
}

/// A reference resolved to geometry in the plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefGeom {
    Line(Point, Point),
    Point(Point),
    Arc(ArcGeom),
}

/// A circular arc in the plan: the angles are in radians, `sweep` is signed (positive counter-clockwise).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcGeom {
    pub centre: Point,
    pub radius: f64,
    pub a0: f64,
    pub sweep: f64,
}

impl ArcGeom {
    pub fn at(&self, a: f64, r: f64) -> Point {
        Point::new(self.centre.x + r * a.cos(), self.centre.y + r * a.sin())
    }

    /// Whether the direction `a` lies within the arc's span.
    pub fn spans(&self, a: f64) -> bool {
        let t = if self.sweep >= 0.0 { (a - self.a0).rem_euclid(std::f64::consts::TAU) } else { (self.a0 - a).rem_euclid(std::f64::consts::TAU) };
        t <= self.sweep.abs() + 1e-9
    }
}

/// A dimension measured against the model as it is now.
#[derive(Debug, Clone)]
pub struct Measured {
    /// Unit vector the distances are measured along.
    pub u: Point,
    /// Unit vector the dimension line runs along.
    pub t: Point,
    /// Where each reference sits along `u`.
    pub pos: Vec<f64>,
    /// The point each witness line leaves from.
    pub anchors: Vec<Point>,
}

// ---------------------------------------------------------------------------------------------
// text

pub(crate) fn num(v: f64, decimals: u8, suppress: bool) -> String {
    let mut s = format!("{:.*}", decimals as usize, v);
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
        s.remove(0);
    }
    if suppress && decimals > 0 && s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// A length in millimetres written the way the format says.
pub fn format_length(mm: f64, f: &LengthFormat) -> String {
    match f.unit {
        LengthUnit::Millimetres => format!("{}{}", num(mm, f.decimals, f.suppress_trailing_zeros), if f.symbol { " mm" } else { "" }),
        LengthUnit::Centimetres => format!("{}{}", num(mm / 10.0, f.decimals, f.suppress_trailing_zeros), if f.symbol { " cm" } else { "" }),
        LengthUnit::Metres => format!("{}{}", num(mm / 1000.0, f.decimals, f.suppress_trailing_zeros), if f.symbol { " m" } else { "" }),
        LengthUnit::FeetInches => {
            let den = f.fraction.max(1) as i64;
            let units = (mm.abs() / 25.4 * den as f64).round() as i64;
            let feet = units / (12 * den);
            let rem = units % (12 * den);
            let (whole, fr) = (rem / den, rem % den);
            let sign = if units > 0 && mm < 0.0 { "-" } else { "" };
            let frac = if fr > 0 {
                let g = gcd(fr, den);
                format!(" {}/{}", fr / g, den / g)
            } else {
                String::new()
            };
            format!("{sign}{feet}' - {whole}{frac}\"")
        }
    }
}

/// Whether text standing in for a measured value is allowed: it may be words, never a number.
pub fn value_text_ok(s: &str) -> bool {
    !s.chars().any(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------------------------------------
// geometry for the drawing

#[derive(Debug, Clone, Serialize)]
pub struct DrawRef {
    pub spec: RefSpec,
    pub pos: f64,
    /// Where the witness line leaves the element.
    pub anchor: [f64; 2],
    /// Where it meets the dimension line.
    pub at: [f64; 2],
}

#[derive(Debug, Clone, Serialize)]
pub struct DrawSeg {
    pub index: usize,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub value_mm: f64,
    /// What is shown on the line: the value, EQ, a formula or the replacement words. Empty when
    /// another segment carries the formula.
    pub text: String,
    pub alt_text: Option<String>,
    pub prefix: String,
    pub suffix: String,
    pub above: String,
    pub below: String,
    pub text_pos: [f64; 2],
    pub moved: bool,
    /// How far the person dragged the text from its usual place (model mm).
    pub text_shift: [f64; 2],
    pub locked: bool,
    pub anchor: u8,
    pub replaced: bool,
    /// Which way is outwards from the dimension line at the text (round dimensions); zero for a chain.
    pub out: [f64; 2],
    /// The global parameter labelling the segment.
    pub param: Option<Id>,
}

/// The part of a round dimension that is not a witness line: its arc or straight line and the ends.
#[derive(Debug, Clone, Serialize)]
pub struct RoundDraw {
    /// Centre of the circle (the apex of an angular dimension).
    pub centre: [f64; 2],
    /// Radius of the circle measured; for an angular dimension the radius of its arc.
    pub radius: f64,
    /// The dimension arc (angular, arc length): start angle and signed sweep in radians.
    pub arc: Option<[f64; 2]>,
    /// The dimension arc's radius, when it is not `radius` (arc length).
    pub arc_radius: f64,
    /// A straight dimension line (radial, diameter).
    pub line: Option<[[f64; 2]; 2]>,
    /// Where the dimension line ends and which way it points out of the line there.
    pub ends: Vec<([f64; 2], [f64; 2])>,
    pub centre_mark: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DimDraw {
    pub id: Id,
    pub kind: DimKind,
    pub type_id: Id,
    /// Why the dimension cannot be measured (an element it measured is gone, two references are no
    /// longer parallel), shown as a warning.
    pub invalid: Option<String>,
    pub u: [f64; 2],
    pub t: [f64; 2],
    pub refs: Vec<DrawRef>,
    pub segments: Vec<DrawSeg>,
    pub equality: bool,
    pub eq_display: EqDisplay,
    pub label: Option<String>,
    pub pinned: bool,
    /// Where an equality (EQ) toggle and the padlocks are drawn: the middle of the dimension line.
    pub mid: [f64; 2],
    pub round: Option<RoundDraw>,
    /// Angular: the reflex angle is measured.
    pub reflex: bool,
}

/// A line or point a new dimension can take as a reference.
#[derive(Debug, Clone, Serialize)]
pub struct DimCandidate {
    pub spec: RefSpec,
    pub label: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub dist: f64,
    /// For a curved reference: centre x, centre y, radius, start angle, sweep.
    pub arc: Option<[f64; 5]>,
}

/// What a reference has to be for the tool that is picking it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimWant {
    Line,
    Arc,
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

fn pt(p: Point) -> [f64; 2] {
    [p.x, p.y]
}

fn norm(p: Point) -> Option<Point> {
    let l = p.x.hypot(p.y);
    (l > 1e-9).then(|| Point::new(p.x / l, p.y / l))
}

/// Foot of the perpendicular from `p` on the line through `a` and `b`.
fn on_line(a: Point, b: Point, p: Point) -> Point {
    match unit(a, b) {
        Some((d, _)) => {
            let k = (p.x - a.x) * d.x + (p.y - a.y) * d.y;
            Point::new(a.x + d.x * k, a.y + d.y * k)
        }
        None => a,
    }
}

fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let l2 = vx * vx + vy * vy;
    if l2 < 1e-12 {
        return p.dist(&a);
    }
    let t = (((p.x - a.x) * vx + (p.y - a.y) * vy) / l2).clamp(0.0, 1.0);
    p.dist(&Point::new(a.x + vx * t, a.y + vy * t))
}

/// The arc a curved wall's location line follows, and whether it runs counter-clockwise.
pub(crate) fn wall_arc(w: &Wall) -> Option<(ArcGeom, bool)> {
    match w.curve.as_ref()? {
        crate::curve::Curve::Arc { start, end, centre, ccw } => {
            let radius = centre.dist(start);
            if radius < 1e-6 {
                return None;
            }
            let sweep = crate::curve::arc_sweep(*start, *end, *centre, *ccw);
            Some((ArcGeom { centre: *centre, radius, a0: (start.y - centre.y).atan2(start.x - centre.x), sweep }, *ccw))
        }
        _ => None,
    }
}

impl Document {
    pub fn dimensions(&self) -> Vec<(Id, &Dimension)> {
        self.iter().filter_map(|(id, e)| if let Element::Dimension(d) = e { Some((id, d)) } else { None }).collect()
    }

    pub fn dimension(&self, id: Id) -> Option<&Dimension> {
        match self.get(id) {
            Some(Element::Dimension(d)) => Some(d),
            _ => None,
        }
    }

    pub fn dimension_type(&self, id: Id) -> Option<&DimensionType> {
        match self.get(id) {
            Some(Element::DimensionType(t)) => Some(t),
            _ => None,
        }
    }

    pub fn dimension_types(&self) -> Vec<(Id, &DimensionType)> {
        self.iter().filter_map(|(id, e)| if let Element::DimensionType(t) = e { Some((id, t)) } else { None }).collect()
    }

    /// Dimensions in one view, in the order they were made.
    pub fn dimensions_in(&self, view: Id) -> Vec<(Id, &Dimension)> {
        self.dimensions().into_iter().filter(|(_, d)| d.view == view).collect()
    }

    /// A reference resolved to the plan: a line to measure from, or a point.
    pub fn ref_geom(&self, spec: &RefSpec) -> Option<RefGeom> {
        match spec {
            RefSpec::Wall { id, offset } => {
                let Some(Element::Wall(w)) = self.get(*id) else { return None };
                if w.curve.is_some() {
                    let (arc, ccw) = wall_arc(w)?;
                    let r = if ccw { arc.radius - offset } else { arc.radius + offset };
                    return (r > 1e-6).then_some(RefGeom::Arc(ArcGeom { radius: r, ..arc }));
                }
                let (d, _) = unit(w.start, w.end)?;
                let n = Point::new(-d.y, d.x);
                Some(RefGeom::Line(Point::new(w.start.x + n.x * offset, w.start.y + n.y * offset), Point::new(w.end.x + n.x * offset, w.end.y + n.y * offset)))
            }
            RefSpec::WallEnd { id, end } => {
                let Some(Element::Wall(w)) = self.get(*id) else { return None };
                let (hl, hr) = self.wall_extents(w);
                if w.curve.is_some() {
                    let (arc, ccw) = wall_arc(w)?;
                    let a = if *end == 0 { arc.a0 } else { arc.a0 + arc.sweep };
                    let rad = |o: f64| if ccw { arc.radius - o } else { arc.radius + o };
                    return Some(RefGeom::Line(arc.at(a, rad(-hr).max(0.0)), arc.at(a, rad(hl).max(0.0))));
                }
                let (d, len) = unit(w.start, w.end)?;
                let n = Point::new(-d.y, d.x);
                let t = if *end == 0 { 0.0 } else { len };
                let at = |o: f64| Point::new(w.start.x + d.x * t + n.x * o, w.start.y + d.y * t + n.y * o);
                Some(RefGeom::Line(at(-hr), at(hl)))
            }
            RefSpec::Opening { id, at } => {
                let Some(Element::FamilyInstance(i)) = self.get(*id) else { return None };
                let h = i.host?;
                let Some(Element::Wall(w)) = self.get(h.wall) else { return None };
                if w.curve.is_some() {
                    return None;
                }
                let (d, _) = unit(w.start, w.end)?;
                let n = Point::new(-d.y, d.x);
                let (hl, hr) = self.wall_extents(w);
                let t = h.u_mm + at;
                let p = |o: f64| Point::new(w.start.x + d.x * t + n.x * o, w.start.y + d.y * t + n.y * o);
                Some(RefGeom::Line(p(-hr - 300.0), p(hl + 300.0)))
            }
            RefSpec::Grid { id } => {
                let Some(Element::Grid(g)) = self.get(*id) else { return None };
                let (a, b) = g.line()?;
                Some(RefGeom::Line(a, b))
            }
            RefSpec::Level { .. } | RefSpec::Elev { .. } => None,
        }
    }

    /// The references of a dimension in `view` as lines: in the plan, or in an elevation or section view's own frame.
    fn ref_geoms(&self, view: Id, refs: &[DimRef]) -> Result<Vec<RefGeom>, String> {
        let elev = self.elevation_dim_lines(view);
        refs.iter()
            .map(|r| match &elev {
                Some(e) => e.geom(self, &r.spec),
                None => self.ref_geom(&r.spec),
            })
            .map(|g| g.ok_or_else(|| "a measured element is gone or cannot be dimensioned".to_string()))
            .collect()
    }

    /// Move what a reference belongs to by `v`, in the frame of the dimension's view.
    pub(crate) fn move_dim_ref(&mut self, view: Id, spec: &RefSpec, v: Point) -> Result<(), String> {
        if self.is_elevation_like(view) {
            self.move_elev_ref(view, spec, v)
        } else {
            self.move_ref_element(spec, v)
        }
    }

    /// Resolve every reference of a dimension against the model.
    pub fn measure(&self, d: &Dimension) -> Result<Measured, String> {
        if !d.kind.is_chain() {
            return Err("this kind of dimension is not measured yet".into());
        }
        let geoms = self.ref_geoms(d.view, &d.refs)?;
        if geoms.iter().any(|g| matches!(g, RefGeom::Arc(_))) {
            return Err("a curved reference cannot be measured by this kind of dimension".into());
        }
        let hint = Point::new(d.dir[0], d.dir[1]);
        let first_line = geoms.iter().find_map(|g| if let RefGeom::Line(a, b) = g { unit(*a, *b).map(|(dv, _)| dv) } else { None });
        let u = match d.kind {
            DimKind::Linear => norm(hint).unwrap_or(Point::new(1.0, 0.0)),
            _ => match first_line {
                Some(dv) => {
                    let n = Point::new(-dv.y, dv.x);
                    if dot(n, hint) < 0.0 {
                        Point::new(-n.x, -n.y)
                    } else {
                        n
                    }
                }
                None => {
                    let pts: Vec<Point> = geoms.iter().filter_map(|g| if let RefGeom::Point(p) = g { Some(*p) } else { None }).collect();
                    let v = Point::new(pts.last().map_or(0.0, |p| p.x) - pts.first().map_or(0.0, |p| p.x), pts.last().map_or(0.0, |p| p.y) - pts.first().map_or(0.0, |p| p.y));
                    let u = norm(v).or_else(|| norm(hint)).ok_or("the references coincide")?;
                    if dot(u, hint) < 0.0 {
                        Point::new(-u.x, -u.y)
                    } else {
                        u
                    }
                }
            },
        };
        if d.kind == DimKind::Aligned {
            if let Some(f) = first_line {
                for g in &geoms {
                    if let RefGeom::Line(a, b) = g {
                        if let Some((dv, _)) = unit(*a, *b) {
                            if (f.x * dv.y - f.y * dv.x).abs() > PARALLEL_TOL {
                                return Err("the references are no longer parallel".into());
                            }
                        }
                    }
                }
            }
        }
        let t = Point::new(-u.y, u.x);
        let mut anchors = vec![];
        let mut pos = vec![];
        for (g, r) in geoms.iter().zip(&d.refs) {
            let a = match g {
                RefGeom::Line(a, b) => {
                    let pick = r.pick.map_or(Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), |p| Point::new(p[0], p[1]));
                    on_line(*a, *b, pick)
                }
                RefGeom::Point(p) => *p,
                RefGeom::Arc(_) => unreachable!("arcs are refused above"),
            };
            pos.push(dot(a, u));
            anchors.push(a);
        }
        Ok(Measured { u, t, pos, anchors })
    }

    pub fn dimension_draw(&self, id: Id) -> Option<DimDraw> {
        let d = self.dimension(id)?;
        Some(self.draw_dimension(id, d))
    }

    /// Draw a dimension that may not be in the document (a preview while placing or dragging).
    pub fn draw_dimension(&self, id: Id, d: &Dimension) -> DimDraw {
        let ty = self.dimension_type(d.dim_type).cloned().unwrap_or_else(DimensionType::standard);
        let mut out = DimDraw {
            id,
            kind: d.kind,
            type_id: d.dim_type,
            invalid: None,
            u: d.dir,
            t: [-d.dir[1], d.dir[0]],
            refs: vec![],
            segments: vec![],
            equality: d.equality,
            eq_display: d.eq_display,
            label: d.label.clone(),
            pinned: d.pinned,
            mid: d.line_point,
            round: None,
            reflex: d.reflex,
        };
        if !d.kind.is_chain() {
            out.mid = d.refs.first().and_then(|r| r.pick).unwrap_or([0.0, 0.0]);
            return self.draw_round(d, &ty, out);
        }
        let m = match self.measure(d) {
            Ok(m) => m,
            Err(e) => {
                out.invalid = Some(e);
                return out;
            }
        };
        let line_at = d.line_point[0] * m.t.x + d.line_point[1] * m.t.y;
        let at = |p: f64| [m.u.x * p + m.t.x * line_at, m.u.y * p + m.t.y * line_at];
        out.u = pt(m.u);
        out.t = pt(m.t);
        out.refs = m.pos.iter().zip(&m.anchors).zip(&d.refs).map(|((p, a), r)| DrawRef { spec: r.spec.clone(), pos: *p, anchor: pt(*a), at: at(*p) }).collect();
        let (lo, hi) = m.pos.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(*p), h.max(*p)));
        out.mid = at((lo + hi) / 2.0);
        let n = d.segments.len();
        let total = (m.pos.last().copied().unwrap_or(0.0) - m.pos.first().copied().unwrap_or(0.0)).abs();
        for (i, s) in d.segments.iter().enumerate() {
            if i + 1 >= m.pos.len() {
                break;
            }
            let value = (m.pos[i + 1] - m.pos[i]).abs();
            let (a, b) = (at(m.pos[i]), at(m.pos[i + 1]));
            let label = s.param.and_then(|g| self.global_param(g));
            let mut text = if let Some(g) = label {
                // a labelled segment shows its parameter's name, and its value when the type says so
                if ty.show_label_value { format!("{} = {}", g.name, format_length(value, &ty.format)) } else { g.name.clone() }
            } else if let Some(w) = &s.value_text {
                w.clone()
            } else if d.equality && d.eq_display == EqDisplay::Eq {
                "EQ".to_string()
            } else if d.equality && d.eq_display == EqDisplay::Formula {
                if i == (n - 1) / 2 {
                    format!("{} @ {} = {}", n, format_length(total / n as f64, &ty.format), format_length(total, &ty.format))
                } else {
                    String::new()
                }
            } else {
                format_length(value, &ty.format)
            };
            let mut alt_text = None;
            if s.value_text.is_none() && !text.is_empty() && !(d.equality && d.eq_display != EqDisplay::Value) {
                if let Some(alt) = &ty.alt {
                    let a = format_length(value, &alt.format);
                    match alt.placement {
                        AltPlacement::Right => text = format!("{text} [{a}]"),
                        AltPlacement::Below => alt_text = Some(a),
                    }
                }
            }
            let mid = [(a[0] + b[0]) / 2.0 + s.text_shift[0], (a[1] + b[1]) / 2.0 + s.text_shift[1]];
            out.segments.push(DrawSeg {
                index: i,
                a,
                b,
                value_mm: value,
                text,
                alt_text,
                prefix: s.prefix.clone(),
                suffix: s.suffix.clone(),
                above: s.above.clone(),
                below: s.below.clone(),
                text_pos: mid,
                moved: s.text_shift != [0.0, 0.0],
                text_shift: s.text_shift,
                locked: s.locked,
                anchor: s.anchor,
                replaced: s.value_text.is_some(),
                out: [0.0, 0.0],
                param: s.param,
            });
        }
        out
    }

    /// Change a dimension type (Edit Type): any of its fields, `alt` null to drop the second units.
    pub fn patch_dimension_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.dimension_type(id).cloned().ok_or("not a dimension type")?;
        let new: DimensionType = crate::doc::apply_patch(&old, patch)?;
        if new == old {
            return Ok(());
        }
        self.check_dimension_type(Some(id), &new)?;
        self.transact("Change dimension type", |tx| tx.update(id, Element::DimensionType(new)))
    }

    pub fn duplicate_dimension_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let t = self.dimension_type(id).cloned().ok_or("not a dimension type")?;
        let new = DimensionType { name: name.trim().to_string(), ..t };
        self.check_dimension_type(None, &new)?;
        self.transact("Duplicate dimension type", |tx| Ok(tx.add(Element::DimensionType(new))))
    }

    fn check_dimension_type(&self, me: Option<Id>, t: &DimensionType) -> Result<(), String> {
        if t.name.trim().is_empty() {
            return Err("a dimension type needs a name".into());
        }
        if self.dimension_types().iter().any(|(i, o)| Some(*i) != me && o.name.eq_ignore_ascii_case(t.name.trim())) {
            return Err(format!("there is already a dimension type called {}", t.name.trim()));
        }
        for (what, v) in [("Text Size", t.text_size_mm), ("Tick Mark Size", t.tick_size_mm)] {
            if !(v > 0.0 && v <= 50.0) {
                return Err(format!("{what} must be more than 0 and at most 50 mm"));
            }
        }
        for (what, v) in [("Witness Line Gap", t.witness_gap_mm), ("Witness Line Extension", t.witness_extension_mm), ("Text Offset", t.text_offset_mm)] {
            if !(0.0..=50.0).contains(&v) {
                return Err(format!("{what} must be from 0 to 50 mm"));
            }
        }
        if t.format.decimals > 6 || t.angle_decimals > 6 {
            return Err("at most 6 decimal places".into());
        }
        Ok(())
    }

    /// Whether a dimension type's labelled dimensions show their value after the parameter's name.
    pub fn set_dimension_type_label_value(&mut self, tid: Id, show: bool) -> Result<(), String> {
        let mut t = self.dimension_type(tid).cloned().ok_or("not a dimension type")?;
        if t.show_label_value == show {
            return Ok(());
        }
        t.show_label_value = show;
        self.transact("Change dimension type", |tx| tx.update(tid, Element::DimensionType(t)))
    }

    pub fn add_dimension_type(&mut self, t: DimensionType) -> Result<Id, String> {
        self.transact("Add dimension type", |tx| Ok(tx.add(Element::DimensionType(t))))
    }

    fn default_dimension_type(&mut self) -> Result<Id, String> {
        if let Some((id, _)) = self.dimension_types().first() {
            return Ok(*id);
        }
        self.transact("Add dimension type", |tx| Ok(tx.add(Element::DimensionType(DimensionType::standard()))))
    }

    /// A dimension ready to be stored, checked against the model. `axis` is the direction a linear
    /// dimension measures along; an aligned one works its direction out from the references. For
    /// the round kinds `line_point` is where in the plan the dimension line was put.
    fn new_dimension(&self, view: Id, kind: DimKind, refs: Vec<DimRef>, line_point: Point, axis: Option<[f64; 2]>, tid: Id) -> Result<Dimension, String> {
        match self.view(view) {
            Some(v) if matches!(v.view_kind, ViewKind::FloorPlan | ViewKind::CeilingPlan | ViewKind::StructuralPlan | ViewKind::AreaPlan) => {}
            Some(_) if self.is_elevation_like(view) => {
                if !kind.is_chain() {
                    return Err("radial, diameter, angular and arc length dimensions are placed in plan views".into());
                }
            }
            Some(_) => return Err("dimensions are placed in plan, elevation and section views".into()),
            None => return Err("no such view".into()),
        }
        if kind.is_chain() {
            if refs.len() < kind.min_refs() {
                return Err("pick at least two references".into());
            }
        } else if refs.len() != kind.min_refs() {
            return Err(match kind {
                DimKind::Angular => "an angular dimension takes two straight references",
                DimKind::ArcLength => "an arc length dimension takes an arc and the two lines that bound it",
                _ => "a radial or diameter dimension takes one curved reference",
            }
            .into());
        }
        let mut d = Dimension {
            view,
            kind,
            dim_type: tid,
            refs,
            dir: axis.unwrap_or([0.0, 0.0]),
            line_point: pt(line_point),
            segments: vec![],
            equality: false,
            eq_display: EqDisplay::Value,
            solved: vec![],
            label: None,
            pinned: false,
            sides: [1, 1],
            reflex: false,
            centre_mark: false,
        };
        d.sync_segments();
        if !kind.is_chain() {
            self.set_round_line_point(&mut d, line_point)?;
            let probe = self.round_shape(&d)?;
            if probe.value < 1e-9 {
                return Err("there is nothing to measure".into());
            }
            return Ok(d);
        }
        // which way is positive: from the first reference towards the last
        if kind == DimKind::Aligned {
            let probe = self.measure(&d)?;
            let span = probe.pos.last().copied().unwrap_or(0.0) - probe.pos.first().copied().unwrap_or(0.0);
            d.dir = if span < 0.0 { [-probe.u.x, -probe.u.y] } else { pt(probe.u) };
        } else if norm(Point::new(d.dir[0], d.dir[1])).is_none() {
            d.dir = [1.0, 0.0];
        }
        let m = self.measure(&d)?;
        if m.pos.windows(2).any(|w| (w[1] - w[0]).abs() < 1e-6) {
            return Err("two of the references are in the same place".into());
        }
        d.solved = m.pos;
        Ok(d)
    }

    /// Place a dimension (see `new_dimension`).
    pub fn add_dimension(&mut self, view: Id, kind: DimKind, refs: Vec<DimRef>, line_point: Point, axis: Option<[f64; 2]>, type_id: Option<Id>) -> Result<Id, String> {
        let tid = match type_id {
            Some(t) if self.dimension_type(t).is_some() => t,
            Some(_) => return Err("no such dimension type".into()),
            None => self.default_dimension_type()?,
        };
        let d = self.new_dimension(view, kind, refs, line_point, axis, tid)?;
        self.transact("Dimension", |tx| Ok(tx.add(Element::Dimension(d))))
    }

    /// Change a dimension: `equality`, `eq_display`, `line_point`, `dim_type`, `label`, `pinned`, and
    /// `segments`, a list of `{ index, locked, prefix, suffix, above, below, value_text, text_shift, anchor }`.
    pub fn patch_dimension(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::Dimension(old)) = self.get(id).cloned() else { return Err("not a dimension".into()) };
        let mut d = old.clone();
        if let Some(v) = patch.get("equality").and_then(|v| v.as_bool()) {
            if v && d.refs.len() < 3 {
                return Err("equality needs a dimension with three or more references".into());
            }
            d.equality = v && d.kind.is_chain();
        }
        if let Some(v) = patch.get("eq_display") {
            d.eq_display = serde_json::from_value(v.clone()).map_err(|_| "unknown equality display".to_string())?;
        }
        if let Some(v) = patch.get("line_point") {
            let p: [f64; 2] = serde_json::from_value(v.clone()).map_err(|_| "bad line point".to_string())?;
            if d.kind.is_chain() {
                d.line_point = p;
            } else {
                self.set_round_line_point(&mut d, Point::new(p[0], p[1]))?;
            }
        }
        if let Some(v) = patch.get("reflex").and_then(|v| v.as_bool()) {
            if d.kind != DimKind::Angular {
                return Err("only an angular dimension has a reflex angle".into());
            }
            d.reflex = v;
        }
        if let Some(v) = patch.get("centre_mark").and_then(|v| v.as_bool()) {
            if !matches!(d.kind, DimKind::Radial | DimKind::Diameter) {
                return Err("only radial and diameter dimensions have a centre mark".into());
            }
            d.centre_mark = v;
        }
        if let Some(v) = patch.get("dim_type").and_then(|v| v.as_u64()) {
            if self.dimension_type(v).is_none() {
                return Err("no such dimension type".into());
            }
            d.dim_type = v;
        }
        if let Some(v) = patch.get("label") {
            d.label = v.as_str().filter(|s| !s.is_empty()).map(String::from);
        }
        if let Some(v) = patch.get("pinned").and_then(|v| v.as_bool()) {
            d.pinned = v;
        }
        if let Some(list) = patch.get("segments").and_then(|v| v.as_array()) {
            for s in list {
                let i = s.get("index").and_then(|v| v.as_u64()).ok_or("a segment needs an index")? as usize;
                let seg = d.segments.get_mut(i).ok_or("no such segment")?;
                if let Some(v) = s.get("locked").and_then(|v| v.as_bool()) {
                    seg.locked = v;
                }
                for (key, slot) in [("prefix", &mut seg.prefix), ("suffix", &mut seg.suffix), ("above", &mut seg.above), ("below", &mut seg.below)] {
                    if let Some(v) = s.get(key).and_then(|v| v.as_str()) {
                        *slot = v.to_string();
                    }
                }
                if let Some(v) = s.get("value_text") {
                    match v.as_str() {
                        Some(t) if !t.trim().is_empty() => {
                            if !value_text_ok(t) {
                                return Err("a measured value cannot be replaced by a number; move the elements to change it".into());
                            }
                            seg.value_text = Some(t.to_string());
                        }
                        _ => seg.value_text = None,
                    }
                }
                if let Some(v) = s.get("text_shift") {
                    seg.text_shift = serde_json::from_value(v.clone()).map_err(|_| "bad text shift".to_string())?;
                }
                if let Some(v) = s.get("anchor").and_then(|v| v.as_u64()) {
                    seg.anchor = (v as u8).min(1);
                }
            }
        }
        if d == old {
            return Ok(());
        }
        // a lock or equality starts from how the elements stand now (a round dimension holds its one value)
        if !d.kind.is_chain() {
            if d.constrained() {
                d.solved = vec![self.round_value(&d)?];
            }
        } else if d.constrained() && !old.constrained() || (d.constrained() && d.solved.len() != d.refs.len()) {
            if let Ok(m) = self.measure(&d) {
                d.solved = m.pos;
            }
        }
        // switching Equality on evens the spacing now, as part of the same undo step; if the elements cannot
        // move (pinned, or tied by other constraints) the whole change is refused
        if d.equality && !old.equality {
            // the solver moves what differs from `solved`: record the evenly spaced positions it should reach
            let n = d.solved.len();
            if n >= 3 {
                let (e0, en) = (d.solved[0], d.solved[n - 1]);
                for i in 1..n - 1 {
                    d.solved[i] = e0 + (en - e0) * i as f64 / (n - 1) as f64;
                }
            }
            self.transact_solving("Change dimension", |tx| tx.update(id, Element::Dimension(d)))
        } else {
            self.transact("Change dimension", |tx| tx.update(id, Element::Dimension(d)))
        }
    }

    /// Add a witness line to a dimension (`insert`: the position and the reference) or take one away.
    pub fn edit_dimension_refs(&mut self, id: Id, insert: Option<(usize, DimRef)>, remove: Option<usize>) -> Result<(), String> {
        let Some(Element::Dimension(mut d)) = self.get(id).cloned() else { return Err("not a dimension".into()) };
        if !d.kind.is_chain() {
            return Err("only aligned and linear dimensions have references to add or remove".into());
        }
        if let Some(k) = remove {
            if d.refs.len() <= 2 {
                return Err("a dimension needs two references; delete it instead".into());
            }
            d.remove_ref(k);
        }
        if let Some((k, r)) = insert {
            if d.refs.iter().any(|x| x.spec == r.spec) {
                return Err("that line is already measured".into());
            }
            d.insert_ref(k, r);
        }
        let m = self.measure(&d)?;
        if d.constrained() {
            d.solved = m.pos;
        } else {
            d.solved.clear();
        }
        self.transact("Change dimension", |tx| tx.update(id, Element::Dimension(d)))
    }

    /// Type a value into a segment: the end that is not the anchor moves (with the element it
    /// belongs to and whatever is joined or hosted on it) until the segment measures `value`.
    pub fn set_dimension_value(&mut self, id: Id, segment: usize, value: f64) -> Result<(), String> {
        // typing into a segment a driving parameter labels changes the parameter, and with it every dimension it labels
        let held = self.dimension(id).and_then(|d| d.segments.get(segment)).and_then(|s| s.param).and_then(|g| self.global_param(g).map(|x| (g, x.reporting, x.name.clone())));
        if let Some((g, reporting, name)) = held {
            if !reporting {
                if !value.is_finite() || value <= 0.0 {
                    return Err("a dimension must be greater than zero".into());
                }
                return self.patch_global_param(g, &serde_json::json!({ "value": value })).map_err(|e| format!("{name}: {e}"));
            }
        }
        self.set_dimension_value_raw(id, segment, value)
    }

    /// Move the geometry so a segment measures `value`, lifting the segment's own lock or label while it moves.
    pub(crate) fn set_dimension_value_raw(&mut self, id: Id, segment: usize, value: f64) -> Result<(), String> {
        if !value.is_finite() || value <= 0.0 {
            return Err("a dimension must be greater than zero".into());
        }
        let Some(Element::Dimension(d)) = self.get(id).cloned() else { return Err("not a dimension".into()) };
        if !d.kind.is_chain() {
            return self.set_round_value_raw(id, value);
        }
        let seg = d.segments.get(segment).ok_or("no such segment")?.clone();
        if seg.value_text.is_some() {
            return Err("this dimension shows words in place of its value; clear them first".into());
        }
        let m = self.measure(&d)?;
        let (i, j) = (segment, segment + 1);
        let cur = m.pos[j] - m.pos[i];
        let sign = if cur < 0.0 { -1.0 } else { 1.0 };
        let v = cur.abs();
        if (value - v).abs() < TOL {
            return Ok(());
        }
        let (mover, shift) = if seg.anchor == 0 { (j, sign * (value - v)) } else { (i, -sign * (value - v)) };
        let spec = d.refs[mover].spec.clone();
        // the elements it moves must not be pinned; hosted openings follow a moved wall by themselves
        let vec = Point::new(m.u.x * shift, m.u.y * shift);
        self.grouped("Change dimension", |doc| {
            // the typed value replaces the old distance, so the segment's own lock is lifted while it moves
            let mut unlocked = d.clone();
            unlocked.segments[segment].locked = false;
            unlocked.segments[segment].param = None;
            let was_locked = d.segments[segment].locked || d.segments[segment].param.is_some();
            if was_locked {
                doc.transact("Change dimension", |tx| tx.update(id, Element::Dimension(unlocked.clone())))?;
            }
            doc.move_dim_ref(d.view, &spec, vec)?;
            if was_locked {
                let Some(Element::Dimension(mut now)) = doc.get(id).cloned() else { return Ok(()) };
                now.segments[segment].locked = d.segments[segment].locked;
                now.segments[segment].param = d.segments[segment].param;
                if let Ok(m2) = doc.measure(&now) {
                    now.solved = m2.pos;
                }
                doc.transact("Change dimension", |tx| tx.update(id, Element::Dimension(now)))?;
            }
            Ok(())
        })
    }

    /// Translate the element a reference belongs to by `v`. Walls, openings and grids slide; the end
    /// of a wall moves that end.
    pub(crate) fn move_ref_element(&mut self, spec: &RefSpec, v: Point) -> Result<(), String> {
        if v.x.hypot(v.y) < 1e-9 {
            return Ok(());
        }
        match spec {
            RefSpec::Wall { id, .. } | RefSpec::Opening { id, .. } | RefSpec::Grid { id } => {
                self.transform_elements(&[*id], Xform::Move { dx: v.x, dy: v.y }, EditOptions::default()).map(|_| ())
            }
            RefSpec::WallEnd { id, end } => {
                let Some(Element::Wall(w)) = self.get(*id).cloned() else { return Err("not a wall".into()) };
                self.refuse_pinned(&[*id], "moved")?;
                let (s, e) = if *end == 0 { (Point::new(w.start.x + v.x, w.start.y + v.y), w.end) } else { (w.start, Point::new(w.end.x + v.x, w.end.y + v.y)) };
                let mut moves = BTreeMap::new();
                moves.insert(*id, (s, e));
                let plan = self.plan_wall_ends(&moves, true)?;
                let list: Vec<(Id, Wall)> = plan.into_iter().collect();
                self.apply_wall_ends(&list, "Move wall end")
            }
            RefSpec::Level { .. } | RefSpec::Elev { .. } => Err("that line is moved in an elevation or section view".into()),
        }
    }

    // -----------------------------------------------------------------------------------------
    // constraints

    pub(crate) fn has_dimension_constraints(&self) -> bool {
        self.iter().any(|(_, e)| matches!(e, Element::Dimension(d) if d.constrained()))
    }

    fn store_solved(&mut self, id: Id, d: &Dimension, pos: &[f64]) -> Result<(), String> {
        let mut nd = d.clone();
        nd.solved = pos.to_vec();
        self.transact("Update dimension", |tx| tx.update(id, Element::Dimension(nd)))
    }

    /// Make locked segments and equal chains hold after an edit. Moves one element at a time and
    /// looks again, so elements that carry others (a wall with its doors) are not moved twice.
    pub(crate) fn solve_dimension_constraints(&mut self) -> Result<(), String> {
        for _ in 0..60 {
            let ids: Vec<Id> = self.iter().filter_map(|(i, e)| if let Element::Dimension(d) = e { d.constrained().then_some(i) } else { None }).collect();
            let mut moved_something = false;
            for did in ids {
                let Some(Element::Dimension(d)) = self.get(did).cloned() else { continue };
                let Ok(m) = self.measure(&d) else { continue };
                let n = m.pos.len();
                if d.solved.len() != n {
                    self.store_solved(did, &d, &m.pos)?;
                    continue;
                }
                let changed: Vec<bool> = (0..n).map(|i| (m.pos[i] - d.solved[i]).abs() > TOL).collect();
                if !changed.iter().any(|c| *c) {
                    continue;
                }
                // where each reference has to be: the ones that moved stay where they went, the rest follow
                let mut tgt: Vec<Option<f64>> = (0..n).map(|i| changed[i].then_some(m.pos[i])).collect();
                if d.equality && n >= 3 {
                    let (e0, en) = (tgt[0].unwrap_or(m.pos[0]), tgt[n - 1].unwrap_or(m.pos[n - 1]));
                    for i in 1..n - 1 {
                        tgt[i] = Some(e0 + (en - e0) * i as f64 / (n - 1) as f64);
                    }
                }
                let conflict = || "the dimension constraints cannot all be met (the locks or equal dimensions disagree)".to_string();
                loop {
                    let mut grew = false;
                    for (i, seg) in d.segments.iter().enumerate() {
                        // a driving parameter's label holds its length as a lock does
                        let held = seg.locked || seg.param.is_some_and(|g| self.global_param(g).is_some_and(|x| !x.reporting));
                        if !held || i + 1 >= n {
                            continue;
                        }
                        let s = d.solved[i + 1] - d.solved[i];
                        match (tgt[i], tgt[i + 1]) {
                            (Some(a), None) => {
                                tgt[i + 1] = Some(a + s);
                                grew = true;
                            }
                            (None, Some(b)) => {
                                tgt[i] = Some(b - s);
                                grew = true;
                            }
                            (Some(a), Some(b)) if (b - a - s).abs() > TOL => return Err(conflict()),
                            _ => {}
                        }
                    }
                    if !grew {
                        break;
                    }
                }
                // one shift per element; two references of one element cannot ask for different ones
                let mut shifts: Vec<(RefSpec, f64)> = vec![];
                for i in 0..n {
                    let Some(t) = tgt[i] else { continue };
                    let delta = t - m.pos[i];
                    let el = d.refs[i].spec.element();
                    match shifts.iter().find(|(s, _)| s.element() == el) {
                        Some((_, prev)) if (prev - delta).abs() > TOL => return Err(conflict()),
                        Some(_) => {}
                        None => shifts.push((d.refs[i].spec.clone(), delta)),
                    }
                }
                match shifts.iter().find(|(_, delta)| delta.abs() > TOL) {
                    None => {
                        self.store_solved(did, &d, &m.pos)?;
                    }
                    Some((spec, delta)) => {
                        let v = Point::new(m.u.x * delta, m.u.y * delta);
                        self.move_dim_ref(d.view, spec, v).map_err(|e| format!("a locked or equal dimension needs to move another element, but {e}"))?;
                        moved_something = true;
                        break;
                    }
                }
            }
            if !moved_something {
                // every constrained dimension agrees with the model
                let ids: Vec<Id> = self.iter().filter_map(|(i, e)| if let Element::Dimension(d) = e { d.constrained().then_some(i) } else { None }).collect();
                let settled = ids.iter().all(|did| {
                    let Some(Element::Dimension(d)) = self.get(*did) else { return true };
                    match self.measure(d) {
                        Ok(m) => d.solved.len() == m.pos.len() && m.pos.iter().zip(&d.solved).all(|(a, b)| (a - b).abs() <= TOL),
                        Err(_) => true,
                    }
                });
                if settled {
                    return Ok(());
                }
            }
        }
        Err("the dimension constraints cannot all be met".into())
    }

    // -----------------------------------------------------------------------------------------
    // deleting what a dimension measures

    /// The dimensions that measure `gone`, as they should be once it is deleted: the reference is
    /// dropped and the chain closes up around it, or the dimension goes when too little is left.
    pub(crate) fn healed_dimensions(&self, gone: Id) -> Vec<(Id, Option<Dimension>)> {
        let mut out = vec![];
        'dims: for (id, e) in self.iter() {
            let Element::Dimension(d) = e else { continue };
            if !d.refs.iter().any(|r| r.spec.element() == gone) {
                continue;
            }
            let mut nd = d.clone();
            while let Some(k) = nd.refs.iter().position(|r| r.spec.element() == gone) {
                if !nd.kind.is_chain() || nd.refs.len() <= nd.kind.min_refs() {
                    out.push((id, None));
                    continue 'dims;
                }
                nd.remove_ref(k);
            }
            out.push((id, Some(nd)));
        }
        out
    }

    // -----------------------------------------------------------------------------------------
    // picking references

    /// Lines near `point` in a plan view that a dimension can measure to, best first. The setting
    /// decides which line of a wall or opening comes first (faces, centreline, core ...).
    pub fn dim_candidates(&self, view: Id, point: Point, tol: f64, s: TempDimSettings) -> Vec<DimCandidate> {
        self.dim_candidates_for(view, point, tol, s, DimWant::Line)
    }

    /// The same, for a tool that needs straight lines (`DimWant::Line`) or curved ones (`DimWant::Arc`).
    pub fn dim_candidates_for(&self, view: Id, point: Point, tol: f64, s: TempDimSettings, want: DimWant) -> Vec<DimCandidate> {
        if self.is_elevation_like(view) {
            return if want == DimWant::Line { self.elevation_dim_candidates(view, point, tol) } else { vec![] };
        }
        let Some(v) = self.view(view) else { return vec![] };
        let mut out: Vec<DimCandidate> = vec![];
        let want_open = match s.opening_ref {
            OpeningRef::Centreline => "Centre",
            OpeningRef::Opening => "Opening edge",
        };
        let push = |spec: RefSpec, label: String, a: Point, b: Point, preferred: bool, out: &mut Vec<DimCandidate>| {
            let dist = dist_to_segment(point, a, b);
            if dist <= tol {
                // preferred lines sort ahead of the others at any distance within the pick tolerance
                out.push(DimCandidate { spec, label, a: pt(a), b: pt(b), dist: if preferred { dist } else { dist + 1.0e6 }, arc: None });
            }
        };
        for (id, e) in self.iter() {
            match e {
                Element::Wall(w) if w.curve.is_some() && v.level.map_or(true, |l| l == w.base_level) && wall_arc(w).is_some() => {
                    let wanted = self.wall_ref_offsets_pub(w, s.wall_ref);
                    if want == DimWant::Arc {
                        for (off, label) in self.wall_boundaries(w) {
                            let spec = RefSpec::Wall { id, offset: off };
                            if let Some(RefGeom::Arc(g)) = self.ref_geom(&spec) {
                                let ang = angle_of(g.centre, point);
                                let dist = if g.spans(ang) {
                                    (point.dist(&g.centre) - g.radius).abs()
                                } else {
                                    point.dist(&g.at(g.a0, g.radius)).min(point.dist(&g.at(g.a0 + g.sweep, g.radius)))
                                };
                                if dist <= tol {
                                    let preferred = wanted.iter().any(|o| (o - off).abs() < 0.01);
                                    out.push(DimCandidate { spec, label, a: pt(g.at(g.a0, g.radius)), b: pt(g.at(g.a0 + g.sweep, g.radius)), dist: if preferred { dist } else { dist + 1.0e6 }, arc: Some([g.centre.x, g.centre.y, g.radius, g.a0, g.sweep]) });
                                }
                            }
                        }
                    } else {
                        for end in [0u8, 1u8] {
                            let spec = RefSpec::WallEnd { id, end };
                            if let Some(RefGeom::Line(a, b)) = self.ref_geom(&spec) {
                                push(spec, "Wall end".into(), a, b, false, &mut out);
                            }
                        }
                    }
                }
                Element::Wall(w) if w.curve.is_none() && want == DimWant::Line && v.level.map_or(true, |l| l == w.base_level) => {
                    let wanted = self.wall_ref_offsets_pub(w, s.wall_ref);
                    for (off, label) in self.wall_boundaries(w) {
                        let spec = RefSpec::Wall { id, offset: off };
                        if let Some(RefGeom::Line(a, b)) = self.ref_geom(&spec) {
                            push(spec, label.clone(), a, b, wanted.iter().any(|o| (o - off).abs() < 0.01), &mut out);
                        }
                    }
                    for end in [0u8, 1u8] {
                        let spec = RefSpec::WallEnd { id, end };
                        if let Some(RefGeom::Line(a, b)) = self.ref_geom(&spec) {
                            push(spec, "Wall end".into(), a, b, false, &mut out);
                        }
                    }
                }
                Element::FamilyInstance(i) if i.host.is_some() && want == DimWant::Line => {
                    let host_ok = i.host.and_then(|h| self.get(h.wall)).is_some_and(|h| matches!(h, Element::Wall(w) if v.level.map_or(true, |l| l == w.base_level)));
                    if !host_ok {
                        continue;
                    }
                    for (off, label) in self.opening_offsets(id) {
                        let spec = RefSpec::Opening { id, at: off };
                        if let Some(RefGeom::Line(a, b)) = self.ref_geom(&spec) {
                            push(spec, label.clone(), a, b, label == want_open, &mut out);
                        }
                    }
                }
                Element::Grid(_) if want == DimWant::Line => {
                    let spec = RefSpec::Grid { id };
                    if let Some(RefGeom::Line(a, b)) = self.ref_geom(&spec) {
                        push(spec, "Grid".into(), a, b, true, &mut out);
                    }
                }
                _ => {}
            }
        }
        out.sort_by(|a, b| a.dist.total_cmp(&b.dist));
        for c in &mut out {
            if c.dist >= 1.0e6 {
                c.dist -= 1.0e6;
            }
        }
        out.truncate(16);
        out
    }

    /// Turn one of the temporary dimensions of the selected element into a permanent one in `view`,
    /// drawn `offset_mm` away from the element where the temporary one stood off it.
    pub fn promote_temp_dimension(&mut self, view: Id, element: Id, key: &str, s: TempDimSettings, refs: &BTreeMap<String, Id>, pins: &crate::tempdim::Pins, offset_mm: f64) -> Result<Id, String> {
        let dims = self.temp_dimensions_pinned(element, s, refs, pins);
        let t = dims.iter().find(|d| d.key == key).ok_or("that dimension is not showing")?.clone();
        let lines = self.dim_lines(element).ok_or("that element has no dimensions")?;
        let (a, b) = (Point::new(t.a[0], t.a[1]), Point::new(t.b[0], t.b[1]));
        let along = |p: Point| (p.x - lines.origin[0]) * lines.axis[0] + (p.y - lines.origin[1]) * lines.axis[1];
        let (refs_two, line_point, axis): (Vec<DimRef>, Point, Option<[f64; 2]>) = if key == "length" {
            let wall = element;
            let (specs, _) = ([RefSpec::WallEnd { id: wall, end: 0 }, RefSpec::WallEnd { id: wall, end: 1 }], ());
            let n = Point::new(t.normal[0], t.normal[1]);
            let mid = Point::new((a.x + b.x) / 2.0 + n.x * offset_mm, (a.y + b.y) / 2.0 + n.y * offset_mm);
            (vec![DimRef { spec: specs[0].clone(), pick: Some(pt(a)) }, DimRef { spec: specs[1].clone(), pick: Some(pt(b)) }], mid, None)
        } else {
            let own = lines.own.iter().filter(|l| (l.pos - along(a)).abs() < 0.01).min_by(|x, y| x.pos.total_cmp(&y.pos)).ok_or("cannot tell which line the dimension starts from")?;
            let target = lines
                .targets
                .iter()
                .filter(|l| (l.pos - along(b)).abs() < 0.01 && t.to.map_or(true, |e| l.spec.element() == e))
                .min_by(|x, y| x.pos.total_cmp(&y.pos))
                .ok_or("cannot tell which line the dimension measures to")?;
            let n = Point::new(t.normal[0], t.normal[1]);
            let off = if key == "up" || key == "down" { 0.0 } else { offset_mm };
            let mid = Point::new((a.x + b.x) / 2.0 + n.x * off, (a.y + b.y) / 2.0 + n.y * off);
            (vec![DimRef { spec: own.spec.clone(), pick: Some(pt(a)) }, DimRef { spec: target.spec.clone(), pick: Some(pt(b)) }], mid, None)
        };
        self.add_dimension(view, DimKind::Aligned, refs_two, line_point, axis, None)
    }
}

// ---------------------------------------------------------------------------------------------
// round dimensions: angular, radial, diameter, arc length

fn cross2(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

fn angle_of(c: Point, p: Point) -> f64 {
    (p.y - c.y).atan2(p.x - c.x)
}

fn dir_at(a: f64) -> Point {
    Point::new(a.cos(), a.sin())
}

/// The shortest signed turn from angle `a` to angle `b`, in (-pi, pi].
fn turn(a: f64, b: f64) -> f64 {
    let t = (b - a).rem_euclid(std::f64::consts::TAU);
    if t > std::f64::consts::PI {
        t - std::f64::consts::TAU
    } else {
        t
    }
}

fn sgn(v: f64) -> f64 {
    if v < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// What a round dimension measures, worked out from the model as it stands.
pub(crate) struct RoundShape {
    pub(crate) centre: Point,
    /// Radius of the circle measured (for an angular dimension, of its arc).
    radius: f64,
    /// Millimetres, or degrees for an angular dimension.
    pub(crate) value: f64,
    angular: bool,
    /// The dimension arc: radius, start angle, signed sweep.
    pub(crate) arc: Option<(f64, f64, f64)>,
    line: Option<(Point, Point)>,
    ends: Vec<(Point, Point)>,
    /// One per reference: where its witness line starts and where it meets the dimension.
    witness: Vec<(Point, Point)>,
    /// Where the text sits before any dragging, the direction away from the dimension line there and the line's direction.
    text_at: Point,
    out: Point,
    tangent: Point,
    sym: &'static str,
}

impl Document {
    fn arc_of(&self, r: &DimRef) -> Result<ArcGeom, String> {
        match self.ref_geom(&r.spec) {
            Some(RefGeom::Arc(a)) => Ok(a),
            Some(_) => Err("this dimension measures a curved reference, and the line it was given is straight".into()),
            None => Err("a measured element is gone or cannot be dimensioned".into()),
        }
    }

    fn line_of(&self, r: &DimRef) -> Result<(Point, Point), String> {
        match self.ref_geom(&r.spec) {
            Some(RefGeom::Line(a, b)) => Ok((a, b)),
            Some(_) => Err("this dimension measures straight lines, and a curved reference was given".into()),
            None => Err("a measured element is gone or cannot be dimensioned".into()),
        }
    }

    /// The point the references of an angular dimension meet at.
    fn apex_of(&self, d: &Dimension) -> Result<(Point, [(Point, Point); 2]), String> {
        let l0 = self.line_of(&d.refs[0])?;
        let l1 = self.line_of(&d.refs[1])?;
        let (d0, _) = unit(l0.0, l0.1).ok_or("a reference has no length")?;
        let (d1, _) = unit(l1.0, l1.1).ok_or("a reference has no length")?;
        let cr = cross2(d0, d1);
        if cr.abs() < PARALLEL_TOL {
            return Err("the references are parallel, so there is no angle between them".into());
        }
        let s = cross2(Point::new(l1.0.x - l0.0.x, l1.0.y - l0.0.y), d1) / cr;
        Ok((Point::new(l0.0.x + d0.x * s, l0.0.y + d0.y * s), [l0, l1]))
    }

    /// The centre a round dimension's `line_point` is measured from.
    pub fn round_centre(&self, d: &Dimension) -> Result<Point, String> {
        match d.kind {
            DimKind::Angular => Ok(self.apex_of(d)?.0),
            DimKind::Radial | DimKind::Diameter | DimKind::ArcLength => Ok(self.arc_of(d.refs.first().ok_or("no reference")?)?.centre),
            _ => Err("not a round dimension".into()),
        }
    }

    /// Where the dimension line of a round dimension was put, given in the plan: stored as an offset
    /// from the centre; an angular dimension also takes the quadrant of the point to choose its angle.
    fn set_round_line_point(&self, d: &mut Dimension, at: Point) -> Result<(), String> {
        let c = self.round_centre(d)?;
        let v = Point::new(at.x - c.x, at.y - c.y);
        if d.kind == DimKind::Angular {
            let (_, [l0, l1]) = self.apex_of(d)?;
            let (d0, _) = unit(l0.0, l0.1).ok_or("a reference has no length")?;
            let (d1, _) = unit(l1.0, l1.1).ok_or("a reference has no length")?;
            if v.x.hypot(v.y) > 1e-9 {
                // the arm along each line is the one on the same side of the other line as the cursor
                d.sides = [(sgn(cross2(d1, v)) * sgn(cross2(d1, d0))) as i8, (sgn(cross2(d0, v)) * sgn(cross2(d0, d1))) as i8];
            }
        }
        d.line_point = pt(v);
        Ok(())
    }

    pub(crate) fn round_shape(&self, d: &Dimension) -> Result<RoundShape, String> {
        let lp = Point::new(d.line_point[0], d.line_point[1]);
        let len = lp.x.hypot(lp.y);
        let tau = std::f64::consts::TAU;
        match d.kind {
            DimKind::Radial | DimKind::Diameter => {
                let arc = self.arc_of(&d.refs[0])?;
                let c = arc.centre;
                let dir = if len > 1e-6 { Point::new(lp.x / len, lp.y / len) } else { Point::new(1.0, 0.0) };
                let ext = len.max(arc.radius);
                let p = Point::new(c.x + dir.x * arc.radius, c.y + dir.y * arc.radius);
                let far = Point::new(c.x + dir.x * ext, c.y + dir.y * ext);
                let text_at = Point::new(c.x + dir.x * len.max(1.0), c.y + dir.y * len.max(1.0));
                if d.kind == DimKind::Radial {
                    Ok(RoundShape { centre: c, radius: arc.radius, value: arc.radius, angular: false, arc: None, line: Some((c, far)), ends: vec![(p, dir)], witness: vec![(p, p)], text_at, out: Point::new(0.0, 0.0), tangent: dir, sym: "radius" })
                } else {
                    let q = Point::new(c.x - dir.x * arc.radius, c.y - dir.y * arc.radius);
                    Ok(RoundShape { centre: c, radius: arc.radius, value: arc.radius * 2.0, angular: false, arc: None, line: Some((q, far)), ends: vec![(q, Point::new(-dir.x, -dir.y)), (p, dir)], witness: vec![(p, p)], text_at, out: Point::new(0.0, 0.0), tangent: dir, sym: "diameter" })
                }
            }
            DimKind::Angular => {
                let (apex, [l0, l1]) = self.apex_of(d)?;
                let (d0, _) = unit(l0.0, l0.1).ok_or("a reference has no length")?;
                let (d1, _) = unit(l1.0, l1.1).ok_or("a reference has no length")?;
                let rays = [Point::new(d0.x * d.sides[0] as f64, d0.y * d.sides[0] as f64), Point::new(d1.x * d.sides[1] as f64, d1.y * d.sides[1] as f64)];
                let delta = cross2(rays[0], rays[1]).atan2(dot(rays[0], rays[1]));
                let sweep = if d.reflex { delta - sgn(delta) * tau } else { delta };
                let a0 = rays[0].y.atan2(rays[0].x);
                let r = len.max(10.0);
                let mut witness = vec![];
                for (i, l) in [l0, l1].iter().enumerate() {
                    let pick = d.refs[i].pick.map_or(Point::new((l.0.x + l.1.x) / 2.0, (l.0.y + l.1.y) / 2.0), |p| Point::new(p[0], p[1]));
                    let foot = on_line(l.0, l.1, pick);
                    let along = dot(Point::new(foot.x - apex.x, foot.y - apex.y), rays[i]).max(0.0);
                    witness.push((Point::new(apex.x + rays[i].x * along, apex.y + rays[i].y * along), Point::new(apex.x + rays[i].x * r, apex.y + rays[i].y * r)));
                }
                let (a1, s) = (a0 + sweep, sgn(sweep));
                let tang = |a: f64| Point::new(-a.sin(), a.cos());
                let ends = vec![
                    (dir_at_pt(apex, a0, r), Point::new(-s * tang(a0).x, -s * tang(a0).y)),
                    (dir_at_pt(apex, a1, r), Point::new(s * tang(a1).x, s * tang(a1).y)),
                ];
                let am = a0 + sweep / 2.0;
                Ok(RoundShape { centre: apex, radius: r, value: sweep.abs().to_degrees(), angular: true, arc: Some((r, a0, sweep)), line: None, ends, witness, text_at: dir_at_pt(apex, am, r), out: dir_at(am), tangent: tang(am), sym: "" })
            }
            DimKind::ArcLength => {
                let arc = self.arc_of(&d.refs[0])?;
                let c = arc.centre;
                let mut angles = [0.0; 2];
                for k in 0..2 {
                    let r = &d.refs[k + 1];
                    let (a, b) = self.line_of(r)?;
                    let (dv, _) = unit(a, b).ok_or("a reference has no length")?;
                    let pick = r.pick.map_or(Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), |p| Point::new(p[0], p[1]));
                    let f = Point::new(a.x - c.x, a.y - c.y);
                    let (bq, cq) = (dot(f, dv), dot(f, f) - arc.radius * arc.radius);
                    let disc = bq * bq - cq;
                    angles[k] = if disc >= 0.0 {
                        let mut hits: Vec<Point> = [-bq - disc.sqrt(), -bq + disc.sqrt()].iter().map(|t| Point::new(a.x + dv.x * t, a.y + dv.y * t)).collect();
                        // a hit on the arc itself first, then the one nearer where it was picked
                        hits.sort_by(|p, q| {
                            let kp = (!arc.spans(angle_of(c, *p)), p.dist(&pick));
                            let kq = (!arc.spans(angle_of(c, *q)), q.dist(&pick));
                            kp.partial_cmp(&kq).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        angle_of(c, hits[0])
                    } else {
                        angle_of(c, on_line(a, b, c))
                    };
                }
                let short = turn(angles[0], angles[1]);
                if short.abs() < 1e-9 {
                    return Err("the two boundaries are in the same place".into());
                }
                let long = short - sgn(short) * tau;
                let want = if len > 1e-6 { angle_of(Point::new(0.0, 0.0), lp) } else { f64::NAN };
                let mid = |sw: f64| angles[0] + sw / 2.0;
                let sweep = if want.is_nan() {
                    if arc.spans(mid(short)) || !arc.spans(mid(long)) { short } else { long }
                } else if turn(want, mid(short)).abs() <= turn(want, mid(long)).abs() {
                    short
                } else {
                    long
                };
                let rd = if len > 1.0 { len } else { arc.radius };
                let a0 = angles[0];
                let a1 = a0 + sweep;
                let s = sgn(sweep);
                let tang = |a: f64| Point::new(-a.sin(), a.cos());
                let am = a0 + sweep / 2.0;
                let witness = vec![
                    (dir_at_pt(c, am, arc.radius), dir_at_pt(c, am, arc.radius)),
                    (dir_at_pt(c, a0, arc.radius), dir_at_pt(c, a0, rd)),
                    (dir_at_pt(c, a1, arc.radius), dir_at_pt(c, a1, rd)),
                ];
                let ends = vec![
                    (dir_at_pt(c, a0, rd), Point::new(-s * tang(a0).x, -s * tang(a0).y)),
                    (dir_at_pt(c, a1, rd), Point::new(s * tang(a1).x, s * tang(a1).y)),
                ];
                Ok(RoundShape { centre: c, radius: arc.radius, value: arc.radius * sweep.abs(), angular: false, arc: Some((rd, a0, sweep)), line: None, ends, witness, text_at: dir_at_pt(c, am, rd), out: dir_at(am), tangent: tang(am), sym: "arc" })
            }
            _ => Err("not a round dimension".into()),
        }
    }

    fn draw_round(&self, d: &Dimension, ty: &DimensionType, mut out: DimDraw) -> DimDraw {
        let sh = match self.round_shape(d) {
            Ok(s) => s,
            Err(e) => {
                out.invalid = Some(e);
                return out;
            }
        };
        let seg = d.segments.first().cloned().unwrap_or_default();
        let number = if sh.angular {
            format!("{}\u{00B0}", num(sh.value, ty.angle_decimals, ty.format.suppress_trailing_zeros))
        } else {
            let symbol = match sh.sym {
                "radius" => ty.symbols.radius.as_str(),
                "diameter" => ty.symbols.diameter.as_str(),
                _ => ty.symbols.arc_length.as_str(),
            };
            format!("{}{}", symbol, format_length(sh.value, &ty.format))
        };
        let label = seg.param.and_then(|g| self.global_param(g));
        let mut text = match label {
            // a labelled radius, diameter or angle shows its parameter's name, and its value when the type says so
            Some(g) if ty.show_label_value => format!("{} = {number}", g.name),
            Some(g) => g.name.clone(),
            None => seg.value_text.clone().unwrap_or(number),
        };
        let mut alt_text = None;
        if seg.value_text.is_none() && label.is_none() && !sh.angular {
            if let Some(alt) = &ty.alt {
                let a = format_length(sh.value, &alt.format);
                match alt.placement {
                    AltPlacement::Right => text = format!("{text} [{a}]"),
                    AltPlacement::Below => alt_text = Some(a),
                }
            }
        }
        out.u = pt(sh.tangent);
        out.t = [-sh.tangent.y, sh.tangent.x];
        out.mid = pt(sh.text_at);
        out.refs = d.refs.iter().zip(&sh.witness).map(|(r, (a, b))| DrawRef { spec: r.spec.clone(), pos: 0.0, anchor: pt(*a), at: pt(*b) }).collect();
        let t = sh.tangent;
        let home = sh.text_at;
        out.segments.push(DrawSeg {
            index: 0,
            a: [home.x - t.x, home.y - t.y],
            b: [home.x + t.x, home.y + t.y],
            value_mm: sh.value,
            text,
            alt_text,
            prefix: seg.prefix.clone(),
            suffix: seg.suffix.clone(),
            above: seg.above.clone(),
            below: seg.below.clone(),
            text_pos: [home.x + seg.text_shift[0], home.y + seg.text_shift[1]],
            moved: seg.text_shift != [0.0, 0.0],
            text_shift: seg.text_shift,
            locked: false,
            anchor: 0,
            replaced: seg.value_text.is_some(),
            out: pt(sh.out),
            param: seg.param,
        });
        out.round = Some(RoundDraw {
            centre: pt(sh.centre),
            radius: sh.radius,
            arc: sh.arc.map(|(_, a0, sw)| [a0, sw]),
            arc_radius: sh.arc.map_or(0.0, |(r, _, _)| r),
            line: sh.line.map(|(a, b)| [pt(a), pt(b)]),
            ends: sh.ends.iter().map(|(p, dir)| (pt(*p), pt(*dir))).collect(),
            centre_mark: d.centre_mark && matches!(d.kind, DimKind::Radial | DimKind::Diameter),
        });
        out
    }

    /// A dimension as it would be if placed now, for the ghost drawn while the tool is active or a line is dragged.
    pub fn preview_dimension(&self, view: Id, kind: DimKind, refs: Vec<DimRef>, line_point: Point, axis: Option<[f64; 2]>, type_id: Option<Id>, reflex: bool) -> Result<DimDraw, String> {
        let tid = type_id.filter(|t| self.dimension_type(*t).is_some()).or_else(|| self.dimension_types().first().map(|(i, _)| *i)).unwrap_or(0);
        let mut d = self.new_dimension(view, kind, refs, line_point, axis, tid)?;
        d.reflex = reflex && kind == DimKind::Angular;
        Ok(self.draw_dimension(0, &d))
    }

    /// An existing dimension with its line moved to `line_point`, drawn but not stored.
    pub fn preview_dimension_line(&self, id: Id, line_point: Point) -> Result<DimDraw, String> {
        let mut d = self.dimension(id).cloned().ok_or("not a dimension")?;
        if d.kind.is_chain() {
            d.line_point = pt(line_point);
        } else {
            self.set_round_line_point(&mut d, line_point)?;
        }
        Ok(self.draw_dimension(id, &d))
    }
}

fn dir_at_pt(c: Point, a: f64, r: f64) -> Point {
    Point::new(c.x + r * a.cos(), c.y + r * a.sin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempdim::WallRef;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn room() -> (Document, Vec<Id>, Id) {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Internal Stud 90").unwrap().0;
        let mut ids = vec![];
        for x in [0.0, 3000.0, 6000.0, 9000.0] {
            ids.push(d.add_wall(wt, lvl, p(x, 0.0), p(x, 4000.0), 2700.0).unwrap());
        }
        let view = d.views().iter().find(|(_, v)| v.view_kind == ViewKind::FloorPlan).unwrap().0;
        (d, ids, view)
    }

    fn centre(w: Id) -> DimRef {
        DimRef { spec: RefSpec::Wall { id: w, offset: 0.0 }, pick: Some([0.0, 500.0]) }
    }

    fn place(d: &mut Document, view: Id, walls: &[Id]) -> Id {
        d.add_dimension(view, DimKind::Aligned, walls.iter().map(|w| centre(*w)).collect(), p(4500.0, 4800.0), None, None).unwrap()
    }

    fn x_of(d: &Document, w: Id) -> f64 {
        match d.get(w) {
            Some(Element::Wall(w)) => w.start.x,
            _ => panic!("not a wall"),
        }
    }

    #[test]
    fn a_chain_measures_each_segment_and_follows_a_moved_wall() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w);
        let draw = d.dimension_draw(id).unwrap();
        assert!(draw.invalid.is_none());
        let vals: Vec<f64> = draw.segments.iter().map(|s| s.value_mm).collect();
        assert_eq!(vals, vec![3000.0, 3000.0, 3000.0]);
        assert_eq!(draw.segments[0].text, "3000");
        d.transform_elements(&[w[1]], Xform::Move { dx: 250.0, dy: 0.0 }, EditOptions::default()).unwrap();
        let vals: Vec<f64> = d.dimension_draw(id).unwrap().segments.iter().map(|s| s.value_mm).collect();
        assert_eq!(vals, vec![3250.0, 2750.0, 3000.0]);
    }

    #[test]
    fn typing_a_value_moves_the_far_element_or_the_other_when_flipped() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w[..2]);
        d.set_dimension_value(id, 0, 3500.0).unwrap();
        assert_eq!(x_of(&d, w[1]), 3500.0);
        assert_eq!(x_of(&d, w[0]), 0.0);
        d.patch_dimension(id, &serde_json::json!({ "segments": [{ "index": 0, "anchor": 1 }] })).unwrap();
        d.set_dimension_value(id, 0, 3000.0).unwrap();
        assert_eq!(x_of(&d, w[0]), 500.0, "the first wall moved this time");
        assert_eq!(x_of(&d, w[1]), 3500.0);
        d.undo();
        assert_eq!(x_of(&d, w[0]), 0.0);
    }

    #[test]
    fn a_padlock_makes_the_other_wall_travel_with_the_one_moved() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w[..2]);
        d.patch_dimension(id, &serde_json::json!({ "segments": [{ "index": 0, "locked": true }] })).unwrap();
        d.transform_elements(&[w[0]], Xform::Move { dx: 400.0, dy: 0.0 }, EditOptions::default()).unwrap();
        assert_eq!(x_of(&d, w[1]), 3400.0);
        // one undo step for the move and the follow
        d.undo();
        assert_eq!(x_of(&d, w[0]), 0.0);
        assert_eq!(x_of(&d, w[1]), 3000.0);
    }

    #[test]
    fn a_lock_that_cannot_be_kept_refuses_the_edit() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w[..2]);
        d.patch_dimension(id, &serde_json::json!({ "segments": [{ "index": 0, "locked": true }] })).unwrap();
        d.set_pinned(&[w[1]], true).unwrap();
        let err = d.transform_elements(&[w[0]], Xform::Move { dx: 400.0, dy: 0.0 }, EditOptions::default()).unwrap_err();
        assert!(err.contains("another element"), "{err}");
        assert_eq!(x_of(&d, w[0]), 0.0, "the first move was taken back");
    }

    #[test]
    fn equality_keeps_the_inner_walls_evenly_spaced() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w);
        d.transform_elements(&[w[1]], Xform::Move { dx: 500.0, dy: 0.0 }, EditOptions::default()).unwrap();
        d.patch_dimension(id, &serde_json::json!({ "equality": true })).unwrap();
        // switching it on evens the spacing straight away
        assert!((x_of(&d, w[1]) - 3000.0).abs() < 1e-6 && (x_of(&d, w[2]) - 6000.0).abs() < 1e-6, "{} {}", x_of(&d, w[1]), x_of(&d, w[2]));
        // the end wall moves: the two inner ones redistribute
        d.transform_elements(&[w[3]], Xform::Move { dx: 600.0, dy: 0.0 }, EditOptions::default()).unwrap();
        assert!((x_of(&d, w[1]) - 3200.0).abs() < 1e-6, "{}", x_of(&d, w[1]));
        assert!((x_of(&d, w[2]) - 6400.0).abs() < 1e-6, "{}", x_of(&d, w[2]));
        let draw = d.dimension_draw(id).unwrap();
        assert!(draw.segments.iter().all(|s| (s.value_mm - 3200.0).abs() < 1e-6));
        d.patch_dimension(id, &serde_json::json!({ "eq_display": "eq" })).unwrap();
        assert!(d.dimension_draw(id).unwrap().segments.iter().all(|s| s.text == "EQ"));
        d.patch_dimension(id, &serde_json::json!({ "eq_display": "formula" })).unwrap();
        let t: Vec<String> = d.dimension_draw(id).unwrap().segments.iter().map(|s| s.text.clone()).collect();
        assert_eq!(t, vec!["", "3 @ 3200 = 9600", ""]);
    }

    #[test]
    fn deleting_an_inner_wall_closes_the_chain() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w);
        d.delete(&[w[1]]).unwrap();
        let dim = d.dimension(id).unwrap();
        assert_eq!(dim.refs.len(), 3);
        assert_eq!(dim.segments.len(), 2);
        let vals: Vec<f64> = d.dimension_draw(id).unwrap().segments.iter().map(|s| s.value_mm).collect();
        assert_eq!(vals, vec![6000.0, 3000.0]);
        d.delete(&[w[2]]).unwrap();
        assert_eq!(d.dimension(id).unwrap().refs.len(), 2);
        d.delete(&[w[3]]).unwrap();
        assert!(d.dimension(id).is_none(), "nothing left to measure");
        d.undo();
        d.undo();
        d.undo();
        assert_eq!(d.dimension(id).unwrap().refs.len(), 4, "undo brings the whole chain back");
    }

    #[test]
    fn replacement_text_must_not_be_a_number() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w[..2]);
        let bad = d.patch_dimension(id, &serde_json::json!({ "segments": [{ "index": 0, "value_text": "3200" }] }));
        assert!(bad.is_err());
        d.patch_dimension(id, &serde_json::json!({ "segments": [{ "index": 0, "value_text": "V.I.F.", "prefix": "(MIN.)" }] })).unwrap();
        let s = &d.dimension_draw(id).unwrap().segments[0];
        assert_eq!((s.text.as_str(), s.prefix.as_str(), s.replaced), ("V.I.F.", "(MIN.)", true));
        assert!(d.set_dimension_value(id, 0, 3200.0).is_err(), "the words stand in for the value");
    }

    #[test]
    fn rotating_a_wall_out_of_parallel_flags_the_dimension() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w[..2]);
        d.transform_elements(&[w[1]], Xform::Rotate { cx: 3000.0, cy: 2000.0, angle_deg: 5.0 }, EditOptions::default()).unwrap();
        assert!(d.dimension_draw(id).unwrap().invalid.is_some());
    }

    #[test]
    fn a_linear_dimension_measures_along_the_axis() {
        let (mut d, w, view) = room();
        let id = d.add_dimension(view, DimKind::Linear, vec![centre(w[0]), centre(w[2])], p(3000.0, -600.0), Some([1.0, 0.0]), None).unwrap();
        assert_eq!(d.dimension_draw(id).unwrap().segments[0].value_mm, 6000.0);
    }

    #[test]
    fn lengths_are_written_in_the_project_units() {
        let mm = LengthFormat::default();
        assert_eq!(format_length(2500.0, &mm), "2500");
        assert_eq!(format_length(2500.4, &mm), "2500");
        let m = LengthFormat { unit: LengthUnit::Metres, decimals: 3, symbol: true, ..LengthFormat::default() };
        assert_eq!(format_length(2500.0, &m), "2.5 m");
        let ft = LengthFormat { unit: LengthUnit::FeetInches, ..LengthFormat::default() };
        assert_eq!(format_length(3048.0, &ft), "10' - 0\"");
        assert_eq!(format_length(1676.4 + 12.7, &ft), "5' - 6 1/2\"");
    }

    #[test]
    fn dual_units_show_beside_or_below() {
        let (mut d, w, view) = room();
        let id = place(&mut d, view, &w[..2]);
        let mut ty = DimensionType::standard();
        ty.alt = Some(AltUnits { format: LengthFormat { unit: LengthUnit::FeetInches, ..LengthFormat::default() }, placement: AltPlacement::Right });
        let tid = d.add_dimension_type(ty.clone()).unwrap();
        d.patch_dimension(id, &serde_json::json!({ "dim_type": tid })).unwrap();
        assert_eq!(d.dimension_draw(id).unwrap().segments[0].text, "3000 [9' - 10 1/8\"]");
    }

    #[test]
    fn candidates_prefer_the_chosen_wall_line() {
        let (d, w, view) = room();
        let faces = d.dim_candidates(view, p(0.0, 2000.0), 120.0, TempDimSettings { wall_ref: WallRef::Faces, ..Default::default() });
        assert!(faces[0].label == "Face" && faces[0].spec.element() == w[0], "{:?}", faces.iter().map(|c| &c.label).collect::<Vec<_>>());
        let centre = d.dim_candidates(view, p(0.0, 2000.0), 120.0, TempDimSettings { wall_ref: WallRef::Centreline, ..Default::default() });
        assert!(matches!(centre[0].spec, RefSpec::Wall { offset, .. } if offset.abs() < 0.01), "{:?}", centre[0]);
    }

    #[test]
    fn a_temporary_dimension_becomes_a_permanent_one() {
        let (mut d, w, view) = room();
        let s = TempDimSettings { wall_ref: WallRef::Centreline, ..Default::default() };
        let temps = d.temp_dimensions(w[1], s, &Default::default());
        assert!(temps.iter().any(|t| t.key == "up" || t.key == "down"));
        let key = temps.iter().find(|t| t.key == "up" || t.key == "down").unwrap().key.clone();
        let id = d.promote_temp_dimension(view, w[1], &key, s, &Default::default(), &Default::default(), 300.0).unwrap();
        let v = d.dimension_draw(id).unwrap().segments[0].value_mm;
        assert!((v - 3000.0).abs() < 1e-6, "{v}");
        let len = d.promote_temp_dimension(view, w[1], "length", s, &Default::default(), &Default::default(), 300.0).unwrap();
        assert!((d.dimension_draw(len).unwrap().segments[0].value_mm - 4000.0).abs() < 1e-6);
    }
    fn arc_wall(d: &mut Document, r: f64) -> Id {
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Internal Stud 90").unwrap().0;
        let mut w = Wall::new(wt, lvl, p(r, 0.0), p(0.0, r), 2700.0);
        w.curve = Some(crate::curve::Curve::Arc { start: p(r, 0.0), end: p(0.0, r), centre: p(0.0, 0.0), ccw: true });
        d.add_wall_with(w).unwrap()
    }

    fn at(id: Id, off: f64) -> DimRef {
        DimRef { spec: RefSpec::Wall { id, offset: off }, pick: None }
    }

    fn text_of(d: &Document, id: Id) -> String {
        let dr = d.dimension_draw(id).unwrap();
        assert!(dr.invalid.is_none(), "{:?}", dr.invalid);
        let s = &dr.segments[0];
        format!("{}", s.text)
    }

    #[test]
    fn an_angular_dimension_takes_the_quadrant_of_the_cursor_and_can_go_the_long_way() {
        let (mut d, _, view) = room();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Internal Stud 90").unwrap().0;
        let a = d.add_wall(wt, lvl, p(0.0, -9000.0), p(4000.0, -9000.0), 2700.0).unwrap();
        let b = d.add_wall(wt, lvl, p(0.0, -9000.0), p(3000.0, -6000.0), 2700.0).unwrap();
        let inside = d.add_dimension(view, DimKind::Angular, vec![at(a, 0.0), at(b, 0.0)], p(1500.0, -8800.0), None, None).unwrap();
        assert_eq!(text_of(&d, inside), "45\u{00B0}");
        let outside = d.add_dimension(view, DimKind::Angular, vec![at(a, 0.0), at(b, 0.0)], p(-1500.0, -8800.0), None, None).unwrap();
        assert_eq!(text_of(&d, outside), "135\u{00B0}");
        d.patch_dimension(inside, &serde_json::json!({ "reflex": true })).unwrap();
        assert_eq!(text_of(&d, inside), "315\u{00B0}");
        // the angle follows the wall when it turns
        d.transform_elements(&[b], Xform::Rotate { cx: 0.0, cy: -9000.0, angle_deg: 15.0 }, EditOptions::default()).unwrap();
        assert_eq!(text_of(&d, outside), "120\u{00B0}");
        // parallel references have no angle
        let c = d.add_wall(wt, lvl, p(0.0, -8000.0), p(4000.0, -8000.0), 2700.0).unwrap();
        assert!(d.add_dimension(view, DimKind::Angular, vec![at(a, 0.0), at(c, 0.0)], p(1000.0, -8500.0), None, None).is_err());
    }

    #[test]
    fn radial_and_diameter_dimensions_measure_the_face_they_were_picked_on() {
        let (mut d, _, view) = room();
        let w = arc_wall(&mut d, 5000.0);
        let rad = d.add_dimension(view, DimKind::Radial, vec![at(w, 0.0)], p(1800.0, 1800.0), None, None).unwrap();
        assert_eq!(text_of(&d, rad), "R5000");
        let dia = d.add_dimension(view, DimKind::Diameter, vec![at(w, 100.0)], p(1800.0, 1800.0), None, None).unwrap();
        assert_eq!(text_of(&d, dia), "\u{00D8}9800", "the left face of a counter-clockwise wall is the inner one");
        let outer = d.add_dimension(view, DimKind::Radial, vec![at(w, -100.0)], p(1800.0, 1800.0), None, None).unwrap();
        assert_eq!(text_of(&d, outer), "R5100");
        // a straight line is not a curve
        assert!(d.add_dimension(view, DimKind::Radial, vec![centre(0)], p(0.0, 0.0), None, None).is_err());
        // a centre mark is for radial and diameter only
        d.patch_dimension(rad, &serde_json::json!({ "centre_mark": true })).unwrap();
        assert!(d.dimension_draw(rad).unwrap().round.unwrap().centre_mark);
        assert!(d.patch_dimension(rad, &serde_json::json!({ "reflex": true })).is_err());
        // they follow the wall, and go with it
        d.transform_elements(&[w], Xform::Move { dx: 1000.0, dy: 0.0 }, EditOptions::default()).unwrap();
        assert_eq!(text_of(&d, rad), "R5000");
        assert_eq!(d.dimension_draw(rad).unwrap().round.unwrap().centre, [1000.0, 0.0]);
        d.delete(&[w]).unwrap();
        assert!(d.dimension(rad).is_none() && d.dimension(dia).is_none());
        d.undo();
        assert!(d.dimension(rad).is_some());
        // a typed radius changes the wall; a locked one holds against another change
        d.set_dimension_value(rad, 0, 4000.0).unwrap();
        d.patch_dimension(rad, &serde_json::json!({ "segments": [{ "index": 0, "locked": true }] })).unwrap();
        assert!(d.set_dimension_value(dia, 0, 9000.0).is_err());
    }

    #[test]
    fn an_arc_length_dimension_measures_between_its_boundaries_on_the_side_of_the_cursor() {
        let (mut d, _, view) = room();
        let w = arc_wall(&mut d, 5000.0);
        let refs = vec![at(w, 0.0), DimRef { spec: RefSpec::WallEnd { id: w, end: 0 }, pick: None }, DimRef { spec: RefSpec::WallEnd { id: w, end: 1 }, pick: None }];
        let short = d.add_dimension(view, DimKind::ArcLength, refs.clone(), p(3000.0, 3000.0), None, None).unwrap();
        assert_eq!(text_of(&d, short), format!("\u{2312}{:.0}", 5000.0 * std::f64::consts::FRAC_PI_2));
        let long = d.add_dimension(view, DimKind::ArcLength, refs, p(-3000.0, -3000.0), None, None).unwrap();
        assert_eq!(text_of(&d, long), format!("\u{2312}{:.0}", 5000.0 * 3.0 * std::f64::consts::FRAC_PI_2));
        let dr = d.dimension_draw(short).unwrap();
        let rd = dr.round.unwrap();
        assert!((rd.arc_radius - 3000.0 * 2f64.sqrt()).abs() < 1.0, "the dimension arc passes through the cursor");
        assert_eq!(dr.refs.len(), 3);
        assert!(d.add_dimension(view, DimKind::ArcLength, vec![at(w, 0.0), at(w, 0.0)], p(3000.0, 3000.0), None, None).is_err());
    }

    #[test]
    fn candidates_for_round_tools_are_curves_and_lines_are_never_curves() {
        let (mut d, _, view) = room();
        let w = arc_wall(&mut d, 5000.0);
        let near = p(5000.0 * 0.7071, 5000.0 * 0.7071);
        let arcs = d.dim_candidates_for(view, near, 150.0, TempDimSettings::default(), DimWant::Arc);
        assert!(!arcs.is_empty() && arcs.iter().all(|c| c.arc.is_some() && c.spec.element() == w));
        let lines = d.dim_candidates_for(view, near, 150.0, TempDimSettings::default(), DimWant::Line);
        assert!(lines.is_empty(), "the middle of a curved wall has no straight line");
        let ends = d.dim_candidates_for(view, p(5000.0, 10.0), 150.0, TempDimSettings::default(), DimWant::Line);
        assert!(ends.iter().all(|c| matches!(c.spec, RefSpec::WallEnd { .. })) && !ends.is_empty());
    }

}
