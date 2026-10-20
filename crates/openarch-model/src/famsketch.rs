//! Sketches of family forms. A form's profile is drawn with the same sketch tools as a floor
//! (lines, rectangles, polygons, circles, arcs, fillets, splines, ellipses, picked lines), as
//! curves in the view's own axes. Finishing the sketch turns the curves into the form's loops,
//! and Edit (form) turns the loops back into curves.
//!
//! Locks: a straight sketch line lying on a reference plane can be locked to it. A locked line's
//! points are stored on the plane, so the line follows the plane when it moves. Points that did
//! not move keep what they were tied to before the edit (a plane and an offset, say).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::curve::{sketch_loops, Curve, SketchError, JOIN_TOL};
use crate::famgeom::{Coord, Loop, Node, PlaneId, SegShape};
use crate::geom::Point;

/// Which coordinate a lock ties: `u` (a vertical line in the view, on a plane across the view's
/// horizontal axis) or `v` (a horizontal line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Along {
    U,
    V,
}

/// A sketch line on a reference plane, locked (`on`) or deliberately left free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchLock {
    /// Index of the curve in the sketch.
    pub curve: usize,
    pub plane: PlaneId,
    pub coord: Along,
    pub on: bool,
}

/// What a point of the loops was tied to before the edit, by where it stood.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub at: [f64; 2],
    pub u: Coord,
    pub v: Coord,
}

const SAME: f64 = 0.01;

fn round(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn coord_pos(c: &Coord, planes: &BTreeMap<PlaneId, f64>) -> Option<f64> {
    match c.plane {
        Some(p) => planes.get(&p).map(|at| at + c.off),
        None => Some(c.off),
    }
}

fn node_pos(n: &Node, planes: &BTreeMap<PlaneId, f64>) -> Option<Point> {
    Some(Point::new(coord_pos(&n.u, planes)?, coord_pos(&n.v, planes)?))
}

/// The sketch curves of a form's loops (view axes, mm), with the locks its straight edges have and
/// the ties of every point, so finishing the sketch unchanged gives the same loops back.
pub fn curves_from_loops(loops: &[Loop], planes: &BTreeMap<PlaneId, f64>) -> Result<(Vec<Curve>, Vec<SketchLock>, Vec<Kept>), String> {
    let mut curves = vec![];
    let mut locks = vec![];
    let mut kept = vec![];
    let missing = || "a sketch point refers to a missing reference plane".to_string();
    let keep = |n: &Node, p: Point, kept: &mut Vec<Kept>| kept.push(Kept { at: [p.x, p.y], u: n.u, v: n.v });
    for l in loops {
        let pts: Vec<Point> = l.nodes.iter().map(|n| node_pos(n, planes).ok_or_else(missing)).collect::<Result<_, _>>()?;
        for (n, p) in l.nodes.iter().zip(&pts) {
            keep(n, *p, &mut kept);
        }
        let k = pts.len();
        if k == 0 {
            continue;
        }
        let segs = if l.closed { k } else { k - 1 };
        // a closed loop of one or two nodes made only of curved segments is a circle, an ellipse or a closed spline
        if l.closed && k <= 2 {
            match l.shapes.first().cloned().flatten() {
                Some(SegShape::Spline { through }) if k == 1 => {
                    let mut points = vec![pts[0]];
                    for n in &through {
                        let p = node_pos(n, planes).ok_or_else(missing)?;
                        keep(n, p, &mut kept);
                        points.push(p);
                    }
                    curves.push(Curve::Spline { points, closed: true });
                    continue;
                }
                Some(SegShape::Ellipse { centre, rx, ry, rotation_deg, ccw: _ }) if l.shapes.iter().all(|s| matches!(s, Some(SegShape::Ellipse { .. }))) => {
                    let c = node_pos(&centre, planes).ok_or_else(missing)?;
                    keep(&centre, c, &mut kept);
                    curves.push(Curve::Ellipse { centre: c, rx, ry, rotation_deg, t0_deg: None, t1_deg: None, ccw: true });
                    continue;
                }
                None if k == 2 && (l.bulges.first().copied().unwrap_or(0.0) - 1.0).abs() < 1e-9 && (l.bulges.get(1).copied().unwrap_or(0.0) - 1.0).abs() < 1e-9 => {
                    let c = Point::new((pts[0].x + pts[1].x) / 2.0, (pts[0].y + pts[1].y) / 2.0);
                    curves.push(Curve::Circle { centre: c, radius: c.dist(&pts[0]) });
                    continue;
                }
                _ => {}
            }
        }
        for i in 0..segs {
            let (a, b) = (pts[i], pts[(i + 1) % k]);
            let (na, nb) = (&l.nodes[i], &l.nodes[(i + 1) % k]);
            match l.shapes.get(i).cloned().flatten() {
                Some(SegShape::Spline { through }) => {
                    let mut points = vec![a];
                    for n in &through {
                        let p = node_pos(n, planes).ok_or_else(missing)?;
                        keep(n, p, &mut kept);
                        points.push(p);
                    }
                    points.push(b);
                    curves.push(Curve::Spline { points, closed: false });
                }
                Some(SegShape::Ellipse { centre, rx, ry, rotation_deg, ccw }) => {
                    let c = node_pos(&centre, planes).ok_or_else(missing)?;
                    keep(&centre, c, &mut kept);
                    let e = Curve::Ellipse { centre: c, rx, ry, rotation_deg, t0_deg: None, t1_deg: None, ccw };
                    let (t0, t1) = (crate::sketch::ellipse_param(&e, a).unwrap_or(0.0), crate::sketch::ellipse_param(&e, b).unwrap_or(0.0));
                    curves.push(Curve::Ellipse { centre: c, rx, ry, rotation_deg, t0_deg: Some(t0), t1_deg: Some(t1), ccw });
                }
                None => {
                    let bulge = l.bulges.get(i).copied().unwrap_or(0.0);
                    if bulge.abs() > 1e-9 {
                        let theta = 4.0 * bulge.atan();
                        let ch = a.dist(&b);
                        let h = (ch / 2.0) / (theta / 2.0).tan();
                        let (dx, dy) = ((b.x - a.x) / ch, (b.y - a.y) / ch);
                        let centre = Point::new((a.x + b.x) / 2.0 - dy * h, (a.y + b.y) / 2.0 + dx * h);
                        curves.push(Curve::Arc { start: a, end: b, centre, ccw: bulge > 0.0 });
                    } else {
                        let idx = curves.len();
                        curves.push(Curve::Line { a, b });
                        // an edge whose two ends sit on the same plane is locked to it
                        for (along, ca, cb) in [(Along::U, &na.u, &nb.u), (Along::V, &na.v, &nb.v)] {
                            if let (Some(pa), Some(pb)) = (ca.plane, cb.plane) {
                                if pa == pb && ca.off.abs() < 1e-9 && cb.off.abs() < 1e-9 {
                                    locks.push(SketchLock { curve: idx, plane: pa, coord: along, on: true });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok((curves, locks, kept))
}

/// The loops of a finished sketch. Points of locked lines go on their planes; a point standing
/// where one stood before keeps its old ties (unless a line through it was unlocked from that
/// plane); every other point is a fixed offset.
pub fn loops_from_curves(curves: &[Curve], locks: &[SketchLock], kept: &[Kept], planes: &BTreeMap<PlaneId, f64>) -> Result<Vec<Loop>, SketchError> {
    let grouped = sketch_loops(curves)?;
    let ends = |c: &Curve| -> Option<(Point, Point)> { if c.is_closed() { None } else { Some((c.start(), c.end())) } };
    // the curves (by index in `curves`) that have an end at p
    let touching = |p: Point| -> Vec<usize> {
        curves.iter().enumerate().filter(|(_, c)| ends(c).is_some_and(|(a, b)| a.dist(&p) <= JOIN_TOL || b.dist(&p) <= JOIN_TOL)).map(|(i, _)| i).collect()
    };
    let one = |p: Point, along: Along| -> Coord {
        let x = if along == Along::U { p.x } else { p.y };
        let near = touching(p);
        for l in locks.iter().filter(|l| l.on && l.coord == along && near.contains(&l.curve)) {
            if planes.get(&l.plane).is_some_and(|at| (at - x).abs() < SAME) {
                return Coord::on(l.plane);
            }
        }
        if let Some(k) = kept.iter().find(|k| (k.at[0] - p.x).abs() < SAME && (k.at[1] - p.y).abs() < SAME) {
            let c = if along == Along::U { k.u } else { k.v };
            let freed = c.plane.is_some_and(|pl| locks.iter().any(|l| !l.on && l.plane == pl && l.coord == along && near.contains(&l.curve)));
            if !freed && coord_pos(&c, planes).is_some_and(|at| (at - x).abs() < SAME) {
                return c;
            }
        }
        Coord::at(round(x))
    };
    let node = |p: Point| Node::new(one(p, Along::U), one(p, Along::V));
    let mut out = vec![];
    for chain in grouped {
        if chain.len() == 1 && chain[0].is_closed() {
            out.push(match &chain[0] {
                Curve::Circle { centre, radius } => {
                    let (a, b) = (Point::new(centre.x + radius, centre.y), Point::new(centre.x - radius, centre.y));
                    Loop { nodes: vec![node(a), node(b)], bulges: vec![1.0, 1.0], closed: true, shapes: vec![] }
                }
                Curve::Ellipse { centre, rx, ry, rotation_deg, .. } => {
                    let rot = rotation_deg.to_radians();
                    let a = Point::new(centre.x + rx * rot.cos(), centre.y + rx * rot.sin());
                    let b = Point::new(centre.x - rx * rot.cos(), centre.y - rx * rot.sin());
                    let sh = SegShape::Ellipse { centre: node(*centre), rx: *rx, ry: *ry, rotation_deg: *rotation_deg, ccw: true };
                    Loop { nodes: vec![node(a), node(b)], bulges: vec![0.0, 0.0], closed: true, shapes: vec![Some(sh.clone()), Some(sh)] }
                }
                Curve::Spline { points, .. } => {
                    let sh = SegShape::Spline { through: points[1..].iter().map(|p| node(*p)).collect() };
                    Loop { nodes: vec![node(points[0])], bulges: vec![0.0], closed: true, shapes: vec![Some(sh)] }
                }
                _ => continue,
            });
            continue;
        }
        let mut l = Loop { nodes: vec![], bulges: vec![], closed: true, shapes: vec![] };
        for c in &chain {
            l.nodes.push(node(c.start()));
            match c {
                Curve::Line { .. } => {
                    l.bulges.push(0.0);
                    l.shapes.push(None);
                }
                Curve::Arc { start, end, centre, ccw } => {
                    let sweep = crate::curve::arc_sweep(*start, *end, *centre, *ccw);
                    l.bulges.push((sweep / 4.0).tan());
                    l.shapes.push(None);
                }
                Curve::Ellipse { centre, rx, ry, rotation_deg, ccw, .. } => {
                    l.bulges.push(0.0);
                    l.shapes.push(Some(SegShape::Ellipse { centre: node(*centre), rx: *rx, ry: *ry, rotation_deg: *rotation_deg, ccw: *ccw }));
                }
                Curve::Spline { points, .. } => {
                    l.bulges.push(0.0);
                    l.shapes.push(Some(SegShape::Spline { through: points[1..points.len().saturating_sub(1)].iter().map(|p| node(*p)).collect() }));
                }
                Curve::Circle { .. } => {}
            }
        }
        if l.shapes.iter().all(|s| s.is_none()) {
            l.shapes.clear();
        }
        out.push(l);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::famgeom::{tessellate_loop, PLANE_FB, PLANE_LR};

    fn planes() -> BTreeMap<PlaneId, f64> {
        // the two origin planes at 0 and a side plane at 500
        [(PLANE_LR, 0.0), (PLANE_FB, 0.0), (50, 500.0)].into_iter().collect()
    }

    #[test]
    fn a_locked_rectangle_round_trips_and_its_locks_come_back() {
        let pl = planes();
        let p = |x: f64, y: f64| Point::new(x, y);
        let line = |a: Point, b: Point| Curve::Line { a, b };
        let curves = vec![line(p(0.0, 0.0), p(500.0, 0.0)), line(p(500.0, 0.0), p(500.0, 300.0)), line(p(500.0, 300.0), p(0.0, 300.0)), line(p(0.0, 300.0), p(0.0, 0.0))];
        // the right edge locks to plane 50, the left to the centre plane; the bottom is left free
        let locks = vec![SketchLock { curve: 1, plane: 50, coord: Along::U, on: true }, SketchLock { curve: 3, plane: PLANE_LR, coord: Along::U, on: true }, SketchLock { curve: 0, plane: PLANE_FB, coord: Along::V, on: false }];
        let loops = loops_from_curves(&curves, &locks, &[], &pl).unwrap();
        assert_eq!(loops.len(), 1);
        let l = &loops[0];
        assert!(l.nodes.iter().filter(|n| n.u.plane == Some(50)).count() == 2);
        assert!(l.nodes.iter().filter(|n| n.u.plane == Some(PLANE_LR)).count() == 2);
        assert!(l.nodes.iter().all(|n| n.v.plane.is_none()));
        let (back, locks2, kept) = curves_from_loops(&loops, &pl).unwrap();
        assert_eq!(back.len(), 4);
        assert_eq!(locks2.iter().filter(|l| l.on).count(), 2);
        // unchanged, the sketch gives the same loop again
        assert_eq!(loops_from_curves(&back, &locks2, &kept, &pl).unwrap(), loops);
    }

    #[test]
    fn untouched_points_keep_their_offsets_and_moved_ones_are_fixed() {
        let pl = planes();
        // a node 100 to the right of plane 50
        let n = |u: Coord, v: Coord| Node::new(u, v);
        let lp = Loop { nodes: vec![n(Coord::at(0.0), Coord::at(0.0)), n(Coord::from(50, 100.0), Coord::at(0.0)), n(Coord::from(50, 100.0), Coord::at(200.0))], bulges: vec![], closed: true, shapes: vec![] };
        let (curves, locks, kept) = curves_from_loops(std::slice::from_ref(&lp), &pl).unwrap();
        assert!(locks.is_empty());
        let same = loops_from_curves(&curves, &locks, &kept, &pl).unwrap();
        assert_eq!(same[0].nodes.iter().filter(|n| n.u == Coord::from(50, 100.0)).count(), 2);
        // move the top point: it becomes a plain offset, the others stay as they were
        let mut moved = curves.clone();
        moved[1] = Curve::Line { a: Point::new(600.0, 0.0), b: Point::new(650.0, 200.0) };
        moved[2] = Curve::Line { a: Point::new(650.0, 200.0), b: Point::new(0.0, 0.0) };
        let l2 = loops_from_curves(&moved, &locks, &kept, &pl).unwrap();
        assert!(l2[0].nodes.iter().any(|n| n.u == Coord::at(650.0)));
        assert!(l2[0].nodes.iter().any(|n| n.u == Coord::from(50, 100.0)));
    }

    #[test]
    fn circles_ellipses_splines_and_arcs_become_loops_and_come_back() {
        let pl = planes();
        let circle = Curve::Circle { centre: Point::new(0.0, 0.0), radius: 100.0 };
        let ellipse = Curve::Ellipse { centre: Point::new(1000.0, 0.0), rx: 200.0, ry: 100.0, rotation_deg: 0.0, t0_deg: None, t1_deg: None, ccw: true };
        let spline = Curve::Spline { points: vec![Point::new(2000.0, 0.0), Point::new(2200.0, 100.0), Point::new(2100.0, 300.0), Point::new(1900.0, 200.0)], closed: true };
        let loops = loops_from_curves(&[circle, ellipse, spline], &[], &[], &pl).unwrap();
        assert_eq!(loops.len(), 3);
        assert_eq!(loops[0].bulges, vec![1.0, 1.0]);
        assert!(matches!(loops[1].shapes[0], Some(SegShape::Ellipse { .. })));
        assert!(matches!(loops[2].shapes[0], Some(SegShape::Spline { .. })));
        let (back, _, _) = curves_from_loops(&loops, &pl).unwrap();
        assert!(matches!(back[0], Curve::Circle { .. }));
        assert!(matches!(back[1], Curve::Ellipse { t0_deg: None, .. }));
        assert!(matches!(back[2], Curve::Spline { closed: true, .. }));
        // a D shape: a line and a half circle arc
        let d = vec![Curve::Line { a: Point::new(0.0, -100.0), b: Point::new(0.0, 100.0) }, Curve::Arc { start: Point::new(0.0, 100.0), end: Point::new(0.0, -100.0), centre: Point::new(0.0, 0.0), ccw: true }];
        let dl = loops_from_curves(&d, &[], &[], &pl).unwrap();
        assert!((dl[0].bulges[1] - 1.0).abs() < 1e-9);
        let pts: Vec<Point> = dl[0].nodes.iter().map(|n| Point::new(n.u.off, n.v.off)).collect();
        let ring = tessellate_loop(&pts, &dl[0], 0.5);
        assert!(ring.iter().any(|p| p.x < -99.0), "the arc bulges to the left");
    }
}
