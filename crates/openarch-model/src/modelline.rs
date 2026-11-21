//! Model lines (Architecture > Model Line): lines that belong to the model, not to a view. Each lies flat on a level,
//! at its elevation, and shows in that level's floor and structural plans and in 3D views, in its line style (the
//! Lines category, as detail lines).

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::doc::Document;
use crate::element::{Element, Id};

fn thin() -> String {
    "Thin Lines".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelLine {
    pub level: Id,
    /// In plan, model mm; it lies at the level's elevation.
    pub curve: Curve,
    #[serde(default = "thin")]
    pub style: String,
    #[serde(default)]
    pub pinned: bool,
}

/// One line of the Model Line session: an existing line (its id) or a new one.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelLineEdit {
    #[serde(default)]
    pub id: Option<Id>,
    pub curve: Curve,
    #[serde(default = "thin")]
    pub style: String,
}

impl Document {
    pub fn model_line(&self, id: Id) -> Option<&ModelLine> {
        match self.get(id) {
            Some(Element::ModelLine(l)) => Some(l),
            _ => None,
        }
    }

    pub fn model_lines_on(&self, level: Id) -> Vec<(Id, &ModelLine)> {
        self.iter().filter_map(|(id, e)| match e { Element::ModelLine(l) if l.level == level => Some((id, l)), _ => None }).collect()
    }

    /// The model lines a view shows: a floor or structural plan, those on its level.
    pub fn model_lines_in(&self, view: Id) -> Vec<(Id, &ModelLine)> {
        let Some(v) = self.view(view) else { return vec![] };
        if !matches!(v.view_kind, crate::view::ViewKind::FloorPlan | crate::view::ViewKind::StructuralPlan) {
            return vec![];
        }
        let Some(level) = v.level else { return vec![] };
        self.model_lines_on(level).into_iter().filter(|(id, _)| !self.hidden_in(view, *id)).collect()
    }

    /// Every model line as points in 3D (model mm, at its level's elevation) in its line style's colour, for 3D views;
    /// `hidden` leaves some out.
    pub fn model_lines_3d(&self, hidden: impl Fn(Id) -> bool) -> Vec<(Id, [u8; 3], Vec<[f64; 3]>)> {
        self.iter()
            .filter_map(|(id, e)| match e {
                Element::ModelLine(l) if !hidden(id) && l.style != crate::annotation::INVISIBLE_LINES => {
                    let z = self.level(l.level)?.elevation_mm;
                    Some((id, self.line_style(&l.style).color, l.curve.tessellate(5.0).iter().map(|q| [q.x, q.y, z]).collect()))
                }
                _ => None,
            })
            .collect()
    }

    /// Lines drawn on a level in one sketch, one undo step.
    pub fn add_model_lines(&mut self, level: Id, curves: Vec<Curve>, style: &str) -> Result<Vec<Id>, String> {
        if self.level(level).is_none() {
            return Err("Unknown level".into());
        }
        self.check_line_style(style, false)?;
        if curves.is_empty() {
            return Err("Draw at least one line.".into());
        }
        for c in &curves {
            let len = c.length();
            if !len.is_finite() || len <= 0.5 || c.tessellate(1.0).iter().any(|q| !q.x.is_finite() || !q.y.is_finite()) {
                return Err("a model line needs some length".into());
            }
        }
        self.transact("Model lines", |tx| {
            Ok(curves.into_iter().map(|curve| tx.add(Element::ModelLine(ModelLine { level, curve, style: style.into(), pinned: false }))).collect())
        })
    }

    /// The Model Line tool's session in a plan, as Detail Line's (`set_detail_lines`): the lines the plan shows of its
    /// level as they are now. Lines with an id are changed, new ones added, the plan's others deleted; one undo step.
    /// Lines hidden in the view are left alone. Returns each line's id in order.
    pub fn set_model_lines(&mut self, view: Id, lines: Vec<ModelLineEdit>) -> Result<Vec<Id>, String> {
        let level = self
            .view(view)
            .filter(|v| matches!(v.view_kind, crate::view::ViewKind::FloorPlan | crate::view::ViewKind::StructuralPlan))
            .and_then(|v| v.level)
            .ok_or("Model lines are drawn in a floor or structural plan.")?;
        for l in &lines {
            self.check_line_style(&l.style, false)?;
            let len = l.curve.length();
            if !len.is_finite() || len <= 0.5 {
                return Err("a model line needs some length".into());
            }
        }
        let have: Vec<Id> = self.model_lines_in(view).iter().map(|(id, _)| *id).collect();
        let mut used = std::collections::BTreeSet::new();
        let plan: Vec<(Option<Id>, ModelLine)> = lines
            .into_iter()
            .map(|l| {
                let id = l.id.filter(|i| have.contains(i) && used.insert(*i));
                let pinned = id.and_then(|i| self.model_line(i)).is_some_and(|m| m.pinned);
                (id, ModelLine { level, curve: l.curve, style: l.style, pinned })
            })
            .collect();
        let gone: Vec<Id> = have.iter().copied().filter(|i| !used.contains(i)).collect();
        let changed = !gone.is_empty() || plan.iter().any(|(id, l)| id.is_none_or(|i| self.model_line(i) != Some(l)));
        if !changed {
            return Ok(plan.iter().filter_map(|(id, _)| *id).collect());
        }
        self.refuse_pinned(&gone, "deleted")?;
        self.transact("Model lines", |tx| {
            for i in &gone {
                tx.remove(*i)?;
            }
            let mut out = vec![];
            for (id, l) in plan {
                match id {
                    Some(i) => {
                        if tx.doc_ref().get(i) != Some(&Element::ModelLine(l.clone())) {
                            tx.update(i, Element::ModelLine(l))?;
                        }
                        out.push(i);
                    }
                    None => out.push(tx.add(Element::ModelLine(l))),
                }
            }
            Ok(out)
        })
    }

    /// Line style, the curve and Pinned; a model line stays on its level.
    pub fn patch_model_line(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.model_line(id).cloned().ok_or("not a model line")?;
        let new: ModelLine = crate::doc::apply_patch(&old, patch)?;
        if new == old {
            return Ok(());
        }
        if new.level != old.level {
            return Err("a model line stays on its level".into());
        }
        self.check_line_style(&new.style, false)?;
        if new.curve.length() <= 0.5 {
            return Err("a model line needs some length".into());
        }
        self.transact("Change model line", |tx| tx.update(id, Element::ModelLine(new)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;

    #[test]
    fn model_lines_sit_on_a_level_and_show_in_its_plans() {
        let mut d = Document::new_project();
        let (plan, level) = d.views().into_iter().find_map(|(id, v)| (v.view_kind == crate::view::ViewKind::FloorPlan).then_some((id, v.level?))).unwrap();
        let ids = d.add_model_lines(level, vec![Curve::line(Point::new(0.0, 0.0), Point::new(3000.0, 0.0)), Curve::line(Point::new(0.0, 0.0), Point::new(0.0, 2000.0))], "Wide Lines").unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(d.model_lines_in(plan).len(), 2);
        // another level's plan does not show them
        let other = d.views().into_iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::FloorPlan && v.level != Some(level)).map(|(id, _)| id);
        if let Some(o) = other {
            assert!(d.model_lines_in(o).is_empty());
        }
        // drawn through the view's detail paths, in the style's weight
        let p = d.detail_paths(plan, 100.0, true, true);
        assert_eq!(p.strokes.iter().filter(|s| s.line.is_some_and(|l| ids.contains(&(l as Id)))).count(), 2);
        assert!(d.add_model_lines(level, vec![Curve::line(Point::new(0.0, 0.0), Point::new(0.1, 0.0))], "Thin Lines").is_err());
        assert!(d.add_model_lines(level, vec![Curve::line(Point::new(0.0, 0.0), Point::new(10.0, 0.0))], "No Such").is_err());
        d.patch_model_line(ids[0], &serde_json::json!({ "style": "Thin Lines" })).unwrap();
        assert_eq!(d.model_line(ids[0]).unwrap().style, "Thin Lines");
        assert!(d.patch_model_line(ids[0], &serde_json::json!({ "level": 999 })).is_err());
        // moved by the Modify tools, deleted with Delete
        d.transform_elements(&[ids[0]], crate::edit::Xform::Move { dx: 0.0, dy: 500.0 }, crate::edit::EditOptions { copy: false, disjoin: false }).unwrap();
        assert_eq!(d.model_line(ids[0]).unwrap().curve.start().y, 500.0);
        d.delete(&[ids[1]]).unwrap();
        assert_eq!(d.model_lines_on(level).len(), 1);
        assert!(d.level_users(level).contains(&ids[0]));
        // the session: the one line shortened and a new one, in one step
        let e = |id: Option<Id>, x: f64| ModelLineEdit { id, curve: Curve::line(Point::new(0.0, 500.0), Point::new(x, 500.0)), style: "Thin Lines".into() };
        let got = d.set_model_lines(plan, vec![e(Some(ids[0]), 1000.0), e(None, 2000.0)]).unwrap();
        assert_eq!((got[0], d.model_lines_on(level).len()), (ids[0], 2));
        assert_eq!(d.model_line(ids[0]).unwrap().curve.end().x, 1000.0);
        // leaving one out deletes it
        d.set_model_lines(plan, vec![e(Some(got[1]), 2000.0)]).unwrap();
        assert_eq!(d.model_lines_on(level).len(), 1);
        let three = d.views().into_iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::ThreeD).unwrap().0;
        assert!(d.set_model_lines(three, vec![]).is_err());
        assert_eq!(d.model_lines_3d(|_| false)[0].2.len(), 2);
    }
}
