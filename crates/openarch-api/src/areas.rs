//! Areas: schemes, area plans, boundary lines, areas and area tags; Properties and what an area plan draws.
use serde_json::{json, Value};

use openarch_model::areas::{Area, AreaStatus, AreaType, SchemeRules};
use openarch_model::curve::Curve;
use openarch_model::view::ViewKind;
use openarch_model::{Document, Element, Id};

use crate::{id_of, ids_of, point_of, Res, Session};

fn status_label(s: AreaStatus) -> &'static str {
    match s { AreaStatus::Placed => "Placed", AreaStatus::NotPlaced => "Not Placed", AreaStatus::NotEnclosed => "Not Enclosed", AreaStatus::Redundant => "Redundant" }
}

/// The schedule category an area is listed under: one per scheme, "Areas (Gross Building)".
pub(crate) fn area_category(d: &Document, a: &Area) -> Option<String> {
    Some(format!("Areas ({})", d.area_scheme(d.scheme_of_area(a)?)?.name))
}

impl Session {
    pub(crate) fn handle_areas(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "ensure_area_schemes" => (|| { self.doc.ensure_area_schemes()?; Ok(json!({})) })(),
            // Create Area Plan: { scheme_id, levels: [ids], boundaries: bool }
            "create_area_plans" => (|| {
                self.doc.ensure_area_schemes()?;
                let scheme = id_of(p, "scheme_id")?;
                Ok(json!({ "ids": self.doc.create_area_plans(scheme, &ids_of(p, "levels")?, p["boundaries"].as_bool().unwrap_or(true))? }))
            })(),
            "add_area_boundaries" => (|| {
                let curves: Vec<Curve> = p["curves"].as_array().ok_or("missing curves")?.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad curve: {e}"))).collect::<Result<_, _>>()?;
                Ok(json!({ "ids": self.doc.add_area_boundaries(id_of(p, "view_id")?, curves, p["apply_rules"].as_bool().unwrap_or(true))? }))
            })(),
            // Pick Walls with Apply Area Rules: the boundary line for a wall in this plan's scheme
            "area_boundary_for_wall" => (|| Ok(json!({ "curve": self.doc.area_boundary_for_wall(id_of(p, "view_id")?, id_of(p, "wall_id")?)? })))(),
            "patch_area_boundary" => (|| {
                let id = id_of(p, "id")?;
                let Some(Element::AreaBoundary(old)) = self.doc.get(id).cloned() else { return Err("not an area boundary".into()) };
                let new: openarch_model::areas::AreaBoundary = openarch_model::doc::apply_patch(&old, p.get("patch").ok_or("missing patch")?)?;
                if old.pinned && new.pinned && new.curve != old.curve { return Err("Unpin the boundary before changing it.".into()); }
                let new = openarch_model::areas::AreaBoundary { view: old.view, ..new };
                self.doc.transact("Area boundary", |tx| tx.update(id, Element::AreaBoundary(new)))?;
                Ok(json!({}))
            })(),
            // { view_id, point? (none: an unplaced area, a schedule row), tag }
            "add_area" => (|| {
                let at = if p["point"].is_array() { Some(point_of(&p["point"])?) } else { None };
                Ok(json!({ "id": self.doc.add_area(id_of(p, "view_id")?, at, p["tag"].as_bool().unwrap_or(true))? }))
            })(),
            "place_area" => (|| { self.doc.patch_area(id_of(p, "id")?, &json!({ "at": point_of(&p["point"])? }))?; Ok(json!({})) })(),
            "patch_area" => (|| { self.doc.patch_area(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "unplace_area" => (|| { self.doc.unplace_area(id_of(p, "id")?)?; Ok(json!({})) })(),
            "place_all_areas" => (|| Ok(json!({ "ids": self.doc.place_all_areas(id_of(p, "view_id")?, p["tag"].as_bool().unwrap_or(true))? })))(),
            "area_at" => (|| Ok(json!({ "id": self.doc.area_at(id_of(p, "view_id")?, point_of(&p["point"])?) })))(),
            "add_area_tag" => (|| {
                let view = id_of(p, "view_id")?;
                let at = point_of(&p["point"])?;
                let area = match p["area_id"].as_u64() { Some(a) => a, None => self.doc.area_at(view, at).ok_or("Click inside a placed area.")? };
                Ok(json!({ "id": self.doc.add_area_tag(view, area, at)? }))
            })(),
            "patch_area_tag" => (|| {
                let id = id_of(p, "id")?;
                let Some(Element::AreaTag(old)) = self.doc.get(id).cloned() else { return Err("not an area tag".into()) };
                let new: openarch_model::areas::AreaTag = openarch_model::doc::apply_patch(&old, p.get("patch").ok_or("missing patch")?)?;
                if old.pinned && new.pinned && new.at != old.at { return Err("Unpin the tag before moving it.".into()); }
                let new = openarch_model::areas::AreaTag { view: old.view, area: old.area, ..new };
                self.doc.transact("Area tag", |tx| tx.update(id, Element::AreaTag(new)))?;
                Ok(json!({}))
            })(),
            _ => return None,
        })
    }

    pub(crate) fn area_props(&self, id: Id, a: &Area) -> Value {
        let d = &self.doc;
        let c = d.area_calculation(id).unwrap();
        let scheme = d.scheme_of_area(a).and_then(|s| d.area_scheme(s));
        let rules = scheme.map_or(SchemeRules::GrossBuilding, |s| s.rules);
        let placed = c.status == AreaStatus::Placed;
        let types: Vec<Value> = AreaType::of(rules).iter().map(|t| json!({ "label": t.label(), "patch": { "area_type": t }, "selected": *t == a.area_type })).collect();
        let text = |label: &str, path: &str, v: &str| json!({ "label": label, "path": path, "kind": "text", "value": v });
        let ro = |label: &str, v: String, raw: Option<f64>, unit: &str| json!({ "label": label, "kind": "ro", "value": v, "raw": raw, "parameter_unit": unit });
        let dup = d.areas().iter().any(|(o, x)| *o != id && x.number == a.number && d.scheme_of_area(x) == d.scheme_of_area(a));
        let mut identity = vec![text("Number", "number", &a.number), text("Name", "name", &a.name), text("Comments", "comments", &a.comments), json!({ "label": "Area Type", "kind": "select", "value": a.area_type.label(), "options": types }), ro("Status", status_label(c.status).into(), None, "text")];
        if dup { identity.push(ro("Warning", "Another area in this scheme has the same number.".into(), None, "text")); }
        json!({
            "id": id, "kind": "Area", "category": area_category(d, a), "title": format!("Area: {} {}", a.number, a.name), "type_label": "Area",
            "target": { "method": "patch_area", "id": id },
            "groups": [
                { "name": "Constraints", "rows": [
                    ro("Level", d.level_of_area(a).and_then(|l| d.level(l)).map_or(String::new(), |l| l.name.clone()), None, "text"),
                    ro("Area Scheme", scheme.map_or(String::new(), |s| s.name.clone()), None, "text"),
                    { "label": "Pinned", "kind": "bool", "path": "pinned", "raw": a.pinned, "value": if a.pinned { "Yes" } else { "No" } },
                ] },
                { "name": "Dimensions", "rows": [
                    ro("Area", if placed { format!("{:.2} m\u{b2}", c.area_mm2 / 1e6) } else { status_label(c.status).into() }, placed.then_some(c.area_mm2 / 1e6), "area"),
                    ro("Perimeter", format!("{:.0}", c.perimeter_mm), placed.then_some(c.perimeter_mm), "length"),
                ] },
                { "name": "Identity Data", "rows": identity },
            ],
        })
    }

    pub(crate) fn area_tag_props(&self, id: Id, t: &openarch_model::areas::AreaTag) -> Value {
        let d = &self.doc;
        let fams: Vec<Value> = d.annotation_families().into_iter().filter(|(_, f)| f.kind == openarch_model::annotation::AnnotationKind::AreaTag).map(|(fid, f)| json!({ "label": f.name, "patch": { "family": fid }, "selected": Some(fid) == t.family })).collect();
        json!({ "id": id, "kind": "AreaTag", "title": "Area Tag", "type_label": "Area Tag", "target": { "method": "patch_area_tag", "id": id }, "groups": [
            { "name": "Identity Data", "rows": [{ "label": "Area", "kind": "ro", "value": d.area(t.area).map_or(String::new(), |a| format!("{} {}", a.number, a.name)) }] },
            { "name": "Graphics", "rows": [
                { "label": "Leader", "kind": "bool", "path": "leader", "raw": t.leader, "value": if t.leader { "Yes" } else { "No" } },
                { "label": "Family", "kind": "select", "value": t.family.and_then(|f| d.annotation_family(f)).map_or(String::new(), |f| f.name.clone()), "options": fams },
                { "label": "Pinned", "kind": "bool", "path": "pinned", "raw": t.pinned, "value": if t.pinned { "Yes" } else { "No" } },
            ] },
        ] })
    }
}

/// What an area plan draws: its areas (filled faces), boundary lines and area tags.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if v.view_kind != ViewKind::AreaPlan { return json!({}); }
    let settings = d.effective_settings(view).unwrap_or_default();
    let shown = |cat: &str| settings.overrides.model.get(cat).or_else(|| settings.overrides.annotation.get(cat)).is_none_or(|o| o.visible);
    let rings = |r: &openarch_model::geom::Region| std::iter::once(&r.outer).chain(&r.holes).map(|ring| ring.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>();
    let areas: Vec<Value> = d.areas().into_iter().filter(|(id, a)| a.view == view && a.at.is_some() && shown("Areas") && !v.hidden_elements.contains(id)).map(|(id, a)| {
        let c = d.area_calculation(id).unwrap();
        json!({ "id": id, "number": a.number, "name": a.name, "at": a.at, "pinned": a.pinned, "status": c.status, "rings": c.region.as_ref().map(rings).unwrap_or_default() })
    }).collect();
    let boundaries: Vec<Value> = d.area_boundaries(view).into_iter().filter(|(id, _)| shown("Area Boundary") && !v.hidden_elements.contains(id)).map(|(id, b)| json!({ "id": id, "pinned": b.pinned, "pts": b.curve.tessellate(1.5).iter().map(|p| [p.x, p.y]).collect::<Vec<_>>() })).collect();
    let tags: Vec<Value> = d.area_tags(view).into_iter().filter(|(id, _)| shown("Area Tags") && !v.hidden_elements.contains(id)).map(|(id, t)| {
        let fam = t.family.and_then(|f| d.annotation_family(f));
        json!({ "id": id, "area": t.area, "at": t.at, "pinned": t.pinned, "leader": t.leader, "family": fam.map(|f| d.symbol_drawing(f, false)), "labels": d.area_tag_labels(t), "area_at": d.area(t.area).and_then(|a| a.at) })
    }).collect();
    json!({ "areas": areas, "area_boundaries": boundaries, "area_tags": tags })
}
