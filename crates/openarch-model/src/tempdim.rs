//! Temporary dimensions (the blue dimensions that appear around a selected
//! element or while one is placed).
//!
//! A selected wall shows its distance to the nearest parallel wall on each side
//! and its length. A selected (or about to be placed) door or window shows the
//! distance from its opening to the nearest wall or opening either side along its
//! host. Where the dimension measures from is a per-user setting, as in the
//! Temporary Dimension Properties (centrelines, faces, core centre, core faces for
//! walls; centrelines or opening edges for doors and windows).
//!
//! Each dimension carries the edit that changes it, so the UI holds no rules: the
//! value typed in is turned into a move of the element (joins and hosted openings
//! follow as for any Move) or a change of wall length.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::curve::Curve;
use crate::edit::{EditOptions, Xform};
use crate::element::*;
use crate::geom::Point;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WallRef {
    #[default]
    Centreline,
    Faces,
    CoreCentre,
    CoreFaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpeningRef {
    #[default]
    Centreline,
    Opening,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TempDimSettings {
    #[serde(default)]
    pub wall_ref: WallRef,
    #[serde(default)]
    pub opening_ref: OpeningRef,
}

/// A reference line a dimension can measure from or to. It names a line on an element
/// rather than a position, so the dimension follows the element when it moves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RefSpec {
    /// A line parallel to a wall's location line, `offset` mm to its left: a face, a layer
    /// boundary, the core or the centre.
    Wall { id: Id, offset: f64 },
    /// A line across the host wall, `at` mm along from the centre of an opening: its centre,
    /// the hole, the reveal or the edge of the frame.
    Opening { id: Id, at: f64 },
    /// The end of a wall (0 start, 1 end).
    WallEnd { id: Id, end: u8 },
    /// A grid line.
    Grid { id: Id },
    /// A level (its height, seen in elevations).
    Level { id: Id },
    /// A line seen in an elevation or section view, named the way the view names it (`ElevRef::key`: "top",
    /// "sill", "end:min", "w:-45.0" ...), so it follows its element.
    Elev { id: Id, key: String },
}

impl RefSpec {
    fn same(&self, o: &RefSpec) -> bool {
        match (self, o) {
            (RefSpec::Wall { id: a, offset: x }, RefSpec::Wall { id: b, offset: y }) => a == b && (x - y).abs() < 0.01,
            (RefSpec::Opening { id: a, at: x }, RefSpec::Opening { id: b, at: y }) => a == b && (x - y).abs() < 0.01,
            (RefSpec::WallEnd { id: a, end: x }, RefSpec::WallEnd { id: b, end: y }) => a == b && x == y,
            (RefSpec::Grid { id: a }, RefSpec::Grid { id: b }) | (RefSpec::Level { id: a }, RefSpec::Level { id: b }) => a == b,
            (RefSpec::Elev { id: a, key: x }, RefSpec::Elev { id: b, key: y }) => a == b && x == y,
            _ => false,
        }
    }

    /// The element the line belongs to.
    pub fn element(&self) -> Id {
        match self {
            RefSpec::Wall { id, .. } | RefSpec::Opening { id, .. } | RefSpec::WallEnd { id, .. } | RefSpec::Grid { id } | RefSpec::Level { id } | RefSpec::Elev { id, .. } => *id,
        }
    }
}

/// Where the person has dragged a dimension's ends: `own` is the end on the selected
/// element, `target` the end it measures to. An end left unset keeps its default.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DimPin {
    #[serde(default)]
    pub own: Option<RefSpec>,
    #[serde(default)]
    pub target: Option<RefSpec>,
}

pub type Pins = BTreeMap<String, DimPin>;

/// One reference line on offer, with where it sits along the dimension axis and a
/// segment of it to highlight.
#[derive(Debug, Clone, Serialize)]
pub struct RefLine {
    pub spec: RefSpec,
    pub pos: f64,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub label: String,
}

/// The lines a selected element's dimensions can use. Positions are measured from `origin`
/// along `axis` (the direction the dimensions run).
#[derive(Debug, Clone, Serialize)]
pub struct DimLines {
    pub origin: [f64; 2],
    pub axis: [f64; 2],
    pub own: Vec<RefLine>,
    pub targets: Vec<RefLine>,
}

/// The change a dimension makes when its value is edited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DimEdit {
    /// Move these elements along `dir` by (new value - current value).
    Move { ids: Vec<Id>, dir: [f64; 2] },
    /// Set a wall's length by moving one end (0 start, 1 end).
    Length { wall: Id, end: u8 },
    /// Set the radius of an arc wall, keeping its ends.
    Radius { wall: Id },
}

#[derive(Debug, Clone, Serialize)]
pub struct TempDim {
    /// "up", "down", "left", "right" or "length"
    pub key: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub value: f64,
    /// Unit vector pointing from the element towards the side the dimension line is drawn on.
    pub normal: [f64; 2],
    /// The element measured to, if any (the witness line grip can be re-pointed).
    pub to: Option<Id>,
    pub edit: Option<DimEdit>,
}

/// How far a probing dimension looks for a neighbour (mm). Keeps selection responsive on a
/// large site, where hundreds of walls lie far beyond anything worth dimensioning to.
pub const TEMP_DIM_REACH_MM: f64 = 60_000.0;

pub(crate) fn unit(a: Point, b: Point) -> Option<(Point, f64)> {
    let l = a.dist(&b);
    if l < 1e-9 {
        None
    } else {
        Some((Point::new((b.x - a.x) / l, (b.y - a.y) / l), l))
    }
}

impl Document {
    /// Positions (mm, left of the location line positive) of the reference lines a
    /// wall offers for the setting: one for centrelines, two for faces.
    pub(crate) fn wall_ref_offsets_pub(&self, w: &Wall, r: WallRef) -> Vec<f64> {
        self.wall_ref_offsets(w, r)
    }

    fn wall_ref_offsets(&self, w: &Wall, r: WallRef) -> Vec<f64> {
        let (l, rt) = self.wall_extents(w);
        match r {
            WallRef::Centreline => vec![(l - rt) / 2.0],
            WallRef::Faces => vec![l, -rt],
            WallRef::CoreCentre | WallRef::CoreFaces => {
                let (cs, ce) = match self.wall_type_for(w) {
                    Some(t) => {
                        let (mut at, mut s, mut e) = (0.0, None, None);
                        for layer in &t.layers {
                            if layer.core {
                                s.get_or_insert(at);
                                e = Some(at + layer.thickness_mm);
                            }
                            at += layer.thickness_mm;
                        }
                        (s.unwrap_or(0.0), e.unwrap_or(at))
                    }
                    None => (0.0, l + rt),
                };
                // layers run from the left (exterior) face
                let (a, b) = (l - cs, l - ce);
                if r == WallRef::CoreCentre {
                    vec![(a + b) / 2.0]
                } else {
                    vec![a, b]
                }
            }
        }
    }

    /// Layer boundaries, the centre and the core centre of a wall, as offsets to the left of its
    /// location line (exterior face first) with a name for each.
    pub(crate) fn wall_boundaries(&self, w: &Wall) -> Vec<(f64, String)> {
        let (l, r) = self.wall_extents(w);
        let mut out: Vec<(f64, String)> = vec![];
        let mut push = |v: f64, name: String| {
            if !out.iter().any(|(o, _)| (o - v).abs() < 0.01) {
                out.push((v, name));
            }
        };
        if let Some(t) = self.wall_type_for(w) {
            let (cs, ce) = t.core_span();
            let n = t.layers.len();
            let mut at = 0.0;
            for k in 0..=n {
                let name = if (at - cs).abs() < 0.01 && n > 0 && t.layers.iter().any(|x| x.core) {
                    "Core face".to_string()
                } else if (at - ce).abs() < 0.01 && t.layers.iter().any(|x| x.core) {
                    "Core face".to_string()
                } else if k == 0 || k == n {
                    "Face".to_string()
                } else {
                    format!("{} | {}", t.layers[k - 1].material, t.layers[k].material)
                };
                push(l - at, name);
                if k < n {
                    at += t.layers[k].thickness_mm;
                }
            }
            if t.layers.iter().any(|x| x.core) {
                push(l - (cs + ce) / 2.0, "Core centre".into());
            }
        } else {
            push(l, "Face".into());
            push(-r, "Face".into());
        }
        push((l - r) / 2.0, "Centreline".into());
        out
    }

    /// Reference lines across an opening's host that the opening itself offers.
    pub(crate) fn opening_offsets(&self, id: Id) -> Vec<(f64, String)> {
        let Some(Element::FamilyInstance(i)) = self.get(id) else { return vec![] };
        let (ow, _, _) = self.instance_opening_size(i);
        let mut out: Vec<(f64, String)> = vec![(0.0, "Centre".into())];
        let mut both = |h: f64, name: &str| {
            for v in [-h, h] {
                if !out.iter().any(|(o, _)| (o - v).abs() < 0.01) {
                    out.push((v, name.to_string()));
                }
            }
        };
        both(ow / 2.0, "Opening edge");
        if let Some(f) = self.door_frame(id) {
            both(f.reveal_w / 2.0, "Reveal");
            both(f.outer_half(), "Frame edge");
        }
        out
    }

    /// Lines parallel to a wall: its own (`own`) or those of the other parallel walls.
    fn wall_ref_lines(&self, id: Id, own: bool) -> Vec<RefLine> {
        let mut out = vec![];
        let Some(Element::Wall(w)) = self.get(id) else { return out };
        let Some((d, _)) = unit(w.start, w.end) else { return out };
        let n = Point::new(-d.y, d.x);
        let across = |p: Point| (p.x - w.start.x) * n.x + (p.y - w.start.y) * n.y;
        let add_wall = |oid: Id, o: &Wall, same: bool, out: &mut Vec<RefLine>| {
            let Some((od, _)) = unit(o.start, o.end) else { return };
            let on = Point::new(-od.y, od.x);
            let base = across(o.start);
            for (v, label) in self.wall_boundaries(o) {
                out.push(RefLine {
                    spec: RefSpec::Wall { id: oid, offset: v },
                    pos: base + if same { v } else { -v },
                    a: [o.start.x + on.x * v, o.start.y + on.y * v],
                    b: [o.end.x + on.x * v, o.end.y + on.y * v],
                    label,
                });
            }
        };
        if own {
            add_wall(id, w, true, &mut out);
            return out;
        }
        for (oid, e) in self.iter() {
            let Element::Wall(o) = e else { continue };
            if o.curve.is_some() {
                continue; // temporary dimensions measure straight walls only so far
            }
            if oid == id || o.base_level != w.base_level {
                continue;
            }
            let Some((od, _)) = unit(o.start, o.end) else { continue };
            if (d.x * od.y - d.y * od.x).abs() > 0.0087 {
                continue;
            }
            add_wall(oid, o, d.x * od.x + d.y * od.y > 0.0, &mut out);
        }
        out
    }

    /// Lines across a host wall: the opening's own (`own`), or those of the walls that cross
    /// the host, the other openings in it and the host's ends.
    fn opening_ref_lines(&self, instance: Option<Id>, host: Id, own: bool) -> Vec<RefLine> {
        let mut out = vec![];
        let Some(Element::Wall(w)) = self.get(host) else { return out };
        let Some((d, len)) = unit(w.start, w.end) else { return out };
        let n = Point::new(-d.y, d.x);
        let (hl, hr) = self.wall_extents(w);
        let at = |t: f64, o: f64| [w.start.x + d.x * t + n.x * o, w.start.y + d.y * t + n.y * o];
        let across_line = |t: f64| (at(t, -hr - 300.0), at(t, hl + 300.0));
        let opening = |id: Id, u: f64, out: &mut Vec<RefLine>| {
            for (v, label) in self.opening_offsets(id) {
                let (a, b) = across_line(u + v);
                out.push(RefLine { spec: RefSpec::Opening { id, at: v }, pos: u + v, a, b, label });
            }
        };
        if own {
            if let Some(Element::FamilyInstance(i)) = instance.and_then(|x| self.get(x)) {
                if let (Some(id), Some(h)) = (instance, i.host) {
                    opening(id, h.u_mm, &mut out);
                }
            }
            return out;
        }
        for (end, t) in [(0u8, 0.0), (1u8, len)] {
            let (a, b) = across_line(t);
            out.push(RefLine { spec: RefSpec::WallEnd { id: host, end }, pos: t, a, b, label: "Wall end".into() });
        }
        for (oid, i) in self.instances() {
            if Some(oid) == instance {
                continue;
            }
            if let Some(h) = i.host.filter(|h| h.wall == host) {
                opening(oid, h.u_mm, &mut out);
            }
        }
        let host_half = hl.max(hr);
        for (oid, e) in self.iter() {
            let Element::Wall(o) = e else { continue };
            if o.curve.is_some() {
                continue; // temporary dimensions measure straight walls only so far
            }
            if oid == host || o.base_level != w.base_level {
                continue;
            }
            let Some((od, olen)) = unit(o.start, o.end) else { continue };
            let den = d.x * od.y - d.y * od.x;
            if den.abs() < 0.17 {
                continue;
            }
            let (rx, ry) = (o.start.x - w.start.x, o.start.y - w.start.y);
            let t_host = (rx * od.y - ry * od.x) / den;
            let t_other = (rx * d.y - ry * d.x) / den;
            let (ol, or_) = self.wall_extents(o);
            let reach_o = (ol.max(or_) + host_half) / den.abs() + 1.0;
            let reach_h = ol.max(or_) / den.abs() + 1.0;
            if t_other < -reach_o || t_other > olen + reach_o || t_host < -reach_h || t_host > len + reach_h {
                continue;
            }
            let on = Point::new(-od.y, od.x);
            for (v, label) in self.wall_boundaries(o) {
                let (px, py) = (o.start.x + on.x * v, o.start.y + on.y * v);
                let t = ((px - w.start.x) * od.y - (py - w.start.y) * od.x) / den;
                let c = at(t, 0.0);
                let reach = host_half + 400.0;
                out.push(RefLine {
                    spec: RefSpec::Wall { id: oid, offset: v },
                    pos: t,
                    a: [c[0] - od.x * reach, c[1] - od.y * reach],
                    b: [c[0] + od.x * reach, c[1] + od.y * reach],
                    label,
                });
            }
        }
        out
    }

    /// Is this a curved wall, or a door or window hosted in one?
    fn on_curved_wall(&self, id: Id) -> bool {
        match self.get(id) {
            Some(Element::Wall(w)) => w.curve.is_some(),
            Some(Element::FamilyInstance(i)) => i.host.is_some_and(|h| matches!(self.get(h.wall), Some(Element::Wall(w)) if w.curve.is_some())),
            _ => false,
        }
    }

    /// Every reference line the dimensions of one selected element can use.
    pub fn dim_lines(&self, id: Id) -> Option<DimLines> {
        if self.on_curved_wall(id) {
            return None;
        }
        match self.get(id)? {
            Element::Wall(w) => {
                let (d, _) = unit(w.start, w.end)?;
                Some(DimLines { origin: [w.start.x, w.start.y], axis: [-d.y, d.x], own: self.wall_ref_lines(id, true), targets: self.wall_ref_lines(id, false) })
            }
            Element::FamilyInstance(i) => {
                let h = i.host?;
                let Some(Element::Wall(w)) = self.get(h.wall) else { return None };
                let (d, _) = unit(w.start, w.end)?;
                Some(DimLines { origin: [w.start.x, w.start.y], axis: [d.x, d.y], own: self.opening_ref_lines(Some(id), h.wall, true), targets: self.opening_ref_lines(Some(id), h.wall, false) })
            }
            _ => None,
        }
    }

    /// Dimensions for one selected element.
    pub fn temp_dimensions(&self, id: Id, s: TempDimSettings, refs: &BTreeMap<String, Id>) -> Vec<TempDim> {
        self.temp_dimensions_pinned(id, s, refs, &Pins::new())
    }

    /// Dimensions for one selected element, with the ends the person has moved to other lines.
    pub fn temp_dimensions_pinned(&self, id: Id, s: TempDimSettings, refs: &BTreeMap<String, Id>, pins: &Pins) -> Vec<TempDim> {
        let dims = self.default_temp_dimensions(id, s, refs);
        if pins.is_empty() {
            return dims;
        }
        self.apply_pins(id, s, dims, pins)
    }

    /// Replace the dimensions whose ends have been dragged to chosen lines.
    fn apply_pins(&self, id: Id, s: TempDimSettings, mut dims: Vec<TempDim>, pins: &Pins) -> Vec<TempDim> {
        if self.on_curved_wall(id) {
            return dims;
        }
        let (host, instance, w) = match self.get(id) {
            Some(Element::Wall(w)) => (id, None, w),
            Some(Element::FamilyInstance(i)) => match (i.host, i.host.and_then(|h| self.get(h.wall))) {
                (Some(h), Some(Element::Wall(w))) => (h.wall, Some(id), w),
                _ => return dims,
            },
            _ => return dims,
        };
        let Some((d, len)) = unit(w.start, w.end) else { return dims };
        let n = Point::new(-d.y, d.x);
        let along = |p: [f64; 2]| (p[0] - w.start.x) * d.x + (p[1] - w.start.y) * d.y;
        let across = |p: [f64; 2]| (p[0] - w.start.x) * n.x + (p[1] - w.start.y) * n.y;
        let (own_lines, tgt_lines) = match instance {
            None => (self.wall_ref_lines(id, true), self.wall_ref_lines(id, false)),
            Some(i) => (self.opening_ref_lines(Some(i), host, true), self.opening_ref_lines(Some(i), host, false)),
        };
        let find = |lines: &[RefLine], spec: &Option<RefSpec>| spec.as_ref().and_then(|sp| lines.iter().find(|l| l.spec.same(sp)).cloned());
        for (key, pin) in pins {
            let slot = ["up", "down", "left", "right"].contains(&key.as_str());
            let right_kind = if instance.is_some() { key == "left" || key == "right" } else { key == "up" || key == "down" };
            if !slot || !right_kind {
                continue;
            }
            let existing = dims.iter().position(|x| &x.key == key);
            let (own_line, tgt_line) = (find(&own_lines, &pin.own), find(&tgt_lines, &pin.target));
            if own_line.is_none() && tgt_line.is_none() {
                continue;
            }
            let (own_pos, tgt_pos);
            let mid;
            match instance {
                None => {
                    let own_default = {
                        let offs = self.wall_ref_offsets(w, s.wall_ref);
                        if key == "up" { offs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) } else { offs.iter().cloned().fold(f64::INFINITY, f64::min) }
                    };
                    own_pos = own_line.as_ref().map(|l| l.pos).or(existing.map(|k| across(dims[k].a))).unwrap_or(own_default);
                    let Some(tp) = tgt_line.as_ref().map(|l| l.pos).or(existing.map(|k| across(dims[k].b))) else { continue };
                    tgt_pos = tp;
                    mid = match &tgt_line {
                        Some(l) => {
                            let (t0, t1) = (along(l.a).min(along(l.b)), along(l.a).max(along(l.b)));
                            let (lo, hi) = (t0.max(0.0), t1.min(len));
                            if hi >= lo { (lo + hi) / 2.0 } else { ((t0 + t1) / 2.0).clamp(0.0, len) }
                        }
                        None => existing.map_or(len / 2.0, |k| along(dims[k].a)),
                    };
                }
                Some(i) => {
                    let Some(Element::FamilyInstance(inst)) = self.get(i) else { continue };
                    let Some(h) = inst.host else { continue };
                    let (ow, _, _) = self.instance_opening_size(inst);
                    let own_default = match (s.opening_ref, key.as_str()) {
                        (OpeningRef::Centreline, _) => h.u_mm,
                        (OpeningRef::Opening, "left") => h.u_mm - ow / 2.0,
                        (OpeningRef::Opening, _) => h.u_mm + ow / 2.0,
                    };
                    own_pos = own_line.as_ref().map(|l| l.pos).or(existing.map(|k| along(dims[k].a))).unwrap_or(own_default);
                    let Some(tp) = tgt_line.as_ref().map(|l| l.pos).or(existing.map(|k| along(dims[k].b))) else { continue };
                    tgt_pos = tp;
                    mid = 0.0;
                }
            }
            let value = (tgt_pos - own_pos).abs();
            if value < 0.5 {
                dims.retain(|x| &x.key != key);
                continue;
            }
            let sign = if tgt_pos > own_pos { 1.0 } else { -1.0 };
            let to = tgt_line.as_ref().map(|l| l.spec.element()).or(existing.and_then(|k| dims[k].to));
            let dim = match instance {
                None => {
                    let at = |t: f64, o: f64| [w.start.x + d.x * t + n.x * o, w.start.y + d.y * t + n.y * o];
                    TempDim { key: key.clone(), a: at(mid, own_pos), b: at(mid, tgt_pos), value, normal: [n.x * sign, n.y * sign], to, edit: Some(DimEdit::Move { ids: vec![id], dir: [-n.x * sign, -n.y * sign] }) }
                }
                Some(i) => {
                    let at = |t: f64| [w.start.x + d.x * t, w.start.y + d.y * t];
                    TempDim { key: key.clone(), a: at(own_pos), b: at(tgt_pos), value, normal: [n.x, n.y], to, edit: Some(DimEdit::Move { ids: vec![i], dir: [-d.x * sign, -d.y * sign] }) }
                }
            };
            match existing {
                Some(k) => dims[k] = dim,
                None => dims.push(dim),
            }
        }
        dims
    }

    fn default_temp_dimensions(&self, id: Id, s: TempDimSettings, refs: &BTreeMap<String, Id>) -> Vec<TempDim> {
        match self.get(id) {
            Some(Element::Wall(w)) if w.curve.is_some() => self.curved_wall_dims(id, w),
            Some(Element::Wall(w)) => self.wall_dims(id, w, s, refs),
            Some(Element::FamilyInstance(i)) => match i.host {
                Some(h) => {
                    let (width, _, _) = self.instance_opening_size(i);
                    self.opening_dims(Some(id), h.wall, h.u_mm, width, s, refs, true)
                }
                None => vec![],
            },
            _ => vec![],
        }
    }

    fn wall_dims(&self, id: Id, w: &Wall, s: TempDimSettings, refs: &BTreeMap<String, Id>) -> Vec<TempDim> {
        let mut out = vec![];
        let Some((d, len)) = unit(w.start, w.end) else { return out };
        let n = Point::new(-d.y, d.x);
        let own = self.wall_ref_offsets(w, s.wall_ref);
        let along = |p: Point| (p.x - w.start.x) * d.x + (p.y - w.start.y) * d.y;
        let across = |p: Point| (p.x - w.start.x) * n.x + (p.y - w.start.y) * n.y;
        // best neighbour per side: (distance, other wall, own ref, other ref, overlap middle)
        let mut best: [Option<(f64, Id, f64, f64, f64)>; 2] = [None, None]; // [up (left of the wall), down]
        for (oid, e) in self.iter() {
            let Element::Wall(o) = e else { continue };
            if o.curve.is_some() {
                continue; // temporary dimensions measure straight walls only so far
            }
            if oid == id || o.base_level != w.base_level {
                continue;
            }
            let Some((od, olen)) = unit(o.start, o.end) else { continue };
            let cross = d.x * od.y - d.y * od.x;
            if cross.abs() > 0.0087 {
                continue;
            }
            let same = d.x * od.x + d.y * od.y > 0.0;
            let (t0, t1) = (along(o.start).min(along(o.end)), along(o.start).max(along(o.end)));
            let (lo, hi) = (t0.max(0.0), t1.min(len));
            let forced = |k: &str| refs.get(k) == Some(&oid);
            if hi - lo < 1.0 && !(forced("up") || forced("down")) {
                continue;
            }
            let _ = olen;
            let off = across(o.start);
            // the other wall's reference lines in this wall's frame
            let theirs: Vec<f64> = self.wall_ref_offsets(o, s.wall_ref).into_iter().map(|v| off + if same { v } else { -v }).collect();
            let mid = if hi >= lo { (lo + hi) / 2.0 } else { (t0 + t1) / 2.0 };
            let own_up = own.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let own_dn = own.iter().cloned().fold(f64::INFINITY, f64::min);
            if let Some(&b) = theirs.iter().filter(|v| **v > own_up + 0.5 && **v - own_up <= TEMP_DIM_REACH_MM).min_by(|a, b| a.total_cmp(b)) {
                if refs.get("up").map_or(true, |f| *f == oid) && best[0].map_or(true, |x| b - own_up < x.0 || refs.get("up") == Some(&oid)) {
                    best[0] = Some((b - own_up, oid, own_up, b, mid));
                }
            }
            if let Some(&b) = theirs.iter().filter(|v| **v < own_dn - 0.5 && own_dn - **v <= TEMP_DIM_REACH_MM).max_by(|a, b| a.total_cmp(b)) {
                if refs.get("down").map_or(true, |f| *f == oid) && best[1].map_or(true, |x| own_dn - b < x.0 || refs.get("down") == Some(&oid)) {
                    best[1] = Some((own_dn - b, oid, own_dn, b, mid));
                }
            }
        }
        let at = |t: f64, o: f64| [w.start.x + d.x * t + n.x * o, w.start.y + d.y * t + n.y * o];
        for (k, (key, sign)) in [("up", 1.0), ("down", -1.0)].into_iter().enumerate() {
            if let Some((dist, oid, a_ref, b_ref, mid)) = best[k] {
                out.push(TempDim {
                    key: key.into(),
                    a: at(mid, a_ref),
                    b: at(mid, b_ref),
                    value: dist,
                    normal: [n.x * sign, n.y * sign],
                    to: Some(oid),
                    // growing the dimension pushes the wall away from the neighbour
                    edit: Some(DimEdit::Move { ids: vec![id], dir: [-n.x * sign, -n.y * sign] }),
                });
            }
        }
        out.push(TempDim {
            key: "length".into(),
            a: [w.start.x, w.start.y],
            b: [w.end.x, w.end.y],
            value: len,
            normal: [-n.x, -n.y],
            to: None,
            edit: Some(DimEdit::Length { wall: id, end: 1 }),
        });
        out
    }

    /// Dimensions of an opening at `u` along `host`, `width` wide. `instance` is
    /// the opening itself when it exists (so it is left out of the obstacles).
    pub fn opening_dims(&self, instance: Option<Id>, host: Id, u: f64, width: f64, s: TempDimSettings, refs: &BTreeMap<String, Id>, editable: bool) -> Vec<TempDim> {
        let mut out = vec![];
        let Some(Element::Wall(w)) = self.get(host) else { return out };
        if w.curve.is_some() {
            return self.curved_opening_dims(instance, host, u, width, s, refs, editable);
        }
        let Some((d, len)) = unit(w.start, w.end) else { return out };
        let n = Point::new(-d.y, d.x);
        let (hl, hr) = self.wall_extents(w);
        let host_half = hl.max(hr);
        // obstacles along the host: (position, owner)
        let mut ob: Vec<(f64, Option<Id>)> = vec![(0.0, None), (len, None)];
        for (oid, e) in self.iter() {
            let Element::Wall(o) = e else { continue };
            if o.curve.is_some() {
                continue; // temporary dimensions measure straight walls only so far
            }
            if oid == host || o.base_level != w.base_level {
                continue;
            }
            let Some((od, olen)) = unit(o.start, o.end) else { continue };
            let sin = d.x * od.y - d.y * od.x;
            if sin.abs() < 0.17 {
                continue;
            }
            // where the host's location line meets the other wall's location line
            let den = d.x * od.y - d.y * od.x;
            let (rx, ry) = (o.start.x - w.start.x, o.start.y - w.start.y);
            let t_host = (rx * od.y - ry * od.x) / den;
            let t_other = (rx * d.y - ry * d.x) / den;
            let (ol, or_) = self.wall_extents(o);
            let reach_o = (ol.max(or_) + host_half) / sin.abs() + 1.0;
            let reach_h = (ol.max(or_)) / sin.abs() + 1.0;
            if t_other < -reach_o || t_other > olen + reach_o || t_host < -reach_h || t_host > len + reach_h {
                continue;
            }
            // each reference line of the other wall, met by the host line
            for off in self.wall_ref_offsets(o, s.wall_ref) {
                // the other wall's line shifted by `off` along its left normal
                let (nx, ny) = (-od.y, od.x);
                let (px, py) = (o.start.x + nx * off, o.start.y + ny * off);
                let t = ((px - w.start.x) * od.y - (py - w.start.y) * od.x) / den;
                ob.push((t, Some(oid)));
            }
        }
        for (oid, i) in self.instances() {
            if Some(oid) == instance {
                continue;
            }
            let Some(h) = i.host else { continue };
            if h.wall != host {
                continue;
            }
            let (ow, _, _) = self.instance_opening_size(i);
            match s.opening_ref {
                OpeningRef::Centreline => ob.push((h.u_mm, Some(oid))),
                OpeningRef::Opening => {
                    ob.push((h.u_mm - ow / 2.0, Some(oid)));
                    ob.push((h.u_mm + ow / 2.0, Some(oid)));
                }
            }
        }
        let (own_l, own_r) = match s.opening_ref {
            OpeningRef::Centreline => (u, u),
            OpeningRef::Opening => (u - width / 2.0, u + width / 2.0),
        };
        let pick = |key: &str, left: bool| -> Option<(f64, Option<Id>)> {
            let forced = refs.get(key).copied();
            ob.iter()
                .filter(|(p, o)| {
                    (if left { *p <= own_l - 0.5 } else { *p >= own_r + 0.5 }) && forced.map_or(true, |f| *o == Some(f))
                })
                .cloned()
                .min_by(|a, b| if left { b.0.total_cmp(&a.0) } else { a.0.total_cmp(&b.0) })
        };
        let at = |t: f64| [w.start.x + d.x * t, w.start.y + d.y * t];
        // the line is drawn on the left of the wall (the normal), clear of the wall body
        for (key, left) in [("left", true), ("right", false)] {
            if let Some((p, owner)) = pick(key, left) {
                let own = if left { own_l } else { own_r };
                let dir = if left { 1.0 } else { -1.0 };
                out.push(TempDim {
                    key: key.into(),
                    a: at(own),
                    b: at(p),
                    value: (own - p).abs(),
                    normal: [n.x, n.y],
                    to: owner,
                    edit: match (editable, instance) {
                        (true, Some(i)) => Some(DimEdit::Move { ids: vec![i], dir: [d.x * dir, d.y * dir] }),
                        _ => None,
                    },
                });
            }
        }
        out
    }

    /// Dimensions of a selected curved wall: the radius of an arc (editable, the ends stay)
    /// and the length along the curve.
    fn curved_wall_dims(&self, id: Id, w: &Wall) -> Vec<TempDim> {
        let mut out = vec![];
        let Some(curve) = &w.curve else { return out };
        let path = w.path();
        let (mid, _) = path.at(path.length() / 2.0);
        if let Curve::Arc { centre, start, .. } = curve {
            let r = centre.dist(start);
            let (dx, dy) = (mid.x - centre.x, mid.y - centre.y);
            let l = dx.hypot(dy).max(1e-9);
            let b = Point::new(centre.x + dx / l * r, centre.y + dy / l * r);
            out.push(TempDim { key: "radius".into(), a: [centre.x, centre.y], b: [b.x, b.y], value: r, normal: [-dy / l, dx / l], to: None, edit: Some(DimEdit::Radius { wall: id }) });
        }
        let (cx, cy) = ((w.start.x + w.end.x) / 2.0, (w.start.y + w.end.y) / 2.0);
        let (ox, oy) = (mid.x - cx, mid.y - cy);
        let ol = ox.hypot(oy);
        let normal = if ol > 1e-6 { [ox / ol, oy / ol] } else {
            let (d, _) = unit(w.start, w.end).unwrap_or((Point::new(1.0, 0.0), 0.0));
            [-d.y, d.x]
        };
        out.push(TempDim { key: "length".into(), a: [w.start.x, w.start.y], b: [w.end.x, w.end.y], value: path.length(), normal, to: None, edit: None });
        out
    }

    /// Distances of an opening in a curved wall to the ends of the wall and to the
    /// neighbouring openings, measured along the curve.
    #[allow(clippy::too_many_arguments)]
    fn curved_opening_dims(&self, instance: Option<Id>, host: Id, u: f64, width: f64, s: TempDimSettings, refs: &BTreeMap<String, Id>, editable: bool) -> Vec<TempDim> {
        let mut out = vec![];
        let Some(Element::Wall(w)) = self.get(host) else { return out };
        let path = w.path();
        let mut ob: Vec<(f64, Option<Id>)> = vec![(0.0, None), (path.length(), None)];
        for (oid, i) in self.instances() {
            if Some(oid) == instance {
                continue;
            }
            let Some(h) = i.host else { continue };
            if h.wall != host {
                continue;
            }
            let (ow, _, _) = self.instance_opening_size(i);
            match s.opening_ref {
                OpeningRef::Centreline => ob.push((h.u_mm, Some(oid))),
                OpeningRef::Opening => {
                    ob.push((h.u_mm - ow / 2.0, Some(oid)));
                    ob.push((h.u_mm + ow / 2.0, Some(oid)));
                }
            }
        }
        let (own_l, own_r) = match s.opening_ref {
            OpeningRef::Centreline => (u, u),
            OpeningRef::Opening => (u - width / 2.0, u + width / 2.0),
        };
        let (_, dir) = path.at(u);
        let n = [-dir.y, dir.x];
        for (key, left) in [("cleft", true), ("cright", false)] {
            let forced = refs.get(if left { "left" } else { "right" }).copied();
            let pick = ob
                .iter()
                .filter(|(p, o)| (if left { *p <= own_l - 0.5 } else { *p >= own_r + 0.5 }) && forced.map_or(true, |f| *o == Some(f)))
                .cloned()
                .min_by(|a, b| if left { b.0.total_cmp(&a.0) } else { a.0.total_cmp(&b.0) });
            let Some((p, owner)) = pick else { continue };
            let own = if left { own_l } else { own_r };
            let sign = if left { 1.0 } else { -1.0 };
            let (pa, pb) = (path.at(own).0, path.at(p).0);
            out.push(TempDim {
                key: key.into(),
                a: [pa.x, pa.y],
                b: [pb.x, pb.y],
                value: (own - p).abs(),
                normal: n,
                to: owner,
                // the instance moves along the curve, so its position along the wall is exactly what changes
                edit: match (editable, instance) {
                    (true, Some(i)) => Some(DimEdit::Move { ids: vec![i], dir: [dir.x * sign, dir.y * sign] }),
                    _ => None,
                },
            });
        }
        out
    }

    /// Where an opening of this type would sit if placed at `point` on `wall`
    /// (the position along the wall, clamped as placing does) and its width.
    pub fn placement_at(&self, type_id: Id, wall: Id, point: Point) -> Option<(f64, f64)> {
        let ft = self.family_type(type_id)?.clone();
        let Some(Element::Wall(w)) = self.get(wall) else { return None };
        let path = w.path();
        let len = path.length();
        let mut probe = crate::family::FamilyInstance::new(type_id, w.base_level);
        if let Some(s) = self.type_param(&ft, "Default Sill Height") {
            probe.values.insert("Sill Height".into(), s);
        }
        let width = self.instance_opening_size(&probe).0;
        if width > len {
            return None;
        }
        let along = path.project(point).0;
        Some((along.clamp(width / 2.0, len - width / 2.0), width))
    }

    /// Apply an edited dimension: the new measured value for the dimension whose
    /// current value is `current`.
    pub fn apply_temp_dimension(&mut self, edit: &DimEdit, current: f64, value: f64) -> Result<Vec<Id>, String> {
        if !value.is_finite() || value <= 0.0 {
            return Err("a dimension must be greater than zero".into());
        }
        let delta = ((value - current) * 1000.0).round() / 1000.0;
        if delta.abs() < 1e-6 {
            return Ok(vec![]);
        }
        match edit {
            DimEdit::Move { ids, dir } => {
                let r = self.transform_elements(ids, Xform::Move { dx: dir[0] * delta, dy: dir[1] * delta }, EditOptions::default());
                if r.is_ok() {
                    self.rename_last("Change dimension");
                }
                r
            }
            DimEdit::Radius { wall } => {
                let Some(Element::Wall(w)) = self.get(*wall).cloned() else { return Err("not a wall".into()) };
                self.refuse_pinned(&[*wall], "changed")?;
                let Some(Curve::Arc { start, end, centre, ccw }) = w.curve.clone() else { return Err("only an arc wall has a radius".into()) };
                let chord = start.dist(&end);
                if value < chord / 2.0 {
                    return Err(format!("the radius cannot be less than half the chord ({:.0} mm)", chord / 2.0));
                }
                let (cx, cy) = ((end.x - start.x) / chord, (end.y - start.y) / chord);
                let left = (end.x - start.x) * (centre.y - start.y) - (end.y - start.y) * (centre.x - start.x);
                let side = if left.abs() > 1e-6 { left.signum() } else if ccw { 1.0 } else { -1.0 };
                let h = (value * value - chord * chord / 4.0).max(0.0).sqrt();
                let m = Point::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0);
                let nc = Point::new(m.x - cy * h * side, m.y + cx * h * side);
                let mut nw = w.clone();
                nw.curve = Some(Curve::Arc { start, end, centre: nc, ccw });
                self.transact("Change dimension", |tx| {
                    tx.update(*wall, Element::Wall(nw))?;
                    let doc = tx.doc_ref();
                    for (id, i) in doc.instances() {
                        if i.host.is_some_and(|h| h.wall == *wall) {
                            doc.fits(i, Some(id)).map_err(|e| format!("a door or window would not fit: {e}"))?;
                        }
                    }
                    Ok(())
                })?;
                Ok(vec![*wall])
            }
            DimEdit::Length { wall, end } => {
                let Some(Element::Wall(w)) = self.get(*wall).cloned() else { return Err("not a wall".into()) };
                self.refuse_pinned(&[*wall], "changed")?;
                let (fixed, free) = if *end == 1 { (w.start, w.end) } else { (w.end, w.start) };
                let (d, _) = unit(fixed, free).ok_or("the wall has no length")?;
                let moved = Point::new(fixed.x + d.x * value, fixed.y + d.y * value);
                let (s, e) = if *end == 1 { (fixed, moved) } else { (moved, fixed) };
                let mut moves = BTreeMap::new();
                moves.insert(*wall, (s, e));
                let plan = self.plan_wall_ends(&moves, true)?;
                let list: Vec<(Id, Wall)> = plan.into_iter().collect();
                let ids: Vec<Id> = list.iter().map(|(i, _)| *i).collect();
                self.apply_wall_ends(&list, "Change dimension")?;
                let _: BTreeSet<Id> = ids.iter().copied().collect();
                Ok(vec![*wall])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn wall(d: &Document, id: Id) -> Wall {
        match d.get(id) {
            Some(Element::Wall(w)) => w.clone(),
            _ => panic!("not a wall"),
        }
    }

    fn inst(d: &Document, id: Id) -> crate::family::FamilyInstance {
        match d.get(id) {
            Some(Element::FamilyInstance(i)) => i.clone(),
            _ => panic!("not an instance"),
        }
    }

    fn room() -> (Document, Vec<Id>) {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        let mut ids = vec![];
        for (a, b) in [((0.0, 0.0), (6000.0, 0.0)), ((6000.0, 0.0), (6000.0, 4000.0)), ((6000.0, 4000.0), (0.0, 4000.0)), ((0.0, 4000.0), (0.0, 0.0))] {
            ids.push(d.add_wall(wt, lvl, p(a.0, a.1), p(b.0, b.1), 2700.0).unwrap());
        }
        (d, ids)
    }

    fn door_type(d: &Document) -> Id {
        d.family_types().iter().find(|(_, t)| t.name.contains("820 x 2040")).unwrap().0
    }

    #[test]
    fn wall_shows_distance_to_the_opposite_wall_and_its_length() {
        let (d, w) = room();
        let dims = d.temp_dimensions(w[0], TempDimSettings::default(), &BTreeMap::new());
        let up = dims.iter().find(|x| x.key == "up").expect("the north wall is 4000 away");
        assert!((up.value - 4000.0).abs() < 1e-6, "{}", up.value);
        assert!(dims.iter().all(|x| x.key != "down"));
        let len = dims.iter().find(|x| x.key == "length").unwrap();
        assert!((len.value - 6000.0).abs() < 1e-6);
    }

    #[test]
    fn a_wall_beyond_the_search_reach_is_not_dimensioned_to() {
        let (mut d, w) = room();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        d.add_wall(wt, lvl, p(0.0, 200_000.0), p(6000.0, 200_000.0), 2700.0).unwrap();
        // the far wall is out of reach, the room's own north wall is still found
        let dims = d.temp_dimensions(w[0], TempDimSettings::default(), &BTreeMap::new());
        assert!((dims.iter().find(|x| x.key == "up").unwrap().value - 4000.0).abs() < 1e-6);
        let lone = d.temp_dimensions(w[2], TempDimSettings::default(), &BTreeMap::new());
        // the north wall runs west, so its right-hand side (down) faces the far wall
        assert!(lone.iter().all(|x| x.key != "down"), "nothing within reach beyond the north wall");
    }

    #[test]
    fn faces_setting_measures_between_facing_faces() {
        let (d, w) = room();
        let s = TempDimSettings { wall_ref: WallRef::Faces, ..Default::default() };
        let dims = d.temp_dimensions(w[0], s, &BTreeMap::new());
        let up = dims.iter().find(|x| x.key == "up").unwrap();
        // the bottom wall runs west to east so its body reaches `l` above its line; the top
        // wall runs east to west so its body reaches `r` (of its own left and right) below
        let (l, _) = d.wall_extents(&wall(&d, w[0]));
        let (_, r) = d.wall_extents(&wall(&d, w[2]));
        assert!((up.value - (4000.0 - l - r)).abs() < 1e-6, "{} vs {}", up.value, 4000.0 - l - r);
    }

    #[test]
    fn editing_a_wall_dimension_moves_the_wall_and_the_joined_ends_follow() {
        let (mut d, w) = room();
        let dims = d.temp_dimensions(w[0], TempDimSettings::default(), &BTreeMap::new());
        let up = dims.iter().find(|x| x.key == "up").unwrap().clone();
        d.apply_temp_dimension(&up.edit.unwrap(), up.value, 3000.0).unwrap();
        let bottom = wall(&d, w[0]);
        assert!((bottom.start.y - 1000.0).abs() < 1e-6 && (bottom.end.y - 1000.0).abs() < 1e-6);
        assert!((wall(&d, w[1]).start.y - 1000.0).abs() < 1e-6, "the side wall stretched");
        d.undo();
        assert!(wall(&d, w[0]).start.y.abs() < 1e-6);
    }

    #[test]
    fn editing_length_moves_the_end_and_joined_walls() {
        let (mut d, w) = room();
        let dims = d.temp_dimensions(w[0], TempDimSettings::default(), &BTreeMap::new());
        let len = dims.iter().find(|x| x.key == "length").unwrap().clone();
        d.apply_temp_dimension(&len.edit.unwrap(), len.value, 5000.0).unwrap();
        assert!((wall(&d, w[0]).end.x - 5000.0).abs() < 1e-6);
        assert!((wall(&d, w[1]).start.x - 5000.0).abs() < 1e-6);
    }

    #[test]
    fn a_door_measures_to_the_inside_faces_of_the_corner_walls_and_edits_by_sliding() {
        let (mut d, w) = room();
        let door = door_type(&d);
        let id = d.place_on_wall(door, w[0], p(2000.0, 0.0)).unwrap();
        let width = d.instance_opening_size(&inst(&d, id)).0;
        let centre = d.temp_dimensions(id, TempDimSettings::default(), &BTreeMap::new());
        let half = d.wall_extents(&wall(&d, w[3])).0.max(d.wall_extents(&wall(&d, w[3])).1);
        // centrelines to the west wall's centreline
        let left = centre.iter().find(|x| x.key == "left").unwrap().clone();
        assert!((left.value - 2000.0).abs() < 1e-6, "{}", left.value);
        // opening edge to the west wall's inside face
        let faces = TempDimSettings { wall_ref: WallRef::Faces, opening_ref: OpeningRef::Opening };
        let dims = d.temp_dimensions(id, faces, &BTreeMap::new());
        let left_f = dims.iter().find(|x| x.key == "left").unwrap();
        // the face is `half` closer than the centreline would be (the walls are symmetric here)
        assert!((left_f.value - (2000.0 - width / 2.0 - half)).abs() < 1e-6, "{} vs {}", left_f.value, 2000.0 - width / 2.0 - half);
        // growing the left dimension by 500 slides the door 500 along the wall
        d.apply_temp_dimension(left.edit.as_ref().unwrap(), left.value, left.value + 500.0).unwrap();
        assert!((inst(&d, id).host.unwrap().u_mm - 2500.0).abs() < 1e-6);
        d.undo();
        assert!((inst(&d, id).host.unwrap().u_mm - 2000.0).abs() < 1e-6);
    }

    #[test]
    fn a_dimension_to_a_neighbouring_opening_uses_its_edge_and_can_be_re_pointed() {
        let (mut d, w) = room();
        let door = door_type(&d);
        let a = d.place_on_wall(door, w[0], p(1500.0, 0.0)).unwrap();
        let b = d.place_on_wall(door, w[0], p(3500.0, 0.0)).unwrap();
        let s = TempDimSettings { opening_ref: OpeningRef::Opening, ..Default::default() };
        let dims = d.temp_dimensions(b, s, &BTreeMap::new());
        let left = dims.iter().find(|x| x.key == "left").unwrap();
        assert_eq!(left.to, Some(a));
        let wa = d.instance_opening_size(&inst(&d, a)).0;
        let wb = d.instance_opening_size(&inst(&d, b)).0;
        assert!((left.value - (2000.0 - wa / 2.0 - wb / 2.0)).abs() < 1e-6, "{}", left.value);
        let mut refs = BTreeMap::new();
        refs.insert("left".to_string(), w[3]);
        let forced = d.temp_dimensions(b, s, &refs);
        assert_eq!(forced.iter().find(|x| x.key == "left").unwrap().to, Some(w[3]));
    }

    #[test]
    fn a_pinned_wall_refuses_its_dimensions() {
        let (mut d, w) = room();
        d.set_pinned(&[w[0]], true).unwrap();
        let dims = d.temp_dimensions(w[0], TempDimSettings::default(), &BTreeMap::new());
        let up = dims.iter().find(|x| x.key == "up").unwrap().clone();
        assert!(d.apply_temp_dimension(&up.edit.unwrap(), up.value, 3000.0).is_err());
        let len = dims.iter().find(|x| x.key == "length").unwrap().clone();
        assert!(d.apply_temp_dimension(&len.edit.unwrap(), len.value, 5000.0).is_err());
    }

    fn pin(own: Option<RefSpec>, target: Option<RefSpec>) -> Pins {
        let mut m = Pins::new();
        m.insert("up".into(), DimPin { own, target });
        m
    }

    #[test]
    fn a_wall_dimension_can_be_pointed_at_any_layer_of_the_opposite_wall() {
        let (mut d, w) = room();
        let lines = d.dim_lines(w[0]).unwrap();
        // the wall offers each layer boundary, the core and the centre
        assert!(lines.own.len() >= 6, "{}", lines.own.len());
        assert!(lines.own.iter().any(|l| l.label == "Core face") && lines.own.iter().any(|l| l.label.contains('|')));
        // the facing wall runs the other way, so its layers are reversed in our frame
        let top = lines.targets.iter().filter(|l| l.spec.element() == w[2]).cloned().collect::<Vec<_>>();
        assert!(top.len() >= 6);
        let near = top.iter().min_by(|a, b| a.pos.total_cmp(&b.pos)).unwrap().clone();
        let own_face = lines.own.iter().max_by(|a, b| a.pos.total_cmp(&b.pos)).unwrap().clone();
        let dims = d.temp_dimensions_pinned(w[0], TempDimSettings::default(), &BTreeMap::new(), &pin(Some(own_face.spec.clone()), Some(near.spec.clone())));
        let up = dims.iter().find(|x| x.key == "up").unwrap();
        assert!((up.value - (near.pos - own_face.pos)).abs() < 1e-6, "{} vs {}", up.value, near.pos - own_face.pos);
        assert!(up.value < 4000.0 - 100.0);
        // editing it sets that distance, and the pinned dimension then reads the new value
        d.apply_temp_dimension(up.edit.as_ref().unwrap(), up.value, 3000.0).unwrap();
        let pins = pin(Some(own_face.spec), Some(near.spec));
        let again = d.temp_dimensions_pinned(w[0], TempDimSettings::default(), &BTreeMap::new(), &pins);
        assert!((again.iter().find(|x| x.key == "up").unwrap().value - 3000.0).abs() < 1e-6);
        // a pin on a line that no longer exists is ignored
        let gone = RefSpec::Wall { id: 9999, offset: 0.0 };
        let dims = d.temp_dimensions_pinned(w[0], TempDimSettings::default(), &BTreeMap::new(), &pin(None, Some(gone)));
        assert!(dims.iter().any(|x| x.key == "up"));
    }

    #[test]
    fn a_door_can_be_dimensioned_from_its_frame_edge_to_a_layer_of_a_crossing_wall() {
        let (mut d, w) = room();
        let t = door_type(&d);
        let door = d.place_on_wall(t, w[0], p(2000.0, 0.0)).unwrap();
        let lines = d.dim_lines(door).unwrap();
        let labels: Vec<&str> = lines.own.iter().map(|l| l.label.as_str()).collect();
        assert!(labels.contains(&"Centre") && labels.contains(&"Opening edge") && labels.contains(&"Reveal"), "{labels:?}");
        // a metal frame stands proud of the hole with its architrave, so it has an edge of its own
        let lvl = d.levels()[0].0;
        let stud = d.wall_types().iter().find(|(_, t)| t.name == "Internal Stud 90").unwrap().0;
        let sw = d.add_wall(stud, lvl, p(0.0, 9000.0), p(6000.0, 9000.0), 2700.0).unwrap();
        let metal = d.place_on_wall(t, sw, p(2000.0, 9000.0)).unwrap();
        assert!(d.dim_lines(metal).unwrap().own.iter().any(|l| l.label == "Frame edge"));
        // the left edge of the hole and the inner face of the west wall
        let own = lines.own.iter().filter(|l| l.label == "Opening edge").min_by(|a, b| a.pos.total_cmp(&b.pos)).unwrap().clone();
        let tgt = lines.targets.iter().filter(|l| l.spec.element() == w[3]).max_by(|a, b| a.pos.total_cmp(&b.pos)).unwrap().clone();
        let mut m = Pins::new();
        m.insert("left".into(), DimPin { own: Some(own.spec.clone()), target: Some(tgt.spec.clone()) });
        let dims = d.temp_dimensions_pinned(door, TempDimSettings::default(), &BTreeMap::new(), &m);
        let left = dims.iter().find(|x| x.key == "left").unwrap();
        assert!((left.value - (own.pos - tgt.pos)).abs() < 1e-6, "{} vs {}", left.value, own.pos - tgt.pos);
        // typing a value moves the door so the frame edge is that far from the wall layer
        d.apply_temp_dimension(left.edit.as_ref().unwrap(), left.value, 500.0).unwrap();
        let after = d.temp_dimensions_pinned(door, TempDimSettings::default(), &BTreeMap::new(), &m);
        assert!((after.iter().find(|x| x.key == "left").unwrap().value - 500.0).abs() < 1e-6);
        // the default dimension to the nearest wall is still offered alongside
        assert!(after.iter().any(|x| x.key == "right"));
    }

    #[test]
    fn a_curved_wall_shows_its_radius_and_length_and_a_door_in_it_measures_along_the_curve() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types().iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0);
        let mut cw = Wall::new(wt, lvl, p(5000.0, 0.0), p(0.0, 5000.0), 2700.0);
        cw.curve = Some(Curve::Arc { start: p(5000.0, 0.0), end: p(0.0, 5000.0), centre: p(0.0, 0.0), ccw: true });
        let w = d.add_wall_with(cw).unwrap();
        let dims = d.temp_dimensions(w, TempDimSettings::default(), &BTreeMap::new());
        let radius = dims.iter().find(|x| x.key == "radius").unwrap();
        assert!((radius.value - 5000.0).abs() < 1e-6);
        let len = dims.iter().find(|x| x.key == "length").unwrap();
        assert!((len.value - 5000.0 * std::f64::consts::FRAC_PI_2).abs() < 2.0);
        // a bigger radius keeps the ends and flattens the arc
        d.apply_temp_dimension(radius.edit.as_ref().unwrap(), 5000.0, 8000.0).unwrap();
        let nw = wall(&d, w);
        assert!(nw.start.dist(&p(5000.0, 0.0)) < 1e-9 && nw.end.dist(&p(0.0, 5000.0)) < 1e-9);
        let Some(Curve::Arc { centre, start, .. }) = nw.curve.clone() else { panic!("still an arc") };
        assert!((centre.dist(&start) - 8000.0).abs() < 1e-6);
        assert!(centre.x < 0.0 && centre.y < 0.0, "the centre stays on the same side of the chord: {centre:?}");
        assert!(d.apply_temp_dimension(radius.edit.as_ref().unwrap(), 8000.0, 3000.0).is_err(), "less than half the chord");
        d.undo();
        // a door half way round measures along the curve to both ends
        let t = door_type(&d);
        let c = 5000.0 * std::f64::consts::FRAC_1_SQRT_2;
        let door = d.place_on_wall(t, w, p(c, c)).unwrap();
        let dims = d.temp_dimensions(door, TempDimSettings::default(), &BTreeMap::new());
        let (l, r) = (dims.iter().find(|x| x.key == "cleft").unwrap(), dims.iter().find(|x| x.key == "cright").unwrap());
        assert!((l.value - r.value).abs() < 2.0, "{} {}", l.value, r.value);
        d.apply_temp_dimension(l.edit.as_ref().unwrap(), l.value, 1000.0).unwrap();
        let dims = d.temp_dimensions(door, TempDimSettings::default(), &BTreeMap::new());
        assert!((dims.iter().find(|x| x.key == "cleft").unwrap().value - 1000.0).abs() < 1.0);
    }
}
