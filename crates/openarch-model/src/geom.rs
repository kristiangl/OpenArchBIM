//! 2D geometry helpers. All units are millimetres.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    pub fn dist(&self, o: &Point) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

/// Signed area (positive when counter-clockwise).
pub fn signed_area(poly: &[Point]) -> f64 {
    let n = poly.len();
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

pub fn polygon_area_abs(poly: &[Point]) -> f64 {
    signed_area(poly).abs()
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn in_triangle(p: Point, a: Point, b: Point, c: Point) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let neg = d1 < -1e-9 || d2 < -1e-9 || d3 < -1e-9;
    let pos = d1 > 1e-9 || d2 > 1e-9 || d3 > 1e-9;
    !(neg && pos)
}

/// Ear-clipping triangulation of a simple polygon. Returns index triples into
/// `poly`, wound counter-clockwise. Returns `None` for degenerate input.
pub fn triangulate(poly: &[Point]) -> Option<Vec<[usize; 3]>> {
    let n = poly.len();
    if n < 3 || signed_area(poly).abs() < 1e-6 {
        return None;
    }
    let mut idx: Vec<usize> = (0..n).collect();
    if signed_area(poly) < 0.0 {
        idx.reverse();
    }
    let mut tris = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 {
        guard += 1;
        if guard > n * n {
            return None; // self-intersecting or otherwise not simple
        }
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross(a, b, c) <= 1e-9 {
                continue; // reflex or collinear
            }
            let blocked = idx.iter().any(|&j| {
                j != ia && j != ib && j != ic && {
                    let p = poly[j];
                    // vertices coincident with a corner don't block
                    p != a && p != b && p != c && in_triangle(p, a, b, c)
                }
            });
            if blocked {
                continue;
            }
            tris.push([ia, ib, ic]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            return None;
        }
    }
    tris.push([idx[0], idx[1], idx[2]]);
    Some(tris)
}

/// One piece of a sketched shape: an outline and the openings inside it.
/// Oriented so the material is on the left of every edge (outline counter-clockwise, holes clockwise).
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Vec<Point>,
    pub holes: Vec<Vec<Point>>,
}

impl Region {
    pub fn area(&self) -> f64 {
        signed_area(&self.outer).abs() - self.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>()
    }
}

/// Split closed polygons (which must not cross) into regions by nesting: a polygon inside an
/// odd number of others is an opening in its parent, inside an even number it starts a new region.
pub fn regions(polys: &[Vec<Point>]) -> Vec<Region> {
    let depth = crate::curve::loop_depths(polys);
    let mut out: Vec<Region> = vec![];
    let mut owner: Vec<Option<usize>> = vec![None; polys.len()];
    for (i, p) in polys.iter().enumerate() {
        if depth[i] % 2 == 0 {
            let mut o = p.clone();
            if signed_area(&o) < 0.0 {
                o.reverse();
            }
            owner[i] = Some(out.len());
            out.push(Region { outer: o, holes: vec![] });
        }
    }
    for (i, p) in polys.iter().enumerate() {
        if depth[i] % 2 == 1 {
            // the smallest outer polygon that contains it
            let parent = (0..polys.len())
                .filter(|&j| depth[j] == depth[i] - 1 && crate::curve::point_in_polygon(p[0], &polys[j]))
                .min_by(|&a, &b| signed_area(&polys[a]).abs().total_cmp(&signed_area(&polys[b]).abs()));
            if let Some(Some(r)) = parent.map(|j| owner[j]) {
                let mut h = p.clone();
                if signed_area(&h) > 0.0 {
                    h.reverse();
                }
                out[r].holes.push(h);
            }
        }
    }
    out
}

/// Triangulate a region (with its openings). Returns the vertices (outline first, then each
/// hole) and counter-clockwise index triples into them.
pub fn triangulate_region(r: &Region) -> Option<(Vec<Point>, Vec<[usize; 3]>)> {
    if r.outer.len() < 3 {
        return None;
    }
    let mut pts: Vec<Point> = r.outer.clone();
    let mut holes: Vec<usize> = vec![];
    for h in &r.holes {
        holes.push(pts.len());
        pts.extend(h.iter().copied());
    }
    let flat: Vec<f64> = pts.iter().flat_map(|p| [p.x, p.y]).collect();
    let idx = earcutr::earcut(&flat, &holes, 2).ok()?;
    if idx.is_empty() {
        return None;
    }
    let tris: Vec<[usize; 3]> = idx
        .chunks(3)
        .map(|t| {
            if signed_area(&[pts[t[0]], pts[t[1]], pts[t[2]]]) < 0.0 { [t[0], t[2], t[1]] } else { [t[0], t[1], t[2]] }
        })
        .collect();
    // the triangles must account for the whole area, or the outline was not simple
    let total: f64 = tris.iter().map(|t| signed_area(&[pts[t[0]], pts[t[1]], pts[t[2]]])).sum();
    if (total - r.area()).abs() > 1e-3 * r.area().abs().max(1.0) {
        return None;
    }
    Some((pts, tris))
}

/// Rectangle (4 corners, counter-clockwise) of a wall of `thickness` centred on
/// the line `a`-`b`. `None` when the wall has no length.
pub fn wall_outline(a: Point, b: Point, thickness: f64) -> Option<[Point; 4]> {
    let len = a.dist(&b);
    if len < 1e-6 {
        return None;
    }
    let (dx, dy) = ((b.x - a.x) / len, (b.y - a.y) / len);
    let (nx, ny) = (-dy * thickness * 0.5, dx * thickness * 0.5);
    Some([
        Point::new(a.x - nx, a.y - ny),
        Point::new(b.x - nx, b.y - ny),
        Point::new(b.x + nx, b.y + ny),
        Point::new(a.x + nx, a.y + ny),
    ])
}

/// Distance from `p` to the segment `a`-`b`.
pub fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    if l2 < 1e-12 {
        return p.dist(&a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0);
    p.dist(&Point::new(a.x + t * dx, a.y + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn square_and_l_shape_area_is_preserved() {
        let sq = [p(0., 0.), p(4000., 0.), p(4000., 3000.), p(0., 3000.)];
        let tris = triangulate(&sq).unwrap();
        assert_eq!(tris.len(), 2);
        let area: f64 = tris.iter().map(|t| signed_area(&[sq[t[0]], sq[t[1]], sq[t[2]]])).sum();
        assert!((area - 12_000_000.0).abs() < 1e-6);

        let l = [p(0., 0.), p(6., 0.), p(6., 2.), p(2., 2.), p(2., 6.), p(0., 6.)];
        let tris = triangulate(&l).unwrap();
        assert_eq!(tris.len(), 4);
        let area: f64 = tris.iter().map(|t| signed_area(&[l[t[0]], l[t[1]], l[t[2]]])).sum();
        assert!((area - 20.0).abs() < 1e-9);
        assert!(tris.iter().all(|t| signed_area(&[l[t[0]], l[t[1]], l[t[2]]]) > 0.0));
    }

    #[test]
    fn clockwise_input_and_degenerates() {
        let cw = [p(0., 0.), p(0., 3.), p(4., 3.), p(4., 0.)];
        assert_eq!(triangulate(&cw).unwrap().len(), 2);
        assert!(triangulate(&[p(0., 0.), p(1., 1.)]).is_none());
        assert!(triangulate(&[p(0., 0.), p(1., 1.), p(2., 2.)]).is_none());
    }

    #[test]
    fn wall_outline_is_centred() {
        let o = wall_outline(p(0., 0.), p(1000., 0.), 200.).unwrap();
        assert_eq!((o[0].y, o[3].y), (-100.0, 100.0));
        assert!(wall_outline(p(5., 5.), p(5., 5.), 200.).is_none());
    }

    #[test]
    fn segment_distance() {
        assert_eq!(dist_to_segment(p(5., 3.), p(0., 0.), p(10., 0.)), 3.0);
        assert_eq!(dist_to_segment(p(-4., 3.), p(0., 0.), p(10., 0.)), 5.0);
    }
}
