//! View templates beyond the basics in view.rs and doc.rs: duplicating one, applying one to a view once (its
//! values stamped on, no link kept), bringing one up to date from a view, and Temporary View Properties.
//!
//! Temporary View Properties puts a template over a view for now, to look at the model another way (a checking
//! template with coloured filters, say) without changing the view as it is saved or as it prints. It is kept beside
//! the document, not in it: nothing is written to the file and there is nothing to undo. `effective_settings`
//! lays the temporary template's included properties over everything else.

use crate::view::{TemplateProp, ViewTemplate};
use crate::{Document, Element, Id};

impl Document {
    /// A copy of a template under a new name ("Working Plan Copy 1" when none is given).
    pub fn duplicate_view_template(&mut self, id: Id, name: Option<&str>) -> Result<Id, String> {
        let t = self.view_template(id).cloned().ok_or("not a view template")?;
        let taken = |n: &str| self.view_templates().iter().any(|(_, o)| o.name == n);
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) if taken(n) => return Err(format!("a view template named '{n}' already exists")),
            Some(n) => n.to_string(),
            None => (1..).map(|k| format!("{} Copy {k}", t.name)).find(|n| !taken(n)).unwrap_or_default(),
        };
        self.transact("Duplicate view template", |tx| Ok(tx.add(Element::ViewTemplate(ViewTemplate { name, ..t }))))
    }

    /// Apply Template Properties: the template's included properties stamped onto the view once. The view keeps
    /// whatever template it is bound to, or none; a property its bound template controls is left to that template.
    /// Returns how many properties were taken.
    pub fn apply_view_template_once(&mut self, view: Id, template: Id) -> Result<usize, String> {
        let t = self.view_template(template).cloned().ok_or("not a view template")?;
        let mut v = self.view(view).cloned().ok_or("not a view")?;
        if v.view_kind != t.view_kind { return Err("That template is for another kind of view.".into()); }
        let held: Vec<TemplateProp> = v.template.and_then(|b| self.view_template(b)).map(|b| b.included.iter().copied().collect()).unwrap_or_default();
        let take: Vec<TemplateProp> = t.included.iter().copied().filter(|p| !held.contains(p)).collect();
        if take.is_empty() { return Err("Everything that template sets is controlled by the view's own template.".into()); }
        for p in &take { v.settings.take(&t.settings, *p); }
        if self.view(view) == Some(&v) { return Ok(take.len()); }
        self.transact("Apply view template properties", |tx| tx.update(view, Element::View(v)))?;
        Ok(take.len())
    }

    /// The template's included properties set to what a view shows now (the way to edit a template's graphics:
    /// set a view up, then bring the template up to date from it). Every view bound to the template follows.
    pub fn update_view_template_from_view(&mut self, template: Id, view: Id) -> Result<(), String> {
        let mut t = self.view_template(template).cloned().ok_or("not a view template")?;
        let v = self.view(view).ok_or("not a view")?;
        if v.view_kind != t.view_kind { return Err("That view is another kind of view from the template's.".into()); }
        // what the view shows, less a temporary template over it
        let shown = self.effective_settings_of(v);
        for p in t.included.clone() { t.settings.take(&shown, p); }
        if self.view_template(template) == Some(&t) { return Ok(()); }
        self.transact("Update view template from view", |tx| tx.update(template, Element::ViewTemplate(t)))
    }

    /// Temporary View Properties: `template` over the view for now, or None to go back to the view as saved.
    pub fn set_temp_view_template(&self, view: Id, template: Option<Id>) -> Result<(), String> {
        let v = self.view(view).ok_or("not a view")?;
        match template {
            Some(t) => {
                let tt = self.view_template(t).ok_or("not a view template")?;
                if tt.view_kind != v.view_kind { return Err("That template is for another kind of view.".into()); }
                self.temp_templates.borrow_mut().insert(view, t);
            }
            None => { self.temp_templates.borrow_mut().remove(&view); }
        }
        // what the view's filters match may have changed with it
        *self.filter_cache.borrow_mut() = Default::default();
        Ok(())
    }

    /// The template temporarily over a view, if any (and if it still exists).
    pub fn temp_view_template(&self, view: Id) -> Option<Id> {
        self.temp_templates.borrow().get(&view).copied().filter(|t| self.view_template(*t).is_some())
    }

    /// Every view back to how it is saved (before printing or exporting). Returns how many were temporary.
    pub fn clear_temp_view_templates(&self) -> usize {
        let n = self.temp_templates.borrow().len();
        if n > 0 {
            self.temp_templates.borrow_mut().clear();
            *self.filter_cache.borrow_mut() = Default::default();
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{DetailLevel, ViewKind};

    fn plans(d: &Document) -> Vec<Id> {
        d.iter().filter_map(|(id, e)| match e { Element::View(v) if v.view_kind == ViewKind::FloorPlan => Some(id), _ => None }).collect()
    }

    #[test]
    fn a_template_is_applied_once_updated_from_a_view_and_put_on_temporarily() {
        let mut d = Document::new_project();
        let ps = plans(&d);
        let (a, b) = (ps[0], ps[1]);
        d.patch_view(a, &serde_json::json!({ "settings": { "scale": 50, "detail_level": "fine" } })).unwrap();
        let t = d.create_view_template("Fine 1:50", a).unwrap();
        d.patch_view_template(t, &serde_json::json!({ "included": ["scale", "detail_level"] })).unwrap();

        // once: b takes the values and stays unbound, so it can change again
        assert_eq!(d.apply_view_template_once(b, t).unwrap(), 2);
        let vb = d.view(b).unwrap();
        assert_eq!((vb.settings.scale, vb.settings.detail_level, vb.template), (50, DetailLevel::Fine, None));
        d.patch_view(b, &serde_json::json!({ "settings": { "scale": 200 } })).unwrap();
        d.undo().unwrap();
        d.undo().unwrap();
        assert_ne!(d.view(b).unwrap().settings.scale, 50, "one undo step for the stamp");

        // a copy, and the template brought up to date from a view
        let copy = d.duplicate_view_template(t, None).unwrap();
        assert_eq!(d.view_template(copy).unwrap().name, "Fine 1:50 Copy 1");
        assert!(d.duplicate_view_template(t, Some("Fine 1:50")).is_err());
        d.patch_view(a, &serde_json::json!({ "settings": { "scale": 20 } })).unwrap();
        d.update_view_template_from_view(t, a).unwrap();
        assert_eq!(d.view_template(t).unwrap().settings.scale, 20);

        // temporarily over b: b draws at the template's scale, and is unchanged underneath
        let before = d.view(b).unwrap().clone();
        let revision = d.revision();
        d.set_temp_view_template(b, Some(t)).unwrap();
        assert_eq!(d.effective_settings(b).unwrap().scale, 20);
        assert_eq!((d.view(b).unwrap(), d.revision()), (&before, revision), "nothing written to the document");
        assert_eq!(d.clear_temp_view_templates(), 1);
        assert_eq!(d.effective_settings(b).unwrap().scale, before.settings.scale);
    }
}
