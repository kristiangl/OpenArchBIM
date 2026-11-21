//! Wall foundations (Structure > Strip Footing, Revit's Wall Foundation): a strip footing under a wall, from a type
//! that sets its width, thickness and material. It follows its wall (along its location line, centred on the wall body
//! less any eccentricity, its top at the wall's base) and goes when the wall is deleted. Ends that meet another wall
//! run on half the footing's width so corners and tees close. Plans of the wall's level show it dashed (it is below
//! the cut), 3D views, elevations and sections draw it in the Structural Foundations category.

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::doc::Document;
use crate::element::{Element, Id};
use crate::geom::Point;

fn d_width() -> f64 {
    600.0
}
fn d_thick() -> f64 {
    300.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallFoundationType {
    pub name: String,
    #[serde(default = "d_width")]
    pub width_mm: f64,
    #[serde(default = "d_thick")]
    pub thickness_mm: f64,
    #[serde(default)]
    pub material: String,
    /// How far the footing runs past a free end of its wall.
    #[serde(default)]
    pub end_extension_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallFoundation {
    pub wall: Id,
    pub foundation_type: Id,
    /// Off the wall's centre, to the left looking from the wall's start (mm).
    #[serde(default)]
    pub eccentricity_mm: f64,
    #[serde(default)]
    pub pinned: bool,
}

impl Document {
    pub fn wall_foundation_types(&self) -> Vec<(Id, &WallFoundationType)> {
        self.iter().filter_map(|(id, e)| match e { Element::WallFoundationType(t) => Some((id, t)), _ => None }).collect()
    }
    pub fn wall_foundation_type(&self, id: Id) -> Option<&WallFoundationType> {
        match self.get(id) { Some(Element::WallFoundationType(t)) => Some(t), _ => None }
    }
    pub fn wall_foundation(&self, id: Id) -> Option<&WallFoundation> {
        match self.get(id) { Some(Element::WallFoundation(f)) => Some(f), _ => None }
    }
    pub fn wall_foundations(&self) -> Vec<(Id, &WallFoundation)> {
        self.iter().filter_map(|(id, e)| match e { Element::WallFoundation(f) => Some((id, f)), _ => None }).collect()
    }
    /// The footing under a wall, if it has one.
    pub fn foundation_of_wall(&self, wall: Id) -> Option<Id> {
        self.wall_foundations().into_iter().find(|(_, f)| f.wall == wall).map(|(id, _)| id)
    }

    fn concrete(&self) -> String {
        let names: Vec<String> = self.materials().into_iter().map(|(_, m)| m.name.clone()).collect();
        names.iter().find(|n| n.eq_ignore_ascii_case("Concrete")).or_else(|| names.iter().find(|n| n.to_lowercase().contains("concrete"))).cloned().unwrap_or_default()
    }

    /// The project's first wall foundation type, made when there is none (Bearing Footing - 600 x 300, concrete).
    fn ensure_wall_foundation_type(&mut self) -> Result<Id, String> {
        if let Some((id, _)) = self.wall_foundation_types().first() {
            return Ok(*id);
        }
        let t = WallFoundationType { name: "Bearing Footing - 600 x 300".into(), width_mm: 600.0, thickness_mm: 300.0, material: self.concrete(), end_extension_mm: 0.0 };
        self.transact("Wall foundation type", |tx| Ok(tx.add(Element::WallFoundationType(t))))
    }

    /// Strip footings under walls, one each (walls that have one already are left as they are), one undo step.
    pub fn add_wall_foundations(&mut self, walls: &[Id], foundation_type: Option<Id>) -> Result<Vec<Id>, String> {
        let todo: Vec<Id> = walls.iter().copied().filter(|w| matches!(self.get(*w), Some(Element::Wall(_))) && self.foundation_of_wall(*w).is_none()).collect();
        if todo.is_empty() {
            return Err(if walls.iter().any(|w| matches!(self.get(*w), Some(Element::Wall(_)))) { "That wall has a footing already.".into() } else { "Strip footings go under walls: pick a wall.".into() });
        }
        if let Some(t) = foundation_type {
            if self.wall_foundation_type(t).is_none() {
                return Err("not a wall foundation type".into());
            }
        }
        self.grouped("Wall foundation", |d| {
            let t = match foundation_type { Some(t) => t, None => d.ensure_wall_foundation_type()? };
            d.transact("Wall foundation", |tx| Ok(todo.iter().map(|w| tx.add(Element::WallFoundation(WallFoundation { wall: *w, foundation_type: t, eccentricity_mm: 0.0, pinned: false }))).collect()))
        })
    }

    /// Type, Eccentricity and Pinned; a footing stays under its wall.
    pub fn patch_wall_foundation(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.wall_foundation(id).cloned().ok_or("not a wall foundation")?;
        let new: WallFoundation = crate::doc::apply_patch(&old, patch)?;
        if new == old {
            return Ok(());
        }
        if new.wall != old.wall {
            return Err("A footing stays under its wall.".into());
        }
        if self.wall_foundation_type(new.foundation_type).is_none() {
            return Err("not a wall foundation type".into());
        }
        if !new.eccentricity_mm.is_finite() || new.eccentricity_mm.abs() > 5000.0 {
            return Err("Eccentricity is up to 5000 mm either way.".into());
        }
        self.transact("Change wall foundation", |tx| tx.update(id, Element::WallFoundation(new)))
    }

    pub fn patch_wall_foundation_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.wall_foundation_type(id).cloned().ok_or("not a wall foundation type")?;
        let mut new: WallFoundationType = crate::doc::apply_patch(&old, patch)?;
        new.name = new.name.trim().into();
        if new == old {
            return Ok(());
        }
        self.check_wall_foundation_type(Some(id), &new)?;
        self.transact("Change wall foundation type", |tx| tx.update(id, Element::WallFoundationType(new)))
    }

    pub fn duplicate_wall_foundation_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let t = WallFoundationType { name: name.trim().into(), ..self.wall_foundation_type(id).cloned().ok_or("not a wall foundation type")? };
        self.check_wall_foundation_type(None, &t)?;
        self.transact("Duplicate wall foundation type", |tx| Ok(tx.add(Element::WallFoundationType(t))))
    }

    fn check_wall_foundation_type(&self, id: Option<Id>, t: &WallFoundationType) -> Result<(), String> {
        if t.name.is_empty() {
            return Err("A type needs a name.".into());
        }
        if self.wall_foundation_types().iter().any(|(i, o)| Some(*i) != id && o.name == t.name) {
            return Err(format!("A wall foundation type named '{}' already exists.", t.name));
        }
        let ok = |v: f64| v.is_finite() && v > 0.0 && v <= 10_000.0;
        if !ok(t.width_mm) || !ok(t.thickness_mm) {
            return Err("Width and thickness are more than 0 and up to 10 m.".into());
        }
        if !t.end_extension_mm.is_finite() || !(0.0..=10_000.0).contains(&t.end_extension_mm) {
            return Err("End extension is 0 to 10 m.".into());
        }
        if !t.material.is_empty() && self.material(&t.material).is_none() {
            return Err(format!("There is no material named '{}'.", t.material));
        }
        Ok(())
    }

    /// A footing's outline in plan, and the heights of its bottom and top (mm). None when its wall or type is gone.
    pub fn wall_foundation_shape(&self, id: Id) -> Option<(Vec<Point>, f64, f64)> {
        let f = self.wall_foundation(id)?;
        let Some(Element::Wall(w)) = self.get(f.wall) else { return None };
        let t = self.wall_foundation_type(f.foundation_type)?;
        let (l, r) = self.wall_extents(w);
        let off = (l - r) / 2.0 + f.eccentricity_mm;
        let half = t.width_mm / 2.0;
        // an end that meets another wall runs on half the width (so corners and tees close), a free end by the type's
        // end extension
        let touches = |p: Point| {
            self.iter().any(|(oid, e)| match e {
                Element::Wall(o) if oid != f.wall => {
                    let reach = t.width_mm.max(self.wall_extents(o).0 + self.wall_extents(o).1);
                    o.path().pts.windows(2).any(|s| crate::geom::dist_to_segment(p, s[0], s[1]) <= reach / 2.0 + 1.0)
                }
                _ => false,
            })
        };
        let ext0 = if touches(w.start) { half } else { t.end_extension_mm };
        let ext1 = if touches(w.end) { half } else { t.end_extension_mm };
        let (base, _) = self.wall_extent(w);
        // the centre line of the footing, extended at its ends, then its two sides
        let centre: Vec<Point> = match &w.curve {
            None => {
                let len = w.start.dist(&w.end);
                if len < 1.0 {
                    return None;
                }
                let d = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
                vec![Point::new(w.start.x - d.x * ext0, w.start.y - d.y * ext0), Point::new(w.end.x + d.x * ext1, w.end.y + d.y * ext1)]
            }
            Some(_) => {
                let mut pts = w.path().pts;
                if pts.len() < 2 {
                    return None;
                }
                let n = pts.len();
                let run = |a: Point, b: Point, k: f64| { let l = a.dist(&b).max(1e-9); Point::new(a.x + (a.x - b.x) / l * k, a.y + (a.y - b.y) / l * k) };
                pts[0] = run(pts[0], pts[1], ext0);
                pts[n - 1] = run(pts[n - 1], pts[n - 2], ext1);
                pts
            }
        };
        let side = |o: f64| -> Vec<Point> {
            (0..centre.len() as usize)
                .map(|i: usize| {
                    let a = centre[i.saturating_sub(1)];
                    let b = centre[(i + 1).min(centre.len() - 1)];
                    let l = a.dist(&b).max(1e-9);
                    let n = Point::new(-(b.y - a.y) / l, (b.x - a.x) / l);
                    Point::new(centre[i].x + n.x * o, centre[i].y + n.y * o)
                })
                .collect()
        };
        let mut ring = side(off + half);
        ring.extend(side(off - half).into_iter().rev());
        Some((ring, base - t.thickness_mm, base))
    }

    /// The footings a plan shows: those under walls standing on its level.
    pub fn wall_foundations_in(&self, view: Id) -> Vec<(Id, Vec<Point>)> {
        let Some(v) = self.view(view) else { return vec![] };
        if !v.view_kind.is_plan() {
            return vec![];
        }
        let Some(level) = v.level else { return vec![] };
        self.wall_foundations()
            .into_iter()
            .filter(|(id, f)| !self.hidden_in(view, *id) && matches!(self.get(f.wall), Some(Element::Wall(w)) if w.base_level == level))
            .filter_map(|(id, _)| self.wall_foundation_shape(id).map(|(r, _, _)| (id, r)))
            .collect()
    }

    /// The footings as solids, for 3D views, elevations, sections and IFC (with the structural members).
    pub fn foundation_meshes(&self) -> Vec<(Id, &'static str, [u8; 3], Vec<f32>)> {
        self.wall_foundations()
            .into_iter()
            .filter_map(|(id, f)| {
                let (ring, z0, z1) = self.wall_foundation_shape(id)?;
                let colour = self.wall_foundation_type(f.foundation_type).and_then(|t| self.material(&t.material)).map_or([190, 190, 190], |m| m.graphics.shading_color);
                Some((id, "WallFoundation", colour, crate::mesh::solid_mesh(&ring, z0, z1)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_footings_follow_their_walls_and_close_at_corners() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types()[1].0;
        let a = d.add_wall(wt, lvl, Point::new(0.0, 0.0), Point::new(4000.0, 0.0), 2700.0).unwrap();
        let b = d.add_wall(wt, lvl, Point::new(4000.0, 0.0), Point::new(4000.0, 3000.0), 2700.0).unwrap();
        let ids = d.add_wall_foundations(&[a, b], None).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(d.add_wall_foundations(&[a], None).is_err());
        assert!(d.add_wall_foundations(&[lvl], None).is_err());
        let (ring, z0, z1) = d.wall_foundation_shape(ids[0]).unwrap();
        let base = d.level(lvl).unwrap().elevation_mm;
        assert_eq!((z0, z1), (base - 300.0, base));
        // 600 wide about the wall's centre, the free start flush, the corner end run on 300
        let xs: Vec<f64> = ring.iter().map(|p| p.x).collect();
        let (lo, hi) = (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
        assert_eq!((lo.round(), hi.round()), (0.0, 4300.0));
        let ys: Vec<f64> = ring.iter().map(|p| p.y).collect();
        let span = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - ys.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!((span - 600.0).abs() < 1e-6);
        // the plan of the level shows both; a type change widens it
        let plan = d.views().into_iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::FloorPlan && v.level == Some(lvl)).unwrap().0;
        assert_eq!(d.wall_foundations_in(plan).len(), 2);
        let t = d.wall_foundations_types_first();
        let wide = d.duplicate_wall_foundation_type(t, "Bearing Footing - 900 x 400").unwrap();
        d.patch_wall_foundation_type(wide, &serde_json::json!({ "width_mm": 900.0, "thickness_mm": 400.0 })).unwrap();
        d.patch_wall_foundation(ids[0], &serde_json::json!({ "foundation_type": wide, "eccentricity_mm": 100.0 })).unwrap();
        let (_, z0, _) = d.wall_foundation_shape(ids[0]).unwrap();
        assert_eq!(z0, base - 400.0);
        assert!(d.duplicate_wall_foundation_type(t, "Bearing Footing - 900 x 400").is_err());
        assert!(d.patch_wall_foundation_type(t, &serde_json::json!({ "width_mm": 0.0 })).is_err());
        assert_eq!(d.foundation_meshes().len(), 2);
        // moving the wall takes the footing; deleting it takes it away
        let low = |d: &Document| d.wall_foundation_shape(ids[0]).unwrap().0.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let before = low(&d);
        d.transform_elements(&[a], crate::edit::Xform::Move { dx: 0.0, dy: -500.0 }, crate::edit::EditOptions { copy: false, disjoin: false }).unwrap();
        assert!((low(&d) - (before - 500.0)).abs() < 1e-6);
        // on its own it does not move
        assert!(d.transform_elements(&[ids[0]], crate::edit::Xform::Move { dx: 0.0, dy: 100.0 }, crate::edit::EditOptions { copy: false, disjoin: false }).is_err());
        d.delete(&[b]).unwrap();
        assert!(d.wall_foundation(ids[1]).is_none());
    }

    impl Document {
        fn wall_foundations_types_first(&self) -> Id {
            self.wall_foundation_types()[0].0
        }
    }
}
