//! The path a wall follows. A straight wall is one segment; a curved wall (arc, partial
//! ellipse or spline) is the curve cut into short straight facets. Everything that lays
//! layers, joins and openings along a wall reads this, so a curved wall is built from the
//! same pieces as a straight one: each facet is a straight wall of the same type, mitred to
//! its neighbours, and the pieces are merged back into one wall afterwards.

use crate::element::{Id, Wall};
use crate::geom::Point;

/// How far (mm) a facet may sit from the true curve.
pub const WALL_TOL: f64 = 1.5;

/// Facets of one wall are told apart from other walls by packing the facet number into the
/// low bits of the id. Straight walls are facet 0, so the order of ids (which decides the
/// older wall of a join) stays the order of the walls.
const FACET_BITS: u32 = 20;

pub fn facet_id(wall: Id, k: usize) -> Id {
    (wall << FACET_BITS) | k as u64
}

pub fn wall_of(facet: Id) -> Id {
    facet >> FACET_BITS
}

pub fn facet_no(facet: Id) -> usize {
    (facet & ((1 << FACET_BITS) - 1)) as usize
}

#[derive(Debug, Clone)]
pub struct WallPath {
    pub pts: Vec<Point>,
    /// Distance along the path to each point.
    pub cum: Vec<f64>,
}

impl WallPath {
    pub fn of(w: &Wall) -> WallPath {
        let mut pts = match &w.curve {
            Some(c) if !c.is_closed() => c.tessellate(WALL_TOL),
            _ => vec![],
        };
        if pts.len() < 2 {
            pts = vec![w.start, w.end];
        } else {
            // the stored ends are authoritative
            pts[0] = w.start;
            let n = pts.len();
            pts[n - 1] = w.end;
        }
        let mut cum = vec![0.0];
        for i in 1..pts.len() {
            cum.push(cum[i - 1] + pts[i - 1].dist(&pts[i]));
        }
        WallPath { pts, cum }
    }

    pub fn length(&self) -> f64 {
        *self.cum.last().unwrap_or(&0.0)
    }

    pub fn is_straight(&self) -> bool {
        self.pts.len() == 2
    }

    pub fn facets(&self) -> Vec<(Point, Point)> {
        self.pts.windows(2).map(|w| (w[0], w[1])).collect()
    }

    fn dir(a: Point, b: Point) -> Point {
        let l = a.dist(&b).max(1e-9);
        Point::new((b.x - a.x) / l, (b.y - a.y) / l)
    }

    /// The point `u` along the path and the unit direction of travel there.
    pub fn at(&self, u: f64) -> (Point, Point) {
        let u = u.clamp(0.0, self.length());
        let i = self.cum.partition_point(|c| *c <= u).clamp(1, self.pts.len() - 1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let seg = (self.cum[i] - self.cum[i - 1]).max(1e-9);
        let t = (u - self.cum[i - 1]) / seg;
        (Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t), Self::dir(a, b))
    }

    /// The nearest point of the path to `p`: how far along it, how far to the left of it
    /// (negative on the right) and the distance.
    pub fn project(&self, p: Point) -> (f64, f64, f64) {
        let mut best = (0.0, 0.0, f64::INFINITY);
        for i in 1..self.pts.len() {
            let (a, b) = (self.pts[i - 1], self.pts[i]);
            let d = Self::dir(a, b);
            let seg = self.cum[i] - self.cum[i - 1];
            let rel = Point::new(p.x - a.x, p.y - a.y);
            let along = (rel.x * d.x + rel.y * d.y).clamp(0.0, seg);
            let q = Point::new(a.x + d.x * along, a.y + d.y * along);
            let dist = q.dist(&p);
            if dist < best.2 {
                let across = -d.y * (p.x - q.x) + d.x * (p.y - q.y);
                best = (self.cum[i - 1] + along, across, dist);
            }
        }
        best
    }
}

impl Wall {
    pub fn path(&self) -> WallPath {
        WallPath::of(self)
    }

    /// Length along the location line (arc length for a curved wall).
    pub fn length(&self) -> f64 {
        if self.curve.is_none() { self.start.dist(&self.end) } else { self.path().length() }
    }

    pub fn is_curved(&self) -> bool {
        self.curve.is_some()
    }

    /// Move the ends of the wall. A curved wall keeps its shape, scaled and turned with its
    /// chord, so dragging an end of an arc keeps the angle it sweeps.
    pub fn retarget(&mut self, start: Point, end: Point) {
        if let Some(c) = &self.curve {
            self.curve = Some(c.similar(self.start, self.end, start, end));
        }
        self.start = start;
        self.end = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn quarter() -> Wall {
        let mut w = Wall::new(1, 1, p(5000.0, 0.0), p(0.0, 5000.0), 2700.0);
        w.curve = Some(Curve::Arc { start: p(5000.0, 0.0), end: p(0.0, 5000.0), centre: p(0.0, 0.0), ccw: true });
        w
    }

    #[test]
    fn a_quarter_circle_is_cut_into_facets_that_follow_it() {
        let path = quarter().path();
        assert!(path.pts.len() > 10);
        assert!((path.length() - 5000.0 * std::f64::consts::FRAC_PI_2).abs() < 3.0);
        for q in &path.pts {
            assert!((q.dist(&p(0.0, 0.0)) - 5000.0).abs() < 1e-6);
        }
        assert_eq!(path.pts[0], p(5000.0, 0.0));
        assert_eq!(*path.pts.last().unwrap(), p(0.0, 5000.0));
    }

    #[test]
    fn at_and_project_agree_along_a_curved_path() {
        let path = quarter().path();
        let (mid, dir) = path.at(path.length() / 2.0);
        // half way round the quarter circle: on the diagonal, travelling at right angles to it
        assert!((mid.x - mid.y).abs() < 5.0 && (mid.dist(&p(0.0, 0.0)) - 5000.0).abs() < 2.0);
        assert!((dir.x + dir.y).abs() < 0.01);
        let (u, across, dist) = path.project(Point::new(mid.x * 1.1, mid.y * 1.1));
        assert!((u - path.length() / 2.0).abs() < 20.0);
        assert!(across < 0.0 && dist > 400.0); // outside the circle is the right of an anticlockwise path
    }

    #[test]
    fn a_straight_wall_is_one_facet_and_ids_keep_the_order_of_walls() {
        let w = Wall::new(1, 1, p(0.0, 0.0), p(3000.0, 0.0), 2700.0);
        assert!(w.path().is_straight() && (w.length() - 3000.0).abs() < 1e-9);
        assert!(facet_id(5, 999) < facet_id(6, 0));
        assert_eq!((wall_of(facet_id(7, 3)), facet_no(facet_id(7, 3))), (7, 3));
    }
}
