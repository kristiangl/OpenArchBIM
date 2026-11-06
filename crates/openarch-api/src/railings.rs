//! Railings: Sketch Path and Edit Path, Place on Stair, Properties, Type Properties, and what a plan draws (each part
//! seen from above; parts above the cut plane dashed, balusters and posts above it left out).
use serde_json::{json, Value};

use openarch_model::railing::{RailPart, RailShape, Railing, RailingPlacement, RailingType};
use openarch_model::curve::Curve;
use openarch_model::view::ViewKind;
use openarch_model::{Document, Id};

use crate::graphics::{style_for, Hint};
use crate::props::{boolean, enum_select, group, name_of, num, ro, select, text};
use crate::{id_of, Res, Session};

fn curves_of(p: &Value) -> Result<Vec<Curve>, String> {
    p["curves"].as_array().ok_or("missing curves")?.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad curve: {e}"))).collect()
}

fn placement_of(p: &Value) -> RailingPlacement {
    serde_json::from_value(p["placement"].clone()).unwrap_or_default()
}

impl Session {
    pub(crate) fn handle_railings(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "ensure_railing_types" => (|| { self.doc.ensure_railing_types()?; Ok(json!({})) })(),
            // Sketch Path's Finish: { type_id, level_id, curves }
            "add_railing" => (|| Ok(json!({ "id": self.doc.add_railing(id_of(p, "type_id")?, id_of(p, "level_id")?, &curves_of(p)?)? })))(),
            // Edit Path: the path to reopen, and its Finish
            "railing_path" => (|| {
                let r = self.doc.railing(id_of(p, "id")?).ok_or("not a railing")?;
                if r.host.is_some() { return Err("A railing on a stair follows the stair: edit the stair.".into()); }
                // as drawn (arcs stay arcs), or its points joined by lines
                let curves: Vec<Value> = if r.curves.is_empty() { r.path.windows(2).map(|w| crate::describe_curve(&Curve::line(w[0], w[1]))).collect() } else { r.curves.iter().map(crate::describe_curve).collect() };
                Ok(json!({ "level_id": r.level, "type_id": r.railing_type, "curves": curves }))
            })(),
            "set_railing_path" => (|| { self.doc.set_railing_path(id_of(p, "id")?, &curves_of(p)?)?; Ok(json!({})) })(),
            // Place on Stair: { stair_id, type_id, placement: treads | stringer }
            "add_stair_railings" => (|| Ok(json!({ "ids": self.doc.add_stair_railings(id_of(p, "stair_id")?, id_of(p, "type_id")?, placement_of(p))? })))(),
            "patch_railing" => (|| { self.doc.patch_railing(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "patch_railing_type" => (|| { self.doc.patch_railing_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "duplicate_railing_type" => (|| Ok(json!({ "id": self.doc.duplicate_railing_type(id_of(p, "id")?, p["name"].as_str().ok_or("missing name")?)? })))(),
            _ => return None,
        })
    }

    pub(crate) fn railing_props(&self, id: Id, r: &Railing) -> Value {
        let d = &self.doc;
        let t = d.railing_type(r.railing_type);
        let free = !r.pinned;
        let pieces = d.railing_pieces(r);
        let count = |k: RailPart| pieces.iter().filter(|p| p.part == k).count();
        let warnings = d.railing_warnings(r);
        let hosted = r.host.is_some();
        let mut rules: Vec<Value> = warnings.iter().map(|m| json!({ "label": "Warning", "value": m, "kind": "ro", "warning": true })).collect();
        if rules.is_empty() { rules.push(ro("Maximum Opening", "Met")); }
        let mut constraints = vec![
            ro("Level", d.level(r.level).map_or(String::new(), |l| l.name.clone())),
            ro("Host", if hosted { if r.host.is_some_and(|h| d.ramp(h.stair).is_some()) { "Ramp" } else { "Stair" } } else if r.floor_host.is_some() { "Floor" } else { "Level" }),
        ];
        if let Some(h) = r.host {
            constraints.push(ro("Side", if h.side == openarch_model::railing::RailingSide::Left { "Left" } else { "Right" }));
            constraints.push(enum_select("Placement", "placement", &name_of(&r.placement), &[("treads", "Treads"), ("stringer", "Stringer")]));
            constraints.push(boolean("Flipped", "flipped", r.flipped, free));
        } else {
            constraints.push(num("Base Offset", "base_offset_mm", r.base_offset_mm, free));
            constraints.push(boolean("Flipped", "flipped", r.flipped, free));
        }
        constraints.push(enum_select("Slope", "slope", &name_of(&r.slope), &[("by_host", "By Host"), ("flat", "Flat"), ("sloped", "Sloped")]));
        constraints.push(boolean("Pinned", "pinned", r.pinned, true));
        let len = d.railing_length(r);
        json!({
            "id": id, "kind": "Railing", "category": "Railings", "type_id": r.railing_type,
            "title": format!("Railing: {}", t.map_or("?", |t| t.name.as_str())),
            "target": { "method": "patch_railing", "id": id },
            "warnings": warnings, "hosted": hosted,
            "groups": [
                group("Constraints", constraints),
                group("Dimensions", vec![
                    json!({ "label": "Length", "kind": "ro", "value": format!("{len:.0}"), "raw": len }),
                    json!({ "label": "Baluster Count", "kind": "ro", "value": count(RailPart::Baluster).to_string(), "raw": count(RailPart::Baluster) }),
                    json!({ "label": "Post Count", "kind": "ro", "value": count(RailPart::Post).to_string(), "raw": count(RailPart::Post) }),
                ]),
                group("Rules", rules),
                group("Identity Data", vec![text("Comments", "comments", &r.comments), text("Mark", "mark", &r.mark)]),
            ],
        })
    }
}

pub(crate) fn railing_type_props(d: &Document, id: Id, t: &RailingType) -> Value {
    let mats: Vec<String> = d.materials().into_iter().map(|(_, m)| m.name.clone()).collect();
    let material = |label: &str, path: &str, cur: &str| {
        let mut o = vec![("<By Category>".to_string(), crate::props::nest(path, json!("")), cur.is_empty())];
        o.extend(mats.iter().map(|m| (m.clone(), crate::props::nest(path, json!(m)), m == cur)));
        select(label, o)
    };
    // the starter balustrade gives a part's defaults when one is switched on
    let starter = openarch_model::railing::starter_railing_types().into_iter().next();
    let def = |part: &str| -> Value { starter.as_ref().and_then(|s| serde_json::to_value(s).ok()).map_or(Value::Null, |v| v[part].clone()) };
    let profile_choice = |label: &str, part: &str, cur: Option<RailShape>, whole: Value| {
        let with = |shape: &str| { let mut v = if whole.is_null() { def(part) } else { whole.clone() }; v["profile"]["shape"] = json!(shape); json!({ part: v }) };
        select(label, vec![
            ("None".to_string(), json!({ part: null }), cur.is_none()),
            ("Round".to_string(), with("round"), cur == Some(RailShape::Round)),
            ("Rectangular".to_string(), with("rectangular"), cur == Some(RailShape::Rectangular)),
        ])
    };
    let tv = serde_json::to_value(t).unwrap_or_default();
    let mut top = vec![profile_choice("Top Rail", "top_rail", t.top_rail.as_ref().map(|r| r.profile.shape), tv["top_rail"].clone())];
    if let Some(r) = &t.top_rail {
        top.extend([
            num("Height", "top_rail.height_mm", r.height_mm, true),
            num("Height on Flights", "top_rail.flight_height_mm", r.flight_height_mm, true),
            num(if r.profile.shape == RailShape::Round { "Diameter" } else { "Width" }, "top_rail.profile.width_mm", r.profile.width_mm, true),
            num("Depth", "top_rail.profile.height_mm", r.profile.height_mm, r.profile.shape == RailShape::Rectangular),
            material("Material", "top_rail.profile.material", &r.profile.material),
        ]);
    }
    let hand = |n: u8, h: &Option<openarch_model::railing::Handrail>| {
        let part = if n == 1 { "handrail_1" } else { "handrail_2" };
        let mut rows = vec![profile_choice(&format!("Handrail {n}"), part, h.as_ref().map(|x| x.profile.shape), tv[part].clone())];
        if let Some(h) = h {
            rows.extend([
                num("Height", &format!("{part}.height_mm"), h.height_mm, true),
                num("Offset", &format!("{part}.offset_mm"), h.offset_mm, true),
                num(if h.profile.shape == RailShape::Round { "Diameter" } else { "Width" }, &format!("{part}.profile.width_mm"), h.profile.width_mm, true),
                num("Extension at Top", &format!("{part}.extension_top_mm"), h.extension_top_mm, true),
                num("Extension at Bottom", &format!("{part}.extension_bottom_mm"), h.extension_bottom_mm, true),
                enum_select("Termination", &format!("{part}.termination"), &name_of(&h.termination), &[("none", "None"), ("wall", "Wall"), ("floor", "Floor"), ("post", "Post")]),
                num("Wall Clearance", &format!("{part}.clearance_mm"), h.clearance_mm, true),
                num("Bracket Spacing", &format!("{part}.bracket_spacing_mm"), h.bracket_spacing_mm, true),
                material("Material", &format!("{part}.profile.material"), &h.profile.material),
            ]);
        }
        rows
    };
    let mut bal = vec![select("Balusters", vec![("None".to_string(), json!({ "balusters": null }), t.balusters.is_none()), ("Square".to_string(), json!({ "balusters": if tv["balusters"].is_null() { def("balusters") } else { tv["balusters"].clone() } }), t.balusters.is_some())])];
    if let Some(b) = &t.balusters {
        bal.extend([
            num("Width", "balusters.width_mm", b.width_mm, true),
            num("Spacing", "balusters.spacing_mm", b.spacing_mm, true),
            enum_select("Justify", "balusters.justification", &name_of(&b.justification), &[("beginning", "Beginning"), ("end", "End"), ("center", "Center"), ("spread_to_fit", "Spread Pattern to Fit")]),
            num("Balusters per Tread on Stairs", "balusters.per_tread", b.per_tread.unwrap_or(0) as f64, true),
            material("Material", "balusters.material", &b.material),
        ]);
    }
    let mut posts = vec![select("Posts", vec![("None".to_string(), json!({ "posts": null }), t.posts.is_none()), ("Square".to_string(), json!({ "posts": if tv["posts"].is_null() { def("posts") } else { tv["posts"].clone() } }), t.posts.is_some())])];
    if let Some(p) = &t.posts {
        posts.extend([
            num("Width", "posts.width_mm", p.width_mm, true),
            boolean("Corner Posts", "posts.corners", p.corners, true),
            num("Corner Angle", "posts.corner_angle_deg", p.corner_angle_deg, p.corners),
            material("Material", "posts.material", &p.material),
        ]);
    }
    json!({
        "id": id, "kind": "RailingType", "title": t.name,
        "siblings": d.railing_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "groups": [
            group("Top Rail", top),
            group("Handrail 1", hand(1, &t.handrail_1)),
            group("Handrail 2", hand(2, &t.handrail_2)),
            group("Balusters", bal),
            group("Posts", posts),
            group("Rules", vec![num("Maximum Opening", "max_gap_mm", t.max_gap_mm, true)]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// What a plan draws for its railings: each part from above. On a stair the parts above the cut plane are dashed
/// (rails) or left out (balusters and posts), as the stair's own lines are.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if !matches!(v.view_kind, ViewKind::FloorPlan | ViewKind::AreaPlan | ViewKind::StructuralPlan) { return json!({ "railings": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    let Some(range) = d.resolve_view_range(v, &settings) else { return json!({ "railings": [] }) };
    let mut out = vec![];
    for (id, r) in d.railings() {
        if v.hidden_elements.contains(&id) { continue; }
        let eo = v.element_overrides.get(&id);
        let Some(line) = style_for(d, &settings, "Railings", None, eo, "projection", &Hint::default()) else { continue };
        let Some(host) = d.railing_host_line(r) else { continue };
        let (lo, hi) = host.pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.1), b.max(p.1)));
        if lo > range.cut + 1e-6 || hi < range.depth - 1.0 { continue; }
        let mut polys = vec![];
        // Coarse: the railing as a line along its path
        if settings.detail_level == openarch_model::view::DetailLevel::Coarse {
            polys.push(json!({ "pts": host.pts.iter().map(|(p, _)| [p.x, p.y]).collect::<Vec<_>>(), "above": false, "open": true }));
        }
        for p in d.railing_pieces_at(r, settings.detail_level).into_iter().filter(|_| settings.detail_level != openarch_model::view::DetailLevel::Coarse) {
            let (z0, _) = p.solid.z_range();
            let above = z0 > range.cut + 1e-6;
            if above && matches!(p.part, RailPart::Baluster | RailPart::Post) { continue; }
            let hull: Vec<[f64; 2]> = p.solid.plan_hull().iter().map(|q| [q.x, q.y]).collect();
            if hull.len() >= 3 { polys.push(json!({ "pts": hull, "above": above })); }
        }
        // for picking: a band along the path
        let rings: Vec<Vec<[f64; 2]>> = host.pts.windows(2).filter_map(|w| {
            let (a, b) = (w[0].0, w[1].0);
            let l = a.dist(&b);
            if l < 1.0 { return None; }
            let (nx, ny) = (-(b.y - a.y) / l * 100.0, (b.x - a.x) / l * 100.0);
            Some(vec![[a.x + nx, a.y + ny], [b.x + nx, b.y + ny], [b.x - nx, b.y - ny], [a.x - nx, a.y - ny]])
        }).collect();
        out.push(json!({ "id": id, "pinned": r.pinned, "hosted": r.host.is_some(), "flipped": r.flipped, "floor_host": r.floor_host, "polys": polys, "rings": rings, "style": line }));
    }
    json!({ "railings": out })
}
