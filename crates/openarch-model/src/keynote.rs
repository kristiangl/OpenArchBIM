//! Keynotes (Annotate > Keynote): a keynote table read from a keynote file (Revit's format: each line a key, its
//! text and, for a note under a heading, the heading's key, separated by tabs), and keynote tags. An Element keynote
//! shows the Keynote parameter of the element's type, a Material keynote the Keynote of the material at the point
//! tagged, a User keynote a key chosen from the table. The tags are element tags (tags.rs) of the Keynote Tag kind.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::{Element, Id};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeynoteEntry {
    pub key: String,
    pub text: String,
    /// The heading this note sits under (empty for a heading).
    #[serde(default)]
    pub parent: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct KeynoteTable {
    /// The file it was read from, for the record.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub entries: Vec<KeynoteEntry>,
}

/// Where a keynote tag's key comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeynoteSource {
    Element,
    Material,
    User { key: String },
}

/// Read a keynote file: tab-separated key, text and parent key; blank lines and lines starting with # are skipped.
pub fn parse_keynotes(text: &str) -> Result<Vec<KeynoteEntry>, String> {
    let mut out: Vec<KeynoteEntry> = vec![];
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let key = cols.next().unwrap_or("").trim().to_string();
        let text = cols.next().unwrap_or("").trim().to_string();
        let parent = cols.next().unwrap_or("").trim().to_string();
        if key.is_empty() {
            return Err(format!("Line {} has no key.", n + 1));
        }
        if out.iter().any(|e| e.key == key) {
            return Err(format!("Line {}: the key {key} is there twice.", n + 1));
        }
        out.push(KeynoteEntry { key, text, parent });
    }
    if out.is_empty() {
        return Err("That file has no keynotes in it (each line: key, tab, text, and a tab and the heading's key for a note).".into());
    }
    Ok(out)
}

impl Document {
    pub fn keynote_table(&self) -> KeynoteTable {
        self.iter().find_map(|(_, e)| match e { Element::KeynoteTable(t) => Some(t.clone()), _ => None }).unwrap_or_default()
    }

    /// Replace the keynote table (Load keynote file), one undo step.
    pub fn set_keynote_table(&mut self, t: KeynoteTable) -> Result<(), String> {
        let id = self.iter().find_map(|(id, e)| matches!(e, Element::KeynoteTable(_)).then_some(id));
        self.transact("Keynote file", |tx| match id {
            Some(id) => tx.update(id, Element::KeynoteTable(t)),
            None => {
                tx.add(Element::KeynoteTable(t));
                Ok(())
            }
        })
    }

    pub fn keynote_text(&self, key: &str) -> Option<String> {
        self.keynote_table().entries.into_iter().find(|e| e.key == key).map(|e| e.text)
    }

    /// The key a keynote tag shows: its type's Keynote, its material's, or the one chosen.
    pub fn keynote_key(&self, t: &crate::tags::ElementTag) -> Option<String> {
        match t.keynote.as_ref()? {
            KeynoteSource::User { key } => Some(key.clone()),
            KeynoteSource::Material => t.material.as_deref().and_then(|m| self.material(m)).map(|m| m.identity.keynote.clone()),
            KeynoteSource::Element => Some(self.type_keynote(t.element)),
        }
    }

    /// The Keynote parameter of an element's type (system types' identity data, a family type's Keynote value).
    pub fn type_keynote(&self, element: Id) -> String {
        let info = |i: &crate::element::TypeInfo| i.keynote.clone();
        match self.get(element) {
            Some(Element::Wall(w)) => self.wall_type(w.wall_type).map(|t| info(&t.info)),
            Some(Element::Floor(f)) => self.floor_type(f.floor_type).map(|t| info(&t.info)),
            Some(Element::Roof(r)) => self.roof_type(r.roof_type).map(|t| info(&t.info)),
            Some(Element::Ceiling(c)) => self.ceiling_type(c.ceiling_type).map(|t| info(&t.info)),
            Some(Element::FamilyInstance(i)) => self.instance_family(i).and_then(|(_, ty)| match ty.values.get("Keynote") {
                Some(crate::family::ParamValue::Text(s)) => Some(s.clone()),
                _ => None,
            }),
            Some(e) => e.type_id().and_then(|t| match self.get(t) {
                Some(Element::StructuralType(s)) => Some(info(&s.info)),
                _ => None,
            }),
            None => None,
        }
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keynote_files_are_read_and_tags_show_their_keys() {
        let file = "# a comment\nE\tExternal walls\nE10\tFace brick veneer, 110 mm\tE\n\nE20\tTimber frame, 90 mm\tE\r\n";
        let entries = parse_keynotes(file).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!((entries[1].key.as_str(), entries[1].parent.as_str()), ("E10", "E"));
        assert!(parse_keynotes("E10\tA\nE10\tB").is_err());
        assert!(parse_keynotes("\n# nothing\n").is_err());
        assert!(parse_keynotes("\tNo key").is_err());

        let mut d = Document::new_project();
        d.set_keynote_table(KeynoteTable { source: "test.txt".into(), entries }).unwrap();
        assert_eq!(d.keynote_text("E20").as_deref(), Some("Timber frame, 90 mm"));
        let lvl = d.levels()[0].0;
        let wt = d.wall_types()[1].0;
        let w = d.add_wall(wt, lvl, crate::geom::Point::new(0.0, 0.0), crate::geom::Point::new(4000.0, 0.0), 2700.0).unwrap();
        d.patch_system_type(wt, &serde_json::json!({ "info": { "keynote": "E10" } })).unwrap();
        let plan = d.views().into_iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::FloorPlan && v.level == Some(lvl)).unwrap().0;
        let o = crate::tags::TagOptions::default();
        let el = d.add_keynote_tag(plan, w, KeynoteSource::Element, None, crate::geom::Point::new(2000.0, 0.0), &o).unwrap();
        assert_eq!(d.keynote_key(d.element_tag(el).unwrap()).as_deref(), Some("E10"));
        let us = d.add_keynote_tag(plan, w, KeynoteSource::User { key: "E20".into() }, None, crate::geom::Point::new(1000.0, 0.0), &o).unwrap();
        assert_eq!(d.keynote_key(d.element_tag(us).unwrap()).as_deref(), Some("E20"));
        assert!(d.add_keynote_tag(plan, w, KeynoteSource::User { key: "Z99".into() }, None, crate::geom::Point::new(1000.0, 0.0), &o).is_err());
        let mat = d.element_materials(w)[0].clone();
        let mk = d.add_keynote_tag(plan, w, KeynoteSource::Material, Some(&mat), crate::geom::Point::new(3000.0, 0.0), &o).unwrap();
        assert_eq!(d.kind_of_tag(d.element_tag(mk).unwrap()), Some(crate::annotation::AnnotationKind::KeynoteTag));
    }
}
