//! Tags of model elements: view-owned annotations that read their element's parameters (doors and windows now;
//! the tag kind comes from the element's category, so walls and multi-category tags can follow).
//!
//! A tag's head is stored as an offset from its element's tag point in the view, so the tag follows the element when
//! it moves. Plans measure in plan x and y; elevations and sections along the view and up.

use serde::{Deserialize, Serialize};

use crate::annotation::AnnotationKind;
use crate::family::FamilyCategory;
use crate::{Document, Element, Id, Point};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// Where a tag's leader ends: on its element (and moving with it), or at a point of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaderEnd {
    #[default]
    Attached,
    Free,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElementTag {
    pub view: Id,
    pub element: Id,
    /// The head's offset from the element's tag point, view mm.
    #[serde(default)]
    pub offset: [f64; 2],
    #[serde(default)]
    pub family: Option<Id>,
    #[serde(default)]
    pub orientation: TagOrientation,
    #[serde(default)]
    pub leader: bool,
    #[serde(default)]
    pub leader_end: LeaderEnd,
    /// A free leader end, as an offset from the tag point.
    #[serde(default)]
    pub end_offset: Option<[f64; 2]>,
    /// The leader's elbow, as an offset from the tag point; none draws the leader straight.
    #[serde(default)]
    pub elbow: Option<[f64; 2]>,
    #[serde(default)]
    pub pinned: bool,
    /// A material tag (Annotate > Material Tag): the material it names, of the tagged element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Where the tag points on its element (view mm), when that is not the element's own tag point (a material tag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 2]>,
    /// A keynote (Annotate > Keynote): where its key comes from (keynote.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keynote: Option<crate::keynote::KeynoteSource>,
}

/// How a new tag is placed: Tag by Category, Tag All and Tag on Placement share these.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagOptions {
    pub orientation: TagOrientation,
    pub leader: bool,
    /// Leader length on paper (mm): how far the head sits from the element when Leader is on.
    pub leader_mm: f64,
    /// A tag family to use instead of the category's first.
    pub family: Option<Id>,
}

impl Document {
    pub fn element_tags(&self, view: Id) -> Vec<(Id, &ElementTag)> {
        self.iter().filter_map(|(id, e)| match e { Element::ElementTag(t) if t.view == view => Some((id, t)), _ => None }).collect()
    }

    pub fn element_tag(&self, id: Id) -> Option<&ElementTag> {
        match self.get(id) { Some(Element::ElementTag(t)) => Some(t), _ => None }
    }

    /// The tag family kind an element takes, if it can be tagged.
    pub fn tag_kind(&self, element: Id) -> Option<AnnotationKind> {
        if let Some(Element::Ceiling(_)) = self.get(element) {
            return Some(AnnotationKind::CeilingTag);
        }
        let Some(Element::FamilyInstance(i)) = self.get(element) else { return None };
        match self.instance_family(i)?.0.category {
            FamilyCategory::Doors => Some(AnnotationKind::DoorTag),
            FamilyCategory::Windows => Some(AnnotationKind::WindowTag),
            _ => None,
        }
    }

    /// The tag family kind a tag takes: a material tag's, or its element's.
    pub fn kind_of_tag(&self, t: &ElementTag) -> Option<AnnotationKind> {
        if t.keynote.is_some() { Some(AnnotationKind::KeynoteTag) } else if t.material.is_some() { Some(AnnotationKind::MaterialTag) } else { self.tag_kind(t.element) }
    }

    /// Where a tag hangs from: its own point, or its element's tag point.
    pub fn tag_anchor(&self, t: &ElementTag) -> Option<Point> {
        match t.point {
            Some(p) if self.get(t.element).is_some() => Some(Point::new(p[0], p[1])),
            Some(_) => None,
            None => self.tag_point(t.view, t.element),
        }
    }

    /// The materials an element is made of, for Material Tag: its type's layers (outside or top first, voids left
    /// out). Walls, floors, roofs and ceilings.
    pub fn element_materials(&self, element: Id) -> Vec<String> {
        let layers = match self.get(element) {
            Some(Element::Wall(w)) => self.wall_type(w.wall_type).map(|t| t.layers.clone()),
            Some(Element::Floor(f)) => self.floor_type(f.floor_type).map(|t| t.layers.clone()),
            Some(Element::Roof(r)) => self.roof_type(r.roof_type).map(|t| t.layers.clone()),
            Some(Element::Ceiling(c)) => self.ceiling_type(c.ceiling_type).map(|t| t.layers.clone()),
            _ => None,
        };
        let mut out: Vec<String> = vec![];
        for l in layers.unwrap_or_default() {
            if !l.is_void && !l.material.is_empty() && !out.contains(&l.material) {
                out.push(l.material);
            }
        }
        out
    }

    /// Tag the material at a point of a wall, floor, roof or ceiling (Annotate > Material Tag): `material` is the one
    /// under the click when the view knows it, else the element's structural (or first) material. The head sits a
    /// leader's length up and to the right; the leader is on, as in Revit.
    pub fn add_material_tag(&mut self, view: Id, element: Id, material: Option<&str>, point: Point, o: &TagOptions) -> Result<Id, String> {
        let mats = self.element_materials(element);
        if mats.is_empty() {
            return Err("Material tags go on walls, floors, roofs and ceilings so far.".into());
        }
        let v = self.view(view).ok_or("not a view")?;
        if !(v.view_kind.is_plan() || self.elevation_frame(view).is_some()) {
            return Err("Tags go in plan, elevation and section views.".into());
        }
        let structural = || {
            let layers = match self.get(element) {
                Some(Element::Wall(w)) => self.wall_type(w.wall_type).map(|t| t.layers.clone()),
                Some(Element::Floor(f)) => self.floor_type(f.floor_type).map(|t| t.layers.clone()),
                Some(Element::Roof(r)) => self.roof_type(r.roof_type).map(|t| t.layers.clone()),
                _ => None,
            };
            layers.unwrap_or_default().into_iter().find(|l| l.structural_material || l.core).map(|l| l.material)
        };
        let material = match material.filter(|m| self.material(m).is_some()) {
            Some(m) => m.to_string(),
            None => structural().filter(|m| mats.contains(m)).unwrap_or_else(|| mats[0].clone()),
        };
        let paper = if o.leader_mm > 0.0 { o.leader_mm } else { 10.0 };
        let k = paper * self.tag_scale(view) * std::f64::consts::FRAC_1_SQRT_2;
        let mut t = ElementTag { view, element, offset: [k, k], family: o.family, orientation: o.orientation, leader: true, leader_end: LeaderEnd::Attached, end_offset: None, elbow: None, pinned: false, material: Some(material), point: Some([point.x, point.y]), keynote: None };
        self.check_tag(&t)?;
        self.grouped("Material Tag", |d| {
            if t.family.is_none() { t.family = Some(d.ensure_tag_family(AnnotationKind::MaterialTag)?); }
            d.transact("Material Tag", |tx| Ok(tx.add(Element::ElementTag(t))))
        })
    }

    /// A keynote on an element at a point (Annotate > Keynote): Element (its type's Keynote), Material (the material
    /// there, as Material Tag picks it) or User (a key from the keynote table). The leader is on, as in Revit.
    pub fn add_keynote_tag(&mut self, view: Id, element: Id, source: crate::keynote::KeynoteSource, material: Option<&str>, point: Point, o: &TagOptions) -> Result<Id, String> {
        if self.get(element).is_none() {
            return Err("Click an element to keynote.".into());
        }
        let v = self.view(view).ok_or("not a view")?;
        if !(v.view_kind.is_plan() || self.elevation_frame(view).is_some()) {
            return Err("Keynotes go in plan, elevation and section views.".into());
        }
        let material = match &source {
            crate::keynote::KeynoteSource::Material => {
                let mats = self.element_materials(element);
                if mats.is_empty() { return Err("Material keynotes go on walls, floors, roofs and ceilings so far.".into()); }
                Some(material.filter(|m| self.material(m).is_some()).map(str::to_string).unwrap_or_else(|| mats[0].clone()))
            }
            crate::keynote::KeynoteSource::User { key } => {
                if self.keynote_text(key).is_none() { return Err(format!("The keynote table has no key {key}: load a keynote file first.")); }
                None
            }
            crate::keynote::KeynoteSource::Element => None,
        };
        let paper = if o.leader_mm > 0.0 { o.leader_mm } else { 10.0 };
        let k = paper * self.tag_scale(view) * std::f64::consts::FRAC_1_SQRT_2;
        let mut t = ElementTag { view, element, offset: [k, k], family: o.family, orientation: o.orientation, leader: true, leader_end: LeaderEnd::Attached, end_offset: None, elbow: None, pinned: false, material, point: Some([point.x, point.y]), keynote: Some(source) };
        self.check_tag(&t)?;
        self.grouped("Keynote", |d| {
            if t.family.is_none() { t.family = Some(d.ensure_tag_family(AnnotationKind::KeynoteTag)?); }
            d.transact("Keynote", |tx| Ok(tx.add(Element::ElementTag(t))))
        })
    }

    /// Where an element's tags hang from in a view: a door or window's opening centre (on the wall's location line in
    /// plan), in the view's own frame. None when the view cannot show the element's tags.
    pub fn tag_point(&self, view: Id, element: Id) -> Option<Point> {
        let v = self.view(view)?;
        // a ceiling's tag: in the middle of its largest piece, in plans of its level
        if let Some(Element::Ceiling(c)) = self.get(element) {
            if !v.view_kind.is_plan() || v.level != Some(c.level) {
                return None;
            }
            let r = c.regions(0.5).into_iter().max_by(|a, b| a.area().total_cmp(&b.area()))?;
            return Some(inside_point(&r));
        }
        let Some(Element::FamilyInstance(i)) = self.get(element) else { return None };
        let frame = self.instance_frame(element)?;
        let at = frame.fam_world(0.0, 0.0);
        if v.view_kind.is_plan() {
            return Some(at);
        }
        let f = self.elevation_frame(view)?;
        let z = match self.instance_cut_box(i) {
            Some(b) => frame.z_ref + (b[0][2] + b[1][2]) / 2.0,
            None => frame.z_ref + self.instance_opening_size(i).1 / 2.0,
        };
        Some(Point::new((at.x - f.origin[0]) * f.right[0] + (at.y - f.origin[1]) * f.right[1], z))
    }

    /// Whether an element already has a tag in this view (Tag All skips it).
    pub fn is_tagged(&self, view: Id, element: Id) -> bool {
        self.iter().any(|(_, e)| matches!(e, Element::ElementTag(t) if t.view == view && t.element == element))
    }

    /// The view's paper scale (1:n), for paper-mm leader lengths.
    fn tag_scale(&self, view: Id) -> f64 {
        self.effective_settings(view).map_or(100.0, |s| s.scale.max(1) as f64)
    }

    fn check_tag(&self, t: &ElementTag) -> Result<(), String> {
        let kind = self.kind_of_tag(t).ok_or("Only doors and windows can be tagged so far.")?;
        if self.tag_anchor(t).is_none() {
            return Err("Tags go in plan, elevation and section views.".into());
        }
        if let Some(m) = &t.material {
            if self.material(m).is_none() {
                return Err(format!("There is no material named '{m}'."));
            }
        }
        if t.point.is_some_and(|p| !p.iter().all(|v| v.is_finite())) {
            return Err("The tag's position must be numbers.".into());
        }
        if let Some(f) = t.family {
            if self.annotation_family(f).is_none_or(|f| f.kind != kind) {
                return Err(format!("Choose a {} family.", kind.label().to_lowercase()));
            }
        }
        let finite = |p: &[f64; 2]| p.iter().all(|v| v.is_finite());
        if !finite(&t.offset) || t.end_offset.as_ref().is_some_and(|p| !finite(p)) || t.elbow.as_ref().is_some_and(|p| !finite(p)) {
            return Err("The tag's position must be numbers.".into());
        }
        Ok(())
    }

    /// Tag an element in a view: the head at the tag point, or a leader length away (above in plan, beside the
    /// opening in an elevation) when the leader is on.
    pub fn add_element_tag(&mut self, view: Id, element: Id, o: &TagOptions) -> Result<Id, String> {
        let kind = self.tag_kind(element).ok_or("Only doors and windows can be tagged so far.")?;
        if !o.leader_mm.is_finite() || o.leader_mm < 0.0 {
            return Err("The leader length must be a positive distance.".into());
        }
        let paper = if o.leader_mm > 0.0 { o.leader_mm } else { 12.0 };
        let length = if o.leader { paper * self.tag_scale(view) } else { 0.0 };
        let offset = if self.view(view).is_some_and(|v| v.view_kind.is_plan()) { [0.0, length] } else { [length, 0.0] };
        let mut t = ElementTag { view, element, offset, family: o.family, orientation: o.orientation, leader: o.leader, leader_end: LeaderEnd::Attached, end_offset: None, elbow: None, pinned: false, material: None, point: None, keynote: None };
        self.check_tag(&t)?;
        self.grouped("Tag", |d| {
            if t.family.is_none() { t.family = Some(d.ensure_tag_family(kind)?); }
            d.transact("Tag", |tx| Ok(tx.add(Element::ElementTag(t))))
        })
    }

    /// The elements of these tag kinds the view shows that have no tag in it (only those in `only` when given): what
    /// Tag All tags. A plan shows its own level's doors and windows; an elevation or section what it draws (within
    /// its crop and far clip, hidden behind other elements or not). Elements hidden in the view are left out.
    pub fn untagged(&self, view: Id, kinds: &[AnnotationKind], only: Option<&[Id]>) -> Vec<Id> {
        let Some(v) = self.view(view) else { return vec![] };
        let drawn: Option<std::collections::BTreeSet<Id>> = if v.view_kind.is_plan() { None } else {
            match self.elevation_data(view) { Ok(e) => Some(e.items.iter().map(|i| i.id).collect()), Err(_) => return vec![] }
        };
        let mut out: Vec<Id> = self.instances().into_iter()
            .filter(|(id, _)| only.is_none_or(|s| s.contains(id)) && !v.hidden_elements.contains(id))
            .filter(|(id, i)| match &drawn { Some(d) => d.contains(id), None => v.level.is_none_or(|l| l == i.level) })
            .filter(|(id, _)| self.tag_kind(*id).is_some_and(|k| kinds.contains(&k)) && !self.is_tagged(view, *id) && self.tag_point(view, *id).is_some())
            .map(|(id, _)| id).collect();
        // ceilings: those of the plan's level
        if kinds.contains(&AnnotationKind::CeilingTag) {
            out.extend(self.ceilings().into_iter().map(|(id, _)| id)
                .filter(|id| only.is_none_or(|s| s.contains(id)) && !v.hidden_elements.contains(id) && !self.is_tagged(view, *id) && self.tag_point(view, *id).is_some()));
        }
        out
    }

    /// Tag every untagged door and window of `kinds` the view shows (or only those in `only`), as one undo step; each
    /// kind with its family, or the category's first.
    pub fn tag_all(&mut self, view: Id, kinds: &[(AnnotationKind, Option<Id>)], only: Option<&[Id]>, o: &TagOptions) -> Result<Vec<Id>, String> {
        let wanted: Vec<AnnotationKind> = kinds.iter().map(|(k, _)| *k).collect();
        let todo: Vec<(Id, Option<Id>)> = self.untagged(view, &wanted, only).into_iter()
            .filter_map(|id| { let k = self.tag_kind(id)?; Some((id, kinds.iter().find(|(kk, _)| *kk == k)?.1)) }).collect();
        self.grouped("Tag All", |d| todo.into_iter().map(|(id, family)| d.add_element_tag(view, id, &TagOptions { family, ..o.clone() })).collect())
    }

    pub fn patch_element_tag(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.element_tag(id).cloned().ok_or("not a tag")?;
        let mut t: ElementTag = crate::doc::apply_patch(&old, patch)?;
        t.view = old.view;
        t.element = old.element;
        if old.pinned && t.pinned && (t.offset != old.offset || t.end_offset != old.end_offset || t.elbow != old.elbow) {
            return Err("Unpin the tag before moving it.".into());
        }
        // a free end starts where the attached end was
        if t.leader_end == LeaderEnd::Free && t.end_offset.is_none() { t.end_offset = Some([0.0, 0.0]); }
        self.check_tag(&t)?;
        self.transact("Tag properties", |tx| tx.update(id, Element::ElementTag(t)))
    }

    /// The Mark a newly placed instance takes: doors and windows always number on; other categories only carry on a
    /// numbering someone has started.
    pub fn auto_mark(&self, category: FamilyCategory) -> String {
        let started = || self.instances().iter().any(|(_, i)| !i.mark.trim().is_empty() && self.instance_family(i).is_some_and(|(f, _)| f.category == category));
        if matches!(category, FamilyCategory::Doors | FamilyCategory::Windows) || started() { self.next_mark(category) } else { String::new() }
    }

    /// The next Mark for a new door or window: the most recent Mark in its category, its number counted up (prefix and
    /// zero padding kept), skipping Marks already used; "1" when the category has none.
    pub fn next_mark(&self, category: FamilyCategory) -> String {
        let marks: Vec<(Id, &str)> = self.instances().into_iter()
            .filter(|(_, i)| self.instance_family(i).is_some_and(|(f, _)| f.category == category))
            .map(|(id, i)| (id, i.mark.as_str())).collect();
        let Some(&(_, last)) = marks.iter().filter(|(_, m)| !m.trim().is_empty()).max_by_key(|(id, _)| *id) else { return "1".into() };
        let start = last.char_indices().rev().take_while(|(_, c)| c.is_ascii_digit()).last().map_or(last.len(), |(i, _)| i);
        let (prefix, digits) = last.split_at(start);
        let width = digits.len();
        let mut n: u64 = digits.parse().unwrap_or(0);
        loop {
            n += 1;
            let mark = format!("{prefix}{n:0width$}");
            if !marks.iter().any(|(_, m)| *m == mark) { return mark; }
        }
    }
}

/// A point inside a region for a label: its outline's centroid when that falls inside, else the middle of its
/// largest triangle.
fn inside_point(r: &crate::geom::Region) -> Point {
    let pts = &r.outer;
    let a = crate::geom::signed_area(pts);
    if a.abs() > 1e-9 {
        let (mut cx, mut cy) = (0.0, 0.0);
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            let k = p.x * q.y - q.x * p.y;
            cx += (p.x + q.x) * k;
            cy += (p.y + q.y) * k;
        }
        let c = Point::new(cx / (6.0 * a), cy / (6.0 * a));
        if crate::rooms::contains(r, c) {
            return c;
        }
    }
    crate::geom::triangulate_region(r)
        .and_then(|(ps, tris)| tris.iter().map(|t| [ps[t[0]], ps[t[1]], ps[t[2]]]).max_by(|a, b| tri_area(a).total_cmp(&tri_area(b))))
        .map_or(pts[0], |t| Point::new((t[0].x + t[1].x + t[2].x) / 3.0, (t[0].y + t[1].y + t[2].y) / 3.0))
}

fn tri_area(t: &[Point; 3]) -> f64 {
    ((t[1].x - t[0].x) * (t[2].y - t[0].y) - (t[2].x - t[0].x) * (t[1].y - t[0].y)).abs() / 2.0
}
