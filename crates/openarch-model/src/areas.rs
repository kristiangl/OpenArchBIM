//! Areas: area schemes (Gross Building, Rentable...), area plans that belong to one scheme and level, the area
//! boundary lines drawn in an area plan, the areas that fill them, and area tags.
//!
//! An area plan's boundaries are its own: an area fills the face its boundary lines enclose round its point (and is
//! not enclosed until they close). Walls do not bound areas; Create Area Plan can draw boundary lines on the
//! outside of the building, and Pick Walls in the boundary sketch draws them where the scheme's rule puts them.

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::geom::Region;
use crate::view::{View, ViewKind};
use crate::{Document, Element, Id, Point};

/// Which rules a scheme's areas measure by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemeRules {
    /// The whole building to the outside face of its exterior walls.
    #[default]
    GrossBuilding,
    /// The office floor-area method: wall centrelines between tenancies, faces at exterior walls and shafts.
    Rentable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaScheme {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub rules: SchemeRules,
    /// The Gross Building scheme cannot be edited or deleted.
    #[serde(default)]
    pub built_in: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaType {
    #[default]
    GrossBuildingArea,
    ExteriorArea,
    BuildingCommonArea,
    OfficeArea,
    FloorArea,
    MajorVerticalPenetration,
    StoreArea,
}

impl AreaType {
    pub fn label(self) -> &'static str {
        match self {
            AreaType::GrossBuildingArea => "Gross Building Area",
            AreaType::ExteriorArea => "Exterior Area",
            AreaType::BuildingCommonArea => "Building Common Area",
            AreaType::OfficeArea => "Office Area",
            AreaType::FloorArea => "Floor Area",
            AreaType::MajorVerticalPenetration => "Major Vertical Penetration",
            AreaType::StoreArea => "Store Area",
        }
    }
    /// The area types a scheme's areas can take.
    pub fn of(rules: SchemeRules) -> &'static [AreaType] {
        use AreaType::*;
        match rules {
            SchemeRules::GrossBuilding => &[GrossBuildingArea, ExteriorArea],
            SchemeRules::Rentable => &[BuildingCommonArea, OfficeArea, ExteriorArea, FloorArea, MajorVerticalPenetration, StoreArea],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaBoundary {
    /// The area plan it belongs to.
    pub view: Id,
    pub curve: Curve,
    /// The wall it was picked from, kept for boundaries that follow their walls later.
    #[serde(default)]
    pub wall: Option<Id>,
    /// Placed by the scheme's area rules (Apply Area Rules).
    #[serde(default)]
    pub apply_rules: bool,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Area {
    /// The area plan it belongs to (its scheme and level come from there).
    pub view: Id,
    /// Where it was placed; None: not placed (it keeps its data, as a schedule row).
    #[serde(default)]
    pub at: Option<Point>,
    pub number: String,
    pub name: String,
    #[serde(default)]
    pub area_type: AreaType,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaTag {
    pub view: Id,
    pub area: Id,
    pub at: Point,
    #[serde(default)]
    pub family: Option<Id>,
    #[serde(default)]
    pub leader: bool,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaStatus { Placed, NotPlaced, NotEnclosed, Redundant }

#[derive(Debug, Clone)]
pub struct AreaCalculation { pub status: AreaStatus, pub region: Option<Region>, pub area_mm2: f64, pub perimeter_mm: f64 }

impl Document {
    pub fn area_schemes(&self) -> Vec<(Id, &AreaScheme)> {
        self.iter().filter_map(|(i, e)| if let Element::AreaScheme(s) = e { Some((i, s)) } else { None }).collect()
    }
    pub fn area_scheme(&self, id: Id) -> Option<&AreaScheme> { match self.get(id)? { Element::AreaScheme(s) => Some(s), _ => None } }
    pub fn areas(&self) -> Vec<(Id, &Area)> { self.iter().filter_map(|(i, e)| if let Element::Area(a) = e { Some((i, a)) } else { None }).collect() }
    pub fn area(&self, id: Id) -> Option<&Area> { match self.get(id)? { Element::Area(a) => Some(a), _ => None } }
    pub fn area_boundaries(&self, view: Id) -> Vec<(Id, &AreaBoundary)> { self.iter().filter_map(|(i, e)| match e { Element::AreaBoundary(b) if b.view == view => Some((i, b)), _ => None }).collect() }
    pub fn area_tags(&self, view: Id) -> Vec<(Id, &AreaTag)> { self.iter().filter_map(|(i, e)| match e { Element::AreaTag(t) if t.view == view => Some((i, t)), _ => None }).collect() }

    /// An area's scheme, through its area plan.
    pub fn scheme_of_area(&self, a: &Area) -> Option<Id> { self.view(a.view)?.area_scheme }
    pub fn level_of_area(&self, a: &Area) -> Option<Id> { self.view(a.view)?.level }

    /// The two starter schemes, added to a project that has none.
    pub fn ensure_area_schemes(&mut self) -> Result<(), String> {
        if !self.area_schemes().is_empty() { return Ok(()); }
        self.transact("Area schemes", |tx| {
            tx.add(Element::AreaScheme(AreaScheme { name: "Gross Building".into(), description: "Total constructed area of a building".into(), rules: SchemeRules::GrossBuilding, built_in: true }));
            tx.add(Element::AreaScheme(AreaScheme { name: "Rentable".into(), description: "Area measurements based on the standard method for measuring floor area in office buildings".into(), rules: SchemeRules::Rentable, built_in: false }));
            Ok(())
        })
    }

    /// Create Area Plan: one plan per level in `scheme`, named after the level (numbered when the name is taken), with
    /// boundary lines round the outside of the building when `boundaries` is set. One undo step; the plans in order.
    pub fn create_area_plans(&mut self, scheme: Id, levels: &[Id], boundaries: bool) -> Result<Vec<Id>, String> {
        let s = self.area_scheme(scheme).ok_or("Choose an area scheme.")?.clone();
        if levels.is_empty() { return Err("Choose at least one level.".into()); }
        let mut plans = vec![];
        for &level in levels {
            let l = self.level(level).ok_or("unknown level")?;
            let taken = |n: &str| self.views().iter().any(|(_, v)| v.view_kind == ViewKind::AreaPlan && v.area_scheme == Some(scheme) && v.name == n);
            let mut name = l.name.clone();
            let mut k = 1;
            while taken(&name) { name = format!("{}({k})", l.name); k += 1; }
            let outline = if boundaries { self.building_outlines(level, l.elevation_mm + 1200.0) } else { vec![] };
            plans.push((View::area_plan(&name, level, scheme), outline));
        }
        let _ = s;
        self.transact("Create area plan", |tx| {
            let mut ids = vec![];
            for (v, outline) in plans {
                let id = tx.add(Element::View(v));
                for ring in outline {
                    for i in 0..ring.len() {
                        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                        if a.dist(&b) > 1.0 { tx.add(Element::AreaBoundary(AreaBoundary { view: id, curve: Curve::line(a, b), wall: None, apply_rules: true, pinned: false })); }
                    }
                }
                ids.push(id);
            }
            Ok(ids)
        })
    }

    fn area_plan_check(&self, view: Id) -> Result<(), String> {
        match self.view(view) { Some(v) if v.view_kind == ViewKind::AreaPlan && v.area_scheme.is_some() => Ok(()), _ => Err("Open an area plan.".into()) }
    }

    /// Area boundary lines drawn in an area plan (a sketch's curves), one undo step.
    pub fn add_area_boundaries(&mut self, view: Id, curves: Vec<Curve>, apply_rules: bool) -> Result<Vec<Id>, String> {
        self.area_plan_check(view)?;
        for c in &curves { if c.length() < 0.1 || c.tessellate(1.5).iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) { return Err("Invalid area boundary line.".into()); } }
        self.transact("Area boundary", |tx| Ok(curves.into_iter().map(|curve| tx.add(Element::AreaBoundary(AreaBoundary { view, curve, wall: None, apply_rules, pinned: false }))).collect()))
    }

    /// Where Pick Walls puts an area boundary for a wall in a scheme: Gross Building on the wall's exterior face,
    /// Rentable on its centreline (faces at exterior walls and shafts come with area types bordering each other).
    pub fn area_boundary_for_wall(&self, view: Id, wall: Id) -> Result<Curve, String> {
        let scheme = self.view(view).and_then(|v| v.area_scheme).and_then(|s| self.area_scheme(s)).ok_or("Open an area plan.")?;
        let Some(Element::Wall(w)) = self.get(wall) else { return Err("Pick a wall.".into()) };
        let (left, right) = self.wall_extents(w);
        // the exterior face is on the wall's left, looking from start to end
        let off = match scheme.rules { SchemeRules::GrossBuilding => left, SchemeRules::Rentable => (left - right) / 2.0 };
        let path = w.path();
        let len = w.length();
        if len < 1.0 { return Err("The wall is too short.".into()); }
        let d = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
        let n = Point::new(-d.y, d.x);
        let _ = path;
        if w.curve.is_some() { return Err("Pick a straight wall, or draw the boundary for a curved one.".into()); }
        // run on past each end by the wall's thickness, to meet boundaries on the faces of the walls it joins
        let reach = left + right + 10.0;
        let (a, b) = (Point::new(w.start.x - d.x * reach, w.start.y - d.y * reach), Point::new(w.end.x + d.x * reach, w.end.y + d.y * reach));
        Ok(Curve::line(Point::new(a.x + n.x * off, a.y + n.y * off), Point::new(b.x + n.x * off, b.y + n.y * off)))
    }

    /// The faces an area plan's boundary lines enclose.
    pub fn area_faces(&self, view: Id) -> Vec<Region> {
        let clips: Vec<Vec<Point>> = self.area_boundaries(view).iter().flat_map(|(_, b)| b.curve.tessellate(1.5).windows(2).map(|p| crate::rooms::line_strip(p[0], p[1])).collect::<Vec<_>>()).collect();
        crate::rooms::faces_of_clips(&clips).0
    }

    pub fn area_calculation(&self, id: Id) -> Option<AreaCalculation> {
        let a = self.area(id)?;
        let mut c = AreaCalculation { status: AreaStatus::NotPlaced, region: None, area_mm2: 0.0, perimeter_mm: 0.0 };
        let Some(at) = a.at else { return Some(c) };
        c.status = AreaStatus::NotEnclosed;
        if let Some(region) = self.area_faces(a.view).into_iter().find(|r| crate::rooms::contains(r, at)) {
            let earlier = self.areas().into_iter().any(|(other, o)| other < id && o.view == a.view && o.at.is_some_and(|p| crate::rooms::contains(&region, p)));
            if earlier { c.status = AreaStatus::Redundant; return Some(c); }
            c.status = AreaStatus::Placed;
            c.area_mm2 = region.area();
            c.perimeter_mm = std::iter::once(&region.outer).chain(&region.holes).map(|ring| (0..ring.len()).map(|i| ring[i].dist(&ring[(i + 1) % ring.len()])).sum::<f64>()).sum();
            c.region = Some(region);
        }
        Some(c)
    }

    /// The next area number in a scheme: the most recent number counted up.
    fn next_area_number(&self, scheme: Option<Id>) -> String {
        let nums: Vec<String> = self.areas().into_iter().filter(|(_, a)| self.scheme_of_area(a) == scheme).map(|(_, a)| a.number.clone()).collect();
        let last = nums.last().cloned().unwrap_or_else(|| "0".into());
        let start = last.char_indices().rev().take_while(|(_, c)| c.is_ascii_digit()).last().map_or(last.len(), |(i, _)| i);
        let (prefix, digits) = last.split_at(start);
        let mut n: u64 = digits.parse().unwrap_or(0);
        loop { n += 1; let s = format!("{prefix}{n:0w$}", w = digits.len()); if !nums.contains(&s) { return s; } }
    }

    /// Place an area in an area plan (None: an unplaced area, a schedule row), tagged there when `tag`.
    pub fn add_area(&mut self, view: Id, at: Option<Point>, tag: bool) -> Result<Id, String> {
        self.area_plan_check(view)?;
        if at.is_some_and(|p| !p.x.is_finite() || !p.y.is_finite()) { return Err("Invalid area location".into()); }
        let scheme = self.view(view).and_then(|v| v.area_scheme);
        let rules = scheme.and_then(|s| self.area_scheme(s)).map_or(SchemeRules::GrossBuilding, |s| s.rules);
        let area = Area { view, at, number: self.next_area_number(scheme), name: "Area".into(), area_type: AreaType::of(rules)[0], comments: String::new(), pinned: false };
        self.grouped("Place area", |d| {
            let id = d.transact("Area", |tx| Ok(tx.add(Element::Area(area))))?;
            if let (true, Some(p)) = (tag, at) { d.add_area_tag(view, id, p)?; }
            Ok(id)
        })
    }

    pub fn patch_area(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.area(id).cloned().ok_or("not an area")?;
        let mut a: Area = crate::doc::apply_patch(&old, patch)?;
        a.view = old.view;
        if old.pinned && a.pinned && a.at != old.at { return Err("Unpin the area before moving it.".into()); }
        if a.number.trim().is_empty() || a.name.trim().is_empty() { return Err("An area needs a name and number.".into()); }
        let rules = self.scheme_of_area(&a).and_then(|s| self.area_scheme(s)).map_or(SchemeRules::GrossBuilding, |s| s.rules);
        if !AreaType::of(rules).contains(&a.area_type) { return Err(format!("{} is not an area type of this scheme.", a.area_type.label())); }
        self.transact("Area properties", |tx| tx.update(id, Element::Area(a)))
    }

    /// Delete in an area plan: the area leaves the plan and its tags go, its data stays (a schedule row).
    pub fn unplace_area(&mut self, id: Id) -> Result<(), String> {
        let tags: Vec<Id> = self.iter().filter_map(|(t, e)| matches!(e, Element::AreaTag(x) if x.area == id).then_some(t)).collect();
        self.grouped("Unplace area", |d| {
            d.patch_area(id, &serde_json::json!({ "at": null }))?;
            d.transact("Remove area tags", |tx| { for t in tags { tx.remove(t)?; } Ok(()) })
        })
    }

    pub fn add_area_tag(&mut self, view: Id, area: Id, at: Point) -> Result<Id, String> {
        if self.area(area).is_none() { return Err("Unknown area".into()); }
        self.area_plan_check(view)?;
        let family = self.ensure_area_tag_family()?;
        self.transact("Tag area", |tx| Ok(tx.add(Element::AreaTag(AreaTag { view, area, at, family: Some(family), leader: false, pinned: false }))))
    }

    /// The area under a point in an area plan.
    pub fn area_at(&self, view: Id, at: Point) -> Option<Id> {
        let region = self.area_faces(view).into_iter().find(|r| crate::rooms::contains(r, at))?;
        self.areas().into_iter().filter(|(_, a)| a.view == view && a.at.is_some_and(|p| crate::rooms::contains(&region, p))).map(|(id, _)| id).min()
    }

    /// Place an area in every enclosed face of an area plan that has none, one undo step.
    pub fn place_all_areas(&mut self, view: Id, tag: bool) -> Result<Vec<Id>, String> {
        let points: Vec<Point> = self.area_faces(view).iter().filter_map(|r| { let (pts, tris) = crate::geom::triangulate_region(r)?; let t = tris.first()?; Some(Point::new((pts[t[0]].x + pts[t[1]].x + pts[t[2]].x) / 3.0, (pts[t[0]].y + pts[t[1]].y + pts[t[2]].y) / 3.0)) }).filter(|p| self.area_at(view, *p).is_none()).collect();
        self.grouped("Place areas automatically", |d| points.into_iter().map(|p| d.add_area(view, Some(p), tag)).collect())
    }
}
