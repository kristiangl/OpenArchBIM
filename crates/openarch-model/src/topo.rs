//! Toposolids: the ground as a solid. A closed boundary sketched on a level, a set of surveyed or added points
//! inside it, and a type whose layers give the solid its thickness below the surface.
//!
//! The top surface is a constrained Delaunay triangulation (TIN) of the boundary's corners and the points, with
//! the boundary's edges and the split lines kept as edges (shape.rs, which floors and roofs are to share). Point heights are stored above the unmodified top (the level plus Height
//! Offset From Level), so moving the level or the offset moves the whole surface. A boundary corner with no point
//! on it sits at the unmodified top. The solid follows the top down by the type's thickness, unless a layer of the
//! type is variable: then the underside stays flat at the unmodified top less the thickness, and the variable
//! layer takes up the difference.
//!
//! Contour lines come from the type's contour sets, measured as shared elevations (from the shared site origin,
//! which the Survey Point sets).
//!
//! Excavate: floors, roofs and other toposolids a toposolid lists take out of it whatever stands above their
//! undersides within their footprints (shape::excavate), worked out afresh from where they are, so the hole follows
//! them as they move or change. Everything that draws or measures the solid uses `toposolid_surface`, the top as
//! excavated; editing points and lines works on the unexcavated top (`toposolid_tin`).
//!
//! Sub-divide: a toposolid with a host is a sub-division of it. Its top follows the host's unexcavated top within
//! its own boundary (clipped to the host's), raised by its offset (lowered when negative); it has its own type and
//! no points of its own. It reaches down by its type's thickness, or to the host's top when it stands higher than
//! that, and it excavates the host below it.
//!
//! Split and Merge: a toposolid cut in pieces along a sketch, or two made one. The points the pieces keep, and the
//! ones added where the cut crosses the surface's edges, keep the surface where it was.
//!
//! Graded Region: a toposolid keeps the ground as it was when it was graded (its top then, as a fixed surface in
//! absolute heights), and its cut and fill are measured from that: within its boundary, the volume between that
//! ground and its top as built (excavations included), cut where the top is lower and fill where it is higher.
//! Plans draw the old ground's contours as existing contours. There are no phases yet, so this stands in for the
//! demolished original and its graded copy.

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::element::{default_coarse_fill, IfcParams, Layer, LayerFunction, TypeInfo};
use crate::geom::Region;
pub use crate::shape::{triangulate, Tin};
use crate::shape::{within, Cutter};
use crate::site::P3;
use crate::styles::PatternGraphics;
use crate::{Document, Element, Id, Point};

/// Label Contours: a line drawn across contours in one plan view; each contour it crosses is labelled there with
/// its shared elevation, the text running along the contour. The line itself shows only when selected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContourLabel {
    pub view: Id,
    pub a: Point,
    pub b: Point,
    /// Elevation Base: measured from the survey point (shared elevations, as the contours are) or the project base
    /// point.
    #[serde(default = "survey")]
    pub base: TopoBase,
    /// Places after the decimal point, in metres.
    #[serde(default = "one")]
    pub decimals: u8,
    /// Text height on paper, mm.
    #[serde(default = "label_size")]
    pub size_mm: f64,
    /// Label only the primary contours.
    #[serde(default)]
    pub primary_only: bool,
    #[serde(default)]
    pub pinned: bool,
}

fn survey() -> TopoBase { TopoBase::SurveyPoint }
fn one() -> u8 { 1 }
fn label_size() -> f64 { 2.0 }

/// One label: where it sits (model mm), its turn (radians, kept readable), its text, and whether its contour is
/// primary.
#[derive(Debug, Clone, Serialize)]
pub struct ContourLabelText {
    pub at: [f64; 2],
    pub angle: f64,
    pub text: String,
    pub primary: bool,
}

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
    /// Split lines: pairs of indices into `points`; the surface folds along each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub split_lines: Vec<[usize; 2]>,
    /// Excavate: the floors, roofs and other toposolids that cut into it. One no longer there is passed over.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excavated_by: Vec<Id>,
    /// A sub-division: the toposolid it stands on. Its `offset_mm` is then its height above the host's top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Id>,
    /// A graded region: the ground as it was when it was graded (absolute heights), which cut and fill are
    /// measured from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<Tin>,
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
        Toposolid { toposolid_type, level, loops, offset_mm, points: vec![], split_lines: vec![], excavated_by: vec![], host: None, existing: None, pinned: false, comments: String::new(), mark: String::new(), ifc: IfcParams { guid: crate::element::new_ifc_guid(), ..Default::default() } }
    }

    /// The boundary as polygons (arcs and splines broken into short chords).
    pub fn polygons(&self, tol: f64) -> Vec<Vec<Point>> {
        self.loops.iter().map(|l| crate::curve::loop_polygon(l, tol)).filter(|p| p.len() >= 3).collect()
    }

    pub fn regions(&self, tol: f64) -> Vec<Region> {
        crate::geom::regions(&self.polygons(tol))
    }
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

    /// The top surface, triangulated. A sub-division's is the host's top over the part of its boundary on the host,
    /// raised by its offset.
    pub fn toposolid_tin(&self, t: &Toposolid) -> Option<Tin> {
        if let Some(h) = t.host {
            let host = self.toposolid(h).filter(|h| h.host.is_none())?;
            let rings = crate::shape::overlap(&t.polygons(20.0), &host.polygons(20.0));
            return crate::shape::drape(&rings, &self.toposolid_tin(host)?, t.offset_mm);
        }
        let base = self.toposolid_base_z(t);
        let pts: Vec<[f64; 3]> = t.points.iter().map(|p| [p.x, p.y, base + p.z]).collect();
        triangulate(&t.polygons(20.0), &pts, &t.split_lines, base)
    }

    /// The underside's height under each corner of the surface. With a variable layer it stays flat at the
    /// unmodified top less the type's thickness, following the top down only where the ground dips so far that the
    /// other layers would not fit; without one it is the top less the thickness.
    /// A sub-division's reaches down by the thickness, or to the host's top where it stands higher than that.
    pub fn toposolid_bottoms(&self, t: &Toposolid, tin: &Tin) -> Vec<f64> {
        let ty = self.toposolid_type(t.toposolid_type);
        let th = ty.map_or(0.0, |ty| ty.thickness_mm());
        if t.host.is_some() { return tin.verts.iter().map(|v| v[2] - th.max(t.offset_mm)).collect(); }
        let fixed = ty.and_then(|ty| ty.variable_layer().map(|v| th - ty.layers[v].thickness_mm));
        let flat = self.toposolid_base_z(t) - th;
        tin.verts.iter().map(|v| match fixed { Some(f) => flat.min(v[2] - f), None => v[2] - th }).collect()
    }

    /// The toposolid as built: its top with the excavations taken out, the underside's height under each corner of
    /// it, and the footprints of what excavates it.
    pub fn toposolid_surface(&self, t: &Toposolid) -> Option<(Tin, Vec<f64>, Vec<Vec<Point>>)> {
        self.toposolid_surface_less(t, None)
    }

    /// The same, as if `skip` did not excavate it.
    fn toposolid_surface_less(&self, t: &Toposolid, skip: Option<Id>) -> Option<(Tin, Vec<f64>, Vec<Vec<Point>>)> {
        let tin = self.toposolid_tin(t)?;
        let bot = self.toposolid_bottoms(t, &tin);
        // what it lists, and its sub-divisions
        let subs = self.toposolid_id(t).map(|id| self.toposolid_subdivisions(id)).unwrap_or_default();
        let cutters: Vec<Cutter> = t.excavated_by.iter().chain(&subs).filter(|&&id| Some(id) != skip).filter_map(|&id| self.cutter(id)).collect();
        if cutters.is_empty() { return Some((tin, bot, vec![])); }
        let under = Tin { verts: tin.verts.iter().zip(&bot).map(|(v, &z)| [v[0], v[1], z]).collect(), tris: tin.tris.clone() };
        match crate::shape::excavate(&t.polygons(20.0), &tin, &under, &cutters) {
            Some((top, b)) => Some((top, b, cutters.into_iter().flat_map(|c| c.rings).collect())),
            None => Some((tin, bot, vec![])),
        }
    }

    /// The id of a toposolid (or of the one a copy of it was taken from): the same one, else the one with its IFC GUID.
    pub fn toposolid_id(&self, t: &Toposolid) -> Option<Id> {
        let all = self.toposolids();
        all.iter().find(|(_, x)| std::ptr::eq(*x, t)).or_else(|| all.iter().find(|(_, x)| !t.ifc.guid.is_empty() && x.ifc.guid == t.ifc.guid)).map(|(i, _)| *i)
    }

    /// Its outline as built: a sub-division's is the part of its boundary on its host.
    pub fn toposolid_rings(&self, t: &Toposolid) -> Vec<Vec<Point>> {
        match t.host.and_then(|h| self.toposolid(h)) {
            Some(h) => crate::shape::overlap(&t.polygons(20.0), &h.polygons(20.0)),
            None => t.polygons(20.0),
        }
    }

    /// The sub-divisions standing on a toposolid.
    pub fn toposolid_subdivisions(&self, host: Id) -> Vec<Id> {
        self.toposolids().into_iter().filter(|(_, t)| t.host == Some(host)).map(|(i, _)| i).collect()
    }

    /// How much of its host a sub-division takes out (mm³).
    pub fn subdivision_excavation(&self, id: Id) -> f64 {
        let Some(host) = self.toposolid(id).and_then(|t| t.host).and_then(|h| self.toposolid(h)) else { return 0.0 };
        let vol = |s: Option<(Tin, Vec<f64>, Vec<Vec<Point>>)>| s.map_or(0.0, |(tin, bot, _)| solid_volume(&tin, &bot));
        (vol(self.toposolid_surface_less(host, Some(id))) - vol(self.toposolid_surface(host))).max(0.0)
    }

    /// An element as something that excavates a toposolid: a floor over its outline (an opening in it leaves no
    /// ground standing), a roof over its faces, a toposolid over its boundary, each down to its underside. Toposolids
    /// cut with their unexcavated shape, so two can excavate each other.
    fn cutter(&self, id: Id) -> Option<Cutter> {
        let ring_edges = |rings: &[Vec<Point>]| -> Vec<[Point; 2]> {
            rings.iter().flat_map(|r| (0..r.len()).map(move |i| [r[i], r[(i + 1) % r.len()]])).collect()
        };
        match self.get(id)? {
            Element::Floor(f) => {
                let surf = self.floor_surface(f)?;
                let th = self.floor_type(f.floor_type)?.thickness_mm();
                let rings: Vec<Vec<Point>> = f.regions(20.0).into_iter().map(|r| r.outer).collect();
                if rings.is_empty() { return None; }
                Some(Cutter { rings, folds: vec![], under: Box::new(move |p| Some(surf.z(p) - th)) })
            }
            Element::Roof(r) => {
                let g = self.roof_geometry(r)?;
                let th = self.roof_type(r.roof_type)?.thickness_mm();
                // each face, flat piece and cap with the plane of the underside under it (the top less the thickness
                // square to the face)
                let mut pieces: Vec<(Vec<Point>, [f64; 3])> = vec![];
                let plan = |poly: &[[f64; 3]]| poly.iter().map(|q| Point::new(q[0], q[1])).collect::<Vec<_>>();
                for f in g.faces.iter().filter(|f| !f.vertical) {
                    if let Some(pl) = crate::shape::plane_through(&f.poly) {
                        pieces.push((plan(&f.poly), [pl[0], pl[1], pl[2] - th * (1.0 + pl[0] * pl[0] + pl[1] * pl[1]).sqrt()]));
                    }
                }
                for rg in &g.flat { pieces.push((rg.outer.clone(), [0.0, 0.0, g.base_z - th])); }
                for c in &g.caps { if c.len() >= 3 { pieces.push((plan(c), [0.0, 0.0, c[0][2] - th])); } }
                // a shape edited flat roof: the underside under each triangle of its top
                if let Some((tin, bot)) = &g.shaped {
                    for k in &tin.tris {
                        let under: Vec<[f64; 3]> = k.iter().map(|&i| [tin.verts[i][0], tin.verts[i][1], bot[i]]).collect();
                        if let Some(pl) = crate::shape::plane_through(&under) { pieces.push((plan(&under), pl)); }
                    }
                }
                pieces.retain(|p| p.0.len() >= 3);
                if pieces.is_empty() { return None; }
                let rings: Vec<Vec<Point>> = pieces.iter().map(|p| p.0.clone()).collect();
                let folds = ring_edges(&rings);
                Some(Cutter { rings, folds, under: Box::new(move |p| {
                    pieces.iter().filter(|(poly, _)| within(std::slice::from_ref(poly), p)).map(|(_, pl)| pl[0] * p.x + pl[1] * p.y + pl[2]).reduce(f64::max)
                }) })
            }
            Element::Toposolid(o) => {
                let tin = self.toposolid_tin(o)?;
                let bot = self.toposolid_bottoms(o, &tin);
                let folds = tin.edges().into_iter().map(|([a, b], _)| [Point::new(tin.verts[a][0], tin.verts[a][1]), Point::new(tin.verts[b][0], tin.verts[b][1])]).collect();
                let under = Tin { verts: tin.verts.iter().zip(&bot).map(|(v, &z)| [v[0], v[1], z]).collect(), tris: tin.tris.clone() };
                Some(Cutter { rings: o.polygons(20.0), folds, under: Box::new(move |p| under.z_near(p)) })
            }
            _ => None,
        }
    }

    /// Excavate: each of `by` (a floor, roof or other toposolid) takes out of the toposolid whatever stands above its
    /// underside within its footprint, from then on wherever it goes. Each must take something out. Returns how
    /// many were added (one already excavating is passed over).
    pub fn excavate_toposolid(&mut self, id: Id, by: &[Id]) -> Result<usize, String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        self.refuse_pinned(&[id], "edited")?;
        let mut added = 0;
        for &c in by {
            if c == id { return Err("A toposolid can't excavate itself.".into()); }
            if self.toposolid(c).is_some_and(|s| s.host == Some(id)) { return Err("A sub-division already excavates the toposolid it's on.".into()); }
            let noun = match self.get(c) {
                Some(Element::Floor(_)) => "floor",
                Some(Element::Roof(_)) => "roof",
                Some(Element::Toposolid(_)) => "toposolid",
                _ => return Err("Floors, roofs and other toposolids excavate a toposolid.".into()),
            };
            if t.excavated_by.contains(&c) { continue; }
            let mut with = t.clone();
            with.excavated_by.push(c);
            // a litre at least, so one that only touches the ground is refused
            if self.toposolid_volume(&with) > self.toposolid_volume(&t) - 1e6 {
                return Err(format!("The {noun} doesn't cut into the toposolid: its underside has to be below the ground somewhere inside the boundary."));
            }
            t = with;
            added += 1;
        }
        if added == 0 { return Ok(0); }
        self.transact("Excavate", |tx| tx.update(id, Element::Toposolid(t)))?;
        Ok(added)
    }

    /// Remove Excavation: `by` no longer cut into the toposolid. Returns how many were removed.
    pub fn remove_excavation(&mut self, id: Id, by: &[Id]) -> Result<usize, String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        self.refuse_pinned(&[id], "edited")?;
        let before = t.excavated_by.len();
        t.excavated_by.retain(|c| !by.contains(c));
        let removed = before - t.excavated_by.len();
        if removed == 0 { return Err("That doesn't excavate the toposolid.".into()); }
        self.transact("Remove excavation", |tx| tx.update(id, Element::Toposolid(t)))?;
        Ok(removed)
    }

    /// What excavates a toposolid, of what is still there.
    pub fn toposolid_excavators(&self, t: &Toposolid) -> Vec<Id> {
        t.excavated_by.iter().copied().filter(|&c| matches!(self.get(c), Some(Element::Floor(_) | Element::Roof(_) | Element::Toposolid(_)))).collect()
    }

    /// The top and the underside at a plan point: the top's height, upward normal and whether the point is on a fold,
    /// and the underside's height. None off the toposolid.
    pub fn toposolid_sample(&self, t: &Toposolid, p: Point) -> Option<((f64, [f64; 3], bool), f64)> {
        let (tin, bot, _) = self.toposolid_surface(t)?;
        let top = tin.sample(p)?;
        let under = Tin { verts: tin.verts.iter().zip(&bot).map(|(v, &z)| [v[0], v[1], z]).collect(), tris: tin.tris.clone() };
        Some((top, under.z_at(p)?))
    }

    /// The solid's volume (mm³): over each triangle, its plan area by the mean depth at its corners.
    pub fn toposolid_volume(&self, t: &Toposolid) -> f64 {
        let Some((tin, bot, _)) = self.toposolid_surface(t) else { return 0.0 };
        solid_volume(&tin, &bot)
    }

    /// Where a floor plan's cut plane at height `z` passes through the ground: the pieces of plan where the surface
    /// is above the plane and the underside below it (filled with the cut pattern), and the lines round them (where
    /// the top or the underside meets the plane, and the boundary in between).
    pub fn toposolid_plan_cut(&self, t: &Toposolid, z: f64) -> (Vec<Vec<Point>>, Vec<[Point; 2]>) {
        let Some((tin, bot, _)) = self.toposolid_surface(t) else { return (vec![], vec![]) };
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
        self.toposolid_contour_lines(t).into_iter().map(|(_, primary, segs)| (primary, segs)).collect()
    }

    /// The contours with their heights (absolute, mm).
    pub fn toposolid_contour_lines(&self, t: &Toposolid) -> Vec<(f64, bool, Vec<[Point; 2]>)> {
        let (Some(ty), Some((tin, _, edges))) = (self.toposolid_type(t.toposolid_type), self.toposolid_surface(t)) else { return vec![] };
        let on_side = |s: &[Point; 2]| {
            let m = Point::new((s[0].x + s[1].x) / 2.0, (s[0].y + s[1].y) / 2.0);
            edges.iter().any(|r| (0..r.len()).any(|i| crate::geom::dist_to_segment(m, r[i], r[(i + 1) % r.len()]) < crate::shape::EXCAVATION_EDGE * 1.5))
        };
        let subs: Vec<Vec<Vec<Point>>> = self.toposolid_id(t).map(|id| self.toposolid_subdivisions(id)).unwrap_or_default().into_iter().filter_map(|s| self.toposolid(s)).map(|s| self.toposolid_rings(s)).collect();
        let under_sub = |s: &[Point; 2]| {
            let m = Point::new((s[0].x + s[1].x) / 2.0, (s[0].y + s[1].y) / 2.0);
            subs.iter().any(|r| crate::shape::inside(r, m))
        };
        let zero = self.topo_base_z(TopoBase::SurveyPoint, t.level);
        let (lo, hi) = tin.z_range();
        let heights = contour_heights(ty, lo, hi, zero);
        heights.into_iter().map(|(z, primary)| (z, primary, tin.contour(z).into_iter().filter(|s| (edges.is_empty() || !on_side(s)) && !under_sub(s)).collect::<Vec<_>>())).filter(|(_, _, l)| !l.is_empty()).collect()
    }

    /// A graded region's existing contours: the type's contours of the ground as it was, within its boundary.
    pub fn toposolid_existing_contours(&self, t: &Toposolid) -> Vec<(bool, Vec<[Point; 2]>)> {
        let (Some(ty), Some(old)) = (self.toposolid_type(t.toposolid_type), t.existing.as_ref()) else { return vec![] };
        let rings = t.polygons(20.0);
        let zero = self.topo_base_z(TopoBase::SurveyPoint, t.level);
        let (lo, hi) = old.z_range();
        contour_heights(ty, lo, hi, zero).into_iter().map(|(z, primary)| {
            (primary, old.contour(z).into_iter().filter(|s| crate::shape::inside(&rings, Point::new((s[0].x + s[1].x) / 2.0, (s[0].y + s[1].y) / 2.0))).collect::<Vec<_>>())
        }).filter(|(_, l)| !l.is_empty()).collect()
    }

    /// A graded region's cut and fill (mm³): within its boundary, between the ground as it was and its top as built.
    pub fn toposolid_cut_fill(&self, t: &Toposolid) -> Option<(f64, f64)> {
        let old = t.existing.as_ref()?;
        let (top, _, _) = self.toposolid_surface(t)?;
        Some(crate::shape::cut_fill(&t.polygons(20.0), old, &top))
    }

    /// Graded Region: the toposolid keeps its ground as it is now as the existing ground, which cut and fill are
    /// measured from as it is reshaped. With `perimeter_only` its inside points and split lines go, leaving the
    /// boundary at the heights it has (the simple start for grading); otherwise it stays as it is.
    pub fn grade_toposolid(&mut self, id: Id, perimeter_only: bool) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        if t.host.is_some() { return Err("A sub-division follows the toposolid it's on: make that toposolid a graded region.".into()); }
        if t.existing.is_some() { return Err("This toposolid is already a graded region: its cut and fill are measured from the ground as it was when it was graded.".into()); }
        self.refuse_pinned(&[id], "graded")?;
        let tin = self.toposolid_tin(&t).ok_or("the toposolid has no surface")?;
        if perimeter_only {
            let base = self.toposolid_base_z(&t);
            let rings = t.polygons(20.0);
            let on_edge = |p: Point| rings.iter().any(|r| crate::shape::on_ring(r, p));
            let mut pts: Vec<P3> = rings.concat().into_iter().filter_map(|q| tin.z_near(q).map(|z| P3 { x: q.x, y: q.y, z: z - base })).collect();
            pts.extend(t.points.iter().filter(|p| on_edge(Point::new(p.x, p.y))).copied());
            (t.points, t.split_lines) = crate::shape::tidy(&rings, pts, vec![], "toposolid")?;
        }
        t.existing = Some(tin);
        self.transact("Graded region", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// Reset Shape: the points and split lines go, leaving the top flat at Height Offset From Level.
    pub fn reset_toposolid_shape(&mut self, id: Id) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        Self::refuse_subdivision_shape(&t)?;
        if t.points.is_empty() && t.split_lines.is_empty() { return Err("The toposolid is already flat: it has no points or split lines.".into()); }
        self.refuse_pinned(&[id], "reshaped")?;
        t.points.clear();
        t.split_lines.clear();
        self.transact("Reset shape", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// The toposolid stops being a graded region: it forgets the existing ground and keeps its ground as it is.
    pub fn remove_graded_region(&mut self, id: Id) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        if t.existing.take().is_none() { return Err("This toposolid isn't a graded region.".into()); }
        self.transact("Remove graded region", |tx| tx.update(id, Element::Toposolid(t)))
    }
}

/// The contour heights (absolute, mm) a type gives over a surface from `lo` to `hi`, measured from `zero`, each
/// with whether a primary set gives it.
fn contour_heights(ty: &ToposolidType, lo: f64, hi: f64, zero: f64) -> Vec<(f64, bool)> {
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
    heights.into_iter().map(|(k, primary)| (k as f64 / 1000.0 + zero, primary)).collect()
}

impl Document {
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
        if t.split_lines.iter().any(|l| l[0] >= t.points.len() || l[1] >= t.points.len() || l[0] == l[1]) { return Err("a split line must join two of the points".into()); }
        self.check_floor_sketch(&t.loops.concat())?;
        if let Some(h) = t.host {
            if !self.toposolid(h).is_some_and(|h| h.host.is_none()) { return Err("A sub-division stands on a toposolid that isn't itself a sub-division.".into()); }
            if self.toposolid_tin(t).is_none_or(|s| s.plan_area() < 1.0) { return Err("The sub-division's boundary has to overlap the toposolid.".into()); }
        }
        Ok(())
    }

    /// Sub-Divide: a region of a toposolid sketched on it, with its own type, its top `offset_mm` above the host's.
    pub fn add_toposolid_subdivision(&mut self, host: Id, toposolid_type: Id, curves: Vec<Curve>, offset_mm: f64) -> Result<Id, String> {
        let h = self.toposolid(host).ok_or("not a toposolid")?;
        if h.host.is_some() { return Err("A sub-division can't be sub-divided: sub-divide the toposolid it's on.".into()); }
        let loops = self.check_floor_sketch(&curves)?;
        let mut t = Toposolid::new(toposolid_type, h.level, loops, offset_mm);
        t.host = Some(host);
        self.check_toposolid(&t)?;
        self.transact("Sub-divide toposolid", |tx| Ok(tx.add(Element::Toposolid(t))))
    }

    fn refuse_subdivision_shape(t: &Toposolid) -> Result<(), String> {
        if t.host.is_some() { return Err("A sub-division follows the shape of the toposolid it's on: edit that toposolid's points.".into()); }
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
        self.add_toposolid_points_within(toposolid_type, level, points, None)
    }

    /// The same with a boundary given (a lot's outline) in place of the convex hull. The ground inside it is the
    /// surface through all the points, cut off at the boundary: the points inside are kept, a point is added where
    /// each edge of that surface crosses the boundary, and each corner of the boundary takes the surface's height
    /// there (the nearest point's where the points don't reach that far).
    pub fn add_toposolid_points_within(&mut self, toposolid_type: Id, level: Id, points: Vec<[f64; 3]>, boundary: Option<Vec<Point>>) -> Result<Id, String> {
        let mut points = thin(points, MAX_POINTS);
        let mut hull = convex_hull(&points.iter().map(|p| Point::new(p[0], p[1])).collect::<Vec<_>>());
        if hull.len() < 3 || crate::geom::signed_area(&hull).abs() < 1.0 {
            return Err("The points need to cover an area: at least three that are not in a line.".into());
        }
        if let Some(ring) = boundary {
            if ring.len() < 3 || crate::geom::signed_area(&ring).abs() < 1.0 { return Err("The boundary needs to enclose an area.".into()); }
            let all = crate::shape::triangulate(&[hull.clone()], &points, &[], 0.0).ok_or("The points could not be made into a surface.")?;
            let nearest = |q: Point| points.iter().map(|p| ((p[0] - q.x).hypot(p[1] - q.y), p[2])).min_by(|a, b| a.0.total_cmp(&b.0)).map_or(0.0, |m| m.1);
            let mut edge: Vec<[f64; 3]> = ring.iter().map(|q| [q.x, q.y, all.z_at(*q).unwrap_or_else(|| nearest(*q))]).collect();
            let n = ring.len();
            let mut crossings = 0;
            for ([i, j], _) in all.edges() {
                let (u, v) = (all.verts[i], all.verts[j]);
                let (pu, pv) = (Point::new(u[0], u[1]), Point::new(v[0], v[1]));
                for k in 0..n {
                    if let Some((t, _)) = seg_cross(pu, pv, ring[k], ring[(k + 1) % n]) {
                        edge.push([u[0] + (v[0] - u[0]) * t, u[1] + (v[1] - u[1]) * t, u[2] + (v[2] - u[2]) * t]);
                        crossings += 1;
                    }
                }
            }
            let rings = [ring.clone()];
            points.retain(|p| within(&rings, Point::new(p[0], p[1])));
            if points.is_empty() && crossings == 0 { return Err("The boundary doesn't lie over the contours and points: is it the right layer?".into()); }
            // the edge samples after the points, so a point already on the boundary keeps its own height
            for e in edge {
                if !points.iter().any(|p| (p[0] - e[0]).hypot(p[1] - e[1]) < 1.0) { points.push(e); }
            }
            points = thin(points, MAX_POINTS);
            hull = ring;
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
        Self::refuse_subdivision_shape(&t)?;
        self.refuse_pinned(&[id], "edited")?;
        if !(at.x.is_finite() && at.y.is_finite() && z_abs.is_finite()) { return Err("the point must be numbers".into()); }
        let rings = t.polygons(20.0);
        if !within(&rings, at) { return Err("Click inside the toposolid's boundary.".into()); }
        let z = z_abs - self.toposolid_base_z(&t);
        // a point on top of another replaces it (and takes its split lines)
        let mut pts = t.points.clone();
        pts.push(P3 { x: at.x, y: at.y, z });
        (t.points, t.split_lines) = crate::shape::tidy(&rings, pts, t.split_lines.clone(), "toposolid")?;
        self.transact("Add point", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// Edit Boundary's Finish: the new sketch; points outside it are dropped.
    pub fn set_toposolid_sketch(&mut self, id: Id, curves: Vec<Curve>) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        self.refuse_pinned(&[id], "edited")?;
        t.loops = self.check_floor_sketch(&curves)?;
        let rings = t.polygons(20.0);
        (t.points, t.split_lines) = crate::shape::retain_within(&rings, &t.points, &t.split_lines);
        self.check_toposolid(&t)?;
        self.transact("Edit toposolid boundary", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// Modify Sub Elements: the toposolid's points and split lines replaced (plan positions, heights above the
    /// unmodified top; lines as pairs of indices). `lines` None keeps the lines as they are. See `shape::tidy`.
    pub fn set_toposolid_points(&mut self, id: Id, points: Vec<P3>, lines: Option<Vec<[usize; 2]>>) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        Self::refuse_subdivision_shape(&t)?;
        self.refuse_pinned(&[id], "edited")?;
        let rings = t.polygons(20.0);
        let lines = lines.unwrap_or_else(|| t.split_lines.iter().copied().filter(|l| l[0] < points.len() && l[1] < points.len()).collect());
        let (pts, ls) = crate::shape::tidy(&rings, points, lines, "toposolid")?;
        if pts == t.points && ls == t.split_lines { return Ok(()); }
        (t.points, t.split_lines) = (pts, ls);
        self.transact("Modify toposolid points", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// Add Split Line: a line across the surface from `a` to `b` (plan points). Each end that is not on a point
    /// becomes one at the surface's height there; the surface then folds along the line, and moving or raising its
    /// points (Modify Sub Elements) shapes it.
    pub fn add_toposolid_split_line(&mut self, id: Id, a: Point, b: Point) -> Result<(), String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        Self::refuse_subdivision_shape(&t)?;
        self.refuse_pinned(&[id], "edited")?;
        if a.dist(&b) < 1.0 { return Err("The split line needs two ends that are apart.".into()); }
        let rings = t.polygons(20.0);
        let tin = self.toposolid_tin(&t).ok_or("the toposolid has no surface")?;
        let base = self.toposolid_base_z(&t);
        let mut pts = t.points.clone();
        let mut end = |q: Point| -> Result<usize, String> {
            if !within(&rings, q) { return Err("Both ends of a split line go inside the toposolid's boundary.".into()); }
            if let Some(i) = pts.iter().position(|p| (p.x - q.x).hypot(p.y - q.y) <= 1.0) { return Ok(i); }
            let z = tin.z_at(q).ok_or("Both ends of a split line go inside the toposolid's boundary.")? - base;
            pts.push(P3 { x: q.x, y: q.y, z });
            Ok(pts.len() - 1)
        };
        let (i, j) = (end(a)?, end(b)?);
        let mut ls = t.split_lines.clone();
        ls.push([i, j]);
        (t.points, t.split_lines) = crate::shape::tidy(&rings, pts, ls, "toposolid")?;
        self.transact("Add split line", |tx| tx.update(id, Element::Toposolid(t)))
    }

    /// Simplify Toposolid: keep about `keep` (a fraction, 0 to 1) of the points (`shape::simplify`). Returns how many
    /// were dropped.
    pub fn simplify_toposolid(&mut self, id: Id, keep: f64) -> Result<usize, String> {
        let mut t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        Self::refuse_subdivision_shape(&t)?;
        self.refuse_pinned(&[id], "edited")?;
        let (pts, ls, removed) = crate::shape::simplify(&t.points, &t.split_lines, keep)?;
        if removed == 0 { return Ok(0); }
        (t.points, t.split_lines) = (pts, ls);
        self.transact("Simplify toposolid", |tx| tx.update(id, Element::Toposolid(t)))?;
        Ok(removed)
    }

    pub fn contour_labels_in(&self, view: Id) -> Vec<(Id, &ContourLabel)> {
        self.iter().filter_map(|(i, e)| match e { Element::ContourLabel(l) if l.view == view => Some((i, l)), _ => None }).collect()
    }

    /// The labels a contour label line gives: one where it crosses each contour of the toposolids the view shows.
    pub fn contour_label_texts(&self, l: &ContourLabel) -> Vec<ContourLabelText> {
        let hidden = self.view(l.view).map(|v| v.hidden_elements.clone()).unwrap_or_default();
        let zero = self.topo_base_z(l.base, 0);
        let (dx, dy) = (l.b.x - l.a.x, l.b.y - l.a.y);
        let mut out: Vec<(f64, ContourLabelText)> = vec![];
        for (id, t) in self.toposolids() {
            if hidden.contains(&id) { continue; }
            for (z, primary, segs) in self.toposolid_contour_lines(t) {
                if l.primary_only && !primary { continue; }
                for s in segs {
                    let (sx, sy) = (s[1].x - s[0].x, s[1].y - s[0].y);
                    let den = dx * sy - dy * sx;
                    if den.abs() < 1e-9 { continue; }
                    let u = ((s[0].x - l.a.x) * sy - (s[0].y - l.a.y) * sx) / den;
                    let v = ((s[0].x - l.a.x) * dy - (s[0].y - l.a.y) * dx) / den;
                    if !(0.0..=1.0).contains(&u) || !(0.0..1.0).contains(&v) { continue; }
                    // along the contour, turned so it never reads upside down
                    let mut angle = sy.atan2(sx);
                    if angle > std::f64::consts::FRAC_PI_2 + 1e-9 { angle -= std::f64::consts::PI; }
                    if angle <= -std::f64::consts::FRAC_PI_2 + 1e-9 { angle += std::f64::consts::PI; }
                    let text = format!("{:.*}", l.decimals as usize, (z - zero) / 1000.0);
                    out.push((u, ContourLabelText { at: [l.a.x + dx * u, l.a.y + dy * u], angle, text, primary }));
                }
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        // a contour piece ending exactly on the line is met twice; one label is enough
        out.dedup_by(|a, b| (a.1.at[0] - b.1.at[0]).hypot(a.1.at[1] - b.1.at[1]) < 1.0 && a.1.text == b.1.text);
        out.into_iter().map(|(_, t)| t).collect()
    }

    /// Label Contours: a line across the contours of a floor plan.
    pub fn add_contour_label(&mut self, view: Id, a: Point, b: Point) -> Result<Id, String> {
        let v = self.view(view).ok_or("unknown view")?;
        if v.view_kind != crate::view::ViewKind::FloorPlan { return Err("Contours are labelled in a floor plan.".into()); }
        if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) || a.dist(&b) < 1.0 { return Err("The label line needs two ends that are apart.".into()); }
        let l = ContourLabel { view, a, b, base: TopoBase::SurveyPoint, decimals: 1, size_mm: label_size(), primary_only: false, pinned: false };
        self.transact("Label contours", |tx| Ok(tx.add(Element::ContourLabel(l))))
    }

    pub fn patch_contour_label(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::ContourLabel(old)) = self.get(id).cloned() else { return Err("not a contour label".into()) };
        let l: ContourLabel = crate::doc::apply_patch(&old, patch)?;
        if old.pinned && l.pinned && (l.a != old.a || l.b != old.b) { return Err("Unpin the contour label before moving it.".into()); }
        if l.a.dist(&l.b) < 1.0 { return Err("The label line needs two ends that are apart.".into()); }
        if l.decimals > 4 { return Err("At most 4 decimal places.".into()); }
        if !(l.size_mm.is_finite() && l.size_mm > 0.0 && l.size_mm <= 20.0) { return Err("The text size must be more than 0 and at most 20 mm.".into()); }
        if matches!(l.base, TopoBase::CurrentLevel | TopoBase::InternalOrigin) { return Err("Contour labels measure from the survey point or the project base point.".into()); }
        let l = ContourLabel { view: old.view, ..l };
        self.transact("Contour label", |tx| tx.update(id, Element::ContourLabel(l)))
    }

    pub fn patch_toposolid(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        let mut t: Toposolid = crate::doc::apply_patch(&old, patch)?;
        if old.pinned && t.pinned && (t.loops != old.loops || t.level != old.level || t.offset_mm != old.offset_mm || t.points != old.points || t.split_lines != old.split_lines) {
            return Err("Unpin the toposolid before moving it.".into());
        }
        // a sub-division stays on its toposolid, at its level; the existing ground isn't a property
        t.host = old.host;
        t.existing = old.existing.clone();
        if let Some(h) = t.host.and_then(|h| self.toposolid(h)) { t.level = h.level; }
        self.check_toposolid(&t)?;
        let subs: Vec<(Id, Toposolid)> = if t.level != old.level {
            self.toposolid_subdivisions(id).into_iter().filter_map(|s| self.toposolid(s).map(|x| (s, Toposolid { level: t.level, ..x.clone() }))).collect()
        } else { vec![] };
        self.transact("Toposolid properties", |tx| {
            tx.update(id, Element::Toposolid(t))?;
            for (s, x) in subs { tx.update(s, Element::Toposolid(x))?; }
            Ok(())
        })
    }

    /// Split: the toposolid cut along the sketch (a closed loop inside it, or lines from edge to edge) into pieces,
    /// each its own toposolid with the original's type and properties; the largest keeps the original. The pieces
    /// keep their points and split lines (a line the cut crosses is broken there), and a point goes wherever the cut
    /// crosses an edge of the surface, so the ground keeps its shape. Sub-divisions go with the piece they are on,
    /// or are split with it; spots go with the piece they stand on. Returns the pieces, the original first.
    pub fn split_toposolid(&mut self, id: Id, curves: &[Curve]) -> Result<Vec<Id>, String> {
        let t = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        self.refuse_pinned(&[id], "split")?;
        let blade: Vec<[Point; 2]> = curves.iter().flat_map(|c| c.tessellate(20.0).windows(2).map(|w| [w[0], w[1]]).collect::<Vec<_>>()).filter(|s| s[0].dist(&s[1]) > 1e-6).collect();
        if blade.is_empty() { return Err("Sketch where to split it first.".into()); }
        let rings = t.polygons(20.0);
        let pieces = crate::shape::split_rings(&rings, &blade);
        if pieces.len() < 2 { return Err("The sketch doesn't cut the toposolid in two: sketch a closed loop inside it, or lines that run from one edge to another.".into()); }
        let tin = self.toposolid_tin(&t).ok_or("the toposolid has no surface")?;
        let base = self.toposolid_base_z(&t);
        // where the cut crosses the surface's edges (the boundary and the split lines among them), and its corners,
        // at the surface's height: these come first, so a point of the toposolid on the same spot wins
        let mut pts: Vec<P3> = vec![];
        if t.host.is_none() {
            let edges = tin.edges();
            let corner = |i: usize| Point::new(tin.verts[i][0], tin.verts[i][1]);
            for s in &blade {
                let mut at = vec![s[0], s[1]];
                for ([i, j], _) in &edges {
                    if let Some((u, _)) = seg_cross(s[0], s[1], corner(*i), corner(*j)) { at.push(Point::new(s[0].x + (s[1].x - s[0].x) * u, s[0].y + (s[1].y - s[0].y) * u)); }
                }
                for q in at { if let Some(z) = tin.z_near(q) { pts.push(P3 { x: q.x, y: q.y, z: z - base }); } }
            }
        }
        let off = pts.len();
        pts.extend(t.points.iter().copied());
        // split lines broken where the cut crosses them
        let mut lines = vec![];
        for l in &t.split_lines {
            let (a, b) = (t.points[l[0]], t.points[l[1]]);
            let mut us: Vec<f64> = blade.iter().filter_map(|s| seg_cross(Point::new(a.x, a.y), Point::new(b.x, b.y), s[0], s[1]).map(|c| c.0)).filter(|u| *u > 1e-9 && *u < 1.0 - 1e-9).collect();
            us.sort_by(f64::total_cmp);
            let mut prev = off + l[0];
            for u in us {
                pts.push(P3 { x: a.x + (b.x - a.x) * u, y: a.y + (b.y - a.y) * u, z: a.z + (b.z - a.z) * u });
                lines.push([prev, pts.len() - 1]);
                prev = pts.len() - 1;
            }
            lines.push([prev, off + l[1]]);
        }
        let mut made: Vec<Toposolid> = vec![];
        for (k, piece) in pieces.iter().enumerate() {
            let mut p = t.clone();
            p.loops = self.check_floor_sketch(&ring_curves(piece))?;
            let (ps, ls) = crate::shape::retain_within(piece, &pts, &lines);
            (p.points, p.split_lines) = crate::shape::tidy(piece, ps, ls, "toposolid")?;
            if k > 0 { p.pinned = false; p.mark.clear(); p.ifc.guid = crate::element::new_ifc_guid(); }
            made.push(p);
        }
        let piece_rings: Vec<Vec<Vec<Point>>> = made.iter().map(|p| p.polygons(20.0)).collect();
        let area = |r: &[Vec<Point>]| -> f64 { r.iter().map(|q| crate::geom::signed_area(q)).sum::<f64>().abs() };
        // sub-divisions: on one piece they move to it; across the cut they are split with it
        let mut subs: Vec<(Option<Id>, Toposolid, usize)> = vec![];
        for s in self.toposolid_subdivisions(id) {
            let Some(st) = self.toposolid(s).cloned() else { continue };
            let sr = st.polygons(20.0);
            let parts: Vec<(usize, Vec<Vec<Point>>)> = piece_rings.iter().enumerate().map(|(k, pr)| (k, crate::shape::overlap(&sr, pr))).filter(|(_, o)| area(o) > 1.0).collect();
            if parts.len() <= 1 {
                subs.push((Some(s), st, parts.first().map_or(0, |p| p.0)));
                continue;
            }
            let mut parts = parts;
            parts.sort_by(|a, b| area(&b.1).total_cmp(&area(&a.1)));
            for (n, (k, o)) in parts.into_iter().enumerate() {
                let mut x = st.clone();
                x.loops = self.check_floor_sketch(&ring_curves(&o))?;
                if n > 0 { x.pinned = false; x.mark.clear(); x.ifc.guid = crate::element::new_ifc_guid(); }
                subs.push((if n == 0 { Some(s) } else { None }, x, k));
            }
        }
        let spots: Vec<(Id, crate::spot::Spot, usize)> = self.spots().into_iter().filter(|(_, s)| s.host == Some(id)).filter_map(|(i, s)| {
            let k = piece_rings.iter().position(|r| within(r, s.target))?;
            (k > 0).then(|| (i, s.clone(), k))
        }).collect();
        let excavated: Vec<(Id, Toposolid)> = self.toposolids().into_iter().filter(|(_, x)| x.excavated_by.contains(&id)).map(|(i, x)| (i, x.clone())).collect();
        self.transact("Split toposolid", |tx| {
            let mut ids = vec![id];
            for (k, p) in made.into_iter().enumerate() {
                if k == 0 { tx.update(id, Element::Toposolid(p))?; } else { let n = tx.add(Element::Toposolid(p)); tx.copy_parameter_data(id, n)?; ids.push(n); }
            }
            for (s, mut x, k) in subs {
                x.host = Some(ids[k]);
                match s { Some(s) => tx.update(s, Element::Toposolid(x))?, None => { tx.add(Element::Toposolid(x)); } }
            }
            for (i, mut s, k) in spots { s.host = Some(ids[k]); tx.update(i, Element::Spot(s))?; }
            for (i, mut x) in excavated { x.excavated_by.extend(ids[1..].iter().copied()); tx.update(i, Element::Toposolid(x))?; }
            Ok(ids)
        })
    }

    /// Merge: `other` made part of the toposolid. They have to overlap or share an edge. The toposolid keeps its
    /// type and properties; where they overlap its ground wins, and elsewhere the other's points (and a point at
    /// each of its corners) keep the other's ground. What excavated either excavates the merged one, and the
    /// other's sub-divisions and spots move to it.
    pub fn merge_toposolids(&mut self, id: Id, other: Id) -> Result<(), String> {
        if id == other { return Err("Pick another toposolid to merge with this one.".into()); }
        let mut a = self.toposolid(id).cloned().ok_or("not a toposolid")?;
        let b = self.toposolid(other).cloned().ok_or("Pick a toposolid to merge with this one.")?;
        if a.host.is_some() || b.host.is_some() { return Err("Sub-divisions can't be merged: merge the toposolids they are on.".into()); }
        self.refuse_pinned(&[id, other], "merged")?;
        let (ra, rb) = (a.polygons(20.0), b.polygons(20.0));
        let u = crate::shape::union(&ra, &rb);
        if u.len() != 1 { return Err("The toposolids have to overlap or share an edge to merge.".into()); }
        let rings = u.into_iter().next().unwrap();
        let (ta, tb) = (self.toposolid_tin(&a).ok_or("the toposolid has no surface")?, self.toposolid_tin(&b).ok_or("the other toposolid has no surface")?);
        let (za, zb) = (self.toposolid_base_z(&a), self.toposolid_base_z(&b));
        let height = |p: Point| if within(&ra, p) { ta.z_near(p) } else { tb.z_near(p) };
        // a point at each corner that would not otherwise keep its height: the other's, the merged outline's, and
        // this one's that are no longer corners
        let outline: Vec<Point> = rings.concat();
        let is_corner = |q: &Point| outline.iter().any(|c| c.dist(q) < 0.5);
        let mut pts: Vec<P3> = vec![];
        for q in rb.concat().into_iter().chain(outline.iter().copied()).chain(ra.concat().into_iter().filter(|q| !is_corner(q))) {
            if let Some(z) = height(q) { pts.push(P3 { x: q.x, y: q.y, z: z - za }); }
        }
        // this one's edges inside the merged outline stay as split lines, so its ground keeps its shape
        let mut lines: Vec<[usize; 2]> = vec![];
        for r in &ra {
            for i in 0..r.len() {
                let (p, q) = (r[i], r[(i + 1) % r.len()]);
                let len2 = (q.x - p.x).powi(2) + (q.y - p.y).powi(2);
                if len2 < 1.0 { continue; }
                let at = |u: f64| Point::new(p.x + (q.x - p.x) * u, p.y + (q.y - p.y) * u);
                let mut us = vec![0.0, 1.0];
                us.extend(outline.iter().filter(|c| crate::geom::dist_to_segment(**c, p, q) < 0.5).map(|c| ((c.x - p.x) * (q.x - p.x) + (c.y - p.y) * (q.y - p.y)) / len2));
                us.sort_by(f64::total_cmp);
                us.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
                for w in us.windows(2) {
                    let (s, e) = (at(w[0]), at(w[1]));
                    let m = at((w[0] + w[1]) / 2.0);
                    if s.dist(&e) < 1.0 || rings.iter().any(|r| crate::shape::on_ring(r, m)) || !crate::shape::inside(&rings, m) { continue; }
                    let (Some(zs), Some(ze)) = (ta.z_near(s), ta.z_near(e)) else { continue };
                    pts.push(P3 { x: s.x, y: s.y, z: zs - za });
                    pts.push(P3 { x: e.x, y: e.y, z: ze - za });
                    lines.push([pts.len() - 2, pts.len() - 1]);
                }
            }
        }
        let off_a = pts.len();
        pts.extend(a.points.iter().copied());
        lines.extend(a.split_lines.iter().map(|l| [off_a + l[0], off_a + l[1]]));
        // the other's points and lines outside this one, at the heights they had
        let mut to = vec![None; b.points.len()];
        for (i, p) in b.points.iter().enumerate() {
            if within(&ra, Point::new(p.x, p.y)) { continue; }
            to[i] = Some(pts.len());
            pts.push(P3 { z: p.z + zb - za, ..*p });
        }
        for l in &b.split_lines {
            if let (Some(i), Some(j)) = (to[l[0]], to[l[1]]) {
                let m = Point::new((pts[i].x + pts[j].x) / 2.0, (pts[i].y + pts[j].y) / 2.0);
                if !crate::shape::inside(&ra, m) { lines.push([i, j]); }
            }
        }
        a.loops = self.check_floor_sketch(&ring_curves(&rings))?;
        let (ps, ls) = crate::shape::retain_within(&rings, &pts, &lines);
        (a.points, a.split_lines) = crate::shape::tidy(&rings, ps, ls, "toposolid")?;
        for c in &b.excavated_by { if !a.excavated_by.contains(c) { a.excavated_by.push(*c); } }
        a.excavated_by.retain(|c| *c != id && *c != other);
        // graded regions: the existing ground of both, this one's where they overlap
        a.existing = match (a.existing.take(), &b.existing) {
            (Some(x), Some(y)) => Some(crate::shape::join_tins(&x, y)),
            (x, y) => x.or_else(|| y.clone()),
        };
        self.check_toposolid(&a)?;
        let mut updates: Vec<(Id, Element)> = vec![];
        for (i, x) in self.toposolids() {
            if i == id || i == other { continue; }
            let mut x = x.clone();
            let mut changed = false;
            if x.host == Some(other) { x.host = Some(id); changed = true; }
            if x.excavated_by.contains(&other) {
                x.excavated_by.retain(|c| *c != other);
                if !x.excavated_by.contains(&id) { x.excavated_by.push(id); }
                changed = true;
            }
            if changed { updates.push((i, Element::Toposolid(x))); }
        }
        for (i, s) in self.spots() {
            if s.host == Some(other) { updates.push((i, Element::Spot(crate::spot::Spot { host: Some(id), ..s.clone() }))); }
        }
        self.transact("Merge toposolids", |tx| {
            tx.update(id, Element::Toposolid(a))?;
            for (i, e) in updates { tx.update(i, e)?; }
            tx.remove(other)
        })
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
        let Some((tin, bot, _)) = self.toposolid_surface(t) else { return vec![] };
        let th = self.toposolid_thickness(t);
        let n = tin.verts.len();
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
            // nothing left where an excavation goes right through
            if k.iter().all(|&i| tin.verts[i][2] - bot[i] < 0.5) && th > 0.0 { continue; }
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
    /// with a band per layer below it. The layers lie as they would without excavations, cut off by the excavated top.
    pub(crate) fn toposolid_cut_items(&self, id: Id, t: &Toposolid, sl: &crate::section::Slice, plane: f64) -> Vec<crate::site::ElevItem> {
        use crate::site::ElevItem;
        let (Some(tin), Some((dug, _, _))) = (self.toposolid_tin(t), self.toposolid_surface(t)) else { return vec![] };
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
        // a sub-division's reaches down to the host's top when it stands higher than its thickness
        let fixed = ty.and_then(|ty| ty.variable_layer().map(|v| th - ty.layers[v].thickness_mm)).filter(|_| t.host.is_none());
        let flat = self.toposolid_base_z(t) - th;
        let deep = if t.host.is_some() { th.max(t.offset_mm) } else { th };
        let bottom = |top: f64| match fixed { Some(f) => flat.min(top - f), None => top - deep };
        // with no variable layer, the last one reaches the underside
        let spans = |top: f64| -> Vec<(f64, f64)> {
            match ty {
                Some(ty) => {
                    let b = bottom(top);
                    let mut s = ty.layer_spans(top, b);
                    if ty.variable_layer().is_none() { if let Some(l) = s.last_mut() { l.1 = l.1.min(b); } }
                    s
                }
                None => vec![(top, top - 2.0)],
            }
        };
        // the excavated top has an edge along every edge of the unexcavated one, so its profile has every corner of
        // both: each point of it is along the cut, the excavated top and the unexcavated one
        let edges = dug.edges();
        let mut out = vec![];
        for cut in sl.cuts(&rings, plane) {
            let mut prof: Vec<[f64; 3]> = vec![];
            for ([i, j], _) in &edges {
                let (a, b) = (dug.verts[*i], dug.verts[*j]);
                let (da, db) = (sl.d(Point::new(a[0], a[1])) - plane, sl.d(Point::new(b[0], b[1])) - plane);
                if (da < 0.0) == (db < 0.0) { continue; }
                let s = da / (da - db);
                let p = Point::new(a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s);
                let u = sl.u(p);
                if u < cut.u0 - 1e-6 || u > cut.u1 + 1e-6 { continue; }
                let z = a[2] + (b[2] - a[2]) * s;
                prof.push([u, z, tin.z_near(p).unwrap_or(z)]);
            }
            for (u, p) in [(cut.u0, cut.p0), (cut.u1, cut.p1)] {
                if !prof.iter().any(|q| (q[0] - u).abs() < 1e-3) {
                    if let (Some(z), Some(g)) = (dug.z_near(p), tin.z_near(p)) { prof.push([u, z, g]); }
                }
            }
            prof.sort_by(|a, b| a[0].total_cmp(&b[0]));
            prof.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-6);
            if prof.len() < 2 { continue; }
            // where a flat underside starts to follow the top down
            if let Some(f) = fixed {
                let mut with = vec![prof[0]];
                for w in prof.windows(2) {
                    let (ga, gb) = (w[0][2] - f - flat, w[1][2] - f - flat);
                    if (ga < 0.0) != (gb < 0.0) {
                        let s = ga / (ga - gb);
                        with.push([0, 1, 2].map(|k| w[0][k] + (w[1][k] - w[0][k]) * s));
                    }
                    with.push(w[1]);
                }
                prof = with;
            }
            let at: Vec<Vec<(f64, f64)>> = prof.iter().map(|q| spans(q[2]).into_iter().map(|(hi, lo)| (hi.min(q[1]), lo.min(q[1]))).collect()).collect();
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

/// A solid's volume (mm³) from its top and the underside under each corner: over each triangle, its plan area by
/// the mean depth at its corners.
fn solid_volume(tin: &Tin, bot: &[f64]) -> f64 {
    tin.tris.iter().map(|k| {
        let (a, b, c) = (tin.verts[k[0]], tin.verts[k[1]], tin.verts[k[2]]);
        let area = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0;
        let depth = k.iter().map(|&i| (tin.verts[i][2] - bot[i]).max(0.0)).sum::<f64>() / 3.0;
        area * depth
    }).sum()
}

/// Where segments a-b and c-d meet, ends included: the fraction along each.
fn seg_cross(a: Point, b: Point, c: Point, d: Point) -> Option<(f64, f64)> {
    let (rx, ry, sx, sy) = (b.x - a.x, b.y - a.y, d.x - c.x, d.y - c.y);
    let den = rx * sy - ry * sx;
    if den.abs() < 1e-9 { return None; }
    let t = ((c.x - a.x) * sy - (c.y - a.y) * sx) / den;
    let u = ((c.x - a.x) * ry - (c.y - a.y) * rx) / den;
    let e = 1e-9;
    ((-e..=1.0 + e).contains(&t) && (-e..=1.0 + e).contains(&u)).then_some((t.clamp(0.0, 1.0), u.clamp(0.0, 1.0)))
}

/// Rings as a sketch of straight lines.
fn ring_curves(rings: &[Vec<Point>]) -> Vec<Curve> {
    rings.iter().flat_map(|r| (0..r.len()).map(move |i| Curve::line(r[i], r[(i + 1) % r.len()]))).collect()
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
        d.set_toposolid_points(id, pts.clone(), None).unwrap();
        assert_eq!(d.toposolid(id).unwrap().points.len(), 122);
        let mut out = pts.clone();
        out[0].x = -50.0;
        assert!(d.set_toposolid_points(id, out, None).is_err(), "outside the boundary");
        let removed = d.simplify_toposolid(id, 0.1).unwrap();
        let left = &d.toposolid(id).unwrap().points;
        assert_eq!(removed, 122 - left.len());
        assert!(left.len() <= 13 && left.len() >= 3, "{}", left.len());
        assert!(left.iter().any(|p| p.z == 5000.0), "the peak stays");
        d.undo();
        assert_eq!(d.toposolid(id).unwrap().points.len(), 122);
    }

    #[test]
    fn a_roof_sunk_into_the_ground_excavates_it_to_its_underside() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        // ground 1000 up, so its underside is at the level
        let t = d.add_toposolid_sketch(ty, level, square(10000.0), 1000.0).unwrap();
        let before = d.toposolid_volume(d.toposolid(t).unwrap());
        let rt = d.roof_types()[0].0;
        let c = [Point::new(0.0, 2000.0), Point::new(10000.0, 2000.0), Point::new(10000.0, 8000.0), Point::new(0.0, 8000.0)];
        let edges = (0..4).map(|i| crate::roof::RoofEdge::new(Curve::line(c[i], c[(i + 1) % 4]))).collect();
        let roof = d.add_roof_sketch(rt, level, edges).unwrap();
        assert_eq!(d.excavate_toposolid(t, &[roof]).unwrap(), 1);
        let topo = d.toposolid(t).unwrap().clone();
        assert!(d.toposolid_volume(&topo) < before);
        let ((z, _, _), bottom) = d.toposolid_sample(&topo, Point::new(100.0, 5000.0)).unwrap();
        // near the eaves the roof's underside is below the ground's, so nothing is left
        assert!((z - bottom).abs() < 1e-6 && (bottom - lz).abs() < 1e-6, "{z} {bottom}");
        // under the ridge the ground stands at the roof's underside, or its own top where that is lower
        let g = d.roof_geometry(d.roof(roof).unwrap()).unwrap();
        let rth = d.roof_type(rt).unwrap().thickness_mm();
        let under = g.top_z - rth / 22.5f64.to_radians().cos();
        let ((z, _, _), _) = d.toposolid_sample(&topo, Point::new(5000.0, 5000.0)).unwrap();
        assert!((z - under.min(lz + 1000.0)).abs() < 1e-3, "{z} {under}");
        // outside the roof the ground is untouched
        let ((z, _, _), _) = d.toposolid_sample(&topo, Point::new(8000.0, 9000.0)).unwrap();
        assert!((z - (lz + 1000.0)).abs() < 1e-6);
        assert_eq!(d.remove_excavation(t, &[roof]).unwrap(), 1);
        assert!((d.toposolid_volume(d.toposolid(t).unwrap()) - before).abs() < 1e-3);
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Curve> {
        let p = [Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
        (0..4).map(|i| Curve::line(p[i], p[(i + 1) % 4])).collect()
    }
    fn top(d: &Document, id: Id, x: f64, y: f64) -> f64 { d.toposolid_sample(d.toposolid(id).unwrap(), Point::new(x, y)).unwrap().0 .0 }
    fn vol(d: &Document, id: Id) -> f64 { d.toposolid_volume(d.toposolid(id).unwrap()) }

    #[test]
    fn a_sub_division_follows_its_host_and_excavates_it() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let host = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        d.add_toposolid_point(host, Point::new(5000.0, 5000.0), lz + 1800.0).unwrap();
        let before = vol(&d, host);
        let g = top(&d, host, 3000.0, 4000.0);
        // a 150 mm kerbed island of the 1000 mm type: it stands 150 above the ground and goes 850 into it
        let sub = d.add_toposolid_subdivision(host, ty, rect(2000.0, 2000.0, 6000.0, 6000.0), 150.0).unwrap();
        assert!((top(&d, sub, 3000.0, 4000.0) - (g + 150.0)).abs() < 1e-6);
        assert!((top(&d, host, 3000.0, 4000.0) - (g - 850.0)).abs() < 1e-3);
        let area = 4000.0 * 4000.0;
        // the side of the hole leans out over 1 mm
        assert!((before - vol(&d, host) - area * 850.0).abs() < 1e7, "{}", before - vol(&d, host));
        assert!((d.subdivision_excavation(sub) - area * 850.0).abs() < 1e7);
        assert!((vol(&d, sub) - area * 1000.0).abs() < 1e3);
        // standing higher than its thickness it reaches down to the ground, and takes nothing out
        d.patch_toposolid(sub, &serde_json::json!({ "offset_mm": 1500.0 })).unwrap();
        assert!((vol(&d, host) - before).abs() < 1e3);
        assert!((vol(&d, sub) - area * 1500.0).abs() < 1e3);
        // below the ground it takes out all it covers, down to the host's underside
        d.patch_toposolid(sub, &serde_json::json!({ "offset_mm": -300.0 })).unwrap();
        assert!((before - vol(&d, host) - area * 1000.0).abs() < 1e7);
        assert!((top(&d, sub, 3000.0, 4000.0) - (g - 300.0)).abs() < 1e-6);
        // it follows the host's points, and has none of its own
        assert!(d.add_toposolid_point(sub, Point::new(3000.0, 3000.0), 0.0).is_err());
        assert!(d.add_toposolid_subdivision(sub, ty, rect(3000.0, 3000.0, 4000.0, 4000.0), 0.0).is_err());
        assert!(d.excavate_toposolid(host, &[sub]).is_err());
        d.add_toposolid_point(host, Point::new(3000.0, 4000.0), lz + 500.0).unwrap();
        assert!((top(&d, sub, 3000.0, 4000.0) - (lz + 200.0)).abs() < 1e-6);
        // it goes where its host goes, and can't leave it
        d.transform_elements(&[host], crate::edit::Xform::Move { dx: 1000.0, dy: 0.0 }, Default::default()).unwrap();
        assert!((top(&d, sub, 4000.0, 4000.0) - (lz + 200.0)).abs() < 1e-6);
        assert!(d.transform_elements(&[sub], crate::edit::Xform::Move { dx: 50_000.0, dy: 0.0 }, Default::default()).is_err());
        // off the edge it is cut off by the host's boundary
        let edge = d.add_toposolid_subdivision(host, ty, rect(9000.0, 4000.0, 14_000.0, 5000.0), 0.0).unwrap();
        let s = d.toposolid_tin(d.toposolid(edge).unwrap()).unwrap();
        assert!((s.plan_area() - 2000.0 * 1000.0).abs() < 1.0, "{}", s.plan_area());
        assert!(d.add_toposolid_subdivision(host, ty, rect(30_000.0, 0.0, 31_000.0, 1000.0), 0.0).is_err());
        // deleting the host takes it; undo brings both back
        d.delete(&[host]).unwrap();
        assert!(d.toposolid(sub).is_none());
        d.undo();
        assert_eq!(d.toposolid(sub).unwrap().host, Some(host));
        // copied with its host, it goes on the copy
        let copies = d.transform_elements(&[host], crate::edit::Xform::Move { dx: 0.0, dy: 20_000.0 }, crate::edit::EditOptions { copy: true, ..Default::default() }).unwrap();
        assert_eq!(copies.len(), 3);
        let on_copy = d.toposolid_subdivisions(copies[0]);
        assert_eq!(on_copy.len(), 2);
    }

    #[test]
    fn split_keeps_the_ground_and_takes_the_sub_divisions_along() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), lz + 1800.0).unwrap();
        d.add_toposolid_point(id, Point::new(7000.0, 3000.0), lz - 600.0).unwrap();
        d.add_toposolid_split_line(id, Point::new(1000.0, 1000.0), Point::new(9000.0, 3000.0)).unwrap();
        let across = d.add_toposolid_subdivision(id, ty, rect(4000.0, 7000.0, 6000.0, 9000.0), 100.0).unwrap();
        let right = d.add_toposolid_subdivision(id, ty, rect(7000.0, 6000.0, 9000.0, 8000.0), 100.0).unwrap();
        let samples = [(3000.0, 5000.0), (5000.0, 5000.0), (6500.0, 2500.0), (8000.0, 9000.0), (2000.0, 1250.0), (5000.0, 2000.0), (4990.0, 2600.0)];
        let t0 = d.toposolid(id).unwrap().clone();
        let before: Vec<f64> = samples.iter().map(|&(x, y)| d.toposolid_tin(&t0).unwrap().z_at(Point::new(x, y)).unwrap()).collect();
        let v0 = d.toposolid_volume(&t0);
        // a line from edge to edge, running on past them
        assert!(d.split_toposolid(id, &[Curve::line(Point::new(5000.0, 4000.0), Point::new(5000.0, 6000.0))]).is_err(), "it has to cut it in two");
        let ids = d.split_toposolid(id, &[Curve::line(Point::new(5000.0, -1000.0), Point::new(5000.0, 11_000.0))]).unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], id);
        for (k, &(x, y)) in samples.iter().enumerate() {
            let p = Point::new(x, y);
            let on = ids.iter().find(|&&i| within(&d.toposolid(i).unwrap().polygons(20.0), p)).unwrap();
            let z = d.toposolid_tin(d.toposolid(*on).unwrap()).unwrap().z_at(p).unwrap();
            assert!((z - before[k]).abs() < 1e-6, "at {x},{y}: {z} was {}", before[k]);
        }
        // the split line is broken where the cut crosses it, and each piece keeps its half
        for &i in &ids {
            let t = d.toposolid(i).unwrap();
            assert!(t.split_lines.iter().any(|l| [l[0], l[1]].iter().any(|&k| (t.points[k].x - 5000.0).abs() < 1e-6 && (t.points[k].y - 2000.0).abs() < 1e-6)));
        }
        let unsub = |d: &Document, i: Id| { let t = d.toposolid(i).unwrap(); let tin = d.toposolid_tin(t).unwrap(); solid_volume(&tin, &d.toposolid_bottoms(t, &tin)) };
        let total: f64 = ids.iter().map(|&i| unsub(&d, i)).sum();
        assert!((total - unsub_t(&d, &t0)).abs() < 1e3, "{total}");
        let _ = v0;
        // the sub-division across the cut is split with it; the other goes with its piece
        assert_eq!(d.toposolid(right).unwrap().host, Some(ids[1]));
        assert_eq!(d.toposolid(across).unwrap().host.is_some(), true);
        assert_eq!(d.toposolid_subdivisions(ids[0]).len(), 1);
        assert_eq!(d.toposolid_subdivisions(ids[1]).len(), 2);
        d.undo();
        assert_eq!(d.toposolids().iter().filter(|(_, t)| t.host.is_none()).count(), 1);
        assert_eq!(d.toposolid_subdivisions(id).len(), 2);
        // a closed loop inside makes an island and a toposolid round it
        let loop_ids = d.split_toposolid(id, &rect(1000.0, 6000.0, 3000.0, 9000.0)).unwrap();
        assert_eq!(loop_ids.len(), 2);
        let outer = d.toposolid(id).unwrap();
        assert_eq!(outer.loops.len(), 2, "the outer piece has a hole");
        let total: f64 = loop_ids.iter().map(|&i| unsub(&d, i)).sum();
        assert!((total - unsub_t(&d, &t0)).abs() < 1e3, "{total}");
    }

    fn unsub_t(d: &Document, t: &Toposolid) -> f64 { let tin = d.toposolid_tin(t).unwrap(); solid_volume(&tin, &d.toposolid_bottoms(t, &tin)) }

    #[test]
    fn merge_joins_two_toposolids_and_keeps_both_grounds() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let a = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        d.add_toposolid_point(a, Point::new(4000.0, 6000.0), lz + 700.0).unwrap();
        let b = d.add_toposolid_sketch(ty, level, rect(10_000.0, 0.0, 20_000.0, 10_000.0), 500.0).unwrap();
        d.add_toposolid_point(b, Point::new(15_000.0, 5000.0), lz + 2500.0).unwrap();
        let sub = d.add_toposolid_subdivision(b, ty, rect(16_000.0, 1000.0, 18_000.0, 3000.0), 0.0).unwrap();
        let far = d.add_toposolid_sketch(ty, level, rect(30_000.0, 0.0, 40_000.0, 10_000.0), 0.0).unwrap();
        assert!(d.merge_toposolids(a, far).is_err(), "apart");
        let ta = d.toposolid(a).unwrap().clone();
        let a_tin = d.toposolid_tin(&ta).unwrap();
        d.merge_toposolids(a, b).unwrap();
        assert!(d.toposolid(b).is_none());
        let m = d.toposolid(a).unwrap().clone();
        assert_eq!(m.loops.len(), 1);
        let tin = d.toposolid_tin(&m).unwrap();
        assert!((tin.plan_area() - 2e8).abs() < 1.0);
        // this one's ground is as it was; the other's keeps its points
        for (x, y) in [(4000.0, 6000.0), (2000.0, 2000.0), (9000.0, 8000.0), (9999.0, 5000.0)] {
            let p = Point::new(x, y);
            assert!((tin.z_at(p).unwrap() - a_tin.z_at(p).unwrap()).abs() < 1e-6, "at {x},{y}");
        }
        assert!((tin.z_at(Point::new(15_000.0, 5000.0)).unwrap() - (lz + 2500.0)).abs() < 1e-6);
        assert!((tin.z_at(Point::new(20_000.0, 10_000.0)).unwrap() - (lz + 500.0)).abs() < 1e-6);
        assert_eq!(d.toposolid(sub).unwrap().host, Some(a));
        d.undo();
        assert!(d.toposolid(b).is_some());
        assert_eq!(d.toposolid(sub).unwrap().host, Some(b));
    }

    // a sub-division's edge on the seam left by a split and merge lies a hair off the seam's split line; that used to
    // stop the triangulation
    #[test]
    fn a_split_and_merge_round_trip_keeps_the_ground_and_its_sub_divisions() {
        let (mut d, ty, level) = setup();
        let id = d.add_toposolid_sketch(ty, level, rect(-10000.0, -10000.0, 10000.0, 10000.0), 1000.0).unwrap();
        d.add_toposolid_point(id, Point::new(-10000.0, -10000.0), -2000.0).unwrap();
        d.add_toposolid_point(id, Point::new(10000.0, 10000.0), 0.0).unwrap();
        d.add_toposolid_point(id, Point::new(0.0, 0.0), -500.0).unwrap();
        d.add_toposolid_subdivision(id, ty, rect(-2000.0, -2000.0, 2000.0, 2000.0), -300.0).unwrap();
        let v0 = vol(&d, id);
        let ids = d.split_toposolid(id, &[Curve::line(Point::new(-11000.0, -5000.0), Point::new(966.563, 79.505)), Curve::line(Point::new(966.563, 79.505), Point::new(11033.056, 4989.262))]).unwrap();
        let split: f64 = ids.iter().map(|&i| vol(&d, i)).sum();
        assert!((split - v0).abs() < 1e6, "{split} {v0}");
        d.merge_toposolids(ids[0], ids[1]).unwrap();
        assert!((vol(&d, id) - v0).abs() < 1e6);
        let subs = d.toposolid_subdivisions(id);
        assert_eq!(subs.len(), 2);
        let dug: f64 = subs.iter().map(|&s| d.subdivision_excavation(s)).sum();
        let held: f64 = subs.iter().map(|&s| vol(&d, s)).sum();
        assert!((held - 16e9).abs() < 1e6 && (dug - 16e9).abs() < 2e7, "{held} {dug}");
    }

    fn cf(d: &Document, id: Id) -> (f64, f64) { d.toposolid_cut_fill(d.toposolid(id).unwrap()).unwrap() }

    #[test]
    fn a_graded_region_measures_cut_and_fill_from_the_ground_as_it_was() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        assert!(d.toposolid_cut_fill(d.toposolid(id).unwrap()).is_none());
        d.grade_toposolid(id, false).unwrap();
        assert_eq!(cf(&d, id), (0.0, 0.0));
        assert!(d.grade_toposolid(id, false).is_err(), "graded once");
        // a mound in the middle is fill: a pyramid, a third of its base by its height
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), lz + 1000.0).unwrap();
        let (cut, fill) = cf(&d, id);
        assert!(cut.abs() < 1.0 && (fill - 100e6 * 1000.0 / 3.0).abs() < 1e3, "{cut} {fill}");
        // a hollow is cut
        let t = d.toposolid(id).unwrap().clone();
        d.set_toposolid_points(id, vec![P3 { z: -1000.0, ..t.points[0] }], None).unwrap();
        let (cut, fill) = cf(&d, id);
        assert!(fill.abs() < 1.0 && (cut - 100e6 * 1000.0 / 3.0).abs() < 1e3, "{cut} {fill}");
        // it moves with the toposolid
        d.transform_elements(&[id], crate::edit::Xform::Move { dx: 3000.0, dy: -2000.0 }, Default::default()).unwrap();
        let (c2, f2) = cf(&d, id);
        assert!((c2 - cut).abs() < 1e3 && f2.abs() < 1.0, "{c2} {f2}");
        d.transform_elements(&[id], crate::edit::Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 30.0 }, Default::default()).unwrap();
        let (c3, _) = cf(&d, id);
        assert!((c3 - cut).abs() < 1e3, "{c3}");
        // Reset Shape flattens it again: all of the hollow is filled back
        d.reset_toposolid_shape(id).unwrap();
        assert!(d.toposolid(id).unwrap().points.is_empty());
        assert!(d.reset_toposolid_shape(id).is_err(), "already flat");
        assert_eq!(cf(&d, id), (0.0, 0.0));
        d.undo();
        // removing the graded region forgets the old ground; undo brings it back
        d.remove_graded_region(id).unwrap();
        assert!(d.toposolid(id).unwrap().existing.is_none());
        d.undo();
        assert!(d.toposolid(id).unwrap().existing.is_some());
    }

    #[test]
    fn cut_counts_what_excavations_take_out_and_perimeter_only_drops_the_inside() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), lz + 1000.0).unwrap();
        d.add_toposolid_point(id, Point::new(10_000.0, 5000.0), lz + 600.0).unwrap();
        // perimeter only: the mound goes, the point on the edge stays, and losing the mound is cut
        let before = d.toposolid(id).unwrap().clone();
        let old = d.toposolid_tin(&before).unwrap();
        d.grade_toposolid(id, true).unwrap();
        let t = d.toposolid(id).unwrap().clone();
        assert!(t.points.iter().all(|p| p.x.min(p.y) < 1.0 || p.x.max(p.y) > 9999.0), "{:?}", t.points);
        assert!((top(&d, id, 10_000.0, 5000.0) - (lz + 600.0)).abs() < 1e-6);
        assert_eq!(t.existing.as_ref(), Some(&old));
        // the mound's contours stay as existing contours
        let ex = d.toposolid_existing_contours(&t);
        assert!(!ex.is_empty() && ex != d.toposolid_contours(&t));
        let (cut, fill) = cf(&d, id);
        let lost = solid_volume(&old, &vec![lz; old.verts.len()]) - solid_volume(&d.toposolid_tin(&t).unwrap(), &vec![lz; d.toposolid_tin(&t).unwrap().verts.len()]);
        assert!(fill.abs() < 1.0 && (cut - lost).abs() < 1e3, "{cut} {fill} {lost}");
        // a sub-division sunk 200 into it excavates it by its type's thickness below that, down to its underside (1000
        // below its top): all cut
        let flat = d.add_toposolid_sketch(ty, level, rect(20_000.0, 0.0, 30_000.0, 10_000.0), 0.0).unwrap();
        d.grade_toposolid(flat, false).unwrap();
        d.add_toposolid_subdivision(flat, ty, rect(22_000.0, 2000.0, 24_000.0, 4000.0), -200.0).unwrap();
        let (cut, fill) = cf(&d, flat);
        assert!(fill.abs() < 1.0 && (cut - 4e6 * 1000.0).abs() < 2e7, "{cut} {fill}");
    }

    #[test]
    fn split_and_merge_keep_the_existing_ground() {
        let (mut d, ty, level) = setup();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        d.grade_toposolid(id, false).unwrap();
        d.add_toposolid_point(id, Point::new(5000.0, 5000.0), lz + 1000.0).unwrap();
        let (_, fill) = cf(&d, id);
        let ids = d.split_toposolid(id, &[Curve::line(Point::new(5000.0, -100.0), Point::new(5000.0, 10_100.0))]).unwrap();
        assert_eq!(ids.len(), 2);
        let sum: f64 = ids.iter().map(|&i| cf(&d, i).1).sum();
        assert!((sum - fill).abs() < 1e4, "{sum} {fill}");
        d.merge_toposolids(ids[0], ids[1]).unwrap();
        let (c, f) = cf(&d, ids[0]);
        assert!(c.abs() < 1e3 && (f - fill).abs() < 1e4, "{c} {f} {fill}");
    }

    #[test]
    fn each_layer_is_its_own_solid_in_3d() {
        let (mut d, ty, level) = setup();
        let mut t2 = d.toposolid_type(ty).unwrap().clone();
        t2.layers = vec![Layer::new("Earth", 300.0, LayerFunction::Finish1, false), Layer::new("Earth", 700.0, LayerFunction::Structure, true)];
        d.transact("t", |tx| tx.update(ty, Element::ToposolidType(t2))).unwrap();
        let lz = d.level(level).unwrap().elevation_mm;
        let id = d.add_toposolid_sketch(ty, level, square(10_000.0), 0.0).unwrap();
        let meshes = crate::mesh::toposolid_layer_meshes(&d, d.toposolid(id).unwrap());
        assert_eq!(meshes.len(), 2);
        // mesh coordinates are metres
        let zs = |m: &Vec<f32>| m.chunks(6).map(|c| c[2] as f64 * 1000.0).fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(z), b.max(z)));
        let (lo0, hi0) = zs(&meshes[0].1);
        let (lo1, hi1) = zs(&meshes[1].1);
        assert!((hi0 - lz).abs() < 1.0 && (lo0 - (lz - 300.0)).abs() < 1.0, "{lo0} {hi0}");
        assert!((hi1 - (lz - 300.0)).abs() < 1.0 && (lo1 - (lz - 1000.0)).abs() < 1.0, "{lo1} {hi1}");
    }

    #[test]
    fn the_positive_part_of_a_plane_over_a_triangle() {
        let f = crate::shape::cut_fill(&[vec![Point::new(0.0, 0.0), Point::new(1000.0, 0.0), Point::new(0.0, 1000.0)]],
            &Tin { verts: vec![[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, 1000.0, 0.0]], tris: vec![[0, 1, 2]] },
            &Tin { verts: vec![[0.0, 0.0, 1.0], [1000.0, 0.0, -1.0], [0.0, 1000.0, -1.0]], tris: vec![[0, 1, 2]] });
        // fill over the small triangle at the raised corner, cut over the rest; fill less cut is the whole
        assert!((f.1 - 5e5 / 12.0).abs() < 1e-3 && (f.0 - 5e5 * 5.0 / 12.0).abs() < 1e-3, "{f:?}");
    }
}
