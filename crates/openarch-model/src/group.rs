//! Groups: a set of elements defined once (the group type) and placed many times (group
//! instances). A model group holds walls, floors, roofs, doors, windows and components; a
//! detail group holds one view's detail lines, regions and text notes and is placed in a view.
//!
//! Members are ordinary elements in the document, so joins, hosting, drawing and
//! quantities work on them unchanged. The instance records which element plays
//! which member, by a key that stays the same through edits. The type keeps a copy
//! of every member as it stood in one instance (the frame it was captured in);
//! placing the type somewhere else copies those members through the change from
//! that frame to the new one. Finishing an edit captures the edited instance and
//! rebuilds every other instance from it, keeping the elements' ids.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::edit::{EditOptions, Xform};
use crate::element::*;
use crate::family::HostRef;
use crate::geom::Point;

/// Where an instance sits: its origin, turn about Z, whether it is mirrored, and its reference
/// level (a model group) or the view it is drawn in (a detail group, whose `level` is 0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GroupFrame {
    pub origin: Point,
    #[serde(default)]
    pub angle_deg: f64,
    #[serde(default)]
    pub mirrored: bool,
    #[serde(default)]
    pub level: Id,
    #[serde(default)]
    pub view: Option<Id>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    #[default]
    Model,
    Detail,
}

impl GroupKind {
    fn label(self) -> &'static str {
        match self {
            GroupKind::Model => "Model Group",
            GroupKind::Detail => "Detail Group",
        }
    }
}

impl GroupFrame {
    /// The frame after a Move, Rotate or Mirror. A mirror turns the frame to
    /// twice the axis angle less its own and reverses its handedness.
    pub fn transformed(&self, x: Xform) -> GroupFrame {
        let origin = x.apply(self.origin);
        let (angle_deg, mirrored) = match x {
            Xform::Move { .. } => (self.angle_deg, self.mirrored),
            Xform::Rotate { angle_deg, .. } => (self.angle_deg + angle_deg, self.mirrored),
            Xform::Mirror { ax, ay, bx, by } => (2.0 * (by - ay).atan2(bx - ax).to_degrees() - self.angle_deg, !self.mirrored),
        };
        GroupFrame { origin, angle_deg: angle_deg.rem_euclid(360.0), mirrored, ..*self }
    }

    /// The changes, in order, that carry what stands in this frame to `to` (plan only; levels are mapped apart).
    fn steps_to(&self, to: &GroupFrame) -> Vec<Xform> {
        let o = self.origin;
        let mut v = vec![];
        if self.mirrored != to.mirrored {
            let (s, c) = self.angle_deg.to_radians().sin_cos();
            v.push(Xform::Mirror { ax: o.x, ay: o.y, bx: o.x + c * 1000.0, by: o.y + s * 1000.0 });
        }
        let turn = (to.angle_deg - self.angle_deg).rem_euclid(360.0);
        if turn > 1e-9 && (360.0 - turn) > 1e-9 {
            v.push(Xform::Rotate { cx: o.x, cy: o.y, angle_deg: turn });
        }
        let (dx, dy) = (to.origin.x - o.x, to.origin.y - o.y);
        if dx.hypot(dy) > 1e-9 {
            v.push(Xform::Move { dx, dy });
        }
        v
    }
}

/// One member of a group type: the element as it stood when captured. A door or
/// window's host wall is given by that wall's key, not an element id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupMember {
    pub key: Id,
    pub element: Element,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupType {
    pub name: String,
    #[serde(default)]
    pub kind: GroupKind,
    /// The frame `members` were captured in.
    pub frame: GroupFrame,
    pub members: Vec<GroupMember>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupInstance {
    pub group_type: Id,
    pub frame: GroupFrame,
    /// Member key to the element playing it here. An element that has gone counts as excluded.
    pub members: BTreeMap<Id, Id>,
    /// Members left out of this instance only (Exclude).
    #[serde(default)]
    pub excluded: BTreeSet<Id>,
    #[serde(default)]
    pub pinned: bool,
}

impl GroupInstance {
    /// The member elements that are in the document, as (key, element id).
    pub fn live(&self, doc: &Document) -> Vec<(Id, Id)> {
        self.members.iter().filter(|(_, id)| doc.get(**id).is_some()).map(|(k, id)| (*k, *id)).collect()
    }
}

fn groupable(e: &Element) -> bool {
    matches!(e, Element::Wall(_) | Element::Floor(_) | Element::Roof(_) | Element::FamilyInstance(_))
}

fn detail_item(e: &Element) -> bool {
    matches!(e, Element::DetailLine(_) | Element::DetailRegion(_) | Element::TextNote(_))
}

/// What may go in a group of this kind.
fn fits_kind(kind: GroupKind, e: &Element) -> bool {
    match kind {
        GroupKind::Model => groupable(e),
        GroupKind::Detail => detail_item(e),
    }
}

/// The view a detail item is drawn in.
fn detail_view(e: &Element) -> Option<Id> {
    match e {
        Element::DetailLine(l) => Some(l.view),
        Element::DetailRegion(r) => Some(r.view),
        Element::TextNote(t) => Some(t.view),
        _ => None,
    }
}

fn set_detail_view(e: &mut Element, view: Id) {
    match e {
        Element::DetailLine(l) => l.view = view,
        Element::DetailRegion(r) => r.view = view,
        Element::TextNote(t) => t.view = view,
        _ => {}
    }
}

/// Plan points that bound an element, for the origin of a new group.
fn element_points(doc: &Document, id: Id, e: &Element) -> Vec<Point> {
    match e {
        Element::Wall(w) => vec![w.start, w.end],
        Element::Floor(f) => f.loops().iter().flatten().flat_map(|c| [c.start(), c.end()]).collect(),
        Element::Roof(r) => {
            let crate::roof::RoofShape::Footprint { loops } = &r.shape;
            loops.iter().flatten().flat_map(|e| [e.curve.start(), e.curve.end()]).collect()
        }
        Element::FamilyInstance(_) => doc.instance_frame(id).map(|f| vec![f.origin]).unwrap_or_default(),
        Element::DetailLine(l) => vec![l.curve.start(), l.curve.end(), l.curve.mid()],
        Element::DetailRegion(r) => r.loops.iter().flatten().flat_map(|e| [e.curve.start(), e.curve.end()]).collect(),
        Element::TextNote(t) => vec![t.at],
        _ => vec![],
    }
}

fn set_pinned(e: &mut Element, pinned: bool) {
    match e {
        Element::Wall(w) => w.pinned = pinned,
        Element::Floor(f) => f.pinned = pinned,
        Element::Roof(r) => r.pinned = pinned,
        Element::FamilyInstance(i) => i.pinned = pinned,
        Element::DetailLine(l) => l.pinned = pinned,
        Element::DetailRegion(r) => r.pinned = pinned,
        Element::TextNote(t) => t.pinned = pinned,
        _ => {}
    }
}

/// Give a rebuilt member the identity of the element it replaces: its mark, IFC GUID and pin.
fn keep_identity(old: &Element, new: &mut Element) {
    match (old, new) {
        (Element::Wall(o), Element::Wall(n)) => {
            n.mark = o.mark.clone();
            n.ifc.guid = o.ifc.guid.clone();
            n.pinned = o.pinned;
        }
        (Element::Roof(o), Element::Roof(n)) => {
            n.mark = o.mark.clone();
            n.ifc.guid = o.ifc.guid.clone();
            n.pinned = o.pinned;
        }
        (Element::FamilyInstance(o), Element::FamilyInstance(n)) => {
            n.mark = o.mark.clone();
            n.ifc.guid = o.ifc.guid.clone();
            n.pinned = o.pinned;
        }
        (Element::Floor(o), Element::Floor(n)) => n.pinned = o.pinned,
        (Element::DetailLine(o), Element::DetailLine(n)) => n.pinned = o.pinned,
        (Element::DetailRegion(o), Element::DetailRegion(n)) => n.pinned = o.pinned,
        (Element::TextNote(o), Element::TextNote(n)) => n.pinned = o.pinned,
        _ => {}
    }
}

/// A fresh copy of a member for a new instance: no mark, a new IFC GUID, unpinned.
fn fresh(e: &Element) -> Element {
    let mut e = e.clone();
    match &mut e {
        Element::Wall(w) => {
            w.mark.clear();
            w.ifc.guid = new_ifc_guid();
        }
        Element::Roof(r) => {
            r.mark.clear();
            r.ifc.guid = new_ifc_guid();
        }
        Element::FamilyInstance(i) => {
            i.mark.clear();
            i.ifc.guid = new_ifc_guid();
        }
        _ => {}
    }
    set_pinned(&mut e, false);
    e
}

impl Document {
    pub fn group_type(&self, id: Id) -> Option<&GroupType> {
        match self.get(id)? {
            Element::GroupType(t) => Some(t),
            _ => None,
        }
    }

    pub fn group_instance(&self, id: Id) -> Option<&GroupInstance> {
        match self.get(id)? {
            Element::GroupInstance(g) => Some(g),
            _ => None,
        }
    }

    pub fn group_types(&self) -> Vec<(Id, &GroupType)> {
        self.iter().filter_map(|(i, e)| if let Element::GroupType(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn group_instances(&self) -> Vec<(Id, &GroupInstance)> {
        self.iter().filter_map(|(i, e)| if let Element::GroupInstance(g) = e { Some((i, g)) } else { None }).collect()
    }

    /// The group instance an element is a member of.
    pub fn group_of(&self, member: Id) -> Option<Id> {
        self.group_instances().into_iter().find(|(_, g)| g.members.values().any(|m| *m == member)).map(|(i, _)| i)
    }

    /// The level `steps` levels above (or below) `l`, counting by elevation.
    fn level_steps(&self, l: Id, steps: i64) -> Option<Id> {
        let levels = self.levels();
        let i = levels.iter().position(|(id, _)| *id == l)? as i64 + steps;
        if i < 0 {
            return None;
        }
        levels.get(i as usize).map(|(id, _)| *id)
    }

    fn level_index(&self, l: Id) -> Option<i64> {
        self.levels().iter().position(|(id, _)| *id == l).map(|i| i as i64)
    }

    /// A member's levels moved by the same number of levels as the group's reference level. A
    /// wall whose top level runs out becomes unconnected at the height it had, and says so.
    fn shift_levels(&self, e: &mut Element, steps: i64, notes: &mut Vec<String>) -> Result<(), String> {
        if steps == 0 {
            return Ok(());
        }
        let base = |doc: &Document, l: Id| doc.level_steps(l, steps).ok_or_else(|| "there is no level for the group's members that far up or down".to_string());
        let elev = |l: Id| self.level(l).map_or(0.0, |l| l.elevation_mm);
        match e {
            Element::Wall(w) => {
                let old_base = w.base_level;
                w.base_level = base(self, w.base_level)?;
                if let Some(t) = w.top_level {
                    match self.level_steps(t, steps) {
                        Some(nt) => w.top_level = Some(nt),
                        None => {
                            w.height_mm = elev(t) + w.top_offset_mm - elev(old_base) - w.base_offset_mm;
                            w.top_level = None;
                            w.top_offset_mm = 0.0;
                            let note = "a wall in the group had no level to reach up to, so it is unconnected at its old height".to_string();
                            if !notes.contains(&note) {
                                notes.push(note);
                            }
                        }
                    }
                }
            }
            Element::Floor(f) => f.level = base(self, f.level)?,
            Element::Roof(r) => {
                r.level = base(self, r.level)?;
                r.cutoff_level = r.cutoff_level.and_then(|c| self.level_steps(c, steps));
            }
            Element::FamilyInstance(i) => i.level = base(self, i.level)?,
            _ => {}
        }
        Ok(())
    }

    /// Copy a type's members into the document at `to`, leaving out the keys in `skip`. A hosted
    /// member whose wall is left out is left out too, unless `existing` gives the wall's element.
    /// Returns key to new element id.
    fn paste_members(&mut self, ty: &GroupType, to: &GroupFrame, skip: &BTreeSet<Id>, existing: &BTreeMap<Id, Id>, notes: &mut Vec<String>) -> Result<BTreeMap<Id, Id>, String> {
        let steps = match ty.kind {
            GroupKind::Detail => 0,
            GroupKind::Model => match (self.level_index(ty.frame.level), self.level_index(to.level)) {
                (Some(a), Some(b)) => b - a,
                _ => return Err("the group's reference level no longer exists".into()),
            },
        };
        let mut plain = vec![];
        let mut hosted = vec![];
        for m in &ty.members {
            if skip.contains(&m.key) {
                continue;
            }
            let mut e = fresh(&m.element);
            self.shift_levels(&mut e, steps, notes)?;
            if let (GroupKind::Detail, Some(v)) = (ty.kind, to.view) {
                set_detail_view(&mut e, v);
            }
            match &e {
                Element::FamilyInstance(i) if i.host.is_some() => hosted.push((m.key, e)),
                _ => plain.push((m.key, e)),
            }
        }
        let map = self.transact("Place group", |tx| {
            let mut map: BTreeMap<Id, Id> = BTreeMap::new();
            for (k, e) in plain {
                map.insert(k, tx.add(e));
            }
            for (k, mut e) in hosted {
                if let Element::FamilyInstance(i) = &mut e {
                    let h = i.host.unwrap();
                    let Some(&wall) = map.get(&h.wall).or_else(|| existing.get(&h.wall)) else { continue };
                    i.host = Some(HostRef { wall, u_mm: h.u_mm });
                }
                map.insert(k, tx.add(e));
            }
            Ok(map)
        })?;
        let ids: Vec<Id> = map.values().copied().collect();
        if !ids.is_empty() {
            for x in ty.frame.steps_to(to) {
                self.transform_elements_inner(&ids, x, EditOptions { copy: false, disjoin: true })?;
            }
        }
        Ok(map)
    }

    /// Make a group from the selected elements (Create Group): a model group from model elements, or
    /// a detail group when the selection holds only one view's detail items. Levels, grids and anything
    /// else that cannot be grouped are left out and named in the notes. Returns the new instance.
    pub fn create_group(&mut self, ids: &[Id], name: Option<&str>, level: Option<Id>) -> Result<(Id, Vec<String>), String> {
        let mut notes = vec![];
        let mut model: Vec<Id> = vec![];
        let mut detail: Vec<Id> = vec![];
        let mut left_out = 0;
        let mut datums = 0;
        for &id in ids {
            match self.get(id) {
                Some(e) if groupable(e) || detail_item(e) => {
                    if self.group_of(id).is_some() {
                        return Err("an element in the selection is already in a group".into());
                    }
                    let list = if groupable(e) { &mut model } else { &mut detail };
                    if !list.contains(&id) {
                        list.push(id);
                    }
                }
                Some(Element::Level(_)) | Some(Element::Grid(_)) => datums += 1,
                Some(_) => left_out += 1,
                None => return Err(format!("no element {id}")),
            }
        }
        if datums > 0 {
            notes.push("levels and grids are never grouped, so they were left out".to_string());
        }
        if left_out > 0 {
            notes.push(format!("{left_out} item(s) that cannot go in a group were left out"));
        }
        let kind = if model.is_empty() { GroupKind::Detail } else { GroupKind::Model };
        if kind == GroupKind::Model && !detail.is_empty() {
            notes.push("detail items were left out: groups holding both model and detail items come next".to_string());
        }
        let members = if kind == GroupKind::Model { model } else { detail };
        if members.is_empty() {
            return Err("select walls, floors, roofs, doors, windows, components or detail items to group".into());
        }
        let view = match kind {
            GroupKind::Model => None,
            GroupKind::Detail => {
                let views: BTreeSet<Id> = members.iter().filter_map(|i| self.get(*i).and_then(detail_view)).collect();
                if views.len() > 1 {
                    return Err("the detail items of a group must all be in one view".into());
                }
                views.into_iter().next()
            }
        };
        for &id in &members {
            if let Some(Element::FamilyInstance(i)) = self.get(id) {
                if i.host.is_some_and(|h| !members.contains(&h.wall)) {
                    return Err("a door or window in the group needs its wall in the group too".into());
                }
            }
        }
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => {
                if self.group_types().iter().any(|(_, t)| t.name == n) {
                    return Err(format!("there is already a group called {n}"));
                }
                n.to_string()
            }
            None => self.next_group_name(kind),
        };
        let pts: Vec<Point> = members.iter().flat_map(|&id| element_points(self, id, self.get(id).unwrap())).collect();
        let (lo, hi) = pts.iter().fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(lo, hi), p| {
            (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)))
        });
        let origin = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let level = match (kind, level) {
            (GroupKind::Detail, _) => 0,
            (_, Some(l)) if self.level(l).is_some() => l,
            (_, Some(_)) => return Err("unknown level".into()),
            (_, None) => self.lowest_member_level(&members).ok_or("the group's members have no level")?,
        };
        let frame = GroupFrame { origin, angle_deg: 0.0, mirrored: false, level, view };
        let mut inst = GroupInstance { group_type: 0, frame, members: members.iter().map(|&i| (i, i)).collect(), excluded: BTreeSet::new(), pinned: false };
        let ty = GroupType { name, kind, frame, members: self.capture(&inst) };
        let id = self.transact("Create group", |tx| {
            inst.group_type = tx.add(Element::GroupType(ty));
            Ok(tx.add(Element::GroupInstance(inst)))
        })?;
        Ok((id, notes))
    }

    fn lowest_member_level(&self, members: &[Id]) -> Option<Id> {
        let lv = |id: &Id| match self.get(*id)? {
            Element::Wall(w) => Some(w.base_level),
            Element::Floor(f) => Some(f.level),
            Element::Roof(r) => Some(r.level),
            Element::FamilyInstance(i) => Some(i.level),
            _ => None,
        };
        members.iter().filter_map(lv).min_by(|a, b| {
            let e = |l: &Id| self.level(*l).map_or(0.0, |l| l.elevation_mm);
            e(a).partial_cmp(&e(b)).unwrap()
        })
    }

    fn next_group_name(&self, kind: GroupKind) -> String {
        let names: BTreeSet<String> = self.group_types().iter().map(|(_, t)| t.name.clone()).collect();
        (1..).map(|n| format!("{} {n}", kind.label())).find(|n| !names.contains(n)).unwrap()
    }

    /// The members of an instance as they stand now, with hosts given by key.
    fn capture(&self, inst: &GroupInstance) -> Vec<GroupMember> {
        let key_of: BTreeMap<Id, Id> = inst.members.iter().map(|(k, id)| (*id, *k)).collect();
        let mut out = vec![];
        for (k, id) in &inst.members {
            let Some(e) = self.get(*id) else { continue };
            let mut e = e.clone();
            if let Element::FamilyInstance(i) = &mut e {
                if let Some(h) = i.host {
                    // a door whose wall is not a member has nothing to stand in elsewhere
                    let Some(&wk) = key_of.get(&h.wall) else { continue };
                    i.host = Some(HostRef { wall: wk, u_mm: h.u_mm });
                }
            }
            out.push(GroupMember { key: *k, element: e });
        }
        out
    }

    /// Place an instance of a group type with its origin at `at`, turned by `angle_deg`: a model group
    /// on the level `target`, a detail group in the view `target`.
    pub fn place_group(&mut self, group_type: Id, at: Point, target: Id, angle_deg: f64) -> Result<(Id, Vec<String>), String> {
        let ty = self.group_type(group_type).cloned().ok_or("not a group type")?;
        let frame = match ty.kind {
            GroupKind::Model => {
                if self.level(target).is_none() {
                    return Err("a model group is placed on a level".into());
                }
                GroupFrame { origin: at, angle_deg, mirrored: false, level: target, view: None }
            }
            GroupKind::Detail => {
                match self.view(target) {
                    Some(v) if v.view_kind != crate::view::ViewKind::ThreeD => {}
                    Some(_) => return Err("detail groups are placed in 2D views, not 3D views".into()),
                    None => return Err("a detail group is placed in a view".into()),
                }
                GroupFrame { origin: at, angle_deg, mirrored: false, level: 0, view: Some(target) }
            }
        };
        self.grouped("Place group", |d| d.place_frame(group_type, &ty, frame, BTreeSet::new()))
    }

    fn place_frame(&mut self, group_type: Id, ty: &GroupType, frame: GroupFrame, excluded: BTreeSet<Id>) -> Result<(Id, Vec<String>), String> {
        let mut notes = vec![];
        let members = self.paste_members(ty, &frame, &excluded, &BTreeMap::new(), &mut notes)?;
        let inst = GroupInstance { group_type, frame, members, excluded, pinned: false };
        let id = self.transact("Place group", |tx| Ok(tx.add(Element::GroupInstance(inst))))?;
        Ok((id, notes))
    }

    /// Move, Rotate or Mirror a selection that holds group instances: the members move together and
    /// the instance's frame follows; a copy places a new instance of the type. Members of a selected
    /// group that are also selected on their own move once, with their group.
    pub(crate) fn transform_with_groups(&mut self, ids: &[Id], x: Xform, opt: EditOptions) -> Result<Vec<Id>, String> {
        let groups: Vec<Id> = ids.iter().copied().filter(|i| self.group_instance(*i).is_some()).collect();
        let inside: BTreeSet<Id> = groups.iter().flat_map(|g| self.group_instance(*g).unwrap().members.values().copied().collect::<Vec<_>>()).collect();
        let rest: Vec<Id> = ids.iter().copied().filter(|i| !groups.contains(i) && !inside.contains(i)).collect();
        if !opt.copy {
            let pinned = groups.iter().filter(|g| self.group_instance(**g).is_some_and(|g| g.pinned)).count();
            if pinned > 0 {
                return Err("a pinned group cannot be moved; unpin it first".into());
            }
        }
        let name = match (x, opt.copy) {
            (Xform::Move { .. }, false) => "Move",
            (Xform::Move { .. }, true) => "Copy",
            (Xform::Rotate { .. }, false) => "Rotate",
            (Xform::Rotate { .. }, true) => "Rotate copy",
            (Xform::Mirror { .. }, false) => "Mirror",
            (Xform::Mirror { .. }, true) => "Mirror copy",
        };
        self.grouped(name, |d| {
            let mut out = vec![];
            if !rest.is_empty() {
                out.extend(d.transform_elements(&rest, x, opt)?);
            }
            for g in groups {
                let inst = d.group_instance(g).cloned().unwrap();
                let frame = inst.frame.transformed(x);
                if opt.copy {
                    let ty = d.group_type(inst.group_type).cloned().ok_or("the group's type is missing")?;
                    out.push(d.place_frame(inst.group_type, &ty, frame, inst.excluded.clone())?.0);
                } else {
                    let ids: Vec<Id> = inst.live(d).into_iter().map(|(_, id)| id).collect();
                    if !ids.is_empty() {
                        d.transform_elements_inner(&ids, x, opt)?;
                    }
                    d.transact(name, |tx| tx.update(g, Element::GroupInstance(GroupInstance { frame, ..inst })))?;
                    out.push(g);
                }
            }
            Ok(out)
        })
    }

    /// Ungroup: the members stay where they are as ordinary elements. The type stays.
    pub fn ungroup(&mut self, ids: &[Id]) -> Result<Vec<Id>, String> {
        let mut freed = vec![];
        for &g in ids {
            let inst = self.group_instance(g).ok_or("not a group")?;
            freed.extend(inst.live(self).into_iter().map(|(_, id)| id));
        }
        self.transact("Ungroup", |tx| {
            for &g in ids {
                tx.remove(g)?;
            }
            Ok(())
        })?;
        Ok(freed)
    }

    /// Delete group instances and their members.
    pub(crate) fn delete_groups(&mut self, ids: &[Id]) -> Result<(), String> {
        let mut members = vec![];
        for &g in ids {
            let inst = self.group_instance(g).ok_or("not a group")?;
            if inst.pinned {
                return Err("a pinned group cannot be deleted; unpin it first".into());
            }
            members.extend(inst.live(self).into_iter().map(|(_, id)| id));
        }
        self.transact("Delete group", |tx| {
            for &g in ids {
                tx.remove(g)?;
            }
            Ok(())
        })?;
        if !members.is_empty() {
            self.delete(&members)?;
        }
        Ok(())
    }

    /// Remove instances none of whose members are left (their view was deleted, say).
    pub(crate) fn prune_empty_groups(&mut self) -> Result<(), String> {
        let empty: Vec<Id> = self.group_instances().into_iter().filter(|(_, g)| g.excluded.len() < g.members.len() && g.live(self).is_empty()).map(|(i, _)| i).collect();
        if empty.is_empty() {
            return Ok(());
        }
        self.transact("Delete", |tx| {
            for g in empty {
                tx.remove(g)?;
            }
            Ok(())
        })
    }

    /// Exclude members from their instance: they are deleted here only, and doors in an excluded wall go too.
    pub fn exclude_members(&mut self, members: &[Id]) -> Result<(), String> {
        let mut by_group: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
        for &m in members {
            let g = self.group_of(m).ok_or("only a member of a group can be excluded")?;
            by_group.entry(g).or_default().push(m);
        }
        self.grouped("Exclude", |d| {
            let all: Vec<Id> = by_group.values().flatten().copied().collect();
            d.delete(&all)?;
            for g in by_group.keys() {
                let mut inst = d.group_instance(*g).cloned().unwrap();
                // whatever went with the members (doors in an excluded wall) is excluded too
                let gone: Vec<Id> = inst.members.iter().filter(|(_, id)| d.get(**id).is_none()).map(|(k, _)| *k).collect();
                inst.excluded.extend(gone);
                d.transact("Exclude", |tx| tx.update(*g, Element::GroupInstance(inst)))?;
            }
            Ok(())
        })
    }

    /// Restore Excluded: bring back every member excluded from (or missing in) these instances.
    pub fn restore_excluded(&mut self, ids: &[Id]) -> Result<Vec<Id>, String> {
        self.grouped("Restore excluded", |d| {
            let mut back = vec![];
            for &g in ids {
                let mut inst = d.group_instance(g).cloned().ok_or("not a group")?;
                let ty = d.group_type(inst.group_type).cloned().ok_or("the group's type is missing")?;
                let live: BTreeMap<Id, Id> = inst.live(d).into_iter().collect();
                let skip: BTreeSet<Id> = ty.members.iter().map(|m| m.key).filter(|k| live.contains_key(k)).collect();
                if skip.len() == ty.members.len() {
                    continue;
                }
                let mut notes = vec![];
                let made = d.paste_members(&ty, &inst.frame, &skip, &live, &mut notes)?;
                back.extend(made.values().copied());
                inst.members = live.into_iter().chain(made).collect();
                inst.excluded.clear();
                d.transact("Restore excluded", |tx| tx.update(g, Element::GroupInstance(inst)))?;
            }
            Ok(back)
        })
    }

    pub fn rename_group_type(&mut self, id: Id, name: &str) -> Result<(), String> {
        let mut t = self.group_type(id).cloned().ok_or("not a group type")?;
        let name = name.trim();
        if name.is_empty() {
            return Err("the name cannot be empty".into());
        }
        if t.name == name {
            return Ok(());
        }
        if self.group_types().iter().any(|(i, o)| *i != id && o.name == name) {
            return Err(format!("there is already a group called {name}"));
        }
        t.name = name.to_string();
        self.transact("Rename group", |tx| tx.update(id, Element::GroupType(t)))
    }

    /// Add elements to a group being edited (Edit Group > Add).
    pub fn add_to_group(&mut self, group: Id, ids: &[Id]) -> Result<(), String> {
        let mut inst = self.group_instance(group).cloned().ok_or("not a group")?;
        let kind = self.group_type(inst.group_type).map(|t| t.kind).unwrap_or_default();
        for &id in ids {
            let e = self.get(id).ok_or_else(|| format!("no element {id}"))?;
            if matches!(e, Element::Level(_) | Element::Grid(_)) {
                return Err("levels and grids are never grouped".into());
            }
            if !fits_kind(kind, e) {
                return Err(match kind {
                    GroupKind::Model => "only walls, floors, roofs, doors, windows and components go in a model group",
                    GroupKind::Detail => "only detail lines, regions and text notes go in a detail group",
                }
                .into());
            }
            if kind == GroupKind::Detail && detail_view(e) != inst.frame.view {
                return Err("a detail group takes detail items from its own view".into());
            }
            match self.group_of(id) {
                Some(g) if g == group => continue,
                Some(_) => return Err("that element is already in another group".into()),
                None => {}
            }
            inst.members.insert(id, id);
        }
        // a door joins with its wall
        let walls: BTreeSet<Id> = inst.members.values().copied().collect();
        for (iid, i) in self.instances() {
            if i.host.is_some_and(|h| walls.contains(&h.wall) && ids.contains(&h.wall)) && self.group_of(iid).is_none() {
                inst.members.insert(iid, iid);
            }
        }
        self.transact("Add to group", |tx| tx.update(group, Element::GroupInstance(inst)))
    }

    /// Take elements out of a group being edited (Edit Group > Remove). They stay as ordinary elements.
    pub fn remove_from_group(&mut self, group: Id, ids: &[Id]) -> Result<(), String> {
        let mut inst = self.group_instance(group).cloned().ok_or("not a group")?;
        let before = inst.members.len();
        // a door goes with its wall
        let hosted: Vec<Id> = self.instances().into_iter().filter(|(_, i)| i.host.is_some_and(|h| ids.contains(&h.wall))).map(|(id, _)| id).collect();
        inst.members.retain(|_, id| !ids.contains(id) && !hosted.contains(id));
        if inst.members.len() == before {
            return Err("those elements are not in the group".into());
        }
        if inst.members.values().all(|id| self.get(*id).is_none()) {
            return Err("a group needs at least one member; use Ungroup instead".into());
        }
        self.transact("Remove from group", |tx| tx.update(group, Element::GroupInstance(inst)))
    }

    /// Finish Group: the edited instance becomes the type's definition and every other instance of
    /// the type is rebuilt from it in its own place. Elements keep their ids, marks and GUIDs.
    pub fn finish_group_edit(&mut self, group: Id) -> Result<Vec<String>, String> {
        let inst = self.group_instance(group).cloned().ok_or("not a group")?;
        if inst.live(self).is_empty() {
            return Err("a group needs at least one member; use Ungroup instead".into());
        }
        let tid = inst.group_type;
        let mut ty = self.group_type(tid).cloned().ok_or("the group's type is missing")?;
        ty.members = self.capture(&inst);
        ty.frame = inst.frame;
        let keys: BTreeSet<Id> = ty.members.iter().map(|m| m.key).collect();
        let mut notes = vec![];
        let ty2 = ty.clone();
        let mut edited = inst.clone();
        edited.members.retain(|k, id| keys.contains(k) || self.get(*id).is_some());
        edited.excluded.retain(|k| keys.contains(k));
        self.transact("Edit group", |tx| {
            tx.update(tid, Element::GroupType(ty2))?;
            tx.update(group, Element::GroupInstance(edited))
        })?;
        let others: Vec<Id> = self.group_instances().into_iter().filter(|(i, g)| *i != group && g.group_type == tid).map(|(i, _)| i).collect();
        for o in others {
            self.rebuild_instance(o, &ty, &mut notes)?;
        }
        Ok(notes)
    }

    /// Rebuild one instance from its type, in its own frame.
    fn rebuild_instance(&mut self, g: Id, ty: &GroupType, notes: &mut Vec<String>) -> Result<(), String> {
        let mut inst = self.group_instance(g).cloned().unwrap();
        let keys: BTreeSet<Id> = ty.members.iter().map(|m| m.key).collect();
        // members taken out of the type go from this instance
        let dropped: Vec<Id> = inst.members.iter().filter(|(k, id)| !keys.contains(k) && self.get(**id).is_some()).map(|(_, id)| *id).collect();
        if !dropped.is_empty() {
            self.delete(&dropped)?;
        }
        inst.members.retain(|k, _| keys.contains(k));
        inst.excluded.retain(|k| keys.contains(k));
        // an existing member whose element has gone stays out
        let mut skip = inst.excluded.clone();
        skip.extend(inst.members.iter().filter(|(_, id)| self.get(**id).is_none()).map(|(k, _)| *k));
        let made = self.paste_members(ty, &inst.frame, &skip, &BTreeMap::new(), notes)?;
        // each new copy takes over the element it replaces, so ids hold
        let mut final_id: BTreeMap<Id, Id> = BTreeMap::new(); // temporary id to the id it ends up with
        let mut swaps: Vec<(Id, Id)> = vec![]; // (old, temporary)
        for (k, tmp) in &made {
            match inst.members.get(k) {
                Some(old) if self.get(*old).is_some() => {
                    final_id.insert(*tmp, *old);
                    swaps.push((*old, *tmp));
                }
                _ => {
                    final_id.insert(*tmp, *tmp);
                }
            }
        }
        self.transact("Edit group", |tx| {
            for &(old, tmp) in &swaps {
                let doc = tx.doc_ref();
                let mut e = doc.get(tmp).cloned().unwrap();
                keep_identity(doc.get(old).unwrap(), &mut e);
                if let Element::FamilyInstance(i) = &mut e {
                    if let Some(h) = i.host {
                        i.host = Some(HostRef { wall: *final_id.get(&h.wall).unwrap_or(&h.wall), u_mm: h.u_mm });
                    }
                }
                tx.update(old, e)?;
            }
            // new members hosted in a wall that took over an old id point at that id
            for (&tmp, &fin) in &final_id {
                if tmp != fin {
                    continue;
                }
                if let Some(Element::FamilyInstance(i)) = tx.doc_ref().get(tmp).cloned() {
                    if let Some(h) = i.host {
                        if let Some(&w) = final_id.get(&h.wall) {
                            if w != h.wall {
                                tx.update(tmp, Element::FamilyInstance(crate::family::FamilyInstance { host: Some(HostRef { wall: w, u_mm: h.u_mm }), ..i }))?;
                            }
                        }
                    }
                }
            }
            for &(_, tmp) in &swaps {
                tx.remove(tmp)?;
            }
            for (k, tmp) in &made {
                inst.members.insert(*k, final_id[tmp]);
            }
            tx.update(g, Element::GroupInstance(inst))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall_of(d: &Document, id: Id) -> Wall {
        match d.get(id) {
            Some(Element::Wall(w)) => w.clone(),
            _ => panic!("not a wall"),
        }
    }

    #[test]
    fn levels_follow_the_reference_level() {
        let mut d = Document::new_project();
        let levels: Vec<Id> = d.levels().iter().map(|(i, _)| *i).collect();
        let (l1, l2) = (levels[0], levels[1]);
        let wt = d.wall_types()[0].0;
        let w = d.add_wall(wt, l1, Point::new(0.0, 0.0), Point::new(3000.0, 0.0), 2700.0).unwrap();
        let mut ww = wall_of(&d, w);
        ww.top_level = Some(l2);
        d.transact("top", |tx| tx.update(w, Element::Wall(ww))).unwrap();
        let (g, _) = d.create_group(&[w], None, None).unwrap();
        let t = d.group_instance(g).unwrap().group_type;
        let first = |d: &Document, g: Id| wall_of(d, *d.group_instance(g).unwrap().members.values().next().unwrap());
        // one level up: the top goes one level up too, unless there is none, when it stands unconnected
        let top = levels.get(2).copied();
        let (g2, notes) = d.place_group(t, Point::new(0.0, 10000.0), l2, 0.0).unwrap();
        let w2 = first(&d, g2);
        assert_eq!(w2.base_level, l2);
        assert_eq!(w2.top_level, top);
        let gap = d.level(l2).unwrap().elevation_mm - d.level(l1).unwrap().elevation_mm;
        if top.is_none() {
            assert!((w2.height_mm - gap).abs() < 1e-6);
            assert!(notes[0].contains("unconnected"));
        }
        // a level added above lets the next one connect
        let l3 = d.add_level(None, 9000.0, false).unwrap();
        if top.is_none() {
            let (g3, _) = d.place_group(t, Point::new(0.0, 20000.0), l2, 0.0).unwrap();
            assert_eq!(first(&d, g3).top_level, Some(l3));
        }
        // a mirrored copy reverses the frame and flips the walls
        let g4 = d.transform_elements(&[g], Xform::Mirror { ax: 0.0, ay: -1000.0, bx: 1000.0, by: -1000.0 }, EditOptions { copy: true, disjoin: false }).unwrap()[0];
        let f = d.group_instance(g4).unwrap().frame;
        assert!(f.mirrored);
        let w4 = first(&d, g4);
        assert!(w4.flipped != wall_of(&d, w).flipped);
        assert!((w4.start.y + 2000.0).abs() < 1e-6);
    }
}
