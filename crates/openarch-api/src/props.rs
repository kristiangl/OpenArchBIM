//! Properties palette content. Each row says what to show and, when it can be
//! edited, which JSON patch an edit produces, so the UI needs no per-property
//! code: it sends the patch to `patch_wall` or `patch_view` and refreshes.
//!
//! Row shape: `{ label, value, kind, ... }` where kind is one of
//! `ro` (read only), `text`, `number`, `bool`, `select`, `dialog`.
//! `path` is the property path for text/number/bool rows. `select` rows carry
//! `options: [{ label, patch, selected }]`. `locked` marks a property that a
//! view template controls.

use openarch_model::view::*;
use openarch_model::*;
use crate::Res;
use serde_json::{json, Value};

pub const SCALES: &[u32] = &[1, 2, 5, 10, 20, 25, 50, 75, 100, 125, 150, 200, 250, 300, 400, 500, 1000, 2000, 5000];

/// Nest `value` under a dotted path: ("settings.scale", 50) -> {"settings":{"scale":50}}.
pub fn nest(path: &str, value: Value) -> Value {
    path.rsplit('.').fold(value, |v, k| json!({ k: v }))
}

fn ro(label: &str, value: impl Into<String>) -> Value {
    json!({ "label": label, "value": value.into(), "kind": "ro" })
}

fn num(label: &str, path: &str, v: f64, enabled: bool) -> Value {
    json!({ "label": label, "value": format!("{v}"), "raw": v, "kind": if enabled { "number" } else { "ro" }, "path": path })
}

fn text(label: &str, path: &str, v: &str) -> Value {
    json!({ "label": label, "value": v, "kind": "text", "path": path })
}

fn boolean(label: &str, path: &str, v: bool, enabled: bool) -> Value {
    json!({ "label": label, "value": if v { "Yes" } else { "No" }, "raw": v, "kind": if enabled { "bool" } else { "ro" }, "path": path })
}

fn select(label: &str, opts: Vec<(String, Value, bool)>) -> Value {
    let shown = opts.iter().find(|(_, _, sel)| *sel).map(|(l, _, _)| l.clone()).unwrap_or_default();
    json!({
        "label": label, "value": shown, "kind": "select",
        "options": opts.into_iter().map(|(l, p, s)| json!({ "label": l, "patch": p, "selected": s })).collect::<Vec<_>>(),
    })
}

/// A fill pattern, chosen in the Fill Patterns window (opened on the `prefer` tab, drafting or model); the patch
/// sets `path` to the name, or to null for no pattern.
fn fill_pattern_row(label: &str, path: &str, current: Option<&str>, prefer: &str) -> Value {
    json!({ "label": label, "value": current.unwrap_or("<none>"), "raw": current, "kind": "fill_pattern", "path": path, "prefer": prefer })
}

fn dialog(label: &str, name: &str, button: &str) -> Value {
    json!({ "label": label, "value": button, "kind": "dialog", "dialog": name })
}

fn locked(mut row: Value, is: bool) -> Value {
    if is {
        row["locked"] = json!(true);
    }
    row
}

fn group(name: &str, rows: Vec<Value>) -> Value {
    json!({ "name": name, "rows": rows })
}

/// Select over an enum serialised as snake_case: `current` and the variants are
/// given as (json name, label).
fn enum_select(label: &str, path: &str, current: &str, variants: &[(&str, &str)]) -> Value {
    select(label, variants.iter().map(|(v, l)| (l.to_string(), nest(path, json!(v)), *v == current)).collect())
}

fn name_of<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

pub fn wall_props(d: &Document, id: Id, w: &Wall) -> Value {
    let t = d.wall_type(w.wall_type);
    let (z0, z1) = d.wall_extent(w);
    let len = Document::wall_length_mm(w);
    let thick = t.map_or(0.0, |t| t.thickness_mm());
    let levels = d.levels();
    let level_opts = |current: Option<Id>, path: &str, none: Option<&str>, prefix: &str| {
        let mut o: Vec<(String, Value, bool)> = vec![];
        if let Some(n) = none {
            o.push((n.to_string(), nest(path, Value::Null), current.is_none()));
        }
        for (lid, l) in &levels {
            o.push((format!("{prefix}{}", l.name), nest(path, json!(lid)), current == Some(*lid)));
        }
        o
    };
    let ll = name_of(&w.location_line);
    let slanted = w.cross_section == CrossSection::Slanted;
    let has_top_level = w.top_level.is_some();
    let area = (len * (z1 - z0) - d.wall_opening_area(id)) / 1e6; // net of openings
    let mut cross = vec![enum_select(
        "Cross-Section",
        "cross_section",
        &name_of(&w.cross_section),
        &[("vertical", "Vertical"), ("slanted", "Slanted"), ("tapered", "Tapered")],
    )];
    if slanted {
        cross.push(num("Angle from Vertical", "angle_from_vertical_deg", w.angle_from_vertical_deg, true));
    }
    json!({
        "id": id, "kind": "Wall", "type_id": w.wall_type,
        "title": format!("Basic Wall: {}", t.map_or("?", |t| t.name.as_str())),
        "target": { "method": "patch_wall", "id": id },
        "height_mm": w.height_mm,
        "groups": [
            group("Constraints", vec![
                enum_select("Location Line", "location_line", &ll, &[
                    ("wall_centreline", "Wall Centerline"), ("core_centreline", "Core Centerline"),
                    ("finish_face_exterior", "Finish Face: Exterior"), ("finish_face_interior", "Finish Face: Interior"),
                    ("core_face_exterior", "Core Face: Exterior"), ("core_face_interior", "Core Face: Interior"),
                ]),
                boolean("Flipped", "flipped", w.flipped, true),
                boolean("Pinned", "pinned", w.pinned, true),
                select("Base Constraint", level_opts(Some(w.base_level), "base_level", None, "")),
                num("Base Offset", "base_offset_mm", w.base_offset_mm, true),
                ro("Base is Attached", "No"),
                num("Base Extension Distance", "base_extension_mm", w.base_extension_mm, true),
                select("Top Constraint", {
                    let mut o = vec![("Unconnected".to_string(), nest("top_level", Value::Null), !has_top_level)];
                    o.extend(level_opts(w.top_level, "top_level", None, "Up to level: "));
                    o
                }),
                num("Unconnected Height", "height_mm", w.height_mm, !has_top_level),
                num("Top Offset", "top_offset_mm", w.top_offset_mm, has_top_level),
                ro("Top is Attached", "No"),
                num("Top Extension Distance", "top_extension_mm", w.top_extension_mm, true),
                boolean("Room Bounding", "room_bounding", w.room_bounding, true),
                ro("Related to Mass", "No"),
            ]),
            group("Cross-Section Definition", cross),
            group("Structural", vec![
                boolean("Structural", "structural", w.structural, true),
                if w.structural {
                    enum_select("Structural Usage", "structural_usage", &name_of(&w.structural_usage), &[
                        ("bearing", "Bearing"), ("shear", "Shear"), ("structural_combination", "Structural combination"), ("non_bearing", "Non-bearing"),
                    ])
                } else {
                    ro("Structural Usage", "Non-bearing")
                },
            ]),
            group("Joins", vec![ro("Join at Start", join_label(&w.start_join)), ro("Join at End", join_label(&w.end_join))]),
            group("Dimensions", vec![
                ro("Length", format!("{len:.0}")),
                ro("Thickness", format!("{thick:.0}")),
                ro("Area", format!("{area:.3} m\u{b2}")),
                ro("Volume", format!("{:.3} m\u{b3}", area * thick / 1000.0)),
            ]),
            group("Identity Data", vec![text("Comments", "comments", &w.comments), text("Mark", "mark", &w.mark)]),
            group("Phasing", vec![ro("Phase Created", "New Construction"), ro("Phase Demolished", "None")]),
            group("IFC Parameters", vec![
                enum_select("Export to IFC", "ifc.export", &name_of(&w.ifc.export), &[("by_type", "By Type"), ("yes", "Yes"), ("no", "No")]),
                text("Export to IFC As", "ifc.export_as", &w.ifc.export_as),
                text("IFC Predefined Type", "ifc.predefined_type", &w.ifc.predefined_type),
                ro("IfcGUID", w.ifc.guid.clone()),
            ]),
        ],
    })
}

pub fn join_label(j: &EndJoin) -> String {
    if !j.allowed {
        return "Not allowed".into();
    }
    match j.kind {
        JoinKind::Butt => "Butt",
        JoinKind::Miter => "Miter",
        JoinKind::SquareOff => "Square off",
    }
    .into()
}

pub fn floor_props(d: &Document, id: Id, f: &Floor) -> Value {
    let t = d.floor_type(f.floor_type);
    json!({
        "id": id, "kind": "Floor", "type_id": f.floor_type,
        "title": format!("Floor: {}", t.map_or("?", |t| t.name.as_str())),
        "target": { "method": "patch_floor", "id": id },
        "groups": [
            group("Constraints", vec![
                ro("Level", d.level(f.level).map_or("?", |l| l.name.as_str())),
                num("Height Offset From Level", "offset_mm", f.offset_mm, !f.pinned),
                boolean("Pinned", "pinned", f.pinned, true),
            ]),
            group("Structural", vec![boolean("Structural", "structural", f.structural, !f.pinned)]),
            group("Dimensions", vec![
                ro("Area", format!("{:.2} m\u{b2}", f.area_mm2() / 1e6)),
                ro("Thickness", format!("{:.0}", t.map_or(0.0, |t| t.thickness_mm()))),
                ro("Slope", d.floor_surface(f).map_or("Flat".to_string(), |s| if s.rise.abs() < 1e-9 { "Flat".to_string() } else { format!("{:.2}%", s.percent()) })),
            ]),
        ],
    })
}

pub fn roof_props(d: &Document, id: Id, r: &openarch_model::roof::Roof) -> Value {
    use openarch_model::roof::RafterCut;
    let t = d.roof_type(r.roof_type);
    let g = d.roof_geometry(r);
    let area = g.as_ref().map_or(0.0, |g| g.surface_area_mm2());
    let thick = t.map_or(0.0, |t| t.thickness_mm());
    let level_opts: Vec<(String, Value, bool)> = std::iter::once(("None".to_string(), json!({ "cutoff_level": null }), r.cutoff_level.is_none()))
        .chain(d.levels().iter().map(|(lid, l)| (l.name.clone(), json!({ "cutoff_level": lid }), r.cutoff_level == Some(*lid))))
        .collect();
    let cut_opts: Vec<(String, Value, bool)> = [(RafterCut::Plumb, "plumb", "Plumb Cut"), (RafterCut::TwoCutPlumb, "two_cut_plumb", "Two Cut - Plumb"), (RafterCut::Square, "square", "Square Cut")]
        .iter()
        .map(|(c, k, l)| (l.to_string(), json!({ "rafter_cut": k }), r.rafter_cut == *c))
        .collect();
    let slope_row = match (r.has_slope_edges(), r.common_slope()) {
        (true, Some(s)) => num("Slope (degrees)", "slope_deg", s, !r.pinned),
        (true, None) => ro("Slope (degrees)", "*Varies*"),
        (false, _) => ro("Slope (degrees)", ""),
    };
    json!({
        "id": id, "kind": "Roof", "type_id": r.roof_type, "type_label": "Roof",
        "title": format!("Roof: {}", t.map_or("?", |t| t.name.as_str())),
        "target": { "method": "patch_roof", "id": id },
        "groups": [
            group("Constraints", vec![
                ro("Base Level", d.level(r.level).map_or("?", |l| l.name.as_str())),
                boolean("Room Bounding", "room_bounding", r.room_bounding, true),
                num("Base Offset From Level", "base_offset_mm", r.base_offset_mm, true),
                select("Cutoff Level", level_opts),
                num("Cutoff Offset", "cutoff_offset_mm", r.cutoff_offset_mm, r.cutoff_level.is_some()),
                boolean("Pinned", "pinned", r.pinned, true),
            ]),
            group("Construction", vec![
                select("Rafter Cut", cut_opts),
                num("Fascia Depth", "fascia_depth_mm", r.fascia_depth_mm, true),
            ]),
            group("Dimensions", vec![
                slope_row,
                ro("Thickness", format!("{thick:.0}")),
                ro("Area", format!("{:.2} m\u{b2}", area / 1e6)),
                ro("Volume", format!("{:.2} m\u{b3}", area * thick / 1e9)),
                ro("Maximum Ridge Height", g.as_ref().map_or(String::new(), |g| format!("{:.0}", g.top_z - d.level(r.level).map_or(0.0, |l| l.elevation_mm)))),
            ]),
            group("Identity Data", vec![text("Mark", "mark", &r.mark), text("Comments", "comments", &r.comments)]),
        ],
    })
}

fn kind_label(k: ViewKind) -> &'static str {
    match k {
        ViewKind::FloorPlan => "Floor Plan",
        ViewKind::CeilingPlan => "Ceiling Plan",
        ViewKind::StructuralPlan => "Structural Plan",
        ViewKind::AreaPlan => "Area Plan",
        ViewKind::Elevation => "Elevation",
        ViewKind::Section => "Section",
        ViewKind::Callout => "Callout",
        ViewKind::Drafting => "Drafting View",
        ViewKind::Legend => "Legend",
        ViewKind::ThreeD => "3D View",
    }
}

pub fn view_props(d: &Document, id: Id, v: &View) -> Value {
    let eff = d.effective_settings(id).unwrap_or_else(|| v.settings.clone());
    let tpl = v.template.and_then(|t| d.view_template(t));
    let inc = |p: TemplateProp| tpl.is_some_and(|t| t.included.contains(&p));
    let plan = v.view_kind.is_plan();
    let levels = d.levels();
    let s = &eff;

    let scale_label = |n: u32| format!("1 : {n}");
    let mut scale_opts: Vec<(String, Value, bool)> = SCALES
        .iter()
        .map(|n| (scale_label(*n), nest("settings", json!({ "scale": n, "custom_scale": false })), !s.custom_scale && s.scale == *n))
        .collect();
    scale_opts.push(("Custom".into(), nest("settings", json!({ "custom_scale": true })), s.custom_scale));

    let mut graphics = vec![
        locked(select("View Scale", scale_opts), inc(TemplateProp::Scale)),
        locked(num("Scale Value  1:", "settings.scale", s.scale as f64, s.custom_scale), inc(TemplateProp::Scale)),
        locked(enum_select("Display Model", "settings.display_model", &name_of(&s.display_model), &[("normal", "Normal"), ("do_not_display", "Do not display"), ("halftone", "Halftone")]), inc(TemplateProp::DisplayModel)),
        locked(enum_select("Detail Level", "settings.detail_level", &name_of(&s.detail_level), &[("coarse", "Coarse"), ("medium", "Medium"), ("fine", "Fine")]), inc(TemplateProp::DetailLevel)),
        locked(enum_select("Parts Visibility", "settings.parts_visibility", &name_of(&s.parts_visibility), &[("show_original", "Show Original"), ("show_parts", "Show Parts"), ("show_both", "Show Both")]), inc(TemplateProp::PartsVisibility)),
        locked(dialog("Visibility/Graphics Overrides", "vg", "Edit..."), inc(TemplateProp::VgModel)),
        locked(enum_select("Visual Style", "settings.visual_style", &name_of(&s.visual_style), &[("wireframe", "Wireframe"), ("hidden_line", "Hidden Line"), ("shaded", "Shaded"), ("consistent_colors", "Consistent Colors"), ("realistic", "Realistic")]), inc(TemplateProp::VisualStyle)),
    ];
    if plan {
        graphics.extend([
            locked(enum_select("Orientation", "settings.orientation", &name_of(&s.orientation), &[("project_north", "Project North"), ("true_north", "True North")]), inc(TemplateProp::Orientation)),
            locked(enum_select("Wall Join Display", "settings.wall_join_display", &name_of(&s.wall_join_display), &[("clean_all", "Clean all wall joins"), ("clean_same_type", "Clean same type wall joins")]), inc(TemplateProp::WallJoinDisplay)),
            locked(enum_select("Discipline", "settings.discipline", &name_of(&s.discipline), &[("architectural", "Architectural"), ("structural", "Structural"), ("mechanical", "Mechanical"), ("electrical", "Electrical"), ("plumbing", "Plumbing"), ("coordination", "Coordination")]), inc(TemplateProp::Discipline)),
            locked(enum_select("Show Hidden Lines", "settings.show_hidden_lines", &name_of(&s.show_hidden_lines), &[("by_discipline", "By Discipline"), ("none", "None"), ("all", "All")]), inc(TemplateProp::ShowHiddenLines)),
            locked(enum_select("Color Scheme Location", "settings.color_scheme_location", &name_of(&s.color_scheme_location), &[("background", "Background"), ("foreground", "Foreground")]), inc(TemplateProp::ColorScheme)),
            locked(ro("Color Scheme", "<none>"), inc(TemplateProp::ColorScheme)),
            locked(boolean("Sun Path", "settings.sun_path", s.sun_path, true), inc(TemplateProp::SunPath)),
        ]);
    }

    graphics.extend([
        locked(boolean("Show Edges", "settings.graphic_display.show_edges", s.graphic_display.show_edges, true), inc(TemplateProp::GraphicDisplay)),
        locked(boolean("Show Silhouettes", "settings.graphic_display.silhouettes", s.graphic_display.silhouettes, true), inc(TemplateProp::GraphicDisplay)),
        locked(boolean("Cast Shadows", "settings.graphic_display.cast_shadows", s.graphic_display.cast_shadows, true), inc(TemplateProp::GraphicDisplay)),
        locked(boolean("Sketchy Lines", "settings.graphic_display.sketchy_lines", s.graphic_display.sketchy_lines, true), inc(TemplateProp::GraphicDisplay)),
    ]);
    if matches!(v.view_kind, ViewKind::Elevation | ViewKind::Section) {
        // Depth Cueing (Graphic Display Options): fade from a start to an end along the view depth, down to a limit
        let g = &s.graphic_display;
        let cue = |label: &str, key: &str, value: u8| locked(num(label, &format!("settings.graphic_display.{key}"), value as f64, g.depth_cue), inc(TemplateProp::GraphicDisplay));
        graphics.extend([
            locked(boolean("Show Depth", "settings.graphic_display.depth_cue", g.depth_cue, true), inc(TemplateProp::GraphicDisplay)),
            cue("Depth Fade Start (%)", "depth_cue_start", g.depth_cue_start),
            cue("Depth Fade End (%)", "depth_cue_end", g.depth_cue_end),
            cue("Depth Fade Limit (%)", "depth_cue_fade", g.depth_cue_fade),
        ]);
    }
    let mut groups = vec![group("Graphics", graphics)];
    if plan {
        let level_opts = |current: Option<Id>, path: &str| {
            let mut o = vec![("None".to_string(), nest(path, Value::Null), current.is_none())];
            for (lid, l) in &levels {
                o.push((l.name.clone(), nest(path, json!(lid)), current == Some(*lid)));
            }
            o
        };
        let has_under = v.underlay.base_level.is_some();
        let mut top_opts = vec![("Unbounded".to_string(), nest("underlay.top_level", Value::Null), v.underlay.top_level.is_none())];
        for (lid, l) in &levels {
            top_opts.push((l.name.clone(), nest("underlay.top_level", json!(lid)), v.underlay.top_level == Some(*lid)));
        }
        let mut top = select("Range: Top Level", top_opts);
        if !has_under {
            top["kind"] = json!("ro");
        }
        groups.push(group("Underlay", vec![
            select("Range: Base Level", level_opts(v.underlay.base_level, "underlay.base_level")),
            top,
            locked(
                if has_under {
                    enum_select("Underlay Orientation", "settings.underlay_orientation", &name_of(&s.underlay_orientation), &[("look_down", "Look down"), ("look_up", "Look up")])
                } else {
                    ro("Underlay Orientation", "Look down")
                },
                inc(TemplateProp::UnderlayOrientation),
            ),
        ]));
        groups.push(group("Extents", vec![
            boolean("Crop View", "crop.enabled", v.crop.enabled, true),
            boolean("Crop Region Visible", "crop.region_visible", v.crop.region_visible, v.crop.enabled),
            boolean("Annotation Crop", "crop.annotation_crop", v.crop.annotation_crop, v.crop.enabled),
            locked(dialog("View Range", "view_range", "Edit..."), inc(TemplateProp::ViewRange)),
            ro("Associated Level", v.level.and_then(|l| d.level(l)).map_or("", |l| l.name.as_str())),
            ro("Scope Box", "None"),
            num("Column Symbolic Offset", "column_symbolic_offset_mm", v.column_symbolic_offset_mm, true),
            locked(enum_select("Depth Clipping", "settings.depth_clipping", &name_of(&s.depth_clipping), &[("no_clip", "No clip"), ("clip_with_line", "Clip with line"), ("clip_without_line", "Clip without line")]), inc(TemplateProp::DepthClipping)),
        ]));
    } else {
        groups.push(group("Extents", vec![
            boolean("Crop View", "crop.enabled", v.crop.enabled, true),
            boolean("Crop Region Visible", "crop.region_visible", v.crop.region_visible, v.crop.enabled),
        ]));
    }
    let mut tpl_opts = vec![("<None>".to_string(), nest("template", Value::Null), v.template.is_none())];
    for (tid, t) in d.view_templates() {
        if t.view_kind == v.view_kind {
            tpl_opts.push((t.name.clone(), nest("template", json!(tid)), v.template == Some(tid)));
        }
    }
    groups.push(group("Identity Data", vec![
        select("View Template", tpl_opts),
        text("View Name", "name", &v.name),
        ro("Dependency", if v.parent.is_some() { "Dependent" } else { "Independent" }),
        text("Title on Sheet", "title_on_sheet", &v.title_on_sheet),
        ro("Sheet Number", "<None>"),
        ro("Sheet Name", "<None>"),
    ]));
    groups.push(group("Phasing", vec![ro("Phase Filter", "Show All"), ro("Phase", "New Construction")]));
    json!({
        "id": id, "kind": "View",
        "title": format!("{}: {}", kind_label(v.view_kind), v.name),
        "type_label": kind_label(v.view_kind),
        "target": { "method": "patch_view", "id": id },
        "groups": groups,
    })
}

/// A family parameter row a global parameter can set: it names the global when one does (and then cannot be typed
/// into), and says it can take one when it could (no formula, not reporting).
fn with_global(d: &Document, mut row: Value, owner: Id, p: &ParamDef, editable_here: bool) -> Value {
    let held = match d.get(owner) {
        Some(openarch_model::element::Element::FamilyType(t)) => t.globals.get(&p.name).copied(),
        Some(openarch_model::element::Element::FamilyInstance(i)) => i.globals.get(&p.name).copied(),
        _ => None,
    };
    if let Some(gid) = held {
        row["kind"] = json!("ro");
        row["global"] = json!({ "id": gid, "name": d.global_param(gid).map(|g| g.name.clone()).unwrap_or_default(), "param": p.name, "owner": owner, "kind": p.param_kind });
    } else if editable_here && !p.reporting && p.formula.is_none() {
        row["associate"] = json!({ "param": p.name, "owner": owner, "kind": p.param_kind });
    }
    row
}

fn param_row(d: &Document, i: &FamilyInstance, p: &ParamDef, ro_only: bool) -> Value {
    let v = d.instance_param(i, &p.name).unwrap_or_else(|| p.default.clone());
    let editable = p.instance && !p.reporting && !ro_only;
    let path = format!("values.{}", p.name);
    match (&v, p.param_kind) {
        (ParamValue::Bool(b), _) => boolean(&p.name, &path, *b, editable),
        (ParamValue::Number(n), _) => num(&p.name, &path, *n, editable),
        (ParamValue::Text(t), _) if editable && !p.choices.is_empty() => {
            select(&p.name, p.choices.iter().map(|c| (c.clone(), json!({ "values": { p.name.clone(): c } }), c == t)).collect())
        }
        (ParamValue::Text(t), _) if editable => text(&p.name, &path, t),
        (other, _) => ro(&p.name, other.display()),
    }
}

/// Properties of a placed door, window or other family instance.
pub fn instance_props(d: &Document, id: Id, i: &FamilyInstance) -> Res {
    let (fam, t) = d.instance_family(i).ok_or("instance has no family")?;
    let (_, height, sill) = d.instance_opening_size(i);
    let mut groups: Vec<(String, Vec<Value>)> = vec![];
    let mut add = |g: &str, row: Value| match groups.iter_mut().find(|(n, _)| n == g) {
        Some((_, rows)) => rows.push(row),
        None => groups.push((g.to_string(), vec![row])),
    };
    let levels = d.levels();
    add(
        "Constraints",
        select("Level", levels.iter().map(|(lid, l)| (l.name.clone(), nest("level", json!(lid)), *lid == i.level)).collect()),
    );
    match i.host {
        Some(h) => {
            add("Constraints", ro("Host", format!("Basic Wall ({})", h.wall)));
            add("Constraints", ro("Head Height", format!("{:.0}", sill + height)));
        }
        None => {
            let lv = d.level(i.level).map_or(String::new(), |l| l.name.clone());
            add("Constraints", ro("Host", format!("Level : {lv}")));
            add("Constraints", num("Elevation from Level", "offset_mm", i.offset_mm, true));
            add("Constraints", boolean("Moves With Nearby Elements", "moves_with_nearby", i.moves_with_nearby, true));
        }
    }
    if let Some(msg) = d.frame_fit(id) {
        add("Construction", ro("Frame Fit", msg));
    }
    // instance parameters edit; type parameters show their value for context
    for p in fam.params.iter().filter(|p| p.instance) {
        add(if p.group.is_empty() { "Other" } else { &p.group }, with_global(d, param_row(d, i, p, false), id, p, true));
    }
    for p in fam.params.iter().filter(|p| !p.instance && p.group != "Identity Data") {
        add(if p.group.is_empty() { "Other" } else { &p.group }, param_row(d, i, p, true));
    }
    add("Identity Data", text("Comments", "comments", &i.comments));
    add("Identity Data", text("Mark", "mark", &i.mark));
    if let Some(h) = i.host {
        let _ = h;
        add("Constraints", boolean("Flip Facing", "flip_facing", i.flip_facing, true));
        add("Constraints", boolean("Flip Hand", "flip_hand", i.flip_hand, true));
    }
    add("Constraints", boolean("Pinned", "pinned", i.pinned, true));
    add("Phasing", ro("Phase Created", "New Construction"));
    add("Phasing", ro("Phase Demolished", "None"));
    add("IFC Parameters", enum_select("Export to IFC", "ifc.export", &name_of(&i.ifc.export), &[("by_type", "By Type"), ("yes", "Yes"), ("no", "No")]));
    add("IFC Parameters", text("Export to IFC As", "ifc.export_as", &i.ifc.export_as));
    add("IFC Parameters", text("IFC Predefined Type", "ifc.predefined_type", &i.ifc.predefined_type));
    add("IFC Parameters", ro("IfcGUID", i.ifc.guid.clone()));
    let order = ["Constraints", "Construction", "Materials and Finishes", "Dimensions", "Identity Data", "Phasing", "IFC Parameters"];
    groups.sort_by_key(|(n, _)| order.iter().position(|o| o == n).unwrap_or(order.len()));
    Ok(json!({
        "id": id, "kind": "FamilyInstance", "category": fam.category.label(), "type_id": i.type_id, "type_label": format!("{}: {}", fam.name, t.name),
        "title": format!("{}: {}", fam.category.label().trim_end_matches('s'), fam.name),
        "target": { "method": "patch_instance", "id": id },
        "groups": groups.into_iter().map(|(n, rows)| group(&n, rows)).collect::<Vec<_>>(),
    }))
}

/// Type parameters of a family type, grouped like the Edit Type dialog.
pub fn type_props(d: &Document, id: Id, t: &FamilyType) -> Res {
    let fam = d.family(t.family).ok_or("type has no family")?;
    let mut groups: Vec<(String, Vec<Value>)> = vec![];
    for p in fam.params.iter().filter(|p| !p.instance) {
        let v = d.type_param(t, &p.name).unwrap_or_else(|| p.default.clone());
        let path = format!("values.{}", p.name);
        let row = match v {
            ParamValue::Bool(b) => boolean(&p.name, &path, b, true),
            ParamValue::Number(n) => num(&p.name, &path, n, true),
            ParamValue::Text(s) => text(&p.name, &path, &s),
        };
        let row = with_global(d, row, id, p, true);
        let g = if p.group.is_empty() { "Other".to_string() } else { p.group.clone() };
        match groups.iter_mut().find(|(n, _)| *n == g) {
            Some((_, rows)) => rows.push(row),
            None => groups.push((g, vec![row])),
        }
    }
    let order = ["Constraints", "Construction", "Materials and Finishes", "Dimensions", "Identity Data", "IFC Parameters", "Analytical Properties"];
    groups.sort_by_key(|(n, _)| order.iter().position(|o| o == n).unwrap_or(order.len()));
    Ok(json!({
        "id": id, "kind": "FamilyType", "family": fam.name, "name": t.name,
        "siblings": d.family_types().iter().filter(|(_, o)| o.family == t.family).map(|(i, o)| json!({ "id": i, "name": o.name })).collect::<Vec<_>>(),
        "target": { "method": "patch_family_type", "id": id },
        "groups": groups.into_iter().map(|(n, rows)| group(&n, rows)).collect::<Vec<_>>(),
    }))
}

/// Type Properties of a wall, floor or roof type: the assembly (Structure, edited in Edit Assembly),
/// wrapping and function, coarse fill, the thermal values that follow from the layers, identity data
/// and IFC export settings.
pub fn sys_type_props(d: &Document, id: Id) -> Res {
    use openarch_model::systypes::{assembly_values, SysKind};
    let kind = d.sys_kind(id).ok_or("not a wall, floor or roof type")?;
    let layers = d.sys_layers(id).unwrap_or(&[]);
    let (name, coarse, info) = match d.get(id) {
        Some(Element::WallType(t)) => (&t.name, &t.coarse_fill, &t.info),
        Some(Element::FloorType(t)) => (&t.name, &t.coarse_fill, &t.info),
        Some(Element::RoofType(t)) => (&t.name, &t.coarse_fill, &t.info),
        _ => unreachable!(),
    };
    let v = assembly_values(d, layers);
    let curtain = match d.get(id) {
        Some(Element::WallType(t)) => t.curtain.as_ref(),
        _ => None,
    };
    let mut construction = if curtain.is_some() { vec![] } else { vec![dialog("Structure", "assembly", "Edit...")] };
    let mut curtain_groups: Vec<Value> = vec![];
    let siblings: Vec<Value> = match kind {
        SysKind::Wall => d.wall_types().iter().map(|(i, t)| json!({ "id": i, "name": t.name })).collect(),
        SysKind::Floor => d.floor_types().iter().map(|(i, t)| json!({ "id": i, "name": t.name })).collect(),
        SysKind::Roof => d.roof_types().iter().map(|(i, t)| json!({ "id": i, "name": t.name })).collect(),
    };
    let mut wall_extra = json!(null);
    match d.get(id) {
        Some(Element::WallType(t)) if t.curtain.is_some() => {
            let c = t.curtain.as_ref().unwrap();
            construction.push(enum_select(
                "Function",
                "function",
                &name_of(&t.function),
                &[("interior", "Interior"), ("exterior", "Exterior"), ("foundation", "Foundation"), ("retaining", "Retaining"), ("soffit", "Soffit"), ("core_shaft", "Core-shaft")],
            ));
            construction.push(boolean("Automatically Embed", "curtain.auto_embed", c.auto_embed, true));
            construction.push(enum_select(
                "Join Condition",
                "curtain.join",
                &name_of(&c.join),
                &[("not_defined", "Not Defined"), ("vertical_continuous", "Vertical Grid Continuous"), ("horizontal_continuous", "Horizontal Grid Continuous"), ("border_and_vertical", "Border and Vertical Grid Continuous"), ("border_and_horizontal", "Border and Horizontal Grid Continuous")],
            ));
            curtain_groups = curtain_type_groups(d, c);
            wall_extra = json!({ "sample_height_mm": t.sample_height_mm });
        }
        Some(Element::WallType(t)) => {
            construction.push(enum_select("Wrapping at Inserts", "wrap_inserts", &name_of(&t.wrap_inserts), &[("none", "Do not wrap"), ("exterior", "Exterior"), ("interior", "Interior"), ("both", "Both")]));
            construction.push(enum_select("Wrapping at Ends", "wrap_ends", &name_of(&t.wrap_ends), &[("none", "None"), ("exterior", "Exterior"), ("interior", "Interior")]));
            construction.push(ro("Width", format!("{}", v.thickness_mm)));
            construction.push(enum_select(
                "Function",
                "function",
                &name_of(&t.function),
                &[("interior", "Interior"), ("exterior", "Exterior"), ("foundation", "Foundation"), ("retaining", "Retaining"), ("soffit", "Soffit"), ("core_shaft", "Core-shaft")],
            ));
            wall_extra = json!({ "wrap_ends": t.wrap_ends, "wrap_inserts": t.wrap_inserts, "sample_height_mm": t.sample_height_mm });
        }
        Some(Element::FloorType(t)) => {
            construction.push(ro("Default Thickness", format!("{}", v.thickness_mm)));
            construction.push(enum_select("Function", "function", &name_of(&t.function), &[("interior", "Interior"), ("exterior", "Exterior")]));
        }
        _ => construction.push(ro("Default Thickness", format!("{}", v.thickness_mm))),
    }
    let colour = coarse.color.unwrap_or([0, 0, 0]);
    let fmt = |x: Option<f64>, digits: usize, unit: &str| x.map_or(String::new(), |x| format!("{x:.digits$} {unit}"));
    let roughness: Vec<(String, Value, bool)> = [(1, "1 (Very Rough)"), (2, "2 (Rough)"), (3, "3 (Medium Rough)"), (4, "4 (Medium Smooth)"), (5, "5 (Smooth)"), (6, "6 (Very Smooth)")]
        .iter()
        .map(|(n, l)| (l.to_string(), json!({ "info": { "roughness": n } }), info.roughness == *n))
        .collect();
    let identity = vec![
        text("Keynote", "info.keynote", &info.keynote),
        text("Model", "info.model", &info.model),
        text("Manufacturer", "info.manufacturer", &info.manufacturer),
        text("Type Comments", "info.type_comments", &info.type_comments),
        text("URL", "info.url", &info.url),
        text("Description", "info.description", &info.description),
        text("Assembly Description", "info.assembly_description", &info.assembly_description),
        text("Assembly Code", "info.assembly_code", &info.assembly_code),
        text("Type Mark", "info.type_mark", &info.type_mark),
        text("Fire Rating", "info.fire_rating", &info.fire_rating),
        num("Cost", "info.cost", info.cost.unwrap_or(0.0), true),
    ];
    Ok(json!({
        "id": id, "kind": d.get(id).map(|e| e.kind()), "sys_kind": kind, "family": kind.family(), "name": name, "title": name,
        "siblings": siblings,
        "target": { "method": "patch_system_type", "id": id },
        "patch": "patch_system_type", "duplicate": "duplicate_system_type",
        "sides": [kind.sides().0, kind.sides().1],
        "layers": layers, "wall": wall_extra, "values": v,
        "curtain": curtain.is_some(),
        "groups": all_groups(group("Construction", construction), curtain_groups, vec![
            group("Graphics", vec![
                fill_pattern_row("Coarse Scale Fill Pattern", "coarse_fill.pattern", coarse.pattern.as_deref(), "drafting"),
                json!({ "label": "Coarse Scale Fill Color", "value": format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2]), "raw": colour, "kind": "color", "path": "coarse_fill.color" }),
            ]),
            group("Materials and Finishes", vec![ro("Structural Material", v.structural_material.clone().unwrap_or_else(|| "<By Category>".into()))]),
            group("Analytical Properties", vec![
                ro("Heat Transfer Coefficient (U)", fmt(v.u_w_m2k, 4, "W/(m\u{b2}\u{b7}K)")),
                ro("Thermal Resistance (R)", fmt(v.r_m2k_w, 4, "(m\u{b2}\u{b7}K)/W")),
                ro("Thermal Mass", fmt(v.thermal_mass_kj_m2k, 2, "kJ/(m\u{b2}\u{b7}K)")),
                num("Absorptance", "info.absorptance", info.absorptance, true),
                select("Roughness", roughness),
            ]),
            group("Identity Data", identity),
            group("IFC Parameters", vec![
                text("Export Type to IFC As", "info.ifc_export_as", &info.ifc_export_as),
                text("Type IFC Predefined Type", "info.ifc_predefined_type", &info.ifc_predefined_type),
            ]),
        ]),
    }))
}

/// Construction first, then a curtain type's own groups, then the rest.
fn all_groups(construction: Value, curtain: Vec<Value>, rest: Vec<Value>) -> Vec<Value> {
    std::iter::once(construction).chain(curtain).chain(rest).collect()
}

/// A curtain wall type's grids, panel and mullions as Type Properties groups.
fn curtain_type_groups(d: &Document, c: &openarch_model::curtain::CurtainSpec) -> Vec<Value> {
    use openarch_model::curtain::{MullionShape, MullionSpec};
    let grid = |key: &str, title: &str, r: &openarch_model::curtain::GridRule| {
        group(title, vec![
            enum_select("Layout", &format!("curtain.{key}.layout"), &name_of(&r.layout), &[("none", "None"), ("fixed_distance", "Fixed Distance"), ("fixed_number", "Fixed Number"), ("maximum_spacing", "Maximum Spacing"), ("minimum_spacing", "Minimum Spacing")]),
            num("Spacing", &format!("curtain.{key}.spacing_mm"), r.spacing_mm, true),
            num("Number", &format!("curtain.{key}.number"), f64::from(r.number), true),
            enum_select("Justification", &format!("curtain.{key}.justify"), &name_of(&r.justify), &[("beginning", "Beginning"), ("center", "Center"), ("end", "End")]),
            num("Offset", &format!("curtain.{key}.offset_mm"), r.offset_mm, true),
        ])
    };
    let materials: Vec<String> = d.materials().into_iter().map(|(_, m)| m.name.clone()).collect();
    let material_row = |label: &str, path: &str, current: &str| {
        let mut opts: Vec<(String, Value, bool)> = materials.iter().map(|m| (m.clone(), nest(path, json!(m)), m == current)).collect();
        if !materials.iter().any(|m| m == current) {
            opts.insert(0, (current.to_string(), nest(path, json!(current)), true));
        }
        select(label, opts)
    };
    // mullion types by their size; one that is none of these shows as Custom
    let presets: Vec<(&str, Option<MullionSpec>)> = vec![
        ("None", None),
        ("Rectangular 50 x 150", Some(MullionSpec::rectangular(50.0, 150.0))),
        ("Rectangular 50 x 100", Some(MullionSpec::rectangular(50.0, 100.0))),
        ("Rectangular 30 x 50", Some(MullionSpec::rectangular(30.0, 50.0))),
        ("Circular 50 Diameter", Some(MullionSpec { shape: MullionShape::Circular { radius_mm: 25.0 }, offset_mm: 0.0, material: "Aluminium".into() })),
    ];
    let mullion = |label: &str, key: &str, cur: &Option<MullionSpec>| {
        let same = |p: &Option<MullionSpec>| match (p, cur) {
            (None, None) => true,
            (Some(a), Some(b)) => a.shape == b.shape,
            _ => false,
        };
        let mut opts: Vec<(String, Value, bool)> = presets.iter().map(|(l, p)| (l.to_string(), nest(&format!("curtain.{key}"), json!(p)), same(p))).collect();
        if !presets.iter().any(|(_, p)| same(p)) {
            opts.insert(0, ("Custom".into(), nest(&format!("curtain.{key}"), json!(cur)), true));
        }
        select(label, opts)
    };
    vec![
        grid("u", "Vertical Grid", &c.u),
        grid("v", "Horizontal Grid", &c.v),
        group("Panel", vec![
            enum_select("Panel", "curtain.panel.kind", &name_of(&c.panel.kind), &[("glazed", "System Panel: Glazed"), ("solid", "System Panel: Solid"), ("empty", "Empty System Panel")]),
            num("Thickness", "curtain.panel.thickness_mm", c.panel.thickness_mm, true),
            num("Offset", "curtain.panel.offset_mm", c.panel.offset_mm, true),
            material_row("Material", "curtain.panel.material", &c.panel.material),
        ]),
        group("Vertical Mullions", vec![mullion("Interior Type", "u_interior", &c.u_interior), mullion("Border 1 Type", "u_border1", &c.u_border1), mullion("Border 2 Type", "u_border2", &c.u_border2)]),
        group("Horizontal Mullions", vec![mullion("Interior Type", "v_interior", &c.v_interior), mullion("Border 1 Type", "v_border1", &c.v_border1), mullion("Border 2 Type", "v_border2", &c.v_border2)]),
    ]
}

/// Properties of several selected elements together. A row shows when every element has it with
/// the same way of editing it (same patch), so one patch applies to all of them. Where the values
/// differ the row says "*Varies*". Walls of different types still share height, location line and
/// so on; a selection of one category also offers the type selector.
pub fn multi_props(d: &Document, ids: &[Id]) -> Res {
    let mut all: Vec<Value> = vec![];
    for &id in ids {
        all.push(match d.get(id).ok_or("no such element")? {
            Element::Wall(w) => wall_props(d, id, w),
            Element::Floor(f) => floor_props(d, id, f),
            Element::Roof(r) => roof_props(d, id, r),
            Element::FamilyInstance(i) => instance_props(d, id, i)?,
            Element::Grid(g) => grid_props(d, id, g),
            Element::DetailLine(l) => detail_line_props(d, id, l),
            _ => return Err("these elements cannot be edited together".into()),
        });
    }
    let first = all.first().ok_or("nothing selected")?;
    let kind_of = |v: &Value| match v["kind"].as_str() {
        Some("FamilyInstance") => v["category"].as_str().unwrap_or("Elements").to_string(),
        Some("Wall") => "Walls".to_string(),
        Some("Floor") => "Floors".to_string(),
        Some("Roof") => "Roofs".to_string(),
        Some("Grid") => "Grids".to_string(),
        Some("DetailLine") => "Lines".to_string(),
        _ => "Elements".to_string(),
    };
    let label = kind_of(first);
    let same_category = all.iter().all(|v| kind_of(v) == label);
    let title = if same_category { format!("{label} ({})", ids.len()) } else { format!("Multiple Categories ({})", ids.len()) };
    // the type selector: one category (all walls, all floors, or all doors, windows...)
    let type_kind = if !same_category { Value::Null } else { match first["kind"].as_str() { Some("Wall") => json!("wall"), Some("Floor") => json!("floor"), Some("Roof") => json!("roof"), Some("Grid") => json!("grid"), _ => json!("instance") } };
    let type_ids: Vec<Value> = all.iter().map(|v| match v["kind"].as_str() {
        Some("Wall") => v["type_id"].clone(),
        Some("Floor") => v["type_id"].clone(),
        _ => v["type_id"].clone(),
    }).collect();
    let common_type = if type_ids.iter().all(|t| *t == type_ids[0]) { type_ids[0].clone() } else { Value::Null };
    // rows
    let mut groups: Vec<Value> = vec![];
    for g in first["groups"].as_array().into_iter().flatten() {
        let gname = g["name"].as_str().unwrap_or("");
        let mut rows: Vec<Value> = vec![];
        for row in g["rows"].as_array().into_iter().flatten() {
            let label = row["label"].as_str().unwrap_or("");
            let mut found: Vec<&Value> = vec![];
            for v in &all {
                let r = v["groups"].as_array().into_iter().flatten().filter(|x| x["name"].as_str() == Some(gname)).flat_map(|x| x["rows"].as_array().into_iter().flatten()).find(|r| r["label"].as_str() == Some(label));
                match r {
                    Some(r) => found.push(r),
                    None => break,
                }
            }
            if found.len() != all.len() || row["kind"] == "dialog" {
                continue;
            }
            let same_value = found.iter().all(|r| r["value"] == row["value"]);
            let editable = found.iter().all(|r| r["kind"] == row["kind"] && r["kind"] != "ro") && matches!(row["kind"].as_str(), Some("number" | "text" | "bool" | "select"));
            let mut out = row.clone();
            if !editable {
                let mut ro_row = ro(label, if same_value { row["value"].as_str().unwrap_or("").to_string() } else { "*Varies*".to_string() });
                if let Some(k) = row.get("kind") { if k != "ro" && !same_value { ro_row["varies"] = json!(true); } }
                rows.push(ro_row);
                continue;
            }
            match row["kind"].as_str() {
                Some("select") => {
                    // the options every element offers with the same patch
                    let opts: Vec<Value> = row["options"].as_array().into_iter().flatten().filter(|o| found.iter().all(|r| r["options"].as_array().into_iter().flatten().any(|p| p["label"] == o["label"] && p["patch"] == o["patch"]))).cloned().collect();
                    if opts.is_empty() {
                        continue;
                    }
                    let sel_label = |r: &Value| r["options"].as_array().into_iter().flatten().find(|o| o["selected"] == true).map(|o| o["label"].clone());
                    let same = found.iter().all(|r| sel_label(r) == sel_label(found[0]));
                    out["options"] = json!(opts.into_iter().map(|mut o| { o["selected"] = json!(same && Some(o["label"].clone()) == sel_label(found[0])); o }).collect::<Vec<_>>());
                    if !same { out["varies"] = json!(true); }
                }
                _ => {
                    if !found.iter().all(|r| r["path"] == row["path"]) {
                        continue;
                    }
                    if !same_value {
                        out["varies"] = json!(true);
                        out["value"] = json!("");
                        out["raw"] = Value::Null;
                    }
                }
            }
            rows.push(out);
        }
        if !rows.is_empty() {
            groups.push(group(gname, rows));
        }
    }
    Ok(json!({
        "kind": "Multi", "title": title, "count": ids.len(), "type_kind": type_kind, "category": if same_category { json!(label) } else { Value::Null },
        "type_id": common_type, "ids": ids,
        "target": { "method": "patch_multi", "id": 0, "extra": { "ids": ids } },
        "groups": groups,
    }))
}

/// Properties of a level (the Level properties: Elevation, Name, Building Story, Structural
/// and the type's Elevation Base).
pub fn level_props(d: &Document, id: Id, l: &Level) -> Value {
    let above = d.levels().into_iter().filter(|(_, o)| o.elevation_mm > l.elevation_mm + 1e-6).min_by(|a, b| a.1.elevation_mm.total_cmp(&b.1.elevation_mm)).map(|(_, o)| o.name.clone());
    let base = name_of(&l.elevation_base);
    json!({
        "id": id, "kind": "Level", "title": format!("Level: {}", l.name), "type_label": "Level",
        "type_id": d.level_type_of(l).map(|(t, _)| t),
        "target": { "method": "patch_level", "id": id },
        "groups": [
            group("Constraints", vec![
                num("Elevation", "display_elevation_mm", d.level_display_elevation(l), true),
                ro("Story Above", above.unwrap_or_else(|| "Default".into())),
                ro("Computation Height", format!("{:.0}", l.elevation_mm - d.level_datum_z(l) + 0.0)),
            ]),
            group("Identity Data", vec![
                text("Name", "name", &l.name),
                text("Level Number", "number", &l.number),
                boolean("Building Story", "building_story", l.building_story, true),
                boolean("Structural", "structural", l.structural, true),
            ]),
            group("Type Parameters", vec![
                enum_select("Elevation Base", "elevation_base", &base, &[("project_base_point", "Project Base Point"), ("survey_point", "Survey Point")]),
            ]),
        ],
    })
}

/// Properties of the project base point or the survey point (shown as N/S, E/W and Elev as
/// shared coordinates, and the angle to true north on the base point).
pub fn site_point_props(d: &Document, which: openarch_model::site::SitePoint) -> Res {
    use openarch_model::site::{SitePoint, P3};
    let (_, loc) = d.project_location().ok_or("the project has no location")?;
    let (pos, pinned_path, pinned) = match which {
        SitePoint::Base => (loc.base, "base_pinned", loc.base_pinned),
        SitePoint::Survey => (loc.survey, "survey_pinned", loc.survey_pinned),
    };
    let shared = d.shared_coordinates(P3 { x: pos.x, y: pos.y, z: pos.z });
    let (title, prefix, editable) = match which {
        SitePoint::Base => ("Project Base Point", "base", true),
        SitePoint::Survey => ("Survey Point", "survey", !loc.survey_clipped),
    };
    let mut loc_rows = vec![
        num("N/S", &format!("{prefix}_shared_y"), shared.y, editable),
        num("E/W", &format!("{prefix}_shared_x"), shared.x, editable),
        num("Elev", &format!("{prefix}_shared_z"), shared.z, editable),
    ];
    if which == SitePoint::Base {
        loc_rows.push(num("Angle to True North", "angle_true_north_deg", loc.angle_true_north_deg, true));
    }
    let mut cons = vec![boolean("Pinned", pinned_path, pinned, true)];
    if which == SitePoint::Survey {
        cons.push(boolean("Clipped", "survey_clipped", loc.survey_clipped, true));
    }
    Ok(json!({
        "id": if which == SitePoint::Base { crate::SITE_BASE_ID } else { crate::SITE_SURVEY_ID }, "kind": "SitePoint", "title": title, "type_label": title,
        "target": { "method": "patch_project_location", "id": 0 },
        "groups": [group("Constraints", cons), group("Location", loc_rows), group("Identity Data", vec![ro("Category", "Site")])],
    }))
}

/// Properties of an elevation marker: which of its four pointers have views, and its pose.
pub fn marker_props(d: &Document, id: Id, m: &openarch_model::site::ElevationMarker) -> Value {
    use openarch_model::site::SLOT_NAMES;
    let slots: Vec<Value> = (0..4)
        .map(|k| {
            let label = match m.slots[k].and_then(|v| d.view(v)) {
                Some(v) => format!("View {}: {}", openarch_model::site::ElevationMarker::view_letter(k), v.name),
                None => format!("View {}: {}", openarch_model::site::ElevationMarker::view_letter(k), if m.rotation_deg == 0.0 { SLOT_NAMES[k] } else { "(off)" }),
            };
            boolean(&label, &format!("slot_on.{k}"), m.slots[k].is_some(), true)
        })
        .collect();
    json!({
        "id": id, "kind": "ElevationMarker", "title": "Elevation Marker", "type_label": "Elevation Marker",
        "target": { "method": "patch_marker", "id": id },
        "groups": [
            group("Constraints", vec![boolean("Pinned", "pinned", m.pinned, true), num("Rotation", "rotation_deg", m.rotation_deg, true)]),
            group("Graphics", vec![select("Symbol", d.annotation_families().into_iter().filter(|(_, f)| f.kind == openarch_model::annotation::AnnotationKind::ElevationMarker).map(|(fid, f)| (f.name.clone(), nest("family", json!(fid)), Some(fid) == d.marker_family(m.family).map(|(i, _)| i))).collect())]),
            group("Views", slots),
            group("Identity Data", vec![ro("Level", d.level(m.level).map_or("?", |l| l.name.as_str()))]),
        ],
    })
}

/// Properties of a grid (Constraints, Identity Data; the type's settings are in Edit Type).
pub fn grid_props(d: &Document, id: Id, g: &openarch_model::grid::Grid) -> Value {
    let kind = if g.multi_segment() { format!("Multi-Segment ({})", g.curves.len()) } else if g.line().is_some() { "Line".to_string() } else { "Arc".to_string() };
    json!({
        "id": id, "kind": "Grid", "title": format!("Grid: {}", g.name), "type_label": "Grid",
        "type_id": g.grid_type,
        "target": { "method": "patch_grid", "id": id },
        "groups": [
            group("Constraints", vec![boolean("Pinned", "pinned", g.pinned, true), ro("Scope Box", if g.scope_box.is_some() { "Set" } else { "None" })]),
            group("Dimensions", vec![ro("Length", format!("{:.0}", g.length())), ro("Shape", kind)]),
            group("Identity Data", vec![text("Name", "name", &g.name)]),
        ],
    })
}

/// Properties of a detail line: its line style and length.
pub fn detail_line_props(d: &Document, id: Id, l: &openarch_model::detail::DetailLine) -> Value {
    let styles: Vec<(String, Value, bool)> = d.line_styles().into_iter().map(|s| (s.clone(), json!({ "style": s }), s == l.style)).collect();
    json!({
        "id": id, "kind": "DetailLine", "title": "Lines (1)", "type_label": "Detail Lines",
        "target": { "method": "patch_detail_line", "id": id },
        "groups": [
            group("Graphics", vec![select("Line Style", styles)]),
            group("Constraints", vec![boolean("Pinned", "pinned", l.pinned, true)]),
            group("Dimensions", vec![ro("Length", format!("{:.0}", l.curve.length()))]),
        ],
    })
}

/// Properties of a filled region (its type, with Edit Type) or a masking region, and its area.
pub fn detail_region_props(d: &Document, id: Id, r: &openarch_model::detail::DetailRegion) -> Value {
    let types: Vec<(String, Value, bool)> = d.filled_region_types().into_iter().map(|(tid, t)| (t.name.clone(), json!({ "region_type": tid }), Some(tid) == r.region_type)).collect();
    let area = d.detail_region_area(r) / 1.0e6;
    let mut groups = vec![];
    if !r.masking {
        groups.push(group("Graphics", vec![select("Filled Region Type", types)]));
    }
    groups.push(group("Constraints", vec![boolean("Pinned", "pinned", r.pinned, true)]));
    groups.push(group("Dimensions", vec![ro("Area", format!("{area:.3} m²"))]));
    json!({
        "id": id, "kind": "DetailRegion", "title": if r.masking { "Masking Region" } else { "Filled Region" },
        "type_label": if r.masking { "Masking Region" } else { "Filled region" },
        "type_id": r.region_type,
        "target": { "method": "patch_detail_region", "id": id },
        "groups": groups,
    })
}

/// Type Properties of a filled region type: what it is filled with.
pub fn filled_region_type_props(d: &Document, id: Id, t: &openarch_model::annotation::RegionType) -> Value {
    let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let weights: Vec<(String, Value, bool)> = (1..=16u8).map(|w| (w.to_string(), json!({ "line_weight": w }), w == t.line_weight)).collect();
    json!({
        "id": id, "kind": "FilledRegionType", "title": t.name,
        "siblings": d.filled_region_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "groups": [
            group("Graphics", vec![
                fill_pattern_row("Foreground Fill Pattern", "foreground", t.foreground.as_deref(), "drafting"),
                json!({ "label": "Foreground Pattern Color", "value": hex(t.foreground_color), "raw": t.foreground_color, "kind": "color", "path": "foreground_color" }),
                fill_pattern_row("Background Fill Pattern", "background", t.background.as_deref(), "drafting"),
                json!({ "label": "Background Pattern Color", "value": hex(t.background_color), "raw": t.background_color, "kind": "color", "path": "background_color" }),
                select("Line Weight", weights),
                boolean("Masking", "masking", t.masking, true),
            ]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// Properties of a text note: alignment, where its leaders leave the box, Keep Readable.
pub fn text_note_props(_d: &Document, id: Id, t: &openarch_model::detail::TextNote) -> Value {
    let va = |v: openarch_model::annotation::VAlign| name_of(&v);
    let vs = [("top", "Top"), ("middle", "Middle"), ("bottom", "Bottom")];
    json!({
        "id": id, "kind": "TextNote", "title": "Text Notes (1)", "type_label": "Text",
        "type_id": t.text_type, "text": t.text,
        "target": { "method": "patch_text_note", "id": id },
        "groups": [
            group("Graphics", vec![
                enum_select("Leader Left Attachment", "left_attach", &va(t.left_attach), &vs),
                enum_select("Leader Right Attachment", "right_attach", &va(t.right_attach), &vs),
                enum_select("Horizontal Align", "halign", &name_of(&t.halign), &[("left", "Left"), ("center", "Center"), ("right", "Right")]),
                enum_select("Vertical Align", "valign", &va(t.valign), &vs),
                boolean("Keep Readable", "keep_readable", t.keep_readable, true),
            ]),
            group("Constraints", vec![boolean("Pinned", "pinned", t.pinned, true)]),
            group("Text", vec![ro("Text", t.text.replace('\n', " / ")), ro("Leaders", t.leaders.len().to_string())]),
        ],
    })
}

/// Type Properties of a text type (text notes): Graphics and Text, as the Type Properties dialog lists them.
pub fn text_type_props(d: &Document, id: Id, t: &openarch_model::annotation::TextType) -> Value {
    let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let weights: Vec<(String, Value, bool)> = (1..=16u8).map(|w| (w.to_string(), json!({ "line_weight": w }), w == t.line_weight)).collect();
    let mut fonts: Vec<&str> = vec!["Arial", "Arial Narrow", "Calibri", "Century Gothic", "Consolas", "Courier New", "Georgia", "Segoe UI", "Tahoma", "Times New Roman", "Verdana"];
    if !fonts.contains(&t.font.as_str()) {
        fonts.insert(0, &t.font);
    }
    json!({
        "id": id, "kind": "TextNoteType", "title": t.name,
        "siblings": d.text_note_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "groups": [
            group("Graphics", vec![
                json!({ "label": "Color", "value": hex(t.color), "raw": t.color, "kind": "color", "path": "color" }),
                select("Line Weight", weights),
                select("Background", vec![("Opaque".into(), json!({ "opaque": true }), t.opaque), ("Transparent".into(), json!({ "opaque": false }), !t.opaque)]),
                boolean("Show Border", "show_border", t.show_border, true),
                num("Leader/Border Offset", "border_offset_mm", t.border_offset_mm, true),
            ]),
            group("Text", vec![
                select("Text Font", fonts.iter().map(|f| (f.to_string(), json!({ "font": f }), *f == t.font)).collect()),
                num("Text Size", "size_mm", t.size_mm, true),
                num("Tab Size", "tab_size_mm", t.tab_size_mm, true),
                boolean("Bold", "bold", t.bold, true),
                boolean("Italic", "italic", t.italic, true),
                boolean("Underline", "underline", t.underline, true),
                num("Width Factor", "width_factor", t.width_factor, true),
            ]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// Properties of a permanent dimension.
pub fn dimension_props(d: &Document, id: Id, x: &openarch_model::dimension::Dimension) -> Value {
    use openarch_model::dimension::EqDisplay;
    let types: Vec<(String, Value, bool)> = d.dimension_types().into_iter().map(|(tid, t)| (t.name.clone(), nest("dim_type", json!(tid)), tid == x.dim_type)).collect();
    let eq = match x.eq_display { EqDisplay::Value => "value", EqDisplay::Eq => "eq", EqDisplay::Formula => "formula" };
    let draw = d.dimension_draw(id);
    let total = draw.as_ref().map(|dr| dr.segments.iter().map(|s| s.value_mm).sum::<f64>()).unwrap_or(0.0);
    let round_value = draw.as_ref().and_then(|dr| dr.segments.first().map(|s| s.value_mm));
    let warning = draw.and_then(|dr| dr.invalid);
    let chain = x.refs.len() >= 3;
    json!({
        "id": id, "kind": "Dimension", "title": format!("Dimension ({})", match x.kind { openarch_model::dimension::DimKind::Aligned => "Aligned", openarch_model::dimension::DimKind::Linear => "Linear", openarch_model::dimension::DimKind::Angular => "Angular", openarch_model::dimension::DimKind::Radial => "Radial", openarch_model::dimension::DimKind::Diameter => "Diameter", openarch_model::dimension::DimKind::ArcLength => "Arc Length" }),
        "type_label": "Dimension", "type_id": x.dim_type,
        "target": { "method": "patch_dimension", "id": id },
        "groups": [
            group("Graphics", vec![select("Dimension Type", types)]),
            group("Constraints", if x.kind.is_chain() {
                vec![
                    boolean("Equality", "equality", x.equality, chain),
                    enum_select("Equality Display", "eq_display", eq, &[("value", "Value"), ("eq", "Equality Text"), ("formula", "Equality Formula")]),
                    boolean("Pinned", "pinned", x.pinned, true),
                ]
            } else {
                vec![boolean("Pinned", "pinned", x.pinned, true)]
            }),
            group("Dimensions", if x.kind.is_chain() {
                vec![ro("Segments", format!("{}", x.segments.len())), ro("Overall Length", if warning.is_some() { String::new() } else { format!("{:.0}", total) })].into_iter().chain(warning.map(|w| ro("Warning", w))).collect()
            } else {
                let (what, unit) = match x.kind {
                    openarch_model::dimension::DimKind::Angular => ("Angle", "\u{00B0}"),
                    openarch_model::dimension::DimKind::Radial => ("Radius", ""),
                    openarch_model::dimension::DimKind::Diameter => ("Diameter", ""),
                    _ => ("Arc Length", ""),
                };
                let mut rows = vec![ro(what, if warning.is_some() { String::new() } else { round_value.map_or(String::new(), |v| if unit.is_empty() { format!("{:.0}", v) } else { format!("{:.2}{unit}", v) }) })];
                match x.kind {
                    openarch_model::dimension::DimKind::Angular => rows.push(boolean("Reflex Angle", "reflex", x.reflex, true)),
                    openarch_model::dimension::DimKind::Radial | openarch_model::dimension::DimKind::Diameter => rows.push(boolean("Centre Mark", "centre_mark", x.centre_mark, true)),
                    _ => {}
                }
                rows.into_iter().chain(warning.map(|w| ro("Warning", w))).collect()
            }),
            group("Label", if x.kind != openarch_model::dimension::DimKind::ArcLength { label_rows(d, x) } else { vec![ro("Label", "Arc length dimensions take no label yet")] }),
        ],
    })
}

/// Type Properties of a dimension type (Edit Type).
pub fn dimension_type_props(d: &Document, id: Id, t: &openarch_model::dimension::DimensionType) -> Value {
    use openarch_model::dimension::{AltPlacement, LengthUnit, TickStyle};
    let unit_name = |u: LengthUnit| match u { LengthUnit::Millimetres => "millimetres", LengthUnit::Centimetres => "centimetres", LengthUnit::Metres => "metres", LengthUnit::FeetInches => "feet_inches" };
    let units = |path: &str, cur: LengthUnit| select("Units", [("Millimetres", LengthUnit::Millimetres), ("Centimetres", LengthUnit::Centimetres), ("Metres", LengthUnit::Metres), ("Feet and inches", LengthUnit::FeetInches)].iter().map(|(l, u)| (l.to_string(), nest(path, json!(unit_name(*u))), *u == cur)).collect());
    let tick = |s: TickStyle| match s { TickStyle::Tick => "tick", TickStyle::Arrow => "arrow", TickStyle::Dot => "dot", TickStyle::None => "none" };
    let mut format = vec![
        units("format.unit", t.format.unit),
        num("Decimal Places", "format.decimals", t.format.decimals as f64, true),
        boolean("Suppress Trailing Zeros", "format.suppress_trailing_zeros", t.format.suppress_trailing_zeros, true),
        boolean("Unit Symbol", "format.symbol", t.format.symbol, true),
        num("Angle Decimal Places", "angle_decimals", t.angle_decimals as f64, true),
    ];
    let alt_now = t.alt.map(|a| a.placement);
    let alt_fmt = t.alt.map(|a| a.format).unwrap_or(openarch_model::dimension::LengthFormat { unit: LengthUnit::FeetInches, ..Default::default() });
    let alt_opt = |p: AltPlacement| json!({ "alt": { "format": alt_fmt, "placement": if p == AltPlacement::Below { "below" } else { "right" } } });
    format.push(select("Alternate Units", vec![("None".into(), json!({ "alt": null }), alt_now.is_none()), ("Right".into(), alt_opt(AltPlacement::Right), alt_now == Some(AltPlacement::Right)), ("Below".into(), alt_opt(AltPlacement::Below), alt_now == Some(AltPlacement::Below))]));
    if t.alt.is_some() {
        let mut u = units("alt.format.unit", alt_fmt.unit);
        u["label"] = json!("Alternate Units Format");
        format.push(u);
        format.push(num("Alternate Decimal Places", "alt.format.decimals", alt_fmt.decimals as f64, true));
    }
    json!({
        "id": id, "kind": "DimensionType", "title": t.name,
        "siblings": d.dimension_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "groups": [
            group("Graphics", vec![
                select("Tick Mark", [("Diagonal tick", TickStyle::Tick), ("Arrow", TickStyle::Arrow), ("Dot", TickStyle::Dot), ("None", TickStyle::None)].iter().map(|(l, s)| (l.to_string(), json!({ "tick": tick(*s) }), *s == t.tick)).collect()),
                num("Tick Mark Size", "tick_size_mm", t.tick_size_mm, true),
                num("Witness Line Gap to Element", "witness_gap_mm", t.witness_gap_mm, true),
                num("Witness Line Extension", "witness_extension_mm", t.witness_extension_mm, true),
            ]),
            group("Text", vec![
                num("Text Size", "text_size_mm", t.text_size_mm, true),
                num("Text Offset", "text_offset_mm", t.text_offset_mm, true),
                boolean("Text Background (opaque)", "text_background", t.text_background, true),
                boolean("Show Value With Label", "show_label_value", t.show_label_value, true),
                text("Radius Symbol", "symbols.radius", &t.symbols.radius),
                text("Diameter Symbol", "symbols.diameter", &t.symbols.diameter),
                text("Arc Length Symbol", "symbols.arc_length", &t.symbols.arc_length),
            ]),
            group("Units Format", format),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// One Label row per segment: the global length parameters a segment can take (a reporting one only on a dimension of
/// one segment), and whether the type shows the value after the name.
fn label_rows(d: &Document, x: &openarch_model::dimension::Dimension) -> Vec<Value> {
    let one = x.segments.len() == 1;
    let globals: Vec<(Id, &openarch_model::globalparam::GlobalParam)> = d.global_params().into_iter().filter(|(_, g)| openarch_model::globalparam::labels_kind(g.kind, x.kind) && (one || !g.reporting)).collect();
    let rows: Vec<Value> = x
        .segments
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let mut opts: Vec<(String, Value, bool)> = vec![("<None>".to_string(), json!({ "labels": [{ "index": k, "param": null }] }), s.param.is_none())];
            for (gid, g) in &globals {
                let name = if g.reporting { format!("{} (reporting)", g.name) } else { g.name.clone() };
                opts.push((name, json!({ "labels": [{ "index": k, "param": gid }] }), s.param == Some(*gid)));
            }
            let mut row = select(if one { "Label".to_string() } else { format!("Label, segment {}", k + 1) }.as_str(), opts);
            row["add_global"] = json!({ "index": k });
            row
        })
        .collect();
    rows
}

/// Properties of a spot elevation, coordinate or slope.
pub fn spot_props(d: &Document, id: Id, x: &openarch_model::spot::Spot) -> Value {
    use openarch_model::spot::{SpotKind, SpotSurface};
    let types: Vec<(String, Value, bool)> = d.spot_types(x.kind).into_iter().map(|(tid, t)| (t.name.clone(), nest("spot_type", json!(tid)), tid == x.spot_type)).collect();
    let draw = d.spot_draw(id);
    let warning = draw.as_ref().and_then(|dr| dr.invalid.clone());
    let host_name = x.host.and_then(|h| d.get(h)).map(|e| e.kind().to_string()).unwrap_or_else(|| "None".to_string());
    let title = match x.kind { SpotKind::Elevation => "Spot Elevation", SpotKind::Coordinate => "Spot Coordinate", SpotKind::Slope => "Spot Slope" };
    let surface = name_of(&x.surface);
    let mut dims: Vec<Value> = vec![];
    if let Some(dr) = &draw {
        if warning.is_none() {
            for (i, l) in dr.lines.iter().enumerate() {
                dims.push(ro(if dr.lines.len() > 1 { ["Line 1", "Line 2", "Line 3"][i.min(2)] } else { "Value" }, l.clone()));
            }
        }
    }
    dims.extend(warning.map(|w| ro("Warning", w)));
    let mut graphics = vec![select("Spot Type", types), boolean("Leader", "leader", x.leader, true)];
    if x.host.is_some() && x.kind != SpotKind::Coordinate {
        graphics.insert(1, enum_select("Surface", "surface", &surface, &[("top", "Top"), ("core", "Core"), ("bottom", "Bottom")]));
    } else if x.host.is_some() {
        graphics.insert(1, ro("Surface", match x.surface { SpotSurface::Top => "Top", SpotSurface::Core => "Core", SpotSurface::Bottom => "Bottom" }));
    }
    json!({
        "id": id, "kind": "Spot", "title": title,
        "type_label": "Spot", "type_id": x.spot_type,
        "target": { "method": "patch_spot", "id": id },
        "groups": [
            group("Graphics", graphics),
            group("Text", vec![
                text("Prefix", "prefix", &x.prefix),
                text("Suffix", "suffix", &x.suffix),
                text("Text Above", "above", &x.above),
                text("Text Below", "below", &x.below),
            ]),
            group("Constraints", vec![boolean("Pinned", "pinned", x.pinned, true), ro("Host", host_name)]),
            group("Dimensions", dims),
        ],
    })
}

/// Type Properties of a grid type: the head size, the centre segment and the default heads.
pub fn grid_type_props(d: &Document, id: Id, t: &openarch_model::grid::GridType) -> Value {
    use openarch_model::grid::CentreSegment;
    let cs = match t.centre_segment { CentreSegment::None => "none", CentreSegment::Continuous => "continuous", CentreSegment::Custom => "custom" };
    json!({
        "id": id, "kind": "GridType", "title": t.name,
        "siblings": d.grid_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "groups": [
            group("Graphics", vec![
                enum_select("Centre Segment", "centre_segment", cs, &[("none", "None"), ("continuous", "Continuous"), ("custom", "Custom")]),
                num("End Segment Length", "end_segment_mm", t.end_segment_mm, t.centre_segment != CentreSegment::None),
                num("Head Diameter", "bubble_dia_mm", t.bubble_dia_mm, true),
            ]),
            group("Plan View Symbols", vec![
                boolean("End 1 Default", "bubble_default.0", t.bubble_default[0], true),
                boolean("End 2 Default", "bubble_default.1", t.bubble_default[1], true),
            ]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// Type Properties of a level type: the head family and the default heads.
pub fn level_type_props(d: &Document, id: Id, t: &openarch_model::annotation::LevelType) -> Value {
    let heads: Vec<(String, Value, bool)> = d
        .annotation_families()
        .into_iter()
        .filter(|(_, f)| f.kind == openarch_model::annotation::AnnotationKind::LevelHead)
        .map(|(fid, f)| (f.name.clone(), nest("head", json!(fid)), Some(fid) == t.head))
        .collect();
    json!({
        "id": id, "kind": "LevelType", "title": t.name, "head": t.head,
        "siblings": d.level_types().iter().map(|(i, x)| json!({ "id": i, "name": x.name })).collect::<Vec<_>>(),
        "groups": [
            group("Graphics", vec![select("Symbol", heads)]),
            group("Plan View Symbols", vec![
                boolean("End 1 Default", "end_default.0", t.end_default[0], true),
                boolean("End 2 Default", "end_default.1", t.end_default[1], true),
            ]),
            group("Identity Data", vec![text("Type Name", "name", &t.name)]),
        ],
    })
}

/// Properties of a section: its view, the side it looks at, how far, and the far clip. A reference section has no
/// view of its own, so instead of the extents of one it names the view it points at.
pub fn section_props(d: &Document, id: Id, s: &openarch_model::section::Section) -> Value {
    use openarch_model::section::FarClip;
    let view = s.view.and_then(|v| d.view(v));
    let far = match s.far_clip {
        FarClip::WithLine => "with_line",
        FarClip::WithoutLine => "without_line",
        FarClip::NoClip => "no_clip",
    };
    let (z0, z1) = s.z_range.unwrap_or_else(|| {
        let (a, b) = d.model_z_extent();
        (a - 1500.0, b + 1500.0)
    });
    let runs = s.drawn().map_or(1, |dr| dr.runs.len());
    let mut graphics = vec![
        select("Symbol", d.annotation_families().into_iter().filter(|(_, f)| f.kind == openarch_model::annotation::AnnotationKind::SectionMarker).map(|(fid, f)| (f.name.clone(), nest("family", json!(fid)), Some(fid) == d.section_marker_family(s.family).map(|(i, _)| i))).collect()),
        boolean("Flipped", "flipped", s.flipped, true),
    ];
    let (title, extents, identity) = match s.reference {
        None => {
            graphics.push(boolean("Fold Lines", "fold_lines", s.fold_lines, runs > 1));
            let extents = vec![
                enum_select("Far Clipping", "far_clip", far, &[("with_line", "Clip with line"), ("without_line", "Clip without line"), ("no_clip", "No clip")]),
                num("Far Clip Offset", "far_clip_mm", s.far_clip_mm.unwrap_or(0.0), s.far_clip != FarClip::NoClip),
                ro("Width", format!("{:.0}", s.length())),
                ro("Segments", runs.to_string()),
                ro("Bottom", format!("{z0:.0}")),
                ro("Top", format!("{z1:.0}")),
            ];
            ("Section", extents, vec![ro("View Name", view.map_or(String::new(), |v| v.name.clone()))])
        }
        Some(target) => {
            let opts = d.reference_targets().into_iter().map(|(vid, v)| (v.name.clone(), nest("reference", json!(vid)), vid == target)).collect();
            ("Reference Section", vec![ro("Width", format!("{:.0}", s.length())), ro("Segments", runs.to_string())], vec![select("Referenced View", opts)])
        }
    };
    let mut identity = identity;
    identity.push(ro("Level", d.level(s.level).map_or("?".to_string(), |l| l.name.clone())));
    json!({
        "id": id, "kind": "Section", "title": title, "type_label": "Section",
        "target": { "method": "patch_section", "id": id },
        "groups": [
            group("Constraints", vec![boolean("Pinned", "pinned", s.pinned, true)]),
            group("Graphics", graphics),
            group("Extents", extents),
            group("Identity Data", identity),
        ],
    })
}

/// Properties of a model group instance: its type, reference level and how many members it has.
pub fn group_props(d: &Document, id: Id, g: &openarch_model::group::GroupInstance) -> Value {
    let name = d.group_type(g.group_type).map(|t| t.name.clone()).unwrap_or_default();
    let total = d.group_type(g.group_type).map_or(0, |t| t.members.len());
    let live = g.live(d).len();
    let detail = d.group_type(g.group_type).is_some_and(|t| t.kind == openarch_model::group::GroupKind::Detail);
    let constraints = if detail {
        let view = g.frame.view.and_then(|v| d.view(v)).map(|v| v.name.clone()).unwrap_or_default();
        let mut c = vec![ro("Owner View", view)];
        if let Some(t) = d.group_type(g.group_type).and_then(|t| t.attached_to).and_then(|t| d.group_type(t)) {
            c.push(ro("Attached to", t.name.clone()));
        }
        c.push(boolean("Pinned", "pinned", g.pinned, true));
        c
    } else {
        let level = d.level(g.frame.level).map(|l| l.name.clone()).unwrap_or_default();
        vec![ro("Reference Level", level), ro("Origin Level Offset", "0"), boolean("Pinned", "pinned", g.pinned, true)]
    };
    let (many, one) = if detail { ("Detail Groups", "Detail Group") } else { ("Model Groups", "Model Group") };
    json!({
        "id": id, "kind": "GroupInstance", "title": format!("{many}: {name}"), "type_label": format!("{one}: {name}"),
        "type_id": g.group_type,
        "target": { "method": "patch_group", "id": id },
        "groups": [
            group("Constraints", constraints),
            group("Other", vec![ro("Members", live.to_string()), ro("Excluded", total.saturating_sub(live).to_string()), ro("Angle", format!("{:.2}°", g.frame.angle_deg)), ro("Mirrored", if g.frame.mirrored { "Yes" } else { "No" })]),
        ],
    })
}

/// Properties of a part of a curtain wall: a grid line, a panel or a mullion. Panels list the system panels as their
/// types; a pinned panel keeps the type's until it is unpinned.
pub fn curtain_part_props(d: &Document, id: Id) -> Res {
    use openarch_model::curtain::{part_of, CurtainRef, MullionShape, PanelKind};
    let (wall, r) = part_of(id).ok_or("not a part of a curtain wall")?;
    let Some(Element::Wall(w)) = d.get(wall) else { return Err("its curtain wall is gone".into()) };
    let spec = d.curtain_of(w).ok_or("not a curtain wall")?;
    let lay = d.curtain_layout(w).ok_or("the curtain wall has no grid")?;
    let target = json!({ "method": "patch_curtain_part", "id": id });
    let round = |x: f64| (x * 10.0).round() / 10.0;
    match r {
        CurtainRef::Line { vertical, k } => {
            let lines = lay.lines(vertical);
            let at = *lines.get(k).ok_or("the grid line is gone")?;
            let typed = lay.typed(vertical)[k];
            let edge = k == 0 || k + 1 == lines.len();
            let on = if vertical { &lay.u_on[k] } else { &lay.v_on[k] };
            Ok(json!({
                "id": id, "kind": "CurtainGrid", "wall": wall, "vertical": vertical, "k": k, "typed": typed, "edge": edge,
                "title": "Curtain Wall Grid", "type_label": "Curtain Wall Grid", "target": target,
                "groups": [
                    group("Constraints", vec![
                        num(if vertical { "Distance from Start" } else { "Height from Base" }, "at", round(at), !typed),
                        boolean("Pinned", "pinned", typed, false),
                    ]),
                    group("Identity Data", vec![
                        ro("Category", "Curtain Wall Grids"),
                        ro("Laid Out By", if edge { "Curtain wall edge" } else if typed { "Type" } else { "Placed by hand" }),
                        ro("Segments", format!("{} of {}", on.iter().filter(|x| **x).count(), on.len())),
                    ]),
                ],
            }))
        }
        CurtainRef::Panel { i, j } => {
            let n = *lay.cell_panel.get(i).and_then(|c| c.get(j)).ok_or("the panel is gone")?;
            let cells = &lay.panels[n];
            let (pinned, panel) = d.curtain_panel_state(w, &lay, n);
            let (i0, i1) = (cells.iter().map(|c| c.0).min().unwrap_or(0), cells.iter().map(|c| c.0).max().unwrap_or(0));
            let (j0, j1) = (cells.iter().map(|c| c.1).min().unwrap_or(0), cells.iter().map(|c| c.1).max().unwrap_or(0));
            let (width, height) = (lay.us[i1 + 1] - lay.us[i0], lay.vs[j1 + 1] - lay.vs[j0]);
            let (kind_id, family, name) = match panel.kind {
                PanelKind::Glazed => (0, "System Panel", "Glazed"),
                PanelKind::Solid => (1, "System Panel", "Solid"),
                PanelKind::Empty => (2, "Empty System Panel", "Empty"),
            };
            Ok(json!({
                "id": id, "kind": "CurtainPanel", "wall": wall, "type_id": kind_id, "pinned": pinned,
                "types": [
                    { "id": 0, "name": "Glazed", "family": "System Panel", "category": "Curtain Panels" },
                    { "id": 1, "name": "Solid", "family": "System Panel", "category": "Curtain Panels" },
                    { "id": 2, "name": "Empty", "family": "Empty System Panel", "category": "Curtain Panels" },
                ],
                "title": format!("{family}: {name}"), "type_label": format!("{family}: {name}"), "target": target,
                "groups": [
                    group("Constraints", vec![boolean("Pinned", "pinned", pinned, true)]),
                    group("Dimensions", vec![
                        num("Width", "", round(width), false),
                        num("Height", "", round(height), false),
                        ro("Area", format!("{:.3} m\u{b2}", width * height / 1e6)),
                        num("Thickness", "", if panel.kind == PanelKind::Empty { 0.0 } else { round(panel.thickness_mm) }, false),
                        num("Offset", "", round(panel.offset_mm), false),
                    ]),
                    group("Materials and Finishes", vec![ro("Material", if panel.kind == PanelKind::Empty { "<none>".to_string() } else { panel.material.clone() })]),
                    group("Identity Data", vec![ro("Category", "Curtain Panels"), ro("Cells", cells.len().to_string())]),
                ],
            }))
        }
        CurtainRef::Mullion { vertical, k, s } => {
            let run = lay.run_of(vertical, k, s).ok_or("the mullion is gone")?;
            let last = lay.lines(vertical).len() - 1;
            let m = spec.mullion_on(vertical, k, last).clone().ok_or("the mullion is gone")?;
            let title = match m.shape {
                MullionShape::Rectangular { width1_mm, width2_mm, thickness_mm } => format!("Rectangular Mullion: {} x {}", round(width1_mm + width2_mm), round(thickness_mm)),
                MullionShape::Circular { radius_mm } => format!("Circular Mullion: {} mm radius", round(radius_mm)),
            };
            Ok(json!({
                "id": id, "kind": "Mullion", "wall": wall, "pinned": run.pinned,
                "title": title, "type_label": title, "target": target,
                "groups": [
                    group("Constraints", vec![boolean("Pinned", "pinned", run.pinned, lay.typed(vertical)[k])]),
                    group("Dimensions", vec![num("Length", "", round(run.hi - run.lo), false)]),
                    group("Construction", vec![
                        ro("Orientation", if vertical { "Vertical" } else { "Horizontal" }),
                        ro("Position", if k == 0 || k == last { "Border" } else { "Interior" }),
                        num("Offset", "", round(m.offset_mm), false),
                    ]),
                    group("Materials and Finishes", vec![ro("Material", m.material.clone())]),
                    group("Identity Data", vec![ro("Category", "Curtain Wall Mullions")]),
                ],
            }))
        }
    }
}
