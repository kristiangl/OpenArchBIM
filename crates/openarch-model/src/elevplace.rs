//! Doors and windows placed in an elevation or section view.
//!
//! A click across the view (`u`) and up (`z`) is a line of sight into the model: the nearest straight wall it meets,
//! within the wall's height, hosts the opening at that point along it. The opening faces the viewer, and a window's
//! sill is set to the height clicked (a door stays on its floor).

use crate::doc::Document;
use crate::element::*;
use crate::family::FamilyCategory;
use crate::geom::Point;

impl Document {
    /// The wall a click in an elevation or section looks at, and the point on its location line in the plan.
    pub fn wall_in_elevation(&self, view: Id, u: f64, z: f64) -> Option<(Id, Point)> {
        let f = self.elevation_frame(view)?;
        let v = self.view(view)?;
        let (look, right) = (Point::new(f.look[0], f.look[1]), Point::new(f.right[0], f.right[1]));
        let r0 = Point::new(f.origin[0] + right.x * u, f.origin[1] + right.y * u);
        let mut best: Option<(f64, Id, Point)> = None;
        for (id, e) in self.iter() {
            let Element::Wall(w) = e else { continue };
            if w.curve.is_some() || v.hidden_elements.contains(&id) {
                continue;
            }
            let (z0, z1) = self.wall_extent(w);
            if z < z0 - 1.0 || z > z1 + 1.0 {
                continue;
            }
            let d = Point::new(w.end.x - w.start.x, w.end.y - w.start.y);
            let len = d.x.hypot(d.y);
            // a wall seen end on has no face to put an opening in
            if len < 1e-6 || (d.x * right.x + d.y * right.y).abs() / len < 0.2 {
                continue;
            }
            // start + s d = r0 + t look
            let det = d.x * (-look.y) - d.y * (-look.x);
            if det.abs() < 1e-9 {
                continue;
            }
            let (bx, by) = (r0.x - w.start.x, r0.y - w.start.y);
            let s = (bx * (-look.y) - by * (-look.x)) / det;
            let t = (d.x * by - d.y * bx) / det;
            // in a section the cut wall sits on the plane itself, so a little behind it still counts
            if !(0.0..=1.0).contains(&s) || t < -600.0 || t > f.depth_mm {
                continue;
            }
            if best.as_ref().is_none_or(|(bt, _, _)| t < *bt) {
                best = Some((t, id, Point::new(w.start.x + d.x * s, w.start.y + d.y * s)));
            }
        }
        best.map(|(_, id, p)| (id, p))
    }

    /// Place a door or window from a click in an elevation or section.
    pub fn place_in_elevation(&mut self, view: Id, type_id: Id, u: f64, z: f64, flip_hand: bool) -> Result<Id, String> {
        let (wall, p) = self.wall_in_elevation(view, u, z).ok_or("click on a wall seen in this view, within its height")?;
        let f = self.elevation_frame(view).ok_or("not an elevation or section view")?;
        let Some(Element::Wall(w)) = self.get(wall).cloned() else { return Err("not a wall".into()) };
        // face the viewer: the wall's exterior (its left) towards the one looking
        let (dx, dy) = (w.end.x - w.start.x, w.end.y - w.start.y);
        let left_to_viewer = -dy * -f.look[0] + dx * -f.look[1] > 0.0;
        let window = self.family_type(type_id).and_then(|t| self.family(t.family)).is_some_and(|fam| fam.category == FamilyCategory::Windows);
        self.grouped("Place", |d| {
            let id = d.place_on_wall_flipped(type_id, wall, p, flip_hand, !left_to_viewer)?;
            if window {
                if let Some(fr) = d.instance_frame(id) {
                    let dz = (z / 10.0).round() * 10.0 - fr.z0;
                    if dz.abs() > 0.5 {
                        d.move_vertical(&[id], dz)?;
                    }
                }
            }
            Ok(id)
        })
    }
}
