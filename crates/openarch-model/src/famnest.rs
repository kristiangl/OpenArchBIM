//! Placing a nested family (a Component) inside another one. The nested family is solved on its
//! own by the project; this turns the result into the parent's coordinates so every view that
//! draws a solved body (plan, elevations, sections, 3D) draws the component without knowing about it.

use crate::famgeom::{Axis, Nested, PrismPart, Solid, Solved, SolvedForm, SolvedModelLine, SolvedSym, SolvedVis, SymView, Tri};
use crate::geom::Point;

/// Every nested piece gets an id of its own in the parent: this base, then ten thousand per component.
const ID_BASE: u32 = 1_000_000;
const ID_STRIDE: u32 = 10_000;

/// Where a component's own axes go in the parent: a point is `m * p + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Basis {
    /// Columns are the images of the component's X, Y and Z.
    pub m: [[f64; 3]; 3],
    pub t: [f64; 3],
}

impl Basis {
    pub fn point(&self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.m;
        [
            m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + self.t[0],
            m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + self.t[1],
            m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + self.t[2],
        ]
    }

    pub fn dir(&self, d: [f64; 3]) -> [f64; 3] {
        let m = &self.m;
        [
            m[0][0] * d[0] + m[0][1] * d[1] + m[0][2] * d[2],
            m[1][0] * d[0] + m[1][1] * d[1] + m[1][2] * d[2],
            m[2][0] * d[0] + m[2][1] * d[1] + m[2][2] * d[2],
        ]
    }

    /// True when the map turns the handedness round (a mirror), so triangles need reversing.
    pub fn mirrors(&self) -> bool {
        let m = &self.m;
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        det < 0.0
    }

    /// Only moves the component: its axes stay the parent's.
    pub fn is_translation(&self) -> bool {
        (0..3).all(|r| (0..3).all(|c| (self.m[r][c] - if r == c { 1.0 } else { 0.0 }).abs() < 1e-9))
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The map for a component at `origin`. A component of a work-plane-based family stands on its plane:
/// its Z is the plane's normal (reversed by `flip`) and its Y is up (or +Y on a level plane).
/// Any other component keeps the parent's axes. `rotation_deg` turns it about its own Z,
/// and `mirror` swaps its left and right (X) before that.
pub fn basis(origin: [f64; 3], plane: Option<Axis>, work_plane_based: bool, flip: bool, rotation_deg: f64, mirror: bool) -> Basis {
    let (y, z) = match plane.filter(|_| work_plane_based) {
        Some(Axis::Z) => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        Some(Axis::Y) => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        Some(Axis::X) => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        None => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    };
    let flip = flip && plane.is_some() && work_plane_based;
    let z = if flip { [-z[0], -z[1], -z[2]] } else { z };
    let x = cross(y, z);
    // spin about the component's own Z, after the mirror
    let (s, c) = rotation_deg.to_radians().sin_cos();
    let mx = if mirror { -1.0 } else { 1.0 };
    // local X and Y after mirror and spin, written in the component's frame
    let lx = [c * mx, s * mx, 0.0];
    let ly = [-s, c, 0.0];
    let img = |v: [f64; 3]| [x[0] * v[0] + y[0] * v[1] + z[0] * v[2], x[1] * v[0] + y[1] * v[1] + z[1] * v[2], x[2] * v[0] + y[2] * v[1] + z[2] * v[2]];
    let (ix, iy, iz) = (img(lx), img(ly), img([0.0, 0.0, 1.0]));
    Basis { m: [[ix[0], iy[0], iz[0]], [ix[1], iy[1], iz[1]], [ix[2], iy[2], iz[2]]], t: origin }
}

fn and(a: &SolvedVis, b: &SolvedVis) -> SolvedVis {
    SolvedVis { coarse: a.coarse && b.coarse, medium: a.medium && b.medium, fine: a.fine && b.fine, plan: a.plan && b.plan, front_back: a.front_back && b.front_back, left_right: a.left_right && b.left_right, present: a.present && b.present }
}

/// The direction a view looks along, in the family's axes.
fn view_dir(v: SymView) -> [f64; 3] {
    match v {
        SymView::Plan => [0.0, 0.0, -1.0],
        SymView::Front => [0.0, 1.0, 0.0],
        SymView::Back => [0.0, -1.0, 0.0],
        SymView::Left => [1.0, 0.0, 0.0],
        SymView::Right => [-1.0, 0.0, 0.0],
    }
}

fn view_for(d: [f64; 3]) -> Option<SymView> {
    [SymView::Plan, SymView::Front, SymView::Back, SymView::Left, SymView::Right].into_iter().find(|v| {
        let w = view_dir(*v);
        (0..3).map(|i| w[i] * d[i]).sum::<f64>() > 0.999
    })
}

fn axis_index(a: Axis) -> usize {
    match a {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// Add the solved component `child` (in its own axes) to `out`, moved by `b`. `vis` is how the
/// component itself shows; a piece shows only where both agree. Returns how many pieces were added.
pub fn place(child: Solved, n: &Nested, b: &Basis, vis: &SolvedVis, out: &mut Solved) -> usize {
    let base = ID_BASE.saturating_add(n.id.saturating_mul(ID_STRIDE));
    let mut next = 0u32;
    let mut take = || {
        let id = base.saturating_add(next);
        next = (next + 1).min(ID_STRIDE - 1);
        id
    };
    let mut added = 0;
    let mirrored = b.mirrors();
    let label = if n.name.is_empty() { n.family.clone() } else { n.name.clone() };
    for f in child.forms {
        let solid = move_solid(f.solid, b, mirrored);
        let span = if solid.axis.is_some() {
            f.span.map(|(a, c)| {
                let ax = axis_index(solid.axis.unwrap());
                (a + b.t[ax], c + b.t[ax])
            })
        } else {
            None
        };
        out.forms.push(SolvedForm {
            id: take(),
            name: if f.name.is_empty() { label.clone() } else { format!("{label}: {}", f.name) },
            void: f.void,
            // a void inside a component shapes the component; it does not cut the parent's host
            cuts_host: false,
            material: f.material,
            subcategory: f.subcategory,
            vis: and(&f.vis, vis),
            solid,
            span,
            text: f.text,
            owner: Some(n.id),
        });
        added += 1;
    }
    for s in child.syms {
        let (nu, nv) = s.view.normal().sketch_axes();
        let dir = b.dir(view_dir(s.view));
        let Some(view) = view_for(dir) else { continue };
        let (tu, tv) = view.normal().sketch_axes();
        let lift = |p: &Point| -> Point {
            let mut v = [0.0; 3];
            v[axis_index(nu)] = p.x;
            v[axis_index(nv)] = p.y;
            let w = b.point(v);
            Point::new(w[axis_index(tu)], w[axis_index(tv)])
        };
        out.syms.push(SolvedSym { id: take(), view, points: s.points.iter().map(lift).collect(), closed: s.closed, filled: s.filled, masking: s.masking, subcategory: s.subcategory, vis: and(&s.vis, vis), owner: Some(n.id) });
        added += 1;
    }
    for m in child.model_lines {
        out.model_lines.push(SolvedModelLine {
            id: take(),
            name: m.name,
            points: m.points.iter().map(|p| b.point(*p)).collect(),
            closed: m.closed,
            subcategory: m.subcategory,
            vis: and(&m.vis, vis),
            owner: Some(n.id),
        });
        added += 1;
    }
    added
}

fn move_solid(s: Solid, b: &Basis, mirrored: bool) -> Solid {
    if b.is_translation() {
        // exact pieces stay exact: shift the sketch regions and the depths
        if let Some(axis) = s.axis {
            let (au, av) = axis.sketch_axes();
            let (du, dv, dw) = (b.t[axis_index(au)], b.t[axis_index(av)], b.t[axis_index(axis)]);
            let parts = s
                .parts
                .into_iter()
                .map(|p| PrismPart { lo: p.lo + dw, hi: p.hi + dw, shapes: p.shapes.into_iter().map(|sh| sh.into_iter().map(|ring| ring.into_iter().map(|q| Point::new(q.x + du, q.y + dv)).collect()).collect()).collect() })
                .collect();
            let tris = s.tris.iter().map(|t| t.map(|p| b.point(p))).collect();
            return Solid { axis: Some(axis), parts, tris };
        }
    }
    let tris: Vec<Tri> = s
        .tris
        .iter()
        .map(|t| {
            let (p0, p1, p2) = (b.point(t[0]), b.point(t[1]), b.point(t[2]));
            if mirrored {
                [p0, p2, p1]
            } else {
                [p0, p1, p2]
            }
        })
        .collect();
    Solid { axis: None, parts: vec![], tris }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    #[test]
    fn a_free_component_keeps_the_parents_axes_and_a_quarter_turn_swaps_x_and_y() {
        let b = basis([10.0, 20.0, 30.0], None, false, false, 0.0, false);
        assert!(b.is_translation() && !b.mirrors());
        assert!(close(b.point([1.0, 2.0, 3.0]), [11.0, 22.0, 33.0]));
        let q = basis([0.0; 3], None, false, false, 90.0, false);
        assert!(close(q.point([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        assert!(close(q.point([0.0, 1.0, 0.0]), [-1.0, 0.0, 0.0]));
    }

    #[test]
    fn a_component_on_a_vertical_plane_stands_out_of_it() {
        // its Z is the plane's normal, its Y is up
        let b = basis([0.0; 3], Some(Axis::Y), true, false, 0.0, false);
        assert!(close(b.dir([0.0, 0.0, 1.0]), [0.0, 1.0, 0.0]));
        assert!(close(b.dir([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!(!b.mirrors());
        let f = basis([0.0; 3], Some(Axis::Y), true, true, 0.0, false);
        assert!(close(f.dir([0.0, 0.0, 1.0]), [0.0, -1.0, 0.0]));
        assert!(!f.mirrors());
        let m = basis([0.0; 3], None, false, false, 0.0, true);
        assert!(m.mirrors());
        assert!(close(m.point([5.0, 0.0, 0.0]), [-5.0, 0.0, 0.0]));
    }
}
