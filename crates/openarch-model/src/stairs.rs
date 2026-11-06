//! Stairs built from components: a stair owns its runs and landings (and later its supports), which exist only
//! inside it. The stair works out its riser height from its height and riser count, lays out each run's risers and
//! treads along its location line, bridges consecutive runs with automatic landings, and checks the result against
//! its type's calculation rules. Every part of the stair is a convex solid, so the 3D mesh, elevations, sections and
//! IFC all come from one list (`stair_pieces`); plans are drawn from the layout itself (`stair_layout`).

use serde::{Deserialize, Serialize};

use crate::element::{IfcParams, TypeInfo};
use crate::{Document, Element, Id, Point};

fn yes() -> bool { true }
fn d_max_riser() -> f64 { 190.0 }
fn d_min_riser() -> f64 { 115.0 }
fn d_min_tread() -> f64 { 250.0 }
fn d_max_tread() -> f64 { 355.0 }
fn d_min_width() -> f64 { 1000.0 }
fn d_min_2rt() -> f64 { 550.0 }
fn d_max_2rt() -> f64 { 700.0 }
fn d_max_risers() -> u32 { 18 }
fn d_min_risers() -> u32 { 2 }
fn d_depth() -> f64 { 150.0 }
fn d_tread() -> f64 { 40.0 }
fn d_nosing() -> f64 { 25.0 }
fn d_riser() -> f64 { 19.0 }
fn d_landing() -> f64 { 200.0 }
fn d_angle() -> f64 { 30.0 }
fn d_width() -> f64 { 1000.0 }
fn d_headroom() -> f64 { 2000.0 }
fn d_support_w() -> f64 { 45.0 }
fn d_support_d() -> f64 { 290.0 }
fn d_support_above() -> f64 { 50.0 }
fn d_one() -> u32 { 1 }

/// A stair type's calculation rules. Breaking one is a warning on the stair, not an error. The defaults follow the
/// NCC for public stairs: risers 115 to 190, goings 250 to 355, 2R + G from 550 to 700, 2 to 18 risers a flight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StairRules {
    #[serde(default = "d_max_riser")]
    pub max_riser_mm: f64,
    #[serde(default = "d_min_riser")]
    pub min_riser_mm: f64,
    #[serde(default = "d_min_tread")]
    pub min_tread_mm: f64,
    #[serde(default = "d_max_tread")]
    pub max_tread_mm: f64,
    #[serde(default = "d_min_width")]
    pub min_run_width_mm: f64,
    /// 2R + T must lie between these.
    #[serde(default = "d_min_2rt")]
    pub min_2r_t_mm: f64,
    #[serde(default = "d_max_2rt")]
    pub max_2r_t_mm: f64,
    #[serde(default = "d_max_risers")]
    pub max_risers_per_run: u32,
    #[serde(default = "d_min_risers")]
    pub min_risers_per_run: u32,
    /// Walking comfort: T - R of at least 120 (off by default).
    #[serde(default)]
    pub check_t_minus_r: bool,
    /// Safety: R + T about 450 to 480 (off by default).
    #[serde(default)]
    pub check_r_plus_t: bool,
    /// Clear height over the nosing line to floors and ceilings above (the NCC's 2 m).
    #[serde(default = "d_headroom")]
    pub min_headroom_mm: f64,
    #[serde(default = "yes")]
    pub check_headroom: bool,
}

impl Default for StairRules {
    fn default() -> Self {
        StairRules { max_riser_mm: d_max_riser(), min_riser_mm: d_min_riser(), min_tread_mm: d_min_tread(), max_tread_mm: d_max_tread(), min_run_width_mm: d_min_width(), min_2r_t_mm: d_min_2rt(), max_2r_t_mm: d_max_2rt(), max_risers_per_run: d_max_risers(), min_risers_per_run: d_min_risers(), check_t_minus_r: false, check_r_plus_t: false, min_headroom_mm: d_headroom(), check_headroom: true }
    }
}

/// How a run is built: one monolithic flight (cast concrete) or separate treads and risers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunConstruction {
    #[serde(default = "yes")]
    pub monolithic: bool,
    /// A monolithic flight's thickness, square to its pitch, under the nosing line.
    #[serde(default = "d_depth")]
    pub structural_depth_mm: f64,
    #[serde(default)]
    pub material: String,
    #[serde(default = "yes")]
    pub treads: bool,
    #[serde(default = "d_tread")]
    pub tread_thickness_mm: f64,
    /// How far a tread runs past the riser below it.
    #[serde(default = "d_nosing")]
    pub nosing_mm: f64,
    #[serde(default)]
    pub tread_material: String,
    #[serde(default = "yes")]
    pub risers: bool,
    #[serde(default = "d_riser")]
    pub riser_thickness_mm: f64,
    #[serde(default)]
    pub riser_material: String,
    /// Slanted riser faces (Fine detail; stored for later).
    #[serde(default)]
    pub slanted_risers: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LandingConstruction {
    #[serde(default = "yes")]
    pub monolithic: bool,
    #[serde(default = "d_landing")]
    pub thickness_mm: f64,
    #[serde(default)]
    pub material: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    /// A closed stringer: a board along the outside of the run, its top above the nosing line.
    #[default]
    Stringer,
    /// A carriage: under the treads, its top cut to the steps.
    Carriage,
}

/// A support under or beside a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Support {
    #[serde(default)]
    pub kind: SupportKind,
    #[serde(default = "d_support_w")]
    pub width_mm: f64,
    /// Square to the pitch.
    #[serde(default = "d_support_d")]
    pub total_depth_mm: f64,
    /// A stringer's top above the nosing line.
    #[serde(default = "d_support_above")]
    pub above_nosing_mm: f64,
    /// Out from the edge of the run (a stringer) or in from it (a carriage).
    #[serde(default)]
    pub offset_mm: f64,
    #[serde(default)]
    pub material: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Supports {
    #[serde(default)]
    pub left: Option<Support>,
    #[serde(default)]
    pub right: Option<Support>,
    /// Carriages under the middle of the run, evenly across it.
    #[serde(default)]
    pub middle: Option<Support>,
    #[serde(default = "d_one")]
    pub middle_count: u32,
}

impl Supports {
    /// How far a side's stringer reaches out past the run's edge.
    pub fn outside(&self, left: bool) -> f64 {
        match if left { &self.left } else { &self.right } {
            Some(s) if s.kind == SupportKind::Stringer => s.offset_mm + s.width_mm,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutMarkStyle {
    #[default]
    Zigzag,
    Curved,
    SingleLine,
    DoubleLine,
}

/// The break line a plan draws where its cut plane passes through a flight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CutMark {
    #[serde(default)]
    pub style: CutMarkStyle,
    /// The angle between the cut line and the risers.
    #[serde(default = "d_angle")]
    pub angle_deg: f64,
}

impl Default for CutMark {
    fn default() -> Self {
        CutMark { style: CutMarkStyle::Zigzag, angle_deg: d_angle() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StairFunction {
    #[default]
    Interior,
    Exterior,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StairType {
    pub name: String,
    #[serde(default)]
    pub rules: StairRules,
    pub run: RunConstruction,
    pub landing: LandingConstruction,
    #[serde(default)]
    pub cut_mark: CutMark,
    #[serde(default)]
    pub supports: Supports,
    #[serde(default)]
    pub function: StairFunction,
    #[serde(default)]
    pub info: TypeInfo,
}

/// Where a run's location line sits across it, looking up the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunJustification {
    Left,
    #[default]
    Center,
    Right,
    /// The outside face of the left support (supports come later: until then the left edge).
    LeftExterior,
    RightExterior,
}

impl RunJustification {
    /// The run's centre from its location line, in run widths to the left.
    fn centre_offset(self) -> f64 {
        match self {
            RunJustification::Left | RunJustification::LeftExterior => -0.5,
            RunJustification::Center => 0.0,
            RunJustification::Right | RunJustification::RightExterior => 0.5,
        }
    }

    pub fn mirrored(self) -> Self {
        match self {
            RunJustification::Left => RunJustification::Right,
            RunJustification::Right => RunJustification::Left,
            RunJustification::LeftExterior => RunJustification::RightExterior,
            RunJustification::RightExterior => RunJustification::LeftExterior,
            RunJustification::Center => RunJustification::Center,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunShape {
    #[default]
    Straight,
    /// Round `centre` from the run's start (its radius and where it begins), anticlockwise unless `clockwise`. A
    /// full-step spiral and a centre-ends spiral are both this; they differ only in how they are drawn.
    Spiral { centre: Point, #[serde(default)] clockwise: bool },
    /// From the run's start through each corner and on towards its end, with winders round the corners.
    Winder { corners: Vec<Point>, #[serde(default)] style: crate::runpath::WinderStyle },
    /// Sketched: its left and right boundaries (up the run) and its riser lines from the bottom up, left to right. The
    /// riser count is the number of riser lines.
    Sketched { left: Vec<Point>, right: Vec<Point>, risers: Vec<[Point; 2]> },
}

/// A run: risers and treads along a location line from `start` towards `end`. Its riser count is its own; its
/// length follows from the count and the stair's tread depth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StairRun {
    #[serde(default)]
    pub shape: RunShape,
    pub start: Point,
    /// A point along the run's direction (where it was drawn to).
    pub end: Point,
    #[serde(default)]
    pub justification: RunJustification,
    #[serde(default = "d_width")]
    pub width_mm: f64,
    pub risers: u32,
    #[serde(default = "yes")]
    pub begins_with_riser: bool,
    #[serde(default = "yes")]
    pub ends_with_riser: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandingKind {
    /// Bridges the end of run `after_run` and the start of the next one.
    #[default]
    Automatic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StairLanding {
    #[serde(default)]
    pub kind: LandingKind,
    pub after_run: usize,
    /// A sketched landing's outline, at the top of run `after_run` (between it and the next, or at the top of the
    /// last). None: automatic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<Point>>,
}

/// One storey of a multistory stair, unpinned: its own runs and sketched landings instead of a repeat of the stair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StairStorey {
    /// The level the storey starts from.
    pub base_level: Id,
    pub runs: Vec<StairRun>,
    #[serde(default)]
    pub landings: Vec<StairLanding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stair {
    pub stair_type: Id,
    pub base_level: Id,
    #[serde(default)]
    pub base_offset_mm: f64,
    /// None: the stair is `unconnected_height_mm` high.
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub top_offset_mm: f64,
    #[serde(default)]
    pub unconnected_height_mm: f64,
    pub desired_risers: u32,
    pub tread_depth_mm: f64,
    #[serde(default)]
    pub runs: Vec<StairRun>,
    #[serde(default)]
    pub landings: Vec<StairLanding>,
    /// Multistory Top Level: the stair repeats, storey by storey, up to this level.
    #[serde(default)]
    pub multistory_top: Option<Id>,
    /// Storeys of a multistory stair unpinned from the stair below, each with its own runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub storeys: Vec<StairStorey>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub ifc: IfcParams,
}

// ---- small plan vector helpers
fn add(a: Point, b: Point) -> Point { Point::new(a.x + b.x, a.y + b.y) }
fn sub(a: Point, b: Point) -> Point { Point::new(a.x - b.x, a.y - b.y) }
fn mul(a: Point, k: f64) -> Point { Point::new(a.x * k, a.y * k) }
fn dot(a: Point, b: Point) -> f64 { a.x * b.x + a.y * b.y }
fn cross(a: Point, b: Point) -> f64 { a.x * b.y - a.y * b.x }
fn add_p(a: Point, b: Point) -> Point { add(a, b) }

/// Down the run from a riser line drawn from its left end `a` to its right end `b`, `k` long.
fn down_run(a: Point, b: Point, k: f64) -> Point {
    let lv = sub(a, b);
    let n = (lv.x * lv.x + lv.y * lv.y).sqrt().max(1e-9);
    Point::new(-lv.y / n * k, lv.x / n * k)
}

/// A spiral's, winder's or sketched run's supports: stringers along its edges, carriages under each tread, in short
/// straight stretches.
fn path_supports(run: &RunLayout, path: &crate::runpath::RunPath, riser: f64, tread: f64, t: &StairType, floor: f64, out: &mut Vec<StairPiece>) {
    use crate::railing::offset_line;
    use crate::runpath::slice;
    let (rc, sp) = (&t.run, &t.supports);
    let mat = |m: &str| if m.is_empty() { None } else { Some(m.to_string()) };
    let pitch = if tread > 0.0 { riser / tread } else { 0.0 };
    let slope = (1.0 + pitch * pitch).sqrt();
    let under = if rc.treads && !rc.monolithic { rc.tread_thickness_mm } else { 0.0 };
    // a band between two lines that run side by side (point for point), from `bottom` to `top` by distance along the
    // walkline: one solid a stretch
    let band = |a: &[(Point, f64)], b: &[(Point, f64)], top: &dyn Fn(f64) -> f64, bottom: &dyn Fn(f64) -> f64, m: &Option<String>, out: &mut Vec<StairPiece>| {
        for i in 0..a.len().min(b.len()).saturating_sub(1) {
            let cloud: Vec<[f64; 3]> = [a[i], a[i + 1], b[i], b[i + 1]].iter().flat_map(|(p, s)| {
                let hi = top(*s);
                [[p.x, p.y, hi], [p.x, p.y, bottom(*s).min(hi)]]
            }).collect();
            if let Some(c) = Convex::hull_of(&cloud).and_then(|c| c.clip(&|p| p[2], floor, true)) {
                out.push(StairPiece { part: StairPart::Support(run.index), material: m.clone(), solid: c });
            }
        }
    };
    let carriage = |s: &Support, a: &[(Point, f64)], b: &[(Point, f64)], out: &mut Vec<StairPiece>| {
        let drop = s.total_depth_mm * slope;
        for &(s0, s1, top) in &run.treads {
            band(&slice(a, s0, s1), &slice(b, s0, s1), &|_| top - under, &|sv| run.nosing_z(sv) - under - drop, &mat(&s.material), out);
        }
    };
    for (side, s) in [(1.0, &sp.left), (-1.0, &sp.right)] {
        let Some(s) = s else { continue };
        let edge = if side > 0.0 { &path.left } else { &path.right };
        match s.kind {
            SupportKind::Stringer => {
                let (a, b) = (offset_line(edge, side * s.offset_mm), offset_line(edge, side * (s.offset_mm + s.width_mm)));
                let drop = s.total_depth_mm * slope;
                band(&a, &b, &|sv| run.nosing_z(sv) + s.above_nosing_mm, &|sv| run.nosing_z(sv) + s.above_nosing_mm - drop, &mat(&s.material), out);
            }
            SupportKind::Carriage => {
                let (a, b) = (offset_line(edge, -side * s.offset_mm), offset_line(edge, -side * (s.offset_mm + s.width_mm)));
                carriage(s, &a, &b, out);
            }
        }
    }
    if let Some(m) = &sp.middle {
        let n = sp.middle_count.max(1);
        for k in 0..n {
            let c = -run.width / 2.0 + run.width * (k + 1) as f64 / (n + 1) as f64;
            carriage(m, &offset_line(&path.walk, c - m.width_mm / 2.0), &offset_line(&path.walk, c + m.width_mm / 2.0), out);
        }
    }
}

/// A plan polygon as convex pieces: itself when it is convex, else triangles.
fn convex_parts(poly: &[Point]) -> Vec<Vec<Point>> {
    let n = poly.len();
    if n < 3 { return vec![]; }
    let turns: Vec<f64> = (0..n).map(|i| cross(sub(poly[(i + 1) % n], poly[i]), sub(poly[(i + 2) % n], poly[(i + 1) % n]))).collect();
    if turns.iter().all(|t| *t >= -1e-6) || turns.iter().all(|t| *t <= 1e-6) { return vec![poly.to_vec()]; }
    let region = crate::geom::Region { outer: poly.to_vec(), holes: vec![] };
    match crate::geom::triangulate_region(&region) {
        Some((pts, tris)) => tris.iter().map(|t| vec![pts[t[0]], pts[t[1]], pts[t[2]]]).collect(),
        None => vec![poly.to_vec()],
    }
}

/// The convex hull of plan points, counter-clockwise.
pub fn hull(mut pts: Vec<Point>) -> Vec<Point> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup_by(|a, b| a.dist(b) < 1e-6);
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<Point> = vec![];
    for p in &pts {
        while lower.len() >= 2 && cross(sub(lower[lower.len() - 1], lower[lower.len() - 2]), sub(*p, lower[lower.len() - 2])) <= 1e-9 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<Point> = vec![];
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(sub(upper[upper.len() - 1], upper[upper.len() - 2]), sub(*p, upper[upper.len() - 2])) <= 1e-9 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// A run laid out: its frame (start of the centre line, direction up the run, left), and its steps.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunLayout {
    pub index: usize,
    pub origin: Point,
    pub dir: Point,
    pub left: Point,
    pub width: f64,
    /// Along the run, where it ends (the last riser, or the back of the last tread).
    pub length: f64,
    /// The height the run starts from and the height it reaches.
    pub z0: f64,
    pub z1: f64,
    /// Each riser: where it stands along the run and the top of the step it rises to.
    pub risers: Vec<(f64, f64)>,
    /// Each tread: from and to along the run, and its top.
    pub treads: Vec<(f64, f64, f64)>,
    /// A spiral's or winder's layout along its walkline (None for a straight run, which `at` describes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<crate::runpath::RunPath>,
}

impl RunLayout {
    /// A plan point `s` along the run and `t` to the left of its centre line.
    pub fn at(&self, s: f64, t: f64) -> Point {
        add(add(self.origin, mul(self.dir, s)), mul(self.left, t))
    }

    /// The run's footprint, counter-clockwise.
    pub fn footprint(&self) -> Vec<Point> {
        if let Some(p) = &self.path {
            let mut out: Vec<Point> = p.right.iter().map(|x| x.0).collect();
            out.extend(p.left.iter().rev().map(|x| x.0));
            if crate::geom::signed_area(&out) < 0.0 { out.reverse(); }
            return out;
        }
        let w = self.width / 2.0;
        vec![self.at(0.0, -w), self.at(self.length, -w), self.at(self.length, w), self.at(0.0, w)]
    }

    /// The height of the nosing line at `s`: through the front edge of every tread above a riser. A run with no
    /// risers (a ramp's) is its sloped surface.
    pub fn nosing_z(&self, s: f64) -> f64 {
        match (self.risers.first(), self.risers.last()) {
            (Some(a), Some(b)) if self.risers.len() > 1 => a.1 + (s - a.0) * (b.1 - a.1) / (b.0 - a.0),
            (Some(a), _) => a.1,
            _ if self.length > 1e-9 => self.z0 + (self.z1 - self.z0) * s / self.length,
            _ => self.z0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LandingLayout {
    pub index: usize,
    pub after_run: usize,
    /// Counter-clockwise, convex.
    pub outline: Vec<Point>,
    pub z: f64,
    pub thickness: f64,
}

/// A stair worked out: heights, riser and tread, runs, landings, and what breaks its type's rules.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StairLayout {
    pub base_z: f64,
    pub top_z: f64,
    pub height: f64,
    pub desired_risers: u32,
    pub actual_risers: u32,
    pub riser_mm: f64,
    pub tread_mm: f64,
    pub runs: Vec<RunLayout>,
    pub landings: Vec<LandingLayout>,
    pub warnings: Vec<String>,
}

impl StairLayout {
    /// The height reached by the last run.
    pub fn reached_z(&self) -> f64 {
        self.runs.last().map_or(self.base_z, |r| r.z1)
    }
}

/// Treads in a run of `risers` (ends with a riser: the floor or landing above is its last tread).
pub fn treads_in(risers: u32, begins: bool, ends: bool) -> u32 {
    (risers + u32::from(!begins) + u32::from(!ends)).saturating_sub(1)
}

/// The risers a run drawn `length` long holds at tread depth `tread`.
pub fn risers_for_length(length: f64, tread: f64, begins: bool, ends: bool) -> u32 {
    let nt = (length / tread).round().max(0.0) as u32;
    (nt + 1).saturating_sub(u32::from(!begins) + u32::from(!ends)).max(1)
}

/// The automatic landing between the end of `a` and the start of `b`: the convex hull bridging the two, squared off
/// where their sides meet, and as deep as the run is wide when the second run turns back. None when the runs simply
/// continue one into the other.
pub fn landing_between(a: &RunLayout, b: &RunLayout) -> Option<Vec<Point>> {
    // spirals and winders turn without landings; one that meets another run joins it directly
    if a.path.is_some() || b.path.is_some() { return None; }
    let (wa, wb) = (a.width / 2.0, b.width / 2.0);
    let e1 = [a.at(a.length, -wa), a.at(a.length, wa)];
    let s2 = [b.at(0.0, -wb), b.at(0.0, wb)];
    let mut pts = vec![e1[0], e1[1], s2[0], s2[1]];
    if dot(a.dir, b.dir) < -0.5 {
        // turning back: the landing runs on past both runs by a run's width
        let depth = a.width.max(b.width);
        pts.extend([add(e1[0], mul(a.dir, depth)), add(e1[1], mul(a.dir, depth)), sub(s2[0], mul(b.dir, depth)), sub(s2[1], mul(b.dir, depth))]);
    } else {
        // where the sides of the two runs meet ahead of the first and behind the second
        for p in e1 {
            for q in s2 {
                let den = cross(a.dir, b.dir);
                if den.abs() < 1e-9 {
                    continue;
                }
                let t = cross(sub(q, p), b.dir) / den;
                let x = add(p, mul(a.dir, t));
                let behind = dot(sub(q, x), b.dir);
                if t >= -1.0 && behind >= -1.0 {
                    pts.push(x);
                }
            }
        }
    }
    let h = hull(pts);
    let area = crate::geom::signed_area(&h);
    (h.len() >= 3 && area.abs() > 1000.0).then_some(h)
}

impl Document {
    pub fn stair_types(&self) -> Vec<(Id, &StairType)> {
        self.iter().filter_map(|(i, e)| if let Element::StairType(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn stair_type(&self, id: Id) -> Option<&StairType> {
        match self.get(id)? { Element::StairType(t) => Some(t), _ => None }
    }

    pub fn stairs(&self) -> Vec<(Id, &Stair)> {
        self.iter().filter_map(|(i, e)| if let Element::Stair(s) = e { Some((i, s)) } else { None }).collect()
    }

    pub fn stair(&self, id: Id) -> Option<&Stair> {
        match self.get(id)? { Element::Stair(s) => Some(s), _ => None }
    }

    /// The starter stair types, added to a project that has none: a cast concrete stair and a timber one.
    pub fn ensure_stair_types(&mut self) -> Result<(), String> {
        if !self.stair_types().is_empty() { return Ok(()); }
        self.transact("Stair types", |tx| {
            for t in starter_stair_types() { tx.add(Element::StairType(t)); }
            Ok(())
        })
    }

    /// The heights a stair spans: its base and its top.
    pub fn stair_heights(&self, s: &Stair) -> Option<(f64, f64)> {
        let base = self.level(s.base_level)?.elevation_mm + s.base_offset_mm;
        let top = match s.top_level {
            Some(t) => self.level(t)?.elevation_mm + s.top_offset_mm,
            None => base + s.unconnected_height_mm,
        };
        Some((base, top))
    }

    /// A new stair from base to top level with its type's tread and the fewest risers its rules allow.
    pub fn new_stair(&self, stair_type: Id, base_level: Id, top_level: Option<Id>) -> Result<Stair, String> {
        let t = self.stair_type(stair_type).ok_or("unknown stair type")?;
        let mut s = Stair { stair_type, base_level, base_offset_mm: 0.0, top_level, top_offset_mm: 0.0, unconnected_height_mm: 3000.0, desired_risers: 1, tread_depth_mm: t.rules.min_tread_mm, runs: vec![], landings: vec![], multistory_top: None, storeys: vec![], pinned: false, comments: String::new(), mark: String::new(), ifc: IfcParams { guid: crate::element::new_ifc_guid(), ..Default::default() } };
        let (b, top) = self.stair_heights(&s).ok_or("unknown level")?;
        if top - b <= 0.0 {
            return Err("The top of the stair must be above its base.".into());
        }
        s.desired_risers = ((top - b) / t.rules.max_riser_mm - 1e-9).ceil().max(1.0) as u32;
        Ok(s)
    }

    /// Lay the stair out: riser height, runs, landings and the rule checks.
    pub fn stair_layout(&self, s: &Stair) -> Option<StairLayout> {
        let t = self.stair_type(s.stair_type)?;
        let (base_z, top_z) = self.stair_heights(s)?;
        let height = top_z - base_z;
        let n = s.desired_risers.max(1);
        let r = height / n as f64;
        let tr = s.tread_depth_mm;
        let mut runs = vec![];
        let mut z = base_z;
        for (i, run) in s.runs.iter().enumerate() {
            let d = sub(run.end, run.start);
            let len = (d.x * d.x + d.y * d.y).sqrt();
            let dir = if len > 1e-9 { mul(d, 1.0 / len) } else { Point::new(1.0, 0.0) };
            let left = Point::new(-dir.y, dir.x);
            let mut origin = add(run.start, mul(left, run.justification.centre_offset() * run.width_mm));
            match run.justification {
                RunJustification::LeftExterior => origin = add(origin, mul(left, -t.supports.outside(true))),
                RunJustification::RightExterior => origin = add(origin, mul(left, t.supports.outside(false))),
                _ => {}
            }
            if let RunShape::Sketched { left: bl, right: br, risers: lines } = &run.shape {
                // where each riser line stands along the walkline through their middles
                let mids: Vec<Point> = lines.iter().map(|[a, b]| Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)).collect();
                let mut acc = 0.0;
                let ss: Vec<f64> = mids.iter().enumerate().map(|(k, p)| { if k > 0 { acc += mids[k - 1].dist(p); } acc }).collect();
                let risers: Vec<(f64, f64)> = ss.iter().enumerate().map(|(k, s)| (*s, z + (k + 1) as f64 * r)).collect();
                let treads: Vec<(f64, f64, f64)> = (0..ss.len().saturating_sub(1)).map(|k| (ss[k], ss[k + 1], risers[k].1)).collect();
                let tops: Vec<f64> = treads.iter().map(|x| x.2).collect();
                let z1 = z + lines.len() as f64 * r;
                let path = crate::runpath::sketched(bl, br, lines, &tops);
                let first = mids.first().copied().unwrap_or(run.start);
                let d = sub(mids.get(1).copied().unwrap_or(run.end), first);
                let dl = (d.x * d.x + d.y * d.y).sqrt().max(1e-9);
                let dir = mul(d, 1.0 / dl);
                runs.push(RunLayout { index: i, origin: first, dir, left: Point::new(-dir.y, dir.x), width: run.width_mm, length: ss.last().copied().unwrap_or(0.0), z0: z, z1, risers, treads, path: Some(path) });
                z = z1;
                continue;
            }
            let off = u32::from(!run.begins_with_riser);
            let nt = treads_in(run.risers, run.begins_with_riser, run.ends_with_riser);
            let risers: Vec<(f64, f64)> = (0..run.risers).map(|k| ((k + off) as f64 * tr, z + (k + 1) as f64 * r)).collect();
            let treads: Vec<(f64, f64, f64)> = (0..nt).map(|j| {
                let s0 = j as f64 * tr;
                let up = risers.iter().filter(|(rs, _)| *rs <= s0 + 1e-6).count();
                (s0, s0 + tr, z + up as f64 * r)
            }).collect();
            let z1 = z + run.risers as f64 * r;
            let steps = crate::runpath::Steps { length: nt as f64 * tr, risers: risers.iter().map(|x| x.0).collect(), treads: treads.clone() };
            let path = match &run.shape {
                RunShape::Straight | RunShape::Sketched { .. } => None,
                RunShape::Spiral { centre, clockwise } => Some(crate::runpath::spiral(*centre, run.start, *clockwise, run.width_mm, &steps)),
                RunShape::Winder { corners, style } => {
                    let mut pts = vec![run.start];
                    pts.extend(corners.iter().copied());
                    pts.push(run.end);
                    Some(crate::runpath::winder(&pts, *style, run.width_mm, &steps))
                }
            };
            runs.push(RunLayout { index: i, origin, dir, left, width: run.width_mm, length: nt as f64 * tr, z0: z, z1, risers, treads, path });
            z = z1;
        }
        let mut landings = vec![];
        for (k, l) in s.landings.iter().enumerate() {
            if let (Some(o), Some(a)) = (&l.outline, runs.get(l.after_run)) {
                let mut outline = o.clone();
                if crate::geom::signed_area(&outline) < 0.0 { outline.reverse(); }
                landings.push(LandingLayout { index: k, after_run: l.after_run, outline, z: a.z1, thickness: t.landing.thickness_mm });
                continue;
            }
            let (Some(a), Some(b)) = (runs.get(l.after_run), runs.get(l.after_run + 1)) else { continue };
            if let Some(outline) = landing_between(a, b) {
                landings.push(LandingLayout { index: k, after_run: l.after_run, outline, z: a.z1, thickness: t.landing.thickness_mm });
            }
        }
        let actual: u32 = s.runs.iter().map(|r| r.risers).sum();
        let rl = &t.rules;
        let mut warnings = vec![];
        if actual < n {
            warnings.push(format!("The stair does not reach its top: {actual} of {n} risers are in its runs."));
        }
        if r > rl.max_riser_mm + 1e-6 {
            warnings.push(format!("The riser height {r:.1} is over the maximum of {}.", rl.max_riser_mm));
        }
        if r < rl.min_riser_mm - 1e-6 {
            warnings.push(format!("The riser height {r:.1} is under the minimum of {}.", rl.min_riser_mm));
        }
        if tr < rl.min_tread_mm - 1e-6 {
            warnings.push(format!("The tread depth {tr} is under the minimum of {}.", rl.min_tread_mm));
        }
        if tr > rl.max_tread_mm + 1e-6 {
            warnings.push(format!("The tread depth {tr} is over the maximum of {}.", rl.max_tread_mm));
        }
        let blondel = 2.0 * r + tr;
        if blondel < rl.min_2r_t_mm - 1e-6 || blondel > rl.max_2r_t_mm + 1e-6 {
            warnings.push(format!("2R + T is {blondel:.1}, outside {} to {}.", rl.min_2r_t_mm, rl.max_2r_t_mm));
        }
        if rl.check_t_minus_r && tr - r < 120.0 - 1e-6 {
            warnings.push(format!("T - R is {:.1}, under 120.", tr - r));
        }
        if rl.check_r_plus_t && !(450.0 - 1e-6..=480.0 + 1e-6).contains(&(r + tr)) {
            warnings.push(format!("R + T is {:.1}, outside 450 to 480.", r + tr));
        }
        for (i, run) in s.runs.iter().enumerate() {
            if run.risers > rl.max_risers_per_run {
                warnings.push(format!("Run {} has {} risers; a flight may have at most {}.", i + 1, run.risers, rl.max_risers_per_run));
            }
            if run.risers < rl.min_risers_per_run {
                warnings.push(format!("Run {} has {} riser{}; a flight needs at least {}.", i + 1, run.risers, if run.risers == 1 { "" } else { "s" }, rl.min_risers_per_run));
            }
            if run.width_mm < rl.min_run_width_mm - 1e-6 {
                warnings.push(format!("Run {} is {} wide, under the minimum of {}.", i + 1, run.width_mm, rl.min_run_width_mm));
            }
        }
        if rl.check_headroom {
            for run in &runs {
                if let Some(h) = self.headroom(run).filter(|h| *h < rl.min_headroom_mm - 1e-6) {
                    warnings.push(format!("The headroom over run {} is {h:.0}, under {}.", run.index + 1, rl.min_headroom_mm));
                }
            }
        }
        Some(StairLayout { base_z, top_z, height, desired_risers: n, actual_risers: actual, riser_mm: r, tread_mm: tr, runs, landings, warnings })
    }

    fn check_stair(&self, s: &Stair) -> Result<(), String> {
        if self.stair_type(s.stair_type).is_none() { return Err("unknown stair type".into()); }
        let (b, t) = self.stair_heights(s).ok_or("unknown level")?;
        if !(b.is_finite() && t.is_finite() && s.tread_depth_mm.is_finite()) { return Err("the heights must be numbers".into()); }
        if t - b <= 0.0 { return Err("The top of the stair must be above its base.".into()); }
        if s.desired_risers < 1 { return Err("A stair needs at least one riser.".into()); }
        if s.tread_depth_mm <= 0.0 { return Err("The tread depth must be more than zero.".into()); }
        for run in &s.runs {
            if !(run.width_mm.is_finite() && run.width_mm > 0.0) { return Err("A run's width must be more than zero.".into()); }
            if run.risers < 1 { return Err("A run needs at least one riser.".into()); }
            if run.start.dist(&run.end) < 1e-6 { return Err("A run needs a direction: its start and end must differ.".into()); }
        }
        if s.landings.iter().any(|l| if l.outline.is_some() { l.after_run >= s.runs.len() } else { l.after_run + 1 >= s.runs.len() }) { return Err("A landing must stand after one of the stair's runs.".into()); }
        if s.landings.iter().any(|l| l.outline.as_ref().is_some_and(|o| o.len() < 3 || crate::geom::signed_area(o).abs() < 1000.0)) { return Err("A sketched landing needs a closed outline.".into()); }
        for run in &s.runs {
            if let RunShape::Sketched { risers, .. } = &run.shape {
                if risers.len() < 2 { return Err("A sketched run needs at least two riser lines.".into()); }
                if risers.len() as u32 != run.risers { return Err("A sketched run's riser count is its number of riser lines.".into()); }
            }
        }
        if s.multistory_top.is_some_and(|m| self.level(m).is_none()) { return Err("unknown multistory top level".into()); }
        Ok(())
    }

    /// Landings between every pair of runs that do not simply continue one another.
    fn automatic_landings(&self, s: &Stair) -> Vec<StairLanding> {
        let sketched: Vec<StairLanding> = s.landings.iter().filter(|l| l.outline.is_some() && l.after_run < s.runs.len()).cloned().collect();
        let mut probe = s.clone();
        probe.landings.clear();
        let Some(l) = self.stair_layout(&probe) else { return sketched };
        let mut out: Vec<StairLanding> = (0..l.runs.len().saturating_sub(1))
            .filter(|i| !sketched.iter().any(|x| x.after_run == *i) && landing_between(&l.runs[*i], &l.runs[i + 1]).is_some())
            .map(|i| StairLanding { kind: LandingKind::Automatic, after_run: i, outline: None }).collect();
        out.extend(sketched);
        out.sort_by_key(|x| x.after_run);
        out
    }

    /// A multistory stair's storeys: its own, then one per level up to its Multistory Top Level, each from one level to
    /// the next. A storey of another height keeps the risers' depth and the runs; its riser count changes (on the
    /// last run) only where the riser height would break the type's limits.
    pub fn stair_storeys(&self, s: &Stair) -> Vec<Stair> {
        let mut out = vec![s.clone()];
        let (Some(top), Some(mt), Some(t)) = (s.top_level, s.multistory_top, self.stair_type(s.stair_type)) else { return out };
        let mut levels: Vec<(Id, f64)> = self.levels().iter().map(|(i, l)| (*i, l.elevation_mm)).collect();
        levels.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (Some(from), Some(to)) = (levels.iter().position(|l| l.0 == top), levels.iter().position(|l| l.0 == mt)) else { return out };
        for k in from..to {
            let mut c = s.clone();
            c.base_level = levels[k].0;
            c.top_level = Some(levels[k + 1].0);
            c.landings.retain(|l| l.outline.is_none());
            let h = levels[k + 1].1 + c.top_offset_mm - levels[k].1 - c.base_offset_mm;
            if h <= 0.0 { break; }
            // unpinned: its own runs, risers and landings
            if let Some(o) = s.storeys.iter().find(|o| o.base_level == levels[k].0) {
                c.runs = o.runs.clone();
                c.landings = o.landings.clone();
                c.desired_risers = c.runs.iter().map(|r| r.risers).sum::<u32>().max(1);
                c.landings = self.automatic_landings(&c);
                out.push(c);
                continue;
            }
            let n = c.desired_risers.max(1);
            let r = h / n as f64;
            if r > t.rules.max_riser_mm + 1e-6 || r < t.rules.min_riser_mm - 1e-6 {
                let want = (h / t.rules.max_riser_mm - 1e-9).ceil().max(1.0) as i64;
                let mut change = want - n as i64;
                c.desired_risers = want as u32;
                let mut i = c.runs.len();
                while change != 0 && i > 0 {
                    i -= 1;
                    if matches!(c.runs[i].shape, RunShape::Sketched { .. }) { continue; }
                    let next = (c.runs[i].risers as i64 + change).max(1);
                    change -= next - c.runs[i].risers as i64;
                    c.runs[i].risers = next as u32;
                    if change > 0 { break; }
                }
            }
            out.push(c);
        }
        out
    }

    /// The storey of a multistory stair that starts at `level` (above its first).
    pub fn stair_storey(&self, s: &Stair, level: Id) -> Option<Stair> {
        self.stair_storeys(s).into_iter().skip(1).find(|c| c.base_level == level)
    }

    /// Unpin a storey from the stair below (it keeps the runs it has now, to change on their own), or pin it again.
    fn pin_storey(&self, s: &mut Stair, level: Id, pinned: bool) -> Result<(), String> {
        if pinned { s.storeys.retain(|o| o.base_level != level); return Ok(()); }
        if s.storeys.iter().any(|o| o.base_level == level) { return Ok(()); }
        let c = self.stair_storey(s, level).ok_or("The stair has no storey starting at that level.")?;
        s.storeys.push(StairStorey { base_level: level, runs: c.runs, landings: c.landings.into_iter().filter(|l| l.outline.is_some()).collect() });
        Ok(())
    }

    /// Pin or unpin one storey of a multistory stair, one undo step.
    pub fn set_stair_storey_pinned(&mut self, id: Id, level: Id, pinned: bool) -> Result<(), String> {
        self.refuse_pinned(&[id], "edited")?;
        let mut s = self.stair(id).cloned().ok_or("not a stair")?;
        self.pin_storey(&mut s, level, pinned)?;
        self.transact(if pinned { "Pin storey" } else { "Unpin storey" }, |tx| tx.update(id, Element::Stair(s)))
    }

    /// The runs (and sketched landings) of an unpinned storey, to change; an error when it is pinned.
    fn storey_mut<'a>(s: &'a mut Stair, level: Id) -> Result<&'a mut StairStorey, String> {
        s.storeys.iter_mut().find(|o| o.base_level == level).ok_or_else(|| "This storey repeats the stair below: unpin it to change it on its own.".to_string())
    }

    /// Check an unpinned storey as laid out.
    fn check_storey(&self, s: &Stair, level: Id) -> Result<(), String> {
        let c = self.stair_storey(s, level).ok_or("The stair has no storey starting at that level.")?;
        self.check_stair(&c)?;
        self.stair_layout(&c).map(|_| ()).ok_or_else(|| "The storey cannot be laid out.".to_string())
    }

    /// Every storey's layout (one for a stair that is not multistory).
    pub fn stair_layouts(&self, s: &Stair) -> Vec<StairLayout> {
        self.stair_storeys(s).iter().filter_map(|c| self.stair_layout(c)).collect()
    }

    /// The Stair tool's Finish: a stair with these runs (automatic landings between them), one undo step.
    pub fn add_stair(&mut self, mut s: Stair) -> Result<Id, String> {
        if s.runs.is_empty() { return Err("Draw at least one run.".into()); }
        s.landings = self.automatic_landings(&s);
        if s.ifc.guid.is_empty() { s.ifc.guid = crate::element::new_ifc_guid(); }
        self.check_stair(&s)?;
        self.transact("Create stair", |tx| Ok(tx.add(Element::Stair(s))))
    }

    /// Edit Stairs' Finish: the stair's runs drawn again, its landings worked out again, one undo step.
    pub fn set_stair_runs(&mut self, id: Id, runs: Vec<StairRun>) -> Result<(), String> {
        let keep: Vec<StairLanding> = self.stair(id).map(|s| s.landings.iter().filter(|l| l.outline.is_some()).cloned().collect()).unwrap_or_default();
        self.set_stair_components(id, runs, keep)
    }

    /// Edit Stairs' Finish with its sketched landings: runs and landings drawn again, one undo step.
    pub fn set_stair_components(&mut self, id: Id, runs: Vec<StairRun>, sketched: Vec<StairLanding>) -> Result<(), String> {
        self.set_stair_storey_components(id, None, runs, sketched)
    }

    /// Edit Stairs' Finish on the stair, or (`storey`: the level it starts from) on an unpinned storey.
    pub fn set_stair_storey_components(&mut self, id: Id, storey: Option<Id>, runs: Vec<StairRun>, sketched: Vec<StairLanding>) -> Result<(), String> {
        self.refuse_pinned(&[id], "edited")?;
        let mut s = self.stair(id).cloned().ok_or("not a stair")?;
        if runs.is_empty() { return Err("A stair needs at least one run.".into()); }
        if let Some(level) = storey.filter(|l| *l != s.base_level) {
            let o = Self::storey_mut(&mut s, level)?;
            o.runs = runs;
            o.landings = sketched.into_iter().filter(|l| l.outline.is_some()).collect();
            self.check_storey(&s, level)?;
            return self.transact("Edit stair", |tx| tx.update(id, Element::Stair(s)));
        }
        s.runs = runs;
        s.landings = sketched.into_iter().filter(|l| l.outline.is_some()).collect();
        s.landings = self.automatic_landings(&s);
        self.check_stair(&s)?;
        self.transact("Edit stair", |tx| tx.update(id, Element::Stair(s)))
    }

    /// Properties of a stair. A change to the desired riser count takes risers off, or adds them to, the last run;
    /// a run width set on the stair applies to every run.
    pub fn patch_stair(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.stair(id).cloned().ok_or("not a stair")?;
        let mut patch = patch.clone();
        let width = patch.as_object_mut().and_then(|o| o.remove("run_width_mm")).and_then(|v| v.as_f64());
        // { storey_pinned: { "<level id>": bool } }
        let pins = patch.as_object_mut().and_then(|o| o.remove("storey_pinned"));
        let mut s: Stair = crate::doc::apply_patch(&old, &patch)?;
        if let Some(serde_json::Value::Object(m)) = pins {
            if old.pinned { return Err("Unpin the stair before changing it.".into()); }
            for (k, v) in m {
                let level: Id = k.parse().map_err(|_| "bad storey level")?;
                self.pin_storey(&mut s, level, v.as_bool().unwrap_or(true))?;
            }
        }
        if let Some(w) = width {
            for r in &mut s.runs { r.width_mm = w; }
        }
        if s.desired_risers != old.desired_risers && !s.runs.is_empty() {
            let have: i64 = s.runs.iter().map(|r| r.risers as i64).sum();
            let mut change = s.desired_risers as i64 - old.desired_risers as i64;
            // only as far as the runs reached the old count: a stair short of its top stays short by as much
            if change > 0 && have < old.desired_risers as i64 { change = 0; }
            let mut k = s.runs.len();
            while change != 0 && k > 0 {
                k -= 1;
                let r = &mut s.runs[k];
                let next = (r.risers as i64 + change).max(1);
                change -= next - r.risers as i64;
                r.risers = next as u32;
                if change > 0 { break; }
            }
        }
        let moved = s.base_level != old.base_level || s.top_level != old.top_level || s.base_offset_mm != old.base_offset_mm || s.top_offset_mm != old.top_offset_mm || s.runs != old.runs;
        if old.pinned && s.pinned && moved {
            return Err("Unpin the stair before changing it.".into());
        }
        s.landings = self.automatic_landings(&s);
        self.check_stair(&s)?;
        self.transact("Stair properties", |tx| tx.update(id, Element::Stair(s)))
    }

    pub fn patch_stair_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.stair_type(id).cloned().ok_or("not a stair type")?;
        let t: StairType = crate::doc::apply_patch(&old, patch)?;
        if t.name.trim().is_empty() { return Err("a type needs a name".into()); }
        if self.stair_types().iter().any(|(i, o)| *i != id && o.name == t.name) { return Err(format!("a stair type named '{}' already exists", t.name)); }
        let r = &t.rules;
        let nums = [r.max_riser_mm, r.min_riser_mm, r.min_tread_mm, r.max_tread_mm, r.min_run_width_mm, r.min_2r_t_mm, r.max_2r_t_mm, t.run.structural_depth_mm, t.run.tread_thickness_mm, t.run.nosing_mm, t.run.riser_thickness_mm, t.landing.thickness_mm, t.cut_mark.angle_deg];
        if nums.iter().any(|v| !v.is_finite() || *v < 0.0) { return Err("the type's sizes must be numbers, zero or more".into()); }
        if r.max_riser_mm <= 0.0 || r.min_riser_mm > r.max_riser_mm { return Err("the riser limits do not leave any riser height".into()); }
        if r.min_tread_mm > r.max_tread_mm { return Err("the tread limits do not leave any tread depth".into()); }
        if r.min_2r_t_mm > r.max_2r_t_mm { return Err("the 2R + T limits are the wrong way round".into()); }
        if t.landing.thickness_mm <= 0.0 { return Err("a landing must have some thickness".into()); }
        self.transact("Change stair type", |tx| tx.update(id, Element::StairType(t)))
    }

    pub fn duplicate_stair_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let mut t = self.stair_type(id).cloned().ok_or("not a stair type")?;
        if name.trim().is_empty() { return Err("a type needs a name".into()); }
        if self.stair_types().iter().any(|(_, o)| o.name == name) { return Err(format!("a stair type named '{name}' already exists")); }
        t.name = name.into();
        self.transact("Duplicate type", |tx| Ok(tx.add(Element::StairType(t))))
    }

    /// The stair's parts as convex solids, with the material each is made of.
    pub fn stair_pieces(&self, s: &Stair) -> Vec<StairPiece> {
        let mut out = vec![];
        for (c, st) in self.stair_storeys(s).iter().enumerate() {
            let off = c * 100;
            for mut p in self.storey_pieces(st) {
                p.part = match p.part {
                    StairPart::Run(i) => StairPart::Run(i + off),
                    StairPart::Support(i) => StairPart::Support(i + off),
                    StairPart::Tread(i) => StairPart::Tread(i + off),
                    StairPart::Riser(i) => StairPart::Riser(i + off),
                    StairPart::Landing(i) => StairPart::Landing(i + off),
                };
                out.push(p);
            }
        }
        out
    }

    /// One storey's parts.
    fn storey_pieces(&self, s: &Stair) -> Vec<StairPiece> {
        let (Some(t), Some(l)) = (self.stair_type(s.stair_type), self.stair_layout(s)) else { return vec![] };
        let mut out = vec![];
        let rc = &t.run;
        let mat = |m: &str| if m.is_empty() { None } else { Some(m.to_string()) };
        for run in &l.runs {
            let w = run.width / 2.0;
            let on_landing = l.landings.iter().any(|x| x.after_run + 1 == run.index);
            let floor = run.z0 - if on_landing { t.landing.thickness_mm } else { 0.0 };
            if let Some(path) = &run.path {
                self.path_pieces(run, path, &l, t, floor, &mut out);
                continue;
            }
            if rc.monolithic {
                // one column under each tread, down to the soffit: a plane square to the pitch, the structural depth
                // under the nosing line
                let pitch = if l.tread_mm > 0.0 { l.riser_mm / l.tread_mm } else { 0.0 };
                let drop = rc.structural_depth_mm * (1.0 + pitch * pitch).sqrt();
                let soffit = |sv: f64| (run.nosing_z(sv) - drop).max(floor);
                for &(s0, s1, top) in &run.treads {
                    let (b0, b1) = (soffit(s0), soffit(s1));
                    if top - b0.min(b1) < 1e-3 { continue; }
                    let profile = vec![[s0, b0.min(top)], [s1, b1.min(top)], [s1, top], [s0, top]];
                    out.push(StairPiece { part: StairPart::Run(run.index), material: mat(&rc.material), solid: Convex::across(run, &profile, -w, w) });
                }
            } else {
                for (s0, s1, top) in run.treads.iter().filter(|_| rc.treads) {
                    let front = if run.risers.iter().any(|(rs, _)| (rs - s0).abs() < 1e-6) { s0 - rc.nosing_mm } else { *s0 };
                    let profile = vec![[front, top - rc.tread_thickness_mm], [*s1, top - rc.tread_thickness_mm], [*s1, *top], [front, *top]];
                    out.push(StairPiece { part: StairPart::Tread(run.index), material: mat(&rc.tread_material), solid: Convex::across(run, &profile, -w, w) });
                }
                if rc.risers {
                    for (k, (rs, top)) in run.risers.iter().enumerate() {
                        let below = if k == 0 { run.z0 } else { run.risers[k - 1].1 };
                        let hi = top - if rc.treads && *rs < run.length - 1e-6 { rc.tread_thickness_mm } else { 0.0 };
                        if hi - below < 1e-3 { continue; }
                        let profile = vec![[*rs, below], [rs + rc.riser_thickness_mm, below], [rs + rc.riser_thickness_mm, hi], [*rs, hi]];
                        out.push(StairPiece { part: StairPart::Riser(run.index), material: mat(&rc.riser_material), solid: Convex::across(run, &profile, -w, w) });
                    }
                }
            }
        }
        // supports: stringers beside the run, carriages under its treads
        let sp = &t.supports;
        let pitch = if l.tread_mm > 0.0 { l.riser_mm / l.tread_mm } else { 0.0 };
        let slope = (1.0 + pitch * pitch).sqrt();
        for run in &l.runs {
            let w = run.width / 2.0;
            let on_landing = l.landings.iter().any(|x| x.after_run + 1 == run.index);
            let floor = run.z0 - if on_landing { t.landing.thickness_mm } else { 0.0 };
            if let Some(path) = &run.path {
                path_supports(run, path, l.riser_mm, l.tread_mm, t, floor, &mut out);
                continue;
            }
            let above_floor = |c: Convex| c.clip(&|p| p[2], floor, true);
            let under = if rc.treads && !rc.monolithic { rc.tread_thickness_mm } else { 0.0 };
            // a carriage between `t0` and `t1` across the run: a column under each tread down to its soffit
            let carriage = |s: &Support, t0: f64, t1: f64, out: &mut Vec<StairPiece>| {
                let drop = s.total_depth_mm * slope;
                for &(s0, s1, top) in &run.treads {
                    let (b0, b1) = (run.nosing_z(s0) - under - drop, run.nosing_z(s1) - under - drop);
                    let top = top - under;
                    if top - b0.min(b1) < 1.0 { continue; }
                    let profile = vec![[s0, b0.min(top)], [s1, b1.min(top)], [s1, top], [s0, top]];
                    if let Some(c) = above_floor(Convex::across(run, &profile, t0, t1)) {
                        out.push(StairPiece { part: StairPart::Support(run.index), material: mat(&s.material), solid: c });
                    }
                }
            };
            for (side, s) in [(1.0, &sp.left), (-1.0, &sp.right)] {
                let Some(s) = s else { continue };
                match s.kind {
                    SupportKind::Stringer => {
                        let (a, b) = (w + s.offset_mm, w + s.offset_mm + s.width_mm);
                        let (t0, t1) = if side > 0.0 { (a, b) } else { (-b, -a) };
                        let top = |sv: f64| run.nosing_z(sv) + s.above_nosing_mm;
                        let drop = s.total_depth_mm * slope;
                        let profile = vec![[0.0, top(0.0) - drop], [run.length, top(run.length) - drop], [run.length, top(run.length)], [0.0, top(0.0)]];
                        if let Some(c) = above_floor(Convex::across(run, &profile, t0, t1)) {
                            out.push(StairPiece { part: StairPart::Support(run.index), material: mat(&s.material), solid: c });
                        }
                    }
                    SupportKind::Carriage => {
                        let (a, b) = (w - s.offset_mm - s.width_mm, w - s.offset_mm);
                        let (t0, t1) = if side > 0.0 { (a, b) } else { (-b, -a) };
                        carriage(s, t0, t1, &mut out);
                    }
                }
            }
            if let Some(m) = &sp.middle {
                let n = sp.middle_count.max(1);
                for k in 0..n {
                    let c = -w + run.width * (k + 1) as f64 / (n + 1) as f64;
                    carriage(m, c - m.width_mm / 2.0, c + m.width_mm / 2.0, &mut out);
                }
            }
        }
        for g in &l.landings {
            out.push(StairPiece { part: StairPart::Landing(g.index), material: mat(&t.landing.material), solid: Convex::prism(&g.outline, g.z - g.thickness, g.z) });
        }
        out
    }

    /// A spiral's, winder's or sketched run's parts: a column under each tread (monolithic) down to a soffit the
    /// structural depth under the nosing line, or each tread (with its nosing) and riser.
    fn path_pieces(&self, run: &RunLayout, path: &crate::runpath::RunPath, l: &StairLayout, t: &StairType, floor: f64, out: &mut Vec<StairPiece>) {
        let rc = &t.run;
        let mat = |m: &str| if m.is_empty() { None } else { Some(m.to_string()) };
        let pitch = if l.tread_mm > 0.0 { l.riser_mm / l.tread_mm } else { 0.0 };
        let drop = rc.structural_depth_mm * (1.0 + pitch * pitch).sqrt();
        let mut hulls: Vec<StairPiece> = vec![];
        let mut add = |part: StairPart, m: Option<String>, poly: &[Point], z0: f64, z1: f64| {
            if z1 - z0 < 1e-3 { return; }
            for p in convex_parts(poly) {
                out.push(StairPiece { part, material: m.clone(), solid: Convex::prism(&p, z0, z1) });
            }
        };
        for (poly, top) in &path.treads {
            if rc.monolithic {
                // the soffit follows the walkline's pitch: each corner's height from where it lies along the run
                for part in convex_parts(poly) {
                    let cloud: Vec<[f64; 3]> = part.iter().flat_map(|p| {
                        let lo = (run.nosing_z(path.s_at(*p)) - drop).max(floor).min(*top);
                        [[p.x, p.y, *top], [p.x, p.y, lo]]
                    }).collect();
                    if let Some(c) = Convex::hull_of(&cloud) {
                        hulls.push(StairPiece { part: StairPart::Run(run.index), material: mat(&rc.material), solid: c });
                    }
                }
            } else if rc.treads {
                add(StairPart::Tread(run.index), mat(&rc.tread_material), poly, top - rc.tread_thickness_mm, *top);
            }
        }
        // each tread's nosing past the riser below it
        if !rc.monolithic && rc.treads && rc.nosing_mm > 0.0 {
            for (k, [a, b]) in path.riser_lines.iter().enumerate() {
                let rs = run.risers[k].0;
                let Some(&(_, _, top)) = run.treads.iter().find(|x| (x.0 - rs).abs() < 1e-6) else { continue };
                let d = down_run(*a, *b, rc.nosing_mm);
                add(StairPart::Tread(run.index), mat(&rc.tread_material), &[*a, *b, add_p(*b, d), add_p(*a, d)], top - rc.tread_thickness_mm, top);
            }
        }
        if !rc.monolithic && rc.risers {
            for (k, [a, b]) in path.riser_lines.iter().enumerate() {
                let (_, top) = run.risers[k];
                let below = if k == 0 { run.z0 } else { run.risers[k - 1].1 };
                let hi = top - if rc.treads && run.risers[k].0 < run.length - 1e-6 { rc.tread_thickness_mm } else { 0.0 };
                let d = mul(down_run(*a, *b, rc.riser_thickness_mm), -1.0);
                add(StairPart::Riser(run.index), mat(&rc.riser_material), &[*a, *b, add_p(*b, d), add_p(*a, d)], below, hi);
            }
        }
        out.extend(hulls);
    }

    /// Change one run (its location line, width, risers, Begins and Ends with Riser), one undo step.
    pub fn patch_stair_run(&mut self, id: Id, run: usize, patch: &serde_json::Value) -> Result<(), String> {
        self.patch_stair_storey_run(id, None, run, patch)
    }

    /// Change one run of the stair, or (`storey`: the level it starts from) of an unpinned storey.
    pub fn patch_stair_storey_run(&mut self, id: Id, storey: Option<Id>, run: usize, patch: &serde_json::Value) -> Result<(), String> {
        self.refuse_pinned(&[id], "edited")?;
        let mut s = self.stair(id).cloned().ok_or("not a stair")?;
        if let Some(level) = storey.filter(|l| *l != s.base_level) {
            let o = Self::storey_mut(&mut s, level)?;
            let old = o.runs.get(run).cloned().ok_or("no such run")?;
            o.runs[run] = crate::doc::apply_patch(&old, patch)?;
            self.check_storey(&s, level)?;
            return self.transact("Stair run properties", |tx| tx.update(id, Element::Stair(s)));
        }
        let old = s.runs.get(run).cloned().ok_or("no such run")?;
        s.runs[run] = crate::doc::apply_patch(&old, patch)?;
        s.landings = self.automatic_landings(&s);
        self.check_stair(&s)?;
        self.transact("Stair run properties", |tx| tx.update(id, Element::Stair(s)))
    }

    /// The least clear height over a run's nosings to the floors and ceilings above it, where one is lower than the
    /// type allows.
    fn headroom(&self, run: &RunLayout) -> Option<f64> {
        let mut worst: Option<f64> = None;
        let w = run.width / 2.0 - 50.0;
        let mut overhead: Vec<(Vec<crate::geom::Region>, f64, crate::element::Floor)> = vec![];
        for (_, f) in self.iter().filter_map(|(i, e)| if let Element::Floor(f) = e { Some((i, f)) } else { None }) {
            let t = self.floor_type(f.floor_type).map_or(0.0, |t| t.thickness_mm());
            overhead.push((f.regions(0.5), t, f.clone()));
        }
        for (_, c) in self.ceilings() {
            let t = self.ceiling_thickness(c);
            overhead.push((c.regions(0.5), t, self.ceiling_slab(c)));
        }
        let probes: Vec<(Point, f64)> = match &run.path {
            Some(path) => path.riser_lines.iter().zip(&run.risers).flat_map(|([a, b], (_, z))| {
                let m = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                [(Point::new(a.x + (m.x - a.x) * 0.1, a.y + (m.y - a.y) * 0.1), *z), (m, *z), (Point::new(b.x + (m.x - b.x) * 0.1, b.y + (m.y - b.y) * 0.1), *z)]
            }).collect(),
            None => run.risers.iter().flat_map(|(s, z)| [-w, 0.0, w].map(|t| (run.at(*s, t), *z))).collect(),
        };
        for (p, z) in &probes {
            {
                let p = *p;
                for (regions, thick, slab) in &overhead {
                    if !regions.iter().any(|r| crate::rooms::contains(r, p)) { continue; }
                    let Some(surf) = self.floor_surface(slab) else { continue };
                    let under = surf.z(p) - thick;
                    if under > z + 1.0 {
                        let h = under - z;
                        worst = Some(worst.map_or(h, |x: f64| x.min(h)));
                    }
                }
            }
        }
        worst
    }

    /// The 3D view's meshes: one per material, the pieces made of it together.
    pub fn stair_meshes(&self, s: &Stair) -> Vec<(Option<String>, Vec<f32>)> {
        let mut by: Vec<(Option<String>, Vec<f32>)> = vec![];
        for p in self.stair_pieces(s) {
            let k = match by.iter().position(|(m, _)| *m == p.material) {
                Some(k) => k,
                None => { by.push((p.material.clone(), vec![])); by.len() - 1 }
            };
            p.solid.mesh(&mut by[k].1);
        }
        by
    }

    /// Elevations and sections: each piece as seen, the hull of its corners in the view (horizontal `u`, height),
    /// at the depth of its nearest corner, and for a section what its plane cuts. `d` is depth into the view.
    pub(crate) fn stair_view_items(&self, id: Id, s: &Stair, u: &dyn Fn(Point) -> f64, d: &dyn Fn(Point) -> f64, near: f64, far: f64, cut: Option<f64>) -> Vec<crate::site::ElevItem> {
        let pieces: Vec<(Option<String>, Convex)> = self.stair_pieces(s).into_iter().map(|p| (p.material, p.solid)).collect();
        self.convex_view_items(id, "Stairs", &pieces, u, d, near, far, cut)
    }

    /// Convex solids in an elevation or section: each as seen (the hull of its corners in the view, at the depth of its
    /// nearest corner), or for a section's `cut` plane, what the plane cuts.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convex_view_items(&self, id: Id, category: &'static str, pieces: &[(Option<String>, Convex)], u: &dyn Fn(Point) -> f64, d: &dyn Fn(Point) -> f64, near: f64, far: f64, cut: Option<f64>) -> Vec<crate::site::ElevItem> {
        let mut out = vec![];
        let fill = |m: &Option<String>| m.as_deref().and_then(|m| self.material(m)).map_or([190, 186, 178], |m| m.graphics.shading_color);
        let dd = |p: [f64; 3]| d(Point::new(p[0], p[1]));
        for (material, solid) in pieces {
            if let Some(plane) = cut {
                let cutp: Vec<[f64; 2]> = solid.section(&dd, plane).iter().map(|q| [u(Point::new(q[0], q[1])), q[2]]).collect();
                let hull = crate::site::convex_hull_2d(cutp);
                if hull.len() >= 3 {
                    out.push(crate::site::ElevItem { id, category, depth: -1.0 + 0.004, polys: vec![hull], fill: fill(material), cut: true, material: material.clone(), ..Default::default() });
                }
                continue;
            }
            // what lies beyond the near plane (a section's cut plane, or the elevation's own) and before the far clip
            let Some(seen) = solid.clip(&dd, near, true).and_then(|c| c.clip(&dd, far, false)) else { continue };
            let pts: Vec<[f64; 2]> = seen.pts.iter().map(|q| [u(Point::new(q[0], q[1])), q[2]]).collect();
            let depth = seen.pts.iter().map(|q| dd(*q)).fold(f64::INFINITY, f64::min);
            let hull = crate::site::convex_hull_2d(pts);
            if hull.len() >= 3 {
                out.push(crate::site::ElevItem { id, category, depth, polys: vec![hull], fill: fill(material), stroke: true, ..Default::default() });
            }
        }
        out
    }

    /// The plan outline of each run and landing, for picking and for what a plan hides under the stair.
    pub fn stair_outlines(&self, s: &Stair) -> Vec<Vec<Point>> {
        let Some(l) = self.stair_layout(s) else { return vec![] };
        l.runs.iter().map(|r| r.footprint()).chain(l.landings.iter().map(|g| g.outline.clone())).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StairPart {
    /// A monolithic run's flight.
    Run(usize),
    /// A stringer or carriage of a run.
    Support(usize),
    Tread(usize),
    Riser(usize),
    Landing(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StairPiece {
    pub part: StairPart,
    pub material: Option<String>,
    pub solid: Convex,
}

/// A convex solid: its corners and its faces (each a list of corners, outward facing, counter-clockwise from outside).
#[derive(Debug, Clone, PartialEq)]
pub struct Convex {
    pub pts: Vec<[f64; 3]>,
    pub faces: Vec<Vec<usize>>,
}

fn v3sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn v3cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn v3dot(a: [f64; 3], b: [f64; 3]) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }

impl Convex {
    /// A plan polygon extruded between two heights.
    pub fn prism(poly: &[Point], z0: f64, z1: f64) -> Convex {
        let mut ring = poly.to_vec();
        if crate::geom::signed_area(&ring) < 0.0 { ring.reverse(); }
        let n = ring.len();
        let pts: Vec<[f64; 3]> = ring.iter().map(|p| [p.x, p.y, z0]).chain(ring.iter().map(|p| [p.x, p.y, z1])).collect();
        let mut faces = vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
        for i in 0..n {
            let j = (i + 1) % n;
            faces.push(vec![i, j, n + j, n + i]);
        }
        Convex { pts, faces }.oriented()
    }

    /// A profile in a run's vertical plane (along the run, height) swept across it from `t0` to `t1` (left of the
    /// centre line).
    pub fn across(run: &RunLayout, profile: &[[f64; 2]], t0: f64, t1: f64) -> Convex {
        let n = profile.len();
        let p3 = |q: [f64; 2], t: f64| { let p = run.at(q[0], t); [p.x, p.y, q[1]] };
        let pts: Vec<[f64; 3]> = profile.iter().map(|q| p3(*q, t0)).chain(profile.iter().map(|q| p3(*q, t1))).collect();
        let mut faces = vec![(0..n).collect::<Vec<_>>(), (n..2 * n).rev().collect()];
        for i in 0..n {
            let j = (i + 1) % n;
            faces.push(vec![i, n + i, n + j, j]);
        }
        Convex { pts, faces }.oriented()
    }

    /// A straight member from `a` to `b` whose cross-section is `profile`: points across it (horizontal, square to the
    /// member) and up (square to both).
    pub fn beam(a: [f64; 3], b: [f64; 3], profile: &[[f64; 2]]) -> Convex {
        let ax = v3sub(b, a);
        let l = v3dot(ax, ax).sqrt().max(1e-9);
        let ax = [ax[0] / l, ax[1] / l, ax[2] / l];
        let h = (ax[0] * ax[0] + ax[1] * ax[1]).sqrt();
        let across = if h > 1e-6 { [-ax[1] / h, ax[0] / h, 0.0] } else { [1.0, 0.0, 0.0] };
        let up = v3cross(across, ax);
        let n = profile.len();
        let at = |o: [f64; 3], q: [f64; 2]| [o[0] + across[0] * q[0] + up[0] * q[1], o[1] + across[1] * q[0] + up[1] * q[1], o[2] + across[2] * q[0] + up[2] * q[1]];
        let pts: Vec<[f64; 3]> = profile.iter().map(|q| at(a, *q)).chain(profile.iter().map(|q| at(b, *q))).collect();
        let mut faces = vec![(0..n).collect::<Vec<_>>(), (n..2 * n).rev().collect()];
        for i in 0..n {
            let j = (i + 1) % n;
            faces.push(vec![i, n + i, n + j, j]);
        }
        Convex { pts, faces }.oriented()
    }

    /// The convex hull of a cloud of points (mm). None when they are flat or too few.
    pub fn hull_of(cloud: &[[f64; 3]]) -> Option<Convex> {
        let mut pts: Vec<[f64; 3]> = vec![];
        for q in cloud {
            if !pts.iter().any(|p| v3dot(v3sub(*p, *q), v3sub(*p, *q)) < 0.01) { pts.push(*q); }
        }
        let n = pts.len();
        if n < 4 { return None; }
        let tol = 1e-3;
        let mut planes: Vec<([f64; 3], f64)> = vec![];
        let mut faces: Vec<Vec<usize>> = vec![];
        for i in 0..n {
            for j in i + 1..n {
                for k in j + 1..n {
                    let c = v3cross(v3sub(pts[j], pts[i]), v3sub(pts[k], pts[i]));
                    let l = v3dot(c, c).sqrt();
                    if l < 1.0 { continue; }
                    let mut nrm = [c[0] / l, c[1] / l, c[2] / l];
                    let mut d = v3dot(nrm, pts[i]);
                    let (mut above, mut below) = (false, false);
                    for q in &pts {
                        let v = v3dot(nrm, *q) - d;
                        if v > tol { above = true; } else if v < -tol { below = true; }
                    }
                    if above && below { continue; }
                    // outward: every point on or below the plane
                    if above { nrm = [-nrm[0], -nrm[1], -nrm[2]]; d = -d; }
                    if planes.iter().any(|(m, e)| v3dot(*m, nrm) > 1.0 - 1e-9 && (e - d).abs() < tol) { continue; }
                    planes.push((nrm, d));
                    // the points on it, in order round their centre
                    let on: Vec<usize> = (0..n).filter(|&q| (v3dot(nrm, pts[q]) - d).abs() <= tol).collect();
                    let c = on.iter().fold([0.0; 3], |a, q| [a[0] + pts[*q][0], a[1] + pts[*q][1], a[2] + pts[*q][2]]);
                    let c = [c[0] / on.len() as f64, c[1] / on.len() as f64, c[2] / on.len() as f64];
                    let e1 = v3sub(pts[on[0]], c);
                    let e2 = v3cross(nrm, e1);
                    let mut ring = on;
                    ring.sort_by(|a, b| {
                        let ang = |q: usize| { let v = v3sub(pts[q], c); v3dot(v, e2).atan2(v3dot(v, e1)) };
                        ang(*a).total_cmp(&ang(*b))
                    });
                    faces.push(ring);
                }
            }
        }
        if faces.len() < 4 { return None; }
        Some(Convex { pts, faces }.oriented())
    }

    /// The solid seen from above: the hull of its corners in plan.
    pub fn plan_hull(&self) -> Vec<Point> {
        hull(self.pts.iter().map(|p| Point::new(p[0], p[1])).collect())
    }

    fn centroid(&self) -> [f64; 3] {
        let k = self.pts.len().max(1) as f64;
        let s = self.pts.iter().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
        [s[0] / k, s[1] / k, s[2] / k]
    }

    fn normal(&self, f: &[usize]) -> [f64; 3] {
        // Newell's method: right for any planar polygon
        let mut n = [0.0; 3];
        for i in 0..f.len() {
            let (a, b) = (self.pts[f[i]], self.pts[f[(i + 1) % f.len()]]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let l = v3dot(n, n).sqrt();
        if l > 1e-12 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0, 0.0, 0.0] }
    }

    /// Every face turned to face outward, and degenerate faces dropped.
    fn oriented(mut self) -> Convex {
        let c = self.centroid();
        let mut faces = std::mem::take(&mut self.faces);
        faces.retain(|f| f.len() >= 3);
        for f in faces.iter_mut() {
            f.dedup_by(|a, b| { let (p, q) = (self.pts[*a], self.pts[*b]); v3dot(v3sub(p, q), v3sub(p, q)) < 1e-12 });
            let n = self.normal(f);
            let fc = f.iter().fold([0.0; 3], |a, i| { let p = self.pts[*i]; [a[0] + p[0], a[1] + p[1], a[2] + p[2]] });
            let k = f.len() as f64;
            if v3dot(n, v3sub([fc[0] / k, fc[1] / k, fc[2] / k], c)) < 0.0 { f.reverse(); }
        }
        faces.retain(|f| f.len() >= 3 && v3dot(self.normal(f), self.normal(f)) > 0.5);
        self.faces = faces;
        self
    }

    /// The part on one side of a plane: where `f` is at least `at` (`keep_above`) or at most. None when nothing is left.
    pub fn clip(&self, f: &dyn Fn([f64; 3]) -> f64, at: f64, keep_above: bool) -> Option<Convex> {
        let inside = |v: f64| if keep_above { v >= at - 1e-9 } else { v <= at + 1e-9 };
        if self.pts.iter().all(|p| inside(f(*p))) { return Some(self.clone()); }
        let mut pts: Vec<[f64; 3]> = vec![];
        let mut faces: Vec<Vec<usize>> = vec![];
        let mut cap: Vec<[f64; 3]> = vec![];
        let idx = |p: [f64; 3], pts: &mut Vec<[f64; 3]>| -> usize {
            if let Some(i) = pts.iter().position(|q| v3dot(v3sub(*q, p), v3sub(*q, p)) < 1e-10) { return i; }
            pts.push(p);
            pts.len() - 1
        };
        for face in &self.faces {
            let mut out = vec![];
            for i in 0..face.len() {
                let (a, b) = (self.pts[face[i]], self.pts[face[(i + 1) % face.len()]]);
                let (fa, fb) = (f(a), f(b));
                if inside(fa) { out.push(idx(a, &mut pts)); }
                if inside(fa) != inside(fb) {
                    let k = (at - fa) / (fb - fa);
                    let p = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
                    out.push(idx(p, &mut pts));
                    cap.push(p);
                }
            }
            if out.len() >= 3 { faces.push(out); }
        }
        if faces.is_empty() { return None; }
        // the new face where the plane cut: its points in order round their centre
        if cap.len() >= 3 {
            let c = cap.iter().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
            let c = [c[0] / cap.len() as f64, c[1] / cap.len() as f64, c[2] / cap.len() as f64];
            let n = v3cross(v3sub(cap[0], c), v3sub(cap[1], c));
            let (e1, e2) = if v3dot(n, n) > 1e-12 {
                let e1 = v3sub(cap[0], c);
                (e1, v3cross(n, e1))
            } else {
                (v3sub(cap[0], c), v3sub(cap[2], c))
            };
            let mut ring: Vec<usize> = cap.iter().map(|p| idx(*p, &mut pts)).collect();
            ring.sort_by(|a, b| {
                let ang = |i: usize| { let d = v3sub(pts[i], c); v3dot(d, e2).atan2(v3dot(d, e1)) };
                ang(*a).total_cmp(&ang(*b))
            });
            ring.dedup();
            if ring.len() >= 3 { faces.push(ring); }
        }
        Some(Convex { pts, faces }.oriented())
    }

    /// Where a plane `f = at` passes through the solid: the corners of the cut, unordered.
    pub fn section(&self, f: &dyn Fn([f64; 3]) -> f64, at: f64) -> Vec<[f64; 3]> {
        let mut out = vec![];
        for face in &self.faces {
            for i in 0..face.len() {
                let (a, b) = (self.pts[face[i]], self.pts[face[(i + 1) % face.len()]]);
                let (fa, fb) = (f(a) - at, f(b) - at);
                if (fa < 0.0) != (fb < 0.0) {
                    let k = fa / (fa - fb);
                    out.push([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]);
                }
            }
        }
        out
    }

    /// Triangles for display, interleaved `x y z nx ny nz` in metres as the other meshes are.
    pub fn mesh(&self, out: &mut Vec<f32>) {
        for f in &self.faces {
            let n = self.normal(f);
            for k in 1..f.len() - 1 {
                for i in [f[0], f[k], f[k + 1]] {
                    let p = self.pts[i];
                    out.extend_from_slice(&[(p[0] * 0.001) as f32, (p[1] * 0.001) as f32, (p[2] * 0.001) as f32, n[0] as f32, n[1] as f32, n[2] as f32]);
                }
            }
        }
    }

    pub fn z_range(&self) -> (f64, f64) {
        self.pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[2]), b.max(p[2])))
    }

    /// The solid's volume (m³ come from dividing by 1e9).
    pub fn volume(&self) -> f64 {
        let c = self.centroid();
        self.faces.iter().map(|f| {
            (1..f.len() - 1).map(|k| {
                let (a, b, d) = (v3sub(self.pts[f[0]], c), v3sub(self.pts[f[k]], c), v3sub(self.pts[f[k + 1]], c));
                v3dot(a, v3cross(b, d)) / 6.0
            }).sum::<f64>()
        }).sum::<f64>().abs()
    }
}

pub(crate) fn starter_stair_types() -> Vec<StairType> {
    vec![
        StairType {
            name: "Concrete Stair".into(),
            rules: StairRules::default(),
            run: RunConstruction { monolithic: true, structural_depth_mm: 150.0, material: "Concrete".into(), treads: false, tread_thickness_mm: 0.0, nosing_mm: 0.0, tread_material: String::new(), risers: false, riser_thickness_mm: 0.0, riser_material: String::new(), slanted_risers: false },
            landing: LandingConstruction { monolithic: true, thickness_mm: 200.0, material: "Concrete".into() },
            cut_mark: CutMark::default(),
            supports: Supports::default(),
            function: StairFunction::Interior,
            info: TypeInfo::default(),
        },
        StairType {
            name: "Timber Stair".into(),
            rules: StairRules::default(),
            run: RunConstruction { monolithic: false, structural_depth_mm: 0.0, material: String::new(), treads: true, tread_thickness_mm: 40.0, nosing_mm: 25.0, tread_material: "Timber Flooring".into(), risers: true, riser_thickness_mm: 19.0, riser_material: "Timber Flooring".into(), slanted_risers: false },
            landing: LandingConstruction { monolithic: false, thickness_mm: 200.0, material: "Timber Flooring".into() },
            cut_mark: CutMark::default(),
            supports: Supports {
                left: Some(Support { kind: SupportKind::Stringer, width_mm: 45.0, total_depth_mm: 290.0, above_nosing_mm: 50.0, offset_mm: 0.0, material: "Timber Joist".into() }),
                right: Some(Support { kind: SupportKind::Stringer, width_mm: 45.0, total_depth_mm: 290.0, above_nosing_mm: 50.0, offset_mm: 0.0, material: "Timber Joist".into() }),
                middle: None,
                middle_count: 1,
            },
            function: StairFunction::Interior,
            info: TypeInfo::default(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> (Document, Id, Id) {
        let mut d = Document::new_project();
        d.ensure_stair_types().unwrap();
        let mut levels: Vec<(Id, f64)> = d.levels().iter().map(|(i, l)| (*i, l.elevation_mm)).collect();
        levels.sort_by(|a, b| a.1.total_cmp(&b.1));
        (d, levels[0].0, levels[1].0)
    }

    #[test]
    fn risers_balance_to_the_height_and_runs_take_their_share() {
        let (mut d, l1, l2) = doc();
        let ty = d.stair_types()[0].0;
        let mut s = d.new_stair(ty, l1, Some(l2)).unwrap();
        let (b, t) = d.stair_heights(&s).unwrap();
        let h = t - b;
        assert_eq!(s.desired_risers, (h / 190.0).ceil() as u32);
        let n = s.desired_risers;
        // two runs of half the risers each, the second turned back beside the first (a U)
        let half = n / 2;
        s.runs = vec![
            StairRun { shape: RunShape::Straight, start: Point::new(0.0, 0.0), end: Point::new(0.0, 1000.0), justification: RunJustification::Center, width_mm: 1000.0, risers: half, begins_with_riser: true, ends_with_riser: true },
            StairRun { shape: RunShape::Straight, start: Point::new(1200.0, (half - 1) as f64 * 250.0), end: Point::new(1200.0, 0.0), justification: RunJustification::Center, width_mm: 1000.0, risers: n - half, begins_with_riser: true, ends_with_riser: true },
        ];
        let id = d.add_stair(s).unwrap();
        let st = d.stair(id).unwrap().clone();
        assert_eq!(st.landings.len(), 1, "a landing between the runs");
        let l = d.stair_layout(&st).unwrap();
        assert!((l.riser_mm - h / n as f64).abs() < 1e-9);
        assert!((l.reached_z() - t).abs() < 1e-6, "the runs reach the top");
        assert_eq!(l.runs[0].treads.len() as u32, half - 1, "ends with a riser: one tread fewer than risers");
        // the U landing spans both runs and is a run's width deep past them
        let g = &l.landings[0];
        let ys: Vec<f64> = g.outline.iter().map(|p| p.y).collect();
        let xs: Vec<f64> = g.outline.iter().map(|p| p.x).collect();
        let run_end = (half - 1) as f64 * 250.0;
        assert!((ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - (run_end + 1000.0)).abs() < 1e-6);
        assert!((xs.iter().cloned().fold(f64::INFINITY, f64::min) + 500.0).abs() < 1e-6 && (xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 1700.0).abs() < 1e-6);
        assert!((g.z - (b + half as f64 * l.riser_mm)).abs() < 1e-6);
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    }

    #[test]
    fn an_l_landing_is_square_and_a_short_stair_warns() {
        let (mut d, l1, l2) = doc();
        let ty = d.stair_types()[0].0;
        let mut s = d.new_stair(ty, l1, Some(l2)).unwrap();
        s.runs = vec![
            StairRun { shape: RunShape::Straight, start: Point::new(0.0, 0.0), end: Point::new(0.0, 1.0), justification: RunJustification::Left, width_mm: 1000.0, risers: 4, begins_with_riser: true, ends_with_riser: true },
            StairRun { shape: RunShape::Straight, start: Point::new(1000.0, 1750.0), end: Point::new(2000.0, 1750.0), justification: RunJustification::Left, width_mm: 1000.0, risers: 4, begins_with_riser: true, ends_with_riser: true },
        ];
        let id = d.add_stair(s).unwrap();
        let st = d.stair(id).unwrap().clone();
        let l = d.stair_layout(&st).unwrap();
        // run 1 goes north with its left edge on x = 0, run 2 east with its left edge on y = 1750
        let g = &l.landings[0];
        assert_eq!(g.outline.len(), 4, "a square corner landing: {:?}", g.outline);
        assert!((crate::geom::signed_area(&g.outline).abs() - 1.0e6).abs() < 1.0);
        assert!(l.warnings.iter().any(|w| w.contains("does not reach its top")), "{:?}", l.warnings);
        // pieces: monolithic run columns and a landing slab, all closed solids with volume
        let pieces = d.stair_pieces(&st);
        assert!(pieces.iter().any(|p| p.part == StairPart::Landing(0)));
        assert!(pieces.iter().all(|p| p.solid.volume() > 0.0));
    }

    #[test]
    fn a_convex_solid_clips_and_cuts() {
        let sq = vec![Point::new(0.0, 0.0), Point::new(1000.0, 0.0), Point::new(1000.0, 1000.0), Point::new(0.0, 1000.0)];
        let c = Convex::prism(&sq, 0.0, 1000.0);
        assert!((c.volume() - 1e9).abs() < 1.0);
        let half = c.clip(&|p| p[0], 400.0, true).unwrap();
        assert!((half.volume() - 0.6e9).abs() < 1.0);
        assert_eq!(half.faces.len(), 6);
        assert_eq!(c.section(&|p| p[2], 500.0).len(), 4 * 2, "each crossing edge is met from both faces");
        assert!(c.clip(&|p| p[0], 2000.0, true).is_none());
    }

    #[test]
    fn a_spiral_turns_its_risers_round_the_centre_at_the_tread_depth_on_the_walkline() {
        let (mut d, l1, l2) = doc();
        let ty = d.stair_types()[0].0;
        let mut s = d.new_stair(ty, l1, Some(l2)).unwrap();
        let n = s.desired_risers;
        s.runs = vec![StairRun { shape: RunShape::Spiral { centre: Point::new(0.0, 0.0), clockwise: false }, start: Point::new(1500.0, 0.0), end: Point::new(1500.0, 1.0), justification: RunJustification::Center, width_mm: 1000.0, risers: n, begins_with_riser: true, ends_with_riser: true }];
        let id = d.add_stair(s).unwrap();
        let st = d.stair(id).unwrap().clone();
        let l = d.stair_layout(&st).unwrap();
        let p = l.runs[0].path.as_ref().unwrap();
        assert_eq!(p.riser_lines.len() as u32, n);
        assert_eq!(p.treads.len() as u32, n - 1);
        // the first riser along +x from radius 1000 to 2000, the last (n - 1) treads of 250 round at radius 1500
        let [a, b] = p.riser_lines[0];
        assert!(a.dist(&Point::new(1000.0, 0.0)) < 1e-6 && b.dist(&Point::new(2000.0, 0.0)) < 1e-6, "{a:?} {b:?}");
        let [a, _] = p.riser_lines[(n - 1) as usize];
        let angle = a.y.atan2(a.x).rem_euclid(std::f64::consts::TAU);
        assert!((angle - ((n - 1) as f64 * 250.0 / 1500.0).rem_euclid(std::f64::consts::TAU)).abs() < 1e-6);
        assert!(l.landings.is_empty());
        let pieces = d.stair_pieces(&st);
        assert!(pieces.len() as u32 >= n - 1 && pieces.iter().all(|x| x.solid.volume() > 0.0));
        // a railing follows its outer edge round and up
        let line = crate::railing::stair_side(&l, crate::railing::RailingSide::Right);
        assert!(line.pts.iter().all(|(q, _)| (q.dist(&Point::new(0.0, 0.0)) - 2000.0).abs() < 1.0));
        assert!((line.pts.last().unwrap().1 - l.reached_z()).abs() < 1e-6);
    }

    #[test]
    fn a_spiral_has_a_smooth_soffit_stringers_round_its_edges_and_nosings() {
        let (mut d, l1, l2) = doc();
        let by_name = |d: &Document, n: &str| d.stair_types().iter().find(|(_, t)| t.name == n).unwrap().0;
        let (concrete, timber) = (by_name(&d, "Concrete Stair"), by_name(&d, "Timber Stair"));
        let spiral = |d: &mut Document, ty: Id| {
            let mut s = d.new_stair(ty, l1, Some(l2)).unwrap();
            s.runs = vec![StairRun { shape: RunShape::Spiral { centre: Point::new(0.0, 0.0), clockwise: false }, start: Point::new(1500.0, 0.0), end: Point::new(1500.0, 1.0), justification: RunJustification::Center, width_mm: 1000.0, risers: s.desired_risers, begins_with_riser: true, ends_with_riser: true }];
            let id = d.add_stair(s).unwrap();
            d.stair(id).cloned().unwrap()
        };
        // concrete: each column's underside is the structural depth under the nosing line, not a riser lower
        let st = spiral(&mut d, concrete);
        let l = d.stair_layout(&st).unwrap();
        let t = d.stair_type(concrete).unwrap();
        let pitch = l.riser_mm / l.tread_mm;
        let drop = t.run.structural_depth_mm * (1.0 + pitch * pitch).sqrt();
        let pieces = d.stair_pieces(&st);
        let runs: Vec<&StairPiece> = pieces.iter().filter(|p| matches!(p.part, StairPart::Run(_))).collect();
        assert!(!runs.is_empty() && runs.iter().all(|p| p.solid.volume() > 0.0));
        for p in runs.iter().filter(|p| p.solid.z_range().0 > l.base_z + 1.0) {
            let (lo, hi) = p.solid.z_range();
            assert!(hi - lo < drop + 1.0, "{} against {}", hi - lo, drop);
        }
        assert!(runs.iter().any(|p| {
            let mut zs: Vec<i64> = p.solid.pts.iter().map(|q| q[2].round() as i64).collect();
            zs.sort();
            zs.dedup();
            zs.len() >= 3
        }), "the underside slopes");
        // timber: closed stringers round both edges, and a nosing on each tread
        let st = spiral(&mut d, timber);
        let pieces = d.stair_pieces(&st);
        let sup: Vec<&StairPiece> = pieces.iter().filter(|p| matches!(p.part, StairPart::Support(_))).collect();
        assert!(!sup.is_empty() && sup.iter().all(|p| p.material.as_deref() == Some("Timber Joist") && p.solid.volume() > 0.0));
        for p in &sup {
            for q in &p.solid.pts {
                let r = q[0].hypot(q[1]);
                assert!((954.0..=1001.0).contains(&r) || (1999.0..=2046.0).contains(&r), "{r}");
            }
        }
        let l = d.stair_layout(&st).unwrap();
        let treads = pieces.iter().filter(|p| matches!(p.part, StairPart::Tread(_))).count();
        assert!(treads > l.runs[0].treads.len() * 3 / 2, "{treads}");
    }

    #[test]
    fn a_hull_of_points_is_a_closed_solid() {
        let cube: Vec<[f64; 3]> = (0..8).map(|i| [(i & 1) as f64 * 100.0, ((i >> 1) & 1) as f64 * 100.0, ((i >> 2) & 1) as f64 * 100.0]).collect();
        let mut cloud = cube.clone();
        cloud.push([50.0, 50.0, 50.0]);
        cloud.push([50.0, 0.0, 100.0]);
        let c = Convex::hull_of(&cloud).unwrap();
        assert_eq!(c.faces.len(), 6);
        assert!((c.volume() - 1e6).abs() < 1e-3);
        assert!(Convex::hull_of(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]).is_none());
    }

    #[test]
    fn a_winder_turns_its_corner_without_its_risers_crossing() {
        let (mut d, l1, l2) = doc();
        let ty = d.stair_types()[0].0;
        let mut s = d.new_stair(ty, l1, Some(l2)).unwrap();
        let n = s.desired_risers;
        for style in [crate::runpath::WinderStyle::Balanced, crate::runpath::WinderStyle::SinglePoint] {
            s.runs = vec![StairRun { shape: RunShape::Winder { corners: vec![Point::new(0.0, 2000.0)], style }, start: Point::new(0.0, 0.0), end: Point::new(2000.0, 2000.0), justification: RunJustification::Center, width_mm: 1000.0, risers: n, begins_with_riser: true, ends_with_riser: true }];
            let l = d.stair_layout(&s).unwrap();
            let p = l.runs[0].path.as_ref().unwrap();
            // consecutive riser lines do not cross inside the run
            for w in p.riser_lines.windows(2) {
                let ([a, b], [c, e]) = (w[0], w[1]);
                let side = |p: Point, q: Point, r: Point| cross(sub(q, p), sub(r, p));
                let crosses = side(a, b, c) * side(a, b, e) < -1e-6 && side(c, e, a) * side(c, e, b) < -1e-6;
                assert!(!crosses, "{style:?}: {w:?}");
            }
            assert!(p.treads.iter().all(|(t, _)| crate::geom::signed_area(t) > 1000.0), "{style:?}");
            // the last riser is on the way out, square to it
            let [a, b] = *p.riser_lines.last().unwrap();
            assert!((a.x - b.x).abs() < 1e-6 && a.x > 0.0, "{a:?} {b:?}");
        }
        let id = d.add_stair(s).unwrap();
        let st = d.stair(id).unwrap().clone();
        assert!(d.stair_pieces(&st).iter().all(|x| x.solid.volume() > 0.0));
    }

    #[test]
    fn a_sketched_run_is_arranged_from_its_lines_and_a_sketched_landing_takes_its_outline() {
        let (mut d, l1, l2) = doc();
        let ty = d.stair_types()[0].0;
        let mut s = d.new_stair(ty, l1, Some(l2)).unwrap();
        let n = s.desired_risers;
        // boundaries drawn either way, risers in any order but the bottom one first
        let len = (n - 1) as f64 * 260.0;
        let bounds = vec![vec![Point::new(1000.0, len), Point::new(1000.0, 0.0)], vec![Point::new(0.0, 0.0), Point::new(0.0, len)]];
        let mut lines: Vec<[Point; 2]> = (0..n).rev().map(|k| [Point::new(1000.0, k as f64 * 260.0), Point::new(0.0, k as f64 * 260.0)]).collect();
        let bottom = lines.pop().unwrap();
        lines.insert(0, bottom);
        let (left, right, risers) = crate::runpath::arrange(&bounds, &lines).unwrap();
        assert_eq!(left[0], Point::new(0.0, 0.0), "going up y, the left is x = 0");
        assert_eq!(risers[0], [Point::new(0.0, 0.0), Point::new(1000.0, 0.0)]);
        assert_eq!(risers[1][0].y, 260.0);
        s.runs = vec![StairRun { shape: RunShape::Sketched { left, right, risers }, start: Point::new(500.0, 0.0), end: Point::new(500.0, len), justification: RunJustification::Center, width_mm: 1000.0, risers: n, begins_with_riser: true, ends_with_riser: true }];
        let id = d.add_stair(s.clone()).unwrap();
        let l = d.stair_layout(d.stair(id).unwrap()).unwrap();
        assert_eq!(l.runs[0].path.as_ref().unwrap().treads.len() as u32, n - 1);
        assert!((l.reached_z() - l.top_z).abs() < 1e-6);
        assert!(d.stair_pieces(d.stair(id).unwrap()).iter().all(|p| p.solid.volume() > 0.0));
        // two straight runs and a sketched landing of six sides between them
        let half = n / 2;
        let l0 = (half - 1) as f64 * 250.0;
        let mut s2 = d.new_stair(ty, l1, Some(l2)).unwrap();
        s2.runs = vec![
            StairRun { shape: RunShape::Straight, start: Point::new(5000.0, 0.0), end: Point::new(5000.0, 1.0), justification: RunJustification::Center, width_mm: 1000.0, risers: half, begins_with_riser: true, ends_with_riser: true },
            StairRun { shape: RunShape::Straight, start: Point::new(6200.0, l0), end: Point::new(6200.0, 0.0), justification: RunJustification::Center, width_mm: 1000.0, risers: n - half, begins_with_riser: true, ends_with_riser: true },
        ];
        let outline = vec![Point::new(4500.0, l0), Point::new(6700.0, l0), Point::new(6700.0, l0 + 800.0), Point::new(6200.0, l0 + 1300.0), Point::new(5000.0, l0 + 1300.0), Point::new(4500.0, l0 + 800.0)];
        s2.landings = vec![StairLanding { kind: LandingKind::Automatic, after_run: 0, outline: Some(outline.clone()) }];
        let id2 = d.add_stair(s2).unwrap();
        let st2 = d.stair(id2).unwrap().clone();
        assert_eq!(st2.landings.len(), 1, "the sketched landing stands in for the automatic one");
        let lay = d.stair_layout(&st2).unwrap();
        assert_eq!(lay.landings[0].outline.len(), 6);
        assert!((lay.landings[0].z - lay.runs[0].z1).abs() < 1e-6);
    }

    #[test]
    fn a_multistory_stair_repeats_up_to_its_top_level_with_its_railings() {
        let (mut d, l1, l2) = doc();
        let e2 = d.level(l2).unwrap().elevation_mm;
        let e1 = d.level(l1).unwrap().elevation_mm;
        let h = e2 - e1;
        let l3 = d.add_level(Some("Level 3"), e2 + h, false).unwrap();
        let l4 = d.add_level(Some("Level 4"), e2 + h + 2400.0, false).unwrap();
        let ty = d.stair_types()[0].0;
        let mut s = d.new_stair(ty, l1, Some(l2)).unwrap();
        let n = s.desired_risers;
        s.runs = vec![StairRun { shape: RunShape::Straight, start: Point::new(0.0, 0.0), end: Point::new(0.0, 1.0), justification: RunJustification::Center, width_mm: 1000.0, risers: n, begins_with_riser: true, ends_with_riser: true }];
        s.multistory_top = Some(l4);
        let id = d.add_stair(s).unwrap();
        let st = d.stair(id).unwrap().clone();
        let lays = d.stair_layouts(&st);
        assert_eq!(lays.len(), 3);
        assert!((lays[1].base_z - e2).abs() < 1e-6 && (lays[1].riser_mm - lays[0].riser_mm).abs() < 1e-6, "the same storey height, the same stair");
        // the 2400 storey: fewer risers would be too low, so it keeps the count only if the riser stays in limits
        assert!((lays[2].reached_z() - (e2 + h + 2400.0)).abs() < 1e-6);
        assert!(lays[2].riser_mm >= 115.0 && lays[2].riser_mm <= 190.0, "{}", lays[2].riser_mm);
        let pieces = d.stair_pieces(&st);
        assert!(pieces.iter().any(|p| p.part == StairPart::Run(200)));
        // a railing on it repeats too
        let rt = d.railing_types()[0].0;
        let rails = d.add_stair_railings(id, rt, crate::railing::RailingPlacement::Treads).unwrap();
        let r = d.railing(rails[0]).unwrap().clone();
        let posts = d.railing_pieces(&r).iter().filter(|p| p.part == crate::railing::RailPart::Post).count();
        assert_eq!(posts, 3 * 2, "two posts a storey");
        // the levels it uses cannot go
        assert!(d.delete_level(l4).is_err());
        let _ = l3;
    }

    #[test]
    fn treads_follow_begins_and_ends_with_riser() {
        assert_eq!(treads_in(10, true, true), 9);
        assert_eq!(treads_in(10, true, false), 10);
        assert_eq!(treads_in(10, false, false), 11);
        assert_eq!(risers_for_length(9.0 * 250.0, 250.0, true, true), 10);
        assert_eq!(risers_for_length(10.0 * 250.0, 250.0, true, false), 10);
    }
}
