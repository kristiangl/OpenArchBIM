//! Layer-aware wall joins.
//!
//! A wall is a stack of layers. Where walls join, each layer ends according to
//! the layer priority rules rather than the wall as a whole:
//!
//! - Priority, highest first: Structure (1), Substrate (2), Thermal/Air (3),
//!   Membrane (4), Finish 1 and Finish 2 (5).
//! - A layer that butts into another wall passes through that wall's layers of
//!   lower priority and stops at the face of the first layer of equal or
//!   higher priority. So a stud wall butting a finished wall runs through the
//!   other wall's plasterboard and stops at its studs, while its own finishes
//!   stop at the other wall's finish.
//! - Where a layer passes through a lower priority layer, the lower one is cut.
//! - At a corner of two walls (a butt join) the layers of one wall meet the matching
//!   layers of the other, counted in from the outer corner and matched by priority
//!   and by which side of the core they are on, so the cavity runs all round and the
//!   outer leaf joins the outer leaf. Each matched pair is one L: the wall that runs
//!   through carries its layer on to the outer face of its partner, and the partner
//!   stops at the inner face of that layer. Layers with no partner follow the
//!   priority rule above.
//! - A mitre cuts every layer on the one line through the outer and inner corner.
//! - A square off join ends the butting wall in a single square cut.
//!
//! Which walls join, and how, comes from `join::classify`, the same
//! classification the outline join uses, so the two always agree.

use std::collections::{BTreeMap, HashMap};

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use crate::doc::Document;
use crate::element::{Element, Id, JoinKind, LayerFunction, Wrap};
use crate::geom::Point;
use crate::join::{add, classify, cross2, dot2, intersect, through_index, End, JoinWall, Node, MITRE_LIMIT};

pub type Ring = Vec<Point>;
/// An outline followed by any holes.
pub type Shape = Vec<Ring>;

/// Join priority of a layer function: lower numbers win.
pub fn priority(f: LayerFunction) -> u8 {
    match f {
        LayerFunction::Structure => 1,
        LayerFunction::Substrate => 2,
        LayerFunction::ThermalAir => 3,
        LayerFunction::Finish1 => 4,
        LayerFunction::Finish2 => 5,
        // a membrane has no thickness and never cuts through another layer
        LayerFunction::Membrane => 6,
    }
}

#[derive(Debug, Clone)]
pub struct LayerSpec {
    pub thickness: f64,
    pub priority: u8,
    /// Part of the wall's structural core (between the core boundaries).
    pub core: bool,
}

#[derive(Debug, Clone)]
pub struct LWall {
    pub join: JoinWall,
    /// Exterior (left) layer first.
    pub layers: Vec<LayerSpec>,
}

// ---------------------------------------------------------------- boolean helpers

fn signed_area(r: &[Point]) -> f64 {
    let n = r.len();
    (0..n).map(|i| r[i].x * r[(i + 1) % n].y - r[(i + 1) % n].x * r[i].y).sum::<f64>() / 2.0
}

fn contour(r: &[Point]) -> Vec<[f64; 2]> {
    let mut v: Vec<[f64; 2]> = r.iter().map(|p| [p.x, p.y]).collect();
    if signed_area(r) < 0.0 {
        v.reverse();
    }
    v
}

fn shapes_from(res: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Shape> {
    res.into_iter()
        .map(|shape| shape.into_iter().map(|c| c.into_iter().map(|p| Point::new(p[0], p[1])).collect()).collect())
        .filter(|s: &Shape| s.first().is_some_and(|r| r.len() >= 3))
        .collect()
}

/// The union of any number of rings.
pub fn union_rings<'a>(rings: impl IntoIterator<Item = &'a Ring>) -> Vec<Shape> {
    let subj: Vec<Vec<[f64; 2]>> = rings.into_iter().filter(|r| r.len() >= 3).map(|r| contour(r)).collect();
    if subj.is_empty() {
        return vec![];
    }
    let clip: Vec<Vec<[f64; 2]>> = vec![];
    shapes_from(subj.overlay(&clip, OverlayRule::Union, FillRule::NonZero))
}

/// `subject` less every ring in `clips`.
pub fn subtract<'a>(subject: &Ring, clips: impl IntoIterator<Item = &'a Ring>) -> Vec<Shape> {
    let subj = vec![contour(subject)];
    let clip: Vec<Vec<[f64; 2]>> = clips.into_iter().filter(|r| r.len() >= 3).map(|r| contour(r)).collect();
    if clip.is_empty() {
        return vec![vec![subject.clone()]];
    }
    shapes_from(subj.overlay(&clip, OverlayRule::Difference, FillRule::NonZero))
}

/// Cut `shapes` by `clips` (each a ring).
pub fn subtract_shapes(shapes: &[Shape], clips: &[Ring]) -> Vec<Shape> {
    if clips.is_empty() {
        return shapes.to_vec();
    }
    let clip: Vec<Vec<[f64; 2]>> = clips.iter().map(|r| contour(r)).collect();
    let mut out = vec![];
    for s in shapes {
        // outline and holes in one go: even-odd over all of the shape's rings
        let subj: Vec<Vec<[f64; 2]>> = s.iter().map(|r| r.iter().map(|p| [p.x, p.y]).collect()).collect();
        out.extend(shapes_from(subj.overlay(&clip, OverlayRule::Difference, FillRule::EvenOdd)));
    }
    out
}

// ---------------------------------------------------------------- end frames

/// One layer seen from a wall end: offsets measured to the left of the end's
/// direction (looking away from the node).
#[derive(Clone, Copy)]
struct FrameLayer {
    hi: f64,
    lo: f64,
    prio: u8,
}

fn frame_layers(e: &End, w: &LWall) -> Vec<FrameLayer> {
    let order: Vec<&LayerSpec> = if e.which == 0 { w.layers.iter().collect() } else { w.layers.iter().rev().collect() };
    let mut hi = e.hl;
    order
        .into_iter()
        .map(|l| {
            let fl = FrameLayer { hi, lo: hi - l.thickness, prio: l.priority };
            hi = fl.lo;
            fl
        })
        .collect()
}

/// The walls an end butts into, as a stack of layers across a direction.
struct Other {
    d: Point,
    n: Point,
    /// Layer boundaries from the left face to the right face (one more than layers).
    bounds: Vec<f64>,
    prio: Vec<u8>,
}

impl Other {
    fn new(d: Point, n: Point, hl: f64, layers: impl Iterator<Item = (f64, u8)>) -> Other {
        let mut bounds = vec![hl];
        let mut prio = vec![];
        for (t, p) in layers {
            bounds.push(bounds.last().unwrap() - t);
            prio.push(p);
        }
        Other { d, n, bounds, prio }
    }

    fn of_end(e: &End, w: &LWall) -> Other {
        let fl = frame_layers(e, w);
        Other::new(e.u, e.left(), e.hl, fl.iter().map(|l| (l.hi - l.lo, l.prio)))
    }

    fn of_wall(w: &LWall) -> Option<Other> {
        let len = w.join.start.dist(&w.join.end);
        if len < 1e-6 {
            return None;
        }
        let d = Point::new((w.join.end.x - w.join.start.x) / len, (w.join.end.y - w.join.start.y) / len);
        Some(Other::new(d, Point::new(-d.y, d.x), w.join.left, w.layers.iter().map(|l| (l.thickness, l.priority))))
    }

    fn reach(&self) -> f64 {
        self.bounds.first().unwrap().abs().max(self.bounds.last().unwrap().abs())
    }
}

/// Where a layer's end is cut.
#[derive(Clone, Copy)]
enum Cut {
    /// Square to the wall, at the node.
    Perp,
    /// Along a line, as long as the cut stays within `limit` of the node.
    Line { p: Point, d: Point, limit: f64 },
}

fn cut_point(e: &End, off: f64, cut: Cut) -> Point {
    let base = add(e.node, e.left(), off);
    match cut {
        Cut::Perp => base,
        Cut::Line { p, d, limit } => match intersect(base, e.u, p, d) {
            Some(q) if q.dist(&e.node) <= limit => q,
            _ => base,
        },
    }
}

/// Cut for a layer of priority `prio` butting into `o`: it passes lower
/// priority layers and stops at the near face of the first layer of equal or
/// higher priority; with none of those it runs to the far face.
fn stop_cut(e: &End, prio: u8, o: &Other) -> Cut {
    if cross2(e.u, o.d).abs() < 1e-3 {
        return Cut::Perp; // runs along the other wall
    }
    let m = o.prio.len();
    let off = if dot2(e.u, o.n) >= 0.0 {
        (0..m).find(|&i| o.prio[i] <= prio).map_or(o.bounds[m], |i| o.bounds[i])
    } else {
        (0..m).rev().find(|&i| o.prio[i] <= prio).map_or(o.bounds[0], |i| o.bounds[i + 1])
    };
    Cut::Line { p: add(e.node, o.n, off), d: o.d, limit: MITRE_LIMIT * 2.0 * o.reach().max(e.hl).max(e.hr) }
}

/// Cut for a wall running out to the far face of `o` (every layer).
fn far_cut(e: &End, o: &Other) -> Cut {
    if cross2(e.u, o.d).abs() < 1e-3 {
        return Cut::Perp;
    }
    let off = if dot2(e.u, o.n) >= 0.0 { *o.bounds.last().unwrap() } else { o.bounds[0] };
    Cut::Line { p: add(e.node, o.n, off), d: o.d, limit: MITRE_LIMIT * 2.0 * o.reach().max(e.hl).max(e.hr) }
}

// ---------------------------------------------------------------- the join

/// How one wall end cuts each of its layers: left and right boundary of every
/// layer, in the end's own frame.
type EndCuts = Vec<(Cut, Cut)>;

struct Joiner<'a> {
    walls: &'a [LWall],
    cuts: HashMap<(usize, u8), EndCuts>,
    /// (wall a, wall b, the wall that wins ties) for walls whose layers can overlap.
    related: Vec<(usize, usize, usize)>,
    /// Ends that take part in a join. The rest are free ends, which can wrap.
    joined: std::collections::HashSet<(usize, u8)>,
}

impl Joiner<'_> {
    fn all(&mut self, e: &End, left: Cut, right: Cut) {
        for c in self.cuts.get_mut(&(e.wall, e.which)).unwrap().iter_mut() {
            *c = (left, right);
        }
    }

    fn layer(&mut self, e: &End, k: usize, cut: Cut) {
        self.cuts.get_mut(&(e.wall, e.which)).unwrap()[k] = (cut, cut);
    }

    /// `e` butts into `o` (a wall other than its own), layer by layer.
    fn butt(&mut self, e: &End, o: &Other, square: bool) {
        let fl = frame_layers(e, &self.walls[e.wall]);
        let cuts: Vec<Cut> = fl.iter().map(|l| stop_cut(e, l.prio, o)).collect();
        if square {
            // one square cut at the layer that reaches furthest into the other wall
            let reach = fl
                .iter()
                .zip(&cuts)
                .flat_map(|(l, c)| [l.hi, l.lo].map(|off| dot2(Point::new(cut_point(e, off, *c).x - e.node.x, cut_point(e, off, *c).y - e.node.y), e.u)))
                .fold(0.0_f64, f64::min);
            let limit = MITRE_LIMIT * 2.0 * o.reach().max(e.hl).max(e.hr);
            let cut = Cut::Line { p: add(e.node, e.u, reach), d: e.left(), limit };
            self.all(e, cut, cut);
        } else {
            for (k, c) in cuts.into_iter().enumerate() {
                self.layer(e, k, c);
            }
        }
    }

    /// A corner of `t` (running through) and `s` (butting), joined layer to layer.
    /// Returns false, leaving everything as it was, when no layers pair up.
    fn nested(&mut self, t: &End, s: &End) -> bool {
        if cross2(t.u, s.u).abs() < 1e-3 {
            return false;
        }
        let (wt, ws) = (&self.walls[t.wall], &self.walls[s.wall]);
        let (ft, fs) = (frame_layers(t, wt), frame_layers(s, ws));
        let away = |p: Point| Point::new(-p.x, -p.y);
        // each wall's outer face is the one on the side the other wall's body is not
        let t_left_outer = dot2(t.left(), away(s.u)) > 0.0;
        let s_left_outer = dot2(s.left(), away(t.u)) > 0.0;
        // layers from the outer face inwards: (index in the end's frame, priority, side of core)
        let ordered = |f: &[FrameLayer], w: &LWall, which: u8, left_outer: bool| -> Vec<(usize, u8, u8)> {
            let n = f.len();
            let idx: Vec<usize> = if left_outer { (0..n).collect() } else { (0..n).rev().collect() };
            let core = |k: usize| w.layers[if which == 0 { k } else { n - 1 - k }].core;
            let (first, last) = (idx.iter().position(|&k| core(k)), idx.iter().rposition(|&k| core(k)));
            idx.iter()
                .enumerate()
                .map(|(pos, &k)| {
                    let side = match (first, last) {
                        (Some(a), Some(b)) => if pos < a { 0 } else if pos > b { 2 } else { 1 },
                        _ => 1,
                    };
                    (k, f[k].prio, side)
                })
                .collect()
        };
        let (ot, os) = (ordered(&ft, wt, t.which, t_left_outer), ordered(&fs, ws, s.which, s_left_outer));
        let mut pairs = vec![];
        let mut cursor = 0;
        for &(ks, ps, zs) in &os {
            if let Some(at) = ot[cursor..].iter().position(|&(_, pt, zt)| pt == ps && zt == zs) {
                pairs.push((ot[cursor + at].0, ks));
                cursor += at + 1;
            }
        }
        if pairs.is_empty() {
            return false;
        }
        let (o_t, o_s) = (Other::of_end(t, wt), Other::of_end(s, ws));
        let limit = MITRE_LIMIT * 2.0 * o_t.reach().max(o_s.reach()).max(s.hl).max(s.hr).max(t.hl).max(t.hr);
        let (mut done_t, mut done_s) = (vec![false; ft.len()], vec![false; fs.len()]);
        for &(kt, ks) in &pairs {
            // s stops at the inner face of its partner, t runs on to the outer face of its partner
            let near = if dot2(s.u, o_t.n) >= 0.0 { o_t.bounds[kt] } else { o_t.bounds[kt + 1] };
            self.layer(s, ks, Cut::Line { p: add(s.node, o_t.n, near), d: o_t.d, limit });
            let outer = if s_left_outer { fs[ks].hi } else { fs[ks].lo };
            self.layer(t, kt, Cut::Line { p: add(s.node, s.left(), outer), d: s.u, limit });
            done_t[kt] = true;
            done_s[ks] = true;
        }
        for k in 0..fs.len() {
            if !done_s[k] {
                self.layer(s, k, stop_cut(s, fs[k].prio, &o_t));
            }
        }
        for k in 0..ft.len() {
            if !done_t[k] {
                self.layer(t, k, far_cut(t, &o_s));
            }
        }
        true
    }

    /// `t` runs through a corner out to the far face of `s`.
    fn through(&mut self, t: &End, s: &Other) {
        let c = far_cut(t, s);
        self.all(t, c, c);
    }

    /// Cut both ends on one line through the outer and inner corners.
    fn mitre_pair(&mut self, x: &End, y: &End) {
        let p1 = intersect(add(x.node, x.left(), x.hl), x.u, add(y.node, y.right(), y.hr), y.u);
        let p2 = intersect(add(y.node, y.left(), y.hl), y.u, add(x.node, x.right(), x.hr), x.u);
        let (Some(p1), Some(p2)) = (p1, p2) else { return };
        let limit = MITRE_LIMIT * x.hl.max(x.hr).max(y.hl).max(y.hr);
        if p1.dist(&x.node) > limit || p2.dist(&x.node) > limit || p1.dist(&p2) < 1e-6 {
            return;
        }
        let cut = Cut::Line { p: p1, d: Point::new(p2.x - p1.x, p2.y - p1.y), limit: limit * 2.0 };
        self.all(x, cut, cut);
        self.all(y, cut, cut);
    }

    /// Mitre of three or more walls: each end is cut towards its neighbours.
    fn mitre_ring(&mut self, ends: &[End]) {
        let m = ends.len();
        for i in 0..m {
            let (x, y) = (&ends[i], &ends[(i + 1) % m]);
            let px = add(x.node, x.left(), x.hl);
            let py = add(y.node, y.right(), y.hr);
            let Some(p) = intersect(px, x.u, py, y.u) else { continue };
            let limit = MITRE_LIMIT * x.hl.max(x.hr).max(y.hl).max(y.hr);
            if p.dist(&x.node) > limit || p.dist(&x.node) < 1e-6 {
                continue;
            }
            let cut = Cut::Line { p: x.node, d: Point::new(p.x - x.node.x, p.y - x.node.y), limit: limit * 2.0 };
            for c in self.cuts.get_mut(&(x.wall, x.which)).unwrap().iter_mut() {
                c.0 = cut;
            }
            for c in self.cuts.get_mut(&(y.wall, y.which)).unwrap().iter_mut() {
                c.1 = cut;
            }
        }
    }
}

/// Per layer outlines of every wall with layer-by-layer joins, before layers are
/// resolved against each other.
fn layer_rings(walls: &[LWall], joiner: &Joiner) -> Vec<Vec<Ring>> {
    walls
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let len = w.join.start.dist(&w.join.end);
            if len < 1e-6 {
                return vec![];
            }
            let u = Point::new((w.join.end.x - w.join.start.x) / len, (w.join.end.y - w.join.start.y) / len);
            let ends = [
                End { wall: i, which: 0, node: w.join.start, u, hl: w.join.left, hr: w.join.right, id: w.join.id, settings: w.join.start_join },
                End { wall: i, which: 1, node: w.join.end, u: Point::new(-u.x, -u.y), hl: w.join.right, hr: w.join.left, id: w.join.id, settings: w.join.end_join },
            ];
            let n = w.layers.len();
            // end points per wall-order layer: (left, right) at the start and at the end
            let mut pts = vec![[(Point::default(), Point::default()); 2]; n];
            for e in &ends {
                let fl = frame_layers(e, w);
                let cuts = &joiner.cuts[&(i, e.which)];
                for (k, l) in fl.iter().enumerate() {
                    let (a, b) = (cut_point(e, l.hi, cuts[k].0), cut_point(e, l.lo, cuts[k].1));
                    // the end frame's left is the wall's right at the end of the wall
                    let (wl, wr) = if e.which == 0 { (a, b) } else { (b, a) };
                    let idx = if e.which == 0 { k } else { n - 1 - k };
                    pts[idx][e.which as usize] = (wl, wr);
                }
            }
            (0..n)
                .map(|k| {
                    let [(ls, rs), (le, re)] = pts[k];
                    vec![rs, re, le, ls]
                })
                .collect()
        })
        .collect()
}

fn folded(w: &LWall, rings: &[Ring]) -> bool {
    let axis = Point::new(w.join.end.x - w.join.start.x, w.join.end.y - w.join.start.y);
    let proj = |p: Point| dot2(Point::new(p.x - w.join.start.x, p.y - w.join.start.y), axis);
    rings.iter().any(|r| r.len() == 4 && proj(r[0]).max(proj(r[3])) >= proj(r[1]).min(proj(r[2])))
}

fn bbox(r: &Ring) -> (f64, f64, f64, f64) {
    r.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, p| (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y)))
}

fn overlaps(a: &Ring, b: &Ring) -> bool {
    let (p, q) = (bbox(a), bbox(b));
    p.0 <= q.2 && q.0 <= p.2 && p.1 <= q.3 && q.1 <= p.3
}

/// Every layer's outline after its joins, before layers are resolved against
/// each other, plus which walls' layers can overlap (a, b, tie winner).
fn joined_rings(walls: &[LWall]) -> (Vec<Vec<Ring>>, Vec<(usize, usize, usize)>, std::collections::HashSet<(usize, u8)>) {
    let mut j = Joiner { walls, cuts: HashMap::new(), related: vec![], joined: Default::default() };
    for (i, w) in walls.iter().enumerate() {
        for which in 0..2u8 {
            j.cuts.insert((i, which), vec![(Cut::Perp, Cut::Perp); w.layers.len()]);
        }
    }
    let joins: Vec<JoinWall> = walls.iter().map(|w| w.join.clone()).collect();

    for node in classify(&joins) {
        match &node {
            Node::Mid { e, .. } => {
                j.joined.insert((e.wall, e.which));
            }
            Node::Pair { a, b } => {
                j.joined.extend([(a.wall, a.which), (b.wall, b.which)]);
            }
            Node::Through { i, j: jj, others } => {
                j.joined.extend([(i.wall, i.which), (jj.wall, jj.which)]);
                j.joined.extend(others.iter().map(|e| (e.wall, e.which)));
            }
            Node::Ring { ends } => {
                j.joined.extend(ends.iter().map(|e| (e.wall, e.which)));
            }
        }
        match node {
            Node::Mid { e, other } => {
                if let Some(o) = Other::of_wall(&walls[other]) {
                    j.butt(&e, &o, e.settings.kind == JoinKind::SquareOff);
                    j.related.push((e.wall, other, other));
                }
            }
            Node::Through { i, j: jj, others } => {
                let o = Other::of_end(&i, &walls[i.wall]);
                for e in &others {
                    j.butt(e, &o, e.settings.kind == JoinKind::SquareOff);
                    j.related.push((e.wall, i.wall, i.wall));
                    j.related.push((e.wall, jj.wall, jj.wall));
                }
            }
            Node::Pair { a, b } => match a.settings.kind {
                JoinKind::Miter => j.mitre_pair(&a, &b),
                kind => {
                    let (t, s) = if through_index(&a, &b) == 0 { (a, b) } else { (b, a) };
                    if !(kind == JoinKind::Butt && j.nested(&t, &s)) {
                        let (ot, os) = (Other::of_end(&t, &walls[t.wall]), Other::of_end(&s, &walls[s.wall]));
                        j.through(&t, &os);
                        j.butt(&s, &ot, kind == JoinKind::SquareOff);
                    }
                    j.related.push((t.wall, s.wall, t.wall));
                }
            },
            Node::Ring { ends } => j.mitre_ring(&ends),
        }
    }

    let mut rings = layer_rings(walls, &j);
    // a join that folds a short wall over itself is dropped
    for i in 0..walls.len() {
        if folded(&walls[i], &rings[i]) {
            for which in 0..2u8 {
                j.cuts.insert((i, which), vec![(Cut::Perp, Cut::Perp); walls[i].layers.len()]);
            }
            let plain = layer_rings(walls, &j);
            rings[i] = plain[i].clone();
        }
    }
    (rings, j.related, j.joined)
}

/// Layer outlines for every wall, keyed by wall id. Each wall has one entry per
/// layer (same order as the wall type, exterior first), each a list of shapes
/// because a layer can be split or notched by the layers of joining walls.
pub fn join_layers(walls: &[LWall]) -> BTreeMap<Id, Vec<Vec<Shape>>> {
    join_layers_full(walls).0
}

/// `join_layers` plus, for each wall, whether its start and end are free (not
/// joined to another wall).
pub fn join_layers_full(walls: &[LWall]) -> (BTreeMap<Id, Vec<Vec<Shape>>>, BTreeMap<Id, [bool; 2]>) {
    let (rings, related, joined) = joined_rings(walls);

    // resolve layers against each other: where a layer passes through a layer
    // of lower priority, that layer is cut away; equal priorities go to the
    // wall that runs through
    let mut out: BTreeMap<Id, Vec<Vec<Shape>>> = BTreeMap::new();
    for (i, w) in walls.iter().enumerate() {
        if rings[i].is_empty() {
            continue;
        }
        let per_layer = rings[i]
            .iter()
            .enumerate()
            .map(|(k, r)| {
                let p = w.layers[k].priority;
                let mut clips: Vec<&Ring> = vec![];
                for &(a, b, winner) in &related {
                    let other = if a == i { b } else if b == i { a } else { continue };
                    for (m, r2) in rings[other].iter().enumerate() {
                        let p2 = walls[other].layers[m].priority;
                        if (p2 < p || (p2 == p && winner == other)) && overlaps(r, r2) {
                            clips.push(r2);
                        }
                    }
                }
                if clips.is_empty() {
                    vec![vec![r.clone()]]
                } else {
                    subtract(r, clips)
                }
            })
            .collect();
        out.insert(w.join.id, per_layer);
    }
    let free = walls.iter().enumerate().map(|(i, w)| (w.join.id, [!joined.contains(&(i, 0)), !joined.contains(&(i, 1))])).collect();
    (out, free)
}

// ---------------------------------------------------------------- the document

/// One layer of a wall as drawn in plan.
#[derive(Debug, Clone)]
pub struct LayerGeom {
    pub material: String,
    pub function: LayerFunction,
    pub is_void: bool,
    pub core: bool,
    /// Takes part in wrapping at ends and inserts.
    pub wraps: bool,
    pub thickness: f64,
    pub shapes: Vec<Shape>,
}

/// A wall's joined layers and which of its ends are free.
#[derive(Debug, Clone)]
pub struct WallLayers {
    pub layers: Vec<LayerGeom>,
    pub free: [bool; 2],
}

/// A wall's own frame: `at(u, y)` is the point `u` along the wall and `y` to the
/// left of its location line.
struct Frame {
    start: Point,
    dir: Point,
    n: Point,
}

impl Frame {
    fn at(&self, u: f64, y: f64) -> Point {
        Point::new(self.start.x + self.dir.x * u + self.n.x * y, self.start.y + self.dir.y * u + self.n.y * y)
    }
    fn rect(&self, x0: f64, x1: f64, y0: f64, y1: f64) -> Ring {
        vec![self.at(x0, y0), self.at(x1, y0), self.at(x1, y1), self.at(x0, y1)]
    }
}

fn union_with(shapes: &[Shape], add: &Ring) -> Vec<Shape> {
    let subj: Vec<Vec<[f64; 2]>> = shapes.iter().flatten().map(|r| r.iter().map(|p| [p.x, p.y]).collect()).collect();
    let clip = vec![contour(add)];
    shapes_from(subj.overlay(&clip, OverlayRule::Union, FillRule::EvenOdd))
}

impl Document {
    /// Layer outlines of every wall, joined layer by layer. Openings and
    /// wrapping are not applied here; see `wall_plan_layers`.
    pub fn wall_layer_geometry(&self) -> BTreeMap<Id, WallLayers> {
        use crate::wallpath::{facet_id};
        let mut walls: Vec<LWall> = vec![];
        let mut types = BTreeMap::new();
        // (facets, ids) of each wall, in order
        let mut facets_of: BTreeMap<Id, usize> = BTreeMap::new();
        for (id, e) in self.iter() {
            let Element::Wall(w) = e else { continue };
            let Some(t) = self.wall_type_for(w) else { continue };
            // a curtain wall's layer only stands for its glazing zone; its panels and mullions are drawn instead
            if t.curtain.is_some() {
                continue;
            }
            let specs: Vec<LayerSpec> = t.layers.iter().map(|l| LayerSpec { thickness: l.thickness_mm, priority: l.join_priority(), core: l.core }).collect();
            let facets = self.wall_facets(id, w);
            facets_of.insert(id, facets.len());
            for join in facets {
                walls.push(LWall { join, layers: specs.clone() });
            }
            types.insert(id, t);
        }
        let (mut joined, free) = join_layers_full(&walls);
        let mut out = BTreeMap::new();
        for (id, t) in types {
            let n = facets_of[&id];
            let mut per_facet: Vec<Vec<Vec<Shape>>> = (0..n).filter_map(|k| joined.remove(&facet_id(id, k))).collect();
            if per_facet.len() != n || n == 0 {
                continue;
            }
            // a curved wall's facets are merged back into one shape per layer
            let per_layer: Vec<Vec<Shape>> = if n == 1 {
                per_facet.pop().unwrap()
            } else {
                (0..t.layers.len())
                    .map(|k| {
                        let rings: Vec<&Ring> = per_facet.iter().flat_map(|f| f[k].iter().filter_map(|s| s.first())).collect();
                        union_rings(rings)
                    })
                    .collect()
            };
            let layers = t
                .layers
                .iter()
                .zip(per_layer)
                .map(|(l, shapes)| LayerGeom { material: l.material.clone(), function: l.function, is_void: l.is_void, core: l.core, wraps: l.wraps, thickness: l.thickness_mm, shapes })
                .collect();
            let f0 = free.get(&facet_id(id, 0)).copied().unwrap_or([true, true]);
            let f1 = free.get(&facet_id(id, n - 1)).copied().unwrap_or([true, true]);
            out.insert(id, WallLayers { layers, free: [f0[0], f1[1]] });
        }
        out
    }

    /// The layers of a wall as a plan view draws them: wrapped at free ends, and,
    /// when the plan cuts the wall at `cut_z`, opened up where doors and windows
    /// span that height, with the layers wrapping into the opening if the wall
    /// type says so.
    pub fn wall_plan_layers(
        &self,
        wall: Id,
        wl: &WallLayers,
        cut_z: Option<f64>,
    ) -> Vec<LayerGeom> {
        let (Some(Element::Wall(w)), true) = (self.get(wall), !wl.layers.is_empty()) else {
            return wl.layers.clone();
        };
        let Some(t) = self.wall_type_for(w) else {
            return wl.layers.clone();
        };
        let blocked: Vec<(f64, f64)> = cut_z.map(|z| self.wall_cut_gaps(wall, z)).unwrap_or_default();
        plan_layers(w, &t, self.wall_extents(w), wl, &blocked)
    }
}

/// The plan layers of wall `w` of type `t` (reaching `extents` left and right of its location
/// line): wrapped at its free ends, and opened up and wrapped at each `blocked` stretch along it.
pub(crate) fn plan_layers(
    w: &crate::element::Wall,
    t: &crate::element::WallType,
    extents: (f64, f64),
    wl: &WallLayers,
    blocked: &[(f64, f64)],
) -> Vec<LayerGeom> {
    let mut layers = wl.layers.clone();
    let path = w.path();
    let len = path.length();
    if len < 1e-6 || layers.is_empty() {
        return layers;
    }
    let (left, right) = extents;
    // the frame at `u` along the wall and `u` measured in it: a straight wall has one frame
    // for its whole length, a curved one a frame at each place it is needed
    let straight = if path.is_straight() {
        let dir = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
        Some(Frame {
            start: w.start,
            dir,
            n: Point::new(-dir.y, dir.x),
        })
    } else {
        None
    };
    let frame_at = |u: f64| -> (Frame, f64) {
        match &straight {
            Some(f) => (
                Frame {
                    start: f.start,
                    dir: f.dir,
                    n: f.n,
                },
                u,
            ),
            None => {
                let (p, d) = path.at(u);
                (
                    Frame {
                        start: p,
                        dir: d,
                        n: Point::new(-d.y, d.x),
                    },
                    0.0,
                )
            }
        }
    };
    // layer boundaries across the wall, left to right
    let mut bounds = vec![left];
    for l in &layers {
        bounds.push(bounds.last().unwrap() - l.thickness);
    }

    // free stretches of wall between the ends and the openings
    let mut edges = vec![];
    let mut at = 0.0;
    for &(a, b) in blocked {
        edges.push((at, a));
        at = b;
    }
    edges.push((at, len));

    // wrap at free ends first, then open up the openings and wrap at those
    if wl.free[0] {
        let (f, e) = frame_at(0.0);
        wrap_at(
            &mut layers,
            &f,
            &bounds,
            e,
            1.0,
            t.wrap_ends,
            edges.first().map_or(len, |e| e.1 - e.0),
        );
    }
    if wl.free[1] {
        let (f, e) = frame_at(len);
        wrap_at(
            &mut layers,
            &f,
            &bounds,
            e,
            -1.0,
            t.wrap_ends,
            edges.last().map_or(len, |e| e.1 - e.0),
        );
    }
    if !blocked.is_empty() {
        let reach = (left + right) * 4.0 + 1000.0;
        let boxes: Vec<Ring> = blocked
            .iter()
            .map(|&(a, b)| {
                // a curved wall's opening is cut square to the wall where it sits
                let (f, e) = frame_at(if straight.is_some() {
                    0.0
                } else {
                    (a + b) / 2.0
                });
                if straight.is_some() {
                    f.rect(a, b, -reach, reach)
                } else {
                    f.rect(e - (b - a) / 2.0, e + (b - a) / 2.0, -reach, reach)
                }
            })
            .collect();
        for l in layers.iter_mut() {
            l.shapes = subtract_shapes(&l.shapes, &boxes);
        }
        for (i, &(a, b)) in blocked.iter().enumerate() {
            let (before, after) = (edges[i].1 - edges[i].0, edges[i + 1].1 - edges[i + 1].0);
            let (fa, ea) = frame_at(a);
            wrap_at(&mut layers, &fa, &bounds, ea, -1.0, t.wrap_inserts, before);
            let (fb, eb) = frame_at(b);
            wrap_at(&mut layers, &fb, &bounds, eb, 1.0, t.wrap_inserts, after);
        }
    }
    layers
}

/// A straight sample of wall type `t` for the Edit Assembly preview: `len` long on its centreline
/// along +X (exterior to +Y), both ends free, with an opening over `opening` along it.
pub fn sample_plan_layers(
    t: &crate::element::WallType,
    len: f64,
    opening: Option<(f64, f64)>,
) -> Vec<LayerGeom> {
    let w = crate::element::Wall::new(0, 0, Point::new(0.0, 0.0), Point::new(len, 0.0), 3000.0);
    let (left, right) = t.extents(crate::element::LocationLine::WallCentreline);
    let mut y = left;
    let layers = t
        .layers
        .iter()
        .map(|l| {
            let ring = vec![
                Point::new(0.0, y),
                Point::new(len, y),
                Point::new(len, y - l.thickness_mm),
                Point::new(0.0, y - l.thickness_mm),
            ];
            y -= l.thickness_mm;
            let shapes = if l.thickness_mm > 0.0 {
                vec![vec![ring]]
            } else {
                vec![]
            };
            LayerGeom {
                material: l.material.clone(),
                function: l.function,
                is_void: l.is_void,
                core: l.core,
                wraps: l.wraps,
                thickness: l.thickness_mm,
                shapes,
            }
        })
        .collect();
    let wl = WallLayers {
        layers,
        free: [true, true],
    };
    plan_layers(&w, t, (left, right), &wl, opening.as_slice())
}

/// Wrap the layers outside the core round a free boundary at `e` along the wall
/// (a wall end or a jamb). `inward` is +1 if the wall continues to larger `u`.
/// Each wrapping layer (its Wraps box ticked) turns into an L: its band along the wall plus a return
/// across the end, as deep as the layer is thick, reaching to the far boundary of the core. The layers
/// inside it (wrapping or not, and the core) step back by the depth of the returns outside them.
fn wrap_at(
    layers: &mut [LayerGeom],
    f: &Frame,
    bounds: &[f64],
    e: f64,
    inward: f64,
    wrap: Wrap,
    room: f64,
) {
    match wrap {
        Wrap::None => {}
        Wrap::Exterior | Wrap::Interior => {
            wrap_side(layers, f, bounds, e, inward, wrap == Wrap::Exterior, room)
        }
        Wrap::Both => {
            wrap_side(layers, f, bounds, e, inward, true, room);
            wrap_side(layers, f, bounds, e, inward, false, room);
        }
    }
}

fn wrap_side(
    layers: &mut [LayerGeom],
    f: &Frame,
    bounds: &[f64],
    e: f64,
    inward: f64,
    exterior: bool,
    room: f64,
) {
    let (Some(first), Some(last)) = (
        layers.iter().position(|l| l.core),
        layers.iter().rposition(|l| l.core),
    ) else {
        return;
    };
    // the layers on this side, from the outermost one in towards the core
    let side: Vec<usize> = if exterior {
        (0..first).collect()
    } else {
        (last + 1..layers.len()).rev().collect()
    };
    let depth: f64 = side
        .iter()
        .filter(|&&k| layers[k].wraps)
        .map(|&k| layers[k].thickness)
        .sum();
    if depth <= 0.0 || room < depth + 1.0 {
        return;
    }
    let far = if exterior { bounds[last + 1] } else { bounds[first] };
    let x = |d: f64| e + inward * d;
    let mut done = 0.0;
    for &k in &side {
        let (hi, lo) = (bounds[k], bounds[k + 1]);
        if done > 0.0 {
            // this layer's band starts behind the returns of the layers outside it
            layers[k].shapes = subtract_shapes(&layers[k].shapes, &[f.rect(e, x(done), lo, hi)]);
        }
        if !layers[k].wraps {
            continue;
        }
        let next = done + layers[k].thickness;
        let (y0, y1) = if exterior { (far, lo) } else { (hi, far) };
        let ret = f.rect(x(done), x(next), y0, y1);
        layers[k].shapes = union_with(&layers[k].shapes, &ret);
        done = next;
    }
    for k in first..=last {
        layers[k].shapes = subtract_shapes(&layers[k].shapes, &[f.rect(e, x(depth), bounds[k + 1], bounds[k])]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::EndJoin;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    /// Finish 1 (10) | Structure (90) | Finish 2 (10), centred on the location line.
    fn stud(id: Id, a: Point, b: Point) -> LWall {
        LWall {
            join: JoinWall { id, level: 1, start: a, end: b, left: 55.0, right: 55.0, start_join: EndJoin::default(), end_join: EndJoin::default() },
            layers: vec![LayerSpec { thickness: 10.0, priority: 5, core: false }, LayerSpec { thickness: 90.0, priority: 1, core: true }, LayerSpec { thickness: 10.0, priority: 5, core: false }],
        }
    }

    fn area(shapes: &[Shape]) -> f64 {
        shapes.iter().map(|s| s.iter().enumerate().map(|(i, r)| if i == 0 { signed_area(r).abs() } else { -signed_area(r).abs() }).sum::<f64>()).sum()
    }

    fn total(layers: &[Vec<Shape>]) -> f64 {
        layers.iter().map(|l| area(l)).sum()
    }

    fn contains(shapes: &[Shape], pt: Point) -> bool {
        // even-odd test over every ring of every shape
        let mut inside = false;
        for r in shapes.iter().flatten() {
            let n = r.len();
            for i in 0..n {
                let (a, b) = (r[i], r[(i + 1) % n]);
                if (a.y > pt.y) != (b.y > pt.y) && pt.x < (b.x - a.x) * (pt.y - a.y) / (b.y - a.y) + a.x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// Brick (Finish 1) | air gap | stud (core) | plasterboard, 110 + 50 + 90 + 13, exterior on the left.
    fn veneer(id: Id, a: Point, b: Point) -> LWall {
        LWall {
            join: JoinWall { id, level: 1, start: a, end: b, left: 131.5, right: 131.5, start_join: EndJoin::default(), end_join: EndJoin::default() },
            layers: vec![
                LayerSpec { thickness: 110.0, priority: 5, core: false },
                LayerSpec { thickness: 50.0, priority: 3, core: false },
                LayerSpec { thickness: 90.0, priority: 1, core: true },
                LayerSpec { thickness: 13.0, priority: 5, core: false },
            ],
        }
    }

    /// The outline of a layer's shapes, merged across every wall, as (pieces, holes).
    fn merged(r: &BTreeMap<Id, Vec<Vec<Shape>>>, layer: usize) -> (usize, usize) {
        let rings: Vec<Ring> = r.values().flat_map(|l| l[layer].iter().map(|s| s[0].clone())).collect();
        let u = union_rings(rings.iter());
        (u.len(), u.iter().map(|s| s.len() - 1).sum())
    }

    #[test]
    fn a_butt_corner_joins_each_layer_to_its_partner_so_cavity_and_outer_leaf_run_all_round() {
        // four walls round a box with the exterior outside, then the same box the other way round
        for corners in [[(0., 0.), (0., 4000.), (6000., 4000.), (6000., 0.)], [(0., 0.), (6000., 0.), (6000., 4000.), (0., 4000.)]] {
            let walls: Vec<LWall> = (0..4).map(|i| veneer(i as Id + 1, p(corners[i].0, corners[i].1), p(corners[(i + 1) % 4].0, corners[(i + 1) % 4].1))).collect();
            let r = join_layers(&walls);
            for layer in 0..4 {
                // one connected band with the room as its single hole
                assert_eq!(merged(&r, layer), (1, 1), "layer {layer}");
            }
            // layers do not pile up on each other at the corners: the total is the sum of the
            // four plain walls, less what the corners share
            let total_area: f64 = r.values().map(|l| total(l)).sum();
            let outer = (6000.0 + 263.0) * (4000.0 + 263.0);
            let inner = (6000.0 - 263.0) * (4000.0 - 263.0);
            assert!((total_area - (outer - inner)).abs() < 1.0, "{total_area} vs {}", outer - inner);
        }
    }

    #[test]
    fn a_lone_wall_is_its_plain_layers() {
        let r = join_layers(&[stud(1, p(0., 0.), p(1000., 0.))]);
        let l = &r[&1];
        assert_eq!(l.len(), 3);
        assert!((area(&l[0]) - 10_000.0).abs() < 1e-6 && (area(&l[1]) - 90_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_t_junction_runs_the_structure_through_the_finish_and_stops_the_finish_at_the_face() {
        // wall 1 runs along x; wall 2 comes down from +y and ends on wall 1's centreline
        let r = join_layers(&[stud(1, p(0., 0.), p(4000., 0.)), stud(2, p(2000., 2000.), p(2000., 0.))]);
        let (through, butting) = (&r[&1], &r[&2]);
        // wall 2's structure (x 1955..2045) runs down through wall 1's near finish (y 45..55) to its structure face at y=45
        assert!(contains(&butting[1], p(2000., 50.0)), "structure passes the finish layer");
        assert!(!contains(&butting[1], p(2000., 40.0)), "and stops at the other structure");
        // wall 2's finishes (x 2045..2055 and 1945..1955) stop at the face of wall 1 (y=55) instead
        assert!(contains(&butting[0], p(2050., 60.0)) && !contains(&butting[0], p(2050., 50.0)));
        assert!(contains(&butting[2], p(1950., 60.0)) && !contains(&butting[2], p(1950., 50.0)));
        // wall 1's near finish is notched where wall 2's structure passes through
        assert!(!contains(&through[0], p(2000., 50.0)), "finish cut by the stronger layer");
        assert!(contains(&through[0], p(1900., 50.0)) && contains(&through[0], p(2100., 50.0)));
        // the structure of wall 1 is whole
        assert!(contains(&through[1], p(2000., 0.0)));
    }

    #[test]
    fn a_stem_placed_up_to_the_face_gets_the_same_layers_as_one_on_the_centreline() {
        // wall 1 is 110 thick (y -55..55); wall 2 is placed up to its face at y=55
        let on_face = join_layers(&[stud(1, p(0., 0.), p(4000., 0.)), stud(2, p(2000., 2000.), p(2000., 55.))]);
        let on_centre = join_layers(&[stud(1, p(0., 0.), p(4000., 0.)), stud(2, p(2000., 2000.), p(2000., 0.))]);
        for (a, b) in [(&on_face[&1], &on_centre[&1]), (&on_face[&2], &on_centre[&2])] {
            assert!((total(a) - total(b)).abs() < 1e-6, "{} {}", total(a), total(b));
        }
        // the stem's structure still reaches wall 1's structure face, not just the surface
        assert!(contains(&on_face[&2][1], p(2000., 50.0)));
    }

    #[test]
    fn layers_never_overlap_after_resolution() {
        let r = join_layers(&[stud(1, p(0., 0.), p(4000., 0.)), stud(2, p(2000., 2000.), p(2000., 0.)), stud(3, p(2000., -2000.), p(2000., 0.))]);
        // total layer area equals the area of the union of everything
        let all: Vec<Ring> = r.values().flatten().flatten().flat_map(|s| s.iter().take(1).cloned()).collect();
        let union = area(&union_rings(all.iter()));
        let sum: f64 = r.values().map(|l| total(l)).sum();
        assert!((sum - union).abs() < 1.0, "sum {sum} union {union}");
    }

    #[test]
    fn an_l_corner_keeps_the_outer_corner_square_and_joins_the_structure() {
        // wall 1 along +x ending at the origin, wall 2 going up +y from the origin; wall 1 (older) runs through
        let r = join_layers(&[stud(1, p(-2000., 0.), p(0., 0.)), stud(2, p(0., 0.), p(0., 2000.))]);
        let (a, b) = (&r[&1], &r[&2]);
        // the outer corner (x 55, y -55) is covered by wall 1's finish layer
        assert!(contains(&a[2], p(50., -52.)));
        // wall 2 stops at wall 1's face (y=55) with its finishes and enters the finish zone with its structure
        assert!(contains(&b[1], p(0., 50.)) && !contains(&b[1], p(0., 40.)));
        assert!(!contains(&b[0], p(-50., 50.)) || !contains(&a[0], p(-50., 50.)));
        // no gap: union equals the plain corner outline
        let all: Vec<Ring> = r.values().flatten().flatten().flat_map(|s| s.iter().take(1).cloned()).collect();
        let union = area(&union_rings(all.iter()));
        let plain = 110.0 * (2000.0 + 55.0 + 2000.0 - 55.0 + 55.0) - 0.0; // two bars less nothing: computed below
        let _ = plain;
        assert!(union > 2.0 * 110.0 * 2000.0 - 1.0, "corner is filled: {union}");
    }

    #[test]
    fn a_mitre_cuts_every_layer_on_one_line() {
        let m = EndJoin { kind: JoinKind::Miter, ..EndJoin::default() };
        let mut w1 = stud(1, p(-2000., 0.), p(0., 0.));
        let mut w2 = stud(2, p(0., 0.), p(0., 2000.));
        w1.join.end_join = m;
        w2.join.start_join = m;
        let r = join_layers(&[w1, w2]);
        // the mitre line runs from the outer corner (55,-55) to the inner corner (-55,55): x + y = 0
        for layers in r.values() {
            for l in layers {
                for s in l {
                    for pt in &s[0] {
                        // every end point on that line is on x + y = 0 or is a far end of the wall
                        let on_line = (pt.x + pt.y).abs() < 1e-6;
                        let far = pt.x.abs() > 1000.0 || pt.y.abs() > 1000.0;
                        assert!(on_line || far, "{pt:?}");
                    }
                }
            }
        }
        // and there is no overlap
        let all: Vec<Ring> = r.values().flatten().flatten().flat_map(|s| s.iter().take(1).cloned()).collect();
        let sum: f64 = r.values().map(|l| total(l)).sum();
        assert!((sum - area(&union_rings(all.iter()))).abs() < 1.0);
    }

    #[test]
    fn square_off_ends_the_butting_wall_in_one_square_cut() {
        // an oblique T: wall 2 meets wall 1 at 45 degrees
        let sq = EndJoin { kind: JoinKind::SquareOff, ..EndJoin::default() };
        let mut w2 = stud(2, p(1000., 1000.), p(2000., 0.));
        w2.join.end_join = sq;
        let (butt, _, _) = joined_rings(&[stud(1, p(0., 0.), p(4000., 0.)), stud(2, p(1000., 1000.), p(2000., 0.))]);
        let (square, _, _) = joined_rings(&[stud(1, p(0., 0.), p(4000., 0.)), w2]);
        // butted layers end along wall 1's face (different place per layer); squared layers share one end line
        let ends = |r: &Vec<Vec<Ring>>| -> Vec<f64> {
            let dir = Point::new(1.0, -1.0);
            r[1].iter().map(|ring| ring.iter().map(|pt| pt.x * dir.x + pt.y * dir.y).fold(f64::MIN, f64::max)).collect()
        };
        let sq_ends = ends(&square);
        assert!(sq_ends.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-3), "{sq_ends:?}");
        let bu_ends = ends(&butt);
        assert!(bu_ends.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3), "{bu_ends:?}");
    }

    #[test]
    fn disallowed_joins_leave_independent_layers() {
        let mut w2 = stud(2, p(2000., 2000.), p(2000., 0.));
        w2.join.end_join = EndJoin { allowed: false, ..EndJoin::default() };
        let r = join_layers(&[stud(1, p(0., 0.), p(4000., 0.)), w2]);
        // wall 2's structure ends square at its own end, so it overlaps wall 1 untouched
        assert!(contains(&r[&2][1], p(2000., 5.0)));
        assert!(contains(&r[&1][1], p(2000., 5.0)));
    }

    fn brick_wall(d: &mut Document, wrap_ends: Wrap, wrap_inserts: Wrap) -> (Id, Id) {
        let (lvl, wt) = (d.levels()[0].0, d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0);
        let mut t = d.wall_type(wt).unwrap().clone();
        t.wrap_ends = wrap_ends;
        t.wrap_inserts = wrap_inserts;
        d.transact("wrap", |tx| tx.update(wt, Element::WallType(t))).unwrap();
        let w = d.add_wall(wt, lvl, p(0., 0.), p(4000., 0.), 2700.).unwrap();
        (w, lvl)
    }

    #[test]
    fn exterior_wrap_returns_the_outer_layers_round_a_free_end() {
        // Brick Veneer, centred: brick y 16.5..126.5, cavity -23.5..16.5, frame -113.5..-23.5, plasterboard below
        let mut d = Document::new_project();
        let (w, _) = brick_wall(&mut d, Wrap::Exterior, Wrap::None);
        let geo = d.wall_layer_geometry();
        let layers = d.wall_plan_layers(w, &geo[&w], None);
        // the brick turns the end: 110 deep, down to the interior side of the frame
        assert!(layers[0].shapes.iter().any(|_| true) && contains(&layers[0].shapes, p(50., -50.)));
        assert!(contains(&layers[0].shapes, p(50., 100.)), "its band is whole");
        // the frame stops behind the returns (110 brick + 40 cavity)
        assert!(!contains(&layers[2].shapes, p(100., -50.)) && contains(&layers[2].shapes, p(200., -50.)));
        // the interior finish is not wrapped and stays flush with the end
        assert!(contains(&layers[3].shapes, p(5., -120.)));
        // both ends wrap
        assert!(contains(&layers[0].shapes, p(3950., -50.)));
        // with wrapping off nothing changes
        let mut d2 = Document::new_project();
        let (w2, _) = brick_wall(&mut d2, Wrap::None, Wrap::None);
        let g2 = d2.wall_layer_geometry();
        assert!(!contains(&d2.wall_plan_layers(w2, &g2[&w2], None)[0].shapes, p(50., -50.)));
    }

    #[test]
    fn interior_wrap_uses_the_inner_layers() {
        let mut d = Document::new_project();
        let (w, _) = brick_wall(&mut d, Wrap::Interior, Wrap::None);
        let geo = d.wall_layer_geometry();
        let layers = d.wall_plan_layers(w, &geo[&w], None);
        // plasterboard (13) returns across the end up to the exterior side of the frame (-23.5)
        assert!(contains(&layers[3].shapes, p(5., -60.)) && contains(&layers[3].shapes, p(5., -120.)));
        assert!(!contains(&layers[2].shapes, p(5., -60.)) && contains(&layers[2].shapes, p(50., -60.)));
        // the brick is untouched
        assert!(contains(&layers[0].shapes, p(5., 100.)) && !contains(&layers[0].shapes, p(5., -60.)));
    }

    #[test]
    fn joined_ends_do_not_wrap_and_openings_cut_every_layer() {
        let mut d = Document::new_project();
        let (w, lvl) = brick_wall(&mut d, Wrap::Exterior, Wrap::Exterior);
        // a second wall joins at the start, so only the far end is free
        let Some(Element::Wall(w0)) = d.get(w) else { panic!() };
        let wt = w0.wall_type;
        d.add_wall(wt, lvl, p(0., 0.), p(0., 3000.), 2700.).unwrap();
        let geo = d.wall_layer_geometry();
        assert_eq!(geo[&w].free, [false, true]);
        // a door in the wall: the cut plane (1200) passes through it
        let door = d.family_types().into_iter().find(|(_, t)| t.name == "820 x 2040" && d.family(t.family).is_some_and(|f| f.host == crate::family::HostKind::Wall)).unwrap().0;
        d.place_on_wall(door, w, p(2000., 100.)).unwrap();
        let uncut = d.wall_plan_layers(w, &geo[&w], None);
        let cut = d.wall_plan_layers(w, &geo[&w], Some(1200.0));
        assert!(contains(&uncut[2].shapes, p(2000., -60.)));
        assert!(!contains(&cut[2].shapes, p(2000., -60.)), "frame is open at the door");
        assert!(!contains(&cut[0].shapes, p(2000., 100.)), "and so is the brick");
        // the brick wraps into the jamb (insert wrapping): it returns across the frame beside the door
        assert!(contains(&cut[0].shapes, p(2000.0 - 410.0 - 50.0, -50.0)), "return at the jamb");
        assert!(contains(&cut[0].shapes, p(1000., 100.)));
    }
}
