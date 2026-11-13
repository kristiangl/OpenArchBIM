//! View filters: named tests that pick elements out of a view so it can hide them or draw them differently.
//!
//! A filter is a project element. A rule-based one lists the categories it looks at and a tree of AND and OR groups
//! over parameter rules ("Fire Rating is greater than or equal to 2", "Comments contains Shaft"); a selection one is
//! a fixed set of elements. A view (or its template, when that controls Filters) applies filters in an ordered
//! list, each with its own graphic overrides, a Visibility switch (off hides what matches) and an Enable switch (off
//! leaves the filter in the list but skips it).
//!
//! Where several filters match one element, each graphic property comes from the filter highest in the list that
//! sets it, so red cut lines from the first and a grey cut fill from the second both show. An override set on the
//! element itself in the view (Override Graphics in View) wins over every filter, property by property.
//!
//! Reading an element's parameters is the palette's job (the API crate), so which elements a filter matches is
//! worked out there and handed to the document (`set_filter_hits`), valid until the next change. What draws a view
//! asks `graphics_of` for an element's overrides and `filter_hides` for whether a filter hides it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::schedule::FilterOp;
use crate::view::{CategoryOverride, FillOverride, FilterApplication, LineOverride, View};
use crate::{Document, Element, Id};

/// How deep groups may sit inside groups, and how many rules a filter may hold.
pub const MAX_DEPTH: usize = 6;
pub const MAX_RULES: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum FilterNode {
    /// Every one of these passes (an empty group passes).
    And { of: Vec<FilterNode> },
    /// At least one of these passes (an empty group passes nothing).
    Or { of: Vec<FilterNode> },
    /// One parameter test. `not` turns the result round; `match_case` makes text comparisons mind capitals.
    Rule {
        field: String,
        op: FilterOp,
        #[serde(default)]
        value: String,
        #[serde(default)]
        not: bool,
        #[serde(default)]
        match_case: bool,
    },
}

impl FilterNode {
    fn count(&self, depth: usize) -> Result<usize, String> {
        if depth > MAX_DEPTH { return Err(format!("Groups can sit {MAX_DEPTH} deep at most.")); }
        match self {
            FilterNode::And { of } | FilterNode::Or { of } => {
                let mut n = 0;
                for c in of { n += c.count(depth + 1)?; }
                Ok(n)
            }
            FilterNode::Rule { field, .. } => {
                if field.trim().is_empty() { return Err("A rule needs a parameter.".into()); }
                Ok(1)
            }
        }
    }

    /// Whether an element passes, given a way to test one rule.
    pub fn passes(&self, rule: &dyn Fn(&str, FilterOp, &str, bool) -> bool) -> bool {
        match self {
            FilterNode::And { of } => of.iter().all(|c| c.passes(rule)),
            FilterNode::Or { of } => of.iter().any(|c| c.passes(rule)),
            FilterNode::Rule { field, op, value, not, match_case } => rule(field, *op, value, *match_case) != *not,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterKind {
    /// The elements of these categories that pass the rules. No rules at all passes every element of them.
    Rules { categories: Vec<String>, root: FilterNode },
    /// These elements, whatever they are.
    Selection { ids: BTreeSet<Id> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewFilter {
    pub name: String,
    #[serde(flatten)]
    pub kind: FilterKind,
}

/// What the filters of each view do to its elements, as last worked out (see the module notes).
#[derive(Debug, Default)]
pub struct FilterCache {
    /// The document revision this was worked out at; anything else is stale.
    revision: Option<u64>,
    by_view: BTreeMap<Id, BTreeMap<Id, CategoryOverride>>,
    /// The same, found from a `&View` in hand (its address while the document is unchanged).
    by_address: HashMap<usize, Id>,
}

/// A copy of the document starts with nothing worked out: its views sit at other addresses.
impl Clone for FilterCache {
    fn clone(&self) -> Self {
        FilterCache::default()
    }
}

fn line_over(top: &LineOverride, under: &LineOverride) -> LineOverride {
    LineOverride { pattern: top.pattern.clone().or_else(|| under.pattern.clone()), color: top.color.or(under.color), weight: top.weight.or(under.weight) }
}

fn fill_over(top: &FillOverride, under: &FillOverride) -> FillOverride {
    FillOverride { visible: top.visible && under.visible, color: top.color.or(under.color), pattern: top.pattern.clone().or_else(|| under.pattern.clone()) }
}

impl CategoryOverride {
    /// These overrides laid over others: each property from here where it is set, else from `under`. Either one
    /// hiding the element hides it; halftone from either; subcategories are left to the category itself.
    pub fn over(&self, under: &CategoryOverride) -> CategoryOverride {
        CategoryOverride {
            visible: self.visible && under.visible,
            halftone: self.halftone || under.halftone,
            projection_lines: line_over(&self.projection_lines, &under.projection_lines),
            surface_foreground: fill_over(&self.surface_foreground, &under.surface_foreground),
            surface_background: fill_over(&self.surface_background, &under.surface_background),
            transparency: if self.transparency > 0 { self.transparency } else { under.transparency },
            cut_lines: line_over(&self.cut_lines, &under.cut_lines),
            cut_foreground: fill_over(&self.cut_foreground, &under.cut_foreground),
            cut_background: fill_over(&self.cut_background, &under.cut_background),
            detail_level: self.detail_level.or(under.detail_level),
            subcategories: BTreeMap::new(),
        }
    }
}

impl Document {
    pub fn view_filters(&self) -> Vec<(Id, &ViewFilter)> {
        self.iter().filter_map(|(id, e)| match e { Element::ViewFilter(f) => Some((id, f)), _ => None }).collect()
    }

    pub fn view_filter(&self, id: Id) -> Option<&ViewFilter> {
        match self.get(id) { Some(Element::ViewFilter(f)) => Some(f), _ => None }
    }

    fn check_view_filter(&self, f: &ViewFilter, own: Option<Id>) -> Result<(), String> {
        let name = f.name.trim();
        if name.is_empty() { return Err("A filter needs a name.".into()); }
        if self.view_filters().iter().any(|(i, o)| Some(*i) != own && o.name.trim().eq_ignore_ascii_case(name)) {
            return Err(format!("There is already a filter called {name}."));
        }
        match &f.kind {
            FilterKind::Rules { categories, root } => {
                if categories.is_empty() { return Err("Tick at least one category for the filter to look at.".into()); }
                let known = |c: &String| crate::view::MODEL_CATEGORIES.contains(&c.as_str()) || crate::view::ANNOTATION_CATEGORIES.contains(&c.as_str());
                if let Some(c) = categories.iter().find(|c| !known(c)) { return Err(format!("{c} is not a category.")); }
                if root.count(0)? > MAX_RULES { return Err(format!("A filter can hold {MAX_RULES} rules at most.")); }
            }
            FilterKind::Selection { ids } => {
                if ids.is_empty() { return Err("Select the elements for the filter first.".into()); }
                if let Some(i) = ids.iter().find(|i| self.visibility_category(**i).is_none()) { return Err(format!("Element {i} is not something a view draws.")); }
            }
        }
        Ok(())
    }

    pub fn add_view_filter(&mut self, f: ViewFilter) -> Result<Id, String> {
        self.check_view_filter(&f, None)?;
        self.transact("Create filter", |tx| Ok(tx.add(Element::ViewFilter(f))))
    }

    /// A filter replaced whole (its name, categories and rules, or its elements).
    pub fn set_view_filter(&mut self, id: Id, f: ViewFilter) -> Result<(), String> {
        let old = self.view_filter(id).ok_or("not a filter")?;
        if *old == f { return Ok(()); }
        self.check_view_filter(&f, Some(id))?;
        self.transact("Edit filter", |tx| tx.update(id, Element::ViewFilter(f)))
    }

    /// A filter deleted, and taken out of every view and template that applied it. One undo step.
    pub fn delete_view_filter(&mut self, id: Id) -> Result<(), String> {
        if self.view_filter(id).is_none() { return Err("not a filter".into()); }
        let mut updates: Vec<(Id, Element)> = vec![];
        for (i, e) in self.iter() {
            match e {
                Element::View(v) if v.settings.overrides.filters.iter().any(|a| a.filter == id) => {
                    let mut v = v.clone();
                    v.settings.overrides.filters.retain(|a| a.filter != id);
                    updates.push((i, Element::View(v)));
                }
                Element::ViewTemplate(t) if t.settings.overrides.filters.iter().any(|a| a.filter == id) => {
                    let mut t = t.clone();
                    t.settings.overrides.filters.retain(|a| a.filter != id);
                    updates.push((i, Element::ViewTemplate(t)));
                }
                _ => {}
            }
        }
        self.transact("Delete filter", |tx| {
            for (i, e) in updates { tx.update(i, e)?; }
            tx.remove(id)
        })
    }

    /// The filters a view applies, in order (the whole list). Refused while the view's template controls Filters.
    pub fn set_view_filter_list(&mut self, view: Id, list: Vec<FilterApplication>) -> Result<(), String> {
        let v = self.view(view).ok_or("Unknown view.")?;
        if v.template.and_then(|t| self.view_template(t)).is_some_and(|t| t.included.contains(&crate::view::TemplateProp::VgFilters)) {
            return Err("The filters of this view are controlled by its view template.".into());
        }
        self.check_filter_list(&list)?;
        if v.settings.overrides.filters == list { return Ok(()); }
        let mut v = v.clone();
        v.settings.overrides.filters = list;
        self.transact("Filters in view", |tx| tx.update(view, Element::View(v)))
    }

    /// The same for a view template.
    pub fn set_template_filter_list(&mut self, template: Id, list: Vec<FilterApplication>) -> Result<(), String> {
        let t = self.view_template(template).ok_or("not a view template")?;
        self.check_filter_list(&list)?;
        if t.settings.overrides.filters == list { return Ok(()); }
        let mut t = t.clone();
        t.settings.overrides.filters = list;
        self.transact("Filters in view template", |tx| tx.update(template, Element::ViewTemplate(t)))
    }

    fn check_filter_list(&self, list: &[FilterApplication]) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for a in list {
            if self.view_filter(a.filter).is_none() { return Err("That filter no longer exists.".into()); }
            if !seen.insert(a.filter) { return Err("A filter can be in a view's list once.".into()); }
            if a.overrides.transparency > 100 { return Err("Transparency is from 0 to 100.".into()); }
        }
        Ok(())
    }

    /// The views and templates that apply filters, each with its list as it takes effect (a view whose template
    /// controls Filters takes the template's): what `set_filter_hits` wants worked out.
    pub fn views_with_filters(&self) -> Vec<(Id, Vec<FilterApplication>)> {
        self.iter().filter_map(|(id, e)| match e {
            Element::View(_) => {
                let list = self.effective_settings(id)?.overrides.filters;
                (!list.is_empty()).then_some((id, list))
            }
            _ => None,
        }).collect()
    }

    /// Whether the filter hits were worked out for the document as it is now.
    pub fn filter_hits_current(&self) -> bool {
        self.filter_cache.borrow().revision == Some(self.revision())
    }

    /// What each view's filters do to its elements, for the document as it is now: for each view, the overrides of
    /// every element a filter matches (already laid over one another by the order of the view's list).
    pub fn set_filter_hits(&self, hits: BTreeMap<Id, BTreeMap<Id, CategoryOverride>>) {
        let by_address = hits.keys().filter_map(|id| self.view(*id).map(|v| (v as *const View as usize, *id))).collect();
        *self.filter_cache.borrow_mut() = FilterCache { revision: Some(self.revision()), by_view: hits, by_address };
    }

    fn filter_override(&self, view: &View, id: Id) -> Option<CategoryOverride> {
        let c = self.filter_cache.borrow();
        if c.revision != Some(self.revision()) { return None; }
        let vid = c.by_address.get(&(view as *const View as usize))?;
        c.by_view.get(vid)?.get(&id).cloned()
    }

    /// The graphic overrides of an element in a view: its own (Override Graphics in View) over those of the
    /// view's filters that match it.
    pub fn graphics_of(&self, view: &View, id: &Id) -> Option<CategoryOverride> {
        let own = view.element_overrides.get(id);
        match (own, self.filter_override(view, *id)) {
            (Some(e), Some(f)) => Some(e.over(&f)),
            (Some(e), None) => Some(e.clone()),
            (None, f) => f,
        }
    }

    /// Whether a filter of the view hides the element.
    pub fn filter_hides(&self, view: &View, id: Id) -> bool {
        self.filter_override(view, id).is_some_and(|o| !o.visible)
    }

    /// The elements a view's filters hide.
    pub fn filter_hidden(&self, view: Id) -> BTreeSet<Id> {
        let c = self.filter_cache.borrow();
        if c.revision != Some(self.revision()) { return BTreeSet::new(); }
        c.by_view.get(&view).map(|m| m.iter().filter(|(_, o)| !o.visible).map(|(i, _)| *i).collect()).unwrap_or_default()
    }
}

/// The overrides of the filters that match one element, laid over one another: `matched` are the applications in
/// the view's order (top first), already narrowed to enabled filters that match.
pub fn merged(matched: &[&FilterApplication]) -> Option<CategoryOverride> {
    let mut it = matched.iter();
    let first = it.next()?;
    let mut acc = CategoryOverride { visible: first.visible, ..first.overrides.clone() };
    for a in it {
        acc = acc.over(&CategoryOverride { visible: a.visible, ..a.overrides.clone() });
    }
    Some(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::ViewKind;

    fn rule(field: &str, op: FilterOp, value: &str) -> FilterNode {
        FilterNode::Rule { field: field.into(), op, value: value.into(), not: false, match_case: false }
    }

    #[test]
    fn groups_pass_as_and_and_or_and_not() {
        // (Fire Rating >= 2 AND Function = Interior) OR (Type contains Core AND Comments = Shaft)
        let tree = FilterNode::Or { of: vec![
            FilterNode::And { of: vec![rule("Fire Rating", FilterOp::GreaterOrEqual, "2"), rule("Function", FilterOp::Equal, "Interior")] },
            FilterNode::And { of: vec![rule("Type", FilterOp::Contains, "Core"), rule("Comments", FilterOp::Equal, "Shaft")] },
        ] };
        let wall = |fire: &'static str, function: &'static str, ty: &'static str, comments: &'static str| move |f: &str, op: FilterOp, against: &str, _: bool| {
            let v = match f { "Fire Rating" => fire, "Function" => function, "Type" => ty, _ => comments };
            op.test(v, v.parse().ok(), against)
        };
        assert!(tree.passes(&wall("2", "Interior", "Stud 92", "")));
        assert!(!tree.passes(&wall("1", "Interior", "Stud 92", "")));
        assert!(tree.passes(&wall("0", "Exterior", "Core Wall 200", "shaft")));
        assert!(!tree.passes(&wall("0", "Exterior", "Core Wall 200", "Riser")));
        let not = FilterNode::Rule { field: "Comments".into(), op: FilterOp::Equal, value: "Shaft".into(), not: true, match_case: false };
        assert!(not.passes(&wall("0", "", "", "Riser")) && !not.passes(&wall("0", "", "", "Shaft")));
        assert!(FilterNode::And { of: vec![] }.passes(&wall("", "", "", "")) && !FilterNode::Or { of: vec![] }.passes(&wall("", "", "", "")));
    }

    #[test]
    fn the_higher_filter_wins_each_property_and_the_rest_add_up() {
        let red = [255, 0, 0];
        let blue = [0, 0, 255];
        let a = FilterApplication { filter: 1, visible: true, enabled: true, overrides: CategoryOverride { cut_lines: LineOverride { color: Some(red), weight: Some(5), pattern: None }, ..Default::default() } };
        let b = FilterApplication { filter: 2, visible: true, enabled: true, overrides: CategoryOverride {
            cut_lines: LineOverride { color: Some(blue), weight: Some(3), pattern: None },
            cut_foreground: FillOverride { visible: true, color: Some([128, 128, 128]), pattern: Some("Solid fill".into()) },
            transparency: 40, ..Default::default() } };
        let m = merged(&[&a, &b]).unwrap();
        assert_eq!((m.cut_lines.color, m.cut_lines.weight), (Some(red), Some(5)));
        assert_eq!((m.cut_foreground.pattern.as_deref(), m.transparency, m.visible), (Some("Solid fill"), 40, true));
        // a filter with Visibility off hides what it matches, wherever it is in the list
        let hide = FilterApplication { filter: 3, visible: false, enabled: true, overrides: Default::default() };
        assert!(!merged(&[&a, &hide]).unwrap().visible);
    }

    #[test]
    fn filters_are_made_applied_to_views_and_deleted_cleanly() {
        let mut d = Document::new_project();
        let view = d.iter().find_map(|(id, e)| match e { Element::View(v) if v.view_kind == ViewKind::FloorPlan => Some(id), _ => None }).unwrap();
        let f = ViewFilter { name: "Fire walls".into(), kind: FilterKind::Rules { categories: vec!["Walls".into()], root: FilterNode::And { of: vec![rule("Fire Rating", FilterOp::HasValue, "")] } } };
        let id = d.add_view_filter(f.clone()).unwrap();
        assert!(d.add_view_filter(f.clone()).is_err(), "names are unique");
        assert!(d.add_view_filter(ViewFilter { name: "x".into(), kind: FilterKind::Rules { categories: vec!["Nonsense".into()], root: FilterNode::And { of: vec![] } } }).is_err());
        assert!(d.add_view_filter(ViewFilter { name: "y".into(), kind: FilterKind::Selection { ids: BTreeSet::new() } }).is_err());

        let app = FilterApplication { filter: id, visible: true, enabled: true, overrides: Default::default() };
        d.set_view_filter_list(view, vec![app.clone()]).unwrap();
        assert!(d.set_view_filter_list(view, vec![app.clone(), app.clone()]).is_err());
        assert_eq!(d.views_with_filters(), vec![(view, vec![app.clone()])]);

        // hits are good only for the document they were worked out for
        let wall = 999;
        d.set_filter_hits(BTreeMap::from([(view, BTreeMap::from([(wall, CategoryOverride { visible: false, ..Default::default() })]))]));
        assert!(d.filter_hits_current() && d.filter_hides(d.view(view).unwrap(), wall) && d.filter_hidden(view).contains(&wall));
        d.delete_view_filter(id).unwrap();
        assert!(!d.filter_hits_current() && !d.filter_hides(d.view(view).unwrap(), wall));
        assert!(d.view(view).unwrap().settings.overrides.filters.is_empty() && d.view_filters().is_empty());
        d.undo().unwrap();
        assert_eq!((d.view_filters().len(), d.view(view).unwrap().settings.overrides.filters.len()), (1, 1));
    }
}
