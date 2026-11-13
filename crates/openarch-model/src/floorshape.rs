//! Shape editing of floors: points (plan position and height above the floor's unmodified top) and split lines, as
//! toposolids have (shape.rs is shared). A floor with any is shaped: its top is the triangulated surface through
//! its boundary corners (at the unmodified top), the points and along the split lines. With a variable layer in its
//! type the underside stays flat at the unmodified top less the thickness (the variable layer takes up the
//! difference), as a topping screed laid to falls does; without one the whole slab bends, its thickness the same
//! everywhere. A floor with a slope arrow is sloped instead and is not shape edited.

use crate::element::Floor;
use crate::shape::{within, Tin};
use crate::site::P3;
use crate::{Document, Element, Id, Point};

/// Each layer's top and bottom (absolute) where a shaped floor's top is at `top` and its underside at `bottom`:
/// layers above the variable one hang from the top, those below it stand on the underside, and the variable one
/// fills between; with no variable layer every layer hangs from the top.
pub fn floor_layer_spans(ty: &crate::element::FloorType, top: f64, bottom: f64) -> Vec<(f64, f64)> {
    layer_spans(&ty.layers, top, bottom)
}

/// The same for any layered type (a shaped roof's layers lie as a shaped floor's do).
pub fn layer_spans(layers: &[crate::element::Layer], top: f64, bottom: f64) -> Vec<(f64, f64)> {
    let v = layers.iter().position(|l| l.variable);
    let mut out = vec![(0.0, 0.0); layers.len()];
    let stop = v.unwrap_or(layers.len());
    let mut z = top;
    for (k, l) in layers.iter().enumerate().take(stop) { out[k] = (z, z - l.thickness_mm); z -= l.thickness_mm; }
    if let Some(v) = v {
        let mut z = bottom;
        for k in (v + 1..layers.len()).rev() { out[k] = (z + layers[k].thickness_mm, z); z += layers[k].thickness_mm; }
        out[v] = (top - layers[..v].iter().map(|l| l.thickness_mm).sum::<f64>(), z);
    }
    out
}

impl Floor {
    /// Whether the floor has been shape edited.
    pub fn shaped(&self) -> bool {
        !self.points.is_empty() || !self.split_lines.is_empty()
    }
}

impl Document {
    /// The unmodified top of a floor (absolute, mm): its level plus Height Offset From Level.
    pub fn floor_base_z(&self, f: &Floor) -> f64 {
        self.level(f.level).map_or(0.0, |l| l.elevation_mm) + f.offset_mm
    }

    /// A shaped floor's top surface; None for a floor that isn't shaped (its top is `floor_surface`).
    pub fn floor_shape(&self, f: &Floor) -> Option<Tin> {
        if !f.shaped() { return None; }
        let base = self.floor_base_z(f);
        let pts: Vec<[f64; 3]> = f.points.iter().map(|p| [p.x, p.y, base + p.z]).collect();
        crate::shape::triangulate(&f.polygons(20.0), &pts, &f.split_lines, base)
    }

    /// The underside's height under each corner of a shaped floor's top: flat at the unmodified top less the
    /// thickness with a variable layer (lower where the top dips so far that the other layers wouldn't fit),
    /// otherwise the top less the thickness.
    pub fn floor_bottoms(&self, f: &Floor, tin: &Tin) -> Vec<f64> {
        let ty = self.floor_type(f.floor_type);
        let th = ty.map_or(0.0, |t| t.thickness_mm());
        let fixed = ty.and_then(|t| t.layers.iter().position(|l| l.variable).map(|v| th - t.layers[v].thickness_mm));
        let flat = self.floor_base_z(f) - th;
        tin.verts.iter().map(|v| match fixed { Some(k) => flat.min(v[2] - k), None => v[2] - th }).collect()
    }

    /// The height of a floor's top at a plan point: the shaped surface where it is shaped, else its plane.
    pub fn floor_top_at(&self, f: &Floor, p: Point) -> Option<f64> {
        match self.floor_shape(f) {
            Some(t) => t.z_near(p),
            None => Some(self.floor_surface(f)?.z(p)),
        }
    }

    fn shaped_floor(&self, id: Id) -> Result<Floor, String> {
        let f = match self.get(id) { Some(Element::Floor(f)) => f.clone(), _ => return Err("not a floor".into()) };
        if f.slope.is_some() { return Err("A floor with a slope arrow is sloped, not shape edited: remove the slope arrow first (Edit Boundary).".into()); }
        Ok(f)
    }

    /// Where a shaped floor's top and underside are along the plan segment `a` to `b`: (fraction along, top,
    /// underside) at its ends and wherever it crosses an edge of the surface, so a section through it is exact.
    pub fn floor_profile(&self, tin: &Tin, bot: &[f64], a: Point, b: Point) -> Vec<(f64, f64, f64)> {
        let under = Tin { verts: tin.verts.iter().zip(bot).map(|(v, &z)| [v[0], v[1], z]).collect(), tris: tin.tris.clone() };
        let mut ts = vec![0.0, 1.0];
        let (rx, ry) = (b.x - a.x, b.y - a.y);
        for ([i, j], _) in tin.edges() {
            let (c, d) = (tin.verts[i], tin.verts[j]);
            let (sx, sy) = (d[0] - c[0], d[1] - c[1]);
            let den = rx * sy - ry * sx;
            if den.abs() < 1e-12 { continue; }
            let t = ((c[0] - a.x) * sy - (c[1] - a.y) * sx) / den;
            let u = ((c[0] - a.x) * ry - (c[1] - a.y) * rx) / den;
            if (0.0..=1.0).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) { ts.push(t); }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        ts.into_iter().filter_map(|t| {
            let p = Point::new(a.x + rx * t, a.y + ry * t);
            Some((t, tin.z_near(p)?, under.z_near(p)?))
        }).collect()
    }

    /// Modify Sub Elements: the floor's points and split lines replaced (plan positions, heights above the
    /// unmodified top; lines as pairs of indices). `lines` None keeps the lines that still fit. See `shape::tidy`.
    pub fn set_floor_points(&mut self, id: Id, points: Vec<P3>, lines: Option<Vec<[usize; 2]>>) -> Result<(), String> {
        let mut f = self.shaped_floor(id)?;
        self.refuse_pinned(&[id], "edited")?;
        let rings = f.polygons(20.0);
        let lines = lines.unwrap_or_else(|| f.split_lines.iter().copied().filter(|l| l[0] < points.len() && l[1] < points.len()).collect());
        let (pts, ls) = crate::shape::tidy(&rings, points, lines, "floor")?;
        if pts == f.points && ls == f.split_lines { return Ok(()); }
        (f.points, f.split_lines) = (pts, ls);
        self.transact("Modify floor points", |tx| tx.update(id, Element::Floor(f)))
    }

    /// Add Point: a point at a plan position, `z` above the floor's unmodified top. Outside the boundary it is refused.
    pub fn add_floor_point(&mut self, id: Id, at: Point, z: f64) -> Result<(), String> {
        let mut f = self.shaped_floor(id)?;
        self.refuse_pinned(&[id], "edited")?;
        if !(at.x.is_finite() && at.y.is_finite() && z.is_finite()) { return Err("the point must be numbers".into()); }
        let rings = f.polygons(20.0);
        if !within(&rings, at) { return Err("Click inside the floor's boundary.".into()); }
        let mut pts = f.points.clone();
        pts.push(P3 { x: at.x, y: at.y, z });
        (f.points, f.split_lines) = crate::shape::tidy(&rings, pts, f.split_lines.clone(), "floor")?;
        self.transact("Add point", |tx| tx.update(id, Element::Floor(f)))
    }

    /// Add Split Line: a line across the floor from `a` to `b`; each end not on a point becomes one at the
    /// surface's height there, and the floor then folds along the line.
    pub fn add_floor_split_line(&mut self, id: Id, a: Point, b: Point) -> Result<(), String> {
        let mut f = self.shaped_floor(id)?;
        self.refuse_pinned(&[id], "edited")?;
        if a.dist(&b) < 1.0 { return Err("The split line needs two ends that are apart.".into()); }
        let rings = f.polygons(20.0);
        let base = self.floor_base_z(&f);
        let tin = self.floor_shape(&f);
        let mut pts = f.points.clone();
        let mut end = |q: Point| -> Result<usize, String> {
            if !within(&rings, q) { return Err("Both ends of a split line go inside the floor's boundary.".into()); }
            if let Some(i) = pts.iter().position(|p| (p.x - q.x).hypot(p.y - q.y) <= 1.0) { return Ok(i); }
            let z = tin.as_ref().and_then(|t| t.z_near(q)).map_or(0.0, |z| z - base);
            pts.push(P3 { x: q.x, y: q.y, z });
            Ok(pts.len() - 1)
        };
        let (i, j) = (end(a)?, end(b)?);
        let mut ls = f.split_lines.clone();
        ls.push([i, j]);
        (f.points, f.split_lines) = crate::shape::tidy(&rings, pts, ls, "floor")?;
        self.transact("Add split line", |tx| tx.update(id, Element::Floor(f)))
    }

    /// Reset Shape: the points and split lines go; the floor is flat again.
    pub fn reset_floor_shape(&mut self, id: Id) -> Result<(), String> {
        let mut f = match self.get(id) { Some(Element::Floor(f)) => f.clone(), _ => return Err("not a floor".into()) };
        if !f.shaped() { return Err("The floor is already flat: it has no points or split lines.".into()); }
        self.refuse_pinned(&[id], "reshaped")?;
        f.points.clear();
        f.split_lines.clear();
        self.transact("Reset shape", |tx| tx.update(id, Element::Floor(f)))
    }

    /// The boundary of a floor after Edit Boundary: points and split lines outside the new boundary are dropped.
    pub(crate) fn keep_floor_points_within(f: &mut Floor) {
        if !f.shaped() { return; }
        let rings = f.polygons(20.0);
        (f.points, f.split_lines) = crate::shape::retain_within(&rings, &f.points, &f.split_lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;

    fn setup() -> (Document, Id) {
        let mut d = Document::new_project();
        let ft = d.floor_types()[0].0;
        let lvl = d.levels()[0].0;
        let p = |x: f64, y: f64| Point::new(x, y);
        let sq = [p(0.0, 0.0), p(6000.0, 0.0), p(6000.0, 4000.0), p(0.0, 4000.0)];
        let id = d.add_floor_sketch(ft, lvl, (0..4).map(|i| Curve::line(sq[i], sq[(i + 1) % 4])).collect(), None, None).unwrap();
        (d, id)
    }
    fn floor(d: &Document, id: Id) -> Floor { match d.get(id) { Some(Element::Floor(f)) => f.clone(), _ => panic!() } }

    #[test]
    fn a_floor_is_shaped_by_points_and_split_lines_and_reset_flat() {
        let (mut d, id) = setup();
        let f = floor(&d, id);
        let base = d.floor_base_z(&f);
        assert!(d.floor_shape(&f).is_none());
        // a drain point 30 below in the middle: the top falls to it
        d.add_floor_point(id, Point::new(3000.0, 2000.0), -30.0).unwrap();
        let f = floor(&d, id);
        assert!((d.floor_top_at(&f, Point::new(3000.0, 2000.0)).unwrap() - (base - 30.0)).abs() < 1e-6);
        assert!((d.floor_top_at(&f, Point::new(1500.0, 1000.0)).unwrap() - (base - 15.0)).abs() < 1e-6);
        // the whole slab bends: the underside follows the top down by the thickness
        let tin = d.floor_shape(&f).unwrap();
        let th = d.floor_type(f.floor_type).unwrap().thickness_mm();
        let bot = d.floor_bottoms(&f, &tin);
        assert!(tin.verts.iter().zip(&bot).all(|(v, b)| (v[2] - b - th).abs() < 1e-6));
        // a split line across: its new ends sit on the surface
        d.add_floor_split_line(id, Point::new(0.0, 2000.0), Point::new(6000.0, 2000.0)).unwrap();
        let f = floor(&d, id);
        assert_eq!(f.split_lines.len(), 2, "broken at the drain point: {:?}", f.split_lines);
        assert!(d.add_floor_point(id, Point::new(9000.0, 0.0), 0.0).is_err());
        d.reset_floor_shape(id).unwrap();
        assert!(!floor(&d, id).shaped());
        assert!(d.reset_floor_shape(id).is_err());
        d.undo();
        assert!(floor(&d, id).shaped());
    }

    #[test]
    fn a_section_through_a_shaped_floor_follows_its_top() {
        let (mut d, id) = setup();
        d.add_floor_point(id, Point::new(3000.0, 2000.0), -30.0).unwrap();
        let f = floor(&d, id);
        let base = d.floor_base_z(&f);
        let tin = d.floor_shape(&f).unwrap();
        let bot = d.floor_bottoms(&f, &tin);
        let prof = d.floor_profile(&tin, &bot, Point::new(0.0, 2000.0), Point::new(6000.0, 2000.0));
        let mid = prof.iter().find(|(t, _, _)| (t - 0.5).abs() < 1e-6).expect("a sample at the drain");
        assert!((mid.1 - (base - 30.0)).abs() < 1e-6 && (prof[0].1 - base).abs() < 1e-6, "{prof:?}");
        // layers: with no variable layer each hangs from the top
        let ty = d.floor_type(f.floor_type).unwrap();
        let spans = floor_layer_spans(ty, mid.1, mid.2);
        assert!((spans[0].0 - mid.1).abs() < 1e-9 && (spans.last().unwrap().1 - mid.2).abs() < 1e-6, "{spans:?}");
        // a spot on it reads the fall
        let mut ty2 = ty.clone();
        ty2.layers[0].variable = true;
        let spans = floor_layer_spans(&ty2, base - 30.0, base - ty2.thickness_mm());
        assert!((spans[0].0 - (base - 30.0)).abs() < 1e-9 && (spans.last().unwrap().1 - (base - ty2.thickness_mm())).abs() < 1e-9);
    }

    #[test]
    fn a_variable_layer_keeps_the_underside_flat() {
        let (mut d, id) = setup();
        let f = floor(&d, id);
        let mut ty = d.floor_type(f.floor_type).unwrap().clone();
        ty.layers[0].variable = true;
        let tid = f.floor_type;
        d.transact("t", |tx| tx.update(tid, Element::FloorType(ty))).unwrap();
        d.add_floor_point(id, Point::new(3000.0, 2000.0), 40.0).unwrap();
        let f = floor(&d, id);
        let tin = d.floor_shape(&f).unwrap();
        let bot = d.floor_bottoms(&f, &tin);
        let flat = d.floor_base_z(&f) - d.floor_type(f.floor_type).unwrap().thickness_mm();
        assert!(bot.iter().all(|b| (b - flat).abs() < 1e-6), "{bot:?}");
    }
}
