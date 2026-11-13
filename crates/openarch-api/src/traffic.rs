//! Traffic: Line Marking, Parking Bay (a row of bays) and Kerb, their Properties, and what a floor plan draws.
use serde_json::{json, Value};

use openarch_model::curve::Curve;
use openarch_model::traffic::{Paint, ParkingRow};
use openarch_model::view::ViewKind;
use openarch_model::{Document, Element, Id};

use crate::graphics::{style_for, Hint};
use crate::{id_of, point_of, Res, Session};

fn curves_of(p: &Value) -> Result<Vec<Curve>, String> {
    p["curves"].as_array().ok_or("missing curves")?.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad curve: {e}"))).collect()
}

fn paint_name(p: Paint) -> &'static str { if p == Paint::Yellow { "Yellow" } else { "White" } }

impl Session {
    pub(crate) fn handle_traffic(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // Line Marking: { level_id, curves }
            "add_line_marking" => (|| Ok(json!({ "id": self.doc.add_line_marking(id_of(p, "level_id")?, curves_of(p)?)? })))(),
            // Kerb: { level_id, curves, height_mm?, width_mm? }
            "add_kerb" => (|| Ok(json!({ "id": self.doc.add_kerb(id_of(p, "level_id")?, curves_of(p)?, p["height_mm"].as_f64().unwrap_or(150.0), p["width_mm"].as_f64().unwrap_or(150.0))? })))(),
            // Parking Bay: { level_id, start, end, left?, angle_deg?, bay_width_mm?, bay_length_mm?, wheel_stops?, paint? }
            "add_parking_row" => (|| {
                let mut row = ParkingRow { level: id_of(p, "level_id")?, host: None, start: point_of(&p["start"])?, end: point_of(&p["end"])?, left: p["left"].as_bool().unwrap_or(true),
                    angle_deg: p["angle_deg"].as_f64().unwrap_or(90.0), bay_width_mm: p["bay_width_mm"].as_f64().unwrap_or(2500.0), bay_length_mm: p["bay_length_mm"].as_f64().unwrap_or(5400.0),
                    line_width_mm: 80.0, paint: Paint::White, wheel_stops: p["wheel_stops"].as_bool().unwrap_or(false), wheel_stop_offset_mm: p["wheel_stop_offset_mm"].as_f64().unwrap_or(900.0), mark: String::new(), pinned: false, accessible: p["accessible"].as_bool().unwrap_or(false) };
                if p["paint"].as_str() == Some("yellow") { row.paint = Paint::Yellow; }
                Ok(json!({ "id": self.doc.add_parking_row(row)? }))
            })(),
            // Kerb Ramp: { level_id, top (at the kerb), bottom, width_mm?, rise_mm? }
            "add_kerb_ramp" => (|| Ok(json!({ "id": self.doc.add_kerb_ramp(id_of(p, "level_id")?, point_of(&p["top"])?, point_of(&p["bottom"])?, p["width_mm"].as_f64().unwrap_or(1200.0), p["rise_mm"].as_f64().unwrap_or(150.0))? })))(),
            // Edit Path: { id } gives the path to sketch; set_traffic_path { id, curves }
            "traffic_path" => (|| {
                let curves = match self.doc.get(id_of(p, "id")?) { Some(Element::LineMarking(m)) => m.curves.clone(), Some(Element::Kerb(k)) => k.curves.clone(), _ => return Err("not a line marking or kerb".into()) };
                Ok(json!({ "curves": curves.iter().map(crate::describe_curve).collect::<Vec<_>>() }))
            })(),
            "set_traffic_path" => (|| { self.doc.set_traffic_path(id_of(p, "id")?, curves_of(p)?)?; Ok(json!({})) })(),
            "patch_traffic" => (|| { self.doc.patch_traffic(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            _ => return None,
        })
    }

    pub(crate) fn traffic_props(&self, id: Id, e: &Element) -> Option<Value> {
        let d = &self.doc;
        let num = |label: &str, path: &str, v: f64| json!({ "label": label, "path": path, "kind": "number", "raw": v, "value": format!("{v}") });
        let boolean = |label: &str, path: &str, v: bool| json!({ "label": label, "path": path, "kind": "bool", "raw": v, "value": if v { "Yes" } else { "No" } });
        let ro = |label: &str, v: String| json!({ "label": label, "kind": "ro", "value": v });
        let paint = |v: Paint| json!({ "label": "Paint", "kind": "select", "value": paint_name(v), "options": [
            { "label": "White", "patch": { "paint": "white" }, "selected": v == Paint::White }, { "label": "Yellow", "patch": { "paint": "yellow" }, "selected": v == Paint::Yellow },
        ] });
        let level = |l: Id| ro("Level", d.level(l).map_or(String::new(), |l| l.name.clone()));
        let host = |h: Option<Id>| ro("On", match h.and_then(|h| d.get(h)) { Some(Element::Toposolid(t)) if t.host.is_some() => "Sub-division".into(), Some(Element::Toposolid(_)) => "Toposolid".into(), Some(Element::Floor(_)) => "Floor".into(), _ => "Level".into() });
        let pinned = |v: bool| boolean("Pinned", "pinned", v);
        let (kind, cat, title, groups) = match e {
            Element::LineMarking(m) => ("LineMarking", "Parking", "Line Marking", json!([
                { "name": "Constraints", "rows": [level(m.level), host(m.host), pinned(m.pinned)] },
                { "name": "Graphics", "rows": [num("Width", "width_mm", m.width_mm), paint(m.paint), boolean("Dashed", "dashed", m.dashed), num("Dash Length", "dash_mm", m.dash_mm), num("Gap", "gap_mm", m.gap_mm)] },
                { "name": "Dimensions", "rows": [ro("Length", format!("{:.0}", m.curves.iter().map(|c| c.length()).sum::<f64>()))] },
            ])),
            Element::ParkingRow(r) => {
                let bays = r.bays().len();
                ("ParkingRow", "Parking", "Parking Bays", json!([
                    { "name": "Constraints", "rows": [level(r.level), host(r.host), pinned(r.pinned)] },
                    { "name": "Layout", "rows": [
                        num("Angle", "angle_deg", r.angle_deg), num("Bay Width", "bay_width_mm", r.bay_width_mm), num("Bay Length", "bay_length_mm", r.bay_length_mm),
                        boolean("Bays on the Left", "left", r.left), boolean("Accessible", "accessible", r.accessible), ro("Bays", bays.to_string()),
                    ] },
                    { "name": "Marking", "rows": [num("Line Width", "line_width_mm", r.line_width_mm), paint(r.paint)] },
                    { "name": "Wheel Stops", "rows": [boolean("Wheel Stops", "wheel_stops", r.wheel_stops), num("Wheel Stop Offset", "wheel_stop_offset_mm", r.wheel_stop_offset_mm)] },
                    { "name": "Identity Data", "rows": [{ "label": "Mark", "path": "mark", "kind": "text", "value": r.mark }] },
                ]))
            }
            Element::Kerb(k) => ("Kerb", "Hardscape", "Kerb", json!([
                { "name": "Constraints", "rows": [level(k.level), host(k.host), pinned(k.pinned)] },
                { "name": "Dimensions", "rows": [num("Height", "height_mm", k.height_mm), num("Width", "width_mm", k.width_mm), ro("Length", format!("{:.0}", k.curves.iter().map(|c| c.length()).sum::<f64>()))] },
            ])),
            Element::KerbRamp(k) => {
                // AS 1428.1: no steeper than 1:8, rising no more than 190, no longer than 1520
                let warnings: Vec<Value> = k.warnings().into_iter().map(|m| json!({ "label": "Warning", "value": m, "kind": "ro", "warning": true })).collect();
                let mut rules = warnings;
                if rules.is_empty() { rules.push(ro("AS 1428.1 Kerb Ramp", "Met".into())); }
                let grade = if k.rise_mm > 0.0 { format!("1:{:.1}", k.length() / k.rise_mm) } else { "-".into() };
                ("KerbRamp", "Hardscape", "Kerb Ramp", json!([
                    { "name": "Constraints", "rows": [level(k.level), host(k.host), pinned(k.pinned)] },
                    { "name": "Dimensions", "rows": [num("Width", "width_mm", k.width_mm), num("Rise", "rise_mm", k.rise_mm), ro("Length", format!("{:.0}", k.length())), ro("Grade", grade)] },
                    { "name": "Rules", "rows": rules },
                ]))
            }
            _ => return None,
        };
        Some(json!({ "id": id, "kind": kind, "category": cat, "title": title, "type_label": title, "target": { "method": "patch_traffic", "id": id }, "groups": groups }))
    }
}

/// What a floor plan draws for traffic items on its level: line markings and parking rows (lines and wheel stops)
/// in the Parking style, kerbs (both faces) in the Hardscape style.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if v.view_kind != ViewKind::FloorPlan { return json!({ "traffic": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    let on_level = |l: Id| v.level == Some(l);
    let pts = |q: &[openarch_model::Point]| q.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
    let mut out: Vec<Value> = vec![];
    let style = |cat: &str, id: Id| style_for(d, &settings, cat, None, d.graphics_of(v, &id).as_ref(), "projection", &Hint::default());
    for (id, m) in d.line_markings() {
        if !on_level(m.level) || v.hidden_elements.contains(&id) { continue; }
        let Some(st) = style("Parking", id) else { continue };
        out.push(json!({ "id": id, "kind": "linemarking", "lines": m.pieces().iter().map(|q| pts(q)).collect::<Vec<_>>(), "width": m.width_mm, "paint": paint_name(m.paint), "pinned": m.pinned, "style": st }));
    }
    for (id, r) in d.parking_rows() {
        if !on_level(r.level) || v.hidden_elements.contains(&id) { continue; }
        let Some(st) = style("Parking", id) else { continue };
        out.push(json!({ "id": id, "kind": "parkingrow", "lines": r.lines().iter().map(|l| pts(l)).collect::<Vec<_>>(), "width": r.line_width_mm, "paint": paint_name(r.line_paint()),
            "symbols": r.access_symbols().iter().map(|c| pts(c)).collect::<Vec<_>>(),
            "stops": r.wheel_stops().iter().map(|c| pts(c)).collect::<Vec<_>>(), "bays": r.bays().iter().map(|c| pts(c)).collect::<Vec<_>>(), "start": [r.start.x, r.start.y], "end": [r.end.x, r.end.y], "pinned": r.pinned, "style": st }));
    }
    for (id, k) in d.kerb_ramps() {
        if !on_level(k.level) || v.hidden_elements.contains(&id) { continue; }
        let Some(st) = style("Hardscape", id) else { continue };
        let o = k.outline();
        out.push(json!({ "id": id, "kind": "kerbramp", "outline": pts(&o), "lines": [pts(&[o[0], o[1], o[2], o[3], o[0]])], "top": [k.top.x, k.top.y], "bottom": [k.bottom.x, k.bottom.y],
            "grade": if k.rise_mm > 0.0 { format!("1:{:.0}", k.length() / k.rise_mm) } else { String::new() }, "ok": k.warnings().is_empty(), "pinned": k.pinned, "style": st }));
    }
    for (id, k) in d.kerbs() {
        if !on_level(k.level) || v.hidden_elements.contains(&id) { continue; }
        let Some(st) = style("Hardscape", id) else { continue };
        let mut path: Vec<openarch_model::Point> = vec![];
        for c in &k.curves { let q = c.tessellate(20.0); path.extend(q.into_iter().skip(if path.is_empty() { 0 } else { 1 })); }
        // both faces: the path moved out half the width each side
        let side = |s: f64| -> Vec<[f64; 2]> {
            path.iter().enumerate().map(|(i, p)| {
                let (a, b) = if i + 1 < path.len() { (path[i], path[i + 1]) } else { (path[i - 1], path[i]) };
                let l = a.dist(&b).max(1e-9);
                [p.x - (b.y - a.y) / l * s * k.width_mm / 2.0, p.y + (b.x - a.x) / l * s * k.width_mm / 2.0]
            }).collect()
        };
        if path.len() < 2 { continue; }
        out.push(json!({ "id": id, "kind": "kerb", "lines": [side(1.0), side(-1.0)], "path": pts(&path), "width": k.width_mm, "pinned": k.pinned, "style": st }));
    }
    json!({ "traffic": out })
}

/// The 3D meshes of traffic items whose category shows.
pub(crate) fn meshes(d: &Document, shown: &dyn Fn(&str) -> bool, hidden: &std::collections::BTreeSet<Id>) -> Vec<Value> {
    openarch_model::mesh::traffic_meshes(d).into_iter().filter(|(id, kind, _, v)| !v.is_empty() && !hidden.contains(id) && shown(if *kind == "Kerb" || *kind == "KerbRamp" { "Hardscape" } else { "Parking" }))
        .map(|(id, kind, color, verts)| json!({ "id": id, "kind": kind, "color": color, "verts": verts })).collect()
}
