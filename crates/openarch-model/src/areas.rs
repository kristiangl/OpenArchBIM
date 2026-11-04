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
    /// The wall it was picked from: the line follows the wall, and `curve` only keeps its last place (the line
    /// stays there when the wall is deleted).
    #[serde(default)]
    pub wall: Option<Id>,
    /// Placed by the scheme's area rules (Apply Area Rules): on a wall's centreline, one of its faces or its glass,
    /// by the area types each side.
    #[serde(default)]
    pub apply_rules: bool,
    /// Without the rules, how far a wall's boundary sits from the wall's centre, toward its exterior side (mm).
    #[serde(default)]
    pub offset_mm: f64,
    #[serde(default)]
    pub pinned: bool,
}

/// Where the rules put a wall's boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallLine { Centre, Left, Right, Glass }

/// Which side the area rules put a boundary toward, between areas of types `a` and `b` (None: the wall
/// centreline). Outside the building counts as Exterior Area.
pub fn rule_toward(a: AreaType, b: AreaType) -> Option<AreaType> {
    use AreaType::*;
    if a == b { return None; }
    let tenant = |t: AreaType| matches!(t, BuildingCommonArea | OfficeArea | StoreArea);
    let outer = |t: AreaType| matches!(t, ExteriorArea | MajorVerticalPenetration);
    // (first, second) -> Some(true): toward the first, Some(false): toward the second, None: centreline
    let rule = |x: AreaType, y: AreaType| -> Option<Option<bool>> {
        if tenant(x) && tenant(y) { return Some(None); }
        if x == StoreArea && y == ExteriorArea { return Some(Some(false)); }
        if tenant(x) && outer(y) { return Some(Some(true)); }
        if x == FloorArea && tenant(y) { return Some(Some(false)); }
        if x == FloorArea && outer(y) { return Some(Some(true)); }
        if x == MajorVerticalPenetration && y == ExteriorArea { return Some(Some(true)); }
        if x == GrossBuildingArea && y == ExteriorArea { return Some(Some(false)); }
        None
    };
    match rule(a, b).map(|r| r.map(|first| if first { a } else { b })) {
        Some(r) => r,
        None => rule(b, a).and_then(|r| r.map(|first| if first { b } else { a })),
    }
}

/// A wall's boundary `off` to the left of its location line, run on past each end by `reach`.
fn wall_boundary_curve(w: &crate::element::Wall, off: f64, reach: f64) -> Option<Curve> {
    let polar = |c: Point, r: f64, a: f64| Point::new(c.x + r * a.cos(), c.y + r * a.sin());
    match &w.curve {
        None => {
            let len = w.start.dist(&w.end);
            if len < 1.0 { return None; }
            let d = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
            let n = Point::new(-d.y, d.x);
            Some(Curve::line(Point::new(w.start.x + n.x * off - d.x * reach, w.start.y + n.y * off - d.y * reach), Point::new(w.end.x + n.x * off + d.x * reach, w.end.y + n.y * off + d.y * reach)))
        }
        Some(Curve::Arc { start, end, centre, ccw }) => {
            let r = centre.dist(start);
            // the left of travel is toward the centre on a counter-clockwise arc
            let nr = if *ccw { r - off } else { r + off };
            if nr < 1.0 { return None; }
            let da = if *ccw { reach / nr } else { -reach / nr };
            let (a0, a1) = ((start.y - centre.y).atan2(start.x - centre.x) - da, (end.y - centre.y).atan2(end.x - centre.x) + da);
            Some(Curve::Arc { start: polar(*centre, nr, a0), end: polar(*centre, nr, a1), centre: *centre, ccw: *ccw })
        }
        Some(_) => {
            let path = w.path();
            let len = path.length();
            if len < 1.0 { return None; }
            let steps = ((len / 300.0).ceil() as usize).clamp(4, 200);
            let mut pts = vec![];
            for k in 0..=steps {
                let (p, d) = path.at(len * k as f64 / steps as f64);
                let q = Point::new(p.x - d.y * off, p.y + d.x * off);
                if k == 0 { pts.push(Point::new(q.x - d.x * reach, q.y - d.y * reach)); }
                pts.push(q);
                if k == steps { pts.push(Point::new(q.x + d.x * reach, q.y + d.y * reach)); }
            }
            Some(Curve::Spline { points: pts, closed: false })
        }
    }
}

/// Where the segment p0-p1 crosses q0-q1, as the fraction along p0-p1.
fn crossing(p0: Point, p1: Point, q0: Point, q1: Point) -> Option<f64> {
    let (r, s) = (Point::new(p1.x - p0.x, p1.y - p0.y), Point::new(q1.x - q0.x, q1.y - q0.y));
    let den = r.x * s.y - r.y * s.x;
    if den.abs() < 1e-12 { return None; }
    let (wx, wy) = (q0.x - p0.x, q0.y - p0.y);
    let t = (wx * s.y - wy * s.x) / den;
    let u = (wx * r.y - wy * r.x) / den;
    ((-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-6..=1.0 + 1e-6).contains(&u)).then_some(t)
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

    /// A new area scheme, measuring by Gross Building or Rentable rules.
    pub fn add_area_scheme(&mut self, name: &str, description: &str, rules: SchemeRules) -> Result<Id, String> {
        self.ensure_area_schemes()?;
        let name = name.trim();
        if name.is_empty() { return Err("An area scheme needs a name.".into()); }
        if self.area_schemes().iter().any(|(_, s)| s.name == name) { return Err(format!("There is already an area scheme called {name}.")); }
        self.transact("New area scheme", |tx| Ok(tx.add(Element::AreaScheme(AreaScheme { name: name.into(), description: description.trim().into(), rules, built_in: false }))))
    }

    /// Rename or describe a scheme. The Gross Building scheme cannot change, and a scheme with areas keeps its rules
    /// (their area types belong to them).
    pub fn patch_area_scheme(&mut self, id: Id, name: &str, description: &str, rules: SchemeRules) -> Result<(), String> {
        let old = self.area_scheme(id).ok_or("Unknown area scheme.")?.clone();
        if old.built_in { return Err("The Gross Building scheme cannot be changed.".into()); }
        let name = name.trim();
        if name.is_empty() { return Err("An area scheme needs a name.".into()); }
        if self.area_schemes().iter().any(|(o, s)| *o != id && s.name == name) { return Err(format!("There is already an area scheme called {name}.")); }
        if rules != old.rules && self.areas().iter().any(|(_, a)| self.scheme_of_area(a) == Some(id)) { return Err("The scheme has areas: its rules cannot change.".into()); }
        let s = AreaScheme { name: name.into(), description: description.trim().into(), rules, built_in: false };
        self.transact("Area scheme", |tx| tx.update(id, Element::AreaScheme(s)))
    }

    /// Delete a scheme with its area plans (their boundaries, areas and tags) and its colour schemes.
    pub fn delete_area_scheme(&mut self, id: Id) -> Result<(), String> {
        let s = self.area_scheme(id).ok_or("Unknown area scheme.")?;
        if s.built_in { return Err("The Gross Building scheme cannot be deleted.".into()); }
        let plans: Vec<Id> = self.views().iter().filter(|(_, v)| v.area_scheme == Some(id)).map(|(v, _)| *v).collect();
        let colors: Vec<Id> = self.color_fill_schemes().iter().filter(|(_, c)| c.category == crate::colorfill::ColorCategory::Areas { scheme: id }).map(|(c, _)| *c).collect();
        self.grouped("Delete area scheme", |d| {
            for c in colors { d.delete_color_scheme(c)?; }
            if !plans.is_empty() { d.delete_items(&plans)?; }
            d.transact("Delete area scheme", |tx| tx.remove(id))
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
            let (walls, outline) = if boundaries { self.exterior_walls(level, l.elevation_mm + 1200.0) } else { (vec![], vec![]) };
            plans.push((View::area_plan(&name, level, scheme), walls, outline));
        }
        let _ = s;
        self.transact("Create area plan", |tx| {
            let mut ids = vec![];
            for (v, walls, outline) in plans {
                let id = tx.add(Element::View(v));
                for (wall, curve) in walls { tx.add(Element::AreaBoundary(AreaBoundary { view: id, curve, wall: Some(wall), apply_rules: true, offset_mm: 0.0, pinned: false })); }
                for ring in outline {
                    for i in 0..ring.len() {
                        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                        if a.dist(&b) > 1.0 { tx.add(Element::AreaBoundary(AreaBoundary { view: id, curve: Curve::line(a, b), wall: None, apply_rules: true, offset_mm: 0.0, pinned: false })); }
                    }
                }
                ids.push(id);
            }
            Ok(ids)
        })
    }

    /// The walls round the outside of the building on a level at height `z` (one face outside, the other in), with
    /// their centrelines; when they do not close round anything, no walls and the building's outline instead.
    fn exterior_walls(&self, level: Id, z: f64) -> (Vec<(Id, Curve)>, Vec<Vec<Point>>) {
        let outline = self.building_outlines(level, z);
        let outside = |p: Point| !outline.iter().any(|ring| crate::curve::point_in_polygon(p, ring));
        let mut walls = vec![];
        for (id, e) in self.iter() {
            let Element::Wall(w) = e else { continue };
            let (lo, hi) = self.wall_extent(w);
            if !w.room_bounding || z < lo - 0.01 || z >= hi - 0.001 { continue; }
            let path = w.path();
            if path.length() < 1.0 { continue; }
            let (p, d) = path.at(path.length() / 2.0);
            let (l, r) = self.wall_extents(w);
            let n = Point::new(-d.y, d.x);
            let side = |k: f64| Point::new(p.x + n.x * k, p.y + n.y * k);
            if outside(side(l + 5.0)) == outside(side(-r - 5.0)) { continue; }
            if let Some(c) = wall_boundary_curve(w, (l - r) / 2.0, 0.0) { walls.push((id, c)); }
        }
        let reach: Vec<Curve> = walls.iter().filter_map(|(id, _)| match self.get(*id) { Some(Element::Wall(w)) => { let (l, r) = self.wall_extents(w); wall_boundary_curve(w, (l - r) / 2.0, l + r + 10.0) } _ => None }).collect();
        if walls.is_empty() || faces_of_curves(reach.iter()).is_empty() { (vec![], outline) } else { (walls, vec![]) }
    }

    fn area_plan_check(&self, view: Id) -> Result<(), String> {
        match self.view(view) { Some(v) if v.view_kind == ViewKind::AreaPlan && v.area_scheme.is_some() => Ok(()), _ => Err("Open an area plan.".into()) }
    }

    /// Area boundary lines drawn in an area plan (a sketch's curves), one undo step.
    pub fn add_area_boundaries(&mut self, view: Id, curves: Vec<Curve>, apply_rules: bool) -> Result<Vec<Id>, String> {
        self.area_plan_check(view)?;
        for c in &curves { if c.length() < 0.1 || c.tessellate(1.5).iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) { return Err("Invalid area boundary line.".into()); } }
        self.transact("Area boundary", |tx| Ok(curves.into_iter().map(|curve| tx.add(Element::AreaBoundary(AreaBoundary { view, curve, wall: None, apply_rules, offset_mm: 0.0, pinned: false }))).collect()))
    }

    /// Pick Walls: boundaries that follow `walls` in an area plan, placed by the area rules or at `offset_mm` from
    /// each wall's centre toward its exterior. A wall that already has one in this plan is skipped. One undo step.
    pub fn add_wall_area_boundaries(&mut self, view: Id, walls: &[Id], apply_rules: bool, offset_mm: f64) -> Result<Vec<Id>, String> {
        self.area_plan_check(view)?;
        if !offset_mm.is_finite() { return Err("Invalid offset.".into()); }
        let taken: Vec<Id> = self.area_boundaries(view).iter().filter_map(|(_, b)| b.wall).collect();
        let mut new = vec![];
        for &id in walls {
            let Some(Element::Wall(w)) = self.get(id) else { return Err("Pick a wall.".into()) };
            if taken.contains(&id) || new.iter().any(|b: &AreaBoundary| b.wall == Some(id)) { continue; }
            let (l, r) = self.wall_extents(w);
            let curve = wall_boundary_curve(w, (l - r) / 2.0, 0.0).ok_or("The wall is too short.")?;
            new.push(AreaBoundary { view, curve, wall: Some(id), apply_rules, offset_mm, pinned: false });
        }
        if new.is_empty() { return Err("That wall already has an area boundary in this plan.".into()); }
        self.transact("Area boundary", |tx| Ok(new.into_iter().map(|b| tx.add(Element::AreaBoundary(b))).collect()))
    }

    /// Where a wall's boundary sits across it by the area rules, from the area types each side of it.
    fn rule_line(&self, rules: SchemeRules, wall: Id, left: Option<AreaType>, right: Option<AreaType>) -> WallLine {
        let ext = AreaType::ExteriorArea;
        let (l, r) = (left.unwrap_or(ext), right.unwrap_or(ext));
        let Some(t) = rule_toward(l, r) else { return WallLine::Centre };
        let toward_left = t == l && t != r;
        // Rentable: beside the outside, a wall whose windows are over half its height measures to the glass
        if rules == SchemeRules::Rentable && (l == ext) != (r == ext) && t != ext && self.wall_glazed(wall) { return WallLine::Glass; }
        if toward_left { WallLine::Left } else { WallLine::Right }
    }

    /// A curtain wall, or a wall with a window taller than half the wall.
    fn wall_glazed(&self, id: Id) -> bool {
        let Some(Element::Wall(w)) = self.get(id) else { return false };
        if self.is_curtain_wall(id) { return true; }
        let (lo, hi) = self.wall_extent(w);
        self.hosted_in(&[id]).into_iter().any(|i| {
            let window = match self.get(i) { Some(Element::FamilyInstance(fi)) => self.instance_family(fi).is_some_and(|(f, _)| f.category == crate::family::FamilyCategory::Windows), _ => false };
            window && self.instance_frame(i).is_some_and(|f| f.height > (hi - lo) / 2.0)
        })
    }

    /// The lines an area plan's boundaries make now: those that follow walls at their walls (by the area rules
    /// from the areas each side, or at their offsets), trimmed where they meet the boundaries they run on to.
    pub fn area_boundary_curves(&self, view: Id) -> std::sync::Arc<Vec<(Id, Curve)>> {
        self.room_cache_reset();
        if let Some(c) = self.room_cache.borrow().area_lines.get(&view) { return c.clone(); }
        let rules = self.view(view).and_then(|v| v.area_scheme).and_then(|s| self.area_scheme(s)).map_or(SchemeRules::GrossBuilding, |s| s.rules);
        let bounds = self.area_boundaries(view);
        let wall_of = |b: &AreaBoundary| b.wall.and_then(|w| match self.get(w) { Some(Element::Wall(w)) => Some(w), _ => None });
        // the walls' lines at their centres first (or offsets), to see which areas lie each side
        let at = |w: &crate::element::Wall, off_from_centre: f64| { let (l, r) = self.wall_extents(w); wall_boundary_curve(w, (l - r) / 2.0 + off_from_centre, l + r + 10.0) };
        let first: Vec<(Id, Curve)> = bounds.iter().map(|(id, b)| (*id, match wall_of(b) { Some(w) => at(w, if b.apply_rules { 0.0 } else { b.offset_mm }).unwrap_or_else(|| b.curve.clone()), None => b.curve.clone() })).collect();
        let ruled = bounds.iter().any(|(_, b)| b.apply_rules && wall_of(b).is_some());
        let faces = if ruled { faces_of_curves(first.iter().map(|(_, c)| c)) } else { vec![] };
        let areas: Vec<(Id, &Area)> = self.areas().into_iter().filter(|(_, a)| a.view == view && a.at.is_some()).collect();
        let default = AreaType::of(rules)[if rules == SchemeRules::Rentable { 1 } else { 0 }];
        let type_at = |p: Point| -> Option<AreaType> {
            let f = faces.iter().find(|r| crate::rooms::contains(r, p))?;
            Some(areas.iter().filter(|(_, a)| a.at.is_some_and(|q| crate::rooms::contains(f, q))).min_by_key(|(id, _)| *id).map_or(default, |(_, a)| a.area_type))
        };
        // (boundary, line, how far it runs past its start and end, to trim where it meets another boundary)
        let mut lines: Vec<(Id, Curve, Option<f64>, Option<f64>)> = vec![];
        for (id, b) in &bounds {
            let Some(w) = wall_of(b) else { lines.push((*id, b.curve.clone(), None, None)); continue };
            let wid = b.wall.unwrap();
            let (l, r) = self.wall_extents(w);
            let centre = (l - r) / 2.0;
            // run on past each end by the wall's thickness, to meet the boundaries of the walls it joins
            let reach = l + r + 10.0;
            let whole = |off: f64| wall_boundary_curve(w, off, reach).unwrap_or_else(|| b.curve.clone());
            if !b.apply_rules { lines.push((*id, whole(centre + b.offset_mm), Some(reach), Some(reach))); continue; }
            // where the rules put the line beside the point `p` of the location line (None: no area either side)
            let rule_off = |p: Point, d: Point| -> Option<f64> {
                let n = Point::new(-d.y, d.x);
                let s = (l + r) / 2.0 + 1.0;
                let side = |k: f64| type_at(Point::new(p.x + n.x * (centre + k * s), p.y + n.y * (centre + k * s)));
                let (a, c) = (side(1.0), side(-1.0));
                if a.is_none() && c.is_none() { return None; }
                Some(match self.rule_line(rules, wid, a, c) { WallLine::Centre | WallLine::Glass => centre, WallLine::Left => l, WallLine::Right => -r })
            };
            let len = w.start.dist(&w.end);
            if w.curve.is_some() || len < 1.0 {
                // a curved wall goes by the areas at its middle (or near it)
                let path = w.path();
                let off = [0.5, 0.4, 0.6, 0.3, 0.7].iter().find_map(|f| { let (p, d) = path.at(path.length() * f); rule_off(p, d) }).unwrap_or(centre);
                lines.push((*id, whole(off), Some(reach), Some(reach)));
                continue;
            }
            // a straight wall splits where other boundaries meet it, each part by the areas beside it
            let d = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
            let n = Point::new(-d.y, d.x);
            let at = |u: f64, off: f64| Point::new(w.start.x + d.x * u + n.x * off, w.start.y + d.y * u + n.y * off);
            let mut stops: Vec<f64> = first.iter().filter(|(o, _)| o != id).flat_map(|(_, c)| c.tessellate(1.5).windows(2).filter_map(|q| crossing(w.start, w.end, q[0], q[1])).collect::<Vec<_>>()).map(|t| t * len).filter(|u| *u > 1.0 && *u < len - 1.0).collect();
            stops.push(0.0);
            stops.push(len);
            stops.sort_by(f64::total_cmp);
            stops.dedup_by(|a, b| (*a - *b).abs() < 1.0);
            let mut parts: Vec<(f64, f64, Option<f64>)> = stops.windows(2).map(|s| (s[0], s[1], rule_off(at((s[0] + s[1]) / 2.0, 0.0), d))).collect();
            // a part with no area either side takes its neighbour's place
            for k in 1..parts.len() { if parts[k].2.is_none() { parts[k].2 = parts[k - 1].2; } }
            for k in (0..parts.len().saturating_sub(1)).rev() { if parts[k].2.is_none() { parts[k].2 = parts[k + 1].2; } }
            let mut merged: Vec<(f64, f64, f64)> = vec![];
            for (u0, u1, off) in parts {
                let off = off.unwrap_or(centre);
                match merged.last_mut() { Some(m) if (m.2 - off).abs() < 0.01 => m.1 = u1, _ => merged.push((u0, u1, off)) }
            }
            let last = merged.len() - 1;
            for (k, (u0, u1, off)) in merged.iter().copied().enumerate() {
                let (from, to) = (if k == 0 { u0 - reach } else { u0 }, if k == last { u1 + reach } else { u1 });
                lines.push((*id, Curve::line(at(from, off), at(to, off)), (k == 0).then_some(reach), (k == last).then_some(reach)));
                if k < last { lines.push((*id, Curve::line(at(u1, off), at(u1, merged[k + 1].2)), None, None)); }
            }
        }
        let segs: Vec<(Id, Vec<Point>)> = lines.iter().map(|(id, c, _, _)| (*id, c.tessellate(1.5))).collect();
        // trim each run-on end at its outermost crossing, from the run-on out to one wall thickness inside the end
        let out: Vec<(Id, Curve)> = lines.iter().map(|(id, c, rs, re)| {
            let Curve::Line { a, b } = c else { return (*id, c.clone()) };
            if rs.is_none() && re.is_none() { return (*id, c.clone()); }
            let len = a.dist(b);
            let ts: Vec<f64> = segs.iter().filter(|(o, _)| o != id).flat_map(|(_, pts)| pts.windows(2).filter_map(|q| crossing(*a, *b, q[0], q[1])).collect::<Vec<_>>()).collect();
            let t0 = match rs { Some(reach) if len > reach + 1.0 => { let r = reach / len; let t = ts.iter().copied().filter(|t| *t <= 2.0 * r + 1e-6).fold(f64::NAN, f64::min); if t.is_nan() { r } else { t } } _ => 0.0 };
            let t1 = match re { Some(reach) if len > reach + 1.0 => { let r = reach / len; let t = ts.iter().copied().filter(|t| *t >= 1.0 - 2.0 * r - 1e-6).fold(f64::NAN, f64::max); if t.is_nan() { 1.0 - r } else { t } } _ => 1.0 };
            if t1 - t0 < 1e-6 { return (*id, c.clone()); }
            let pt = |t: f64| Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            (*id, Curve::line(pt(t0), pt(t1)))
        }).collect();
        let out = std::sync::Arc::new(out);
        self.room_cache.borrow_mut().area_lines.insert(view, out.clone());
        out
    }

    /// The faces an area plan's boundary lines enclose.
    pub fn area_faces(&self, view: Id) -> std::sync::Arc<Vec<Region>> {
        self.room_cache_reset();
        if let Some(f) = self.room_cache.borrow().area_faces.get(&view) { return f.clone(); }
        let lines = self.area_boundary_curves(view);
        let faces = std::sync::Arc::new(faces_of_curves(lines.iter().map(|(_, c)| c)));
        self.room_cache.borrow_mut().area_faces.insert(view, faces.clone());
        faces
    }

    /// An area boundary's lines as they are now: one, or the parts a wall split into by the areas beside it, in
    /// order along the wall and joined end to end.
    pub fn area_boundary_pieces(&self, id: Id) -> Vec<Curve> {
        let Some(Element::AreaBoundary(b)) = self.get(id) else { return vec![] };
        self.area_boundary_curves(b.view).iter().filter(|(i, _)| *i == id).map(|(_, c)| c.clone()).collect()
    }

    /// An area boundary's line as it is now (the longest part of a split one).
    pub fn area_boundary_curve(&self, id: Id) -> Option<Curve> {
        self.area_boundary_pieces(id).into_iter().max_by(|a, b| a.length().total_cmp(&b.length()))
    }

    pub fn area_calculation(&self, id: Id) -> Option<AreaCalculation> {
        let a = self.area(id)?;
        let mut c = AreaCalculation { status: AreaStatus::NotPlaced, region: None, area_mm2: 0.0, perimeter_mm: 0.0 };
        let Some(at) = a.at else { return Some(c) };
        c.status = AreaStatus::NotEnclosed;
        if let Some(region) = self.area_faces(a.view).iter().find(|r| crate::rooms::contains(r, at)).cloned() {
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
        let faces = self.area_faces(view);
        let region = faces.iter().find(|r| crate::rooms::contains(r, at))?;
        self.areas().into_iter().filter(|(_, a)| a.view == view && a.at.is_some_and(|p| crate::rooms::contains(region, p))).map(|(id, _)| id).min()
    }

    /// Place an area in every enclosed face of an area plan that has none, one undo step.
    pub fn place_all_areas(&mut self, view: Id, tag: bool) -> Result<Vec<Id>, String> {
        let points: Vec<Point> = self.area_faces(view).iter().filter_map(|r| { let (pts, tris) = crate::geom::triangulate_region(r)?; let t = tris.first()?; Some(Point::new((pts[t[0]].x + pts[t[1]].x + pts[t[2]].x) / 3.0, (pts[t[0]].y + pts[t[1]].y + pts[t[2]].y) / 3.0)) }).filter(|p| self.area_at(view, *p).is_none()).collect();
        self.grouped("Place areas automatically", |d| points.into_iter().map(|p| d.add_area(view, Some(p), tag)).collect())
    }
}

/// The faces boundary lines enclose.
fn faces_of_curves<'a>(curves: impl Iterator<Item = &'a Curve>) -> Vec<Region> {
    let clips: Vec<Vec<Point>> = curves.flat_map(|c| c.tessellate(1.5).windows(2).map(|p| crate::rooms::line_strip(p[0], p[1])).collect::<Vec<_>>()).collect();
    crate::rooms::faces_of_clips(&clips).0
}
