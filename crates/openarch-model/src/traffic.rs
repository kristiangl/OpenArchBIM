//! Traffic: line marking, parking rows (bays with their lines and wheel stops) and kerbs, laid on the ground. Each
//! follows the surface under it: the toposolid (a sub-division first, so a road sub-division) or floor it was placed
//! on (`host`), else its level.
//!
//! Sizes from AS/NZS 2890.1:2004 as quoted by City of Karratha CKS-600 (9 Nov 2026): line marking 80 mm wide (Clause
//! 4.4.1); wheel stops 100 high and 1600 to 1700 long (1650 typical), 900 from the front of the space for front-in
//! parking and 620 for rear-in (Table 2.1). Bay width and length are the person's to set (2500 by 5400 to start).

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::{Document, Element, Id, Point};

/// Line marking paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Paint {
    #[default]
    White,
    Yellow,
}

impl Paint {
    /// The colour in 3D and on screen.
    pub fn rgb(self) -> [u8; 3] {
        match self { Paint::White => [236, 236, 230], Paint::Yellow => [232, 180, 20] }
    }
}

fn line_width() -> f64 { 80.0 }
fn dash() -> f64 { 1000.0 }
fn bay_w() -> f64 { 2500.0 }
fn bay_l() -> f64 { 5400.0 }
fn ninety() -> f64 { 90.0 }
fn stop_off() -> f64 { 900.0 }
fn kerb_h() -> f64 { 150.0 }
fn kerb_w() -> f64 { 150.0 }

/// Line marking: a painted line along a run of lines and arcs, continuous or dashed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineMarking {
    pub level: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Id>,
    pub curves: Vec<Curve>,
    #[serde(default = "line_width")]
    pub width_mm: f64,
    #[serde(default)]
    pub paint: Paint,
    #[serde(default)]
    pub dashed: bool,
    #[serde(default = "dash")]
    pub dash_mm: f64,
    #[serde(default = "dash")]
    pub gap_mm: f64,
    #[serde(default)]
    pub pinned: bool,
}

/// A row of parking bays along a line (the front of the bays, where the cars nose in), on its left or right, at an
/// angle to it (90, 60, 45 or 30; 0 is parallel parking), with lines between the bays and wheel stops if asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParkingRow {
    pub level: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Id>,
    pub start: Point,
    pub end: Point,
    /// The bays are on the left of start to end (else the right).
    #[serde(default = "yes")]
    pub left: bool,
    #[serde(default = "ninety")]
    pub angle_deg: f64,
    #[serde(default = "bay_w")]
    pub bay_width_mm: f64,
    #[serde(default = "bay_l")]
    pub bay_length_mm: f64,
    #[serde(default = "line_width")]
    pub line_width_mm: f64,
    #[serde(default)]
    pub paint: Paint,
    #[serde(default)]
    pub wheel_stops: bool,
    /// From the front of the bay to the wheel stop's face (900 front-in, 620 rear-in).
    #[serde(default = "stop_off")]
    pub wheel_stop_offset_mm: f64,
    /// Accessible spaces (AS 2890.6): each space has a shared area as wide beside it, both marked in yellow, and
    /// the symbol of access on a blue square in the space.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub accessible: bool,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub pinned: bool,
}

fn yes() -> bool { true }

/// A kerb along a run of lines and arcs (its centreline), `width_mm` wide and `height_mm` above the ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kerb {
    pub level: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Id>,
    pub curves: Vec<Curve>,
    #[serde(default = "kerb_h")]
    pub height_mm: f64,
    #[serde(default = "kerb_w")]
    pub width_mm: f64,
    #[serde(default)]
    pub pinned: bool,
}

/// Kerb ramp limits (AS 1428.1-2009 as summarised by Sunshine Coast Council, 9 Nov 2026): as steep as 1 in [0],
/// rising no more than [1] and no longer than [2] (mm).
pub const KERB_RAMP: [f64; 3] = [8.0, 190.0, 1520.0];

/// A kerb ramp: from the top of a kerb (`top`, the middle of its high edge) down to the ground at `bottom` (the middle
/// of its low edge), `width_mm` wide, rising `rise_mm`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KerbRamp {
    pub level: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Id>,
    pub top: Point,
    pub bottom: Point,
    #[serde(default = "ramp_w")]
    pub width_mm: f64,
    #[serde(default = "kerb_h")]
    pub rise_mm: f64,
    #[serde(default)]
    pub pinned: bool,
}

fn ramp_w() -> f64 { 1200.0 }

impl KerbRamp {
    pub fn length(&self) -> f64 { self.top.dist(&self.bottom) }

    /// Its plan outline: top left, top right, bottom right, bottom left.
    pub fn outline(&self) -> [Point; 4] {
        let l = self.length().max(1e-9);
        let d = Point::new((self.bottom.x - self.top.x) / l, (self.bottom.y - self.top.y) / l);
        let h = self.width_mm / 2.0;
        let s = Point::new(-d.y * h, d.x * h);
        [Point::new(self.top.x - s.x, self.top.y - s.y), Point::new(self.top.x + s.x, self.top.y + s.y), Point::new(self.bottom.x + s.x, self.bottom.y + s.y), Point::new(self.bottom.x - s.x, self.bottom.y - s.y)]
    }

    /// Where it breaks the kerb ramp limits.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = vec![];
        let (l, r) = (self.length(), self.rise_mm);
        if r > 0.0 && l / r < KERB_RAMP[0] - 1e-6 { out.push(format!("It slopes at 1:{:.1}, steeper than 1:{}: make it {:.0} long.", l / r, KERB_RAMP[0], r * KERB_RAMP[0])); }
        if r > KERB_RAMP[1] + 1e-6 { out.push(format!("It rises {r:.0}, more than {:.0}.", KERB_RAMP[1])); }
        if l > KERB_RAMP[2] + 1e-6 { out.push(format!("It is {l:.0} long, longer than {:.0}.", KERB_RAMP[2])); }
        out
    }
}

/// Wheel stop size (mm): long, deep (front to back) and high.
pub const WHEEL_STOP: [f64; 3] = [1650.0, 150.0, 100.0];

impl ParkingRow {
    fn axes(&self) -> Option<(Point, Point, f64)> {
        let len = self.start.dist(&self.end);
        if len < 1.0 { return None; }
        let u = Point::new((self.end.x - self.start.x) / len, (self.end.y - self.start.y) / len);
        let n = if self.left { Point::new(-u.y, u.x) } else { Point::new(u.y, -u.x) };
        Some((u, n, len))
    }

    /// What fills the row, in order: each a cell's four corners (front left, front right, back right, back left:
    /// front on the row's line) and whether it is an accessible space's shared area. An accessible row goes space,
    /// shared area, space, shared area, each as wide as a bay; a row ends with a space.
    fn cells(&self) -> Vec<([Point; 4], bool)> {
        let Some((u, n, len)) = self.axes() else { return vec![] };
        let at = |s: f64, d: f64| Point::new(self.start.x + u.x * s + n.x * d, self.start.y + u.y * s + n.y * d);
        let (pitch, lean, depth) = if self.angle_deg < 1.0 {
            // parallel: the bay's length runs along the row, its width out from it
            (self.bay_length_mm, 0.0, self.bay_width_mm)
        } else {
            // at an angle each bay takes its width over the sine along the row; its sides lean back by the cosine
            let a = self.angle_deg.clamp(1.0, 90.0).to_radians();
            (self.bay_width_mm / a.sin(), self.bay_length_mm * a.cos(), self.bay_length_mm * a.sin())
        };
        let mut out = vec![];
        let mut s = 0.0;
        loop {
            let shared = self.accessible && out.len() % 2 == 1;
            if s + pitch + lean.max(0.0) > len + 1e-6 { break; }
            out.push(([at(s, 0.0), at(s + pitch, 0.0), at(s + pitch + lean, depth), at(s + lean, depth)], shared));
            s += pitch;
        }
        if out.last().is_some_and(|c| c.1) { out.pop(); }
        out
    }

    /// The parking spaces, each as its four corners (front left, front right, back right, back left: front on the
    /// row's line).
    pub fn bays(&self) -> Vec<[Point; 4]> {
        self.cells().into_iter().filter(|c| !c.1).map(|c| c.0).collect()
    }

    /// An accessible row's shared areas.
    pub fn shared_areas(&self) -> Vec<[Point; 4]> {
        self.cells().into_iter().filter(|c| c.1).map(|c| c.0).collect()
    }

    /// The painted lines: the sides of every space and shared area (shared sides once); parallel bays and shared
    /// areas are closed at their outer edge too.
    pub fn lines(&self) -> Vec<[Point; 2]> {
        let cells = self.cells();
        let mut out: Vec<[Point; 2]> = vec![];
        for (i, (b, shared)) in cells.iter().enumerate() {
            if i == 0 { out.push([b[0], b[3]]); }
            out.push([b[1], b[2]]);
            if self.angle_deg < 1.0 || *shared { out.push([b[3], b[2]]); }
        }
        out
    }

    /// The paint: yellow for an accessible row (AS 2890.6), else as set.
    pub fn line_paint(&self) -> Paint {
        if self.accessible { Paint::Yellow } else { self.paint }
    }

    /// An accessible row's symbols of access: a 1200 square (the blue ground) in the middle of each space.
    pub fn access_symbols(&self) -> Vec<[Point; 4]> {
        if !self.accessible { return vec![]; }
        self.bays().iter().map(|b| {
            let c = Point::new((b[0].x + b[1].x + b[2].x + b[3].x) / 4.0, (b[0].y + b[1].y + b[2].y + b[3].y) / 4.0);
            let (u, n) = self.axes().map(|(u, n, _)| (u, n)).unwrap_or((Point::new(1.0, 0.0), Point::new(0.0, 1.0)));
            let h = 600.0;
            let p = |a: f64, b: f64| Point::new(c.x + u.x * a + n.x * b, c.y + u.y * a + n.y * b);
            [p(-h, -h), p(h, -h), p(h, h), p(-h, h)]
        }).collect()
    }

    /// The wheel stops: each as its plan corners, `wheel_stop_offset_mm` in from the front of its bay, centred.
    pub fn wheel_stops(&self) -> Vec<[Point; 4]> {
        if !self.wheel_stops || self.angle_deg < 1.0 { return vec![]; }
        self.bays().iter().map(|b| {
            let mid_front = Point::new((b[0].x + b[1].x) / 2.0, (b[0].y + b[1].y) / 2.0);
            let mid_back = Point::new((b[2].x + b[3].x) / 2.0, (b[2].y + b[3].y) / 2.0);
            let l = mid_front.dist(&mid_back).max(1.0);
            let d = Point::new((mid_back.x - mid_front.x) / l, (mid_back.y - mid_front.y) / l);
            let s = Point::new(-d.y, d.x);
            let c = Point::new(mid_front.x + d.x * (self.wheel_stop_offset_mm + WHEEL_STOP[1] / 2.0), mid_front.y + d.y * (self.wheel_stop_offset_mm + WHEEL_STOP[1] / 2.0));
            let (h, t) = (WHEEL_STOP[0] / 2.0, WHEEL_STOP[1] / 2.0);
            let p = |a: f64, b: f64| Point::new(c.x + s.x * a + d.x * b, c.y + s.y * a + d.y * b);
            [p(-h, -t), p(h, -t), p(h, t), p(-h, t)]
        }).collect()
    }
}

impl LineMarking {
    /// The painted pieces: the whole run, or its dashes.
    pub fn pieces(&self) -> Vec<Vec<Point>> {
        let path: Vec<Point> = {
            let mut out: Vec<Point> = vec![];
            for c in &self.curves { let p = c.tessellate(20.0); out.extend(p.into_iter().skip(if out.is_empty() { 0 } else { 1 })); }
            out
        };
        if !self.dashed || self.dash_mm < 10.0 { return vec![path]; }
        let (mut out, mut cur): (Vec<Vec<Point>>, Vec<Point>) = (vec![], vec![]);
        let (mut on, mut left) = (true, self.dash_mm);
        for w in path.windows(2) {
            let (mut a, b) = (w[0], w[1]);
            let mut seg = a.dist(&b);
            if on && cur.is_empty() { cur.push(a); }
            while seg > 1e-9 {
                let step = seg.min(left);
                let q = Point::new(a.x + (b.x - a.x) / seg * step, a.y + (b.y - a.y) / seg * step);
                if on { cur.push(q); }
                seg -= step; left -= step; a = q;
                if left <= 1e-9 {
                    on = !on;
                    left = if on { self.dash_mm } else { self.gap_mm.max(10.0) };
                    if on { cur = vec![a]; } else if cur.len() > 1 { out.push(std::mem::take(&mut cur)); }
                }
            }
        }
        if on && cur.len() > 1 { out.push(cur); }
        out
    }
}

impl Document {
    pub fn line_markings(&self) -> Vec<(Id, &LineMarking)> { self.iter().filter_map(|(id, e)| if let Element::LineMarking(x) = e { Some((id, x)) } else { None }).collect() }
    pub fn parking_rows(&self) -> Vec<(Id, &ParkingRow)> { self.iter().filter_map(|(id, e)| if let Element::ParkingRow(x) = e { Some((id, x)) } else { None }).collect() }
    pub fn kerb_ramps(&self) -> Vec<(Id, &KerbRamp)> { self.iter().filter_map(|(id, e)| if let Element::KerbRamp(x) = e { Some((id, x)) } else { None }).collect() }
    pub fn kerbs(&self) -> Vec<(Id, &Kerb)> { self.iter().filter_map(|(id, e)| if let Element::Kerb(x) = e { Some((id, x)) } else { None }).collect() }

    /// The ground under a plan point for traffic items on `level`: a sub-division or toposolid there (as built), else
    /// a floor on the level; None for the level itself.
    pub fn traffic_host_at(&self, level: Id, p: Point) -> Option<Id> {
        let mut topos = self.toposolids();
        topos.sort_by_key(|(_, t)| t.host.is_none());
        for (id, t) in topos {
            if crate::shape::within(&self.toposolid_rings(t), p) { return Some(id); }
        }
        self.iter().find_map(|(id, e)| match e {
            Element::Floor(f) if f.level == level && f.regions(1.0).iter().any(|r| crate::rooms::contains(r, p)) => Some(id),
            _ => None,
        })
    }

    /// The height of the ground a traffic item stands on at a plan point.
    pub fn traffic_z(&self, level: Id, host: Option<Id>, p: Point) -> f64 {
        let flat = self.level(level).map_or(0.0, |l| l.elevation_mm);
        match host.and_then(|h| self.get(h)) {
            Some(Element::Toposolid(t)) => self.toposolid_surface(t).and_then(|(tin, _, _)| tin.z_near(p)).unwrap_or(flat),
            Some(Element::Floor(f)) => self.floor_top_at(f, p).unwrap_or(flat),
            _ => flat,
        }
    }

    fn check_path(curves: &[Curve]) -> Result<Vec<Curve>, String> {
        if curves.iter().any(|c| !matches!(c, Curve::Line { .. } | Curve::Arc { .. })) { return Err("Draw it with lines and arcs.".into()); }
        crate::propline::chain(curves).ok_or_else(|| "Draw one run of lines and arcs, each starting where the last one ends.".into())
    }

    /// Line Marking along a sketched run, on the ground under its middle.
    pub fn add_line_marking(&mut self, level: Id, curves: Vec<Curve>) -> Result<Id, String> {
        if self.level(level).is_none() { return Err("unknown level".into()); }
        let curves = Self::check_path(&curves)?;
        let host = curves.first().and_then(|c| self.traffic_host_at(level, c.mid()));
        let m = LineMarking { level, host, curves, width_mm: line_width(), paint: Paint::White, dashed: false, dash_mm: dash(), gap_mm: dash(), pinned: false };
        self.transact("Line marking", |tx| Ok(tx.add(Element::LineMarking(m))))
    }

    /// Kerb along a sketched run (its centreline), on the ground under its middle.
    pub fn add_kerb(&mut self, level: Id, curves: Vec<Curve>, height_mm: f64, width_mm: f64) -> Result<Id, String> {
        if self.level(level).is_none() { return Err("unknown level".into()); }
        if !(height_mm > 0.0 && width_mm > 0.0) { return Err("A kerb needs a height and a width.".into()); }
        let curves = Self::check_path(&curves)?;
        let host = curves.first().and_then(|c| self.traffic_host_at(level, c.mid()));
        let k = Kerb { level, host, curves, height_mm, width_mm, pinned: false };
        self.transact("Kerb", |tx| Ok(tx.add(Element::Kerb(k))))
    }

    /// Kerb Ramp: from `top` (at the kerb) down to `bottom`, on the ground under its bottom.
    pub fn add_kerb_ramp(&mut self, level: Id, top: Point, bottom: Point, width_mm: f64, rise_mm: f64) -> Result<Id, String> {
        if self.level(level).is_none() { return Err("unknown level".into()); }
        if top.dist(&bottom) < 100.0 { return Err("A kerb ramp needs a top and a bottom at least 100 apart.".into()); }
        if !(width_mm > 0.0 && rise_mm > 0.0) { return Err("A kerb ramp needs a width and a rise.".into()); }
        let host = self.traffic_host_at(level, bottom);
        let k = KerbRamp { level, host, top, bottom, width_mm, rise_mm, pinned: false };
        self.transact("Kerb ramp", |tx| Ok(tx.add(Element::KerbRamp(k))))
    }

    /// Parking Bay: a row of bays along `start` to `end` (the front of the bays) as `row` sets out.
    pub fn add_parking_row(&mut self, mut row: ParkingRow) -> Result<Id, String> {
        Self::check_parking(&row)?;
        if self.level(row.level).is_none() { return Err("unknown level".into()); }
        let mid = Point::new((row.start.x + row.end.x) / 2.0, (row.start.y + row.end.y) / 2.0);
        row.host = self.traffic_host_at(row.level, mid);
        if row.bays().is_empty() { return Err("The row is too short for one bay.".into()); }
        self.transact("Parking bays", |tx| Ok(tx.add(Element::ParkingRow(row))))
    }

    fn check_parking(r: &ParkingRow) -> Result<(), String> {
        if r.start.dist(&r.end) < 1.0 { return Err("The row needs a start and an end that are apart.".into()); }
        if !(r.bay_width_mm >= 1000.0 && r.bay_length_mm >= 2000.0) { return Err("A bay needs to be at least 1000 wide and 2000 long.".into()); }
        if !(0.0..=90.0).contains(&r.angle_deg) { return Err("The angle is from 0 (parallel) to 90.".into()); }
        if !(r.line_width_mm > 0.0 && r.wheel_stop_offset_mm >= 0.0) { return Err("Line width and wheel stop offset must be positive.".into()); }
        Ok(())
    }

    pub fn set_traffic_path(&mut self, id: Id, curves: Vec<Curve>) -> Result<(), String> {
        let curves = Self::check_path(&curves)?;
        self.refuse_pinned(&[id], "edited")?;
        let e = match self.get(id).cloned() {
            Some(Element::LineMarking(mut m)) => { m.curves = curves; Element::LineMarking(m) }
            Some(Element::Kerb(mut k)) => { k.curves = curves; Element::Kerb(k) }
            _ => return Err("not a line marking or kerb".into()),
        };
        self.transact("Edit path", |tx| tx.update(id, e))
    }

    /// Properties of a line marking, parking row or kerb.
    pub fn patch_traffic(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let e = match self.get(id).cloned() {
            Some(Element::LineMarking(m)) => {
                let n: LineMarking = crate::doc::apply_patch(&m, patch)?;
                if !(n.width_mm > 0.0 && n.dash_mm > 0.0 && n.gap_mm > 0.0) { return Err("Width, dash and gap must be positive.".into()); }
                Element::LineMarking(n)
            }
            Some(Element::ParkingRow(r)) => {
                let n: ParkingRow = crate::doc::apply_patch(&r, patch)?;
                Self::check_parking(&n)?;
                if n.bays().is_empty() { return Err("The row is too short for one bay at that size.".into()); }
                Element::ParkingRow(n)
            }
            Some(Element::KerbRamp(k)) => {
                let n: KerbRamp = crate::doc::apply_patch(&k, patch)?;
                if !(n.width_mm > 0.0 && n.rise_mm > 0.0) || n.length() < 100.0 { return Err("A kerb ramp needs a width, a rise and a length.".into()); }
                Element::KerbRamp(n)
            }
            Some(Element::Kerb(k)) => {
                let n: Kerb = crate::doc::apply_patch(&k, patch)?;
                if !(n.height_mm > 0.0 && n.width_mm > 0.0) { return Err("A kerb needs a height and a width.".into()); }
                Element::Kerb(n)
            }
            _ => return Err("not a traffic item".into()),
        };
        self.transact("Properties", |tx| tx.update(id, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point { Point::new(x, y) }
    fn row(angle: f64) -> ParkingRow {
        ParkingRow { level: 0, host: None, start: p(0.0, 0.0), end: p(25_000.0, 0.0), left: true, angle_deg: angle, bay_width_mm: 2500.0, bay_length_mm: 5400.0, line_width_mm: 80.0, paint: Paint::White, wheel_stops: true, wheel_stop_offset_mm: 900.0, mark: String::new(), pinned: false, accessible: false }
    }

    #[test]
    fn a_row_of_bays_at_ninety_and_at_an_angle() {
        let r = row(90.0);
        let bays = r.bays();
        assert_eq!(bays.len(), 10);
        assert!(bays[0][3].dist(&p(0.0, 5400.0)) < 1e-6 && bays[9][1].dist(&p(25_000.0, 0.0)) < 1e-6);
        assert_eq!(r.lines().len(), 11, "a line each side, shared between bays");
        let stops = r.wheel_stops();
        assert_eq!(stops.len(), 10);
        // the stop's face 900 in from the front, centred on the bay
        assert!((stops[0][0].y - 900.0).abs() < 1e-6 && ((stops[0][0].x + stops[0][1].x) / 2.0 - 1250.0).abs() < 1e-6, "{:?}", stops[0]);
        // at 45 degrees a bay takes its width over the sine along the row, and leans back
        let r = row(45.0);
        let b = r.bays();
        let pitch = 2500.0 / 45f64.to_radians().sin();
        assert!((b[0][1].x - pitch).abs() < 1e-6 && (b[0][3].y - 5400.0 * 45f64.to_radians().sin()).abs() < 1e-6);
        assert!(b.last().unwrap()[2].x <= 25_000.0 + 1e-6, "the last bay's back stays within the row");
        // parallel: length along the row, marked on three sides, no wheel stops
        let r = row(0.0);
        assert_eq!(r.bays().len(), 4);
        assert!(r.wheel_stops().is_empty());
    }

    #[test]
    fn an_accessible_row_alternates_spaces_and_shared_areas_in_yellow() {
        let mut r = row(90.0);
        r.accessible = true;
        r.bay_width_mm = 2700.0;
        r.bay_length_mm = 5500.0;
        // 25 m takes nine cells of 2.7: five spaces with four shared areas between, ending on a space
        assert_eq!(r.bays().len(), 5);
        assert_eq!(r.shared_areas().len(), 4);
        assert_eq!(r.line_paint(), Paint::Yellow);
        assert_eq!(r.access_symbols().len(), 5);
        assert_eq!(r.wheel_stops().len(), 5);
        // the shared areas are closed at the back
        assert_eq!(r.lines().len(), 10 + 4);
    }

    #[test]
    fn a_kerb_ramp_is_checked_against_one_in_eight() {
        let k = KerbRamp { level: 0, host: None, top: p(0.0, 0.0), bottom: p(0.0, -1200.0), width_mm: 1200.0, rise_mm: 150.0, pinned: false };
        assert!(k.warnings().is_empty(), "150 over 1200 is 1:8");
        let steep = KerbRamp { bottom: p(0.0, -900.0), ..k.clone() };
        assert!(steep.warnings()[0].contains("steeper than 1:8"));
        let high = KerbRamp { rise_mm: 200.0, bottom: p(0.0, -1600.0), ..k.clone() };
        assert_eq!(high.warnings().len(), 2, "{:?}", high.warnings());
        assert!((k.outline()[1].dist(&k.outline()[0]) - 1200.0).abs() < 1e-6);
    }

    #[test]
    fn a_dashed_marking_breaks_into_dashes() {
        let m = LineMarking { level: 0, host: None, curves: vec![Curve::line(p(0.0, 0.0), p(5500.0, 0.0))], width_mm: 80.0, paint: Paint::White, dashed: true, dash_mm: 1000.0, gap_mm: 500.0, pinned: false };
        let d = m.pieces();
        assert_eq!(d.len(), 4, "{d:?}");
        assert!((d[1][0].x - 1500.0).abs() < 1e-6 && (d[3].last().unwrap().x - 5500.0).abs() < 1e-6);
    }

    #[test]
    fn traffic_items_sit_on_the_toposolid_under_them() {
        let mut d = Document::new_project();
        d.ensure_toposolid_types().unwrap();
        let ty = d.toposolid_types()[0].0;
        let lvl = d.levels()[0].0;
        let lz = d.level(lvl).unwrap().elevation_mm;
        let sq = [p(-5000.0, -5000.0), p(40_000.0, -5000.0), p(40_000.0, 20_000.0), p(-5000.0, 20_000.0)];
        let t = d.add_toposolid_sketch(ty, lvl, (0..4).map(|i| Curve::line(sq[i], sq[(i + 1) % 4])).collect(), 300.0).unwrap();
        let mut r = row(90.0);
        r.level = lvl;
        let id = d.add_parking_row(r).unwrap();
        let Some(Element::ParkingRow(r)) = d.get(id).cloned() else { panic!() };
        assert_eq!(r.host, Some(t));
        assert!((d.traffic_z(lvl, r.host, p(1000.0, 1000.0)) - (lz + 300.0)).abs() < 1e-6);
        let k = d.add_kerb(lvl, vec![Curve::line(p(0.0, -500.0), p(25_000.0, -500.0))], 150.0, 150.0).unwrap();
        assert!(matches!(d.get(k), Some(Element::Kerb(k)) if k.host == Some(t)));
        assert!(d.patch_traffic(id, &serde_json::json!({ "bay_width_mm": 30_000.0 })).is_err(), "too wide for the row");
        d.patch_traffic(id, &serde_json::json!({ "angle_deg": 60.0 })).unwrap();
        assert!(d.add_line_marking(lvl, vec![Curve::line(p(0.0, 0.0), p(0.0, 0.0))]).is_err());
    }
}
