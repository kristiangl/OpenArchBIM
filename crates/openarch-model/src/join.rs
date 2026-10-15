//! Wall joins. Which walls join is found from the geometry every time, so
//! moving a wall re-joins its neighbours. How they join is stored on each wall
//! end (`EndJoin`): allowed or not, butt / miter / square off, which wall runs
//! through, and cleanup display. That is what the Wall Joins tool edits.
//!
//! Rules (all on one base level):
//! - Two walls meeting end to end join by the stored kind (default butt: one
//!   wall runs through, the other stops at its face). Collinear walls and very
//!   sharp angles keep square ends.
//! - A wall end with joins disallowed stays square and independent.
//! - Three or more walls at a node: if two of them run straight through, those
//!   stay square and the others are butted into their face (T and cross
//!   junctions). Otherwise neighbours are mitred around the node.
//! - A wall end that lands on the middle of another wall is butted into that
//!   wall's face.

use std::collections::{BTreeMap, HashMap};

use crate::element::{EndJoin, Id, JoinKind};
use crate::geom::{dist_to_segment, Point};

/// Ends closer than this (mm) count as the same node.
pub(crate) const TOL: f64 = 1.0;
/// A mitre reaching further than this many half-thicknesses becomes a square end.
pub(crate) const MITRE_LIMIT: f64 = 4.0;

#[derive(Clone, Debug)]
pub struct JoinWall {
    pub id: Id,
    pub level: Id,
    pub start: Point,
    pub end: Point,
    /// How far the wall body reaches left and right of its location line,
    /// looking from start to end.
    pub left: f64,
    pub right: f64,
    pub start_join: EndJoin,
    pub end_join: EndJoin,
}

#[derive(Clone, Copy)]
pub(crate) struct End {
    pub wall: usize,
    pub which: u8, // 0 = start, 1 = end
    pub node: Point,
    pub u: Point, // unit direction pointing away from the node, along the wall
    pub hl: f64,  // reach to the left of `u` (looking away from the node)
    pub hr: f64,  // reach to the right of `u`
    pub id: Id,
    pub settings: EndJoin,
}

impl End {
    pub fn left(&self) -> Point {
        Point::new(-self.u.y, self.u.x)
    }
    pub fn right(&self) -> Point {
        Point::new(self.u.y, -self.u.x)
    }
}

pub(crate) fn add(a: Point, b: Point, k: f64) -> Point {
    Point::new(a.x + b.x * k, a.y + b.y * k)
}

pub(crate) fn cross2(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

pub(crate) fn dot2(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

/// Intersection of `p1 + t*d1` with `p2 + s*d2`.
pub(crate) fn intersect(p1: Point, d1: Point, p2: Point, d2: Point) -> Option<Point> {
    let den = cross2(d1, d2);
    if den.abs() < 1e-6 {
        return None;
    }
    let w = Point::new(p2.x - p1.x, p2.y - p1.y);
    let t = cross2(w, d2) / den;
    Some(add(p1, d1, t))
}

/// Outline index (see `wall_outline`: start-right, end-right, end-left, start-left).
fn slot(e: &End, left: bool) -> usize {
    match (e.which, left) {
        (0, true) => 3,
        (0, false) => 0,
        (_, true) => 1,
        (_, false) => 2,
    }
}

fn set(out: &mut [[Point; 4]], e: &End, left: bool, p: Point) {
    out[e.wall][slot(e, left)] = p;
}

/// Square the end `e` against a wall passing the node along `d` (unit). The
/// other wall reaches `hl` to the left of `d` and `hr` to the right. `far` =
/// false stops the end at the near face (butting in), true runs it out to the
/// far face (running through).
fn square_to(out: &mut [[Point; 4]], e: &End, d: Point, hl: f64, hr: f64, far: bool) {
    let n = Point::new(-d.y, d.x);
    if cross2(e.u, d).abs() < 1e-3 {
        return; // runs along the other wall, nothing to meet
    }
    let toward_left = dot2(e.u, n) >= 0.0;
    let on_left = toward_left != far;
    let (side, reach) = if on_left { (1.0, hl) } else { (-1.0, hr) };
    let face = add(e.node, n, side * reach);
    let limit = MITRE_LIMIT * 2.0 * hl.max(hr).max(e.hl).max(e.hr);
    let mut pts = [(true, e.node), (false, e.node)];
    for (left, p) in pts.iter_mut() {
        let line_pt = if *left { add(e.node, e.left(), e.hl) } else { add(e.node, e.right(), e.hr) };
        match intersect(line_pt, e.u, face, d) {
            Some(q) if q.dist(&e.node) <= limit => *p = q,
            _ => return, // too sharp, keep the square end
        }
    }
    for (left, p) in pts {
        set(out, e, left, p);
    }
}

fn butt(out: &mut [[Point; 4]], e: &End, d: Point, hl: f64, hr: f64) {
    square_to(out, e, d, hl, hr, false);
}

fn mitre(out: &mut [[Point; 4]], x: &End, y: &End) {
    // corner between x's left face and y's right face
    let px = add(x.node, x.left(), x.hl);
    let py = add(y.node, y.right(), y.hr);
    let Some(p) = intersect(px, x.u, py, y.u) else { return };
    if p.dist(&x.node) > MITRE_LIMIT * x.hl.max(x.hr).max(y.hl).max(y.hr) {
        return;
    }
    set(out, x, true, p);
    set(out, y, false, p);
}

/// Which of two ends runs through a butt or square-off join. Explicit choices
/// win; otherwise the older wall (lower id) runs through.
pub(crate) fn through_index(a: &End, b: &End) -> usize {
    match (a.settings.through, b.settings.through) {
        (Some(true), Some(true)) | (None, None) => (a.id > b.id) as usize,
        (Some(true), _) | (_, Some(false)) => 0,
        (_, Some(true)) | (Some(false), _) => 1,
    }
}

/// Outline of a wall body: start-right, end-right, end-left, start-left.
pub(crate) fn plain_outline(w: &JoinWall) -> Option<[Point; 4]> {
    let len = w.start.dist(&w.end);
    if len < 1e-6 {
        return None;
    }
    let n = Point::new(-(w.end.y - w.start.y) / len, (w.end.x - w.start.x) / len); // left normal
    Some([
        add(w.start, n, -w.right),
        add(w.end, n, -w.right),
        add(w.end, n, w.left),
        add(w.start, n, w.left),
    ])
}

/// A group of wall ends found at one place, classified by how they meet. The
/// outline join (`join_walls`) and the layer join (`layers::join_layers`) both
/// start from this, so they always agree about which walls join and how.
#[derive(Clone)]
pub(crate) enum Node {
    /// One end landing on the body of another wall (`other` indexes the walls).
    Mid { e: End, other: usize },
    /// Two walls meeting end to end. `a` is the older wall.
    Pair { a: End, b: End },
    /// Two walls running straight through (`i` and `j` face each other) with
    /// the rest butted into their face.
    Through { i: End, j: End, others: Vec<End> },
    /// Three or more walls meeting with no through pair, mitred around the node.
    Ring { ends: Vec<End> },
}

/// An end that lands inside the body of wall `o` (but not on its location line)
/// and is not nearly parallel to it. Returns how far the end is from the
/// location line and the point where its own line meets that location line,
/// which must lie within the wall and clear of its ends.
fn lands_in_body(e: &End, o: &JoinWall) -> Option<(f64, Point)> {
    let len = o.start.dist(&o.end);
    let d = Point::new((o.end.x - o.start.x) / len, (o.end.y - o.start.y) / len);
    let s = cross2(d, Point::new(e.node.x - o.start.x, e.node.y - o.start.y)); // + is left of o
    if s > o.left + TOL || s < -(o.right + TOL) {
        return None;
    }
    let sin = cross2(e.u, d);
    if sin.abs() < 0.2 {
        return None;
    }
    let q = intersect(e.node, e.u, o.start, d)?;
    let t = dot2(Point::new(q.x - o.start.x, q.y - o.start.y), d);
    if t <= TOL || t >= len - TOL {
        return None;
    }
    Some((s.abs(), q))
}

/// Find and classify every join. Invalid (zero length) walls are skipped.
pub(crate) fn classify(walls: &[JoinWall]) -> Vec<Node> {
    let valid: Vec<bool> = walls.iter().map(|w| w.start.dist(&w.end) >= 1e-6).collect();

    // ends grouped into nodes
    let mut nodes: HashMap<(Id, i64, i64), Vec<End>> = HashMap::new();
    for (i, w) in walls.iter().enumerate() {
        if !valid[i] {
            continue;
        }
        let len = w.start.dist(&w.end);
        let u = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
        // at the end of the wall we look back along it, so left and right swap
        let ends = [
            End { wall: i, which: 0, node: w.start, u, hl: w.left, hr: w.right, id: w.id, settings: w.start_join },
            End { wall: i, which: 1, node: w.end, u: Point::new(-u.x, -u.y), hl: w.right, hr: w.left, id: w.id, settings: w.end_join },
        ];
        for e in ends {
            if !e.settings.allowed {
                continue; // Disallow Join: independent square end
            }
            let key = (w.level, (e.node.x / TOL).round() as i64, (e.node.y / TOL).round() as i64);
            nodes.entry(key).or_default().push(e);
        }
    }

    // coarse grid over wall bounding boxes, for ends that land mid-wall
    const CELL: f64 = 5000.0;
    let mut grid: HashMap<(Id, i64, i64), Vec<usize>> = HashMap::new();
    for (i, w) in walls.iter().enumerate() {
        if !valid[i] {
            continue;
        }
        let (x0, x1) = (w.start.x.min(w.end.x) - TOL, w.start.x.max(w.end.x) + TOL);
        let (y0, y1) = (w.start.y.min(w.end.y) - TOL, w.start.y.max(w.end.y) + TOL);
        for cx in (x0 / CELL).floor() as i64..=(x1 / CELL).floor() as i64 {
            for cy in (y0 / CELL).floor() as i64..=(y1 / CELL).floor() as i64 {
                grid.entry((w.level, cx, cy)).or_default().push(i);
            }
        }
    }

    let mut out = vec![];
    for ((level, _, _), mut ends) in nodes {
        if ends.len() == 1 {
            let e = ends[0];
            let cell = (level, (e.node.x / CELL).floor() as i64, (e.node.y / CELL).floor() as i64);
            let Some(cands) = grid.get(&cell) else { continue };
            // the host is the wall whose location line the end lands on, or failing
            // that the nearest wall whose body it lands in
            let mut best: Option<(f64, usize, Point)> = None;
            for &j in cands {
                if j == e.wall {
                    continue;
                }
                let o = &walls[j];
                if o.start.dist(&e.node) <= TOL || o.end.dist(&e.node) <= TOL {
                    continue;
                }
                if dist_to_segment(e.node, o.start, o.end) <= TOL {
                    best = Some((0.0, j, e.node));
                    break;
                }
                if let Some((off, q)) = lands_in_body(&e, o) {
                    if best.map_or(true, |b| off < b.0) {
                        best = Some((off, j, q));
                    }
                }
            }
            if let Some((_, j, q)) = best {
                // a wall ending in the body of another runs on to its centreline, so
                // the join is the same as for an end placed on the centreline
                let mut e = e;
                e.node = q;
                out.push(Node::Mid { e, other: j });
            }
            continue;
        }

        // average position keeps the corner stable when ends differ by under TOL
        let n = ends.len() as f64;
        let c = Point::new(ends.iter().map(|e| e.node.x).sum::<f64>() / n, ends.iter().map(|e| e.node.y).sum::<f64>() / n);
        for e in ends.iter_mut() {
            e.node = c;
        }
        ends.sort_by(|a, b| a.u.y.atan2(a.u.x).partial_cmp(&b.u.y.atan2(b.u.x)).unwrap());

        // two walls running straight through the node?
        let mut through: Option<(usize, usize)> = None;
        if ends.len() >= 3 {
            'find: for i in 0..ends.len() {
                for j in i + 1..ends.len() {
                    if cross2(ends[i].u, ends[j].u).abs() < 0.035 && dot2(ends[i].u, ends[j].u) < 0.0 {
                        through = Some((i, j));
                        break 'find;
                    }
                }
            }
        }

        if let Some((i, j)) = through {
            let others = ends.iter().enumerate().filter(|(k, _)| *k != i && *k != j).map(|(_, e)| *e).collect();
            out.push(Node::Through { i: ends[i], j: ends[j], others });
        } else if ends.len() == 2 {
            // two collinear walls meeting on the face of a third wall both butt into it
            let collinear = cross2(ends[0].u, ends[1].u).abs() < 0.035 && dot2(ends[0].u, ends[1].u) < 0.0;
            if collinear {
                let cell = (level, (c.x / CELL).floor() as i64, (c.y / CELL).floor() as i64);
                let host = grid.get(&cell).and_then(|cands| {
                    cands.iter().copied().find(|&j| {
                        let o = &walls[j];
                        j != ends[0].wall && j != ends[1].wall && o.start.dist(&c) > TOL && o.end.dist(&c) > TOL && dist_to_segment(c, o.start, o.end) <= TOL
                    })
                });
                if let Some(j) = host {
                    out.push(Node::Mid { e: ends[0], other: j });
                    out.push(Node::Mid { e: ends[1], other: j });
                    continue;
                }
            }
            // the older wall's end decides the kind, as the tool sets both together
            let (a, b) = if ends[0].id <= ends[1].id { (ends[0], ends[1]) } else { (ends[1], ends[0]) };
            out.push(Node::Pair { a, b });
        } else {
            out.push(Node::Ring { ends });
        }
    }
    out
}

/// Joined outline of every wall, keyed by id. Walls with no length are left out.
pub fn join_walls(walls: &[JoinWall]) -> BTreeMap<Id, [Point; 4]> {
    let mut out: Vec<[Point; 4]> = Vec::with_capacity(walls.len());
    let mut valid: Vec<bool> = Vec::with_capacity(walls.len());
    for w in walls {
        match plain_outline(w) {
            Some(o) => {
                out.push(o);
                valid.push(true);
            }
            None => {
                out.push([w.start; 4]);
                valid.push(false);
            }
        }
    }
    let plain = out.clone();

    for node in classify(walls) {
        match node {
            Node::Mid { e, other } => {
                let o = &walls[other];
                let len = o.start.dist(&o.end);
                let d = Point::new((o.end.x - o.start.x) / len, (o.end.y - o.start.y) / len);
                butt(&mut out, &e, d, o.left, o.right);
            }
            Node::Through { i, j, others } => {
                let d = i.u;
                // the through walls face each other, so one's left is the other's right
                let hl = i.hl.max(j.hr);
                let hr = i.hr.max(j.hl);
                for e in &others {
                    butt(&mut out, e, d, hl, hr);
                }
            }
            Node::Pair { a, b } => match a.settings.kind {
                JoinKind::Miter => {
                    mitre(&mut out, &a, &b);
                    mitre(&mut out, &b, &a);
                }
                JoinKind::Butt | JoinKind::SquareOff => {
                    let (t, s) = if through_index(&a, &b) == 0 { (a, b) } else { (b, a) };
                    // t runs out to the far face of s, s stops at the near face of t
                    square_to(&mut out, &t, s.u, s.hl, s.hr, true);
                    square_to(&mut out, &s, t.u, t.hl, t.hr, false);
                }
            },
            Node::Ring { ends } => {
                let m = ends.len();
                for i in 0..m {
                    mitre(&mut out, &ends[i], &ends[(i + 1) % m]);
                }
            }
        }
    }

    // a join that folds a short wall over itself is dropped
    let mut result = BTreeMap::new();
    for (i, w) in walls.iter().enumerate() {
        if !valid[i] {
            continue;
        }
        let o = out[i];
        let axis = Point::new(w.end.x - w.start.x, w.end.y - w.start.y);
        let proj = |p: Point| dot2(Point::new(p.x - w.start.x, p.y - w.start.y), axis);
        let starts = proj(o[0]).max(proj(o[3]));
        let ends = proj(o[1]).min(proj(o[2]));
        result.insert(w.id, if starts < ends { o } else { plain[i] });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::signed_area;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn jw(id: Id, a: Point, b: Point) -> JoinWall {
        JoinWall { id, level: 1, start: a, end: b, left: 100.0, right: 100.0, start_join: EndJoin::default(), end_join: EndJoin::default() }
    }

    fn jwm(id: Id, a: Point, b: Point) -> JoinWall {
        let m = EndJoin { kind: JoinKind::Miter, ..EndJoin::default() };
        JoinWall { start_join: m, end_join: m, ..jw(id, a, b) }
    }

    fn close(a: Point, x: f64, y: f64) -> bool {
        (a.x - x).abs() < 1e-6 && (a.y - y).abs() < 1e-6
    }

    #[test]
    fn l_corner_is_mitred_without_overlap() {
        let r = join_walls(&[jwm(1, p(0., 0.), p(4000., 0.)), jwm(2, p(4000., 0.), p(4000., 3000.))]);
        let (a, b) = (r[&1], r[&2]);
        assert!(close(a[1], 4100., -100.) && close(a[2], 3900., 100.));
        assert!(close(b[0], 4100., -100.) && close(b[3], 3900., 100.));
        // together they cover the L exactly once
        let total = signed_area(&a) + signed_area(&b);
        assert!((total - 1_400_000.0).abs() < 1e-6, "{total}");
    }

    #[test]
    fn corner_works_whichever_way_the_walls_are_drawn() {
        // second wall drawn towards the corner, first away from it
        let r = join_walls(&[jwm(1, p(4000., 0.), p(0., 0.)), jwm(2, p(4000., 3000.), p(4000., 0.))]);
        let total = signed_area(&r[&1]) + signed_area(&r[&2]);
        assert!((total - 1_400_000.0).abs() < 1e-6, "{total}");
    }

    #[test]
    fn rectangle_of_four_walls_closes_cleanly_for_every_kind() {
        let c = [p(0., 0.), p(6000., 0.), p(6000., 4000.), p(0., 4000.)];
        for make in [jw as fn(Id, Point, Point) -> JoinWall, jwm] {
            let walls: Vec<JoinWall> = (0..4).map(|i| make(i as Id + 1, c[i], c[(i + 1) % 4])).collect();
            let r = join_walls(&walls);
            let total: f64 = r.values().map(|o| signed_area(o)).sum();
            // outer 6200 x 4200 less inner 5800 x 3800
            assert!((total - (6200.0 * 4200.0 - 5800.0 * 3800.0)).abs() < 1e-6, "{total}");
        }
    }

    #[test]
    fn butt_is_the_default_and_the_older_wall_runs_through() {
        let r = join_walls(&[jw(1, p(0., 0.), p(4000., 0.)), jw(2, p(4000., 0.), p(4000., 3000.))]);
        let (a, b) = (r[&1], r[&2]);
        // wall 1 runs out to the far face of wall 2, wall 2 starts at wall 1's face
        assert!(close(a[1], 4100., -100.) && close(a[2], 4100., 100.), "{a:?}");
        assert!(close(b[0], 4100., 100.) && close(b[3], 3900., 100.), "{b:?}");
        assert!((signed_area(&a) + signed_area(&b) - 1_400_000.0).abs() < 1e-6);
    }

    #[test]
    fn join_order_can_be_switched() {
        let mut second = jw(2, p(4000., 0.), p(4000., 3000.));
        second.start_join.through = Some(true);
        let r = join_walls(&[jw(1, p(0., 0.), p(4000., 0.)), second]);
        let (a, b) = (r[&1], r[&2]);
        // now wall 2 runs through to the far face of wall 1 and wall 1 stops at wall 2's face
        assert!(close(b[0], 4100., -100.) && close(b[3], 3900., -100.), "{b:?}");
        assert!(close(a[1], 3900., -100.) && close(a[2], 3900., 100.), "{a:?}");
        assert!((signed_area(&a) + signed_area(&b) - 1_400_000.0).abs() < 1e-6);
    }

    #[test]
    fn square_off_draws_like_butt_for_now() {
        let sq = EndJoin { kind: JoinKind::SquareOff, ..EndJoin::default() };
        let mut x = jw(1, p(0., 0.), p(4000., 0.));
        x.end_join = sq;
        let r1 = join_walls(&[x, jw(2, p(4000., 0.), p(4000., 3000.))]);
        let r2 = join_walls(&[jw(1, p(0., 0.), p(4000., 0.)), jw(2, p(4000., 0.), p(4000., 3000.))]);
        assert_eq!(r1, r2);
    }

    #[test]
    fn disallowed_end_stays_square_and_independent() {
        let mut a = jw(1, p(0., 0.), p(4000., 0.));
        a.end_join.allowed = false;
        let r = join_walls(&[a, jw(2, p(4000., 0.), p(4000., 3000.))]);
        // wall 1 keeps its square end, wall 2 has nothing to join with
        assert!(close(r[&1][1], 4000., -100.) && close(r[&1][2], 4000., 100.));
        assert!(close(r[&2][0], 4100., 0.) && close(r[&2][3], 3900., 0.));
    }

    #[test]
    fn disallowed_end_does_not_butt_into_a_wall_it_touches() {
        let mut stem = jw(2, p(3000., 0.), p(3000., 2000.));
        stem.start_join.allowed = false;
        let r = join_walls(&[jw(1, p(0., 0.), p(6000., 0.)), stem]);
        assert!(close(r[&2][0], 3100., 0.));
    }

    #[test]
    fn collinear_walls_stay_square() {
        let r = join_walls(&[jw(1, p(0., 0.), p(3000., 0.)), jw(2, p(3000., 0.), p(6000., 0.))]);
        assert!(close(r[&1][1], 3000., -100.) && close(r[&1][2], 3000., 100.));
        assert!(close(r[&2][0], 3000., -100.));
    }

    #[test]
    fn stem_butts_into_a_wall_it_lands_on() {
        let r = join_walls(&[jw(1, p(0., 0.), p(6000., 0.)), jw(2, p(3000., 0.), p(3000., 2000.))]);
        let s = r[&2];
        assert!(close(s[0], 3100., 100.) && close(s[3], 2900., 100.));
        assert!((signed_area(&s) - 200.0 * 1900.0).abs() < 1e-6);
        // the through wall is untouched
        assert!(close(r[&1][1], 6000., -100.));
    }

    #[test]
    fn a_stem_ending_at_the_face_of_a_wall_joins_as_if_it_ended_on_the_centreline() {
        // wall 1 is 200 thick (y -100..100); the stem is placed up to its face at y=100
        let on_face = join_walls(&[jw(1, p(0., 0.), p(6000., 0.)), jw(2, p(3000., 2000.), p(3000., 100.))]);
        let on_centre = join_walls(&[jw(1, p(0., 0.), p(6000., 0.)), jw(2, p(3000., 2000.), p(3000., 0.))]);
        assert_eq!(on_face[&2], on_centre[&2]);
        assert!(close(on_face[&2][1], 2900., 100.) && close(on_face[&2][2], 3100., 100.), "{:?}", on_face[&2]);
        // an end that has gone into the wall past its centreline does the same
        let past = join_walls(&[jw(1, p(0., 0.), p(6000., 0.)), jw(2, p(3000., 2000.), p(3000., -60.))]);
        assert_eq!(past[&2], on_centre[&2]);
        // the through wall is untouched
        assert!(close(on_face[&1][1], 6000., -100.));
    }

    #[test]
    fn an_end_inside_the_body_of_a_nearly_parallel_wall_does_not_join() {
        let r = join_walls(&[jw(1, p(0., 0.), p(6000., 0.)), jw(2, p(3000., 300.), p(1000., 50.))]);
        assert_eq!(r[&2], plain_outline(&jw(2, p(3000., 300.), p(1000., 50.))).unwrap());
    }

    #[test]
    fn t_node_with_through_walls_split_at_the_node() {
        let r = join_walls(&[
            jw(1, p(0., 0.), p(3000., 0.)),
            jw(2, p(3000., 0.), p(6000., 0.)),
            jw(3, p(3000., 0.), p(3000., 2000.)),
        ]);
        assert!(close(r[&1][1], 3000., -100.) && close(r[&2][0], 3000., -100.));
        assert!(close(r[&3][0], 3100., 100.) && close(r[&3][3], 2900., 100.));
    }

    #[test]
    fn cross_junction_butts_both_stems() {
        let r = join_walls(&[
            jw(1, p(0., 0.), p(3000., 0.)),
            jw(2, p(3000., 0.), p(6000., 0.)),
            jw(3, p(3000., 0.), p(3000., 2000.)),
            jw(4, p(3000., 0.), p(3000., -2000.)),
        ]);
        assert!(close(r[&3][0], 3100., 100.));
        assert!(close(r[&4][3], 3100., -100.) || close(r[&4][0], 2900., -100.));
    }

    #[test]
    fn sharp_angle_keeps_square_ends() {
        let end = p(4000. * 10f64.to_radians().cos(), 4000. * 10f64.to_radians().sin());
        let r = join_walls(&[jwm(1, p(0., 0.), p(4000., 0.)), jwm(2, p(0., 0.), end)]);
        let plain = plain_outline(&jw(1, p(0., 0.), p(4000., 0.))).unwrap();
        assert_eq!(r[&1], plain);
    }

    #[test]
    fn different_levels_do_not_join() {
        let mut b = jw(2, p(4000., 0.), p(4000., 3000.));
        b.level = 2;
        let r = join_walls(&[jw(1, p(0., 0.), p(4000., 0.)), b]);
        assert!(close(r[&1][1], 4000., -100.) && close(r[&1][2], 4000., 100.));
    }

    #[test]
    fn short_wall_between_mitres_falls_back() {
        // 90 mm wall between two 90 degree corners would fold over itself
        let r = join_walls(&[
            jw(1, p(0., 0.), p(0., 3000.)),
            jw(2, p(0., 3000.), p(90., 3000.)),
            jw(3, p(90., 3000.), p(90., 0.)),
        ]);
        for o in r.values() {
            assert!(signed_area(o) > 0.0);
        }
    }

    #[test]
    fn many_walls_are_fast() {
        // a grid of 40 x 40 rooms, ~3200 walls
        let mut walls = vec![];
        let mut id = 1;
        for i in 0..=40 {
            for j in 0..40 {
                let (a, b) = (i as f64 * 4000., j as f64 * 4000.);
                walls.push(jw(id, p(a, b), p(a, b + 4000.)));
                walls.push(jw(id + 1, p(b, a), p(b + 4000., a)));
                id += 2;
            }
        }
        let t = std::time::Instant::now();
        let r = join_walls(&walls);
        assert_eq!(r.len(), walls.len());
        assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
    }

    #[test]
    fn walls_on_their_exterior_face_keep_the_outside_on_the_drawn_line() {
        // drawn clockwise so the exterior (left) side faces out; location line is
        // the exterior face, so the body lies entirely inside the drawn rectangle
        let c = [p(0., 0.), p(0., 4000.), p(6000., 4000.), p(6000., 0.)];
        for kind in [JoinKind::Butt, JoinKind::Miter] {
            let j = EndJoin { kind, ..EndJoin::default() };
            let walls: Vec<JoinWall> = (0..4)
                .map(|i| JoinWall { id: i as Id + 1, level: 1, start: c[i], end: c[(i + 1) % 4], left: 0.0, right: 200.0, start_join: j, end_join: j })
                .collect();
            let r = join_walls(&walls);
            let total: f64 = r.values().map(|o| signed_area(o)).sum();
            assert!((total - (6000.0 * 4000.0 - 5600.0 * 3600.0)).abs() < 1e-6, "{kind:?} {total}");
            for o in r.values() {
                for q in o {
                    assert!(q.x >= -1e-6 && q.x <= 6000.0 + 1e-6 && q.y >= -1e-6 && q.y <= 4000.0 + 1e-6, "{kind:?} {q:?}");
                }
            }
        }
    }

    #[test]
    fn stem_butts_to_the_near_face_of_an_off_centre_wall() {
        // through wall reaches 50 left and 150 right of its location line
        let mut through = jw(1, p(0., 0.), p(6000., 0.));
        through.left = 50.0;
        through.right = 150.0;
        // stem comes from below (right side of the through wall)
        let stem = jw(2, p(3000., 0.), p(3000., -2000.));
        let r = join_walls(&[through, stem]);
        let top = r[&2].iter().map(|q| q.y).fold(f64::MIN, f64::max);
        assert!((top + 150.0).abs() < 1e-6, "{:?}", r[&2]);
    }
}
