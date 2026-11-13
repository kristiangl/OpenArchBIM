//! CAD drawings (DXF and DWG) imported or linked into the project: a surveyor's or a consultant's drawing drawn in
//! floor plans, to trace over, snap to and check against.
//!
//! An import is one element. It holds the drawing as it was read (flat shapes in the drawing's own units, see the
//! `openarch-cad` crate) and where it sits: the drawing's origin on a model point, turned, at a scale (mm to one
//! drawing unit). It shows in every floor plan of its level, or in one view only (Current view only). Layers can be
//! hidden one by one. A linked drawing remembers its file and can be read again (Reload); an imported one doesn't.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub use openarch_cad::{Drawing, Kind, Layer, Shape};

use crate::edit::Xform;
use crate::view::ViewKind;
use crate::{Document, Element, Id, Point};

/// The drawing an import holds. Shared, not copied, when the element is cloned (undo, move, copy), and two are
/// taken as equal when they are the same reading of a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CadData(pub Arc<Drawing>);

impl PartialEq for CadData {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.0, &o.0)
    }
}

impl std::ops::Deref for CadData {
    type Target = Drawing;
    fn deref(&self) -> &Drawing {
        &self.0
    }
}

/// How an import's lines are coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CadColors {
    /// The file's colours (white becomes black on paper).
    #[default]
    Preserve,
    BlackAndWhite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadImport {
    /// The file's name, as shown.
    pub name: String,
    /// A linked drawing's file, read again by Reload. None for an import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub drawing: CadData,
    /// Changes with each reading of the file, so what draws the drawing knows when to fetch it again.
    pub stamp: String,
    pub level: Id,
    /// Current view only: the one view that shows it. None shows it in every floor plan of its level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Id>,
    /// Where the drawing's origin sits (mm).
    pub at: Point,
    /// Turned anticlockwise about `at`.
    #[serde(default)]
    pub angle_deg: f64,
    /// Millimetres to one drawing unit.
    pub scale: f64,
    /// Layers not drawn, by name.
    #[serde(default)]
    pub hidden_layers: BTreeSet<String>,
    #[serde(default)]
    pub colors: CadColors,
    /// Drawn over the model instead of under it.
    #[serde(default)]
    pub foreground: bool,
    #[serde(default)]
    pub pinned: bool,
}

/// Where an import goes when it comes in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CadPosition {
    /// The middle of the drawing (strays left out) on the point.
    Center(Point),
    /// The drawing's origin on the point.
    Origin(Point),
}

#[derive(Debug, Clone)]
pub struct CadOptions {
    /// The floor plan it is brought into.
    pub view: Id,
    pub current_view_only: bool,
    /// Millimetres to one drawing unit; none takes the file's unit, or guesses when the file doesn't say.
    pub scale: Option<f64>,
    pub position: CadPosition,
    /// Hide the layers the file has off or frozen (else every layer shows).
    pub visible_layers_only: bool,
    pub colors: CadColors,
}

/// What an import brought in.
#[derive(Debug, Clone)]
pub struct CadReport {
    pub id: Id,
    /// Millimetres to a drawing unit, as used, and how it was arrived at: "file" (the file says), "given", or
    /// "guessed" (the file doesn't say: metres for a drawing under 2000 units across, else millimetres).
    pub scale: f64,
    pub scale_from: &'static str,
}

impl CadImport {
    /// A point of the drawing, in the model (mm).
    pub fn place(&self, p: [f64; 2]) -> Point {
        let (s, c) = self.angle_deg.to_radians().sin_cos();
        let (x, y) = (p[0] * self.scale, p[1] * self.scale);
        Point::new(self.at.x + x * c - y * s, self.at.y + x * s + y * c)
    }

    /// A model point, in the drawing.
    pub fn unplace(&self, p: Point) -> [f64; 2] {
        let (s, c) = self.angle_deg.to_radians().sin_cos();
        let (x, y) = (p.x - self.at.x, p.y - self.at.y);
        [(x * c + y * s) / self.scale, (-x * s + y * c) / self.scale]
    }

    /// The corners of the box round the drawing's bulk, in the model.
    pub fn corners(&self) -> Option<[Point; 4]> {
        let b = self.drawing.bulk.or(self.drawing.extents)?;
        Some([self.place([b[0], b[1]]), self.place([b[2], b[1]]), self.place([b[2], b[3]]), self.place([b[0], b[3]])])
    }

    pub fn layer_shown(&self, l: &Layer) -> bool {
        !self.hidden_layers.contains(&l.name)
    }

    /// Whether a view draws it.
    pub fn shows_in(&self, view: Id, kind: ViewKind, level: Option<Id>) -> bool {
        match self.view {
            Some(v) => v == view,
            None => kind == ViewKind::FloorPlan && level == Some(self.level),
        }
    }
}

fn check(c: &CadImport) -> Result<(), String> {
    if !(c.scale.is_finite() && c.scale > 1e-6 && c.scale < 1e9) {
        return Err("The scale must be more than zero.".into());
    }
    if !(c.angle_deg.is_finite() && c.at.x.is_finite() && c.at.y.is_finite()) {
        return Err("The drawing's position isn't a number.".into());
    }
    Ok(())
}

static STAMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn new_stamp() -> String {
    format!("{}-{}", uuid::Uuid::new_v4().simple(), STAMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

impl Document {
    pub fn cad_imports(&self) -> Vec<(Id, &CadImport)> {
        self.iter().filter_map(|(id, e)| match e { Element::CadImport(c) => Some((id, c)), _ => None }).collect()
    }

    pub fn cad_import(&self, id: Id) -> Option<&CadImport> {
        match self.get(id) { Some(Element::CadImport(c)) => Some(c), _ => None }
    }

    /// The imports a view draws.
    pub fn cad_imports_in(&self, view: Id) -> Vec<(Id, &CadImport)> {
        let Some(v) = self.view(view) else { return vec![] };
        self.cad_imports().into_iter().filter(|(_, c)| c.shows_in(view, v.view_kind, v.level)).collect()
    }

    /// Import CAD or Link CAD (`path` kept): the file's bytes read and placed in a floor plan. One undo step.
    pub fn import_cad(&mut self, bytes: &[u8], name: &str, path: Option<String>, opt: &CadOptions) -> Result<CadReport, String> {
        let v = self.view(opt.view).ok_or("unknown view")?;
        if v.view_kind != ViewKind::FloorPlan { return Err("Open a floor plan to bring a CAD drawing into.".into()); }
        let level = v.level.ok_or("the view has no level")?;
        let drawing = openarch_cad::read(bytes)?;
        if drawing.shapes.is_empty() {
            return Err("The drawing has nothing in its model space to bring in.".into());
        }
        let b = drawing.bulk.or(drawing.extents).ok_or("the drawing is empty")?;
        let (scale, scale_from) = match opt.scale {
            Some(s) => (s, "given"),
            None if drawing.unit_mm > 0.0 => (drawing.unit_mm, "file"),
            // a building drawn in millimetres is thousands of units across; a site in metres isn't
            None => (if (b[2] - b[0]).max(b[3] - b[1]) < 2000.0 { 1000.0 } else { 1.0 }, "guessed"),
        };
        let at = match opt.position {
            CadPosition::Origin(p) => p,
            CadPosition::Center(p) => Point::new(p.x - (b[0] + b[2]) / 2.0 * scale, p.y - (b[1] + b[3]) / 2.0 * scale),
        };
        let hidden_layers = if opt.visible_layers_only { drawing.layers.iter().filter(|l| !l.on).map(|l| l.name.clone()).collect() } else { BTreeSet::new() };
        let c = CadImport {
            name: name.to_string(), path, drawing: CadData(Arc::new(drawing)), stamp: new_stamp(), level,
            view: opt.current_view_only.then_some(opt.view), at, angle_deg: 0.0, scale, hidden_layers, colors: opt.colors, foreground: false, pinned: false,
        };
        check(&c)?;
        let label = if c.path.is_some() { "Link CAD" } else { "Import CAD" };
        let id = self.transact(label, |tx| Ok(tx.add(Element::CadImport(c))))?;
        Ok(CadReport { id, scale, scale_from })
    }

    /// Reload: a linked drawing read again from `bytes` (its file as it is now). It stays where it is; layers hidden
    /// stay hidden.
    pub fn reload_cad(&mut self, id: Id, bytes: &[u8]) -> Result<(), String> {
        let mut c = self.cad_import(id).cloned().ok_or("not a CAD drawing")?;
        let drawing = openarch_cad::read(bytes)?;
        if drawing.shapes.is_empty() { return Err("The file now has nothing in its model space.".into()); }
        c.drawing = CadData(Arc::new(drawing));
        c.stamp = new_stamp();
        self.transact("Reload CAD", |tx| tx.update(id, Element::CadImport(c)))
    }

    /// Properties: pinned, angle_deg, scale, colors, foreground, hidden_layers, at, name.
    pub fn patch_cad_import(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.cad_import(id).cloned().ok_or("not a CAD drawing")?;
        if patch.get("drawing").is_some() || patch.get("stamp").is_some() { return Err("the drawing itself can't be set".into()); }
        // the drawing is left out of the round trip through JSON a patch makes
        let mut slim = old.clone();
        slim.drawing = CadData(Arc::new(Drawing::default()));
        let mut c: CadImport = crate::doc::apply_patch(&slim, patch)?;
        c.drawing = old.drawing.clone();
        check(&c)?;
        if old.pinned && c.pinned && (c.at != old.at || c.angle_deg != old.angle_deg || c.scale != old.scale) {
            return Err("Unpin the drawing before moving it.".into());
        }
        let known: BTreeSet<&str> = c.drawing.layers.iter().map(|l| l.name.as_str()).collect();
        c.hidden_layers.retain(|l| known.contains(l.as_str()));
        self.transact("CAD drawing properties", |tx| tx.update(id, Element::CadImport(c)))
    }

    /// An import moved, turned or copied. A drawing can't be mirrored (its text would read backwards).
    pub(crate) fn cad_transformed(c: &CadImport, x: &Xform, copy: bool) -> Result<CadImport, String> {
        let mut c = c.clone();
        match *x {
            Xform::Mirror { .. } => return Err("A CAD drawing can't be mirrored.".into()),
            Xform::Rotate { angle_deg, .. } => c.angle_deg = (c.angle_deg + angle_deg).rem_euclid(360.0),
            Xform::Move { .. } => {}
        }
        c.at = x.apply(c.at);
        c.pinned = c.pinned && !copy;
        Ok(c)
    }
}

/// What a layer of an import holds that has a height: contours, 3D lines and survey points.
#[derive(Debug, Clone, PartialEq)]
pub struct CadHeights {
    pub layer: String,
    /// Shapes with a height other than zero, and those lying at zero.
    pub raised: usize,
    pub flat: usize,
    /// The lowest and highest height, in the drawing's units (of the raised ones; 0, 0 with none).
    pub lo: f64,
    pub hi: f64,
    /// Text that reads as a level (a number with a decimal point, as a surveyor writes a spot level).
    pub level_texts: usize,
    /// Closed lines, any of which can be the boundary of the ground made from the drawing.
    pub closed: usize,
}

/// A text read as a spot level: a number with a decimal point, with or without RL in front ("12.45", "RL 12.45").
pub fn level_of_text(text: &str) -> Option<f64> {
    let t = text.trim();
    let t = t.strip_prefix("RL").or_else(|| t.strip_prefix("rl")).unwrap_or(t).trim_start_matches([':', '=', ' ']).trim();
    let t = t.strip_prefix('+').unwrap_or(t);
    if !t.contains('.') || t.contains(char::is_whitespace) { return None; }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The points of a shape with their heights, in the drawing's units. Text and fills have none.
fn heights_of(k: &Kind, out: &mut Vec<[f64; 3]>) {
    match k {
        Kind::Poly { pts, z, .. } => {
            for (i, p) in pts.iter().enumerate() {
                let h = match z.len() { 0 => 0.0, 1 => z[0], _ => z.get(i).copied().unwrap_or(0.0) };
                out.push([p[0], p[1], h]);
            }
        }
        Kind::Point { at, z } => out.push([at[0], at[1], *z]),
        Kind::Circle { c, r, z } => {
            for i in 0..12 {
                let a = std::f64::consts::TAU * i as f64 / 12.0;
                out.push([c[0] + r * a.cos(), c[1] + r * a.sin(), *z]);
            }
        }
        Kind::Arc { c, r, a0, a1, z } => {
            let sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
            for i in 0..=4 {
                let a = a0 + sweep * i as f64 / 4.0;
                out.push([c[0] + r * a.cos(), c[1] + r * a.sin(), *z]);
            }
        }
        Kind::Text { .. } | Kind::Fill { .. } => {}
    }
}

impl Document {
    /// The layers of an import and what on each has a height, for Create from CAD.
    pub fn cad_heights(&self, id: Id) -> Result<Vec<CadHeights>, String> {
        let c = self.cad_import(id).ok_or("not a CAD drawing")?;
        let mut out: Vec<CadHeights> = c.drawing.layers.iter().map(|l| CadHeights { layer: l.name.clone(), raised: 0, flat: 0, lo: f64::INFINITY, hi: f64::NEG_INFINITY, level_texts: 0, closed: 0 }).collect();
        let mut pts = vec![];
        for s in &c.drawing.shapes {
            let Some(h) = out.get_mut(s.layer as usize) else { continue };
            match &s.kind {
                Kind::Text { text, .. } if level_of_text(text).is_some() => h.level_texts += 1,
                Kind::Poly { pts, closed: true, .. } if pts.len() >= 3 => h.closed += 1,
                _ => {}
            }
            pts.clear();
            heights_of(&s.kind, &mut pts);
            if pts.is_empty() { continue; }
            let mut any = false;
            for p in pts.iter().filter(|p| p[2] != 0.0 && p[2].is_finite()) {
                any = true;
                h.lo = h.lo.min(p[2]);
                h.hi = h.hi.max(p[2]);
            }
            if any { h.raised += 1 } else { h.flat += 1 }
        }
        for h in &mut out { if h.raised == 0 { (h.lo, h.hi) = (0.0, 0.0); } }
        Ok(out)
    }

    /// The points of an import's chosen layers with their heights, where the import sits in the model: plan
    /// positions as placed, heights at the import's scale above `base_z` (the height the drawing's zero is at).
    /// `skip_zero` leaves out what lies at height zero (flat linework sharing a layer with the contours).
    /// `text_levels` also takes each text that reads as a level (`level_of_text`) as a point where the text
    /// starts. Points within a millimetre of one already taken are left out.
    pub fn topo_points_from_cad(&self, id: Id, layers: &BTreeSet<String>, skip_zero: bool, text_levels: bool, base_z: f64) -> Result<Vec<[f64; 3]>, String> {
        let c = self.cad_import(id).ok_or("not a CAD drawing")?;
        let chosen: Vec<bool> = c.drawing.layers.iter().map(|l| layers.contains(&l.name)).collect();
        let mut seen = std::collections::HashSet::new();
        let (mut raw, mut out) = (vec![], vec![]);
        for s in &c.drawing.shapes {
            if !chosen.get(s.layer as usize).copied().unwrap_or(false) { continue; }
            raw.clear();
            heights_of(&s.kind, &mut raw);
            if let (true, Kind::Text { at, text, .. }) = (text_levels, &s.kind) {
                if let Some(z) = level_of_text(text) { raw.push([at[0], at[1], z]); }
            }
            for p in &raw {
                if !p[2].is_finite() || (skip_zero && p[2] == 0.0) { continue; }
                let q = c.place([p[0], p[1]]);
                if seen.insert((q.x.round() as i64, q.y.round() as i64)) { out.push([q.x, q.y, p[2] * c.scale + base_z]); }
            }
        }
        if out.len() < 3 {
            return Err(if skip_zero { "Nothing on those layers has a height. Pick the layers with the contours or survey points, or untick Leave out what lies at height 0." } else { "Those layers have fewer than three points." }.into());
        }
        Ok(out)
    }

    /// The largest closed line on a layer of an import, where the import sits in the model: the boundary of a lot,
    /// for the edge of the ground made from the drawing.
    pub fn cad_boundary(&self, id: Id, layer: &str) -> Result<Vec<Point>, String> {
        let c = self.cad_import(id).ok_or("not a CAD drawing")?;
        let li = c.drawing.layers.iter().position(|l| l.name == layer).ok_or_else(|| format!("The drawing has no layer {layer}."))?;
        let mut best: Option<(f64, Vec<Point>)> = None;
        for s in c.drawing.shapes.iter().filter(|s| s.layer as usize == li) {
            let Kind::Poly { pts, closed: true, .. } = &s.kind else { continue };
            let mut ring: Vec<Point> = pts.iter().map(|p| c.place(*p)).collect();
            ring.dedup_by(|a, b| a.dist(b) < 1.0);
            if ring.len() > 3 && ring[0].dist(&ring[ring.len() - 1]) < 1.0 { ring.pop(); }
            if ring.len() < 3 { continue; }
            let a = crate::geom::signed_area(&ring).abs();
            if best.as_ref().is_none_or(|b| a > b.0) { best = Some((a, ring)); }
        }
        match best {
            Some((a, ring)) if a > 1.0 => Ok(ring),
            _ => Err(format!("Layer {layer} has no closed line to use as the boundary.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nWALLS\n10\n0.0\n20\n0.0\n11\n10.0\n21\n0.0\n0\nLINE\n8\nNOTES\n10\n10.0\n20\n0.0\n11\n10.0\n21\n6.0\n0\nENDSEC\n0\nEOF\n";

    fn plan(d: &Document) -> Id {
        d.iter().find_map(|(id, e)| match e { Element::View(v) if v.view_kind == ViewKind::FloorPlan => Some(id), _ => None }).unwrap()
    }

    #[test]
    fn a_drawing_is_placed_moved_turned_and_undone() {
        let mut d = Document::new_project();
        let view = plan(&d);
        let opt = CadOptions { view, current_view_only: false, scale: None, position: CadPosition::Center(Point::new(0.0, 0.0)), visible_layers_only: true, colors: CadColors::Preserve };
        let r = d.import_cad(DXF.as_bytes(), "survey.dxf", None, &opt).unwrap();
        // no unit in the file and only 10 across: taken as metres, its middle (5, 3) on the origin
        assert_eq!((r.scale, r.scale_from), (1000.0, "guessed"));
        let c = d.cad_import(r.id).unwrap();
        assert_eq!(c.at, Point::new(-5000.0, -3000.0));
        assert_eq!(c.place([10.0, 6.0]), Point::new(5000.0, 3000.0));
        let back = c.unplace(Point::new(5000.0, 3000.0));
        assert!((back[0] - 10.0).abs() < 1e-9 && (back[1] - 6.0).abs() < 1e-9);
        assert_eq!(d.cad_imports_in(view).len(), 1);

        d.patch_cad_import(r.id, &serde_json::json!({ "hidden_layers": ["NOTES", "no such layer"], "scale": 1.0 })).unwrap();
        let c = d.cad_import(r.id).unwrap();
        assert_eq!(c.hidden_layers.iter().collect::<Vec<_>>(), vec!["NOTES"]);
        assert_eq!((c.scale, c.drawing.shapes.len()), (1.0, 2));

        let turned = Document::cad_transformed(c, &Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 }, false).unwrap();
        assert!((turned.at.x - 3000.0).abs() < 1e-6 && (turned.at.y + 5000.0).abs() < 1e-6 && turned.angle_deg == 90.0);
        assert!(Document::cad_transformed(c, &Xform::Mirror { ax: 0.0, ay: 0.0, bx: 1.0, by: 0.0 }, false).is_err());

        d.undo().unwrap();
        d.undo().unwrap();
        assert!(d.cad_imports().is_empty());
    }
}
