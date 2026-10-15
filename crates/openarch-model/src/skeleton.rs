//! Weighted straight skeleton of a polygon with holes: the roof surface of a footprint.
//!
//! Every edge of the footprint moves inward over time, each at its own horizontal speed. Time
//! is the height above the base, so an edge with a slope of `theta` moves at `1 / tan(theta)`,
//! and an edge with no slope (a gable end) does not move at all. The path each corner takes
//! while the outline shrinks is a hip, ridge or valley, and the area an edge sweeps is the
//! roof plane over that edge.
//!
//! The outline is simulated event by event: an *edge event* is an edge shrinking to nothing
//! (a hip meeting a ridge), a *split event* is a reflex corner running into another edge (a
//! valley), which cuts the outline in two or, when the edge belongs to another loop, joins two
//! loops into one. Faces are collected as the corners that bound them are replaced.
//!
//! The outer loop has to run counter-clockwise and holes clockwise, so the material is on the
//! left of every edge. Roofs orient their loops before calling in.

use crate::geom::Point;

/// One roof plane (or a vertical gable face) as a polygon of x, y and height above the base.
#[derive(Debug, Clone)]
pub struct SkFace {
    /// The loop and edge, as numbered in the input, this plane grew from.
    pub loop_idx: usize,
    pub edge_idx: usize,
    pub poly: Vec<[f64; 3]>,
    /// The edge does not slope: the face is vertical.
    pub vertical: bool,
}

type V2 = [f64; 2];

struct Line {
    n: V2,
    d: V2,
    c: f64,
    s: f64,
    loop_idx: usize,
    edge_idx: usize,
}

struct Vtx {
    p: V2,
    t0: f64,
    v: V2,
    lin: usize,
    lout: usize,
    pin: usize,
    pout: usize,
    prev: usize,
    next: usize,
    alive: bool,
    reflex: bool,
}

impl Vtx {
    fn at(&self, t: f64) -> V2 {
        [self.p[0] + self.v[0] * (t - self.t0), self.p[1] + self.v[1] * (t - self.t0)]
    }
}

struct Piece {
    line: usize,
    left: Vec<[f64; 3]>,
    right: Vec<[f64; 3]>,
    /// A piece that was cut in two by a split event hands over to its two halves.
    children: Option<(usize, usize)>,
}

enum Ev {
    Edge(usize),
    Split { v: usize, ha: usize },
}

fn dot(a: V2, b: V2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// How fast a corner between two edges travels, so that it stays on both of their lines.
fn corner_velocity(a: &Line, b: &Line) -> V2 {
    let det = cross(a.n, b.n);
    if det.abs() > 1e-9 {
        [(a.s * b.n[1] - b.s * a.n[1]) / det, (a.n[0] * b.s - b.n[0] * a.s) / det]
    } else if dot(a.n, b.n) > 0.0 {
        // collinear edges: the corner slides along the normal at their mean speed
        let s = (a.s + b.s) / 2.0;
        [a.n[0] * s, a.n[1] * s]
    } else {
        [0.0, 0.0]
    }
}

struct Sk {
    lines: Vec<Line>,
    vs: Vec<Vtx>,
    pieces: Vec<Piece>,
}

impl Sk {
    fn new_vertex(&mut self, p: V2, t: f64, lin: usize, lout: usize, pin: usize, pout: usize, prev: usize, next: usize) -> usize {
        let v = corner_velocity(&self.lines[lin], &self.lines[lout]);
        let reflex = cross(self.lines[lin].d, self.lines[lout].d) < -1e-9;
        self.vs.push(Vtx { p, t0: t, v, lin, lout, pin, pout, prev, next, alive: true, reflex });
        self.vs.len() - 1
    }

    /// A loop that has shrunk to two corners is finished: its two pieces close along the last segment.
    fn finish_if_pair(&mut self, a: usize, t: f64) {
        if !self.vs[a].alive {
            return;
        }
        let b = self.vs[a].next;
        if b == a || self.vs[b].next != a {
            return;
        }
        for &k in &[a, b] {
            let q = self.vs[k].at(t);
            let (pin, pout) = (self.vs[k].pin, self.vs[k].pout);
            self.pieces[pin].right.push([q[0], q[1], t]);
            self.pieces[pout].left.push([q[0], q[1], t]);
            self.vs[k].alive = false;
        }
    }
}

/// Build the roof surface. `speeds[l][i]` is the horizontal speed of the edge from point `i` to
/// point `i + 1` of loop `l` (0 for an edge that does not slope). Faces come back as polygons
/// whose heights are the time at which the outline reached each corner.
pub fn skeleton(loops: &[Vec<Point>], speeds: &[Vec<f64>]) -> Vec<SkFace> {
    let mut sk = Sk { lines: vec![], vs: vec![], pieces: vec![] };
    // set up the loops: one line, one piece and one corner per edge
    for (li, pts) in loops.iter().enumerate() {
        // drop corners that sit on top of the next one
        let keep: Vec<usize> = (0..pts.len()).filter(|&i| pts[i].dist(&pts[(i + 1) % pts.len()]) > 1e-6).collect();
        let n = keep.len();
        if n < 3 {
            continue;
        }
        let first_line = sk.lines.len();
        let first_piece = sk.pieces.len();
        for (k, &i) in keep.iter().enumerate() {
            let a = pts[i];
            let b = pts[keep[(k + 1) % n]];
            let len = a.dist(&b);
            let d = [(b.x - a.x) / len, (b.y - a.y) / len];
            let nrm = [-d[1], d[0]];
            let s = speeds.get(li).and_then(|v| v.get(i)).copied().unwrap_or(0.0).max(0.0);
            sk.lines.push(Line { n: nrm, d, c: nrm[0] * a.x + nrm[1] * a.y, s, loop_idx: li, edge_idx: i });
            sk.pieces.push(Piece { line: first_line + k, left: vec![[a.x, a.y, 0.0]], right: vec![[b.x, b.y, 0.0]], children: None });
        }
        let first_v = sk.vs.len();
        for (k, &i) in keep.iter().enumerate() {
            let prev_k = (k + n - 1) % n;
            let p = pts[i];
            sk.new_vertex([p.x, p.y], 0.0, first_line + prev_k, first_line + k, first_piece + prev_k, first_piece + k, first_v + prev_k, first_v + (k + 1) % n);
        }
    }

    let initial_pieces = sk.pieces.len();
    let mut t = 0.0;
    for _ in 0..20000 {
        let alive: Vec<usize> = (0..sk.vs.len()).filter(|&i| sk.vs[i].alive).collect();
        if alive.is_empty() {
            break;
        }
        let mut best: Option<(f64, Ev)> = None;
        let consider = |time: f64, ev: Ev, edge_pref: bool, best: &mut Option<(f64, Ev)>| {
            let better = match best {
                None => true,
                Some((bt, _)) => time < *bt - 1e-9 || (edge_pref && time < *bt + 1e-9 && matches!(best, Some((_, Ev::Split { .. })))),
            };
            if better {
                *best = Some((time, ev));
            }
        };
        for &a in &alive {
            let va = &sk.vs[a];
            let b = va.next;
            let vb = &sk.vs[b];
            // the edge from a to b shrinks to nothing
            let d = sk.lines[va.lout].d;
            let (pa, pb) = (va.at(t), vb.at(t));
            let gap = dot([pb[0] - pa[0], pb[1] - pa[1]], d);
            let rate = dot([vb.v[0] - va.v[0], vb.v[1] - va.v[1]], d);
            if rate < -1e-12 {
                let tau = (gap / -rate).max(0.0);
                consider(t + tau, Ev::Edge(a), true, &mut best);
            } else if gap <= 1e-7 && rate <= 1e-12 {
                consider(t, Ev::Edge(a), true, &mut best);
            }
            // a reflex corner runs into another edge
            if va.reflex {
                for &ha in &alive {
                    let hv = &sk.vs[ha];
                    let hl = &sk.lines[hv.lout];
                    if hv.lout == va.lin || hv.lout == va.lout {
                        continue;
                    }
                    let den = hl.s - dot(hl.n, va.v);
                    if den <= 1e-12 {
                        continue;
                    }
                    let dist = dot(hl.n, pa) - (hl.c + hl.s * t);
                    if dist < -1e-6 {
                        continue;
                    }
                    let tau = dist.max(0.0) / den;
                    let x = va.at(t + tau);
                    let hb = &sk.vs[hv.next];
                    let (a2, b2) = (hv.at(t + tau), hb.at(t + tau));
                    let along = dot([x[0] - a2[0], x[1] - a2[1]], hl.d);
                    let len = dot([b2[0] - a2[0], b2[1] - a2[1]], hl.d);
                    if len < -1e-6 || along < -1e-6 || along > len + 1e-6 {
                        continue;
                    }
                    consider(t + tau, Ev::Split { v: a, ha }, false, &mut best);
                }
            }
        }
        let Some((time, ev)) = best else { break };
        t = time.max(t);
        match ev {
            Ev::Edge(a) => {
                let b = sk.vs[a].next;
                let (pa, pb) = (sk.vs[a].at(t), sk.vs[b].at(t));
                let x = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
                let x3 = [x[0], x[1], t];
                let (a_prev, b_next) = (sk.vs[a].prev, sk.vs[b].next);
                let (lin, pin, lout, pout) = (sk.vs[a].lin, sk.vs[a].pin, sk.vs[b].lout, sk.vs[b].pout);
                let gone = sk.vs[a].pout;
                sk.pieces[pin].right.push(x3);
                sk.pieces[pout].left.push(x3);
                sk.pieces[gone].left.push(x3);
                sk.pieces[gone].right.push(x3);
                sk.vs[a].alive = false;
                sk.vs[b].alive = false;
                if a_prev == b {
                    // the loop was a pair already
                    continue;
                }
                let nv = sk.new_vertex(x, t, lin, lout, pin, pout, a_prev, b_next);
                if a_prev == b_next {
                    sk.vs[a_prev].next = nv;
                    sk.vs[a_prev].prev = nv;
                } else {
                    sk.vs[a_prev].next = nv;
                    sk.vs[b_next].prev = nv;
                }
                sk.finish_if_pair(nv, t);
            }
            Ev::Split { v, ha } => {
                let hb = sk.vs[ha].next;
                let x = sk.vs[v].at(t);
                let x3 = [x[0], x[1], t];
                let (v_prev, v_next) = (sk.vs[v].prev, sk.vs[v].next);
                let (vlin, vlout, p, q) = (sk.vs[v].lin, sk.vs[v].lout, sk.vs[v].pin, sk.vs[v].pout);
                let h = sk.vs[ha].pout;
                let hline = sk.vs[ha].lout;
                sk.pieces[p].right.push(x3);
                sk.pieces[q].left.push(x3);
                let (h_left, h_right) = (sk.pieces[h].left.clone(), sk.pieces[h].right.clone());
                        let line = sk.pieces[h].line;
                sk.pieces.push(Piece { line, left: h_left, right: vec![x3], children: None });
                let h1 = sk.pieces.len() - 1;
                sk.pieces.push(Piece { line, left: vec![x3], right: h_right, children: None });
                let h2 = sk.pieces.len() - 1;
                sk.pieces[h].children = Some((h1, h2));
                sk.vs[v].alive = false;
                let n1 = sk.new_vertex(x, t, vlin, hline, p, h2, v_prev, hb);
                let n2 = sk.new_vertex(x, t, hline, vlout, h1, q, ha, v_next);
                // relink (the neighbours may be the same corner)
                sk.vs[v_prev].next = n1;
                sk.vs[hb].prev = n1;
                sk.vs[ha].next = n2;
                sk.vs[v_next].prev = n2;
                sk.vs[ha].pout = h1;
                sk.vs[hb].pin = h2;
                sk.finish_if_pair(n1, t);
                sk.finish_if_pair(n2, t);
            }
        }
    }
    // anything left over (a degenerate outline) closes where it stands
    for i in 0..sk.vs.len() {
        if sk.vs[i].alive {
            let q = sk.vs[i].at(t);
            let (pin, pout) = (sk.vs[i].pin, sk.vs[i].pout);
            sk.pieces[pin].right.push([q[0], q[1], t]);
            sk.pieces[pout].left.push([q[0], q[1], t]);
            sk.vs[i].alive = false;
        }
    }

    // a plane cut in two by a split is still one plane: its halves meet at the split point
    fn outline(pieces: &[Piece], i: usize, out: &mut Vec<[f64; 3]>) {
        match pieces[i].children {
            Some((a, b)) => {
                outline(pieces, a, out);
                outline(pieces, b, out);
            }
            None => {
                out.extend(pieces[i].left.iter().copied());
                out.extend(pieces[i].right.iter().rev().copied());
            }
        }
    }
    let mut faces = vec![];
    for (i, pc) in sk.pieces.iter().enumerate().take(initial_pieces) {
        let mut poly: Vec<[f64; 3]> = vec![];
        outline(&sk.pieces, i, &mut poly);
        let same = |l: &[f64; 3], q: &[f64; 3]| (l[0] - q[0]).hypot(l[1] - q[1]).max((l[2] - q[2]).abs()) <= 1e-6;
        let mut clean: Vec<[f64; 3]> = vec![];
        for q in poly {
            if clean.last().map_or(true, |l| !same(l, &q)) {
                clean.push(q);
            }
        }
        while clean.len() > 1 && same(&clean[0], &clean[clean.len() - 1]) {
            clean.pop();
        }
        if clean.len() < 3 {
            continue;
        }
        let line = &sk.lines[pc.line];
        faces.push(SkFace { loop_idx: line.loop_idx, edge_idx: line.edge_idx, poly: clean, vertical: line.s <= 1e-12 });
    }
    faces
}

/// Area of a face as seen from above (shoelace on x and y).
pub fn plan_area(poly: &[[f64; 3]]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| poly[i][0] * poly[(i + 1) % n][1] - poly[(i + 1) % n][0] * poly[i][1]).sum::<f64>().abs() / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(v: &[(f64, f64)]) -> Vec<Point> {
        v.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    fn cot(deg: f64) -> f64 {
        1.0 / deg.to_radians().tan()
    }

    fn area(p: &[Point]) -> f64 {
        let n = p.len();
        (0..n).map(|i| p[i].x * p[(i + 1) % n].y - p[(i + 1) % n].x * p[i].y).sum::<f64>() / 2.0
    }

    /// Every sloped face is flat and sits on its plane, and the faces tile the footprint.
    fn check(loops: &[Vec<Point>], speeds: &[Vec<f64>], faces: &[SkFace]) {
        let footprint: f64 = loops.iter().map(|l| area(l)).sum();
        let covered: f64 = faces.iter().map(|f| plan_area(&f.poly)).sum();
        assert!((covered - footprint).abs() < footprint * 1e-6 + 1.0, "faces cover {covered} of {footprint}");
        for f in faces.iter().filter(|f| !f.vertical) {
            let l = &loops[f.loop_idx];
            let (a, b) = (l[f.edge_idx], l[(f.edge_idx + 1) % l.len()]);
            let len = a.dist(&b);
            let n = [-(b.y - a.y) / len, (b.x - a.x) / len];
            let s = speeds[f.loop_idx][f.edge_idx];
            for q in &f.poly {
                let dist = (q[0] - a.x) * n[0] + (q[1] - a.y) * n[1];
                assert!((q[2] - dist / s).abs() < 1e-4, "corner {q:?} is off the plane of edge {} (height {} against {})", f.edge_idx, q[2], dist / s);
            }
        }
    }

    fn top(faces: &[SkFace]) -> f64 {
        faces.iter().flat_map(|f| f.poly.iter().map(|q| q[2])).fold(0.0, f64::max)
    }

    #[test]
    fn a_rectangle_makes_a_hip_roof() {
        let l = pts(&[(0., 0.), (10000., 0.), (10000., 6000.), (0., 6000.)]);
        let sp = vec![vec![cot(30.0); 4]];
        let faces = skeleton(&[l.clone()], &sp);
        assert_eq!(faces.len(), 4);
        assert!((top(&faces) - 3000.0 * 30f64.to_radians().tan()).abs() < 1e-6);
        check(&[l], &sp, &faces);
    }

    #[test]
    fn a_square_makes_a_pyramid() {
        let l = pts(&[(0., 0.), (6000., 0.), (6000., 6000.), (0., 6000.)]);
        let sp = vec![vec![cot(45.0); 4]];
        let faces = skeleton(&[l.clone()], &sp);
        assert_eq!(faces.len(), 4);
        assert!((top(&faces) - 3000.0).abs() < 1e-6);
        check(&[l], &sp, &faces);
    }

    #[test]
    fn edges_that_do_not_slope_make_a_gable() {
        let l = pts(&[(0., 0.), (10000., 0.), (10000., 6000.), (0., 6000.)]);
        let c = cot(30.0);
        let sp = vec![vec![c, 0.0, c, 0.0]];
        let faces = skeleton(&[l.clone()], &sp);
        assert_eq!(faces.iter().filter(|f| f.vertical).count(), 2, "two gable ends");
        assert_eq!(faces.iter().filter(|f| !f.vertical).count(), 2, "two roof planes");
        assert!((top(&faces) - 3000.0 * 30f64.to_radians().tan()).abs() < 1e-6);
        // the ridge runs the whole length
        let ridge: Vec<&[f64; 3]> = faces.iter().flat_map(|f| f.poly.iter()).filter(|q| (q[2] - top(&faces)).abs() < 1e-6).collect();
        let xs: Vec<f64> = ridge.iter().map(|q| q[0]).collect();
        assert!(xs.iter().cloned().fold(f64::MAX, f64::min) < 1.0 && xs.iter().cloned().fold(f64::MIN, f64::max) > 9999.0);
        check(&[l], &sp, &faces);
    }

    #[test]
    fn an_l_shape_gets_a_valley() {
        let l = pts(&[(0., 0.), (10000., 0.), (10000., 4000.), (4000., 4000.), (4000., 10000.), (0., 10000.)]);
        let sp = vec![vec![cot(25.0); 6]];
        let faces = skeleton(&[l.clone()], &sp);
        assert_eq!(faces.len(), 6);
        check(&[l], &sp, &faces);
    }

    #[test]
    fn a_u_shape_and_a_t_shape_cover_their_footprint() {
        let u = pts(&[(0., 0.), (12000., 0.), (12000., 8000.), (8000., 8000.), (8000., 3000.), (4000., 3000.), (4000., 8000.), (0., 8000.)]);
        let t = pts(&[(0., 0.), (12000., 0.), (12000., 4000.), (8000., 4000.), (8000., 10000.), (4000., 10000.), (4000., 4000.), (0., 4000.)]);
        for l in [u, t] {
            let sp = vec![vec![cot(22.5); l.len()]];
            let faces = skeleton(&[l.clone()], &sp);
            check(&[l], &sp, &faces);
        }
    }

    #[test]
    fn different_slopes_per_edge_still_make_planes() {
        let l = pts(&[(0., 0.), (10000., 0.), (10000., 6000.), (0., 6000.)]);
        let sp = vec![vec![cot(20.0), cot(35.0), cot(45.0), cot(35.0)]];
        let faces = skeleton(&[l.clone()], &sp);
        check(&[l], &sp, &faces);
    }

    #[test]
    fn a_hole_makes_a_ring_roof() {
        let outer = pts(&[(0., 0.), (12000., 0.), (12000., 12000.), (0., 12000.)]);
        // clockwise, so the material is on its left
        let hole = pts(&[(4000., 4000.), (4000., 8000.), (8000., 8000.), (8000., 4000.)]);
        let sp = vec![vec![cot(30.0); 4], vec![cot(30.0); 4]];
        let loops = [outer, hole];
        let faces = skeleton(&loops, &sp);
        check(&loops, &sp, &faces);
        // the ring has 8 planes
        assert_eq!(faces.len(), 8);
    }

    #[test]
    fn a_triangle_and_a_pentagon_close() {
        let tri = pts(&[(0., 0.), (8000., 0.), (3000., 6000.)]);
        let pent = pts(&[(0., 0.), (8000., 0.), (10000., 5000.), (4000., 9000.), (-2000., 5000.)]);
        for l in [tri, pent] {
            let sp = vec![vec![cot(30.0); l.len()]];
            let faces = skeleton(&[l.clone()], &sp);
            check(&[l], &sp, &faces);
        }
    }

    #[test]
    fn a_circle_makes_a_cone() {
        let n = 48;
        let l: Vec<Point> = (0..n).map(|i| Point::new(5000.0 * (i as f64 * std::f64::consts::TAU / n as f64).cos(), 5000.0 * (i as f64 * std::f64::consts::TAU / n as f64).sin())).collect();
        let sp = vec![vec![cot(30.0); n]];
        let faces = skeleton(&[l.clone()], &sp);
        check(&[l], &sp, &faces);
        let h = 5000.0 * (std::f64::consts::PI / n as f64).cos() * 30f64.to_radians().tan();
        assert!((top(&faces) - h).abs() < 1.0, "{} vs {}", top(&faces), h);
    }

    /// Star-shaped outlines with random radii and slopes: every one has to tile and stay planar.
    #[test]
    fn random_star_shapes_tile_their_footprint() {
        let mut seed = 12345u64;
        let mut rnd = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / ((1u64 << 31) as f64)
        };
        let mut done = 0;
        for case in 0..300 {
            let n = 4 + (rnd() * 8.0) as usize;
            let mut angles: Vec<f64> = (0..n).map(|_| rnd() * std::f64::consts::TAU).collect();
            angles.sort_by(|a, b| a.total_cmp(b));
            let l: Vec<Point> = angles.iter().map(|a| { let r = 3000.0 + rnd() * 6000.0; Point::new(r * a.cos(), r * a.sin()) }).collect();
            // keep well-formed outlines: no tiny edges or tiny turns
            // star-shaped about the origin needs every angular gap under half a turn
            let gaps_ok = (0..n).all(|i| { let g = angles[(i + 1) % n] - angles[i]; (if g <= 0.0 { g + std::f64::consts::TAU } else { g }) < 3.0 });
            let ok = gaps_ok && (0..n).all(|i| l[i].dist(&l[(i + 1) % n]) > 800.0) && area(&l) > 1.0e7;
            if !ok {
                continue;
            }
            let uniform = case % 2 == 0;
            let base = 15.0 + rnd() * 30.0;
            let gables = case % 3 == 0;
            let mut sp = vec![(0..n).map(|_| if uniform { cot(base) } else { cot(15.0 + rnd() * 30.0) }).collect::<Vec<_>>()];
            if gables {
                // a few edges that do not slope; at least one stays sloped
                for i in 0..n {
                    if i % 3 == 1 {
                        sp[0][i] = 0.0;
                    }
                }
            }
            let faces = skeleton(&[l.clone()], &sp);
            let covered: f64 = faces.iter().map(|f| plan_area(&f.poly)).sum();
            if (covered - area(&l)).abs() > area(&l) * 1e-6 + 1.0 {
                println!("CASE {case} uniform {uniform}: {:?} speeds {:?}", l.iter().map(|p| (p.x.round(), p.y.round())).collect::<Vec<_>>(), sp[0].iter().map(|c| (1.0 / c).atan().to_degrees()).collect::<Vec<_>>());
                for f in &faces { println!("  edge {} area {:.0} {:?}", f.edge_idx, plan_area(&f.poly), f.poly.iter().map(|q| (q[0].round(), q[1].round(), q[2].round())).collect::<Vec<_>>()); }
            }
            check(&[l], &sp, &faces);
            done += 1;
        }
        assert!(done > 100, "only {done} usable shapes");
    }
}

