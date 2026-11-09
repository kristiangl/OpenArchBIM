//! Ceilings: placing (automatic and sketched), Properties, and what a reflected ceiling plan draws.
use serde_json::{json, Value};

use openarch_model::ceiling::{Ceiling, DEFAULT_HEIGHT_MM};
use openarch_model::curve::Curve;
use openarch_model::element::SlopeArrow;
use openarch_model::view::ViewKind;
use openarch_model::{Document, Id};

use crate::{id_of, point_of, Res, Session};

fn curves_of(p: &Value) -> Result<Vec<Curve>, String> {
    p["curves"].as_array().ok_or("missing curves")?.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad curve: {e}"))).collect()
}

fn slope_of(p: &Value) -> Result<Option<SlopeArrow>, String> {
    match p.get("slope").filter(|v| !v.is_null()) { Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|e| format!("bad slope arrow: {e}")), None => Ok(None) }
}

/// Area (m²), perimeter (mm) and the rings of a ceiling's plan shape.
fn measures(c: &Ceiling) -> (f64, f64, Vec<Vec<[f64; 2]>>) {
    let regions = c.regions(0.5);
    let area: f64 = regions.iter().map(|r| r.area()).sum::<f64>() / 1e6;
    let rings: Vec<Vec<[f64; 2]>> = regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(r.holes.iter())).map(|ring| ring.iter().map(|p| [p.x, p.y]).collect()).collect();
    let perimeter = rings.iter().map(|ring| (0..ring.len()).map(|i| { let (a, b) = (ring[i], ring[(i + 1) % ring.len()]); (b[0] - a[0]).hypot(b[1] - a[1]) }).sum::<f64>()).sum();
    (area, perimeter, rings)
}

impl Session {
    pub(crate) fn handle_ceilings(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "ensure_ceiling_types" => (|| { self.doc.ensure_ceiling_types()?; Ok(json!({})) })(),
            // { type_id, level_id, offset_mm?, point } places an automatic ceiling; { ..., curves, slope? } a sketched one
            "add_ceiling" => (|| {
                let (ty, level) = (id_of(p, "type_id")?, id_of(p, "level_id")?);
                let offset = p["offset_mm"].as_f64().unwrap_or(DEFAULT_HEIGHT_MM);
                let id = if p.get("point").is_some_and(|v| !v.is_null()) {
                    self.doc.add_automatic_ceiling(ty, level, offset, point_of(&p["point"])?)?
                } else {
                    self.doc.add_ceiling_sketch(ty, level, curves_of(p)?, offset, slope_of(p)?)?
                };
                Ok(json!({ "id": id }))
            })(),
            "set_ceiling_sketch" => (|| { self.doc.set_ceiling_sketch(id_of(p, "id")?, curves_of(p)?, slope_of(p)?)?; Ok(json!({})) })(),
            // a ceiling-hosted family on the ceiling over the click: { type_id, level_id, point, rotation? }
            "place_on_ceiling" => (|| {
                let level = id_of(p, "level_id")?;
                let at = point_of(&p["point"])?;
                let ceiling = self.doc.ceiling_at(level, at).ok_or("Click inside a ceiling to place it on.")?;
                Ok(json!({ "id": self.doc.place_on_ceiling(id_of(p, "type_id")?, ceiling, at, p["rotation"].as_f64().unwrap_or(0.0))?, "ceiling": ceiling }))
            })(),
            // Edit Boundary: the sketch to reopen
            "ceiling_sketch" => (|| {
                let c = self.doc.ceiling(id_of(p, "id")?).ok_or("not a ceiling")?;
                Ok(json!({ "level_id": c.level, "type_id": c.ceiling_type, "curves": c.loops.concat().iter().map(crate::describe_curve).collect::<Vec<_>>(), "slope": c.slope }))
            })(),
            "patch_ceiling" => (|| { self.doc.patch_ceiling(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            // Automatic Ceiling's preview: the enclosure under the cursor at the ceiling's height
            "ceiling_region_at" => (|| {
                let r = self.doc.ceiling_region_at(id_of(p, "level_id")?, p["offset_mm"].as_f64().unwrap_or(DEFAULT_HEIGHT_MM), point_of(&p["point"])?);
                Ok(json!({ "rings": r.map(|r| std::iter::once(r.outer).chain(r.holes).map(|ring| ring.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>()).collect::<Vec<_>>()) }))
            })(),
            _ => return None,
        })
    }

    pub(crate) fn ceiling_props(&self, id: Id, c: &Ceiling) -> Value {
        let d = &self.doc;
        let t = d.ceiling_type(c.ceiling_type);
        let thick = d.ceiling_thickness(c);
        let (area, perimeter, _) = measures(c);
        let slope = d.floor_surface(&d.ceiling_slab(c)).map_or("Flat".to_string(), |s| if s.rise.abs() < 1e-9 { "Flat".into() } else { format!("{:.2}%", s.percent()) });
        let free = !c.pinned;
        let num = |label: &str, path: &str, v: f64, on: bool| json!({ "label": label, "path": path, "kind": if on { "number" } else { "ro" }, "raw": v, "value": format!("{v}") });
        let boolean = |label: &str, path: &str, v: bool| json!({ "label": label, "path": path, "kind": "bool", "raw": v, "value": if v { "Yes" } else { "No" } });
        let text = |label: &str, path: &str, v: &str| json!({ "label": label, "path": path, "kind": "text", "value": v });
        let ro = |label: &str, value: String, raw: Option<f64>| json!({ "label": label, "kind": "ro", "value": value, "raw": raw });
        json!({
            "id": id, "kind": "Ceiling", "category": "Ceilings", "type_id": c.ceiling_type,
            "title": format!("Ceiling: {}", t.map_or("?", |t| t.name.as_str())),
            "target": { "method": "patch_ceiling", "id": id },
            "groups": [
                { "name": "Constraints", "rows": [
                    crate::props::level_select(d, "Level", "level", c.level, free),
                    num("Height Offset From Level", "offset_mm", c.offset_mm, free),
                    boolean("Room Bounding", "room_bounding", c.room_bounding),
                    boolean("Pinned", "pinned", c.pinned),
                ] },
                { "name": "Dimensions", "rows": [
                    ro("Slope", slope, None),
                    ro("Perimeter", format!("{perimeter:.0}"), Some(perimeter)),
                    ro("Area", format!("{area:.2} m\u{b2}"), Some(area)),
                    ro("Volume", format!("{:.3} m\u{b3}", area * thick / 1000.0), Some(area * thick / 1000.0)),
                    ro("Thickness", format!("{thick:.0}"), Some(thick)),
                ] },
                { "name": "Identity Data", "rows": [text("Comments", "comments", &c.comments), text("Mark", "mark", &c.mark)] },
            ],
        })
    }
}

/// What a plan view draws for ceilings: a reflected ceiling plan shows its level's ceilings with their grids; a floor
/// plan shows none (they are above its cut plane).
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if v.view_kind != ViewKind::CeilingPlan { return json!({ "ceilings": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    if settings.overrides.model.get("Ceilings").is_some_and(|o| !o.visible) { return json!({ "ceilings": [] }); }
    let ceilings: Vec<Value> = d.ceilings().into_iter().filter(|(id, c)| Some(c.level) == v.level && !v.hidden_elements.contains(id)).map(|(id, c)| {
        let (_, _, rings) = measures(c);
        let grid = d.ceiling_type(c.ceiling_type).and_then(|t| t.grid);
        let origin = c.grid_origin.or_else(|| c.loops.first().and_then(|l| l.first()).map(|k| k.start()));
        json!({ "id": id, "rings": rings, "pinned": c.pinned, "grid": grid, "grid_origin": origin, "grid_angle_deg": c.grid_angle_deg })
    }).collect();
    json!({ "ceilings": ceilings })
}
