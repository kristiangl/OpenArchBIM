//! Strip footings (Revit's Wall Foundation): their calls, Properties and type properties, and how a plan draws them
//! (foundation.rs in the core).
use serde_json::{json, Value};

use openarch_model::foundation::{IsolatedFooting, IsolatedFootingType, SlabEdge, SlabEdgeType, WallFoundation, WallFoundationType};
use openarch_model::{Document, Id};

use crate::props::{boolean, group, num, ro, select, text};
use crate::{id_of, Res, Session};

impl Session {
    pub(crate) fn handle_foundations(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // Strip Footing: { walls: [ids], type_id? }
            "add_wall_foundations" => (|| {
                let walls = crate::ids_of(p, "walls")?;
                Ok(json!({ "ids": self.doc.add_wall_foundations(&walls, p.get("type_id").and_then(|v| v.as_u64()))? }))
            })(),
            "patch_wall_foundation" => (|| { self.doc.patch_wall_foundation(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "patch_wall_foundation_type" => (|| { self.doc.patch_wall_foundation_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            // Isolated Footing: { level_id, points: [[x, y]], type_id? }, or { columns: [ids] } for At Columns
            "add_isolated_footings" => (|| {
                let t = p.get("type_id").and_then(|v| v.as_u64());
                if p.get("columns").is_some() {
                    return Ok(json!({ "ids": self.doc.add_isolated_footings_at_columns(&crate::ids_of(p, "columns")?, t)? }));
                }
                let pts: Vec<[f64; 2]> = serde_json::from_value(p.get("points").cloned().ok_or("missing points")?).map_err(|e| format!("bad points: {e}"))?;
                let at: Vec<openarch_model::Point> = pts.iter().map(|q| openarch_model::Point::new(q[0], q[1])).collect();
                Ok(json!({ "ids": self.doc.add_isolated_footings(id_of(p, "level_id")?, &at, t)? }))
            })(),
            "patch_isolated_footing" => (|| { self.doc.patch_isolated_footing(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "patch_isolated_footing_type" => (|| { self.doc.patch_isolated_footing_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "duplicate_isolated_footing_type" => (|| Ok(json!({ "id": self.doc.duplicate_isolated_footing_type(id_of(p, "id")?, p["name"].as_str().ok_or("missing name")?)? })))(),
            // Slab / Edge Thickening: { floors: [ids], type_id? }
            "add_slab_edges" => (|| Ok(json!({ "ids": self.doc.add_slab_edges(&crate::ids_of(p, "floors")?, p.get("type_id").and_then(|v| v.as_u64()))? })))(),
            "patch_slab_edge" => (|| { self.doc.patch_slab_edge(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "patch_slab_edge_type" => (|| { self.doc.patch_slab_edge_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "duplicate_slab_edge_type" => (|| Ok(json!({ "id": self.doc.duplicate_slab_edge_type(id_of(p, "id")?, p["name"].as_str().ok_or("missing name")?)? })))(),
            // Model Text: { level_id, point, text, size_mm?, depth_mm?, upright?, rotation_deg? }
            "add_model_text" => (|| {
                let q = p["point"].as_array().ok_or("missing point")?;
                let at = openarch_model::Point::new(q[0].as_f64().unwrap_or(f64::NAN), q.get(1).and_then(|v| v.as_f64()).unwrap_or(f64::NAN));
                let mut t = openarch_model::modeltext::ModelText::new(id_of(p, "level_id")?, at, p["text"].as_str().unwrap_or(""));
                if let Some(v) = p["size_mm"].as_f64() { t.size_mm = v; }
                if let Some(v) = p["depth_mm"].as_f64() { t.depth_mm = v; }
                if let Some(v) = p["rotation_deg"].as_f64() { t.rotation_deg = v; }
                t.upright = p["upright"].as_bool().unwrap_or(false);
                Ok(json!({ "id": self.doc.add_model_text(t)? }))
            })(),
            "patch_model_text" => (|| { self.doc.patch_model_text(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "duplicate_wall_foundation_type" => (|| Ok(json!({ "id": self.doc.duplicate_wall_foundation_type(id_of(p, "id")?, p["name"].as_str().ok_or("missing name")?)? })))(),
            _ => return None,
        })
    }
}

/// A strip footing's Properties: its type, eccentricity, pinned, and its size.
pub(crate) fn foundation_props(d: &Document, id: Id, f: &WallFoundation) -> Value {
    let types: Vec<(String, Value, bool)> = d.wall_foundation_types().into_iter().map(|(tid, t)| (t.name.clone(), json!({ "foundation_type": tid }), tid == f.foundation_type)).collect();
    let t = d.wall_foundation_type(f.foundation_type);
    let (len, vol) = d.wall_foundation_shape(id).map_or((0.0, 0.0), |(r, z0, z1)| {
        let area = openarch_model::geom::signed_area(&r).abs();
        (t.map_or(0.0, |t| area / t.width_mm.max(1.0)), area * (z1 - z0))
    });
    json!({
        "id": id, "kind": "WallFoundation", "category": "Structural Foundations", "title": "Wall Foundation", "type_label": t.map_or(String::new(), |t| t.name.clone()),
        "type_id": f.foundation_type,
        "target": { "method": "patch_wall_foundation", "id": id },
        "groups": [
            group("Constraints", vec![num("Eccentricity", "eccentricity_mm", f.eccentricity_mm, !f.pinned), boolean("Pinned", "pinned", f.pinned, true)]),
            group("Type", vec![select("Type", types)]),
            group("Dimensions", vec![
                ro("Width", t.map_or(String::new(), |t| format!("{:.0}", t.width_mm))),
                ro("Foundation Thickness", t.map_or(String::new(), |t| format!("{:.0}", t.thickness_mm))),
                ro("Length", format!("{len:.0}")),
                ro("Volume", d.project_units().volume(vol)),
            ]),
            group("Identity Data", vec![ro("Wall", format!("{}", f.wall))]),
        ],
    })
}

/// A wall foundation type's properties.
pub(crate) fn foundation_type_props(d: &Document, id: Id, t: &WallFoundationType) -> Value {
    let mats: Vec<(String, Value, bool)> = std::iter::once(("<By Category>".to_string(), json!({ "material": "" }), t.material.is_empty()))
        .chain(d.materials().into_iter().map(|(_, m)| (m.name.clone(), json!({ "material": m.name }), m.name == t.material)))
        .collect();
    json!({
        "id": id, "kind": "WallFoundationType", "title": t.name, "family": "Wall Foundation",
        "siblings": d.wall_foundation_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "patch": "patch_wall_foundation_type", "duplicate": "duplicate_wall_foundation_type",
        "groups": [
            group("Materials and Finishes", vec![select("Structural Material", mats)]),
            group("Dimensions", vec![
                num("Width", "width_mm", t.width_mm, true),
                num("Foundation Thickness", "thickness_mm", t.thickness_mm, true),
                num("Default End Extension Length", "end_extension_mm", t.end_extension_mm, true),
            ]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// A pad footing's Properties.
pub(crate) fn isolated_props(d: &Document, id: Id, f: &IsolatedFooting) -> Value {
    let types: Vec<(String, Value, bool)> = d.isolated_footing_types().into_iter().map(|(tid, t)| (t.name.clone(), json!({ "footing_type": tid }), tid == f.footing_type)).collect();
    let t = d.isolated_footing_type(f.footing_type);
    let free = !f.pinned;
    json!({
        "id": id, "kind": "IsolatedFooting", "category": "Structural Foundations", "title": "Isolated Foundation", "type_label": t.map_or(String::new(), |t| t.name.clone()),
        "type_id": f.footing_type,
        "target": { "method": "patch_isolated_footing", "id": id },
        "groups": [
            group("Constraints", vec![
                crate::props::level_select(d, "Level", "level", f.level, free),
                num("Height Offset From Level", "offset_mm", f.offset_mm, free),
                num("Rotation", "rotation_deg", f.rotation_deg, free),
                boolean("Pinned", "pinned", f.pinned, true),
            ]),
            group("Type", vec![select("Type", types)]),
            group("Dimensions", vec![
                ro("Width", t.map_or(String::new(), |t| format!("{:.0}", t.width_mm))),
                ro("Length", t.map_or(String::new(), |t| format!("{:.0}", t.length_mm))),
                ro("Foundation Thickness", t.map_or(String::new(), |t| format!("{:.0}", t.thickness_mm))),
                ro("Volume", t.map_or(String::new(), |t| d.project_units().volume(t.width_mm * t.length_mm * t.thickness_mm))),
            ]),
        ],
    })
}

/// A pad footing type's properties.
pub(crate) fn isolated_type_props(d: &Document, id: Id, t: &IsolatedFootingType) -> Value {
    let mats: Vec<(String, Value, bool)> = std::iter::once(("<By Category>".to_string(), json!({ "material": "" }), t.material.is_empty()))
        .chain(d.materials().into_iter().map(|(_, m)| (m.name.clone(), json!({ "material": m.name }), m.name == t.material)))
        .collect();
    json!({
        "id": id, "kind": "IsolatedFootingType", "title": t.name, "family": "Isolated Foundation",
        "siblings": d.isolated_footing_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "patch": "patch_isolated_footing_type", "duplicate": "duplicate_isolated_footing_type",
        "groups": [
            group("Materials and Finishes", vec![select("Structural Material", mats)]),
            group("Dimensions", vec![
                num("Width", "width_mm", t.width_mm, true),
                num("Length", "length_mm", t.length_mm, true),
                num("Foundation Thickness", "thickness_mm", t.thickness_mm, true),
            ]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// A thickened slab edge's Properties.
pub(crate) fn slab_edge_props(d: &Document, id: Id, s: &SlabEdge) -> Value {
    let types: Vec<(String, Value, bool)> = d.slab_edge_types().into_iter().map(|(tid, t)| (t.name.clone(), json!({ "edge_type": tid }), tid == s.edge_type)).collect();
    let t = d.slab_edge_type(s.edge_type);
    let vol = d.slab_edge_shape(id).map_or(0.0, |sh| sh.pieces.iter().map(|r| openarch_model::geom::signed_area(r).abs()).sum::<f64>() * (sh.z1 - sh.z0));
    json!({
        "id": id, "kind": "SlabEdge", "category": "Structural Foundations", "title": "Slab Edge", "type_label": t.map_or(String::new(), |t| t.name.clone()),
        "type_id": s.edge_type,
        "target": { "method": "patch_slab_edge", "id": id },
        "groups": [
            group("Constraints", vec![boolean("Pinned", "pinned", s.pinned, true)]),
            group("Type", vec![select("Type", types)]),
            group("Dimensions", vec![
                ro("Width", t.map_or(String::new(), |t| format!("{:.0}", t.width_mm))),
                ro("Depth Below Slab", t.map_or(String::new(), |t| format!("{:.0}", t.depth_mm))),
                ro("Volume", d.project_units().volume(vol)),
            ]),
            group("Identity Data", vec![ro("Floor", format!("{}", s.floor))]),
        ],
    })
}

/// A thickened slab edge type's properties.
pub(crate) fn slab_edge_type_props(d: &Document, id: Id, t: &SlabEdgeType) -> Value {
    let mats: Vec<(String, Value, bool)> = std::iter::once(("<By Category>".to_string(), json!({ "material": "" }), t.material.is_empty()))
        .chain(d.materials().into_iter().map(|(_, m)| (m.name.clone(), json!({ "material": m.name }), m.name == t.material)))
        .collect();
    json!({
        "id": id, "kind": "SlabEdgeType", "title": t.name, "family": "Slab Edge",
        "siblings": d.slab_edge_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "patch": "patch_slab_edge_type", "duplicate": "duplicate_slab_edge_type",
        "groups": [
            group("Materials and Finishes", vec![select("Structural Material", mats)]),
            group("Dimensions", vec![num("Width", "width_mm", t.width_mm, true), num("Depth Below Slab", "depth_mm", t.depth_mm, true)]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// Model text's Properties.
pub(crate) fn model_text_props(d: &Document, id: Id, t: &openarch_model::modeltext::ModelText) -> Value {
    let free = !t.pinned;
    let mats: Vec<(String, Value, bool)> = std::iter::once(("<By Category>".to_string(), json!({ "material": "" }), t.material.is_empty()))
        .chain(d.materials().into_iter().map(|(_, m)| (m.name.clone(), json!({ "material": m.name }), m.name == t.material)))
        .collect();
    let align = |v: &str, l: &str| (l.to_string(), json!({ "align": v }), serde_json::to_value(t.align).ok() == Some(json!(v)));
    json!({
        "id": id, "kind": "ModelText", "category": "Generic Models", "title": "Model Text", "type_label": "Model Text",
        "target": { "method": "patch_model_text", "id": id },
        "groups": [
            group("Constraints", vec![
                crate::props::level_select(d, "Level", "level", t.level, free),
                num("Offset From Level", "offset_mm", t.offset_mm, free),
                num("Rotation", "rotation_deg", t.rotation_deg, free),
                boolean("Upright", "upright", t.upright, free),
                boolean("Pinned", "pinned", t.pinned, true),
            ]),
            group("Text", vec![
                text("Text", "text", &t.text),
                num("Size", "size_mm", t.size_mm, true),
                boolean("Bold", "bold", t.bold, true),
                boolean("Italic", "italic", t.italic, true),
                select("Horizontal Align", vec![align("left", "Left"), align("center", "Center"), align("right", "Right")]),
                ro("Font", t.font.clone()),
            ]),
            group("Dimensions", vec![num("Depth", "depth_mm", t.depth_mm, true)]),
            group("Materials and Finishes", vec![select("Material", mats)]),
        ],
    })
}

/// Model text a plan shows: each one's letter outlines (or the strip upright text stands on).
pub(crate) fn model_text_drawing(d: &Document, view: Id, shown: bool) -> Value {
    if !shown { return json!({ "model_texts": [] }); }
    json!({ "model_texts": d.model_texts_in(view).into_iter().map(|(id, rings)| json!({
        "id": id, "rings": rings.iter().map(|r| r.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "upright": d.model_text(id).is_some_and(|t| t.upright), "pinned": d.model_text(id).is_some_and(|t| t.pinned),
    })).collect::<Vec<_>>() })
}

/// The footings a plan shows, dashed (they are below the cut): each one's outline for picking, and the union of
/// them all for drawing, so corners and tees read as one footing.
pub(crate) fn plan_drawing(d: &Document, view: Id, shown: bool) -> Value {
    if !shown {
        return json!({ "wall_foundations": [], "slab_edges": [], "foundation_outline": [] });
    }
    let list = d.wall_foundations_in(view);
    let edges = d.slab_edges_in(view);
    let pts = |r: &[openarch_model::Point]| r.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>();
    let union = openarch_model::layers::union_rings(list.iter().map(|(_, r)| r).chain(edges.iter().flat_map(|(_, s)| s.pieces.iter())));
    json!({
        "wall_foundations": list.iter().map(|(id, r)| json!({ "id": id, "ring": pts(r), "pinned": d.wall_foundation(*id).map(|f| f.pinned).or_else(|| d.isolated_footing(*id).map(|f| f.pinned)).unwrap_or(false), "isolated": d.isolated_footing(*id).is_some() })).collect::<Vec<_>>(),
        // a slab edge is picked by the band between its outer loop and the loop set in from it
        "slab_edges": edges.iter().map(|(id, sh)| json!({ "id": id, "rings": sh.outer.iter().chain(sh.inner.iter()).map(|r| pts(r)).collect::<Vec<_>>(), "pinned": d.slab_edge(*id).is_some_and(|s| s.pinned) })).collect::<Vec<_>>(),
        "foundation_outline": union.iter().map(|shape| shape.iter().map(|r| pts(r)).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}
