//! Property lines: Create by sketching, Create by entering distances and bearings, Edit Sketch, Edit Table,
//! Properties, and what a floor plan draws (the lines and each segment's bearing and distance).
use serde_json::{json, Value};

use openarch_model::curve::Curve;
use openarch_model::propline::{format_bearing, parse_bearing, Bearing, PropertyLine};
use openarch_model::view::ViewKind;
use openarch_model::{Document, Id};

use crate::graphics::{style_for, Hint};
use crate::{id_of, point_of, Res, Session};

fn curves_of(p: &Value) -> Result<Vec<Curve>, String> {
    p["curves"].as_array().ok_or("missing curves")?.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad curve: {e}"))).collect()
}

/// Table rows: { distance_mm, bearing (a number of degrees, or text such as 82°30'00" or N 82°30' E), radius_mm?, left? }.
fn rows_of(p: &Value) -> Result<Vec<Bearing>, String> {
    p["rows"].as_array().ok_or("missing rows")?.iter().enumerate().map(|(i, r)| {
        let distance_mm = r["distance_mm"].as_f64().ok_or_else(|| format!("Segment {}: type its distance.", i + 1))?;
        let bearing_deg = match &r["bearing"] {
            Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            Value::String(s) => parse_bearing(s).map_err(|e| format!("Segment {}: {e}", i + 1))?,
            _ => return Err(format!("Segment {}: type its bearing.", i + 1)),
        };
        let radius_mm = r["radius_mm"].as_f64().filter(|v| *v > 0.0);
        Ok(Bearing { distance_mm, bearing_deg, radius_mm, left: r["left"].as_bool().unwrap_or(false) })
    }).collect()
}

/// The table as the dialog shows it.
fn table_json(d: &Document, pl: &PropertyLine) -> Value {
    json!(d.property_line_table(pl).into_iter().map(|r| json!({
        "distance_mm": r.distance_mm, "bearing": format_bearing(r.bearing_deg), "bearing_deg": r.bearing_deg,
        "radius_mm": r.radius_mm, "left": r.left,
    })).collect::<Vec<_>>())
}

impl Session {
    pub(crate) fn handle_propline(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // Create by sketching: { curves }
            "add_property_line" => (|| Ok(json!({ "id": self.doc.add_property_line_sketch(curves_of(p)?)? })))(),
            // Create by entering distances and bearings: { at, rows }
            "add_property_line_table" => (|| Ok(json!({ "id": self.doc.add_property_line_table(point_of(&p["at"])?, &rows_of(p)?)? })))(),
            // the lines a table gives, from (0, 0), before it is placed: { rows }
            "property_line_preview" => (|| {
                let rows = rows_of(p)?;
                let curves = self.doc.property_line_curves(openarch_model::Point::new(0.0, 0.0), &rows)?;
                let pl = PropertyLine { curves, table: true, name: String::new(), label_segments: true, pinned: false };
                Ok(json!({
                    "lines": pl.curves.iter().map(|c| c.tessellate(20.0).iter().map(|q| [q.x, q.y]).collect::<Vec<_>>()).collect::<Vec<_>>(),
                    "closed": pl.closed(), "area_m2": pl.area().map(|a| a / 1e6), "rows": table_json(&self.doc, &pl),
                }))
            })(),
            // What Edit Sketch and Edit Table start from: { id }
            "property_line" => (|| {
                let pl = self.doc.property_line(id_of(p, "id")?).ok_or("not a property line")?;
                Ok(json!({ "curves": pl.curves.iter().map(crate::describe_curve).collect::<Vec<_>>(), "table": pl.table, "rows": table_json(&self.doc, pl), "closed": pl.closed() }))
            })(),
            "set_property_line_sketch" => (|| { self.doc.set_property_line_sketch(id_of(p, "id")?, curves_of(p)?)?; Ok(json!({})) })(),
            "set_property_line_table" => (|| { self.doc.set_property_line_table(id_of(p, "id")?, &rows_of(p)?)?; Ok(json!({})) })(),
            "patch_property_line" => (|| { self.doc.patch_property_line(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            // a bearing as typed, read: { text } gives { deg, text } (the text as it will show)
            "parse_bearing" => (|| { let deg = parse_bearing(p["text"].as_str().unwrap_or(""))?; Ok(json!({ "deg": deg, "text": format_bearing(deg) })) })(),
            _ => return None,
        })
    }

    pub(crate) fn property_line_props(&self, id: Id, pl: &PropertyLine) -> Value {
        let area = pl.area().map(|a| a / 1e6);
        let length: f64 = pl.curves.iter().map(|c| c.length()).sum();
        json!({
            "id": id, "kind": "PropertyLine", "category": "Site", "title": "Property Line", "type_label": "Property Line",
            "target": { "method": "patch_property_line", "id": id },
            "groups": [
                { "name": "Constraints", "rows": [
                    { "label": "Pinned", "kind": "bool", "path": "pinned", "raw": pl.pinned, "value": if pl.pinned { "Yes" } else { "No" } },
                ] },
                { "name": "Dimensions", "rows": [
                    { "label": "Area", "kind": "ro", "value": area.map_or("Not closed".to_string(), |a| format!("{a:.2} m\u{b2}")), "raw": area },
                    { "label": "Length", "kind": "ro", "value": format!("{length:.0}"), "raw": length },
                ] },
                { "name": "Graphics", "rows": [
                    { "label": "Label Segments", "kind": "bool", "path": "label_segments", "raw": pl.label_segments, "value": if pl.label_segments { "Yes" } else { "No" } },
                ] },
                { "name": "Identity Data", "rows": [
                    { "label": "Name", "kind": "text", "path": "name", "value": pl.name },
                    { "label": "Made From", "kind": "ro", "value": if pl.table { "Distances and bearings" } else { "Sketch" } },
                ] },
            ],
        })
    }
}

/// What a floor plan draws for property lines: each one's lines in the Property Lines style, and its segment labels.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if v.view_kind != ViewKind::FloorPlan { return json!({ "property_lines": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    let list: Vec<Value> = d.property_lines().into_iter().filter(|(id, _)| !v.hidden_elements.contains(id)).filter_map(|(id, pl)| {
        let style = style_for(d, &settings, "Site", Some("Property Lines"), v.element_overrides.get(&id), "projection", &Hint::default())?;
        let lines: Vec<Vec<[f64; 2]>> = pl.curves.iter().map(|c| c.tessellate(20.0).iter().map(|p| [p.x, p.y]).collect()).collect();
        let labels: Vec<Value> = if pl.label_segments {
            d.property_line_labels(pl).into_iter().map(|(at, angle, text)| json!({ "at": [at.x, at.y], "angle": angle, "text": text })).collect()
        } else { vec![] };
        Some(json!({ "id": id, "lines": lines, "pinned": pl.pinned, "style": style, "labels": labels, "table": pl.table }))
    }).collect();
    json!({ "property_lines": list })
}
