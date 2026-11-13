//! Ramps: the Ramp tool (a draft laid out as runs are drawn, then Finish), Edit Ramp, Properties, Type Properties, and
//! what a plan draws (the outline, solid below the cut and dashed above, the cut line, the walkline with UP or DN and
//! each run's slope).
use serde_json::{json, Value};

use openarch_model::ramp::{Ramp, RampRun, RampType};
use openarch_model::stairs::{CutMarkStyle, RunJustification};
use openarch_model::view::ViewKind;
use openarch_model::{Document, Id, Point};

use crate::graphics::{style_for, Hint};
use crate::props::{enum_select, group, name_of, num, ro, select, text};
use crate::stairs::{cut_mark, preview_lines, pt, split, walkline};
use crate::{id_of, point_of, Res, Session};

fn runs_of(p: &Value) -> Result<Vec<RampRun>, String> {
    match p.get("runs") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|e| format!("bad runs: {e}")),
        _ => Ok(vec![]),
    }
}

fn draft_of(d: &Document, p: &Value) -> Result<Ramp, String> {
    let mut r = d.new_ramp(id_of(p, "type_id")?, id_of(p, "base_level")?, p["top_level"].as_u64())?;
    if let Some(v) = p["base_offset_mm"].as_f64() { r.base_offset_mm = v; }
    if let Some(v) = p["top_offset_mm"].as_f64() { r.top_offset_mm = v; }
    if let Some(v) = p["unconnected_height_mm"].as_f64() { r.unconnected_height_mm = v; }
    r.runs = runs_of(p)?;
    Ok(r)
}

/// The length still to draw for the ramp to reach its top at its steepest slope.
fn needed(d: &Document, r: &Ramp) -> (f64, f64) {
    let ratio = d.ramp_type(r.ramp_type).map_or(14.0, |t| t.max_slope_ratio);
    let h = d.ramp_heights(r).map_or(0.0, |(b, t)| t - b);
    let drawn: f64 = r.runs.iter().map(|x| x.start.dist(&x.end)).sum();
    (h * ratio, drawn)
}

impl Session {
    pub(crate) fn handle_ramps(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "ensure_ramp_types" => (|| { self.doc.ensure_ramp_types()?; Ok(json!({})) })(),
            // the tool's preview: the draft and the run under the cursor, which stops where the ramp reaches its top
            "ramp_draft" => (|| {
                let d = &self.doc;
                let mut r = draft_of(d, p)?;
                let (need, drawn) = needed(d, &r);
                let mut pending = None;
                if let Some(pr) = p.get("pending").filter(|v| !v.is_null()) {
                    let start = point_of(&pr["start"])?;
                    let mut end = point_of(&pr["end"])?;
                    let len = start.dist(&end);
                    let left = need - drawn;
                    if len > 1.0 {
                        if left > 1.0 && len > left {
                            end = Point::new(start.x + (end.x - start.x) * left / len, start.y + (end.y - start.y) * left / len);
                        }
                        let justification: RunJustification = serde_json::from_value(pr["justification"].clone()).unwrap_or_default();
                        pending = Some(RampRun { start, end, justification, width_mm: pr["width_mm"].as_f64().unwrap_or(1200.0) });
                    }
                }
                if let Some(x) = &pending { r.runs.push(x.clone()); }
                r.landings = (0..r.runs.len().saturating_sub(1)).collect();
                let l = d.ramp_layout(&r).ok_or("unknown ramp type or level")?;
                let drawn_now: f64 = r.runs.iter().map(|x| x.start.dist(&x.end)).sum();
                Ok(json!({ "lines": preview_lines(&l), "needed_mm": need, "drawn_mm": drawn_now, "remaining_mm": (need - drawn_now).max(0.0), "pending": pending, "height_mm": l.height }))
            })(),
            "add_ramp" => (|| {
                let r = draft_of(&self.doc, p)?;
                let rail = p["railing_type"].as_u64();
                let id = self.doc.grouped("Create ramp", |d| {
                    let id = d.add_ramp(r)?;
                    if let Some(t) = rail { d.add_stair_railings(id, t, Default::default())?; }
                    Ok(id)
                })?;
                Ok(json!({ "id": id, "railings": self.doc.stair_railings(id) }))
            })(),
            "ramp_edit" => (|| {
                let r = self.doc.ramp(id_of(p, "id")?).ok_or("not a ramp")?;
                Ok(json!({ "type_id": r.ramp_type, "base_level": r.base_level, "top_level": r.top_level, "base_offset_mm": r.base_offset_mm, "top_offset_mm": r.top_offset_mm, "unconnected_height_mm": r.unconnected_height_mm, "runs": r.runs }))
            })(),
            "set_ramp_runs" => (|| { self.doc.set_ramp_runs(id_of(p, "id")?, runs_of(p)?)?; Ok(json!({})) })(),
            "patch_ramp" => (|| { self.doc.patch_ramp(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "patch_ramp_type" => (|| { self.doc.patch_ramp_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "duplicate_ramp_type" => (|| Ok(json!({ "id": self.doc.duplicate_ramp_type(id_of(p, "id")?, p["name"].as_str().ok_or("missing name")?)? })))(),
            _ => return None,
        })
    }

    pub(crate) fn ramp_props(&self, id: Id, r: &Ramp) -> Value {
        let d = &self.doc;
        let t = d.ramp_type(r.ramp_type);
        let l = d.ramp_layout(r);
        let free = !r.pinned;
        let levels: Vec<(Id, String)> = { let mut v: Vec<_> = d.levels().iter().map(|(i, x)| (*i, x.name.clone(), x.elevation_mm)).collect(); v.sort_by(|a, b| a.2.total_cmp(&b.2)); v.into_iter().map(|(i, n, _)| (i, n)).collect() };
        let base_opts: Vec<(String, Value, bool)> = levels.iter().map(|(i, n)| (n.clone(), json!({ "base_level": i }), *i == r.base_level)).collect();
        let mut top_opts = vec![("None".to_string(), json!({ "top_level": null }), r.top_level.is_none())];
        top_opts.extend(levels.iter().map(|(i, n)| (n.clone(), json!({ "top_level": i }), Some(*i) == r.top_level)));
        let total: f64 = r.runs.iter().map(|x| x.start.dist(&x.end)).sum();
        let height = l.as_ref().map_or(0.0, |l| l.height);
        let slope = if height > 0.0 && total > 0.0 { format!("1:{:.1}", total / height) } else { String::new() };
        let widths: Vec<f64> = r.runs.iter().map(|x| x.width_mm).collect();
        let width_row = match widths.first() {
            Some(w) if widths.iter().all(|x| (x - w).abs() < 1e-6) => num("Width", "run_width_mm", *w, free),
            Some(_) => ro("Width", "*Varies*"),
            None => ro("Width", ""),
        };
        let warnings = l.as_ref().map(|l| l.warnings.clone()).unwrap_or_default();
        let mut rules: Vec<Value> = warnings.iter().map(|m| json!({ "label": "Warning", "value": m, "kind": "ro", "warning": true })).collect();
        if rules.is_empty() { rules.push(ro("Slope and Lengths", "Met")); }
        json!({
            "id": id, "kind": "Ramp", "category": "Ramps", "type_id": r.ramp_type,
            "title": format!("Ramp: {}", t.map_or("?", |t| t.name.as_str())),
            "target": { "method": "patch_ramp", "id": id },
            "warnings": warnings,
            "groups": [
                group("Constraints", vec![
                    select("Base Level", base_opts),
                    num("Base Offset", "base_offset_mm", r.base_offset_mm, free),
                    select("Top Level", top_opts),
                    num("Top Offset", "top_offset_mm", r.top_offset_mm, free && r.top_level.is_some()),
                    num("Desired Height", "unconnected_height_mm", height, free && r.top_level.is_none()),
                    crate::props::boolean("Pinned", "pinned", r.pinned, true),
                ]),
                group("Dimensions", vec![
                    ro("Slope", slope),
                    json!({ "label": "Length", "kind": "ro", "value": format!("{total:.0}"), "raw": total }),
                    width_row,
                    ro("Number of Runs", r.runs.len().to_string()),
                ]),
                group("Rules", rules),
                group("Identity Data", vec![text("Comments", "comments", &r.comments), text("Mark", "mark", &r.mark)]),
            ],
        })
    }
}

pub(crate) fn ramp_type_props(d: &Document, id: Id, t: &RampType) -> Value {
    let mats: Vec<String> = d.materials().into_iter().map(|(_, m)| m.name.clone()).collect();
    let mut mo = vec![("<By Category>".to_string(), json!({ "material": "" }), t.material.is_empty())];
    mo.extend(mats.iter().map(|m| (m.clone(), json!({ "material": m }), *m == t.material)));
    json!({
        "id": id, "kind": "RampType", "title": t.name,
        "siblings": d.ramp_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "groups": [
            group("Constraints", vec![
                num("Maximum Slope (1:x)", "max_slope_ratio", t.max_slope_ratio, true),
                num("Maximum Run Length", "max_run_length_mm", t.max_run_length_mm, true),
                num("Maximum Run Length at 1:20", "max_run_length_at_1_in_20_mm", t.max_run_length_at_1_in_20_mm, true),
                num("Minimum Landing Length", "min_landing_mm", t.min_landing_mm, true),
            ]),
            group("Construction", vec![
                enum_select("Shape", "shape", &name_of(&t.shape), &[("thick", "Thick"), ("solid", "Solid")]),
                num("Thickness", "thickness_mm", t.thickness_mm, true),
                select("Material", mo),
                enum_select("Function", "function", &name_of(&t.function), &[("interior", "Interior"), ("exterior", "Exterior")]),
            ]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// What a plan draws for its ramps: as stairs, with a straight cut line, and each run's slope written on it.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if !matches!(v.view_kind, ViewKind::FloorPlan | ViewKind::AreaPlan | ViewKind::StructuralPlan) { return json!({ "ramps": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    let Some(range) = d.resolve_view_range(v, &settings) else { return json!({ "ramps": [] }) };
    let Some(level_z) = v.level.and_then(|l| d.level(l)).map(|l| l.elevation_mm) else { return json!({ "ramps": [] }) };
    let scale = settings.scale as f64;
    let mut out = vec![];
    for (id, r) in d.ramps() {
        if v.hidden_elements.contains(&id) { continue; }
        let eo_held = d.graphics_of(v, &id);
        let eo = eo_held.as_ref();
        let Some(line) = style_for(d, &settings, "Ramps", None, eo, "projection", &Hint::default()) else { continue };
        let Some(l) = d.ramp_layout(r) else { continue };
        let (base, top) = (l.base_z, l.reached_z());
        if base > range.cut + 1e-6 || top < range.depth - 1.0 { continue; }
        let cut_here = base < range.cut - 1e-6 && top > range.cut + 1e-6;
        let down = !cut_here && base < level_z - 1.0;
        let mut cut: Option<(usize, Vec<Vec<[f64; 2]>>, Point, Point)> = None;
        if cut_here {
            if let Some(rn) = l.runs.iter().find(|x| x.z0 < range.cut && x.z1 > range.cut) {
                let sc = (range.cut - rn.z0) / (rn.z1 - rn.z0) * rn.length;
                let (lines, c, up) = cut_mark(rn, sc, 0.0, CutMarkStyle::SingleLine, scale);
                cut = Some((rn.index, lines, c, up));
            }
        }
        let mut lines: Vec<Value> = vec![];
        let mut push = |a: Point, b: Point, run: Option<usize>, z: f64| {
            match (&cut, run) {
                (Some((ci, _, c, up)), Some(ri)) if *ci == ri => {
                    let side = |p: Point| (p.x - c.x) * up.x + (p.y - c.y) * up.y;
                    let (lo, hi) = split(a, b, &side);
                    if let Some([p, q]) = lo { lines.push(json!({ "a": pt(p), "b": pt(q), "above": false })); }
                    if let Some([p, q]) = hi { lines.push(json!({ "a": pt(p), "b": pt(q), "above": true })); }
                }
                _ => lines.push(json!({ "a": pt(a), "b": pt(b), "above": cut_here && z > range.cut + 1e-6 })),
            }
        };
        let mut slopes = vec![];
        for rn in &l.runs {
            let f = rn.footprint();
            let zr = if rn.z1 <= range.cut + 1e-6 { rn.z0 } else { rn.z1 };
            for i in 0..4 { push(f[i], f[(i + 1) % 4], Some(rn.index), if i == 3 { rn.z0 } else if i == 1 { rn.z1 } else { zr }); }
            if rn.z1 > rn.z0 + 1e-6 { slopes.push(json!({ "at": pt(rn.at(rn.length / 2.0, 0.0)), "text": format!("1:{:.1}", rn.length / (rn.z1 - rn.z0)), "dir": pt(rn.dir) })); }
        }
        for g in &l.landings {
            let n = g.outline.len();
            for i in 0..n { push(g.outline[i], g.outline[(i + 1) % n], None, g.z); }
        }
        let walk = walkline(&l);
        let mut path_segs: Vec<Value> = vec![];
        for w in walk.windows(2) {
            let ((a, za), (b, zb)) = (w[0], w[1]);
            match (&cut, cut_here && za < range.cut && zb > range.cut) {
                (Some((_, _, c, _)), true) => {
                    path_segs.push(json!({ "a": pt(a), "b": pt(*c), "above": false }));
                    path_segs.push(json!({ "a": pt(*c), "b": pt(b), "above": true }));
                }
                _ => path_segs.push(json!({ "a": pt(a), "b": pt(b), "above": cut_here && za.min(zb) >= range.cut - 1e-6 })),
            }
        }
        let mut path: Vec<Point> = walk.iter().map(|p| p.0).collect();
        if down { path.reverse(); }
        let outlines: Vec<Vec<[f64; 2]>> = d.ramp_outlines(r).iter().map(|o| o.iter().map(|p| pt(*p)).collect()).collect();
        out.push(json!({
            "id": id, "pinned": r.pinned, "rings": outlines, "lines": lines,
            "cut": cut.as_ref().map(|c| c.1.clone()).unwrap_or_default(),
            "path": path_segs, "label": if down { "DN" } else { "UP" }, "label_at": path.first().map(|p| pt(*p)),
            "arrow": path.len().checked_sub(2).map(|k| json!({ "at": pt(path[k + 1]), "from": pt(path[k]) })),
            "slopes": slopes, "style": line.clone(), "above_style": line.clone(), "cut_style": line,
        }));
    }
    json!({ "ramps": out })
}
