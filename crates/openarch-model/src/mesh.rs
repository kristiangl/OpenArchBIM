//! Triangle meshes for display. Output is interleaved `x y z nx ny nz` in
//! metres (the viewport's unit), non-indexed, counter-clockwise front faces.

use crate::doc::Document;
use crate::element::{Floor, Wall};
use crate::roof::Roof;
use crate::family::{FamilyCategory, Opening};
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

/// Shapes (an outline and its holes each) swept from `z0` to `z1`: model text's letters.
pub fn shapes_prism(shapes: &[crate::layers::Shape], z0: f64, z1: f64) -> Vec<f32> {
    let mut out = vec![];
    cap(&mut out, shapes, z1, true);
    cap(&mut out, shapes, z0, false);
    for s in shapes {
        for (k, ring) in s.iter().enumerate() {
            // sides face out of the solid: an outline counter-clockwise, a hole clockwise
            let mut r = ring.clone();
            if (signed_area(&r) > 0.0) != (k == 0) {
                r.reverse();
            }
            prism_sides(&mut out, &r, z0, z1);
        }
    }
    out
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

/// A footprint swept from `z0` to `z1` (a curtain wall's panel or mullion, which on a curved wall follows the curve),
/// as triangles.
pub fn solid_mesh(ring: &[Point], z0: f64, z1: f64) -> Vec<f32> {
    let mut ring = ring.to_vec();
    if crate::geom::signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    let fan = || (1..ring.len().saturating_sub(1)).map(|i| [0, i, i + 1]).collect();
    let tris: Vec<[usize; 3]> = if ring.len() > 4 { crate::geom::triangulate(&ring).unwrap_or_else(fan) } else { fan() };
    let mut out = vec![];
    prism(&mut out, &ring, &tris, z0, z1);
    out
}

/// Mesh for a wall. `outline` is its joined plan outline (see
/// `Document::wall_outlines`); `openings` are the holes cut by hosted doors and
/// windows (see `Document::wall_openings`).
pub fn wall_mesh(doc: &Document, w: &Wall, outline: &[Point; 4], openings: &[Opening]) -> Option<Vec<f32>> {
    if w.curve.is_some() {
        return None; // curved walls are meshed layer by layer
    }
    // an edited profile, or a door or window with a shaped head: the outline less the openings, across the wall
    let face = doc.wall_profile_shapes(w).or_else(|| openings.iter().any(|o| doc.opening_shape(o).is_some()).then(|| vec![doc.wall_rect(w)]));
    if let Some(shapes) = face {
        let z0 = doc.wall_extent(w).0;
        let mut out = vec![];
        profile_prism(&mut out, w, z0, outline, &doc.less_openings(&shapes, openings, w));
        return Some(out);
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
    if let Some(shapes) = doc.wall_face_solid(id, w) {
        // an edited profile (or a shaped opening): the outline across each layer, its ends where the layer's ends are
        let mut out = vec![];
        for l in wl.layers.iter().filter(|l| !l.is_void) {
            let mut verts = vec![];
            for ring in l.shapes.iter().filter_map(|s| s.first()) {
                profile_prism(&mut verts, w, z0, ring, &shapes);
            }
            if !verts.is_empty() {
                out.push(LayerMesh { material: l.material.clone(), color: doc.material(&l.material).map_or([200, 200, 200], |m| m.graphics.shading_color), verts });
            }
        }
        return out;
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
pub fn instance_meshes(doc: &Document, id: u64) -> Option<(FamilyCategory, Vec<f32>, Vec<f32>)> {
    let crate::element::Element::FamilyInstance(inst) = doc.get(id)? else { return None };
    let (fam, _) = doc.instance_family(inst)?;
    let f = doc.instance_frame(id)?;
    let cat = fam.category;
    if let Some(solved) = doc.instance_solved(id) {
        let s = solved.as_ref().as_ref().ok()?;
        let (mut solid, mut glass) = (Vec::new(), Vec::new());
        for form in s.forms.iter().filter(|x| !x.void && x.vis.present && x.vis.three_d) {
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
    // a family without a body draws nothing
    Some((cat, Vec::new(), Vec::new()))
}

/// A curved wall's frame for the surface patterns and textures on its faces: the centre of its arc and the angle
/// the middle of the wall stands at. Along a face the pattern's s is the arc length, `arc_s`, so it runs round the
/// wall as it runs along a straight one; 3D views and elevations both lay it so.
pub fn arc_frame(w: &Wall) -> Option<(Point, f64)> {
    match w.curve.as_ref()? {
        crate::curve::Curve::Arc { start, end, centre, ccw } => {
            let a0 = (start.y - centre.y).atan2(start.x - centre.x);
            let a1 = (end.y - centre.y).atan2(end.x - centre.x);
            let tau = std::f64::consts::TAU;
            let sweep = if *ccw { (a1 - a0).rem_euclid(tau) } else { -(a0 - a1).rem_euclid(tau) };
            let mid = a0 + sweep / 2.0;
            Some((*centre, (mid + std::f64::consts::PI).rem_euclid(tau) - std::f64::consts::PI))
        }
        crate::curve::Curve::Circle { centre, .. } => Some((*centre, 0.0)),
        _ => None,
    }
}

/// Where plan point `p` on a curved face lies along it (mm): its radius times its angle from the frame's middle
/// (within half a turn) plus the middle's angle, counter-clockwise on a face whose normal `n` points away from the
/// centre and clockwise on one facing it, the way the face's own frame runs (along is Z x n).
pub fn arc_s(centre: Point, mid: f64, p: Point, n: Point) -> f64 {
    let (dx, dy) = (p.x - centre.x, p.y - centre.y);
    let (mx, my) = (mid.cos(), mid.sin());
    let delta = (mx * dy - my * dx).atan2(mx * dx + my * dy);
    let sign = if dx * n.x + dy * n.y >= 0.0 { 1.0 } else { -1.0 };
    sign * dx.hypot(dy) * (mid + delta)
}

pub fn floor_mesh(doc: &Document, f: &Floor) -> Option<Vec<f32>> {
    slab_mesh(doc, f, doc.floor_type(f.floor_type)?.thickness_mm())
}

/// A floor by its type's layers: one closed solid per layer in the layer's material, as sections cut it, so the
/// slab edge shows the build-up.
pub fn floor_layer_meshes(doc: &Document, f: &Floor) -> Vec<(Option<String>, Vec<f32>)> {
    let Some(ty) = doc.floor_type(f.floor_type) else { return vec![] };
    slab_layer_meshes(doc, f, &ty.layers, ty.layers.first())
}

/// A ceiling by its type's layers (top first), the same way; a basic ceiling is the one plane in the material
/// facing the room.
pub fn ceiling_layer_meshes(doc: &Document, c: &crate::ceiling::Ceiling) -> Vec<(Option<String>, Vec<f32>)> {
    let Some(ty) = doc.ceiling_type(c.ceiling_type) else { return vec![] };
    slab_layer_meshes(doc, &c.as_slab(ty.thickness_mm()), &ty.layers, ty.layers.last())
}

/// Each layer of a slab (top first) as a solid of its own; with fewer than two layers that have a thickness, the
/// whole slab in the one layer's material (or `whole`'s).
fn slab_layer_meshes(doc: &Document, f: &Floor, layers: &[crate::element::Layer], whole: Option<&crate::element::Layer>) -> Vec<(Option<String>, Vec<f32>)> {
    let t: f64 = layers.iter().map(|l| l.thickness_mm).sum();
    let thick: Vec<_> = layers.iter().filter(|l| l.thickness_mm > 0.0 || l.variable).collect();
    if thick.len() < 2 {
        let mat = thick.first().copied().or(whole).map(|l| l.material.clone());
        return slab_mesh(doc, f, t).map(|m| vec![(mat, m)]).unwrap_or_default();
    }
    if let Some(tin) = doc.floor_shape(f) {
        let bot = doc.floor_bottoms(f, &tin);
        let spans: Vec<Vec<(f64, f64)>> = tin.verts.iter().zip(&bot).map(|(v, &b)| crate::floorshape::layer_spans(layers, v[2], b)).collect();
        return layers.iter().enumerate().filter_map(|(k, l)| {
            let m = layer_tin_mesh(&tin, &spans, k);
            (!m.is_empty()).then(|| (Some(l.material.clone()), m))
        }).collect();
    }
    let mut out = vec![];
    let mut o = 0.0;
    for l in layers {
        if l.thickness_mm > 0.0 {
            if let Some(m) = slab_band(doc, f, o, o + l.thickness_mm) {
                out.push((Some(l.material.clone()), m));
            }
        }
        o += l.thickness_mm;
    }
    out
}

/// Layer `k` of a shaped top: the solid between its span's top and bottom under each corner (`spans`, from
/// `layer_spans`).
fn layer_tin_mesh(tin: &crate::shape::Tin, spans: &[Vec<(f64, f64)>], k: usize) -> Vec<f32> {
    let top = crate::shape::Tin { verts: tin.verts.iter().zip(spans).map(|(v, s)| [v[0], v[1], s[k].0]).collect(), tris: tin.tris.clone() };
    let low: Vec<f64> = spans.iter().map(|s| s[k].1).collect();
    let th = spans.iter().map(|s| s[k].0 - s[k].1).fold(0.0, f64::max);
    if th < 0.5 {
        return vec![];
    }
    tin_solid_mesh(&top, &low, th)
}

/// A ceiling: its slab from the underside up through its layers; a basic ceiling is one plane seen from both sides.
pub fn ceiling_mesh(doc: &Document, c: &crate::ceiling::Ceiling) -> Option<Vec<f32>> {
    let t = doc.ceiling_thickness(c);
    slab_mesh(doc, &c.as_slab(t), t)
}

/// A toposolid: its surface, the same surface lowered by the type's thickness underneath, and upright sides round
/// the boundary. A type with no layers is drawn as the surface alone.
pub fn toposolid_mesh(doc: &Document, t: &crate::topo::Toposolid) -> Option<Vec<f32>> {
    let (tin, bot, _) = doc.toposolid_surface(t)?;
    Some(tin_solid_mesh(&tin, &bot, doc.toposolid_thickness(t)))
}

/// A toposolid by its type's layers: each layer a solid of its own (its material's), between where it starts and
/// ends under each corner of the surface as built (excavations taken out), as sections cut it.
pub fn toposolid_layer_meshes(doc: &Document, t: &crate::topo::Toposolid) -> Vec<(Option<String>, Vec<f32>)> {
    let (Some((tin, bot, _)), Some(ty)) = (doc.toposolid_surface(t), doc.toposolid_type(t.toposolid_type)) else { return vec![] };
    if ty.layers.len() < 2 {
        return toposolid_mesh(doc, t).map(|m| vec![(ty.layers.first().map(|l| l.material.clone()), m)]).unwrap_or_default();
    }
    let spans: Vec<Vec<(f64, f64)>> = tin.verts.iter().zip(&bot).map(|(v, &b)| ty.layer_spans(v[2], b)).collect();
    let mut out = vec![];
    for (k, l) in ty.layers.iter().enumerate() {
        if l.thickness_mm <= 0.0 { continue; }
        // a layer stays between the top as built and the underside: an excavation takes it out where it goes deep
        let (top, low): (Vec<f64>, Vec<f64>) = tin.verts.iter().zip(&bot).zip(&spans).map(|((v, &b), s)| {
            let lo = s[k].1.max(b).min(v[2]);
            (s[k].0.min(v[2]).max(lo), lo)
        }).unzip();
        let lt = crate::shape::Tin { verts: tin.verts.iter().zip(&top).map(|(v, &z)| [v[0], v[1], z]).collect(), tris: tin.tris.clone() };
        let m = tin_solid_mesh(&lt, &low, l.thickness_mm);
        if !m.is_empty() { out.push((Some(l.material.clone()), m)); }
    }
    out
}

/// A solid between a triangulated top and an underside under each of its corners (`bot`): the top, the underside
/// and the sides round its boundary. Where top and underside meet (an excavation right through) nothing is drawn.
fn tin_solid_mesh(tin: &crate::shape::Tin, bot: &[f64], th: f64) -> Vec<f32> {
    let mut out = Vec::new();
    let normal = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        [n[0] / l, n[1] / l, n[2] / l]
    };
    let low = |i: usize| [tin.verts[i][0], tin.verts[i][1], bot[i]];
    for k in &tin.tris {
        // nothing left where an excavation goes right through
        if th > 0.0 && k.iter().all(|&i| tin.verts[i][2] - bot[i] < 0.5) { continue; }
        let (a, b, c) = (tin.verts[k[0]], tin.verts[k[1]], tin.verts[k[2]]);
        tri(&mut out, a, b, c, normal(a, b, c));
        if th > 0.0 {
            let (a, b, c) = (low(k[0]), low(k[1]), low(k[2]));
            tri(&mut out, a, c, b, normal(a, c, b));
        }
    }
    if th > 0.0 {
        for [i, j] in tin.boundary_edges() {
            let (a, b) = (tin.verts[i], tin.verts[j]);
            let (la, lb) = (low(i), low(j));
            if a[2] - la[2] < 0.5 && b[2] - lb[2] < 0.5 { continue; }
            // the outside is on the right of a boundary edge
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = dx.hypot(dy).max(1e-12);
            let n = [dy / l, -dx / l, 0.0];
            tri(&mut out, a, la, lb, n);
            tri(&mut out, a, lb, b, n);
        }
    }
    out
}

/// A slab `t` thick under the top surface of `f`'s level, offset and slope, filling its loops. A shape edited
/// floor follows its points and split lines (floorshape.rs).
fn slab_mesh(doc: &Document, f: &Floor, t: f64) -> Option<Vec<f32>> {
    if let Some(tin) = doc.floor_shape(f) {
        let bot = doc.floor_bottoms(f, &tin);
        return Some(tin_solid_mesh(&tin, &bot, t));
    }
    slab_band(doc, f, 0.0, t)
}

/// The part of a slab that isn't shape edited from `a` to `b` below its top (measured plumb, `a` <= `b`), closed.
fn slab_band(doc: &Document, f: &Floor, a: f64, b: f64) -> Option<Vec<f32>> {
    let t = b - a;
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
        let top = |p: Point| [p.x, p.y, surf.z(p) - a];
        let bot = |p: Point| [p.x, p.y, surf.z(p) - b];
        for tr in &tris {
            let (a, b, c) = (pts[tr[0]], pts[tr[1]], pts[tr[2]]);
            tri(&mut out, top(a), top(b), top(c), up);
            tri(&mut out, bot(a), bot(c), bot(b), down);
        }
        if t <= 0.0 {
            continue;
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

/// A straight wall's edited profile (`shapes`, along the wall and up from `z0`) extruded across the plan `ring` of the
/// wall or one of its layers. The outline's points on the wall's ends go to where the ring ends on each face, so the
/// ends follow the wall's joins.
fn profile_prism(out: &mut Vec<f32>, w: &Wall, z0: f64, ring: &[Point], shapes: &[crate::layers::Shape]) {
    let len = w.length();
    if len < 1e-6 || ring.len() < 3 {
        return;
    }
    let d = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
    let n = Point::new(-d.y, d.x);
    let (along, across) = (|p: &Point| (p.x - w.start.x) * d.x + (p.y - w.start.y) * d.y, |p: &Point| (p.x - w.start.x) * n.x + (p.y - w.start.y) * n.y);
    let cs: Vec<f64> = ring.iter().map(across).collect();
    let (c0, c1) = (cs.iter().cloned().fold(f64::INFINITY, f64::min), cs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    if c1 - c0 < 1e-6 {
        return;
    }
    // where the ring starts and ends along the wall on the line `c` across it
    let ends = |c: f64| -> (f64, f64) {
        let mut xs = vec![];
        for i in 0..ring.len() {
            let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
            let (cp, cq) = (across(&p), across(&q));
            if (cp - c) * (cq - c) <= 0.0 && (cp - cq).abs() > 1e-9 {
                let t = (c - cp) / (cq - cp);
                xs.push(along(&p) + (along(&q) - along(&p)) * t);
            }
        }
        (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
    };
    let eps = (c1 - c0) * 1e-3;
    let sides = [(c0, ends(c0 + eps)), (c1, ends(c1 - eps))];
    if sides.iter().any(|(_, (a, b))| !a.is_finite() || !b.is_finite()) {
        return;
    }
    let at = |q: Point, side: usize| -> [f64; 3] {
        let (c, (s, e)) = sides[side];
        let x = if q.x <= 1.0 { s } else if q.x >= len - 1.0 { e } else { q.x };
        [w.start.x + d.x * x + n.x * c, w.start.y + d.y * x + n.y * c, z0 + q.y]
    };
    for shape in shapes {
        extrude_face(out, shape, d, &at);
    }
}

/// A curtain wall's piece cut to an edited profile: its shape in the face of straight wall `w`, extruded across it.
pub fn face_mesh(w: &Wall, f: &crate::curtain::FacePiece) -> Vec<f32> {
    let len = w.length();
    let mut out = vec![];
    if len < 1e-6 {
        return out;
    }
    let d = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
    let at = |q: Point, side: usize| -> [f64; 3] {
        let c = if side == 0 { f.c0 } else { f.c1 };
        [w.start.x + d.x * q.x - d.y * c, w.start.y + d.y * q.x + d.x * c, q.y]
    };
    extrude_face(&mut out, &f.shape, d, &at);
    out
}

/// A shape in a wall's face (outline then holes, along the wall `d` and up) extruded across it: `at(point, 0)` is a
/// point on the back face, `at(point, 1)` on the front (the wall's left).
fn extrude_face(out: &mut Vec<f32>, shape: &[Vec<Point>], d: Point, at: &dyn Fn(Point, usize) -> [f64; 3]) {
    let n = Point::new(-d.y, d.x);
    let (front, back) = ([n.x, n.y, 0.0], [-n.x, -n.y, 0.0]);
    let Some(outer) = shape.first() else { return };
    let region = crate::geom::Region { outer: outer.clone(), holes: shape[1..].to_vec() };
    if let Some((pts, tris)) = triangulate_region(&region) {
        for t in tris {
            tri_facing(out, at(pts[t[0]], 1), at(pts[t[1]], 1), at(pts[t[2]], 1), front);
            tri_facing(out, at(pts[t[0]], 0), at(pts[t[1]], 0), at(pts[t[2]], 0), back);
        }
    }
    // the edges round the outline and its holes, across the wall: each faces away from the solid
    for (k, r) in shape.iter().enumerate() {
        let mut r = r.clone();
        if (signed_area(&r) > 0.0) != (k == 0) {
            r.reverse();
        }
        for i in 0..r.len() {
            let (p, q) = (r[i], r[(i + 1) % r.len()]);
            let (ex, ey) = (q.x - p.x, q.y - p.y);
            let l = ex.hypot(ey);
            if l < 1e-9 {
                continue;
            }
            let nrm = [d.x * ey / l, d.y * ey / l, -ex / l];
            tri_facing(out, at(p, 0), at(q, 0), at(q, 1), nrm);
            tri_facing(out, at(p, 0), at(q, 1), at(p, 1), nrm);
        }
    }
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

/// The roof as a shell (the whole type's thickness).
pub fn roof_mesh(doc: &Document, r: &Roof) -> Option<Vec<f32>> {
    let t = doc.roof_type(r.roof_type)?.thickness_mm();
    let g = doc.roof_geometry(r)?;
    let mut out = roof_band(&g, 0.0, t);
    // a shape edited flat roof: the solid under its surface
    if let Some((tin, bot)) = &g.shaped {
        out.extend(tin_solid_mesh(tin, bot, t));
    }
    (!out.is_empty()).then_some(out)
}

/// A roof by its type's layers (top first): one shell per layer in the layer's material, so eaves, verges and
/// slab edges show the build-up.
pub fn roof_layer_meshes(doc: &Document, r: &Roof) -> Vec<(Option<String>, Vec<f32>)> {
    let (Some(ty), Some(g)) = (doc.roof_type(r.roof_type), doc.roof_geometry(r)) else { return vec![] };
    if ty.layers.iter().filter(|l| l.thickness_mm > 0.0 || l.variable).count() < 2 {
        return roof_mesh(doc, r).map(|m| vec![(ty.layers.first().map(|l| l.material.clone()), m)]).unwrap_or_default();
    }
    let spans: Option<Vec<Vec<(f64, f64)>>> = g.shaped.as_ref().map(|(tin, bot)| tin.verts.iter().zip(bot).map(|(v, &b)| crate::floorshape::layer_spans(&ty.layers, v[2], b)).collect());
    let mut out = vec![];
    let mut o = 0.0;
    for (k, l) in ty.layers.iter().enumerate() {
        let mut m = if l.thickness_mm > 0.0 { roof_band(&g, o, o + l.thickness_mm) } else { vec![] };
        if let (Some((tin, _)), Some(sp)) = (&g.shaped, &spans) {
            m.extend(layer_tin_mesh(tin, sp, k));
        }
        o += l.thickness_mm;
        if !m.is_empty() {
            out.push((Some(l.material.clone()), m));
        }
    }
    out
}

/// The part of a roof from `a` to `b` under its top surface, measured square to each plane (plumb on flat parts),
/// as a closed shell: its sloping planes, plumb ends at the eaves and along gable ends, flat parts as slabs, and the
/// flat tops a cutoff level leaves. Hips and ridges of roofs with different slopes on each side leave a hairline
/// between undersides; the top is exact. A shape edited flat part is drawn by the caller.
fn roof_band(g: &crate::roof::RoofGeometry, a: f64, b: f64) -> Vec<f32> {
    let mut out = Vec::new();
    // the drop that keeps a depth square to a plane of this slope
    let drop_for = |deg: f64, x: f64| x / deg.to_radians().cos().max(0.05);
    for f in g.faces.iter().filter(|f| !f.vertical) {
        let plan: Vec<Point> = f.poly.iter().map(|q| Point::new(q[0], q[1])).collect();
        let Some((_, tris)) = triangulate_region(&crate::geom::Region { outer: plan, holes: vec![] }) else { continue };
        let (da, db) = (drop_for(f.slope_deg, a), drop_for(f.slope_deg, b));
        let sl = f.slope_deg.to_radians();
        // the plane normal: up, tilted away from the direction the roof rises
        let up = [f.outward[0] * sl.sin(), f.outward[1] * sl.sin(), sl.cos()];
        let down = [-up[0], -up[1], -up[2]];
        let at = |q: [f64; 3], d: f64| [q[0], q[1], q[2] - d];
        for tr in &tris {
            let (p, q, r) = (f.poly[tr[0]], f.poly[tr[1]], f.poly[tr[2]]);
            tri_facing(&mut out, at(p, da), at(q, da), at(r, da), up);
            tri_facing(&mut out, at(p, db), at(q, db), at(r, db), down);
        }
        // plumb end at the eave: the edge from the last corner back to the first
        let (p, q) = (f.poly[f.poly.len() - 1], f.poly[0]);
        let out_n = [f.outward[0], f.outward[1], 0.0];
        tri_facing(&mut out, at(p, da), at(q, da), at(q, db), out_n);
        tri_facing(&mut out, at(p, da), at(q, db), at(p, db), out_n);
    }
    // gable ends: the band between the planes' top and underside
    for f in g.faces.iter().filter(|f| f.vertical) {
        let n = f.poly.len();
        let out_n = [f.outward[0], f.outward[1], 0.0];
        for i in 0..n {
            let (p, q) = (f.poly[i], f.poly[(i + 1) % n]);
            // the eave edge (last to first) is the footprint line itself
            if i == n - 1 || ((p[2] - g.base_z).abs() < 1e-6 && (q[2] - g.base_z).abs() < 1e-6) {
                continue;
            }
            // the sloping plane that shares this edge sets how far the band drops
            let near = |u: &[f64; 3], v: &[f64; 3]| (u[0] - v[0]).hypot(u[1] - v[1]) < 1e-3 && (u[2] - v[2]).abs() < 1e-3;
            let deg = g
                .faces
                .iter()
                .filter(|o| !o.vertical)
                .find(|o| {
                    let m = o.poly.len();
                    (0..m).any(|k| (near(&o.poly[k], &p) && near(&o.poly[(k + 1) % m], &q)) || (near(&o.poly[k], &q) && near(&o.poly[(k + 1) % m], &p)))
                })
                .map_or(crate::roof::DEFAULT_SLOPE_DEG, |o| o.slope_deg);
            let (da, db) = (drop_for(deg, a), drop_for(deg, b));
            let at = |u: [f64; 3], d: f64| [u[0], u[1], u[2] - d];
            tri_facing(&mut out, at(p, da), at(q, da), at(q, db), out_n);
            tri_facing(&mut out, at(p, da), at(q, db), at(p, db), out_n);
        }
    }
    // flat roofs are slabs, as floors are
    for reg in &g.flat {
        let Some((pts, tris)) = triangulate_region(reg) else { continue };
        let top = |p: Point| [p.x, p.y, g.base_z - a];
        let bot = |p: Point| [p.x, p.y, g.base_z - b];
        for tr in &tris {
            let (p, q, r) = (pts[tr[0]], pts[tr[1]], pts[tr[2]]);
            tri(&mut out, top(p), top(q), top(r), [0., 0., 1.]);
            tri(&mut out, bot(p), bot(r), bot(q), [0., 0., -1.]);
        }
        for ring in std::iter::once(&reg.outer).chain(reg.holes.iter()) {
            let n = ring.len();
            for i in 0..n {
                let (p, q) = (ring[i], ring[(i + 1) % n]);
                let len = p.dist(&q);
                if len < 1e-9 {
                    continue;
                }
                let nrm = [(q.y - p.y) / len, -(q.x - p.x) / len, 0.];
                tri(&mut out, bot(p), bot(q), top(q), nrm);
                tri(&mut out, bot(p), top(q), top(p), nrm);
            }
        }
    }
    // the flat top left by a cutoff level
    for cap in &g.caps {
        let plan: Vec<Point> = cap.iter().map(|q| Point::new(q[0], q[1])).collect();
        let Some((_, tris)) = triangulate_region(&crate::geom::Region { outer: plan, holes: vec![] }) else { continue };
        let z = cap[0][2];
        for tr in &tris {
            let (p, q, r) = (cap[tr[0]], cap[tr[1]], cap[tr[2]]);
            tri_facing(&mut out, [p[0], p[1], z - a], [q[0], q[1], z - a], [r[0], r[1], z - a], [0., 0., 1.]);
            tri_facing(&mut out, [p[0], p[1], z - b], [q[0], q[1], z - b], [r[0], r[1], z - b], [0., 0., -1.]);
        }
    }
    out
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

    #[test]
    fn floors_roofs_and_ceilings_draw_each_layer_in_its_material() {
        use crate::curve::Curve;
        use crate::roof::RoofEdge;
        let mut d = Document::new_project();
        let lvl = d.levels()[1].0;
        let zr = |m: &[f32]| m.chunks(6).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(f64::from(v[2]) * 1000.0), b.max(f64::from(v[2]) * 1000.0)));
        // a floor of several layers: one closed solid each, stacked down from the top
        let (ft, fty) = d.floor_types().into_iter().find(|(_, t)| t.layers.iter().filter(|l| l.thickness_mm > 0.0).count() > 1).map(|(i, t)| (i, t.clone())).unwrap();
        let id = d.add_floor(ft, lvl, vec![p(0., 0.), p(4000., 0.), p(4000., 3000.), p(0., 3000.)]).unwrap();
        let Some(Element::Floor(f)) = d.get(id).cloned() else { panic!() };
        let parts = floor_layer_meshes(&d, &f);
        let layers: Vec<_> = fty.layers.iter().filter(|l| l.thickness_mm > 0.0).collect();
        assert_eq!(parts.len(), layers.len());
        let mut z = d.floor_base_z(&f);
        for ((mat, m), l) in parts.iter().zip(&layers) {
            assert_eq!(mat.as_deref(), Some(l.material.as_str()));
            assert_eq!(m.len(), 12 * 18, "a box per layer");
            let (lo, hi) = zr(m);
            assert!((hi - z).abs() < 0.01 && (lo - (z - l.thickness_mm)).abs() < 0.01, "{}: {lo}..{hi}", l.material);
            z -= l.thickness_mm;
        }
        // a roof of several layers: the first takes the top, the last the underside of the whole roof
        let (rt, rty) = d.roof_types().into_iter().find(|(_, t)| t.layers.iter().filter(|l| l.thickness_mm > 0.0).count() > 1).map(|(i, t)| (i, t.clone())).unwrap();
        let c = [p(0., 0.), p(10000., 0.), p(10000., 6000.), p(0., 6000.)];
        let edges: Vec<RoofEdge> = (0..4).map(|i| RoofEdge::new(Curve::line(c[i], c[(i + 1) % 4]))).collect();
        let id = d.add_roof_sketch(rt, lvl, edges).unwrap();
        let r = d.roof(id).unwrap().clone();
        let parts = roof_layer_meshes(&d, &r);
        assert_eq!(parts.len(), rty.layers.iter().filter(|l| l.thickness_mm > 0.0).count());
        assert_eq!(parts[0].0.as_deref(), Some(rty.layers[0].material.as_str()));
        let whole = zr(&roof_mesh(&d, &r).unwrap());
        assert!((zr(&parts[0].1).1 - whole.1).abs() < 0.01);
        assert!((zr(&parts.last().unwrap().1).0 - whole.0).abs() < 0.01);
        let tops: Vec<f64> = parts.iter().map(|(_, m)| zr(m).1).collect();
        assert!(tops.windows(2).all(|w| w[1] < w[0]), "each layer lies under the one before: {tops:?}");
        // a ceiling: its layers the same way, a one-layer ceiling in the material facing the room
        for (ct, cty) in d.ceiling_types().into_iter().map(|(i, t)| (i, t.clone())).collect::<Vec<_>>() {
            let c = crate::ceiling::Ceiling::new(ct, lvl, vec![(0..4).map(|i| Curve::line([p(0., 0.), p(3000., 0.), p(3000., 3000.), p(0., 3000.)][i], [p(0., 0.), p(3000., 0.), p(3000., 3000.), p(0., 3000.)][(i + 1) % 4])).collect()], 2400.0);
            let parts = ceiling_layer_meshes(&d, &c);
            let n = cty.layers.iter().filter(|l| l.thickness_mm > 0.0).count();
            assert_eq!(parts.len(), n.max(1), "{}", cty.name);
            assert_eq!(parts.last().unwrap().0.as_deref(), cty.layers.last().map(|l| l.material.as_str()));
        }
    }
}

/// Line markings, parking rows (their lines and wheel stops) and kerbs, laid on the ground under them: each item's
/// meshes with its kind, colour and id.
pub fn traffic_meshes(doc: &Document) -> Vec<(crate::Id, &'static str, [u8; 3], Vec<f32>)> {
    use crate::traffic::{Paint, WHEEL_STOP};
    // a painted strip along a polyline, `w` wide, 3 mm above the ground, broken every half metre so it follows it
    let ribbon = |out: &mut Vec<f32>, pts: &[Point], w: f64, z: &dyn Fn(Point) -> f64| {
        for s in pts.windows(2) {
            let (a, b) = (s[0], s[1]);
            let l = a.dist(&b);
            if l < 1e-6 { continue; }
            let n = Point::new(-(b.y - a.y) / l * w / 2.0, (b.x - a.x) / l * w / 2.0);
            let k = (l / 500.0).ceil().max(1.0) as usize;
            for i in 0..k {
                let (t0, t1) = (i as f64 / k as f64, (i + 1) as f64 / k as f64);
                let p0 = Point::new(a.x + (b.x - a.x) * t0, a.y + (b.y - a.y) * t0);
                let p1 = Point::new(a.x + (b.x - a.x) * t1, a.y + (b.y - a.y) * t1);
                let v = |p: Point, sgn: f64| { let q = Point::new(p.x + n.x * sgn, p.y + n.y * sgn); [q.x, q.y, z(q) + 3.0] };
                let up = [0.0, 0.0, 1.0];
                tri(out, v(p0, -1.0), v(p1, -1.0), v(p1, 1.0), up);
                tri(out, v(p0, -1.0), v(p1, 1.0), v(p0, 1.0), up);
            }
        }
    };
    // a box on the ground: plan corners (counter-clockwise or not), from the ground at its middle up `h`
    let block = |out: &mut Vec<f32>, c: &[Point; 4], base: f64, h: f64| {
        let mut ring = c.to_vec();
        if crate::geom::signed_area(&ring) < 0.0 { ring.reverse(); }
        prism(out, &ring, &[[0, 1, 2], [0, 2, 3]], base, base + h);
    };
    let mut out = vec![];
    for (id, m) in doc.line_markings() {
        let z = |p: Point| doc.traffic_z(m.level, m.host, p);
        let mut v = vec![];
        for piece in m.pieces() { ribbon(&mut v, &piece, m.width_mm, &z); }
        out.push((id, "LineMarking", m.paint.rgb(), v));
    }
    for (id, r) in doc.parking_rows() {
        let z = |p: Point| doc.traffic_z(r.level, r.host, p);
        let mut v = vec![];
        for l in r.lines() { ribbon(&mut v, &l, r.line_width_mm, &z); }
        out.push((id, "ParkingRow", r.line_paint().rgb(), v));
        // the symbol of access's blue ground, painted on each accessible space
        let mut b = vec![];
        for c in r.access_symbols() {
            let m = |p: Point, q: Point| Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
            ribbon(&mut b, &[m(c[0], c[3]), m(c[1], c[2])], c[0].dist(&c[3]), &|p| z(p) + 1.0);
        }
        if !b.is_empty() { out.push((id, "AccessSymbol", [30, 90, 180], b)); }
        let mut s = vec![];
        for c in r.wheel_stops() {
            let mid = Point::new((c[0].x + c[2].x) / 2.0, (c[0].y + c[2].y) / 2.0);
            block(&mut s, &c, z(mid), WHEEL_STOP[2]);
        }
        if !s.is_empty() { out.push((id, "WheelStop", if r.paint == Paint::Yellow { [232, 180, 20] } else { [70, 70, 70] }, s)); }
    }
    // kerb ramps: a wedge from the top of the kerb at its high edge down to the ground at its low edge
    for (id, k) in doc.kerb_ramps() {
        let z = |p: Point| doc.traffic_z(k.level, k.host, p);
        let o = k.outline();
        let low = |p: Point| [p.x, p.y, z(p)];
        let high = |p: Point| [p.x, p.y, z(k.bottom) + k.rise_mm];
        let nrm = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            [n[0] / l, n[1] / l, n[2] / l]
        };
        let mut v = vec![];
        let mut face = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| { let n = nrm(a, b, c); let n = if n[2] < -1e-9 { [-n[0], -n[1], -n[2]] } else { n }; tri(&mut v, a, b, c, n); };
        // the sloped top, the two sides and the high end
        let (t0, t1, b1, b0) = (high(o[0]), high(o[1]), low(o[2]), low(o[3]));
        face(t0, b0, b1); face(t0, b1, t1);
        face(low(o[0]), b0, t0); face(low(o[1]), t1, b1);
        face(low(o[0]), t0, t1); face(low(o[0]), t1, low(o[1]));
        out.push((id, "KerbRamp", [175, 175, 170], v));
    }
    for (id, k) in doc.kerbs() {
        let z = |p: Point| doc.traffic_z(k.level, k.host, p);
        let mut pts: Vec<Point> = vec![];
        for c in &k.curves { let p = c.tessellate(20.0); pts.extend(p.into_iter().skip(if pts.is_empty() { 0 } else { 1 })); }
        let mut v = vec![];
        // in short pieces, each a block from the ground under its middle up the kerb's height
        for s in pts.windows(2) {
            let (a, b) = (s[0], s[1]);
            let l = a.dist(&b);
            if l < 1e-6 { continue; }
            let kk = (l / 300.0).ceil().max(1.0) as usize;
            let n = Point::new(-(b.y - a.y) / l * k.width_mm / 2.0, (b.x - a.x) / l * k.width_mm / 2.0);
            for i in 0..kk {
                let (t0, t1) = (i as f64 / kk as f64, (i + 1) as f64 / kk as f64);
                let p0 = Point::new(a.x + (b.x - a.x) * t0, a.y + (b.y - a.y) * t0);
                let p1 = Point::new(a.x + (b.x - a.x) * t1, a.y + (b.y - a.y) * t1);
                let c = [Point::new(p0.x - n.x, p0.y - n.y), Point::new(p1.x - n.x, p1.y - n.y), Point::new(p1.x + n.x, p1.y + n.y), Point::new(p0.x + n.x, p0.y + n.y)];
                let mid = Point::new((p0.x + p1.x) / 2.0, (p0.y + p1.y) / 2.0);
                block(&mut v, &c, z(mid), k.height_mm);
            }
        }
        out.push((id, "Kerb", [190, 188, 182], v));
    }
    out
}
