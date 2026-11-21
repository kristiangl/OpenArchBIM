//! Reference planes in the project (Architecture > Ref Plane): a named vertical plane through two points drawn in a
//! plan, for setting out. Every plan view shows it (as a dashed line between its ends) and the snaps find it.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::geom::Point;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefPlane {
    /// The ends as drawn (the plane runs through them, upright); model mm.
    pub a: Point,
    pub b: Point,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pinned: bool,
}

impl Document {
    pub fn ref_plane(&self, id: Id) -> Option<&RefPlane> {
        match self.get(id) {
            Some(Element::RefPlane(r)) => Some(r),
            _ => None,
        }
    }

    pub fn ref_planes(&self) -> Vec<(Id, &RefPlane)> {
        self.iter().filter_map(|(id, e)| match e { Element::RefPlane(r) => Some((id, r)), _ => None }).collect()
    }

    /// The reference planes a view shows: all of them in a plan, unless hidden there.
    pub fn ref_planes_in(&self, view: Id) -> Vec<(Id, &RefPlane)> {
        if !self.view(view).is_some_and(|v| v.view_kind.is_plan()) {
            return vec![];
        }
        self.ref_planes().into_iter().filter(|(id, _)| !self.hidden_in(view, *id)).collect()
    }

    pub fn add_ref_plane(&mut self, a: Point, b: Point, name: &str) -> Result<Id, String> {
        let r = RefPlane { a, b, name: name.trim().into(), pinned: false };
        self.check_ref_plane(None, &r)?;
        self.transact("Reference plane", |tx| Ok(tx.add(Element::RefPlane(r))))
    }

    /// Name, ends and Pinned.
    pub fn patch_ref_plane(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.ref_plane(id).cloned().ok_or("not a reference plane")?;
        let mut new: RefPlane = crate::doc::apply_patch(&old, patch)?;
        new.name = new.name.trim().into();
        if new == old {
            return Ok(());
        }
        self.check_ref_plane(Some(id), &new)?;
        self.transact("Change reference plane", |tx| tx.update(id, Element::RefPlane(new)))
    }

    fn check_ref_plane(&self, id: Option<Id>, r: &RefPlane) -> Result<(), String> {
        let len = (r.b.x - r.a.x).hypot(r.b.y - r.a.y);
        if !len.is_finite() || len < 1.0 {
            return Err("A reference plane needs some length.".into());
        }
        if !r.name.is_empty() && self.ref_planes().iter().any(|(i, o)| Some(*i) != id && o.name.eq_ignore_ascii_case(&r.name)) {
            return Err(format!("A reference plane named '{}' already exists.", r.name));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{EditOptions, Xform};

    #[test]
    fn reference_planes_show_in_every_plan_and_move_with_the_modify_tools() {
        let mut d = Document::new_project();
        let id = d.add_ref_plane(Point::new(0.0, 0.0), Point::new(0.0, 5000.0), " Setout A ").unwrap();
        assert_eq!(d.ref_plane(id).unwrap().name, "Setout A");
        let plans: Vec<Id> = d.views().into_iter().filter(|(_, v)| v.view_kind.is_plan()).map(|(i, _)| i).collect();
        assert!(plans.len() > 1 && plans.iter().all(|p| d.ref_planes_in(*p).len() == 1));
        let three = d.views().into_iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::ThreeD).unwrap().0;
        assert!(d.ref_planes_in(three).is_empty());
        assert!(d.add_ref_plane(Point::new(0.0, 0.0), Point::new(0.5, 0.0), "").is_err());
        assert!(d.add_ref_plane(Point::new(1000.0, 0.0), Point::new(1000.0, 5000.0), "setout a").is_err());
        // unnamed planes may share no name
        d.add_ref_plane(Point::new(1000.0, 0.0), Point::new(1000.0, 5000.0), "").unwrap();
        d.add_ref_plane(Point::new(2000.0, 0.0), Point::new(2000.0, 5000.0), "").unwrap();
        d.patch_ref_plane(id, &serde_json::json!({ "name": "Grid line 1" })).unwrap();
        assert_eq!(d.ref_plane(id).unwrap().name, "Grid line 1");
        let copies = d.transform_elements(&[id], Xform::Move { dx: 600.0, dy: 0.0 }, EditOptions { copy: true, disjoin: false }).unwrap();
        assert_eq!(d.ref_plane(copies[0]).unwrap().a.x, 600.0);
        // a copy of a named plane is left unnamed, as names are unique
        assert_eq!(d.ref_plane(copies[0]).unwrap().name, "");
        d.transform_elements(&[id], Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 }, EditOptions { copy: false, disjoin: false }).unwrap();
        assert!((d.ref_plane(id).unwrap().b.x + 5000.0).abs() < 1e-6);
        d.delete(&[id]).unwrap();
        assert!(d.ref_plane(id).is_none());
    }
}
