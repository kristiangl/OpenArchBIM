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

/// The sketched landings drawn with the tool: [{ after_run, outline }].
fn landings_of(p: &Value) -> Result<Vec<openarch_model::stairs::StairLanding>, String> {
    match p.get("landings") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|e| format!("bad landings: {e}")),
        _ => Ok(vec![]),
    }
}

/// Sketched curves gathered into chains, each joined end to end.
fn chains(curves: &[openarch_model::curve::Curve]) -> Vec<Vec<Point>> {
    let mut left: Vec<openarch_model::curve::Curve> = curves.to_vec();
    let mut out = vec![];
    while !left.is_empty() {
        // grow a group from the first curve: every curve touching it, until none more join
        let mut group = vec![left.remove(0)];
        loop {
            let ends: Vec<Point> = group.iter().flat_map(|c| [c.start(), c.end()]).collect();
            let Some(k) = left.iter().position(|c| ends.iter().any(|e| e.dist(&c.start()) < 1.0 || e.dist(&c.end()) < 1.0)) else { break };
            group.push(left.remove(k));
        }
        if let Some(p) = openarch_model::railing::chain_path(&group) { out.push(p); }
    }
    out
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
    s.landings = landings_of(p)?;
    s.multistory_top = p["multistory_top"].as_u64();
    Ok(s)
}

pub(crate) fn pt(p: Point) -> [f64; 2] { [p.x, p.y] }

/// The run the cursor is drawing: from `start` towards `end`, as many risers as its length holds, no more than remain.
fn pending_run(s: &Stair, p: &Value) -> Result<Option<StairRun>, String> {
    let Some(pr) = p.get("pending").filter(|v| !v.is_null()) else { return Ok(None) };
    if let Some(kind) = pr["kind"].as_str().filter(|k| *k != "straight") {
        return pending_shape(s, pr, kind);
    }
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

/// A spiral or winder being drawn: { kind: spiral_full | spiral_ends | winder_l | winder_u, points, clockwise, style }.
/// A full-step spiral takes every riser left; the others as many as their walkline holds, no more than remain.
fn pending_shape(s: &Stair, pr: &Value, kind: &str) -> Result<Option<StairRun>, String> {
    let pts: Vec<Point> = pr["points"].as_array().ok_or("missing points")?.iter().map(point_of).collect::<Result<_, _>>()?;
    let need = match kind { "spiral_full" => 2, "spiral_ends" | "winder_l" => 3, "winder_u" => 4, _ => return Err(format!("unknown run shape {kind}")) };
    if pts.len() < need || pts.windows(2).any(|w| w[0].dist(&w[1]) < 1.0) { return Ok(None); }
    let have: u32 = s.runs.iter().map(|r| r.risers).sum();
    let left = s.desired_risers.saturating_sub(have);
    if left == 0 { return Ok(None); }
    let justification: RunJustification = serde_json::from_value(pr["justification"].clone()).unwrap_or_default();
    let width = pr["width_mm"].as_f64().unwrap_or(1000.0);
    let tr = s.tread_depth_mm;
    let (shape, start, end, risers) = match kind {
        "spiral_full" | "spiral_ends" => {
            let (c, a) = (pts[0], pts[1]);
            let r = c.dist(&a);
            if r < width / 2.0 + 1.0 { return Err("The spiral's radius must be more than half its width.".into()); }
            let mut clockwise = pr["clockwise"].as_bool().unwrap_or(false);
            let n = if kind == "spiral_full" { left } else {
                let b = pts[2];
                let (t0, t1) = ((a.y - c.y).atan2(a.x - c.x), (b.y - c.y).atan2(b.x - c.x));
                let mut sweep = (t1 - t0).rem_euclid(std::f64::consts::TAU);
                // the shorter way round, unless the direction was set
                if pr["clockwise"].is_null() && sweep > std::f64::consts::PI { clockwise = true; }
                if clockwise { sweep = std::f64::consts::TAU - sweep; }
                risers_for_length(r * sweep, tr, true, true).min(left)
            };
            let tangent = if clockwise { Point::new(a.y - c.y, -(a.x - c.x)) } else { Point::new(-(a.y - c.y), a.x - c.x) };
            (RunShape::Spiral { centre: c, clockwise }, a, Point::new(a.x + tangent.x / r, a.y + tangent.y / r), n)
        }
        _ => {
            let len: f64 = pts.windows(2).map(|w| w[0].dist(&w[1])).sum();
            let style = serde_json::from_value(pr["style"].clone()).unwrap_or_default();
            (RunShape::Winder { corners: pts[1..pts.len() - 1].to_vec(), style }, pts[0], *pts.last().unwrap(), risers_for_length(len, tr, true, true).min(left))
        }
    };
    Ok(Some(StairRun { shape, start, end, justification, width_mm: width, risers, begins_with_riser: true, ends_with_riser: true }))
}

/// Plan lines of a laid out stair for the tool's preview: footprints and risers.
pub(crate) fn preview_lines(l: &StairLayout) -> Vec<[[f64; 2]; 2]> {
    let mut out = vec![];
    for r in &l.runs {
        if let Some(p) = &r.path {
            for (poly, _) in &p.treads { for i in 0..poly.len() { out.push([pt(poly[i]), pt(poly[(i + 1) % poly.len()])]); } }
            continue;
        }
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
                // the sketched landings drawn so far, and an automatic one wherever runs meet without one
                let sketched = probe.landings.clone();
                for i in 0..probe.runs.len().saturating_sub(1) {
                    if !sketched.iter().any(|x| x.after_run == i) { probe.landings.push(openarch_model::stairs::StairLanding { kind: Default::default(), after_run: i, outline: None }); }
                }
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
            // with `level`: the unpinned storey that starts there, when there is one
            "stair_edit" => (|| {
                let s = self.doc.stair(id_of(p, "id")?).ok_or("not a stair")?;
                if let Some(c) = p["level"].as_u64().filter(|l| s.storeys.iter().any(|o| o.base_level == *l)).and_then(|l| self.doc.stair_storey(s, l)) {
                    return Ok(json!({ "type_id": c.stair_type, "base_level": c.base_level, "top_level": c.top_level, "base_offset_mm": c.base_offset_mm, "top_offset_mm": c.top_offset_mm, "unconnected_height_mm": c.unconnected_height_mm, "desired_risers": c.desired_risers, "tread_depth_mm": c.tread_depth_mm, "runs": c.runs, "landings": c.landings.iter().filter(|l| l.outline.is_some()).collect::<Vec<_>>(), "storey": c.base_level }));
                }
                Ok(json!({ "type_id": s.stair_type, "base_level": s.base_level, "top_level": s.top_level, "base_offset_mm": s.base_offset_mm, "top_offset_mm": s.top_offset_mm, "unconnected_height_mm": s.unconnected_height_mm, "desired_risers": s.desired_risers, "tread_depth_mm": s.tread_depth_mm, "runs": s.runs, "landings": s.landings.iter().filter(|l| l.outline.is_some()).collect::<Vec<_>>(), "multistory_top": s.multistory_top }))
            })(),
            "set_stair_runs" => (|| { self.doc.set_stair_storey_components(id_of(p, "id")?, p["storey"].as_u64(), runs_of(p)?, landings_of(p)?)?; Ok(json!({})) })(),
            // one run or landing of a stair: { id, part: { kind: run | landing, index, storey } }
            "stair_part" => (|| self.stair_part_props(id_of(p, "id")?, &p["part"]))(),
            "set_stair_storey_pinned" => (|| { self.doc.set_stair_storey_pinned(id_of(p, "id")?, id_of(p, "level")?, p["pinned"].as_bool().unwrap_or(true))?; Ok(json!({})) })(),
            // a sketched run: { edges: [{ curve, role: boundary | riser }], width_mm } gives the run
            "stair_sketch_run" => (|| {
                let edges = p["edges"].as_array().ok_or("missing edges")?;
                let mut bounds = vec![];
                let mut risers = vec![];
                for e in edges {
                    let c: openarch_model::curve::Curve = serde_json::from_value(e["curve"].clone()).map_err(|x| format!("bad curve: {x}"))?;
                    if e["role"] == "riser" { risers.push([c.start(), c.end()]); } else { bounds.push(c); }
                }
                let chains = chains(&bounds);
                if chains.len() != 2 { return Err(format!("Draw two boundaries (one each side of the run): there are {}.", chains.len())); }
                if risers.len() < 2 { return Err("Draw at least two riser lines across the run.".into()); }
                let (left, right, risers) = openarch_model::runpath::arrange(&chains, &risers).ok_or("The boundaries and risers do not make a run.")?;
                let n = risers.len() as u32;
                let m = |r: &[Point; 2]| Point::new((r[0].x + r[1].x) / 2.0, (r[0].y + r[1].y) / 2.0);
                let (start, end) = (m(&risers[0]), m(risers.last().unwrap()));
                let width = risers[0][0].dist(&risers[0][1]);
                let run = StairRun { shape: RunShape::Sketched { left, right, risers }, start, end, justification: RunJustification::Center, width_mm: width, risers: n, begins_with_riser: true, ends_with_riser: true };
                Ok(json!({ "run": run }))
            })(),
            // a sketched landing: { curves } gives its outline
            "stair_sketch_landing" => (|| {
                let curves: Vec<openarch_model::curve::Curve> = p["curves"].as_array().ok_or("missing curves")?.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad curve: {e}"))).collect::<Result<_, _>>()?;
                let mut outline = openarch_model::railing::chain_path(&curves).ok_or("A landing's outline must be one closed loop.")?;
                if outline.len() > 2 && outline[0].dist(outline.last().unwrap()) < 1.0 { outline.pop(); }
                if outline.len() < 3 { return Err("A landing's outline must be one closed loop.".into()); }
                Ok(json!({ "outline": outline }))
            })(),
            "patch_stair" => (|| { self.doc.patch_stair(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            // one run: { id, run, patch }
            "patch_stair_run" => (|| { self.doc.patch_stair_storey_run(id_of(p, "id")?, p["storey"].as_u64(), p["run"].as_u64().ok_or("missing run")? as usize, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
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
        // a multistory stair's storeys above its first: each pinned to the stair below, or unpinned with runs of its own
        let level_name = |lv: Id| d.level(lv).map_or(String::new(), |x| x.name.clone());
        let storeys: Vec<Value> = if s.multistory_top.is_some() {
            let rows: Vec<Value> = d.stair_storeys(s).iter().skip(1).map(|c| {
                let own = s.storeys.iter().any(|o| o.base_level == c.base_level);
                boolean(&format!("{} to {}: Pinned", level_name(c.base_level), c.top_level.map_or(String::new(), level_name)), &format!("storey_pinned.{}", c.base_level), !own, free)
            }).collect();
            if rows.is_empty() { vec![] } else { vec![group("Multistory", rows)] }
        } else { vec![] };
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
            "groups": (vec![
                group("Constraints", vec![
                    select("Base Level", base_opts),
                    num("Base Offset", "base_offset_mm", s.base_offset_mm, free),
                    select("Top Level", top_opts),
                    num("Top Offset", "top_offset_mm", s.top_offset_mm, free && s.top_level.is_some()),
                    num("Desired Stair Height", "unconnected_height_mm", l.as_ref().map_or(s.unconnected_height_mm, |l| l.height), free && s.top_level.is_none()),
                    select("Multistory Top Level", {
                        let top_z = s.top_level.and_then(|t| d.level(t)).map(|x| x.elevation_mm);
                        let mut o = vec![("None".to_string(), json!({ "multistory_top": null }), s.multistory_top.is_none())];
                        o.extend(levels.iter().filter(|(i, _)| top_z.is_some_and(|z| d.level(*i).is_some_and(|x| x.elevation_mm > z + 1.0))).map(|(i, n)| (n.clone(), json!({ "multistory_top": i }), Some(*i) == s.multistory_top)));
                        o
                    }),
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
            ].into_iter().chain(storeys).collect::<Vec<_>>()),
            "run_groups": s.runs.iter().enumerate().map(|(k, r)| {
                let len = l.as_ref().and_then(|l| l.runs.get(k)).map_or(0.0, |x| x.length);
                json!({ "name": format!("Run {}", k + 1), "target": { "method": "patch_stair_run", "id": id, "extra": { "run": k } }, "rows": run_rows(r, len, free) })
            }).collect::<Vec<_>>(),
        })
    }

    /// One run's or landing's own Properties (picked with Tab in a plan). A run of an unpinned storey changes that
    /// storey only; on a pinned one it is the stair's run, which every pinned storey repeats.
    fn stair_part_props(&self, id: Id, part: &Value) -> Result<Value, String> {
        let d = &self.doc;
        let s = d.stair(id).ok_or("not a stair")?;
        let free = !s.pinned;
        let storey = part["storey"].as_u64().filter(|l| s.storeys.iter().any(|o| o.base_level == *l));
        let c = match storey { Some(lv) => d.stair_storey(s, lv).ok_or("no such storey")?, None => s.clone() };
        let l = d.stair_layout(&c).ok_or("the stair cannot be laid out")?;
        let k = part["index"].as_u64().ok_or("missing part index")? as usize;
        let level_name = |lv: Id| d.level(lv).map_or(String::new(), |x| x.name.clone());
        let storey_row = ro("Storey", format!("{} to {}{}", level_name(c.base_level), c.top_level.map_or(String::new(), level_name), if s.multistory_top.is_none() { "" } else if storey.is_some() { " (unpinned)" } else { " (pinned)" }));
        if part["kind"] == "landing" {
            let g = l.landings.iter().find(|g| g.index == k).ok_or("no such landing")?;
            let sketched = c.landings.iter().find(|x| x.after_run == g.after_run).is_some_and(|x| x.outline.is_some());
            let area = openarch_model::geom::signed_area(&g.outline).abs() / 1e6;
            return Ok(json!({
                "id": id, "kind": "StairLanding", "category": "Stairs", "title": format!("Stair Landing: Landing {}", k + 1),
                "target": { "method": "patch_stair", "id": id },
                "groups": [
                    group("Constraints", vec![storey_row, json!({ "label": "Relative Height", "kind": "ro", "value": format!("{:.0}", g.z - l.base_z), "raw": g.z - l.base_z })]),
                    group("Construction", vec![ro("Landing", if sketched { "Sketched" } else { "Automatic" }), json!({ "label": "Thickness", "kind": "ro", "value": format!("{:.0}", g.thickness), "raw": g.thickness }), ro("After Run", format!("Run {}", g.after_run + 1))]),
                    group("Dimensions", vec![json!({ "label": "Area", "kind": "ro", "value": format!("{area:.2} m²"), "raw": area })]),
                ],
            }));
        }
        let r = c.runs.get(k).ok_or("no such run")?;
        let len = l.runs.get(k).map_or(0.0, |x| x.length);
        let shape = match r.shape { RunShape::Straight => "Straight", RunShape::Spiral { .. } => "Spiral", RunShape::Winder { .. } => "Winder", RunShape::Sketched { .. } => "Sketched" };
        let mut rows = vec![storey_row, ro("Shape", shape)];
        rows.extend(run_rows(r, len, free));
        Ok(json!({
            "id": id, "kind": "StairRun", "category": "Stairs", "title": format!("Stair Run: Run {}", k + 1),
            "target": { "method": "patch_stair_run", "id": id, "extra": { "run": k, "storey": storey } },
            "groups": [group("Run", rows)],
        }))
    }
}

/// A run's own rows: location line, width, risers, Begins and Ends with Riser, its length.
fn run_rows(r: &StairRun, len: f64, free: bool) -> Vec<Value> {
    vec![
        enum_select("Location Line", "justification", &name_of(&r.justification), &[("left", "Run: Left"), ("center", "Run: Center"), ("right", "Run: Right"), ("left_exterior", "Exterior Support: Left"), ("right_exterior", "Exterior Support: Right")]),
        num("Actual Run Width", "width_mm", r.width_mm, free),
        num("Risers", "risers", r.risers as f64, free && !matches!(r.shape, RunShape::Sketched { .. })),
        boolean("Begin with Riser", "begins_with_riser", r.begins_with_riser, free),
        boolean("End with Riser", "ends_with_riser", r.ends_with_riser, free),
        json!({ "label": "Run Length", "kind": "ro", "value": format!("{len:.0}"), "raw": len }),
    ]
}

/// The type's supports: each side and the middle switched between none, stringer and carriage, with their sizes.
fn support_rows(t: &StairType, material: &dyn Fn(&str, &str, &str) -> Value) -> Vec<Value> {
    use openarch_model::stairs::{Support, SupportKind};
    let sp = &t.supports;
    let def = Support { kind: SupportKind::Stringer, width_mm: 45.0, total_depth_mm: 290.0, above_nosing_mm: 50.0, offset_mm: 0.0, material: String::new() };
    let mut rows = vec![];
    for (label, key, cur, kinds) in [("Left Support", "left", &sp.left, true), ("Right Support", "right", &sp.right, true), ("Middle Support", "middle", &sp.middle, false)] {
        let with = |kind: &str| {
            let mut v = serde_json::to_value(cur.clone().unwrap_or_else(|| def.clone())).unwrap_or_default();
            v["kind"] = json!(kind);
            json!({ "supports": { key: v } })
        };
        let mut opts = vec![("None".to_string(), json!({ "supports": { key: null } }), cur.is_none())];
        if kinds { opts.push(("Stringer (Closed)".to_string(), with("stringer"), cur.as_ref().is_some_and(|s| s.kind == SupportKind::Stringer))); }
        opts.push(("Carriage (Open)".to_string(), with("carriage"), cur.as_ref().is_some_and(|s| s.kind == SupportKind::Carriage)));
        rows.push(select(label, opts));
        if let Some(s) = cur {
            let p = |f: &str| format!("supports.{key}.{f}");
            rows.push(num(&format!("{label}: Width"), &p("width_mm"), s.width_mm, true));
            rows.push(num(&format!("{label}: Total Depth"), &p("total_depth_mm"), s.total_depth_mm, true));
            if s.kind == SupportKind::Stringer { rows.push(num(&format!("{label}: Above Nosing"), &p("above_nosing_mm"), s.above_nosing_mm, true)); }
            rows.push(num(&format!("{label}: Offset"), &p("offset_mm"), s.offset_mm, true));
            rows.push(material(&format!("{label}: Material"), &p("material"), &s.material));
            if key == "middle" { rows.push(num("Number of Middle Supports", "supports.middle_count", sp.middle_count as f64, true)); }
        }
    }
    rows
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
                boolean("Check Headroom", "rules.check_headroom", r.check_headroom, true),
                num("Minimum Headroom", "rules.min_headroom_mm", r.min_headroom_mm, r.check_headroom),
            ]),
            group("Supports", support_rows(t, &material)),
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
pub(crate) fn split(a: Point, b: Point, side: &dyn Fn(Point) -> f64) -> (Option<[Point; 2]>, Option<[Point; 2]>) {
    let (fa, fb) = (side(a), side(b));
    if fa <= 0.0 && fb <= 0.0 { return (Some([a, b]), None); }
    if fa >= 0.0 && fb >= 0.0 { return (None, Some([a, b])); }
    let k = fa / (fa - fb);
    let m = Point::new(a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k);
    if fa < 0.0 { (Some([a, m]), Some([m, b])) } else { (Some([m, b]), Some([a, m])) }
}

/// The walkline through the runs' centres, turning at the landings, each point with the height of the walk there.
pub(crate) fn walkline(l: &StairLayout) -> Vec<(Point, f64)> {
    let mut pts: Vec<(Point, f64)> = vec![];
    for (i, r) in l.runs.iter().enumerate() {
        if let Some(p) = &r.path {
            for (q, s) in &p.walk { pts.push((*q, r.nosing_z(*s).min(r.z1))); }
            continue;
        }
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
pub(crate) fn cut_mark(r: &RunLayout, s: f64, angle_deg: f64, style: CutMarkStyle, scale: f64) -> (Vec<Vec<[f64; 2]>>, Point, Point) {
    let a = angle_deg.to_radians();
    // along the cut line, and square to it pointing up the run
    let along = Point::new(r.left.x * a.cos() + r.dir.x * a.sin(), r.left.y * a.cos() + r.dir.y * a.sin());
    let up = Point::new(r.dir.x * a.cos() - r.left.x * a.sin(), r.dir.y * a.cos() - r.left.y * a.sin());
    let half = r.width / 2.0 / a.cos().max(0.2) + 2.0 * scale;
    cut_mark_at(r.at(s, 0.0), along, up, half, style, scale)
}

/// A cut mark centred on `c`, its line `along` (a unit direction) reaching `half` each way, `up` pointing up the stair.
pub(crate) fn cut_mark_at(c: Point, along: Point, up: Point, half: f64, style: CutMarkStyle, scale: f64) -> (Vec<Vec<[f64; 2]>>, Point, Point) {
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
        let Some(t) = d.stair_type(s.stair_type) else { continue };
        // Coarse: the outline and walkline only; Fine: the treads' nosings too
        let detail = settings.detail_level;
        for (storey, l) in d.stair_storeys(s).iter().filter_map(|c| d.stair_layout(c).map(|l| (s.storeys.iter().any(|o| o.base_level == c.base_level).then_some(c.base_level), l))) {
        let (base, top) = (l.base_z, l.reached_z());
        // seen from this plan: up to the cut plane, and down to the view depth (the plan of the level a stair arrives
        // at sees it below)
        if base > range.cut + 1e-6 || top < range.depth - 1.0 { continue; }
        let cut_here = base < range.cut - 1e-6 && top > range.cut + 1e-6;
        let down = !cut_here && base < level_z - 1.0;
        // where the cut plane crosses the walk: on the run that spans it
        let mut cut: Option<(usize, Vec<Vec<[f64; 2]>>, Point, Point)> = None;
        if cut_here {
            if let Some(r) = l.runs.iter().find(|r| r.z0 < range.cut && r.z1 > range.cut && r.path.is_some()) {
                // on a spiral or winder: across the first riser above the cut
                let p = r.path.as_ref().unwrap();
                if let Some(k) = r.risers.iter().position(|x| x.1 > range.cut) {
                    let [a, b] = p.riser_lines[k];
                    let c = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                    let len = a.dist(&b).max(1e-9);
                    let along = Point::new((a.x - b.x) / len, (a.y - b.y) / len);
                    let (lines, c, up) = cut_mark_at(c, along, Point::new(along.y, -along.x), len / 2.0 + 2.0 * scale, t.cut_mark.style, scale);
                    cut = Some((usize::MAX, lines, c, up));
                }
            } else if let Some(r) = l.runs.iter().find(|r| r.z0 < range.cut && r.z1 > range.cut) {
                let (a, b) = (r.risers.first(), r.risers.last());
                let sc = match (a, b) { (Some(a), Some(b)) if b.1 > a.1 => a.0 + (range.cut - a.1) * (b.0 - a.0) / (b.1 - a.1), (Some(a), _) => a.0, _ => 0.0 };
                let sc = sc.clamp(0.0, r.length);
                let (lines, c, up) = cut_mark(r, sc, t.cut_mark.angle_deg, t.cut_mark.style, scale);
                cut = Some((r.index, lines, c, up));
            }
        }
        // which side of the cut a point of run `i` (or a landing after run `i`) is on: below is drawn solid
        let mut lines: Vec<Value> = vec![];
        let coarse = detail == openarch_model::view::DetailLevel::Coarse;
        let mut push = |a: Point, b: Point, kind: &str, run: Option<usize>, z: f64| {
            if coarse && kind == "riser" { return; }
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
            if let Some(p) = &r.path {
                // each tread's outline, above or below the cut by its height (Coarse: the run's outline)
                if coarse {
                    let f = r.footprint();
                    for i in 0..f.len() { push(f[i], f[(i + 1) % f.len()], "outline", None, r.z0); }
                    continue;
                }
                for (poly, top) in &p.treads {
                    for i in 0..poly.len() { push(poly[i], poly[(i + 1) % poly.len()], "riser", None, *top); }
                }
                // stringers beside the run: their faces along its edges, closed off at both ends
                for (side, sup) in [(1.0, &t.supports.left), (-1.0, &t.supports.right)] {
                    let Some(sp) = sup.as_ref().filter(|x| x.kind == openarch_model::stairs::SupportKind::Stringer) else { continue };
                    let edge = if side > 0.0 { &p.left } else { &p.right };
                    let outer = openarch_model::railing::offset_line(edge, side * (sp.offset_mm + sp.width_mm));
                    for e in [sp.offset_mm, sp.offset_mm + sp.width_mm] {
                        if e.abs() < 1e-6 { continue; }
                        let o = openarch_model::railing::offset_line(edge, side * e);
                        for w in o.windows(2) { push(w[0].0, w[1].0, "outline", None, r.nosing_z(w[0].1)); }
                    }
                    if let (Some(a), Some(b), Some(c), Some(d)) = (edge.first(), outer.first(), edge.last(), outer.last()) {
                        push(a.0, b.0, "outline", None, r.z0);
                        push(c.0, d.0, "outline", None, r.z1);
                    }
                }
                // Fine: each tread's front edge, past its riser
                let nosing = (detail == openarch_model::view::DetailLevel::Fine && !t.run.monolithic && t.run.treads).then_some(t.run.nosing_mm).filter(|n| *n > 0.0);
                if let Some(nz) = nosing {
                    for (k, [a, b]) in p.riser_lines.iter().enumerate() {
                        let (rs, top) = r.risers[k];
                        if !r.treads.iter().any(|x| (x.0 - rs).abs() < 1e-6) { continue; }
                        let len = a.dist(b).max(1e-9);
                        let d = Point::new(-(a.y - b.y) / len * nz, (a.x - b.x) / len * nz);
                        push(Point::new(a.x + d.x, a.y + d.y), Point::new(b.x + d.x, b.y + d.y), "nosing", None, top);
                    }
                }
                continue;
            }
            let w = r.width / 2.0;
            // a run above the cut, or below it, is wholly one or the other; the cut run splits
            let zr = if r.z1 <= range.cut + 1e-6 { r.z0 } else { r.z1 };
            push(r.at(0.0, -w), r.at(r.length, -w), "outline", Some(r.index), zr);
            push(r.at(0.0, w), r.at(r.length, w), "outline", Some(r.index), zr);
            // stringers beside the run: their faces
            for (side, sup) in [(1.0, &t.supports.left), (-1.0, &t.supports.right)] {
                let Some(sp) = sup.as_ref().filter(|x| x.kind == openarch_model::stairs::SupportKind::Stringer) else { continue };
                for e in [w + sp.offset_mm, w + sp.offset_mm + sp.width_mm] {
                    if (e - w).abs() < 1e-6 { continue; }
                    push(r.at(0.0, side * e), r.at(r.length, side * e), "outline", Some(r.index), zr);
                }
                let e = side * (w + sp.offset_mm + sp.width_mm);
                push(r.at(0.0, side * w), r.at(0.0, e), "outline", Some(r.index), r.z0);
                push(r.at(r.length, side * w), r.at(r.length, e), "outline", Some(r.index), r.z1);
            }
            let mut ends = vec![0.0, r.length];
            let nosing = (detail == openarch_model::view::DetailLevel::Fine && !t.run.monolithic && t.run.treads).then_some(t.run.nosing_mm).filter(|n| *n > 0.0);
            for (rs, top) in &r.risers {
                ends.retain(|e| (e - rs).abs() > 1e-6);
                push(r.at(*rs, -w), r.at(*rs, w), "riser", Some(r.index), *top);
                // Fine: the tread's front edge, past the riser
                if let Some(nz) = nosing { if *rs - nz > -1e-6 { push(r.at(rs - nz, -w), r.at(rs - nz, w), "nosing", Some(r.index), *top); } }
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
        // each run and landing, for Tab to pick on its own
        let mut parts: Vec<Value> = l.runs.iter().map(|r| json!({ "kind": "run", "index": r.index, "storey": storey, "ring": r.footprint().iter().map(|p| pt(*p)).collect::<Vec<_>>() })).collect();
        parts.extend(l.landings.iter().map(|g| json!({ "kind": "landing", "index": g.index, "storey": storey, "ring": g.outline.iter().map(|p| pt(*p)).collect::<Vec<_>>() })));
        out.push(json!({
            "id": id, "pinned": s.pinned, "rings": outlines, "lines": lines, "parts": parts, "storey": storey,
            "cut": cut.as_ref().map(|c| c.1.clone()).unwrap_or_default(),
            "path": path_segs, "label": label, "label_at": path.first().map(|p| pt(*p)),
            "arrow": path.len().checked_sub(2).map(|k| json!({ "at": pt(path[k + 1]), "from": pt(path[k]) })),
            "style": line.clone(), "above_style": above.clone(), "cut_style": cut_style.clone(),
        }));
        }
    }
    json!({ "stairs": out })
}
