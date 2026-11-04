//! Colour fill schemes and legends: Edit Color Scheme, the view's scheme, legend placement, and what a plan draws.
use serde_json::{json, Value};

use openarch_model::colorfill::{color_parameters, ColorEntry, ColorFillLegend, ColorFillScheme};
use openarch_model::{Document, Element, Id};

use crate::{id_of, point_of, Res, Session};

/// The view a request is about: a view id, or a colour fill legend's id (its view).
fn view_of(d: &Document, id: Id) -> Id {
    match d.get(id) { Some(Element::ColorFillLegend(l)) => l.view, _ => id }
}

fn hex(c: [u8; 3]) -> String { format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]) }

fn rgb(v: &Value) -> Result<[u8; 3], String> {
    let s = v.as_str().ok_or("missing colour")?.trim_start_matches('#');
    if s.len() != 6 { return Err("bad colour".into()); }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "bad colour".to_string());
    Ok([p(0)?, p(2)?, p(4)?])
}

impl Session {
    pub(crate) fn handle_color_fills(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // Edit Color Scheme: { id } (a view or a legend): the view's category, its schemes and the one it shows
            "color_schemes" => (|| {
                let view = view_of(&self.doc, id_of(p, "id")?);
                let category = self.doc.color_category_of_view(view).ok_or("Colour schemes are for floor plans and area plans.")?;
                self.doc.ensure_color_scheme(category)?;
                let d = &self.doc;
                let locked = d.view(view).and_then(|v| v.template).and_then(|t| d.view_template(t)).is_some_and(|t| t.included.contains(&openarch_model::view::TemplateProp::ColorScheme));
                let schemes: Vec<Value> = d.color_fill_schemes().into_iter().filter(|(_, s)| s.category == category).map(|(id, s)| json!({
                    "id": id, "name": s.name, "title": s.title, "parameter": s.parameter, "by_range": s.by_range,
                    "rows": d.color_rows(id, None).into_iter().map(|r| json!({ "label": r.label, "value": r.value, "at_least": r.at_least, "color": hex(r.color), "visible": r.visible, "in_use": r.in_use })).collect::<Vec<_>>(),
                })).collect();
                Ok(json!({
                    "view_id": view, "category": d.category_label(category), "schemes": schemes, "current": d.view_color_scheme(view), "locked": locked,
                    "parameters": color_parameters(category).iter().map(|(n, num)| json!({ "name": n, "numeric": num })).collect::<Vec<_>>(),
                }))
            })(),
            // { id, scheme: { name, title, parameter, by_range, rows: [{ value, at_least, color, visible }] } }
            "set_color_scheme" => (|| {
                let id = id_of(p, "id")?;
                let old = self.doc.color_fill_scheme(id).ok_or("Unknown colour scheme.")?.clone();
                let s = &p["scheme"];
                let by_range = s["by_range"].as_bool().unwrap_or(old.by_range);
                let entries = match s["rows"].as_array() {
                    Some(rows) => rows.iter().map(|r| Ok(ColorEntry {
                        value: r["value"].as_str().unwrap_or("").to_string(),
                        at_least: if by_range { Some(r["at_least"].as_f64().ok_or("A range needs a number.")?) } else { None },
                        color: rgb(&r["color"])?, visible: r["visible"].as_bool().unwrap_or(true),
                    })).collect::<Result<Vec<_>, String>>()?,
                    None => old.entries.clone(),
                };
                let new = ColorFillScheme {
                    name: s["name"].as_str().map_or(old.name.clone(), str::to_string), category: old.category,
                    title: s["title"].as_str().map_or(old.title.clone(), str::to_string),
                    parameter: s["parameter"].as_str().map_or(old.parameter.clone(), str::to_string), by_range, entries,
                };
                self.doc.set_color_scheme(id, new)?;
                Ok(json!({}))
            })(),
            "duplicate_color_scheme" => (|| Ok(json!({ "id": self.doc.duplicate_color_scheme(id_of(p, "id")?, p["name"].as_str().unwrap_or(""))? })))(),
            "delete_color_scheme" => (|| { self.doc.delete_color_scheme(id_of(p, "id")?)?; Ok(json!({})) })(),
            // { view_id, scheme_id (null: none) }
            "set_view_color_scheme" => (|| {
                let view = view_of(&self.doc, id_of(p, "view_id")?);
                self.doc.set_view_color_scheme(view, p["scheme_id"].as_u64())?;
                Ok(json!({}))
            })(),
            "add_color_legend" => (|| Ok(json!({ "id": self.doc.add_color_legend(id_of(p, "view_id")?, point_of(&p["point"])?)? })))(),
            "patch_color_legend" => (|| {
                let id = id_of(p, "id")?;
                let Some(Element::ColorFillLegend(old)) = self.doc.get(id).cloned() else { return Err("not a colour fill legend".into()) };
                let new: ColorFillLegend = openarch_model::doc::apply_patch(&old, p.get("patch").ok_or("missing patch")?)?;
                if old.pinned && new.pinned && new.at != old.at { return Err("Unpin the legend before moving it.".into()); }
                let new = ColorFillLegend { view: old.view, ..new };
                self.doc.transact("Color fill legend", |tx| tx.update(id, Element::ColorFillLegend(new)))?;
                Ok(json!({}))
            })(),
            _ => return None,
        })
    }

    pub(crate) fn color_legend_props(&self, id: Id, l: &ColorFillLegend) -> Value {
        let d = &self.doc;
        let scheme = d.view_color_scheme(l.view).and_then(|s| d.color_fill_scheme(s)).map_or("<none>".to_string(), |s| s.name.clone());
        json!({ "id": id, "kind": "ColorFillLegend", "title": "Color Fill Legend", "type_label": "Color Fill Legend", "target": { "method": "patch_color_legend", "id": id }, "groups": [
            { "name": "Graphics", "rows": [
                { "label": "Color Scheme", "kind": "dialog", "dialog": "color_scheme", "value": scheme },
                { "label": "Pinned", "kind": "bool", "path": "pinned", "raw": l.pinned, "value": if l.pinned { "Yes" } else { "No" } },
            ] },
        ] })
    }
}

/// What a plan draws for colour fills: the coloured rooms or areas (behind or in front of the model, as the view
/// says) and its legends, which list the colours in the view.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if d.color_category_of_view(view).is_none() { return json!({}); }
    let settings = d.effective_settings(view).unwrap_or_default();
    let rings = |r: &openarch_model::geom::Region| std::iter::once(&r.outer).chain(&r.holes).map(|ring| ring.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>();
    let fills: Vec<Value> = d.color_fills(view).iter().map(|f| json!({ "element": f.element, "color": hex(f.color), "rings": rings(&f.region) })).collect();
    let scheme = d.view_color_scheme(view);
    let rows: Vec<Value> = scheme.map(|s| d.color_rows(s, Some(view)).into_iter().filter(|r| r.in_use && r.visible).map(|r| json!({ "label": r.label, "color": hex(r.color) })).collect()).unwrap_or_default();
    let title = scheme.and_then(|s| d.color_fill_scheme(s)).map(|s| s.title.clone());
    let legends: Vec<Value> = d.color_fill_legends(view).into_iter().filter(|(id, _)| !v.hidden_elements.contains(id)).map(|(id, l)| json!({
        "id": id, "at": l.at, "pinned": l.pinned, "title": title.clone().unwrap_or_else(|| "No colour scheme".into()), "rows": rows,
    })).collect();
    json!({ "color_fills": fills, "color_legends": legends, "color_location": settings.color_scheme_location })
}
