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

/// Which built-in family a symbol with no family of its own is placed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinSymbol {
    NorthArrow,
    ScaleBar,
}

impl AnnotationFamily {
    /// A scale bar about 60 mm long on paper; what it shows comes from the view's scale (`scale_bar_drawing`).
    pub fn scale_bar() -> AnnotationFamily {
        let mut f = AnnotationFamily::drawn_with("Scale Bar", AnnotationKind::Symbol, vec![], vec![], vec![]);
        f.scale_bar_mm = Some(60.0);
        f
    }

    /// A scale bar drawn for a view at 1 : `scale`: the longest round length (1, 2 or 5 times a power of ten metres)
    /// that fits in the family's paper length, cut into 4 or 5 parts filled in turn, numbered at 0, the middle and
    /// the end, in metres. Starts at its point and runs to the right.
    pub fn scale_bar_drawing(&self, scale: f64) -> AnnotationFamily {
        let target = self.scale_bar_mm.unwrap_or(60.0);
        let s = scale.max(1.0);
        // the round length in metres
        let mut best = 0.001;
        for p in -3..=5 {
            for m in [1.0, 2.0, 5.0] {
                let len_m = m * 10f64.powi(p);
                if len_m * 1000.0 / s <= target + 1e-9 {
                    best = f64::max(best, len_m);
                }
            }
        }
        let paper = best * 1000.0 / s;
        let lead = best / 10f64.powf(best.log10().floor());
        let parts = if (lead - 2.0).abs() < 1e-6 { 4 } else { 5 };
        let h = 2.0;
        let mut shapes = vec![AnnShape::Polygon { pts: vec![[0.0, 0.0], [paper, 0.0], [paper, h], [0.0, h]], filled: false }];
        for k in 0..parts {
            let (x0, x1) = (paper * k as f64 / parts as f64, paper * (k + 1) as f64 / parts as f64);
            shapes.push(AnnShape::Polygon { pts: vec![[x0, 0.0], [x1, 0.0], [x1, h], [x0, h]], filled: k % 2 == 0 });
        }
        let mut f = AnnotationFamily::drawn_with(&self.name, AnnotationKind::Symbol, shapes, vec![], vec![]);
        let fmt = |v: f64| { let t = format!("{v:.3}"); t.trim_end_matches('0').trim_end_matches('.').to_string() };
        let text = |x: f64, t: String| AnnText { x, y: h + 1.0, text: t, text_type: "2.5mm".into(), halign: HAlign::Center, valign: VAlign::Bottom, width_mm: 0.0, keep_readable: false, visible: true };
        f.texts.push(text(0.0, "0".into()));
        f.texts.push(text(paper / 2.0, fmt(best / 2.0)));
        f.texts.push(text(paper, format!("{} m", fmt(best))));
        f.texts.push(AnnText { x: paper / 2.0, y: -1.0, text: format!("1 : {}", s.round()), text_type: "2.5mm".into(), halign: HAlign::Center, valign: VAlign::Top, width_mm: 0.0, keep_readable: false, visible: true });
        f
    }


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
        self.add_symbol_of(view, family, BuiltinSymbol::NorthArrow, at, rotation_deg)
    }

    /// Place a symbol; with no family, the built-in one (made the first time, in the same undo step). A scale bar
    /// goes in a view, not on a sheet (a sheet has no scale of its own).
    pub fn add_symbol_of(&mut self, view: Id, family: Option<Id>, builtin: BuiltinSymbol, at: Point, rotation_deg: f64) -> Result<Id, String> {
        self.check_detail_view(view)?;
        let on_sheet = self.sheet(view).is_some();
        let wants_bar = match family {
            Some(f) => self.annotation_family(f).is_some_and(|x| x.scale_bar_mm.is_some()),
            None => builtin == BuiltinSymbol::ScaleBar,
        };
        if on_sheet && wants_bar {
            return Err("A scale bar goes in a view: a sheet has no scale of its own.".into());
        }
        if let Some(f) = family {
            if !self.annotation_family(f).is_some_and(|x| x.kind == AnnotationKind::Symbol) {
                return Err("A symbol is placed from a generic annotation family.".into());
            }
        }
        if !rotation_deg.is_finite() {
            return Err("bad rotation".into());
        }
        let name = if builtin == BuiltinSymbol::ScaleBar { "Scale Bar" } else { "North Arrow" };
        let existing = self.symbol_families().iter().find(|(_, f)| f.name == name).map(|(id, _)| *id);
        self.transact("Symbol", |tx| {
            let family = match family.or(existing) {
                Some(f) => f,
                None => tx.add(Element::AnnotationFamily(if builtin == BuiltinSymbol::ScaleBar { AnnotationFamily::scale_bar() } else { AnnotationFamily::north_arrow() })),
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
