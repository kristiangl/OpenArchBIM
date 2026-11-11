//! CAD drawings (DXF, ASCII or binary, and DWG) read into flat shapes a view can draw.
//!
//! The file is decoded by the `acadrust` crate (MPL-2.0, used unmodified as a separate library). Nothing outside this
//! crate sees its types: what comes out is a [`Drawing`], model space only, in the drawing's own units, with every
//! block reference already expanded into the lines, arcs, text and fills it places. Layers keep their colour, line
//! weight and dash pattern; a shape keeps its layer and, when it has one of its own, its colour.
//!
//! Lines and polylines keep their heights (a contour's elevation, a survey point's level), so a drawing can also feed
//! site tools.

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::io::Cursor;

use acadrust::entities::mtext_format::parse_mtext;
use acadrust::entities::{BoundaryEdge, EntityType, Hatch, Insert, TextHorizontalAlignment as HAlign, TextVerticalAlignment as VAlign};
use acadrust::types::Color;
use acadrust::{DwgReadOptions, DwgReader, DxfReader, DxfReaderConfiguration};
use serde::{Deserialize, Serialize};

/// The largest file read at all.
pub const MAX_BYTES: usize = 128 << 20;
/// The most points a drawing may come to once its blocks are expanded; the rest is left out and the drawing says so.
pub const MAX_POINTS: usize = 6_000_000;
/// How deep blocks may sit inside blocks.
const MAX_DEPTH: usize = 16;
/// Chords to a full circle where a curve has to become straight pieces.
const CHORDS: f64 = 96.0;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Drawing {
    /// "DXF" or "DWG".
    pub format: String,
    /// The version the file says it is (AC1032 and so on).
    #[serde(default)]
    pub version: String,
    /// The unit the file says it is drawn in ("mm", "m", ...), empty when it doesn't say.
    pub unit: String,
    /// Millimetres to one of its units; 0 when it doesn't say.
    pub unit_mm: f64,
    pub layers: Vec<Layer>,
    pub shapes: Vec<Shape>,
    /// The box round everything drawn: min x, min y, max x, max y.
    pub extents: Option<[f64; 4]>,
    /// The same with strays left out (a point at the origin of a survey kilometres from its site).
    pub bulk: Option<[f64; 4]>,
    /// What wasn't read, counted by kind.
    #[serde(default)]
    pub left_out: BTreeMap<String, usize>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub color: [u8; 3],
    /// Shown in the file (neither off nor frozen).
    pub on: bool,
    /// Line weight in mm on paper, when the layer sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight_mm: Option<f64>,
    /// Dash, gap, dash, ... in the drawing's units; empty for a continuous line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dash: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    /// Index into the drawing's layers.
    pub layer: u32,
    /// Its own colour; none takes the layer's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    /// A run of straight pieces. `z` is empty (flat at 0), one height for the whole run, or one for each point.
    Poly {
        pts: Vec<[f64; 2]>,
        closed: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        z: Vec<f64>,
    },
    Circle {
        c: [f64; 2],
        r: f64,
        #[serde(default)]
        z: f64,
    },
    /// Anticlockwise from `a0` to `a1` (radians).
    Arc {
        c: [f64; 2],
        r: f64,
        a0: f64,
        a1: f64,
        #[serde(default)]
        z: f64,
    },
    /// `ha`: 0 left, 1 centre, 2 right. `va`: 0 baseline, 1 bottom, 2 middle, 3 top. Lines are split by '\n'.
    Text { at: [f64; 2], h: f64, angle: f64, text: String, ha: u8, va: u8 },
    /// A filled area (even-odd over its loops); `pale` for a patterned hatch, whose pattern isn't kept.
    Fill { loops: Vec<Vec<[f64; 2]>>, pale: bool },
    Point {
        at: [f64; 2],
        #[serde(default)]
        z: f64,
    },
}

/// Whether `bytes` begin as a DWG does: "AC" and four digits (AC1015 is 2000, AC1032 is 2018).
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() > 6 && bytes.starts_with(b"AC") && bytes[2..6].iter().all(u8::is_ascii_digit)}

/// Millimetres to a drawing unit, by the file's insertion units code, with the unit's name.
fn unit_of(code: i16) -> (&'static str, f64) {
    match code {
        1 => ("inches", 25.4),
        2 => ("feet", 304.8),
        3 => ("miles", 1_609_344.0),
        4 => ("mm", 1.0),
        5 => ("cm", 10.0),
        6 => ("m", 1000.0),
        7 => ("km", 1_000_000.0),
        10 => ("yards", 914.4),
        14 => ("dm", 100.0),
        _ => ("", 0.0),
    }
}

/// Millimetres to a unit by name, for a unit the person picks: "mm", "cm", "m", "inches", "feet".
pub fn unit_mm(name: &str) -> Option<f64> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "mm" | "millimetres" | "millimeters" => 1.0,
        "cm" | "centimetres" | "centimeters" => 10.0,
        "dm" => 100.0,
        "m" | "metres" | "meters" => 1000.0,
        "km" => 1_000_000.0,
        "in" | "inch" | "inches" => 25.4,
        "ft" | "foot" | "feet" => 304.8,
        "yd" | "yard" | "yards" => 914.4,
        _ => return None,
    })
}

/// Read a DXF or DWG file's bytes.
pub fn read(bytes: &[u8]) -> Result<Drawing, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("The file is {:.0} MB; the largest drawing read is {} MB.", bytes.len() as f64 / 1048576.0, MAX_BYTES >> 20));
    }
    let dwg = is_dwg(bytes);
    let owned = bytes.to_vec();
    // failsafe: a record the decoder can't read is left out and counted, rather than failing the whole file
    let outcome = std::panic::catch_unwind(move || {
        if dwg {
            DwgReader::from_stream_with_options(Cursor::new(owned), DwgReadOptions::failsafe()).read_with_stats()
        } else {
            DxfReader::from_reader(Cursor::new(owned))
                .and_then(|r| r.with_configuration(DxfReaderConfiguration { failsafe: true, ..Default::default() }).read_with_stats())
        }
    })
    .map_err(|_| "The drawing couldn't be read (the reader failed on this file).".to_string())?
    .map_err(|e| format!("The drawing couldn't be read: {e}"))?;
    let doc = &outcome.document;
    let stats = &outcome.stats;
    if dwg && !stats.record_stream_read {
        let why = stats.diagnostics.first().map(|d| d.message.trim_start_matches("Failsafe: ").to_string());
        return Err(format!("The drawing couldn't be read: {}.", why.unwrap_or_else(|| "its objects couldn't be read".into()).trim_end_matches('.')));
    }

    let (unit, unit_mm) = unit_of(doc.header.insertion_units);
    let ltscale = if doc.header.linetype_scale > 0.0 { doc.header.linetype_scale } else { 1.0 };
    let mut cx = Cx { doc, layers: vec![], by_name: BTreeMap::new(), shapes: vec![], left: BTreeMap::new(), points: 0, full: false, ltscale };
    for l in doc.layers.iter() {
        cx.add_layer(&l.name);
    }
    let top = Frame { m: M::IDENT, zk: 1.0, z0: 0.0, layer: None, color: None, depth: 0 };
    for e in doc.model_space_entities() {
        cx.entity(e, &top);
    }
    let mut notes = vec![];
    if stats.skipped_source_records > 0 {
        notes.push(format!("{} of the file's records couldn't be read and were left out.", stats.skipped_source_records));
    }
    if cx.full {
        notes.push(format!("The drawing is very large: what came after its first {} million points was left out.", MAX_POINTS / 1_000_000));
    }
    let (extents, bulk) = extents(&cx.shapes);
    Ok(Drawing {
        format: if dwg { "DWG" } else { "DXF" }.into(),
        version: stats.source_version.clone(),
        unit: unit.into(),
        unit_mm,
        layers: cx.layers,
        shapes: cx.shapes,
        extents,
        bulk,
        left_out: cx.left,
        notes,
    })
}

// ---- a plane transform ---------------------------------------------------------------------------------------------

/// x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug)]
struct M {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl M {
    const IDENT: M = M { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
    const FLIP_X: M = M { a: -1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn at(&self, x: f64, y: f64) -> [f64; 2] {
        [self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f]
    }
    fn dir(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }
    /// self after `o`: o is applied first.
    fn mul(&self, o: &M) -> M {
        M {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }
    fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }
    fn scale(&self) -> f64 {
        self.det().abs().sqrt()
    }
    /// Circles stay circles: the same scale both ways, turned and perhaps mirrored.
    fn similar(&self) -> bool {
        let (sx, sy) = (self.a.hypot(self.b), self.c.hypot(self.d));
        let dot = self.a * self.c + self.b * self.d;
        sx > 0.0 && (sx - sy).abs() <= 1e-9 * sx.max(sy) && dot.abs() <= 1e-9 * sx * sy
    }
    fn place(x: f64, y: f64, rot: f64, sx: f64, sy: f64) -> M {
        let (s, c) = rot.sin_cos();
        M { a: c * sx, b: s * sx, c: -s * sy, d: c * sy, e: x, f: y }
    }
}

/// Where an entity sits: the transform down from its block references, heights scaled and lifted the same way, and
/// what it takes from the reference (the layer for what is on layer 0, the colour for what is "by block").
#[derive(Clone, Copy)]
struct Frame {
    m: M,
    zk: f64,
    z0: f64,
    layer: Option<u32>,
    color: Option<[u8; 3]>,
    depth: usize,
}

impl Frame {
    fn z(&self, z: f64) -> f64 {
        let v = self.z0 + self.zk * z;
        if v.is_finite() {
            v
        } else {
            0.0
        }
    }
    /// An entity drawn in its own plane (circles, arcs, 2D polylines, text): one facing down is mirrored.
    fn plane(&self, normal_z: f64) -> Frame {
        if normal_z < -0.5 {
            Frame { m: self.m.mul(&M::FLIP_X), ..*self }
        } else {
            *self
        }
    }
}

/// A layer and a colour of the shape's own.
type Look = (u32, Option<[u8; 3]>);

struct Cx<'a> {
    doc: &'a acadrust::CadDocument,
    layers: Vec<Layer>,
    by_name: BTreeMap<String, u32>,
    shapes: Vec<Shape>,
    left: BTreeMap<String, usize>,
    points: usize,
    full: bool,
    ltscale: f64,
}

fn rgb(c: &Color) -> Option<[u8; 3]> {
    c.rgb().map(|(r, g, b)| [r, g, b])
}

/// How far round anticlockwise from `a0` to `a1`; all the way when they are the same.
fn sweep_ccw(a0: f64, a1: f64) -> f64 {
    let s = (a1 - a0).rem_euclid(TAU);
    if s < 1e-12 {
        TAU
    } else {
        s
    }
}

/// Points along an arc from `a0` round by `sweep` (signed), ends included.
fn arc_points(cx: f64, cy: f64, r: f64, a0: f64, sweep: f64, skip_first: bool, out: &mut Vec<(f64, f64)>) {
    let n = ((sweep.abs() / TAU * CHORDS).ceil() as usize).clamp(2, 2000);
    for i in (if skip_first { 1 } else { 0 })..=n {
        let a = a0 + sweep * i as f64 / n as f64;
        out.push((cx + r * a.cos(), cy + r * a.sin()));
    }
}

/// A polyline's points with its bulges drawn as arcs: each vertex is (x, y, bulge to the next).
fn bulged(v: &[(f64, f64, f64)], closed: bool) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(v.len());
    let n = v.len();
    for i in 0..n {
        let (x0, y0, b) = v[i];
        if out.is_empty() {
            out.push((x0, y0));
        }
        if i + 1 == n && !closed {
            break;
        }
        let (x1, y1, _) = v[(i + 1) % n];
        let chord = (x1 - x0).hypot(y1 - y0);
        if b.abs() > 1e-9 && b.is_finite() && chord > 1e-12 {
            // the bulge is the tangent of a quarter of the arc's angle, positive to the left
            let sweep = 4.0 * b.atan();
            let r = chord / (2.0 * (sweep / 2.0).sin().abs());
            let h = r * (sweep / 2.0).cos() * sweep.signum();
            let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            let (px, py) = (mx - h * (y1 - y0) / chord, my + h * (x1 - x0) / chord);
            arc_points(px, py, r, (y0 - py).atan2(x0 - px), sweep, true, &mut out);
        } else {
            out.push((x1, y1));
        }
    }
    if closed && out.len() > 1 {
        out.pop(); // the last point is the first again
    }
    out
}

/// A spline as points: evaluated from its control points and knots when they agree, else its fit points.
fn spline_points(degree: usize, knots: &[f64], ctrl: &[(f64, f64)], weights: &[f64], fit: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let n = ctrl.len();
    let p = degree.max(1);
    if n > p && knots.len() == n + p + 1 && knots.windows(2).all(|w| w[1] >= w[0]) && knots[n] > knots[p] {
        let w = |i: usize| if weights.len() == n && weights[i] > 0.0 { weights[i] } else { 1.0 };
        let samples = (n * 8).clamp(16, 4000);
        let (t0, t1) = (knots[p], knots[n]);
        let mut out = Vec::with_capacity(samples + 1);
        for s in 0..=samples {
            let t = (t0 + (t1 - t0) * s as f64 / samples as f64).min(t1);
            // the knot span holding t
            let mut k = p;
            while k + 1 < n && knots[k + 1] <= t {
                k += 1;
            }
            // de Boor, on weighted points
            let mut d: Vec<(f64, f64, f64)> = (0..=p)
                .map(|j| {
                    let i = k - p + j;
                    (ctrl[i].0 * w(i), ctrl[i].1 * w(i), w(i))
                })
                .collect();
            for r in 1..=p {
                for j in (r..=p).rev() {
                    let i = k - p + j;
                    let den = knots[i + p + 1 - r] - knots[i];
                    let a = if den.abs() < 1e-14 { 0.0 } else { (t - knots[i]) / den };
                    d[j] = ((1.0 - a) * d[j - 1].0 + a * d[j].0, (1.0 - a) * d[j - 1].1 + a * d[j].1, (1.0 - a) * d[j - 1].2 + a * d[j].2);
                }
            }
            let (x, y, ww) = d[p];
            if ww.abs() > 1e-14 {
                out.push((x / ww, y / ww));
            }
        }
        return out;
    }
    if fit.len() >= 2 {
        fit.to_vec()
    } else {
        ctrl.to_vec()
    }
}

impl Cx<'_> {
    fn add_layer(&mut self, name: &str) -> u32 {
        let key = name.to_lowercase();
        if let Some(&i) = self.by_name.get(&key) {
            return i;
        }
        let l = self.doc.layers.get(name);
        let dash = l
            .and_then(|l| self.doc.line_types.get(&l.line_type))
            .map(|lt| lt.elements.iter().map(|e| e.length.abs() * self.ltscale).collect::<Vec<f64>>())
            .filter(|d| d.len() >= 2 && d.iter().all(|v| v.is_finite()) && d.iter().sum::<f64>() > 0.0)
            .unwrap_or_default();
        self.layers.push(Layer {
            name: name.to_string(),
            color: l.and_then(|l| rgb(&l.color)).unwrap_or([255, 255, 255]),
            on: l.is_none_or(|l| !l.flags.off && !l.flags.frozen),
            weight_mm: l.and_then(|l| l.line_weight.millimeters()).filter(|w| *w > 0.0),
            dash,
        });
        let i = (self.layers.len() - 1) as u32;
        self.by_name.insert(key, i);
        i
    }

    fn left_out(&mut self, what: &str) {
        *self.left.entry(what.to_string()).or_default() += 1;
    }

    fn room(&mut self, n: usize) -> bool {
        if self.points + n > MAX_POINTS {
            self.full = true;
        }
        if self.full {
            return false;
        }
        self.points += n;
        true
    }

    fn push(&mut self, lc: Look, kind: Kind) {
        self.shapes.push(Shape { layer: lc.0, color: lc.1, kind });
    }

    fn poly(&mut self, f: &Frame, lc: Look, pts: &[(f64, f64)], closed: bool, z: Vec<f64>) {
        let ok = pts.iter().all(|p| p.0.is_finite() && p.1.is_finite());
        if !ok || pts.len() < 2 || !self.room(pts.len()) {
            return;
        }
        let pts: Vec<[f64; 2]> = pts.iter().map(|p| f.m.at(p.0, p.1)).collect();
        let z = if z.len() == 1 || z.len() == pts.len() { z } else { vec![] };
        let z = if z.iter().all(|v| v.abs() < 1e-9) {
            vec![]
        } else if z.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9) {
            vec![z[0]]
        } else {
            z
        };
        self.push(lc, Kind::Poly { pts, closed, z });
    }

    /// A circle (`arc` none) or an arc from its first angle anticlockwise to its second, in the entity's plane.
    fn round(&mut self, f: &Frame, lc: Look, c: (f64, f64), r: f64, arc: Option<(f64, f64)>, z: f64) {
        if !(r.is_finite() && r > 0.0 && c.0.is_finite() && c.1.is_finite()) {
            return;
        }
        if f.m.similar() {
            if !self.room(2) {
                return;
            }
            let (cc, rr) = (f.m.at(c.0, c.1), r * f.m.scale());
            match arc {
                None => self.push(lc, Kind::Circle { c: cc, r: rr, z }),
                Some((a0, a1)) => {
                    let ang = |a: f64| {
                        let (x, y) = f.m.dir(a.cos(), a.sin());
                        y.atan2(x)
                    };
                    // mirrored, the arc runs the other way round, so its ends swap
                    let (b0, b1) = if f.m.det() < 0.0 { (ang(a1), ang(a0)) } else { (ang(a0), ang(a1)) };
                    self.push(lc, Kind::Arc { c: cc, r: rr, a0: b0, a1: b1, z });
                }
            }
        } else {
            let mut pts = vec![];
            match arc {
                None => {
                    arc_points(c.0, c.1, r, 0.0, TAU, false, &mut pts);
                    pts.pop();
                }
                Some((a0, a1)) => arc_points(c.0, c.1, r, a0, sweep_ccw(a0, a1), false, &mut pts),
            }
            self.poly(f, lc, &pts, arc.is_none(), vec![z]);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, f: &Frame, lc: Look, at: (f64, f64), h: f64, rot: f64, text: &str, ha: u8, va: u8) {
        let text = text.trim_end();
        let p = f.m.at(at.0, at.1);
        if text.is_empty() || !(h.is_finite() && h > 0.0 && p[0].is_finite() && p[1].is_finite()) || !self.room(4) {
            return;
        }
        let (dx, dy) = f.m.dir(rot.cos(), rot.sin());
        let text: String = text.chars().take(4000).collect();
        self.push(lc, Kind::Text { at: p, h: h * f.m.scale(), angle: dy.atan2(dx), text, ha, va });
    }

    fn fill(&mut self, f: &Frame, lc: Look, loops: Vec<Vec<(f64, f64)>>, pale: bool) {
        let loops: Vec<Vec<[f64; 2]>> = loops
            .into_iter()
            .filter(|l| l.len() >= 3 && l.iter().all(|p| p.0.is_finite() && p.1.is_finite()))
            .map(|l| l.iter().map(|p| f.m.at(p.0, p.1)).collect())
            .collect();
        let n: usize = loops.iter().map(Vec::len).sum();
        if loops.is_empty() || !self.room(n) {
            return;
        }
        self.push(lc, Kind::Fill { loops, pale });
    }

    /// The layer and colour an entity draws with, given what its block reference hands down.
    fn look(&mut self, layer: &str, color: &Color, f: &Frame) -> Look {
        let layer = match f.layer {
            Some(l) if layer == "0" || layer.is_empty() => l,
            _ => self.add_layer(layer),
        };
        let color = match color {
            Color::ByLayer | Color::None => None,
            Color::ByBlock => f.color.or_else(|| f.layer.map(|l| self.layers[l as usize].color)),
            other => rgb(other),
        };
        (layer, color)
    }

    fn entity(&mut self, e: &EntityType, f: &Frame) {
        let common = e.common();
        if self.full || common.invisible {
            return;
        }
        let lc = self.look(&common.layer, &common.color, f);
        match e {
            EntityType::Line(l) => {
                let (a, b) = (&l.start, &l.end);
                self.poly(f, lc, &[(a.x, a.y), (b.x, b.y)], false, vec![f.z(a.z), f.z(b.z)]);
            }
            EntityType::Point(p) => {
                let at = f.m.at(p.location.x, p.location.y);
                if at[0].is_finite() && at[1].is_finite() && self.room(1) {
                    self.push(lc, Kind::Point { at, z: f.z(p.location.z) });
                }
            }
            EntityType::Circle(c) => self.round(&f.plane(c.normal.z), lc, (c.center.x, c.center.y), c.radius, None, f.z(c.center.z)),
            EntityType::Arc(a) => {
                self.round(&f.plane(a.normal.z), lc, (a.center.x, a.center.y), a.radius, Some((a.start_angle, a.end_angle)), f.z(a.center.z))
            }
            EntityType::Ellipse(el) => {
                let (mx, my) = (el.major_axis.x, el.major_axis.y);
                let k = el.minor_axis_ratio * if el.normal.z < -0.5 { -1.0 } else { 1.0 };
                let (nx, ny) = (-my * k, mx * k);
                let (t0, mut t1) = (el.start_parameter, el.end_parameter);
                let full = (t1 - t0).abs() < 1e-9 || ((t1 - t0).abs() - TAU).abs() < 1e-9;
                if full {
                    t1 = t0 + TAU;
                } else if t1 < t0 {
                    t1 += TAU;
                }
                let n = (((t1 - t0) / TAU * CHORDS).ceil() as usize).clamp(4, 2000);
                let mut pts: Vec<(f64, f64)> = (0..=n)
                    .map(|i| {
                        let t = t0 + (t1 - t0) * i as f64 / n as f64;
                        (el.center.x + mx * t.cos() + nx * t.sin(), el.center.y + my * t.cos() + ny * t.sin())
                    })
                    .collect();
                if full {
                    pts.pop();
                }
                self.poly(f, lc, &pts, full, vec![f.z(el.center.z)]);
            }
            EntityType::LwPolyline(p) => {
                let v: Vec<(f64, f64, f64)> = p.vertices.iter().map(|v| (v.location.x, v.location.y, v.bulge)).collect();
                self.poly(&f.plane(p.normal.z), lc, &bulged(&v, p.is_closed), p.is_closed, vec![f.z(p.elevation)]);
            }
            EntityType::Polyline2D(p) => {
                let v: Vec<(f64, f64, f64)> = p.vertices.iter().map(|v| (v.location.x, v.location.y, v.bulge)).collect();
                let closed = p.flags.is_closed();
                let z = if p.elevation != 0.0 { p.elevation } else { p.vertices.first().map_or(0.0, |v| v.location.z) };
                self.poly(&f.plane(p.normal.z), lc, &bulged(&v, closed), closed, vec![f.z(z)]);
            }
            EntityType::Polyline(p) => {
                let pts: Vec<(f64, f64)> = p.vertices.iter().map(|v| (v.location.x, v.location.y)).collect();
                self.poly(f, lc, &pts, p.flags.is_closed(), p.vertices.iter().map(|v| f.z(v.location.z)).collect());
            }
            EntityType::Polyline3D(p) => {
                if p.flags.is_3d_mesh || p.flags.is_polyface_mesh {
                    self.left_out("Mesh");
                } else {
                    let pts: Vec<(f64, f64)> = p.vertices.iter().map(|v| (v.position.x, v.position.y)).collect();
                    self.poly(f, lc, &pts, p.flags.closed, p.vertices.iter().map(|v| f.z(v.position.z)).collect());
                }
            }
            EntityType::Spline(s) => {
                let ctrl: Vec<(f64, f64)> = s.control_points.iter().map(|p| (p.x, p.y)).collect();
                let fit: Vec<(f64, f64)> = s.fit_points.iter().map(|p| (p.x, p.y)).collect();
                let pts = spline_points(s.degree.max(1) as usize, &s.knots, &ctrl, &s.weights, &fit);
                let z = s.control_points.first().or(s.fit_points.first()).map_or(0.0, |p| p.z);
                self.poly(f, lc, &pts, false, vec![f.z(z)]);
            }
            EntityType::Leader(l) => {
                let pts: Vec<(f64, f64)> = l.vertices.iter().map(|p| (p.x, p.y)).collect();
                self.poly(f, lc, &pts, false, vec![]);
            }
            EntityType::Solid(s) => {
                // a solid's corners go 1, 2, 4, 3 round it
                let q = [&s.first_corner, &s.second_corner, &s.fourth_corner, &s.third_corner];
                let mut ring: Vec<(f64, f64)> = q.iter().map(|p| (p.x, p.y)).collect();
                ring.dedup();
                self.fill(&f.plane(s.normal.z), lc, vec![ring], false);
            }
            EntityType::Text(t) => {
                let (ha, va) = match (t.horizontal_alignment, t.vertical_alignment) {
                    (HAlign::Middle, _) => (1, 2),
                    (h, v) => (
                        match h {
                            HAlign::Center => 1,
                            HAlign::Right => 2,
                            _ => 0,
                        },
                        match v {
                            VAlign::Bottom => 1,
                            VAlign::Middle => 2,
                            VAlign::Top => 3,
                            _ => 0,
                        },
                    ),
                };
                // text set from anywhere but its bottom left stands on its alignment point
                let p = match t.alignment_point {
                    Some(a) if (ha, va) != (0, 0) && (a.x != 0.0 || a.y != 0.0) => a,
                    _ => t.insertion_point,
                };
                self.text(&f.plane(t.normal.z), lc, (p.x, p.y), t.height, t.rotation, &t.value, ha, va);
            }
            EntityType::MText(t) => {
                let plain = parse_mtext(&t.value, true).to_plain_text();
                let ap = (t.attachment_point as u8).clamp(1, 9);
                let (ha, va) = ((ap - 1) % 3, [3, 2, 1][((ap - 1) / 3) as usize]);
                let rot = t.dwg_x_direction.map_or(t.rotation, |d| d.y.atan2(d.x));
                self.text(f, lc, (t.insertion_point.x, t.insertion_point.y), t.height, rot, &plain, ha, va);
            }
            EntityType::AttributeEntity(a) => {
                self.text(f, lc, (a.insertion_point.x, a.insertion_point.y), a.height, a.rotation, &a.value, 0, 0);
            }
            EntityType::Hatch(h) => self.hatch(h, f, lc),
            EntityType::Insert(i) => self.insert(i, f, lc),
            EntityType::Dimension(d) => {
                // a dimension draws the block made for it, which is already where the dimension is
                let name = d.base().block_name.clone();
                if name.is_empty() || self.doc.block_records.get(&name).is_none() || f.depth >= MAX_DEPTH {
                    self.left_out("Dimension");
                } else {
                    let inner = Frame { layer: Some(lc.0), color: lc.1, depth: f.depth + 1, ..*f };
                    let doc = self.doc;
                    for e in doc.entities_in_block(&name) {
                        self.entity(e, &inner);
                    }
                }
            }
            // what marks a block's start and end, attribute templates, lines with no end, paper space windows
            EntityType::Block(_)
            | EntityType::BlockEnd(_)
            | EntityType::Seqend(_)
            | EntityType::AttributeDefinition(_)
            | EntityType::Ray(_)
            | EntityType::XLine(_)
            | EntityType::Viewport(_) => {}
            other => {
                let name = other.as_entity().entity_type().to_string();
                self.left_out(&name);
            }
        }
    }

    fn insert(&mut self, i: &Insert, f: &Frame, lc: Look) {
        let doc = self.doc;
        let Some(rec) = doc.block_records.get(&i.block_name) else {
            return self.left_out("Block reference (its block is missing)");
        };
        if f.depth >= MAX_DEPTH {
            return self.left_out("Block reference (nested too deep)");
        }
        let (cols, rows) = (i.column_count.max(1) as usize, i.row_count.max(1) as usize);
        if cols * rows > 10_000 {
            return self.left_out("Block reference (array too large)");
        }
        let base = match doc.get_entity(rec.block_entity_handle) {
            Some(EntityType::Block(b)) => (b.base_point.x, b.base_point.y, b.base_point.z),
            _ => (0.0, 0.0, 0.0),
        };
        let own = f.plane(i.normal.z);
        let (sx, sy, sz) = (i.x_scale(), i.y_scale(), i.z_scale());
        let (s, co) = i.rotation.sin_cos();
        for r in 0..rows {
            for c in 0..cols {
                let (ox, oy) = (c as f64 * i.column_spacing, r as f64 * i.row_spacing);
                let at = (i.insert_point.x + ox * co - oy * s, i.insert_point.y + ox * s + oy * co);
                let m = own.m.mul(&M::place(at.0, at.1, i.rotation, sx, sy)).mul(&M { e: -base.0, f: -base.1, ..M::IDENT });
                let zk = f.zk * sz;
                let inner = Frame { m, zk, z0: f.z(i.insert_point.z) - zk * base.2, layer: Some(lc.0), color: lc.1.or(f.color), depth: f.depth + 1 };
                for e in doc.entities_in_block(&i.block_name) {
                    self.entity(e, &inner);
                }
            }
        }
        // the values typed into the block's attributes sit in the reference's own space
        let within = Frame { layer: Some(lc.0), color: lc.1, ..*f };
        for a in &i.attributes {
            if a.common.invisible {
                continue;
            }
            let alc = self.look(&a.common.layer, &a.common.color, &within);
            self.text(f, alc, (a.insertion_point.x, a.insertion_point.y), a.height, a.rotation, &a.value, 0, 0);
        }
    }

    fn hatch(&mut self, h: &Hatch, f: &Frame, lc: Look) {
        let mut loops = vec![];
        for path in &h.paths {
            let mut ring: Vec<(f64, f64)> = vec![];
            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Line(l) => {
                        ring.push((l.start.x, l.start.y));
                        ring.push((l.end.x, l.end.y));
                    }
                    // a polyline edge keeps each vertex's bulge as its z
                    BoundaryEdge::Polyline(p) => {
                        let v: Vec<(f64, f64, f64)> = p.vertices.iter().map(|v| (v.x, v.y, v.z)).collect();
                        ring.extend(bulged(&v, p.is_closed));
                    }
                    BoundaryEdge::CircularArc(a) => {
                        // a clockwise edge measures its angles clockwise too
                        let sw = sweep_ccw(a.start_angle, a.end_angle);
                        let (a0, sweep) = if a.counter_clockwise { (a.start_angle, sw) } else { (-a.start_angle, -sw) };
                        arc_points(a.center.x, a.center.y, a.radius, a0, sweep, false, &mut ring);
                    }
                    BoundaryEdge::EllipticArc(a) => {
                        let (mx, my) = (a.major_axis_endpoint.x, a.major_axis_endpoint.y);
                        let (nx, ny) = (-my * a.minor_axis_ratio, mx * a.minor_axis_ratio);
                        let sw = sweep_ccw(a.start_angle, a.end_angle);
                        let (t0, sweep) = if a.counter_clockwise { (a.start_angle, sw) } else { (-a.start_angle, -sw) };
                        let n = ((sweep.abs() / TAU * CHORDS).ceil() as usize).clamp(2, 2000);
                        for k in 0..=n {
                            let t = t0 + sweep * k as f64 / n as f64;
                            ring.push((a.center.x + mx * t.cos() + nx * t.sin(), a.center.y + my * t.cos() + ny * t.sin()));
                        }
                    }
                    BoundaryEdge::Spline(s) => {
                        let ctrl: Vec<(f64, f64)> = s.control_points.iter().map(|p| (p.x, p.y)).collect();
                        let w: Vec<f64> = if s.rational { s.control_points.iter().map(|p| p.z).collect() } else { vec![] };
                        let fit: Vec<(f64, f64)> = s.fit_points.iter().map(|p| (p.x, p.y)).collect();
                        ring.extend(spline_points(s.degree.max(1) as usize, &s.knots, &ctrl, &w, &fit));
                    }
                }
            }
            ring.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
            if ring.len() >= 3 {
                loops.push(ring);
            }
        }
        self.fill(&f.plane(h.normal.z), lc, loops, !h.is_solid);
    }
}

/// The box round the shapes, and the box round their bulk: what lies far from the middle of the drawing (more than
/// three times the spread of its middle 96 percent) is left out of the second.
fn extents(shapes: &[Shape]) -> (Option<[f64; 4]>, Option<[f64; 4]>) {
    const NONE: [f64; 4] = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let join = |a: [f64; 4], b: &[f64; 4]| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])];
    let mut boxes: Vec<[f64; 4]> = Vec::with_capacity(shapes.len());
    for s in shapes {
        let mut b = NONE;
        let mut add = |x: f64, y: f64| b = join(b, &[x, y, x, y]);
        match &s.kind {
            Kind::Poly { pts, .. } => pts.iter().for_each(|p| add(p[0], p[1])),
            Kind::Circle { c, r, .. } | Kind::Arc { c, r, .. } => {
                add(c[0] - r, c[1] - r);
                add(c[0] + r, c[1] + r);
            }
            Kind::Text { at, .. } | Kind::Point { at, .. } => add(at[0], at[1]),
            Kind::Fill { loops, .. } => loops.iter().flatten().for_each(|p| add(p[0], p[1])),
        }
        if b[0] <= b[2] {
            boxes.push(b);
        }
    }
    if boxes.is_empty() {
        return (None, None);
    }
    let all = boxes.iter().fold(NONE, join);
    let span = |k: usize| {
        let mut v: Vec<f64> = boxes.iter().map(|b| (b[k] + b[k + 2]) / 2.0).collect();
        v.sort_by(|a, b| a.total_cmp(b));
        let (lo, hi) = (v[v.len() * 2 / 100], v[(v.len() * 98 / 100).min(v.len() - 1)]);
        let grow = (hi - lo).max(1e-6) * 3.0;
        (lo - grow, hi + grow)
    };
    let (x, y) = (span(0), span(1));
    let near = boxes.iter().filter(|b| b[0] >= x.0 && b[2] <= x.1 && b[1] >= y.0 && b[3] <= y.1);
    let bulk = near.fold(NONE, join);
    (Some(all), Some(if bulk[0] <= bulk[2] { bulk } else { all }))
}

impl Drawing {
    pub fn layer_index(&self, name: &str) -> Option<u32> {
        self.layers.iter().position(|l| l.name.eq_ignore_ascii_case(name)).map(|i| i as u32)
    }

    /// How many shapes of each kind: runs of lines, arcs and circles, texts, fills, points.
    pub fn counts(&self) -> [usize; 5] {
        let mut n = [0; 5];
        for s in &self.shapes {
            n[match s.kind {
                Kind::Poly { .. } => 0,
                Kind::Circle { .. } | Kind::Arc { .. } => 1,
                Kind::Text { .. } => 2,
                Kind::Fill { .. } => 3,
                Kind::Point { .. } => 4,
            }] += 1;
        }
        n
    }
}

/// A DWG made from DXF text by the same library, so tests need no CAD program's files.
pub fn dwg_from_dxf(dxf: &str) -> Result<Vec<u8>, String> {
    let doc = DxfReader::from_reader(Cursor::new(dxf.as_bytes().to_vec()))
        .and_then(DxfReader::read)
        .map_err(|e| format!("the DXF couldn't be read: {e}"))?;
    acadrust::DwgWriter::write_to_vec(&doc).map_err(|e| format!("the DWG couldn't be written: {e}"))
}

#[cfg(test)]
mod tests;
