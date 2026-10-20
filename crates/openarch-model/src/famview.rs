//! A solved family body seen from a direction: the cut through it at a plane and the
//! shape it projects to. Plan, elevation and section views of a placed family are
//! drawn from these, so every family draws the same way in every view.
//!
//! Extrusions are cut and projected exactly. Lofts and revolves go through their
//! triangle meshes. Results are in the view's own axes: for a view looking along
//! `normal`, the first value runs along `normal.sketch_axes().0` and the second along `.1`.

use std::collections::HashMap;

use crate::famgeom::{Axis, Solid, Solved, SolvedForm, Tri};
use crate::geom::{signed_area, Point};
use crate::layers::{union_rings, Shape};

/// A piece of one form seen in a view.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece2D {
    pub form: u32,
    pub name: String,
    pub void: bool,
    pub material: String,
    pub subcategory: String,
    pub shape: Shape,
}

fn axis_index(a: Axis) -> usize {
    match a {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

fn rect(p: (f64, f64), q: (f64, f64)) -> Shape {
    vec![vec![Point::new(p.0, q.0), Point::new(p.1, q.0), Point::new(p.1, q.1), Point::new(p.0, q.1)]]
}

/// The extent of the rings of a shape along coordinate `k` (0 = u, 1 = v).
fn extent(shape: &Shape, k: usize) -> Option<(f64, f64)> {
    let get = |p: &Point| if k == 0 { p.x } else { p.y };
    let outer = shape.first()?;
    let lo = outer.iter().map(get).fold(f64::INFINITY, f64::min);
    let hi = outer.iter().map(get).fold(f64::NEG_INFINITY, f64::max);
    (hi > lo).then_some((lo, hi))
}

/// Intervals of the shape's material along `s_axis` (0 = u, 1 = v) at `t` on the other one.
fn cut_line(shape: &Shape, other_axis: usize, t: f64) -> Vec<(f64, f64)> {
    let t = t + 1e-7; // keep clear of vertices
    let mut xs = Vec::new();
    for ring in shape {
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let (ao, bo) = if other_axis == 0 { (a.x, b.x) } else { (a.y, b.y) };
            if (ao < t) != (bo < t) {
                let f = (t - ao) / (bo - ao);
                xs.push(if other_axis == 0 { a.y + f * (b.y - a.y) } else { a.x + f * (b.x - a.x) });
            }
        }
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.chunks(2).filter(|c| c.len() == 2 && c[1] - c[0] > 1e-9).map(|c| (c[0], c[1])).collect()
}

/// Lay a rectangle given by a range along the extrusion axis and a range along the
/// sketch axis `s` into the view's axes.
fn place(view: Axis, prism_axis: Axis, a_range: (f64, f64), s_axis: Axis, s_range: (f64, f64)) -> Shape {
    let (vu, _) = view.sketch_axes();
    // first view coordinate is the extrusion axis when the view's u is that axis
    let _ = s_axis;
    if vu == prism_axis {
        rect(a_range, s_range)
    } else {
        rect(s_range, a_range)
    }
}

fn piece(f: &SolvedForm, shape: Shape) -> Piece2D {
    Piece2D { form: f.id, name: f.name.clone(), void: f.void, material: f.material.clone(), subcategory: f.subcategory.clone(), shape }
}

impl Solved {
    /// Forms that are really there (their visibility condition holds), solids first.
    fn present(&self) -> impl Iterator<Item = &SolvedForm> {
        self.forms.iter().filter(|f| f.vis.present)
    }

    /// The shape of every present form projected along `view`. Voids are included, flagged.
    pub fn project(&self, view: Axis) -> Vec<Piece2D> {
        let mut out = Vec::new();
        for f in self.present() {
            match f.solid.axis {
                Some(a) => {
                    for part in &f.solid.parts {
                        for shape in &part.shapes {
                            if a == view {
                                out.push(piece(f, shape.clone()));
                                continue;
                            }
                            // the sketch axis that is not the view direction
                            let (su, sv) = a.sketch_axes();
                            let (s_axis, k) = if su == view { (sv, 1) } else { (su, 0) };
                            if let Some(r) = extent(shape, k) {
                                out.push(piece(f, place(view, a, (part.lo, part.hi), s_axis, r)));
                            }
                        }
                    }
                }
                None => {
                    let rings = project_tris(&f.solid.tris, view);
                    for s in union_rings(rings.iter()) {
                        out.push(piece(f, s));
                    }
                }
            }
        }
        out
    }

    /// The cut through every present form at depth `at` along `view` (a plan cut at height `at` is `Axis::Z`).
    pub fn section(&self, view: Axis, at: f64) -> Vec<Piece2D> {
        let mut out = Vec::new();
        for f in self.present() {
            match f.solid.axis {
                Some(a) => {
                    for part in &f.solid.parts {
                        if a == view {
                            if at > part.lo && at < part.hi {
                                out.extend(part.shapes.iter().map(|s| piece(f, s.clone())));
                            }
                            continue;
                        }
                        let (su, sv) = a.sketch_axes();
                        // `view` is one of the sketch axes; the cut line runs along the other
                        let (view_k, s_axis, s_k) = if su == view { (0, sv, 1) } else { (1, su, 0) };
                        let _ = s_k;
                        for shape in &part.shapes {
                            for r in cut_line(shape, view_k, at) {
                                out.push(piece(f, place(view, a, (part.lo, part.hi), s_axis, r)));
                            }
                        }
                    }
                }
                None => {
                    for ring in slice_tris(&f.solid.tris, view, at) {
                        out.push(piece(f, vec![ring]));
                    }
                }
            }
        }
        out
    }

    /// The box the host-cutting voids span, in family coordinates: `[min, max]` per axis.
    pub fn host_cut_bounds(&self) -> Option<[[f64; 3]; 2]> {
        let mut b: Option<[[f64; 3]; 2]> = None;
        for f in self.present().filter(|f| f.cuts_host) {
            let Some(fb) = f.solid.bounds() else { continue };
            b = Some(match b {
                None => fb,
                Some(o) => [[o[0][0].min(fb[0][0]), o[0][1].min(fb[0][1]), o[0][2].min(fb[0][2])], [o[1][0].max(fb[1][0]), o[1][1].max(fb[1][1]), o[1][2].max(fb[1][2])]],
            });
        }
        b
    }
}

impl Solid {
    pub fn bounds(&self) -> Option<[[f64; 3]; 2]> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for t in &self.tris {
            for v in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        (lo[0].is_finite()).then_some([lo, hi])
    }
}

/// Every triangle flattened along `view`, as rings for a union.
fn project_tris(tris: &[Tri], view: Axis) -> Vec<Vec<Point>> {
    let (u, v) = view.sketch_axes();
    let (iu, iv) = (axis_index(u), axis_index(v));
    tris.iter()
        .map(|t| t.iter().map(|p| Point::new(p[iu], p[iv])).collect::<Vec<_>>())
        .filter(|r| signed_area(r).abs() > 1e-9)
        .collect()
}

/// Closed loops where the plane `view = at` cuts a closed mesh, chained from the cut edges.
fn slice_tris(tris: &[Tri], view: Axis, at: f64) -> Vec<Vec<Point>> {
    let (u, v) = view.sketch_axes();
    let (iu, iv, iw) = (axis_index(u), axis_index(v), axis_index(view));
    let at = at + 1e-7;
    let mut segs: Vec<(Point, Point)> = Vec::new();
    for t in tris {
        let mut cut: Vec<Point> = Vec::new();
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            if (a[iw] < at) != (b[iw] < at) {
                let f = (at - a[iw]) / (b[iw] - a[iw]);
                cut.push(Point::new(a[iu] + f * (b[iu] - a[iu]), a[iv] + f * (b[iv] - a[iv])));
            }
        }
        if cut.len() == 2 {
            segs.push((cut[0], cut[1]));
        }
    }
    let key = |p: &Point| ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64);
    let mut next: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        next.entry(key(&s.0)).or_default().push(i);
        next.entry(key(&s.1)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut loops = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut ring = vec![segs[start].0, segs[start].1];
        loop {
            let last = *ring.last().unwrap();
            let Some(&n) = next.get(&key(&last)).and_then(|v| v.iter().find(|&&i| !used[i])) else { break };
            used[n] = true;
            let (a, b) = segs[n];
            ring.push(if key(&a) == key(&last) { b } else { a });
        }
        if ring.len() > 3 && key(&ring[0]) == key(ring.last().unwrap()) {
            ring.pop();
        }
        if ring.len() >= 3 && signed_area(&ring).abs() > 1e-9 {
            loops.push(ring);
        }
    }
    loops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::famgeom::*;
    use openarch_params::{EvalError, Value};

    fn none(n: &str) -> Result<Value, EvalError> {
        Err(EvalError::UnknownName(n.to_string()))
    }

    fn form(shape: FormShape, void: bool) -> Form {
        Form { id: 0, name: String::new(), shape, void, cuts_host: true, cut_by: vec![], opening: false, material: String::new(), material_param: None, subcategory: String::new(), vis: Visibility::default() }
    }

    fn area(s: &Shape) -> f64 {
        s.iter().enumerate().map(|(i, r)| if i == 0 { signed_area(r).abs() } else { -signed_area(r).abs() }).sum()
    }

    /// A 100 x 40 x 2000 (x, y, z) post, made as a plan footprint extruded up.
    fn post() -> Solved {
        let mut b = FamilyBody::template();
        b.add_form(form(
            FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_LEVEL, start: "0".into(), end: "2000".into(), profile: vec![Loop::rect(Coord::at(-50.0), Coord::at(50.0), Coord::at(-20.0), Coord::at(20.0))] },
            false,
        ));
        b.solve(&mut none).unwrap()
    }

    #[test]
    fn a_post_cuts_to_its_footprint_and_projects_to_an_elevation_rectangle() {
        let s = post();
        let plan = s.section(Axis::Z, 1000.0);
        assert_eq!(plan.len(), 1);
        assert!((area(&plan[0].shape) - 4000.0).abs() < 1e-6);
        assert!(s.section(Axis::Z, 2500.0).is_empty());
        // front elevation looks along y: x across, z up
        let front = s.project(Axis::Y);
        assert_eq!(front.len(), 1);
        assert!((area(&front[0].shape) - 100.0 * 2000.0).abs() < 1e-6, "{:?}", front[0].shape);
        // a horizontal cut through the front view at x = 0 is the 40 wide, 2000 tall strip across (y, z)
        let side = s.section(Axis::X, 0.0);
        assert!((area(&side[0].shape) - 40.0 * 2000.0).abs() < 1e-6);
    }

    #[test]
    fn a_leaf_extruded_from_the_front_cuts_to_a_plan_strip_and_has_gaps() {
        // a 800 x 2000 leaf, 40 thick, with a 300 x 500 vision panel hole
        let mut b = FamilyBody::template();
        let outer = Loop::rect(Coord::at(-400.0), Coord::at(400.0), Coord::at(0.0), Coord::at(2000.0));
        let hole = Loop::rect(Coord::at(-150.0), Coord::at(150.0), Coord::at(1200.0), Coord::at(1700.0));
        b.add_form(form(FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_FB, start: "0".into(), end: "40".into(), profile: vec![outer, hole] }, false));
        let s = b.solve(&mut none).unwrap();
        // at 1500 the cut passes through the panel: two strips either side, each 40 deep
        let plan = s.section(Axis::Z, 1500.0);
        assert_eq!(plan.len(), 2);
        assert!(plan.iter().all(|p| (area(&p.shape) - 250.0 * 40.0).abs() < 1e-3), "{plan:?}");
        // below the panel it is one strip, 800 wide
        let low = s.section(Axis::Z, 500.0);
        assert_eq!(low.len(), 1);
        assert!((area(&low[0].shape) - 800.0 * 40.0).abs() < 1e-3);
        // projected on plan it is the whole 800 x 40 (the hole is not a gap along y)
        let proj = s.project(Axis::Z);
        assert!((area(&proj[0].shape) - 800.0 * 40.0).abs() < 1e-3);
    }

    #[test]
    fn a_revolved_post_slices_to_a_disc_and_the_cut_box_comes_from_the_voids() {
        let mut b = FamilyBody::template();
        b.add_form(form(
            FormShape::Revolve {
                plane: PLANE_FB,
                axis_a: Node::new(Coord::at(0.0), Coord::at(0.0)),
                axis_b: Node::new(Coord::at(0.0), Coord::at(100.0)),
                start_deg: 0.0,
                end_deg: 360.0,
                profile: vec![Loop::rect(Coord::at(0.0), Coord::at(30.0), Coord::at(0.0), Coord::at(100.0))],
            },
            false,
        ));
        let opening = form(
            FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_FB, start: "-60".into(), end: "60".into(), profile: vec![Loop::rect(Coord::at(-410.0), Coord::at(410.0), Coord::at(0.0), Coord::at(2050.0))] },
            true,
        );
        b.add_form(opening);
        let s = b.solve(&mut none).unwrap();
        let disc: Vec<_> = s.section(Axis::Z, 50.0).into_iter().filter(|p| !p.void).collect();
        assert_eq!(disc.len(), 1);
        let want = std::f64::consts::PI * 900.0;
        assert!((area(&disc[0].shape) - want).abs() / want < 0.03, "{}", area(&disc[0].shape));
        // the host cut comes from the void only, not the post
        let cut = s.host_cut_bounds().unwrap();
        assert_eq!((cut[0][0], cut[1][0], cut[0][2], cut[1][2], cut[0][1], cut[1][1]), (-410.0, 410.0, 0.0, 2050.0, -60.0, 60.0));
    }
}
