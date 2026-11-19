//! Phasing (private/docs/phasing.md): the Phases dialog's calls, element and view phases, Demolish, the active
//! view's phase for new elements, and the Phasing rows of Properties.
use serde_json::{json, Value};

use openarch_model::element::Element;
use openarch_model::phasing::{is_phased, PhaseFilter, PhaseStatus, Phasing};
use openarch_model::view::CategoryOverride;
use openarch_model::{Document, Id};

use crate::{id_of, ids_of, Res, Session};

/// The phasing as the Phases dialog shows it, with how many elements each phase builds and takes down.
fn phasing_json(d: &Document) -> Value {
    let Some((_, ph)) = d.phasing() else { return json!(null) };
    let count = |m: &std::collections::BTreeMap<Id, Id>, p: Id| m.iter().filter(|(e, v)| **v == p && d.get(**e).is_some()).count();
    let built_default = d.iter().filter(|(id, e)| is_phased(e) && !ph.created.contains_key(id)).count();
    json!({
        "phases": ph.phases.iter().map(|p| json!({
            "id": p.id, "name": p.name, "description": p.description,
            "created": if p.id == ph.default_phase { built_default } else { count(&ph.created, p.id) },
            "demolished": count(&ph.demolished, p.id),
            "views": d.views().iter().filter(|(_, v)| v.phase.unwrap_or(ph.default_phase) == p.id && v.schedule.is_none()).count(),
        })).collect::<Vec<_>>(),
        "filters": ph.filters,
        "overrides": PhaseStatus::ALL.iter().map(|s| json!({ "status": s, "label": s.label(), "override": ph.overrides.get(*s) })).collect::<Vec<_>>(),
        "default_phase": ph.default_phase,
    })
}

impl Session {
    pub(crate) fn handle_phasing(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // the phasing, made with its starting phases and filters the first time
            // { peek: true } answers null for a project without phasing instead of giving it phasing
            "phasing" => (|| {
                if p.get("peek").and_then(|v| v.as_bool()) == Some(true) {
                    return Ok(phasing_json(&self.doc));
                }
                self.doc.ensure_phasing()?;
                Ok(phasing_json(&self.doc))
            })(),
            // the Phases dialog's edits, each one undo step; answers the phasing as it is now
            "phase_op" => (|| {
                let text = |k: &str| p.get(k).and_then(|v| v.as_str()).map(|s| s.trim().to_string());
                match p.get("op").and_then(|v| v.as_str()).ok_or("missing op")? {
                    "insert" => {
                        let name = text("name").filter(|n| !n.is_empty()).ok_or("a phase needs a name")?;
                        self.doc.insert_phase(p.get("after").and_then(|v| v.as_u64()), &name)?;
                    }
                    "rename" => {
                        let id = id_of(p, "id")?;
                        let (name, description) = (text("name"), text("description"));
                        self.doc.edit_phasing("Rename phase", |ph| {
                            let at = ph.index(id).ok_or("no such phase")?;
                            if let Some(n) = name {
                                ph.phases[at].name = n;
                            }
                            if let Some(t) = description {
                                ph.phases[at].description = t;
                            }
                            Ok(())
                        })?;
                    }
                    "combine" => self.doc.combine_phase(id_of(p, "id")?, id_of(p, "into")?)?,
                    "set_filter" => {
                        let f: PhaseFilter = serde_json::from_value(p.get("filter").cloned().ok_or("missing filter")?).map_err(|e| format!("bad phase filter: {e}"))?;
                        self.doc.edit_phasing("Phase filter", |ph| {
                            let at = ph.filters.iter().position(|x| x.id == f.id).ok_or("no such phase filter")?;
                            ph.filters[at] = f;
                            Ok(())
                        })?;
                    }
                    "add_filter" => {
                        let name = text("name").filter(|n| !n.is_empty()).ok_or("a phase filter needs a name")?;
                        self.doc.edit_phasing("New phase filter", |ph| {
                            let id = ph.next_id;
                            ph.next_id += 1;
                            ph.filters.push(PhaseFilter { id, name, new: Default::default(), existing: Default::default(), demolished: Default::default(), temporary: Default::default() });
                            Ok(())
                        })?;
                    }
                    "delete_filter" => {
                        let id = id_of(p, "id")?;
                        let used = self.doc.views().iter().any(|(_, v)| v.settings.phase_filter == Some(id)) || self.doc.view_templates().iter().any(|(_, t)| t.settings.phase_filter == Some(id));
                        if used {
                            return Err("a view or template uses that phase filter".into());
                        }
                        self.doc.edit_phasing("Delete phase filter", |ph| {
                            ph.filters.retain(|f| f.id != id);
                            if ph.filters.is_empty() {
                                return Err("a project needs at least one phase filter".into());
                            }
                            Ok(())
                        })?;
                    }
                    "set_override" => {
                        let s: PhaseStatus = serde_json::from_value(p.get("status").cloned().ok_or("missing status")?).map_err(|_| "unknown status".to_string())?;
                        let o: CategoryOverride = serde_json::from_value(p.get("override").cloned().ok_or("missing override")?).map_err(|e| format!("bad override: {e}"))?;
                        self.doc.edit_phasing("Phase graphic overrides", |ph| {
                            *ph.overrides.get_mut(s) = CategoryOverride { visible: true, ..o };
                            Ok(())
                        })?;
                    }
                    other => return Err(format!("unknown phase operation '{other}'")),
                }
                Ok(phasing_json(&self.doc))
            })(),
            // Properties' Phasing rows: { ids or id, patch: { created?: phase, demolished?: phase or null } }
            "set_element_phases" => (|| {
                let ids = ids_of(p, "ids").or_else(|_| id_of(p, "id").map(|i| vec![i]))?;
                let patch = p.get("patch").unwrap_or(p);
                let created = patch.get("created").and_then(|v| v.as_u64());
                let demolished = patch.get("demolished").map(|v| v.as_u64());
                self.doc.set_element_phases(&ids, created, demolished)?;
                Ok(json!({}))
            })(),
            // a view's Phase and Phase Filter: { id, patch: { phase?: id or null, phase_filter?: id or null } }
            "set_view_phase" => (|| {
                let id = id_of(p, "id")?;
                let patch = p.get("patch").unwrap_or(p);
                if let Some(v) = patch.get("phase") {
                    self.doc.set_view_phase(id, v.as_u64())?;
                }
                if let Some(v) = patch.get("phase_filter") {
                    self.doc.set_view_phase_filter(id, v.as_u64())?;
                }
                Ok(json!({}))
            })(),
            // the view the person works in: new physical elements are built in its phase
            "active_view" => (|| {
                let view = p.get("view_id").and_then(|v| v.as_u64());
                let phase = view.and_then(|v| self.doc.view(v)).and_then(|v| self.doc.view_phase(v)).map(|(ph, _)| ph);
                self.doc.set_creation_phase(phase);
                Ok(json!({ "phase": phase }))
            })(),
            // the Demolish tool: { view_id, ids }
            "demolish" => (|| {
                let view = id_of(p, "view_id")?;
                let ids = ids_of(p, "ids")?;
                self.doc.demolish_in_view(view, &ids)?;
                // the active view's phase may be new to the project's phasing
                let phase = self.doc.view(view).and_then(|v| self.doc.view_phase(v)).map(|(ph, _)| ph);
                self.doc.set_creation_phase(phase);
                Ok(json!({}))
            })(),
            _ => return None,
        })
    }

    /// The Phasing rows of model elements' Properties (one element or several).
    pub(crate) fn add_phase_properties(&self, ids: &[Id], props: &mut Value) {
        if ids.is_empty() || !ids.iter().all(|id| self.doc.get(*id).is_some_and(is_phased)) {
            return;
        }
        let fallback;
        let ph = match self.doc.phasing() {
            Some((_, ph)) => ph,
            None => {
                fallback = Phasing::default();
                &fallback
            }
        };
        let created: Vec<Id> = ids.iter().map(|id| ph.created_of(*id)).collect();
        let demolished: Vec<Option<Id>> = ids.iter().map(|id| ph.demolished.get(id).copied()).collect();
        let name = |p: Id| ph.phase(p).map_or("?".to_string(), |x| x.name.clone());
        let c0 = created[0];
        let d0 = demolished[0];
        let c_varies = created.iter().any(|c| *c != c0);
        let d_varies = demolished.iter().any(|d| *d != d0);
        let pinned = ids.iter().any(|id| match self.doc.get(*id) {
            Some(Element::Wall(w)) => w.pinned,
            Some(Element::FamilyInstance(i)) => i.pinned,
            _ => false,
        });
        let created_row = json!({
            "label": "Phase Created", "value": if c_varies { "<varies>".to_string() } else { name(c0) }, "kind": if pinned { "ro" } else { "select" }, "varies": c_varies,
            "options": ph.phases.iter().map(|p| json!({ "label": p.name, "selected": !c_varies && p.id == c0, "patch": { "created": p.id } })).collect::<Vec<_>>(),
        });
        let mut demo_options = vec![json!({ "label": "None", "selected": !d_varies && d0.is_none(), "patch": { "demolished": null } })];
        demo_options.extend(ph.phases.iter().map(|p| json!({ "label": p.name, "selected": !d_varies && d0 == Some(p.id), "patch": { "demolished": p.id } })));
        let demolished_row = json!({
            "label": "Phase Demolished", "value": if d_varies { "<varies>".to_string() } else { d0.map_or("None".to_string(), name) }, "kind": if pinned { "ro" } else { "select" }, "varies": d_varies,
            "options": demo_options,
        });
        if let Some(groups) = props["groups"].as_array_mut() {
            groups.push(json!({ "name": "Phasing", "target": { "method": "set_element_phases", "id": ids[0], "extra": { "ids": ids } }, "rows": [created_row, demolished_row] }));
        }
    }
}

/// A view's Phasing rows: Phase Filter (read-only while its template sets it) and Phase.
pub(crate) fn view_phase_group(d: &Document, id: Id, v: &openarch_model::view::View) -> Value {
    let fallback;
    let ph = match d.phasing() {
        Some((_, ph)) => ph,
        None => {
            fallback = Phasing::default();
            &fallback
        }
    };
    let phase = v.phase.filter(|p| ph.index(*p).is_some()).unwrap_or(ph.default_phase);
    let locked = v.template.and_then(|t| d.view_template(t)).is_some_and(|t| t.included.contains(&openarch_model::view::TemplateProp::PhaseFilter));
    let filter = match v.template.and_then(|t| d.view_template(t)) {
        Some(t) if locked => t.settings.phase_filter,
        _ => v.settings.phase_filter,
    }
    .and_then(|f| ph.filter(f))
    .or_else(|| ph.default_filter())
    .map(|f| f.id);
    let fname = filter.and_then(|f| ph.filter(f)).map_or("Show All".to_string(), |f| f.name.clone());
    let filter_row = if locked {
        json!({ "label": "Phase Filter", "value": fname, "kind": "ro", "locked": true, "lock_reason": "Set by the view's template" })
    } else {
        json!({ "label": "Phase Filter", "value": fname, "kind": "select", "options": ph.filters.iter().map(|f| json!({ "label": f.name, "selected": Some(f.id) == filter, "patch": { "phase_filter": f.id } })).collect::<Vec<_>>() })
    };
    let phase_row = json!({ "label": "Phase", "value": ph.phase(phase).map_or(String::new(), |p| p.name.clone()), "kind": "select",
        "options": ph.phases.iter().map(|p| json!({ "label": p.name, "selected": p.id == phase, "patch": { "phase": p.id } })).collect::<Vec<_>>() });
    json!({ "name": "Phasing", "target": { "method": "set_view_phase", "id": id }, "rows": [filter_row, phase_row] })
}
