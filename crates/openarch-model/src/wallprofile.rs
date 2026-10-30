//! Edit Profile: a straight wall's elevation outline sketched by hand in an elevation or section view. The sketch
//! starts from the wall as it is, its bottom edge on the base constraint and its top edge on the top constraint
//! (with their offsets), and its ends at the wall's ends. The outline it leaves is kept on the wall in the wall's own
//! terms, along it from its start and up from its base, so it moves with the wall. Points that were on the wall's
//! top or far end when it was sketched stay on them: change the top constraint or the length and the outline follows
//! (and a top or base attached to something will move those edges the same way once walls attach). Loops inside the
//! outline are openings through the wall. Doors and windows cut through it as before. Reset Profile takes it off.
//!
//! The plan cuts a profiled wall where the outline crosses the cut plane; elevations and sections draw its outline;
//! 3D extrudes the outline across each layer, the ends following the wall's joins.

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::doc::Document;
use crate::element::*;
use crate::geom::{signed_area, Point};
use crate::layers::{Ring, Shape};

/// A wall's sketched outline: closed loops of curves, along the wall from its start (x) and up from its base (y).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallProfile {
    pub loops: Vec<Vec<Curve>>,
    /// The wall's length and height when it was sketched: points on its far end or its top follow them.
    pub length_mm: f64,
    pub height_mm: f64,
}

/// A view's horizontal: the view's `u` at a point in plan is `(p - origin) . right`.
struct Across {
    origin: Point,
    right: Point,
}

impl Document {
    /// The wall's outline as it is now: one shape per region (outer ring counter-clockwise, then its holes), along
    /// the wall and up from its base. None for a wall without a sketched profile.
    pub fn wall_profile_shapes(&self, w: &Wall) -> Option<Vec<Shape>> {
        let p = w.profile.as_ref()?;
        let (z0, z1) = self.wall_extent(w);
        let (len, h) = (w.length(), z1 - z0);
        let follow = |q: Point| Point::new(if (q.x - p.length_mm).abs() < 1.0 { len } else { q.x }, if (q.y - p.height_mm).abs() < 1.0 { h } else { q.y });
        let polys: Vec<Vec<Point>> = p.loops.iter().map(|l| crate::curve::loop_polygon(l, 1.0).into_iter().map(follow).collect()).collect();
        Some(crate::geom::regions(&polys).into_iter().map(|r| std::iter::once(r.outer).chain(r.holes).collect()).collect())
    }

    /// The outline less the doors and windows cut in it, along the wall and up from its base.
    pub fn wall_profile_solid(&self, id: Id, w: &Wall) -> Option<Vec<Shape>> {
        let shapes = self.wall_profile_shapes(w)?;
        let (z0, _) = self.wall_extent(w);
        let holes: Vec<Ring> = self.wall_openings(id).iter().map(|o| vec![Point::new(o.u0, o.z0 - z0), Point::new(o.u1, o.z0 - z0), Point::new(o.u1, o.z1 - z0), Point::new(o.u0, o.z1 - z0)]).collect();
        Some(crate::layers::subtract_shapes(&shapes, &holes))
    }

    /// The heights a wall reaches: its outline's when it has one, else its base and top.
    pub fn wall_z_range(&self, w: &Wall) -> (f64, f64) {
        let (z0, z1) = self.wall_extent(w);
        match self.wall_profile_shapes(w) {
            Some(shapes) => {
                let ys = shapes.iter().flat_map(|s| s.first().into_iter().flatten()).map(|p| p.y);
                let (lo, hi) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| (a.min(y), b.max(y)));
                if lo.is_finite() { (z0 + lo, z0 + hi) } else { (z0, z1) }
            }
            None => (z0, z1),
        }
    }

    /// Where along the wall it is not there at height `z` (absolute), by its outline: sorted stretches inside its
    /// length. Empty for a wall without a profile.
    pub fn wall_profile_gaps(&self, w: &Wall, z: f64) -> Vec<(f64, f64)> {
        let Some(shapes) = self.wall_profile_shapes(w) else { return vec![] };
        let (z0, _) = self.wall_extent(w);
        let len = w.length();
        let covered = row(&shapes, z - z0);
        let mut gaps = vec![];
        let mut at = 0.0;
        for (a, b) in covered {
            if a > at + 1e-6 {
                gaps.push((at, a.min(len)));
            }
            at = at.max(b);
        }
        if at < len - 1e-6 {
            gaps.push((at, len));
        }
        gaps.retain(|(a, b)| b - a > 1e-6);
        gaps
    }

    /// What a section cutting a wall at `along` (from its start) passes through, by its outline: the heights
    /// (absolute), bottom up. None for a wall without a profile.
    pub fn wall_profile_column(&self, w: &Wall, along: f64) -> Option<Vec<(f64, f64)>> {
        let shapes = self.wall_profile_shapes(w)?;
        let (z0, _) = self.wall_extent(w);
        Some(column(&shapes, along).into_iter().map(|(a, b)| (z0 + a, z0 + b)).collect())
    }

    /// Where a plan cut at `cut` opens a wall: its doors and windows spanning that height and the gaps in its outline,
    /// sorted and merged.
    pub fn wall_cut_gaps(&self, id: Id, cut: f64) -> Vec<(f64, f64)> {
        let mut all: Vec<(f64, f64)> = self.wall_openings(id).into_iter().filter(|o| o.z0 < cut && cut < o.z1).map(|o| (o.u0, o.u1)).collect();
        if let Some(Element::Wall(w)) = self.get(id) {
            all.extend(self.wall_profile_gaps(w, cut));
        }
        merge(all)
    }

    /// The area of the wall's face: its outline less the openings in it (mm²). None for a wall without a profile.
    pub fn wall_profile_area(&self, id: Id, w: &Wall) -> Option<f64> {
        Some(self.wall_profile_solid(id, w)?.iter().map(|s| s.iter().map(|r| signed_area(r)).sum::<f64>()).sum())
    }

    /// How a view sees a wall's face: the view's `u` of the wall's start, how much `u` moves per mm along the wall,
    /// and the wall's base height. Edit Profile needs a straight wall that the view looks at square on.
    fn profile_view(&self, id: Id, view: Id) -> Result<(Wall, f64, f64, f64), String> {
        let Some(Element::Wall(w)) = self.get(id).cloned() else { return Err("not a wall".into()) };
        if w.curve.is_some() {
            return Err("Edit Profile works on straight walls.".into());
        }
        let f = self.elevation_frame(view).ok_or("Edit Profile works in an elevation or section view.")?;
        let a = Across { origin: Point::new(f.origin[0], f.origin[1]), right: Point::new(f.right[0], f.right[1]) };
        let len = w.length().max(1e-9);
        let dir = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
        let s = dir.x * a.right.x + dir.y * a.right.y;
        if s.abs() < 0.999 {
            return Err("Edit Profile needs a view that looks square at the wall: open an elevation or section parallel to it.".into());
        }
        let u0 = (w.start.x - a.origin.x) * a.right.x + (w.start.y - a.origin.y) * a.right.y;
        let (z0, _) = self.wall_extent(&w);
        Ok((w, u0, s.signum(), z0))
    }

    /// The sketch Edit Profile starts from, in the view's terms (`x` its horizontal, `y` height): the wall's outline,
    /// or its rectangle from base to top constraint and end to end. Also where the base and top constraints are.
    pub fn wall_profile_sketch(&self, id: Id, view: Id) -> Result<(Vec<Curve>, f64, f64), String> {
        let (w, u0, s, z0) = self.profile_view(id, view)?;
        let (_, z1) = self.wall_extent(&w);
        let (len, h) = (w.length(), z1 - z0);
        let loops: Vec<Vec<Curve>> = match &w.profile {
            Some(p) => p.loops.clone(),
            None => {
                let c = [Point::new(0.0, 0.0), Point::new(len, 0.0), Point::new(len, h), Point::new(0.0, h)];
                vec![(0..4).map(|i| Curve::line(c[i], c[(i + 1) % 4])).collect()]
            }
        };
        let follow = |q: Point| match &w.profile {
            Some(p) => Point::new(if (q.x - p.length_mm).abs() < 1.0 { len } else { q.x }, if (q.y - p.height_mm).abs() < 1.0 { h } else { q.y }),
            None => q,
        };
        let to_view = to_view(u0, s, z0);
        let curves = loops.iter().flatten().map(|c| moved_points(c, follow).transform(to_view.0).transform(to_view.1)).collect();
        Ok((curves, z0, z1))
    }

    /// Edit Profile's Finish: the sketched curves (in the view's terms) become the wall's outline. One undo step.
    pub fn set_wall_profile(&mut self, id: Id, view: Id, curves: &[Curve]) -> Result<(), String> {
        let (mut w, u0, s, z0) = self.profile_view(id, view)?;
        let loops = crate::curve::sketch_loops(curves).map_err(|e| e.message)?;
        let (from_mirror, from_move) = from_view(u0, s, z0);
        let loops: Vec<Vec<Curve>> = loops.iter().map(|l| l.iter().map(|c| c.transform(from_mirror).transform(from_move)).collect()).collect();
        // one outline, everything else inside it
        let polys: Vec<Vec<Point>> = loops.iter().map(|l| crate::curve::loop_polygon(l, 1.0)).collect();
        let regions = crate::geom::regions(&polys);
        if regions.len() != 1 {
            return Err("A wall's profile is one closed outline, with any openings drawn inside it.".into());
        }
        let (_, z1) = self.wall_extent(&w);
        w.profile = Some(WallProfile { loops, length_mm: w.length(), height_mm: z1 - z0 });
        self.transact("Edit Profile", |tx| tx.update(id, Element::Wall(w)))
    }

    /// Reset Profile: the wall goes back to its rectangle. One undo step.
    pub fn reset_wall_profile(&mut self, id: Id) -> Result<(), String> {
        let Some(Element::Wall(mut w)) = self.get(id).cloned() else { return Err("not a wall".into()) };
        if w.profile.is_none() {
            return Err("The wall has no edited profile.".into());
        }
        w.profile = None;
        self.transact("Reset Profile", |tx| tx.update(id, Element::Wall(w)))
    }
}

/// The wall's terms to the view's: turned round when the wall runs right to left in the view, then moved to its start
/// and base.
fn to_view(u0: f64, s: f64, z0: f64) -> (crate::edit::Xform, crate::edit::Xform) {
    use crate::edit::Xform;
    let flip = if s < 0.0 { Xform::Mirror { ax: 0.0, ay: 0.0, bx: 0.0, by: 1.0 } } else { Xform::Move { dx: 0.0, dy: 0.0 } };
    (flip, Xform::Move { dx: u0, dy: z0 })
}

/// The view's terms to the wall's.
fn from_view(u0: f64, s: f64, z0: f64) -> (crate::edit::Xform, crate::edit::Xform) {
    use crate::edit::Xform;
    let flip = if s < 0.0 { Xform::Mirror { ax: u0, ay: 0.0, bx: u0, by: 1.0 } } else { Xform::Move { dx: 0.0, dy: 0.0 } };
    (flip, Xform::Move { dx: -u0, dy: -z0 })
}

/// A curve with its end points (and a line's or spline's points) moved by `f`; arcs keep their centre where both
/// ends move together.
fn moved_points(c: &Curve, f: impl Fn(Point) -> Point) -> Curve {
    match c {
        Curve::Line { a, b } => Curve::Line { a: f(*a), b: f(*b) },
        Curve::Spline { points, closed } => Curve::Spline { points: points.iter().map(|p| f(*p)).collect(), closed: *closed },
        Curve::Arc { start, end, centre, ccw } => {
            let (s2, e2) = (f(*start), f(*end));
            let d = Point::new(s2.x - start.x, s2.y - start.y);
            if (e2.x - end.x - d.x).abs() < 1e-6 && (e2.y - end.y - d.y).abs() < 1e-6 {
                Curve::Arc { start: s2, end: e2, centre: Point::new(centre.x + d.x, centre.y + d.y), ccw: *ccw }
            } else {
                Curve::line(s2, e2)
            }
        }
        other => other.clone(),
    }
}

/// Where shapes cover the horizontal line at height `y`: sorted stretches of x, run together.
pub(crate) fn row(shapes: &[Shape], y: f64) -> Vec<(f64, f64)> {
    merge(shapes.iter().flat_map(|s| crossings(s, y)).collect())
}

/// Where shapes cover the upright line at `x`: sorted stretches of y, run together.
pub(crate) fn column(shapes: &[Shape], x: f64) -> Vec<(f64, f64)> {
    let turned: Vec<Shape> = shapes.iter().map(|s| s.iter().map(|r| r.iter().map(|p| Point::new(p.y, p.x)).collect()).collect()).collect();
    row(&turned, x)
}

/// Where a shape (outer ring and holes) covers the horizontal line at height `y`: sorted stretches of x.
fn crossings(shape: &Shape, y: f64) -> Vec<(f64, f64)> {
    let mut xs: Vec<f64> = vec![];
    for ring in shape {
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            // half-open in y, so a vertex on the line counts once
            if (a.y <= y) != (b.y <= y) {
                xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect()
}

/// Stretches sorted and run together where they touch or overlap.
fn merge(mut v: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = vec![];
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 + 1e-6 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_named(d: &Document, name: &str) -> Id {
        d.views().into_iter().find(|(_, v)| v.name == name).unwrap().0
    }

    /// A gable: the eaves at the wall's top, a ridge 4000 up in the middle, and a window-sized hole.
    fn gable(len: f64, h: f64, u0: f64, s: f64) -> Vec<Curve> {
        let v = |x: f64, y: f64| Point::new(u0 + s * x, y);
        let outer = [v(0.0, 0.0), v(len, 0.0), v(len, h), v(len / 2.0, 4000.0), v(0.0, h)];
        let hole = [v(1000.0, 1000.0), v(1600.0, 1000.0), v(1600.0, 1600.0), v(1000.0, 1600.0)];
        let lines = |r: &[Point]| (0..r.len()).map(|i| Curve::line(r[i], r[(i + 1) % r.len()])).collect::<Vec<_>>();
        lines(&outer).into_iter().chain(lines(&hole)).collect()
    }

    #[test]
    fn a_wall_takes_an_edited_profile() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        let south = view_named(&d, "South");
        let f = d.elevation_frame(south).unwrap();
        // drawn left to right and right to left as the South elevation sees them
        for (start, end) in [(Point::new(0.0, 0.0), Point::new(5000.0, 0.0)), (Point::new(5000.0, -3000.0), Point::new(0.0, -3000.0))] {
            let w = d.add_wall(wt, lvl, start, end, 2700.0).unwrap();
            let wall = |d: &Document| match d.get(w) {
                Some(Element::Wall(x)) => x.clone(),
                _ => unreachable!(),
            };
            // the sketch starts as the wall's rectangle, in the view's terms
            let (curves, base, top) = d.wall_profile_sketch(w, south).unwrap();
            assert_eq!((curves.len(), base, top), (4, 0.0, 2700.0));
            let u_of = |p: Point| (p.x - f.origin[0]) * f.right[0] + (p.y - f.origin[1]) * f.right[1];
            let (u0, s) = (u_of(start), (u_of(end) - u_of(start)).signum());
            assert!(curves.iter().any(|c| (c.start().x - u0).abs() < 1e-6 && c.start().y == 0.0));
            d.set_wall_profile(w, south, &gable(5000.0, 2700.0, u0, s)).unwrap();
            let x = wall(&d);
            assert_eq!(d.wall_z_range(&x), (0.0, 4000.0));
            // plan: cut at 1200 the hole opens it; at 3500 only the middle stands
            let gaps = d.wall_profile_gaps(&x, 1200.0);
            assert_eq!(gaps, vec![(1000.0, 1600.0)]);
            let gaps = d.wall_profile_gaps(&x, 3500.0);
            assert_eq!(gaps.len(), 2);
            // the slopes cross 3500 at 2500 x 800/1300 from each end
            let slope = 2500.0 * 800.0 / 1300.0;
            assert!(gaps[0].0 == 0.0 && (gaps[0].1 - slope).abs() < 1e-6 && (gaps[1].0 - (5000.0 - slope)).abs() < 1e-6 && gaps[1].1 == 5000.0, "{gaps:?}");
            // area: the gable less the hole
            let area = 5000.0 * 2700.0 + 5000.0 * 1300.0 / 2.0 - 600.0 * 600.0;
            assert!((d.wall_profile_area(w, &x).unwrap() - area).abs() < 1.0);
            // 3D reaches the ridge
            let layers = d.wall_layer_geometry();
            let meshes = crate::mesh::wall_layer_meshes(&d, w, &x, &layers[&w]);
            let zmax = meshes.iter().flat_map(|m| m.verts.chunks(6).map(|v| v[2])).fold(0.0f32, f32::max);
            assert!((zmax - 4.0).abs() < 1e-3, "{zmax}");
            // a section across the ridge cuts the wall up to it, less the hole beside it
            if start.y == 0.0 {
                let sec = d.add_section(lvl, Point::new(2500.0, -1500.0), Point::new(2500.0, 1500.0), false).unwrap();
                let sv = d.section(sec).unwrap().view.unwrap();
                let e = d.elevation_data(sv).unwrap();
                let tops: Vec<f64> = e.items.iter().filter(|i| i.id == w && i.cut).flat_map(|i| i.polys.iter().flatten().map(|q| q[1])).collect();
                assert!(!tops.is_empty() && (tops.iter().cloned().fold(0.0, f64::max) - 4000.0).abs() < 1e-3, "{tops:?}");
                assert_eq!(d.wall_profile_column(&x, 1300.0).unwrap(), vec![(0.0, 1000.0), (1600.0, 2700.0 + 1300.0 * 1300.0 / 2500.0)]);
            }
            // a taller wall: the eaves follow the top, the ridge stays
            d.set_wall_height(w, 3000.0).unwrap();
            let x = wall(&d);
            assert_eq!(d.wall_profile_gaps(&x, 2900.0), vec![]);
            assert_eq!(d.wall_z_range(&x), (0.0, 4000.0));
            // the sketch comes back as it was drawn, the eaves at the new top
            let (curves, _, _) = d.wall_profile_sketch(w, south).unwrap();
            assert_eq!(curves.len(), 9);
            assert!(curves.iter().any(|c| (c.start().y - 3000.0).abs() < 1e-6));
            // reset
            d.reset_wall_profile(w).unwrap();
            assert!(wall(&d).profile.is_none());
            assert!(d.reset_wall_profile(w).is_err());
            d.undo().unwrap();
            assert!(wall(&d).profile.is_some());
        }
        // an outline with a loop outside it, and a wall the view sees end on, are refused
        let w = d.add_wall(wt, lvl, Point::new(0.0, 5000.0), Point::new(4000.0, 5000.0), 2700.0).unwrap();
        let mut two = gable(4000.0, 2700.0, 0.0, 1.0);
        two.truncate(5);
        let far = [Point::new(6000.0, 0.0), Point::new(7000.0, 0.0), Point::new(7000.0, 1000.0)];
        two.extend((0..3).map(|i| Curve::line(far[i], far[(i + 1) % 3])));
        assert!(d.set_wall_profile(w, south, &two).is_err());
        let side = d.add_wall(wt, lvl, Point::new(9000.0, 0.0), Point::new(9000.0, 4000.0), 2700.0).unwrap();
        assert!(d.wall_profile_sketch(side, south).is_err());
        // IFC: the profile swept across the wall
        let f = d.export_ifc("t");
        assert!(f.contains("IFCDIRECTION((0.,-1.,0.))"));
    }
}
