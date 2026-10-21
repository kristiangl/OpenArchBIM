//! Triangle meshes for display. Output is interleaved `x y z nx ny nz` in
//! metres (the viewport's unit), non-indexed, counter-clockwise front faces.

use crate::doc::Document;
use crate::element::{Floor, Wall};
use crate::roof::Roof;
use crate::family::{FamilyCategory, Opening};
use crate::hosting::InstanceFrame;
use crate::geom::{signed_area, triangulate_region, Point};
use crate::layers::WallLayers;
use crate::view::Color;

const MM: f64 = 0.001;

fn push(out: &mut Vec<f32>, p: [f64; 3], n: [f64; 3]) {
    out.extend_from_slice(&[(p[0] * MM) as f32, (p[1] * MM) as f32, (p[2] * MM) as f32, n[0] as f32, n[1] as f32, n[2] as f32]);
}

fn tri(out: &mut Vec<f32>, a: [f64; 3], b: [f64; 3], c: [f64; 3], n: [f64; 3]) {
    push(out, a, n);
    push(out, b, n);
    push(out, c, n);
}

/// Extrude a counter-clockwise polygon (already triangulated) between z0 and z1.
fn prism(out: &mut Vec<f32>, poly: &[Point], tris: &[[usize; 3]], z0: f64, z1: f64) {
    let v = |p: Point, z: f64| [p.x, p.y, z];
    for t in tris {
        let (a, b, c) = (poly[t[0]], poly[t[1]], poly[t[2]]);
        tri(out, v(a, z1), v(b, z1), v(c, z1), [0., 0., 1.]);
        tri(out, v(a, z0), v(c, z0), v(b, z0), [0., 0., -1.]);
    }
    prism_sides(out, poly, z0, z1);
}

/// A flat face at height `z` over each of `shapes` (an outline and its holes), facing up or down.
fn cap(out: &mut Vec<f32>, shapes: &[crate::layers::Shape], z: f64, up: bool) {
    for s in shapes {
        let Some(outer) = s.first() else { continue };
        // a sliver left where two bands nearly agree is not a face
        if signed_area(outer).abs() < 10.0 {
            continue;
        }
        let region = crate::geom::Region { outer: outer.clone(), holes: s[1..].to_vec() };
        let Some((pts, tris)) = triangulate_region(&region) else { continue };
        let v = |p: Point| [p.x, p.y, z];
        for t in tris {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            if up {
                tri(out, v(a), v(b), v(c), [0., 0., 1.]);
            } else {
                tri(out, v(a), v(c), v(b), [0., 0., -1.]);
            }
        }
    }
}

/// The upright sides of a counter-clockwise polygon extruded between z0 and z1.
fn prism_sides(out: &mut Vec<f32>, poly: &[Point], z0: f64, z1: f64) {
    let v = |p: Point, z: f64| [p.x, p.y, z];
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let len = a.dist(&b);
        if len < 1e-9 {
            continue;
        }
        let nrm = [(b.y - a.y) / len, -(b.x - a.x) / len, 0.];
        tri(out, v(a, z0), v(b, z0), v(b, z1), nrm);
        tri(out, v(a, z0), v(b, z1), v(a, z1), nrm);
    }
}

/// Mesh for a wall. `outline` is its joined plan outline (see
/// `Document::wall_outlines`); `openings` are the holes cut by hosted doors and
/// windows (see `Document::wall_openings`).
pub fn wall_mesh(doc: &Document, w: &Wall, outline: &[Point; 4], openings: &[Opening]) -> Option<Vec<f32>> {
    if w.curve.is_some() {
        return None; // curved walls are meshed layer by layer
    }
    let o = outline;
    let (z0, z1) = doc.wall_extent(w);
    if z1 <= z0 {
        return None;
    }
    let quad = [[0, 1, 2], [0, 2, 3]];
    let mut out = Vec::with_capacity(36 * 6 * 2);
    if openings.is_empty() {
        prism(&mut out, o, &quad, z0, z1);
        return Some(out);
    }
    let len = w.start.dist(&w.end);
    // full-height pieces between the openings
    let mut cuts = vec![0.0];
    for op in openings {
        cuts.push(op.u0);
        cuts.push(op.u1);
    }
    cuts.push(len);
    for c in cuts.chunks(2).filter(|c| c[1] - c[0] > 1.0) {
        prism(&mut out, &doc.wall_slice(w, o, c[0], c[1], len), &quad, z0, z1);
    }
    // wall left under and over each opening
    for op in openings {
        let q = doc.wall_slice(w, o, op.u0, op.u1, len);
        if op.z0 > z0 + 1e-6 {
            prism(&mut out, &q, &quad, z0, op.z0);
        }
        if op.z1 < z1 - 1e-6 {
            prism(&mut out, &q, &quad, op.z1, z1);
        }
    }
    Some(out)
}


/// One wall layer as 3D geometry: its material and the triangles.
pub struct LayerMesh {
    pub material: String,
    pub color: Color,
    pub verts: Vec<f32>,
}

/// Meshes for a wall built layer by layer. The wall is cut into height bands at
/// the tops and bottoms of its openings, and each band takes the plan layers
/// as they are at that height, so openings, jamb wrapping and joins all carry
/// over from the plan geometry. Void layers are skipped. A band has a top or
/// bottom face only where nothing solid sits on it or under it (a sill, a head,
/// the top and base of the wall), so the bands make one closed surface with no
/// faces hidden inside the wall.
pub fn wall_layer_meshes(doc: &Document, id: u64, w: &Wall, wl: &WallLayers) -> Vec<LayerMesh> {
    let (z0, z1) = doc.wall_extent(w);
    if z1 <= z0 || wl.layers.is_empty() {
        return vec![];
    }
    let mut zs = vec![z0, z1];
    for op in doc.wall_openings(id) {
        for z in [op.z0, op.z1] {
            if z > z0 + 1.0 && z < z1 - 1.0 {
                zs.push(z);
            }
        }
    }
    zs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    zs.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    let mut out: Vec<LayerMesh> = wl
        .layers
        .iter()
        .map(|l| LayerMesh {
            material: l.material.clone(),
            color: doc.material(&l.material).map_or([200, 200, 200], |m| m.graphics.shading_color),
            verts: vec![],
        })
        .collect();
    let bands: Vec<_> = zs.windows(2).map(|b| doc.wall_plan_layers(id, wl, Some((b[0] + b[1]) / 2.0))).collect();
    // everything solid in a band, in plan
    let solid = |bi: usize| -> Vec<crate::layers::Ring> { bands[bi].iter().filter(|l| !l.is_void).flat_map(|l| l.shapes.iter().filter_map(|s| s.first().cloned())).collect() };
    for (bi, band) in zs.windows(2).enumerate() {
        let below = if bi > 0 { solid(bi - 1) } else { vec![] };
        let above = if bi + 1 < bands.len() { solid(bi + 1) } else { vec![] };
        for (k, l) in bands[bi].iter().enumerate() {
            if l.is_void {
                continue;
            }
            for shape in &l.shapes {
                let Some(ring) = shape.first() else { continue };
                let mut ring = ring.clone();
                if signed_area(&ring) < 0.0 {
                    ring.reverse();
                }
                if ring.len() < 3 || signed_area(&ring).abs() < 1e-6 {
                    continue;
                }
                prism_sides(&mut out[k].verts, &ring, band[0], band[1]);
                cap(&mut out[k].verts, &crate::layers::subtract(&ring, &above), band[1], true);
                cap(&mut out[k].verts, &crate::layers::subtract(&ring, &below), band[0], false);
            }
        }
    }
    out.retain(|m| !m.verts.is_empty());
    out
}

/// A box in an instance's own coordinates: along `x0..x1`, across `y0..y1`
/// (left positive), elevations `z0..z1`.
fn local_box(out: &mut Vec<f32>, f: &InstanceFrame, x: (f64, f64), y: (f64, f64), z: (f64, f64)) {
    if x.1 - x.0 < 1e-6 || y.1 - y.0 < 1e-6 || z.1 - z.0 < 1e-6 {
        return;
    }
    let poly = [f.world(x.0, y.0), f.world(x.1, y.0), f.world(x.1, y.1), f.world(x.0, y.1)];
    prism(out, &poly, &[[0, 1, 2], [0, 2, 3]], z.0, z.1);
}

/// A framed door: the frame section swept up both jambs and across the head,
/// and the leaf (or pair) closed against the stop.
fn framed_door_mesh(sf: &crate::frame::DoorFrame, f: &InstanceFrame) -> Vec<f32> {
    let mut out = Vec::new();
    let swing = if f.flip_facing { -1.0 } else { 1.0 };
    let ym = f.y_mid() + sf.shift;
    let r2 = sf.reveal_w / 2.0;
    let yr = |b0: f64, b1: f64| {
        let (y0, y1) = (ym + swing * b0, ym + swing * b1);
        (y0.min(y1), y0.max(y1))
    };
    let (z_floor, z_head) = (f.z0, f.z0 + sf.reveal_h);
    let a_max = sf.reach();
    for [a0, a1, b0, b1] in sf.section() {
        let y = yr(b0, b1);
        // jambs stand on the floor up to the underside of the head
        local_box(&mut out, f, (-(r2 + a1), -(r2 + a0)), y, (z_floor, z_head + a0.min(0.0)));
        local_box(&mut out, f, (r2 + a0, r2 + a1), y, (z_floor, z_head + a0.min(0.0)));
        // the head: the same section turned so that `a` runs up
        local_box(&mut out, f, (-(r2 + a_max), r2 + a_max), y, (z_head + a0, z_head + a1));
    }
    // leaves
    let (y0, y1) = yr(sf.leaf_b0(), sf.leaf_b0() + sf.leaf_t);
    let (zl0, zl1) = (z_floor + sf.bottom_gap, z_floor + sf.bottom_gap + sf.leaf_h);
    let edge = r2 - sf.side_gap;
    if sf.leaves == 2 {
        local_box(&mut out, f, (-edge, -sf.side_gap / 2.0), (y0, y1), (zl0, zl1));
        local_box(&mut out, f, (sf.side_gap / 2.0, edge), (y0, y1), (zl0, zl1));
    } else {
        local_box(&mut out, f, (-edge, edge), (y0, y1), (zl0, zl1));
    }
    out
}

/// Meshes for a hosted door or window: `(frame_and_leaf, glass)`. The leaf is
/// drawn closed, in the middle of the wall.
pub fn instance_meshes(doc: &Document, id: u64) -> Option<(FamilyCategory, Vec<f32>, Vec<f32>)> {
    let crate::element::Element::FamilyInstance(inst) = doc.get(id)? else { return None };
    let (fam, _) = doc.instance_family(inst)?;
    let f = doc.instance_frame(id)?;
    let cat = fam.category;
    if let Some(solved) = doc.instance_solved(id) {
        let s = solved.as_ref().as_ref().ok()?;
        let (mut solid, mut glass) = (Vec::new(), Vec::new());
        for form in s.forms.iter().filter(|x| !x.void && x.vis.present) {
            let out = if form.subcategory == "Glass" { &mut glass } else { &mut solid };
            for t in &form.solid.tris {
                let mut p = t.map(|v| f.fam_point(v));
                if f.fam_mirrors() {
                    p.swap(1, 2);
                }
                let (u, v) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if l > 1e-12 {
                    tri(out, p[0], p[1], p[2], [n[0] / l, n[1] / l, n[2] / l]);
                }
            }
        }
        return Some((cat, solid, glass));
    }
    if let Some(sf) = doc.door_frame(id) {
        return Some((cat, framed_door_mesh(&sf, &f), Vec::new()));
    }
    let num = |n: &str, d: f64| doc.instance_param(inst, n).and_then(|v| v.as_f64()).unwrap_or(d);
    let fw = num("Frame Width", 70.0).min(f.width / 4.0);
    let (hw, ym) = (f.width / 2.0, f.y_mid());
    let depth = num("Frame Depth", 90.0).min(f.wall_left + f.wall_right);
    let (y0, y1) = (ym - depth / 2.0, ym + depth / 2.0);
    let mut frame = Vec::new();
    let mut glass = Vec::new();
    // frame: two jambs and a head (windows also get a sill)
    local_box(&mut frame, &f, (-hw, -hw + fw), (y0, y1), (f.z0, f.z1));
    local_box(&mut frame, &f, (hw - fw, hw), (y0, y1), (f.z0, f.z1));
    local_box(&mut frame, &f, (-hw + fw, hw - fw), (y0, y1), (f.z1 - fw, f.z1));
    match cat {
        FamilyCategory::Doors => {
            let t = num("Thickness", 40.0);
            local_box(&mut frame, &f, (-hw + fw, hw - fw), (ym - t / 2.0, ym + t / 2.0), (f.z0, f.z1 - fw));
        }
        FamilyCategory::Windows => {
            local_box(&mut frame, &f, (-hw + fw, hw - fw), (y0, y1), (f.z0, f.z0 + fw));
            let g = num("Glazing Thickness", 6.0);
            local_box(&mut glass, &f, (-hw + fw, hw - fw), (ym - g / 2.0, ym + g / 2.0), (f.z0 + fw, f.z1 - fw));
        }
        _ => {}
    }
    Some((cat, frame, glass))
}

pub fn floor_mesh(doc: &Document, f: &Floor) -> Option<Vec<f32>> {
    let t = doc.floor_type(f.floor_type)?.thickness_mm();
    let surf = doc.floor_surface(f)?;
    let regions = f.regions(0.5);
    if regions.is_empty() {
        return None;
    }
    // the top plane's normal, tilted by the slope
    let nl = (1.0 + surf.rise * surf.rise).sqrt();
    let up = [-surf.rise * surf.dir.x / nl, -surf.rise * surf.dir.y / nl, 1.0 / nl];
    let down = [-up[0], -up[1], -up[2]];
    let mut out = Vec::new();
    for r in &regions {
        let (pts, tris) = triangulate_region(r)?;
        let top = |p: Point| [p.x, p.y, surf.z(p)];
        let bot = |p: Point| [p.x, p.y, surf.z(p) - t];
        for tr in &tris {
            let (a, b, c) = (pts[tr[0]], pts[tr[1]], pts[tr[2]]);
            tri(&mut out, top(a), top(b), top(c), up);
            tri(&mut out, bot(a), bot(c), bot(b), down);
        }
        // sides: the material is on the left of every edge, so the outside is on the right
        for ring in std::iter::once(&r.outer).chain(r.holes.iter()) {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                let len = a.dist(&b);
                if len < 1e-9 {
                    continue;
                }
                let nrm = [(b.y - a.y) / len, -(b.x - a.x) / len, 0.];
                tri(&mut out, bot(a), bot(b), top(b), nrm);
                tri(&mut out, bot(a), top(b), top(a), nrm);
            }
        }
    }
    Some(out)
}

/// A triangle wound so that its front face looks along `n`.
fn tri_facing(out: &mut Vec<f32>, a: [f64; 3], b: [f64; 3], c: [f64; 3], n: [f64; 3]) {
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let cr = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    if cr[0] * n[0] + cr[1] * n[1] + cr[2] * n[2] >= 0.0 {
        tri(out, a, b, c, n);
    } else {
        tri(out, a, c, b, n);
    }
}

/// The roof as a shell: the top planes, the same planes dropped by the roof's thickness for the
/// underside, and plumb ends at the eaves and along gable ends. Hips and ridges of roofs with
/// different slopes on each side leave a hairline between undersides; the top is exact.
pub fn roof_mesh(doc: &Document, r: &Roof) -> Option<Vec<f32>> {
    let t = doc.roof_type(r.roof_type)?.thickness_mm();
    let g = doc.roof_geometry(r)?;
    let mut out = Vec::new();
    // the drop that keeps the thickness square to a plane of this slope
    let drop_for = |deg: f64| t / deg.to_radians().cos().max(0.05);
    for f in g.faces.iter().filter(|f| !f.vertical) {
        let plan: Vec<Point> = f.poly.iter().map(|q| Point::new(q[0], q[1])).collect();
        let Some((_, tris)) = triangulate_region(&crate::geom::Region { outer: plan, holes: vec![] }) else { continue };
        let d = drop_for(f.slope_deg);
        let sl = f.slope_deg.to_radians();
        // the plane normal: up, tilted away from the direction the roof rises
        let up = [f.outward[0] * sl.sin(), f.outward[1] * sl.sin(), sl.cos()];
        let down = [-up[0], -up[1], -up[2]];
        for tr in &tris {
            let (a, b, c) = (f.poly[tr[0]], f.poly[tr[1]], f.poly[tr[2]]);
            tri_facing(&mut out, a, b, c, up);
            tri_facing(&mut out, [a[0], a[1], a[2] - d], [b[0], b[1], b[2] - d], [c[0], c[1], c[2] - d], down);
        }
        // plumb end at the eave: the edge from the last corner back to the first
        let (a, b) = (f.poly[f.poly.len() - 1], f.poly[0]);
        let out_n = [f.outward[0], f.outward[1], 0.0];
        let (a2, b2) = ([a[0], a[1], a[2] - d], [b[0], b[1], b[2] - d]);
        tri_facing(&mut out, a, b, b2, out_n);
        tri_facing(&mut out, a, b2, a2, out_n);
    }
    // gable ends: the band between the top surface and the underside
    for f in g.faces.iter().filter(|f| f.vertical) {
        let n = f.poly.len();
        let out_n = [f.outward[0], f.outward[1], 0.0];
        for i in 0..n {
            let (a, b) = (f.poly[i], f.poly[(i + 1) % n]);
            // the eave edge (last to first) is the footprint line itself
            if i == n - 1 || ((a[2] - g.base_z).abs() < 1e-6 && (b[2] - g.base_z).abs() < 1e-6) {
                continue;
            }
            // the sloping plane that shares this edge sets how far the underside drops
            let near = |p: &[f64; 3], q: &[f64; 3]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-3 && (p[2] - q[2]).abs() < 1e-3;
            let deg = g
                .faces
                .iter()
                .filter(|o| !o.vertical)
                .find(|o| {
                    let m = o.poly.len();
                    (0..m).any(|k| (near(&o.poly[k], &a) && near(&o.poly[(k + 1) % m], &b)) || (near(&o.poly[k], &b) && near(&o.poly[(k + 1) % m], &a)))
                })
                .map_or(crate::roof::DEFAULT_SLOPE_DEG, |o| o.slope_deg);
            let d = drop_for(deg);
            let (a2, b2) = ([a[0], a[1], a[2] - d], [b[0], b[1], b[2] - d]);
            tri_facing(&mut out, a, b, b2, out_n);
            tri_facing(&mut out, a, b2, a2, out_n);
        }
    }
    // flat roofs are slabs, as floors are
    for reg in &g.flat {
        let (pts, tris) = triangulate_region(reg)?;
        let top = |p: Point| [p.x, p.y, g.base_z];
        let bot = |p: Point| [p.x, p.y, g.base_z - t];
        for tr in &tris {
            let (a, b, c) = (pts[tr[0]], pts[tr[1]], pts[tr[2]]);
            tri(&mut out, top(a), top(b), top(c), [0., 0., 1.]);
            tri(&mut out, bot(a), bot(c), bot(b), [0., 0., -1.]);
        }
        for ring in std::iter::once(&reg.outer).chain(reg.holes.iter()) {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                let len = a.dist(&b);
                if len < 1e-9 {
                    continue;
                }
                let nrm = [(b.y - a.y) / len, -(b.x - a.x) / len, 0.];
                tri(&mut out, bot(a), bot(b), top(b), nrm);
                tri(&mut out, bot(a), top(b), top(a), nrm);
            }
        }
    }
    // the flat top left by a cutoff level
    for cap in &g.caps {
        let plan: Vec<Point> = cap.iter().map(|q| Point::new(q[0], q[1])).collect();
        let Some((_, tris)) = triangulate_region(&crate::geom::Region { outer: plan, holes: vec![] }) else { continue };
        let z = cap[0][2];
        for tr in &tris {
            let (a, b, c) = (cap[tr[0]], cap[tr[1]], cap[tr[2]]);
            tri_facing(&mut out, a, b, c, [0., 0., 1.]);
            tri_facing(&mut out, [a[0], a[1], z - t], [b[0], b[1], z - t], [c[0], c[1], z - t], [0., 0., -1.]);
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn wall_mesh_has_expected_shape() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0); // Concrete 200
        let id = d.add_wall(wt, lvl, p(0., 0.), p(5000., 0.), 2700.).unwrap();
        let Some(Element::Wall(w)) = d.get(id).cloned() else { panic!() };
        let o = d.wall_outlines()[&id];
        let m = wall_mesh(&d, &w, &o, &[]).unwrap();
        // 12 triangles, 3 vertices, 6 floats
        assert_eq!(m.len(), 12 * 3 * 6);
        let xs: Vec<f32> = m.chunks(6).map(|v| v[0]).collect();
        let zs: Vec<f32> = m.chunks(6).map(|v| v[2]).collect();
        assert_eq!(xs.iter().cloned().fold(f32::MIN, f32::max), 5.0);
        assert_eq!(zs.iter().cloned().fold(f32::MIN, f32::max), 2.7);
        assert_eq!(zs.iter().cloned().fold(f32::MAX, f32::min), 0.0);
    }

    #[test]
    fn layered_walls_have_no_faces_inside_them_round_an_opening() {
        // A window cuts the wall into height bands. Only the sill, the head, the top and the base are flat faces;
        // a band sitting on another has no face between them, which drew as lines across the wall in 3D.
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().into_iter().find(|(_, t)| t.name == "Double Brick Cavity").unwrap().0;
        let id = d.add_wall(wt, lvl, p(0., 0.), p(6000., 0.), 2700.).unwrap();
        let ty = d.family_types().into_iter().find(|(_, t)| d.family(t.family).is_some_and(|f| f.category == FamilyCategory::Windows)).unwrap().0;
        let win = d.place_on_wall(ty, id, p(3000., 0.)).unwrap();
        let f = d.instance_frame(win).unwrap();
        let Some(Element::Wall(w)) = d.get(id).cloned() else { panic!() };
        let wl = d.wall_layer_geometry().remove(&id).unwrap();
        let meshes = wall_layer_meshes(&d, id, &w, &wl);
        assert!(!meshes.is_empty());
        let (z_top, half) = (2.7, f.width / 2000.0 + 0.2);
        for m in &meshes {
            for t in m.verts.chunks(18) {
                let (zs, nz) = ([t[2], t[8], t[14]], t[5]);
                if nz.abs() < 0.5 {
                    continue; // an upright face
                }
                let z = zs[0] as f64;
                if z.abs() < 1e-4 || (z - z_top).abs() < 1e-4 {
                    continue; // the base and the top of the wall
                }
                // a flat face part way up must be the sill or the head of the window
                let cx = (t[0] + t[6] + t[12]) as f64 / 3.0;
                assert!((cx - 3.0).abs() < half, "a face inside the wall at z {z}, x {cx}");
                assert!((z - f.z0 / 1000.0).abs() < 1e-3 || (z - f.z1 / 1000.0).abs() < 1e-3, "flat face at {z}");
            }
        }
    }

    #[test]
    fn every_triangle_faces_along_its_normal() {
        // winding must agree with the stored normal or back-face culling hides the wall
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[1].0);
        let w1 = d.add_wall(wt, lvl, p(0., 0.), p(5000., 3000.), 2700.).unwrap();
        let ft = d.floor_types()[0].0;
        let f1 = d.add_floor(ft, lvl, vec![p(0., 0.), p(6000., 0.), p(6000., 2000.), p(2000., 2000.), p(2000., 6000.), p(0., 6000.)]).unwrap();
        for id in [w1, f1] {
            let m = match d.get(id).cloned().unwrap() {
                Element::Wall(w) => wall_mesh(&d, &w, &d.wall_outlines()[&id], &[]).unwrap(),
                Element::Floor(f) => floor_mesh(&d, &f).unwrap(),
                _ => unreachable!(),
            };
            for t in m.chunks(18) {
                let v = |i: usize| [t[i * 6] as f64, t[i * 6 + 1] as f64, t[i * 6 + 2] as f64];
                let (a, b, c) = (v(0), v(1), v(2));
                let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                let stored = [t[3] as f64, t[4] as f64, t[5] as f64];
                let dot = n[0] * stored[0] + n[1] * stored[1] + n[2] * stored[2];
                assert!(dot > 0.0, "triangle winding disagrees with its normal");
            }
        }
    }

    #[test]
    fn floor_sits_below_its_level() {
        let mut d = Document::new_project();
        let lvl = d.levels()[1].0; // Level 2 at 3000
        let ft = d.floor_types()[0].0; // 200 slab
        let id = d.add_floor(ft, lvl, vec![p(0., 0.), p(4000., 0.), p(4000., 3000.), p(0., 3000.)]).unwrap();
        let Some(Element::Floor(f)) = d.get(id).cloned() else { panic!() };
        let m = floor_mesh(&d, &f).unwrap();
        let zs: Vec<f32> = m.chunks(6).map(|v| v[2]).collect();
        assert_eq!(zs.iter().cloned().fold(f32::MIN, f32::max), 3.0);
        assert!((zs.iter().cloned().fold(f32::MAX, f32::min) - 2.8).abs() < 1e-6);
    }

    #[test]
    fn roof_mesh_covers_the_top_and_stays_inside_its_box() {
        use crate::curve::Curve;
        use crate::roof::RoofEdge;
        let mut d = Document::new_project();
        let (lvl, rt) = (d.levels()[1].0, d.roof_types()[0].0);
        let c = [p(0., 0.), p(10000., 0.), p(10000., 6000.), p(0., 6000.)];
        let edges: Vec<RoofEdge> = (0..4).map(|i| RoofEdge::new(Curve::line(c[i], c[(i + 1) % 4]))).collect();
        let id = d.add_roof_sketch(rt, lvl, edges).unwrap();
        let r = d.roof(id).unwrap().clone();
        let m = roof_mesh(&d, &r).unwrap();
        assert_eq!(m.len() % 18, 0);
        let zs: Vec<f32> = m.chunks(6).map(|v| v[2]).collect();
        let top = zs.iter().cloned().fold(f32::MIN, f32::max);
        let g = d.roof_geometry(&r).unwrap();
        assert!((top as f64 - g.top_z / 1000.0).abs() < 1e-3);
        // upward facing triangles cover the sloping surface area
        let mut up_area = 0.0;
        for tr in m.chunks(18) {
            if tr[5] > 0.1 {
                let v = |i: usize| [tr[i * 6] as f64, tr[i * 6 + 1] as f64, tr[i * 6 + 2] as f64];
                up_area += crate::roof::poly_area_3d(&[[v(0)[0], v(0)[1], v(0)[2]], [v(1)[0], v(1)[1], v(1)[2]], [v(2)[0], v(2)[1], v(2)[2]]]);
            }
        }
        assert!((up_area - g.surface_area_mm2() / 1e6).abs() < 0.01, "{up_area} vs {}", g.surface_area_mm2() / 1e6);
        // winding agrees with the stored normals: the front face looks along them
        for tr in m.chunks(18) {
            let v = |i: usize| [tr[i * 6] as f64, tr[i * 6 + 1] as f64, tr[i * 6 + 2] as f64];
            let (a, b, c) = (v(0), v(1), v(2));
            let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let cr = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            assert!(cr[0] * tr[3] as f64 + cr[1] * tr[4] as f64 + cr[2] * tr[5] as f64 > -1e-9);
        }
    }
}
