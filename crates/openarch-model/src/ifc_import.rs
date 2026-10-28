//! IFC import: an IFC2x3 or IFC4 file read into a fresh project. Storeys become levels; straight walls, flat floors
//! (with their holes) and the doors and windows in the walls come in as OpenArch elements, each measured from its
//! geometry (an extruded profile, a B-rep or a triangle mesh alike), so how a file describes an element does not
//! matter. Wall and floor types are matched by name and thickness, or made; door and window types by size.
//! What has no OpenArch element yet, or cannot be read, is left out and listed.
//!
//! Ported from BIMCraft (https://github.com/IPedrax/BIMCraft, crates/core/src/import.rs at commit c269c8c),
//! Copyright (c) 2026 IPedrax and the BIMCraft contributors, MIT OR Apache-2.0: the geometry reader (placements,
//! profiles, curves, extrusions, B-reps, face sets, mapped items), the units, the relations between elements, the
//! outline of a slab's top, the far-from-origin shift and the joining of near-meeting walls. Rewritten against
//! OpenArch's model; see THIRD-PARTY-NOTICES.md.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::PI;

use crate::curve::Curve;
use crate::doc::Document;
use crate::element::*;
use crate::family::{FamilyCategory, FamilyType, ParamValue};
use crate::geom::Point;
use crate::step::{self, Entity, File, V};

type P3 = [f64; 3];

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: P3, k: f64) -> P3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { mul(a, 1.0 / l) } else { a }
}

fn real(v: Option<&V>) -> Option<f64> {
    let x = match v? {
        V::Real(x) => *x,
        V::Int(i) => *i as f64,
        V::Typed(_, inner) => return real(Some(inner)),
        _ => return None,
    };
    x.is_finite().then_some(x)
}

fn text(v: Option<&V>) -> Option<&str> {
    match v? {
        V::Str(s) => Some(s.trim()).filter(|s| !s.is_empty()),
        V::Typed(_, inner) => text(Some(inner)),
        _ => None,
    }
}

fn reference(v: Option<&V>) -> Option<u64> {
    match v? {
        V::Ref(r) => Some(*r),
        _ => None,
    }
}

fn list(v: Option<&V>) -> &[V] {
    match v {
        Some(V::List(l)) => l,
        _ => &[],
    }
}

fn refs(v: Option<&V>) -> Vec<u64> {
    list(v).iter().filter_map(|x| reference(Some(x))).collect()
}

fn enumeration(v: Option<&V>) -> Option<&str> {
    match v? {
        V::Enum(e) => Some(e),
        V::Typed(_, inner) => enumeration(Some(inner)),
        _ => None,
    }
}

/// 1-based indices, as face sets list them.
fn indices(v: Option<&V>) -> Vec<usize> {
    list(v).iter().filter_map(|x| match x {
        V::Int(i) if *i > 0 => Some(*i as usize),
        V::Real(r) if *r > 0.0 => Some(*r as usize),
        _ => None,
    })
    .collect()
}

/// The least and the most of `xs`.
fn span(xs: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    xs.fold(None, |s: Option<(f64, f64)>, x| Some(s.map_or((x, x), |(lo, hi)| (lo.min(x), hi.max(x)))))
}

/// Rounded to the hundredth of a millimetre.
fn mm(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[derive(Clone, Copy)]
struct Frame {
    o: P3,
    x: P3,
    y: P3,
    z: P3,
}

const WORLD: Frame = Frame { o: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0] };

impl Frame {
    fn at(&self, p: P3) -> P3 {
        add(self.o, self.dir(p))
    }
    fn dir(&self, d: P3) -> P3 {
        add(add(mul(self.x, d[0]), mul(self.y, d[1])), mul(self.z, d[2]))
    }
    /// `local`, given in this frame's terms, in its parent's.
    fn then(&self, local: Frame) -> Frame {
        Frame { o: self.at(local.o), x: self.dir(local.x), y: self.dir(local.y), z: self.dir(local.z) }
    }
}

/// Axes at `o`: along `z`, and `x` turned square to it.
fn orthonormal(o: P3, x: P3, z: P3) -> Frame {
    let z = if dot(z, z) > 1e-18 { unit(z) } else { [0.0, 0.0, 1.0] };
    let square = |v: P3| sub(v, mul(z, dot(v, z)));
    let x = if dot(square(x), square(x)) > 1e-12 { unit(square(x)) } else { unit(square(if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] })) };
    Frame { o, x, y: cross(z, x), z }
}

/// A polygon's normal by Newell's method, as long as twice its area.
fn newell(f: &[P3]) -> P3 {
    f.iter().zip(f.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| add(n, cross(*a, *b)))
}

fn normal(f: &[P3]) -> P3 {
    let n = newell(f);
    if dot(n, n) > 1e-12 { unit(n) } else { [0.0; 3] }
}

/// A ring and its copy `up` from it, as faces turned out.
fn prism(out: &mut Vec<Vec<P3>>, mut bottom: Vec<P3>, up: P3) {
    if dot(newell(&bottom), up) < 0.0 {
        bottom.reverse();
    }
    let top: Vec<P3> = bottom.iter().map(|p| add(*p, up)).collect();
    for ((a, b), (c, d)) in bottom.iter().zip(&top).zip(bottom.iter().zip(&top).cycle().skip(1)) {
        out.push(vec![*a, *c, *d, *b]);
    }
    out.push(bottom.into_iter().rev().collect());
    out.push(top);
}

/// Points less repeats in a row and the closing repeat of the first.
fn open(mut pts: Vec<Point>) -> Vec<Point> {
    pts.dedup_by(|a, b| a.dist(b) < 1e-6);
    if pts.len() > 1 && pts[0].dist(pts.last().unwrap()) < 1e-6 {
        pts.pop();
    }
    pts
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(&Point::new(a.x + dx * t, a.y + dy * t))
}

/// The loops the edges of `faces` no two share make, each face turned counter-clockwise in plan: the outline
/// counter-clockwise, its holes clockwise. An edge with another face's corner on it counts as two edges there.
fn loops<'a>(faces: impl Iterator<Item = &'a Vec<P3>>) -> Vec<Vec<Point>> {
    type K = (i64, i64);
    let key = |p: Point| -> K { ((p.y * 10.0).round() as i64, (p.x * 10.0).round() as i64) };
    let rings: Vec<Vec<Point>> = faces
        .map(|f| {
            let ring: Vec<Point> = f.iter().map(|p| Point::new(p[0], p[1])).collect();
            if crate::geom::signed_area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring }
        })
        .collect();
    let corners: Vec<Point> = rings.iter().flatten().copied().collect();
    let mut edges: BTreeMap<(K, K), Point> = BTreeMap::new();
    for ring in &rings {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            let len2 = (b.x - a.x).powi(2) + (b.y - a.y).powi(2);
            let mut on: Vec<(f64, Point)> = corners
                .iter()
                .filter(|p| seg_dist(**p, a, b) < 0.05 && p.dist(&a) > 0.05 && p.dist(&b) > 0.05)
                .map(|p| (((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / len2.max(1e-12), *p))
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            let pts: Vec<Point> = std::iter::once(a).chain(on.into_iter().map(|(_, p)| p)).chain(std::iter::once(b)).collect();
            for w in pts.windows(2) {
                let (ka, kb) = (key(w[0]), key(w[1]));
                // an edge two faces share runs both ways, inside the outline
                if ka != kb && edges.remove(&(kb, ka)).is_none() {
                    edges.insert((ka, kb), w[0]);
                }
            }
        }
    }
    let mut next: BTreeMap<K, (K, Point)> = edges.into_iter().map(|((ka, kb), a)| (ka, (kb, a))).collect();
    let mut out = vec![];
    while let Some(&start) = next.keys().next() {
        let (mut ring, mut k) = (vec![], start);
        while let Some((kb, a)) = next.remove(&k) {
            ring.push(a);
            k = kb;
        }
        if ring.len() >= 3 {
            out.push(ring);
        }
    }
    out
}

/// A ring less the corners that lie on a straight run.
fn corners(ring: &[Point]) -> Vec<Point> {
    let n = ring.len();
    if n < 4 {
        return ring.to_vec();
    }
    (0..n)
        .filter(|&i| {
            let (a, b, c) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            ((b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x)).abs() > 1e-3 * a.dist(&b).max(1.0) * b.dist(&c).max(1.0) / 1000.0
        })
        .map(|i| ring[i])
        .collect()
}

/// The outline of the faces facing up at height `z` (within half a millimetre): the largest loop counter-clockwise,
/// and the holes in it clockwise.
fn outline_at(faces: &[Vec<P3>], z: f64) -> Option<(Vec<Point>, Vec<Vec<Point>>)> {
    let picked = faces.iter().filter(|f| f.len() >= 3 && f.iter().all(|p| (p[2] - z).abs() <= 0.5) && normal(f)[2] > 0.2);
    let mut rings: Vec<Vec<Point>> = loops(picked).into_iter().map(|l| corners(&l)).filter(|l| l.len() >= 3).collect();
    rings.sort_by(|a, b| crate::geom::signed_area(b).abs().total_cmp(&crate::geom::signed_area(a).abs()));
    let mut rings = rings.into_iter();
    let mut outer = rings.next()?;
    if crate::geom::signed_area(&outer) < 0.0 {
        outer.reverse();
    }
    let holes = rings.filter(|h| crate::geom::signed_area(h) < 0.0 && h.iter().all(|p| crate::curve::point_in_polygon(*p, &outer))).collect();
    Some((outer, holes))
}

/// Reads the geometry of a parsed file in millimetres, with the far-from-origin shift taken off plan points.
struct Reader<'a> {
    f: &'a File,
    mm: f64,
    rad: f64,
    shift: Point,
}

impl<'a> Reader<'a> {
    fn get(&self, id: u64) -> Option<&'a Entity> {
        self.f.entities.get(&id)
    }
    fn at(&self, v: Option<&V>) -> Option<&'a Entity> {
        self.get(reference(v)?)
    }
    fn len(&self, v: Option<&V>) -> Option<f64> {
        real(v).map(|x| x * self.mm)
    }
    /// A model point in the plan.
    fn flat(&self, p: P3) -> Point {
        Point::new(p[0] - self.shift.x, p[1] - self.shift.y)
    }
    fn coords(&self, v: Option<&V>) -> Vec<f64> {
        list(v).iter().filter_map(|x| real(Some(x))).collect()
    }
    fn point(&self, id: Option<u64>) -> Option<P3> {
        match self.coords(self.get(id?)?.args.first()).as_slice() {
            [x, y] => Some([x * self.mm, y * self.mm, 0.0]),
            [x, y, z, ..] => Some([x * self.mm, y * self.mm, z * self.mm]),
            _ => None,
        }
    }
    fn direction(&self, id: Option<u64>) -> Option<P3> {
        let d = match self.coords(self.get(id?)?.args.first()).as_slice() {
            [x, y] => [*x, *y, 0.0],
            [x, y, z, ..] => [*x, *y, *z],
            _ => return None,
        };
        (dot(d, d) > 1e-18).then(|| unit(d))
    }
    /// An axis placement, 3D or 2D.
    fn axes(&self, id: Option<u64>) -> Frame {
        let Some(e) = id.and_then(|i| self.get(i)) else { return WORLD };
        let a = |i: usize| reference(e.args.get(i));
        let o = self.point(a(0)).unwrap_or_default();
        match e.name.as_str() {
            "IFCAXIS2PLACEMENT3D" => orthonormal(o, self.direction(a(2)).unwrap_or([1.0, 0.0, 0.0]), self.direction(a(1)).unwrap_or([0.0, 0.0, 1.0])),
            "IFCAXIS2PLACEMENT2D" => orthonormal(o, self.direction(a(1)).unwrap_or([1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]),
            _ => WORLD,
        }
    }
    /// Where a placement puts what it places: its chain of local placements.
    fn placement(&self, id: Option<u64>, depth: usize) -> Frame {
        match id.and_then(|i| self.get(i)) {
            Some(e) if e.name == "IFCLOCALPLACEMENT" && depth < 64 => self.placement(reference(e.args.first()), depth + 1).then(self.axes(reference(e.args.get(1)))),
            _ => WORLD,
        }
    }
    /// A Cartesian transformation operator: axes, scaled.
    fn operator(&self, id: Option<u64>) -> Frame {
        let Some(e) = id.and_then(|i| self.get(i)) else { return WORLD };
        let a = |i: usize| e.args.get(i);
        let s = real(a(3)).unwrap_or(1.0);
        let (z, k) = match e.name.as_str() {
            "IFCCARTESIANTRANSFORMATIONOPERATOR3D" => (self.direction(reference(a(4))), [s, s, s]),
            "IFCCARTESIANTRANSFORMATIONOPERATOR3DNONUNIFORM" => (self.direction(reference(a(4))), [s, real(a(5)).unwrap_or(s), real(a(6)).unwrap_or(s)]),
            "IFCCARTESIANTRANSFORMATIONOPERATOR2D" => (None, [s, s, s]),
            "IFCCARTESIANTRANSFORMATIONOPERATOR2DNONUNIFORM" => (None, [s, real(a(4)).unwrap_or(s), s]),
            _ => return WORLD,
        };
        let o = self.point(reference(a(2))).unwrap_or_default();
        let f = orthonormal(o, self.direction(reference(a(0))).unwrap_or([1.0, 0.0, 0.0]), z.unwrap_or([0.0, 0.0, 1.0]));
        Frame { o: f.o, x: mul(f.x, k[0]), y: mul(f.y, k[1]), z: mul(f.z, k[2]) }
    }
    fn point_list(&self, id: Option<u64>) -> Vec<P3> {
        let Some(e) = id.and_then(|i| self.get(i)) else { return vec![] };
        list(e.args.first())
            .iter()
            .map(|p| {
                let c = self.coords(Some(p));
                let at = |i: usize| c.get(i).copied().unwrap_or(0.0) * self.mm;
                [at(0), at(1), at(2)]
            })
            .collect()
    }
    /// A curve's points in order, in its own coordinates; an arc as short chords.
    fn curve(&self, id: Option<u64>, depth: usize) -> Option<Vec<P3>> {
        let e = self.get(id?)?;
        if depth > 16 {
            return None;
        }
        let a = |i: usize| e.args.get(i);
        match e.name.as_str() {
            "IFCPOLYLINE" => Some(refs(a(0)).into_iter().filter_map(|p| self.point(Some(p))).collect()),
            "IFCINDEXEDPOLYCURVE" => {
                let pts = self.point_list(reference(a(0)));
                let segments = list(a(1));
                if segments.is_empty() {
                    return Some(pts);
                }
                let mut out: Vec<P3> = vec![];
                for i in segments.iter().flat_map(|s| indices(Some(s))) {
                    if let Some(p) = i.checked_sub(1).and_then(|k| pts.get(k)) {
                        if out.last() != Some(p) {
                            out.push(*p);
                        }
                    }
                }
                Some(out)
            }
            "IFCCOMPOSITECURVE" | "IFCCOMPOSITECURVEONSURFACE" => {
                let mut out: Vec<P3> = vec![];
                for seg in refs(a(0)).into_iter().filter_map(|s| self.get(s)) {
                    let Some(mut pts) = self.curve(reference(seg.args.get(2)), depth + 1) else { continue };
                    if enumeration(seg.args.get(1)) == Some("F") {
                        pts.reverse();
                    }
                    for p in pts {
                        if out.last().map_or(true, |q| dot(sub(*q, p), sub(*q, p)) > 1e-12) {
                            out.push(p);
                        }
                    }
                }
                Some(out)
            }
            "IFCTRIMMEDCURVE" => self.trimmed(e),
            "IFCCIRCLE" => {
                let (f, r) = (self.axes(reference(a(0))), self.len(a(1))?);
                Some((0..24).map(|i| f64::from(i) * PI / 12.0).map(|t| f.at([r * t.cos(), r * t.sin(), 0.0])).collect())
            }
            _ => None,
        }
    }
    /// A trimmed line, or a trimmed circle as an arc.
    fn trimmed(&self, e: &Entity) -> Option<Vec<P3>> {
        let basis = self.at(e.args.first())?;
        let trim = |v: Option<&V>| {
            let mut out: (Option<P3>, Option<f64>) = (None, None);
            for t in list(v) {
                match t {
                    V::Ref(r) => out.0 = out.0.or(self.point(Some(*r))),
                    V::Typed(_, p) => out.1 = out.1.or(real(Some(p))),
                    _ => {}
                }
            }
            out
        };
        let (t1, t2) = (trim(e.args.get(1)), trim(e.args.get(2)));
        match basis.name.as_str() {
            "IFCLINE" => {
                let p0 = self.point(reference(basis.args.first()))?;
                let v = self.at(basis.args.get(1));
                let d = v.and_then(|v| self.direction(reference(v.args.first()))).unwrap_or([1.0, 0.0, 0.0]);
                let k = v.and_then(|v| self.len(v.args.get(1))).unwrap_or(1.0);
                let at = |(p, t): (Option<P3>, Option<f64>)| p.or_else(|| t.map(|t| add(p0, mul(d, k * t))));
                Some(vec![at(t1)?, at(t2)?])
            }
            "IFCCIRCLE" => {
                let (f, r) = (self.axes(reference(basis.args.first())), self.len(basis.args.get(1))?);
                let angle = |(p, t): (Option<P3>, Option<f64>)| p.map(|p| dot(sub(p, f.o), f.y).atan2(dot(sub(p, f.o), f.x))).or_else(|| t.map(|t| t * self.rad));
                let (a1, a2) = (angle(t1)?, angle(t2)?);
                let ccw = (a2 - a1).rem_euclid(2.0 * PI);
                let sweep = if enumeration(e.args.get(3)) == Some("F") { ccw - 2.0 * PI } else { ccw };
                Some((0..=12).map(|i| a1 + sweep * f64::from(i) / 12.0).map(|t| f.at([r * t.cos(), r * t.sin(), 0.0])).collect())
            }
            _ => Some(vec![t1.0?, t2.0?]),
        }
    }
    /// A profile's outer edge, in its own plane.
    fn profile(&self, id: Option<u64>, depth: usize) -> Option<Vec<Point>> {
        let e = self.get(id?)?;
        if depth > 16 {
            return None;
        }
        let a = |i: usize| e.args.get(i);
        let placed = |pts: Vec<Point>| {
            let pos = self.axes(reference(a(2)));
            pts.into_iter().map(|p| pos.at([p.x, p.y, 0.0])).map(|q| Point::new(q[0], q[1])).collect::<Vec<_>>()
        };
        let rect = |w: f64, h: f64| vec![Point::new(-w / 2.0, -h / 2.0), Point::new(w / 2.0, -h / 2.0), Point::new(w / 2.0, h / 2.0), Point::new(-w / 2.0, h / 2.0)];
        let ellipse = |rx: f64, ry: f64| (0..24).map(|i| f64::from(i) * PI / 12.0).map(|t| Point::new(rx * t.cos(), ry * t.sin())).collect::<Vec<Point>>();
        match e.name.as_str() {
            "IFCRECTANGLEPROFILEDEF" | "IFCROUNDEDRECTANGLEPROFILEDEF" | "IFCRECTANGLEHOLLOWPROFILEDEF" | "IFCISHAPEPROFILEDEF" | "IFCASYMMETRICISHAPEPROFILEDEF" => {
                Some(placed(rect(self.len(a(3))?, self.len(a(4))?)))
            }
            "IFCLSHAPEPROFILEDEF" | "IFCUSHAPEPROFILEDEF" | "IFCCSHAPEPROFILEDEF" | "IFCTSHAPEPROFILEDEF" | "IFCZSHAPEPROFILEDEF" => Some(placed(rect(self.len(a(4))?, self.len(a(3))?))),
            "IFCCIRCLEPROFILEDEF" | "IFCCIRCLEHOLLOWPROFILEDEF" => {
                let r = self.len(a(3))?;
                Some(placed(ellipse(r, r)))
            }
            "IFCELLIPSEPROFILEDEF" => Some(placed(ellipse(self.len(a(3))?, self.len(a(4))?))),
            "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => Some(open(self.curve(reference(a(2)), depth + 1)?.into_iter().map(|p| Point::new(p[0], p[1])).collect())),
            "IFCDERIVEDPROFILEDEF" => {
                let op = self.operator(reference(a(3)));
                Some(self.profile(reference(a(2)), depth + 1)?.into_iter().map(|p| op.at([p.x, p.y, 0.0])).map(|q| Point::new(q[0], q[1])).collect())
            }
            _ => None,
        }
    }
    /// The holes in a profile with voids, in its own plane.
    fn voids(&self, id: Option<u64>, depth: usize) -> Vec<Vec<Point>> {
        let Some(e) = id.and_then(|i| self.get(i)).filter(|e| e.name == "IFCARBITRARYPROFILEDEFWITHVOIDS") else { return vec![] };
        refs(e.args.get(3)).into_iter().filter_map(|c| self.curve(Some(c), depth + 1)).map(|pts| open(pts.into_iter().map(|p| Point::new(p[0], p[1])).collect())).filter(|h| h.len() >= 3).collect()
    }
    /// The faces of representation item `id`, as polygons in its own coordinates. An extrusion's profile holes
    /// come in as their own prisms too (turned in), so a slab's top outline shows them.
    fn faces(&self, id: u64, depth: usize, out: &mut Vec<Vec<P3>>) {
        let Some(e) = self.get(id) else { return };
        if depth > 16 {
            return;
        }
        let a = |i: usize| e.args.get(i);
        let polygon = |out: &mut Vec<Vec<P3>>, pts: Vec<P3>| {
            if pts.len() >= 3 {
                out.push(pts);
            }
        };
        match e.name.as_str() {
            "IFCEXTRUDEDAREASOLID" | "IFCEXTRUDEDAREASOLIDTAPERED" => {
                let (Some(ring), Some(length)) = (self.profile(reference(a(0)), depth + 1), self.len(a(3))) else { return };
                let pos = self.axes(reference(a(1)));
                let up = mul(pos.dir(self.direction(reference(a(2))).unwrap_or([0.0, 0.0, 1.0])), length);
                if ring.len() < 3 {
                    return;
                }
                let mut ring = ring;
                if crate::geom::signed_area(&ring) < 0.0 {
                    ring.reverse();
                }
                let voids = self.voids(reference(a(0)), depth + 1);
                prism(out, ring.iter().map(|p| pos.at([p.x, p.y, 0.0])).collect(), up);
                if !voids.is_empty() {
                    // the top with its holes as one face: the outline, then each hole clockwise, reached and left by a
                    // slit whose two edges cancel when the face's loops are traced
                    let top = out.pop().unwrap_or_default();
                    let lift = |p: &Point| add(pos.at([p.x, p.y, 0.0]), up);
                    let mut face: Vec<P3> = ring.iter().map(lift).collect();
                    let first = ring[0];
                    for mut v in voids {
                        if crate::geom::signed_area(&v) > 0.0 {
                            v.reverse();
                        }
                        let k = (0..v.len()).min_by(|i, j| v[*i].dist(&first).total_cmp(&v[*j].dist(&first))).unwrap_or(0);
                        face.push(lift(&first));
                        face.extend((0..=v.len()).map(|i| lift(&v[(k + i) % v.len()])));
                    }
                    face.push(lift(&first));
                    if face.len() >= 3 {
                        out.push(face);
                    } else {
                        out.push(top);
                    }
                }
            }
            // what a clipping cuts away does not change what the element is
            "IFCBOOLEANCLIPPINGRESULT" | "IFCBOOLEANRESULT" => {
                if let Some(first) = reference(a(1)) {
                    self.faces(first, depth + 1, out);
                }
                if enumeration(a(0)) == Some("UNION") {
                    if let Some(second) = reference(a(2)) {
                        self.faces(second, depth + 1, out);
                    }
                }
            }
            "IFCFACETEDBREP" | "IFCFACETEDBREPWITHVOIDS" | "IFCCSGSOLID" => {
                if let Some(inner) = reference(a(0)) {
                    self.faces(inner, depth + 1, out);
                }
            }
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" | "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                for f in refs(a(0)) {
                    self.faces(f, depth + 1, out);
                }
            }
            "IFCFACE" | "IFCFACESURFACE" => {
                let bounds: Vec<&Entity> = refs(a(0)).into_iter().filter_map(|b| self.get(b)).collect();
                let outer = bounds.iter().find(|b| b.name == "IFCFACEOUTERBOUND").or(bounds.first());
                if let Some(bound) = outer {
                    let lp = self.at(bound.args.first());
                    let mut pts: Vec<P3> = lp.map_or_else(Vec::new, |l| refs(l.args.first()).into_iter().filter_map(|p| self.point(Some(p))).collect());
                    if enumeration(bound.args.get(1)) == Some("F") {
                        pts.reverse();
                    }
                    polygon(out, pts);
                }
            }
            "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" | "IFCPOLYGONALFACESET" => {
                let pts = self.point_list(reference(a(0)));
                let triangles = e.name != "IFCPOLYGONALFACESET";
                let pn = indices(a(if triangles { 4 } else { 3 }));
                let at = |i: usize| {
                    let i = if pn.is_empty() { i } else { i.checked_sub(1).and_then(|k| pn.get(k)).copied()? };
                    i.checked_sub(1).and_then(|k| pts.get(k)).copied()
                };
                let polys: Vec<Vec<usize>> = if triangles {
                    list(a(3)).iter().map(|t| indices(Some(t))).collect()
                } else {
                    refs(a(2)).into_iter().filter_map(|f| self.get(f)).map(|f| indices(f.args.first())).collect()
                };
                for poly in polys {
                    polygon(out, poly.into_iter().filter_map(at).collect());
                }
            }
            "IFCMAPPEDITEM" => {
                let Some(map) = self.at(a(0)) else { return };
                let mut inner = vec![];
                if let Some(rep) = self.at(map.args.get(1)) {
                    for item in refs(rep.args.get(3)) {
                        self.faces(item, depth + 1, &mut inner);
                    }
                }
                let f = self.operator(reference(a(1))).then(self.axes(reference(map.args.first())));
                out.extend(inner.into_iter().map(|face| face.into_iter().map(|p| f.at(p)).collect()));
            }
            "IFCBOUNDINGBOX" | "IFCBLOCK" => {
                let pos = if e.name == "IFCBLOCK" { self.axes(reference(a(0))) } else { Frame { o: self.point(reference(a(0))).unwrap_or_default(), ..WORLD } };
                if let (Some(x), Some(y), Some(z)) = (self.len(a(1)), self.len(a(2)), self.len(a(3))) {
                    prism(out, [[0.0, 0.0, 0.0], [x, 0.0, 0.0], [x, y, 0.0], [0.0, y, 0.0]].map(|p| pos.at(p)).to_vec(), pos.dir([0.0, 0.0, z]));
                }
            }
            _ => {}
        }
    }
    /// A product's shape representations: their identifiers and items.
    fn reps(&self, product: &Entity) -> Vec<(&'a str, Vec<u64>)> {
        let Some(shape) = self.at(product.args.get(6)) else { return vec![] };
        refs(shape.args.get(2)).into_iter().filter_map(|r| self.get(r)).map(|rep| (text(rep.args.get(1)).unwrap_or(""), refs(rep.args.get(3)))).collect()
    }
    fn frame(&self, product: &Entity) -> Frame {
        self.placement(reference(product.args.get(5)), 0)
    }
    /// A product's body as faces in model coordinates: its Body representation, else any but a line or a footprint.
    fn body(&self, product: &Entity) -> Vec<Vec<P3>> {
        let reps = self.reps(product);
        let items = |keep: &dyn Fn(&str) -> bool| reps.iter().filter(|(i, _)| keep(&i.to_ascii_lowercase())).flat_map(|(_, it)| it.clone()).collect::<Vec<_>>();
        let mut chosen = items(&|i| i == "body" || i == "facetation");
        if chosen.is_empty() {
            chosen = items(&|i| !matches!(i, "axis" | "footprint" | "annotation" | "plan" | "profile" | "reference" | "surface" | "box"));
        }
        let f = self.frame(product);
        let mut out = vec![];
        for item in chosen {
            let mut faces = vec![];
            self.faces(item, 0, &mut faces);
            out.extend(faces.into_iter().map(|face| face.into_iter().map(|p| f.at(p)).collect::<Vec<_>>()));
        }
        out
    }
    /// A product's Axis representation, in model coordinates.
    fn axis(&self, product: &Entity) -> Option<Vec<P3>> {
        let (_, items) = self.reps(product).into_iter().find(|(i, _)| i.eq_ignore_ascii_case("Axis"))?;
        let f = self.frame(product);
        Some(self.curve(items.first().copied(), 0)?.into_iter().map(|p| f.at(p)).collect())
    }
}

/// The project's length unit in mm, and its plane angle unit in radians.
fn units(f: &File) -> Result<(f64, f64), String> {
    fn si(prefix: Option<&str>) -> f64 {
        match prefix {
            Some("KILO") => 1e3,
            Some("HECTO") => 1e2,
            Some("DECA") => 1e1,
            Some("DECI") => 1e-1,
            Some("CENTI") => 1e-2,
            Some("MILLI") => 1e-3,
            Some("MICRO") => 1e-6,
            _ => 1.0,
        }
    }
    fn unit(f: &File, id: Option<u64>, depth: usize) -> Option<(String, f64)> {
        let e = f.entities.get(&id?)?;
        let kind = enumeration(e.args.get(1)).unwrap_or("").to_string();
        match e.name.as_str() {
            "IFCSIUNIT" => Some((kind, si(enumeration(e.args.get(2))))),
            "IFCCONVERSIONBASEDUNIT" | "IFCCONVERSIONBASEDUNITWITHOFFSET" if depth < 8 => {
                let measure = f.entities.get(&reference(e.args.get(3))?)?;
                let (_, base) = unit(f, reference(measure.args.get(1)), depth + 1)?;
                Some((kind, real(measure.args.first())? * base))
            }
            _ => None,
        }
    }
    let project = f.entities.iter().filter(|(_, e)| e.name == "IFCPROJECT").min_by_key(|(id, _)| **id).map(|(_, e)| e).ok_or("it has no IfcProject")?;
    let assignment = reference(project.args.get(8)).and_then(|a| f.entities.get(&a));
    let (mut mm, mut rad) = (1.0, 1.0);
    for u in refs(assignment.and_then(|a| a.args.first())) {
        match unit(f, Some(u), 0) {
            Some((kind, size)) if kind == "LENGTHUNIT" => mm = size * 1000.0,
            Some((kind, size)) if kind == "PLANEANGLEUNIT" => rad = size,
            _ => {}
        }
    }
    if !(1e-6..=1e9).contains(&mm) || !(rad.is_finite() && rad > 0.0) {
        return Err("its length or angle unit is not one OpenArch can use".into());
    }
    Ok((mm, rad))
}

/// How elements relate: where each is, what it is part of, what voids and fills what, and its type's name.
#[derive(Default)]
struct Links {
    within: HashMap<u64, u64>,
    whole: HashMap<u64, u64>,
    host: HashMap<u64, u64>,
    fills: HashMap<u64, u64>,
    type_name: HashMap<u64, String>,
}

impl Links {
    fn read(f: &File, ids: &[u64]) -> Links {
        let mut l = Links::default();
        for e in ids.iter().filter_map(|id| f.entities.get(id)) {
            let a = |i: usize| e.args.get(i);
            match e.name.as_str() {
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                    if let Some(s) = reference(a(5)) {
                        l.within.extend(refs(a(4)).into_iter().map(|x| (x, s)));
                    }
                }
                "IFCRELAGGREGATES" => {
                    if let Some(w) = reference(a(4)) {
                        for x in refs(a(5)) {
                            l.whole.insert(x, w);
                        }
                    }
                }
                "IFCRELVOIDSELEMENT" => {
                    if let (Some(h), Some(o)) = (reference(a(4)), reference(a(5))) {
                        l.host.insert(o, h);
                    }
                }
                "IFCRELFILLSELEMENT" => {
                    if let (Some(o), Some(d)) = (reference(a(4)), reference(a(5))) {
                        l.fills.insert(d, o);
                    }
                }
                "IFCRELDEFINESBYTYPE" => {
                    if let Some(name) = reference(a(5)).and_then(|t| f.entities.get(&t)).and_then(|t| text(t.args.get(2))) {
                        l.type_name.extend(refs(a(4)).into_iter().map(|x| (x, name.to_string())));
                    }
                }
                _ => {}
            }
        }
        l
    }
}

const WALLS: &[&str] = &["IFCWALL", "IFCWALLSTANDARDCASE", "IFCWALLELEMENTEDCASE"];
const OPENINGS: &[&str] = &["IFCDOOR", "IFCDOORSTANDARDCASE", "IFCWINDOW", "IFCWINDOWSTANDARDCASE"];
const SLABS: &[&str] = &["IFCSLAB", "IFCSLABSTANDARDCASE", "IFCSLABELEMENTEDCASE"];
/// Products that have no OpenArch element yet: listed as left out.
const OTHERS: &[&str] = &[
    "IFCROOF", "IFCCOLUMN", "IFCBEAM", "IFCMEMBER", "IFCPLATE", "IFCSTAIR", "IFCSTAIRFLIGHT", "IFCRAMP", "IFCRAMPFLIGHT", "IFCRAILING", "IFCCOVERING",
    "IFCCURTAINWALL", "IFCFURNISHINGELEMENT", "IFCFURNITURE", "IFCBUILDINGELEMENTPROXY", "IFCSPACE", "IFCFOOTING", "IFCPILE", "IFCSANITARYTERMINAL",
    "IFCLIGHTFIXTURE", "IFCFLOWTERMINAL", "IFCGRID",
];

/// A wall measured from the file, before the walls are joined.
struct RawWall {
    ifc: u64,
    start: Point,
    end: Point,
    t: f64,
    z: (f64, f64),
    level: Id,
    type_id: Id,
    guid: String,
}

fn line_intersection(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r, s) = (Point::new(b.x - a.x, b.y - a.y), Point::new(d.x - c.x, d.y - c.y));
    let den = r.x * s.y - r.y * s.x;
    if den.abs() < 1e-9 * (r.x.hypot(r.y) * s.x.hypot(s.y)).max(1e-9) {
        return None;
    }
    let t = ((c.x - a.x) * s.y - (c.y - a.y) * s.x) / den;
    Some(Point::new(a.x + r.x * t, a.y + r.y * t))
}

/// Walls that nearly meet made to meet: an end within a wall's thickness of another wall's line moves onto it,
/// so corners and T junctions join as OpenArch joins walls.
fn meet(walls: &mut [RawWall]) {
    for i in 0..walls.len() {
        for at_end in [false, true] {
            let (a, b, t, z) = (walls[i].start, walls[i].end, walls[i].t, walls[i].z);
            let p = if at_end { b } else { a };
            if walls.iter().enumerate().any(|(j, o)| j != i && (o.start.dist(&p) <= 1.0 || o.end.dist(&p) <= 1.0)) {
                continue;
            }
            let mut best: Option<(f64, Point)> = None;
            for (j, o) in walls.iter().enumerate() {
                if j == i || !(o.z.0 < z.1 && z.0 < o.z.1) {
                    continue;
                }
                let Some(x) = line_intersection(a, b, o.start, o.end) else { continue };
                let reach = t.max(o.t);
                let len = o.start.dist(&o.end);
                let along = ((x.x - o.start.x) * (o.end.x - o.start.x) + (x.y - o.start.y) * (o.end.y - o.start.y)) / len.max(1e-9);
                let d = x.dist(&p);
                if d <= reach && (-reach..=len + reach).contains(&along) && best.map_or(true, |(bd, _)| d < bd) {
                    best = Some((d, x));
                }
            }
            // never so far that the wall turns round or vanishes
            if let Some((_, x)) = best {
                let (s, e) = if at_end { (a, x) } else { (x, b) };
                if (e.x - s.x) * (b.x - a.x) + (e.y - s.y) * (b.y - a.y) > a.dist(&b) {
                    walls[i].start = s;
                    walls[i].end = e;
                }
            }
        }
    }
}

/// What an import brought in and what it left out.
#[derive(Debug, Clone, Default)]
pub struct IfcImport {
    pub schema: String,
    pub project: String,
    /// How many of each kind came in: levels, walls, floors, doors, windows.
    pub counts: BTreeMap<&'static str, usize>,
    /// What was left out, and why.
    pub skipped: Vec<String>,
    /// How far the model was moved to bring it near the origin (a site at map coordinates).
    pub moved: Option<Point>,
}

impl Document {
    /// A new project from an IFC file's text: its storeys, straight walls, flat floors, doors and windows on the
    /// project template's types (matched, or made from them).
    pub fn from_ifc(bytes: &[u8]) -> Result<(Document, IfcImport), String> {
        let file = step::parse(bytes)?;
        if !file.schema.starts_with("IFC") {
            let schema = if file.schema.is_empty() { "no schema" } else { &file.schema };
            return Err(format!("this STEP file is in {schema}, not IFC"));
        }
        let (mmu, rad) = units(&file)?;
        let mut ids: Vec<u64> = file.entities.keys().copied().collect();
        ids.sort_unstable();
        let l = Links::read(&file, &ids);
        // far from the origin (a site at its map coordinates), the model comes nearer
        let probe = Reader { f: &file, mm: mmu, rad, shift: Point::new(0.0, 0.0) };
        let origins: Vec<P3> = l.within.keys().filter_map(|id| probe.get(*id)).map(|x| probe.frame(x).o).collect();
        let shift = match (span(origins.iter().map(|p| p[0])), span(origins.iter().map(|p| p[1]))) {
            (Some((x0, x1)), Some((y0, y1))) if x0.abs().max(x1.abs()).max(y0.abs()).max(y1.abs()) > 1e8 => Point::new((x0 / 1000.0).floor() * 1000.0, (y0 / 1000.0).floor() * 1000.0),
            _ => Point::new(0.0, 0.0),
        };
        let r = Reader { f: &file, mm: mmu, rad, shift };
        let e = |id: u64| r.get(id);
        let named = |names: &[&str]| -> Vec<u64> { ids.iter().copied().filter(|id| e(*id).is_some_and(|x| names.contains(&x.name.as_str()))).collect() };
        let who = |id: u64| e(id).map_or_else(|| format!("#{id}"), |x| format!("{} #{id}{}", x.name, text(x.args.get(2)).map_or_else(String::new, |n| format!(" '{n}'"))));
        let label = |id: u64| l.type_name.get(&id).cloned().or_else(|| e(id).and_then(|x| text(x.args.get(4))).map(String::from));
        let guid_of = |id: u64| e(id).and_then(|x| text(x.args.first())).map(String::from).unwrap_or_default();
        let storey_of = |id: u64| {
            let mut at = id;
            for _ in 0..8 {
                at = *l.within.get(&at).or_else(|| l.whole.get(&at))?;
                if e(at).is_some_and(|x| x.name == "IFCBUILDINGSTOREY") {
                    return Some(at);
                }
            }
            None
        };
        let mut out = IfcImport { schema: file.schema.clone(), moved: (shift.x != 0.0 || shift.y != 0.0).then_some(shift), ..Default::default() };
        let project = ids.iter().filter_map(|id| e(*id)).find(|x| x.name == "IFCPROJECT");
        out.project = project.and_then(|p| text(p.args.get(2)).or_else(|| text(p.args.get(5)))).unwrap_or("Imported project").to_string();
        let mut d = Document::new_project();

        // storeys, lowest first, onto the template's levels where one stands at the same height, else new levels
        let mut storeys: Vec<(u64, String, f64)> = named(&["IFCBUILDINGSTOREY"])
            .into_iter()
            .filter_map(|id| {
                let s = e(id)?;
                let z = match reference(s.args.get(5)) {
                    Some(_) => r.frame(s).o[2],
                    None => r.len(s.args.get(9)).unwrap_or(0.0),
                };
                Some((id, text(s.args.get(2)).or_else(|| text(s.args.get(7))).unwrap_or("Level").to_string(), (z * 1000.0).round() / 1000.0))
            })
            .collect();
        storeys.sort_by(|a, b| a.2.total_cmp(&b.2));
        let template: Vec<(Id, f64)> = d.levels().iter().map(|(i, lv)| (*i, lv.elevation_mm)).collect();
        let mut level_of: HashMap<u64, Id> = HashMap::new();
        let mut used: BTreeSet<Id> = BTreeSet::new();
        // first give the template's levels names no storey uses, so storeys can take theirs
        for (i, (lid, _)) in template.iter().enumerate() {
            d.patch_level(*lid, &serde_json::json!({ "name": format!("\u{1}template {i}") }))?;
        }
        for (sid, name, z) in &storeys {
            let mut name = name.clone();
            let mut k = 2;
            while d.levels().iter().any(|(_, lv)| lv.name == name) {
                name = format!("{} ({k})", name.trim_end_matches(&format!(" ({})", k - 1)));
                k += 1;
            }
            let reuse = template.iter().find(|(lid, tz)| (tz - z).abs() <= 1.0 && !used.contains(lid)).map(|(lid, _)| *lid);
            let lid = match reuse {
                Some(lid) => {
                    d.patch_level(lid, &serde_json::json!({ "name": name }))?;
                    lid
                }
                None => {
                    let datum = d.level_datum_z(&Level::new("", 0.0));
                    d.add_level(Some(&name), z - datum, true)?
                }
            };
            used.insert(lid);
            level_of.insert(*sid, lid);
        }
        // the template's levels no storey took: gone, or named again when the file has no storeys at all
        for (i, (lid, _)) in template.iter().enumerate() {
            if used.contains(lid) {
                continue;
            }
            if used.is_empty() && i == 0 {
                d.patch_level(*lid, &serde_json::json!({ "name": "Level 1" }))?;
                used.insert(*lid);
            } else {
                d.delete_level(*lid)?;
            }
        }
        // rename the template's views to their levels' new names
        for (vid, v) in d.views().into_iter().map(|(i, v)| (i, v.clone())).collect::<Vec<_>>() {
            if let Some(lv) = v.level.and_then(|l| d.level(l)).map(|l| l.name.clone()) {
                if v.name.starts_with('\u{1}') || v.view_kind.is_plan() {
                    let _ = d.patch_view(vid, &serde_json::json!({ "name": lv }));
                }
            }
        }
        out.counts.insert("levels", used.len());
        let levels: Vec<(Id, f64)> = d.levels().iter().map(|(i, lv)| (*i, lv.elevation_mm)).collect();
        let elevation = |lid: Id| levels.iter().find(|(i, _)| *i == lid).map_or(0.0, |(_, z)| *z);
        // an element's level: its storey's, else the highest at or below `z`
        let level_for = |id: u64, z: f64| -> Id {
            storey_of(id)
                .and_then(|s| level_of.get(&s).copied())
                .unwrap_or_else(|| levels.iter().rev().find(|(_, lz)| *lz <= z + 1.0).or(levels.first()).map_or(0, |(i, _)| *i))
        };

        // types: matched by name and thickness, else made from the template's first of the kind
        let mut wall_types: BTreeMap<String, Id> = BTreeMap::new();
        let mut wall_type = |d: &mut Document, base: &str, t: f64| -> Result<Id, String> {
            if let Some((id, _)) = d.wall_types().into_iter().find(|(_, wt)| wt.name == base && (wt.thickness_mm() - t).abs() < 0.05) {
                return Ok(id);
            }
            let key = format!("{base}\u{1}{t}");
            if let Some(id) = wall_types.get(&key) {
                return Ok(*id);
            }
            let (_, tpl) = d.wall_types().into_iter().find(|(_, wt)| wt.layers.len() == 1).or_else(|| d.wall_types().into_iter().next()).ok_or("the template has no wall types")?;
            let mut nt = tpl.clone();
            let material = nt.layers.iter().find(|x| x.core).or(nt.layers.first()).map(|x| x.material.clone()).unwrap_or_default();
            nt.layers = vec![Layer::new(&material, t, LayerFunction::Structure, true)];
            let mut name = base.to_string();
            let mut k = 2;
            while d.wall_types().iter().any(|(_, wt)| wt.name == name) {
                name = format!("{base} ({} mm){}", mm(t), if k > 2 { format!(" {k}") } else { String::new() });
                k += 1;
            }
            nt.name = name;
            let id = d.transact("Import IFC", |tx| Ok(tx.add(Element::WallType(nt))))?;
            wall_types.insert(key, id);
            Ok(id)
        };

        // walls: the line their body runs along and how thick it is across it
        let mut raw: Vec<RawWall> = vec![];
        for id in named(WALLS) {
            let Some(w) = e(id) else { continue };
            let faces = r.body(w);
            let pts: Vec<P3> = faces.iter().flatten().copied().collect();
            let axis = r.axis(w);
            let f = r.frame(w);
            let bent: Vec<Point> = axis.iter().flatten().map(|x| r.flat(*x)).collect();
            if let [p, .., q] = bent.as_slice() {
                if bent.iter().any(|x| seg_dist(*x, *p, *q) > 1.0) {
                    out.skipped.push(format!("{}: curved walls are not imported yet", who(id)));
                    continue;
                }
            }
            let (a, b) = match axis.as_deref() {
                Some([p, .., q]) if r.flat(*p).dist(&r.flat(*q)) > 1.0 => (r.flat(*p), r.flat(*q)),
                // no axis: along the wall's own x, as far as its body goes
                _ => {
                    let o = r.flat(f.o);
                    let x = Point::new(f.x[0], f.x[1]);
                    let xl = x.x.hypot(x.y);
                    let x = Point::new(x.x / xl.max(1e-9), x.y / xl.max(1e-9));
                    match span(pts.iter().map(|p| (r.flat(*p).x - o.x) * x.x + (r.flat(*p).y - o.y) * x.y)) {
                        Some((s0, s1)) if xl > 0.5 => (Point::new(o.x + x.x * s0, o.y + x.y * s0), Point::new(o.x + x.x * s1, o.y + x.y * s1)),
                        _ => (o, o),
                    }
                }
            };
            let len = a.dist(&b);
            if len <= 1.0 {
                out.skipped.push(format!("{}: it has no length OpenArch can read", who(id)));
                continue;
            }
            let u = Point::new((b.x - a.x) / len, (b.y - a.y) / len);
            let n = Point::new(-u.y, u.x);
            let (Some((c0, c1)), Some((z0, z1))) = (span(pts.iter().map(|p| (r.flat(*p).x - a.x) * n.x + (r.flat(*p).y - a.y) * n.y)), span(pts.iter().map(|p| p[2]))) else {
                out.skipped.push(format!("{}: it has no body OpenArch can read", who(id)));
                continue;
            };
            if c1 - c0 <= 0.0 || z1 - z0 < 1.0 {
                out.skipped.push(format!("{}: it has no thickness or height OpenArch can read", who(id)));
                continue;
            }
            let t = mm(c1 - c0);
            let mid = (c0 + c1) / 2.0;
            let base = label(id).unwrap_or_else(|| format!("Imported {} mm", mm(t)));
            let type_id = wall_type(&mut d, &base, t)?;
            raw.push(RawWall {
                ifc: id,
                start: Point::new(a.x + n.x * mid, a.y + n.y * mid),
                end: Point::new(b.x + n.x * mid, b.y + n.y * mid),
                t,
                z: (z0, z1),
                level: level_for(id, z0),
                type_id,
                guid: guid_of(id),
            });
        }
        meet(&mut raw);
        // each wall made: its id, its line and the height of its base
        let mut walls: HashMap<u64, (Id, Point, Point, f64)> = HashMap::new();
        let made = d.transact("Import IFC", |tx| {
            let mut made = vec![];
            for w in &raw {
                let base = elevation(w.level);
                let mut nw = Wall::new(w.type_id, w.level, w.start, w.end, mm(w.z.1 - w.z.0));
                nw.base_offset_mm = mm(w.z.0 - base);
                // up to a level the top meets, else unconnected at its height
                if let Some((lid, _)) = levels.iter().find(|(lid, lz)| *lid != w.level && *lz > base + 1.0 && (lz - w.z.1).abs() <= 1.0) {
                    nw.top_level = Some(*lid);
                    nw.top_offset_mm = 0.0;
                }
                if valid_guid(&w.guid) {
                    nw.ifc.guid = w.guid.clone();
                }
                made.push((w.ifc, tx.add(Element::Wall(nw)), w.start, w.end, w.z.0));
            }
            Ok(made)
        })?;
        for (ifc, id, a, b, z0) in made {
            walls.insert(ifc, (id, a, b, z0));
        }
        out.counts.insert("walls", walls.len());

        // floors: their top outline and the holes in it, and how thick they are below it
        let mut floor_types: BTreeMap<String, Id> = BTreeMap::new();
        let mut floors = 0;
        for id in named(SLABS) {
            let Some(s) = e(id) else { continue };
            if l.whole.get(&id).and_then(|w| e(*w)).is_some_and(|w| matches!(w.name.as_str(), "IFCROOF" | "IFCSTAIR" | "IFCRAMP")) {
                continue;
            }
            match enumeration(s.args.get(8)) {
                Some("ROOF") => {
                    out.skipped.push(format!("{}: roofs are not imported yet", who(id)));
                    continue;
                }
                Some("LANDING") => {
                    out.skipped.push(format!("{}: landings are not imported yet", who(id)));
                    continue;
                }
                _ => {}
            }
            let faces = r.body(s);
            let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
                out.skipped.push(format!("{}: it has no body OpenArch can read", who(id)));
                continue;
            };
            let Some((outer, holes)) = outline_at(&faces, z1) else {
                out.skipped.push(format!("{}: its top is not level (sloped floors are not imported yet)", who(id)));
                continue;
            };
            let t = mm(z1 - z0);
            let base = label(id).unwrap_or_else(|| format!("Imported {} mm", mm(t)));
            let tid = match d.floor_types().into_iter().find(|(_, ft)| ft.name == base && (ft.thickness_mm() - t).abs() < 0.05) {
                Some((tid, _)) => tid,
                None => match floor_types.get(&format!("{base}\u{1}{t}")) {
                    Some(tid) => *tid,
                    None => {
                        let (_, tpl) = d.floor_types().into_iter().next().ok_or("the template has no floor types")?;
                        let mut nt = tpl.clone();
                        let material = nt.layers.iter().find(|x| x.core).or(nt.layers.first()).map(|x| x.material.clone()).unwrap_or_default();
                        nt.layers = vec![Layer::new(&material, t, LayerFunction::Structure, true)];
                        let mut name = base.clone();
                        if d.floor_types().iter().any(|(_, ft)| ft.name == name) {
                            name = format!("{base} ({} mm)", mm(t));
                        }
                        nt.name = name;
                        let tid = d.transact("Import IFC", |tx| Ok(tx.add(Element::FloorType(nt))))?;
                        floor_types.insert(format!("{base}\u{1}{t}"), tid);
                        tid
                    }
                },
            };
            let level = levels.iter().find(|(_, lz)| (lz - z1).abs() <= 1.0).map(|(i, _)| *i).unwrap_or_else(|| level_for(id, z1));
            let ring = |pts: &[Point]| -> Vec<Curve> {
                let pts: Vec<Point> = pts.iter().map(|p| r.flat([p.x, p.y, 0.0])).collect();
                (0..pts.len()).map(|i| Curve::line(pts[i], pts[(i + 1) % pts.len()])).collect()
            };
            let mut loops = vec![ring(&outer)];
            loops.extend(holes.iter().map(|h| ring(h)));
            let mut nf = Floor::new(tid, level, loops);
            nf.offset_mm = mm(z1 - elevation(level));
            d.transact("Import IFC", |tx| Ok(tx.add(Element::Floor(nf))))?;
            floors += 1;
        }
        out.counts.insert("floors", floors);

        // doors and windows: where their openings cut their walls, on a type of their size
        let (mut doors, mut windows) = (0, 0);
        for id in named(OPENINGS) {
            let Some(o) = e(id) else { continue };
            let is_door = o.name.starts_with("IFCDOOR");
            let opening = l.fills.get(&id).copied();
            let Some(&(host, a, b, _)) = opening.and_then(|op| l.host.get(&op)).and_then(|h| walls.get(h)) else {
                out.skipped.push(format!("{}: {}", who(id), if opening.is_some() { "its wall was left out" } else { "it is not in a wall" }));
                continue;
            };
            let mut faces = opening.and_then(e).map(|op| r.body(op)).unwrap_or_default();
            if faces.is_empty() {
                faces = r.body(o);
            }
            let len = a.dist(&b).max(1e-9);
            let u = Point::new((b.x - a.x) / len, (b.y - a.y) / len);
            let along = |p: &P3| (r.flat(*p).x - a.x) * u.x + (r.flat(*p).y - a.y) * u.y;
            let (Some((s0, s1)), Some((z0, z1))) = (span(faces.iter().flatten().map(along)), span(faces.iter().flatten().map(|p| p[2]))) else {
                out.skipped.push(format!("{}: it has no body OpenArch can read", who(id)));
                continue;
            };
            let width = mm(r.len(o.args.get(9)).filter(|w| *w > 0.0).unwrap_or(s1 - s0));
            let height = mm(r.len(o.args.get(8)).filter(|h| *h > 0.0).unwrap_or(z1 - z0));
            let cat = if is_door { FamilyCategory::Doors } else { FamilyCategory::Windows };
            let tid = match opening_type(&mut d, cat, width, height, label(id).as_deref()) {
                Ok(t) => t,
                Err(why) => {
                    out.skipped.push(format!("{}: {why}", who(id)));
                    continue;
                }
            };
            let mid = (s0 + s1) / 2.0;
            let at = Point::new(a.x + u.x * mid, a.y + u.y * mid);
            let iid = match d.place_on_wall(tid, host, at) {
                Ok(i) => i,
                Err(why) => {
                    out.skipped.push(format!("{}: {why}", who(id)));
                    continue;
                }
            };
            // a window's sill (and a door's, when it is raised) from the bottom of its opening
            if let Some(Element::FamilyInstance(mut i)) = d.get(iid).cloned() {
                let sill = mm(z0 - elevation(i.level));
                if !is_door || sill.abs() > 1.0 {
                    i.values.insert("Sill Height".into(), ParamValue::Number(sill.max(0.0)));
                }
                if enumeration(o.args.get(11)).is_some_and(|op| op.contains("RIGHT")) {
                    i.flip_hand = true;
                }
                let g = guid_of(id);
                if valid_guid(&g) {
                    i.ifc.guid = g;
                }
                let _ = d.transact("Import IFC", |tx| tx.update(iid, Element::FamilyInstance(i)));
            }
            if is_door {
                doors += 1;
            } else {
                windows += 1;
            }
        }
        out.counts.insert("doors", doors);
        out.counts.insert("windows", windows);

        // what has no OpenArch element yet
        let mut others: BTreeMap<&str, usize> = BTreeMap::new();
        for id in named(OTHERS) {
            if let Some(x) = e(id) {
                *others.entry(x.name.as_str()).or_default() += 1;
            }
        }
        for (name, n) in others {
            out.skipped.push(format!("{n} {name}: not imported yet"));
        }
        d.clear_history();
        Ok((d, out))
    }
}

fn valid_guid(g: &str) -> bool {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    g.len() == 22 && g.bytes().all(|c| DIGITS.contains(&c)) && DIGITS.iter().position(|d| *d == g.as_bytes()[0]).is_some_and(|d| d < 4)
}

/// A door or window type `width` by `height`: one the project has, else a copy of the first of the category resized.
fn opening_type(d: &mut Document, cat: FamilyCategory, width: f64, height: f64, name: Option<&str>) -> Result<Id, String> {
    let of_cat: Vec<(Id, FamilyType)> = d.family_types().into_iter().filter(|(_, t)| d.family(t.family).is_some_and(|f| f.category == cat)).map(|(i, t)| (i, t.clone())).collect();
    let size = |d: &Document, t: &FamilyType| {
        let w = d.type_param(t, "Width").and_then(|v| v.as_f64());
        let h = d.type_param(t, "Height").and_then(|v| v.as_f64());
        (w, h)
    };
    if let Some((id, _)) = of_cat.iter().find(|(_, t)| matches!(size(d, t), (Some(w), Some(h)) if (w - width).abs() < 0.5 && (h - height).abs() < 0.5)) {
        return Ok(*id);
    }
    let (_, tpl) = of_cat.first().ok_or("the template has no family of that category")?;
    let mut nt = tpl.clone();
    nt.values.insert("Width".into(), ParamValue::Number(width));
    nt.values.insert("Height".into(), ParamValue::Number(height));
    let base = name.map(str::to_string).unwrap_or_else(|| format!("{} x {}", mm(width), mm(height)));
    let mut name = base.clone();
    let mut k = 2;
    while d.family_types().iter().any(|(_, t)| t.family == nt.family && t.name == name) {
        name = format!("{base} {k}");
        k += 1;
    }
    nt.name = name;
    d.transact("Import IFC", |tx| Ok(tx.add(Element::FamilyType(nt))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_export_comes_back() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Brick Veneer").map(|(i, _)| *i).unwrap_or(d.wall_types()[0].0);
        let a = d.add_wall(wt, lvl, Point::new(0.0, 0.0), Point::new(6000.0, 0.0), 2700.0).unwrap();
        d.add_wall(wt, lvl, Point::new(6000.0, 0.0), Point::new(6000.0, 4000.0), 2700.0).unwrap();
        d.add_wall(wt, lvl, Point::new(6000.0, 4000.0), Point::new(0.0, 4000.0), 2700.0).unwrap();
        d.add_wall(wt, lvl, Point::new(0.0, 4000.0), Point::new(0.0, 0.0), 2700.0).unwrap();
        let win = d.family_types().into_iter().find(|(_, t)| t.name.contains("1200")).map(|(i, _)| i).unwrap();
        d.place_on_wall(win, a, Point::new(3000.0, 0.0)).unwrap();
        let door = d.family_types().into_iter().find(|(_, t)| d.family(t.family).is_some_and(|f| f.category == FamilyCategory::Doors)).map(|(i, _)| i).unwrap();
        d.place_on_wall(door, a, Point::new(1200.0, 0.0)).unwrap();
        let ft = d.floor_types()[0].0;
        d.add_floor(ft, lvl, vec![Point::new(0.0, 0.0), Point::new(6000.0, 0.0), Point::new(6000.0, 4000.0), Point::new(0.0, 4000.0)]).unwrap();
        let text = d.export_ifc("Round trip");
        let (n, report) = Document::from_ifc(text.as_bytes()).unwrap();
        assert_eq!(report.project, "Round trip");
        assert_eq!(report.counts["walls"], 4, "{report:?}");
        assert_eq!((report.counts["floors"], report.counts["doors"], report.counts["windows"]), (1, 1, 1), "{report:?}");
        assert_eq!(report.counts["levels"], d.levels().iter().filter(|(_, l)| l.building_story).count());
        // walls on the same lines, as thick, keeping their GlobalIds; corners still meet
        let walls: Vec<&Wall> = n.iter().filter_map(|(_, e)| if let Element::Wall(w) = e { Some(w) } else { None }).collect();
        let first = walls.iter().find(|w| (w.start.y.abs() < 1.0 && w.end.y.abs() < 1.0)).expect("the south wall");
        assert!((first.start.x.min(first.end.x)).abs() < 1.0 && (first.start.x.max(first.end.x) - 6000.0).abs() < 1.0, "{:?} {:?}", first.start, first.end);
        let thick = |doc: &Document, w: &Wall| doc.wall_type(w.wall_type).unwrap().thickness_mm();
        assert!((thick(&n, first) - d.wall_type(wt).unwrap().thickness_mm()).abs() < 0.5);
        assert!((n.wall_extent(first).1 - n.wall_extent(first).0 - 2700.0).abs() < 0.5);
        let guid = match d.get(a) {
            Some(Element::Wall(w)) => w.ifc.guid.clone(),
            _ => unreachable!(),
        };
        assert!(walls.iter().any(|w| w.ifc.guid == guid));
        // the window keeps its size and sill
        let (_, wi) = n.instances().into_iter().find(|(_, i)| n.instance_family(i).is_some_and(|(f, _)| f.category == FamilyCategory::Windows)).unwrap();
        let (w, h, sill) = n.instance_opening_size(wi);
        let (w0, h0, sill0) = d.instance_opening_size(d.instances().into_iter().find(|(_, i)| d.instance_family(i).is_some_and(|(f, _)| f.category == FamilyCategory::Windows)).unwrap().1);
        assert!((w - w0).abs() < 0.5 && (h - h0).abs() < 0.5 && (sill - sill0).abs() < 0.5, "{w} {h} {sill} against {w0} {h0} {sill0}");
        // doors and windows land on the template's own types, not new ones
        let types = |doc: &Document| doc.family_types().len();
        assert_eq!(types(&n), types(&Document::new_project()));
    }

    #[test]
    fn a_file_written_another_way_reads_in_metres_with_a_mapped_wall() {
        // metres, a wall with no axis whose body is a polyline profile extruded, a slab with a hole, a column left out
        let f = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('x','',(''),(''),'','','');\nFILE_SCHEMA(('IFC2X3'));\nENDSEC;\nDATA;\n\
#1=IFCPROJECT('0aaaaaaaaaaaaaaaaaaaaa',$,'Metres',$,$,$,$,(#2),#3);\n\
#2=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#10,$);\n\
#3=IFCUNITASSIGNMENT((#4));\n#4=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n\
#10=IFCAXIS2PLACEMENT3D(#11,$,$);\n#11=IFCCARTESIANPOINT((0.,0.,0.));\n\
#20=IFCBUILDINGSTOREY('0bbbbbbbbbbbbbbbbbbbbb',$,'GF',$,$,#21,$,$,.ELEMENT.,0.);\n#21=IFCLOCALPLACEMENT($,#10);\n\
#22=IFCBUILDINGSTOREY('0ccccccccccccccccccccc',$,'L2',$,$,#23,$,$,.ELEMENT.,3.2);\n#23=IFCLOCALPLACEMENT($,#24);\n#24=IFCAXIS2PLACEMENT3D(#25,$,$);\n#25=IFCCARTESIANPOINT((0.,0.,3.2));\n\
#30=IFCWALL('0ddddddddddddddddddddd',$,'W1',$,$,#31,#32,$,$);\n#31=IFCLOCALPLACEMENT(#21,#33);\n#33=IFCAXIS2PLACEMENT3D(#34,$,#35);\n#34=IFCCARTESIANPOINT((1.,1.,0.));\n#35=IFCDIRECTION((0.,1.,0.));\n\
#32=IFCPRODUCTDEFINITIONSHAPE($,$,(#36));\n#36=IFCSHAPEREPRESENTATION(#2,'Body','SweptSolid',(#37));\n#37=IFCEXTRUDEDAREASOLID(#38,#10,#39,3.2);\n#39=IFCDIRECTION((0.,0.,1.));\n\
#38=IFCRECTANGLEPROFILEDEF(.AREA.,$,#40,5.,0.2);\n#40=IFCAXIS2PLACEMENT2D(#41,$);\n#41=IFCCARTESIANPOINT((2.5,0.));\n\
#50=IFCSLAB('0eeeeeeeeeeeeeeeeeeeee',$,'S1',$,$,#21,#51,$,.FLOOR.);\n#51=IFCPRODUCTDEFINITIONSHAPE($,$,(#52));\n#52=IFCSHAPEREPRESENTATION(#2,'Body','SweptSolid',(#53));\n#53=IFCEXTRUDEDAREASOLID(#54,#55,#39,0.25);\n#55=IFCAXIS2PLACEMENT3D(#56,$,$);\n#56=IFCCARTESIANPOINT((0.,0.,-0.25));\n\
#54=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#57,(#58));\n#57=IFCPOLYLINE((#60,#61,#62,#63,#60));\n#58=IFCPOLYLINE((#64,#65,#66,#67,#64));\n\
#60=IFCCARTESIANPOINT((0.,0.));\n#61=IFCCARTESIANPOINT((4.,0.));\n#62=IFCCARTESIANPOINT((4.,3.));\n#63=IFCCARTESIANPOINT((0.,3.));\n\
#64=IFCCARTESIANPOINT((1.,1.));\n#65=IFCCARTESIANPOINT((1.,2.));\n#66=IFCCARTESIANPOINT((2.,2.));\n#67=IFCCARTESIANPOINT((2.,1.));\n\
#70=IFCCOLUMN('0fffffffffffffffffffff',$,'C1',$,$,#21,$,$);\n\
#80=IFCRELCONTAINEDINSPATIALSTRUCTURE('0ggggggggggggggggggggg',$,$,$,(#30,#50,#70),#20);\n\
ENDSEC;\nEND-ISO-10303-21;\n";
        let (n, report) = Document::from_ifc(f.as_bytes()).unwrap();
        assert_eq!(report.schema, "IFC2X3");
        let names: Vec<String> = n.levels().iter().map(|(_, l)| l.name.clone()).collect();
        assert_eq!(names, vec!["GF", "L2"]);
        assert!((n.levels()[1].1.elevation_mm - 3200.0).abs() < 0.01);
        let w = n.iter().find_map(|(_, e)| if let Element::Wall(w) = e { Some(w.clone()) } else { None }).expect("the wall");
        // placed at (1, 1) m and turned to run north, 5 m long and 200 mm thick
        let (lo, hi) = (w.start.y.min(w.end.y), w.start.y.max(w.end.y));
        assert!((w.start.x - 1000.0).abs() < 0.5 && (w.end.x - 1000.0).abs() < 0.5 && (lo - 1000.0).abs() < 0.5 && (hi - 6000.0).abs() < 0.5, "{w:?}");
        assert!((n.wall_type(w.wall_type).unwrap().thickness_mm() - 200.0).abs() < 0.01);
        assert_eq!(w.top_level, Some(n.levels()[1].0), "its top meets L2");
        let fl = n.iter().find_map(|(_, e)| if let Element::Floor(f) = e { Some(f.clone()) } else { None }).expect("the slab");
        assert_eq!(fl.loops().len(), 2, "the outline and its hole");
        assert!((n.floor_type(fl.floor_type).unwrap().thickness_mm() - 250.0).abs() < 0.01);
        assert!(fl.offset_mm.abs() < 0.01);
        assert!(report.skipped.iter().any(|s| s.contains("IFCCOLUMN")), "{:?}", report.skipped);
        assert!(Document::from_ifc(b"not a file").is_err());
    }
}
