//! Railings: a top rail, up to two handrails, balusters and posts along a path. A sketched railing follows its path
//! on its level; one placed on a stair follows one side of it, over its flights (parallel to the nosing line) and round
//! its landings (level), and moves and changes with the stair. Every part is a convex solid, as a stair's are, so 3D,
//! elevations, sections and IFC share one list (`railing_pieces`).

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::element::{IfcParams, TypeInfo};
use crate::stairs::{Convex, StairLayout};
use crate::{Document, Element, Id, Point};

fn yes() -> bool { true }
fn d_height() -> f64 { 1000.0 }
fn d_hand() -> f64 { 900.0 }
fn d_gap() -> f64 { 125.0 }
fn d_ext() -> f64 { 300.0 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RailShape {
    #[default]
    Round,
    Rectangular,
}

/// A rail's section: round (`width_mm` across) or rectangular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RailProfile {
    #[serde(default)]
    pub shape: RailShape,
    pub width_mm: f64,
    #[serde(default)]
    pub height_mm: f64,
    #[serde(default)]
    pub material: String,
}

impl RailProfile {
    fn depth(&self) -> f64 {
        if self.shape == RailShape::Round { self.width_mm } else { self.height_mm }
    }

    /// The section's outline across and up, about its centre.
    fn outline(&self) -> Vec<[f64; 2]> {
        let (w, h) = (self.width_mm / 2.0, self.depth() / 2.0);
        match self.shape {
            RailShape::Round => (0..12).map(|k| { let a = std::f64::consts::TAU * k as f64 / 12.0; [w * a.cos(), w * a.sin()] }).collect(),
            RailShape::Rectangular => vec![[-w, -h], [w, -h], [w, h], [-w, h]],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopRail {
    /// The top of the rail above floors and landings.
    #[serde(default = "d_height")]
    pub height_mm: f64,
    /// Above the nosing line of a flight.
    #[serde(default = "d_height")]
    pub flight_height_mm: f64,
    pub profile: RailProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Termination {
    #[default]
    None,
    Wall,
    Floor,
    Post,
}

/// A graspable handrail: its height above nosings and floors, how far it sits in from the railing's line towards the
/// walking side, and its extensions past the top and bottom risers on a stair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handrail {
    #[serde(default = "d_hand")]
    pub height_mm: f64,
    #[serde(default)]
    pub offset_mm: f64,
    pub profile: RailProfile,
    /// Horizontal past the top riser.
    #[serde(default = "d_ext")]
    pub extension_top_mm: f64,
    /// One tread on along the pitch past the bottom riser, then this far level.
    #[serde(default = "d_ext")]
    pub extension_bottom_mm: f64,
    #[serde(default)]
    pub termination: Termination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternJustification {
    Beginning,
    End,
    Center,
    #[default]
    SpreadToFit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Balusters {
    /// Square balusters this wide.
    pub width_mm: f64,
    pub spacing_mm: f64,
    #[serde(default)]
    pub justification: PatternJustification,
    /// On a stair's flights, this many to a tread in place of the pattern.
    #[serde(default)]
    pub per_tread: Option<u32>,
    #[serde(default)]
    pub material: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Posts {
    pub width_mm: f64,
    #[serde(default = "yes")]
    pub corners: bool,
    /// A corner post where the path turns by more than this.
    #[serde(default)]
    pub corner_angle_deg: f64,
    #[serde(default)]
    pub material: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RailingType {
    pub name: String,
    #[serde(default)]
    pub top_rail: Option<TopRail>,
    #[serde(default)]
    pub handrail_1: Option<Handrail>,
    #[serde(default)]
    pub handrail_2: Option<Handrail>,
    #[serde(default)]
    pub balusters: Option<Balusters>,
    #[serde(default)]
    pub posts: Option<Posts>,
    /// The largest opening the infill may leave (the NCC's 125 sphere).
    #[serde(default = "d_gap")]
    pub max_gap_mm: f64,
    #[serde(default)]
    pub info: TypeInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RailingSide {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RailingPlacement {
    /// On the treads, in from the edge of the run.
    #[default]
    Treads,
    /// On the edge of the run (the stringer, once stairs have them).
    Stringer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RailingSlope {
    #[default]
    ByHost,
    Flat,
    Sloped,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RailingHost {
    pub stair: Id,
    pub side: RailingSide,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Railing {
    pub railing_type: Id,
    pub level: Id,
    #[serde(default)]
    pub base_offset_mm: f64,
    /// A sketched railing's path; empty for one on a stair, whose path follows the stair.
    #[serde(default)]
    pub path: Vec<Point>,
    #[serde(default)]
    pub host: Option<RailingHost>,
    #[serde(default)]
    pub placement: RailingPlacement,
    #[serde(default)]
    pub slope: RailingSlope,
    /// Which side of a sketched path the handrails and balusters sit towards.
    #[serde(default)]
    pub flipped: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub ifc: IfcParams,
}

/// The surface a railing stands on, as a line in 3D: plan points with the height of the host there. A segment over a
/// flight carries its run's index (the height there follows the nosing line).
#[derive(Debug, Clone, PartialEq)]
pub struct HostLine {
    pub pts: Vec<(Point, f64)>,
    pub runs: Vec<Option<usize>>,
    /// The stair's tread depth, for handrail extensions, when on a stair.
    pub tread: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RailPart {
    TopRail,
    Handrail(u8),
    Baluster,
    Post,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RailPiece {
    pub part: RailPart,
    pub material: Option<String>,
    pub solid: Convex,
}

fn sub(a: Point, b: Point) -> Point { Point::new(a.x - b.x, a.y - b.y) }
fn norm(a: Point) -> Option<Point> { let l = (a.x * a.x + a.y * a.y).sqrt(); (l > 1e-6).then(|| Point::new(a.x / l, a.y / l)) }

/// Each vertex moved `d` to the left of the line, mitred at the corners. Points that repeat (a step in height at one
/// plan point) move with their neighbours.
pub fn offset_line(pts: &[(Point, f64)], d: f64) -> Vec<(Point, f64)> {
    let n = pts.len();
    let dir_before = |i: usize| (0..i).rev().find_map(|k| norm(sub(pts[i].0, pts[k].0)));
    let dir_after = |i: usize| (i + 1..n).find_map(|k| norm(sub(pts[k].0, pts[i].0)));
    (0..n).map(|i| {
        let (a, b) = (dir_before(i), dir_after(i));
        let left = |u: Point| Point::new(-u.y, u.x);
        let m = match (a, b) {
            (Some(a), Some(b)) => {
                let (na, nb) = (left(a), left(b));
                let s = Point::new(na.x + nb.x, na.y + nb.y);
                let k = 1.0 + na.x * nb.x + na.y * nb.y;
                if k > 0.1 { Point::new(s.x / k, s.y / k) } else { na }
            }
            (Some(a), None) => left(a),
            (None, Some(b)) => left(b),
            (None, None) => Point::new(0.0, 0.0),
        };
        (Point::new(pts[i].0.x + m.x * d, pts[i].0.y + m.y * d), pts[i].1)
    }).collect()
}

/// The way round a landing's outline from `from` to `to` that does not run along the runs' own ends (`avoid`).
fn landing_path(outline: &[Point], from: Point, to: Point, avoid: &[[Point; 2]]) -> Vec<Point> {
    let n = outline.len();
    if n < 3 || from.dist(&to) < 1.0 { return vec![]; }
    // distances round the perimeter: each vertex's, and where a point lies
    let lens: Vec<f64> = (0..n).map(|i| outline[i].dist(&outline[(i + 1) % n])).collect();
    let per: f64 = lens.iter().sum();
    let cum: Vec<f64> = (0..n).map(|i| lens[..i].iter().sum()).collect();
    let locate = |p: Point| -> f64 {
        let mut best = (0.0, f64::INFINITY);
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            let ab = sub(b, a);
            let l2 = (ab.x * ab.x + ab.y * ab.y).max(1e-12);
            let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / l2).clamp(0.0, 1.0);
            let dd = Point::new(a.x + ab.x * t, a.y + ab.y * t).dist(&p);
            if dd < best.1 { best = (cum[i] + t * lens[i], dd); }
        }
        best.0
    };
    let (pf, pt) = (locate(from), locate(to));
    let ahead = (pt - pf).rem_euclid(per);
    let mut fwd: Vec<(f64, Point)> = (0..n).map(|i| ((cum[i] - pf).rem_euclid(per), outline[i])).filter(|(d, _)| *d > 1e-6 && *d < ahead - 1e-6).collect();
    fwd.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut back: Vec<(f64, Point)> = (0..n).map(|i| ((pf - cum[i]).rem_euclid(per), outline[i])).filter(|(d, _)| *d > 1e-6 && *d < per - ahead - 1e-6).collect();
    back.sort_by(|a, b| a.0.total_cmp(&b.0));
    let path_of = |mid: &[(f64, Point)]| { let mut p = vec![from]; p.extend(mid.iter().map(|m| m.1)); p.push(to); p };
    // a stretch lying along one of the runs' ends for more than a millimetre
    let overlaps = |a: Point, b: Point| avoid.iter().any(|[p, q]| {
        let pq = sub(*q, *p);
        let l = (pq.x * pq.x + pq.y * pq.y).sqrt().max(1e-9);
        let (ux, uy) = (pq.x / l, pq.y / l);
        let off = |x: Point| ((x.x - p.x) * uy - (x.y - p.y) * ux).abs();
        if off(a) > 1.0 || off(b) > 1.0 { return false; }
        let (ta, tb) = ((a.x - p.x) * ux + (a.y - p.y) * uy, (b.x - p.x) * ux + (b.y - p.y) * uy);
        ta.max(tb).min(l) - ta.min(tb).max(0.0) > 1.0
    });
    let ok = |mid: &[(f64, Point)]| path_of(mid).windows(2).all(|w| !overlaps(w[0], w[1]));
    let len = |mid: &[(f64, Point)]| path_of(mid).windows(2).map(|w| w[0].dist(&w[1])).sum::<f64>();
    let pick = match (ok(&fwd), ok(&back)) { (true, false) => &fwd, (false, true) => &back, _ => if len(&fwd) <= len(&back) { &fwd } else { &back } };
    pick.iter().map(|m| m.1).collect()
}
/// One side of a stair as a line: up each run's edge, round each landing.
pub fn stair_side(l: &StairLayout, side: RailingSide) -> HostLine {
    let sign = if side == RailingSide::Left { 1.0 } else { -1.0 };
    let mut pts: Vec<(Point, f64)> = vec![];
    let mut runs: Vec<Option<usize>> = vec![];
    let push = |p: Point, z: f64, run: Option<usize>, pts: &mut Vec<(Point, f64)>, runs: &mut Vec<Option<usize>>| {
        if let Some(last) = pts.last() { if last.0.dist(&p) < 1e-6 && (last.1 - z).abs() < 1e-6 { return; } }
        if !pts.is_empty() { runs.push(run); }
        pts.push((p, z));
    };
    for (i, r) in l.runs.iter().enumerate() {
        let w = sign * r.width / 2.0;
        push(r.at(0.0, w), r.nosing_z(0.0), None, &mut pts, &mut runs);
        push(r.at(r.length, w), r.nosing_z(r.length), Some(i), &mut pts, &mut runs);
        if let Some(n) = l.runs.get(i + 1) {
            let wn = sign * n.width / 2.0;
            if let Some(g) = l.landings.iter().find(|g| g.after_run == i) {
                let (wa, wb) = (r.width / 2.0, n.width / 2.0);
                let avoid = [[r.at(r.length, -wa), r.at(r.length, wa)], [n.at(0.0, -wb), n.at(0.0, wb)]];
                for p in landing_path(&g.outline, r.at(r.length, w), n.at(0.0, wn), &avoid) {
                    push(p, g.z, None, &mut pts, &mut runs);
                }
            }
        }
    }
    HostLine { pts, runs, tread: Some(l.tread_mm) }
}

/// A chain of sketched curves as one path, end to end. None when they do not join up.
pub fn chain_path(curves: &[Curve]) -> Option<Vec<Point>> {
    if curves.is_empty() { return None; }
    let mut left: Vec<Vec<Point>> = curves.iter().map(|c| c.tessellate(20.0)).filter(|p| p.len() >= 2).collect();
    let mut path = left.remove(0);
    while !left.is_empty() {
        let (s, e) = (path[0], *path.last()?);
        let k = left.iter().position(|p| p[0].dist(&e) < 1.0 || p.last().unwrap().dist(&e) < 1.0 || p[0].dist(&s) < 1.0 || p.last().unwrap().dist(&s) < 1.0)?;
        let mut p = left.remove(k);
        if p[0].dist(&e) < 1.0 { path.extend(p.into_iter().skip(1)); }
        else if p.last().unwrap().dist(&e) < 1.0 { p.reverse(); path.extend(p.into_iter().skip(1)); }
        else if p.last().unwrap().dist(&s) < 1.0 { p.pop(); p.extend(path); path = p; }
        else { p.reverse(); p.pop(); p.extend(path); path = p; }
    }
    Some(path)
}

/// Where `n` items go along a stretch `len` long at `spacing`, by the justification; the ends (posts) are left clear.
fn pattern(len: f64, spacing: f64, j: PatternJustification) -> Vec<f64> {
    if spacing <= 1.0 || len <= spacing * 0.5 { return vec![]; }
    match j {
        PatternJustification::SpreadToFit => {
            let n = (len / spacing).ceil().max(1.0) as usize;
            (1..n).map(|k| len * k as f64 / n as f64).collect()
        }
        _ => {
            let n = (len / spacing).floor() as usize;
            let start = match j { PatternJustification::Beginning => spacing, PatternJustification::End => len - (n as f64 - 1.0) * spacing - spacing, _ => (len - (n as f64 - 1.0) * spacing) / 2.0 };
            (0..n).map(|k| start + k as f64 * spacing).filter(|s| *s > 1.0 && *s < len - 1.0).collect()
        }
    }
}

impl Document {
    pub fn railing_types(&self) -> Vec<(Id, &RailingType)> {
        self.iter().filter_map(|(i, e)| if let Element::RailingType(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn railing_type(&self, id: Id) -> Option<&RailingType> {
        match self.get(id)? { Element::RailingType(t) => Some(t), _ => None }
    }

    pub fn railings(&self) -> Vec<(Id, &Railing)> {
        self.iter().filter_map(|(i, e)| if let Element::Railing(r) = e { Some((i, r)) } else { None }).collect()
    }

    pub fn railing(&self, id: Id) -> Option<&Railing> {
        match self.get(id)? { Element::Railing(r) => Some(r), _ => None }
    }

    /// The railings on a stair.
    pub fn stair_railings(&self, stair: Id) -> Vec<Id> {
        self.railings().into_iter().filter(|(_, r)| r.host.is_some_and(|h| h.stair == stair)).map(|(i, _)| i).collect()
    }

    pub fn ensure_railing_types(&mut self) -> Result<(), String> {
        if !self.railing_types().is_empty() { return Ok(()); }
        self.transact("Railing types", |tx| {
            for t in starter_railing_types() { tx.add(Element::RailingType(t)); }
            Ok(())
        })
    }

    /// The line a railing stands on: its sketched path on its level, or its side of its stair; moved in from a stair's
    /// edge when it stands on the treads.
    pub fn railing_host_line(&self, r: &Railing) -> Option<HostLine> {
        let t = self.railing_type(r.railing_type)?;
        match r.host {
            Some(h) => {
                let s = self.stair(h.stair)?;
                let l = self.stair_layout(s)?;
                let line = stair_side(&l, h.side);
                // in from the edge by half the widest upright, and a little
                let inset = match r.placement {
                    RailingPlacement::Treads => t.posts.as_ref().map_or(0.0, |p| p.width_mm).max(t.balusters.as_ref().map_or(0.0, |b| b.width_mm)) / 2.0 + 25.0,
                    RailingPlacement::Stringer => 0.0,
                };
                let d = if h.side == RailingSide::Left { -inset } else { inset };
                Some(HostLine { pts: offset_line(&line.pts, d), runs: line.runs, tread: line.tread })
            }
            None => {
                let z = self.level(r.level)?.elevation_mm + r.base_offset_mm;
                if r.path.len() < 2 { return None; }
                Some(HostLine { pts: r.path.iter().map(|p| (*p, z)).collect(), runs: vec![None; r.path.len() - 1], tread: None })
            }
        }
    }

    /// Which way a railing's handrails and balusters face (left of the line: +1), towards the stair's walking side.
    fn railing_inward(&self, r: &Railing) -> f64 {
        match r.host {
            Some(h) => if h.side == RailingSide::Left { -1.0 } else { 1.0 },
            None => if r.flipped { -1.0 } else { 1.0 },
        }
    }

    /// The length of a railing's path in plan.
    pub fn railing_length(&self, r: &Railing) -> f64 {
        self.railing_host_line(r).map_or(0.0, |l| l.pts.windows(2).map(|w| w[0].0.dist(&w[1].0)).sum())
    }

    /// A railing's parts: its top rail and handrails along the path, its balusters and posts.
    pub fn railing_pieces(&self, r: &Railing) -> Vec<RailPiece> {
        let (Some(t), Some(line)) = (self.railing_type(r.railing_type), self.railing_host_line(r)) else { return vec![] };
        let mat = |m: &str| if m.is_empty() { None } else { Some(m.to_string()) };
        let stair_layout = r.host.and_then(|h| self.stair(h.stair)).and_then(|s| self.stair_layout(s));
        let mut out = vec![];
        let pts = &line.pts;
        let flight = |k: usize| line.runs.get(k).copied().flatten().is_some();
        // the height of a rail over the line: one height over flights, another elsewhere, a vertex between the two
        // taking the flight's
        let rail_z = |k: usize, on_flight: f64, flat: f64| -> f64 {
            let near = (k > 0 && flight(k - 1)) || flight(k);
            pts[k].1 + if near { on_flight } else { flat }
        };
        let beams = |line3: &[[f64; 3]], prof: &RailProfile, part: RailPart, out: &mut Vec<RailPiece>| {
            let o = prof.outline();
            for w in line3.windows(2) {
                let (a, b) = (w[0], w[1]);
                if ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt() < 1.0 { continue; }
                out.push(RailPiece { part, material: mat(&prof.material), solid: Convex::beam(a, b, &o) });
            }
        };
        let mut top_at: Option<Vec<f64>> = None;
        if let Some(tr) = &t.top_rail {
            let zs: Vec<f64> = (0..pts.len()).map(|k| rail_z(k, tr.flight_height_mm, tr.height_mm)).collect();
            let c = tr.profile.depth() / 2.0;
            let l3: Vec<[f64; 3]> = pts.iter().zip(&zs).map(|((p, _), z)| [p.x, p.y, z - c]).collect();
            beams(&l3, &tr.profile, RailPart::TopRail, &mut out);
            top_at = Some(zs.iter().map(|z| z - tr.profile.depth()).collect());
        }
        let inward = self.railing_inward(r);
        for (k, h) in [&t.handrail_1, &t.handrail_2].into_iter().enumerate() {
            let Some(h) = h else { continue };
            let side = offset_line(pts, inward * h.offset_mm);
            let c = h.profile.depth() / 2.0;
            let mut l3: Vec<[f64; 3]> = side.iter().enumerate().map(|(i, (p, z))| [p.x, p.y, z - pts[i].1 + rail_z(i, h.height_mm, h.height_mm) - c]).collect();
            // on a stair: one tread on along the pitch and level past the bottom riser, level past the top
            if let (Some(tread), true) = (line.tread, l3.len() >= 2) {
                let (a, b) = (l3[0], l3[1]);
                let pl = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                if pl > 1e-6 && h.extension_bottom_mm > 0.0 {
                    let (ux, uy, k) = ((b[0] - a[0]) / pl, (b[1] - a[1]) / pl, (b[2] - a[2]) / pl);
                    let e1 = [a[0] - ux * tread, a[1] - uy * tread, a[2] - k * tread];
                    let e2 = [e1[0] - ux * h.extension_bottom_mm, e1[1] - uy * h.extension_bottom_mm, e1[2]];
                    l3.splice(0..0, [e2, e1]);
                }
                let n = l3.len();
                let (a, b) = (l3[n - 2], l3[n - 1]);
                let pl = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                if pl > 1e-6 && h.extension_top_mm > 0.0 {
                    l3.push([b[0] + (b[0] - a[0]) / pl * h.extension_top_mm, b[1] + (b[1] - a[1]) / pl * h.extension_top_mm, b[2]]);
                }
            }
            beams(&l3, &h.profile, RailPart::Handrail(k as u8 + 1), &mut out);
        }
        // balusters, up to the underside of the top rail
        if let (Some(b), Some(tops)) = (&t.balusters, &top_at) {
            let w = b.width_mm / 2.0;
            for k in 0..pts.len().saturating_sub(1) {
                let ((p, z0), (q, z1)) = (pts[k], pts[k + 1]);
                let len = p.dist(&q);
                if len < 1.0 { continue; }
                let run = line.runs[k].and_then(|i| stair_layout.as_ref().and_then(|l| l.runs.get(i)));
                // fractions along the segment, each with the height of what the baluster stands on
                let mut at: Vec<(f64, f64)> = vec![];
                match (run, b.per_tread) {
                    (Some(rl), Some(n)) if n > 0 => {
                        for (s0, s1, top) in &rl.treads {
                            for j in 0..n { let s = s0 + (j as f64 + 0.5) * (s1 - s0) / n as f64; at.push((s / rl.length.max(1e-9), *top)); }
                        }
                    }
                    _ => {
                        for d in pattern(len, b.spacing_mm, b.justification) {
                            let f = d / len;
                            let base = match run {
                                Some(rl) => { let s = f * rl.length; rl.treads.iter().find(|t| s >= t.0 - 1e-6 && s <= t.1 + 1e-6).map_or(z0 + (z1 - z0) * f, |t| t.2) }
                                None => z0 + (z1 - z0) * f,
                            };
                            at.push((f, base));
                        }
                    }
                }
                for (f, base) in at {
                    let c = Point::new(p.x + (q.x - p.x) * f, p.y + (q.y - p.y) * f);
                    let top = tops[k] + (tops[k + 1] - tops[k]) * f;
                    if top - base < 10.0 { continue; }
                    let sq = [Point::new(c.x - w, c.y - w), Point::new(c.x + w, c.y - w), Point::new(c.x + w, c.y + w), Point::new(c.x - w, c.y + w)];
                    out.push(RailPiece { part: RailPart::Baluster, material: mat(&b.material), solid: Convex::prism(&sq, base, top) });
                }
            }
        }
        // posts at the ends and the corners
        if let Some(po) = &t.posts {
            let top_of = |k: usize| top_at.as_ref().map(|tz| tz[k] + t.top_rail.as_ref().map_or(0.0, |r| r.profile.depth())).or_else(|| t.handrail_1.as_ref().map(|h| pts[k].1 + h.height_mm));
            let mut at: Vec<usize> = vec![0, pts.len() - 1];
            if po.corners {
                for k in 1..pts.len() - 1 {
                    let a = (0..k).rev().find_map(|i| norm(sub(pts[k].0, pts[i].0)));
                    let b = (k + 1..pts.len()).find_map(|i| norm(sub(pts[i].0, pts[k].0)));
                    if let (Some(a), Some(b)) = (a, b) {
                        let turn = (a.x * b.y - a.y * b.x).atan2(a.x * b.x + a.y * b.y).to_degrees().abs();
                        if turn > po.corner_angle_deg.max(1.0) && pts[k].0.dist(&pts[k - 1].0) > 1e-6 { at.push(k); }
                    }
                }
            }
            let w = po.width_mm / 2.0;
            for k in at {
                let Some(top) = top_of(k) else { continue };
                let (c, base) = pts[k];
                let sq = [Point::new(c.x - w, c.y - w), Point::new(c.x + w, c.y - w), Point::new(c.x + w, c.y + w), Point::new(c.x - w, c.y + w)];
                if top - base > 10.0 { out.push(RailPiece { part: RailPart::Post, material: mat(&po.material), solid: Convex::prism(&sq, base, top) }); }
            }
        }
        out
    }

    /// What breaks the railing type's rules here: openings in the infill wider than the type allows.
    pub fn railing_warnings(&self, r: &Railing) -> Vec<String> {
        let mut out = vec![];
        let Some(t) = self.railing_type(r.railing_type) else { return out };
        if r.host.is_some_and(|h| self.stair(h.stair).is_none()) { out.push("The railing's stair is gone.".into()); return out; }
        let Some(b) = &t.balusters else { return out };
        let Some(line) = self.railing_host_line(r) else { return out };
        let tread = r.host.and_then(|h| self.stair(h.stair)).map(|s| s.tread_depth_mm);
        let mut worst: f64 = 0.0;
        for k in 0..line.pts.len().saturating_sub(1) {
            let len = line.pts[k].0.dist(&line.pts[k + 1].0);
            if len < 1.0 { continue; }
            let gap = match (line.runs[k], b.per_tread, tread) {
                (Some(_), Some(n), Some(tr)) if n > 0 => tr / n as f64 - b.width_mm,
                _ => {
                    let at = pattern(len, b.spacing_mm, b.justification);
                    let mut stops = vec![0.0];
                    stops.extend(at);
                    stops.push(len);
                    stops.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max) - b.width_mm
                }
            };
            worst = worst.max(gap);
        }
        if worst > t.max_gap_mm + 1e-6 {
            out.push(format!("The balusters leave a {worst:.0} opening, over the {} the type allows.", t.max_gap_mm));
        }
        out
    }

    fn check_railing(&self, r: &Railing) -> Result<(), String> {
        if self.railing_type(r.railing_type).is_none() { return Err("unknown railing type".into()); }
        if self.level(r.level).is_none() { return Err("unknown level".into()); }
        if !r.base_offset_mm.is_finite() { return Err("the offset must be a number".into()); }
        match r.host {
            Some(h) => { if self.stair(h.stair).is_none() { return Err("unknown stair".into()); } }
            None => { if r.path.len() < 2 { return Err("A railing path needs at least one line.".into()); } }
        }
        Ok(())
    }

    fn new_railing(&self, railing_type: Id, level: Id) -> Railing {
        Railing { railing_type, level, base_offset_mm: 0.0, path: vec![], host: None, placement: RailingPlacement::Treads, slope: RailingSlope::ByHost, flipped: false, pinned: false, comments: String::new(), mark: String::new(), ifc: IfcParams { guid: crate::element::new_ifc_guid(), ..Default::default() } }
    }

    /// Sketch Path's Finish: a railing along the sketched lines, which must join end to end.
    pub fn add_railing(&mut self, railing_type: Id, level: Id, curves: &[Curve]) -> Result<Id, String> {
        let mut r = self.new_railing(railing_type, level);
        r.path = chain_path(curves).ok_or("The railing path must be one chain of lines joined end to end.")?;
        self.check_railing(&r)?;
        self.transact("Create railing", |tx| Ok(tx.add(Element::Railing(r))))
    }

    /// Edit Path's Finish.
    pub fn set_railing_path(&mut self, id: Id, curves: &[Curve]) -> Result<(), String> {
        self.refuse_pinned(&[id], "edited")?;
        let mut r = self.railing(id).cloned().ok_or("not a railing")?;
        if r.host.is_some() { return Err("A railing on a stair follows the stair.".into()); }
        r.path = chain_path(curves).ok_or("The railing path must be one chain of lines joined end to end.")?;
        self.check_railing(&r)?;
        self.transact("Edit railing path", |tx| tx.update(id, Element::Railing(r)))
    }

    /// Place on Stair: a railing on each side of the stair, one undo step (as part of a larger one when grouped).
    pub fn add_stair_railings(&mut self, stair: Id, railing_type: Id, placement: RailingPlacement) -> Result<Vec<Id>, String> {
        let s = self.stair(stair).ok_or("Click a stair.")?;
        let level = s.base_level;
        let mut out = vec![];
        let mut rs = vec![];
        for side in [RailingSide::Left, RailingSide::Right] {
            let mut r = self.new_railing(railing_type, level);
            r.host = Some(RailingHost { stair, side });
            r.placement = placement;
            self.check_railing(&r)?;
            rs.push(r);
        }
        self.transact("Place railing on stair", |tx| {
            for r in rs { out.push(tx.add(Element::Railing(r))); }
            Ok(())
        })?;
        Ok(out)
    }

    pub fn patch_railing(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.railing(id).cloned().ok_or("not a railing")?;
        let r: Railing = crate::doc::apply_patch(&old, patch)?;
        if old.pinned && r.pinned && (r.path != old.path || r.level != old.level || r.base_offset_mm != old.base_offset_mm || r.placement != old.placement) {
            return Err("Unpin the railing before changing it.".into());
        }
        if r.host != old.host { return Err("A railing keeps its stair.".into()); }
        self.check_railing(&r)?;
        self.transact("Railing properties", |tx| tx.update(id, Element::Railing(r)))
    }

    pub fn patch_railing_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.railing_type(id).cloned().ok_or("not a railing type")?;
        let t: RailingType = crate::doc::apply_patch(&old, patch)?;
        if t.name.trim().is_empty() { return Err("a type needs a name".into()); }
        if self.railing_types().iter().any(|(i, o)| *i != id && o.name == t.name) { return Err(format!("a railing type named '{}' already exists", t.name)); }
        let mut sizes = vec![t.max_gap_mm];
        if let Some(r) = &t.top_rail { sizes.extend([r.height_mm, r.flight_height_mm, r.profile.width_mm, r.profile.height_mm]); }
        for h in [&t.handrail_1, &t.handrail_2].into_iter().flatten() { sizes.extend([h.height_mm, h.offset_mm.abs(), h.profile.width_mm, h.profile.height_mm, h.extension_top_mm, h.extension_bottom_mm]); }
        if let Some(b) = &t.balusters { sizes.extend([b.width_mm, b.spacing_mm]); if b.spacing_mm < b.width_mm { return Err("the baluster spacing must be more than their width".into()); } }
        if let Some(p) = &t.posts { sizes.extend([p.width_mm, p.corner_angle_deg]); }
        if sizes.iter().any(|v| !v.is_finite() || *v < 0.0) { return Err("the type's sizes must be numbers, zero or more".into()); }
        self.transact("Change railing type", |tx| tx.update(id, Element::RailingType(t)))
    }

    pub fn duplicate_railing_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let mut t = self.railing_type(id).cloned().ok_or("not a railing type")?;
        if name.trim().is_empty() { return Err("a type needs a name".into()); }
        if self.railing_types().iter().any(|(_, o)| o.name == name) { return Err(format!("a railing type named '{name}' already exists")); }
        t.name = name.into();
        self.transact("Duplicate type", |tx| Ok(tx.add(Element::RailingType(t))))
    }

    /// The 3D view's meshes: one per material.
    pub fn railing_meshes(&self, r: &Railing) -> Vec<(Option<String>, Vec<f32>)> {
        let mut by: Vec<(Option<String>, Vec<f32>)> = vec![];
        for p in self.railing_pieces(r) {
            let k = match by.iter().position(|(m, _)| *m == p.material) { Some(k) => k, None => { by.push((p.material.clone(), vec![])); by.len() - 1 } };
            p.solid.mesh(&mut by[k].1);
        }
        by
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn railing_view_items(&self, id: Id, r: &Railing, u: &dyn Fn(Point) -> f64, d: &dyn Fn(Point) -> f64, near: f64, far: f64, cut: Option<f64>) -> Vec<crate::site::ElevItem> {
        let pieces: Vec<(Option<String>, Convex)> = self.railing_pieces(r).into_iter().map(|p| (p.material, p.solid)).collect();
        self.convex_view_items(id, "Railings", &pieces, u, d, near, far, cut)
    }
}

pub fn starter_railing_types() -> Vec<RailingType> {
    let round = |d: f64| RailProfile { shape: RailShape::Round, width_mm: d, height_mm: d, material: "Aluminium".into() };
    let hand = |height: f64, offset: f64| Handrail { height_mm: height, offset_mm: offset, profile: round(40.0), extension_top_mm: 300.0, extension_bottom_mm: 300.0, termination: Termination::None };
    vec![
        RailingType {
            name: "Balustrade 1000".into(),
            top_rail: Some(TopRail { height_mm: 1000.0, flight_height_mm: 1000.0, profile: RailProfile { shape: RailShape::Rectangular, width_mm: 50.0, height_mm: 50.0, material: "Aluminium".into() } }),
            handrail_1: Some(hand(900.0, 60.0)),
            handrail_2: None,
            balusters: Some(Balusters { width_mm: 20.0, spacing_mm: 120.0, justification: PatternJustification::SpreadToFit, per_tread: Some(2), material: "Aluminium".into() }),
            posts: Some(Posts { width_mm: 50.0, corners: true, corner_angle_deg: 15.0, material: "Aluminium".into() }),
            max_gap_mm: 125.0,
            info: TypeInfo::default(),
        },
        RailingType {
            name: "Handrail 900".into(),
            top_rail: None,
            handrail_1: Some(hand(900.0, 0.0)),
            handrail_2: None,
            balusters: None,
            posts: None,
            max_gap_mm: 125.0,
            info: TypeInfo::default(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stairs::{RunJustification, RunShape, StairRun};

    fn stair_doc() -> (Document, Id) {
        let mut d = Document::new_project();
        let mut levels: Vec<(Id, f64)> = d.levels().iter().map(|(i, l)| (*i, l.elevation_mm)).collect();
        levels.sort_by(|a, b| a.1.total_cmp(&b.1));
        let ty = d.stair_types()[0].0;
        let mut s = d.new_stair(ty, levels[0].0, Some(levels[1].0)).unwrap();
        let n = s.desired_risers;
        let half = n / 2;
        let run = |start: Point, end: Point, risers: u32| StairRun { shape: RunShape::Straight, start, end, justification: RunJustification::Center, width_mm: 1000.0, risers, begins_with_riser: true, ends_with_riser: true };
        let l1 = (half - 1) as f64 * 250.0;
        s.runs = vec![run(Point::new(0.0, 0.0), Point::new(0.0, 1.0), half), run(Point::new(1200.0, l1), Point::new(1200.0, 0.0), n - half)];
        let id = d.add_stair(s).unwrap();
        (d, id)
    }

    #[test]
    fn a_u_stairs_outer_railing_goes_round_the_landing_and_the_inner_one_across_the_gap() {
        let (mut d, stair) = stair_doc();
        let ty = d.railing_types().iter().find(|(_, t)| t.name == "Balustrade 1000").unwrap().0;
        let ids = d.add_stair_railings(stair, ty, RailingPlacement::Stringer).unwrap();
        let s = d.stair(stair).unwrap().clone();
        let l = d.stair_layout(&s).unwrap();
        let outer = stair_side(&l, RailingSide::Left);
        let inner = stair_side(&l, RailingSide::Right);
        // run 1 goes up y on x -500..500; its left (outer) is x = -500, the landing reaches 1000 past its end
        let top = l.runs[0].length + 1000.0;
        assert!(outer.pts.iter().any(|(p, _)| (p.x + 500.0).abs() < 1e-6 && (p.y - top).abs() < 1e-6), "{:?}", outer.pts);
        assert!(outer.pts.iter().any(|(p, _)| (p.x - 1700.0).abs() < 1e-6 && (p.y - top).abs() < 1e-6));
        // the inner side crosses the 200 gap at the landing's edge
        let ys: Vec<f64> = inner.pts.iter().map(|(p, _)| p.y).collect();
        assert!(ys.iter().all(|y| *y <= l.runs[0].length + 1e-6), "{:?}", inner.pts);
        // heights: the line climbs from the first nosing to the landing and on to the top
        assert!((outer.pts[0].1 - (l.base_z + l.riser_mm)).abs() < 1e-6);
        assert!((outer.pts.last().unwrap().1 - l.reached_z()).abs() < 1e-6);
        // pieces: rails, two balusters a tread, posts; the opening between balusters is in limits
        let r = d.railing(ids[0]).unwrap().clone();
        let pieces = d.railing_pieces(&r);
        let treads: usize = l.runs.iter().map(|x| x.treads.len()).sum();
        let bal = pieces.iter().filter(|p| p.part == RailPart::Baluster).count();
        assert!(bal >= treads * 2, "{bal} balusters for {treads} treads");
        assert!(pieces.iter().any(|p| p.part == RailPart::Post) && pieces.iter().any(|p| p.part == RailPart::TopRail) && pieces.iter().any(|p| p.part == RailPart::Handrail(1)));
        assert!(d.railing_warnings(&r).is_empty(), "{:?}", d.railing_warnings(&r));
        assert!(pieces.iter().all(|p| p.solid.volume() > 0.0));
    }

    #[test]
    fn a_sketched_railing_chains_its_lines_and_checks_its_openings() {
        let mut d = Document::new_project();
        let lv = d.levels()[0].0;
        let ty = d.railing_types().iter().find(|(_, t)| t.name == "Balustrade 1000").unwrap().0;
        // drawn out of order and one line backwards
        let curves = vec![Curve::line(Point::new(3000.0, 0.0), Point::new(3000.0, 2000.0)), Curve::line(Point::new(0.0, 0.0), Point::new(3000.0, 0.0))];
        let id = d.add_railing(ty, lv, &curves).unwrap();
        let r = d.railing(id).unwrap().clone();
        assert_eq!(r.path.len(), 3);
        assert!((d.railing_length(&r) - 5000.0).abs() < 1e-6);
        let pieces = d.railing_pieces(&r);
        assert_eq!(pieces.iter().filter(|p| p.part == RailPart::Post).count(), 3, "two ends and the corner");
        assert!(d.railing_warnings(&r).is_empty());
        // spacing the balusters wider leaves too big an opening
        d.patch_railing_type(ty, &serde_json::json!({ "balusters": { "spacing_mm": 200.0, "justification": "beginning" } })).unwrap();
        let r = d.railing(id).unwrap().clone();
        assert!(d.railing_warnings(&r).iter().any(|w| w.contains("opening")), "{:?}", d.railing_warnings(&r));
        assert!(d.add_railing(ty, lv, &[Curve::line(Point::new(0.0, 0.0), Point::new(1.0, 0.0)), Curve::line(Point::new(10.0, 10.0), Point::new(20.0, 10.0))]).is_err());
        assert_eq!(pattern(1000.0, 120.0, PatternJustification::SpreadToFit).len(), 8);
    }
}
