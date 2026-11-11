//! Shape editing of flat roofs: points (plan position and height above the roof's base, where its top is when
//! flat) and split lines, as floors and toposolids have (shape.rs is shared). A roof with any is shaped: its top is
//! the triangulated surface through its footprint corners (at the base), the points and along the split lines,
//! which is how falls to outlets and crickets are set out. With a variable layer in its type the underside stays
//! flat (tapered insulation over a level deck); without one the whole roof bends. A roof with an edge that defines
//! slope is pitched instead and is not shape edited.

use crate::roof::Roof;
use crate::shape::{within, Tin};
use crate::site::P3;
use crate::{Document, Element, Id, Point};

impl Roof {
    /// Whether the roof has been shape edited.
    pub fn shaped(&self) -> bool {
        !self.points.is_empty() || !self.split_lines.is_empty()
    }

    /// The footprint's loops as polygons (curves as chords within `tol` mm).
    pub fn polygons(&self, tol: f64) -> Vec<Vec<Point>> {
        self.loops().iter().map(|l| {
            let mut ring: Vec<Point> = vec![];
            for e in l {
                let t = e.curve.tessellate(tol);
                ring.extend_from_slice(&t[..t.len().saturating_sub(1)]);
            }
            ring
        }).filter(|r| r.len() >= 3).collect()
    }
}

/// The footprint tolerance shape editing works to (the roof's own geometry uses the same).
const TOL: f64 = 2.0;

impl Document {
    /// A shaped roof's top surface with its top at `base` when flat; None for a roof that isn't shaped.
    pub(crate) fn roof_shape_at(&self, r: &Roof, base: f64) -> Option<Tin> {
        if !r.shaped() || r.has_slope_edges() { return None; }
        let pts: Vec<[f64; 3]> = r.points.iter().map(|p| [p.x, p.y, base + p.z]).collect();
        crate::shape::triangulate(&r.polygons(TOL), &pts, &r.split_lines, base)
    }

    /// The base of a roof (absolute, mm): its level plus Base Offset From Level.
    pub fn roof_base_z(&self, r: &Roof) -> f64 {
        self.level(r.level).map_or(0.0, |l| l.elevation_mm) + r.base_offset_mm
    }

    /// A shaped roof's top surface.
    pub fn roof_shape(&self, r: &Roof) -> Option<Tin> {
        self.roof_shape_at(r, self.roof_base_z(r))
    }

    /// The underside's height under each corner of a shaped roof's top: flat at the base less the thickness with a
    /// variable layer (lower where the top dips so far that the other layers wouldn't fit), otherwise the top less
    /// the thickness.
    pub(crate) fn roof_bottoms(&self, r: &Roof, tin: &Tin, base: f64) -> Vec<f64> {
        let ty = self.roof_type(r.roof_type);
        let th = ty.map_or(0.0, |t| t.thickness_mm());
        let fixed = ty.and_then(|t| t.layers.iter().position(|l| l.variable).map(|v| th - t.layers[v].thickness_mm));
        let flat = base - th;
        tin.verts.iter().map(|v| match fixed { Some(k) => flat.min(v[2] - k), None => v[2] - th }).collect()
    }

    /// The edges of a shaped top where it folds (the two triangles either side lie in different planes), in plan.
    pub(crate) fn tin_folds(tin: &Tin) -> Vec<[Point; 2]> {
        let normal = |t: &[usize; 3]| {
            let (a, b, c) = (tin.verts[t[0]], tin.verts[t[1]], tin.verts[t[2]]);
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12) * n[2].signum();
            [n[0] / l, n[1] / l, n[2] / l]
        };
        tin.edges().into_iter().filter_map(|([i, j], ts)| {
            let [s, t] = ts.as_slice() else { return None };
            let (m, n) = (normal(&tin.tris[*s]), normal(&tin.tris[*t]));
            let apart = (m[0] - n[0]).abs() + (m[1] - n[1]).abs() + (m[2] - n[2]).abs();
            (apart > 1e-6).then(|| [Point::new(tin.verts[i][0], tin.verts[i][1]), Point::new(tin.verts[j][0], tin.verts[j][1])])
        }).collect()
    }

    fn flat_roof(&self, id: Id) -> Result<Roof, String> {
        let r = self.roof(id).cloned().ok_or("not a roof")?;
        if r.has_slope_edges() { return Err("A roof with edges that define slope is pitched, not shape edited: untick Defines Slope on every edge first (Edit Footprint).".into()); }
        Ok(r)
    }

    /// Modify Sub Elements: the roof's points and split lines replaced (plan positions, heights above the base;
    /// lines as pairs of indices). `lines` None keeps the lines that still fit. See `shape::tidy`.
    pub fn set_roof_points(&mut self, id: Id, points: Vec<P3>, lines: Option<Vec<[usize; 2]>>) -> Result<(), String> {
        let mut r = self.flat_roof(id)?;
        self.refuse_pinned(&[id], "edited")?;
        let rings = r.polygons(TOL);
        let lines = lines.unwrap_or_else(|| r.split_lines.iter().copied().filter(|l| l[0] < points.len() && l[1] < points.len()).collect());
        let (pts, ls) = crate::shape::tidy(&rings, points, lines, "roof")?;
        if pts == r.points && ls == r.split_lines { return Ok(()); }
        (r.points, r.split_lines) = (pts, ls);
        self.transact("Modify roof points", |tx| tx.update(id, Element::Roof(r)))
    }

    /// Add Point: a point at a plan position, `z` above the roof's base. Outside the footprint it is refused.
    pub fn add_roof_point(&mut self, id: Id, at: Point, z: f64) -> Result<(), String> {
        let mut r = self.flat_roof(id)?;
        self.refuse_pinned(&[id], "edited")?;
        if !(at.x.is_finite() && at.y.is_finite() && z.is_finite()) { return Err("the point must be numbers".into()); }
        let rings = r.polygons(TOL);
        if !within(&rings, at) { return Err("Click inside the roof's footprint.".into()); }
        let mut pts = r.points.clone();
        pts.push(P3 { x: at.x, y: at.y, z });
        (r.points, r.split_lines) = crate::shape::tidy(&rings, pts, r.split_lines.clone(), "roof")?;
        self.transact("Add point", |tx| tx.update(id, Element::Roof(r)))
    }

    /// Add Split Line: a line across the roof from `a` to `b`; each end not on a point becomes one at the
    /// surface's height there, and the roof then folds along the line.
    pub fn add_roof_split_line(&mut self, id: Id, a: Point, b: Point) -> Result<(), String> {
        let mut r = self.flat_roof(id)?;
        self.refuse_pinned(&[id], "edited")?;
        if a.dist(&b) < 1.0 { return Err("The split line needs two ends that are apart.".into()); }
        let rings = r.polygons(TOL);
        let base = self.roof_base_z(&r);
        let tin = self.roof_shape(&r);
        let mut pts = r.points.clone();
        let mut end = |q: Point| -> Result<usize, String> {
            if !within(&rings, q) { return Err("Both ends of a split line go inside the roof's footprint.".into()); }
            if let Some(i) = pts.iter().position(|p| (p.x - q.x).hypot(p.y - q.y) <= 1.0) { return Ok(i); }
            let z = tin.as_ref().and_then(|t| t.z_near(q)).map_or(0.0, |z| z - base);
            pts.push(P3 { x: q.x, y: q.y, z });
            Ok(pts.len() - 1)
        };
        let (i, j) = (end(a)?, end(b)?);
        let mut ls = r.split_lines.clone();
        ls.push([i, j]);
        (r.points, r.split_lines) = crate::shape::tidy(&rings, pts, ls, "roof")?;
        self.transact("Add split line", |tx| tx.update(id, Element::Roof(r)))
    }

    /// Reset Shape: the points and split lines go; the roof is flat again.
    pub fn reset_roof_shape(&mut self, id: Id) -> Result<(), String> {
        let mut r = self.roof(id).cloned().ok_or("not a roof")?;
        if !r.shaped() { return Err("The roof is already flat: it has no points or split lines.".into()); }
        self.refuse_pinned(&[id], "reshaped")?;
        r.points.clear();
        r.split_lines.clear();
        self.transact("Reset shape", |tx| tx.update(id, Element::Roof(r)))
    }

    /// The footprint of a roof after Edit Footprint: points and split lines outside the new footprint are dropped.
    pub(crate) fn keep_roof_points_within(r: &mut Roof) {
        if !r.shaped() { return; }
        let rings = r.polygons(TOL);
        (r.points, r.split_lines) = crate::shape::retain_within(&rings, &r.points, &r.split_lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;
    use crate::roof::RoofEdge;

    fn setup(slope: bool) -> (Document, Id) {
        let mut d = Document::new_project();
        d.ensure_roof_defaults();
        let rt = d.roof_types()[0].0;
        let lvl = d.levels()[0].0;
        let p = |x: f64, y: f64| Point::new(x, y);
        let sq = [p(0.0, 0.0), p(6000.0, 0.0), p(6000.0, 4000.0), p(0.0, 4000.0)];
        let edges = (0..4).map(|i| RoofEdge { defines_slope: slope, ..RoofEdge::new(Curve::line(sq[i], sq[(i + 1) % 4])) }).collect();
        let id = d.add_roof_sketch(rt, lvl, edges).unwrap();
        (d, id)
    }

    #[test]
    fn a_flat_roof_falls_to_an_outlet_and_folds_along_its_valleys() {
        let (mut d, id) = setup(false);
        let base = d.roof_base_z(d.roof(id).unwrap());
        let g = d.roof_geometry(d.roof(id).unwrap()).unwrap();
        assert!(g.shaped.is_none() && g.flat.len() == 1 && g.ridges.is_empty());
        // an outlet 60 below in the middle
        d.add_roof_point(id, Point::new(3000.0, 2000.0), -60.0).unwrap();
        let r = d.roof(id).unwrap().clone();
        let g = d.roof_geometry(&r).unwrap();
        let (tin, bot) = g.shaped.as_ref().expect("a shaped top");
        assert!(g.flat.is_empty() && g.faces.is_empty());
        assert!((tin.z_at(Point::new(3000.0, 2000.0)).unwrap() - (base - 60.0)).abs() < 1e-6);
        assert!((tin.z_at(Point::new(1500.0, 1000.0)).unwrap() - (base - 30.0)).abs() < 1e-6);
        // four valleys run from the corners to the outlet, drawn in plan
        assert_eq!(g.ridges.len(), 4, "{:?}", g.ridges);
        assert!((g.top_z - base).abs() < 1e-9);
        // no variable layer: the roof keeps its thickness
        let th = d.roof_type(r.roof_type).unwrap().thickness_mm();
        assert!(tin.verts.iter().zip(bot).all(|(v, b)| (v[2] - b - th).abs() < 1e-6));
        // the mesh follows (the outlet is its lowest top corner)
        let mesh = crate::mesh::roof_mesh(&d, &r).unwrap();
        let zs: Vec<f32> = mesh.chunks(6).map(|v| v[2]).collect();
        let lo = zs.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!((lo as f64 - (base - 60.0 - th) / 1000.0).abs() < 1e-3 || (lo as f64 - (base - 60.0 - th)).abs() < 1e-2, "{lo}");
        // a split line, then flat again and back by undo
        d.add_roof_split_line(id, Point::new(0.0, 2000.0), Point::new(6000.0, 2000.0)).unwrap();
        assert_eq!(d.roof(id).unwrap().split_lines.len(), 2);
        assert!(d.add_roof_point(id, Point::new(9000.0, 0.0), 0.0).is_err());
        d.reset_roof_shape(id).unwrap();
        assert!(!d.roof(id).unwrap().shaped());
        d.undo();
        assert!(d.roof(id).unwrap().shaped());
    }

    #[test]
    fn a_pitched_roof_is_not_shape_edited_and_a_shaped_one_takes_no_slope() {
        let (mut d, id) = setup(true);
        assert!(d.add_roof_point(id, Point::new(3000.0, 2000.0), 50.0).is_err());
        let (mut d2, id2) = setup(false);
        d2.add_roof_point(id2, Point::new(3000.0, 2000.0), 50.0).unwrap();
        let mut edges: Vec<RoofEdge> = d2.roof(id2).unwrap().edges().into_iter().cloned().collect();
        edges[0].defines_slope = true;
        assert!(d2.set_roof_sketch(id2, edges.clone()).is_err());
        // a smaller footprint drops the point outside it
        for e in edges.iter_mut() { e.defines_slope = false; }
        let p = |x: f64, y: f64| Point::new(x, y);
        let sq = [p(0.0, 0.0), p(2000.0, 0.0), p(2000.0, 4000.0), p(0.0, 4000.0)];
        let small: Vec<RoofEdge> = (0..4).map(|i| RoofEdge { defines_slope: false, ..RoofEdge::new(Curve::line(sq[i], sq[(i + 1) % 4])) }).collect();
        d2.set_roof_sketch(id2, small).unwrap();
        assert!(!d2.roof(id2).unwrap().shaped());
    }

    #[test]
    fn a_variable_layer_keeps_the_underside_flat() {
        let (mut d, id) = setup(false);
        let r = d.roof(id).unwrap().clone();
        let mut ty = d.roof_type(r.roof_type).unwrap().clone();
        ty.layers[0].variable = true;
        let tid = r.roof_type;
        d.transact("t", |tx| tx.update(tid, Element::RoofType(ty))).unwrap();
        d.add_roof_point(id, Point::new(3000.0, 2000.0), 80.0).unwrap();
        let r = d.roof(id).unwrap().clone();
        let g = d.roof_geometry(&r).unwrap();
        let (_, bot) = g.shaped.as_ref().unwrap();
        let flat = d.roof_base_z(&r) - d.roof_type(r.roof_type).unwrap().thickness_mm();
        assert!(bot.iter().all(|b| (b - flat).abs() < 1e-6), "{bot:?}");
        assert!((g.top_z - (d.roof_base_z(&r) + 80.0)).abs() < 1e-9);
    }
}
