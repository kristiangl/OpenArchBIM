//! Symbols (Annotate > Symbol, North Arrow): a generic annotation family placed in a view or on a sheet, at a point
//! and turned by an angle, drawn at its paper size whatever the view's scale. A North Arrow family comes with every
//! project the first time one is wanted.

use serde::{Deserialize, Serialize};

use crate::annotation::{AnnShape, AnnText, AnnotationFamily, AnnotationKind, HAlign, TextType, VAlign};
use crate::doc::Document;
use crate::element::{Element, Id};
use crate::geom::Point;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationSymbol {
    /// The view, or the sheet, it is drawn in.
    pub view: Id,
    pub family: Id,
    /// View mm (paper mm on a sheet).
    pub at: Point,
    /// Anticlockwise, degrees.
    #[serde(default)]
    pub rotation_deg: f64,
    #[serde(default)]
    pub pinned: bool,
}

impl AnnotationFamily {
    /// A north arrow: a circle, an arrow half filled, N above it (paper mm about its centre).
    pub fn north_arrow() -> AnnotationFamily {
        let shapes = vec![
            AnnShape::Circle { c: [0.0, 0.0], r: 7.0, filled: false },
            AnnShape::Polygon { pts: vec![[0.0, 9.0], [-3.5, -6.0], [0.0, -3.0]], filled: true },
            AnnShape::Polygon { pts: vec![[0.0, 9.0], [3.5, -6.0], [0.0, -3.0]], filled: false },
        ];
        let mut f = AnnotationFamily::drawn_with("North Arrow", AnnotationKind::Symbol, shapes, vec![], vec![]);
        f.text_types.push(TextType::new("3.5mm", 3.5, true));
        f.texts.push(AnnText { x: 0.0, y: 10.0, text: "N".into(), text_type: "3.5mm".into(), halign: HAlign::Center, valign: VAlign::Bottom, width_mm: 0.0, keep_readable: false, visible: true });
        f
    }
}

impl Document {
    pub fn symbol(&self, id: Id) -> Option<&AnnotationSymbol> {
        match self.get(id) {
            Some(Element::AnnotationSymbol(s)) => Some(s),
            _ => None,
        }
    }

    pub fn symbols_in(&self, view: Id) -> Vec<(Id, &AnnotationSymbol)> {
        self.iter().filter_map(|(id, e)| match e { Element::AnnotationSymbol(s) if s.view == view => Some((id, s)), _ => None }).collect()
    }

    /// The generic annotation families symbols can be placed from.
    pub fn symbol_families(&self) -> Vec<(Id, &AnnotationFamily)> {
        self.annotation_families().into_iter().filter(|(_, f)| f.kind == AnnotationKind::Symbol).collect()
    }

    /// Place a symbol in a view or on a sheet. With no family the project's North Arrow (made the first time, in
    /// the same undo step).
    pub fn add_symbol(&mut self, view: Id, family: Option<Id>, at: Point, rotation_deg: f64) -> Result<Id, String> {
        self.check_detail_view(view)?;
        if let Some(f) = family {
            if !self.annotation_family(f).is_some_and(|x| x.kind == AnnotationKind::Symbol) {
                return Err("A symbol is placed from a generic annotation family.".into());
            }
        }
        if !rotation_deg.is_finite() {
            return Err("bad rotation".into());
        }
        let north = self.symbol_families().iter().find(|(_, f)| f.name == "North Arrow").map(|(id, _)| *id);
        self.transact("Symbol", |tx| {
            let family = match family.or(north) {
                Some(f) => f,
                None => tx.add(Element::AnnotationFamily(AnnotationFamily::north_arrow())),
            };
            Ok(tx.add(Element::AnnotationSymbol(AnnotationSymbol { view, family, at, rotation_deg: rotation_deg.rem_euclid(360.0), pinned: false })))
        })
    }

    pub fn patch_symbol(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.symbol(id).cloned().ok_or("not a symbol")?;
        let mut new: AnnotationSymbol = crate::doc::apply_patch(&old, patch)?;
        new.view = old.view;
        if !self.annotation_family(new.family).is_some_and(|x| x.kind == AnnotationKind::Symbol) {
            return Err("A symbol is placed from a generic annotation family.".into());
        }
        if !new.rotation_deg.is_finite() {
            return Err("bad rotation".into());
        }
        new.rotation_deg = new.rotation_deg.rem_euclid(360.0);
        if new == old {
            return Ok(());
        }
        self.transact("Change symbol", |tx| tx.update(id, Element::AnnotationSymbol(new)))
    }
}
