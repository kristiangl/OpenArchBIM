//! Shape editing: a surface over a boundary, through points and along split lines.
//!
//! The host (a toposolid now; floors and roofs when their shape editing comes) keeps its boundary, a list of points
//! (plan position and height above its unmodified top) and split lines, each joining two of the points by index.
//! The surface is a constrained Delaunay triangulation (TIN) of the boundary's corners and the points, with the
//! boundary's edges and the split lines kept as edges, so it folds along a split line. A corner of the boundary with
//! no point on it sits at the unmodified top.
//!
//! `tidy` is what every edit goes through: points stay inside the boundary, a point on the same spot as a later one
//! gives way to it, split lines stay inside, a line is broken where it runs over a point, and two lines that cross
//! are both broken at a new point there, at the height the earlier line gives (the surface's height there before
//! the later line came).

use crate::site::P3;
use crate::Point;

/// The top surface: corners with their heights (absolute, mm) and counter-clockwise triangles.
#[derive(Debug, Clone, Default)]
pub struct Tin {
    pub verts: Vec<[f64; 3]>,
    pub tris: Vec<[usize; 3]>,
}

impl Tin {
    fn corner(&self, i: usize) -> Point {
        Point::new(self.verts[i][0], self.verts[i][1])
    }

    /// The height of the surface at a plan point, None outside it.
    pub fn z_at(&self, p: Point) -> Option<f64> {
        for t in &self.tris {
            let (a, b, c) = (self.verts[t[0]], self.verts[t[1]], self.verts[t[2]]);
            let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if det.abs() < 1e-12 { continue; }
            let l1 = ((b[1] - c[1]) * (p.x - c[0]) + (c[0] - b[0]) * (p.y - c[1])) / det;
            let l2 = ((c[1] - a[1]) * (p.x - c[0]) + (a[0] - c[0]) * (p.y - c[1])) / det;
            let l3 = 1.0 - l1 - l2;
            let e = -1e-9;
            if l1 >= e && l2 >= e && l3 >= e {
                return Some(l1 * a[2] + l2 * b[2] + l3 * c[2]);
            }
        }
        None
    }

    /// Each edge once, with the triangles on either side (one for an edge on the boundary).
    pub fn edges(&self) -> Vec<([usize; 2], Vec<usize>)> {
        let mut map: std::collections::BTreeMap<[usize; 2], Vec<usize>> = Default::default();
        for (k, t) in self.tris.iter().enumerate() {
            for j in 0..3 {
                let (a, b) = (t[j], t[(j + 1) % 3]);
                map.entry([a.min(b), a.max(b)]).or_default().push(k);
            }
        }
        map.into_iter().collect()
    }

    /// Boundary edges in the direction their triangle runs (so the outside is on the right).
    pub fn boundary_edges(&self) -> Vec<[usize; 2]> {
        let mut count: std::collections::HashMap<[usize; 2], usize> = Default::default();
        for t in &self.tris {
            for j in 0..3 {
                let (a, b) = (t[j], t[(j + 1) % 3]);
                *count.entry([a.min(b), a.max(b)]).or_default() += 1;
            }
        }
        let mut out = vec![];
        for t in &self.tris {
            for j in 0..3 {
                let (a, b) = (t[j], t[(j + 1) % 3]);
                if count[&[a.min(b), a.max(b)]] == 1 { out.push([a, b]); }
            }
        }
        out
    }

    pub fn z_range(&self) -> (f64, f64) {
        self.verts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v[2]), hi.max(v[2])))
    }

    /// Plan area of the surface (mm²).
    pub fn plan_area(&self) -> f64 {
        self.tris.iter().map(|t| {
            let (a, b, c) = (self.corner(t[0]), self.corner(t[1]), self.corner(t[2]));
            ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() / 2.0
        }).sum()
    }

    /// Where the surface crosses height `z`: line pieces in plan.
    pub fn contour(&self, z: f64) -> Vec<[Point; 2]> {
        let mut out = vec![];
        for t in &self.tris {
            let v = [self.verts[t[0]], self.verts[t[1]], self.verts[t[2]]];
            let mut hits = vec![];
            for j in 0..3 {
                let (a, b) = (v[j], v[(j + 1) % 3]);
                // a corner exactly on the height counts as just above it, so each crossing is found once
                let (da, db) = (a[2] - z, b[2] - z);
                let (above_a, above_b) = (da >= 0.0, db >= 0.0);
                if above_a == above_b { continue; }
                let s = da / (da - db);
                hits.push(Point::new(a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s));
            }
            if hits.len() == 2 && (hits[0].x - hits[1].x).hypot(hits[0].y - hits[1].y) > 1e-6 {
                out.push([hits[0], hits[1]]);
            }
        }
        out
    }
}


impl Tin {
    /// The height of the surface at a plan point, its upward normal there, and whether the point is on a fold (an
    /// edge or corner where triangles that do not lie in one plane meet, so no one slope holds).
    pub fn sample(&self, p: Point) -> Option<(f64, [f64; 3], bool)> {
        let mut found: Vec<(f64, [f64; 3])> = vec![];
        for t in &self.tris {
            let (a, b, c) = (self.verts[t[0]], self.verts[t[1]], self.verts[t[2]]);
            let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if det.abs() < 1e-12 { continue; }
            let l1 = ((b[1] - c[1]) * (p.x - c[0]) + (c[0] - b[0]) * (p.y - c[1])) / det;
            let l2 = ((c[1] - a[1]) * (p.x - c[0]) + (a[0] - c[0]) * (p.y - c[1])) / det;
            let l3 = 1.0 - l1 - l2;
            // a few hundredths of a millimetre either side of an edge counts as on it
            let size = (a[0] - b[0]).hypot(a[1] - b[1]).max((b[0] - c[0]).hypot(b[1] - c[1])).max(1.0);
            let e = -0.05 / size;
            if l1 >= e && l2 >= e && l3 >= e {
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if len < 1e-12 { continue; }
                let s = if n[2] < 0.0 { -1.0 } else { 1.0 };
                found.push((l1 * a[2] + l2 * b[2] + l3 * c[2], [s * n[0] / len, s * n[1] / len, s * n[2] / len]));
            }
        }
        let first = *found.first()?;
        let fold = found.iter().any(|f| f.1[0] * first.1[0] + f.1[1] * first.1[1] + f.1[2] * first.1[2] < 0.999_999);
        Some((first.0, first.1, fold))
    }
}

/// Inside the region rings by the even-odd rule.
pub(crate) fn inside(rings: &[Vec<Point>], p: Point) -> bool {
    let mut odd = false;
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
                odd = !odd;
            }
        }
    }
    odd
}

/// On a ring's line, within half a millimetre.
pub(crate) fn on_ring(r: &[Point], p: Point) -> bool {
    let n = r.len();
    (0..n).any(|i| {
        let (a, b) = (r[i], r[(i + 1) % n]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-12 { return false; }
        let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
        (a.x + dx * t - p.x).hypot(a.y + dy * t - p.y) < 0.5
    })
}

/// Inside the boundary or on its line.
pub fn within(rings: &[Vec<Point>], p: Point) -> bool {
    inside(rings, p) || rings.iter().any(|r| on_ring(r, p))
}

/// The triangulated surface over `rings` (the boundary and its holes), through `points` (absolute heights) and
/// along `lines` (pairs of indices into `points`). A point outside the boundary is left out, with any line to it; a
/// line that would cross another is left out (`tidy` keeps that from happening).
pub fn triangulate(rings: &[Vec<Point>], points: &[[f64; 3]], lines: &[[usize; 2]], base_z: f64) -> Option<Tin> {
    use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    let mut zs: Vec<Option<f64>> = vec![];
    let mut put = |cdt: &mut ConstrainedDelaunayTriangulation<Point2<f64>>, x: f64, y: f64, z: Option<f64>| -> Option<spade::handles::FixedVertexHandle> {
        let h = cdt.insert(Point2::new(x, y)).ok()?;
        if zs.len() <= h.index() { zs.resize(h.index() + 1, None); }
        if z.is_some() { zs[h.index()] = z; }
        Some(h)
    };
    // points first, so a boundary corner that coincides with one takes its height
    let mut handles = vec![None; points.len()];
    for (i, p) in points.iter().enumerate() {
        if !(p[0].is_finite() && p[1].is_finite() && p[2].is_finite()) { continue; }
        if within(rings, Point::new(p[0], p[1])) {
            handles[i] = put(&mut cdt, p[0], p[1], Some(p[2]));
        }
    }
    for r in rings {
        let hs: Vec<_> = r.iter().filter_map(|q| put(&mut cdt, q.x, q.y, None)).collect();
        for i in 0..hs.len() {
            let (a, b) = (hs[i], hs[(i + 1) % hs.len()]);
            if a != b && cdt.can_add_constraint(a, b) { cdt.add_constraint(a, b); }
        }
    }
    for l in lines {
        let (Some(Some(a)), Some(Some(b))) = (handles.get(l[0]), handles.get(l[1])) else { continue };
        if a != b && cdt.can_add_constraint(*a, *b) { cdt.add_constraint(*a, *b); }
    }
    if cdt.num_vertices() < 3 { return None; }
    let verts: Vec<[f64; 3]> = cdt.vertices().map(|v| {
        let p = v.position();
        [p.x, p.y, zs.get(v.fix().index()).copied().flatten().unwrap_or(base_z)]
    }).collect();
    let mut tris = vec![];
    for f in cdt.inner_faces() {
        let vs = f.vertices();
        let ids = [vs[0].fix().index(), vs[1].fix().index(), vs[2].fix().index()];
        let c = Point::new((verts[ids[0]][0] + verts[ids[1]][0] + verts[ids[2]][0]) / 3.0, (verts[ids[0]][1] + verts[ids[1]][1] + verts[ids[2]][1]) / 3.0);
        if inside(rings, c) { tris.push(ids); }
    }
    if tris.is_empty() { return None; }
    // keep only the corners the triangles use
    let mut remap = vec![usize::MAX; verts.len()];
    let mut kept = vec![];
    for t in tris.iter_mut() {
        for i in t.iter_mut() {
            if remap[*i] == usize::MAX { remap[*i] = kept.len(); kept.push(verts[*i]); }
            *i = remap[*i];
        }
    }
    Some(Tin { verts: kept, tris })
}

/// Where segments `a-b` and `c-d` cross away from their ends: the fraction along each.
fn crossing(a: P3, b: P3, c: P3, d: P3) -> Option<(f64, f64)> {
    let (rx, ry, sx, sy) = (b.x - a.x, b.y - a.y, d.x - c.x, d.y - c.y);
    let den = rx * sy - ry * sx;
    if den.abs() < 1e-9 { return None; }
    let t = ((c.x - a.x) * sy - (c.y - a.y) * sx) / den;
    let u = ((c.x - a.x) * ry - (c.y - a.y) * rx) / den;
    let (lt, lu) = (rx.hypot(ry), sx.hypot(sy));
    // more than half a millimetre in from the ends of both
    (t * lt > 0.5 && (1.0 - t) * lt > 0.5 && u * lu > 0.5 && (1.0 - u) * lu > 0.5).then_some((t, u))
}

/// Whether a segment between two plan points leaves the boundary: it crosses a ring's line, or its middle is outside.
fn leaves(rings: &[Vec<Point>], a: Point, b: Point) -> bool {
    let (pa, pb) = (P3 { x: a.x, y: a.y, z: 0.0 }, P3 { x: b.x, y: b.y, z: 0.0 });
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let (c, d) = (r[i], r[(i + 1) % n]);
            if crossing(pa, pb, P3 { x: c.x, y: c.y, z: 0.0 }, P3 { x: d.x, y: d.y, z: 0.0 }).is_some() { return true; }
        }
    }
    // a line along the boundary is fine; one that cuts across a notch has its middle outside
    let m = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    !within(rings, m)
}

/// Every edit's last step (see the module notes): the points and lines as they are to be kept. `noun` names the
/// host in refusals.
pub fn tidy(rings: &[Vec<Point>], points: Vec<P3>, lines: Vec<[usize; 2]>, noun: &str) -> Result<(Vec<P3>, Vec<[usize; 2]>), String> {
    for p in &points {
        if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) { return Err("a point must be numbers".into()); }
        if !within(rings, Point::new(p.x, p.y)) { return Err(format!("A point must stay inside the {noun}'s boundary.")); }
    }
    if lines.iter().any(|l| l[0] >= points.len() || l[1] >= points.len()) { return Err("a split line refers to a point that is not there".into()); }
    // a point on the same spot as a later one gives way to it, and its lines go to that one
    let n = points.len();
    let mut to = vec![usize::MAX; n];
    let mut kept: Vec<P3> = vec![];
    for i in (0..n).rev() {
        let p = points[i];
        match kept.iter().position(|k| (k.x - p.x).hypot(k.y - p.y) <= 1.0) {
            Some(k) => to[i] = k,
            None => { to[i] = kept.len(); kept.push(p); }
        }
    }
    let m = kept.len();
    kept.reverse();
    let mut pts = kept;
    let mut ls: Vec<[usize; 2]> = lines.iter().map(|l| [m - 1 - to[l[0]], m - 1 - to[l[1]]]).collect();
    for l in &ls {
        let (a, b) = (pts[l[0]], pts[l[1]]);
        if l[0] != l[1] && leaves(rings, Point::new(a.x, a.y), Point::new(b.x, b.y)) {
            return Err(format!("A split line must stay inside the {noun}'s boundary."));
        }
    }
    // break lines over points and at crossings until none is left (each pass adds a point or shortens a line); each
    // piece keeps the place in the list of the line it came from
    let mut rank: Vec<usize> = (0..ls.len()).collect();
    for _ in 0..10_000 {
        let mut k = 0;
        rank.retain(|_| { let keep = ls[k][0] != ls[k][1]; k += 1; keep });
        ls.retain(|l| l[0] != l[1]);
        let mut changed = false;
        'lines: for k in 0..ls.len() {
            let [i, j] = ls[k];
            let (a, b) = (pts[i], pts[j]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            for (q, p) in pts.iter().enumerate() {
                if q == i || q == j { continue; }
                let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / (len * len);
                if t * len > 0.5 && (1.0 - t) * len > 0.5 && (a.x + dx * t - p.x).hypot(a.y + dy * t - p.y) < 0.5 {
                    ls[k] = [i, q];
                    ls.push([q, j]);
                    rank.push(rank[k]);
                    changed = true;
                    break 'lines;
                }
            }
            for k2 in k + 1..ls.len() {
                let [c, d] = ls[k2];
                if let Some((t, u)) = crossing(a, b, pts[c], pts[d]) {
                    // the new point takes the height the earlier line gives there
                    let z = if rank[k] <= rank[k2] { a.z + (b.z - a.z) * t } else { pts[c].z + (pts[d].z - pts[c].z) * u };
                    let q = pts.len();
                    pts.push(P3 { x: a.x + dx * t, y: a.y + dy * t, z });
                    ls[k] = [i, q];
                    ls[k2] = [c, q];
                    ls.push([q, j]);
                    ls.push([q, d]);
                    rank.push(rank[k]);
                    rank.push(rank[k2]);
                    changed = true;
                    break 'lines;
                }
            }
        }
        if !changed { break; }
    }
    // each line once
    let mut seen = std::collections::HashSet::new();
    ls.retain(|l| seen.insert([l[0].min(l[1]), l[0].max(l[1])]));
    Ok((pts, ls))
}

/// The points and lines left inside a new boundary (Edit Boundary): points outside go, with their lines.
pub fn retain_within(rings: &[Vec<Point>], points: &[P3], lines: &[[usize; 2]]) -> (Vec<P3>, Vec<[usize; 2]>) {
    let mut to = vec![None; points.len()];
    let mut kept = vec![];
    for (i, p) in points.iter().enumerate() {
        if within(rings, Point::new(p.x, p.y)) { to[i] = Some(kept.len()); kept.push(*p); }
    }
    let ls = lines.iter().filter_map(|l| Some([to.get(l[0]).copied().flatten()?, to.get(l[1]).copied().flatten()?]))
        .filter(|l| { let (a, b) = (kept[l[0]], kept[l[1]]); !leaves(rings, Point::new(a.x, a.y), Point::new(b.x, b.y)) }).collect();
    (kept, ls)
}

/// Simplify: keep about `keep` (a fraction, 0 to 1) of the points, dropping first those the surface would miss
/// least: a point whose height its neighbours already give goes before a peak or a hollow. The ends of split lines
/// stay. Returns the points, the lines (renumbered) and how many points went.
pub fn simplify(points: &[P3], lines: &[[usize; 2]], keep: f64) -> Result<(Vec<P3>, Vec<[usize; 2]>, usize), String> {
    if !(keep.is_finite() && keep > 0.0 && keep <= 1.0) { return Err("Points to keep must be more than 0% and at most 100%.".into()); }
    let before = points.len();
    let target = ((before as f64 * keep).ceil() as usize).max(3.min(before));
    // each point carries where it started, so the lines can be renumbered
    let mut pts: Vec<(usize, P3)> = points.iter().copied().enumerate().collect();
    let ends: std::collections::HashSet<usize> = lines.iter().flatten().copied().collect();
    while pts.len() > target {
        let errs = point_errors(&pts.iter().map(|p| p.1).collect::<Vec<_>>());
        let mut order: Vec<usize> = (0..pts.len()).filter(|&i| !ends.contains(&pts[i].0)).collect();
        if order.is_empty() { break; }
        order.sort_by(|&a, &b| errs[a].0.total_cmp(&errs[b].0));
        // from the lowest errors a batch at a time, never two neighbours in one batch, so each judgement still
        // holds when it is made
        let batch = ((pts.len() - target) / 2).max(1).min(pts.len() - target);
        let mut locked = vec![false; pts.len()];
        let mut drop = vec![false; pts.len()];
        for &i in order.iter().take(batch) {
            if locked[i] { continue; }
            drop[i] = true;
            for &j in &errs[i].1 { locked[j] = true; }
        }
        let mut k = 0;
        pts.retain(|_| { let keep = !drop[k]; k += 1; keep });
    }
    let mut to = vec![usize::MAX; before];
    for (n, p) in pts.iter().enumerate() { to[p.0] = n; }
    let ls = lines.iter().map(|l| [to[l[0]], to[l[1]]]).collect();
    Ok((pts.iter().map(|p| p.1).collect(), ls, before - pts.len()))
}

/// give there, weighted by closeness, and those neighbours.
fn point_errors(pts: &[P3]) -> Vec<(f64, Vec<usize>)> {
    use spade::{DelaunayTriangulation, Point2, Triangulation};
    let mut tri = DelaunayTriangulation::<Point2<f64>>::new();
    let mut of_vertex: Vec<Vec<usize>> = vec![];
    for (i, p) in pts.iter().enumerate() {
        let Ok(h) = tri.insert(Point2::new(p.x, p.y)) else { continue };
        if of_vertex.len() <= h.index() { of_vertex.resize(h.index() + 1, vec![]); }
        of_vertex[h.index()].push(i);
    }
    let mut out = vec![(f64::INFINITY, vec![]); pts.len()];
    for v in tri.vertices() {
        let own = &of_vertex[v.fix().index()];
        let near: Vec<usize> = v.out_edges().flat_map(|e| of_vertex[e.to().fix().index()].clone()).collect();
        for (k, &i) in own.iter().enumerate() {
            // a second point on the same spot adds nothing
            if k > 0 { out[i] = (0.0, near.clone()); continue; }
            let p = pts[i];
            let (mut sw, mut sz) = (0.0, 0.0);
            for &j in &near {
                let d2 = (pts[j].x - p.x).powi(2) + (pts[j].y - p.y).powi(2);
                let w = 1.0 / d2.max(1e-6);
                sw += w;
                sz += w * pts[j].z;
            }
            let err = if sw > 0.0 { (p.z - sz / sw).abs() } else { f64::INFINITY };
            out[i] = (err, near.clone());
        }
    }
    out
}


/// Something that cuts into a surface (Excavate): its footprint in plan (rings by the even-odd rule), the lines where
/// its underside folds, and the underside's height at a plan point in the footprint.
pub struct Cutter {
    pub rings: Vec<Vec<Point>>,
    pub folds: Vec<[Point; 2]>,
    pub under: Box<dyn Fn(Point) -> Option<f64>>,
}

/// How far the ground takes to drop at the edge of an excavation (mm). The surface has one height over each plan
/// point, so the side of the hole leans out over this much.
pub const EXCAVATION_EDGE: f64 = 1.0;

impl Tin {
    /// The height at a plan point, or on the nearest triangle when the point is a hair outside it (a corner made where
    /// a line crosses the boundary).
    pub(crate) fn z_near(&self, p: Point) -> Option<f64> {
        if let Some(z) = self.z_at(p) { return Some(z); }
        let mut best: Option<(f64, f64)> = None;
        for t in &self.tris {
            let (a, b, c) = (self.verts[t[0]], self.verts[t[1]], self.verts[t[2]]);
            let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if det.abs() < 1e-12 { continue; }
            let l1 = ((b[1] - c[1]) * (p.x - c[0]) + (c[0] - b[0]) * (p.y - c[1])) / det;
            let l2 = ((c[1] - a[1]) * (p.x - c[0]) + (a[0] - c[0]) * (p.y - c[1])) / det;
            let l3 = 1.0 - l1 - l2;
            let m = l1.min(l2).min(l3);
            if best.is_none_or(|b| m > b.0) { best = Some((m, l1 * a[2] + l2 * b[2] + l3 * c[2])); }
        }
        best.filter(|b| b.0 > -1e-4).map(|b| b.1)
    }
}

/// Lines `w` outside the edge of a footprint (rings by the even-odd rule): each edge with the footprint on one side
/// only, moved out, and the gaps at the corners between them closed. An edge between two pieces of the footprint
/// (two faces of a roof) has none.
fn outset(rings: &[Vec<Point>], w: f64) -> Vec<[Point; 2]> {
    let mut out = vec![];
    for r in rings {
        let n = r.len();
        let moved: Vec<Option<[Point; 2]>> = (0..n).map(|i| {
            let (a, b) = (r[i], r[(i + 1) % n]);
            let len = a.dist(&b);
            if len < 1e-9 { return None; }
            let (nx, ny) = (-(b.y - a.y) / len * w, (b.x - a.x) / len * w);
            let m = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let (left, right) = (within(rings, Point::new(m.x + nx, m.y + ny)), within(rings, Point::new(m.x - nx, m.y - ny)));
            let s = match (left, right) { (false, true) => 1.0, (true, false) => -1.0, _ => return None };
            Some([Point::new(a.x + nx * s, a.y + ny * s), Point::new(b.x + nx * s, b.y + ny * s)])
        }).collect();
        for i in 0..n {
            let Some(e) = moved[i] else { continue };
            out.push(e);
            if let Some(f) = moved[(i + 1) % n] {
                if e[1].dist(&f[0]) > 1e-9 { out.push([e[1], f[0]]); }
            }
        }
    }
    out
}

/// The surface over `rings` with what the cutters take out of it: within a cutter's footprint, whatever stands above
/// its underside is gone, down to the solid's underside (`bottom`) at most. Returns the new top and the underside's
/// height under each of its corners (as `bottom` has it: excavating leaves the underside where it was).
///
/// The new surface has an edge along every edge of the old one, round each footprint and along each fold of a
/// cutter's underside, and along where the ground passes through an underside, so it is exact; round each footprint
/// the ground drops over EXCAVATION_EDGE.
pub fn excavate(rings: &[Vec<Point>], top: &Tin, bottom: &Tin, cutters: &[Cutter]) -> Option<(Tin, Vec<f64>)> {
    use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
    let cut_at = |p: Point| -> Option<f64> { cutters.iter().filter(|c| within(&c.rings, p)).filter_map(|c| (c.under)(p)).reduce(f64::min) };
    let mut segs: Vec<[Point; 2]> = top.edges().into_iter().map(|([a, b], _)| [top.corner(a), top.corner(b)]).collect();
    for c in cutters {
        for r in &c.rings {
            let n = r.len();
            for i in 0..n { segs.push([r[i], r[(i + 1) % n]]); }
        }
        segs.extend(outset(&c.rings, EXCAVATION_EDGE));
        segs.extend(c.folds.iter().copied());
    }
    let corners: Vec<Point> = (0..top.verts.len()).map(|i| top.corner(i)).collect();
    // the plan triangulation through the old corners and along every line, crossings split
    let mesh = |segs: &[[Point; 2]]| -> Option<(Vec<Point>, Vec<[usize; 3]>)> {
        let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
        for p in &corners { cdt.insert(Point2::new(p.x, p.y)).ok()?; }
        for s in segs {
            if s[0].dist(&s[1]) < 1e-6 { continue; }
            let a = cdt.insert(Point2::new(s[0].x, s[0].y)).ok()?;
            let b = cdt.insert(Point2::new(s[1].x, s[1].y)).ok()?;
            if a != b { cdt.add_constraint_and_split(a, b, |p| p); }
        }
        let mut ps = vec![Point::new(0.0, 0.0); cdt.num_vertices()];
        for v in cdt.vertices() { let p = v.position(); ps[v.fix().index()] = Point::new(p.x, p.y); }
        let tris = cdt.inner_faces().filter_map(|f| {
            let vs = f.vertices();
            let ids = [vs[0].fix().index(), vs[1].fix().index(), vs[2].fix().index()];
            let c = Point::new((ps[ids[0]].x + ps[ids[1]].x + ps[ids[2]].x) / 3.0, (ps[ids[0]].y + ps[ids[1]].y + ps[ids[2]].y) / 3.0);
            inside(rings, c).then_some(ids)
        }).collect();
        Some((ps, tris))
    };
    // the old surface, the underside and the cutters' undersides at a corner
    let heights = |q: Point| -> Option<(f64, f64, Option<f64>)> { Some((top.z_near(q)?, bottom.z_near(q)?, cut_at(q))) };
    let (mut ps, mut tris) = mesh(&segs)?;
    // where the ground passes through a cutter's underside, and where that goes below the solid's underside: in each
    // triangle all three are planes, so each is a straight line
    let h: Vec<_> = ps.iter().map(|&q| heights(q)).collect();
    let mut creases = vec![];
    let gap: [fn((f64, f64, f64)) -> f64; 2] = [|(g, _, u)| g - u, |(_, b, u)| u - b];
    for t in &tris {
        for f in gap {
            let mut at: Vec<Point> = vec![];
            for j in 0..3 {
                let (i, k) = (t[j], t[(j + 1) % 3]);
                let (Some((ga, ba, Some(ua))), Some((gb, bb, Some(ub)))) = (h[i], h[k]) else { continue };
                let (da, db) = (f((ga, ba, ua)), f((gb, bb, ub)));
                if da.abs() < 1e-6 { at.push(ps[i]); continue; }
                if db.abs() >= 1e-6 && (da > 0.0) != (db > 0.0) {
                    let s = da / (da - db);
                    at.push(Point::new(ps[i].x + (ps[k].x - ps[i].x) * s, ps[i].y + (ps[k].y - ps[i].y) * s));
                }
            }
            at.dedup_by(|a, b| a.dist(b) < 1e-6);
            if at.len() == 2 && at[0].dist(&at[1]) > 1e-3 { creases.push([at[0], at[1]]); }
        }
    }
    if !creases.is_empty() {
        segs.extend(creases);
        (ps, tris) = mesh(&segs)?;
    }
    // the corners the triangles use, at their new heights
    let mut remap = vec![usize::MAX; ps.len()];
    let (mut verts, mut bots) = (vec![], vec![]);
    for t in tris.iter_mut() {
        for i in t.iter_mut() {
            if remap[*i] == usize::MAX {
                let (g, b, u) = heights(ps[*i])?;
                remap[*i] = verts.len();
                verts.push([ps[*i].x, ps[*i].y, g.min(u.unwrap_or(f64::INFINITY)).max(b)]);
                bots.push(b);
            }
            *i = remap[*i];
        }
    }
    if tris.is_empty() { return None; }
    Some((Tin { verts, tris }, bots))
}

/// The plane through a planar polygon as z = a·x + b·y + c; None for an upright one.
pub fn plane_through(poly: &[[f64; 3]]) -> Option<[f64; 3]> {
    let n = poly.len();
    if n < 3 { return None; }
    let mut m = [0.0; 3];
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        m[0] += (p[1] - q[1]) * (p[2] + q[2]);
        m[1] += (p[2] - q[2]) * (p[0] + q[0]);
        m[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    if len < 1e-12 || (m[2] / len).abs() < 1e-6 { return None; }
    let (a, b) = (-m[0] / m[2], -m[1] / m[2]);
    Some([a, b, poly[0][2] - a * poly[0][0] - b * poly[0][1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: f64) -> Vec<Vec<Point>> {
        vec![vec![Point::new(0.0, 0.0), Point::new(s, 0.0), Point::new(s, s), Point::new(0.0, s)]]
    }
    fn p(x: f64, y: f64, z: f64) -> P3 {
        P3 { x, y, z }
    }

    #[test]
    fn a_split_line_folds_the_surface_along_it() {
        let rings = sq(10000.0);
        // a ridge corner to corner, both ways round, so the line decides which diagonal the surface takes
        for (a, b) in [((0.0, 10000.0), (10000.0, 0.0)), ((0.0, 0.0), (10000.0, 10000.0))] {
            let pts = [[a.0, a.1, 1000.0], [b.0, b.1, 1000.0]];
            let tin = triangulate(&rings, &pts, &[[0, 1]], 0.0).unwrap();
            // on the ridge the surface is at the ridge height all the way along, and the point is on a fold
            let (z, _, fold) = tin.sample(Point::new(5000.0, 5000.0)).unwrap();
            assert!((z - 1000.0).abs() < 1e-6 && fold, "{z}");
            let (z, n, fold) = tin.sample(Point::new(2000.0, 7000.0)).unwrap();
            assert!(!fold && z < 1000.0 && n[2] > 0.0);
        }
    }

    #[test]
    fn tidy_breaks_crossing_lines_and_lines_over_points() {
        let rings = sq(10000.0);
        let pts = vec![p(0.0, 5000.0, 0.0), p(10000.0, 5000.0, 1000.0), p(5000.0, 0.0, 0.0), p(5000.0, 10000.0, 0.0), p(2500.0, 5000.0, 300.0)];
        let (pts, ls) = tidy(&rings, pts, vec![[0, 1], [2, 3]], "toposolid").unwrap();
        // the first line broken at the point on it, and a new point where they cross, at the height the first line
        // gives there (its piece beyond that point runs from 300 to 1000)
        assert_eq!(pts.len(), 6);
        let x = pts[5];
        let z = 300.0 + 700.0 / 3.0;
        assert!((x.x - 5000.0).abs() < 1e-6 && (x.y - 5000.0).abs() < 1e-6 && (x.z - z).abs() < 1e-6, "{x:?}");
        assert_eq!(ls.len(), 5, "{ls:?}");
        // a line out of the boundary is refused; a point given way to a later one takes its lines along
        let notch = vec![vec![Point::new(0.0, 0.0), Point::new(10000.0, 0.0), Point::new(10000.0, 10000.0), Point::new(5000.0, 3000.0), Point::new(0.0, 10000.0)]];
        assert!(tidy(&notch, vec![p(1000.0, 8000.0, 0.0), p(9000.0, 8000.0, 0.0)], vec![[0, 1]], "toposolid").is_err());
        let (pts, ls) = tidy(&rings, vec![p(1000.0, 1000.0, 0.0), p(9000.0, 9000.0, 0.0), p(1000.5, 1000.0, 50.0)], vec![[0, 1]], "toposolid").unwrap();
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[ls[0][0]].z + pts[ls[0][1]].z, 50.0);
    }

    #[test]
    fn simplify_keeps_the_ends_of_split_lines() {
        let mut pts = vec![];
        for i in 0..=10 { for j in 0..=10 { pts.push(p(i as f64 * 1000.0, j as f64 * 1000.0, 0.0)); } }
        let (kept, ls, removed) = simplify(&pts, &[[12, 13]], 0.1).unwrap();
        assert!(removed > 90);
        assert_eq!(kept[ls[0][0]], pts[12]);
        assert_eq!(kept[ls[0][1]], pts[13]);
    }

    fn box_cutter(x0: f64, y0: f64, x1: f64, y1: f64, under: impl Fn(Point) -> f64 + 'static) -> Cutter {
        Cutter { rings: vec![vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]], folds: vec![], under: Box::new(move |p| Some(under(p))) }
    }
    fn lowered(t: &Tin, by: f64) -> Tin {
        Tin { verts: t.verts.iter().map(|v| [v[0], v[1], v[2] - by]).collect(), tris: t.tris.clone() }
    }
    fn volume(t: &Tin, bot: &[f64]) -> f64 {
        t.tris.iter().map(|k| {
            let (a, b, c) = (t.verts[k[0]], t.verts[k[1]], t.verts[k[2]]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0 * k.iter().map(|&i| t.verts[i][2] - bot[i]).sum::<f64>() / 3.0
        }).sum()
    }

    #[test]
    fn a_cutter_takes_out_the_ground_above_its_underside() {
        let rings = sq(10000.0);
        let top = triangulate(&rings, &[], &[], 0.0).unwrap();
        let bottom = lowered(&top, 1000.0);
        let (t, bot) = excavate(&rings, &top, &bottom, &[box_cutter(3000.0, 3000.0, 7000.0, 7000.0, |_| -300.0)]).unwrap();
        assert!((t.z_at(Point::new(5000.0, 5000.0)).unwrap() + 300.0).abs() < 1e-6);
        assert!(t.z_at(Point::new(2000.0, 5000.0)).unwrap().abs() < 1e-6);
        // the hole's side drops within a millimetre of the footprint
        assert!(t.z_at(Point::new(2999.0, 5000.0)).unwrap().abs() < 1e-6);
        assert!((t.z_at(Point::new(3000.0, 5000.0)).unwrap() + 300.0).abs() < 1e-6);
        let removed = 1e11 - volume(&t, &bot);
        assert!((removed - 4.8e9).abs() < 4.8e9 * 0.002, "{removed}");
        // deeper than the solid: a hole right through, the underside left where it was
        let (t, bot) = excavate(&rings, &top, &bottom, &[box_cutter(3000.0, 3000.0, 7000.0, 7000.0, |_| -3000.0)]).unwrap();
        let c = Point::new(5000.0, 5000.0);
        assert!((t.z_at(c).unwrap() + 1000.0).abs() < 1e-6);
        assert!(bot.iter().all(|&b| (b + 1000.0).abs() < 1e-6));
    }

    #[test]
    fn on_a_slope_only_the_ground_above_the_underside_goes() {
        let rings = sq(10000.0);
        // the ground rises 1 in 10 to the east; a flat underside at 500 crosses it halfway
        let pts = [[0.0, 0.0, 0.0], [10000.0, 0.0, 1000.0], [10000.0, 10000.0, 1000.0], [0.0, 10000.0, 0.0]];
        let top = triangulate(&rings, &pts, &[], 0.0).unwrap();
        let bottom = lowered(&top, 1000.0);
        let (t, _) = excavate(&rings, &top, &bottom, &[box_cutter(2000.0, 2000.0, 8000.0, 8000.0, |_| 500.0)]).unwrap();
        for (x, z) in [(3000.0, 300.0), (4000.0, 400.0), (5000.0, 500.0), (6000.0, 500.0), (7900.0, 500.0), (9000.0, 900.0)] {
            for y in [2500.0, 5000.0, 7500.0] {
                let got = t.z_at(Point::new(x, y)).unwrap();
                assert!((got - z).abs() < 1e-6, "at {x}, {y}: {got} not {z}");
            }
        }
    }
}
