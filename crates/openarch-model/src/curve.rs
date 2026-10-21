//! Curves for sketched elements (floors now; roofs, ceilings, openings, slab edges and
//! stairs later). All units are millimetres.
//!
//! A sketch is a set of curves. `sketch_loops` groups them into closed loops, checks
//! them the way Finish Edit Mode does (closed, no gaps, no crossing or
//! overlapping lines, at most two lines meeting at a point) and nests them so inner
//! loops become openings.

use serde::{Deserialize, Serialize};

use crate::edit::Xform;
use crate::geom::Point;

/// Two ends closer than this are one point (the own tolerance is about 0.8 mm).
pub const JOIN_TOL: f64 = 0.8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Curve {
    Line { a: Point, b: Point },
    /// Circular arc from `start` to `end` about `centre`, counter-clockwise when `ccw`.
    Arc { start: Point, end: Point, centre: Point, ccw: bool },
    Circle { centre: Point, radius: f64 },
    /// An ellipse with semi-axes `rx` (along `rotation_deg`) and `ry`. With `t0_deg` and
    /// `t1_deg` it is the part between those parameter angles (the Partial Ellipse).
    Ellipse {
        centre: Point,
        rx: f64,
        ry: f64,
        #[serde(default)]
        rotation_deg: f64,
        #[serde(default)]
        t0_deg: Option<f64>,
        #[serde(default)]
        t1_deg: Option<f64>,
        /// Direction of travel from `t0_deg` to `t1_deg` (partial ellipses only).
        #[serde(default = "yes")]
        ccw: bool,
    },
    /// A smooth curve through the fit points (Catmull-Rom). `closed` joins the last point back to the first.
    Spline {
        points: Vec<Point>,
        #[serde(default)]
        closed: bool,
    },
}

fn yes() -> bool {
    true
}

fn tau() -> f64 {
    std::f64::consts::TAU
}

fn ang(c: Point, p: Point) -> f64 {
    (p.y - c.y).atan2(p.x - c.x)
}

/// Sweep angle of an arc in radians, positive for counter-clockwise, never zero for a real arc.
pub fn arc_sweep(start: Point, end: Point, centre: Point, ccw: bool) -> f64 {
    let (a0, a1) = (ang(centre, start), ang(centre, end));
    if ccw {
        let s = (a1 - a0).rem_euclid(tau());
        if s < 1e-9 { tau() } else { s }
    } else {
        let s = (a0 - a1).rem_euclid(tau());
        -(if s < 1e-9 { tau() } else { s })
    }
}

fn ellipse_pt(centre: Point, rx: f64, ry: f64, rot: f64, t: f64) -> Point {
    let (x, y) = (rx * t.cos(), ry * t.sin());
    let (s, c) = rot.sin_cos();
    Point::new(centre.x + x * c - y * s, centre.y + x * s + y * c)
}

fn segments_for(radius: f64, sweep: f64, tol: f64) -> usize {
    let r = radius.max(1e-6);
    let step = if tol >= r { std::f64::consts::FRAC_PI_2 } else { 2.0 * (1.0 - tol / r).acos() };
    ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(2, 720)
}

fn catmull(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let (t2, t3) = (t * t, t * t * t);
    let f = |a: f64, b: f64, c: f64, d: f64| 0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
    Point::new(f(p0.x, p1.x, p2.x, p3.x), f(p0.y, p1.y, p2.y, p3.y))
}

impl Curve {
    pub fn line(a: Point, b: Point) -> Curve {
        Curve::Line { a, b }
    }

    pub fn is_closed(&self) -> bool {
        match self {
            Curve::Circle { .. } => true,
            Curve::Ellipse { t0_deg, t1_deg, .. } => t0_deg.is_none() && t1_deg.is_none(),
            Curve::Spline { closed, .. } => *closed,
            _ => false,
        }
    }

    /// Where the curve starts (for a closed curve, where it begins and ends).
    pub fn start(&self) -> Point {
        match self {
            Curve::Line { a, .. } => *a,
            Curve::Arc { start, .. } => *start,
            Curve::Circle { centre, radius } => Point::new(centre.x + radius, centre.y),
            Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg, .. } => ellipse_pt(*centre, *rx, *ry, rotation_deg.to_radians(), t0_deg.unwrap_or(0.0).to_radians()),
            Curve::Spline { points, .. } => points.first().copied().unwrap_or_default(),
        }
    }

    pub fn end(&self) -> Point {
        match self {
            Curve::Line { b, .. } => *b,
            Curve::Arc { end, .. } => *end,
            Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg, t1_deg, .. } if t0_deg.is_some() || t1_deg.is_some() => {
                ellipse_pt(*centre, *rx, *ry, rotation_deg.to_radians(), t1_deg.unwrap_or(360.0).to_radians())
            }
            Curve::Spline { points, closed: false } => points.last().copied().unwrap_or_default(),
            _ => self.start(),
        }
    }

    /// Points along the curve, within `tol` mm of it. Open curves include both ends; a closed
    /// curve starts at its start point and does not repeat it.
    pub fn tessellate(&self, tol: f64) -> Vec<Point> {
        match self {
            Curve::Line { a, b } => vec![*a, *b],
            Curve::Arc { start, end, centre, ccw } => {
                let r = centre.dist(start);
                let sweep = arc_sweep(*start, *end, *centre, *ccw);
                let n = segments_for(r, sweep, tol);
                let a0 = ang(*centre, *start);
                let mut v: Vec<Point> = (0..=n).map(|i| {
                    let a = a0 + sweep * i as f64 / n as f64;
                    Point::new(centre.x + r * a.cos(), centre.y + r * a.sin())
                }).collect();
                v[0] = *start;
                *v.last_mut().unwrap() = *end;
                v
            }
            Curve::Circle { centre, radius } => {
                let n = segments_for(*radius, tau(), tol).max(8);
                (0..n).map(|i| {
                    let a = tau() * i as f64 / n as f64;
                    Point::new(centre.x + radius * a.cos(), centre.y + radius * a.sin())
                }).collect()
            }
            Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg, t1_deg, ccw } => {
                let rot = rotation_deg.to_radians();
                let partial = t0_deg.is_some() || t1_deg.is_some();
                let (t0, t1) = (t0_deg.unwrap_or(0.0).to_radians(), t1_deg.unwrap_or(360.0).to_radians());
                let sweep = if !partial {
                    tau()
                } else if *ccw {
                    let s = (t1 - t0).rem_euclid(tau());
                    if s < 1e-9 { tau() } else { s }
                } else {
                    let s = (t0 - t1).rem_euclid(tau());
                    -(if s < 1e-9 { tau() } else { s })
                };
                let n = segments_for(rx.max(*ry), sweep, tol).max(if partial { 4 } else { 16 });
                let last = if partial { n } else { n - 1 };
                (0..=last).map(|i| ellipse_pt(*centre, *rx, *ry, rot, t0 + sweep * i as f64 / n as f64)).collect()
            }
            Curve::Spline { points, closed } => {
                let n = points.len();
                if n < 2 {
                    return points.clone();
                }
                let at = |i: isize| -> Point {
                    if *closed {
                        points[i.rem_euclid(n as isize) as usize]
                    } else {
                        points[i.clamp(0, n as isize - 1) as usize]
                    }
                };
                let segs = if *closed { n } else { n - 1 };
                let mut out = vec![];
                for i in 0..segs as isize {
                    let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
                    let steps = ((p1.dist(&p2) / (tol.max(0.1) * 12.0)).ceil() as usize).clamp(6, 64);
                    for k in 0..steps {
                        out.push(catmull(p0, p1, p2, p3, k as f64 / steps as f64));
                    }
                }
                if !*closed {
                    out.push(points[n - 1]);
                }
                out
            }
        }
    }

    /// The point halfway along the curve (by parameter).
    pub fn mid(&self) -> Point {
        match self {
            Curve::Line { a, b } => Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
            Curve::Arc { start, end, centre, ccw } => {
                let (r, sweep) = (centre.dist(start), arc_sweep(*start, *end, *centre, *ccw));
                let a = ang(*centre, *start) + sweep / 2.0;
                Point::new(centre.x + r * a.cos(), centre.y + r * a.sin())
            }
            _ => {
                let v = self.tessellate(1.0);
                v[v.len() / 2]
            }
        }
    }

    pub fn length(&self) -> f64 {
        let v = self.tessellate(0.5);
        let mut l: f64 = v.windows(2).map(|w| w[0].dist(&w[1])).sum();
        if self.is_closed() && v.len() > 1 {
            l += v[v.len() - 1].dist(&v[0]);
        }
        l
    }

    pub fn reversed(&self) -> Curve {
        match self {
            Curve::Line { a, b } => Curve::Line { a: *b, b: *a },
            Curve::Arc { start, end, centre, ccw } => Curve::Arc { start: *end, end: *start, centre: *centre, ccw: !ccw },
            Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg: Some(a), t1_deg: Some(b), ccw } => {
                Curve::Ellipse { centre: *centre, rx: *rx, ry: *ry, rotation_deg: *rotation_deg, t0_deg: Some(*b), t1_deg: Some(*a), ccw: !ccw }
            }
            Curve::Spline { points, closed } => Curve::Spline { points: points.iter().rev().copied().collect(), closed: *closed },
            c => c.clone(),
        }
    }

    /// A closed circle or ellipse as two open halves, which is how a wall goes round one. They run
    /// clockwise, so the exterior (left of the direction of travel) faces out as it does for a
    /// rectangle drawn the usual way.
    pub fn halves(&self) -> Option<[Curve; 2]> {
        match self {
            Curve::Circle { centre, radius } => {
                let (a, b) = (Point::new(centre.x + radius, centre.y), Point::new(centre.x - radius, centre.y));
                Some([Curve::Arc { start: a, end: b, centre: *centre, ccw: false }, Curve::Arc { start: b, end: a, centre: *centre, ccw: false }])
            }
            Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg: None, t1_deg: None, .. } => {
                let half = |t0: f64, t1: f64| Curve::Ellipse { centre: *centre, rx: *rx, ry: *ry, rotation_deg: *rotation_deg, t0_deg: Some(t0), t1_deg: Some(t1), ccw: false };
                Some([half(0.0, 180.0), half(180.0, 360.0)])
            }
            _ => None,
        }
    }

    /// The curve after the rotation, uniform scale and move that carry `a0` and `a1` onto `b0` and
    /// `b1` (no reflection). A curved wall keeps its shape when one end is dragged this way.
    pub fn similar(&self, a0: Point, a1: Point, b0: Point, b1: Point) -> Curve {
        let (da, db) = (Point::new(a1.x - a0.x, a1.y - a0.y), Point::new(b1.x - b0.x, b1.y - b0.y));
        let (la, lb) = (da.x.hypot(da.y), db.x.hypot(db.y));
        if la < 1e-9 || lb < 1e-9 {
            return self.clone();
        }
        let (k, rot) = (lb / la, db.y.atan2(db.x) - da.y.atan2(da.x));
        let (s, co) = rot.sin_cos();
        let map = |p: Point| {
            let (x, y) = (p.x - a0.x, p.y - a0.y);
            Point::new(b0.x + k * (x * co - y * s), b0.y + k * (x * s + y * co))
        };
        match self {
            Curve::Line { a, b } => Curve::Line { a: map(*a), b: map(*b) },
            Curve::Arc { start, end, centre, ccw } => Curve::Arc { start: map(*start), end: map(*end), centre: map(*centre), ccw: *ccw },
            Curve::Circle { centre, radius } => Curve::Circle { centre: map(*centre), radius: radius * k },
            Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg, t1_deg, ccw } => Curve::Ellipse {
                centre: map(*centre),
                rx: rx * k,
                ry: ry * k,
                rotation_deg: rotation_deg + rot.to_degrees(),
                t0_deg: *t0_deg,
                t1_deg: *t1_deg,
                ccw: *ccw,
            },
            Curve::Spline { points, closed } => Curve::Spline { points: points.iter().map(|p| map(*p)).collect(), closed: *closed },
        }
    }

    /// The curve made `k` times larger about the origin.
    pub fn scaled(&self, k: f64) -> Curve {
        let m = |p: &Point| Point::new(p.x * k, p.y * k);
        match self {
            Curve::Line { a, b } => Curve::Line { a: m(a), b: m(b) },
            Curve::Arc { start, end, centre, ccw } => Curve::Arc { start: m(start), end: m(end), centre: m(centre), ccw: *ccw },
            Curve::Circle { centre, radius } => Curve::Circle { centre: m(centre), radius: radius * k },
            Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg, t1_deg, ccw } => Curve::Ellipse { centre: m(centre), rx: rx * k, ry: ry * k, rotation_deg: *rotation_deg, t0_deg: *t0_deg, t1_deg: *t1_deg, ccw: *ccw },
            Curve::Spline { points, closed } => Curve::Spline { points: points.iter().map(m).collect(), closed: *closed },
        }
    }

    pub fn transform(&self, x: Xform) -> Curve {
        let mirror = matches!(x, Xform::Mirror { .. });
        let axis = if let Xform::Mirror { ax, ay, bx, by } = x { (by - ay).atan2(bx - ax).to_degrees() } else { 0.0 };
        let turn = if let Xform::Rotate { angle_deg, .. } = x { angle_deg } else { 0.0 };
        match self {
            Curve::Line { a, b } => Curve::Line { a: x.apply(*a), b: x.apply(*b) },
            Curve::Arc { start, end, centre, ccw } => Curve::Arc { start: x.apply(*start), end: x.apply(*end), centre: x.apply(*centre), ccw: *ccw != mirror },
            Curve::Circle { centre, radius } => Curve::Circle { centre: x.apply(*centre), radius: *radius },
            Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg, t1_deg, ccw } => {
                let rot = if mirror { 2.0 * axis - rotation_deg } else { rotation_deg + turn };
                // reflecting turns each parameter angle into its negative and the direction round
                let (t0, t1) = if mirror { (t0_deg.map(|t| -t), t1_deg.map(|t| -t)) } else { (*t0_deg, *t1_deg) };
                Curve::Ellipse { centre: x.apply(*centre), rx: *rx, ry: *ry, rotation_deg: rot, t0_deg: t0, t1_deg: t1, ccw: *ccw != mirror }
            }
            Curve::Spline { points, closed } => Curve::Spline { points: points.iter().map(|p| x.apply(*p)).collect(), closed: *closed },
        }
    }
}

// ---- loops --------------------------------------------------------------------

/// Why a sketch cannot finish, with where, so the UI can point at it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SketchError {
    pub message: String,
    pub at: Option<[f64; 2]>,
}

impl SketchError {
    fn new(message: impl Into<String>, at: Option<Point>) -> Self {
        SketchError { message: message.into(), at: at.map(|p| [p.x, p.y]) }
    }
}

impl std::fmt::Display for SketchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

fn near(a: Point, b: Point) -> bool {
    a.dist(&b) <= JOIN_TOL
}

/// Group loose curves into closed loops, each ordered head to tail. Checks that nothing is
/// left open, that no more than two curves meet at a point, and that no lines cross or overlap.
pub fn sketch_loops(curves: &[Curve]) -> Result<Vec<Vec<Curve>>, SketchError> {
    Ok(sketch_loop_order(curves)?.iter().map(|l| l.iter().map(|&(i, rev)| if rev { curves[i].reversed() } else { curves[i].clone() }).collect()).collect())
}

/// As `sketch_loops`, giving each loop as the index of each curve in `curves` and whether it runs reversed.
pub fn sketch_loop_order(curves: &[Curve]) -> Result<Vec<Vec<(usize, bool)>>, SketchError> {
    if curves.is_empty() {
        return Err(SketchError::new("the sketch is empty: draw a closed boundary", None));
    }
    for c in curves {
        if c.length() < JOIN_TOL {
            return Err(SketchError::new("a sketch line is too short", Some(c.start())));
        }
    }
    let mut used = vec![false; curves.len()];
    let mut order: Vec<Vec<(usize, bool)>> = vec![];
    for i in 0..curves.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        if curves[i].is_closed() {
            order.push(vec![(i, false)]);
            continue;
        }
        let mut chain = vec![(i, false)];
        let origin = curves[i].start();
        let mut tip = curves[i].end();
        loop {
            if chain.len() > 1 && near(tip, origin) {
                break;
            }
            // every unused curve that touches the tip, in either direction
            let touching: Vec<(usize, bool)> = (0..curves.len())
                .filter(|j| !used[*j] && !curves[*j].is_closed())
                .filter_map(|j| {
                    if near(curves[j].start(), tip) {
                        Some((j, false))
                    } else if near(curves[j].end(), tip) {
                        Some((j, true))
                    } else {
                        None
                    }
                })
                .collect();
            // a point where three or more lines meet cannot be part of one loop
            let at_tip = curves.iter().filter(|c| !c.is_closed() && (near(c.start(), tip) || near(c.end(), tip))).count();
            if at_tip > 2 {
                return Err(SketchError::new("more than two lines meet at one point", Some(tip)));
            }
            let Some(&(j, rev)) = touching.first() else {
                return Err(SketchError::new("the lines do not form a closed loop: there is a gap or a loose end", Some(tip)));
            };
            used[j] = true;
            chain.push((j, rev));
            tip = if rev { curves[j].start() } else { curves[j].end() };
        }
        order.push(chain);
    }
    let loops: Vec<Vec<Curve>> = order.iter().map(|l| l.iter().map(|&(i, rev)| if rev { curves[i].reversed() } else { curves[i].clone() }).collect()).collect();
    check_no_crossings(&loops)?;
    Ok(order)
}

/// The polygon a loop outlines (counter-clockwise or clockwise as drawn).
pub fn loop_polygon(l: &[Curve], tol: f64) -> Vec<Point> {
    let mut out: Vec<Point> = vec![];
    for c in l {
        let pts = c.tessellate(tol);
        let skip = if !out.is_empty() && !c.is_closed() { 1 } else { 0 };
        out.extend(pts.into_iter().skip(skip));
    }
    // an open chain ends where it began
    if out.len() > 1 && near(out[0], *out.last().unwrap()) && l.len() > 1 {
        out.pop();
    }
    out
}

fn orient(a: Point, b: Point, c: Point) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn on_seg(a: Point, b: Point, p: Point) -> bool {
    p.x >= a.x.min(b.x) - 1e-9 && p.x <= a.x.max(b.x) + 1e-9 && p.y >= a.y.min(b.y) - 1e-9 && p.y <= a.y.max(b.y) + 1e-9
}

/// Do segments ab and cd cross, touch or overlap anywhere other than at a shared end?
fn bad_intersection(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let shared = [(a, c), (a, d), (b, c), (b, d)].iter().filter(|(p, q)| near(*p, *q)).count();
    let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
    let eps = 1e-6;
    if shared > 0 {
        // adjacent segments: only a collinear fold-back overlaps
        let (u, v) = if near(a, c) { (Point::new(b.x - a.x, b.y - a.y), Point::new(d.x - c.x, d.y - c.y)) }
            else if near(a, d) { (Point::new(b.x - a.x, b.y - a.y), Point::new(c.x - d.x, c.y - d.y)) }
            else if near(b, c) { (Point::new(a.x - b.x, a.y - b.y), Point::new(d.x - c.x, d.y - c.y)) }
            else { (Point::new(a.x - b.x, a.y - b.y), Point::new(c.x - d.x, c.y - d.y)) };
        let (lu, lv) = (u.x.hypot(u.y), v.x.hypot(v.y));
        if lu > 1e-9 && lv > 1e-9 && (u.x * v.y - u.y * v.x).abs() / (lu * lv) < 1e-6 && (u.x * v.x + u.y * v.y) > 0.0 {
            return Some(a);
        }
        return None;
    }
    if ((o1 > eps && o2 < -eps) || (o1 < -eps && o2 > eps)) && ((o3 > eps && o4 < -eps) || (o3 < -eps && o4 > eps)) {
        let t = o3 / (o3 - o4);
        return Some(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
    }
    // touching or collinear overlap
    let (l1, l2) = (a.dist(&b).max(1.0), c.dist(&d).max(1.0));
    let z = |o: f64, l: f64| o.abs() / l < 0.05;
    if z(o1, l1) && on_seg(a, b, c) { return Some(c); }
    if z(o2, l1) && on_seg(a, b, d) { return Some(d); }
    if z(o3, l2) && on_seg(c, d, a) { return Some(a); }
    if z(o4, l2) && on_seg(c, d, b) { return Some(b); }
    None
}

fn check_no_crossings(loops: &[Vec<Curve>]) -> Result<(), SketchError> {
    // all edges with the loop they belong to
    let polys: Vec<Vec<Point>> = loops.iter().map(|l| loop_polygon(l, 1.0)).collect();
    let mut edges: Vec<(usize, usize, Point, Point)> = vec![];
    for (li, p) in polys.iter().enumerate() {
        let n = p.len();
        for i in 0..n {
            edges.push((li, i, p[i], p[(i + 1) % n]));
        }
    }
    for i in 0..edges.len() {
        let (la, ia, a, b) = edges[i];
        for &(lb, ib, c, d) in &edges[i + 1..] {
            // neighbours in the same loop share an end by construction
            let n = polys[la].len();
            let adjacent = la == lb && (ib == ia + 1 || (ia == 0 && ib == n - 1) || n == 2);
            if adjacent && !(near(a, c) || near(a, d) || near(b, c) || near(b, d)) {
                continue;
            }
            if let Some(p) = bad_intersection(a, b, c, d) {
                return Err(SketchError::new("sketch lines cross or overlap", Some(p)));
            }
        }
    }
    Ok(())
}

/// Point in polygon (even-odd), boundary treated as outside.
pub fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// How deep each loop is nested inside the others: 0 outer, 1 an opening in it, 2 an island in the opening, and so on.
pub fn loop_depths(polys: &[Vec<Point>]) -> Vec<usize> {
    (0..polys.len())
        .map(|i| {
            // a vertex that is not shared with the other loop (loops cannot touch, so any vertex will do)
            let p = polys[i][0];
            (0..polys.len()).filter(|&j| j != i && point_in_polygon(p, &polys[j])).count()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Curve> {
        vec![
            Curve::line(p(x0, y0), p(x1, y0)),
            Curve::line(p(x1, y0), p(x1, y1)),
            Curve::line(p(x1, y1), p(x0, y1)),
            Curve::line(p(x0, y1), p(x0, y0)),
        ]
    }

    #[test]
    fn arc_runs_the_way_it_says_and_stays_on_its_circle() {
        let c = Curve::Arc { start: p(1000.0, 0.0), end: p(0.0, 1000.0), centre: p(0.0, 0.0), ccw: true };
        let v = c.tessellate(1.0);
        assert!(v.iter().all(|q| (q.dist(&p(0.0, 0.0)) - 1000.0).abs() < 1e-6));
        assert!(v.len() > 8 && v[0] == p(1000.0, 0.0) && *v.last().unwrap() == p(0.0, 1000.0));
        assert!((c.length() - 1000.0 * std::f64::consts::FRAC_PI_2).abs() < 2.0);
        // clockwise from the same ends is the long way round
        let cw = Curve::Arc { start: p(1000.0, 0.0), end: p(0.0, 1000.0), centre: p(0.0, 0.0), ccw: false };
        assert!((cw.length() - 1000.0 * 1.5 * std::f64::consts::PI).abs() < 5.0);
        // reversing keeps the same curve
        let r = c.reversed();
        assert_eq!((r.start(), r.end()), (c.end(), c.start()));
        assert!((r.length() - c.length()).abs() < 1e-6);
    }

    #[test]
    fn circle_ellipse_and_spline_tessellate_within_tolerance() {
        let circ = Curve::Circle { centre: p(0.0, 0.0), radius: 500.0 };
        let v = circ.tessellate(1.0);
        assert!(v.len() >= 16 && v.iter().all(|q| (q.dist(&p(0.0, 0.0)) - 500.0).abs() < 1e-6));
        // the chord midpoints stay within tolerance of the circle
        let worst = (0..v.len()).map(|i| { let (a, b) = (v[i], v[(i + 1) % v.len()]); 500.0 - Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0).dist(&p(0.0, 0.0)) }).fold(0.0, f64::max);
        assert!(worst <= 1.0 + 1e-6, "{worst}");
        let e = Curve::Ellipse { centre: p(0.0, 0.0), rx: 800.0, ry: 400.0, rotation_deg: 90.0, t0_deg: None, t1_deg: None, ccw: true };
        let v = e.tessellate(1.0);
        // major axis turned to vertical
        let (maxy, maxx) = (v.iter().map(|q| q.y).fold(f64::MIN, f64::max), v.iter().map(|q| q.x).fold(f64::MIN, f64::max));
        assert!((maxy - 800.0).abs() < 1.0 && (maxx - 400.0).abs() < 1.0);
        let half = Curve::Ellipse { centre: p(0.0, 0.0), rx: 800.0, ry: 400.0, rotation_deg: 0.0, t0_deg: Some(0.0), t1_deg: Some(180.0), ccw: true };
        assert!(half.start().dist(&p(800.0, 0.0)) < 1e-6 && half.end().dist(&p(-800.0, 0.0)) < 1e-6);
        let s = Curve::Spline { points: vec![p(0.0, 0.0), p(500.0, 300.0), p(1000.0, 0.0), p(1500.0, 300.0)], closed: false };
        let v = s.tessellate(1.0);
        assert!(v[0] == p(0.0, 0.0) && *v.last().unwrap() == p(1500.0, 300.0));
        assert!(v.iter().any(|q| q.dist(&p(500.0, 300.0)) < 1e-6), "passes through its fit points");
    }

    #[test]
    fn loose_lines_group_into_ordered_closed_loops_whatever_direction_they_were_drawn() {
        let mut c = rect(0.0, 0.0, 4000.0, 3000.0);
        c[1] = c[1].reversed(); // drawn the other way
        c.swap(0, 2);
        let loops = sketch_loops(&c).unwrap();
        assert_eq!(loops.len(), 1);
        let l = &loops[0];
        for i in 0..l.len() {
            assert!(l[i].end().dist(&l[(i + 1) % l.len()].start()) < JOIN_TOL, "head to tail");
        }
    }

    #[test]
    fn a_rectangle_with_a_circle_inside_is_an_outer_loop_and_an_opening() {
        let mut c = rect(0.0, 0.0, 4000.0, 3000.0);
        c.push(Curve::Circle { centre: p(2000.0, 1500.0), radius: 500.0 });
        let loops = sketch_loops(&c).unwrap();
        assert_eq!(loops.len(), 2);
        let polys: Vec<Vec<Point>> = loops.iter().map(|l| loop_polygon(l, 1.0)).collect();
        let mut depths = loop_depths(&polys);
        depths.sort();
        assert_eq!(depths, vec![0, 1]);
    }

    #[test]
    fn finish_refuses_gaps_crossings_overlaps_and_branches_and_says_where() {
        // a gap
        let mut c = rect(0.0, 0.0, 4000.0, 3000.0);
        c[3] = Curve::line(p(0.0, 3000.0), p(0.0, 500.0));
        let e = sketch_loops(&c).unwrap_err();
        assert!(e.message.contains("closed loop"), "{}", e.message);
        assert!(e.at.is_some());
        // a line crossing the boundary
        let mut c = rect(0.0, 0.0, 4000.0, 3000.0);
        c.push(Curve::Circle { centre: p(4000.0, 1500.0), radius: 500.0 });
        assert!(sketch_loops(&c).unwrap_err().message.contains("cross"));
        // two loops that overlap along an edge
        let mut c = rect(0.0, 0.0, 4000.0, 3000.0);
        c.extend(rect(1000.0, 0.0, 2000.0, 1000.0));
        assert!(sketch_loops(&c).is_err());
        // a spur: three lines at a point
        let mut c = rect(0.0, 0.0, 4000.0, 3000.0);
        c.push(Curve::line(p(4000.0, 3000.0), p(5000.0, 3000.0)));
        assert!(sketch_loops(&c).unwrap_err().message.contains("more than two"));
        // a line that runs past a corner
        let mut c = rect(0.0, 0.0, 4000.0, 3000.0);
        c[0] = Curve::line(p(0.0, 0.0), p(4500.0, 0.0));
        assert!(sketch_loops(&c).is_err());
        assert!(sketch_loops(&[]).is_err());
    }

    #[test]
    fn transforms_keep_arcs_valid_and_mirror_turns_them() {
        let arc = Curve::Arc { start: p(1000.0, 0.0), end: p(0.0, 1000.0), centre: p(0.0, 0.0), ccw: true };
        let moved = arc.transform(Xform::Move { dx: 100.0, dy: 50.0 });
        assert_eq!(moved.start(), p(1100.0, 50.0));
        let m = arc.transform(Xform::Mirror { ax: 0.0, ay: 0.0, bx: 1.0, by: 0.0 });
        assert!(m.start().dist(&p(1000.0, 0.0)) < 1e-9 && m.end().dist(&p(0.0, -1000.0)) < 1e-9);
        assert!((m.length() - arc.length()).abs() < 1e-6, "same arc, other side");
        let r = arc.transform(Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 });
        assert!(r.start().dist(&p(0.0, 1000.0)) < 1e-9);
    }
}
