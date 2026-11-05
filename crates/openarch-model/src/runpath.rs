//! Runs that are not straight: spirals round a centre, and winders that turn corners without a landing. Each is laid
//! out along its walkline (the run's centre line): where each riser stands along it, the riser's line across the run,
//! each tread's outline in plan, and the run's two edges. Straight runs keep their simpler frame in `stairs.rs`; every
//! consumer of a run (plans, solids, railings, headroom) reads this layout when a run has one.

use serde::{Deserialize, Serialize};

use crate::Point;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinderStyle {
    /// The riser lines turn gradually through the corner, so the treads stay usable at the inside.
    #[default]
    Balanced,
    /// The riser lines in the corner all radiate from its inside point.
    SinglePoint,
}

/// A run laid out along a curved or cornered walkline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunPath {
    /// The walkline, finely, with each point's distance along it.
    pub walk: Vec<(Point, f64)>,
    /// Each riser's line across the run: its left end, its right end (looking up the run).
    pub riser_lines: Vec<[Point; 2]>,
    /// Each tread's outline (counter-clockwise) and its top.
    pub treads: Vec<(Vec<Point>, f64)>,
    /// The run's edges, each point with its distance along the walkline.
    pub left: Vec<(Point, f64)>,
    pub right: Vec<(Point, f64)>,
}

fn sub(a: Point, b: Point) -> Point { Point::new(a.x - b.x, a.y - b.y) }
fn add(a: Point, b: Point) -> Point { Point::new(a.x + b.x, a.y + b.y) }
fn mul(a: Point, k: f64) -> Point { Point::new(a.x * k, a.y * k) }
fn cross(a: Point, b: Point) -> f64 { a.x * b.y - a.y * b.x }
fn unit(a: Point) -> Point { let l = (a.x * a.x + a.y * a.y).sqrt().max(1e-12); Point::new(a.x / l, a.y / l) }
fn left_of(d: Point) -> Point { Point::new(-d.y, d.x) }

/// A point `s` along a polyline, and the direction there; past the ends it carries straight on.
pub fn along(pts: &[Point], s: f64) -> (Point, Point) {
    let mut acc = 0.0;
    for (i, w) in pts.windows(2).enumerate() {
        let l = w[0].dist(&w[1]);
        let last = i + 2 == pts.len();
        if s <= acc + l || last {
            let d = unit(sub(w[1], w[0]));
            let t = if s < 0.0 && i == 0 { s } else { s - acc };
            return (add(w[0], mul(d, t)), d);
        }
        acc += l;
    }
    (pts[0], Point::new(1.0, 0.0))
}

/// Where a point lies along a polyline (its nearest point's distance from the start).
fn param(pts: &[(Point, f64)], p: Point) -> f64 {
    let mut best = (0.0, f64::INFINITY);
    for w in pts.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        let ab = sub(b, a);
        let l2 = (ab.x * ab.x + ab.y * ab.y).max(1e-12);
        let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / l2).clamp(0.0, 1.0);
        let d = add(a, mul(ab, t)).dist(&p);
        if d < best.1 { best = (w[0].1 + (w[1].1 - w[0].1) * t, d); }
    }
    best.0
}

/// The first point where a ray from `o` along `d` meets a polyline.
fn hit(pts: &[(Point, f64)], o: Point, d: Point) -> Option<Point> {
    let mut best: Option<(f64, Point)> = None;
    for w in pts.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        let e = sub(b, a);
        let den = cross(d, e);
        if den.abs() < 1e-12 { continue; }
        let t = cross(sub(a, o), e) / den;
        let u = cross(sub(a, o), d) / den;
        if t > -1e-6 && (-1e-6..=1.0 + 1e-6).contains(&u) && best.is_none_or(|b| t < b.0) {
            best = Some((t, add(o, mul(d, t))));
        }
    }
    best.map(|b| b.1)
}

/// The edges of a polyline walkline `w` wide: points with their distance along the walkline.
fn edges(walk: &[(Point, f64)], w: f64) -> (Vec<(Point, f64)>, Vec<(Point, f64)>) {
    let pts: Vec<(Point, f64)> = walk.iter().map(|(p, _)| (*p, 0.0)).collect();
    let l = crate::railing::offset_line(&pts, w / 2.0);
    let r = crate::railing::offset_line(&pts, -w / 2.0);
    (l.iter().zip(walk).map(|(a, b)| (a.0, b.1)).collect(), r.iter().zip(walk).map(|(a, b)| (a.0, b.1)).collect())
}

/// Each step's position: where risers stand along the walkline, and where its treads begin and end.
pub struct Steps {
    pub length: f64,
    pub risers: Vec<f64>,
    pub treads: Vec<(f64, f64, f64)>,
}

/// Lay a run out along a walkline: `walk` (with distances), the direction across the run at a distance (towards its
/// left), its edges, and its steps.
fn build(walk: Vec<(Point, f64)>, across: &dyn Fn(f64) -> (Point, Point), left: Vec<(Point, f64)>, right: Vec<(Point, f64)>, steps: &Steps, width: f64) -> RunPath {
    // a riser's line: from the walkline out to each edge
    let line = |s: f64| -> [Point; 2] {
        let (o, u) = across(s);
        let l = hit(&left, o, u).unwrap_or_else(|| add(o, mul(u, width / 2.0)));
        let r = hit(&right, o, mul(u, -1.0)).unwrap_or_else(|| sub(o, mul(u, width / 2.0)));
        [l, r]
    };
    let riser_lines: Vec<[Point; 2]> = steps.risers.iter().map(|s| line(*s)).collect();
    let between = |edge: &[(Point, f64)], a: f64, b: f64| -> Vec<Point> { edge.iter().filter(|(_, s)| *s > a + 1e-6 && *s < b - 1e-6).map(|(p, _)| *p).collect() };
    let treads = steps.treads.iter().map(|(s0, s1, top)| {
        let ([l0, r0], [l1, r1]) = (line(*s0), line(*s1));
        let (pl0, pl1) = (param(&left, l0), param(&left, l1));
        let (pr0, pr1) = (param(&right, r0), param(&right, r1));
        let mut poly = vec![l0, r0];
        poly.extend(between(&right, pr0, pr1));
        poly.extend([r1, l1]);
        let mut back = between(&left, pl0, pl1);
        back.reverse();
        poly.extend(back);
        poly.dedup_by(|a, b| a.dist(b) < 1e-6);
        if crate::geom::signed_area(&poly) < 0.0 { poly.reverse(); }
        (poly, *top)
    }).collect();
    RunPath { walk, riser_lines, treads, left, right }
}

/// A spiral round `centre` from `start`, `length` along its walkline (the run's centre line), anticlockwise unless
/// `clockwise`.
pub fn spiral(centre: Point, start: Point, clockwise: bool, width: f64, steps: &Steps) -> RunPath {
    let r = centre.dist(&start).max(width / 2.0 + 1.0);
    let a0 = (start.y - centre.y).atan2(start.x - centre.x);
    let sign = if clockwise { -1.0 } else { 1.0 };
    let at = |s: f64, rad: f64| { let a = a0 + sign * s / r; Point::new(centre.x + rad * a.cos(), centre.y + rad * a.sin()) };
    let n = ((steps.length / 50.0).ceil() as usize).max(8);
    let ss: Vec<f64> = (0..=n).map(|k| steps.length * k as f64 / n as f64).collect();
    let walk: Vec<(Point, f64)> = ss.iter().map(|s| (at(*s, r), *s)).collect();
    // looking up an anticlockwise run, its left is the inside
    let (rl, rr) = if clockwise { (r + width / 2.0, r - width / 2.0) } else { (r - width / 2.0, r + width / 2.0) };
    let left: Vec<(Point, f64)> = ss.iter().map(|s| (at(*s, rl), *s)).collect();
    let right: Vec<(Point, f64)> = ss.iter().map(|s| (at(*s, rr), *s)).collect();
    let across = |s: f64| { let o = at(s, r); let out = unit(sub(o, centre)); (o, if clockwise { out } else { mul(out, -1.0) }) };
    build(walk, &across, left, right, steps, width)
}

/// A winder: along `pts` (the walkline: start, each corner, then on in the last direction), `steps.length` long,
/// turning its risers round each corner.
pub fn winder(pts: &[Point], style: WinderStyle, width: f64, steps: &Steps) -> RunPath {
    // the walkline, taken on past its last point to the run's length
    let mut line: Vec<Point> = pts.to_vec();
    let drawn: f64 = line.windows(2).map(|w| w[0].dist(&w[1])).sum();
    if let (Some(a), Some(b)) = (line.get(line.len().wrapping_sub(2)).copied(), line.last().copied()) {
        let d = unit(sub(b, a));
        let n = line.len();
        line[n - 1] = add(b, mul(d, steps.length - drawn));
    }
    let mut walk: Vec<(Point, f64)> = vec![];
    let mut acc = 0.0;
    for (i, p) in line.iter().enumerate() {
        if i > 0 { acc += line[i - 1].dist(p); }
        walk.push((*p, acc));
    }
    let (left, right) = edges(&walk, width);
    // each corner: where it is along the walkline, the way in and out, and its inside point
    let corners: Vec<(f64, Point, Point, Point)> = (1..line.len().saturating_sub(1)).map(|i| {
        let (a, b) = (unit(sub(line[i], line[i - 1])), unit(sub(line[i + 1], line[i])));
        let inside = if cross(a, b) > 0.0 { left[i].0 } else { right[i].0 };
        (walk[i].1, a, b, inside)
    }).collect();
    let across = |s: f64| -> (Point, Point) {
        let (o, d) = along(&line, s);
        let mut u = left_of(d);
        if let Some((c, a, b, inside)) = corners.iter().copied().min_by(|x, y| (x.0 - s).abs().total_cmp(&(y.0 - s).abs())) {
            match style {
                WinderStyle::Balanced if (s - c).abs() < width => {
                    // turn from square to the way in to square to the way out, evenly through the corner
                    let f = ((s - (c - width)) / (2.0 * width)).clamp(0.0, 1.0);
                    let (ua, ub) = (left_of(a), left_of(b));
                    let (ta, mut tb) = (ua.y.atan2(ua.x), ub.y.atan2(ub.x));
                    while tb - ta > std::f64::consts::PI { tb -= std::f64::consts::TAU; }
                    while ta - tb > std::f64::consts::PI { tb += std::f64::consts::TAU; }
                    let t = ta + (tb - ta) * f;
                    u = Point::new(t.cos(), t.sin());
                }
                WinderStyle::SinglePoint if (s - c).abs() < width / 2.0 => {
                    // through the corner's inside point
                    let to = unit(sub(inside, o));
                    u = if cross(a, b) > 0.0 { to } else { mul(to, -1.0) };
                }
                _ => {}
            }
        }
        (o, u)
    };
    build(walk, &across, left, right, steps, width)
}

/// A polyline's points with their distances along it.
fn measured(pts: &[Point]) -> Vec<(Point, f64)> {
    let mut acc = 0.0;
    pts.iter().enumerate().map(|(i, p)| { if i > 0 { acc += pts[i - 1].dist(p); } (*p, acc) }).collect()
}

/// A sketched run: its left and right boundaries (in the direction of travel) and its riser lines from the bottom up
/// (each from the left boundary to the right). The walkline joins the risers' middles; each riser's height is `tops`.
pub fn sketched(left: &[Point], right: &[Point], risers: &[[Point; 2]], tops: &[f64]) -> RunPath {
    let (lm, rm) = (measured(left), measured(right));
    // the walkline through the risers' middles, with its distances
    let mids: Vec<Point> = risers.iter().map(|[a, b]| Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)).collect();
    let walk = measured(&mids);
    let ul: Vec<f64> = risers.iter().map(|[a, _]| param(&lm, *a)).collect();
    let ur: Vec<f64> = risers.iter().map(|[_, b]| param(&rm, *b)).collect();
    // an edge's points between the first and last riser, each with the walkline distance it lies at
    let edge = |m: &[(Point, f64)], u: &[f64], ends: &dyn Fn(usize) -> Point| -> Vec<(Point, f64)> {
        let mut out = vec![(ends(0), walk[0].1)];
        for k in 0..u.len().saturating_sub(1) {
            for (p, v) in m.iter().filter(|(_, v)| *v > u[k] + 1e-6 && *v < u[k + 1] - 1e-6) {
                let f = (v - u[k]) / (u[k + 1] - u[k]).max(1e-9);
                out.push((*p, walk[k].1 + f * (walk[k + 1].1 - walk[k].1)));
            }
            out.push((ends(k + 1), walk[k + 1].1));
        }
        out
    };
    let left_edge = edge(&lm, &ul, &|k| risers[k][0]);
    let right_edge = edge(&rm, &ur, &|k| risers[k][1]);
    let treads = (0..risers.len().saturating_sub(1)).map(|k| {
        let ([l0, r0], [l1, r1]) = (risers[k], risers[k + 1]);
        let mut poly = vec![l0, r0];
        poly.extend(rm.iter().filter(|(_, v)| *v > ur[k] + 1e-6 && *v < ur[k + 1] - 1e-6).map(|(p, _)| *p));
        poly.extend([r1, l1]);
        let mut back: Vec<Point> = lm.iter().filter(|(_, v)| *v > ul[k] + 1e-6 && *v < ul[k + 1] - 1e-6).map(|(p, _)| *p).collect();
        back.reverse();
        poly.extend(back);
        poly.dedup_by(|a, b| a.dist(b) < 1e-6);
        if crate::geom::signed_area(&poly) < 0.0 { poly.reverse(); }
        (poly, tops[k])
    }).collect();
    RunPath { walk, riser_lines: risers.to_vec(), treads, left: left_edge, right: right_edge }
}

/// Sort sketched lines into a run: the two boundaries (oriented up the run, left then right) and the riser lines from
/// the bottom (the first one drawn) up, each from left to right. None when they do not make a run.
pub fn arrange(boundaries: &[Vec<Point>], risers: &[[Point; 2]]) -> Option<(Vec<Point>, Vec<Point>, Vec<[Point; 2]>)> {
    if boundaries.len() != 2 || risers.len() < 2 { return None; }
    let a = measured(&boundaries[0]);
    // the risers in order along the first boundary, the first one drawn at the bottom
    let mut order: Vec<(f64, [Point; 2])> = risers.iter().map(|r| {
        let m = Point::new((r[0].x + r[1].x) / 2.0, (r[0].y + r[1].y) / 2.0);
        (param(&a, m), *r)
    }).collect();
    let first = order[0].1;
    order.sort_by(|x, y| x.0.total_cmp(&y.0));
    if order.last().is_some_and(|x| x.1 == first) { order.reverse(); }
    let mut rs: Vec<[Point; 2]> = order.into_iter().map(|x| x.1).collect();
    // up the run: from the first riser's middle to the last's
    let m = |r: &[Point; 2]| Point::new((r[0].x + r[1].x) / 2.0, (r[0].y + r[1].y) / 2.0);
    let up = unit(sub(m(rs.last()?), m(&rs[0])));
    let mut bs: Vec<Vec<Point>> = boundaries.to_vec();
    for b in bs.iter_mut() {
        let d = sub(*b.last()?, b[0]);
        if d.x * up.x + d.y * up.y < 0.0 { b.reverse(); }
    }
    // the left boundary lies to the left of the walk
    let mid = m(&rs[0]);
    let side = |b: &Vec<Point>| { let c = b[b.len() / 2]; cross(up, sub(c, mid)) };
    if side(&bs[0]) < side(&bs[1]) { bs.swap(0, 1); }
    // each riser from the left boundary's end to the right's
    let bl = measured(&bs[0]);
    for r in rs.iter_mut() {
        let (d0, d1) = (param_dist(&bl, r[0]), param_dist(&bl, r[1]));
        if d1 < d0 { r.swap(0, 1); }
    }
    Some((bs[0].clone(), bs[1].clone(), rs))
}

/// How far a point is from a polyline.
fn param_dist(pts: &[(Point, f64)], p: Point) -> f64 {
    pts.windows(2).map(|w| {
        let (a, b) = (w[0].0, w[1].0);
        let ab = sub(b, a);
        let l2 = (ab.x * ab.x + ab.y * ab.y).max(1e-12);
        let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / l2).clamp(0.0, 1.0);
        add(a, mul(ab, t)).dist(&p)
    }).fold(f64::INFINITY, f64::min)
}
