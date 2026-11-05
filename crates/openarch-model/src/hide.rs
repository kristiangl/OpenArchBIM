//! Hiding things in a view: Temporary Hide/Isolate (kept for the session, not saved, not an undo step), Hide in
//! View (elements and categories, saved with the view) and Reveal Hidden Elements (shows what is hidden, in its own
//! colour, until it is turned off).
//!
//! The temporary state and Reveal Hidden are folded into a view only while it is drawn (`with_display`), so every
//! renderer that already honours the view's hidden elements and category visibility follows them.

use std::collections::{BTreeMap, BTreeSet};

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::spot::SpotKind;
use crate::view::{CategoryOverride, TemplateProp, ViewSettings, ANNOTATION_CATEGORIES, MODEL_CATEGORIES};

/// The colour Reveal Hidden Elements draws hidden elements in.
pub const REVEAL_COLOR: [u8; 3] = [210, 0, 160];

/// A view's Temporary Hide/Isolate.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TempHide {
    pub hidden: BTreeSet<Id>,
    /// Isolated elements: everything else is hidden.
    pub isolated: Option<BTreeSet<Id>>,
    pub hidden_categories: BTreeSet<String>,
    /// Isolated categories: every other category is hidden.
    pub isolated_categories: Option<BTreeSet<String>>,
}

impl TempHide {
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.isolated.is_none() && self.hidden_categories.is_empty() && self.isolated_categories.is_none()
    }
}

/// What the session shows differently from the saved views.
#[derive(Debug, Clone, Default)]
pub struct ViewDisplay {
    pub(crate) temp: BTreeMap<Id, TempHide>,
    pub(crate) reveal: BTreeSet<Id>,
}

/// The four Temporary Hide/Isolate choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempOp {
    HideElements,
    IsolateElements,
    HideCategories,
    IsolateCategories,
}

/// Annotation categories that are not in the Visibility/Graphics list but are drawn by name.
const EXTRA_ANNOTATION: &[&str] = &["Lines", "Area Boundary", "Area Tags", "Color Fill Legends"];

/// True when `category` belongs to the Annotation Categories tab.
pub fn is_annotation_category(category: &str) -> bool {
    ANNOTATION_CATEGORIES.contains(&category) || EXTRA_ANNOTATION.contains(&category) || category.ends_with(" Tags")
}

fn set_category_visible(s: &mut ViewSettings, category: &str, visible: bool) {
    let map = if is_annotation_category(category) { &mut s.overrides.annotation } else { &mut s.overrides.model };
    map.entry(category.to_string()).or_default().visible = visible;
}

fn category_visible(s: &ViewSettings, category: &str) -> bool {
    s.overrides.model.get(category).or_else(|| s.overrides.annotation.get(category)).is_none_or(|o| o.visible)
}

impl Document {
    /// The category an element shows under in Visibility/Graphics, for the elements a view draws.
    pub fn visibility_category(&self, id: Id) -> Option<String> {
        let c = match self.get(id)? {
            Element::Wall(_) => "Walls",
            Element::Floor(_) => "Floors",
            Element::Roof(_) => "Roofs",
            Element::Ceiling(_) => "Ceilings",
            Element::Stair(_) => "Stairs",
            Element::Railing(_) => "Railings",
            Element::FamilyInstance(i) => return self.instance_family(i).map(|(f, _)| f.category.label().to_string()),
            Element::Room(_) => "Rooms",
            Element::RoomSeparation(_) => "Room Separation",
            Element::RoomTag(_) => "Room Tags",
            Element::Area(_) => "Areas",
            Element::AreaBoundary(_) => "Area Boundary",
            Element::AreaTag(_) => "Area Tags",
            Element::Grid(_) => "Grids",
            Element::Level(_) => "Levels",
            Element::Dimension(_) => "Dimensions",
            Element::Spot(s) => match s.kind { SpotKind::Elevation => "Spot Elevations", SpotKind::Coordinate => "Spot Coordinates", SpotKind::Slope => "Spot Slopes" },
            Element::Section(_) => "Sections",
            Element::ElevationMarker(_) => "Elevations",
            Element::DetailLine(_) => "Lines",
            Element::DetailRegion(_) => "Detail Items",
            Element::TextNote(_) => "Text Notes",
            Element::RevisionCloud(_) => "Revision Clouds",
            Element::RevisionTag(_) => "Revision Tags",
            Element::ElementTag(t) => return self.tag_kind(t.element).map(|k| k.category().to_string()),
            Element::ColorFillLegend(_) => "Color Fill Legends",
            _ => return None,
        };
        Some(c.to_string())
    }

    /// True when the view hides the element itself (Hide in View, or for now).
    pub fn hidden_in(&self, view: Id, id: Id) -> bool {
        self.view(view).is_some_and(|v| v.hidden_elements.contains(&id) || v.element_overrides.get(&id).is_some_and(|o| !o.visible))
    }

    fn display_view(&self, view: Id) -> Result<(), String> {
        let v = self.view(view).ok_or("Unknown view.")?;
        if v.schedule.is_some() { return Err("A schedule has nothing to hide.".into()); }
        Ok(())
    }

    /// The view's Temporary Hide/Isolate, if it has one.
    pub fn temp_hide(&self, view: Id) -> Option<&TempHide> {
        self.display.temp.get(&view)
    }

    /// Temporary Hide/Isolate: hide or isolate the elements `ids`, or their categories. Hiding adds to what is hidden;
    /// isolating replaces what was isolated.
    pub fn temp_hide_apply(&mut self, view: Id, op: TempOp, ids: &[Id]) -> Result<(), String> {
        self.display_view(view)?;
        if ids.is_empty() { return Err("Select elements first.".into()); }
        let cats: BTreeSet<String> = ids.iter().filter_map(|i| self.visibility_category(*i)).collect();
        if matches!(op, TempOp::HideCategories | TempOp::IsolateCategories) && cats.is_empty() {
            return Err("The selection has no category to hide.".into());
        }
        let t = self.display.temp.entry(view).or_default();
        match op {
            TempOp::HideElements => t.hidden.extend(ids.iter().copied()),
            TempOp::IsolateElements => t.isolated = Some(ids.iter().copied().collect()),
            TempOp::HideCategories => t.hidden_categories.extend(cats),
            TempOp::IsolateCategories => t.isolated_categories = Some(cats),
        }
        Ok(())
    }

    /// Reset Temporary Hide/Isolate.
    pub fn temp_hide_reset(&mut self, view: Id) {
        self.display.temp.remove(&view);
    }

    /// Apply Hide/Isolate to View: what is hidden for now becomes hidden in the view (its elements, and its
    /// categories in Visibility/Graphics). One undo step.
    pub fn temp_hide_keep(&mut self, view: Id) -> Result<(), String> {
        let t = self.display.temp.get(&view).cloned().ok_or("Nothing is hidden or isolated for now in this view.")?;
        let mut v = self.view(view).ok_or("Unknown view.")?.clone();
        let mut hide_cats: BTreeSet<String> = t.hidden_categories.clone();
        if let Some(iso) = &t.isolated_categories {
            hide_cats.extend(self.all_visibility_categories().into_iter().filter(|c| !iso.contains(c)));
        }
        if !hide_cats.is_empty() {
            self.refuse_template_categories(&v, &hide_cats)?;
            for c in &hide_cats { set_category_visible(&mut v.settings, c, false); }
        }
        v.hidden_elements.extend(t.hidden.iter().copied());
        if let Some(iso) = &t.isolated {
            let others: Vec<Id> = self.elements.keys().copied().filter(|i| !iso.contains(i) && self.visibility_category(*i).is_some()).collect();
            v.hidden_elements.extend(others);
        }
        self.transact("Apply hide/isolate to view", |tx| tx.update(view, Element::View(v)))?;
        self.display.temp.remove(&view);
        Ok(())
    }

    /// Every category a view could switch off: the listed ones and those of the elements in the project.
    fn all_visibility_categories(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = MODEL_CATEGORIES.iter().chain(ANNOTATION_CATEGORIES).chain(EXTRA_ANNOTATION).map(|s| s.to_string()).collect();
        out.extend(self.elements.keys().filter_map(|i| self.visibility_category(*i)));
        out
    }

    fn refuse_template_categories(&self, v: &crate::view::View, cats: &BTreeSet<String>) -> Result<(), String> {
        let Some(t) = v.template.and_then(|t| self.view_template(t)) else { return Ok(()) };
        let model = cats.iter().any(|c| !is_annotation_category(c)) && t.included.contains(&TemplateProp::VgModel);
        let ann = cats.iter().any(|c| is_annotation_category(c)) && t.included.contains(&TemplateProp::VgAnnotation);
        if model || ann { return Err("The view template controls this view's Visibility/Graphics.".into()); }
        Ok(())
    }

    /// Hide in View > Category (and Unhide Category): the categories of `ids` switched off or on in Visibility/Graphics.
    pub fn hide_categories_in_view(&mut self, view: Id, ids: &[Id], hide: bool) -> Result<Vec<String>, String> {
        self.display_view(view)?;
        let cats: BTreeSet<String> = ids.iter().filter_map(|i| self.visibility_category(*i)).collect();
        if cats.is_empty() { return Err("The selection has no category to hide.".into()); }
        let mut v = self.view(view).ok_or("Unknown view.")?.clone();
        self.refuse_template_categories(&v, &cats)?;
        for c in &cats { set_category_visible(&mut v.settings, c, !hide); }
        self.transact(if hide { "Hide category" } else { "Unhide category" }, |tx| tx.update(view, Element::View(v)))?;
        Ok(cats.into_iter().collect())
    }

    /// Override Graphics in View > By Element: the override set on each of `ids` (None clears it).
    pub fn set_element_overrides(&mut self, view: Id, ids: &[Id], over: Option<CategoryOverride>) -> Result<(), String> {
        self.display_view(view)?;
        if ids.is_empty() { return Err("Select elements first.".into()); }
        let mut v = self.view(view).ok_or("Unknown view.")?.clone();
        for id in ids {
            if self.visibility_category(*id).is_none() { return Err("That element has no graphics to override.".into()); }
            match &over {
                Some(o) => { v.element_overrides.insert(*id, o.clone()); }
                None => { v.element_overrides.remove(id); }
            }
        }
        self.transact("Override graphics in view", |tx| tx.update(view, Element::View(v)))
    }

    /// Reveal Hidden Elements on or off for a view.
    pub fn set_reveal_hidden(&mut self, view: Id, on: bool) -> Result<(), String> {
        self.display_view(view)?;
        if on { self.display.reveal.insert(view); } else { self.display.reveal.remove(&view); }
        Ok(())
    }

    pub fn reveal_hidden(&self, view: Id) -> bool {
        self.display.reveal.contains(&view)
    }

    /// What Reveal Hidden Elements shows in its own colour: elements hidden in the view and those of the categories
    /// it switches off.
    pub fn revealed_elements(&self, view: Id) -> BTreeSet<Id> {
        let Some(v) = self.view(view) else { return BTreeSet::new() };
        let s = self.effective_settings_of(v);
        let mut out: BTreeSet<Id> = v.hidden_elements.clone();
        out.extend(v.element_overrides.iter().filter(|(_, o)| !o.visible).map(|(i, _)| *i));
        out.extend(self.elements.keys().copied().filter(|i| self.visibility_category(*i).is_some_and(|c| !category_visible(&s, &c))));
        out
    }

    /// The view as it is drawn this session: with its Temporary Hide/Isolate, or with what is hidden shown in the
    /// reveal colour. None when it is drawn as saved.
    fn displayed_view(&self, view: Id) -> Option<crate::view::View> {
        let reveal = self.display.reveal.contains(&view);
        let temp = self.display.temp.get(&view).filter(|t| !t.is_empty());
        if !reveal && temp.is_none() { return None; }
        let mut v = self.view(view)?.clone();
        let mut s = self.effective_settings_of(&v);
        if reveal {
            let revealed = self.revealed_elements(view);
            for o in s.overrides.model.values_mut().chain(s.overrides.annotation.values_mut()) { o.visible = true; }
            v.hidden_elements.clear();
            for id in revealed {
                let o = v.element_overrides.entry(id).or_default();
                o.visible = true;
                o.halftone = false;
                o.projection_lines.color = Some(REVEAL_COLOR);
                o.cut_lines.color = Some(REVEAL_COLOR);
            }
        } else if let Some(t) = temp {
            for c in &t.hidden_categories { set_category_visible(&mut s, c, false); }
            if let Some(iso) = &t.isolated_categories {
                for c in self.all_visibility_categories() { if !iso.contains(&c) { set_category_visible(&mut s, &c, false); } }
            }
            v.hidden_elements.extend(t.hidden.iter().copied());
            if let Some(iso) = &t.isolated {
                let others: Vec<Id> = self.elements.keys().copied().filter(|i| !iso.contains(i) && self.visibility_category(*i).is_some()).collect();
                v.hidden_elements.extend(others);
            }
        }
        // the template's settings are folded in above, so the drawn view stands on its own
        v.template = None;
        v.settings = s;
        Some(v)
    }

    /// Puts the view as the session shows it in place of the stored one, for drawing it, and returns the stored one
    /// for `display_end`. None when the view is drawn as saved. Only reads may happen in between: nothing is recorded.
    pub fn display_begin(&mut self, view: Id) -> Option<Element> {
        let shown = self.displayed_view(view)?;
        let slot = self.elements.get_mut(&view)?;
        Some(std::mem::replace(slot, Element::View(shown)))
    }

    /// Puts the stored view back after `display_begin`.
    pub fn display_end(&mut self, view: Id, saved: Element) {
        self.elements.insert(view, saved);
    }

    /// Runs `f` (a drawing of `view`) with the view as the session shows it, then puts the stored view back.
    pub fn with_display<R>(&mut self, view: Id, f: impl FnOnce(&Document) -> R) -> R {
        let Some(saved) = self.display_begin(view) else { return f(self) };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self)));
        self.display_end(view, saved);
        match r {
            Ok(r) => r,
            Err(e) => std::panic::resume_unwind(e),
        }
    }
}
