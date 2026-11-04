//! Clipboard and Match Type: Copy (Ctrl+C), Cut (Ctrl+X), the Paste variants, and Match Type Properties.
use serde_json::{json, Value};

use openarch_model::clipboard::{Clipboard, PasteTarget};
use openarch_model::{Element, Id};

use crate::{id_of, ids_of, point_of, Res, Session};

impl Session {
    fn clip(&self) -> Result<&Clipboard, String> {
        let (serial, c) = self.clipboard.as_ref().ok_or("The clipboard is empty: select elements and Copy or Cut them first.")?;
        if *serial != self.doc_serial { return Err("The clipboard holds elements from another project. Copying between projects comes later.".into()); }
        Ok(c)
    }

    fn skipped_note(c: &Clipboard) -> Value {
        json!(c.skipped.iter().map(|(k, n)| json!({ "kind": k, "count": n })).collect::<Vec<_>>())
    }

    pub(crate) fn handle_clipboard(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // { ids, view_id }
            "copy" => (|| {
                let c = self.doc.copy_to_clipboard(&ids_of(p, "ids")?, p["view_id"].as_u64())?;
                let out = json!({ "count": c.count(), "skipped": Self::skipped_note(&c) });
                self.clipboard = Some((self.doc_serial, c));
                Ok(out)
            })(),
            // Cut: copied, then deleted (one undo step brings them back; the clipboard keeps them)
            "cut" => (|| {
                let ids = ids_of(p, "ids")?;
                let c = self.doc.copy_to_clipboard(&ids, p["view_id"].as_u64())?;
                let copied: Vec<Id> = c.elements.iter().map(|(i, _)| *i).filter(|i| ids.contains(i)).collect();
                self.doc.refuse_pinned(&copied, "cut")?;
                self.doc.delete_items(&copied)?;
                let out = json!({ "count": c.count(), "skipped": Self::skipped_note(&c) });
                self.clipboard = Some((self.doc_serial, c));
                Ok(out)
            })(),
            "clipboard_info" => (|| {
                let Some((serial, c)) = self.clipboard.as_ref() else { return Ok(json!({ "count": 0 })) };
                let v = c.view.and_then(|v| self.doc.view(v));
                Ok(json!({
                    "count": c.count(), "same_project": *serial == self.doc_serial, "has_model": c.has_model(), "has_view_specific": c.has_view_specific(),
                    "origin": [c.origin.x, c.origin.y], "view_id": c.view, "view_kind": v.map(|v| v.view_kind),
                    "preview": c.preview().iter().map(|l| l.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>(),
                }))
            })(),
            // { mode: point | current_view | same_place | levels | views, view_id, point, levels, views }
            "paste" => (|| {
                let target = match p["mode"].as_str().unwrap_or("point") {
                    "point" => PasteTarget::Point { view: id_of(p, "view_id")?, at: point_of(&p["point"])? },
                    "current_view" => PasteTarget::CurrentView { view: id_of(p, "view_id")? },
                    "same_place" => PasteTarget::SamePlace,
                    "levels" => PasteTarget::Levels(ids_of(p, "levels")?),
                    "views" => PasteTarget::Views(ids_of(p, "views")?),
                    m => return Err(format!("unknown paste mode {m}")),
                };
                let c = self.clip()?.clone();
                Ok(json!({ "ids": self.doc.paste(&c, target)? }))
            })(),
            // Match Type Properties: { source, targets } gives each target of the source's kind the source's type
            "match_type" => (|| {
                let source = id_of(p, "source")?;
                let targets = ids_of(p, "targets")?;
                let d = &self.doc;
                let src = d.get(source).ok_or("Pick an element to match.")?.clone();
                let cat = |e: &Element| match e { Element::FamilyInstance(i) => d.instance_family(i).map(|(f, _)| f.category), _ => None };
                let src_cat = cat(&src);
                let ok: Vec<Id> = targets.iter().copied().filter(|t| *t != source && d.get(*t).is_some_and(|e| std::mem::discriminant(e) == std::mem::discriminant(&src) && cat(e) == src_cat)).collect();
                let skipped = targets.len() - ok.len();
                if ok.is_empty() { return Err("Pick elements of the same kind (and category) as the one you matched.".into()); }
                self.doc.grouped("Match type", |d| {
                    for id in &ok {
                        let id = *id;
                        match &src {
                            Element::Wall(w) => d.set_wall_type(id, w.wall_type)?,
                            Element::Floor(f) => d.set_floor_type(id, f.floor_type)?,
                            Element::Roof(r) => d.set_roof_type(id, r.roof_type)?,
                            Element::Ceiling(c) => d.patch_ceiling(id, &json!({ "ceiling_type": c.ceiling_type }))?,
                            Element::FamilyInstance(i) => d.patch_instance(id, &json!({ "type_id": i.type_id }))?,
                            Element::Grid(g) => d.patch_grid(id, &json!({ "grid_type": g.grid_type }))?,
                            Element::TextNote(t) => d.patch_text_note(id, &json!({ "text_type": t.text_type }))?,
                            Element::DetailRegion(r) => d.patch_detail_region(id, &json!({ "region_type": r.region_type }))?,
                            Element::Dimension(x) => d.patch_dimension(id, &json!({ "dim_type": x.dim_type }))?,
                            _ => return Err("That element has no type to match.".into()),
                        }
                    }
                    Ok(())
                })?;
                Ok(json!({ "changed": ok.len(), "skipped": skipped }))
            })(),
            _ => return None,
        })
    }
}
