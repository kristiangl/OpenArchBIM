//! Structural columns and beams: placing them, their Properties and types, and what a floor plan and a 3D view
//! draw. Elevations and sections draw them from their solids (the model's elevation items).
use serde_json::{json, Value};

use openarch_model::framing::{BeamSystem, LayoutRule, SystemJustification, Truss, TrussPart, TrussProfile, TrussWeb};
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
            // { type_id, level_id, at, top_level_id?, height_mm?, base_offset_mm?, top_offset_mm?, rotation_deg?, top_at?, top_attach_id? }
            "add_structural_column" => (|| {
                let c = StructuralColumn {
                    structural_type: id_of(p, "type_id")?, level: id_of(p, "level_id")?, at: point_of(&p["at"])?,
                    base_offset_mm: p["base_offset_mm"].as_f64().unwrap_or(0.0), top_level: p["top_level_id"].as_u64(), top_offset_mm: p["top_offset_mm"].as_f64().unwrap_or(0.0),
                    height_mm: p["height_mm"].as_f64().unwrap_or(3000.0), rotation_deg: p["rotation_deg"].as_f64().unwrap_or(0.0),
                    top_at: match p.get("top_at").filter(|v| !v.is_null()) { Some(v) => Some(point_of(v)?), None => None }, top_attach: p["top_attach_id"].as_u64(),
                    mark: String::new(), comments: String::new(), pinned: false,
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
                    ..Beam::new(0, 0, openarch_model::Point::new(0.0, 0.0), openarch_model::Point::new(0.0, 0.0))
                };
                Ok(json!({ "id": self.doc.add_beam(b)? }))
            })(),
            // a column, a beam, a beam system, a truss or a structural type: { id, patch }
            "patch_structural" => (|| {
                let (id, patch) = (id_of(p, "id")?, p.get("patch").ok_or("missing patch")?);
                if matches!(self.doc.get(id), Some(Element::BeamSystem(_) | Element::Truss(_))) { self.doc.patch_framing(id, patch)?; } else { self.doc.patch_structural(id, patch)?; }
                Ok(json!({}))
            })(),
            // { type_id, level_id, boundary: [[x, y], ...], direction_deg?, rule?, spacing_mm?, count?, justification?, offset_mm?, usage? }
            "add_beam_system" => (|| {
                let mut v = p.clone();
                let o = v.as_object_mut().ok_or("missing parameters")?;
                o.insert("structural_type".into(), json!(id_of(p, "type_id")?));
                o.insert("level".into(), json!(id_of(p, "level_id")?));
                let ring = p["boundary"].as_array().ok_or("missing boundary")?.iter().map(point_of).collect::<Result<Vec<_>, _>>()?;
                o.insert("boundary".into(), json!(ring));
                let s: BeamSystem = serde_json::from_value(v).map_err(|e| format!("bad beam system: {e}"))?;
                Ok(json!({ "id": self.doc.add_beam_system(s)? }))
            })(),
            // { level_id, start, end, top_chord_id, bottom_chord_id, web_type_id, height_mm?, offset_mm?, profile?, web?, panels? }
            "add_truss" => (|| {
                let mut v = p.clone();
                let o = v.as_object_mut().ok_or("missing parameters")?;
                o.insert("level".into(), json!(id_of(p, "level_id")?));
                o.insert("start".into(), json!(point_of(&p["start"])?));
                o.insert("end".into(), json!(point_of(&p["end"])?));
                for (k, from) in [("top_chord", "top_chord_id"), ("bottom_chord", "bottom_chord_id"), ("web_type", "web_type_id")] { o.insert(k.into(), json!(id_of(p, from)?)); }
                let t: Truss = serde_json::from_value(v).map_err(|e| format!("bad truss: {e}"))?;
                Ok(json!({ "id": self.doc.add_truss(t)? }))
            })(),
            "drop_truss" => (|| Ok(json!({ "ids": self.doc.drop_truss(id_of(p, "id")?)? })))(),
            // a column at every crossing of the grids (all of them with none given) that has none yet:
            // { type_id, level_id, grid_ids?, top_level_id?, height_mm?, base_offset_mm?, top_offset_mm?, rotation_deg? }
            "columns_at_grids" => (|| {
                let like = StructuralColumn {
                    base_offset_mm: p["base_offset_mm"].as_f64().unwrap_or(0.0), top_level: p["top_level_id"].as_u64(), top_offset_mm: p["top_offset_mm"].as_f64().unwrap_or(0.0),
                    height_mm: p["height_mm"].as_f64().unwrap_or(3000.0), rotation_deg: p["rotation_deg"].as_f64().unwrap_or(0.0),
                    ..StructuralColumn::new(id_of(p, "type_id")?, id_of(p, "level_id")?, openarch_model::Point::new(0.0, 0.0))
                };
                let grids: Vec<Id> = p["grid_ids"].as_array().map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
                Ok(json!({ "ids": self.doc.add_columns_at_grids(like, &grids)? }))
            })(),
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
                // what its top can attach to: the roofs and floors over it
                let host_label = |h: Id| match d.get(h) {
                    Some(Element::Roof(r)) => format!("Roof: {}", d.roof_type(r.roof_type).map_or("", |t| t.name.as_str())),
                    Some(Element::Floor(f)) => format!("Floor: {}", d.floor_type(f.floor_type).map_or("", |t| t.name.as_str())),
                    _ => "Nothing over it".into(),
                };
                let mut hosts = vec![json!({ "label": "None", "patch": { "top_attach": null }, "selected": c.top_attach.is_none() })];
                hosts.extend(d.column_hosts(c).into_iter().map(|(h, z)| json!({ "label": format!("{} (underside {z:.0})", host_label(h)), "patch": { "top_attach": h }, "selected": c.top_attach == Some(h) })));
                rows.push(select("Top Attached To", &c.top_attach.map_or("None".to_string(), host_label), hosts));
                rows.push(select("Column Style", if c.top_at.is_some() { "Slanted" } else { "Vertical" }, vec![
                    json!({ "label": "Vertical", "patch": { "top_at": null }, "selected": c.top_at.is_none() }),
                    json!({ "label": "Slanted", "patch": { "top_at": c.top_at.unwrap_or(openarch_model::Point::new(c.at.x + 500.0, c.at.y)) }, "selected": c.top_at.is_some() }),
                ]));
                if let Some(t) = c.top_at {
                    rows.push(num("Top X", "top_at.x", t.x));
                    rows.push(num("Top Y", "top_at.y", t.y));
                }
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
                    json!({ "name": "Structural", "rows": [
                        select("Structural Usage", b.usage.label(), FramingUsage::ALL.iter().map(|u| json!({ "label": u.label(), "patch": { "usage": u }, "selected": *u == b.usage })).collect()),
                        boolean("Start Join", "start_join", b.start_join),
                        boolean("End Join", "end_join", b.end_join),
                        num("Start Extension", "start_extension_mm", b.start_extension_mm),
                        num("End Extension", "end_extension_mm", b.end_extension_mm),
                        num("Cutback", "cutback_mm", b.cutback_mm.unwrap_or_else(|| d.structural_type(b.structural_type).map_or(0.0, |t| openarch_model::structural::default_cutback(&t.material)))),
                    ] }),
                    json!({ "name": "Dimensions", "rows": measures.iter().cloned().chain(d.beam_cut_length(id).map(|l| ro_num("Cut Length", l, format!("{l:.0}")))).collect::<Vec<_>>() }),
                    identity(&b.mark, &b.comments),
                ])
            }
            _ => return None,
        };
        let t = d.structural_type(type_id);
        Some(json!({ "id": id, "kind": kind, "category": cat, "type_id": type_id, "title": format!("{noun}: {}", t.map(type_label).unwrap_or_default()),
            "type_label": t.map(type_label).unwrap_or_default(), "target": { "method": "patch_structural", "id": id }, "groups": groups }))
    }

    /// A beam system's or a truss's Properties.
    pub(crate) fn framing_props(&self, id: Id, e: &Element) -> Option<Value> {
        let d = &self.doc;
        let identity = |mark: &str, comments: &str| json!({ "name": "Identity Data", "rows": [text("Comments", "comments", comments), text("Mark", "mark", mark)] });
        let framing = |label: &str, key: &str, current: Id| {
            let options: Vec<Value> = d.structural_types().into_iter().filter(|(_, t)| t.used_for == StructuralUse::Framing).map(|(tid, t)| json!({ "label": type_label(t), "patch": { key: tid }, "selected": tid == current })).collect();
            select(label, &d.structural_type(current).map(type_label).unwrap_or_default(), options)
        };
        let members: Vec<_> = d.structural_members().into_iter().filter(|m| m.0 == id).collect();
        let total: f64 = members.iter().map(|m| m.3.length).sum();
        let volume: f64 = members.iter().map(|m| m.3.length * m.2.section.area_mm2()).sum();
        let measures = |count_label: &str| vec![ro_num(count_label, members.len() as f64, format!("{}", members.len())), ro_num("Total Length", total, format!("{total:.0}")), ro_num("Volume", volume / 1e9, format!("{:.3} m\u{b3}", volume / 1e9))];
        let (kind, title, type_id, groups) = match e {
            Element::BeamSystem(s) => {
                let opt = |label: &str, key: &str, value: Value, now: bool| json!({ "label": label, "patch": { key: value }, "selected": now });
                let spaced = s.rule != LayoutRule::FixedNumber;
                let mut pattern = vec![select("Layout Rule", s.rule.label(), LayoutRule::ALL.iter().map(|r| opt(r.label(), "rule", json!(r), *r == s.rule)).collect())];
                pattern.push(if spaced { num(if s.rule == LayoutRule::ClearSpacing { "Clear Spacing" } else if s.rule == LayoutRule::MaximumSpacing { "Maximum Spacing" } else { "Fixed Spacing" }, "spacing_mm", s.spacing_mm) } else { num("Number of Lines", "count", s.count as f64) });
                if matches!(s.rule, LayoutRule::FixedDistance | LayoutRule::ClearSpacing) {
                    let j = s.justification;
                    pattern.push(select("Justification", match j { SystemJustification::Beginning => "Beginning", SystemJustification::Center => "Center", SystemJustification::End => "End" }, vec![
                        opt("Beginning", "justification", json!("beginning"), j == SystemJustification::Beginning), opt("Center", "justification", json!("center"), j == SystemJustification::Center), opt("End", "justification", json!("end"), j == SystemJustification::End)]));
                }
                pattern.push(num("Beam Direction", "direction_deg", s.direction_deg));
                pattern.push(framing("Beam Type", "structural_type", s.structural_type));
                ("BeamSystem", "Structural Beam System".to_string(), Some(s.structural_type), vec![
                    json!({ "name": "Constraints", "rows": [crate::props::level_select(d, "Level", "level", s.level, !s.pinned), num("Elevation", "offset_mm", s.offset_mm), boolean("Pinned", "pinned", s.pinned)] }),
                    json!({ "name": "Pattern", "rows": pattern }),
                    json!({ "name": "Structural", "rows": [select("Structural Usage", s.usage.label(), FramingUsage::ALL.iter().map(|u| json!({ "label": u.label(), "patch": { "usage": u }, "selected": *u == s.usage })).collect())] }),
                    json!({ "name": "Dimensions", "rows": measures("Number of Beams") }),
                    identity(&s.mark, &s.comments),
                ])
            }
            Element::Truss(t) => {
                let opt = |label: &str, key: &str, value: &str, now: bool| json!({ "label": label, "patch": { key: value }, "selected": now });
                ("Truss", "Structural Truss".to_string(), None, vec![
                    json!({ "name": "Constraints", "rows": [crate::props::level_select(d, "Reference Level", "level", t.level, !t.pinned), num("Bearing Offset", "offset_mm", t.offset_mm), boolean("Pinned", "pinned", t.pinned)] }),
                    json!({ "name": "Layout", "rows": [
                        select("Profile", if t.profile == TrussProfile::Parallel { "Parallel Chords" } else { "Pitched" }, vec![opt("Parallel Chords", "profile", "parallel", t.profile == TrussProfile::Parallel), opt("Pitched", "profile", "pitched", t.profile == TrussProfile::Pitched)]),
                        select("Web Layout", match t.web { TrussWeb::Pratt => "Pratt", TrussWeb::Howe => "Howe", TrussWeb::Warren => "Warren" }, vec![opt("Pratt", "web", "pratt", t.web == TrussWeb::Pratt), opt("Howe", "web", "howe", t.web == TrussWeb::Howe), opt("Warren", "web", "warren", t.web == TrussWeb::Warren)]),
                        num("Truss Height", "height_mm", t.height_mm),
                        num("Panels", "panels", t.panels as f64),
                    ] }),
                    json!({ "name": "Members", "rows": [framing("Top Chord", "top_chord", t.top_chord), framing("Bottom Chord", "bottom_chord", t.bottom_chord), framing("Webs", "web_type", t.web_type)] }),
                    json!({ "name": "Dimensions", "rows": std::iter::once(ro_num("Span", t.start.dist(&t.end), format!("{:.0}", t.start.dist(&t.end)))).chain(measures("Number of Members")).collect::<Vec<_>>() }),
                    identity(&t.mark, &t.comments),
                ])
            }
            _ => return None,
        };
        Some(json!({ "id": id, "kind": kind, "category": "Structural Framing", "type_id": type_id, "title": title, "target": { "method": "patch_structural", "id": id }, "groups": groups }))
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
        // cut where the plan's cut plane meets it; seen from above at its top, from below at its base
        let Some((outer, inner)) = d.column_plan(c, if display == "cut" { range.cut } else if base >= range.cut { base } else { top }) else { continue };
        let (p0, p1) = d.column_ends(c);
        out.push(json!({ "id": id, "kind": "column", "outline": pts(&outer), "inner": inner.map(|r| pts(&r)), "at": [c.at.x, c.at.y], "axis": c.top_at.map(|_| [[p0[0], p0[1]], [p1[0], p1[1]]]), "display": display, "pinned": c.pinned, "style": style }));
    }
    // framing: placed beams, a beam system's beams and a truss (its bottom chord stands for it in plan)
    let mut seen: std::collections::BTreeSet<Id> = Default::default();
    for (id, kind, t, m) in d.structural_members() {
        if kind != "Beam" || v.hidden_elements.contains(&id) { continue; }
        let first = seen.insert(id);
        let (level, pinned, group, boundary) = match d.get(id) {
            Some(Element::Beam(b)) => (b.level, b.pinned, None, None),
            Some(Element::BeamSystem(s)) => (s.level, s.pinned, Some("system"), first.then(|| pts(&s.boundary))),
            Some(Element::Truss(tr)) => { if !first { continue; } (tr.level, tr.pinned, Some("truss"), None) }
            _ => continue,
        };
        let (p0, p1) = (m.origin, [m.origin[0] + m.axis[0] * m.length, m.origin[1] + m.axis[1] * m.length, m.origin[2] + m.axis[2] * m.length]);
        let depth = t.section.size().1.max(t.section.size().0);
        let (lo, hi) = (p0[2].min(p1[2]) - depth, p0[2].max(p1[2]) + depth);
        let display = match shown(range.classify(lo, hi)) {
            Some(x) => x,
            // under its own level, below the plan's range: still drawn in that level's plan
            None if v.level == Some(level) && hi < range.cut => "projection",
            None => continue,
        };
        let hint = d.material(&t.material).map(Hint::from_material).unwrap_or_default();
        let Some(style) = style_for(d, &settings, "Structural Framing", None, d.graphics_of(v, &id).as_ref(), display, &hint) else { continue };
        let outline = Document::beam_plan(&m, &t.section);
        out.push(json!({ "id": id, "kind": "beam", "outline": pts(&outline), "start": [p0[0], p0[1]], "end": [p1[0], p1[1]], "display": display, "pinned": pinned, "style": style, "group": group, "boundary": boundary }));
    }
    json!({ "structure": out })
}

/// The 3D solids of the columns and beams whose category shows.
pub(crate) fn meshes(d: &Document, shown: &dyn Fn(&str) -> bool, hidden: &std::collections::BTreeSet<Id>) -> Vec<Value> {
    d.structural_meshes().into_iter().filter(|(id, kind, _, v)| !v.is_empty() && !hidden.contains(id) && shown(if *kind == "Beam" { "Structural Framing" } else { "Structural Columns" }))
        .map(|(id, kind, color, verts)| json!({ "id": id, "kind": kind, "color": color, "verts": verts })).collect()
}
