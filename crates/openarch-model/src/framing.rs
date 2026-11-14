//! Beam systems, trusses and columns at grids.
//!
//! A beam system is a boundary filled with parallel beams by a layout rule: a fixed distance, a fixed number, a
//! maximum spacing or a clear spacing between them. Its beams are worked out from the boundary whenever they are
//! drawn, and join what they land on as placed beams do.
//!
//! A truss is a span line with a profile (parallel chords, or pitched to an apex), a web layout (Pratt, Howe or
//! Warren), a height and a number of panels, and a section type each for its top chord, bottom chord and webs.
//! Its members meet on their centre lines at the panel points. Drop Truss turns it into separate beams.
//!
//! Columns at grids: a column at every crossing of the chosen grids.

use serde::{Deserialize, Serialize};

use crate::doc::apply_patch;
use crate::edit::Xform;
use crate::structural::{Beam, FramingUsage, Member, StructuralColumn, StructuralUse, ZJustification, V3};
use crate::{Document, Element, Id, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutRule {
    #[default]
    FixedDistance,
    FixedNumber,
    MaximumSpacing,
    ClearSpacing,
}

impl LayoutRule {
    pub const ALL: [LayoutRule; 4] = [LayoutRule::FixedDistance, LayoutRule::FixedNumber, LayoutRule::MaximumSpacing, LayoutRule::ClearSpacing];
    pub fn label(self) -> &'static str {
        match self { LayoutRule::FixedDistance => "Fixed Distance", LayoutRule::FixedNumber => "Fixed Number", LayoutRule::MaximumSpacing => "Maximum Spacing", LayoutRule::ClearSpacing => "Clear Spacing" }
    }
}

/// Where a fixed distance or clear spacing layout starts from, across the beams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemJustification {
    Beginning,
    #[default]
    Center,
    End,
}

fn default_spacing() -> f64 { 600.0 }
fn default_count() -> u32 { 5 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeamSystem {
    /// The beams' type (a framing type).
    pub structural_type: Id,
    pub level: Id,
    pub boundary: Vec<Point>,
    /// The way the beams run, anticlockwise from east.
    #[serde(default)]
    pub direction_deg: f64,
    #[serde(default)]
    pub rule: LayoutRule,
    #[serde(default = "default_spacing")]
    pub spacing_mm: f64,
    #[serde(default = "default_count")]
    pub count: u32,
    #[serde(default)]
    pub justification: SystemJustification,
    /// The beams' height above the level (their tops, as a beam's).
    #[serde(default)]
    pub offset_mm: f64,
    #[serde(default)]
    pub usage: FramingUsage,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrussProfile {
    /// Top and bottom chords parallel, the height apart.
    #[default]
    Parallel,
    /// The top chords rise from each end to an apex over the middle.
    Pitched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrussWeb {
    /// Verticals, and diagonals that fall towards the middle.
    #[default]
    Pratt,
    /// Verticals, and diagonals that rise towards the middle.
    Howe,
    /// Diagonals alone, up and down in turn.
    Warren,
}

fn default_truss_height() -> f64 { 1200.0 }
fn default_panels() -> u32 { 6 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Truss {
    pub level: Id,
    pub start: Point,
    pub end: Point,
    /// The bottom chord's centre line above the level.
    #[serde(default)]
    pub offset_mm: f64,
    /// Between the chords' centre lines (at the apex of a pitched one).
    #[serde(default = "default_truss_height")]
    pub height_mm: f64,
    #[serde(default)]
    pub profile: TrussProfile,
    #[serde(default)]
    pub web: TrussWeb,
    #[serde(default = "default_panels")]
    pub panels: u32,
    pub top_chord: Id,
    pub bottom_chord: Id,
    pub web_type: Id,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub pinned: bool,
}

/// Which part of a truss a member is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrussPart {
    TopChord,
    BottomChord,
    Vertical,
    Diagonal,
}

fn unit(a: V3) -> V3 { let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-12); [a[0] / l, a[1] / l, a[2] / l] }
fn cross(a: V3, b: V3) -> V3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }

impl Document {
    pub fn beam_systems(&self) -> Vec<(Id, &BeamSystem)> {
        self.iter().filter_map(|(id, e)| match e { Element::BeamSystem(s) => Some((id, s)), _ => None }).collect()
    }
    pub fn trusses(&self) -> Vec<(Id, &Truss)> {
        self.iter().filter_map(|(id, e)| match e { Element::Truss(t) => Some((id, t)), _ => None }).collect()
    }

    fn framing_type(&self, id: Id) -> Result<&crate::structural::StructuralType, String> {
        let t = self.structural_type(id).ok_or("unknown structural type")?;
        if t.used_for != StructuralUse::Framing { return Err("That type is a column, not a framing section.".into()); }
        Ok(t)
    }

    fn check_beam_system(&self, s: &BeamSystem) -> Result<(), String> {
        self.framing_type(s.structural_type)?;
        if self.level(s.level).is_none() { return Err("unknown level".into()); }
        if s.boundary.len() < 3 || crate::geom::signed_area(&s.boundary).abs() < 1.0 { return Err("A beam system's boundary is a closed outline of three or more points.".into()); }
        if !s.boundary.iter().all(|p| p.x.is_finite() && p.y.is_finite()) || ![s.direction_deg, s.spacing_mm, s.offset_mm].iter().all(|v| v.is_finite()) { return Err("a beam system's boundary and settings are numbers".into()); }
        if s.spacing_mm < 50.0 { return Err("Beams are spaced 50 or more apart.".into()); }
        if s.count < 1 || s.count > 500 { return Err("A beam system has between 1 and 500 beams.".into()); }
        Ok(())
    }

    pub fn add_beam_system(&mut self, s: BeamSystem) -> Result<Id, String> {
        self.check_beam_system(&s)?;
        self.transact("Beam system", |tx| Ok(tx.add(Element::BeamSystem(s))))
    }

    /// The beams a system lays out: lines across its boundary in its direction, spaced by its rule.
    pub fn system_beams(&self, s: &BeamSystem) -> Vec<Beam> {
        let (sn, cs) = s.direction_deg.to_radians().sin_cos();
        let (u, v) = ([cs, sn], [-sn, cs]);
        let across = |p: &Point| p.x * v[0] + p.y * v[1];
        let lo = s.boundary.iter().map(across).fold(f64::INFINITY, f64::min);
        let hi = s.boundary.iter().map(across).fold(f64::NEG_INFINITY, f64::max);
        let span = hi - lo;
        if !(span > 1.0) { return vec![]; }
        let width = self.structural_type(s.structural_type).map_or(0.0, |t| t.section.size().0);
        let mut at: Vec<f64> = vec![];
        match s.rule {
            LayoutRule::FixedNumber => { for k in 1..=s.count { at.push(lo + span * k as f64 / (s.count + 1) as f64); } }
            LayoutRule::MaximumSpacing => {
                let n = (span / s.spacing_mm.max(50.0)).ceil().max(1.0) as u32;
                for k in 1..n.min(2000) { at.push(lo + span * k as f64 / n as f64); }
            }
            LayoutRule::FixedDistance | LayoutRule::ClearSpacing => {
                let step = s.spacing_mm.max(50.0) + if s.rule == LayoutRule::ClearSpacing { width } else { 0.0 };
                let inside = |x: f64| x > lo + 1.0 && x < hi - 1.0;
                match s.justification {
                    SystemJustification::Beginning => { let mut x = lo + step; while inside(x) && at.len() < 2000 { at.push(x); x += step; } }
                    SystemJustification::End => { let mut x = hi - step; while inside(x) && at.len() < 2000 { at.push(x); x -= step; } }
                    SystemJustification::Center => {
                        let mid = (lo + hi) / 2.0;
                        at.push(mid);
                        let mut k = 1.0;
                        while inside(mid + k * step) && at.len() < 2000 { at.push(mid + k * step); at.push(mid - k * step); k += 1.0; }
                    }
                }
                at.sort_by(f64::total_cmp);
            }
        }
        let mut out = vec![];
        let n = s.boundary.len();
        for x in at {
            // where the line at this distance crosses the boundary, along the beams' direction
            let mut hits: Vec<f64> = vec![];
            for i in 0..n {
                let (a, b) = (s.boundary[i], s.boundary[(i + 1) % n]);
                let (da, db) = (across(&a) - x, across(&b) - x);
                if (da < 0.0) == (db < 0.0) { continue; }
                let f = da / (da - db);
                hits.push((a.x + (b.x - a.x) * f) * u[0] + (a.y + (b.y - a.y) * f) * u[1]);
            }
            hits.sort_by(f64::total_cmp);
            for pair in hits.chunks(2) {
                if pair.len() < 2 || pair[1] - pair[0] < 10.0 { continue; }
                let p = |t: f64| Point::new(u[0] * t + v[0] * x, u[1] * t + v[1] * x);
                out.push(Beam { start_offset_mm: s.offset_mm, end_offset_mm: s.offset_mm, usage: s.usage, ..Beam::new(s.structural_type, s.level, p(pair[0]), p(pair[1])) });
            }
        }
        out
    }

    fn check_truss(&self, t: &Truss) -> Result<(), String> {
        for id in [t.top_chord, t.bottom_chord, t.web_type] { self.framing_type(id)?; }
        if self.level(t.level).is_none() { return Err("unknown level".into()); }
        if ![t.start.x, t.start.y, t.end.x, t.end.y, t.offset_mm, t.height_mm].iter().all(|v| v.is_finite()) { return Err("a truss's ends and sizes are numbers".into()); }
        if t.start.dist(&t.end) < 500.0 { return Err("A truss spans 500 or more.".into()); }
        if !(100.0..=50_000.0).contains(&t.height_mm) { return Err("A truss is between 100 and 50 000 high.".into()); }
        if t.panels < 2 || t.panels > 200 { return Err("A truss has between 2 and 200 panels.".into()); }
        Ok(())
    }

    pub fn add_truss(&mut self, t: Truss) -> Result<Id, String> {
        self.check_truss(&t)?;
        self.transact("Truss", |tx| Ok(tx.add(Element::Truss(t))))
    }

    /// A truss's members as lines between its panel points: which part each is, its type and its two ends.
    pub fn truss_lines(&self, t: &Truss) -> Vec<(TrussPart, Id, V3, V3)> {
        // a pitched truss has a panel point under its apex
        let n = if t.profile == TrussProfile::Pitched { (t.panels + 1) / 2 * 2 } else { t.panels }.max(2) as usize;
        let z0 = self.level(t.level).map_or(0.0, |l| l.elevation_mm) + t.offset_mm;
        let rise = |i: usize| match t.profile { TrussProfile::Parallel => t.height_mm, TrussProfile::Pitched => t.height_mm * (1.0 - (2.0 * i as f64 / n as f64 - 1.0).abs()) };
        let at = |i: usize| { let f = i as f64 / n as f64; (t.start.x + (t.end.x - t.start.x) * f, t.start.y + (t.end.y - t.start.y) * f) };
        let bottom = |i: usize| { let (x, y) = at(i); [x, y, z0] };
        let top = |i: usize| { let (x, y) = at(i); [x, y, z0 + rise(i)] };
        let mut out = vec![(TrussPart::BottomChord, t.bottom_chord, bottom(0), bottom(n))];
        match t.profile {
            TrussProfile::Parallel => out.push((TrussPart::TopChord, t.top_chord, top(0), top(n))),
            TrussProfile::Pitched => { out.push((TrussPart::TopChord, t.top_chord, top(0), top(n / 2))); out.push((TrussPart::TopChord, t.top_chord, top(n / 2), top(n))); }
        }
        let open = |i: usize| rise(i) > 1.0;
        for i in 0..=n {
            let end = i == 0 || i == n;
            if open(i) && (t.web != TrussWeb::Warren || end) { out.push((TrussPart::Vertical, t.web_type, bottom(i), top(i))); }
        }
        for i in 0..n {
            // rising from this panel point to the next one's top, or falling from this one's top
            let rising = match t.web {
                TrussWeb::Warren => i % 2 == 0,
                TrussWeb::Pratt => i >= n / 2,
                TrussWeb::Howe => i < n / 2,
            };
            let (a, b, ia, ib) = if rising { (bottom(i), top(i + 1), i, i + 1) } else { (top(i), bottom(i + 1), i, i + 1) };
            // none along a chord, where the truss has no height at one end of the panel
            if !open(ia) || !open(ib) { continue; }
            out.push((TrussPart::Diagonal, t.web_type, a, b));
        }
        out
    }

    /// A truss's members as solids, each with its type: centred on its line, its section upright in the truss's
    /// plane.
    pub fn truss_members(&self, t: &Truss) -> Vec<(TrussPart, Id, Member)> {
        let len = t.start.dist(&t.end).max(1e-9);
        let right = [(t.end.y - t.start.y) / len, -(t.end.x - t.start.x) / len, 0.0];
        self.truss_lines(t).into_iter().filter_map(|(part, ty, a, b)| {
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if length < 1.0 { return None; }
            let axis = unit(d);
            Some((part, ty, Member { origin: a, across: right, up: unit(cross(right, axis)), axis, length, start_cut: None, end_cut: None }))
        }).collect()
    }

    /// Drop Truss: the truss gives way to separate beams, one for each member, centred on the same lines.
    pub fn drop_truss(&mut self, id: Id) -> Result<Vec<Id>, String> {
        let t = match self.get(id) { Some(Element::Truss(t)) => t.clone(), _ => return Err("not a truss".into()) };
        if t.pinned { return Err("The truss is pinned; unpin it first.".into()); }
        let z = self.level(t.level).map_or(0.0, |l| l.elevation_mm);
        let len = t.start.dist(&t.end).max(1e-9);
        // a vertical web keeps its section in the truss's plane by its rotation
        let turn = (-(-(t.end.x - t.start.x) / len)).atan2((t.end.y - t.start.y) / len).to_degrees();
        let beams: Vec<Beam> = self.truss_lines(&t).into_iter().map(|(part, ty, a, b)| Beam {
            start_offset_mm: a[2] - z, end_offset_mm: b[2] - z, z_justification: ZJustification::Center, start_join: false, end_join: false,
            rotation_deg: if part == TrussPart::Vertical { turn } else { 0.0 },
            usage: match part { TrussPart::Vertical | TrussPart::Diagonal => FramingUsage::VerticalBracing, _ => FramingUsage::Other },
            ..Beam::new(ty, t.level, Point::new(a[0], a[1]), Point::new(b[0], b[1]))
        }).collect();
        self.transact("Drop truss", |tx| {
            tx.remove(id)?;
            Ok(beams.into_iter().map(|b| tx.add(Element::Beam(b))).collect())
        })
    }

    /// Properties of a beam system or a truss, by patch.
    pub fn patch_framing(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        match self.get(id).cloned() {
            Some(Element::BeamSystem(old)) => {
                let new: BeamSystem = apply_patch(&old, patch)?;
                if new == old { return Ok(()); }
                if old.pinned && new.pinned && (BeamSystem { mark: new.mark.clone(), comments: new.comments.clone(), ..old.clone() }) != new { return Err("The beam system is pinned; unpin it first.".into()); }
                self.check_beam_system(&new)?;
                self.transact("Change beam system", |tx| tx.update(id, Element::BeamSystem(new)))
            }
            Some(Element::Truss(old)) => {
                let new: Truss = apply_patch(&old, patch)?;
                if new == old { return Ok(()); }
                if old.pinned && new.pinned && (Truss { mark: new.mark.clone(), comments: new.comments.clone(), ..old.clone() }) != new { return Err("The truss is pinned; unpin it first.".into()); }
                self.check_truss(&new)?;
                self.transact("Change truss", |tx| tx.update(id, Element::Truss(new)))
            }
            _ => Err("not a beam system or a truss".into()),
        }
    }

    /// A beam system or truss moved, turned, mirrored or copied.
    pub(crate) fn framing_transformed(e: &Element, x: &Xform, copy: bool) -> Option<Element> {
        match e {
            Element::BeamSystem(s) => {
                let mut s = s.clone();
                // the direction goes with the boundary: where a step along it lands
                let (sn, cs) = s.direction_deg.to_radians().sin_cos();
                let (o, a) = (x.apply(Point::new(0.0, 0.0)), x.apply(Point::new(cs, sn)));
                s.direction_deg = (a.y - o.y).atan2(a.x - o.x).to_degrees();
                s.boundary = s.boundary.iter().map(|p| x.apply(*p)).collect();
                if copy { s.pinned = false; s.mark.clear(); }
                Some(Element::BeamSystem(s))
            }
            Element::Truss(t) => {
                let mut t = t.clone();
                t.start = x.apply(t.start);
                t.end = x.apply(t.end);
                if copy { t.pinned = false; t.mark.clear(); }
                Some(Element::Truss(t))
            }
            _ => None,
        }
    }

    /// Where the given grids cross each other (every grid with none given).
    pub fn grid_crossings(&self, grids: &[Id]) -> Vec<Point> {
        let lines: Vec<(Id, Vec<Point>)> = self.grids().into_iter().filter(|(id, _)| grids.is_empty() || grids.contains(id))
            .map(|(id, g)| (id, g.curves.iter().flat_map(|c| c.tessellate(2.0)).collect())).collect();
        let mut out: Vec<Point> = vec![];
        for i in 0..lines.len() {
            for j in i + 1..lines.len() {
                for a in lines[i].1.windows(2) {
                    for b in lines[j].1.windows(2) {
                        let (r, s) = ([a[1].x - a[0].x, a[1].y - a[0].y], [b[1].x - b[0].x, b[1].y - b[0].y]);
                        let den = r[0] * s[1] - r[1] * s[0];
                        if den.abs() < 1e-9 { continue; }
                        let (qx, qy) = (b[0].x - a[0].x, b[0].y - a[0].y);
                        let (t, w) = ((qx * s[1] - qy * s[0]) / den, (qx * r[1] - qy * r[0]) / den);
                        if !(-1e-6..=1.0 + 1e-6).contains(&t) || !(-1e-6..=1.0 + 1e-6).contains(&w) { continue; }
                        let p = Point::new(a[0].x + r[0] * t, a[0].y + r[1] * t);
                        if !out.iter().any(|q| q.dist(&p) < 1.0) { out.push(p); }
                    }
                }
            }
        }
        out
    }

    /// At Grids: a column like `like` at every crossing of the grids that has none on that level yet. One undo.
    pub fn add_columns_at_grids(&mut self, like: StructuralColumn, grids: &[Id]) -> Result<Vec<Id>, String> {
        let taken: Vec<Point> = self.structural_columns().into_iter().filter(|(_, c)| c.level == like.level).map(|(_, c)| c.at).collect();
        let at: Vec<Point> = self.grid_crossings(grids).into_iter().filter(|p| !taken.iter().any(|q| q.dist(p) < 1.0)).collect();
        if at.is_empty() { return Err("The grids don't cross anywhere that has no column yet.".into()); }
        let columns: Vec<StructuralColumn> = at.into_iter().map(|p| StructuralColumn { at: p, top_at: None, ..like.clone() }).collect();
        for c in &columns { self.check_column(c)?; }
        self.transact("Columns at grids", |tx| Ok(columns.into_iter().map(|c| tx.add(Element::StructuralColumn(c))).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Document, Id, Id, Id, Id) {
        let mut d = Document::new_project();
        d.ensure_structural_types().unwrap();
        let levels: Vec<Id> = d.levels().iter().map(|(i, _)| *i).collect();
        let col = d.structural_types().iter().find(|(_, t)| t.name == "300 x 300").unwrap().0;
        let ub = d.structural_types().iter().find(|(_, t)| t.name == "200UB25.4").unwrap().0;
        (d, levels[0], levels[1], col, ub)
    }

    fn system(ub: Id, level: Id) -> BeamSystem {
        BeamSystem { structural_type: ub, level, boundary: vec![Point::new(0.0, 0.0), Point::new(6000.0, 0.0), Point::new(6000.0, 3000.0), Point::new(0.0, 3000.0)], direction_deg: -90.0,
            rule: LayoutRule::FixedNumber, spacing_mm: 600.0, count: 5, justification: SystemJustification::Center, offset_mm: 0.0, usage: FramingUsage::Joist, mark: String::new(), comments: String::new(), pinned: false }
    }

    #[test]
    fn a_beam_system_lays_its_beams_out_by_its_rule() {
        let (mut d, _, l1, _, ub) = setup();
        let id = d.add_beam_system(system(ub, l1)).unwrap();
        let xs = |d: &Document| { let Some(Element::BeamSystem(s)) = d.get(id) else { unreachable!() }; d.system_beams(s).iter().map(|b| b.start.x.round()).collect::<Vec<_>>() };
        // five beams across 6 m, running north: at every metre
        assert_eq!(xs(&d), vec![1000.0, 2000.0, 3000.0, 4000.0, 5000.0]);
        let Some(Element::BeamSystem(s)) = d.get(id) else { unreachable!() };
        assert!(d.system_beams(s).iter().all(|b| (b.start.y.min(b.end.y)).abs() < 1e-6 && (b.start.y.max(b.end.y) - 3000.0).abs() < 1e-6));
        // 900 apart from the middle; 900 apart from the start; no more than 1400 apart; 900 clear between 133 wide beams
        d.patch_framing(id, &serde_json::json!({ "rule": "fixed_distance", "spacing_mm": 900.0 })).unwrap();
        assert_eq!(xs(&d), vec![300.0, 1200.0, 2100.0, 3000.0, 3900.0, 4800.0, 5700.0]);
        d.patch_framing(id, &serde_json::json!({ "justification": "beginning" })).unwrap();
        assert_eq!(xs(&d), vec![900.0, 1800.0, 2700.0, 3600.0, 4500.0, 5400.0]);
        d.patch_framing(id, &serde_json::json!({ "rule": "maximum_spacing", "spacing_mm": 1400.0 })).unwrap();
        assert_eq!(xs(&d), vec![1200.0, 2400.0, 3600.0, 4800.0]);
        d.patch_framing(id, &serde_json::json!({ "rule": "clear_spacing", "spacing_mm": 900.0, "justification": "beginning" })).unwrap();
        assert_eq!(xs(&d), vec![1033.0, 2066.0, 3099.0, 4132.0, 5165.0]);
        // the beams are solids under the system's id, and a girder along an edge cuts them back
        d.patch_framing(id, &serde_json::json!({ "rule": "fixed_number", "count": 2 })).unwrap();
        assert_eq!(d.structural_members().iter().filter(|m| m.0 == id).count(), 2);
        let g = d.add_beam(Beam { usage: FramingUsage::Girder, ..Beam::new(ub, l1, Point::new(0.0, 0.0), Point::new(6000.0, 0.0)) }).unwrap();
        let m = d.structural_members().into_iter().find(|m| m.0 == id).unwrap().3;
        assert!((m.length - (3000.0 - 66.5 - 10.0)).abs() < 0.01, "{}", m.length);
        // a quarter turn takes the direction with the boundary
        d.transform_elements(&[id], Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 }, Default::default()).unwrap();
        let Some(Element::BeamSystem(s)) = d.get(id) else { unreachable!() };
        assert!(s.direction_deg.abs() < 1e-6 && (s.boundary[1].y - 6000.0).abs() < 1e-6);
        assert!(d.patch_framing(id, &serde_json::json!({ "spacing_mm": 10.0 })).is_err());
        let _ = g;
    }

    #[test]
    fn a_truss_has_chords_and_webs_and_drops_to_beams() {
        let (mut d, _, l1, _, ub) = setup();
        let z = d.level(l1).unwrap().elevation_mm;
        let t = Truss { level: l1, start: Point::new(0.0, 0.0), end: Point::new(9000.0, 0.0), offset_mm: 0.0, height_mm: 1500.0, profile: TrussProfile::Parallel, web: TrussWeb::Pratt, panels: 6,
            top_chord: ub, bottom_chord: ub, web_type: ub, mark: String::new(), comments: String::new(), pinned: false };
        let id = d.add_truss(t).unwrap();
        let count = |d: &Document, part: TrussPart| { let Some(Element::Truss(t)) = d.get(id) else { unreachable!() }; d.truss_lines(t).iter().filter(|l| l.0 == part).count() };
        // parallel Pratt of six panels: two chords, seven verticals, six diagonals that fall towards the middle
        assert_eq!((count(&d, TrussPart::TopChord), count(&d, TrussPart::BottomChord), count(&d, TrussPart::Vertical), count(&d, TrussPart::Diagonal)), (1, 1, 7, 6));
        let Some(Element::Truss(tr)) = d.get(id) else { unreachable!() };
        let first = d.truss_lines(tr).into_iter().find(|l| l.0 == TrussPart::Diagonal).unwrap();
        assert!((first.2[2] - (z + 1500.0)).abs() < 1e-6 && (first.3[2] - z).abs() < 1e-6 && first.3[0] > first.2[0]);
        // Warren: end verticals only; pitched Howe: an apex over the middle and no web along the chords
        d.patch_framing(id, &serde_json::json!({ "web": "warren" })).unwrap();
        assert_eq!((count(&d, TrussPart::Vertical), count(&d, TrussPart::Diagonal)), (2, 6));
        d.patch_framing(id, &serde_json::json!({ "profile": "pitched", "web": "howe" })).unwrap();
        assert_eq!((count(&d, TrussPart::TopChord), count(&d, TrussPart::Vertical), count(&d, TrussPart::Diagonal)), (2, 5, 4));
        let Some(Element::Truss(tr)) = d.get(id) else { unreachable!() };
        let top = d.truss_members(tr).iter().flat_map(|m| [m.2.origin[2], m.2.origin[2] + m.2.axis[2] * m.2.length]).fold(f64::NEG_INFINITY, f64::max);
        assert!((top - (z + 1500.0)).abs() < 1e-6);
        let solids = d.structural_meshes().iter().filter(|m| m.0 == id).count();
        assert_eq!(solids, 12);
        assert!(d.patch_framing(id, &serde_json::json!({ "panels": 1 })).is_err());
        // dropped: as many beams, the truss gone, and a vertical web still stands
        let beams = d.drop_truss(id).unwrap();
        assert_eq!(beams.len(), 12);
        assert!(d.get(id).is_none());
        let upright = d.structural_members().into_iter().filter(|m| m.3.axis[2] > 0.999).count();
        assert_eq!(upright, 5);
        d.undo().unwrap();
        assert!(d.get(id).is_some() && d.beams().is_empty());
    }

    #[test]
    fn columns_stand_where_grids_cross() {
        let (mut d, l0, l1, col, _) = setup();
        let gt = d.grid_types().first().map(|t| t.0);
        for (name, a, b) in [("A", (0.0, 0.0), (0.0, 9000.0)), ("B", (6000.0, 0.0), (6000.0, 9000.0)), ("1", (-1000.0, 1000.0), (7000.0, 1000.0)), ("2", (-1000.0, 8000.0), (7000.0, 8000.0))] {
            d.add_grid(vec![crate::curve::Curve::Line { a: Point::new(a.0, a.1), b: Point::new(b.0, b.1) }], gt, Some(name)).unwrap();
        }
        let like = StructuralColumn { top_level: Some(l1), ..StructuralColumn::new(col, l0, Point::new(0.0, 0.0)) };
        d.add_structural_column(StructuralColumn { at: Point::new(0.0, 1000.0), ..like.clone() }).unwrap();
        // four crossings, one taken already
        let made = d.add_columns_at_grids(like.clone(), &[]).unwrap();
        assert_eq!((made.len(), d.structural_columns().len()), (3, 4));
        assert!(d.add_columns_at_grids(like, &[]).is_err(), "nothing left to place");
        d.undo().unwrap();
        assert_eq!(d.structural_columns().len(), 1);
    }
}
