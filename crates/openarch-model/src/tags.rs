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
        let Some(Element::FamilyInstance(i)) = self.get(element) else { return None };
        match self.instance_family(i)?.0.category {
            FamilyCategory::Doors => Some(AnnotationKind::DoorTag),
            FamilyCategory::Windows => Some(AnnotationKind::WindowTag),
            _ => None,
        }
    }

    /// Where an element's tags hang from in a view: a door or window's opening centre (on the wall's location line in
    /// plan), in the view's own frame. None when the view cannot show the element's tags.
    pub fn tag_point(&self, view: Id, element: Id) -> Option<Point> {
        let v = self.view(view)?;
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
        let kind = self.tag_kind(t.element).ok_or("Only doors and windows can be tagged so far.")?;
        if self.tag_point(t.view, t.element).is_none() {
            return Err("Tags go in plan, elevation and section views.".into());
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
        let mut t = ElementTag { view, element, offset, family: o.family, orientation: o.orientation, leader: o.leader, leader_end: LeaderEnd::Attached, end_offset: None, elbow: None, pinned: false };
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
        self.instances().into_iter()
            .filter(|(id, _)| only.is_none_or(|s| s.contains(id)) && !v.hidden_elements.contains(id))
            .filter(|(id, i)| match &drawn { Some(d) => d.contains(id), None => v.level.is_none_or(|l| l == i.level) })
            .filter(|(id, _)| self.tag_kind(*id).is_some_and(|k| kinds.contains(&k)) && !self.is_tagged(view, *id) && self.tag_point(view, *id).is_some())
            .map(|(id, _)| id).collect()
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
