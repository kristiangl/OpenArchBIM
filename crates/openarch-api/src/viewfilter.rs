//! View filters: making and editing them, applying them to a view or a view template, the parameters a filter's
//! categories have in common, and working out which elements each filter matches (the model keeps the result and
//! what draws a view reads it from there; see viewfilter.rs in the model).
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use openarch_model::schedule::FilterOp;
use openarch_model::view::{CategoryOverride, FilterApplication};
use openarch_model::viewfilter::{merged, FilterKind, ViewFilter};
use openarch_model::Id;

use crate::schedule::Params;
use crate::{id_of, Res, Session};

fn filter_of(p: &Value) -> Result<ViewFilter, String> {
    serde_json::from_value(p.get("filter").cloned().ok_or("missing filter")?).map_err(|e| format!("bad filter: {e}"))
}

fn list_of(p: &Value) -> Result<Vec<FilterApplication>, String> {
    serde_json::from_value(p.get("filters").cloned().ok_or("missing filters")?).map_err(|e| format!("bad filter list: {e}"))
}

/// One rule against an element's parameters: its own first, then its type's. Text compares without minding
/// capitals unless the rule says to.
fn rule_passes(p: &Params, field: &str, op: FilterOp, against: &str, match_case: bool) -> bool {
    let Some(cell) = p.get(field) else {
        // a parameter the element doesn't have: it has no value, and nothing else is true of it
        return matches!(op, FilterOp::HasNoValue | FilterOp::NotEqual | FilterOp::NotContains | FilterOp::NotBeginsWith | FilterOp::NotEndsWith | FilterOp::NotAssociatedWithGlobal);
    };
    if match_case && cell.number().is_none() {
        let (a, b) = (cell.text_of().trim(), against.trim());
        match op {
            FilterOp::Equal => return a == b,
            FilterOp::NotEqual => return a != b,
            FilterOp::Contains => return a.contains(b),
            FilterOp::NotContains => return !a.contains(b),
            FilterOp::BeginsWith => return a.starts_with(b),
            FilterOp::NotBeginsWith => return !a.starts_with(b),
            FilterOp::EndsWith => return a.ends_with(b),
            FilterOp::NotEndsWith => return !a.ends_with(b),
            _ => {}
        }
    }
    cell.passes(op, against)
}

impl Session {
    /// Works out what every view's filters do to its elements, when the document has changed since the last time.
    /// With no filters applied anywhere this is a look through the views and nothing more.
    pub(crate) fn prime_filters(&self) {
        let d = &self.doc;
        if d.filter_hits_current() { return; }
        let views = d.views_with_filters();
        let mut hits: BTreeMap<Id, BTreeMap<Id, CategoryOverride>> = BTreeMap::new();
        if !views.is_empty() {
            let wanted: BTreeSet<Id> = views.iter().flat_map(|(_, l)| l.iter().filter(|a| a.enabled).map(|a| a.filter)).collect();
            let mut by_category: BTreeMap<String, Vec<Id>> = BTreeMap::new();
            for (id, _) in d.iter() {
                if let Some(c) = d.visibility_category(id) { by_category.entry(c).or_default().push(id); }
            }
            let mut types = BTreeMap::new();
            let mut params: BTreeMap<Id, Params> = BTreeMap::new();
            let mut matches: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
            for fid in wanted {
                let Some(f) = d.view_filter(fid) else { continue };
                let set = match &f.kind {
                    FilterKind::Selection { ids } => ids.iter().copied().filter(|i| d.get(*i).is_some()).collect(),
                    FilterKind::Rules { categories, root } => {
                        let mut set = BTreeSet::new();
                        for id in categories.iter().filter_map(|c| by_category.get(c)).flatten() {
                            if !params.contains_key(id) { params.insert(*id, self.params(*id, &mut types)); }
                            let p = &params[id];
                            if root.passes(&|field, op, against, case| rule_passes(p, field, op, against, case)) { set.insert(*id); }
                        }
                        set
                    }
                };
                matches.insert(fid, set);
            }
            for (view, list) in &views {
                let mut per: BTreeMap<Id, Vec<&FilterApplication>> = BTreeMap::new();
                for a in list.iter().filter(|a| a.enabled) {
                    for id in matches.get(&a.filter).into_iter().flatten() { per.entry(*id).or_default().push(a); }
                }
                let over: BTreeMap<Id, CategoryOverride> = per.into_iter().filter_map(|(id, apps)| merged(&apps).map(|o| (id, o))).collect();
                if !over.is_empty() { hits.insert(*view, over); }
            }
        }
        d.set_filter_hits(hits);
    }

    /// The elements a filter matches now, for the Filters box ("12 elements").
    fn filter_matches(&self, f: &ViewFilter) -> Vec<Id> {
        let d = &self.doc;
        match &f.kind {
            FilterKind::Selection { ids } => ids.iter().copied().filter(|i| d.get(*i).is_some()).collect(),
            FilterKind::Rules { categories, root } => {
                let mut types = BTreeMap::new();
                d.iter().map(|(id, _)| id).filter(|id| d.visibility_category(*id).is_some_and(|c| categories.contains(&c))).filter(|id| {
                    let p = self.params(*id, &mut types);
                    root.passes(&|field, op, against, case| rule_passes(&p, field, op, against, case))
                }).collect()
            }
        }
    }

    /// The parameters every one of `categories` has (a rule can only test what all of them carry), with what each
    /// measures, and the tests a rule can make.
    fn filter_fields(&self, categories: &[String]) -> Value {
        let mut common: Option<Vec<Value>> = None;
        for c in categories {
            let fields: Vec<Value> = self.schedule_fields(c, false).as_array().cloned().unwrap_or_default();
            common = Some(match common {
                None => fields,
                Some(have) => have.into_iter().filter(|f| fields.iter().any(|g| g["name"] == f["name"])).collect(),
            });
        }
        let ops: Vec<Value> = FilterOp::ALL.iter().filter(|o| !matches!(o, FilterOp::HasParameter | FilterOp::AssociatedWithGlobal | FilterOp::NotAssociatedWithGlobal)).map(|o| json!({ "op": o, "label": o.label() })).collect();
        json!({ "fields": common.unwrap_or_default(), "ops": ops })
    }

    pub(crate) fn handle_viewfilter(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // Every filter of the project: { filters: [{ id, name, kind, categories?, root?, ids?, used_in }] }
            "view_filters" => (|| {
                let d = &self.doc;
                let used = d.views_with_filters();
                Ok(json!({ "filters": d.view_filters().into_iter().map(|(id, f)| {
                    let mut v = serde_json::to_value(f).unwrap_or_default();
                    v["id"] = json!(id);
                    v["used_in"] = json!(used.iter().filter(|(_, l)| l.iter().any(|a| a.filter == id)).count());
                    v
                }).collect::<Vec<_>>(), "categories": crate::schedule::model_categories() }))
            })(),
            // { filter: { name, kind: "rules", categories, root } | { name, kind: "selection", ids } }
            "add_view_filter" => (|| Ok(json!({ "id": self.doc.add_view_filter(filter_of(p)?)? })))(),
            "set_view_filter" => (|| { self.doc.set_view_filter(id_of(p, "id")?, filter_of(p)?)?; Ok(json!({})) })(),
            "delete_view_filter" => (|| { self.doc.delete_view_filter(id_of(p, "id")?)?; Ok(json!({})) })(),
            // How many elements a filter (saved, by id, or as given) matches now
            "view_filter_matches" => (|| {
                let f = match p.get("id").and_then(|v| v.as_u64()) {
                    Some(id) => self.doc.view_filter(id).cloned().ok_or("not a filter")?,
                    None => filter_of(p)?,
                };
                let ids = self.filter_matches(&f);
                Ok(json!({ "count": ids.len(), "ids": ids }))
            })(),
            // The parameters a rule can test for these categories: { categories }
            "filter_fields" => (|| {
                let cats: Vec<String> = serde_json::from_value(p["categories"].clone()).map_err(|e| format!("bad categories: {e}"))?;
                Ok(self.filter_fields(&cats))
            })(),
            // The filters a view applies, as they take effect: { view_id } -> { filters, locked (its template's) }
            "view_filter_list" => (|| {
                let view = id_of(p, "view_id")?;
                let d = &self.doc;
                let v = d.view(view).ok_or("Unknown view.")?;
                let locked = v.template.and_then(|t| d.view_template(t)).is_some_and(|t| t.included.contains(&openarch_model::view::TemplateProp::VgFilters));
                let list = d.effective_settings(view).map(|s| s.overrides.filters).unwrap_or_default();
                Ok(json!({ "locked": locked, "filters": list.iter().map(|a| {
                    let mut x = serde_json::to_value(a).unwrap_or_default();
                    x["name"] = json!(d.view_filter(a.filter).map_or("", |f| f.name.as_str()));
                    x
                }).collect::<Vec<_>>() }))
            })(),
            // The whole ordered list for a view or a view template: { view_id | template_id, filters: [{ filter, visible, enabled, overrides }] }
            "set_view_filter_list" => (|| {
                let list = list_of(p)?;
                match p.get("template_id").and_then(|v| v.as_u64()) {
                    Some(t) => self.doc.set_template_filter_list(t, list)?,
                    None => self.doc.set_view_filter_list(id_of(p, "view_id")?, list)?,
                }
                Ok(json!({}))
            })(),
            _ => return None,
        })
    }
}
