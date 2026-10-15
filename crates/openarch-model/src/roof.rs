//! Roofs (Architecture > Build > Roof > Roof by Footprint).
//!
//! A roof is a sketched footprint: closed loops of curves, drawn in a plan view like a floor
//! boundary. Each edge of the footprint can define a slope (a plane rising inward from that
//! edge) or not (a gable end). The roof surface follows from the edges by the weighted straight
//! skeleton in `skeleton.rs`: hips, ridges and valleys are where the planes meet. Loops inside
//! other loops are openings and slope too, so a roof round a courtyard rises away from both.
//!
//! The edge data is stored with the footprint (defines slope, slope, overhang the edge was
//! picked with) so editing the footprint later gives the same controls as drawing it. The
//! geometry is derived: `Document::roof_geometry` rebuilds it from the stored edges. The shape
//! is an enum so roofs by extrusion and by face can be added without reshaping stored roofs.
//!
//! Heights are measured up from the roof's base level plus `base_offset_mm`, which is where the
//! top surface of the roof meets the edges of the footprint (the eaves).

use serde::{Deserialize, Serialize};

use crate::curve::{self, Curve};
use crate::doc::{apply_patch, Document};
use crate::element::*;
use crate::geom::{Point, Region};
use crate::skeleton;

/// The slope a new roof edge starts with, in degrees. the common default is 30; 22.5 is the
/// common pitch for Australian tile and sheet roofs.
pub const DEFAULT_SLOPE_DEG: f64 = 22.5;

fn yes() -> bool {
    true
}

fn default_slope() -> f64 {
    DEFAULT_SLOPE_DEG
}

/// One edge of the footprint: a curve and what the roof does along it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoofEdge {
    pub curve: Curve,
    /// Defines Slope. When off the roof ends in a vertical face along this edge.
    #[serde(default = "yes")]
    pub defines_slope: bool,
    /// The slope of the plane over this edge, degrees from horizontal.
    #[serde(default = "default_slope")]
    pub slope_deg: f64,
    /// The overhang this edge was picked from a wall with. Kept so Edit Footprint shows it.
    #[serde(default)]
    pub overhang_mm: f64,
}

impl RoofEdge {
    pub fn new(curve: Curve) -> RoofEdge {
        RoofEdge { curve, defines_slope: true, slope_deg: DEFAULT_SLOPE_DEG, overhang_mm: 0.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum RoofShape {
    /// Closed loops of edges. A loop inside another is an opening.
    Footprint { loops: Vec<Vec<RoofEdge>> },
}

/// How the end of a rafter is cut at the eave (Rafter Cut).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RafterCut {
    #[default]
    Plumb,
    TwoCutPlumb,
    Square,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Roof {
    pub roof_type: Id,
    pub level: Id,
    pub shape: RoofShape,
    /// Height of the eaves above the base level (Base Offset From Level).
    #[serde(default)]
    pub base_offset_mm: f64,
    /// Geometry above this level (plus offset) is cut off flat, as a Cutoff Level does.
    #[serde(default)]
    pub cutoff_level: Option<Id>,
    #[serde(default)]
    pub cutoff_offset_mm: f64,
    #[serde(default)]
    pub rafter_cut: RafterCut,
    #[serde(default)]
    pub fascia_depth_mm: f64,
    #[serde(default = "yes")]
    pub room_bounding: bool,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub phase_created: Option<Id>,
    #[serde(default)]
    pub phase_demolished: Option<Id>,
    #[serde(default)]
    pub ifc: IfcParams,
    /// Pinned elements refuse move, delete and flip until unpinned.
    #[serde(default)]
    pub pinned: bool,
}

impl Roof {
    pub fn new(roof_type: Id, level: Id, loops: Vec<Vec<RoofEdge>>) -> Roof {
        Roof {
            roof_type,
            level,
            shape: RoofShape::Footprint { loops },
            base_offset_mm: 0.0,
            cutoff_level: None,
            cutoff_offset_mm: 0.0,
            rafter_cut: RafterCut::Plumb,
            fascia_depth_mm: 0.0,
            room_bounding: true,
            mark: String::new(),
            comments: String::new(),
            phase_created: None,
            phase_demolished: None,
            ifc: IfcParams { guid: new_ifc_guid(), ..IfcParams::default() },
            pinned: false,
        }
    }

    pub fn loops(&self) -> &[Vec<RoofEdge>] {
        match &self.shape {
            RoofShape::Footprint { loops } => loops,
        }
    }

    /// Every edge of every loop, in order.
    pub fn edges(&self) -> Vec<&RoofEdge> {
        self.loops().iter().flatten().collect()
    }

    /// The footprint as plain curves (what the sketch session edits).
    pub fn curves(&self) -> Vec<Curve> {
        self.edges().into_iter().map(|e| e.curve.clone()).collect()
    }

    /// The slope shared by every slope-defining edge, if they agree.
    pub fn common_slope(&self) -> Option<f64> {
        let mut it = self.edges().into_iter().filter(|e| e.defines_slope).map(|e| e.slope_deg);
        let first = it.next()?;
        it.all(|s| (s - first).abs() < 1e-9).then_some(first)
    }

    pub fn has_slope_edges(&self) -> bool {
        self.edges().iter().any(|e| e.defines_slope)
    }
}

/// One plane of the roof surface (or a vertical gable face), in absolute millimetres.
#[derive(Debug, Clone)]
pub struct RoofFace {
    pub poly: Vec<[f64; 3]>,
    pub vertical: bool,
    pub slope_deg: f64,
    /// Index of the sketch edge (counting every edge of every loop) this plane grew from.
    pub edge: usize,
    /// Horizontal direction pointing away from the roof at this face's edge.
    pub outward: [f64; 2],
}

/// A slope symbol for a slope-defining edge: where to draw it and which way is up the slope.
#[derive(Debug, Clone)]
pub struct SlopeMark {
    pub at: Point,
    pub up: [f64; 2],
    pub deg: f64,
    pub edge: usize,
}

#[derive(Debug, Clone, Default)]
pub struct RoofGeometry {
    pub faces: Vec<RoofFace>,
    /// Footprint regions with no sloping edge at all: flat roofs, at the base height.
    pub flat: Vec<Region>,
    /// Flat tops left where a cutoff level removed the geometry above it.
    pub caps: Vec<Vec<[f64; 3]>>,
    /// The footprint as drawn, one region per outline with its openings.
    pub regions: Vec<Region>,
    /// Hips, ridges, valleys and the tops of gable ends, in plan.
    pub ridges: Vec<[Point; 2]>,
    pub marks: Vec<SlopeMark>,
    pub base_z: f64,
    pub top_z: f64,
}

impl RoofGeometry {
    /// Surface area of the top of the roof, mm squared.
    pub fn surface_area_mm2(&self) -> f64 {
        let faces: f64 = self.faces.iter().filter(|f| !f.vertical).map(|f| poly_area_3d(&f.poly)).sum();
        let flat: f64 = self.flat.iter().map(|r| r.area()).sum();
        let caps: f64 = self.caps.iter().map(|c| poly_area_3d(c)).sum();
        faces + flat + caps
    }
}

/// Area of a planar 3D polygon (Newell's method).
pub fn poly_area_3d(p: &[[f64; 3]]) -> f64 {
    let n = p.len();
    let (mut sx, mut sy, mut sz) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        sx += (a[1] - b[1]) * (a[2] + b[2]);
        sy += (a[2] - b[2]) * (a[0] + b[0]);
        sz += (a[0] - b[0]) * (a[1] + b[1]);
    }
    (sx * sx + sy * sy + sz * sz).sqrt() / 2.0
}

/// Keep the part of a planar polygon at or below (`below`) or above a height.
pub fn clip_z(poly: &[[f64; 3]], z: f64, below: bool) -> Vec<[f64; 3]> {
    let inside = |q: &[f64; 3]| if below { q[2] <= z + 1e-9 } else { q[2] >= z - 1e-9 };
    let mut out: Vec<[f64; 3]> = vec![];
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (ia, ib) = (inside(&a), inside(&b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let t = (z - a[2]) / (b[2] - a[2]);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, z]);
        }
    }
    out
}

const TOL: f64 = 2.0;

struct LoopPoly {
    pts: Vec<Point>,
    /// For the segment from `pts[i]` to `pts[i + 1]`: defines slope, slope, sketch edge.
    attr: Vec<(bool, f64, usize)>,
}

impl LoopPoly {
    fn area(&self) -> f64 {
        crate::geom::signed_area(&self.pts)
    }

    /// Run the loop the other way round; each segment keeps its own attributes.
    fn reverse(&mut self) {
        let n = self.pts.len();
        self.pts.reverse();
        let old = self.attr.clone();
        for j in 0..n {
            self.attr[j] = old[(n + n - 2 - j) % n];
        }
    }
}

fn loop_poly(l: &[RoofEdge], first_edge: usize) -> LoopPoly {
    let mut pts: Vec<Point> = vec![];
    let mut attr = vec![];
    for (j, e) in l.iter().enumerate() {
        let t = e.curve.tessellate(TOL);
        for k in 0..t.len().saturating_sub(1) {
            pts.push(t[k]);
            attr.push((e.defines_slope, e.slope_deg, first_edge + j));
        }
    }
    LoopPoly { pts, attr }
}

impl Document {
    pub fn roofs(&self) -> Vec<(Id, &Roof)> {
        self.iter().filter_map(|(id, e)| if let Element::Roof(r) = e { Some((id, r)) } else { None }).collect()
    }

    pub fn roof(&self, id: Id) -> Option<&Roof> {
        match self.get(id) {
            Some(Element::Roof(r)) => Some(r),
            _ => None,
        }
    }

    pub fn roof_types(&self) -> Vec<(Id, &RoofType)> {
        self.iter().filter_map(|(id, e)| if let Element::RoofType(t) = e { Some((id, t)) } else { None }).collect()
    }

    pub fn roof_type(&self, id: Id) -> Option<&RoofType> {
        match self.get(id) {
            Some(Element::RoofType(t)) => Some(t),
            _ => None,
        }
    }

    /// The loops of a roof sketch: the curves are grouped and checked as Finish Edit Mode does,
    /// and each ends up with the settings of the edge it came from.
    pub fn check_roof_sketch(&self, edges: &[RoofEdge]) -> Result<Vec<Vec<RoofEdge>>, String> {
        let curves: Vec<Curve> = edges.iter().map(|e| e.curve.clone()).collect();
        let loops = self.check_floor_sketch(&curves)?;
        let mut out: Vec<Vec<RoofEdge>> = vec![];
        let mut used = vec![false; edges.len()];
        for l in loops {
            let mut row = vec![];
            for c in l {
                let rev = c.reversed();
                let Some(i) = (0..edges.len()).find(|&i| !used[i] && (edges[i].curve == c || edges[i].curve == rev)) else {
                    return Err("the sketch changed while it was being checked".into());
                };
                used[i] = true;
                row.push(RoofEdge { curve: c, ..edges[i].clone() });
            }
            out.push(row);
        }
        for e in out.iter().flatten() {
            if !(0.0..90.0).contains(&e.slope_deg) {
                return Err("a slope has to be between 0 and 90 degrees".into());
            }
        }
        let probe = Roof::new(0, 0, out.clone());
        let g = self.roof_geometry_at(&probe, 0.0, None).ok_or("the footprint could not be turned into a roof")?;
        if g.faces.is_empty() && g.flat.is_empty() {
            return Err("the footprint could not be turned into a roof: check it for crossing lines".into());
        }
        Ok(out)
    }

    /// Make a roof from footprint edges (Finish Edit Mode on Create Roof Footprint).
    pub fn add_roof_sketch(&mut self, roof_type: Id, level: Id, edges: Vec<RoofEdge>) -> Result<Id, String> {
        if self.roof_type(roof_type).is_none() {
            return Err("unknown roof type".into());
        }
        if self.level(level).is_none() {
            return Err("unknown level".into());
        }
        let loops = self.check_roof_sketch(&edges)?;
        let r = Roof::new(roof_type, level, loops);
        self.transact("Create roof", |tx| Ok(tx.add(Element::Roof(r))))
    }

    /// Replace the footprint of a roof (Edit Footprint, Finish Edit Mode), one undo step.
    pub fn set_roof_sketch(&mut self, id: Id, edges: Vec<RoofEdge>) -> Result<(), String> {
        let Some(old) = self.roof(id).cloned() else { return Err("not a roof".into()) };
        self.refuse_pinned(&[id], "edited")?;
        let loops = self.check_roof_sketch(&edges)?;
        let mut r = old;
        r.shape = RoofShape::Footprint { loops };
        self.transact("Edit roof footprint", |tx| tx.update(id, Element::Roof(r)))
    }

    pub fn set_roof_type(&mut self, id: Id, roof_type: Id) -> Result<(), String> {
        let Some(mut r) = self.roof(id).cloned() else { return Err("not a roof".into()) };
        if self.roof_type(roof_type).is_none() {
            return Err("unknown roof type".into());
        }
        if r.roof_type == roof_type {
            return Ok(());
        }
        r.roof_type = roof_type;
        self.transact("Change roof type", |tx| tx.update(id, Element::Roof(r)))
    }

    /// Change instance properties with a JSON patch. `slope_deg` sets the slope of every
    /// slope-defining edge, as a Slope property does.
    pub fn patch_roof(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(old) = self.roof(id).cloned() else { return Err("not a roof".into()) };
        let mut patch = patch.clone();
        let slope = patch.as_object_mut().and_then(|o| o.remove("slope_deg")).and_then(|v| v.as_f64());
        if let Some(o) = patch.as_object_mut() {
            // the footprint is edited in sketch mode
            o.remove("shape");
        }
        let mut new: Roof = apply_patch(&old, &patch)?;
        if let Some(s) = slope {
            if !(0.0..90.0).contains(&s) {
                return Err("a slope has to be between 0 and 90 degrees".into());
            }
            let RoofShape::Footprint { loops } = &mut new.shape;
            for e in loops.iter_mut().flatten().filter(|e| e.defines_slope) {
                e.slope_deg = s;
            }
        }
        if new == old {
            return Ok(());
        }
        if old.pinned && new.pinned && new != (Roof { pinned: true, mark: old.mark.clone(), comments: old.comments.clone(), ..new.clone() }) {
            return Err("the roof is pinned; unpin it first".into());
        }
        if self.level(new.level).is_none() {
            return Err("unknown level".into());
        }
        if let Some(c) = new.cutoff_level {
            if self.level(c).is_none() {
                return Err("unknown cutoff level".into());
            }
        }
        if new.fascia_depth_mm < 0.0 {
            return Err("the fascia depth cannot be negative".into());
        }
        self.transact("Change roof", |tx| tx.update(id, Element::Roof(new)))
    }

    /// The roof surface, worked out from the footprint edges.
    pub fn roof_geometry(&self, r: &Roof) -> Option<RoofGeometry> {
        let base = self.level(r.level)?.elevation_mm + r.base_offset_mm;
        let cutoff = r.cutoff_level.and_then(|c| self.level(c)).map(|l| l.elevation_mm + r.cutoff_offset_mm);
        self.roof_geometry_at(r, base, cutoff)
    }

    fn roof_geometry_at(&self, r: &Roof, base_z: f64, cutoff: Option<f64>) -> Option<RoofGeometry> {
        // each loop as a polygon with the settings of every segment
        let mut polys: Vec<LoopPoly> = vec![];
        let mut first = 0;
        for l in r.loops() {
            polys.push(loop_poly(l, first));
            first += l.len();
        }
        if polys.iter().any(|p| p.pts.len() < 3) {
            return None;
        }
        let shapes: Vec<Vec<Point>> = polys.iter().map(|p| p.pts.clone()).collect();
        let depth = curve::loop_depths(&shapes);
        let mut g = RoofGeometry { base_z, top_z: base_z, ..RoofGeometry::default() };
        let outer_ids: Vec<usize> = (0..polys.len()).filter(|&i| depth[i] % 2 == 0).collect();
        for &o in &outer_ids {
            // the openings directly inside this outline
            let holes: Vec<usize> = (0..polys.len())
                .filter(|&h| {
                    depth[h] == depth[o] + 1
                        && curve::point_in_polygon(polys[h].pts[0], &polys[o].pts)
                        && !outer_ids.iter().any(|&o2| o2 != o && depth[o2] == depth[o] && polys[o2].area().abs() < polys[o].area().abs() && curve::point_in_polygon(polys[h].pts[0], &polys[o2].pts))
                })
                .collect();
            let mut set: Vec<LoopPoly> = vec![];
            for (k, i) in std::iter::once(o).chain(holes.iter().copied()).enumerate() {
                let mut lp = LoopPoly { pts: polys[i].pts.clone(), attr: polys[i].attr.clone() };
                // the material is on the left of every edge: outline counter-clockwise, openings clockwise
                if (lp.area() < 0.0) == (k == 0) {
                    lp.reverse();
                }
                set.push(lp);
            }
            g.regions.push(Region { outer: set[0].pts.clone(), holes: set[1..].iter().map(|l| l.pts.clone()).collect() });
            let sloped = set.iter().any(|l| l.attr.iter().any(|a| a.0));
            if !sloped {
                g.flat.push(g.regions.last().unwrap().clone());
                continue;
            }
            let loops: Vec<Vec<Point>> = set.iter().map(|l| l.pts.clone()).collect();
            let speeds: Vec<Vec<f64>> = set.iter().map(|l| l.attr.iter().map(|a| if a.0 { 1.0 / a.1.clamp(0.1, 89.9).to_radians().tan() } else { 0.0 }).collect()).collect();
            for f in skeleton::skeleton(&loops, &speeds) {
                let (_, deg, edge) = set[f.loop_idx].attr[f.edge_idx];
                let (a, b) = (set[f.loop_idx].pts[f.edge_idx], set[f.loop_idx].pts[(f.edge_idx + 1) % set[f.loop_idx].pts.len()]);
                let len = a.dist(&b).max(1e-9);
                // the interior is on the left, so outward is to the right of the edge
                let outward = [(b.y - a.y) / len, -(b.x - a.x) / len];
                g.faces.push(RoofFace { poly: f.poly.iter().map(|q| [q[0], q[1], q[2] + base_z]).collect(), vertical: f.vertical, slope_deg: if f.vertical { 90.0 } else { deg }, edge, outward });
            }
            // slope symbols: the middle segment of each slope-defining edge
            let mut by_edge: std::collections::BTreeMap<usize, Vec<(Point, [f64; 2], f64)>> = Default::default();
            for l in &set {
                let n = l.pts.len();
                for i in 0..n {
                    let (a, b) = (l.pts[i], l.pts[(i + 1) % n]);
                    let len = a.dist(&b);
                    if len < 1e-6 || !l.attr[i].0 {
                        continue;
                    }
                    by_edge.entry(l.attr[i].2).or_default().push((Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), [-(b.y - a.y) / len, (b.x - a.x) / len], l.attr[i].1));
                }
            }
            for (edge, segs) in by_edge {
                let (at, up, deg) = segs[segs.len() / 2];
                g.marks.push(SlopeMark { at, up, deg, edge });
            }
        }
        // the geometry above the cutoff level is removed and the cut left flat
        if let Some(zc) = cutoff {
            let mut faces = vec![];
            for f in std::mem::take(&mut g.faces) {
                let above = clip_z(&f.poly, zc, false);
                if !f.vertical && above.len() >= 3 && poly_area_3d(&above) > 1.0 {
                    g.caps.push(above.iter().map(|q| [q[0], q[1], zc]).collect());
                }
                let below = clip_z(&f.poly, zc, true);
                if below.len() >= 3 && (poly_area_3d(&below) > 1.0 || f.vertical && below.iter().any(|q| q[2] < zc - 1e-6)) {
                    faces.push(RoofFace { poly: below, ..f });
                }
            }
            g.faces = faces;
        }
        // lines in plan: every face edge that is not on the footprint itself
        let mut seen: std::collections::BTreeSet<(i64, i64, i64, i64)> = Default::default();
        let key = |p: &[f64; 3]| ((p[0] * 4.0).round() as i64, (p[1] * 4.0).round() as i64);
        for f in &g.faces {
            let n = f.poly.len();
            for i in 0..n {
                let (a, b) = (f.poly[i], f.poly[(i + 1) % n]);
                if (a[2] - base_z).abs() < 1e-6 && (b[2] - base_z).abs() < 1e-6 {
                    continue;
                }
                if a[0].hypot(a[1]) < 0.0 || ((a[0] - b[0]).hypot(a[1] - b[1])) < 1e-6 {
                    continue;
                }
                let (ka, kb) = (key(&a), key(&b));
                let k = if ka <= kb { (ka.0, ka.1, kb.0, kb.1) } else { (kb.0, kb.1, ka.0, ka.1) };
                if seen.insert(k) {
                    g.ridges.push([Point::new(a[0], a[1]), Point::new(b[0], b[1])]);
                }
            }
        }
        g.top_z = g.faces.iter().flat_map(|f| f.poly.iter().map(|q| q[2])).chain(g.caps.iter().flatten().map(|q| q[2])).fold(base_z, f64::max);
        Some(g)
    }

    /// Area of the sloping top surface, mm squared.
    pub fn roof_area_mm2(&self, r: &Roof) -> f64 {
        self.roof_geometry(r).map_or(0.0, |g| g.surface_area_mm2())
    }

    /// Roof types for projects that have none (new projects, and projects saved before roofs).
    pub fn ensure_roof_defaults(&mut self) {
        if !self.roof_types().is_empty() {
            return;
        }
        let _ = self.transact("Roof defaults", |tx| {
            tx.add_roof_types();
            Ok(())
        });
        self.clear_history();
    }
}

fn roof_layer(material: &str, t: f64, function: LayerFunction) -> Layer {
    Layer { material: material.into(), thickness_mm: t, is_void: false, function, core: function == LayerFunction::Structure }
}

impl crate::doc::Tx<'_> {
    pub(crate) fn add_roof_types(&mut self) {
        use LayerFunction::{Finish1 as F1, Structure as St, Substrate as Sb};
        for (name, layers) in [
            ("Metal Sheet Roof on Rafters", vec![roof_layer("Roof Sheeting", 1.0, F1), roof_layer("Timber Batten", 40.0, Sb), roof_layer("Timber Rafter", 90.0, St)]),
            ("Concrete Tile Roof on Rafters", vec![roof_layer("Roof Tile", 20.0, F1), roof_layer("Timber Batten", 40.0, Sb), roof_layer("Timber Rafter", 90.0, St)]),
            ("Concrete Roof Slab 200", vec![roof_layer("Concrete", 200.0, St)]),
            ("Steel Canopy Roof 150", vec![roof_layer("Roof Sheeting", 1.0, F1), roof_layer("Steel Purlin", 150.0, St)]),
        ] {
            self.add(Element::RoofType(RoofType { name: name.into(), layers, coarse_fill: default_coarse_fill() }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<RoofEdge> {
        let c = [p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)];
        (0..4).map(|i| RoofEdge::new(Curve::line(c[i], c[(i + 1) % 4]))).collect()
    }

    fn setup() -> (Document, Id, Id) {
        let d = Document::new_project();
        let (lvl, rt) = (d.levels()[1].0, d.roof_types()[0].0);
        (d, lvl, rt)
    }

    #[test]
    fn a_new_project_has_roof_types_and_materials_for_them() {
        let d = Document::new_project();
        assert!(d.roof_types().len() >= 3);
        for (_, t) in d.roof_types() {
            for l in &t.layers {
                assert!(d.material(&l.material).is_some(), "material {} exists", l.material);
            }
        }
        assert!(Document::new_project().undo_name().is_none());
    }

    #[test]
    fn a_footprint_makes_a_hip_roof_that_rises_from_the_level() {
        let (mut d, lvl, rt) = setup();
        let id = d.add_roof_sketch(rt, lvl, rect(0., 0., 10000., 6000.)).unwrap();
        let g = d.roof_geometry(d.roof(id).unwrap()).unwrap();
        assert_eq!(g.faces.len(), 4);
        // Level 2 is 3000 up, the ridge is half the width times tan(22.5)
        assert!((g.top_z - (3000.0 + 3000.0 * 22.5f64.to_radians().tan())).abs() < 1e-6);
        assert!((g.base_z - 3000.0).abs() < 1e-9);
        // the hips and ridge show in plan: 4 hips and a ridge
        assert_eq!(g.ridges.len(), 5);
        assert_eq!(g.marks.len(), 4);
        // the surface is bigger than the footprint by 1/cos(slope)
        let flat = 10000.0 * 6000.0;
        assert!((g.surface_area_mm2() - flat / 22.5f64.to_radians().cos()).abs() < 1.0);
    }

    #[test]
    fn edges_without_slope_make_gable_ends() {
        let (mut d, lvl, rt) = setup();
        let mut e = rect(0., 0., 10000., 6000.);
        e[1].defines_slope = false;
        e[3].defines_slope = false;
        let id = d.add_roof_sketch(rt, lvl, e).unwrap();
        let g = d.roof_geometry(d.roof(id).unwrap()).unwrap();
        assert_eq!(g.faces.iter().filter(|f| f.vertical).count(), 2);
        assert_eq!(g.faces.iter().filter(|f| !f.vertical).count(), 2);
        // the ridge runs the full length
        let ridge = g.ridges.iter().map(|r| (r[0].x - r[1].x).abs()).fold(0.0, f64::max);
        assert!((ridge - 10000.0).abs() < 1.0, "ridge {ridge}");
    }

    #[test]
    fn a_footprint_with_no_slope_is_a_flat_roof() {
        let (mut d, lvl, rt) = setup();
        let mut e = rect(0., 0., 5000., 5000.);
        e.iter_mut().for_each(|x| x.defines_slope = false);
        let id = d.add_roof_sketch(rt, lvl, e).unwrap();
        let g = d.roof_geometry(d.roof(id).unwrap()).unwrap();
        assert!(g.faces.is_empty());
        assert_eq!(g.flat.len(), 1);
        assert!((g.top_z - 3000.0).abs() < 1e-9);
        assert!((d.roof_area_mm2(d.roof(id).unwrap()) - 25.0e6).abs() < 1.0);
    }

    #[test]
    fn the_slope_property_sets_every_slope_defining_edge_and_the_base_offset_lifts_it() {
        let (mut d, lvl, rt) = setup();
        let mut e = rect(0., 0., 10000., 6000.);
        e[2].defines_slope = false;
        let id = d.add_roof_sketch(rt, lvl, e).unwrap();
        d.patch_roof(id, &serde_json::json!({ "slope_deg": 30.0, "base_offset_mm": 200.0 })).unwrap();
        let r = d.roof(id).unwrap();
        assert_eq!(r.common_slope(), Some(30.0));
        assert!(!r.edges()[2].defines_slope);
        assert_eq!(r.base_offset_mm, 200.0);
        let g = d.roof_geometry(r).unwrap();
        assert!((g.base_z - 3200.0).abs() < 1e-9);
        d.undo().unwrap();
        assert_eq!(d.roof(id).unwrap().common_slope(), Some(22.5));
        assert!(d.patch_roof(id, &serde_json::json!({ "slope_deg": 95.0 })).is_err());
    }

    #[test]
    fn a_cutoff_level_flattens_the_top() {
        let (mut d, lvl, rt) = setup();
        let id = d.add_roof_sketch(rt, lvl, rect(0., 0., 10000., 6000.)).unwrap();
        let top_before = d.roof_geometry(d.roof(id).unwrap()).unwrap().top_z;
        // cut off 600 above Level 2
        d.patch_roof(id, &serde_json::json!({ "cutoff_level": lvl, "cutoff_offset_mm": 600.0 })).unwrap();
        let g = d.roof_geometry(d.roof(id).unwrap()).unwrap();
        assert!(g.top_z < top_before);
        assert!((g.top_z - 3600.0).abs() < 1e-6);
        assert!(!g.caps.is_empty());
        // the flat top is where the ridge was: the plan area of the caps is the area above the cut
        let capped: f64 = g.caps.iter().map(|c| crate::skeleton::plan_area(c)).sum();
        assert!(capped > 1.0e6 && capped < 60.0e6);
        // below the base there is nothing left
        d.patch_roof(id, &serde_json::json!({ "cutoff_offset_mm": -100.0 })).unwrap();
        let g = d.roof_geometry(d.roof(id).unwrap()).unwrap();
        assert!(g.faces.is_empty());
    }

    #[test]
    fn an_opening_makes_a_ring_roof_and_arcs_make_curved_eaves() {
        let (mut d, lvl, rt) = setup();
        let mut e = rect(0., 0., 12000., 12000.);
        // an opening, drawn the other way round
        let hole = [p(4000., 4000.), p(8000., 4000.), p(8000., 8000.), p(4000., 8000.)];
        e.extend((0..4).map(|i| RoofEdge::new(Curve::line(hole[i], hole[(i + 1) % 4]))));
        let id = d.add_roof_sketch(rt, lvl, e).unwrap();
        let g = d.roof_geometry(d.roof(id).unwrap()).unwrap();
        assert_eq!(g.regions.len(), 1);
        assert_eq!(g.regions[0].holes.len(), 1);
        assert_eq!(g.faces.len(), 8);
        // a circle is a cone
        let c = d.add_roof_sketch(rt, lvl, vec![RoofEdge::new(Curve::Circle { centre: p(30000., 0.), radius: 4000.0 })]).unwrap();
        let g = d.roof_geometry(d.roof(c).unwrap()).unwrap();
        assert!(g.faces.len() > 20);
        assert!((g.top_z - 3000.0 - 4000.0 * 22.5f64.to_radians().tan()).abs() < 5.0);
    }

    #[test]
    fn a_sketch_with_a_gap_or_a_bad_slope_is_refused() {
        let (mut d, lvl, rt) = setup();
        let mut e = rect(0., 0., 5000., 5000.);
        e.pop();
        assert!(d.add_roof_sketch(rt, lvl, e).is_err());
        let mut e = rect(0., 0., 5000., 5000.);
        e[0].slope_deg = 95.0;
        assert!(d.add_roof_sketch(rt, lvl, e).is_err());
        assert!(d.roofs().is_empty());
    }

    #[test]
    fn editing_the_footprint_keeps_each_edge_setting_and_undoes_in_one_step() {
        let (mut d, lvl, rt) = setup();
        let mut e = rect(0., 0., 10000., 6000.);
        e[1].defines_slope = false;
        e[0].slope_deg = 35.0;
        let id = d.add_roof_sketch(rt, lvl, e).unwrap();
        // the sketch comes back as loose edges in any order
        let mut edges: Vec<RoofEdge> = d.roof(id).unwrap().edges().into_iter().cloned().collect();
        edges.reverse();
        for ed in &mut edges {
            if let Curve::Line { a, b } = ed.curve.clone() {
                if ed.slope_deg == 35.0 {
                    ed.curve = Curve::line(b, a); // drawn the other way round
                }
            }
        }
        d.set_roof_sketch(id, edges).unwrap();
        let r = d.roof(id).unwrap();
        assert_eq!(r.edges().iter().filter(|e| !e.defines_slope).count(), 1);
        assert_eq!(r.edges().iter().filter(|e| e.slope_deg == 35.0).count(), 1);
        d.undo().unwrap();
        assert_eq!(d.roof(id).unwrap().loops()[0].len(), 4);
    }

    #[test]
    fn roofs_save_and_open_and_pinned_roofs_refuse_edits() {
        let (mut d, lvl, rt) = setup();
        let id = d.add_roof_sketch(rt, lvl, rect(0., 0., 8000., 5000.)).unwrap();
        d.set_pinned(&[id], true).unwrap();
        assert!(d.delete(&[id]).is_err());
        assert!(d.set_roof_sketch(id, rect(0., 0., 1000., 1000.)).is_err());
        let json = serde_json::to_string(d.get(id).unwrap()).unwrap();
        let back: Element = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, d.get(id).unwrap());
        // an older file with only the required fields reads with the defaults
        let old = serde_json::json!({ "kind": "Roof", "roof_type": rt, "level": lvl, "shape": { "shape": "footprint", "loops": [[
            { "curve": { "type": "line", "a": {"x":0.0,"y":0.0}, "b": {"x":4000.0,"y":0.0} } },
            { "curve": { "type": "line", "a": {"x":4000.0,"y":0.0}, "b": {"x":4000.0,"y":4000.0} } },
            { "curve": { "type": "line", "a": {"x":4000.0,"y":4000.0}, "b": {"x":0.0,"y":0.0} } } ]] } });
        let Element::Roof(r) = serde_json::from_value::<Element>(old).unwrap() else { panic!() };
        assert!(r.room_bounding && !r.pinned && r.cutoff_level.is_none());
        assert_eq!(r.common_slope(), Some(DEFAULT_SLOPE_DEG));
    }
}
