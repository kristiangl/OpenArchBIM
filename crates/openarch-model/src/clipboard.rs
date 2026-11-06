//! The clipboard: Copy and Cut take a snapshot of elements, and Paste adds new elements from it, where the user picks,
//! in the same place, aligned to the current view, to other levels or into other views.
//!
//! A snapshot keeps the elements as they were, so a Cut can be pasted after the originals are gone. Pasting adds the
//! elements again (walls before the doors and windows they host, rooms, clouds and the like before their tags),
//! moves model elements to the target level by the same number of levels up or down, puts view-specific elements
//! (detail lines, text, tags, areas...) in the target view, and then moves the lot with the Move tool's rules.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::Curve;
use crate::edit::{EditOptions, Xform};
use crate::element::*;
use crate::view::ViewKind;
use crate::{Document, HostRef, Point};

#[derive(Debug, Clone)]
pub struct Clipboard {
    /// The copied elements as they were.
    pub elements: Vec<(Id, Element)>,
    /// The view they were copied from: Same Place puts view-specific elements back in it, and the level change of
    /// a paste counts from its level.
    pub view: Option<Id>,
    /// Where each hosted door or window sat on its wall, to find a wall when it is pasted without its own.
    pub hosted_at: BTreeMap<Id, Point>,
    /// The middle of what was copied: Paste from Clipboard puts this point where the user clicks.
    pub origin: Point,
    /// What could not be copied (dimensions, groups, levels, views...), by kind.
    pub skipped: BTreeMap<&'static str, usize>,
}

/// Where a paste goes.
#[derive(Debug, Clone)]
pub enum PasteTarget {
    /// Paste from Clipboard: the middle of the copied elements at `at`, in `view` (model elements onto its level).
    Point { view: Id, at: Point },
    /// Aligned to Current View: the same plan position, onto the view's level and into the view.
    CurrentView { view: Id },
    /// Aligned to Same Place: where they were, on their own levels and views.
    SamePlace,
    /// Aligned to Selected Levels: model elements onto each level, the same plan position.
    Levels(Vec<Id>),
    /// Aligned to Selected Views: view-specific elements into each view (views of the copied view's kind).
    Views(Vec<Id>),
}

impl Clipboard {
    pub fn has_model(&self) -> bool { self.elements.iter().any(|(_, e)| !view_specific(e)) }
    pub fn has_view_specific(&self) -> bool { self.elements.iter().any(|(_, e)| view_specific(e)) }
    pub fn count(&self) -> usize { self.elements.len() }

    /// Outlines to draw while placing a paste: lines along walls and curves, and small crosses at points.
    pub fn preview(&self) -> Vec<Vec<Point>> {
        let mut out = vec![];
        let cross = |p: Point, out: &mut Vec<Vec<Point>>| { out.push(vec![Point::new(p.x - 150.0, p.y - 150.0), Point::new(p.x + 150.0, p.y + 150.0)]); out.push(vec![Point::new(p.x - 150.0, p.y + 150.0), Point::new(p.x + 150.0, p.y - 150.0)]); };
        let curve = |c: &Curve, out: &mut Vec<Vec<Point>>| out.push(c.tessellate(20.0));
        for (id, e) in &self.elements {
            match e {
                Element::Wall(w) => out.push(w.path().pts.clone()),
                Element::Floor(f) => for l in f.loops() { for c in l { curve(&c, &mut out); } },
                Element::Ceiling(c) => for l in &c.loops { for k in l { curve(k, &mut out); } },
                Element::Stair(s) => for r in &s.runs { out.push(vec![r.start, r.end]); },
                Element::Railing(r) => out.push(r.path.clone()),
                Element::Ramp(r) => for x in &r.runs { out.push(vec![x.start, x.end]); },
                Element::Roof(r) => { let crate::roof::RoofShape::Footprint { loops } = &r.shape; for l in loops { for k in l { curve(&k.curve, &mut out); } } }
                Element::RoomSeparation(s) => curve(&s.curve, &mut out),
                Element::Grid(g) => for c in &g.curves { curve(c, &mut out); },
                Element::DetailLine(l) => curve(&l.curve, &mut out),
                Element::DetailRegion(r) => for e in r.loops.iter().flatten() { curve(&e.curve, &mut out); },
                Element::AreaBoundary(b) => curve(&b.curve, &mut out),
                Element::RevisionCloud(c) => for k in &c.curves { curve(k, &mut out); },
                Element::FamilyInstance(i) => cross(self.hosted_at.get(id).copied().unwrap_or(i.location), &mut out),
                _ => if let Some(p) = point_of(e) { cross(p, &mut out); },
            }
        }
        out
    }
}

/// Whether an element belongs to one view (and so pastes into views rather than onto levels).
pub fn view_specific(e: &Element) -> bool {
    matches!(e, Element::DetailLine(_) | Element::DetailRegion(_) | Element::TextNote(_) | Element::RoomTag(_) | Element::ElementTag(_) | Element::AreaTag(_) | Element::Area(_) | Element::AreaBoundary(_) | Element::RevisionCloud(_) | Element::RevisionTag(_) | Element::ColorFillLegend(_))
}

fn view_of(e: &Element) -> Option<Id> {
    match e {
        Element::DetailLine(l) => Some(l.view), Element::DetailRegion(r) => Some(r.view), Element::TextNote(t) => Some(t.view),
        Element::RoomTag(t) => Some(t.view), Element::ElementTag(t) => Some(t.view), Element::AreaTag(t) => Some(t.view), Element::Area(a) => Some(a.view),
        Element::AreaBoundary(b) => Some(b.view), Element::RevisionCloud(c) => Some(c.view), Element::RevisionTag(t) => Some(t.view), Element::ColorFillLegend(l) => Some(l.view),
        _ => None,
    }
}

fn set_view(e: &mut Element, v: Id) {
    match e {
        Element::DetailLine(l) => l.view = v, Element::DetailRegion(r) => r.view = v, Element::TextNote(t) => t.view = v,
        Element::RoomTag(t) => t.view = v, Element::ElementTag(t) => t.view = v, Element::AreaTag(t) => t.view = v, Element::Area(a) => a.view = v,
        Element::AreaBoundary(b) => b.view = v, Element::RevisionCloud(c) => c.view = v, Element::RevisionTag(t) => t.view = v, Element::ColorFillLegend(l) => l.view = v,
        _ => {}
    }
}

/// A point an element stands at, for the middle of a copy.
fn point_of(e: &Element) -> Option<Point> {
    Some(match e {
        Element::Room(r) => r.at?, Element::RoomTag(t) => t.at, Element::AreaTag(t) => t.at, Element::Area(a) => a.at?, Element::TextNote(t) => t.at,
        Element::RevisionTag(t) => t.at, Element::ColorFillLegend(l) => l.at, Element::FamilyInstance(i) => i.location,
        _ => return None,
    })
}

/// The model element's level (walls: their base).
fn level_of(e: &Element) -> Option<Id> {
    match e {
        Element::Wall(w) => Some(w.base_level), Element::Floor(f) => Some(f.level), Element::Roof(r) => Some(r.level), Element::Ceiling(c) => Some(c.level), Element::Stair(s) => Some(s.base_level), Element::Railing(r) => Some(r.level), Element::Ramp(r) => Some(r.base_level),
        Element::FamilyInstance(i) => Some(i.level), Element::Room(r) => Some(r.level), Element::RoomSeparation(s) => Some(s.level),
        _ => None,
    }
}

/// Order of adding on paste: what others refer to first.
fn rank(e: &Element) -> u8 {
    match e {
        Element::Wall(_) => 0,
        Element::Floor(_) | Element::Roof(_) | Element::Ceiling(_) | Element::Stair(_) | Element::Ramp(_) | Element::Room(_) | Element::RoomSeparation(_) | Element::Grid(_) => 1,
        Element::FamilyInstance(_) | Element::Railing(_) => 2,
        Element::RoomTag(_) | Element::ElementTag(_) | Element::AreaTag(_) | Element::RevisionTag(_) => 4,
        _ => 3,
    }
}

impl Document {
    /// Copy to Clipboard: a snapshot of `ids` (with the doors and windows in copied walls and the families on copied
    /// ceilings), copied in `view`.
    pub fn copy_to_clipboard(&self, ids: &[Id], view: Option<Id>) -> Result<Clipboard, String> {
        let mut set: BTreeSet<Id> = ids.iter().copied().collect();
        // hosted elements go with their host
        for (id, i) in self.instances() {
            if i.host.is_some_and(|h| set.contains(&h.wall)) || i.face_host.is_some_and(|h| set.contains(&h.element)) { set.insert(id); }
        }
        for (id, r) in self.railings() {
            if r.host.is_some_and(|h| set.contains(&h.stair)) { set.insert(id); }
        }
        let mut clip = Clipboard { elements: vec![], view, hosted_at: BTreeMap::new(), origin: Point::new(0.0, 0.0), skipped: BTreeMap::new() };
        for id in set {
            let Some(e) = self.get(id) else { continue };
            let ok = matches!(e, Element::Wall(_) | Element::Floor(_) | Element::Roof(_) | Element::Ceiling(_) | Element::Stair(_) | Element::Ramp(_) | Element::Railing(_) | Element::FamilyInstance(_) | Element::Room(_) | Element::RoomSeparation(_) | Element::Grid(_)) || view_specific(e);
            if !ok { *clip.skipped.entry(e.kind()).or_default() += 1; continue; }
            if let Element::RevisionCloud(c) = e { if self.revision_record(c.revision).is_some_and(|r| r.issued) { return Err("Unissue the revision before copying its cloud.".into()); } }
            let mut e = e.clone();
            // a boundary that follows a wall copies as the line it makes now (it follows a copied wall again below)
            if let Element::AreaBoundary(b) = &mut e { if let Some(c) = self.area_boundary_curve(id) { b.curve = c; } }
            if let Element::FamilyInstance(i) = &e {
                if i.host.is_some_and(|h| h.v_mm.is_some()) { *clip.skipped.entry("CurtainPanelDoor").or_default() += 1; continue; }
                if i.host.is_some() { if let Some(f) = self.instance_frame(id) { clip.hosted_at.insert(id, f.origin); } }
            }
            clip.elements.push((id, e));
        }
        if clip.elements.is_empty() { return Err("Nothing in the selection can be copied: select model elements, detail items, text or tags.".into()); }
        // the middle of the copied elements' extents: their lines, and the points of point-like ones
        let mut pts: Vec<Point> = vec![];
        let single = Clipboard { elements: vec![], ..clip.clone() };
        for (id, e) in &clip.elements {
            let at = match e { Element::FamilyInstance(i) => Some(clip.hosted_at.get(id).copied().unwrap_or(i.location)), _ => point_of(e) };
            match at {
                Some(p) => pts.push(p),
                None => pts.extend(Clipboard { elements: vec![(*id, e.clone())], ..single.clone() }.preview().into_iter().flatten()),
            }
        }
        let (x0, x1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        let (y0, y1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.y), b.max(p.y)));
        clip.origin = if x0.is_finite() { Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0) } else { Point::new(0.0, 0.0) };
        Ok(clip)
    }

    /// The levels in order of elevation.
    fn levels_up(&self) -> Vec<Id> {
        let mut l = self.levels();
        l.sort_by(|a, b| a.1.elevation_mm.total_cmp(&b.1.elevation_mm));
        l.into_iter().map(|(i, _)| i).collect()
    }

    /// The level a paste's level change counts from: the copied view's, or the lowest the copied elements stand on.
    fn clipboard_base_level(&self, clip: &Clipboard) -> Option<Id> {
        if let Some(l) = clip.view.and_then(|v| self.view(v)).and_then(|v| v.level) { return Some(l); }
        let order = self.levels_up();
        clip.elements.iter().filter_map(|(_, e)| level_of(e)).min_by_key(|l| order.iter().position(|x| x == l).unwrap_or(usize::MAX))
    }

    /// Paste the clipboard. Returns the new elements. One undo step.
    pub fn paste(&mut self, clip: &Clipboard, target: PasteTarget) -> Result<Vec<Id>, String> {
        let order = self.levels_up();
        let base = self.clipboard_base_level(clip);
        let step = |to: Id| -> Result<i64, String> {
            let (Some(b), Some(t)) = (base.and_then(|b| order.iter().position(|x| *x == b)), order.iter().position(|x| *x == to)) else { return Ok(0) };
            Ok(t as i64 - b as i64)
        };
        // each run: levels up or down, the view for view-specific elements (None: their own), the move, model?, views?
        let mut runs: Vec<(i64, Option<Id>, Point, bool, bool)> = vec![];
        match &target {
            PasteTarget::SamePlace => runs.push((0, None, Point::new(0.0, 0.0), true, true)),
            PasteTarget::CurrentView { view } | PasteTarget::Point { view, .. } => {
                let v = self.view(*view).ok_or("Unknown view.")?;
                let d = match &target { PasteTarget::Point { at, .. } => Point::new(at.x - clip.origin.x, at.y - clip.origin.y), _ => Point::new(0.0, 0.0) };
                let plan = v.view_kind.is_plan();
                if !plan && clip.has_model() && d.x.hypot(d.y) > 1e-9 { return Err("Paste model elements by picking a point in a plan view.".into()); }
                let s = if let Some(l) = v.level.filter(|_| plan) { step(l)? } else { 0 };
                runs.push((s, Some(*view), d, true, true));
            }
            PasteTarget::Levels(levels) => {
                if levels.is_empty() { return Err("Choose at least one level.".into()); }
                if !clip.has_model() { return Err("Aligned to Selected Levels pastes model elements; the clipboard has only view-specific ones.".into()); }
                for &l in levels { self.level(l).ok_or("Unknown level.")?; runs.push((step(l)?, None, Point::new(0.0, 0.0), true, false)); }
            }
            PasteTarget::Views(views) => {
                if views.is_empty() { return Err("Choose at least one view.".into()); }
                if clip.has_model() { return Err("Aligned to Selected Views pastes view-specific elements only: copy detail items, text or tags on their own.".into()); }
                let kind = clip.view.and_then(|v| self.view(v)).map(|v| v.view_kind);
                for &v in views {
                    let vk = self.view(v).ok_or("Unknown view.")?.view_kind;
                    if kind.is_some_and(|k| k != vk) { return Err("Paste into views of the kind the elements were copied from.".into()); }
                    runs.push((0, Some(v), Point::new(0.0, 0.0), false, true));
                }
            }
        }
        let clip = clip.clone();
        self.grouped("Paste", |d| {
            let mut all = vec![];
            for (s, view, dv, model, views) in runs { all.extend(d.paste_run(&clip, &order, s, view, dv, model, views)?); }
            if all.is_empty() { return Err("Nothing to paste here.".into()); }
            Ok(all)
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn paste_run(&mut self, clip: &Clipboard, order: &[Id], step: i64, view: Option<Id>, dv: Point, model: bool, views: bool) -> Result<Vec<Id>, String> {
        let shift = |l: Id| -> Option<Id> { let i = order.iter().position(|x| *x == l)? as i64 + step; (0..order.len() as i64).contains(&i).then(|| order[i as usize]) };
        let lvl = |l: Id| -> Result<Id, String> { shift(l).ok_or_else(|| "There is no level there for some of the elements (they would go above the top level or below the bottom one).".to_string()) };
        let elev = |l: Id| self.level(l).map_or(0.0, |x| x.elevation_mm);
        let ids: BTreeSet<Id> = clip.elements.iter().map(|(i, _)| *i).collect();
        let mut plan: Vec<(Id, Element)> = vec![];
        let mut rehost: BTreeMap<Id, Point> = BTreeMap::new();
        for (old, e) in &clip.elements {
            let mut e = e.clone();
            if view_specific(&e) {
                if !views { continue; }
                let target = view.or_else(|| view_of(&e)).filter(|v| self.view(*v).is_some()).ok_or("The view the elements were copied from is gone: paste them into a view.")?;
                // areas and their boundaries, legends and tags only go where they can be drawn
                if let (Some(src), Some(t)) = (view_of(&e).and_then(|v| self.view(v)), self.view(target)) { if (src.view_kind == ViewKind::AreaPlan) != (t.view_kind == ViewKind::AreaPlan) && matches!(e, Element::Area(_) | Element::AreaBoundary(_) | Element::AreaTag(_)) { continue; } }
                set_view(&mut e, target);
            } else if !model {
                continue;
            }
            match &mut e {
                Element::Wall(w) => {
                    let base = lvl(w.base_level)?;
                    if let Some(t) = w.top_level {
                        match shift(t) { Some(nt) => w.top_level = Some(nt), None => { w.height_mm = elev(t) + w.top_offset_mm - elev(w.base_level); w.top_level = None; } }
                    }
                    w.base_level = base; w.pinned = false; w.mark.clear(); w.ifc.guid = new_ifc_guid();
                }
                Element::Floor(f) => { f.level = lvl(f.level)?; f.pinned = false; }
                Element::Roof(r) => { r.level = lvl(r.level)?; r.pinned = false; r.mark.clear(); r.ifc.guid = new_ifc_guid(); }
                Element::Ceiling(c) => { c.level = lvl(c.level)?; c.pinned = false; c.mark.clear(); c.ifc.guid = new_ifc_guid(); }
                Element::Stair(s) => {
                    let base = lvl(s.base_level)?;
                    if let Some(t) = s.top_level {
                        match shift(t) { Some(nt) => s.top_level = Some(nt), None => { s.unconnected_height_mm = elev(t) + s.top_offset_mm - elev(s.base_level) - s.base_offset_mm; s.top_level = None; } }
                    }
                    s.multistory_top = s.multistory_top.and_then(shift);
                    if s.multistory_top.is_none() { s.storeys.clear(); }
                    s.storeys = s.storeys.drain(..).filter_map(|mut o| { o.base_level = shift(o.base_level)?; Some(o) }).collect();
                    s.base_level = base; s.pinned = false; s.mark.clear(); s.ifc.guid = new_ifc_guid();
                }
                Element::Railing(r) => { r.level = lvl(r.level)?; r.pinned = false; r.mark.clear(); r.ifc.guid = new_ifc_guid(); }
                Element::Ramp(r) => {
                    let base = lvl(r.base_level)?;
                    if let Some(t) = r.top_level {
                        match shift(t) { Some(nt) => r.top_level = Some(nt), None => { r.unconnected_height_mm = elev(t) + r.top_offset_mm - elev(r.base_level) - r.base_offset_mm; r.top_level = None; } }
                    }
                    r.base_level = base; r.pinned = false; r.mark.clear(); r.ifc.guid = new_ifc_guid();
                }
                Element::Room(r) => {
                    let base = lvl(r.level)?;
                    if let Some(u) = r.upper_level { match shift(u) { Some(nu) => r.upper_level = Some(nu), None => { r.limit_offset_mm += elev(u) - elev(r.level); r.upper_level = None; } } }
                    r.level = base; r.pinned = false; r.ifc.guid = new_ifc_guid();
                }
                Element::RoomSeparation(s) => { s.level = lvl(s.level)?; s.pinned = false; }
                Element::FamilyInstance(i) => {
                    i.level = lvl(i.level)?; i.pinned = false; i.mark.clear(); i.ifc.guid = new_ifc_guid();
                    // a door or window without its wall finds the wall under where it goes
                    if i.host.is_some_and(|h| !ids.contains(&h.wall)) {
                        let at = clip.hosted_at.get(old).copied().ok_or("A door or window lost its wall.")?;
                        rehost.insert(*old, Point::new(at.x + dv.x, at.y + dv.y));
                    }
                }
                Element::Grid(g) => g.pinned = false,
                Element::DetailLine(l) => l.pinned = false,
                Element::DetailRegion(r) => r.pinned = false,
                Element::TextNote(t) => t.pinned = false,
                Element::RoomTag(t) => t.pinned = false,
                Element::ElementTag(t) => t.pinned = false,
                Element::AreaTag(t) => t.pinned = false,
                Element::Area(a) => a.pinned = false,
                Element::AreaBoundary(b) => b.pinned = false,
                Element::RevisionCloud(c) => { c.pinned = false; c.mark.clear(); }
                Element::RevisionTag(t) => t.pinned = false,
                Element::ColorFillLegend(l) => l.pinned = false,
                _ => {}
            }
            // grids are the project's: they paste once, not onto each level
            if matches!(e, Element::Grid(_)) && step != 0 { continue; }
            plan.push((*old, e));
        }
        plan.sort_by_key(|(_, e)| rank(e));
        // find hosts for doors and windows pasted without their walls
        for (old, e) in &mut plan {
            let Element::FamilyInstance(i) = e else { continue };
            if let Some(at) = rehost.get(old) {
                let (wall, u) = self.host_wall_at(i.level, *at, None).ok_or("There is no wall to host a door or window where it would go.")?;
                let h = i.host.unwrap();
                i.host = Some(HostRef { wall, u_mm: u, v_mm: None });
                if wall != h.wall { if let Some(Element::Wall(w)) = self.get(wall) { i.flip_facing = w.path().project(*at).1 < 0.0; } }
            }
            if let Some(h) = i.face_host.as_mut() { if !ids.contains(&h.element) { if let Some(c) = self.ceiling_at(i.level, Point::new(i.location.x + dv.x, i.location.y + dv.y)) { h.element = c; } } }
        }
        let added: Vec<(Id, Id)> = self.transact("Paste", |tx| {
            let mut map: BTreeMap<Id, Id> = BTreeMap::new();
            let mut out = vec![];
            for (old, mut e) in plan {
                match &mut e {
                    Element::FamilyInstance(i) => {
                        if let Some(h) = i.host.as_mut() { if let Some(n) = map.get(&h.wall) { h.wall = *n; } }
                        if let Some(h) = i.face_host.as_mut() { if let Some(n) = map.get(&h.element) { h.element = *n; } }
                        if let Some(c) = tx.doc_ref().instance_family(i).map(|(f, _)| f.category) { i.mark = tx.doc_ref().next_mark(c); }
                    }
                    Element::Grid(g) => g.name = tx.doc_ref().next_grid_name(),
                    Element::RoomTag(t) => { if let Some(n) = map.get(&t.room) { t.room = *n; } else if tx.doc_ref().room(t.room).is_none() { continue; } }
                    Element::ElementTag(t) => { if let Some(n) = map.get(&t.element) { t.element = *n; } else if tx.doc_ref().get(t.element).is_none() { continue; } }
                    Element::AreaTag(t) => { if let Some(n) = map.get(&t.area) { t.area = *n; } else if tx.doc_ref().area(t.area).is_none() { continue; } }
                    Element::RevisionTag(t) => { if let Some(n) = map.get(&t.cloud) { t.cloud = *n; } else if tx.get(t.cloud).is_none() { continue; } }
                    Element::AreaBoundary(b) => { b.wall = b.wall.and_then(|w| map.get(&w).copied()); }
                    Element::Railing(r) => { if let Some(h) = r.host.as_mut() { match map.get(&h.stair) { Some(n) => h.stair = *n, None => continue } } }
                    _ => {}
                }
                let id = tx.add(e);
                if tx.get(old).is_some() { tx.copy_parameter_data(old, id)?; }
                map.insert(old, id);
                out.push((old, id));
            }
            Ok(out)
        })?;
        let new: Vec<Id> = added.iter().map(|(_, n)| *n).collect();
        // then the Move tool's rules carry the lot (doors re-hosted above stay where they were put)
        if dv.x.hypot(dv.y) > 1e-9 {
            let moving: Vec<Id> = added.iter().filter(|(o, _)| !rehost.contains_key(o)).map(|(_, n)| *n).collect();
            if !moving.is_empty() { self.transform_elements_inner(&moving, Xform::Move { dx: dv.x, dy: dv.y }, EditOptions { copy: false, disjoin: true })?; }
        }
        for (id, i) in self.instances() { if new.contains(&id) { self.fits(i, Some(id)).map_err(|e| format!("a pasted door or window does not fit: {e}"))?; } }
        Ok(new)
    }
}
