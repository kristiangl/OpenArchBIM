//! Sketch tools (the sketch mode): the geometry behind each Draw and Modify tool.
//!
//! Every function is pure: it takes the picks the user has made so far and returns curves, so
//! the plan view can ask for a preview on every mouse move and commit the same answer on a
//! click. The sketch itself is just a list of curves; `curve::sketch_loops` checks it when it
//! is finished. Floors use this now; roofs, ceilings, openings and slab edges will too.

use crate::curve::{arc_sweep, Curve};
use crate::doc::Document;
use crate::element::*;
use crate::geom::Point;
use crate::tempdim::WallRef;

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}
fn add(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}
fn scale(a: Point, k: f64) -> Point {
    Point::new(a.x * k, a.y * k)
}
fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}
fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}
fn len(a: Point) -> f64 {
    a.x.hypot(a.y)
}
fn unit(a: Point) -> Option<Point> {
    let l = len(a);
    if l < 1e-9 { None } else { Some(scale(a, 1.0 / l)) }
}
fn ang(c: Point, p: Point) -> f64 {
    (p.y - c.y).atan2(p.x - c.x)
}
fn polar(c: Point, r: f64, a: f64) -> Point {
    Point::new(c.x + r * a.cos(), c.y + r * a.sin())
}

/// Centre and radius of the circle through three points.
pub fn circle_through(a: Point, b: Point, c: Point) -> Option<(Point, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (a.x * a.x + a.y * a.y, b.x * b.x + b.y * b.y, c.x * c.x + c.y * c.y);
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let centre = Point::new(ux, uy);
    Some((centre, centre.dist(&a)))
}

/// Intersection of the infinite lines a-b and c-d.
pub fn line_line(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r, s) = (sub(b, a), sub(d, c));
    let den = cross(r, s);
    if den.abs() < 1e-9 * (len(r) * len(s)).max(1e-12) {
        return None;
    }
    let t = cross(sub(c, a), s) / den;
    Some(add(a, scale(r, t)))
}

// ---- Draw tools --------------------------------------------------------------------

/// Straight edges and arcs for a closed polygon, with every corner rounded to `radius` (0 for sharp).
pub fn rounded_polygon(pts: &[Point], radius: f64) -> Vec<Curve> {
    let n = pts.len();
    if radius <= 1e-6 {
        return (0..n).map(|i| Curve::line(pts[i], pts[(i + 1) % n])).collect();
    }
    // tangent points and arc for every corner
    struct Corner {
        t_in: Point,
        t_out: Point,
        centre: Point,
        ccw: bool,
    }
    let mut corners: Vec<Corner> = vec![];
    for i in 0..n {
        let (p0, p1, p2) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let (u, v) = (unit(sub(p0, p1)), unit(sub(p2, p1)));
        let (Some(u), Some(v)) = (u, v) else { return (0..n).map(|i| Curve::line(pts[i], pts[(i + 1) % n])).collect() };
        let theta = dot(u, v).clamp(-1.0, 1.0).acos();
        if theta < 1e-3 || (std::f64::consts::PI - theta) < 1e-3 {
            corners.push(Corner { t_in: p1, t_out: p1, centre: p1, ccw: true });
            continue;
        }
        let max_d = (p0.dist(&p1).min(p1.dist(&p2))) / 2.0;
        let r = radius.min(max_d * (theta / 2.0).tan());
        let d = r / (theta / 2.0).tan();
        let bis = unit(add(u, v)).unwrap_or(u);
        let centre = add(p1, scale(bis, r / (theta / 2.0).sin()));
        // the arc turns the way the polygon turns at this corner
        let turn = cross(sub(p1, p0), sub(p2, p1));
        corners.push(Corner { t_in: add(p1, scale(u, d)), t_out: add(p1, scale(v, d)), centre, ccw: turn > 0.0 });
    }
    let mut out = vec![];
    for i in 0..n {
        let c = &corners[i];
        if c.t_in.dist(&c.t_out) > 1e-6 {
            out.push(Curve::Arc { start: c.t_in, end: c.t_out, centre: c.centre, ccw: c.ccw });
        }
        let next = &corners[(i + 1) % n];
        if c.t_out.dist(&next.t_in) > 1e-6 {
            out.push(Curve::line(c.t_out, next.t_in));
        }
    }
    out
}

/// Rectangle with sides along the axes through two opposite corners.
pub fn rectangle(a: Point, b: Point, radius: f64) -> Vec<Curve> {
    let pts = [Point::new(a.x, a.y), Point::new(b.x, a.y), Point::new(b.x, b.y), Point::new(a.x, b.y)];
    // keep the winding counter-clockwise whichever way the corners were picked
    let ccw = (b.x - a.x) * (b.y - a.y) > 0.0;
    let v: Vec<Point> = if ccw { pts.to_vec() } else { pts.iter().rev().copied().collect() };
    rounded_polygon(&v, radius)
}

/// Regular polygon with its corners on a circle: `vertex` is one corner.
pub fn polygon_inscribed(centre: Point, vertex: Point, sides: usize, radius: f64) -> Vec<Curve> {
    let n = sides.clamp(3, 64);
    let (r, a0) = (centre.dist(&vertex), ang(centre, vertex));
    let pts: Vec<Point> = (0..n).map(|i| polar(centre, r, a0 + std::f64::consts::TAU * i as f64 / n as f64)).collect();
    rounded_polygon(&pts, radius)
}

/// Regular polygon with its sides touching a circle: `edge_mid` is the middle of one side.
pub fn polygon_circumscribed(centre: Point, edge_mid: Point, sides: usize, radius: f64) -> Vec<Curve> {
    let n = sides.clamp(3, 64);
    let apothem = centre.dist(&edge_mid);
    let r = apothem / (std::f64::consts::PI / n as f64).cos();
    let a0 = ang(centre, edge_mid) - std::f64::consts::PI / n as f64;
    let pts: Vec<Point> = (0..n).map(|i| polar(centre, r, a0 + std::f64::consts::TAU * i as f64 / n as f64)).collect();
    rounded_polygon(&pts, radius)
}

pub fn circle(centre: Point, on: Point) -> Option<Curve> {
    let r = centre.dist(&on);
    if r < 1.0 { None } else { Some(Curve::Circle { centre, radius: r }) }
}

/// Arc through three points (start, end, and a point on the arc).
pub fn arc_three_points(start: Point, end: Point, through: Point) -> Option<Curve> {
    let (centre, _) = circle_through(start, through, end)?;
    // start -> through -> end runs counter-clockwise when the triangle does
    let ccw = cross(sub(through, start), sub(end, start)) > 0.0;
    Some(Curve::Arc { start, end, centre, ccw })
}

/// Start-End-Radius arc: the arc between two points with the given radius, bulging towards `side`.
pub fn arc_start_end_radius(start: Point, end: Point, radius: f64, side: Point) -> Option<Curve> {
    let chord = start.dist(&end);
    if chord < 1e-6 || radius < chord / 2.0 - 1e-6 {
        return None;
    }
    let mid = scale(add(start, end), 0.5);
    let n = unit(Point::new(-(end.y - start.y), end.x - start.x))?; // left of start->end
    let h = (radius * radius - chord * chord / 4.0).max(0.0).sqrt();
    // the bulge is on the cursor's side; the centre is on the other side of the chord for a minor arc
    let bulge_left = cross(sub(end, start), sub(side, start)) > 0.0;
    let bulge = if bulge_left { n } else { scale(n, -1.0) };
    // the minor arc's middle sits `radius - h` out from the chord, on the bulge side
    let mid_arc = add(mid, scale(bulge, radius - h));
    arc_three_points(start, end, mid_arc)
}

/// Centre-ends arc: centre, start (sets the radius), and an end point that sets the end angle.
pub fn arc_centre_ends(centre: Point, start: Point, end_dir: Point, ccw: bool) -> Option<Curve> {
    let r = centre.dist(&start);
    if r < 1.0 || centre.dist(&end_dir) < 1e-6 {
        return None;
    }
    let end = polar(centre, r, ang(centre, end_dir));
    if start.dist(&end) < 1e-6 {
        return None;
    }
    Some(Curve::Arc { start, end, centre, ccw })
}

/// Tangent end arc: starts at `start` going along `tangent` (a direction), ends at `end`.
pub fn arc_tangent(start: Point, tangent: Point, end: Point) -> Option<Curve> {
    let t = unit(tangent)?;
    let chord = sub(end, start);
    let n = Point::new(-t.y, t.x); // left of the tangent
    // centre = start + n * k with |centre - end| = |k|  =>  k = |chord|^2 / (2 chord.n)
    let denom = 2.0 * dot(chord, n);
    if denom.abs() < 1e-9 {
        return None;
    }
    let k = dot(chord, chord) / denom;
    let centre = add(start, scale(n, k));
    // k > 0: the centre is on the left, so the arc turns left (counter-clockwise)
    Some(Curve::Arc { start, end, centre, ccw: k > 0.0 })
}

/// Fillet arc between two lines (Fillet Arc). Each line is trimmed or extended to the
/// arc and keeps the part on the side of its pick point.
pub fn fillet_lines(a: &Curve, pick_a: Point, b: &Curve, pick_b: Point, radius: f64) -> Result<Vec<Curve>, String> {
    let (Curve::Line { a: a0, b: a1 }, Curve::Line { a: b0, b: b1 }) = (a, b) else {
        return Err("a fillet joins two straight lines".into());
    };
    if radius <= 0.0 {
        return Err("give the fillet a radius".into());
    }
    let x = line_line(*a0, *a1, *b0, *b1).ok_or("those lines are parallel")?;
    // direction from the meeting point towards the part of each line that stays
    let toward = |p0: Point, p1: Point, pick: Point| {
        let d = unit(sub(p1, p0)).unwrap_or(Point::new(1.0, 0.0));
        if dot(sub(pick, x), d) >= 0.0 { d } else { scale(d, -1.0) }
    };
    let (u, v) = (toward(*a0, *a1, pick_a), toward(*b0, *b1, pick_b));
    let theta = dot(u, v).clamp(-1.0, 1.0).acos();
    if theta < 1e-3 || (std::f64::consts::PI - theta) < 1e-3 {
        return Err("those lines are in line with each other".into());
    }
    let d = radius / (theta / 2.0).tan();
    let (ta, tb) = (add(x, scale(u, d)), add(x, scale(v, d)));
    let bis = unit(add(u, v)).ok_or("those lines fold back on each other")?;
    let centre = add(x, scale(bis, radius / (theta / 2.0).sin()));
    // keep the far end of each line, on the pick side; the near end moves to the arc
    let far = |p0: Point, p1: Point, dir: Point| if dot(sub(p1, x), dir) > dot(sub(p0, x), dir) { p1 } else { p0 };
    let (fa, fb) = (far(*a0, *a1, u), far(*b0, *b1, v));
    if dot(sub(fa, x), u) < d - 1e-6 || dot(sub(fb, x), v) < d - 1e-6 {
        return Err("the radius is too big for those lines".into());
    }
    let ccw = cross(sub(ta, centre), sub(tb, centre)) > 0.0;
    Ok(vec![Curve::line(fa, ta), Curve::Arc { start: ta, end: tb, centre, ccw }, Curve::line(tb, fb)])
}

/// Ellipse from its centre, one end of an axis (sets the first radius and the angle) and a point
/// that sets the other radius.
pub fn ellipse(centre: Point, axis_end: Point, other: Point) -> Option<Curve> {
    let rx = centre.dist(&axis_end);
    let dir = unit(sub(axis_end, centre))?;
    let ry = cross(dir, sub(other, centre)).abs();
    if rx < 1.0 || ry < 1.0 {
        return None;
    }
    Some(Curve::Ellipse { centre, rx, ry, rotation_deg: ang(centre, axis_end).to_degrees(), t0_deg: None, t1_deg: None, ccw: true })
}

/// Parameter angle (degrees) on an ellipse for a point near it.
pub fn ellipse_param(e: &Curve, p: Point) -> Option<f64> {
    let Curve::Ellipse { centre, rx, ry, rotation_deg, .. } = e else { return None };
    let (s, c) = (-rotation_deg.to_radians()).sin_cos();
    let q = sub(p, *centre);
    let (lx, ly) = (q.x * c - q.y * s, q.x * s + q.y * c);
    Some((ly / ry).atan2(lx / rx).to_degrees())
}

/// Partial ellipse: the full ellipse's start and end pick points set the parameter range.
pub fn partial_ellipse(centre: Point, axis_end: Point, other: Point, from: Point, to: Point, ccw: bool) -> Option<Curve> {
    let full = ellipse(centre, axis_end, other)?;
    let (t0, t1) = (ellipse_param(&full, from)?, ellipse_param(&full, to)?);
    let Curve::Ellipse { centre, rx, ry, rotation_deg, .. } = full else { return None };
    Some(Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg: Some(t0), t1_deg: Some(t1), ccw })
}

pub fn spline(points: &[Point], closed: bool) -> Option<Curve> {
    if points.len() < 2 || (closed && points.len() < 3) {
        return None;
    }
    Some(Curve::Spline { points: points.to_vec(), closed })
}

// ---- Modify tools --------------------------------------------------------------------

fn line_ends(c: &Curve) -> Option<(Point, Point)> {
    if let Curve::Line { a, b } = c { Some((*a, *b)) } else { None }
}

/// What a line or arc lies on: the infinite line through it, or its whole circle.
#[derive(Clone, Copy)]
enum Carrier {
    Line(Point, Point),
    Circle(Point, f64),
}

fn carrier(c: &Curve) -> Option<Carrier> {
    match c {
        Curve::Line { a, b } => Some(Carrier::Line(*a, *b)),
        Curve::Arc { start, centre, .. } => Some(Carrier::Circle(*centre, centre.dist(start))),
        Curve::Circle { centre, radius } => Some(Carrier::Circle(*centre, *radius)),
        _ => None,
    }
}

/// Where two circles cross.
fn circle_circle(c1: Point, r1: f64, c2: Point, r2: f64) -> Vec<Point> {
    let d = c1.dist(&c2);
    if d < 1e-9 || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
        return vec![];
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let u = scale(sub(c2, c1), 1.0 / d);
    let m = add(c1, scale(u, a));
    let n = Point::new(-u.y, u.x);
    if h < 1e-9 { vec![m] } else { vec![add(m, scale(n, h)), sub(m, scale(n, h))] }
}

/// Where two carriers meet.
fn meet(a: Carrier, b: Carrier) -> Vec<Point> {
    match (a, b) {
        (Carrier::Line(p, q), Carrier::Line(r, s)) => line_line(p, q, r, s).into_iter().collect(),
        (Carrier::Line(p, q), Carrier::Circle(c, r)) | (Carrier::Circle(c, r), Carrier::Line(p, q)) => line_circle(p, q, c, r),
        (Carrier::Circle(c1, r1), Carrier::Circle(c2, r2)) => circle_circle(c1, r1, c2, r2),
    }
}

/// How far round an arc a point on its circle is, from its start in the arc's own direction (radians, 0 to 2 pi).
fn along_arc(p: Point, start: Point, centre: Point, ccw: bool) -> f64 {
    let d = ang(centre, p) - ang(centre, start);
    (if ccw { d } else { -d }).rem_euclid(std::f64::consts::TAU)
}

/// A line or arc trimmed or extended to `x` (a point on its line or circle), keeping the part that holds `pick`.
fn trim_curve_at(c: &Curve, pick: Point, x: Point) -> Result<Curve, String> {
    match c {
        Curve::Line { a, b } => {
            let d = sub(*b, *a);
            let l2 = dot(d, d);
            let t = |q: Point| dot(sub(q, *a), d) / l2;
            Ok(if t(pick) < t(x) { Curve::line(*a, x) } else { Curve::line(x, *b) })
        }
        Curve::Arc { start, end, centre, ccw } => {
            let (sweep, tx, tp) = (along_arc(*end, *start, *centre, *ccw), along_arc(x, *start, *centre, *ccw), along_arc(pick, *start, *centre, *ccw));
            let out = if tx <= sweep + 1e-9 {
                // on the arc: the piece with the pick
                if tp < tx { (*start, x) } else { (x, *end) }
            } else if tx - sweep < std::f64::consts::TAU - tx {
                // past the end: the end goes on to it
                (*start, x)
            } else {
                // before the start: the start goes back to it
                (x, *end)
            };
            if out.0.dist(&out.1) < 1e-6 {
                return Err("that would leave nothing of the arc".into());
            }
            Ok(Curve::Arc { start: out.0, end: out.1, centre: *centre, ccw: *ccw })
        }
        _ => Err("only lines and arcs can be trimmed".into()),
    }
}

/// Trim or extend two lines or arcs to the corner where they meet (the meeting point nearest the picks), keeping
/// the part of each that holds its pick point.
pub fn trim_corner(a: &Curve, pick_a: Point, b: &Curve, pick_b: Point) -> Result<(Curve, Curve), String> {
    if matches!(a, Curve::Circle { .. }) || matches!(b, Curve::Circle { .. }) {
        return Err("a circle is trimmed with Trim/Extend Single Element: pick the line that cuts it first".into());
    }
    let (ca, cb) = (carrier(a).ok_or("trim to corner works on lines and arcs")?, carrier(b).ok_or("trim to corner works on lines and arcs")?);
    let x = meet(ca, cb).into_iter().min_by(|p, q| (p.dist(&pick_a) + p.dist(&pick_b)).total_cmp(&(q.dist(&pick_a) + q.dist(&pick_b)))).ok_or("those never meet")?;
    Ok((trim_curve_at(a, pick_a, x)?, trim_curve_at(b, pick_b, x)?))
}

/// Intersections of the infinite line p-q with a circle.
fn line_circle(p: Point, q: Point, centre: Point, r: f64) -> Vec<Point> {
    let d = sub(q, p);
    let f = sub(p, centre);
    let (a, b, c) = (dot(d, d), 2.0 * dot(f, d), dot(f, f) - r * r);
    let disc = b * b - 4.0 * a * c;
    if a < 1e-12 || disc < 0.0 {
        return vec![];
    }
    let s = disc.sqrt();
    vec![(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)].into_iter().map(|t| add(p, scale(d, t))).collect()
}

fn on_arc(p: Point, start: Point, end: Point, centre: Point, ccw: bool) -> bool {
    let sweep = arc_sweep(start, end, centre, ccw);
    let a = (ang(centre, p) - ang(centre, start)).rem_euclid(std::f64::consts::TAU);
    if ccw { a <= sweep + 1e-9 } else { (std::f64::consts::TAU - a).rem_euclid(std::f64::consts::TAU) <= -sweep + 1e-9 }
}

/// Trim or extend a line or arc to meet a boundary (line, arc or circle), keeping the part with the pick. A circle
/// cut by the boundary in two places becomes the arc between them that holds the pick.
pub fn trim_to(curve: &Curve, pick: Point, boundary: &Curve) -> Result<Curve, String> {
    let own = carrier(curve).ok_or("only lines, arcs and circles can be trimmed")?;
    let mut hits = meet(own, carrier(boundary).ok_or("trim to a line, arc or circle")?);
    // an arc boundary: where it actually is, when the curve reaches it
    if let Curve::Arc { start, end, centre, ccw } = boundary {
        let on: Vec<Point> = hits.iter().copied().filter(|p| on_arc(*p, *start, *end, *centre, *ccw)).collect();
        if !on.is_empty() {
            hits = on;
        }
    }
    if let Curve::Circle { centre, .. } = curve {
        if hits.len() < 2 {
            return Err("the boundary has to cut the circle in two places".into());
        }
        // the arc from one cut to the next, going round, that holds the pick
        let mut by: Vec<(f64, Point)> = hits.iter().map(|p| (ang(*centre, *p).rem_euclid(std::f64::consts::TAU), *p)).collect();
        by.sort_by(|a, b| a.0.total_cmp(&b.0));
        let tp = ang(*centre, pick).rem_euclid(std::f64::consts::TAU);
        let i = by.iter().rposition(|(t, _)| *t <= tp).unwrap_or(by.len() - 1);
        let (s, e) = (by[i].1, by[(i + 1) % by.len()].1);
        return Ok(Curve::Arc { start: s, end: e, centre: *centre, ccw: true });
    }
    // the meeting point to go to: on a line the nearest the pick; on an arc one it already reaches (nearest the pick
    // round the arc), else the one it reaches by going on the least
    let cost = |x: &Point| match curve {
        Curve::Arc { start, end, centre, ccw } => {
            let (sweep, tx, tp) = (along_arc(*end, *start, *centre, *ccw), along_arc(*x, *start, *centre, *ccw), along_arc(pick, *start, *centre, *ccw));
            if tx <= sweep + 1e-9 { (tx - tp).abs() } else { std::f64::consts::TAU + (tx - sweep).min(std::f64::consts::TAU - tx) }
        }
        _ => x.dist(&pick),
    };
    let x = hits.into_iter().min_by(|p, q| cost(p).total_cmp(&cost(q))).ok_or("that never meets the boundary")?;
    trim_curve_at(curve, pick, x)
}

/// Offset one curve by `dist` towards the side of `towards` (a point). Lines, arcs and circles.
pub fn offset_curve(c: &Curve, towards: Point, dist: f64) -> Result<Curve, String> {
    if dist <= 0.0 {
        return Err("the offset must be greater than zero".into());
    }
    match c {
        Curve::Line { a, b } => {
            let n = unit(Point::new(-(b.y - a.y), b.x - a.x)).ok_or("the line has no length")?;
            let side = if cross(sub(*b, *a), sub(towards, *a)) >= 0.0 { 1.0 } else { -1.0 };
            let s = scale(n, side * dist);
            Ok(Curve::line(add(*a, s), add(*b, s)))
        }
        Curve::Circle { centre, radius } => {
            let out = centre.dist(&towards) > *radius;
            let r = if out { radius + dist } else { radius - dist };
            if r < 1.0 { Err("the offset would collapse the circle".into()) } else { Ok(Curve::Circle { centre: *centre, radius: r }) }
        }
        Curve::Arc { start, end, centre, ccw } => {
            let r = centre.dist(start);
            let out = centre.dist(&towards) > r;
            let nr = if out { r + dist } else { r - dist };
            if nr < 1.0 {
                return Err("the offset would collapse the arc".into());
            }
            let (a0, a1) = (ang(*centre, *start), ang(*centre, *end));
            Ok(Curve::Arc { start: polar(*centre, nr, a0), end: polar(*centre, nr, a1), centre: *centre, ccw: *ccw })
        }
        _ => Err("offsetting ellipses and splines is not supported yet".into()),
    }
}

/// Offset a head-to-tail run of straight lines (open or closed), mitring the corners. `towards`
/// picks the side for the first line; the rest follow it.
pub fn offset_chain(chain: &[Curve], towards: Point, dist: f64) -> Result<Vec<Curve>, String> {
    if dist <= 0.0 {
        return Err("the offset must be greater than zero".into());
    }
    let lines: Vec<(Point, Point)> = chain.iter().map(|c| line_ends(c).ok_or("a chain offset works on straight lines; turn Chain off for arcs")).collect::<Result<_, _>>()?;
    let n = lines.len();
    if n == 0 {
        return Err("nothing to offset".into());
    }
    let (a, b) = lines[0];
    let side = if cross(sub(b, a), sub(towards, a)) >= 0.0 { 1.0 } else { -1.0 };
    let shifted: Vec<(Point, Point)> = lines.iter().map(|(p, q)| {
        let nrm = unit(Point::new(-(q.y - p.y), q.x - p.x)).unwrap_or(Point::new(0.0, 1.0));
        let s = scale(nrm, side * dist);
        (add(*p, s), add(*q, s))
    }).collect();
    let closed = n > 2 && lines[n - 1].1.dist(&lines[0].0) < 1.0;
    // corner between line i and the next
    let corner = |i: usize| -> Point {
        let j = (i + 1) % n;
        line_line(shifted[i].0, shifted[i].1, shifted[j].0, shifted[j].1).unwrap_or(shifted[i].1)
    };
    let mut out = vec![];
    for i in 0..n {
        let start = if i == 0 && !closed { shifted[0].0 } else { corner((i + n - 1) % n) };
        let end = if i == n - 1 && !closed { shifted[i].1 } else { corner(i) };
        if start.dist(&end) < 1e-6 {
            return Err("the offset would collapse a line".into());
        }
        out.push(Curve::line(start, end));
    }
    Ok(out)
}

/// Split a line or arc in two at the point of it nearest `at` (Split Element).
pub fn split_curve(c: &Curve, at: Point) -> Result<(Curve, Curve), String> {
    match c {
        Curve::Line { a, b } => {
            let d = sub(*b, *a);
            let t = (dot(sub(at, *a), d) / dot(d, d)).clamp(0.0, 1.0);
            let m = add(*a, scale(d, t));
            if m.dist(a) < 1.0 || m.dist(b) < 1.0 {
                return Err("split the line away from its ends".into());
            }
            Ok((Curve::line(*a, m), Curve::line(m, *b)))
        }
        Curve::Arc { start, end, centre, ccw } => {
            let r = centre.dist(start);
            let m = polar(*centre, r, ang(*centre, at));
            if !on_arc(m, *start, *end, *centre, *ccw) || m.dist(start) < 1.0 || m.dist(end) < 1.0 {
                return Err("split the arc away from its ends".into());
            }
            Ok((Curve::Arc { start: *start, end: m, centre: *centre, ccw: *ccw }, Curve::Arc { start: m, end: *end, centre: *centre, ccw: *ccw }))
        }
        _ => Err("only lines and arcs can be split".into()),
    }
}

// ---- Pick Walls ------------------------------------------------------------------------

/// Which line of a wall Pick Walls takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PickFace {
    /// The exterior face (the default, as the person draws a building outline).
    #[default]
    Exterior,
    /// The interior face.
    Interior,
    /// The middle of the wall body (the core's middle when measuring to the core).
    Centre,
    /// The face on the side of the cursor (the old behaviour; the middle is not offered).
    Cursor,
}

impl PickFace {
    pub fn parse(s: &str) -> Option<PickFace> {
        Some(match s {
            "exterior" | "external" => PickFace::Exterior,
            "interior" | "internal" => PickFace::Interior,
            "centre" | "center" | "centreline" => PickFace::Centre,
            "cursor" => PickFace::Cursor,
            _ => return None,
        })
    }
}

impl Document {
    /// Sketch lines along walls (the Pick Walls). `ids` is one wall or a connected run of walls.
    /// `face` says which line of each wall to take: its exterior face, interior face or the middle.
    /// `pick` is the cursor, which decides the side only for `PickFace::Cursor` and the direction of
    /// `offset` for `PickFace::Centre`. `offset` moves the line away from the wall (negative goes into it),
    /// and `to_core` measures from the wall's core instead of its finish face ("Extend into wall (to core)").
    /// Neighbouring lines meet at mitred corners.
    pub fn pick_wall_lines(&self, ids: &[Id], pick: Point, offset: f64, to_core: bool, face: PickFace) -> Result<Vec<Curve>, String> {
        let order = crate::edit::chain_order(self, ids)?;
        if order.is_empty() {
            return Err("pick a wall".into());
        }
        let (_, f0, t0) = order[0];
        let side = if cross(sub(t0, f0), sub(pick, f0)) >= 0.0 { 1.0 } else { -1.0 };
        // each wall's line in the run's direction of travel
        let mut lines: Vec<(Point, Point)> = vec![];
        for (id, from, to) in &order {
            let Some(Element::Wall(w)) = self.get(*id) else { return Err("not a wall".into()) };
            let same = w.start.dist(from) < w.end.dist(from);
            // offsets across the wall from its location line, positive to the left of the wall's own direction
            let (l, r) = self.wall_extents(w);
            let (mut left, mut right) = if to_core {
                let c = self.wall_ref_offsets_pub(w, WallRef::CoreFaces);
                (c[0], c[c.len() - 1])
            } else {
                (l, -r)
            };
            // the exterior is on the left of the wall's own direction unless the wall is flipped
            let mut ext_left = !w.flipped;
            if !same {
                (left, right) = (-right, -left);
                ext_left = !ext_left;
            }
            // `away` is +1 when positive offsets move the line to the left of the run's direction
            let (pos, away) = match face {
                PickFace::Exterior => if ext_left { (left, 1.0) } else { (right, -1.0) },
                PickFace::Interior => if ext_left { (right, -1.0) } else { (left, 1.0) },
                PickFace::Centre => ((left + right) / 2.0, side),
                PickFace::Cursor => if side > 0.0 { (left, 1.0) } else { (right, -1.0) },
            };
            let pos = pos + away * offset;
            let d = unit(sub(*to, *from)).ok_or("a wall has no length")?;
            let n = Point::new(-d.y, d.x);
            lines.push((add(*from, scale(n, pos)), add(*to, scale(n, pos))));
        }
        let n = lines.len();
        let closed = n > 2 && order[n - 1].2.dist(&order[0].1) < 1.0;
        let corner = |i: usize| -> Option<Point> {
            let j = (i + 1) % n;
            line_line(lines[i].0, lines[i].1, lines[j].0, lines[j].1)
        };
        let mut out = vec![];
        for i in 0..n {
            let start = if i == 0 && !closed { lines[0].0 } else { corner((i + n - 1) % n).unwrap_or(lines[i].0) };
            let end = if i == n - 1 && !closed { lines[i].1 } else { corner(i).unwrap_or(lines[i].1) };
            if start.dist(&end) > 1e-6 {
                out.push(Curve::line(start, end));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::{loop_polygon, sketch_loops};

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn closed(c: &[Curve]) -> bool {
        sketch_loops(c).is_ok()
    }

    #[test]
    fn rectangle_with_and_without_a_radius_is_a_closed_loop() {
        let r = rectangle(p(0.0, 0.0), p(4000.0, 3000.0), 0.0);
        assert_eq!(r.len(), 4);
        assert!(closed(&r));
        let rr = rectangle(p(4000.0, 3000.0), p(0.0, 0.0), 500.0); // corners picked the other way round
        assert_eq!(rr.len(), 8, "four sides and four corner arcs");
        assert!(closed(&rr));
        let area = crate::geom::signed_area(&loop_polygon(&sketch_loops(&rr).unwrap()[0], 0.2)).abs();
        let expect = 4000.0 * 3000.0 - (4.0 - std::f64::consts::PI) * 500.0 * 500.0;
        assert!((area - expect).abs() < 5000.0, "{area} vs {expect}");
    }

    #[test]
    fn polygons_inscribed_and_circumscribed() {
        let i = polygon_inscribed(p(0.0, 0.0), p(1000.0, 0.0), 6, 0.0);
        assert_eq!(i.len(), 6);
        assert!(i.iter().all(|c| (c.length() - 1000.0).abs() < 1e-6), "a hexagon's side is its radius");
        let o = polygon_circumscribed(p(0.0, 0.0), p(1000.0, 0.0), 4, 0.0);
        let poly = loop_polygon(&sketch_loops(&o).unwrap()[0], 1.0);
        assert!((crate::geom::signed_area(&poly).abs() - 4_000_000.0).abs() < 1.0, "a square 2000 across");
        assert!(closed(&polygon_inscribed(p(0.0, 0.0), p(1000.0, 0.0), 5, 200.0)));
    }

    #[test]
    fn arcs_three_ways() {
        let a = arc_three_points(p(0.0, 0.0), p(2000.0, 0.0), p(1000.0, 1000.0)).unwrap();
        // the arc passes through the third point: a semicircle bulging up
        assert!(a.tessellate(1.0).iter().any(|q| q.dist(&p(1000.0, 1000.0)) < 2.0));
        let b = arc_start_end_radius(p(0.0, 0.0), p(2000.0, 0.0), 1000.0, p(1000.0, -5.0)).unwrap();
        assert!(b.tessellate(1.0).iter().all(|q| q.y <= 1e-6), "bulges towards the side asked for");
        assert!(arc_start_end_radius(p(0.0, 0.0), p(2000.0, 0.0), 500.0, p(1000.0, 5.0)).is_none(), "radius too small");
        let c = arc_centre_ends(p(0.0, 0.0), p(1000.0, 0.0), p(0.0, 5000.0), true).unwrap();
        assert!((c.length() - 1000.0 * std::f64::consts::FRAC_PI_2).abs() < 3.0);
        // tangent to a line running east, curving up to a point 1000 east and 1000 north: a quarter circle
        let t = arc_tangent(p(0.0, 0.0), p(1.0, 0.0), p(1000.0, 1000.0)).unwrap();
        assert!((t.length() - 1000.0 * std::f64::consts::FRAC_PI_2).abs() < 3.0, "{}", t.length());
        let first = t.tessellate(1.0)[1];
        assert!(first.y > 0.0 && first.x > 0.0);
    }

    #[test]
    fn fillet_rounds_a_corner_and_trims_both_lines() {
        let a = Curve::line(p(0.0, 0.0), p(4000.0, 0.0));
        let b = Curve::line(p(4000.0, 0.0), p(4000.0, 3000.0));
        let f = fillet_lines(&a, p(1000.0, 0.0), &b, p(4000.0, 1000.0), 500.0).unwrap();
        assert_eq!(f.len(), 3);
        assert!(f[0].end().dist(&p(3500.0, 0.0)) < 1e-6);
        assert!(f[2].start().dist(&p(4000.0, 500.0)) < 1e-6);
        let Curve::Arc { centre, ccw, .. } = &f[1] else { panic!() };
        assert!(centre.dist(&p(3500.0, 500.0)) < 1e-6 && *ccw);
        assert!(fillet_lines(&a, p(10.0, 0.0), &b, p(4000.0, 100.0), 9000.0).is_err(), "radius too big");
    }

    #[test]
    fn ellipses_full_and_partial() {
        let e = ellipse(p(0.0, 0.0), p(1000.0, 0.0), p(0.0, 400.0)).unwrap();
        let Curve::Ellipse { rx, ry, rotation_deg, .. } = e else { panic!() };
        assert!((rx - 1000.0).abs() < 1e-9 && (ry - 400.0).abs() < 1e-9 && rotation_deg.abs() < 1e-9);
        let half = partial_ellipse(p(0.0, 0.0), p(1000.0, 0.0), p(0.0, 400.0), p(1000.0, 0.0), p(-1000.0, 0.0), true).unwrap();
        assert!(half.start().dist(&p(1000.0, 0.0)) < 1e-6 && half.end().dist(&p(-1000.0, 0.0)) < 1e-6);
        let mut c = vec![half.clone(), Curve::line(half.end(), half.start())];
        assert!(closed(&c), "a half ellipse closed by its chord");
        c[1] = c[1].reversed();
        assert!(closed(&c));
    }

    #[test]
    fn trim_and_extend_lines_to_corners_and_curves() {
        let a = Curve::line(p(0.0, 0.0), p(3000.0, 0.0));
        let b = Curve::line(p(4000.0, -1000.0), p(4000.0, 2000.0));
        // extend a to meet b, and trim b back to the corner keeping the part with the pick
        let (na, nb) = trim_corner(&a, p(500.0, 0.0), &b, p(4000.0, 1500.0)).unwrap();
        assert_eq!(na, Curve::line(p(0.0, 0.0), p(4000.0, 0.0)));
        assert_eq!(nb, Curve::line(p(4000.0, 0.0), p(4000.0, 2000.0)));
        assert!(trim_corner(&a, p(0.0, 0.0), &Curve::line(p(0.0, 5.0), p(3000.0, 5.0)), p(1.0, 5.0)).is_err());
        let circ = Curve::Circle { centre: p(2000.0, 0.0), radius: 500.0 };
        let t = trim_to(&Curve::line(p(0.0, 0.0), p(5000.0, 0.0)), p(100.0, 0.0), &circ).unwrap();
        assert_eq!(t, Curve::line(p(0.0, 0.0), p(1500.0, 0.0)));
        assert!(trim_to(&Curve::line(p(0.0, 9000.0), p(5000.0, 9000.0)), p(0.0, 9000.0), &circ).is_err());
    }

    #[test]
    fn trim_and_extend_arcs_and_cut_circles() {
        let near = |a: Point, b: Point| a.dist(&b) < 1e-6;
        // an arc (radius 1000 round the origin, from 0 to 90 degrees) trimmed to the line x = 500, keeping the top
        let arc = Curve::Arc { start: p(1000.0, 0.0), end: p(0.0, 1000.0), centre: p(0.0, 0.0), ccw: true };
        let cut = Curve::line(p(500.0, -2000.0), p(500.0, 2000.0));
        let Curve::Arc { start, end, .. } = trim_to(&arc, p(0.0, 1000.0), &cut).unwrap() else { panic!() };
        assert!(near(start, p(500.0, 866.0254037844387)) && near(end, p(0.0, 1000.0)), "{start:?} {end:?}");
        // ... and extended to the line x = -500 past its end
        let Curve::Arc { start, end, .. } = trim_to(&arc, p(1000.0, 0.0), &Curve::line(p(-500.0, -2000.0), p(-500.0, 2000.0))).unwrap() else { panic!() };
        assert!(near(start, p(1000.0, 0.0)) && near(end, p(-500.0, 866.0254037844387)));
        // a line trimmed to the arc, and the corner of a line and an arc
        let l = Curve::line(p(0.0, 0.0), p(2000.0, 2000.0));
        let Curve::Line { b, .. } = trim_to(&l, p(100.0, 100.0), &arc).unwrap() else { panic!() };
        assert!(near(b, p(707.1067811865476, 707.1067811865476)));
        let (nl, na) = trim_corner(&Curve::line(p(300.0, 0.0), p(300.0, 500.0)), p(300.0, 100.0), &arc, p(0.0, 1000.0)).unwrap();
        let Curve::Line { b, .. } = nl else { panic!() };
        let Curve::Arc { start, .. } = na else { panic!() };
        assert!(near(b, start) && near(b, p(300.0, 953.9392014169456)), "the line runs up to the arc and the arc ends there");
        // a circle cut by a line across it becomes the arc on the side of the pick
        let circ = Curve::Circle { centre: p(0.0, 0.0), radius: 1000.0 };
        let Curve::Arc { start, end, ccw, .. } = trim_to(&circ, p(-1000.0, 0.0), &cut).unwrap() else { panic!() };
        assert!(ccw && near(start, p(500.0, 866.0254037844387)) && near(end, p(500.0, -866.0254037844387)), "{start:?} {end:?}");
        assert!(trim_corner(&circ, p(0.0, 1000.0), &cut, p(500.0, 0.0)).is_err());
    }

    #[test]
    fn offsets_curves_and_chains() {
        let l = offset_curve(&Curve::line(p(0.0, 0.0), p(4000.0, 0.0)), p(100.0, -50.0), 300.0).unwrap();
        assert_eq!(l, Curve::line(p(0.0, -300.0), p(4000.0, -300.0)));
        let c = offset_curve(&Curve::Circle { centre: p(0.0, 0.0), radius: 500.0 }, p(0.0, 0.0), 100.0).unwrap();
        assert_eq!(c, Curve::Circle { centre: p(0.0, 0.0), radius: 400.0 });
        assert!(offset_curve(&Curve::Circle { centre: p(0.0, 0.0), radius: 500.0 }, p(0.0, 0.0), 600.0).is_err());
        let a = offset_curve(&Curve::Arc { start: p(1000.0, 0.0), end: p(0.0, 1000.0), centre: p(0.0, 0.0), ccw: true }, p(2000.0, 2000.0), 200.0).unwrap();
        assert!((a.length() - 1200.0 * std::f64::consts::FRAC_PI_2).abs() < 3.0);
        // a rectangle offset inwards keeps its corners
        let rect = rectangle(p(0.0, 0.0), p(4000.0, 3000.0), 0.0);
        let inset = offset_chain(&rect, p(2000.0, 1500.0), 250.0).unwrap();
        assert!(closed(&inset));
        let xs: Vec<f64> = inset.iter().flat_map(|c| [c.start().x, c.end().x]).collect();
        assert!(xs.iter().cloned().fold(f64::MAX, f64::min) == 250.0 && xs.iter().cloned().fold(f64::MIN, f64::max) == 3750.0);
    }

    #[test]
    fn pick_walls_takes_the_face_or_the_core_with_an_offset() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0; // 110 brick, 40 cavity, 90 frame (core), 13 board
        let mut ids = vec![];
        for (a, b) in [((0.0, 0.0), (6000.0, 0.0)), ((6000.0, 0.0), (6000.0, 4000.0)), ((6000.0, 4000.0), (0.0, 4000.0)), ((0.0, 4000.0), (0.0, 0.0))] {
            ids.push(d.add_wall(wt, lvl, p(a.0, a.1), p(b.0, b.1), 2700.0).unwrap());
        }
        // clicking inside the room, the lines follow the room faces and meet at corners
        let inside = d.pick_wall_lines(&ids, p(3000.0, 2000.0), 0.0, false, PickFace::Cursor).unwrap();
        assert_eq!(inside.len(), 4);
        assert!(closed(&inside));
        let poly = loop_polygon(&sketch_loops(&inside).unwrap()[0], 1.0);
        let area = crate::geom::signed_area(&poly).abs();
        let w = d.wall_type(wt).unwrap().thickness_mm();
        let (l, r) = d.wall_extents(match d.get(ids[0]) { Some(Element::Wall(w)) => w, _ => unreachable!() });
        assert!(l + r == w);
        // the room is smaller than the centreline rectangle by each wall's inside half
        assert!(area < 6000.0 * 4000.0 && area > (6000.0 - w) * (4000.0 - w) - 1.0, "{area}");
        // a positive offset moves the lines further into the room, a negative one back into the wall
        let off = d.pick_wall_lines(&ids, p(3000.0, 2000.0), 50.0, false, PickFace::Cursor).unwrap();
        let area_off = crate::geom::signed_area(&loop_polygon(&sketch_loops(&off).unwrap()[0], 1.0)).abs();
        assert!(area_off < area);
        // to the core: the lines sit at the core faces, inside the finish face on the room side
        let core = d.pick_wall_lines(&ids, p(3000.0, 2000.0), 0.0, true, PickFace::Cursor).unwrap();
        let area_core = crate::geom::signed_area(&loop_polygon(&sketch_loops(&core).unwrap()[0], 1.0)).abs();
        assert!(area_core > area, "the floor reaches under the finishes");
        // a single wall gives one line along it
        let one = d.pick_wall_lines(&ids[..1], p(3000.0, 2000.0), 0.0, false, PickFace::Cursor).unwrap();
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn pick_walls_takes_the_exterior_interior_or_centre_whatever_side_the_cursor_is_on() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        let area = |c: &[Curve]| crate::geom::signed_area(&loop_polygon(&sketch_loops(c).unwrap()[0], 1.0)).abs();
        // clockwise: the exterior (left of travel) faces out
        let mut ids = vec![];
        for (a, b) in [((0.0, 0.0), (0.0, 4000.0)), ((0.0, 4000.0), (6000.0, 4000.0)), ((6000.0, 4000.0), (6000.0, 0.0)), ((6000.0, 0.0), (0.0, 0.0))] {
            ids.push(d.add_wall(wt, lvl, p(a.0, a.1), p(b.0, b.1), 2700.0).unwrap());
        }
        let inside = p(3000.0, 2000.0);
        let outside = p(-9000.0, -9000.0);
        let ext = d.pick_wall_lines(&ids, inside, 0.0, false, PickFace::Exterior).unwrap();
        let int = d.pick_wall_lines(&ids, inside, 0.0, false, PickFace::Interior).unwrap();
        let mid = d.pick_wall_lines(&ids, inside, 0.0, false, PickFace::Centre).unwrap();
        assert!(area(&ext) > area(&mid) && area(&mid) > area(&int));
        let w = d.wall_type(wt).unwrap().thickness_mm();
        assert!((area(&mid) - 6000.0 * 4000.0).abs() < 1.0, "the centreline rectangle");
        assert!((area(&ext) - (6000.0 + w) * (4000.0 + w)).abs() < 1.0 || area(&ext) > 6000.0 * 4000.0);
        // the cursor does not change the face
        let ext2 = d.pick_wall_lines(&ids, outside, 0.0, false, PickFace::Exterior).unwrap();
        assert!((area(&ext2) - area(&ext)).abs() < 1.0);
        // a positive offset moves away from the wall: the exterior line goes further out, the interior further in
        let ext_off = d.pick_wall_lines(&ids, inside, 100.0, false, PickFace::Exterior).unwrap();
        let int_off = d.pick_wall_lines(&ids, inside, 100.0, false, PickFace::Interior).unwrap();
        assert!(area(&ext_off) > area(&ext) && area(&int_off) < area(&int));
        // the same run walked the other way round gives the same exterior line
        let rev: Vec<Id> = ids.iter().rev().cloned().collect();
        let ext_rev = d.pick_wall_lines(&rev, inside, 0.0, false, PickFace::Exterior).unwrap();
        assert!((area(&ext_rev) - area(&ext)).abs() < 1.0);
        // to the core, the centre is the middle of the core
        let core_mid = d.pick_wall_lines(&ids, inside, 0.0, true, PickFace::Centre).unwrap();
        assert!(area(&core_mid) > 0.0);
    }

    // ---- floors made from sketches ----

    fn vol(m: &[f32]) -> f64 {
        // signed volume of a closed triangle mesh of (pos, normal) vertices, in cubic metres
        let v: Vec<f64> = m.iter().map(|x| *x as f64).collect();
        let mut vol = 0.0;
        for t in v.chunks(18) {
            let (a, b, c) = ([t[0], t[1], t[2]], [t[6], t[7], t[8]], [t[12], t[13], t[14]]);
            vol += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0;
        }
        vol
    }

    #[test]
    fn a_floor_with_a_round_opening_has_the_net_area_and_a_closed_mesh() {
        let mut d = Document::new_project();
        let (lvl, ft) = (d.levels()[0].0, d.floor_types()[0].0);
        let thick = d.floor_type(ft).unwrap().thickness_mm();
        let mut c = rectangle(p(0.0, 0.0), p(4000.0, 3000.0), 0.0);
        c.push(Curve::Circle { centre: p(2000.0, 1500.0), radius: 500.0 });
        let id = d.add_floor_sketch(ft, lvl, c, None, None).unwrap();
        let Some(Element::Floor(f)) = d.get(id).cloned() else { panic!() };
        assert_eq!(f.loops.len(), 2);
        let area = f.area_mm2();
        let expect = 12_000_000.0 - std::f64::consts::PI * 500.0 * 500.0;
        assert!((area - expect).abs() < 3000.0, "{area} vs {expect}");
        let m = crate::mesh::floor_mesh(&d, &f).unwrap();
        assert!((vol(&m) - area * thick / 1e9).abs() < 0.01, "{} vs {}", vol(&m), area * thick / 1e9);
    }

    #[test]
    fn a_sloped_floor_rises_along_its_arrow() {
        let mut d = Document::new_project();
        let (lvl, ft) = (d.levels()[0].0, d.floor_types()[0].0);
        let slope = SlopeArrow { tail: p(0.0, 0.0), head: p(4000.0, 0.0), spec: SlopeSpec::Slope { tail_offset_mm: 0.0, percent: 5.0 }, tail_level: None, head_level: None };
        let id = d.add_floor_sketch(ft, lvl, rectangle(p(0.0, 0.0), p(4000.0, 3000.0), 0.0), Some(slope), None).unwrap();
        let Some(Element::Floor(f)) = d.get(id).cloned() else { panic!() };
        let s = d.floor_surface(&f).unwrap();
        assert!((s.z(p(4000.0, 1000.0)) - s.z(p(0.0, 1000.0)) - 200.0).abs() < 1e-9, "5% over 4 m is 200 mm");
        assert!((s.percent() - 5.0).abs() < 1e-9);
        let m = crate::mesh::floor_mesh(&d, &f).unwrap();
        let zs: Vec<f32> = m.chunks(6).map(|v| v[2]).collect();
        let top = zs.iter().cloned().fold(f32::MIN, f32::max) as f64;
        assert!((top - 0.2).abs() < 1e-6, "the high edge is 200 mm above the level");
        // by heights at tail and head
        let by_h = SlopeArrow { tail: p(0.0, 0.0), head: p(2000.0, 0.0), spec: SlopeSpec::HeightAtTail { tail_offset_mm: 0.0, head_offset_mm: 100.0 }, tail_level: None, head_level: None };
        d.set_floor_sketch(id, rectangle(p(0.0, 0.0), p(4000.0, 3000.0), 0.0), Some(by_h), None).unwrap();
        let Some(Element::Floor(f)) = d.get(id).cloned() else { panic!() };
        assert!((d.floor_surface(&f).unwrap().percent() - 5.0).abs() < 1e-9);
        assert!(d.set_floor_sketch(id, rectangle(p(0.0, 0.0), p(4000.0, 3000.0), 0.0), Some(SlopeArrow { tail: p(5.0, 5.0), head: p(5.0, 5.0), spec: SlopeSpec::Slope { tail_offset_mm: 0.0, percent: 1.0 }, tail_level: None, head_level: None }), None).is_err());
    }

    #[test]
    fn older_files_with_a_plain_boundary_still_open_and_edit() {
        let mut d = Document::new_project();
        let (lvl, ft) = (d.levels()[0].0, d.floor_types()[0].0);
        // the shape an older project file holds: a polygon and no loops
        let old = serde_json::json!({ "kind": "Floor", "floor_type": ft, "level": lvl, "boundary": [{"x":0.0,"y":0.0},{"x":4000.0,"y":0.0},{"x":4000.0,"y":3000.0},{"x":0.0,"y":3000.0}] });
        let e: Element = serde_json::from_value(old).unwrap();
        let Element::Floor(f) = e.clone() else { panic!() };
        assert!(f.loops.is_empty() && f.loops().len() == 1 && f.loops()[0].len() == 4);
        assert!((f.area_mm2() - 12_000_000.0).abs() < 1e-6);
        let id = d.transact("Open old file", |tx| Ok(tx.add(e))).unwrap();
        // moving it converts it to loops and clears the legacy polygon
        d.transform_elements(&[id], crate::edit::Xform::Move { dx: 100.0, dy: 0.0 }, crate::edit::EditOptions::default()).unwrap();
        let Some(Element::Floor(f)) = d.get(id).cloned() else { panic!() };
        assert!(f.boundary.is_empty() && f.loops.len() == 1);
        assert!(f.outline().iter().any(|q| q.dist(&p(4100.0, 3000.0)) < 1e-6));
        // and it serialises without the old field
        assert!(!serde_json::to_string(&Element::Floor(f)).unwrap().contains("boundary"));
    }

    #[test]
    fn editing_a_floor_boundary_is_one_undo_step_refuses_bad_sketches_and_respects_pins() {
        let mut d = Document::new_project();
        let (lvl, ft) = (d.levels()[0].0, d.floor_types()[0].0);
        let id = d.add_floor_sketch(ft, lvl, rectangle(p(0.0, 0.0), p(4000.0, 3000.0), 0.0), None, None).unwrap();
        let round = rounded_polygon(&[p(0.0, 0.0), p(5000.0, 0.0), p(5000.0, 3000.0), p(0.0, 3000.0)], 400.0);
        d.set_floor_sketch(id, round, None, Some(SpanDirection { a: p(100.0, 100.0), b: p(900.0, 100.0) })).unwrap();
        let Some(Element::Floor(f)) = d.get(id).cloned() else { panic!() };
        assert_eq!(f.loops[0].len(), 8);
        assert!(f.span.is_some());
        let before = f.area_mm2();
        assert!(d.set_floor_sketch(id, vec![Curve::line(p(0.0, 0.0), p(1000.0, 0.0))], None, None).is_err());
        let Some(Element::Floor(g)) = d.get(id).cloned() else { panic!() };
        assert_eq!(g.area_mm2(), before, "a refused sketch changes nothing");
        d.undo();
        let Some(Element::Floor(g)) = d.get(id).cloned() else { panic!() };
        assert!((g.area_mm2() - 12_000_000.0).abs() < 1e-6);
        d.set_pinned(&[id], true).unwrap();
        assert!(d.set_floor_sketch(id, rectangle(p(0.0, 0.0), p(1000.0, 1000.0), 0.0), None, None).unwrap_err().contains("pinned"));
    }

    #[test]
    fn moving_and_mirroring_a_floor_with_arcs_and_a_slope_keeps_them_together() {
        let mut d = Document::new_project();
        let (lvl, ft) = (d.levels()[0].0, d.floor_types()[0].0);
        let slope = SlopeArrow { tail: p(0.0, 0.0), head: p(1000.0, 0.0), spec: SlopeSpec::Slope { tail_offset_mm: 0.0, percent: 2.0 }, tail_level: None, head_level: None };
        let id = d.add_floor_sketch(ft, lvl, rounded_polygon(&[p(0.0, 0.0), p(4000.0, 0.0), p(4000.0, 3000.0), p(0.0, 3000.0)], 300.0), Some(slope), None).unwrap();
        let area = match d.get(id) { Some(Element::Floor(f)) => f.area_mm2(), _ => panic!() };
        let out = d.transform_elements(&[id], crate::edit::Xform::Mirror { ax: 0.0, ay: 0.0, bx: 0.0, by: 1.0 }, crate::edit::EditOptions { copy: true, disjoin: false }).unwrap();
        let Some(Element::Floor(m)) = d.get(out[0]).cloned() else { panic!() };
        assert!((m.area_mm2() - area).abs() < 1.0);
        assert!(m.slope.as_ref().unwrap().head.dist(&p(-1000.0, 0.0)) < 1e-6);
        let poly = m.outline();
        assert!(poly.iter().all(|q| q.x <= 1e-6), "the copy is on the other side of the axis");
        assert!(crate::mesh::floor_mesh(&d, &m).is_some());
    }
}
