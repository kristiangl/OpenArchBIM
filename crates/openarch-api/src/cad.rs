//! CAD drawings: Import CAD and Link CAD (DXF and DWG), Reload, Properties, the layers of an import, and what a
//! floor plan is told about the imports it draws. The drawing itself is fetched once (`cad_drawing`) and kept by
//! the page until its stamp changes.
use std::path::PathBuf;

use serde_json::{json, Value};

use openarch_model::cad::{CadColors, CadImport, CadOptions, CadPosition, Kind};
use openarch_model::{Document, Id, Point};

use crate::{id_of, point_of, Res, Session};

fn read_file(path: &str) -> Result<(Vec<u8>, String), String> {
    let p = PathBuf::from(path);
    let bytes = std::fs::read(&p).map_err(|e| format!("Could not read {}: {e}", p.display()))?;
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
    Ok((bytes, name))
}

fn colors_of(v: &Value) -> Result<CadColors, String> {
    match v.as_str() {
        None | Some("preserve") => Ok(CadColors::Preserve),
        Some("black") => Ok(CadColors::BlackAndWhite),
        Some(o) => Err(format!("colors: preserve or black, not {o}")),
    }
}

/// What came in, for the prompt bar and the report.
fn summary(id: Id, c: &CadImport) -> Value {
    let n = c.drawing.counts();
    json!({
        "id": id, "name": c.name, "format": c.drawing.format, "version": c.drawing.version, "unit": c.drawing.unit, "scale": c.scale,
        "linked": c.path.is_some(),
        "counts": { "lines": n[0], "arcs": n[1], "texts": n[2], "fills": n[3], "points": n[4], "layers": c.drawing.layers.len() },
        "left_out": c.drawing.left_out, "notes": c.drawing.notes,
    })
}

/// Rounded to a millionth of a unit: enough for a drawing in metres, and it keeps the text sent to the page short.
fn r6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

impl Session {
    pub(crate) fn handle_cad(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // Insert > Import CAD / Link CAD: { path, view_id, link?, current_view_only?, units? ("auto", "mm", "m", ...),
            // position? ("center" or "origin"), at? [x, y], layers? ("visible" or "all"), colors? ("preserve" or "black") }
            "import_cad" => (|| {
                let path = p["path"].as_str().ok_or("missing path")?;
                let (bytes, name) = read_file(path)?;
                let scale = match p["units"].as_str() {
                    None | Some("auto") => None,
                    Some(u) => Some(openarch_cad::unit_mm(u).ok_or_else(|| format!("unknown unit {u}"))?),
                };
                let at = if p["at"].is_array() { point_of(&p["at"])? } else { Point::new(0.0, 0.0) };
                let position = match p["position"].as_str() {
                    None | Some("center") => CadPosition::Center(at),
                    Some("origin") => CadPosition::Origin(at),
                    Some(o) => return Err(format!("position: center or origin, not {o}")),
                };
                let opt = CadOptions {
                    view: id_of(p, "view_id")?, current_view_only: p["current_view_only"].as_bool().unwrap_or(false), scale, position,
                    visible_layers_only: p["layers"].as_str() != Some("all"), colors: colors_of(&p["colors"])?,
                };
                let link = p["link"].as_bool().unwrap_or(false);
                let r = self.doc.import_cad(&bytes, &name, link.then(|| path.to_string()), &opt)?;
                let c = self.doc.cad_import(r.id).ok_or("the drawing did not come in")?;
                let mut out = summary(r.id, c);
                out["scale_from"] = json!(r.scale_from);
                Ok(out)
            })(),
            // Reload a linked drawing from its file: { id }
            "reload_cad" => (|| {
                let id = id_of(p, "id")?;
                let path = self.doc.cad_import(id).ok_or("not a CAD drawing")?.path.clone().ok_or("This drawing was imported, not linked: there is no file to read again.")?;
                let (bytes, _) = read_file(&path)?;
                self.doc.reload_cad(id, &bytes)?;
                Ok(summary(id, self.doc.cad_import(id).ok_or("gone")?))
            })(),
            "patch_cad_import" => (|| { self.doc.patch_cad_import(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            // The layers of an import hidden in one view only: { view_id, id, hidden: [names] }
            "set_cad_view_layers" => (|| {
                let hidden: std::collections::BTreeSet<String> = serde_json::from_value(p["hidden"].clone()).map_err(|e| format!("bad layers: {e}"))?;
                self.doc.set_cad_view_layers(id_of(p, "view_id")?, id_of(p, "id")?, hidden)?;
                Ok(json!({}))
            })(),
            // An import's layers, for its Layers box: { id, view_id? } (`off_in_view`: hidden in that view only)
            "cad_layers" => (|| {
                let c = self.doc.cad_import(id_of(p, "id")?).ok_or("not a CAD drawing")?;
                let mut used = vec![0usize; c.drawing.layers.len()];
                for s in &c.drawing.shapes { if let Some(n) = used.get_mut(s.layer as usize) { *n += 1; } }
                let off = p["view_id"].as_u64().and_then(|v| self.doc.view(v)).and_then(|v| v.cad_layers_off.get(&id_of(p, "id").unwrap_or(0)).cloned()).unwrap_or_default();
                Ok(json!({ "name": c.name, "layers": c.drawing.layers.iter().enumerate().map(|(i, l)| json!({
                    "name": l.name, "color": l.color, "on_in_file": l.on, "shown": c.layer_shown(l), "shapes": used[i], "off_in_view": off.contains(&l.name),
                })).collect::<Vec<_>>() }))
            })(),
            // What on each layer of an import has a height, for Create from CAD: { id }. Heights in the file's units.
            "cad_heights" => (|| {
                let id = id_of(p, "id")?;
                let c = self.doc.cad_import(id).ok_or("not a CAD drawing")?;
                let rows = self.doc.cad_heights(id)?;
                Ok(json!({ "name": c.name, "unit": c.drawing.unit, "scale": c.scale, "level_id": c.level, "layers": rows.iter().zip(&c.drawing.layers).map(|(h, l)| json!({
                    "name": h.layer, "color": l.color, "shown": c.layer_shown(l), "raised": h.raised, "flat": h.flat, "lo": h.lo, "hi": h.hi, "level_texts": h.level_texts, "closed": h.closed,
                })).collect::<Vec<_>>() }))
            })(),
            // Create from CAD: a toposolid through the contours and points of an import's layers.
            // { id, layers: [names], type_id, level_id?, skip_zero? (true), base? (where the drawing's height 0 is:
            // survey_point, project_base_point or internal_origin), text_levels? (text that reads as a level is a
            // point), boundary_layer? (the largest closed line on it is the boundary, else the convex hull) }
            "toposolid_from_cad" => (|| {
                let id = id_of(p, "id")?;
                let c = self.doc.cad_import(id).ok_or("not a CAD drawing")?;
                let level = p["level_id"].as_u64().unwrap_or(c.level);
                let layers: std::collections::BTreeSet<String> = serde_json::from_value(p["layers"].clone()).map_err(|e| format!("bad layers: {e}"))?;
                let base: openarch_model::topo::TopoBase = match p.get("base").filter(|v| !v.is_null()) {
                    Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad elevation base: {e}"))?,
                    None => openarch_model::topo::TopoBase::SurveyPoint,
                };
                let pts = self.doc.topo_points_from_cad(id, &layers, p["skip_zero"].as_bool().unwrap_or(true), p["text_levels"].as_bool().unwrap_or(false), self.doc.topo_base_z(base, level))?;
                let boundary = match p["boundary_layer"].as_str() { Some(l) => Some(self.doc.cad_boundary(id, l)?), None => None };
                let read = pts.len();
                let (lo, hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[2]), b.max(q[2])));
                let topo = self.doc.add_toposolid_points_within(id_of(p, "type_id")?, level, pts, boundary)?;
                let kept = self.doc.toposolid(topo).map_or(0, |t| t.points.len());
                Ok(json!({ "id": topo, "read": read, "kept": kept, "lo": lo, "hi": hi }))
            })(),
            // The drawing an import holds, as the page draws it: { id }. Coordinates are in the drawing's units, less
            // `base` (the middle of the drawing), so they stay small numbers however far from its origin a survey sits.
            // Shapes: [0, layer, colour, closed, [x, y, ...], z?] a run of lines (z: one height, or one each);
            // [1, layer, colour, x, y, r] a circle; [2, layer, colour, x, y, r, a0, a1] an arc; [3, layer, colour, x, y,
            // height, angle, ha, va, text]; [4, layer, colour, pale, [[x, y, ...], ...]] a fill; [5, layer, colour, x, y, z].
            "cad_drawing" => (|| {
                let c = self.doc.cad_import(id_of(p, "id")?).ok_or("not a CAD drawing")?;
                let d = &c.drawing;
                let b = d.bulk.or(d.extents).unwrap_or([0.0; 4]);
                let (bx, by) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
                let flat = |pts: &[[f64; 2]]| -> Value { Value::Array(pts.iter().flat_map(|q| [json!(r6(q[0] - bx)), json!(r6(q[1] - by))]).collect()) };
                let shapes: Vec<Value> = d.shapes.iter().map(|s| {
                    let (l, col) = (s.layer, s.color);
                    match &s.kind {
                        Kind::Poly { pts, closed, z } if z.is_empty() => json!([0, l, col, *closed as u8, flat(pts)]),
                        Kind::Poly { pts, closed, z } => json!([0, l, col, *closed as u8, flat(pts), z]),
                        Kind::Circle { c, r, .. } => json!([1, l, col, r6(c[0] - bx), r6(c[1] - by), r6(*r)]),
                        Kind::Arc { c, r, a0, a1, .. } => json!([2, l, col, r6(c[0] - bx), r6(c[1] - by), r6(*r), a0, a1]),
                        Kind::Text { at, h, angle, text, ha, va } => json!([3, l, col, r6(at[0] - bx), r6(at[1] - by), r6(*h), angle, ha, va, text]),
                        Kind::Fill { loops, pale } => json!([4, l, col, *pale as u8, loops.iter().map(|q| flat(q)).collect::<Vec<_>>()]),
                        Kind::Point { at, z } => json!([5, l, col, r6(at[0] - bx), r6(at[1] - by), z]),
                    }
                }).collect();
                Ok(json!({
                    "stamp": c.stamp, "base": [bx, by], "extents": [b[0] - bx, b[1] - by, b[2] - bx, b[3] - by],
                    "layers": d.layers.iter().map(|l| json!({ "name": l.name, "color": l.color, "weight_mm": l.weight_mm, "dash": l.dash })).collect::<Vec<_>>(),
                    "shapes": shapes,
                }))
            })(),
            _ => return None,
        })
    }

    pub(crate) fn cad_props(&self, id: Id, c: &CadImport) -> Value {
        let n = c.drawing.counts();
        let yn = |b: bool| if b { "Yes" } else { "No" };
        let level = self.doc.level(c.level).map(|l| l.name.clone()).unwrap_or_default();
        let view = c.view.and_then(|v| self.doc.view(v)).map(|v| v.name.clone());
        let unit = if c.drawing.unit.is_empty() { "Not set in the file".to_string() } else { c.drawing.unit.clone() };
        let left: usize = c.drawing.left_out.values().sum();
        let colors = |v: CadColors, label: &str, key: &str| json!({ "label": label, "patch": { "colors": key }, "selected": c.colors == v });
        let order = |fg: bool, label: &str| json!({ "label": label, "patch": { "foreground": fg }, "selected": c.foreground == fg });
        json!({
            "id": id, "kind": "CadImport", "category": "Imported Drawings", "title": c.name, "type_label": if c.path.is_some() { "Linked CAD Drawing" } else { "Imported CAD Drawing" },
            "target": { "method": "patch_cad_import", "id": id },
            "groups": [
                { "name": "Constraints", "rows": [
                    { "label": "Level", "kind": "ro", "value": level },
                    { "label": "Shown In", "kind": "ro", "value": view.map_or("Every plan of its level".to_string(), |v| format!("{v} only")) },
                    { "label": "Rotation", "kind": "number", "path": "angle_deg", "raw": c.angle_deg, "value": format!("{:.3}\u{b0}", c.angle_deg) },
                    { "label": "Pinned", "kind": "bool", "path": "pinned", "raw": c.pinned, "value": yn(c.pinned) },
                ] },
                { "name": "Dimensions", "rows": [
                    { "label": "Scale (mm per unit)", "kind": "number", "path": "scale", "raw": c.scale, "value": format!("{}", c.scale) },
                    { "label": "File Units", "kind": "ro", "value": unit },
                ] },
                { "name": "Graphics", "rows": [
                    { "label": "Colours", "kind": "select", "value": if c.colors == CadColors::Preserve { "Preserve" } else { "Black and White" },
                      "options": [colors(CadColors::Preserve, "Preserve", "Preserve"), colors(CadColors::BlackAndWhite, "Black and White", "BlackAndWhite")] },
                    { "label": "Draw Layer", "kind": "select", "value": if c.foreground { "Foreground" } else { "Background" },
                      "options": [order(false, "Background"), order(true, "Foreground")] },
                    { "label": "Layers", "kind": "ro", "value": format!("{} of {} shown", c.drawing.layers.iter().filter(|l| c.layer_shown(l)).count(), c.drawing.layers.len()) },
                ] },
                { "name": "Identity Data", "rows": [
                    { "label": "Name", "kind": "text", "path": "name", "value": c.name },
                    { "label": "File", "kind": "ro", "value": c.path.clone().unwrap_or_else(|| "Imported (not linked)".into()) },
                    { "label": "Format", "kind": "ro", "value": format!("{} {}", c.drawing.format, c.drawing.version).trim() },
                    { "label": "Contents", "kind": "ro", "value": format!("{} lines, {} arcs, {} texts, {} fills, {} points", n[0], n[1], n[2], n[3], n[4]) },
                    { "label": "Left Out", "kind": "ro", "value": if left == 0 { "Nothing".to_string() } else { c.drawing.left_out.iter().map(|(k, v)| format!("{v} {k}")).collect::<Vec<_>>().join(", ") } },
                ] },
            ],
        })
    }
}

/// The CAD drawings a 3D view shows, each flat at its level: those not kept to one view, not hidden in the view.
pub(crate) fn scene(d: &Document, view: Option<Id>, shown: bool) -> Vec<Value> {
    if !shown { return vec![]; }
    let v = view.and_then(|v| d.view(v));
    d.cad_imports().into_iter().filter(|(id, c)| c.view.is_none() && !v.is_some_and(|v| v.hidden_elements.contains(id))).map(|(id, c)| {
        let b = c.drawing.bulk.or(c.drawing.extents).unwrap_or([0.0; 4]);
        json!({
            "id": id, "name": c.name, "stamp": c.stamp, "at": [c.at.x, c.at.y], "angle_deg": c.angle_deg, "scale": c.scale,
            "base": [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0], "z": d.level(c.level).map_or(0.0, |l| l.elevation_mm),
            "hidden": view.map_or_else(|| c.hidden_layers.clone(), |v| d.cad_hidden_in(v, id)), "black": c.colors == CadColors::BlackAndWhite,
        })
    }).collect()
}

/// What a view is told about the CAD drawings it draws: where each sits and which of its layers are hidden.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    let list: Vec<Value> = d.cad_imports_in(view).into_iter().filter(|(id, _)| !v.hidden_elements.contains(id)).map(|(id, c)| {
        let b = c.drawing.bulk.or(c.drawing.extents).unwrap_or([0.0; 4]);
        json!({
            "id": id, "name": c.name, "stamp": c.stamp, "at": [c.at.x, c.at.y], "angle_deg": c.angle_deg, "scale": c.scale,
            "base": [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0],
            "ring": c.corners().map(|q| q.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()),
            "hidden": d.cad_hidden_in(view, id), "black": c.colors == CadColors::BlackAndWhite, "foreground": c.foreground, "pinned": c.pinned, "linked": c.path.is_some(),
        })
    }).collect();
    json!({ "cad_imports": list })
}
