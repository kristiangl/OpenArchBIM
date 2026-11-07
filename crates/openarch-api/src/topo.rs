//! Toposolids: Create from Sketch, Create from Import (a points file), Add Point, Edit Boundary, Properties, and
//! what a floor plan draws (the boundary and the type's contour lines).
use serde_json::{json, Value};

use openarch_model::curve::Curve;
use openarch_model::topo::{TopoBase, Toposolid};
use openarch_model::view::ViewKind;
use openarch_model::{Document, Id};

use crate::{id_of, point_of, Res, Session};

fn curves_of(p: &Value) -> Result<Vec<Curve>, String> {
    p["curves"].as_array().ok_or("missing curves")?.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad curve: {e}"))).collect()
}

fn base_of(p: &Value, default: TopoBase) -> Result<TopoBase, String> {
    match p.get("base").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad elevation base: {e}")),
        None => Ok(default),
    }
}

/// Plan area (m²), perimeter (mm) and the boundary's rings.
fn measures(t: &Toposolid) -> (f64, f64, Vec<Vec<[f64; 2]>>) {
    let regions = t.regions(20.0);
    let area: f64 = regions.iter().map(|r| r.area()).sum::<f64>() / 1e6;
    let rings: Vec<Vec<[f64; 2]>> = regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(r.holes.iter())).map(|ring| ring.iter().map(|p| [p.x, p.y]).collect()).collect();
    let perimeter = rings.iter().map(|ring| (0..ring.len()).map(|i| { let (a, b) = (ring[i], ring[(i + 1) % ring.len()]); (b[0] - a[0]).hypot(b[1] - a[1]) }).sum::<f64>()).sum();
    (area, perimeter, rings)
}

impl Session {
    pub(crate) fn handle_topo(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "ensure_toposolid_types" => (|| { self.doc.ensure_toposolid_types()?; Ok(json!({})) })(),
            // Create from Sketch: { type_id, level_id, offset_mm?, curves }
            "add_toposolid" => (|| {
                let id = self.doc.add_toposolid_sketch(id_of(p, "type_id")?, id_of(p, "level_id")?, curves_of(p)?, p["offset_mm"].as_f64().unwrap_or(0.0))?;
                Ok(json!({ "id": id }))
            })(),
            // Create from Import: { type_id, level_id, text, scale (mm per file unit), base }
            "import_toposolid" => (|| {
                let scale = p["scale"].as_f64().filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1000.0);
                let text = match (p["text"].as_str(), p["path"].as_str()) {
                    (Some(t), _) => t.to_string(),
                    (None, Some(path)) => std::fs::read_to_string(path).map_err(|e| format!("could not read {path}: {e}"))?,
                    _ => return Err("missing file".into()),
                };
                let pts = self.doc.topo_points_from_file(&text, scale, base_of(p, TopoBase::SurveyPoint)?)?;
                let read = pts.len();
                let id = self.doc.add_toposolid_points(id_of(p, "type_id")?, id_of(p, "level_id")?, pts)?;
                let kept = self.doc.toposolid(id).map_or(0, |t| t.points.len());
                Ok(json!({ "id": id, "read": read, "kept": kept }))
            })(),
            // Add Point: { id, point, elevation, mode: "absolute" (from `base`) | "surface" (above the surface there) }
            "add_toposolid_point" => (|| {
                let id = id_of(p, "id")?;
                let at = point_of(&p["point"])?;
                let t = self.doc.toposolid(id).ok_or("not a toposolid")?;
                let e = p["elevation"].as_f64().ok_or("missing elevation")?;
                let z = if p["mode"].as_str() == Some("surface") {
                    let tin = self.doc.toposolid_tin(t).ok_or("the toposolid has no surface")?;
                    tin.z_at(at).ok_or("Click inside the toposolid's boundary.")? + e
                } else {
                    self.doc.topo_base_z(base_of(p, TopoBase::CurrentLevel)?, t.level) + e
                };
                self.doc.add_toposolid_point(id, at, z)?;
                Ok(json!({}))
            })(),
            // the toposolid under a plan point, for Add Point
            "toposolid_at" => (|| {
                let at = point_of(&p["point"])?;
                let hit = self.doc.toposolids().into_iter().find(|(_, t)| self.doc.toposolid_tin(t).and_then(|tin| tin.z_at(at)).is_some()).map(|(id, _)| id);
                Ok(json!({ "id": hit }))
            })(),
            "toposolid_sketch" => (|| {
                let t = self.doc.toposolid(id_of(p, "id")?).ok_or("not a toposolid")?;
                Ok(json!({ "level_id": t.level, "type_id": t.toposolid_type, "curves": t.loops.concat().iter().map(crate::describe_curve).collect::<Vec<_>>() }))
            })(),
            "set_toposolid_sketch" => (|| { self.doc.set_toposolid_sketch(id_of(p, "id")?, curves_of(p)?)?; Ok(json!({})) })(),
            "patch_toposolid" => (|| { self.doc.patch_toposolid(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            _ => return None,
        })
    }

    pub(crate) fn toposolid_props(&self, id: Id, t: &Toposolid) -> Value {
        let d = &self.doc;
        let ty = d.toposolid_type(t.toposolid_type);
        let thick = d.toposolid_thickness(t);
        let (area, perimeter, _) = measures(t);
        let tin = d.toposolid_tin(t);
        let plan_area = tin.as_ref().map_or(0.0, |s| s.plan_area()) / 1e6;
        // shared elevations, as levels and spots read them from the survey point
        let zero = d.topo_base_z(TopoBase::SurveyPoint, t.level);
        let (lo, hi) = tin.as_ref().map_or((0.0, 0.0), |s| s.z_range());
        let varies = hi - lo > 0.5;
        let elev = |z: f64| if varies { "Varies".to_string() } else { format!("{:.0}", z - zero) };
        let free = !t.pinned;
        let num = |label: &str, path: &str, v: f64, on: bool| json!({ "label": label, "path": path, "kind": if on { "number" } else { "ro" }, "raw": v, "value": format!("{v}") });
        let boolean = |label: &str, path: &str, v: bool| json!({ "label": label, "path": path, "kind": "bool", "raw": v, "value": if v { "Yes" } else { "No" } });
        let text = |label: &str, path: &str, v: &str| json!({ "label": label, "path": path, "kind": "text", "value": v });
        let ro = |label: &str, value: String, raw: Option<f64>| json!({ "label": label, "kind": "ro", "value": value, "raw": raw });
        json!({
            "id": id, "kind": "Toposolid", "category": "Topography", "type_id": t.toposolid_type,
            "title": format!("Toposolid: {}", ty.map_or("?", |t| t.name.as_str())),
            "target": { "method": "patch_toposolid", "id": id },
            "groups": [
                { "name": "Constraints", "rows": [
                    ro("Level", d.level(t.level).map_or(String::new(), |l| l.name.clone()), None),
                    num("Height Offset From Level", "offset_mm", t.offset_mm, free),
                    boolean("Pinned", "pinned", t.pinned),
                ] },
                { "name": "Dimensions", "rows": [
                    ro("Perimeter", format!("{perimeter:.0}"), Some(perimeter)),
                    ro("Area", format!("{area:.2} m\u{b2}"), Some(area)),
                    ro("Volume", format!("{:.3} m\u{b3}", plan_area * thick / 1000.0), Some(plan_area * thick / 1000.0)),
                    ro("Elevation at Top", elev(hi), (!varies).then_some(hi - zero)),
                    ro("Elevation at Bottom", elev(lo - thick), (!varies).then_some(lo - thick - zero)),
                    ro("Thickness", format!("{thick:.0}"), Some(thick)),
                    ro("Points", format!("{}", t.points.len()), Some(t.points.len() as f64)),
                ] },
                { "name": "Identity Data", "rows": [text("Comments", "comments", &t.comments), text("Mark", "mark", &t.mark)] },
            ],
        })
    }
}

/// What a floor plan draws for toposolids: the boundary and the contour lines of each, and its points.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if v.view_kind != ViewKind::FloorPlan { return json!({ "toposolids": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    if settings.overrides.model.get("Topography").is_some_and(|o| !o.visible) { return json!({ "toposolids": [] }); }
    let list: Vec<Value> = d.toposolids().into_iter().filter(|(id, _)| !v.hidden_elements.contains(id)).map(|(id, t)| {
        let (_, _, rings) = measures(t);
        let contours: Vec<Value> = d.toposolid_contours(t).into_iter().map(|(primary, segs)| {
            json!({ "primary": primary, "segs": segs.iter().map(|s| [s[0].x, s[0].y, s[1].x, s[1].y]).collect::<Vec<_>>() })
        }).collect();
        let base = d.toposolid_base_z(t);
        let points: Vec<[f64; 3]> = t.points.iter().map(|q| [q.x, q.y, base + q.z]).collect();
        json!({ "id": id, "rings": rings, "pinned": t.pinned, "contours": contours, "points": points })
    }).collect();
    json!({ "toposolids": list })
}
