//! Strip footings (Revit's Wall Foundation): their calls, Properties and type properties, and how a plan draws them
//! (foundation.rs in the core).
use serde_json::{json, Value};

use openarch_model::foundation::{WallFoundation, WallFoundationType};
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

/// The footings a plan shows, dashed (they are below the cut): each one's outline for picking, and the union of
/// them all for drawing, so corners and tees read as one footing.
pub(crate) fn plan_drawing(d: &Document, view: Id, shown: bool) -> Value {
    if !shown {
        return json!({ "wall_foundations": [], "foundation_outline": [] });
    }
    let list = d.wall_foundations_in(view);
    let pts = |r: &[openarch_model::Point]| r.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>();
    let union = openarch_model::layers::union_rings(list.iter().map(|(_, r)| r));
    json!({
        "wall_foundations": list.iter().map(|(id, r)| json!({ "id": id, "ring": pts(r), "pinned": d.wall_foundation(*id).is_some_and(|f| f.pinned) })).collect::<Vec<_>>(),
        "foundation_outline": union.iter().map(|shape| shape.iter().map(|r| pts(r)).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}
