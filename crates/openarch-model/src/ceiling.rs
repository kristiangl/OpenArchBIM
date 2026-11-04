//! Ceilings: sketched or automatic boundaries at a height above their level, with a ceiling type whose layers build
//! up from the underside (no layers: a basic ceiling, a plane with no thickness) and an optional ceiling grid.
//!
//! The geometry is a floor's: a ceiling is a slab whose top lies at its underside plus its thickness, so the floor
//! surface, slope arrow and mesh code serve both (`as_slab`).

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::element::{default_coarse_fill, Floor, IfcParams, Layer, LayerFunction, SlopeArrow, TypeInfo};
use crate::styles::PatternGraphics;
use crate::geom::Region;
use crate::rooms::RoomBoundary;
use crate::{Document, Element, Id, Point};

/// A new ceiling's height above its level when none is given (mm).
pub const DEFAULT_HEIGHT_MM: f64 = 2400.0;

fn yes() -> bool { true }
fn default_height() -> f64 { DEFAULT_HEIGHT_MM }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CeilingType {
    pub name: String,
    /// The layers, top first as a floor's are (the last one faces the room); none for a basic ceiling.
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default = "default_coarse_fill")]
    pub coarse_fill: PatternGraphics,
    #[serde(default)]
    pub info: TypeInfo,
    /// The ceiling grid drawn in reflected ceiling plans: spacing across and along (mm). None for a plain ceiling.
    #[serde(default)]
    pub grid: Option<[f64; 2]>,
}

impl CeilingType {
    pub fn thickness_mm(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness_mm).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ceiling {
    pub ceiling_type: Id,
    pub level: Id,
    /// Closed loops of curves; loops inside others are holes.
    pub loops: Vec<Vec<Curve>>,
    /// Height Offset From Level: where the underside sits above the level.
    #[serde(default = "default_height")]
    pub offset_mm: f64,
    #[serde(default)]
    pub slope: Option<SlopeArrow>,
    #[serde(default = "yes")]
    pub room_bounding: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub ifc: IfcParams,
    /// Where the ceiling grid starts and how it is turned (Move and Rotate of the grid lines come later).
    #[serde(default)]
    pub grid_origin: Option<Point>,
    #[serde(default)]
    pub grid_angle_deg: f64,
}

impl Ceiling {
    pub fn new(ceiling_type: Id, level: Id, loops: Vec<Vec<Curve>>, offset_mm: f64) -> Ceiling {
        Ceiling { ceiling_type, level, loops, offset_mm, slope: None, room_bounding: true, pinned: false, comments: String::new(), mark: String::new(), ifc: IfcParams { guid: crate::element::new_ifc_guid(), ..Default::default() }, grid_origin: None, grid_angle_deg: 0.0 }
    }

    /// The ceiling as a slab `thickness` thick: its top at the underside plus the thickness.
    pub fn as_slab(&self, thickness: f64) -> Floor {
        let mut f = Floor::new(self.ceiling_type, self.level, self.loops.clone());
        f.offset_mm = self.offset_mm + thickness;
        f.slope = self.slope.clone();
        f.room_bounding = self.room_bounding;
        f
    }

    pub fn regions(&self, tol: f64) -> Vec<Region> {
        self.as_slab(0.0).regions(tol)
    }
}

impl Document {
    pub fn ceiling_types(&self) -> Vec<(Id, &CeilingType)> {
        self.iter().filter_map(|(i, e)| if let Element::CeilingType(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn ceiling_type(&self, id: Id) -> Option<&CeilingType> {
        match self.get(id)? { Element::CeilingType(t) => Some(t), _ => None }
    }

    pub fn ceilings(&self) -> Vec<(Id, &Ceiling)> {
        self.iter().filter_map(|(i, e)| if let Element::Ceiling(c) = e { Some((i, c)) } else { None }).collect()
    }

    pub fn ceiling(&self, id: Id) -> Option<&Ceiling> {
        match self.get(id)? { Element::Ceiling(c) => Some(c), _ => None }
    }

    pub fn ceiling_thickness(&self, c: &Ceiling) -> f64 {
        self.ceiling_type(c.ceiling_type).map_or(0.0, |t| t.thickness_mm())
    }

    /// The ceiling's slab: its top surface and thickness, for meshes, sections and quantities.
    pub fn ceiling_slab(&self, c: &Ceiling) -> Floor {
        c.as_slab(self.ceiling_thickness(c))
    }

    /// The starter ceiling types, added to a project that has none: plasterboard, and a 1200 x 600 suspended grid.
    pub fn ensure_ceiling_types(&mut self) -> Result<(), String> {
        if !self.ceiling_types().is_empty() { return Ok(()); }
        // projects made before ceilings lack the tile material too
        let materials: Vec<_> = crate::styles::starter_materials().into_iter().filter(|m| m.name == "Ceiling Tile" && self.material(&m.name).is_none()).collect();
        self.transact("Ceiling types", |tx| {
            for m in materials { tx.add(Element::Material(m)); }
            for t in starter_ceiling_types() { tx.add(Element::CeilingType(t)); }
            Ok(())
        })
    }

    fn check_ceiling(&self, c: &Ceiling) -> Result<(), String> {
        if self.ceiling_type(c.ceiling_type).is_none() { return Err("unknown ceiling type".into()); }
        if self.level(c.level).is_none() { return Err("unknown level".into()); }
        if !c.offset_mm.is_finite() || !c.grid_angle_deg.is_finite() { return Err("the height offset must be a number".into()); }
        self.check_floor_sketch(&c.loops.concat())?;
        Ok(())
    }

    /// Sketch Ceiling: a ceiling from the sketch's loose curves, its underside `offset_mm` above the level.
    pub fn add_ceiling_sketch(&mut self, ceiling_type: Id, level: Id, curves: Vec<Curve>, offset_mm: f64, slope: Option<SlopeArrow>) -> Result<Id, String> {
        let loops = self.check_floor_sketch(&curves)?;
        let mut c = Ceiling::new(ceiling_type, level, loops, offset_mm);
        c.slope = slope;
        self.check_ceiling(&c)?;
        self.transact("Create ceiling", |tx| Ok(tx.add(Element::Ceiling(c))))
    }

    /// Edit Boundary's Finish: the ceiling's new sketch, one undo step.
    pub fn set_ceiling_sketch(&mut self, id: Id, curves: Vec<Curve>, slope: Option<SlopeArrow>) -> Result<(), String> {
        let mut c = self.ceiling(id).cloned().ok_or("not a ceiling")?;
        self.refuse_pinned(&[id], "edited")?;
        c.loops = self.check_floor_sketch(&curves)?;
        c.slope = slope;
        self.check_ceiling(&c)?;
        self.transact("Edit ceiling boundary", |tx| tx.update(id, Element::Ceiling(c)))
    }

    pub fn patch_ceiling(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.ceiling(id).cloned().ok_or("not a ceiling")?;
        let c: Ceiling = crate::doc::apply_patch(&old, patch)?;
        if old.pinned && c.pinned && (c.loops != old.loops || c.level != old.level || c.offset_mm != old.offset_mm) {
            return Err("Unpin the ceiling before moving it.".into());
        }
        self.check_ceiling(&c)?;
        self.transact("Ceiling properties", |tx| tx.update(id, Element::Ceiling(c)))
    }

    /// Automatic Ceiling: the area enclosed by walls round `at` at the ceiling's height (inside wall faces,
    /// room-bounding columns; room separation lines do not bound a ceiling). None outside any enclosure.
    pub fn ceiling_region_at(&self, level: Id, offset_mm: f64, at: Point) -> Option<Region> {
        let z = self.level(level)?.elevation_mm + offset_mm;
        self.enclosed_faces(level, z, RoomBoundary::Finish, false).into_iter().find(|r| crate::rooms::contains(r, at))
    }

    pub fn add_automatic_ceiling(&mut self, ceiling_type: Id, level: Id, offset_mm: f64, at: Point) -> Result<Id, String> {
        let region = self.ceiling_region_at(level, offset_mm, at).ok_or("Click inside walls that form a closed loop at the ceiling's height.")?;
        let ring = |pts: &[Point]| -> Vec<Curve> { (0..pts.len()).map(|i| Curve::line(pts[i], pts[(i + 1) % pts.len()])).collect() };
        let curves: Vec<Curve> = std::iter::once(&region.outer).chain(region.holes.iter()).flat_map(|r| ring(r)).collect();
        self.add_ceiling_sketch(ceiling_type, level, curves, offset_mm, None)
    }
}

pub(crate) fn starter_ceiling_types() -> Vec<CeilingType> {
    let fill = || default_coarse_fill();
    vec![
        CeilingType { name: "Plasterboard 13".into(), layers: vec![Layer::new("Plasterboard", 13.0, LayerFunction::Finish2, true)], coarse_fill: fill(), info: TypeInfo::default(), grid: None },
        CeilingType { name: "Suspended Grid 1200 x 600".into(), layers: vec![Layer::new("Ceiling Tile", 15.0, LayerFunction::Finish2, true)], coarse_fill: fill(), info: TypeInfo::default(), grid: Some([600.0, 1200.0]) },
    ]
}
