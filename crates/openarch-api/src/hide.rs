//! Hiding in a view: Temporary Hide/Isolate, Hide in View (elements and categories), Reveal Hidden Elements and
//! Override Graphics in View by element. The temporary state and Reveal Hidden are folded into the view while it is
//! drawn (`Session::drawn`).
use serde_json::{json, Value};

use openarch_model::hide::TempOp;
use openarch_model::view::{CategoryOverride, ViewKind};
use openarch_model::Id;

use crate::{id_of, ids_of, Res, Session};

impl Session {
    /// The view a drawing method draws, when it can carry a temporary state.
    fn drawn_view(&self, method: &str, p: &Value) -> Option<Id> {
        match method {
            "plan" | "elevation" => p["view_id"].as_u64(),
            "scene" => p["view_id"].as_u64().or_else(|| self.doc.views().iter().find(|(_, v)| v.view_kind == ViewKind::ThreeD).map(|(id, _)| *id)),
            _ => None,
        }
    }

    /// Runs a drawing method with the view as the session shows it, and says what it shows differently.
    pub(crate) fn drawn(&mut self, method: &str, p: &Value, draw: impl FnOnce(&mut Session) -> Res) -> Res {
        let Some(view) = self.drawn_view(method, p) else { return draw(self) };
        let saved = self.doc.display_begin(view);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| draw(self)));
        if let Some(s) = saved { self.doc.display_end(view, s); }
        let mut r = match r { Ok(r) => r?, Err(e) => std::panic::resume_unwind(e) };
        if let Some(o) = r.as_object_mut() { o.insert("display".into(), self.display_state(view)); }
        Ok(r)
    }

    fn display_state(&self, view: Id) -> Value {
        let d = &self.doc;
        let t = d.temp_hide(view).filter(|t| !t.is_empty());
        let reveal = d.reveal_hidden(view);
        json!({
            "temporary": t.is_some(),
            "isolated": t.is_some_and(|t| t.isolated.is_some() || t.isolated_categories.is_some()),
            "reveal": reveal,
            "revealed": if reveal { d.revealed_elements(view).into_iter().collect::<Vec<_>>() } else { vec![] },
        })
    }

    pub(crate) fn handle_hide(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "display_state" => (|| { let view = id_of(p, "view_id")?; Ok(self.display_state(view)) })(),
            // { view_id, op: hide_elements | isolate_elements | hide_categories | isolate_categories | reset | apply, ids }
            "temp_hide" => (|| {
                let view = id_of(p, "view_id")?;
                let op = match p["op"].as_str().unwrap_or("") {
                    "hide_elements" => TempOp::HideElements,
                    "isolate_elements" => TempOp::IsolateElements,
                    "hide_categories" => TempOp::HideCategories,
                    "isolate_categories" => TempOp::IsolateCategories,
                    "reset" => { self.doc.temp_hide_reset(view); return Ok(self.display_state(view)); }
                    "apply" => { self.doc.temp_hide_keep(view)?; return Ok(self.display_state(view)); }
                    o => return Err(format!("unknown hide/isolate {o}")),
                };
                self.doc.temp_hide_apply(view, op, &ids_of(p, "ids")?)?;
                Ok(self.display_state(view))
            })(),
            // { view_id, on }
            "reveal_hidden" => (|| {
                let view = id_of(p, "view_id")?;
                self.doc.set_reveal_hidden(view, p["on"].as_bool().unwrap_or(true))?;
                Ok(self.display_state(view))
            })(),
            // Hide in View > Category, and Unhide Category: { view_id, ids, hide }
            "hide_categories" => (|| {
                let cats = self.doc.hide_categories_in_view(id_of(p, "view_id")?, &ids_of(p, "ids")?, p["hide"].as_bool().unwrap_or(true))?;
                Ok(json!({ "categories": cats }))
            })(),
            // Override Graphics in View > By Element: { view_id, ids } gives the first one's override
            "element_overrides" => (|| {
                let view = id_of(p, "view_id")?;
                let ids = ids_of(p, "ids")?;
                let v = self.doc.view(view).ok_or("Unknown view.")?;
                let o = ids.first().and_then(|i| v.element_overrides.get(i)).cloned();
                let cats: Vec<String> = ids.iter().filter_map(|i| self.doc.visibility_category(*i)).collect();
                Ok(json!({ "override": o.clone().unwrap_or_default(), "set": o.is_some(), "categories": cats }))
            })(),
            // { view_id, ids, override } (override null clears it)
            "set_element_overrides" => (|| {
                let over: Option<CategoryOverride> = if p["override"].is_null() { None } else { Some(serde_json::from_value(p["override"].clone()).map_err(|e| e.to_string())?) };
                self.doc.set_element_overrides(id_of(p, "view_id")?, &ids_of(p, "ids")?, over)?;
                Ok(json!({}))
            })(),
            _ => return None,
        })
    }
}
