//! Reusable marker types selected by section and elevation view types.

use serde::{Deserialize, Serialize};
use crate::annotation::AnnotationKind;
use crate::doc::{apply_patch, Document};
use crate::element::{Element, Id};
use crate::view::ViewKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewMarkerType {
    pub name: String,
    pub view_kind: ViewKind,
    #[serde(default)]
    pub head: Option<Id>,
    #[serde(default)]
    pub tail: Option<Id>,
}

impl Document {
    pub fn view_marker_types(&self) -> Vec<(Id, &ViewMarkerType)> {
        self.iter().filter_map(|(id, e)| match e {
            Element::ViewMarkerType(t) => Some((id, t)),
            _ => None,
        }).collect()
    }

    pub fn view_marker_type(&self, id: Id) -> Option<&ViewMarkerType> {
        match self.get(id) {
            Some(Element::ViewMarkerType(t)) => Some(t),
            _ => None,
        }
    }

    pub fn marker_type_for_view(&self, view: Id) -> Option<(Id, &ViewMarkerType)> {
        let view = self.view(view)?;
        let view_type = self.view_type(self.view_type_of(view)?)?;
        let id = view_type.marker_type?;
        self.view_marker_type(id).map(|t| (id, t))
    }

    pub fn section_symbols(&self, section: &crate::section::Section) -> (Option<Id>, Option<Id>) {
        let typed = section.reference_marker_type.and_then(|id| self.view_marker_type(id))
            .or_else(|| section.view.or(section.reference).and_then(|id| self.marker_type_for_view(id).map(|(_, t)| t)));
        typed.map(|t| (t.head, t.tail)).unwrap_or((section.family, section.tail_family))
    }

    pub fn elevation_symbol(&self, marker: &crate::site::ElevationMarker) -> Option<Id> {
        marker.slots.iter().flatten().find_map(|id| self.marker_type_for_view(*id))
            .map(|(_, t)| t.head).unwrap_or(marker.family)
    }

    pub fn patch_view_marker_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.view_marker_type(id).cloned().ok_or("not a marker type")?;
        let new: ViewMarkerType = apply_patch(&old, patch)?;
        if new == old { return Ok(()); }
        if new.view_kind != old.view_kind { return Err("a marker type keeps its view kind".into()); }
        if new.name.trim().is_empty() || self.view_marker_types().iter().any(|(i, t)| *i != id && t.name == new.name) {
            return Err("a marker type needs a name of its own".into());
        }
        let kind = match new.view_kind {
            ViewKind::Section => AnnotationKind::SectionMarker,
            ViewKind::Elevation => AnnotationKind::ElevationMarker,
            _ => return Err("marker types belong to sections or elevations".into()),
        };
        if [new.head, new.tail].into_iter().flatten().any(|id| !self.annotation_family(id).is_some_and(|f| f.kind == kind)) {
            return Err("that symbol belongs to a different marker family".into());
        }
        if new.view_kind == ViewKind::Elevation && new.tail.is_some() {
            return Err("an elevation marker type has one symbol".into());
        }
        self.transact("Change marker type", |tx| tx.update(id, Element::ViewMarkerType(new)))
    }

    pub fn duplicate_view_marker_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let original = self.view_marker_type(id).cloned().ok_or("not a marker type")?;
        let name = name.trim();
        if name.is_empty() || self.view_marker_types().iter().any(|(_, t)| t.name == name) {
            return Err("a marker type needs a name of its own".into());
        }
        self.transact("Duplicate marker type", |tx| Ok(tx.add(Element::ViewMarkerType(ViewMarkerType { name: name.into(), ..original }))))
    }

    pub fn patch_view_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.view_type(id).cloned().ok_or("not a view type")?;
        let new: crate::view::ViewType = apply_patch(&old, patch)?;
        if new == old { return Ok(()); }
        if new.view_kind != old.view_kind { return Err("a view type keeps its kind".into()); }
        if new.name.trim().is_empty() || self.view_types().iter().any(|(i, t)| *i != id && t.name == new.name) {
            return Err("a view type needs a name of its own".into());
        }
        if let Some(marker) = new.marker_type {
            if !self.view_marker_type(marker).is_some_and(|t| t.view_kind == new.view_kind) {
                return Err("that marker type belongs to a different kind of view".into());
            }
        }
        if let Some(template) = new.template_for_new {
            if !self.view_template(template).is_some_and(|t| t.view_kind == new.view_kind) {
                return Err("that template belongs to a different kind of view".into());
            }
        }
        self.transact("Change view type", |tx| tx.update(id, Element::ViewType(new)))
    }

    // Reuse an existing combination during migration. Custom old instance symbols get their own view type.
    fn legacy_marker_type(&mut self, kind: ViewKind, head: Option<Id>, tail: Option<Id>) -> Id {
        if let Some((id, _)) = self.view_marker_types().into_iter().find(|(_, t)| t.view_kind == kind && t.head == head && t.tail == tail) {
            return id;
        }
        let base = if kind == ViewKind::Section { "Section Tag" } else { "Elevation Mark" };
        let mut name = base.to_string();
        let mut index = 2;
        while self.view_marker_types().iter().any(|(_, t)| t.name == name) {
            name = format!("{base} {index}"); index += 1;
        }
        self.transact("Create marker type", |tx| Ok(tx.add(Element::ViewMarkerType(ViewMarkerType { name, view_kind: kind, head, tail })))).expect("valid marker type")
    }

    fn migrate_marker_view(&mut self, id: Id, head: Option<Id>, tail: Option<Id>, old_types: &[Id]) {
        let Some(mut view) = self.view(id).cloned() else { return };
        let Some(old_type_id) = self.view_type_of(&view) else { return };
        if !old_types.contains(&old_type_id) { return; }
        let Some(old_type) = self.view_type(old_type_id).cloned() else { return };
        let marker = self.legacy_marker_type(view.view_kind, head, tail);
        if old_type.marker_type == Some(marker) { return; }
        let reused = self.view_types().into_iter().find(|(_, t)| t.view_kind == old_type.view_kind && t.marker_type == Some(marker)
            && t.name.starts_with(&format!("{} (", old_type.name))).map(|(id, _)| id);
        let target = reused.unwrap_or_else(|| {
            let mut index = 2;
            let name = loop {
                let name = format!("{} (Marker {index})", old_type.name);
                if !self.view_types().iter().any(|(_, t)| t.name == name) { break name; }
                index += 1;
            };
            self.transact("Migrate view type", |tx| Ok(tx.add(Element::ViewType(crate::view::ViewType { name, marker_type: Some(marker), ..old_type })))).expect("valid migrated view type")
        });
        view.view_type = Some(target);
        let _ = self.transact("Migrate marker view", |tx| tx.update(id, Element::View(view)));
    }

    /// Upgrade old per-instance symbols, preserving distinct drawings rather than merging their choices.
    pub fn ensure_view_marker_types(&mut self) {
        let missing: Vec<_> = self.view_types().into_iter().filter(|(_, t)| matches!(t.view_kind, ViewKind::Section | ViewKind::Elevation) && t.marker_type.is_none())
            .map(|(id, t)| (id, t.clone())).collect();
        if missing.is_empty() { return; }
        let old_types: Vec<Id> = missing.iter().map(|(id, _)| *id).collect();
        for (id, mut view_type) in missing {
            let kind = if view_type.view_kind == ViewKind::Section { AnnotationKind::SectionMarker } else { AnnotationKind::ElevationMarker };
            view_type.marker_type = Some(self.legacy_marker_type(view_type.view_kind, self.first_family(kind), None));
            let _ = self.transact("Migrate view type", |tx| tx.update(id, Element::ViewType(view_type)));
        }
        let sections: Vec<_> = self.sections().into_iter().map(|(id, s)| (id, s.clone())).collect();
        for (id, mut section) in sections {
            let head = section.family.or_else(|| self.first_family(AnnotationKind::SectionMarker));
            if let Some(view) = section.view {
                self.migrate_marker_view(view, head, section.tail_family, &old_types);
            } else if section.family.is_some() || section.tail_family.is_some() {
                section.reference_marker_type = Some(self.legacy_marker_type(ViewKind::Section, head, section.tail_family));
                let _ = self.transact("Migrate reference marker", |tx| tx.update(id, Element::Section(section)));
            }
        }
        let elevations: Vec<_> = self.elevation_markers().into_iter().map(|(_, m)| m.clone()).collect();
        for marker in elevations {
            let head = marker.family.or_else(|| self.first_family(AnnotationKind::ElevationMarker));
            for view in marker.slots.into_iter().flatten() {
                self.migrate_marker_view(view, head, None, &old_types);
            }
        }
        self.clear_history();
    }
}
