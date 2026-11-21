//! Work planes (Architecture > Work Plane > Set and Show): what Model Line draws on in a view. A plan's work plane is
//! its level. An elevation or section has none until Set names a reference plane parallel to it; Model Line there
//! draws on that plane (the lines keep the plane's own frame, so every view parallel to it and 3D views show them).

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::edit::Xform;
use crate::element::{Element, Id};
use crate::modelline::{ModelLine, ModelLineEdit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkPlane {
    Level { id: Id },
    RefPlane { id: Id },
}

impl Document {
    /// A view's work plane: what Set chose when it still exists, else a plan's level.
    pub fn work_plane_of(&self, view: Id) -> Option<WorkPlane> {
        let v = self.view(view)?;
        match v.work_plane {
            Some(WorkPlane::RefPlane { id }) if self.ref_plane(id).is_some() && self.plane_to_view(view, id).is_some() => Some(WorkPlane::RefPlane { id }),
            _ if v.view_kind.is_plan() => v.level.map(|id| WorkPlane::Level { id }),
            _ => None,
        }
    }

    /// What Set offers in a view: the level of a plan, or the named reference planes parallel to an elevation or
    /// section, with their names.
    pub fn work_plane_choices(&self, view: Id) -> Vec<(WorkPlane, String)> {
        let Some(v) = self.view(view) else { return vec![] };
        if v.view_kind.is_plan() {
            return v.level.and_then(|l| self.level(l).map(|x| (WorkPlane::Level { id: l }, format!("Level : {}", x.name)))).into_iter().collect();
        }
        self.ref_planes()
            .into_iter()
            .filter(|(id, _)| self.plane_to_view(view, *id).is_some())
            .map(|(id, r)| (WorkPlane::RefPlane { id }, format!("Reference Plane : {}", if r.name.is_empty() { format!("({id})") } else { r.name.clone() })))
            .collect()
    }

    /// Set the work plane of a view, one undo step: a reference plane parallel to an elevation or section. A plan
    /// draws on its own level.
    pub fn set_work_plane(&mut self, view: Id, plane: WorkPlane) -> Result<(), String> {
        let v = self.view(view).cloned().ok_or("not a view")?;
        match plane {
            WorkPlane::Level { id } => {
                if !v.view_kind.is_plan() {
                    return Err("A level is edge on in this view: set a reference plane parallel to it.".into());
                }
                if v.level != Some(id) {
                    return Err("A plan draws on its own level: open the plan of that level.".into());
                }
            }
            WorkPlane::RefPlane { id } => {
                if self.ref_plane(id).is_none() {
                    return Err("not a reference plane".into());
                }
                if v.view_kind.is_plan() {
                    return Err("A reference plane stands upright: set it in an elevation or section that looks at it.".into());
                }
                if self.plane_to_view(view, id).is_none() {
                    return Err("That reference plane is not parallel to this view.".into());
                }
            }
        }
        if v.work_plane == Some(plane) {
            return Ok(());
        }
        self.transact("Set Work Plane", |tx| tx.update(view, Element::View(crate::view::View { work_plane: Some(plane), ..v })))
    }

    /// How a reference plane's own frame (along it from its first point, and height) maps to an elevation or section
    /// parallel to it (along the view, and height); None when the view does not look square at it.
    pub fn plane_to_view(&self, view: Id, plane: Id) -> Option<Xform> {
        let r = self.ref_plane(plane)?;
        let f = self.elevation_frame(view)?;
        let len = r.a.dist(&r.b);
        if len < 1e-6 {
            return None;
        }
        let (dx, dy) = ((r.b.x - r.a.x) / len, (r.b.y - r.a.y) / len);
        let sign = dx * f.right[0] + dy * f.right[1];
        if sign.abs() < 0.9998 {
            return None;
        }
        let off = (r.a.x - f.origin[0]) * f.right[0] + (r.a.y - f.origin[1]) * f.right[1];
        // u along the plane is off + u in the view, or off - u when the plane runs the other way (a mirror)
        Some(if sign > 0.0 { Xform::Move { dx: off, dy: 0.0 } } else { Xform::Mirror { ax: off / 2.0, ay: 0.0, bx: off / 2.0, by: 1.0 } })
    }

    /// The Model Line session in an elevation or section: its lines on the view's work plane, as `set_model_lines`
    /// does in a plan, the curves given in the view's frame.
    pub(crate) fn set_plane_model_lines(&mut self, view: Id, lines: Vec<ModelLineEdit>) -> Result<Vec<Id>, String> {
        let Some(WorkPlane::RefPlane { id: plane }) = self.work_plane_of(view) else {
            return Err("Set a work plane first (Architecture > Work Plane > Set): a reference plane parallel to this view.".into());
        };
        let to_view = self.plane_to_view(view, plane).ok_or("The work plane is not parallel to this view.")?;
        let back = match to_view {
            Xform::Move { dx, dy } => Xform::Move { dx: -dx, dy: -dy },
            m => m,
        };
        let level = self.levels().into_iter().min_by(|a, b| a.1.elevation_mm.total_cmp(&b.1.elevation_mm)).map(|(id, _)| id).ok_or("no levels")?;
        for l in &lines {
            self.check_line_style(&l.style, false)?;
            let len = l.curve.length();
            if !len.is_finite() || len <= 0.5 {
                return Err("a model line needs some length".into());
            }
        }
        let have: Vec<Id> = self.model_lines_drawn(view, Some(plane)).iter().map(|(id, _, _)| *id).collect();
        let mut used = std::collections::BTreeSet::new();
        let plan: Vec<(Option<Id>, ModelLine)> = lines
            .into_iter()
            .map(|l| {
                let id = l.id.filter(|i| have.contains(i) && used.insert(*i));
                let old = id.and_then(|i| self.model_line(i));
                let pinned = old.is_some_and(|m| m.pinned);
                let level = old.map_or(level, |m| m.level);
                (id, ModelLine { level, curve: l.curve.transform(back), plane: Some(plane), style: l.style, pinned })
            })
            .collect();
        let gone: Vec<Id> = have.iter().copied().filter(|i| !used.contains(i)).collect();
        // a line comes back from the view's frame within rounding: unchanged lines stay as they are
        let same = |a: &ModelLine, b: &ModelLine| a.style == b.style && a.plane == b.plane && a.curve.start().dist(&b.curve.start()) < 1e-6 && a.curve.end().dist(&b.curve.end()) < 1e-6 && (a.curve.length() - b.curve.length()).abs() < 1e-6;
        let changed = !gone.is_empty() || plan.iter().any(|(id, l)| id.is_none_or(|i| self.model_line(i).is_none_or(|o| !same(o, l))));
        if !changed {
            return Ok(plan.iter().filter_map(|(id, _)| *id).collect());
        }
        self.refuse_pinned(&gone, "deleted")?;
        let olds: Vec<Option<ModelLine>> = plan.iter().map(|(id, _)| id.and_then(|i| self.model_line(i).cloned())).collect();
        self.transact("Model lines", |tx| {
            for i in &gone {
                tx.remove(*i)?;
            }
            let mut out = vec![];
            for ((id, l), old) in plan.into_iter().zip(olds) {
                match id {
                    Some(i) => {
                        if old.is_none_or(|o| !same(&o, &l)) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;
    use crate::geom::Point;

    #[test]
    fn model_lines_go_on_a_reference_plane_set_in_an_elevation() {
        let mut d = Document::new_project();
        let south = d.views().into_iter().find(|(_, v)| v.name == "South").unwrap().0;
        let east = d.views().into_iter().find(|(_, v)| v.name == "East").unwrap().0;
        let plan = d.views().into_iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::FloorPlan).unwrap().0;
        // a plane running east to west and one north to south
        let ew = d.add_ref_plane(Point::new(6000.0, 1000.0), Point::new(0.0, 1000.0), "Facade").unwrap();
        let ns = d.add_ref_plane(Point::new(0.0, 0.0), Point::new(0.0, 5000.0), "Side").unwrap();
        assert!(d.work_plane_of(south).is_none());
        let names: Vec<String> = d.work_plane_choices(south).into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, vec!["Reference Plane : Facade".to_string()]);
        assert!(d.set_work_plane(south, WorkPlane::RefPlane { id: ns }).is_err());
        assert!(d.set_work_plane(plan, WorkPlane::RefPlane { id: ew }).is_err());
        assert!(d.set_model_lines(south, vec![]).is_err());
        d.set_work_plane(south, WorkPlane::RefPlane { id: ew }).unwrap();
        assert_eq!(d.work_plane_of(south), Some(WorkPlane::RefPlane { id: ew }));
        // a line drawn in the view's frame, 1000 to 3000 along and at 2000 up
        let f = d.elevation_frame(south).unwrap();
        let along = |x: f64, y: f64| (x - f.origin[0]) * f.right[0] + (y - f.origin[1]) * f.right[1];
        let (u0, u1) = (along(1000.0, 1000.0), along(3000.0, 1000.0));
        let edit = |id: Option<Id>, a: f64, b: f64| ModelLineEdit { id, curve: Curve::line(Point::new(a, 2000.0), Point::new(b, 2000.0)), style: "Thin Lines".into() };
        let ids = d.set_model_lines(south, vec![edit(None, u0, u1)]).unwrap();
        let l = d.model_line(ids[0]).unwrap();
        assert_eq!(l.plane, Some(ew));
        // in the plane's frame it runs 3000 to 5000 from the plane's first point (6000, 1000) going west
        let (a, b) = (l.curve.start(), l.curve.end());
        assert!((a.x.min(b.x) - 3000.0).abs() < 1e-6 && (a.x.max(b.x) - 5000.0).abs() < 1e-6 && (a.y - 2000.0).abs() < 1e-6, "{a:?} {b:?}");
        // drawn back in the view where it was drawn; in 3D from x 1000 to 3000 at y 1000, 2000 up
        let drawn = d.model_lines_drawn(south, None);
        assert_eq!(drawn.len(), 1);
        let c = &drawn[0].2;
        assert!((c.start().x.min(c.end().x) - u0.min(u1)).abs() < 1e-6);
        let pts = &d.model_lines_3d(|_| false)[0].2;
        let xs: Vec<f64> = pts.iter().map(|p| p[0]).collect();
        assert!((xs.iter().cloned().fold(f64::INFINITY, f64::min) - 1000.0).abs() < 1e-6 && pts.iter().all(|p| (p[1] - 1000.0).abs() < 1e-6 && (p[2] - 2000.0).abs() < 1e-6));
        // not in the plan, nor in the East elevation (not parallel)
        assert!(d.model_lines_drawn(plan, None).is_empty() && d.model_lines_drawn(east, None).is_empty());
        // the session again unchanged is no undo step; shortened it changes
        d.set_model_lines(south, vec![edit(Some(ids[0]), u0, u1)]).unwrap();
        d.set_model_lines(south, vec![edit(Some(ids[0]), u0, (u0 + u1) / 2.0)]).unwrap();
        assert!((d.model_line(ids[0]).unwrap().curve.length() - 1000.0).abs() < 1e-6);
        // deleting the plane takes its lines
        d.delete(&[ew]).unwrap();
        assert!(d.model_line(ids[0]).is_none());
        assert!(d.work_plane_of(south).is_none());
    }
}
