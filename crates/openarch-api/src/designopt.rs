//! Design options (private/docs/design-options.md): the Design Options dialog's calls, the option being edited,
//! Add to Set and Pick to Edit, and the Design Option rows of Properties for elements and views.
use serde_json::{json, Value};

use openarch_model::designopt::can_be_in_option;
use openarch_model::{Document, Id};

use crate::{id_of, ids_of, Res, Session};

/// The design options as the dialog and the status bar show them: sets with their options (and how many elements
/// each holds), the option being edited, and each element's option (for what may be picked).
pub(crate) fn design_options_json(d: &Document) -> Value {
    let Some((_, o)) = d.design_options() else {
        return json!({ "sets": [], "active": null, "member": {} });
    };
    let count = |opt: Id| o.member.iter().filter(|(e, x)| **x == opt && d.get(**e).is_some()).count();
    json!({
        "sets": o.sets.iter().map(|s| json!({
            "id": s.id, "name": s.name, "primary": s.primary,
            "options": s.options.iter().map(|x| json!({ "id": x.id, "name": x.name, "primary": x.id == s.primary, "count": count(x.id) })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "active": d.active_option(),
        "active_label": d.active_option().and_then(|a| o.label(a)),
        // while an option is edited, what cannot be picked: everything that could be in an option but is outside it
        "outside": if d.active_option().is_some() { d.iter().filter(|(id, _)| d.option_locked(*id)).map(|(id, _)| id).collect::<Vec<_>>() } else { vec![] },
        "member": o.member.iter().filter(|(e, x)| d.get(**e).is_some() && o.set_of(**x).is_some()).map(|(e, x)| (e.to_string(), json!(x))).collect::<serde_json::Map<_, _>>(),
    })
}

impl Session {
    pub(crate) fn handle_design_options(&mut self, method: &str, p: &Value) -> Option<Res> {
        let text = |k: &str| p.get(k).and_then(|v| v.as_str()).map(|s| s.trim().to_string());
        let opt_id = |k: &str| p.get(k).and_then(|v| v.as_u64());
        Some(match method {
            "design_options" => Ok(design_options_json(&self.doc)),
            // the Design Options dialog's edits, each one undo step; answers the design options as they are now
            "design_option_op" => (|| {
                match p.get("op").and_then(|v| v.as_str()).ok_or("missing op")? {
                    "add_set" => {
                        let name = text("name").filter(|n| !n.is_empty()).ok_or("an option set needs a name")?;
                        self.doc.add_option_set(&name)?;
                    }
                    "add_option" => {
                        self.doc.add_design_option(id_of(p, "set")?, text("name").as_deref())?;
                    }
                    "rename" => {
                        let name = text("name").filter(|n| !n.is_empty()).ok_or("a name is needed")?;
                        self.doc.rename_design_option(id_of(p, "id")?, &name)?;
                    }
                    "make_primary" => self.doc.make_primary(id_of(p, "id")?)?,
                    "delete_option" => self.doc.delete_design_option(id_of(p, "id")?)?,
                    "delete_set" => self.doc.delete_option_set(id_of(p, "id")?)?,
                    "accept_primary" => self.doc.accept_primary(id_of(p, "id")?)?,
                    other => return Err(format!("unknown design option operation '{other}'")),
                }
                Ok(design_options_json(&self.doc))
            })(),
            // the option being edited: { option: id or null for the main model }
            "set_active_option" => (|| {
                self.doc.set_active_option(opt_id("option"))?;
                Ok(design_options_json(&self.doc))
            })(),
            // Add to Set: { ids, options: [ids] } (the first option takes the elements, the others copies)
            "add_to_set" => (|| {
                let ids = ids_of(p, "ids")?;
                let options = ids_of(p, "options")?;
                let got = self.doc.add_to_set(&ids, &options)?;
                Ok(json!({ "ids": got }))
            })(),
            // Pick to Edit: edit the option an element is in
            "pick_to_edit" => (|| {
                let id = id_of(p, "id")?;
                let o = self.doc.option_of(id).ok_or("That element is in the main model, not a design option.")?;
                self.doc.set_active_option(Some(o))?;
                Ok(design_options_json(&self.doc))
            })(),
            // Properties' Design Option row: { ids or id, patch: { option: id or null } }
            "set_element_option" => (|| {
                let ids = ids_of(p, "ids").or_else(|_| id_of(p, "id").map(|i| vec![i]))?;
                let patch = p.get("patch").unwrap_or(p);
                self.doc.set_element_option(&ids, patch.get("option").and_then(|v| v.as_u64()))?;
                Ok(json!({}))
            })(),
            // a view's Design Options rows: { id, patch: { set, option: id or null for Automatic } }
            "set_view_option" => (|| {
                let id = id_of(p, "id")?;
                let patch = p.get("patch").unwrap_or(p);
                self.doc.set_view_option(id, id_of(patch, "set")?, patch.get("option").and_then(|v| v.as_u64()))?;
                Ok(json!({}))
            })(),
            _ => return None,
        })
    }

    /// The Design Option row of elements' Properties (one element or several), when the project has option sets.
    pub(crate) fn add_option_properties(&self, ids: &[Id], props: &mut Value) {
        let Some((_, o)) = self.doc.design_options() else { return };
        if o.sets.is_empty() || ids.is_empty() || !ids.iter().all(|id| self.doc.get(*id).is_some_and(can_be_in_option)) {
            return;
        }
        let of: Vec<Option<Id>> = ids.iter().map(|id| self.doc.option_of(*id)).collect();
        let varies = of.iter().any(|x| *x != of[0]);
        let label = |x: Option<Id>| x.and_then(|x| o.label(x)).unwrap_or_else(|| "Main Model".into());
        let row = json!({ "label": "Design Option", "value": if varies { "<varies>".to_string() } else { label(of[0]) }, "kind": "ro", "varies": varies });
        if let Some(groups) = props["groups"].as_array_mut() {
            groups.push(json!({ "name": "Design Options", "rows": [row] }));
        }
    }
}

/// A view's Design Options rows: one per option set, Automatic or one of its options.
pub(crate) fn view_option_group(d: &Document, id: Id, v: &openarch_model::view::View) -> Option<Value> {
    let (_, o) = d.design_options()?;
    if o.sets.is_empty() {
        return None;
    }
    let rows: Vec<Value> = o
        .sets
        .iter()
        .map(|s| {
            let chosen = v.design_options.get(&s.id).copied().filter(|x| s.has(*x));
            let shows = d.view_option(Some(v), s);
            let name = |x: Id| s.option(x).map_or(String::new(), |o| format!("{}{}", o.name, if x == s.primary { " (primary)" } else { "" }));
            let mut options = vec![json!({ "label": format!("<Automatic> ({})", name(shows)), "selected": chosen.is_none(), "patch": { "set": s.id, "option": null } })];
            options.extend(s.options.iter().map(|x| json!({ "label": name(x.id), "selected": chosen == Some(x.id), "patch": { "set": s.id, "option": x.id } })));
            json!({ "label": s.name, "value": chosen.map_or_else(|| format!("<Automatic> ({})", name(shows)), name), "kind": "select", "options": options })
        })
        .collect();
    Some(json!({ "name": "Design Options", "target": { "method": "set_view_option", "id": id }, "rows": rows }))
}
