//! Property lines: the legal boundary of a site, drawn in plans. Sketched, or made from a survey's distances and
//! bearings (Create by entering distances and bearings), segment by segment from a start point; a segment is a
//! line or an arc (its chord's distance and bearing, a radius, and which side it bulges to).
//!
//! The lines are kept as curves, a chain end to end. The table is read back off them, so a sketched property line
//! can be edited as a table too (Edit Table), after which it is a table-based one. Bearings are whole circle
//! bearings from true north (the project's Angle to True North), clockwise. When the chain closes, it has an area.
//! Each segment can be labelled in plans with its bearing and distance.

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::{Document, Element, Id, Point};

/// How far apart two ends can be and still meet (mm).
const MEET: f64 = 1.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyLine {
    /// The segments, end to end.
    pub curves: Vec<Curve>,
    /// Made (or last edited) from distances and bearings: it is edited as a table, not sketched.
    #[serde(default)]
    pub table: bool,
    #[serde(default)]
    pub name: String,
    /// Plans label each segment with its bearing and distance.
    #[serde(default = "yes")]
    pub label_segments: bool,
    #[serde(default)]
    pub pinned: bool,
}

fn yes() -> bool { true }

/// A row of the distances and bearings table: the chord's length and whole circle bearing from true north, and for
/// an arc its radius and whether it bulges to the left of the chord (going along it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bearing {
    pub distance_mm: f64,
    pub bearing_deg: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius_mm: Option<f64>,
    #[serde(default)]
    pub left: bool,
}

impl PropertyLine {
    /// Closed: the chain ends where it starts.
    pub fn closed(&self) -> bool {
        match (self.curves.first(), self.curves.last()) {
            (Some(a), Some(b)) => self.curves.len() > 1 && a.start().dist(&b.end()) < MEET,
            _ => false,
        }
    }

    /// The chain as one polygon (arcs as short chords).
    pub fn polygon(&self) -> Vec<Point> {
        let mut out: Vec<Point> = vec![];
        for c in &self.curves {
            let p = c.tessellate(20.0);
            out.extend(p.into_iter().skip(if out.is_empty() { 0 } else { 1 }));
        }
        if self.closed() { out.pop(); }
        out
    }

    /// Area (mm²) when closed.
    pub fn area(&self) -> Option<f64> {
        self.closed().then(|| crate::geom::signed_area(&self.polygon()).abs())
    }
}

/// Curves put end to end, each turned to follow on from the last. None when they don't make one chain.
pub fn chain(curves: &[Curve]) -> Option<Vec<Curve>> {
    let mut left: Vec<Curve> = curves.iter().filter(|c| c.length() > 1e-6).cloned().collect();
    if left.is_empty() { return None; }
    // start from an end that only one curve has, so an open chain is followed from one end
    let ends = |p: Point, cs: &[Curve]| cs.iter().filter(|c| c.start().dist(&p) < MEET || c.end().dist(&p) < MEET).count();
    let first = (0..left.len()).find(|&i| ends(left[i].start(), &left) == 1).or_else(|| (0..left.len()).find(|&i| ends(left[i].end(), &left) == 1).map(|i| { left[i] = left[i].reversed(); i })).unwrap_or(0);
    let mut out = vec![left.remove(first)];
    while !left.is_empty() {
        let e = out.last()?.end();
        let k = left.iter().position(|c| c.start().dist(&e) < MEET || c.end().dist(&e) < MEET)?;
        let c = left.remove(k);
        out.push(if c.start().dist(&e) < MEET { c } else { c.reversed() });
    }
    Some(out)
}

/// A whole circle bearing in degrees, minutes and seconds: 82°30'00".
pub fn format_bearing(deg: f64) -> String {
    let s = (deg.rem_euclid(360.0) * 3600.0).round() as i64 % (360 * 3600);
    format!("{}\u{b0}{:02}'{:02}\"", s / 3600, s / 60 % 60, s % 60)
}

/// A bearing as typed: whole circle in degrees (82.5), or degrees, minutes and seconds with any marks or spaces
/// between them (82°30'00", 82 30 0, 82-30), or a quadrant bearing (N 82°30' E, S12E).
pub fn parse_bearing(text: &str) -> Result<f64, String> {
    let t = text.trim().to_uppercase();
    let bad = || format!("\"{}\" isn't a bearing: type it as 82°30'00\" (from north, clockwise) or N 82°30' E.", text.trim());
    let (from, body, to) = match (t.chars().next(), t.chars().last()) {
        (Some(a @ ('N' | 'S')), Some(b @ ('E' | 'W'))) => (Some(a), t[1..t.len() - 1].to_string(), Some(b)),
        _ => (None, t.clone(), None),
    };
    let nums: Vec<f64> = body.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|s| !s.is_empty()).map(|s| s.parse::<f64>()).collect::<Result<_, _>>().map_err(|_| bad())?;
    if nums.is_empty() || nums.len() > 3 || nums[1..].iter().any(|v| *v >= 60.0) { return Err(bad()); }
    let deg = nums[0] + nums.get(1).unwrap_or(&0.0) / 60.0 + nums.get(2).unwrap_or(&0.0) / 3600.0;
    match (from, to) {
        (Some(f), Some(e)) => {
            if deg > 90.0 { return Err(bad()); }
            Ok(match (f, e) { ('N', 'E') => deg, ('S', 'E') => 180.0 - deg, ('S', _) => 180.0 + deg, _ => (360.0 - deg) % 360.0 })
        }
        _ if deg < 360.0 => Ok(deg),
        _ => Err(bad()),
    }
}

impl Document {
    pub fn property_lines(&self) -> Vec<(Id, &PropertyLine)> {
        self.iter().filter_map(|(id, e)| if let Element::PropertyLine(p) = e { Some((id, p)) } else { None }).collect()
    }

    pub fn property_line(&self, id: Id) -> Option<&PropertyLine> {
        if let Some(Element::PropertyLine(p)) = self.get(id) { Some(p) } else { None }
    }

    /// The angle from project north to true north (degrees, counter-clockwise).
    fn true_north(&self) -> f64 {
        self.project_location().map_or(0.0, |(_, l)| l.angle_true_north_deg)
    }

    /// A direction in plan from a bearing.
    fn bearing_dir(&self, deg: f64) -> Point {
        let (e, n) = (deg.to_radians().sin(), deg.to_radians().cos());
        let (sn, cs) = self.true_north().to_radians().sin_cos();
        Point::new(e * cs - n * sn, e * sn + n * cs)
    }

    /// The bearing of a direction in plan.
    fn dir_bearing(&self, v: Point) -> f64 {
        let (sn, cs) = (-self.true_north()).to_radians().sin_cos();
        let (e, n) = (v.x * cs - v.y * sn, v.x * sn + v.y * cs);
        e.atan2(n).to_degrees().rem_euclid(360.0)
    }

    /// The segments a table gives, from `at`.
    pub fn property_line_curves(&self, at: Point, rows: &[Bearing]) -> Result<Vec<Curve>, String> {
        if rows.is_empty() { return Err("Add a distance and bearing for each segment.".into()); }
        let mut p = at;
        let mut out = vec![];
        for (i, r) in rows.iter().enumerate() {
            if !(r.distance_mm.is_finite() && r.distance_mm > 1.0) { return Err(format!("Segment {}: the distance has to be more than 1 mm.", i + 1)); }
            if !r.bearing_deg.is_finite() { return Err(format!("Segment {}: the bearing has to be a number.", i + 1)); }
            let d = self.bearing_dir(r.bearing_deg);
            let q = Point::new(p.x + d.x * r.distance_mm, p.y + d.y * r.distance_mm);
            out.push(match r.radius_mm {
                None => Curve::line(p, q),
                Some(rad) => {
                    let side = if r.left { Point::new(p.x - d.y, p.y + d.x) } else { Point::new(p.x + d.y, p.y - d.x) };
                    crate::sketch::arc_start_end_radius(p, q, rad, side).ok_or_else(|| format!("Segment {}: the radius has to be more than half the distance.", i + 1))?
                }
            });
            p = q;
        }
        Ok(out)
    }

    /// The distances and bearings of a property line's segments (an arc by its chord, radius and side).
    pub fn property_line_table(&self, pl: &PropertyLine) -> Vec<Bearing> {
        pl.curves.iter().map(|c| {
            let (a, b) = (c.start(), c.end());
            let (radius_mm, left) = match c {
                Curve::Arc { centre, .. } => {
                    // which side of the chord its middle is on
                    let mid = c.mid();
                    (Some(centre.dist(&a)), (b.x - a.x) * (mid.y - a.y) - (b.y - a.y) * (mid.x - a.x) > 0.0)
                }
                _ => (None, false),
            };
            Bearing { distance_mm: a.dist(&b), bearing_deg: self.dir_bearing(Point::new(b.x - a.x, b.y - a.y)), radius_mm, left }
        }).collect()
    }

    fn check_property_line(curves: &[Curve]) -> Result<Vec<Curve>, String> {
        if curves.iter().any(|c| !matches!(c, Curve::Line { .. } | Curve::Arc { .. })) { return Err("A property line is made of lines and arcs.".into()); }
        chain(curves).ok_or_else(|| "A property line has to be one run of lines and arcs, each starting where the last one ends.".into())
    }

    /// Create by sketching: the lines and arcs, put end to end.
    pub fn add_property_line_sketch(&mut self, curves: Vec<Curve>) -> Result<Id, String> {
        let curves = Self::check_property_line(&curves)?;
        let p = PropertyLine { curves, table: false, name: String::new(), label_segments: true, pinned: false };
        self.transact("Create property line", |tx| Ok(tx.add(Element::PropertyLine(p))))
    }

    /// Create by entering distances and bearings, from `at`.
    pub fn add_property_line_table(&mut self, at: Point, rows: &[Bearing]) -> Result<Id, String> {
        let curves = self.property_line_curves(at, rows)?;
        let p = PropertyLine { curves, table: true, name: String::new(), label_segments: true, pinned: false };
        self.transact("Create property line", |tx| Ok(tx.add(Element::PropertyLine(p))))
    }

    /// Edit Sketch.
    pub fn set_property_line_sketch(&mut self, id: Id, curves: Vec<Curve>) -> Result<(), String> {
        let mut p = self.property_line(id).cloned().ok_or("not a property line")?;
        if p.table { return Err("This property line was made from distances and bearings: edit its table.".into()); }
        self.refuse_pinned(&[id], "edited")?;
        p.curves = Self::check_property_line(&curves)?;
        self.transact("Edit property line", |tx| tx.update(id, Element::PropertyLine(p)))
    }

    /// Edit Table: the segments from the table, from where it starts now. A sketched one becomes table-based.
    pub fn set_property_line_table(&mut self, id: Id, rows: &[Bearing]) -> Result<(), String> {
        let mut p = self.property_line(id).cloned().ok_or("not a property line")?;
        self.refuse_pinned(&[id], "edited")?;
        let at = p.curves.first().map(|c| c.start()).ok_or("the property line has no segments")?;
        p.curves = self.property_line_curves(at, rows)?;
        p.table = true;
        self.transact("Edit property line table", |tx| tx.update(id, Element::PropertyLine(p)))
    }

    pub fn patch_property_line(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.property_line(id).cloned().ok_or("not a property line")?;
        let mut p: PropertyLine = crate::doc::apply_patch(&old, patch)?;
        if old.pinned && p.pinned && p.curves != old.curves { return Err("Unpin the property line before moving it.".into()); }
        p.curves = Self::check_property_line(&p.curves)?;
        self.transact("Property line properties", |tx| tx.update(id, Element::PropertyLine(p)))
    }

    /// What plans write along each segment: where (the middle), the text's angle (radians, reading left to right)
    /// and the text: bearing and distance (m), and an arc's radius.
    pub fn property_line_labels(&self, pl: &PropertyLine) -> Vec<(Point, f64, String)> {
        pl.curves.iter().zip(self.property_line_table(pl)).map(|(c, r)| {
            let (a, b) = (c.start(), c.end());
            let mut ang = (b.y - a.y).atan2(b.x - a.x);
            if ang > std::f64::consts::FRAC_PI_2 + 1e-9 { ang -= std::f64::consts::PI; }
            if ang < -std::f64::consts::FRAC_PI_2 - 1e-9 { ang += std::f64::consts::PI; }
            let mut text = format!("{}  {:.3}", format_bearing(r.bearing_deg), r.distance_mm / 1000.0);
            if let Some(rad) = r.radius_mm { text.push_str(&format!("  R{:.3}", rad / 1000.0)); }
            (c.mid(), ang, text)
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point { Point::new(x, y) }

    #[test]
    fn bearings_read_and_write_as_surveys_give_them() {
        assert_eq!(format_bearing(82.5), "82\u{b0}30'00\"");
        assert_eq!(format_bearing(359.99999), "0\u{b0}00'00\"");
        for (t, v) in [("82°30'00\"", 82.5), ("82 30 0", 82.5), ("82.5", 82.5), ("N 82°30' E", 82.5), ("S12E", 168.0), ("S 12 W", 192.0), ("N12W", 348.0), ("270", 270.0)] {
            assert!((parse_bearing(t).unwrap() - v).abs() < 1e-9, "{t}");
        }
        for t in ["", "abc", "361", "N 95 E", "12 75"] { assert!(parse_bearing(t).is_err(), "{t}"); }
    }

    #[test]
    fn a_table_makes_a_closed_boundary_with_an_area_and_reads_back() {
        let mut d = Document::new_project();
        // a 20 x 40 m lot, east then north then west then south, with an arc on the last side
        let rows = vec![
            Bearing { distance_mm: 20_000.0, bearing_deg: 90.0, radius_mm: None, left: false },
            Bearing { distance_mm: 40_000.0, bearing_deg: 0.0, radius_mm: None, left: false },
            Bearing { distance_mm: 20_000.0, bearing_deg: 270.0, radius_mm: None, left: false },
            Bearing { distance_mm: 40_000.0, bearing_deg: 180.0, radius_mm: None, left: false },
        ];
        let id = d.add_property_line_table(p(1000.0, 2000.0), &rows).unwrap();
        let pl = d.property_line(id).unwrap().clone();
        assert!(pl.closed() && pl.table);
        assert!((pl.area().unwrap() - 800e6).abs() < 1.0);
        let back = d.property_line_table(&pl);
        for (a, b) in back.iter().zip(&rows) { assert!((a.distance_mm - b.distance_mm).abs() < 1e-6 && (a.bearing_deg - b.bearing_deg).abs() < 1e-6, "{back:?}"); }
        assert!(pl.curves[1].end().dist(&p(21_000.0, 42_000.0)) < 1e-6);
        // true north turned: the same table runs the same way on the ground, turned on the plan
        d.patch_project_location(&serde_json::json!({ "angle_true_north_deg": 90.0 })).unwrap();
        let turned = d.property_line_curves(p(0.0, 0.0), &rows[..1]).unwrap();
        assert!(turned[0].end().dist(&p(0.0, 20_000.0)) < 1e-6, "{:?}", turned[0].end());
        assert!((d.property_line_table(&PropertyLine { curves: turned, ..pl.clone() })[0].bearing_deg - 90.0).abs() < 1e-9);
        d.patch_project_location(&serde_json::json!({ "angle_true_north_deg": 0.0 })).unwrap();
        // an arc on the last side, bulging to the right of it (going south, that's west, out of the lot)
        let mut arc = rows.clone();
        arc[3].radius_mm = Some(25_000.0);
        arc[3].left = false;
        d.set_property_line_table(id, &arc).unwrap();
        let pl = d.property_line(id).unwrap().clone();
        assert!(pl.closed());
        let back = d.property_line_table(&pl);
        assert_eq!(back[3].radius_mm.map(|r| r.round()), Some(25_000.0));
        assert!(!back[3].left);
        assert!(pl.area().unwrap() > 800e6 + 1e6, "{}", pl.area().unwrap());
        assert!(d.set_property_line_table(id, &[Bearing { distance_mm: 10_000.0, bearing_deg: 0.0, radius_mm: Some(1000.0), left: true }]).is_err());
        assert!(d.property_line_labels(&pl)[0].2.starts_with("90\u{b0}00'00\"  20.000"));
    }

    #[test]
    fn a_sketch_is_put_end_to_end_and_an_open_one_has_no_area() {
        let mut d = Document::new_project();
        let cs = vec![Curve::line(p(0.0, 0.0), p(1000.0, 0.0)), Curve::line(p(1000.0, 1000.0), p(1000.0, 0.0)), Curve::line(p(1000.0, 1000.0), p(0.0, 1000.0))];
        let id = d.add_property_line_sketch(cs).unwrap();
        let pl = d.property_line(id).unwrap();
        assert!(!pl.closed() && pl.area().is_none() && !pl.table);
        assert!(pl.curves[0].start().dist(&p(0.0, 0.0)) < 1e-6 && pl.curves[2].end().dist(&p(0.0, 1000.0)) < 1e-6);
        assert!(d.add_property_line_sketch(vec![Curve::line(p(0.0, 0.0), p(1.0e4, 0.0)), Curve::line(p(0.0, 5000.0), p(1.0e4, 5000.0))]).is_err());
        // a table-based one isn't sketched
        let t = d.add_property_line_table(p(0.0, 0.0), &[Bearing { distance_mm: 5000.0, bearing_deg: 45.0, radius_mm: None, left: false }]).unwrap();
        assert!(d.set_property_line_sketch(t, vec![Curve::line(p(0.0, 0.0), p(1.0, 1.0e4))]).is_err());
        d.patch_property_line(id, &serde_json::json!({ "name": "Lot 12", "pinned": true })).unwrap();
        assert!(d.delete(&[id]).is_err(), "pinned");
    }
}
