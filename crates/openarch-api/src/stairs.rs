//! Stairs: the Stair tool (a draft laid out as runs are drawn, then Finish), Edit Stairs, Properties, Type
//! Properties, and what a plan draws (cut mark, solid below the cut, dashed above it, the walkline with UP or DN).
use serde_json::{json, Value};

use openarch_model::stairs::{risers_for_length, CutMarkStyle, RunJustification, RunLayout, RunShape, Stair, StairLayout, StairRun, StairType};
use openarch_model::view::ViewKind;
use openarch_model::{Document, Id, Point};

use crate::graphics::{style_for, Hint};
use crate::props::{boolean, enum_select, group, name_of, nest, num, ro, select, text};
use crate::{id_of, point_of, Res, Session};

fn runs_of(p: &Value) -> Result<Vec<StairRun>, String> {
    match p.get("runs") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|e| format!("bad runs: {e}")),
        _ => Ok(vec![]),
    }
}

/// The stair being drawn: its settings from the tool, and its runs so far.
fn draft_of(d: &Document, p: &Value) -> Result<Stair, String> {
    let ty = id_of(p, "type_id")?;
    let base = id_of(p, "base_level")?;
    let top = p["top_level"].as_u64();
    let mut s = d.new_stair(ty, base, top)?;
    if let Some(v) = p["base_offset_mm"].as_f64() { s.base_offset_mm = v; }
    if let Some(v) = p["top_offset_mm"].as_f64() { s.top_offset_mm = v; }
    if let Some(v) = p["unconnected_height_mm"].as_f64() { s.unconnected_height_mm = v; }
    // the riser count follows the height unless it was set
    if let Some((b, t)) = d.stair_heights(&s) {
        let rmax = d.stair_type(ty).map_or(190.0, |t| t.rules.max_riser_mm);
        s.desired_risers = ((t - b) / rmax - 1e-9).ceil().max(1.0) as u32;
    }
    if let Some(n) = p["desired_risers"].as_u64() { s.desired_risers = n.max(1) as u32; }
    if let Some(v) = p["tread_depth_mm"].as_f64() { s.tread_depth_mm = v; }
    s.runs = runs_of(p)?;
    Ok(s)
}

fn pt(p: Point) -> [f64; 2] { [p.x, p.y] }

/// The run the cursor is drawing: from `start` towards `end`, as many risers as its length holds, no more than remain.
fn pending_run(s: &Stair, p: &Value) -> Result<Option<StairRun>, String> {
    let Some(pr) = p.get("pending").filter(|v| !v.is_null()) else { return Ok(None) };
    let start = point_of(&pr["start"])?;
    let end = point_of(&pr["end"])?;
    if start.dist(&end) < 1.0 { return Ok(None); }
    let begins = pr["begins_with_riser"].as_bool().unwrap_or(true);
    let ends = pr["ends_with_riser"].as_bool().unwrap_or(true);
    let have: u32 = s.runs.iter().map(|r| r.risers).sum();
    let left = s.desired_risers.saturating_sub(have);
    if left == 0 { return Ok(None); }
    let n = risers_for_length(start.dist(&end), s.tread_depth_mm, begins, ends).min(left);
    let justification: RunJustification = serde_json::from_value(pr["justification"].clone()).unwrap_or_default();
    Ok(Some(StairRun { shape: RunShape::Straight, start, end, justification, width_mm: pr["width_mm"].as_f64().unwrap_or(1000.0), risers: n, begins_with_riser: begins, ends_with_riser: ends }))
}

/// Plan lines of a laid out stair for the tool's preview: footprints and risers.
fn preview_lines(l: &StairLayout) -> Vec<[[f64; 2]; 2]> {
    let mut out = vec![];
    for r in &l.runs {
        let f = r.footprint();
        for i in 0..f.len() { out.push([pt(f[i]), pt(f[(i + 1) % f.len()])]); }
        let w = r.width / 2.0;
        for (s, _) in &r.risers { out.push([pt(r.at(*s, -w)), pt(r.at(*s, w))]); }
    }
    for g in &l.landings {
        for i in 0..g.outline.len() { out.push([pt(g.outline[i]), pt(g.outline[(i + 1) % g.outline.len()])]); }
    }
    out
}

impl Session {
    pub(crate) fn handle_stairs(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "ensure_stair_types" => (|| { self.doc.ensure_stair_types()?; Ok(json!({})) })(),
            // the tool's preview: { type_id, base_level, top_level, ..., runs, pending: { start, end, justification, width_mm } }
            "stair_draft" => (|| {
                let d = &self.doc;
                let mut s = draft_of(d, p)?;
                let drawn = s.runs.len();
                let pending = pending_run(&s, p)?;
                if let Some(r) = &pending { s.runs.push(r.clone()); }
                let mut probe = s.clone();
                probe.landings = (0..probe.runs.len().saturating_sub(1)).map(|i| openarch_model::stairs::StairLanding { kind: Default::default(), after_run: i }).collect();
                let l = d.stair_layout(&probe).ok_or("unknown stair type or level")?;
                let created: u32 = s.runs.iter().map(|r| r.risers).sum();
                Ok(json!({
                    "lines": preview_lines(&l), "desired_risers": s.desired_risers, "created": created, "remaining": s.desired_risers.saturating_sub(created),
                    "riser_mm": l.riser_mm, "tread_mm": l.tread_mm, "pending": pending, "runs": drawn,
                    "end": l.runs.last().map(|r| pt(r.at(r.length, 0.0))),
                }))
            })(),
            // Finish; with `railing_type`, a railing on each side too, in the same undo step
            "add_stair" => (|| {
                let s = draft_of(&self.doc, p)?;
                let rail = p["railing_type"].as_u64();
                let id = self.doc.grouped("Create stair", |d| {
                    let id = d.add_stair(s)?;
                    if let Some(t) = rail { d.add_stair_railings(id, t, Default::default())?; }
                    Ok(id)
                })?;
                Ok(json!({ "id": id, "railings": self.doc.stair_railings(id) }))
            })(),
            // Edit Stairs: what the tool reopens with
            "stair_edit" => (|| {
                let s = self.doc.stair(id_of(p, "id")?).ok_or("not a stair")?;
                Ok(json!({ "type_id": s.stair_type, "base_level": s.base_level, "top_level": s.top_level, "base_offset_mm": s.base_offset_mm, "top_offset_mm": s.top_offset_mm, "unconnected_height_mm": s.unconnected_height_mm, "desired_risers": s.desired_risers, "tread_depth_mm": s.tread_depth_mm, "runs": s.runs }))
            })(),
            "set_stair_runs" => (|| { self.doc.set_stair_runs(id_of(p, "id")?, runs_of(p)?)?; Ok(json!({})) })(),
            "patch_stair" => (|| { self.doc.patch_stair(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "patch_stair_type" => (|| { self.doc.patch_stair_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            "duplicate_stair_type" => (|| Ok(json!({ "id": self.doc.duplicate_stair_type(id_of(p, "id")?, p["name"].as_str().ok_or("missing name")?)? })))(),
            _ => return None,
        })
    }

    pub(crate) fn stair_props(&self, id: Id, s: &Stair) -> Value {
        let d = &self.doc;
        let t = d.stair_type(s.stair_type);
        let l = d.stair_layout(s);
        let free = !s.pinned;
        let levels: Vec<(Id, String)> = { let mut v: Vec<_> = d.levels().iter().map(|(i, x)| (*i, x.name.clone(), x.elevation_mm)).collect(); v.sort_by(|a, b| a.2.total_cmp(&b.2)); v.into_iter().map(|(i, n, _)| (i, n)).collect() };
        let base_opts: Vec<(String, Value, bool)> = levels.iter().map(|(i, n)| (n.clone(), json!({ "base_level": i }), *i == s.base_level)).collect();
        let mut top_opts = vec![("None".to_string(), json!({ "top_level": null }), s.top_level.is_none())];
        top_opts.extend(levels.iter().map(|(i, n)| (n.clone(), json!({ "top_level": i }), Some(*i) == s.top_level)));
        let widths: Vec<f64> = s.runs.iter().map(|r| r.width_mm).collect();
        let width_row = match widths.first() {
            Some(w) if widths.iter().all(|x| (x - w).abs() < 1e-6) => num("Actual Run Width", "run_width_mm", *w, free),
            Some(_) => ro("Actual Run Width", "*Varies*"),
            None => ro("Actual Run Width", ""),
        };
        let mut rules = vec![];
        match l.as_ref().map(|l| &l.warnings) {
            Some(w) if w.is_empty() => rules.push(ro("Calculation Rules", "Met")),
            Some(w) => for m in w { rules.push(json!({ "label": "Warning", "value": m, "kind": "ro", "warning": true })); },
            None => {}
        }
        json!({
            "id": id, "kind": "Stair", "category": "Stairs", "type_id": s.stair_type,
            "title": format!("Stairs: {}", t.map_or("?", |t| t.name.as_str())),
            "target": { "method": "patch_stair", "id": id },
            "warnings": l.as_ref().map(|l| l.warnings.clone()).unwrap_or_default(),
            "groups": [
                group("Constraints", vec![
                    select("Base Level", base_opts),
                    num("Base Offset", "base_offset_mm", s.base_offset_mm, free),
                    select("Top Level", top_opts),
                    num("Top Offset", "top_offset_mm", s.top_offset_mm, free && s.top_level.is_some()),
                    num("Desired Stair Height", "unconnected_height_mm", l.as_ref().map_or(s.unconnected_height_mm, |l| l.height), free && s.top_level.is_none()),
                    boolean("Pinned", "pinned", s.pinned, true),
                ]),
                group("Dimensions", vec![
                    num("Desired Number of Risers", "desired_risers", s.desired_risers as f64, free),
                    ro("Actual Number of Risers", l.as_ref().map_or(String::new(), |l| l.actual_risers.to_string())),
                    json!({ "label": "Actual Riser Height", "kind": "ro", "value": l.as_ref().map_or(String::new(), |l| format!("{:.1}", l.riser_mm)), "raw": l.as_ref().map(|l| l.riser_mm) }),
                    num("Actual Tread Depth", "tread_depth_mm", s.tread_depth_mm, free),
                    width_row,
                    ro("Number of Runs", s.runs.len().to_string()),
                ]),
                group("Calculation Rules", rules),
                group("Identity Data", vec![text("Comments", "comments", &s.comments), text("Mark", "mark", &s.mark)]),
            ],
        })
    }
}

pub(crate) fn stair_type_props(d: &Document, id: Id, t: &StairType) -> Value {
    let r = &t.rules;
    let mats: Vec<String> = d.materials().into_iter().map(|(_, m)| m.name.clone()).collect();
    let material = |label: &str, path: &str, cur: &str| {
        let mut o = vec![("<By Category>".to_string(), nest(path, json!("")), cur.is_empty())];
        o.extend(mats.iter().map(|m| (m.clone(), nest(path, json!(m)), m == cur)));
        select(label, o)
    };
    let mono = t.run.monolithic;
    json!({
        "id": id, "kind": "StairType", "title": t.name,
        "siblings": d.stair_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "groups": [
            group("Calculation Rules", vec![
                num("Maximum Riser Height", "rules.max_riser_mm", r.max_riser_mm, true),
                num("Minimum Riser Height", "rules.min_riser_mm", r.min_riser_mm, true),
                num("Minimum Tread Depth", "rules.min_tread_mm", r.min_tread_mm, true),
                num("Maximum Tread Depth", "rules.max_tread_mm", r.max_tread_mm, true),
                num("Minimum Run Width", "rules.min_run_width_mm", r.min_run_width_mm, true),
                num("2R + T Minimum", "rules.min_2r_t_mm", r.min_2r_t_mm, true),
                num("2R + T Maximum", "rules.max_2r_t_mm", r.max_2r_t_mm, true),
                num("Maximum Risers per Flight", "rules.max_risers_per_run", r.max_risers_per_run as f64, true),
                num("Minimum Risers per Flight", "rules.min_risers_per_run", r.min_risers_per_run as f64, true),
                boolean("Check T - R of 120 or More", "rules.check_t_minus_r", r.check_t_minus_r, true),
                boolean("Check R + T of 450 to 480", "rules.check_r_plus_t", r.check_r_plus_t, true),
            ]),
            group("Run Type", vec![
                boolean("Monolithic", "run.monolithic", mono, true),
                num("Structural Depth", "run.structural_depth_mm", t.run.structural_depth_mm, mono),
                material("Monolithic Material", "run.material", &t.run.material),
                boolean("Treads", "run.treads", t.run.treads, !mono),
                num("Tread Thickness", "run.tread_thickness_mm", t.run.tread_thickness_mm, !mono && t.run.treads),
                num("Nosing Length", "run.nosing_mm", t.run.nosing_mm, !mono && t.run.treads),
                material("Tread Material", "run.tread_material", &t.run.tread_material),
                boolean("Risers", "run.risers", t.run.risers, !mono),
                num("Riser Thickness", "run.riser_thickness_mm", t.run.riser_thickness_mm, !mono && t.run.risers),
                material("Riser Material", "run.riser_material", &t.run.riser_material),
            ]),
            group("Landing Type", vec![
                num("Thickness", "landing.thickness_mm", t.landing.thickness_mm, true),
                material("Material", "landing.material", &t.landing.material),
            ]),
            group("Graphics", vec![
                enum_select("Cut Mark", "cut_mark.style", &name_of(&t.cut_mark.style), &[("zigzag", "Zigzag"), ("curved", "Curved"), ("single_line", "Single Line"), ("double_line", "Double Line")]),
                num("Cut Line Angle", "cut_mark.angle_deg", t.cut_mark.angle_deg, true),
            ]),
            group("Construction", vec![enum_select("Function", "function", &name_of(&t.function), &[("interior", "Interior"), ("exterior", "Exterior")])]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

// ---- plan drawing

/// Split a segment by the cut line: the part below it (towards the start of the stair) and the part above.
fn split(a: Point, b: Point, side: &dyn Fn(Point) -> f64) -> (Option<[Point; 2]>, Option<[Point; 2]>) {
    let (fa, fb) = (side(a), side(b));
    if fa <= 0.0 && fb <= 0.0 { return (Some([a, b]), None); }
    if fa >= 0.0 && fb >= 0.0 { return (None, Some([a, b])); }
    let k = fa / (fa - fb);
    let m = Point::new(a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k);
    if fa < 0.0 { (Some([a, m]), Some([m, b])) } else { (Some([m, b]), Some([a, m])) }
}

/// The walkline through the runs' centres, turning at the landings, each point with the height of the walk there.
fn walkline(l: &StairLayout) -> Vec<(Point, f64)> {
    let mut pts: Vec<(Point, f64)> = vec![];
    for (i, r) in l.runs.iter().enumerate() {
        let s0 = r.risers.first().map_or(0.0, |x| x.0);
        if i == 0 { pts.push((r.at(s0, 0.0), r.z0)); } else { pts.push((r.at(0.0, 0.0), r.z0)); }
        let end = r.at(r.length, 0.0);
        pts.push((end, r.z1));
        if let Some(n) = l.runs.get(i + 1) {
            let c = r.dir.x * n.dir.y - r.dir.y * n.dir.x;
            let dt = r.dir.x * n.dir.x + r.dir.y * n.dir.y;
            if c.abs() > 1e-6 {
                // where the two centre lines cross
                let q = n.at(0.0, 0.0);
                let k = ((q.x - end.x) * n.dir.y - (q.y - end.y) * n.dir.x) / c;
                if k > 0.0 { pts.push((Point::new(end.x + r.dir.x * k, end.y + r.dir.y * k), r.z1)); }
            } else if dt < 0.0 {
                // turning back: up into the landing and across
                let w = r.width.max(n.width) / 2.0;
                let q = n.at(0.0, 0.0);
                pts.push((Point::new(end.x + r.dir.x * w, end.y + r.dir.y * w), r.z1));
                pts.push((Point::new(q.x - n.dir.x * w, q.y - n.dir.y * w), r.z1));
            }
        }
    }
    pts.dedup_by(|a, b| a.0.dist(&b.0) < 1e-6);
    pts
}

/// The cut mark across `r` at `s`: its lines in plan, drawn at `scale` (paper sizes are 1:1 mm on the sheet).
fn cut_mark(r: &RunLayout, s: f64, angle_deg: f64, style: CutMarkStyle, scale: f64) -> (Vec<Vec<[f64; 2]>>, Point, Point) {
    let a = angle_deg.to_radians();
    // along the cut line, and square to it pointing up the run
    let along = Point::new(r.left.x * a.cos() + r.dir.x * a.sin(), r.left.y * a.cos() + r.dir.y * a.sin());
    let up = Point::new(r.dir.x * a.cos() - r.left.x * a.sin(), r.dir.y * a.cos() - r.left.y * a.sin());
    let c = r.at(s, 0.0);
    let half = r.width / 2.0 / a.cos().max(0.2) + 2.0 * scale;
    let p = |t: f64, n: f64| [c.x + along.x * t + up.x * n, c.y + along.y * t + up.y * n];
    let z = 1.5 * scale; // the zigzag's size: 1.5 mm on paper
    let lines = match style {
        CutMarkStyle::SingleLine => vec![vec![p(-half, 0.0), p(half, 0.0)]],
        CutMarkStyle::DoubleLine => vec![vec![p(-half, -z / 2.0), p(half, -z / 2.0)], vec![p(-half, z / 2.0), p(half, z / 2.0)]],
        CutMarkStyle::Zigzag => vec![vec![p(-half, 0.0), p(-z, 0.0), p(-z / 2.0, z), p(z / 2.0, -z), p(z, 0.0), p(half, 0.0)]],
        CutMarkStyle::Curved => {
            let mut v = vec![p(-half, 0.0)];
            for k in 0..=16 { let t = -2.0 * z + 4.0 * z * k as f64 / 16.0; v.push(p(t, z * (std::f64::consts::PI * t / (2.0 * z)).sin())); }
            v.push(p(half, 0.0));
            vec![v]
        }
    };
    (lines, c, up)
}

/// What a plan draws for its stairs. A stair the plan's cut plane passes through is drawn solid below the cut, dashed
/// above it, with the cut mark between and the walkline marked UP; one that ends at this level is drawn whole with
/// the walkline marked DN; one wholly below the cut and starting here is drawn whole, UP.
pub(crate) fn drawing(d: &Document, view: Id) -> Value {
    let Some(v) = d.view(view) else { return json!({}) };
    if !matches!(v.view_kind, ViewKind::FloorPlan | ViewKind::AreaPlan | ViewKind::StructuralPlan) { return json!({ "stairs": [] }); }
    let settings = d.effective_settings(view).unwrap_or_default();
    let Some(range) = d.resolve_view_range(v, &settings) else { return json!({ "stairs": [] }) };
    let Some(level_z) = v.level.and_then(|l| d.level(l)).map(|l| l.elevation_mm) else { return json!({ "stairs": [] }) };
    let scale = settings.scale as f64;
    let mut out = vec![];
    for (id, s) in d.stairs() {
        if v.hidden_elements.contains(&id) { continue; }
        let eo = v.element_overrides.get(&id);
        let Some(line) = style_for(d, &settings, "Stairs", Some("Outlines"), eo, "projection", &Hint::default()) else { continue };
        let above = style_for(d, &settings, "Stairs", Some("<Above> Outlines"), eo, "projection", &Hint::default()).unwrap_or_else(|| line.clone());
        let cut_style = style_for(d, &settings, "Stairs", Some("Cut Marks"), eo, "projection", &Hint::default()).unwrap_or_else(|| line.clone());
        let Some(l) = d.stair_layout(s) else { continue };
        let Some(t) = d.stair_type(s.stair_type) else { continue };
        let (base, top) = (l.base_z, l.reached_z());
        // seen from this plan: up to the cut plane, and down to the view depth (the plan of the level a stair arrives
        // at sees it below)
        if base > range.cut + 1e-6 || top < range.depth - 1.0 { continue; }
        let cut_here = base < range.cut - 1e-6 && top > range.cut + 1e-6;
        let down = !cut_here && base < level_z - 1.0;
        // where the cut plane crosses the walk: on the run that spans it
        let mut cut: Option<(usize, Vec<Vec<[f64; 2]>>, Point, Point)> = None;
        if cut_here {
            if let Some(r) = l.runs.iter().find(|r| r.z0 < range.cut && r.z1 > range.cut) {
                let (a, b) = (r.risers.first(), r.risers.last());
                let sc = match (a, b) { (Some(a), Some(b)) if b.1 > a.1 => a.0 + (range.cut - a.1) * (b.0 - a.0) / (b.1 - a.1), (Some(a), _) => a.0, _ => 0.0 };
                let sc = sc.clamp(0.0, r.length);
                let (lines, c, up) = cut_mark(r, sc, t.cut_mark.angle_deg, t.cut_mark.style, scale);
                cut = Some((r.index, lines, c, up));
            }
        }
        // which side of the cut a point of run `i` (or a landing after run `i`) is on: below is drawn solid
        let mut lines: Vec<Value> = vec![];
        let mut push = |a: Point, b: Point, kind: &str, run: Option<usize>, z: f64| {
            let whole_above = cut_here && z > range.cut + 1e-6;
            match (&cut, run) {
                (Some((ci, _, c, up)), Some(ri)) if *ci == ri => {
                    let side = |p: Point| (p.x - c.x) * up.x + (p.y - c.y) * up.y;
                    let (lo, hi) = split(a, b, &side);
                    if let Some([p, q]) = lo { lines.push(json!({ "a": pt(p), "b": pt(q), "kind": kind, "above": false })); }
                    if let Some([p, q]) = hi { lines.push(json!({ "a": pt(p), "b": pt(q), "kind": kind, "above": true })); }
                }
                _ => lines.push(json!({ "a": pt(a), "b": pt(b), "kind": kind, "above": whole_above })),
            }
        };
        for r in &l.runs {
            let w = r.width / 2.0;
            // a run above the cut, or below it, is wholly one or the other; the cut run splits
            let zr = if r.z1 <= range.cut + 1e-6 { r.z0 } else { r.z1 };
            push(r.at(0.0, -w), r.at(r.length, -w), "outline", Some(r.index), zr);
            push(r.at(0.0, w), r.at(r.length, w), "outline", Some(r.index), zr);
            let mut ends = vec![0.0, r.length];
            for (rs, top) in &r.risers {
                ends.retain(|e| (e - rs).abs() > 1e-6);
                push(r.at(*rs, -w), r.at(*rs, w), "riser", Some(r.index), *top);
            }
            for e in ends {
                let z = if e < 1e-6 { r.z0 } else { r.z1 };
                push(r.at(e, -w), r.at(e, w), "outline", Some(r.index), z);
            }
        }
        for g in &l.landings {
            let n = g.outline.len();
            for i in 0..n {
                push(g.outline[i], g.outline[(i + 1) % n], "outline", None, g.z);
            }
        }
        // the walkline: UP from the bottom, or DN from the top in the plan the stair arrives at
        let walk = walkline(&l);
        let label = if down { "DN" } else { "UP" };
        // past the cut the walkline is dashed with the rest of the flight: the stretch that rises through the cut
        // plane splits where the cut mark crosses it
        let mut path_segs: Vec<Value> = vec![];
        for w in walk.windows(2) {
            let ((a, za), (b, zb)) = (w[0], w[1]);
            let spans = cut_here && za < range.cut && zb > range.cut;
            match (&cut, spans) {
                (Some((_, _, c, _)), true) => {
                    path_segs.push(json!({ "a": pt(a), "b": pt(*c), "above": false }));
                    path_segs.push(json!({ "a": pt(*c), "b": pt(b), "above": true }));
                }
                _ => path_segs.push(json!({ "a": pt(a), "b": pt(b), "above": cut_here && za.min(zb) >= range.cut - 1e-6 })),
            }
        }
        let mut path: Vec<Point> = walk.iter().map(|p| p.0).collect();
        if down { path.reverse(); }
        let outlines: Vec<Vec<[f64; 2]>> = d.stair_outlines(s).iter().map(|o| o.iter().map(|p| pt(*p)).collect()).collect();
        out.push(json!({
            "id": id, "pinned": s.pinned, "rings": outlines, "lines": lines,
            "cut": cut.as_ref().map(|c| c.1.clone()).unwrap_or_default(),
            "path": path_segs, "label": label, "label_at": path.first().map(|p| pt(*p)),
            "arrow": path.len().checked_sub(2).map(|k| json!({ "at": pt(path[k + 1]), "from": pt(path[k]) })),
            "style": line, "above_style": above, "cut_style": cut_style,
        }));
    }
    json!({ "stairs": out })
}
