//! Structural columns and beams: placing them, their Properties and types, and what a floor plan and a 3D view
//! draw. Elevations and sections draw them from their solids (the model's elevation items).
use serde_json::{json, Value};

use openarch_model::structural::{Beam, FramingUsage, Section, StructuralColumn, StructuralType, StructuralUse, YJustification, ZJustification};
use openarch_model::view::{PlanDisplay, ViewKind};
use openarch_model::{Document, Element, Id};

use crate::graphics::{style_for, Hint};
use crate::{id_of, point_of, Res, Session};

fn num(label: &str, path: &str, v: f64) -> Value {
    json!({ "label": label, "path": path, "kind": "number", "raw": v, "value": format!("{v}") })
}
fn ro(label: &str, v: String) -> Value {
    json!({ "label": label, "kind": "ro", "value": v })
}
fn ro_num(label: &str, v: f64, text: String) -> Value {
    json!({ "label": label, "kind": "ro", "value": text, "raw": v })
}
fn text(label: &str, path: &str, v: &str) -> Value {
    json!({ "label": label, "path": path, "kind": "text", "value": v })
}
fn boolean(label: &str, path: &str, v: bool) -> Value {
    json!({ "label": label, "path": path, "kind": "bool", "raw": v, "value": if v { "Yes" } else { "No" } })
}
fn select(label: &str, value: &str, options: Vec<Value>) -> Value {
    json!({ "label": label, "kind": "select", "value": value, "options": options })
}

fn type_label(t: &StructuralType) -> String {
    format!("{}: {}", t.family, t.name)
}

/// A section's sizes as rows of the type's properties, each patching the whole section.
fn section_rows(s: &Section) -> Vec<Value> {
    let v = serde_json::to_value(s).unwrap_or_default();
    let labels: &[(&str, &str)] = match s {
        Section::Rectangle { .. } => &[("b", "Width"), ("d", "Depth")],
        Section::Round { .. } => &[("d", "Diameter")],
        Section::I { .. } | Section::Channel { .. } => &[("d", "Depth"), ("bf", "Flange Width"), ("tf", "Flange Thickness"), ("tw", "Web Thickness")],
        Section::RectangularHollow { .. } => &[("b", "Width"), ("d", "Depth"), ("t", "Wall Thickness")],
        Section::CircularHollow { .. } => &[("d", "Diameter"), ("t", "Wall Thickness")],
    };
    labels.iter().map(|(k, label)| num(label, &format!("section.{k}"), v[*k].as_f64().unwrap_or(0.0))).collect()
}

fn shape_name(s: &Section) -> &'static str {
    match s {
        Section::Rectangle { .. } => "Rectangle",
        Section::Round { .. } => "Round",
        Section::I { .. } => "I Section",
        Section::Channel { .. } => "Channel",
        Section::RectangularHollow { .. } => "Rectangular Hollow Section",
        Section::CircularHollow { .. } => "Circular Hollow Section",
    }
}

impl Session {
    pub(crate) fn handle_structural(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "ensure_structural_types" => (|| { self.doc.ensure_structural_types()?; Ok(json!({})) })(),
            // { type_id, level_id, at, top_level_id?, height_mm?, base_offset_mm?, top_offset_mm?, rotation_deg? }
            "add_structural_column" => (|| {
                let c = StructuralColumn {
                    structural_type: id_of(p, "type_id")?, level: id_of(p, "level_id")?, at: point_of(&p["at"])?,
                    base_offset_mm: p["base_offset_mm"].as_f64().unwrap_or(0.0), top_level: p["top_level_id"].as_u64(), top_offset_mm: p["top_offset_mm"].as_f64().unwrap_or(0.0),
                    height_mm: p["height_mm"].as_f64().unwrap_or(3000.0), rotation_deg: p["rotation_deg"].as_f64().unwrap_or(0.0), mark: String::new(), comments: String::new(), pinned: false,
                };
                Ok(json!({ "id": self.doc.add_structural_column(c)? }))
            })(),
            // { type_id, level_id, start, end, start_offset_mm?, end_offset_mm?, z_justification?, y_justification?, z_offset_mm?, y_offset_mm?, rotation_deg?, usage? }
            "add_beam" => (|| {
                let pick = |k: &str| p.get(k).filter(|v| !v.is_null()).cloned();
                let b = Beam {
                    structural_type: id_of(p, "type_id")?, level: id_of(p, "level_id")?, start: point_of(&p["start"])?, end: point_of(&p["end"])?,
                    start_offset_mm: p["start_offset_mm"].as_f64().unwrap_or(0.0), end_offset_mm: p["end_offset_mm"].as_f64().unwrap_or(0.0),
                    z_justification: match pick("z_justification") { Some(v) => serde_json::from_value(v).map_err(|e| format!("bad z justification: {e}"))?, None => ZJustification::Top },
                    y_justification: match pick("y_justification") { Some(v) => serde_json::from_value(v).map_err(|e| format!("bad y justification: {e}"))?, None => YJustification::Center },
                    z_offset_mm: p["z_offset_mm"].as_f64().unwrap_or(0.0), y_offset_mm: p["y_offset_mm"].as_f64().unwrap_or(0.0), rotation_deg: p["rotation_deg"].as_f64().unwrap_or(0.0),
                    usage: match pick("usage") { Some(v) => serde_json::from_value(v).map_err(|e| format!("bad usage: {e}"))?, None => FramingUsage::Joist },
                    mark: String::new(), comments: String::new(), pinned: false,
                };
                Ok(json!({ "id": self.doc.add_beam(b)? }))
            })(),
            // a column, a beam or a structural type: { id, patch }
            "patch_structural" => (|| { self.doc.patch_structural(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "duplicate_structural_type" => (|| Ok(json!({ "id": self.doc.duplicate_structural_type(id_of(p, "id")?, p["name"].as_str().unwrap_or(""))? })))(),
            _ => return None,
        })
    }

    /// The types a column or a beam can be, as the options of its Type row.
    fn structural_type_row(&self, current: Id, used_for: StructuralUse) -> Value {
        let d = &self.doc;
        let options: Vec<Value> = d.structural_types().into_iter().filter(|(_, t)| t.used_for == used_for)
            .map(|(id, t)| json!({ "label": type_label(t), "patch": { "structural_type": id }, "selected": id == current })).collect();
        select("Type", &d.structural_type(current).map(type_label).unwrap_or_default(), options)
    }

    pub(crate) fn structural_props(&self, id: Id, e: &Element) -> Option<Value> {
        let d = &self.doc;
        let (length, volume) = d.structural_measures(id).unwrap_or((0.0, 0.0));
        let measures = vec![ro_num("Length", length, format!("{length:.0}")), ro_num("Volume", volume / 1e9, format!("{:.3} m\u{b3}", volume / 1e9))];
        let identity = |mark: &str, comments: &str| json!({ "name": "Identity Data", "rows": [text("Comments", "comments", comments), text("Mark", "mark", mark)] });
        let (kind, cat, noun, type_id, groups) = match e {
            Element::StructuralColumn(c) => {
                let free = !c.pinned;
                let mut top: Vec<(String, Value, bool)> = vec![("Unconnected".into(), json!({ "top_level": null }), c.top_level.is_none())];
                top.extend(d.levels().iter().map(|(lid, l)| (l.name.clone(), json!({ "top_level": lid }), c.top_level == Some(*lid))));
                let top_now = c.top_level.and_then(|l| d.level(l)).map_or("Unconnected".to_string(), |l| l.name.clone());
                let mut rows = vec![
                    crate::props::level_select(d, "Base Level", "level", c.level, free),
                    num("Base Offset", "base_offset_mm", c.base_offset_mm),
                    select("Top Level", &top_now, top.into_iter().map(|(label, patch, selected)| json!({ "label": label, "patch": patch, "selected": selected })).collect()),
                ];
                rows.push(if c.top_level.is_some() { num("Top Offset", "top_offset_mm", c.top_offset_mm) } else { num("Unconnected Height", "height_mm", c.height_mm) });
                rows.push(num("Rotation", "rotation_deg", c.rotation_deg));
                rows.push(boolean("Pinned", "pinned", c.pinned));
                ("StructuralColumn", "Structural Columns", "Structural Column", c.structural_type, vec![
                    json!({ "name": "Constraints", "rows": rows }),
                    json!({ "name": "Type", "rows": [self.structural_type_row(c.structural_type, StructuralUse::Column)] }),
                    json!({ "name": "Dimensions", "rows": measures }),
                    identity(&c.mark, &c.comments),
                ])
            }
            Element::Beam(b) => {
                let free = !b.pinned;
                let opt = |label: &str, key: &str, value: &str, now: bool| json!({ "label": label, "patch": { key: value }, "selected": now });
                let zj = b.z_justification;
                let yj = b.y_justification;
                ("Beam", "Structural Framing", "Structural Framing", b.structural_type, vec![
                    json!({ "name": "Constraints", "rows": [
                        crate::props::level_select(d, "Reference Level", "level", b.level, free),
                        num("Start Level Offset", "start_offset_mm", b.start_offset_mm),
                        num("End Level Offset", "end_offset_mm", b.end_offset_mm),
                        num("Cross-Section Rotation", "rotation_deg", b.rotation_deg),
                        boolean("Pinned", "pinned", b.pinned),
                    ] }),
                    json!({ "name": "Geometric Position", "rows": [
                        select("z Justification", match zj { ZJustification::Top => "Top", ZJustification::Center => "Center", ZJustification::Bottom => "Bottom" }, vec![
                            opt("Top", "z_justification", "top", zj == ZJustification::Top), opt("Center", "z_justification", "center", zj == ZJustification::Center), opt("Bottom", "z_justification", "bottom", zj == ZJustification::Bottom)]),
                        num("z Offset Value", "z_offset_mm", b.z_offset_mm),
                        select("y Justification", match yj { YJustification::Left => "Left", YJustification::Center => "Center", YJustification::Right => "Right" }, vec![
                            opt("Left", "y_justification", "left", yj == YJustification::Left), opt("Center", "y_justification", "center", yj == YJustification::Center), opt("Right", "y_justification", "right", yj == YJustification::Right)]),
                        num("y Offset Value", "y_offset_mm", b.y_offset_mm),
                    ] }),
                    json!({ "name": "Type", "rows": [self.structural_type_row(b.structural_type, StructuralUse::Framing)] }),
                    json!({ "name": "Structural", "rows": [select("Structural Usage", b.usage.label(), FramingUsage::ALL.iter().map(|u| json!({ "label": u.label(), "patch": { "usage": u }, "selected": *u == b.usage })).collect())] }),
                    json!({ "name": "Dimensions", "rows": measures }),
                    identity(&b.mark, &b.comments),
                ])
            }
            _ => return None,
        };
        let t = d.structural_type(type_id);
        Some(json!({ "id": id, "kind": kind, "category": cat, "type_id": type_id, "title": format!("{noun}: {}", t.map(type_label).unwrap_or_default()),
            "type_label": t.map(type_label).unwrap_or_default(), "target": { "method": "patch_structural", "id": id }, "groups": groups }))
    }

    /// A structural type's properties: its section and material, and how many members use it.
    pub(crate) fn structural_type_props(&self, id: Id, t: &StructuralType) -> Value {
        let d = &self.doc;
        let used = d.iter().filter(|(_, e)| e.type_id() == Some(id) && matches!(e, Element::StructuralColumn(_) | Element::Beam(_))).count();
        let (w, depth) = t.section.size();
        json!({ "id": id, "kind": "StructuralType", "family": t.family, "name": t.name, "title": type_label(t),
            "category": if t.used_for == StructuralUse::Column { "Structural Columns" } else { "Structural Framing" },
            "target": { "method": "patch_structural", "id": id },
            "groups": [
                { "name": "Structural", "rows": [ro("Section Shape", shape_name(&t.section).into()), text("Structural Material", "material", &t.material)] },
                { "name": "Dimensions", "rows": section_rows(&t.section).into_iter().chain([ro_num("Section Area", t.section.area_mm2(), format!("{:.0} mm\u{b2}", t.section.area_mm2())), ro("Overall Size", format!("{w} x {depth}"))]).collect::<Vec<_>>() },
                { "name": "Identity Data", "rows": [text("Type Name", "name", &t.name), ro("Family", t.family.clone()), ro("In Use", format!("{used}"))] },
            ] })
    }
}

/// The structural types, for doc_info: what the Column and Beam tools offer.
pub(crate) fn types(d: &Document) -> Vec<Value> {
    d.structural_types().into_iter().map(|(id, t)| { let (w, depth) = t.section.size(); json!({ "id": id, "family": t.family, "name": t.name, "label": type_label(t), "used_for": t.used_for, "width": w, "depth": depth }) }).collect()
}

/// What a floor plan draws for columns and beams: a column's section (cut where the cut plane passes through it),
/// a beam's width along its line. A beam that hangs just under its own level's plan range still shows there, as
/// framing plans do.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if v.view_kind != ViewKind::FloorPlan { return json!({ "structure": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    let Some(range) = d.resolve_view_range(v, &settings) else { return json!({ "structure": [] }) };
    let pts = |q: &[openarch_model::Point]| q.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
    let shown = |dp: PlanDisplay| match dp { PlanDisplay::Cut => Some("cut"), PlanDisplay::Projection => Some("projection"), PlanDisplay::Beyond => Some("beyond"), PlanDisplay::Hidden => None };
    let mut out: Vec<Value> = vec![];
    for (id, c) in d.structural_columns() {
        if v.hidden_elements.contains(&id) { continue; }
        let (base, top) = d.column_z(c);
        let Some(display) = shown(range.classify(base, top)) else { continue };
        let Some(t) = d.structural_type(c.structural_type) else { continue };
        let hint = d.material(&t.material).map(Hint::from_material).unwrap_or_default();
        let Some(style) = style_for(d, &settings, "Structural Columns", None, d.graphics_of(v, &id).as_ref(), display, &hint) else { continue };
        let Some((outer, inner)) = d.column_plan(c) else { continue };
        out.push(json!({ "id": id, "kind": "column", "outline": pts(&outer), "inner": inner.map(|r| pts(&r)), "at": [c.at.x, c.at.y], "display": display, "pinned": c.pinned, "style": style }));
    }
    for (id, b) in d.beams() {
        if v.hidden_elements.contains(&id) { continue; }
        let Some(t) = d.structural_type(b.structural_type) else { continue };
        let (p0, p1) = d.beam_ends(b);
        let depth = t.section.size().1.max(t.section.size().0);
        let (lo, hi) = (p0[2].min(p1[2]) - depth, p0[2].max(p1[2]) + depth);
        let display = match shown(range.classify(lo, hi)) {
            Some(x) => x,
            // under its own level, below the plan's range: still drawn in that level's plan
            None if v.level == Some(b.level) && hi < range.cut => "projection",
            None => continue,
        };
        let hint = d.material(&t.material).map(Hint::from_material).unwrap_or_default();
        let Some(style) = style_for(d, &settings, "Structural Framing", None, d.graphics_of(v, &id).as_ref(), display, &hint) else { continue };
        let Some(outline) = d.beam_plan(b) else { continue };
        out.push(json!({ "id": id, "kind": "beam", "outline": pts(&outline), "start": [b.start.x, b.start.y], "end": [b.end.x, b.end.y], "display": display, "pinned": b.pinned, "style": style }));
    }
    json!({ "structure": out })
}

/// The 3D solids of the columns and beams whose category shows.
pub(crate) fn meshes(d: &Document, shown: &dyn Fn(&str) -> bool, hidden: &std::collections::BTreeSet<Id>) -> Vec<Value> {
    d.structural_meshes().into_iter().filter(|(id, kind, _, v)| !v.is_empty() && !hidden.contains(id) && shown(if *kind == "Beam" { "Structural Framing" } else { "Structural Columns" }))
        .map(|(id, kind, color, verts)| json!({ "id": id, "kind": kind, "color": color, "verts": verts })).collect()
}
