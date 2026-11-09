//! Toposolids: Create from Sketch, Create from Import (a points file), Add Point, Edit Boundary, Sub-Divide, Split,
//! Merge, Properties, and what a floor plan draws (the boundary and the type's contour lines).
use serde_json::{json, Value};

use openarch_model::curve::Curve;
use openarch_model::site::P3;
use openarch_model::topo::{ContourLabel, TopoBase, Toposolid};
use openarch_model::view::ViewKind;
use openarch_model::{Document, Id};

use crate::graphics::{style_for, Hint};
use crate::{id_of, ids_of, point_of, Res, Session};

fn curves_of(p: &Value) -> Result<Vec<Curve>, String> {
    p["curves"].as_array().ok_or("missing curves")?.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad curve: {e}"))).collect()
}

fn base_of(p: &Value, default: TopoBase) -> Result<TopoBase, String> {
    match p.get("base").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad elevation base: {e}")),
        None => Ok(default),
    }
}

/// Plan area (m²), perimeter (mm) and the boundary's rings (a sub-division's, where it is on its host).
fn measures(d: &Document, t: &Toposolid) -> (f64, f64, Vec<Vec<[f64; 2]>>) {
    let regions = openarch_model::geom::regions(&d.toposolid_rings(t));
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
            // Sub-Divide: { host_id, type_id, offset_mm? (above the host's top), curves }
            "add_toposolid_subdivision" => (|| {
                let id = self.doc.add_toposolid_subdivision(id_of(p, "host_id")?, id_of(p, "type_id")?, curves_of(p)?, p["offset_mm"].as_f64().unwrap_or(0.0))?;
                Ok(json!({ "id": id }))
            })(),
            // Split: { id, curves } (a closed loop inside it, or lines from edge to edge); the pieces, the original first
            "split_toposolid" => (|| Ok(json!({ "ids": self.doc.split_toposolid(id_of(p, "id")?, &curves_of(p)?)? })))(),
            // Merge: { id, other }: the other made part of it
            "merge_toposolids" => (|| { self.doc.merge_toposolids(id_of(p, "id")?, id_of(p, "other")?)?; Ok(json!({})) })(),
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
                let hit = self.doc.toposolids().into_iter().filter(|(_, t)| t.host.is_none()).find(|(_, t)| self.doc.toposolid_tin(t).and_then(|tin| tin.z_at(at)).is_some()).map(|(id, _)| id);
                Ok(json!({ "id": hit }))
            })(),
            "toposolid_sketch" => (|| {
                let t = self.doc.toposolid(id_of(p, "id")?).ok_or("not a toposolid")?;
                Ok(json!({ "level_id": t.level, "type_id": t.toposolid_type, "curves": t.loops.concat().iter().map(crate::describe_curve).collect::<Vec<_>>() }))
            })(),
            "set_toposolid_sketch" => (|| { self.doc.set_toposolid_sketch(id_of(p, "id")?, curves_of(p)?)?; Ok(json!({})) })(),
            // Modify Sub Elements: { id, points: [[x, y, height above the unmodified top], ...], lines?: [[i, j], ...] },
            // the whole lists (no lines: they stay as they are)
            "set_toposolid_points" => (|| {
                let pts: Vec<[f64; 3]> = serde_json::from_value(p["points"].clone()).map_err(|e| format!("bad points: {e}"))?;
                let lines: Option<Vec<[usize; 2]>> = match p.get("lines").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad lines: {e}"))?),
                    None => None,
                };
                self.doc.set_toposolid_points(id_of(p, "id")?, pts.into_iter().map(|q| P3 { x: q[0], y: q[1], z: q[2] }).collect(), lines)?;
                Ok(json!({}))
            })(),
            // Add Split Line: { id, a, b } plan points
            "add_toposolid_split_line" => (|| {
                self.doc.add_toposolid_split_line(id_of(p, "id")?, point_of(&p["a"])?, point_of(&p["b"])?)?;
                Ok(json!({}))
            })(),
            // Excavate: { id, by: [floor, roof or toposolid ids] }; Remove Excavation the same
            "excavate_toposolid" => (|| Ok(json!({ "added": self.doc.excavate_toposolid(id_of(p, "id")?, &ids_of(p, "by")?)? })))(),
            "remove_excavation" => (|| Ok(json!({ "removed": self.doc.remove_excavation(id_of(p, "id")?, &ids_of(p, "by")?)? })))(),
            // Simplify Toposolid: { id, keep_percent }
            "simplify_toposolid" => (|| {
                let id = id_of(p, "id")?;
                let keep = p["keep_percent"].as_f64().ok_or("missing keep_percent")? / 100.0;
                let removed = self.doc.simplify_toposolid(id, keep)?;
                Ok(json!({ "removed": removed, "kept": self.doc.toposolid(id).map_or(0, |t| t.points.len()) }))
            })(),
            // Label Contours: { view_id, a, b }
            "add_contour_label" => (|| Ok(json!({ "id": self.doc.add_contour_label(id_of(p, "view_id")?, point_of(&p["a"])?, point_of(&p["b"])?)? })))(),
            "patch_contour_label" => (|| { self.doc.patch_contour_label(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "patch_toposolid" => (|| { self.doc.patch_toposolid(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            _ => return None,
        })
    }

    pub(crate) fn contour_label_props(&self, id: Id, l: &ContourLabel) -> Value {
        let base = |b: TopoBase, label: &str| json!({ "label": label, "patch": { "base": b }, "selected": l.base == b });
        json!({ "id": id, "kind": "ContourLabel", "category": "Topography", "title": "Contour Label", "type_label": "Contour Label", "target": { "method": "patch_contour_label", "id": id }, "groups": [
            { "name": "Text", "rows": [
                { "label": "Elevation Base", "kind": "select", "value": if l.base == TopoBase::ProjectBasePoint { "Project Base Point" } else { "Survey Point" },
                  "options": [base(TopoBase::SurveyPoint, "Survey Point"), base(TopoBase::ProjectBasePoint, "Project Base Point")] },
                { "label": "Decimal Places", "kind": "number", "path": "decimals", "raw": l.decimals, "value": l.decimals.to_string() },
                { "label": "Text Size", "kind": "number", "path": "size_mm", "raw": l.size_mm, "value": format!("{}", l.size_mm) },
                { "label": "Primary Contours Only", "kind": "bool", "path": "primary_only", "raw": l.primary_only, "value": if l.primary_only { "Yes" } else { "No" } },
            ] },
            { "name": "Constraints", "rows": [
                { "label": "Pinned", "kind": "bool", "path": "pinned", "raw": l.pinned, "value": if l.pinned { "Yes" } else { "No" } },
            ] },
        ] })
    }

    pub(crate) fn toposolid_props(&self, id: Id, t: &Toposolid) -> Value {
        let d = &self.doc;
        let ty = d.toposolid_type(t.toposolid_type);
        let thick = d.toposolid_thickness(t);
        let (area, perimeter, _) = measures(d, t);
        let surface = d.toposolid_surface(t);
        let tin = surface.as_ref().map(|s| &s.0);
        let volume = d.toposolid_volume(t) / 1e9;
        let low = surface.as_ref().map(|s| s.1.clone()).unwrap_or_default();
        let (blo, bhi) = low.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &z| (a.min(z), b.max(z)));
        // shared elevations, as levels and spots read them from the survey point
        let zero = d.topo_base_z(TopoBase::SurveyPoint, t.level);
        let (lo, hi) = tin.map_or((0.0, 0.0), |s| s.z_range());
        let varies = hi - lo > 0.5;
        let elev = |z: f64, varies: bool| if varies { "Varies".to_string() } else { format!("{:.0}", z - zero) };
        let bottom_varies = bhi - blo > 0.5;
        let free = !t.pinned;
        let host = t.host.and_then(|h| d.toposolid(h).map(|x| (h, x)));
        let num = |label: &str, path: &str, v: f64, on: bool| json!({ "label": label, "path": path, "kind": if on { "number" } else { "ro" }, "raw": v, "value": format!("{v}") });
        let boolean = |label: &str, path: &str, v: bool| json!({ "label": label, "path": path, "kind": "bool", "raw": v, "value": if v { "Yes" } else { "No" } });
        let text = |label: &str, path: &str, v: &str| json!({ "label": label, "path": path, "kind": "text", "value": v });
        let ro = |label: &str, value: String, raw: Option<f64>| json!({ "label": label, "kind": "ro", "value": value, "raw": raw });
        // a sub-division: its host, its height above it, and what it takes out of it
        let (constraints, mut dims) = match host {
            Some((h, x)) => {
                let name = d.toposolid_type(x.toposolid_type).map_or("Toposolid".to_string(), |ty| ty.name.clone());
                let dug = d.subdivision_excavation(id) / 1e9;
                (vec![
                    ro("Level", d.level(t.level).map_or(String::new(), |l| l.name.clone()), None),
                    ro("Host", if x.mark.is_empty() { format!("{name} ({h})") } else { format!("{name} ({})", x.mark) }, None),
                    num("Sub-divide Height", "offset_mm", t.offset_mm, free),
                    boolean("Pinned", "pinned", t.pinned),
                ], vec![ro("Excavation Volume", format!("{dug:.3} m\u{b3}"), Some(dug))])
            }
            None => (vec![
                ro("Level", d.level(t.level).map_or(String::new(), |l| l.name.clone()), None),
                num("Height Offset From Level", "offset_mm", t.offset_mm, free),
                boolean("Pinned", "pinned", t.pinned),
            ], vec![ro("Points", format!("{}", t.points.len()), Some(t.points.len() as f64))]),
        };
        let mut rows = vec![
            ro("Perimeter", format!("{perimeter:.0}"), Some(perimeter)),
            ro("Area", format!("{area:.2} m\u{b2}"), Some(area)),
            ro("Volume", format!("{volume:.3} m\u{b3}"), Some(volume)),
            ro("Elevation at Top", elev(hi, varies), (!varies).then_some(hi - zero)),
            ro("Elevation at Bottom", elev(blo, bottom_varies), (!bottom_varies && blo.is_finite()).then_some(blo - zero)),
            ro("Thickness", format!("{thick:.0}"), Some(thick)),
        ];
        rows.append(&mut dims);
        let noun = if host.is_some() { "Toposolid Sub-division" } else { "Toposolid" };
        json!({
            "id": id, "kind": "Toposolid", "category": "Topography", "type_id": t.toposolid_type,
            "title": format!("{noun}: {}", ty.map_or("?", |t| t.name.as_str())),
            "target": { "method": "patch_toposolid", "id": id },
            "groups": [
                { "name": "Constraints", "rows": constraints },
                { "name": "Dimensions", "rows": rows },
                { "name": "Identity Data", "rows": [text("Comments", "comments", &t.comments), text("Mark", "mark", &t.mark)] },
            ],
        })
    }
}

/// What a floor plan draws for toposolids: the boundary and the contour lines of each, and its points.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if v.view_kind != ViewKind::FloorPlan { return json!({ "toposolids": [], "contour_labels": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    if settings.overrides.model.get("Topography").is_some_and(|o| !o.visible) { return json!({ "toposolids": [], "contour_labels": [] }); }
    let cut_z = d.resolve_view_range(v, &settings).map(|r| r.cut);
    let mut all = d.toposolids();
    all.sort_by_key(|(_, t)| t.host.is_some());
    let list: Vec<Value> = all.into_iter().filter(|(id, t)| !v.hidden_elements.contains(id) && !t.host.is_some_and(|h| v.hidden_elements.contains(&h))).map(|(id, t)| {
        let (_, _, rings) = measures(d, t);
        let contours: Vec<Value> = d.toposolid_contours(t).into_iter().map(|(primary, segs)| {
            json!({ "primary": primary, "segs": segs.iter().map(|s| [s[0].x, s[0].y, s[1].x, s[1].y]).collect::<Vec<_>>() })
        }).collect();
        let base = d.toposolid_base_z(t);
        // points as plan positions and their heights above the unmodified top (what Modify Sub Elements edits)
        let points: Vec<[f64; 3]> = t.points.iter().map(|q| [q.x, q.y, q.z]).collect();
        // the ground the cut plane passes through, in the cut style of the top layer's material
        let cut = cut_z.map(|z| d.toposolid_plan_cut(t, z)).filter(|(polys, _)| !polys.is_empty()).and_then(|(polys, lines)| {
            let material = d.toposolid_type(t.toposolid_type).and_then(|ty| ty.layers.first()).and_then(|l| d.material(&l.material));
            let hint = material.map(Hint::from_material).unwrap_or_default();
            let style = style_for(d, &settings, "Topography", None, v.element_overrides.get(&id), "cut", &hint)?;
            Some(json!({
                "polys": polys.iter().map(|q| q.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>(),
                "lines": lines.iter().map(|s| [s[0].x, s[0].y, s[1].x, s[1].y]).collect::<Vec<_>>(),
                "style": style,
            }))
        });
        json!({ "id": id, "rings": rings, "pinned": t.pinned, "contours": contours, "points": points, "lines": t.split_lines, "excavated_by": d.toposolid_excavators(t), "base": base, "cut": cut, "host": t.host })
    }).collect();
    let labels: Vec<Value> = d.contour_labels_in(view).into_iter().filter(|(id, _)| !v.hidden_elements.contains(id)).map(|(id, l)| json!({
        "id": id, "a": [l.a.x, l.a.y], "b": [l.b.x, l.b.y], "pinned": l.pinned, "size": l.size_mm, "labels": d.contour_label_texts(l),
    })).collect();
    json!({ "toposolids": list, "contour_labels": labels })
}
