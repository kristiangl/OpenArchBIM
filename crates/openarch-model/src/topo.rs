//! Toposolids: the ground as a solid. A closed boundary sketched on a level, a set of surveyed or added points
//! inside it, and a type whose layers give the solid its thickness below the surface.
//!
//! The top surface is a constrained Delaunay triangulation (TIN) of the boundary's corners and the points, with
//! the boundary's edges kept as edges. Point heights are stored above the unmodified top (the level plus Height
//! Offset From Level), so moving the level or the offset moves the whole surface. A boundary corner with no point
//! on it sits at the unmodified top. The solid follows the top down by the type's thickness, unless a layer of the
//! type is variable: then the underside stays flat at the unmodified top less the thickness, and the variable
//! layer takes up the difference.
//!
//! Contour lines come from the type's contour sets, measured as shared elevations (from the shared site origin,
//! which the Survey Point sets).

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::element::{default_coarse_fill, IfcParams, Layer, LayerFunction, TypeInfo};
use crate::geom::Region;
use crate::site::P3;
use crate::styles::PatternGraphics;
use crate::{Document, Element, Id, Point};

/// The most points a toposolid takes, as from a survey file. Larger files are thinned to this.
pub const MAX_POINTS: usize = 20_000;

/// One row of a type's Contour Display: lines from `start` to `stop` every `interval` (Multiple Values), or one
/// line at `start` (Single Value). Elevations are shared elevations, in mm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContourSet {
    pub start: f64,
    pub stop: f64,
    pub interval: f64,
    /// Single Value: one line at `start`.
    #[serde(default)]
    pub single: bool,
    /// The Primary Contours subcategory (otherwise Secondary Contours).
    #[serde(default)]
    pub primary: bool,
    #[serde(default = "yes")]
    pub visible: bool,
}

fn yes() -> bool { true }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToposolidType {
    pub name: String,
    /// Layers from the surface down.
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default = "default_coarse_fill")]
    pub coarse_fill: PatternGraphics,
    #[serde(default)]
    pub info: TypeInfo,
    #[serde(default = "default_contours")]
    pub contours: Vec<ContourSet>,
}

impl ToposolidType {
    pub fn thickness_mm(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness_mm).sum()
    }

    /// The variable layer's index, when the type has one.
    pub fn variable_layer(&self) -> Option<usize> {
        self.layers.iter().position(|l| l.variable)
    }

    /// Each layer's top and bottom height at a place where the surface is at `top` and the underside at `bottom`:
    /// layers above the variable one hang from the top, those below it stand on the bottom, and the variable one
    /// fills between. With no variable layer every layer hangs from the top.
    pub fn layer_spans(&self, top: f64, bottom: f64) -> Vec<(f64, f64)> {
        let v = self.variable_layer();
        let mut out = vec![];
        let mut off = 0.0;
        for (i, l) in self.layers.iter().enumerate() {
            let below: f64 = self.layers[i + 1..].iter().map(|l| l.thickness_mm).sum();
            let span = match v {
                Some(v) if i > v => (bottom + below + l.thickness_mm, bottom + below),
                Some(v) if i == v => (top - off, (bottom + below).min(top - off)),
                _ => (top - off, top - off - l.thickness_mm),
            };
            out.push(span);
            off += l.thickness_mm;
        }
        out
    }
}

/// Secondary contours every 500 mm and primary ones every 2500 mm.
pub fn default_contours() -> Vec<ContourSet> {
    vec![
        ContourSet { start: 0.0, stop: 1_000_000.0, interval: 500.0, single: false, primary: false, visible: true },
        ContourSet { start: 0.0, stop: 1_000_000.0, interval: 2500.0, single: false, primary: true, visible: true },
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Toposolid {
    pub toposolid_type: Id,
    pub level: Id,
    /// Closed loops of curves; loops inside others are holes.
    pub loops: Vec<Vec<Curve>>,
    /// Height Offset From Level: where the unmodified top sits.
    #[serde(default)]
    pub offset_mm: f64,
    /// Points on the surface: plan position, and height above the unmodified top.
    #[serde(default)]
    pub points: Vec<P3>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub ifc: IfcParams,
}

impl Toposolid {
    pub fn new(toposolid_type: Id, level: Id, loops: Vec<Vec<Curve>>, offset_mm: f64) -> Toposolid {
        Toposolid { toposolid_type, level, loops, offset_mm, points: vec![], pinned: false, comments: String::new(), mark: String::new(), ifc: IfcParams { guid: crate::element::new_ifc_guid(), ..Default::default() } }
    }

    /// The boundary as polygons (arcs and splines broken into short chords).
    pub fn polygons(&self, tol: f64) -> Vec<Vec<Point>> {
        self.loops.iter().map(|l| crate::curve::loop_polygon(l, tol)).filter(|p| p.len() >= 3).collect()
    }

    pub fn regions(&self, tol: f64) -> Vec<Region> {
        crate::geom::regions(&self.polygons(tol))
    }
}

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

/// Inside the region rings by the even-odd rule.
fn inside(rings: &[Vec<Point>], p: Point) -> bool {
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

/// The triangulated surface over `rings` (the boundary and its holes), through `points` (absolute heights). A
/// corner of the boundary with no point within a millimetre takes `base_z`.
pub fn triangulate(rings: &[Vec<Point>], points: &[[f64; 3]], base_z: f64) -> Option<Tin> {
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
    for p in points {
        if !(p[0].is_finite() && p[1].is_finite() && p[2].is_finite()) { continue; }
        // a point on the boundary line counts; one outside is left out
        if inside(rings, Point::new(p[0], p[1])) || rings.iter().any(|r| on_ring(r, Point::new(p[0], p[1]))) {
            put(&mut cdt, p[0], p[1], Some(p[2]));
        }
    }
    for r in rings {
        let hs: Vec<_> = r.iter().filter_map(|q| put(&mut cdt, q.x, q.y, None)).collect();
        for i in 0..hs.len() {
            let (a, b) = (hs[i], hs[(i + 1) % hs.len()]);
            if a != b && cdt.can_add_constraint(a, b) { cdt.add_constraint(a, b); }
        }
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

fn on_ring(r: &[Point], p: Point) -> bool {
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

/// For each point, how far its height is from what its neighbours (in a triangulation of the points alone) would
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

/// The convex hull of plan points, counter-clockwise.
pub fn convex_hull(pts: &[Point]) -> Vec<Point> {
    let mut p: Vec<Point> = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6);
    if p.len() < 3 { return p; }
    let cross = |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut lower: Vec<Point> = vec![];
    for &q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0.0 { lower.pop(); }
        lower.push(q);
    }
    let mut upper: Vec<Point> = vec![];
    for &q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0.0 { upper.pop(); }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Read a points file: one point per line, fields split by commas, tabs, semicolons or spaces. Three numbers are
/// x, y, z; four or more are a point number then x, y, z (the usual survey layout, extra columns such as a code
/// are ignored). Lines that do not read (headings, blanks) are skipped. `scale` turns file units into mm.
pub fn read_points(text: &str, scale: f64) -> Vec<[f64; 3]> {
    let mut out = vec![];
    for line in text.lines() {
        let nums: Vec<f64> = line.split(|c: char| c == ',' || c == '\t' || c == ';' || c.is_whitespace()).filter(|s| !s.is_empty()).map_while(|s| s.parse::<f64>().ok()).collect();
        let xyz = match nums.len() {
            3 => [nums[0], nums[1], nums[2]],
            n if n >= 4 => [nums[1], nums[2], nums[3]],
            _ => continue,
        };
        if xyz.iter().all(|v| v.is_finite()) { out.push([xyz[0] * scale, xyz[1] * scale, xyz[2] * scale]); }
    }
    out
}

/// Keep about `max` of the points, evenly through the list.
fn thin(points: Vec<[f64; 3]>, max: usize) -> Vec<[f64; 3]> {
    if points.len() <= max { return points; }
    let step = points.len() as f64 / max as f64;
    (0..max).map(|k| points[(k as f64 * step) as usize]).collect()
}

/// Where Add Point's height is measured from, or where a points file's coordinates start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopoBase {
    InternalOrigin,
    ProjectBasePoint,
    SurveyPoint,
    CurrentLevel,
}

impl Document {
    pub fn toposolid_types(&self) -> Vec<(Id, &ToposolidType)> {
        self.iter().filter_map(|(i, e)| if let Element::ToposolidType(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn toposolid_type(&self, id: Id) -> Option<&ToposolidType> {
        match self.get(id)? { Element::ToposolidType(t) => Some(t), _ => None }
    }

    pub fn toposolids(&self) -> Vec<(Id, &Toposolid)> {
        self.iter().filter_map(|(i, e)| if let Element::Toposolid(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn toposolid(&self, id: Id) -> Option<&Toposolid> {
        match self.get(id)? { Element::Toposolid(t) => Some(t), _ => None }
    }

    pub fn toposolid_thickness(&self, t: &Toposolid) -> f64 {
        self.toposolid_type(t.toposolid_type).map_or(0.0, |ty| ty.thickness_mm())
    }

    /// The unmodified top: the level plus Height Offset From Level (absolute, mm).
    pub fn toposolid_base_z(&self, t: &Toposolid) -> f64 {
        self.level(t.level).map_or(0.0, |l| l.elevation_mm) + t.offset_mm
    }

    /// The top surface, triangulated.
    pub fn toposolid_tin(&self, t: &Toposolid) -> Option<Tin> {
        let base = self.toposolid_base_z(t);
        let pts: Vec<[f64; 3]> = t.points.iter().map(|p| [p.x, p.y, base + p.z]).collect();
        triangulate(&t.polygons(20.0), &pts, base)
    }

    /// The underside's height under each corner of the surface. With a variable layer it stays flat at the
    /// unmodified top less the type's thickness, following the top down only where the ground dips so far that the
    /// other layers would not fit; without one it is the top less the thickness.
    pub fn toposolid_bottoms(&self, t: &Toposolid, tin: &Tin) -> Vec<f64> {
        let ty = self.toposolid_type(t.toposolid_type);
        let th = ty.map_or(0.0, |ty| ty.thickness_mm());
        let fixed = ty.and_then(|ty| ty.variable_layer().map(|v| th - ty.layers[v].thickness_mm));
        let flat = self.toposolid_base_z(t) - th;
        tin.verts.iter().map(|v| match fixed { Some(f) => flat.min(v[2] - f), None => v[2] - th }).collect()
    }

    /// The solid's volume (mm³): over each triangle, its plan area by the mean depth at its corners.
    pub fn toposolid_volume(&self, t: &Toposolid) -> f64 {
        let Some(tin) = self.toposolid_tin(t) else { return 0.0 };
        let bot = self.toposolid_bottoms(t, &tin);
        tin.tris.iter().map(|k| {
            let (a, b, c) = (tin.verts[k[0]], tin.verts[k[1]], tin.verts[k[2]]);
            let area = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0;
            let depth = k.iter().map(|&i| (tin.verts[i][2] - bot[i]).max(0.0)).sum::<f64>() / 3.0;
            area * depth
        }).sum()
    }

    /// Where a floor plan's cut plane at height `z` passes through the ground: the pieces of plan where the surface
    /// is above the plane and the underside below it (filled with the cut pattern), and the lines round them (where
    /// the top or the underside meets the plane, and the boundary in between).
    pub fn toposolid_plan_cut(&self, t: &Toposolid, z: f64) -> (Vec<Vec<Point>>, Vec<[Point; 2]>) {
        let Some(tin) = self.toposolid_tin(t) else { return (vec![], vec![]) };
        let bot = self.toposolid_bottoms(t, &tin);
        let boundary: std::collections::HashSet<[usize; 2]> = tin.boundary_edges().into_iter().map(|[a, b]| [a.min(b), a.max(b)]).collect();
        let (mut polys, mut lines) = (vec![], vec![]);
        // a corner of a piece: plan position, surface and underside heights, and the triangle edge it lies on (if any)
        type V = (f64, f64, f64, f64, Option<[usize; 2]>);
        let clip = |poly: Vec<V>, g: &dyn Fn(&V) -> f64| -> Vec<V> {
            let n = poly.len();
            let mut out = vec![];
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                let (ga, gb) = (g(&a), g(&b));
                if ga >= 0.0 { out.push(a); }
                if (ga >= 0.0) != (gb >= 0.0) {
                    let s = ga / (ga - gb);
                    // the edge out of a corner where the piece comes back inside runs on along the old edge; out of
                    // one where it leaves, along the plane
                    let edge = if ga < 0.0 { a.4 } else { None };
                    out.push((a.0 + (b.0 - a.0) * s, a.1 + (b.1 - a.1) * s, a.2 + (b.2 - a.2) * s, a.3 + (b.3 - a.3) * s, edge));
                }
            }
            out
        };
        for k in &tin.tris {
            let tops = k.map(|i| tin.verts[i][2]);
            let bots = k.map(|i| bot[i]);
            // ground that only touches the plane (a flat top at the cut height, say) is not cut
            if tops.iter().all(|&h| h <= z + 0.01) || bots.iter().all(|&h| h >= z - 0.01) { continue; }
            // each corner carries the triangle edge that runs out of it, so the boundary can be told among the edges
            let tagged: Vec<V> = (0..3).map(|j| {
                let (a, b) = (k[j], k[(j + 1) % 3]);
                (tin.verts[a][0], tin.verts[a][1], tops[j], bots[j], Some([a.min(b), a.max(b)]))
            }).collect();
            let p1 = clip(tagged, &|v: &V| v.2 - z);
            if p1.len() < 3 { continue; }
            let p2 = clip(p1, &|v: &V| z - v.3);
            let ring: Vec<Point> = p2.iter().map(|v| Point::new(v.0, v.1)).collect();
            if p2.len() < 3 || crate::geom::signed_area(&ring).abs() < 1.0 { continue; }
            let m = p2.len();
            for i in 0..m {
                let (a, b) = (p2[i], p2[(i + 1) % m]);
                let on_top = (a.2 - z).abs() < 1e-6 && (b.2 - z).abs() < 1e-6;
                let on_bottom = (a.3 - z).abs() < 1e-6 && (b.3 - z).abs() < 1e-6;
                let on_boundary = a.4.is_some_and(|e| boundary.contains(&e));
                if (on_top || on_bottom || on_boundary) && (a.0 - b.0).hypot(a.1 - b.1) > 0.01 {
                    lines.push([Point::new(a.0, a.1), Point::new(b.0, b.1)]);
                }
            }
            polys.push(ring);
        }
        (polys, lines)
    }

    /// The height a base stands at (absolute, mm).
    pub fn topo_base_z(&self, base: TopoBase, level: Id) -> f64 {
        let loc = self.project_location().map(|(_, l)| l.clone()).unwrap_or_default();
        match base {
            TopoBase::InternalOrigin => 0.0,
            TopoBase::ProjectBasePoint => loc.base.z,
            TopoBase::SurveyPoint => loc.shared_origin.z,
            TopoBase::CurrentLevel => self.level(level).map_or(0.0, |l| l.elevation_mm),
        }
    }

    /// The plan position a base stands at (internal coordinates, mm).
    fn topo_base_xy(&self, base: TopoBase) -> Point {
        let loc = self.project_location().map(|(_, l)| l.clone()).unwrap_or_default();
        match base {
            TopoBase::ProjectBasePoint => Point::new(loc.base.x, loc.base.y),
            TopoBase::SurveyPoint => Point::new(loc.shared_origin.x, loc.shared_origin.y),
            _ => Point::new(0.0, 0.0),
        }
    }

    /// Contour lines of a toposolid by its type: (primary, line pieces in plan). Where a primary and a secondary
    /// set give the same height, the primary line is drawn.
    pub fn toposolid_contours(&self, t: &Toposolid) -> Vec<(bool, Vec<[Point; 2]>)> {
        let (Some(ty), Some(tin)) = (self.toposolid_type(t.toposolid_type), self.toposolid_tin(t)) else { return vec![] };
        let zero = self.topo_base_z(TopoBase::SurveyPoint, t.level);
        let (lo, hi) = tin.z_range();
        let mut heights: std::collections::BTreeMap<i64, bool> = Default::default();
        for s in ty.contours.iter().filter(|s| s.visible) {
            let mut put = |h: f64| {
                if h + zero >= lo - 1e-6 && h + zero <= hi + 1e-6 {
                    let k = (h * 1000.0).round() as i64;
                    let e = heights.entry(k).or_insert(false);
                    *e |= s.primary;
                }
            };
            if s.single {
                put(s.start);
            } else if s.interval >= 1.0 {
                // from the first line inside the surface's range, to keep the count to what is drawn
                let first = ((lo - zero - s.start) / s.interval).ceil().max(0.0);
                let mut k = first;
                loop {
                    let h = s.start + k * s.interval;
                    if h > s.stop + 1e-6 || h + zero > hi + 1e-6 { break; }
                    put(h);
                    k += 1.0;
                    if k - first > 5000.0 { break; }
                }
            }
        }
        heights.into_iter().map(|(k, primary)| (primary, tin.contour(k as f64 / 1000.0 + zero))).filter(|(_, l)| !l.is_empty()).collect()
    }

    /// The starter type, and the Earth material, added to a project that has none.
    pub fn ensure_toposolid_types(&mut self) -> Result<(), String> {
        if !self.toposolid_types().is_empty() { return Ok(()); }
        let materials: Vec<_> = crate::styles::starter_materials().into_iter().filter(|m| m.name == "Earth" && self.material(&m.name).is_none()).collect();
        self.transact("Toposolid types", |tx| {
            for m in materials { tx.add(Element::Material(m)); }
            for t in starter_toposolid_types() { tx.add(Element::ToposolidType(t)); }
            Ok(())
        })
    }

    fn check_toposolid(&self, t: &Toposolid) -> Result<(), String> {
        if self.toposolid_type(t.toposolid_type).is_none() { return Err("unknown toposolid type".into()); }
        if self.level(t.level).is_none() { return Err("unknown level".into()); }
        if !t.offset_mm.is_finite() { return Err("the height offset must be a number".into()); }
        if t.points.iter().any(|p| !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite())) { return Err("a point must be numbers".into()); }
        self.check_floor_sketch(&t.loops.concat())?;
        Ok(())
    }

    /// Create from Sketch: a flat toposolid over the sketch, its top `offset_mm` above the level.
    pub fn add_toposolid_sketch(&mut self, toposolid_type: Id, level: Id, curves: Vec<Curve>, offset_mm: f64) -> Result<Id, String> {
        let loops = self.check_floor_sketch(&curves)?;
        let t = Toposolid::new(toposolid_type, level, loops, offset_mm);
        self.check_toposolid(&t)?;
        self.transact("Create toposolid", |tx| Ok(tx.add(Element::Toposolid(t))))
    }

    /// Create from Import: a toposolid through surveyed points (absolute, internal coordinates), its boundary
    /// their convex hull, on `level` with no offset. More than `MAX_POINTS` are thinned.
    pub fn add_toposolid_points(&mut self, toposolid_type: Id, level: Id, points: Vec<[f64; 3]>) -> Result<Id, String> {
        let points = thin(points, MAX_POINTS);
        let hull = convex_hull(&points.iter().map(|p| Point::new(p[0], p[1])).collect::<Vec<_>>());
        if hull.len() < 3 || crate::geom::signed_area(&hull).abs() < 1.0 {
            return Err("The points need to cover an area: at least three that are not in a line.".into());
        }
        let far = points.iter().map(|p| p[0].hypot(p[1])).fold(0.0, f64::max);
        if far > 20_000_000.0 {
            return Err(format!("The points are up to {:.0} km from the model. Set the Survey Point's shared coordinates near the site and import relative to it.", far / 1e6));
        }
        let lz = self.level(level).ok_or("unknown level")?.elevation_mm;
        let curves: Vec<Curve> = (0..hull.len()).map(|i| Curve::line(hull[i], hull[(i + 1) % hull.len()])).collect();
        let loops = self.check_floor_sketch(&curves)?;
        let mut t = Toposolid::new(toposolid_type, level, loops, 0.0);
        t.points = points.iter().map(|p| P3 { x: p[0], y: p[1], z: p[2] - lz }).collect();
        self.check_toposolid(&t)?;
        self.transact("Create toposolid from import", |tx| Ok(tx.add(Element::Toposolid(t))))
    }

    /// A points file read as `base` coordinates (the survey point: shared coordinates), in internal coordinates.
    pub fn topo_points_from_file(&self, text: &str, scale: f64, base: TopoBase) -> Result<Vec<[f64; 3]>, String> {
        let pts = read_points(text, scale);
        if pts.len() < 3 { return Err("No points read from the file: each line needs x, y and z (or a point number then x, y and z).".into()); }
        let o = self.topo_base_xy(base);
        let oz = match base { TopoBase::CurrentLevel => 0.0, b => self.topo_base_z(b, 0) };
        Ok(pts.into_iter().map(|p| [p[0] + o.x, p[1] + o.y, p[2] + oz]).collect())
    }

    /// Add Point: a point at a plan position, at an absolute height. Outside the boundary it is refused.
    pub fn add_toposolid_point(&mut self, id: Id, at: Point, z_abs: f64) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        self.refuse_pinned(&[id], "edited")?;
        if !(at.x.is_finite() && at.y.is_finite() && z_abs.is_finite()) { return Err("the point must be numbers".into()); }
        let rings = t.polygons(20.0);
        if !inside(&rings, at) && !rings.iter().any(|r| on_ring(r, at)) { return Err("Click inside the toposolid's boundary.".into()); }
        let z = z_abs - self.toposolid_base_z(&t);
        // a point on top of another replaces it
        t.points.retain(|p| (p.x - at.x).hypot(p.y - at.y) > 1.0);
        t.points.push(P3 { x: at.x, y: at.y, z });
        self.transact("Add point", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// Edit Boundary's Finish: the new sketch; points outside it are dropped.
    pub fn set_toposolid_sketch(&mut self, id: Id, curves: Vec<Curve>) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        self.refuse_pinned(&[id], "edited")?;
        t.loops = self.check_floor_sketch(&curves)?;
        let rings = t.polygons(20.0);
        t.points.retain(|p| { let q = Point::new(p.x, p.y); inside(&rings, q) || rings.iter().any(|r| on_ring(r, q)) });
        self.check_toposolid(&t)?;
        self.transact("Edit toposolid boundary", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// Modify Sub Elements: the toposolid's points replaced (plan positions, heights above the unmodified top). A point
    /// outside the boundary is refused; two on the same spot keep the later one.
    pub fn set_toposolid_points(&mut self, id: Id, points: Vec<P3>) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        self.refuse_pinned(&[id], "edited")?;
        let rings = t.polygons(20.0);
        let mut kept: Vec<P3> = vec![];
        for p in points.into_iter().rev() {
            if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) { return Err("a point must be numbers".into()); }
            let q = Point::new(p.x, p.y);
            if !inside(&rings, q) && !rings.iter().any(|r| on_ring(r, q)) { return Err("A point must stay inside the toposolid's boundary.".into()); }
            if !kept.iter().any(|k| (k.x - p.x).hypot(k.y - p.y) <= 1.0) { kept.push(p); }
        }
        kept.reverse();
        if kept == t.points { return Ok(()); }
        t.points = kept;
        self.transact("Modify toposolid points", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// Simplify Toposolid: keep about `keep` (a fraction, 0 to 1) of the points, dropping first those the surface
    /// would miss least: a point whose height its neighbours already give goes before a peak or a hollow. Returns
    /// how many were dropped.
    pub fn simplify_toposolid(&mut self, id: Id, keep: f64) -> Result<usize, String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        self.refuse_pinned(&[id], "edited")?;
        if !(keep.is_finite() && keep > 0.0 && keep <= 1.0) { return Err("Points to keep must be more than 0% and at most 100%.".into()); }
        let before = t.points.len();
        let target = ((before as f64 * keep).ceil() as usize).max(3.min(before));
        let mut pts = t.points.clone();
        while pts.len() > target {
            let errs = point_errors(&pts);
            let mut order: Vec<usize> = (0..pts.len()).collect();
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
        let removed = before - pts.len();
        if removed == 0 { return Ok(0); }
        t.points = pts;
        self.transact("Simplify toposolid", |tx| tx.update(id, Element::Toposolid(t)))?;
        Ok(removed)
    }

    pub fn patch_toposolid(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        let t: Toposolid = crate::doc::apply_patch(&old, patch)?;
        if old.pinned && t.pinned && (t.loops != old.loops || t.level != old.level || t.offset_mm != old.offset_mm || t.points != old.points) {
            return Err("Unpin the toposolid before moving it.".into());
        }
        self.check_toposolid(&t)?;
        self.transact("Toposolid properties", |tx| tx.update(id, Element::Toposolid(t)))
    }
}

/// One face of a toposolid as an elevation sees it: corners (top corners by index, bottom ones offset by the count),
/// its outward normal and which part it is (0 the surface, 1 a side, 2 the underside).
struct Face {
    corners: Vec<usize>,
    normal: [f64; 3],
    part: u8,
}

impl Document {
    /// A toposolid as an elevation, or the part of a section beyond its cut plane, sees it: the faces turned toward
    /// the viewer, filled without outlines (so the triangles do not show), and the lines where it turns away or
    /// changes from surface to side.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn toposolid_view_items(&self, id: Id, t: &Toposolid, look: Point, u_of: &dyn Fn(Point) -> f64, d_of: &dyn Fn(Point) -> f64, visible: &dyn Fn(f64, f64) -> bool, sl: Option<&crate::section::Slice>) -> Vec<crate::site::ElevItem> {
        use crate::site::ElevItem;
        let Some(tin) = self.toposolid_tin(t) else { return vec![] };
        let th = self.toposolid_thickness(t);
        let n = tin.verts.len();
        let bot = self.toposolid_bottoms(t, &tin);
        let corner = |i: usize| if i < n { tin.verts[i] } else { let v = tin.verts[i - n]; [v[0], v[1], bot[i - n]] };
        let color = self.toposolid_type(t.toposolid_type).and_then(|ty| ty.layers.first()).and_then(|l| self.material(&l.material)).map_or([150, 128, 100], |m| m.graphics.shading_color);
        let normal = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let m = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-12);
            [m[0] / l, m[1] / l, m[2] / l]
        };
        let mut faces: Vec<Face> = vec![];
        for k in &tin.tris {
            faces.push(Face { corners: k.to_vec(), normal: normal(corner(k[0]), corner(k[1]), corner(k[2])), part: 0 });
            if th > 0.0 {
                let b = [k[0] + n, k[2] + n, k[1] + n];
                faces.push(Face { corners: b.to_vec(), normal: normal(corner(b[0]), corner(b[1]), corner(b[2])), part: 2 });
            }
        }
        if th > 0.0 {
            for [i, j] in tin.boundary_edges() {
                let (a, b) = (tin.verts[i], tin.verts[j]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l = dx.hypot(dy).max(1e-12);
                faces.push(Face { corners: vec![i, i + n, j + n, j], normal: [dy / l, -dx / l, 0.0], part: 1 });
            }
        }
        // turned toward the viewer: against the direction of view (a face seen edge on is not drawn)
        let front: Vec<bool> = faces.iter().map(|f| f.normal[0] * look.x + f.normal[1] * look.y < -1e-6).collect();
        let mut depth_of = vec![f64::INFINITY; faces.len()];
        let mut items = vec![];
        for (k, f) in faces.iter().enumerate() {
            if !front[k] { continue; }
            let poly: Vec<[f64; 3]> = f.corners.iter().map(|&i| corner(i)).collect();
            let poly = match sl { Some(s) => s.clip_ring3(&poly), None => poly };
            if poly.len() < 3 { continue; }
            let ds: Vec<f64> = poly.iter().map(|q| d_of(Point::new(q[0], q[1]))).collect();
            let (dmin, dmax) = (ds.iter().cloned().fold(f64::INFINITY, f64::min), ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
            if !visible(dmin, dmax) { continue; }
            let uz: Vec<[f64; 2]> = poly.iter().map(|q| [u_of(Point::new(q[0], q[1])), q[2]]).collect();
            // lit from above: steeper faces a little darker, sides darker still
            let shade = if f.part == 1 { 0.8 } else { 0.82 + 0.18 * f.normal[2].abs() };
            let c = |v: u8| (v as f64 * shade).clamp(0.0, 255.0) as u8;
            depth_of[k] = dmin;
            items.push(ElevItem { id, category: "Topography", depth: dmin, polys: vec![uz], fill: [c(color[0]), c(color[1]), c(color[2])], stroke: false, ..Default::default() });
        }
        // the lines: between a face turned toward the viewer and one turned away, between the surface, the sides and
        // the underside, and at a corner of the sides
        let mut by_edge: std::collections::BTreeMap<[usize; 2], Vec<usize>> = Default::default();
        for (k, f) in faces.iter().enumerate() {
            let m = f.corners.len();
            for j in 0..m {
                let (a, b) = (f.corners[j], f.corners[(j + 1) % m]);
                by_edge.entry([a.min(b), a.max(b)]).or_default().push(k);
            }
        }
        let (near_cut, far_cut) = match sl {
            Some(s) => (s.plane, if s.far == crate::section::FarClip::NoClip { f64::INFINITY } else { s.depth }),
            None => (f64::NEG_INFINITY, f64::INFINITY),
        };
        let lerp = |p: [f64; 3], q: [f64; 3], s: f64| [p[0] + (q[0] - p[0]) * s, p[1] + (q[1] - p[1]) * s, p[2] + (q[2] - p[2]) * s];
        for ([a, b], fs) in by_edge {
            if !fs.iter().any(|&k| front[k]) { continue; }
            let parts: Vec<u8> = fs.iter().map(|&k| faces[k].part).collect();
            let mixed = parts.iter().any(|&p| p != parts[0]);
            let turns = fs.iter().any(|&k| !front[k]) || fs.len() == 1;
            let corner_of_sides = !mixed && parts[0] == 1 && fs.len() == 2 && {
                let (p, q) = (faces[fs[0]].normal, faces[fs[1]].normal);
                p[0] * q[0] + p[1] * q[1] < 0.87
            };
            if !(mixed || turns || corner_of_sides) { continue; }
            let (mut p, mut q) = (corner(a), corner(b));
            // keep to the part of the view between the cut plane and the far clip
            let (dp, dq) = (d_of(Point::new(p[0], p[1])), d_of(Point::new(q[0], q[1])));
            if dp.max(dq) < near_cut || dp.min(dq) > far_cut { continue; }
            if (dq - dp).abs() > 1e-9 && (near_cut.is_finite() || far_cut.is_finite()) {
                let (s0, s1) = ((near_cut - dp) / (dq - dp), (far_cut - dp) / (dq - dp));
                let (smin, smax) = (s0.min(s1).max(0.0), s0.max(s1).min(1.0));
                if smin >= smax { continue; }
                let (p0, q0) = (lerp(p, q, smin), lerp(p, q, smax));
                p = p0;
                q = q0;
            }
            let depth = fs.iter().filter(|&&k| front[k]).map(|&k| depth_of[k]).fold(f64::INFINITY, f64::min);
            if !depth.is_finite() { continue; }
            let line = [[u_of(Point::new(p[0], p[1])), p[2]], [u_of(Point::new(q[0], q[1])), q[2]]];
            items.push(ElevItem { id, category: "Topography", depth: depth - 0.01, lines: vec![line], stroke: false, ..Default::default() });
        }
        items
    }

    /// Where a section's plane cuts a toposolid: each stretch inside the boundary, as the profile of the surface
    /// with a band per layer below it.
    pub(crate) fn toposolid_cut_items(&self, id: Id, t: &Toposolid, sl: &crate::section::Slice, plane: f64) -> Vec<crate::site::ElevItem> {
        use crate::site::ElevItem;
        let Some(tin) = self.toposolid_tin(t) else { return vec![] };
        let polys = t.polygons(20.0);
        let rings: Vec<&[Point]> = polys.iter().map(|p| p.as_slice()).collect();
        let ty = self.toposolid_type(t.toposolid_type).filter(|ty| !ty.layers.is_empty());
        let bands: Vec<(Option<&str>, f64, bool)> = match ty {
            Some(ty) => ty.layers.iter().map(|l| (Some(l.material.as_str()), l.thickness_mm, l.is_void)).collect(),
            None => vec![(None, 2.0, false)],
        };
        let th: f64 = bands.iter().map(|b| b.1).sum();
        // the underside under a point of the profile: flat with a variable layer (as toposolid_bottoms), else the top
        // less the thickness
        let fixed = ty.and_then(|ty| ty.variable_layer().map(|v| th - ty.layers[v].thickness_mm));
        let flat = self.toposolid_base_z(t) - th;
        let bottom = |top: f64| match fixed { Some(f) => flat.min(top - f), None => top - th };
        let spans = |top: f64| -> Vec<(f64, f64)> {
            match ty { Some(ty) => ty.layer_spans(top, bottom(top)), None => vec![(top, top - 2.0)] }
        };
        let edges = tin.edges();
        let mut out = vec![];
        for cut in sl.cuts(&rings, plane) {
            let mut prof: Vec<[f64; 2]> = vec![];
            for ([i, j], _) in &edges {
                let (a, b) = (tin.verts[*i], tin.verts[*j]);
                let (da, db) = (sl.d(Point::new(a[0], a[1])) - plane, sl.d(Point::new(b[0], b[1])) - plane);
                if (da < 0.0) == (db < 0.0) { continue; }
                let s = da / (da - db);
                let p = Point::new(a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s);
                let u = sl.u(p);
                if u < cut.u0 - 1e-6 || u > cut.u1 + 1e-6 { continue; }
                prof.push([u, a[2] + (b[2] - a[2]) * s]);
            }
            for (u, p) in [(cut.u0, cut.p0), (cut.u1, cut.p1)] {
                if !prof.iter().any(|q| (q[0] - u).abs() < 1e-3) {
                    if let Some(z) = tin.z_at(p) { prof.push([u, z]); }
                }
            }
            prof.sort_by(|a, b| a[0].total_cmp(&b[0]));
            prof.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-6);
            if prof.len() < 2 { continue; }
            // where a flat underside starts to follow the top down
            if let Some(f) = fixed {
                let mut with = vec![prof[0]];
                for w in prof.windows(2) {
                    let (ga, gb) = (w[0][1] - f - flat, w[1][1] - f - flat);
                    if (ga < 0.0) != (gb < 0.0) {
                        let s = ga / (ga - gb);
                        with.push([w[0][0] + (w[1][0] - w[0][0]) * s, w[0][1] + (w[1][1] - w[0][1]) * s]);
                    }
                    with.push(w[1]);
                }
                prof = with;
            }
            let at: Vec<Vec<(f64, f64)>> = prof.iter().map(|q| spans(q[1])).collect();
            for (k, (mat, th, void)) in bands.iter().enumerate() {
                if *void || *th <= 0.0 { continue; }
                let mut poly: Vec<[f64; 2]> = prof.iter().zip(&at).map(|(q, s)| [q[0], s[k].0]).collect();
                poly.extend(prof.iter().zip(&at).rev().map(|(q, s)| [q[0], s[k].1]));
                let fill = mat.and_then(|m| self.material(m)).map_or([150, 128, 100], |m| m.graphics.shading_color);
                out.push(ElevItem { id, category: "Topography", depth: -1.0 + 0.003, polys: vec![poly], fill, cut: true, material: mat.map(str::to_string), ..Default::default() });
            }
        }
        out
    }
}

pub(crate) fn starter_toposolid_types() -> Vec<ToposolidType> {
    vec![ToposolidType { name: "Site - Earth 1000".into(), layers: vec![Layer::new("Earth", 1000.0, LayerFunction::Structure, true)], coarse_fill: default_coarse_fill(), info: TypeInfo::default(), contours: default_contours() }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f64) -> Vec<Curve> {
        let p = [Point::new(0.0, 0.0), Point::new(s, 0.0), Point::new(s, s), Point::new(0.0, s)];
        (0..4).map(|i| Curve::line(p[i], p[(i + 1) % 4])).collect()
    }

    fn setup() -> (Document, Id, Id) {
        let mut d = Document::new_project();
        d.ensure_toposolid_types().unwrap();
        let ty = d.toposolid_types()[0].0;
        let level = d.levels()[0].0;
        (d, ty, level)
    }

    #[test]
    fn a_sketched_toposolid_is_flat_until_a_point_is_added() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), -200.0).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        let tin = d.toposolid_tin(&t).unwrap();
        assert_eq!(tin.tris.len(), 2);
        assert!((tin.z_at(Point::new(5000.0, 5000.0)).unwrap() - (lz - 200.0)).abs() < 1e-6);
        // a mound in the middle: the boundary stays at the unmodified top
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), lz + 1800.0).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        let tin = d.toposolid_tin(&t).unwrap();
        assert_eq!(tin.tris.len(), 4);
        assert!((tin.z_at(Point::new(5000.0, 5000.0)).unwrap() - (lz + 1800.0)).abs() < 1e-6);
        assert!((tin.z_at(Point::new(2500.0, 5000.0)).unwrap() - (lz + 800.0)).abs() < 1e-6);
        assert!(d.add_toposolid_point(id, Point::new(20_000.0, 0.0), 0.0).is_err());
        // one undo step each
        d.undo();
        assert!(d.toposolid(id).unwrap().points.is_empty());
    }

    #[test]
    fn contours_follow_the_type_and_primary_wins() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), -lz).unwrap();
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), 3000.0).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        let c = d.toposolid_contours(&t);
        // 500 to 2500: the corners touch 0 and the top touches 3000, which draw nothing; 2500 is primary
        let primaries = c.iter().filter(|(p, _)| *p).count();
        assert_eq!(primaries, 1);
        assert_eq!(c.len(), 5);
        // the 1500 line is a ring round the top: four pieces
        let mid = &c[2].1;
        assert_eq!(mid.len(), 4);
    }

    #[test]
    fn a_points_file_makes_a_toposolid_over_their_hull() {
        let (mut d, ty, level) = setup();
        let text = "PointNo,Easting,Northing,RL,Code\n1,0,0,10.0,NS\n2,20,0,11.0,NS\n3,20,15,12.5,NS\n4,0,15,10.5,NS\n5,10,7,13.0,TOB\n";
        let pts = d.topo_points_from_file(text, 1000.0, TopoBase::InternalOrigin).unwrap();
        assert_eq!(pts.len(), 5);
        let id = d.add_toposolid_points(ty, level, pts).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        let tin = d.toposolid_tin(&t).unwrap();
        assert_eq!(tin.verts.len(), 5);
        assert!((tin.z_at(Point::new(10_000.0, 7000.0)).unwrap() - 13_000.0).abs() < 1e-6);
        assert!((tin.plan_area() - 300e6).abs() < 1.0);
        assert!(d.add_toposolid_points(ty, level, vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 2.0, 0.0]]).is_err());
    }

    #[test]
    fn a_concave_boundary_keeps_its_notch() {
        let (mut d, ty, level) = setup();
        let p = [Point::new(0.0, 0.0), Point::new(10_000.0, 0.0), Point::new(10_000.0, 10_000.0), Point::new(5000.0, 3000.0), Point::new(0.0, 10_000.0)];
        let curves: Vec<Curve> = (0..5).map(|i| Curve::line(p[i], p[(i + 1) % 5])).collect();
        let id = d.add_toposolid_sketch(ty, level, curves, 0.0).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        let tin = d.toposolid_tin(&t).unwrap();
        assert!(tin.z_at(Point::new(5000.0, 8000.0)).is_none());
        assert!((tin.plan_area() - (100e6 - 5000.0 * 7000.0)).abs() < 1.0);
    }

    #[test]
    fn a_variable_layer_keeps_the_underside_flat() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), lz + 1800.0).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        assert!((d.toposolid_volume(&t) - 1e11).abs() < 1e3, "constant thickness: {}", d.toposolid_volume(&t));
        // 300 of fixed earth over 700 variable: the bottom stays at the unmodified top less 1000
        let mut tt = d.toposolid_type(ty).unwrap().clone();
        tt.layers = vec![Layer::new("Earth", 300.0, LayerFunction::Finish1, true), Layer::new("Earth", 700.0, LayerFunction::Structure, true)];
        tt.layers[1].variable = true;
        d.transact("variable", |tx| tx.update(ty, Element::ToposolidType(tt.clone()))).unwrap();
        let tin = d.toposolid_tin(&t).unwrap();
        assert!(d.toposolid_bottoms(&t, &tin).iter().all(|b| (b - (lz - 1000.0)).abs() < 1e-6));
        assert!((d.toposolid_volume(&t) - 1.6e11).abs() < 1e3, "{}", d.toposolid_volume(&t));
        let spans = tt.layer_spans(lz + 1800.0, lz - 1000.0);
        assert_eq!(spans, vec![(lz + 1800.0, lz + 1500.0), (lz + 1500.0, lz - 1000.0)]);
        // a hollow deeper than the fixed layers allow: the underside follows it there
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), lz - 1500.0).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        let tin = d.toposolid_tin(&t).unwrap();
        let bot = d.toposolid_bottoms(&t, &tin);
        let mid = tin.verts.iter().position(|v| (v[0] - 5000.0).abs() < 1e-6 && (v[1] - 5000.0).abs() < 1e-6).unwrap();
        assert!((bot[mid] - (lz - 1800.0)).abs() < 1e-6);
    }

    #[test]
    fn a_plan_cuts_the_ground_above_its_cut_plane() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        assert!(d.toposolid_plan_cut(&t, lz + 1200.0).0.is_empty(), "flat ground below the plane is not cut");
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), lz + 2700.0).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        let (polys, lines) = d.toposolid_plan_cut(&t, lz + 1200.0);
        // the ring where the top is above 1200 and the underside (1000 down) below it
        let area: f64 = polys.iter().map(|p| crate::geom::signed_area(p).abs()).sum();
        let (r1, r2) = (1.0 - 2200.0 / 2700.0, 1.0 - 1200.0 / 2700.0);
        assert!((area - 100e6 * (r2 * r2 - r1 * r1)).abs() < 1e3, "{area}");
        assert_eq!(lines.len(), 8, "the two contours at the plane, one piece per triangle each");
        // cut lower, through the boundary: the sides show as lines too
        let (_, lines) = d.toposolid_plan_cut(&t, lz - 500.0);
        assert!(lines.len() > 4, "{lines:?}");
    }

    #[test]
    fn points_move_and_simplify_keeps_the_peaks() {
        let (mut d, ty, level) = setup();
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        let mut pts = vec![];
        for i in 0..=10 {
            for j in 0..=10 {
                pts.push(P3 { x: i as f64 * 1000.0, y: j as f64 * 1000.0, z: i as f64 * 100.0 });
            }
        }
        pts.push(P3 { x: 4500.0, y: 4500.0, z: 5000.0 });
        d.set_toposolid_points(id, pts.clone()).unwrap();
        assert_eq!(d.toposolid(id).unwrap().points.len(), 122);
        let mut out = pts.clone();
        out[0].x = -50.0;
        assert!(d.set_toposolid_points(id, out).is_err(), "outside the boundary");
        let removed = d.simplify_toposolid(id, 0.1).unwrap();
        let left = &d.toposolid(id).unwrap().points;
        assert_eq!(removed, 122 - left.len());
        assert!(left.len() <= 13 && left.len() >= 3, "{}", left.len());
        assert!(left.iter().any(|p| p.z == 5000.0), "the peak stays");
        d.undo();
        assert_eq!(d.toposolid(id).unwrap().points.len(), 122);
    }
}
