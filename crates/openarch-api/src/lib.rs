//! JSON command interface between the UI and the Rust core.
//!
//! The desktop shell and the dev server both call `Session::handle`, so the UI
//! behaves the same in the app and in a browser during development.

use std::collections::BTreeMap;
use std::path::PathBuf;

use openarch_model::view::*;
use openarch_model::edit::{EditOptions, Xform};
use openarch_model::tempdim::{DimEdit, Pins, TempDimSettings};
use openarch_model::dimension::{DimKind, DimRef, DimWant};
use openarch_model::spot::{SpotKind, SpotSurface};
use openarch_model::curve::{self, Curve};
use openarch_model::sketch;
use openarch_model::{SlopeArrow, SpanDirection};
use openarch_model::{mesh, symbol, Document, Element, EndJoin, FamilyCategory, Id, JoinChange, JoinCleanup, JoinKind, LocationLine, Point, Wall};

mod graphics;
mod props;
use graphics::{layers_hint, style_for, Hint, LayeredWall};
use openarch_params::{Column, ColumnKind, Project, Row, Schedule, Value as PValue};
use serde_json::{json, Value};

pub struct Session {
    pub doc: Document,
    pub path: Option<PathBuf>,
    saved_revision: u64,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

type Res = Result<Value, String>;

fn ids_of(p: &Value, key: &str) -> Result<Vec<Id>, String> {
    let a = p.get(key).and_then(|v| v.as_array()).ok_or_else(|| format!("missing {key}"))?;
    Ok(a.iter().filter_map(|v| v.as_u64()).collect())
}

/// Ids the plan uses for the project base point and the survey point, which are not elements of
/// their own (both live in the project location). Below 2^53 so they survive JSON.
pub const SITE_BASE_ID: u64 = 4_503_599_627_370_001;
pub const SITE_SURVEY_ID: u64 = 4_503_599_627_370_002;

/// Read a family file (.oaf; the older .ibfam files are the same JSON).
fn read_family_file(path: &std::path::Path) -> Result<openarch_model::faminst::FamilyFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{} is not a family file: {e}", path.display()))
}

fn id_of(p: &Value, key: &str) -> Result<Id, String> {
    p.get(key).and_then(|v| v.as_u64()).ok_or_else(|| format!("missing {key}"))
}

fn num_of(p: &Value, key: &str) -> Result<f64, String> {
    p.get(key).and_then(|v| v.as_f64()).ok_or_else(|| format!("missing {key}"))
}

/// A wall can run up to a level (`top_level_id`, with `top_offset_mm`) instead of a fixed height.
fn set_top(w: &mut Wall, p: &Value) -> Result<(), String> {
    if let Some(t) = p.get("top_level_id").filter(|v| !v.is_null()) {
        w.top_level = Some(t.as_u64().ok_or("top_level_id must be an id")?);
        w.top_offset_mm = p.get("top_offset_mm").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }
    Ok(())
}

fn point_of(v: &Value) -> Result<Point, String> {
    let a = v.as_array().filter(|a| a.len() == 2).ok_or("point must be [x, y]")?;
    Ok(Point::new(a[0].as_f64().ok_or("bad x")?, a[1].as_f64().ok_or("bad y")?))
}

/// Section points arrive as [x, y]; the model stores {x, y}.
fn section_patch_points(patch: &mut Value) -> Result<(), String> {
    for key in ["start", "end"] {
        if let Some(v) = patch.get(key).cloned() {
            let pt = point_of(&v)?;
            patch[key] = json!({ "x": pt.x, "y": pt.y });
        }
    }
    if let Some(bends) = patch.get("bends").and_then(|v| v.as_array()).cloned() {
        let pts: Result<Vec<Value>, String> = bends.iter().map(|b| point_of(b).map(|p| json!({ "x": p.x, "y": p.y }))).collect();
        patch["bends"] = Value::Array(pts?);
    }
    Ok(())
}

/// An annotation family as a view draws it: its body and pointer as paths in paper mm.
fn symbol_json(d: &Document, id: Id, f: &openarch_model::annotation::AnnotationFamily) -> Value {
    let dr = d.symbol_drawing(f, false);
    json!({ "id": id, "body": dr.body, "pointer": dr.pointer })
}

/// A section as the plan draws it: the corners of its line, its runs and jogs (for their grips, counted from the
/// head), the view box, the break in this plan if there is one, and its marker.
fn section_json(d: &Document, id: Id, s: &openarch_model::section::Section, plan: Id) -> Value {
    let (right, look) = s.axes().unwrap_or((Point::new(1.0, 0.0), Point::new(0.0, 1.0)));
    let dr = s.drawn();
    let runs: Vec<Value> = dr.as_ref().map_or(vec![], |dr| dr.runs.iter().map(|&(s0, s1, dd)| { let (a, b) = (dr.at(s0, dd), dr.at(s1, dd)); json!({ "a": [a.x, a.y], "b": [b.x, b.y] }) }).collect());
    let jogs: Vec<Value> = dr.as_ref().map_or(vec![], |dr| dr.runs.windows(2).map(|w| { let (a, b) = (dr.at(w[0].1, w[0].2), dr.at(w[1].0, w[1].2)); json!({ "a": [a.x, a.y], "b": [b.x, b.y] }) }).collect());
    // the view box: the left end of the view, across it and how deep (a reference section looks at nothing)
    let depth = s.view.and_then(|v| d.elevation_frame(v)).map(|f| f.depth_mm).filter(|v| *v < 1.0e6);
    let frame = s.view_runs().map(|(o, r)| json!({ "o": [o.x, o.y], "right": [right.x, right.y], "look": [look.x, look.y], "width": r.last().map_or(0.0, |r| r.u1), "depth": depth }));
    let family = d.section_marker_family(s.family);
    json!({
        "id": id, "view_id": s.reference.or(s.view), "name": s.reference.or(s.view).and_then(|v| d.view(v)).map(|v| v.name.clone()),
        "reference": s.reference.is_some(),
        "start": [s.start.x, s.start.y], "end": [s.end.x, s.end.y], "flipped": s.flipped, "look": [look.x, look.y],
        "pts": s.points().iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(), "runs": runs, "jogs": jogs, "frame": frame,
        "path_length": s.path_length(),
        "gap": s.break_in(plan).map(|b| json!({ "head_mm": b.head_mm, "tail_mm": b.tail_mm })),
        "broken_style": family.map(|(_, f)| f.broken_style),
        "pinned": s.pinned, "level_id": s.level, "far_clip": s.far_clip, "depth": depth,
        "family": family.map(|(fid, f)| symbol_json(d, fid, f)),
        "labels": d.section_labels(s),
    })
}

fn end_of(p: &Value) -> Result<u8, String> {
    match p.get("end").and_then(|v| v.as_u64()) {
        Some(e @ (0 | 1)) => Ok(e as u8),
        _ => Err("end must be 0 (start) or 1 (end)".into()),
    }
}

fn kind_name(k: JoinKind) -> &'static str {
    match k {
        JoinKind::Butt => "butt",
        JoinKind::Miter => "miter",
        JoinKind::SquareOff => "square_off",
    }
}

fn cleanup_name(c: JoinCleanup) -> &'static str {
    match c {
        JoinCleanup::UseView => "use_view",
        JoinCleanup::Clean => "clean",
        JoinCleanup::DontClean => "dont_clean",
    }
}

fn pt(p: &Point) -> Value {
    json!([p.x, p.y])
}

const LOCATION_LINES: [(&str, LocationLine, &str); 6] = [
    ("wall_centreline", LocationLine::WallCentreline, "Wall Centerline"),
    ("core_centreline", LocationLine::CoreCentreline, "Core Centerline"),
    ("finish_face_exterior", LocationLine::FinishFaceExterior, "Finish Face: Exterior"),
    ("finish_face_interior", LocationLine::FinishFaceInterior, "Finish Face: Interior"),
    ("core_face_exterior", LocationLine::CoreFaceExterior, "Core Face: Exterior"),
    ("core_face_interior", LocationLine::CoreFaceInterior, "Core Face: Interior"),
];

impl Session {
    pub fn new() -> Self {
        let doc = Document::new_project();
        Session { saved_revision: doc.revision(), doc, path: None }
    }

    pub fn handle(&mut self, method: &str, p: &Value) -> Res {
        match method {
            "doc_info" => Ok(self.doc_info()),
            "standards" => graphics::standards(&self.doc),
            "patch_object_styles" => graphics::patch_object_styles(&mut self.doc, p),
            "patch_line_weights" => graphics::patch_line_weights(&mut self.doc, p),
            "add_style_item" => graphics::add_style_item(&mut self.doc, p),
            "patch_style_item" => graphics::patch_style_item(&mut self.doc, p),
            "delete_style_item" => graphics::delete_style_item(&mut self.doc, p),
            "plan" => self.plan(p),
            "scene" => Ok(self.scene(p)),
            "patch_wall" => {
                self.doc.patch_wall(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "patch_view" => {
                self.doc.patch_view(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "patch_view_template" => {
                self.doc.patch_view_template(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "duplicate_view" => Ok(json!({ "id": self.doc.duplicate_view(id_of(p, "id")?)? })),
            "create_plan_views" => {
                let levels: Vec<Id> = p.get("levels").and_then(|v| v.as_array()).ok_or("missing levels")?.iter().filter_map(|v| v.as_u64()).collect();
                let skip = p.get("skip_existing").and_then(|v| v.as_bool()).unwrap_or(true);
                let scale = p.get("scale").and_then(|v| v.as_u64()).map(|s| s as u32);
                Ok(json!({ "ids": self.doc.create_plan_views(id_of(p, "view_type")?, &levels, skip, scale)? }))
            }
            "add_floor_plan" => {
                let name = p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?;
                Ok(json!({ "id": self.doc.add_floor_plan(id_of(p, "level_id")?, name)? }))
            }
            "create_view_template" => {
                let name = p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?;
                Ok(json!({ "id": self.doc.create_view_template(name, id_of(p, "view_id")?)? }))
            }
            "view_template" => self.view_template_info(id_of(p, "id")?),
            "view_graphics" => self.view_graphics(id_of(p, "view_id")?),
            "view_range" => self.view_range(id_of(p, "view_id")?),
            "hide_elements" => {
                let vid = id_of(p, "view_id")?;
                let ids: Vec<Id> = p.get("ids").and_then(|v| v.as_array()).ok_or("missing ids")?.iter().filter_map(|v| v.as_u64()).collect();
                let hide = p.get("hide").and_then(|v| v.as_bool()).unwrap_or(true);
                let mut set = self.doc.view(vid).ok_or("not a view")?.hidden_elements.clone();
                for i in ids {
                    if hide { set.insert(i); } else { set.remove(&i); }
                }
                self.doc.patch_view(vid, &json!({ "hidden_elements": set }))?;
                Ok(json!({}))
            }
            "describe" => self.describe(id_of(p, "id")?),
            "elevation" => self.elevation_json(id_of(p, "view_id")?),
            "add_level" => {
                let name = p.get("name").and_then(|v| v.as_str());
                let elev = num_of(p, "elevation")?;
                // plans are made when wanted (View > Plan Views), unless the Level tool's Make Plan View asks
                let make_plan = p.get("make_plan").and_then(|v| v.as_bool()).unwrap_or(false);
                Ok(json!({ "id": self.doc.add_level(name, elev, make_plan)? }))
            }
            "patch_level" => {
                self.doc.patch_level(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "delete_level" => {
                self.doc.delete_level(id_of(p, "id")?)?;
                Ok(json!({}))
            }
            "add_elevation_marker" => {
                let slots: Vec<bool> = p.get("slots").and_then(|v| v.as_array()).map(|a| a.iter().map(|v| v.as_bool().unwrap_or(false)).collect()).unwrap_or_else(|| vec![true, false, false, false]);
                let mut s4 = [false; 4];
                for (k, v) in slots.iter().take(4).enumerate() {
                    s4[k] = *v;
                }
                let rot = p.get("rotation_deg").and_then(|v| v.as_f64()).unwrap_or(0.0);
                Ok(json!({ "id": self.doc.add_elevation_marker(id_of(p, "level_id")?, point_of(p.get("point").ok_or("missing point")?)?, rot, s4)? }))
            }
            "add_section" => {
                // { level_id, start: [x, y], end: [x, y], flipped, reference: view id (a reference section, no view of its own) }
                let flipped = p.get("flipped").and_then(|v| v.as_bool()).unwrap_or(false);
                let (level, start, end) = (id_of(p, "level_id")?, point_of(p.get("start").ok_or("missing start")?)?, point_of(p.get("end").ok_or("missing end")?)?);
                let id = match p.get("reference").and_then(|v| v.as_u64()) {
                    Some(target) => self.doc.add_reference_section(level, start, end, flipped, target)?,
                    None => self.doc.add_section(level, start, end, flipped)?,
                };
                Ok(json!({ "id": id, "view_id": self.doc.section(id).and_then(|s| s.view) }))
            }
            "patch_section" => {
                // patch: any of start, end ([x, y]), bends ([[x, y], ...]), flipped, far_clip, far_clip_mm, z_range, pinned, family,
                // fold_lines, reference (the view a reference section points at)
                let mut patch = p.get("patch").cloned().ok_or("missing patch")?;
                section_patch_points(&mut patch)?;
                self.doc.patch_section(id_of(p, "id")?, &patch)?;
                Ok(json!({}))
            }
            "section_edit" => {
                // { id, op: "split" (at, to) | "move_run" (index, to) | "move_jog" (index, to), preview } -> { pts }
                use openarch_model::section::SectionEdit;
                let to = point_of(p.get("to").ok_or("missing to")?)?;
                let index = || p.get("index").and_then(|v| v.as_u64()).map(|v| v as usize).ok_or("missing index");
                let edit = match p.get("op").and_then(|v| v.as_str()) {
                    Some("split") => SectionEdit::Split { at: point_of(p.get("at").ok_or("missing at")?)?, to },
                    Some("move_run") => SectionEdit::MoveRun { index: index()?, to },
                    Some("move_jog") => SectionEdit::MoveJog { index: index()?, to },
                    _ => return Err("op must be split, move_run or move_jog".into()),
                };
                let preview = p.get("preview").and_then(|v| v.as_bool()).unwrap_or(false);
                let pts = self.doc.edit_section(id_of(p, "id")?, edit, preview)?;
                Ok(json!({ "pts": pts.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>() }))
            }
            "section_break" => {
                // { id, view_id } breaks the line in that plan or joins it again; { id, view_id, head_mm, tail_mm } sets the ends
                let (id, view) = (id_of(p, "id")?, id_of(p, "view_id")?);
                match (p.get("head_mm").and_then(|v| v.as_f64()), p.get("tail_mm").and_then(|v| v.as_f64())) {
                    (Some(h), Some(t)) => {
                        self.doc.set_section_break(id, view, h, t)?;
                        Ok(json!({ "broken": true }))
                    }
                    _ => Ok(json!({ "broken": self.doc.toggle_section_break(id, view)? })),
                }
            }
            "reference_targets" => Ok(json!(self.doc.reference_targets().iter().map(|(id, v)| json!({ "id": id, "name": v.name, "view_kind": v.view_kind })).collect::<Vec<_>>())),
            "patch_marker" => {
                // { rotation_deg, pinned, point, slot_on: { "0": true } }
                let id = id_of(p, "id")?;
                let patch = p.get("patch").ok_or("missing patch")?;
                let m = self.doc.elevation_marker(id).ok_or("not an elevation marker")?.clone();
                self.doc.grouped("Change elevation marker", |d| {
                    if let Some(on) = patch.get("slot_on").and_then(|v| v.as_object()) {
                        for (k, v) in on {
                            let slot: usize = k.parse().map_err(|_| "bad pointer".to_string())?;
                            d.set_marker_slot(id, slot, v.as_bool().unwrap_or(false))?;
                        }
                    }
                    if d.elevation_marker(id).is_none() {
                        return Ok(());
                    }
                    if let Some(pin) = patch.get("pinned").and_then(|v| v.as_bool()) {
                        d.set_pinned(&[id], pin)?;
                    }
                    let rot = patch.get("rotation_deg").and_then(|v| v.as_f64()).unwrap_or(m.rotation_deg);
                    let pos = match patch.get("point") {
                        Some(v) => point_of(v)?,
                        None => m.position,
                    };
                    if let Some(f) = patch.get("family") {
                        let fam = f.as_u64();
                        if fam.is_some_and(|f| !d.annotation_family(f).is_some_and(|x| x.kind == openarch_model::annotation::AnnotationKind::ElevationMarker)) {
                            return Err("the marker needs an elevation marker family".into());
                        }
                        d.set_marker_family(id, fam)?;
                    }
                    d.set_marker_pose(id, pos, rot)
                })?;
                Ok(json!({}))
            }
            "move_site_point" => {
                let which = match p.get("which").and_then(|v| v.as_str()) {
                    Some("base") => openarch_model::site::SitePoint::Base,
                    Some("survey") => openarch_model::site::SitePoint::Survey,
                    _ => return Err("which is base or survey".into()),
                };
                let to = point_of(p.get("point").ok_or("missing point")?)?;
                let (_, loc) = self.doc.project_location().ok_or("the project has no location")?;
                let z = match which {
                    openarch_model::site::SitePoint::Base => loc.base.z,
                    openarch_model::site::SitePoint::Survey => loc.survey.z,
                };
                self.doc.move_site_point(which, openarch_model::site::P3 { x: to.x, y: to.y, z })?;
                Ok(json!({}))
            }
            "add_grid" => {
                // { curves: [Curve JSON], type_id?, name? }: a line, an arc or a chain of them
                let curves: Vec<Curve> = serde_json::from_value(p.get("curves").cloned().ok_or("missing curves")?).map_err(|e| format!("bad curves: {e}"))?;
                let t = p.get("type_id").and_then(|v| v.as_u64());
                Ok(json!({ "id": self.doc.add_grid(curves, t, p.get("name").and_then(|v| v.as_str()))? }))
            }
            "patch_grid" => {
                self.doc.patch_grid(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "patch_grid_type" => {
                self.doc.patch_grid_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "duplicate_grid_type" => Ok(json!({ "id": self.doc.duplicate_grid_type(id_of(p, "id")?, p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?)? })),
            "describe_grid_type" => {
                let id = id_of(p, "id")?;
                Ok(props::grid_type_props(&self.doc, id, self.doc.grid_type(id).ok_or("not a grid type")?))
            }
            "set_grid_end" => {
                self.doc.set_grid_end(id_of(p, "id")?, num_of(p, "end")? as usize, point_of(p.get("point").ok_or("missing point")?)?)?;
                Ok(json!({}))
            }
            // alignment pins on the ends of grids and levels: one undo step each
            "set_level_end" => {
                let (id, end) = (id_of(p, "id")?, num_of(p, "end")? as usize);
                let to = point_of(p.get("point").ok_or("missing point")?)?;
                // a level that fits the view has no extent yet; the view's own ends give it one
                let fallback = match p.get("extent").and_then(|v| v.as_array()) {
                    Some(a) if a.len() == 2 => Some([point_of(&a[0])?, point_of(&a[1])?]),
                    _ => None,
                };
                self.doc.grouped("Move aligned ends", |d| {
                    if let (Some(ext), Some(l)) = (fallback, d.level(id)) {
                        if l.extent.is_none() {
                            d.patch_level(id, &json!({ "extent": ext }))?;
                        }
                    }
                    d.move_datum_end(id, end, to)
                })?;
                Ok(json!({}))
            }
            "lock_ends" | "unlock_ends" => {
                let end_ref = |k: &str| -> Result<openarch_model::datum::EndRef, String> {
                    let v = p.get(k).ok_or(format!("missing {k}"))?;
                    Ok(openarch_model::datum::EndRef { id: id_of(v, "id")?, end: num_of(v, "end")? as usize })
                };
                let (a, b) = (end_ref("a")?, end_ref("b")?);
                if method == "lock_ends" {
                    self.doc.lock_datum_ends(a, b)?;
                } else {
                    self.doc.unlock_datum_ends(a, b)?;
                }
                Ok(json!({}))
            }
            "grid_from_line" => Ok(json!({ "id": self.doc.grid_from_line(id_of(p, "source")?, p.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0), point_of(p.get("point").ok_or("missing point")?)?)? })),
            "patch_project_location" => {
                self.doc.patch_project_location_ui(p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "describe_multi" => props::multi_props(&self.doc, &ids_of(p, "ids")?),
            "patch_multi" => {
                // one patch to every selected element, as one undo step
                let (ids, patch) = (ids_of(p, "ids")?, p.get("patch").ok_or("missing patch")?);
                self.doc.grouped("Change properties", |d| {
                    for id in ids {
                        match d.get(id) {
                            Some(Element::Wall(_)) => d.patch_wall(id, patch)?,
                            Some(Element::FamilyInstance(_)) => d.patch_instance(id, patch)?,
                            Some(Element::Grid(_)) => d.patch_grid(id, patch)?,
                            Some(Element::Roof(_)) => d.patch_roof(id, patch)?,
                            Some(Element::Floor(_)) => d.patch_floor(id, patch)?,
                            _ => return Err("those properties cannot be changed for this kind of element".into()),
                        }
                    }
                    Ok(())
                })?;
                Ok(json!({}))
            }
            "set_type_multi" => {
                // give every selected wall, floor or door/window the type
                let (ids, type_id) = (ids_of(p, "ids")?, id_of(p, "type_id")?);
                self.doc.grouped("Change type", |d| {
                    for id in ids {
                        match d.get(id) {
                            Some(Element::Wall(_)) => d.set_wall_type(id, type_id)?,
                            Some(Element::Floor(_)) => d.set_floor_type(id, type_id)?,
                            Some(Element::Roof(_)) => d.set_roof_type(id, type_id)?,
                            Some(Element::FamilyInstance(_)) => d.patch_instance(id, &json!({ "type_id": type_id }))?,
                            Some(Element::Grid(_)) => d.patch_grid(id, &json!({ "grid_type": type_id }))?,
                            _ => return Err("not an element with a type".into()),
                        }
                    }
                    Ok(())
                })?;
                Ok(json!({}))
            }
            "set_floor_type" => {
                self.doc.set_floor_type(id_of(p, "id")?, id_of(p, "type_id")?)?;
                Ok(json!({}))
            }
            "describe_type" => self.describe_type(id_of(p, "id")?),
            "describe_any_type" => self.describe_any_type(id_of(p, "id")?),
            "patch_floor" => {
                self.doc.patch_floor(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "patch_system_type" => {
                self.doc.patch_system_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "duplicate_system_type" => Ok(json!({ "id": self.doc.duplicate_system_type(id_of(p, "id")?, p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?)? })),
            "rename_type" => {
                self.doc.rename_type(id_of(p, "id")?, p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?)?;
                Ok(json!({}))
            }
            "assembly_info" => self.assembly_info(p),
            // Type Properties edits the model as it goes so it can show what follows; Apply and OK keep
            // the edits as one undo step, Cancel drops everything since the last Apply
            "edit_begin" => Ok(json!({ "mark": self.doc.edit_mark() })),
            "edit_commit" => {
                let mark = p.get("mark").and_then(|v| v.as_u64()).ok_or("missing mark")? as usize;
                self.doc.fold_since(mark, p.get("name").and_then(|v| v.as_str()).unwrap_or("Type Properties"));
                Ok(json!({ "mark": self.doc.edit_mark() }))
            }
            "edit_cancel" => {
                let mark = p.get("mark").and_then(|v| v.as_u64()).ok_or("missing mark")? as usize;
                self.doc.roll_back_to(mark);
                Ok(json!({}))
            }
            "place_free_instance" => {
                let pt = point_of(p.get("point").ok_or("missing point")?)?;
                let rot = p.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let off = p.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
                Ok(json!({ "id": self.doc.place_free(id_of(p, "type_id")?, id_of(p, "level_id")?, pt, rot, off)? }))
            }
            "type_plan_preview" => {
                // what a type looks like in plan, in its own axes, for the placing ghost: placed once in a copy
                let tid = id_of(p, "type_id")?;
                let detail: openarch_model::view::DetailLevel = serde_json::from_value(p.get("detail").cloned().unwrap_or(json!("medium"))).unwrap_or_default();
                let mut d2 = self.doc.clone();
                let lvl = d2.levels().first().map(|(id, _)| *id).ok_or("no level")?;
                let id = d2.place_free(tid, lvl, Point::new(0.0, 0.0), 0.0, 0.0)?;
                let sym = symbol::plan_symbol_at(&d2, id, detail, None).ok_or("nothing to draw in plan")?;
                let pt = |q: &Point| [q.x, q.y];
                Ok(json!({
                    "parts": sym.parts.iter().map(|p| json!({ "sub": p.subcategory, "rings": p.rings.iter().map(|r| r.iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>(), "closed": p.closed, "filled": p.filled })).collect::<Vec<_>>(),
                    "footprint": sym.footprint.iter().map(pt).collect::<Vec<_>>(),
                }))
            }
            "place_instance" => {
                let flag = |k: &str| p.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
                let id = self.doc.place_on_wall_flipped(id_of(p, "type_id")?, id_of(p, "host_id")?, point_of(p.get("point").ok_or("missing point")?)?, flag("flip_hand"), flag("flip_facing"))?;
                Ok(json!({ "id": id, "warning": self.doc.frame_fit(id) }))
            }
            "add_catalog_type" => {
                let name = p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?;
                Ok(json!({ "id": self.doc.add_catalog_type(id_of(p, "family_id")?, name)? }))
            }
            "patch_instance" => {
                self.doc.patch_instance(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "patch_family_type" => {
                self.doc.patch_family_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "new_family" => {
                let name = p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?;
                let category = serde_json::from_value(p.get("category").cloned().unwrap_or(json!("generic_models"))).map_err(|e| format!("bad category: {e}"))?;
                let host = serde_json::from_value(p.get("host").cloned().unwrap_or(json!("none"))).map_err(|e| format!("bad host: {e}"))?;
                Ok(json!({ "id": self.doc.new_family(name, category, host)? }))
            }
            "family_def" => {
                let id = id_of(p, "id")?;
                let f = self.doc.family(id).ok_or("no such family")?;
                let mut body = f.body.clone();
                body.fix_dim_ids();
                let types: Vec<Value> = self.doc.family_types().into_iter().filter(|(_, t)| t.family == id).map(|(tid, t)| json!({ "id": tid, "name": t.name, "values": t.values })).collect();
                Ok(json!({ "id": id, "name": f.name, "category": f.category, "category_label": f.category.label(), "host": f.host, "built_in": f.built_in, "source_path": f.source_path, "loaded_at": f.loaded_at, "params": f.params, "body": body, "settings": f.settings, "types": types,
                    "categories": openarch_model::family::FamilyCategory::all().iter().map(|c| json!({ "id": c, "label": c.label(), "discipline": c.discipline() })).collect::<Vec<_>>() }))
            }
            "set_family_def" => {
                let id = id_of(p, "id")?;
                let params = match p.get("params").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad params: {e}"))?),
                    None => None,
                };
                let body = match p.get("body").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad body: {e}"))?),
                    None => None,
                };
                let settings: Option<openarch_model::family::FamilySettings> = match p.get("settings").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad settings: {e}"))?),
                    None => None,
                };
                let renames: Vec<(String, String)> = match p.get("renames").filter(|v| !v.is_null()) {
                    Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad renames: {e}"))?,
                    None => vec![],
                };
                self.doc.set_family_def_full(id, params, body, settings, &renames)?;
                Ok(json!({}))
            }
            "family_rename_param" => {
                // stateless: the editor's parameters, body and type values in, the renamed ones back
                use openarch_model::famgeom::FamilyBody;
                let mut params: Vec<openarch_model::family::ParamDef> = serde_json::from_value(p.get("params").cloned().ok_or("missing params")?).map_err(|e| format!("bad params: {e}"))?;
                let mut body: FamilyBody = serde_json::from_value(p.get("body").cloned().ok_or("missing body")?).map_err(|e| format!("bad body: {e}"))?;
                let old = p.get("old").and_then(|v| v.as_str()).ok_or("missing old")?;
                let new = p.get("new").and_then(|v| v.as_str()).ok_or("missing new")?;
                openarch_model::faminst::rename_family_param(&mut params, &mut body, old, new.trim())?;
                Ok(json!({ "params": params, "body": body }))
            }
            "family_param_uses" => {
                use openarch_model::famgeom::FamilyBody;
                let params: Vec<openarch_model::family::ParamDef> = serde_json::from_value(p.get("params").cloned().ok_or("missing params")?).map_err(|e| format!("bad params: {e}"))?;
                let body: FamilyBody = serde_json::from_value(p.get("body").cloned().ok_or("missing body")?).map_err(|e| format!("bad body: {e}"))?;
                let name = p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?;
                let mut uses = body.param_users(name);
                for q in params.iter().filter(|q| q.name != name) {
                    if q.formula.as_deref().map_or(false, |f| openarch_model::famgeom::expr_names(f).iter().any(|n| n == name)) {
                        uses.push(format!("the formula of '{}'", q.name));
                    }
                }
                Ok(json!({ "uses": uses }))
            }
            "family_param_values" => {
                let id = id_of(p, "id")?;
                let tid = p.get("type").and_then(|v| v.as_u64());
                let over: std::collections::BTreeMap<String, openarch_model::family::ParamValue> = match p.get("values").filter(|v| !v.is_null()) {
                    Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad values: {e}"))?,
                    None => Default::default(),
                };
                let mut d2 = self.doc.clone();
                let body = match p.get("body").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value::<openarch_model::famgeom::FamilyBody>(v.clone()).map_err(|e| format!("bad body: {e}"))?),
                    None => None,
                };
                let params: Option<Vec<openarch_model::family::ParamDef>> = match p.get("params").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad params: {e}"))?),
                    None => None,
                };
                if params.is_some() || body.is_some() {
                    d2.set_family_def(id, params, body)?;
                }
                let vals = d2.family_param_values(id, tid, &over)?;
                Ok(json!({ "values": vals.into_iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>() }))
            }
            "family_preview" => {
                let id = id_of(p, "id")?;
                let tid = p.get("type").and_then(|v| v.as_u64());
                let over: std::collections::BTreeMap<String, openarch_model::family::ParamValue> = match p.get("values").filter(|v| !v.is_null()) {
                    Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad values: {e}"))?,
                    None => Default::default(),
                };
                let body = match p.get("body").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value::<openarch_model::famgeom::FamilyBody>(v.clone()).map_err(|e| format!("bad body: {e}"))?),
                    None => None,
                };
                let params: Option<Vec<openarch_model::family::ParamDef>> = match p.get("params").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad params: {e}"))?),
                    None => None,
                };
                // an unsaved body (and unsaved parameters) in the editor are solved as they stand
                let solved = match (body, params) {
                    (None, None) => self.doc.family_preview(id, tid, &over),
                    (body, params) => {
                        let mut d2 = self.doc.clone();
                        d2.set_family_def(id, params, body)?;
                        d2.family_preview(id, tid, &over)
                    }
                };
                Ok(family_preview_json(solved))
            }
            "family_components" => {
                // the families that can be placed inside the one being edited: the project's other
                // families, minus any that already hold this one (that would nest a family in itself)
                let id = id_of(p, "id")?;
                let me = self.doc.family(id).ok_or("no such family")?.name.clone();
                let all = self.doc.families();
                let holds = |start: &str| -> bool {
                    let mut todo = vec![start.to_string()];
                    let mut seen: Vec<String> = Vec::new();
                    while let Some(n) = todo.pop() {
                        if n == me {
                            return true;
                        }
                        if seen.contains(&n) {
                            continue;
                        }
                        if let Some((_, f)) = all.iter().find(|(_, f)| f.name == n) {
                            todo.extend(f.body.nested.iter().map(|x| x.family.clone()));
                        }
                        seen.push(n);
                    }
                    false
                };
                let mut out = Vec::new();
                for (fid, f) in &all {
                    if *fid == id || holds(&f.name) {
                        continue;
                    }
                    let mut types: Vec<String> = self.doc.family_types().into_iter().filter(|(_, t)| t.family == *fid).map(|(_, t)| t.name.clone()).collect();
                    types.sort();
                    out.push(json!({
                        "name": f.name, "category": f.category, "host": f.host, "built_in": f.built_in, "types": types,
                        "params": f.params.iter().filter(|q| !q.reporting).map(|q| json!({ "name": q.name, "kind": q.param_kind, "instance": q.instance, "group": q.group })).collect::<Vec<_>>(),
                    }));
                }
                Ok(json!({ "families": out }))
            }
            "family_edit" => {
                // edits that depend on where the planes stand: dimensions and plane moves. The body
                // and parameters come from the editor as they stand, unsaved; the new body goes back.
                use openarch_model::famgeom::{DimPatch, FamilyBody, SymView};
                let id = id_of(p, "id")?;
                let tid = p.get("type").and_then(|v| v.as_u64());
                let over: std::collections::BTreeMap<String, openarch_model::family::ParamValue> = match p.get("values").filter(|v| !v.is_null()) {
                    Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad values: {e}"))?,
                    None => Default::default(),
                };
                let body: FamilyBody = match p.get("body").filter(|v| !v.is_null()) {
                    Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad body: {e}"))?,
                    None => self.doc.family(id).ok_or("no such family")?.body.clone(),
                };
                let params: Option<Vec<openarch_model::family::ParamDef>> = match p.get("params").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad params: {e}"))?),
                    None => None,
                };
                let mut d2 = self.doc.clone();
                if params.is_some() {
                    d2.set_family_def(id, params, None)?;
                }
                let op = p.get("op").and_then(|v| v.as_str()).ok_or("missing op")?.to_string();
                let num = |k: &str| p.get(k).and_then(|v| v.as_f64()).ok_or(format!("missing {k}"));
                let mut made: Option<u32> = None;
                let body = d2.family_body_edit(id, tid, &over, body, |b, env| match op.as_str() {
                    "add_dim" => {
                        let refs: Vec<u32> = serde_json::from_value(p.get("refs").cloned().ok_or("missing refs")?).map_err(|e| format!("bad refs: {e}"))?;
                        let view: Option<SymView> = match p.get("view").filter(|v| !v.is_null()) {
                            Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad view: {e}"))?),
                            None => None,
                        };
                        let id = b.add_dim(&refs, view, env)?;
                        if let (Some(at), Some(d)) = (p.get("at").and_then(|v| v.as_f64()), b.dims.iter_mut().find(|d| d.id == id)) {
                            d.at = Some(at);
                        }
                        made = Some(id);
                        Ok(())
                    }
                    "patch_dim" => {
                        let patch: DimPatch = serde_json::from_value(p.get("patch").cloned().ok_or("missing patch")?).map_err(|e| format!("bad patch: {e}"))?;
                        b.patch_dim(num("dim")? as u32, &patch, env)
                    }
                    "remove_dim" => b.remove_dim(num("dim")? as u32, env),
                    "dim_value" => b.set_dim_value(num("dim")? as u32, p.get("seg").and_then(|v| v.as_u64()).unwrap_or(0) as usize, num("value")?, env),
                    "move_plane" => b.move_plane(num("plane")? as u32, num("to")?, env),
                    "remove_plane" => b.remove_plane(num("plane")? as u32),
                    other => Err(format!("unknown family edit '{other}'")),
                })?;
                Ok(json!({ "body": body, "dim": made }))
            }
            "export_family" => {
                let f = self.doc.export_family(id_of(p, "id")?)?;
                Ok(serde_json::to_value(f).map_err(|e| e.to_string())?)
            }
            "duplicate_family" => {
                let name = p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?.to_string();
                Ok(json!({ "id": self.doc.duplicate_family(id_of(p, "id")?, &name)? }))
            }
            "import_family" => {
                let f = serde_json::from_value(p.get("file").cloned().ok_or("missing file")?).map_err(|e| format!("not a family file: {e}"))?;
                Ok(json!({ "id": self.doc.import_family(f)? }))
            }
            // ---- family files (.oaf): the file and the project copy are separate; nothing stays linked ----
            "shared_params" => {
                // one call for every operation on a shared parameter file: it is read, changed and written back each time
                use openarch_model::sharedparams::{decode, SharedParamFile};
                let mut path = PathBuf::from(p.get("path").and_then(|v| v.as_str()).ok_or("missing path")?);
                let op = p.get("op").and_then(|v| v.as_str()).unwrap_or("read");
                let text = |k: &str| p.get(k).and_then(|v| v.as_str());
                let mut file = if op == "create" {
                    if path.extension().is_none() {
                        path.set_extension("txt");
                    }
                    if path.exists() {
                        return Err(format!("{} already exists", path.display()));
                    }
                    SharedParamFile::default()
                } else {
                    let bytes = std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
                    SharedParamFile::parse(&decode(&bytes)?)?
                };
                let gid = || p.get("group").and_then(|v| v.as_u64()).map(|g| g as u32).ok_or("missing group");
                let mut made: Option<Value> = None;
                match op {
                    "read" | "create" => {}
                    "add_group" => made = Some(json!(file.add_group(text("name").ok_or("missing name")?)?)),
                    "rename_group" => file.rename_group(gid()?, text("name").ok_or("missing name")?)?,
                    "remove_group" => file.remove_group(gid()?)?,
                    "add_param" => {
                        let datatype = match (text("datatype"), p.get("kind")) {
                            (Some(d), _) => d.to_string(),
                            (None, Some(k)) => openarch_model::sharedparams::datatype_of(serde_json::from_value(k.clone()).map_err(|_| "unknown kind")?).to_string(),
                            _ => return Err("missing type".into()),
                        };
                        made = Some(json!(file.add_param(gid()?, text("name").ok_or("missing name")?, &datatype, text("description").unwrap_or(""))?));
                    }
                    "edit_param" => file.edit_param(text("guid").ok_or("missing guid")?, text("name"), p.get("group").and_then(|v| v.as_u64()).map(|g| g as u32), text("description"))?,
                    "remove_param" => file.remove_param(text("guid").ok_or("missing guid")?)?,
                    other => return Err(format!("unknown shared parameter operation '{other}'")),
                }
                if op != "read" {
                    std::fs::write(&path, file.to_text()).map_err(|e| format!("could not write {}: {e}", path.display()))?;
                }
                Ok(json!({
                    "path": path.to_string_lossy(),
                    "made": made,
                    "groups": file.groups,
                    "params": file.params.iter().map(|q| json!({ "guid": q.guid, "name": q.name, "datatype": q.datatype, "kind": q.kind(), "group": q.group, "description": q.description, "visible": q.visible, "user_modifiable": q.user_modifiable })).collect::<Vec<_>>(),
                }))
            }
            "read_lookup_csv" => {
                // a lookup table from a CSV file; the editor adds it to the family's body
                let path = PathBuf::from(p.get("path").and_then(|v| v.as_str()).ok_or("missing path")?);
                let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
                let name = match p.get("name").and_then(|v| v.as_str()).filter(|n| !n.trim().is_empty()) {
                    Some(n) => n.to_string(),
                    None => path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                };
                let table = openarch_params::lookup::LookupTable::from_csv(&name, &text)?;
                Ok(json!({ "table": table }))
            }
            "write_lookup_csv" => {
                let mut path = PathBuf::from(p.get("path").and_then(|v| v.as_str()).ok_or("missing path")?);
                if path.extension().is_none() {
                    path.set_extension("csv");
                }
                let table: openarch_params::lookup::LookupTable = serde_json::from_value(p.get("table").cloned().ok_or("missing table")?).map_err(|e| format!("bad table: {e}"))?;
                std::fs::write(&path, table.to_csv()).map_err(|e| format!("could not write {}: {e}", path.display()))?;
                Ok(json!({ "path": path.to_string_lossy() }))
            }
            "save_family_file" => {
                let id = id_of(p, "id")?;
                let mut path = PathBuf::from(p.get("path").and_then(|v| v.as_str()).ok_or("missing path")?);
                if path.extension().is_none() {
                    path.set_extension("oaf");
                }
                let params = match p.get("params").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad params: {e}"))?),
                    None => None,
                };
                let body = match p.get("body").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad body: {e}"))?),
                    None => None,
                };
                let settings = match p.get("settings").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad settings: {e}"))?),
                    None => None,
                };
                let values: std::collections::BTreeMap<Id, std::collections::BTreeMap<String, openarch_model::ParamValue>> = match p.get("values").filter(|v| !v.is_null()) {
                    Some(v) => {
                        let m: std::collections::BTreeMap<String, std::collections::BTreeMap<String, openarch_model::ParamValue>> = serde_json::from_value(v.clone()).map_err(|e| format!("bad values: {e}"))?;
                        m.into_iter().map(|(k, v)| k.parse::<Id>().map(|k| (k, v)).map_err(|_| format!("bad type id '{k}'"))).collect::<Result<_, _>>()?
                    }
                    None => Default::default(),
                };
                let file = self.doc.export_family_with(id, params, body, settings, &values)?;
                let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
                // write beside the target first so a failed write never leaves half a family file
                let tmp = path.with_extension("oaf.tmp");
                std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                let at = p.get("loaded_at").and_then(|v| v.as_str()).map(String::from);
                self.doc.set_family_source(id, Some(path.to_string_lossy().into_owned()), at)?;
                Ok(json!({ "path": path.to_string_lossy() }))
            }
            "family_file_info" => {
                let path = PathBuf::from(p.get("path").and_then(|v| v.as_str()).ok_or("missing path")?);
                let file = read_family_file(&path)?;
                let exists = self.doc.families().into_iter().find(|(_, f)| f.name == file.family.name).map(|(id, _)| id);
                Ok(json!({
                    "name": file.family.name, "category": file.family.category, "category_label": file.family.category.label(),
                    "types": file.types.iter().map(|t| t.name.clone()).collect::<Vec<_>>(), "exists": exists,
                }))
            }
            "import_family_file" => {
                let path = PathBuf::from(p.get("path").and_then(|v| v.as_str()).ok_or("missing path")?);
                let mode = match p.get("mode").filter(|v| !v.is_null()) {
                    Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad mode: {e}"))?,
                    None => openarch_model::faminst::ImportMode::Refuse,
                };
                let file = read_family_file(&path)?;
                let at = p.get("loaded_at").and_then(|v| v.as_str()).map(String::from);
                let (id, replaced) = self.doc.import_family_with(file, mode, Some((path.to_string_lossy().into_owned(), at)))?;
                Ok(json!({ "id": id, "replaced": replaced }))
            }
            "family_file_status" => {
                let f = self.doc.family(id_of(p, "id")?).ok_or("no such family")?;
                let found = f.source_path.as_ref().map_or(false, |s| std::path::Path::new(s).is_file());
                Ok(json!({ "source_path": f.source_path, "found": found, "loaded_at": f.loaded_at }))
            }
            "delete_family_type" => {
                self.doc.delete_family_type(id_of(p, "id")?)?;
                Ok(json!({}))
            }
            "set_family_meta" => {
                let category = match p.get("category").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad category: {e}"))?),
                    None => None,
                };
                let host = match p.get("host").filter(|v| !v.is_null()) {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad host: {e}"))?),
                    None => None,
                };
                self.doc.set_family_meta(id_of(p, "id")?, category, host)?;
                Ok(json!({}))
            }
            "duplicate_family_type" => {
                let name = p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?;
                Ok(json!({ "id": self.doc.duplicate_family_type(id_of(p, "id")?, name)? }))
            }
            "add_wall" => {
                // a curved wall gives its curve; the ends are the curve's ends
                let curve: Option<Curve> = match p.get("curve").filter(|c| !c.is_null()) {
                    Some(c) => Some(serde_json::from_value(c.clone()).map_err(|e| format!("bad curve: {e}"))?),
                    None => None,
                };
                let (start, end) = match &curve {
                    Some(c) => (c.start(), c.end()),
                    None => (point_of(p.get("start").ok_or("missing start")?)?, point_of(p.get("end").ok_or("missing end")?)?),
                };
                let mut w = Wall::new(id_of(p, "type_id")?, id_of(p, "level_id")?, start, end, p.get("height_mm").and_then(|v| v.as_f64()).unwrap_or(2700.0));
                w.curve = curve;
                set_top(&mut w, p)?;
                w.flipped = p.get("flipped").and_then(|v| v.as_bool()).unwrap_or(false);
                if let Some(ll) = p.get("location_line") {
                    w.location_line = serde_json::from_value(ll.clone()).map_err(|_| "unknown location line".to_string())?;
                }
                Ok(json!({ "id": self.doc.add_wall_with(w)? }))
            }
            "move_wall_end" => {
                let end = p.get("end").and_then(|v| v.as_u64()).ok_or("missing end")?;
                if end > 1 {
                    return Err("end is 0 (start) or 1 (end)".into());
                }
                self.doc.move_wall_end(id_of(p, "id")?, end as u8, point_of(p.get("point").ok_or("missing point")?)?)?;
                Ok(json!({}))
            }
            "move_wall_ends" => {
                let ends: Vec<(Id, u8)> = p.get("ends").and_then(|v| v.as_array()).ok_or("missing ends")?.iter().map(|e| Ok((id_of(e, "id")?, e.get("end").and_then(|v| v.as_u64()).ok_or("missing end")? as u8))).collect::<Result<_, String>>()?;
                self.doc.move_wall_ends(&ends, point_of(p.get("point").ok_or("missing point")?)?)?;
                Ok(json!({}))
            }
            "reshape_wall" => {
                // drag point `k` of a curved wall (see `Document::wall_grips`); with `preview` only the new location line comes back
                let (id, k) = (id_of(p, "id")?, p.get("k").and_then(|v| v.as_u64()).ok_or("missing k")? as usize);
                let to = point_of(p.get("point").ok_or("missing point")?)?;
                if p.get("preview").and_then(|v| v.as_bool()).unwrap_or(false) {
                    let nw = self.doc.reshaped_wall(id, k, to)?;
                    let line: Vec<[f64; 2]> = nw.path().pts.iter().map(|q| [q.x, q.y]).collect();
                    return Ok(json!({ "poly": line }));
                }
                self.doc.reshape_wall(id, k, to)?;
                Ok(json!({}))
            }
            "add_walls" => {
                // a run of walls drawn as one shape (rectangle, polygon): one undo step
                let (type_id, level) = (id_of(p, "type_id")?, id_of(p, "level_id")?);
                let height = p.get("height_mm").and_then(|v| v.as_f64()).unwrap_or(2700.0);
                let flipped = p.get("flipped").and_then(|v| v.as_bool()).unwrap_or(false);
                let line = match p.get("location_line") {
                    Some(ll) => Some(serde_json::from_value(ll.clone()).map_err(|_| "unknown location line".to_string())?),
                    None => None,
                };
                let mut walls = vec![];
                for s in p.get("segments").and_then(|v| v.as_array()).ok_or("missing segments")? {
                    // a segment is two points (a straight wall) or { "curve": ... } (an arc, ellipse or spline)
                    let mut ws: Vec<Wall> = vec![];
                    if let Some(a) = s.as_array() {
                        if a.len() != 2 {
                            return Err("a segment is two points".into());
                        }
                        ws.push(Wall::new(type_id, level, point_of(&a[0])?, point_of(&a[1])?, height));
                    } else {
                        let c: Curve = serde_json::from_value(s.get("curve").ok_or("a segment is two points or a curve")?.clone()).map_err(|e| format!("bad curve: {e}"))?;
                        // a closed shape goes round in two walls
                        let parts: Vec<Curve> = if c.is_closed() { c.halves().ok_or("that closed curve cannot be a wall: draw it open")?.to_vec() } else { vec![c] };
                        for c in parts {
                            let mut w = Wall::new(type_id, level, c.start(), c.end(), height);
                            w.curve = Some(c);
                            ws.push(w);
                        }
                    }
                    for mut w in ws {
                        set_top(&mut w, p)?;
                        w.flipped = flipped;
                        if let Some(l) = line {
                            w.location_line = l;
                        }
                        walls.push(w);
                    }
                }
                Ok(json!({ "ids": self.doc.add_walls_with(walls)? }))
            }
            "add_floor" => {
                let pts = p
                    .get("points")
                    .and_then(|v| v.as_array())
                    .ok_or("missing points")?
                    .iter()
                    .map(point_of)
                    .collect::<Result<Vec<_>, _>>()?;
                let id = self.doc.add_floor(id_of(p, "type_id")?, id_of(p, "level_id")?, pts)?;
                Ok(json!({ "id": id }))
            }
            "delete" => {
                let ids: Vec<Id> = p
                    .get("ids")
                    .and_then(|v| v.as_array())
                    .ok_or("missing ids")?
                    .iter()
                    .filter_map(|v| v.as_u64())
                    .collect();
                if ids.iter().any(|i| *i == SITE_BASE_ID || *i == SITE_SURVEY_ID) {
                    return Err("the project base point and survey point cannot be deleted".into());
                }
                self.doc.delete_items(&ids)?;
                Ok(json!({}))
            }
            "set_wall_type" => {
                self.doc.set_wall_type(id_of(p, "id")?, id_of(p, "type_id")?)?;
                Ok(json!({}))
            }
            "transform" => {
                // Move, Copy, Rotate and Mirror: { ids, kind, copy, disjoin, dx, dy | cx, cy, angle_deg | a, b }
                let ids: Vec<Id> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
                let x = match p.get("kind").and_then(|v| v.as_str()) {
                    Some("move") => Xform::Move { dx: num_of(p, "dx")?, dy: num_of(p, "dy")? },
                    Some("rotate") => Xform::Rotate { cx: num_of(p, "cx")?, cy: num_of(p, "cy")?, angle_deg: num_of(p, "angle_deg")? },
                    Some("mirror") => {
                        let (a, b) = (point_of(p.get("a").ok_or("missing a")?)?, point_of(p.get("b").ok_or("missing b")?)?);
                        Xform::Mirror { ax: a.x, ay: a.y, bx: b.x, by: b.y }
                    }
                    _ => return Err("kind must be move, rotate or mirror".into()),
                };
                let flag = |k: &str| p.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
                // elevation markers and the base and survey points move and turn with the rest
                let is_site = |i: &Id| *i == SITE_BASE_ID || *i == SITE_SURVEY_ID;
                let is_section = |i: &Id| self.doc.section(*i).is_some();
                let (marks, rest): (Vec<Id>, Vec<Id>) = ids.iter().copied().partition(|i| is_site(i) || is_section(i) || self.doc.elevation_marker(*i).is_some());
                if marks.is_empty() {
                    let out = self.doc.transform_elements(&ids, x, EditOptions { copy: flag("copy"), disjoin: flag("disjoin") })?;
                    return Ok(json!({ "ids": out }));
                }
                let (copy, mirror) = (flag("copy"), matches!(x, Xform::Mirror { .. }));
                if marks.iter().any(|m| !is_section(m)) {
                    if copy {
                        return Err("elevation markers and the project points cannot be copied".into());
                    }
                    if mirror {
                        return Err("elevation markers and the project points cannot be mirrored".into());
                    }
                }
                let mut made: Vec<Id> = vec![];
                let out = self.doc.grouped(if copy { "Copy" } else { "Move" }, |d| {
                    for &m in &marks {
                        if let Some(sec) = d.section(m).cloned() {
                            // a mirrored section keeps looking at the same side of the model
                            let mut n = sec.clone();
                            n.start = x.apply(sec.start);
                            n.end = x.apply(sec.end);
                            n.bends = sec.bends.iter().map(|b| x.apply(*b)).collect();
                            n.flipped = sec.flipped != mirror;
                            n.pinned = false;
                            let mut patch = serde_json::to_value(&n).map_err(|e| e.to_string())?;
                            if copy {
                                // a copy of a reference section points at the same view; any other copy makes its own
                                let nid = match sec.reference {
                                    Some(target) => d.add_reference_section(sec.level, n.start, n.end, n.flipped, target)?,
                                    None => d.add_section(sec.level, n.start, n.end, n.flipped)?,
                                };
                                d.patch_section(nid, &patch)?;
                                made.push(nid);
                            } else {
                                patch["pinned"] = json!(sec.pinned);
                                d.patch_section(m, &patch)?;
                            }
                        } else if is_site(&m) {
                            let which = if m == SITE_BASE_ID { openarch_model::site::SitePoint::Base } else { openarch_model::site::SitePoint::Survey };
                            let Some((_, loc)) = d.project_location() else { return Err("the project has no location".to_string()) };
                            let cur = if m == SITE_BASE_ID { loc.base } else { loc.survey };
                            let np = x.apply(Point::new(cur.x, cur.y));
                            d.move_site_point(which, openarch_model::site::P3 { x: np.x, y: np.y, z: cur.z })?;
                        } else if let Some(mk) = d.elevation_marker(m).cloned() {
                            let turn = if let Xform::Rotate { angle_deg, .. } = x { angle_deg } else { 0.0 };
                            d.set_marker_pose(m, x.apply(mk.position), mk.rotation_deg + turn)?;
                        }
                    }
                    if rest.is_empty() { Ok(vec![]) } else { d.transform_elements(&rest, x, EditOptions { copy, disjoin: flag("disjoin") }) }
                })?;
                let mut all: Vec<Id> = if copy { vec![] } else { marks.clone() };
                all.extend(made);
                all.extend(out);
                Ok(json!({ "ids": all }))
            }
            "annotation_family" => {
                let id = id_of(p, "id")?;
                let f = self.doc.annotation_family(id).ok_or("not an annotation family")?;
                let mut v = serde_json::to_value(f).map_err(|e| e.to_string())?;
                v["id"] = json!(id);
                v["params"] = json!(f.kind.params());
                v["kind"] = json!(f.kind);
                v["in_use"] = json!(self.doc.annotation_family_in_use(id));
                v["category"] = json!(f.kind.category());
                v["subcategories"] = json!(self.doc.symbol_subcategories(f.kind));
                v["fill_patterns"] = graphics::fill_pattern_defs(&self.doc);
                v["drawing"] = json!(self.doc.symbol_drawing(f, true));
                v["annotation_widths"] = json!(self.doc.line_weights().map(|(_, w)| w.annotation.clone()).unwrap_or_default());
                Ok(v)
            }
            // the family editor's drawing of a family it is changing, as paths (hidden lines and regions marked)
            "annotation_paths" => {
                let mut f: openarch_model::annotation::AnnotationFamily = serde_json::from_value(p.get("family").cloned().ok_or("missing family")?).map_err(|e| format!("bad family: {e}"))?;
                f.read_old_drawing();
                Ok(json!(self.doc.symbol_drawing(&f, true)))
            }
            // a filled or masking region's sketch: its edges grouped into closed loops
            "annotation_region_loops" => {
                let edges: Vec<openarch_model::annotation::RegionEdge> = serde_json::from_value(p.get("edges").cloned().ok_or("missing edges")?).map_err(|e| format!("bad edges: {e}"))?;
                match openarch_model::annotation::region_loops(&edges) {
                    Ok(loops) => Ok(json!({ "ok": true, "loops": loops })),
                    Err(e) => Ok(json!({ "ok": false, "error": { "message": e.message, "at": e.at } })),
                }
            }
            "set_annotation_family" => {
                let f: openarch_model::annotation::AnnotationFamily = serde_json::from_value(p.get("family").cloned().ok_or("missing family")?).map_err(|e| format!("bad family: {e}"))?;
                self.doc.set_annotation_family(id_of(p, "id")?, f)?;
                Ok(json!({}))
            }
            "duplicate_annotation_family" => Ok(json!({ "id": self.doc.duplicate_annotation_family(id_of(p, "id")?, p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?)? })),
            "delete_annotation_family" => {
                self.doc.delete_annotation_family(id_of(p, "id")?)?;
                Ok(json!({}))
            }
            "patch_level_type" => {
                self.doc.patch_level_type(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "duplicate_level_type" => Ok(json!({ "id": self.doc.duplicate_level_type(id_of(p, "id")?, p.get("name").and_then(|v| v.as_str()).ok_or("missing name")?)? })),
            "describe_level_type" => {
                let id = id_of(p, "id")?;
                Ok(props::level_type_props(&self.doc, id, self.doc.level_type(id).ok_or("not a level type")?))
            }
            "move_vertical" => {
                // elevation views: { ids, dz, copy? }. A copy is made first (sideways by dx, dy when given), then lifted.
                let ids = ids_of(p, "ids")?;
                let dz = num_of(p, "dz").unwrap_or(0.0);
                let (dx, dy) = (p.get("dx").and_then(|v| v.as_f64()).unwrap_or(0.0), p.get("dy").and_then(|v| v.as_f64()).unwrap_or(0.0));
                let copy = p.get("copy").and_then(|v| v.as_bool()).unwrap_or(false);
                // levels only move up and down, grids only sideways
                let is_level = |d: &Document, i: &Id| d.level(*i).is_some();
                let is_grid = |d: &Document, i: &Id| d.grid(*i).is_some();
                if copy && ids.iter().any(|i| is_level(&self.doc, i)) {
                    return Err("levels cannot be copied here".into());
                }
                let out = self.doc.grouped(if copy { "Copy" } else { "Move" }, |d| {
                    let lateral: Vec<Id> = ids.iter().copied().filter(|i| !is_level(d, i)).collect();
                    let mut target = ids.clone();
                    if !lateral.is_empty() && (copy || dx != 0.0 || dy != 0.0) {
                        let moved = d.transform_elements(&lateral, Xform::Move { dx, dy }, EditOptions { copy, disjoin: false })?;
                        if copy {
                            target = moved;
                        }
                    }
                    if dz != 0.0 {
                        let lift: Vec<Id> = target.iter().copied().filter(|i| !is_grid(d, i)).collect();
                        if !lift.is_empty() {
                            d.move_vertical(&lift, dz)?;
                        }
                    }
                    Ok(target)
                })?;
                Ok(json!({ "ids": out }))
            }
            "offset" => {
                let ids: Vec<Id> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
                let out = self.doc.offset_walls(&ids, num_of(p, "dist")?, p.get("copy").and_then(|v| v.as_bool()).unwrap_or(false))?;
                Ok(json!({ "ids": out }))
            }
            "trim" => {
                match p.get("mode").and_then(|v| v.as_str()) {
                    Some("corner") => self.doc.trim_corner(id_of(p, "a")?, point_of(p.get("pick_a").ok_or("missing pick_a")?)?, id_of(p, "b")?, point_of(p.get("pick_b").ok_or("missing pick_b")?)?)?,
                    Some("single") => self.doc.trim_extend(id_of(p, "wall")?, point_of(p.get("pick").ok_or("missing pick")?)?, id_of(p, "boundary")?)?,
                    _ => return Err("mode must be corner or single".into()),
                }
                Ok(json!({}))
            }
            "align" => {
                let ids: Vec<Id> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
                let pair = |k: &str| -> Result<(Point, Point), String> {
                    let a = p.get(k).and_then(|v| v.as_array()).filter(|a| a.len() == 2).ok_or_else(|| format!("{k} must be two points"))?;
                    Ok((point_of(&a[0])?, point_of(&a[1])?))
                };
                let out = self.doc.align_elements(&ids, pair("reference")?, pair("target")?)?;
                Ok(json!({ "ids": out }))
            }
            "temp_dimensions" => {
                // dimensions for one selected element, or for a door or window about to be placed
                let settings: TempDimSettings = p.get("settings").map(|v| serde_json::from_value(v.clone()).unwrap_or_default()).unwrap_or_default();
                let refs: BTreeMap<String, Id> = p.get("refs").and_then(|v| v.as_object()).map(|o| o.iter().filter_map(|(k, v)| v.as_u64().map(|i| (k.clone(), i))).collect()).unwrap_or_default();
                let dims = if let Some(t) = p.get("type_id").and_then(|v| v.as_u64()) {
                    let host = id_of(p, "host_id")?;
                    match self.doc.placement_at(t, host, point_of(p.get("point").ok_or("missing point")?)?) {
                        Some((u, width)) => self.doc.opening_dims(None, host, u, width, settings, &refs, false),
                        None => vec![],
                    }
                } else {
                    let pins: Pins = p.get("pins").map(|v| serde_json::from_value(v.clone()).unwrap_or_default()).unwrap_or_default();
                    self.doc.temp_dimensions_pinned(id_of(p, "id")?, settings, &refs, &pins)
                };
                Ok(json!({ "dims": dims }))
            }
            "temp_dim_lines" => {
                // the reference lines the selected element's dimensions can be dragged to
                let lines = self.doc.dim_lines(id_of(p, "id")?).ok_or("that element has no dimensions")?;
                Ok(serde_json::to_value(lines).map_err(|e| e.to_string())?)
            }
            "set_temp_dimension" => {
                let edit: DimEdit = serde_json::from_value(p.get("edit").cloned().ok_or("missing edit")?).map_err(|e| format!("bad edit: {e}"))?;
                self.doc.apply_temp_dimension(&edit, num_of(p, "current")?, num_of(p, "value")?)?;
                Ok(json!({}))
            }
            // ---- permanent dimensions ----
            "dimension_candidates" => {
                let settings: TempDimSettings = p.get("settings").map(|v| serde_json::from_value(v.clone()).unwrap_or_default()).unwrap_or_default();
                let want = if p.get("want").and_then(|v| v.as_str()) == Some("arc") { DimWant::Arc } else { DimWant::Line };
                let c = self.doc.dim_candidates_for(id_of(p, "view_id")?, point_of(p.get("point").ok_or("missing point")?)?, num_of(p, "tol")?, settings, want);
                Ok(json!({ "candidates": c }))
            }
            "preview_dimension" => {
                // a dimension as it would look, without storing it: new (view_id, kind, refs, line_point) or an existing one with its line moved (id, line_point)
                let line_point = point_of(p.get("line_point").ok_or("missing line_point")?)?;
                let draw = if let Some(id) = p.get("id").and_then(|v| v.as_u64()) {
                    self.doc.preview_dimension_line(id, line_point)?
                } else {
                    let kind: DimKind = serde_json::from_value(p.get("kind").cloned().unwrap_or(json!("aligned"))).map_err(|_| "unknown dimension kind".to_string())?;
                    let refs: Vec<DimRef> = serde_json::from_value(p.get("refs").cloned().ok_or("missing refs")?).map_err(|e| format!("bad refs: {e}"))?;
                    let axis = p.get("axis").filter(|v| !v.is_null()).map(|v| serde_json::from_value::<[f64; 2]>(v.clone())).transpose().map_err(|_| "bad axis".to_string())?;
                    self.doc.preview_dimension(id_of(p, "view_id")?, kind, refs, line_point, axis, p.get("type_id").and_then(|v| v.as_u64()), p.get("reflex").and_then(|v| v.as_bool()).unwrap_or(false))?
                };
                let ty = self.doc.dimension_type(draw.type_id).cloned().unwrap_or_else(openarch_model::dimension::DimensionType::standard);
                let mut v = serde_json::to_value(&draw).map_err(|e| e.to_string())?;
                v["type"] = dimension_type_json(&ty);
                Ok(json!({ "dimension": v }))
            }
            "add_dimension" => {
                let kind: DimKind = serde_json::from_value(p.get("kind").cloned().unwrap_or(json!("aligned"))).map_err(|_| "unknown dimension kind".to_string())?;
                let refs: Vec<DimRef> = serde_json::from_value(p.get("refs").cloned().ok_or("missing refs")?).map_err(|e| format!("bad refs: {e}"))?;
                let axis = p.get("axis").filter(|v| !v.is_null()).map(|v| serde_json::from_value::<[f64; 2]>(v.clone())).transpose().map_err(|_| "bad axis".to_string())?;
                let id = self.doc.add_dimension(id_of(p, "view_id")?, kind, refs, point_of(p.get("line_point").ok_or("missing line_point")?)?, axis, p.get("type_id").and_then(|v| v.as_u64()))?;
                Ok(json!({ "id": id }))
            }
            "patch_dimension" => {
                self.doc.patch_dimension(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "set_dimension_value" => {
                let seg = p.get("segment").and_then(|v| v.as_u64()).ok_or("missing segment")? as usize;
                self.doc.set_dimension_value(id_of(p, "id")?, seg, num_of(p, "value")?)?;
                Ok(json!({}))
            }
            "edit_dimension_refs" => {
                let insert = match p.get("insert").filter(|v| !v.is_null()) {
                    Some(v) => {
                        let at = v.get("index").and_then(|x| x.as_u64()).ok_or("missing index")? as usize;
                        let r: DimRef = serde_json::from_value(v.get("ref").cloned().ok_or("missing ref")?).map_err(|e| format!("bad ref: {e}"))?;
                        Some((at, r))
                    }
                    None => None,
                };
                let remove = p.get("remove").and_then(|v| v.as_u64()).map(|v| v as usize);
                self.doc.edit_dimension_refs(id_of(p, "id")?, insert, remove)?;
                Ok(json!({}))
            }
            "promote_temp_dimension" => {
                let settings: TempDimSettings = p.get("settings").map(|v| serde_json::from_value(v.clone()).unwrap_or_default()).unwrap_or_default();
                let refs: BTreeMap<String, Id> = p.get("refs").and_then(|v| v.as_object()).map(|o| o.iter().filter_map(|(k, v)| v.as_u64().map(|i| (k.clone(), i))).collect()).unwrap_or_default();
                let pins: Pins = p.get("pins").map(|v| serde_json::from_value(v.clone()).unwrap_or_default()).unwrap_or_default();
                let key = p.get("key").and_then(|v| v.as_str()).ok_or("missing key")?;
                let id = self.doc.promote_temp_dimension(id_of(p, "view_id")?, id_of(p, "id")?, key, settings, &refs, &pins, num_of(p, "offset_mm")?)?;
                Ok(json!({ "id": id }))
            }
            // ---- spot elevations, coordinates and slopes ----
            "spot_surfaces" => {
                let hits = self.doc.surfaces_at(id_of(p, "view_id")?, point_of(p.get("point").ok_or("missing point")?)?);
                Ok(json!({ "surfaces": hits }))
            }
            "add_spot" => {
                let (view, kind, target, host, surface, shoulder, leader) = spot_args(p)?;
                let id = self.doc.add_spot(view, kind, target, host, surface, shoulder, leader, p.get("type_id").and_then(|v| v.as_u64()))?;
                Ok(json!({ "id": id }))
            }
            "preview_spot" => {
                let (view, kind, target, host, surface, shoulder, leader) = spot_args(p)?;
                let draw = self.doc.preview_spot(view, kind, target, host, surface, shoulder, leader, p.get("type_id").and_then(|v| v.as_u64()))?;
                let ty = self.doc.spot_type(draw.type_id).cloned().unwrap_or_else(|| openarch_model::spot::SpotType::new("Spot", kind));
                let mut v = serde_json::to_value(&draw).map_err(|e| e.to_string())?;
                v["type"] = spot_type_json(&ty);
                Ok(json!({ "spot": v }))
            }
            "patch_spot" => {
                self.doc.patch_spot(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            // ---- sketch mode (floors now; roofs, ceilings and openings reuse these) ----
            "sketch_make" => sketch_make(p),
            // curves as a sketch session holds them (shape, flattened points, snap points)
            "sketch_describe" => Ok(json!({ "curves": curves_of(p, "curves")?.iter().map(describe_curve).collect::<Vec<_>>() })),
            "sketch_edit" => sketch_edit(p),
            "sketch_check" => {
                let curves = curves_of(p, "curves")?;
                match self.doc.check_floor_sketch(&curves) {
                    Ok(loops) => {
                        let polys: Vec<Vec<[f64; 2]>> = loops.iter().map(|l| curve::loop_polygon(l, 0.5).iter().map(|q| [q.x, q.y]).collect()).collect();
                        Ok(json!({ "ok": true, "loops": loops.len(), "polys": polys }))
                    }
                    Err(message) => {
                        let at = curve::sketch_loops(&curves).err().and_then(|e| e.at);
                        Ok(json!({ "ok": false, "error": { "message": message, "at": at } }))
                    }
                }
            }
            "pick_walls" => {
                let ids: Vec<Id> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
                let offset = p.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let core = p.get("to_core").and_then(|v| v.as_bool()).unwrap_or(false);
                let face = match p.get("face").and_then(|v| v.as_str()) {
                    Some(f) => openarch_model::sketch::PickFace::parse(f).ok_or("face must be exterior, interior, centre or cursor")?,
                    None => openarch_model::sketch::PickFace::default(),
                };
                let curves = self.doc.pick_wall_lines(&ids, point_of(p.get("pick").ok_or("missing pick")?)?, offset, core, face)?;
                Ok(json!({ "curves": curves.iter().map(describe_curve).collect::<Vec<_>>() }))
            }
            "add_floor_sketch" => {
                let (slope, span) = slope_span(p)?;
                let id = self.doc.add_floor_sketch(id_of(p, "type_id")?, id_of(p, "level_id")?, curves_of(p, "curves")?, slope, span)?;
                Ok(json!({ "id": id }))
            }
            "set_floor_sketch" => {
                let (slope, span) = slope_span(p)?;
                self.doc.set_floor_sketch(id_of(p, "id")?, curves_of(p, "curves")?, slope, span)?;
                Ok(json!({}))
            }
            // ---- family form sketches: loops to sketch curves and back (openarch_model::famsketch) ----
            "family_sketch_of" => {
                use openarch_model::famgeom::{Loop, PlaneId};
                let loops: Vec<Loop> = serde_json::from_value(p.get("loops").cloned().ok_or("missing loops")?).map_err(|e| format!("bad loops: {e}"))?;
                let planes: std::collections::BTreeMap<PlaneId, f64> = serde_json::from_value(p.get("planes").cloned().unwrap_or(json!({}))).map_err(|e| format!("bad planes: {e}"))?;
                let (curves, locks, kept) = openarch_model::famsketch::curves_from_loops(&loops, &planes)?;
                Ok(json!({ "curves": curves.iter().map(describe_curve).collect::<Vec<_>>(), "locks": locks, "kept": kept }))
            }
            "family_sketch_loops" => {
                use openarch_model::famgeom::PlaneId;
                use openarch_model::famsketch::{Kept, SketchLock};
                let curves = curves_of(p, "curves")?;
                let locks: Vec<SketchLock> = serde_json::from_value(p.get("locks").cloned().unwrap_or(json!([]))).map_err(|e| format!("bad locks: {e}"))?;
                let kept: Vec<Kept> = serde_json::from_value(p.get("kept").cloned().unwrap_or(json!([]))).map_err(|e| format!("bad kept: {e}"))?;
                let planes: std::collections::BTreeMap<PlaneId, f64> = serde_json::from_value(p.get("planes").cloned().unwrap_or(json!({}))).map_err(|e| format!("bad planes: {e}"))?;
                match openarch_model::famsketch::loops_from_curves(&curves, &locks, &kept, &planes) {
                    Ok(loops) => Ok(json!({ "ok": true, "loops": loops })),
                    Err(e) => Ok(json!({ "ok": false, "error": { "message": e.message, "at": e.at } })),
                }
            }
            "floor_sketch" => {
                let id = id_of(p, "id")?;
                let Some(Element::Floor(f)) = self.doc.get(id) else { return Err("not a floor".into()) };
                let curves: Vec<Curve> = f.loops().into_iter().flatten().collect();
                Ok(json!({
                    "id": id, "level_id": f.level, "type_id": f.floor_type,
                    "curves": curves.iter().map(describe_curve).collect::<Vec<_>>(),
                    "slope": f.slope, "span": f.span,
                }))
            }
            // ---- roofs: the footprint is a sketch with per-edge slope settings ----
            "add_roof_sketch" => {
                let id = self.doc.add_roof_sketch(id_of(p, "type_id")?, id_of(p, "level_id")?, roof_edges_of(p, "edges")?)?;
                Ok(json!({ "id": id }))
            }
            "set_roof_sketch" => {
                self.doc.set_roof_sketch(id_of(p, "id")?, roof_edges_of(p, "edges")?)?;
                Ok(json!({}))
            }
            "roof_sketch" => {
                let id = id_of(p, "id")?;
                let Some(r) = self.doc.roof(id) else { return Err("not a roof".into()) };
                let entries: Vec<Value> = r
                    .edges()
                    .into_iter()
                    .map(|e| {
                        let mut v = describe_curve(&e.curve);
                        v["attr"] = json!({ "defines_slope": e.defines_slope, "slope_deg": e.slope_deg, "overhang_mm": e.overhang_mm });
                        v
                    })
                    .collect();
                Ok(json!({ "id": id, "level_id": r.level, "type_id": r.roof_type, "curves": entries }))
            }
            "patch_roof" => {
                self.doc.patch_roof(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
                Ok(json!({}))
            }
            "set_roof_type" => {
                self.doc.set_roof_type(id_of(p, "id")?, id_of(p, "type_id")?)?;
                Ok(json!({}))
            }
            "pin" => {
                let ids: Vec<Id> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
                let pinned = p.get("pinned").and_then(|v| v.as_bool()).unwrap_or(true);
                self.doc.grouped(if pinned { "Pin" } else { "Unpin" }, |d| {
                    let rest: Vec<Id> = ids.iter().copied().filter(|i| *i != SITE_BASE_ID && *i != SITE_SURVEY_ID).collect();
                    d.set_pinned(&rest, pinned)?;
                    if ids.contains(&SITE_BASE_ID) {
                        d.patch_project_location(&json!({ "base_pinned": pinned }))?;
                    }
                    if ids.contains(&SITE_SURVEY_ID) {
                        d.patch_project_location(&json!({ "survey_pinned": pinned }))?;
                    }
                    Ok(())
                })?;
                Ok(json!({}))
            }
            "flip_walls" => {
                let ids: Vec<Id> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
                self.doc.flip_walls(&ids)?;
                Ok(json!({}))
            }
            "flip_instances" => {
                let ids: Vec<Id> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
                self.doc.flip_instances(&ids)?;
                Ok(json!({}))
            }
            "set_wall_height" => {
                self.doc.set_wall_height(id_of(p, "id")?, num_of(p, "height_mm")?)?;
                Ok(json!({}))
            }
            "set_wall_join" => {
                let kind = match p.get("kind").and_then(|v| v.as_str()) {
                    None => None,
                    Some("butt") => Some(JoinKind::Butt),
                    Some("miter") => Some(JoinKind::Miter),
                    Some("square_off") => Some(JoinKind::SquareOff),
                    Some(o) => return Err(format!("unknown join kind '{o}'")),
                };
                let cleanup = match p.get("cleanup").and_then(|v| v.as_str()) {
                    None => None,
                    Some("use_view") => Some(JoinCleanup::UseView),
                    Some("clean") => Some(JoinCleanup::Clean),
                    Some("dont_clean") => Some(JoinCleanup::DontClean),
                    Some(o) => return Err(format!("unknown cleanup '{o}'")),
                };
                let change = JoinChange {
                    allowed: p.get("allowed").and_then(|v| v.as_bool()),
                    kind,
                    through: p.get("through").and_then(|v| v.as_u64()),
                    cleanup,
                };
                self.doc.set_wall_join(id_of(p, "id")?, end_of(p)?, change)?;
                Ok(json!({}))
            }
            "wall_join_info" => self.wall_join_info(id_of(p, "id")?, end_of(p)?),
            "undo" => Ok(json!({ "done": self.doc.undo() })),
            "redo" => Ok(json!({ "done": self.doc.redo() })),
            "schedule" => Ok(self.schedule(
                p.get("wall_rate").and_then(|v| v.as_f64()).unwrap_or(0.0),
                p.get("floor_rate").and_then(|v| v.as_f64()).unwrap_or(0.0),
            )),
            "new_project" => {
                *self = Session::new();
                Ok(json!({}))
            }
            "save" => {
                let path = match p.get("path").and_then(|v| v.as_str()) {
                    Some(s) => PathBuf::from(s),
                    None => self.path.clone().ok_or("no file name yet")?,
                };
                self.doc.save(&path)?;
                self.path = Some(path.clone());
                self.saved_revision = self.doc.revision();
                Ok(json!({ "path": path.to_string_lossy() }))
            }
            "open" => {
                let path = PathBuf::from(p.get("path").and_then(|v| v.as_str()).ok_or("missing path")?);
                let doc = Document::open(&path)?;
                self.saved_revision = doc.revision();
                self.doc = doc;
                self.path = Some(path);
                Ok(json!({}))
            }
            other => Err(format!("unknown method '{other}'")),
        }
    }

    /// An elevation or section view's drawing. Cut items of a section carry the style their category and material give
    /// them (cut line, cut pattern), and the hatch definitions come with the data.
    fn elevation_json(&self, vid: Id) -> Res {
        let d = &self.doc;
        let data = d.elevation_data(vid)?;
        let mut v = serde_json::to_value(&data).map_err(|e| e.to_string())?;
        let Some(view) = d.view(vid).filter(|x| x.section.is_some()) else { return Ok(v) };
        let settings = d.effective_settings(vid).ok_or("not a view")?;
        let mut items: Vec<Value> = vec![];
        for (it, mut j) in data.items.iter().zip(v["items"].as_array().cloned().unwrap_or_default()) {
            if it.cut {
                let hint = it.material.as_deref().and_then(|m| d.material(m)).map(Hint::from_material).unwrap_or_default();
                // a category switched off in the view leaves its cut out
                let Some(style) = style_for(d, &settings, it.category, None, view.element_overrides.get(&it.id), "cut", &hint) else { continue };
                j["style"] = style;
            }
            items.push(j);
        }
        v["items"] = Value::Array(items);
        v["fill_patterns"] = graphics::fill_pattern_defs(d);
        v["line_patterns"] = graphics::line_pattern_defs(d);
        v["section_id"] = json!(view.section);
        v["section"] = view.section.and_then(|sid| d.section(sid).map(|s| json!({ "id": sid, "flipped": s.flipped, "pinned": s.pinned, "length": s.length(), "far_clip": s.far_clip, "runs": s.view_runs().map(|(_, r)| r) }))).unwrap_or(Value::Null);
        Ok(v)
    }

    fn doc_info(&self) -> Value {
        let d = &self.doc;
        json!({
            "revision": d.revision(),
            "dirty": d.revision() != self.saved_revision,
            "path": self.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            "views": d.views().iter().map(|(id, v)| json!({
                "id": id, "name": v.name, "view_kind": v.view_kind, "level_id": v.level, "template_id": v.template,
                "marker_id": v.elevation.as_ref().map(|e| e.marker), "slot": v.elevation.as_ref().map(|e| e.slot), "section_id": v.section, "view_type_id": v.view_type,
            })).collect::<Vec<_>>(),
            "grid_types": d.grid_types().iter().map(|(id, t)| json!({ "id": id, "name": t.name, "label": t.name })).collect::<Vec<_>>(),
            "view_types": d.view_types().iter().map(|(id, t)| json!({ "id": id, "name": t.name, "view_kind": t.view_kind, "template_for_new": t.template_for_new })).collect::<Vec<_>>(),
            "level_types": d.level_types().iter().map(|(id, t)| json!({ "id": id, "name": t.name, "label": t.name, "head": t.head })).collect::<Vec<_>>(),
            "annotation_families": d.annotation_families().iter().map(|(id, f)| json!({ "id": id, "name": f.name, "label": f.name, "kind": f.kind, "category": f.kind.category(), "in_use": d.annotation_family_in_use(*id) })).collect::<Vec<_>>(),
            "view_templates": d.view_templates().iter().map(|(id, t)| json!({ "id": id, "name": t.name, "view_kind": t.view_kind })).collect::<Vec<_>>(),
            "model_categories": MODEL_CATEGORIES,
            "annotation_categories": ANNOTATION_CATEGORIES,
            "levels": d.levels().iter().map(|(id, l)| json!({ "id": id, "name": l.name, "elevation_mm": l.elevation_mm, "elevation": d.level_display_elevation(l), "building_story": l.building_story })).collect::<Vec<_>>(),
            "site_ids": { "base": SITE_BASE_ID, "survey": SITE_SURVEY_ID },
            "wall_types": d.wall_types().iter().map(|(id, t)| json!({
                "id": id, "name": t.name, "thickness_mm": t.thickness_mm(),
                // reach left/right of each location line, for the plan preview
                "extents": LOCATION_LINES.iter().map(|ll| (ll.0.to_string(), t.extents(ll.1))).collect::<std::collections::BTreeMap<_, _>>(),
            })).collect::<Vec<_>>(),
            "location_lines": LOCATION_LINES.iter().map(|ll| json!({ "value": ll.0, "label": ll.2 })).collect::<Vec<_>>(),
            "families": d.families().iter().map(|(id, f)| json!({
                "id": id, "name": f.name, "category": f.category.label(), "host": f.host, "plan_symbol": f.plan_symbol,
                "built_in": f.built_in, "has_body": !f.body.is_empty(), "source_path": f.source_path,
                "catalog": f.catalog.iter().map(|r| r.name.clone()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "family_types": d.family_types().iter().filter_map(|(id, t)| {
                let f = d.family(t.family)?;
                let num = |n: &str| d.type_param(t, n).and_then(|v| v.as_f64()).unwrap_or(0.0);
                Some(json!({
                    "id": id, "name": t.name, "family_id": t.family, "family": f.name, "category": f.category.label(),
                    "label": format!("{}: {}", f.name, t.name), "width": num("Width"), "height": num("Height"),
                }))
            }).collect::<Vec<_>>(),
            "floor_types": d.floor_types().iter().map(|(id, t)| json!({ "id": id, "name": t.name, "thickness_mm": t.thickness_mm() })).collect::<Vec<_>>(),
            "roof_types": d.roof_types().iter().map(|(id, t)| json!({ "id": id, "name": t.name, "thickness_mm": t.thickness_mm() })).collect::<Vec<_>>(),
            "undo": d.undo_name(),
            "redo": d.redo_name(),
        })
    }

    /// Everything the Wall Joins tool needs for one wall end: the walls in the
    /// join with their settings, the configurations to step through with
    /// Next / Previous, and which one is current.
    fn wall_join_info(&self, id: Id, which: u8) -> Res {
        let d = &self.doc;
        let at = d.walls_at_end(id, which);
        if at.is_empty() {
            return Err("not a wall".into());
        }
        let mut walls = vec![];
        let mut ends: Vec<(Id, EndJoin)> = vec![];
        for (wid, end) in &at {
            if let Some(Element::Wall(w)) = d.get(*wid) {
                let j = if *end == 0 { w.start_join } else { w.end_join };
                ends.push((*wid, j));
                walls.push(json!({
                    "id": wid, "end": end, "allowed": j.allowed, "kind": kind_name(j.kind),
                    "through": j.through, "cleanup": cleanup_name(j.cleanup),
                }));
            }
        }
        let joined: Vec<&(Id, EndJoin)> = ends.iter().filter(|(_, j)| j.allowed).collect();
        let mut options = vec![];
        let mut current = None;
        if joined.len() == 2 {
            let (a, b) = (joined[0], joined[1]);
            let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
            // same rule as the geometry: explicit choice wins, else the older wall runs through
            let through = match (lo.1.through, hi.1.through) {
                (Some(true), _) | (_, Some(false)) => lo.0,
                (_, Some(true)) | (Some(false), _) => hi.0,
                _ => lo.0,
            };
            let kind = lo.1.kind;
            for (k, t) in [
                (JoinKind::Butt, Some(lo.0)),
                (JoinKind::Butt, Some(hi.0)),
                (JoinKind::Miter, None),
                (JoinKind::SquareOff, Some(lo.0)),
                (JoinKind::SquareOff, Some(hi.0)),
            ] {
                if k == kind && (k == JoinKind::Miter || t == Some(through)) {
                    current = Some(options.len());
                }
                options.push(json!({ "kind": kind_name(k), "through": t }));
            }
        }
        // a wall meeting the body of another (T join): always a butt into the host, so the
        // configuration is shown but cannot change (Miter and Square off are greyed out)
        let hosts = d.mid_join_hosts();
        let mid = at.iter().find_map(|(wid, end)| hosts.get(&(*wid, *end)).copied());
        let two = joined.len() == 2 && mid.is_none();
        let cur_kind: Option<String> = if mid.is_some() { Some("butt".into()) } else if two { current.and_then(|c| options[c]["kind"].as_str().map(String::from)) } else { None };
        let kinds: Vec<Value> = ["butt", "miter", "square_off"].iter().map(|k| json!({ "kind": k, "enabled": two || (mid.is_some() && *k == "butt") })).collect();
        Ok(json!({ "walls": walls, "options": options, "current": current,
                   "editable": two, "mid": mid, "kind": cur_kind, "kinds": kinds,
                   "steps": if two && cur_kind.as_deref() != Some("miter") { 2 } else { 0 },
                   "too_complex": at.len() > 4 }))
    }

    fn view_template_info(&self, id: Id) -> Res {
        let t = self.doc.view_template(id).ok_or("not a view template")?;
        Ok(json!({ "id": id, "name": t.name, "view_kind": t.view_kind, "included": t.included, "all_properties": TemplateProp::ALL }))
    }

    /// Data for the Visibility/Graphics dialog: every category with the
    /// override in effect, and whether a view template controls the tab.
    fn view_graphics(&self, view_id: Id) -> Res {
        let d = &self.doc;
        let v = d.view(view_id).ok_or("not a view")?;
        let eff = d.effective_settings(view_id).ok_or("not a view")?;
        let tpl = v.template.and_then(|t| d.view_template(t));
        let inc = |p: TemplateProp| tpl.is_some_and(|t| t.included.contains(&p));
        let list = |names: &[&str], map: &std::collections::BTreeMap<String, CategoryOverride>| {
            let mut all: Vec<String> = names.iter().map(|s| s.to_string()).collect();
            for k in map.keys() {
                if !all.contains(k) {
                    all.push(k.clone());
                }
            }
            all.into_iter()
                .map(|n| {
                    let o = map.get(&n).cloned().unwrap_or_default();
                    json!({ "name": n, "override": o })
                })
                .collect::<Vec<_>>()
        };
        Ok(json!({
            "view_id": view_id, "name": v.name,
            "model": list(MODEL_CATEGORIES, &eff.overrides.model), "model_locked": inc(TemplateProp::VgModel),
            "annotation": list(ANNOTATION_CATEGORIES, &eff.overrides.annotation), "annotation_locked": inc(TemplateProp::VgAnnotation),
            "imported": list(&[], &eff.overrides.imported), "imported_locked": inc(TemplateProp::VgImport),
            "filters": eff.overrides.filters, "filters_locked": inc(TemplateProp::VgFilters),
            "detail_level": eff.detail_level,
        }))
    }

    /// Data for the View Range dialog.
    fn view_range(&self, view_id: Id) -> Res {
        let d = &self.doc;
        let v = d.view(view_id).ok_or("not a view")?;
        let eff = d.effective_settings(view_id).ok_or("not a view")?;
        let r = d.resolve_view_range(v, &eff).ok_or("not a plan view")?;
        let tpl = v.template.and_then(|t| d.view_template(t));
        let finite = |x: f64| if x.is_finite() { json!(x) } else { Value::Null };
        Ok(json!({
            "view_id": view_id, "name": v.name, "range": eff.view_range,
            "locked": tpl.is_some_and(|t| t.included.contains(&TemplateProp::ViewRange)),
            "resolved": { "top": finite(r.top), "cut": r.cut, "bottom": finite(r.bottom), "depth": finite(r.depth) },
        }))
    }

    /// The first plan view of a level (for callers that only know the level).
    fn plan_view_of_level(&self, level: Id) -> Result<Id, String> {
        self.doc
            .views()
            .iter()
            .find(|(_, v)| v.view_kind.is_plan() && v.level == Some(level))
            .map(|(id, _)| *id)
            .ok_or_else(|| "no plan view for that level".to_string())
    }

    /// 2D drawing data for a plan view (millimetres): elements inside the view
    /// range, each classified cut / projection / beyond / underlay, with the
    /// style that the view's graphics overrides give them.
    fn plan(&self, p: &Value) -> Res {
        let d = &self.doc;
        let vid = match p.get("view_id").and_then(|v| v.as_u64()) {
            Some(v) => v,
            None => self.plan_view_of_level(id_of(p, "level_id")?)?,
        };
        let view = d.view(vid).filter(|v| v.view_kind.is_plan()).ok_or("not a plan view")?;
        let settings = d.effective_settings(vid).ok_or("not a view")?;
        let range = d.resolve_view_range(view, &settings).ok_or("view has no level")?;
        let polygons = d.wall_polygons();
        let outlines = d.wall_outlines();

        // underlay: other levels from base (inclusive) up to top (exclusive)
        let level_elev = |l: Id| d.level(l).map_or(0.0, |l| l.elevation_mm);
        let underlay = view.underlay.base_level.map(|b| {
            let lo = level_elev(b);
            let hi = match view.underlay.top_level {
                Some(t) => level_elev(t),
                None => d.levels().iter().map(|(_, l)| l.elevation_mm).filter(|e| *e > lo + 1e-6).fold(f64::INFINITY, f64::min),
            };
            (lo, hi)
        });
        let in_underlay = |l: Id| underlay.is_some_and(|(lo, hi)| view.level != Some(l) && level_elev(l) >= lo - 1e-6 && level_elev(l) < hi - 1e-6);

        let model_hidden = settings.display_model == DisplayModel::DoNotDisplay;
        // walls draw layer by layer above coarse detail (a category override can change it)
        let wall_detail = settings.overrides.model.get("Walls").and_then(|c| c.detail_level).unwrap_or(settings.detail_level);
        let layer_geo = if wall_detail == DetailLevel::Coarse { Default::default() } else { d.wall_layer_geometry() };
        let mut layered: Vec<LayeredWall> = vec![];
        let mut walls = vec![];
        let mut floors = vec![];
        let mut roofs = vec![];
        let mut inserts = vec![];
        for (id, e) in d.iter() {
            if view.hidden_elements.contains(&id) || model_hidden {
                continue;
            }
            match e {
                Element::Wall(w) => {
                    let Some(poly) = polygons.get(&id) else { continue };
                    let (z0, z1) = d.wall_extent(w);
                    let display = match range.classify(z0, z1) {
                        PlanDisplay::Hidden if in_underlay(w.base_level) => "underlay",
                        PlanDisplay::Hidden => continue,
                        PlanDisplay::Cut => "cut",
                        PlanDisplay::Projection => "projection",
                        PlanDisplay::Beyond => "beyond",
                    };
                    let t = d.wall_type(w.wall_type);
                    let hint = t.map(|t| layers_hint(d, &t.layers, &t.coarse_fill, settings.detail_level, true)).unwrap_or_default();
                    let Some(style) = style_for(d, &settings, "Walls", None, view.element_overrides.get(&id), display, &hint) else { continue };
                    // doors and windows break the wall where they span the cut plane
                    let pieces: Vec<Vec<openarch_model::geom::Point>> = match outlines.get(&id) {
                        Some(o) => d.wall_plan_pieces(id, o, (display == "cut").then_some(range.cut)).into_iter().map(|q| q.to_vec()).collect(),
                        None => vec![poly.clone()], // a curved wall draws whole; its layers carry the openings
                    };
                    let layers = layer_geo.get(&id).map(|wl| d.wall_plan_layers(id, wl, (display == "cut").then_some(range.cut)));
                    let is_layered = layers.is_some();
                    // the wall as drawn: the outline of all its layers, which is what selecting it highlights
                    let shape: Vec<Vec<Value>> = layers
                        .as_ref()
                        .map(|ls| openarch_model::layers::union_rings(ls.iter().flat_map(|l| l.shapes.iter()).filter_map(|s| s.first())).into_iter().filter_map(|s| s.into_iter().next()).map(|r| r.iter().map(pt).collect()).collect())
                        .unwrap_or_default();
                    if let Some(layers) = layers {
                        layered.push(LayeredWall {
                            id,
                            display: match display { "cut" => "cut", "projection" => "projection", "beyond" => "beyond", _ => "underlay" },
                            wall_type: w.wall_type,
                            dont_clean: w.start_join.cleanup == JoinCleanup::DontClean || w.end_join.cleanup == JoinCleanup::DontClean,
                            layers,
                        });
                    }
                    // every face across the wall (offset from the location line, left positive), for Align and Mirror
                    let faces: Vec<Value> = {
                        let mut v = vec![json!({ "off": 0.0, "kind": "centre" })];
                        if let Some(wt) = d.wall_type_for(w) {
                            let n = wt.layers.len();
                            let core: Vec<usize> = wt.layers.iter().enumerate().filter(|(_, l)| l.core).map(|(i, _)| i).collect();
                            let mut off = d.wall_extents(w).0;
                            for i in 0..=n {
                                let kind = if i == 0 || i == n { "face" } else if core.first() == Some(&i) || core.last().map(|c| c + 1) == Some(i) { "core" } else { "layer" };
                                v.push(json!({ "off": off, "kind": kind }));
                                if i < n {
                                    off -= wt.layers[i].thickness_mm;
                                }
                            }
                        }
                        v
                    };
                    let extents = d.wall_extents(w);
                    walls.push(json!({
                        "layered": is_layered,
                        "id": id,
                        "corners": poly.iter().map(pt).collect::<Vec<_>>(),
                        "curve": w.curve,
                        "grips": Document::wall_grips(w).iter().map(|q| [q.x, q.y]).collect::<Vec<_>>(),
                        "length": w.length(),
                        "pieces": pieces.iter().map(|q| q.iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>(),
                        "shape": shape,
                        "start": pt(&w.start), "end": pt(&w.end),
                        // how far the wall body reaches left and right of its location line
                        "extents": [extents.0, extents.1],
                        "thickness": t.map_or(100.0, |t| t.thickness_mm()),
                        "type": t.map(|t| t.name.clone()),
                        "display": display, "style": style,
                        "pinned": w.pinned,
                        "faces": faces,
                        "dont_clean": w.start_join.cleanup == JoinCleanup::DontClean || w.end_join.cleanup == JoinCleanup::DontClean,
                    }));
                }
                Element::FamilyInstance(inst) => {
                    let Some((fam, _)) = d.instance_family(inst) else { continue };
                    let door_detail = settings.overrides.model.get(fam.category.label()).and_then(|c| c.detail_level).unwrap_or(settings.detail_level);
                    let Some(frame) = d.instance_frame(id) else { continue };
                    let display = match range.classify_category(fam.category.label(), frame.z0, frame.z1) {
                        PlanDisplay::Hidden if in_underlay(inst.level) => "underlay",
                        PlanDisplay::Hidden => continue,
                        PlanDisplay::Cut => "cut",
                        PlanDisplay::Projection => "projection",
                        PlanDisplay::Beyond => "beyond",
                    };
                    let Some(sym) = symbol::plan_symbol_at(d, id, door_detail, (display == "cut").then_some(range.cut)) else { continue };
                    let cat = fam.category.label();
                    let hint = Hint::default();
                    let eo = view.element_overrides.get(&id);
                    let Some(style) = style_for(d, &settings, cat, None, eo, display, &hint) else { continue };
                    // each part of the symbol draws in its subcategory's style
                    let mut subs = serde_json::Map::new();
                    let part_subs: Vec<String> = sym.parts.iter().map(|p| p.subcategory.clone()).collect();
                    for sub in ["Frame/Mullion", "Plan Swing", "Panel", "Glass"].into_iter().chain(part_subs.iter().map(|x| x.as_str())) {
                        if sub.is_empty() || subs.contains_key(sub) {
                            continue;
                        }
                        if let Some(st) = style_for(d, &settings, cat, Some(sub), eo, display, &hint) {
                            subs.insert(sub.to_string(), st);
                        }
                    }
                    inserts.push(json!({
                        "id": id, "category": cat, "display": display, "style": style, "sub_styles": subs, "mark": inst.mark, "pinned": inst.pinned,
                        "parts": sym.parts.iter().map(|p| json!({ "sub": p.subcategory, "rings": p.rings.iter().map(|r| r.iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>(), "closed": p.closed, "filled": p.filled, "cut": p.cut, "masking": p.masking })).collect::<Vec<_>>(),
                        "glass": sym.glass.iter().map(|l| json!([pt(&l[0]), pt(&l[1])])).collect::<Vec<_>>(),
                        "footprint": sym.footprint.iter().map(pt).collect::<Vec<_>>(),
                        "lines": sym.lines.iter().map(|l| json!([pt(&l[0]), pt(&l[1])])).collect::<Vec<_>>(),
                        "arcs": sym.arcs.iter().map(|a| json!({ "c": pt(&a.centre), "r": a.radius, "from": a.from, "to": a.to })).collect::<Vec<_>>(),
                        "polys": sym.polys.iter().map(|q| q.iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>(),
                        "frame": sym.frame.iter().map(|q| q.iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>(),
                        // at Fine detail the frame is the steel itself, drawn solid
                        "frame_solid": door_detail == DetailLevel::Fine,
                    }));
                }
                Element::Floor(f) => {
                    let top = level_elev(f.level) + f.offset_mm;
                    let thick = d.floor_type(f.floor_type).map_or(0.0, |t| t.thickness_mm());
                    let display = match range.classify(top - thick, top) {
                        PlanDisplay::Hidden if in_underlay(f.level) => "underlay",
                        PlanDisplay::Hidden => continue,
                        PlanDisplay::Cut => "cut",
                        PlanDisplay::Projection => "projection",
                        PlanDisplay::Beyond => "beyond",
                    };
                    let hint = d.floor_type(f.floor_type).map(|t| layers_hint(d, &t.layers, &t.coarse_fill, settings.detail_level, true)).unwrap_or_default();
                    let Some(style) = style_for(d, &settings, "Floors", None, view.element_overrides.get(&id), display, &hint) else { continue };
                    floors.push(json!({ "id": id, "points": f.outline().iter().map(pt).collect::<Vec<_>>(), "rings": f.polygons(1.0).iter().map(|r| r.iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>(), "display": display, "style": style, "pinned": f.pinned }));
                }
                Element::Roof(r) => {
                    let Some(g) = d.roof_geometry(r) else { continue };
                    let rt = d.roof_type(r.roof_type);
                    let thick = rt.map_or(0.0, |t| t.thickness_mm());
                    // from the underside at the eave to the ridge
                    let display = match range.classify(g.base_z - thick * 1.2, g.top_z) {
                        PlanDisplay::Hidden if in_underlay(r.level) => "underlay",
                        PlanDisplay::Hidden => continue,
                        PlanDisplay::Cut => "cut",
                        PlanDisplay::Projection => "projection",
                        PlanDisplay::Beyond => "beyond",
                    };
                    let hint = rt.map(|t| layers_hint(d, &t.layers, &t.coarse_fill, settings.detail_level, true)).unwrap_or_default();
                    let Some(style) = style_for(d, &settings, "Roofs", None, view.element_overrides.get(&id), display, &hint) else { continue };
                    let rings: Vec<Vec<Value>> = g.regions.iter().flat_map(|reg| std::iter::once(&reg.outer).chain(reg.holes.iter())).map(|ring| ring.iter().map(pt).collect()).collect();
                    roofs.push(json!({
                        "id": id, "display": display, "style": style, "pinned": r.pinned,
                        "points": g.regions.first().map(|reg| reg.outer.iter().map(pt).collect::<Vec<_>>()).unwrap_or_default(),
                        "regions": g.regions.iter().map(|reg| json!({ "outer": reg.outer.iter().map(pt).collect::<Vec<_>>(), "holes": reg.holes.iter().map(|h| h.iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>() })).collect::<Vec<_>>(),
                        "rings": rings,
                        "ridges": g.ridges.iter().map(|l| json!([pt(&l[0]), pt(&l[1])])).collect::<Vec<_>>(),
                        "marks": g.marks.iter().map(|m| json!({ "at": pt(&m.at), "up": m.up, "deg": m.deg })).collect::<Vec<_>>(),
                        "top": g.top_z, "base": g.base_z, "flat": !g.flat.is_empty() && g.faces.is_empty(),
                    }));
                }
                _ => {}
            }
        }
        let finite = |v: f64| if v.is_finite() { json!(v) } else { Value::Null };
        // elevation markers and the project base and survey points (annotation and Site categories)
        let ann_shown = |cat: &str| settings.overrides.annotation.get(cat).map_or(true, |o| o.visible);
        let model_shown = |cat: &str| settings.overrides.model.get(cat).map_or(true, |o| o.visible);
        let markers: Vec<Value> = if ann_shown("Elevations") && !model_hidden {
            d.elevation_markers()
                .iter()
                .filter(|(id, _)| !view.hidden_elements.contains(id))
                .map(|(id, m)| json!({
                    "id": id, "x": m.position.x, "y": m.position.y, "rotation_deg": m.rotation_deg, "pinned": m.pinned, "level_id": m.level,
                    "family": d.marker_family(m.family).map(|(fid, f)| symbol_json(d, fid, f)),
                    "slots": (0..4).map(|k| json!({
                        "slot": k, "view_id": m.slots[k], "name": m.slots[k].and_then(|v| d.view(v)).map(|v| v.name.clone()),
                        "look": [m.look(k).x, m.look(k).y],
                    })).collect::<Vec<_>>(),
                }))
                .collect()
        } else {
            vec![]
        };
        let sections: Vec<Value> = if ann_shown("Sections") && !model_hidden {
            let cut_z = view.level.and_then(|l| d.level(l)).map_or(0.0, |l| l.elevation_mm);
            d.sections()
                .iter()
                .filter(|(id, s)| !view.hidden_elements.contains(id) && d.section_shows_at(*id, cut_z) && s.axes().is_some())
                .map(|(id, s)| section_json(d, *id, s, vid))
                .collect()
        } else {
            vec![]
        };
        let grids: Vec<Value> = if ann_shown("Grids") && !model_hidden {
            d.grids()
                .iter()
                .filter(|(id, _)| !view.hidden_elements.contains(id))
                .filter_map(|(id, g)| {
                    let style = style_for(d, &settings, "Grids", None, view.element_overrides.get(id), "normal", &Hint::default())?;
                    let t = d.grid_type(g.grid_type);
                    Some(json!({
                        "id": id, "name": g.name, "path": g.path(1.5).iter().map(pt).collect::<Vec<_>>(),
                        "multi": g.multi_segment(), "bubbles": g.bubbles, "pinned": g.pinned, "style": style, "type_id": g.grid_type,
                        "bubble_dia_mm": t.map_or(8.0, |t| t.bubble_dia_mm),
                        "centre_segment": t.map_or(openarch_model::grid::CentreSegment::None, |t| t.centre_segment),
                        "end_segment_mm": t.map_or(20.0, |t| t.end_segment_mm),
                        "head_offsets": g.head_offsets.map(|o| o.map(|p| pt(&p))), "end_locks": g.end_locks,
                        "is_arc": matches!(g.curves.first(), Some(Curve::Arc { .. })) && g.curves.len() == 1,
                    }))
                })
                .collect()
        } else {
            vec![]
        };
        let dimensions: Vec<Value> = if ann_shown("Dimensions") && !model_hidden {
            d.dimensions_in(vid)
                .into_iter()
                .filter(|(id, _)| !view.hidden_elements.contains(id))
                .filter_map(|(id, dm)| {
                    let style = style_for(d, &settings, "Dimensions", None, view.element_overrides.get(&id), "normal", &Hint::default())?;
                    let draw = d.dimension_draw(id)?;
                    let ty = d.dimension_type(dm.dim_type).cloned().unwrap_or_else(openarch_model::dimension::DimensionType::standard);
                    let mut v = serde_json::to_value(&draw).ok()?;
                    v["style"] = style;
                    v["type"] = dimension_type_json(&ty);
                    Some(v)
                })
                .collect()
        } else {
            vec![]
        };
        let spots: Vec<Value> = if !model_hidden {
            d.spots_in(vid)
                .into_iter()
                .filter(|(id, _)| !view.hidden_elements.contains(id))
                .filter_map(|(id, sp)| {
                    let cat = match sp.kind {
                        SpotKind::Elevation => "Spot Elevations",
                        SpotKind::Coordinate => "Spot Coordinates",
                        SpotKind::Slope => "Spot Slopes",
                    };
                    if !ann_shown(cat) {
                        return None;
                    }
                    let style = style_for(d, &settings, cat, None, view.element_overrides.get(&id), "normal", &Hint::default())?;
                    let draw = d.spot_draw(id)?;
                    let ty = d.spot_type(sp.spot_type).cloned().unwrap_or_else(|| openarch_model::spot::SpotType::new("Spot", sp.kind));
                    let mut v = serde_json::to_value(&draw).ok()?;
                    v["style"] = style;
                    v["type"] = spot_type_json(&ty);
                    v["category"] = json!(cat);
                    Some(v)
                })
                .collect()
        } else {
            vec![]
        };
        // each point has its own category, hidden by default outside the Site plan; a hidden one is null
        let site_points = match d.project_location() {
            Some((_, loc)) if !model_hidden && (model_shown("Project Base Point") || model_shown("Survey Point")) => json!({
                "base": if model_shown("Project Base Point") { json!({ "id": SITE_BASE_ID, "x": loc.base.x, "y": loc.base.y, "pinned": loc.base_pinned }) } else { Value::Null },
                "survey": if model_shown("Survey Point") { json!({ "id": SITE_SURVEY_ID, "x": loc.survey.x, "y": loc.survey.y, "pinned": loc.survey_pinned, "clipped": loc.survey_clipped }) } else { Value::Null },
                "angle_true_north_deg": loc.angle_true_north_deg,
                "true_north": settings.orientation == Orientation::TrueNorth,
            }),
            _ => Value::Null,
        };
        Ok(json!({
            "view": {
                "id": vid, "name": view.name, "level_id": view.level, "scale": settings.scale,
                "detail_level": settings.detail_level, "wall_join_display": settings.wall_join_display,
                "display_model": settings.display_model,
            },
            "range": { "top": finite(range.top), "cut": range.cut, "bottom": finite(range.bottom), "depth": finite(range.depth) },
            "crop": {
                "enabled": view.crop.enabled, "visible": view.crop.region_visible,
                "region": view.crop.region.map(|(a, b)| json!([pt(&a), pt(&b)])),
            },
            "walls": walls, "floors": floors, "roofs": roofs, "inserts": inserts,
            "markers": markers, "marker_family": d.marker_family(None).map(|(fid, f)| symbol_json(d, fid, f)), "site_points": site_points, "sections": sections, "section_family": d.section_marker_family(None).map(|(fid, f)| symbol_json(d, fid, f)), "grids": grids, "dimensions": dimensions, "spots": spots,
            "wall_groups": graphics::wall_groups(d, &settings, view, &layered, settings.wall_join_display == WallJoinDisplay::CleanSameType),
            "line_patterns": graphics::line_pattern_defs(d), "fill_patterns": graphics::fill_pattern_defs(d),
        }))
    }

    /// Display meshes for the 3D view (metres). A view id applies that view's
    /// category visibility and hidden elements.
    fn scene(&self, p: &Value) -> Value {
        let d = &self.doc;
        let view = p
            .get("view_id")
            .and_then(|v| v.as_u64())
            .or_else(|| d.views().iter().find(|(_, v)| v.view_kind == ViewKind::ThreeD).map(|(id, _)| *id));
        let settings = view.and_then(|v| d.effective_settings(v));
        let hidden = view.and_then(|v| d.view(v)).map(|v| v.hidden_elements.clone()).unwrap_or_default();
        let shown = |cat: &str| settings.as_ref().map_or(true, |s| s.overrides.model.get(cat).map_or(true, |o| o.visible));
        let mut meshes = vec![];
        let outlines = d.wall_outlines();
        let layered = d.wall_layer_geometry();
        for (id, e) in d.iter() {
            if hidden.contains(&id) {
                continue;
            }
            if let (Element::Wall(w), Some(wl)) = (e, layered.get(&id)) {
                // layered walls draw one mesh per layer in the material's shading colour
                if shown("Walls") {
                    let parts = mesh::wall_layer_meshes(d, id, w, wl);
                    if !parts.is_empty() {
                        for p in parts {
                            meshes.push(json!({ "id": id, "kind": "Wall", "material": p.material, "color": p.color, "verts": p.verts }));
                        }
                        continue;
                    }
                }
            }
            let (kind, verts) = match e {
                Element::Wall(w) if shown("Walls") => ("Wall", outlines.get(&id).and_then(|o| mesh::wall_mesh(d, w, o, &d.wall_openings(id)))),
                Element::FamilyInstance(inst) => {
                    let Some((cat, frame, glass)) = mesh::instance_meshes(d, id) else { continue };
                    if !shown(cat.label()) {
                        continue;
                    }
                    let _ = inst;
                    if !glass.is_empty() {
                        meshes.push(json!({ "id": id, "kind": "Glass", "verts": glass }));
                    }
                    let kind = match cat {
                        FamilyCategory::Doors => "Door",
                        FamilyCategory::Windows => "Window",
                        _ => "Component",
                    };
                    (kind, Some(frame))
                }
                Element::Floor(f) if shown("Floors") => ("Floor", mesh::floor_mesh(d, f)),
                Element::Roof(r) if shown("Roofs") => ("Roof", mesh::roof_mesh(d, r)),
                _ => continue,
            };
            if let Some(v) = verts {
                meshes.push(json!({ "id": id, "kind": kind, "verts": v }));
            }
        }
        json!({ "revision": d.revision(), "meshes": meshes, "visual_style": settings.as_ref().map(|s| s.visual_style) })
    }

    fn describe(&self, id: Id) -> Res {
        let d = &self.doc;
        if id == SITE_BASE_ID {
            return props::site_point_props(d, openarch_model::site::SitePoint::Base);
        }
        if id == SITE_SURVEY_ID {
            return props::site_point_props(d, openarch_model::site::SitePoint::Survey);
        }
        match d.get(id).ok_or("no such element")? {
            Element::Wall(w) => Ok(props::wall_props(d, id, w)),
            Element::Floor(f) => Ok(props::floor_props(d, id, f)),
            Element::Roof(r) => Ok(props::roof_props(d, id, r)),
            Element::View(v) => Ok(props::view_props(d, id, v)),
            Element::FamilyInstance(i) => props::instance_props(d, id, i),
            Element::Level(l) => Ok(props::level_props(d, id, l)),
            Element::ElevationMarker(m) => Ok(props::marker_props(d, id, m)),
            Element::Section(s) => Ok(props::section_props(d, id, s)),
            Element::Grid(g) => Ok(props::grid_props(d, id, g)),
            Element::Dimension(x) => Ok(props::dimension_props(d, id, x)),
            Element::Spot(x) => Ok(props::spot_props(d, id, x)),
            _ => Err("nothing to show".into()),
        }
    }

    /// Type properties of a family type (the Edit Type dialog).
    fn describe_type(&self, id: Id) -> Res {
        let t = self.doc.family_type(id).ok_or("not a family type")?;
        props::type_props(&self.doc, id, t)
    }

    /// Type Properties of any type, with the commands that change, duplicate and rename it, so one
    /// dialog serves family, system, grid and level types.
    fn describe_any_type(&self, id: Id) -> Res {
        let d = &self.doc;
        let mut v = match d.get(id).ok_or("no such type")? {
            Element::FamilyType(t) => {
                let mut v = props::type_props(d, id, t)?;
                v["patch"] = json!("patch_family_type");
                v["duplicate"] = json!("duplicate_family_type");
                v["family_id"] = json!(t.family);
                v
            }
            Element::WallType(_) | Element::FloorType(_) | Element::RoofType(_) => props::sys_type_props(d, id)?,
            Element::GridType(t) => {
                let mut v = props::grid_type_props(d, id, t);
                v["family"] = json!("Grid");
                v["patch"] = json!("patch_grid_type");
                v["duplicate"] = json!("duplicate_grid_type");
                v
            }
            Element::LevelType(t) => {
                let mut v = props::level_type_props(d, id, t);
                v["family"] = json!("Level");
                v["patch"] = json!("patch_level_type");
                v["duplicate"] = json!("duplicate_level_type");
                v
            }
            _ => return Err("not a type".into()),
        };
        if v.get("name").map_or(true, |n| n.is_null()) {
            v["name"] = v["title"].clone();
        }
        Ok(v)
    }

    /// What Edit Assembly shows for a layer list that is not stored yet: the check OK will make,
    /// the thermal values, and for walls a plan sample (both ends free, an opening in the middle)
    /// with the layers wrapped as the wrapping settings say.
    fn assembly_info(&self, p: &Value) -> Res {
        use openarch_model::element::{default_coarse_fill, Layer, TypeInfo, WallFunction, WallType, Wrap};
        use openarch_model::systypes::{assembly_values, check_assembly, SysKind};
        let kind: SysKind = serde_json::from_value(p.get("kind").cloned().ok_or("missing kind")?).map_err(|e| format!("bad kind: {e}"))?;
        let layers: Vec<Layer> = serde_json::from_value(p.get("layers").cloned().ok_or("missing layers")?).map_err(|e| format!("bad layers: {e}"))?;
        let error = check_assembly(&self.doc, kind, &layers).err();
        let values = assembly_values(&self.doc, &layers);
        let mut plan = Value::Null;
        if kind == SysKind::Wall && error.is_none() {
            let wrap = |k: &str| -> Result<Wrap, String> { serde_json::from_value(p.get(k).cloned().unwrap_or(json!("none"))).map_err(|e| format!("bad {k}: {e}")) };
            let t = WallType {
                name: String::new(),
                layers: layers.clone(),
                wrap_ends: wrap("wrap_ends")?,
                wrap_inserts: wrap("wrap_inserts")?,
                coarse_fill: default_coarse_fill(),
                function: WallFunction::default(),
                info: TypeInfo::default(),
                sample_height_mm: 6000.0,
            };
            let (len, opening) = (1800.0, (600.0, 1200.0));
            let pt = |q: &Point| [q.x, q.y];
            let geo = openarch_model::layers::sample_plan_layers(&t, len, Some(opening));
            plan = json!({
                "length": len, "opening": [opening.0, opening.1], "extents": t.extents(LocationLine::WallCentreline),
                "layers": geo.iter().map(|g| json!({
                    "material": g.material, "is_void": g.is_void, "core": g.core, "function": g.function,
                    "shapes": g.shapes.iter().map(|sh| sh.iter().map(|r| r.iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>()).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            });
        }
        Ok(json!({ "error": error, "values": values, "plan": plan }))
    }

    /// Wall and floor quantities as schedules, with rate columns and totals
    /// calculated by the parameter engine.
    fn schedule(&self, wall_rate: f64, floor_rate: f64) -> Value {
        let d = &self.doc;
        let mut walls = vec![];
        let mut floors = vec![];
        for (id, e) in d.iter() {
            match e {
                Element::Wall(w) => {
                    let (z0, z1) = d.wall_extent(w);
                    let len = Document::wall_length_mm(w) / 1000.0;
                    let h = (z1 - z0) / 1000.0;
                    let mut f = std::collections::BTreeMap::new();
                    f.insert("Type".to_string(), PValue::Text(d.wall_type(w.wall_type).map_or("?".into(), |t| t.name.clone())));
                    f.insert("Level".to_string(), PValue::Text(d.level(w.base_level).map_or("?".into(), |l| l.name.clone())));
                    f.insert("Length".to_string(), PValue::Num((len * 100.0).round() / 100.0));
                    f.insert("Height".to_string(), PValue::Num((h * 100.0).round() / 100.0));
                    // net of the doors and windows cut in the wall
                    let net = len * h - d.wall_opening_area(id) / 1e6;
                    f.insert("Area".to_string(), PValue::Num((net * 100.0).round() / 100.0));
                    walls.push(Row { id: id.to_string(), fields: f });
                }
                Element::Floor(fl) => {
                    let mut f = std::collections::BTreeMap::new();
                    f.insert("Type".to_string(), PValue::Text(d.floor_type(fl.floor_type).map_or("?".into(), |t| t.name.clone())));
                    f.insert("Level".to_string(), PValue::Text(d.level(fl.level).map_or("?".into(), |l| l.name.clone())));
                    let a = fl.area_mm2() / 1e6;
                    f.insert("Area".to_string(), PValue::Num((a * 100.0).round() / 100.0));
                    floors.push(Row { id: id.to_string(), fields: f });
                }
                _ => {}
            }
        }
        let field = |n: &str| Column { name: n.into(), kind: ColumnKind::Field };
        let mut p = Project::new();
        p.set_value("Wall Rate", PValue::Num(wall_rate));
        p.set_value("Floor Rate", PValue::Num(floor_rate));
        p.set_formula("Total Cost", "round([Walls.Cost.Sum] + [Floors.Cost.Sum], 2)");
        p.add_schedule(Schedule {
            name: "Walls".into(),
            columns: vec![
                field("Type"),
                field("Level"),
                field("Length"),
                field("Height"),
                field("Area"),
                Column { name: "Cost".into(), kind: ColumnKind::Calculated("round([Area] * [Wall Rate], 2)".into()) },
            ],
            rows: walls,
        });
        p.add_schedule(Schedule {
            name: "Floors".into(),
            columns: vec![
                field("Type"),
                field("Level"),
                field("Area"),
                Column { name: "Cost".into(), kind: ColumnKind::Calculated("round([Area] * [Floor Rate], 2)".into()) },
            ],
            rows: floors,
        });
        let ev = p.evaluate();
        let show = |r: Option<&Result<PValue, openarch_params::EvalError>>| match r {
            Some(Ok(v)) => v.to_string(),
            Some(Err(e)) => format!("#{e}"),
            None => String::new(),
        };
        let table = |name: &str| {
            let s = p.schedules.iter().find(|s| s.name == name).unwrap();
            json!({
                "name": name,
                "columns": s.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                "calculated": s.columns.iter().map(|c| matches!(c.kind, ColumnKind::Calculated(_))).collect::<Vec<_>>(),
                "rows": (0..s.rows.len()).map(|r| (0..s.columns.len()).map(|c| show(ev.cell(name, r, c))).collect::<Vec<_>>()).collect::<Vec<_>>(),
                "totals": s.columns.iter().map(|c| if c.name == "Area" || c.name == "Cost" { show(ev.sum(name, &c.name)) } else { String::new() }).collect::<Vec<_>>(),
            })
        };
        // door and window schedules: one row per instance
        let inserts = |cat: FamilyCategory| {
            let mut rows: Vec<Vec<String>> = vec![];
            for (id, i) in d.instances() {
                let Some((fam, t)) = d.instance_family(i) else { continue };
                if fam.category != cat {
                    continue;
                }
                let (w, h, sill) = d.instance_opening_size(i);
                rows.push(vec![
                    i.mark.clone(),
                    format!("{}: {}", fam.name, t.name),
                    d.level(i.level).map_or("?".into(), |l| l.name.clone()),
                    format!("{w:.0}"),
                    format!("{h:.0}"),
                    format!("{sill:.0}"),
                    d.type_param(t, "Fire Rating").map(|v| v.display()).unwrap_or_default(),
                    id.to_string(),
                ]);
            }
            rows.sort_by(|a, b| a[0].cmp(&b[0]).then(a[1].cmp(&b[1])));
            let count = rows.len();
            json!({
                "name": cat.label(),
                "columns": ["Mark", "Type", "Level", "Width", "Height", "Sill Height", "Fire Rating"],
                "rows": rows.into_iter().map(|mut r| { r.pop(); r }).collect::<Vec<_>>(),
                "count": count,
            })
        };
        json!({
            "walls": table("Walls"), "floors": table("Floors"), "total_cost": show(ev.global("Total Cost")),
            "doors": inserts(FamilyCategory::Doors), "windows": inserts(FamilyCategory::Windows),
        })
    }
}


// ---- sketch helpers ----------------------------------------------------------------------

/// A curve with what the plan needs to draw and snap to it: its polyline and its snap points.
fn describe_curve(c: &Curve) -> Value {
    let poly = c.tessellate(0.5);
    let mut snaps: Vec<Value> = vec![];
    let mut snap = |p: openarch_model::Point, kind: &str| snaps.push(json!({ "x": p.x, "y": p.y, "kind": kind }));
    if !c.is_closed() {
        snap(c.start(), "endpoint");
        snap(c.end(), "endpoint");
    }
    snap(c.mid(), "midpoint");
    let quadrants = |centre: openarch_model::Point, rx: f64, ry: f64, rot: f64, keep: &dyn Fn(f64) -> bool| -> Vec<openarch_model::Point> {
        (0..4).filter_map(|k| {
            let t = k as f64 * 90.0;
            if !keep(t) { return None; }
            let (s, co) = rot.to_radians().sin_cos();
            let (x, y) = (rx * t.to_radians().cos(), ry * t.to_radians().sin());
            Some(openarch_model::Point::new(centre.x + x * co - y * s, centre.y + x * s + y * co))
        }).collect()
    };
    match c {
        Curve::Circle { centre, radius } => {
            snap(*centre, "center");
            for q in quadrants(*centre, *radius, *radius, 0.0, &|_| true) { snap(q, "quadrant"); }
        }
        Curve::Arc { start, end, centre, ccw } => {
            snap(*centre, "center");
            let r = centre.dist(start);
            let sweep = curve::arc_sweep(*start, *end, *centre, *ccw);
            let a0 = (start.y - centre.y).atan2(start.x - centre.x).to_degrees();
            let inside = |t: f64| { let d = if sweep > 0.0 { (t - a0).rem_euclid(360.0) } else { (a0 - t).rem_euclid(360.0) }; d > 1e-6 && d < sweep.abs().to_degrees() - 1e-6 };
            for q in quadrants(*centre, r, r, 0.0, &inside) { snap(q, "quadrant"); }
        }
        Curve::Ellipse { centre, rx, ry, rotation_deg, t0_deg, t1_deg, .. } => {
            snap(*centre, "center");
            let full = t0_deg.is_none() && t1_deg.is_none();
            for q in quadrants(*centre, *rx, *ry, *rotation_deg, &|_| full) { snap(q, "quadrant"); }
        }
        _ => {}
    }
    json!({ "curve": c, "poly": poly.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>(), "closed": c.is_closed(), "snaps": snaps })
}

fn curves_of(p: &Value, key: &str) -> Result<Vec<Curve>, String> {
    serde_json::from_value(p.get(key).cloned().ok_or_else(|| format!("missing {key}"))?).map_err(|e| format!("bad curves: {e}"))
}

fn roof_edges_of(p: &Value, key: &str) -> Result<Vec<openarch_model::roof::RoofEdge>, String> {
    serde_json::from_value(p.get(key).cloned().ok_or_else(|| format!("missing {key}"))?).map_err(|e| format!("bad roof edges: {e}"))
}

fn curve_of(p: &Value, key: &str) -> Result<Curve, String> {
    serde_json::from_value(p.get(key).cloned().ok_or_else(|| format!("missing {key}"))?).map_err(|e| format!("bad curve: {e}"))
}

fn slope_span(p: &Value) -> Result<(Option<SlopeArrow>, Option<SpanDirection>), String> {
    let slope = match p.get("slope") {
        Some(v) if !v.is_null() => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad slope arrow: {e}"))?),
        _ => None,
    };
    let span = match p.get("span") {
        Some(v) if !v.is_null() => Some(serde_json::from_value(v.clone()).map_err(|e| format!("bad span direction: {e}"))?),
        _ => None,
    };
    Ok((slope, span))
}

fn pts_of(p: &Value, key: &str) -> Result<Vec<Point>, String> {
    p.get(key).and_then(|v| v.as_array()).ok_or_else(|| format!("missing {key}"))?.iter().map(point_of).collect()
}

/// Preview or commit a draw tool: the curves the picks so far make. An incomplete set of picks
/// gives no curves rather than an error, since the plan asks on every mouse move.
fn sketch_make(p: &Value) -> Res {
    let tool = p.get("tool").and_then(|v| v.as_str()).ok_or("missing tool")?;
    let pts = pts_of(p, "points")?;
    let opt = p.get("options").cloned().unwrap_or(json!({}));
    let num = |k: &str, d: f64| opt.get(k).and_then(|v| v.as_f64()).unwrap_or(d);
    let flag = |k: &str, d: bool| opt.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
    let need = |n: usize| if pts.len() >= n { Some(()) } else { None };
    let curves: Option<Vec<Curve>> = match tool {
        "line" => need(2).map(|_| vec![Curve::line(pts[0], pts[1])]),
        "rectangle" => need(2).map(|_| sketch::rectangle(pts[0], pts[1], num("radius", 0.0))),
        "polygon_in" => need(2).map(|_| sketch::polygon_inscribed(pts[0], pts[1], num("sides", 5.0) as usize, num("radius", 0.0))),
        "polygon_out" => need(2).map(|_| sketch::polygon_circumscribed(pts[0], pts[1], num("sides", 5.0) as usize, num("radius", 0.0))),
        "circle" => need(2).and_then(|_| sketch::circle(pts[0], pts[1])).map(|c| vec![c]),
        "arc_3pt" => need(3).and_then(|_| sketch::arc_three_points(pts[0], pts[1], pts[2])).map(|c| vec![c]),
        "arc_ser" => need(3).and_then(|_| sketch::arc_start_end_radius(pts[0], pts[1], num("radius", pts[0].dist(&pts[1]) / 2.0).max(1.0), pts[2])).map(|c| vec![c]),
        "arc_centre" => need(3).and_then(|_| sketch::arc_centre_ends(pts[0], pts[1], pts[2], flag("ccw", true))).map(|c| vec![c]),
        "arc_tangent" => need(3).and_then(|_| sketch::arc_tangent(pts[0], Point::new(pts[1].x - pts[0].x, pts[1].y - pts[0].y), pts[2])).map(|c| vec![c]),
        "ellipse" => need(3).and_then(|_| sketch::ellipse(pts[0], pts[1], pts[2])).map(|c| vec![c]),
        "partial_ellipse" => need(5).and_then(|_| sketch::partial_ellipse(pts[0], pts[1], pts[2], pts[3], pts[4], flag("ccw", true))).map(|c| vec![c]),
        "spline" => sketch::spline(&pts, flag("closed", false)).map(|c| vec![c]),
        // straight lines through the points; closed joins the last point back to the first
        "polyline" => need(2).map(|_| {
            let mut v: Vec<Curve> = pts.windows(2).filter(|w| w[0].dist(&w[1]) > 1e-6).map(|w| Curve::line(w[0], w[1])).collect();
            if flag("closed", false) && pts.len() >= 3 && pts[0].dist(&pts[pts.len() - 1]) > 1e-6 {
                v.push(Curve::line(pts[pts.len() - 1], pts[0]));
            }
            v
        }),
        _ => return Err(format!("unknown sketch tool '{tool}'")),
    };
    let curves = curves.unwrap_or_default();
    Ok(json!({ "curves": curves.iter().map(describe_curve).collect::<Vec<_>>() }))
}

/// Modify tools inside a sketch: they take curves and return the curves that replace them.
fn sketch_edit(p: &Value) -> Res {
    let op = p.get("op").and_then(|v| v.as_str()).ok_or("missing op")?;
    let curves: Vec<Curve> = match op {
        "trim_corner" => {
            let (a, b) = sketch::trim_corner(&curve_of(p, "a")?, point_of(p.get("pick_a").ok_or("missing pick_a")?)?, &curve_of(p, "b")?, point_of(p.get("pick_b").ok_or("missing pick_b")?)?)?;
            vec![a, b]
        }
        "trim_to" => vec![sketch::trim_to(&curve_of(p, "curve")?, point_of(p.get("pick").ok_or("missing pick")?)?, &curve_of(p, "boundary")?)?],
        "fillet" => sketch::fillet_lines(&curve_of(p, "a")?, point_of(p.get("pick_a").ok_or("missing pick_a")?)?, &curve_of(p, "b")?, point_of(p.get("pick_b").ok_or("missing pick_b")?)?, num_of(p, "radius")?)?,
        "offset" => {
            let towards = point_of(p.get("towards").ok_or("missing towards")?)?;
            let dist = num_of(p, "dist")?;
            if p.get("chain").is_some() {
                sketch::offset_chain(&curves_of(p, "chain")?, towards, dist)?
            } else {
                vec![sketch::offset_curve(&curve_of(p, "curve")?, towards, dist)?]
            }
        }
        "split" => {
            let (a, b) = sketch::split_curve(&curve_of(p, "curve")?, point_of(p.get("at").ok_or("missing at")?)?)?;
            vec![a, b]
        }
        "transform" => {
            let x = match p.get("kind").and_then(|v| v.as_str()) {
                Some("move") => Xform::Move { dx: num_of(p, "dx")?, dy: num_of(p, "dy")? },
                Some("rotate") => Xform::Rotate { cx: num_of(p, "cx")?, cy: num_of(p, "cy")?, angle_deg: num_of(p, "angle_deg")? },
                Some("mirror") => {
                    let (a, b) = (point_of(p.get("a").ok_or("missing a")?)?, point_of(p.get("b").ok_or("missing b")?)?);
                    if a.dist(&b) < 1e-6 { return Err("the mirror axis needs two different points".into()); }
                    Xform::Mirror { ax: a.x, ay: a.y, bx: b.x, by: b.y }
                }
                _ => return Err("kind must be move, rotate or mirror".into()),
            };
            curves_of(p, "curves")?.iter().map(|c| c.transform(x)).collect()
        }
        _ => return Err(format!("unknown sketch operation '{op}'")),
    };
    Ok(json!({ "curves": curves.iter().map(describe_curve).collect::<Vec<_>>() }))
}


/// A solved family body for the editor: plane positions, forms with triangles (mm), and what each view sees.
fn family_preview_json(solved: Result<openarch_model::famgeom::Solved, String>) -> Value {
    use openarch_model::famgeom::Axis;
    let s = match solved {
        Ok(s) => s,
        Err(e) => return json!({ "error": e }),
    };
    let view = |ax: Axis| -> Vec<Value> {
        s.project(ax)
            .into_iter()
            .map(|p| json!({ "form": p.form, "void": p.void, "sub": p.subcategory, "material": p.material, "rings": p.shape.iter().map(|r| r.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>()).collect::<Vec<_>>() }))
            .collect()
    };
    json!({
        "planes": s.planes,
        "forms": s.forms.iter().map(|f| json!({
            "id": f.id, "name": f.name, "void": f.void, "cuts_host": f.cuts_host, "material": f.material, "sub": f.subcategory, "present": f.vis.present, "span": f.span, "text": f.text, "owner": f.owner,
            "tris": f.solid.tris.iter().flat_map(|t| t.iter().flatten().copied()).collect::<Vec<f64>>(),
        })).collect::<Vec<_>>(),
        "syms": s.syms.iter().map(|y| json!({ "id": y.id, "view": y.view, "points": y.points.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>(), "closed": y.closed, "filled": y.filled, "sub": y.subcategory, "present": y.vis.present, "owner": y.owner })).collect::<Vec<_>>(),
        "model_lines": s.model_lines.iter().map(|m| json!({ "id": m.id, "name": m.name, "points": m.points, "closed": m.closed, "sub": m.subcategory, "present": m.vis.present, "owner": m.owner })).collect::<Vec<_>>(),
        "controls": s.controls.iter().map(|c| json!({ "id": c.id, "view": c.view, "at": [c.at.x, c.at.y], "kind": c.kind, "present": c.vis.present })).collect::<Vec<_>>(),
        "connectors": s.connectors.iter().map(|c| json!({ "id": c.id, "name": c.name, "kind": c.kind, "origin": c.origin, "direction": c.direction, "numbers": c.numbers })).collect::<Vec<_>>(),
        "views": { "plan": view(Axis::Z), "front": view(Axis::Y), "left": view(Axis::X) },
        "cut_box": s.host_cut_bounds(),
        "reports": s.reports.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>(),
        "warnings": s.warnings,
    })
}

/// The common arguments of placing or previewing a spot.
#[allow(clippy::type_complexity)]
fn spot_args(p: &Value) -> Result<(Id, SpotKind, Point, Option<Id>, SpotSurface, [f64; 2], bool), String> {
    let kind: SpotKind = serde_json::from_value(p.get("kind").cloned().unwrap_or(json!("elevation"))).map_err(|_| "unknown spot kind".to_string())?;
    let surface: SpotSurface = serde_json::from_value(p.get("surface").cloned().unwrap_or(json!("top"))).map_err(|_| "unknown surface".to_string())?;
    let shoulder = match p.get("shoulder").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value::<[f64; 2]>(v.clone()).map_err(|_| "bad shoulder".to_string())?,
        None => [0.0, 0.0],
    };
    let host = p.get("host").filter(|v| !v.is_null()).and_then(|v| v.as_u64());
    Ok((id_of(p, "view_id")?, kind, point_of(p.get("target").ok_or("missing target")?)?, host, surface, shoulder, p.get("leader").and_then(|v| v.as_bool()).unwrap_or(true)))
}

/// The parts of a spot type the plan needs to draw with.
fn spot_type_json(ty: &openarch_model::spot::SpotType) -> Value {
    json!({ "name": ty.name, "text_size_mm": ty.text_size_mm, "text_offset_mm": ty.text_offset_mm, "symbol": ty.symbol, "symbol_size_mm": ty.symbol_size_mm })
}

/// The parts of a dimension type the plan needs to draw with.
fn dimension_type_json(ty: &openarch_model::dimension::DimensionType) -> Value {
    json!({
        "name": ty.name, "tick": ty.tick, "tick_size_mm": ty.tick_size_mm, "witness_gap_mm": ty.witness_gap_mm,
        "witness_extension_mm": ty.witness_extension_mm, "text_size_mm": ty.text_size_mm, "text_offset_mm": ty.text_offset_mm,
        "text_background": ty.text_background,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut Session, m: &str, p: Value) -> Value {
        s.handle(m, &p).unwrap_or_else(|e| panic!("{m} failed: {e}"))
    }

    /// Volume of a triangle-list mesh of (pos, normal) vertices, in cubic metres.
    fn volume(verts: &[Value]) -> f64 {
        let v: Vec<f64> = verts.iter().map(|x| x.as_f64().unwrap()).collect();
        v.chunks(18)
            .map(|t| {
                let (a, b, c) = ([t[0], t[1], t[2]], [t[6], t[7], t[8]], [t[12], t[13], t[14]]);
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
            })
            .sum()
    }

    #[test]
    fn layered_wall_meshes_fill_the_wall_and_leave_the_opening() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let lvl = info["levels"][0]["id"].as_u64().unwrap();
        let wt = info["wall_types"].as_array().unwrap().iter().find(|t| t["name"] == "Concrete 200").unwrap()["id"].as_u64().unwrap();
        let w = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 3000}))["id"].as_u64().unwrap();
        let total = |s: &mut Session| -> f64 {
            let scene = call(s, "scene", json!({}));
            scene["meshes"].as_array().unwrap().iter().filter(|m| m["id"].as_u64() == Some(w)).map(|m| volume(m["verts"].as_array().unwrap())).sum()
        };
        // 6 m x 3 m x 0.2 m
        let solid = total(&mut s);
        assert!((solid - 3.6).abs() < 0.05, "{solid}");
        let door = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Single-Flush: 820 x 2040").unwrap()["id"].as_u64().unwrap();
        call(&mut s, "place_instance", json!({"type_id": door, "host_id": w, "point": [3000, 0]}));
        // the door opening removes about 0.82 x 2.04 x 0.2 m
        let with_door = total(&mut s);
        assert!((solid - with_door - 0.82 * 2.04 * 0.2).abs() < 0.05, "{solid} {with_door}");
        let scene = call(&mut s, "scene", json!({}));
        assert!(scene["meshes"].as_array().unwrap().iter().any(|m| m["id"].as_u64() == Some(w) && m["color"].is_array()));
    }

    #[test]
    fn a_circle_of_walls_is_two_arc_walls_that_join_draw_mesh_and_take_a_door() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let lvl = info["levels"][0]["id"].as_u64().unwrap();
        let wt = info["wall_types"].as_array().unwrap().iter().find(|t| t["name"] == "Brick Veneer").unwrap()["id"].as_u64().unwrap();
        let r = call(&mut s, "add_walls", json!({"type_id": wt, "level_id": lvl, "segments": [{"curve": {"type": "circle", "centre": {"x": 0, "y": 0}, "radius": 4000}}]}));
        let ids: Vec<u64> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
        assert_eq!(ids.len(), 2, "a circle goes round in two walls");
        let plan = call(&mut s, "plan", json!({"level_id": lvl}));
        let walls = plan["walls"].as_array().unwrap();
        assert_eq!(walls.len(), 2);
        for w in walls {
            assert!(w["curve"]["type"] == "arc");
            assert!(w["corners"].as_array().unwrap().len() > 40, "a smooth outline, not four corners");
            assert!((w["length"].as_f64().unwrap() - 4000.0 * std::f64::consts::PI).abs() < 5.0);
            // the two halves join end to end, so the layers run all the way round
            assert_eq!(w["layered"], true);
            assert!(!w["shape"].as_array().unwrap().is_empty());
        }
        let scene = call(&mut s, "scene", json!({}));
        let mut mesh_ids: Vec<u64> = scene["meshes"].as_array().unwrap().iter().map(|m| m["id"].as_u64().unwrap()).collect();
        mesh_ids.sort();
        mesh_ids.dedup();
        assert_eq!(mesh_ids, { let mut v = ids.clone(); v.sort(); v });
        // a door placed on the outside of the circle
        let door = info["family_types"].as_array().unwrap().iter().find(|t| t["label"].as_str().unwrap().starts_with("Single-Flush")).unwrap()["id"].as_u64().unwrap();
        let placed = call(&mut s, "place_instance", json!({"type_id": door, "host_id": ids[0], "point": [0, 4300]}));
        assert!(placed["id"].as_u64().is_some());
        // one undo for the door, one for the whole circle
        call(&mut s, "undo", json!({}));
        call(&mut s, "undo", json!({}));
        assert_eq!(call(&mut s, "plan", json!({"level_id": lvl}))["walls"].as_array().unwrap().len(), 0);
        // an arc by itself: describe shows its length
        let a = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "curve": {"type": "arc", "start": {"x": 3000, "y": 0}, "end": {"x": 0, "y": 3000}, "centre": {"x": 0, "y": 0}, "ccw": true}}))["id"].as_u64().unwrap();
        let d = call(&mut s, "describe", json!({"id": a}));
        let text = d.to_string();
        assert!(text.contains("4712"), "length of a quarter circle of 3 m radius: {text}");
    }

    #[test]
    fn sketch_floor_with_opening_and_slope_through_the_api() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let lvl = info["levels"][0]["id"].as_u64().unwrap();
        let ft = info["floor_types"][0]["id"].as_u64().unwrap();
        let rect = |s: &mut Session, a: [f64; 2], b: [f64; 2]| -> Vec<Value> {
            let r = call(s, "sketch_make", json!({"tool": "rectangle", "points": [a, b]}));
            r["curves"].as_array().unwrap().iter().map(|c| c["curve"].clone()).collect()
        };
        // an incomplete pick set gives no curves rather than an error
        let none = call(&mut s, "sketch_make", json!({"tool": "rectangle", "points": [[0, 0]]}));
        assert!(none["curves"].as_array().unwrap().is_empty());
        let mut curves = rect(&mut s, [0.0, 0.0], [6000.0, 4000.0]);
        assert_eq!(curves.len(), 4);
        curves.extend(rect(&mut s, [1000.0, 1000.0], [2000.0, 2000.0]));
        let check = call(&mut s, "sketch_check", json!({"curves": curves}));
        assert_eq!(check["ok"], true);
        assert_eq!(check["loops"], 2);
        // an open sketch reports where it fails
        let open = call(&mut s, "sketch_check", json!({"curves": curves[..3].to_vec()}));
        assert_eq!(open["ok"], false);
        let id = call(&mut s, "add_floor_sketch", json!({"type_id": ft, "level_id": lvl, "curves": curves}))["id"].as_u64().unwrap();
        let got = call(&mut s, "floor_sketch", json!({"id": id}));
        assert_eq!(got["curves"].as_array().unwrap().len(), 8);
        // the opening is cut from the plan area
        let plan = call(&mut s, "plan", json!({"level_id": lvl}));
        let f = plan["floors"].as_array().unwrap().iter().find(|f| f["id"].as_u64() == Some(id)).unwrap();
        assert_eq!(f["rings"].as_array().unwrap().len(), 2);
        // a slope arrow is stored and edit round trips
        let slope = json!({"tail": {"x": 0, "y": 0}, "head": {"x": 6000, "y": 0}, "spec": {"by": "slope", "tail_offset_mm": 0, "percent": 2}});
        call(&mut s, "set_floor_sketch", json!({"id": id, "curves": got["curves"].as_array().unwrap().iter().map(|c| c["curve"].clone()).collect::<Vec<_>>(), "slope": slope}));
        let again = call(&mut s, "floor_sketch", json!({"id": id}));
        assert_eq!(again["slope"]["spec"]["percent"], 2.0);
        // modify tools return replacement curves
        let off = call(&mut s, "sketch_edit", json!({"op": "offset", "curve": curves[0], "towards": [3000, 2000], "dist": 100}));
        assert_eq!(off["curves"].as_array().unwrap().len(), 1);
        let mv = call(&mut s, "sketch_edit", json!({"op": "transform", "kind": "move", "dx": 10, "dy": 0, "curves": [curves[0]]}));
        assert_eq!(mv["curves"][0]["curve"]["a"]["x"], 10.0);
    }

    #[test]
    fn draw_edit_undo_through_the_api() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let lvl = info["levels"][0]["id"].as_u64().unwrap();
        let wt = info["wall_types"][1]["id"].as_u64().unwrap();
        let w = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0]}))["id"].as_u64().unwrap();
        call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [6000, 0], "end": [6000, 4000], "height_mm": 3000}));

        let plan = call(&mut s, "plan", json!({"level_id": lvl}));
        assert_eq!(plan["walls"].as_array().unwrap().len(), 2);
        let scene = call(&mut s, "scene", json!({}));
        // a wall is one mesh per layer, all carrying the wall's id
        let mut ids: Vec<u64> = scene["meshes"].as_array().unwrap().iter().map(|m| m["id"].as_u64().unwrap()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 2);
        assert!(scene["meshes"].as_array().unwrap().len() > 2);

        let d = call(&mut s, "describe", json!({"id": w}));
        assert_eq!(d["kind"], "Wall");
        assert!(d["title"].as_str().unwrap().contains("Brick Veneer"));

        call(&mut s, "set_wall_height", json!({"id": w, "height_mm": 3300}));
        assert_eq!(call(&mut s, "describe", json!({"id": w}))["height_mm"], 3300.0);
        assert_eq!(call(&mut s, "doc_info", json!({}))["undo"], "Change wall height");
        call(&mut s, "undo", json!({}));
        assert_eq!(call(&mut s, "describe", json!({"id": w}))["height_mm"], 2700.0);

        assert!(s.handle("add_wall", &json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [0, 0]})).is_err());
        assert!(s.handle("nope", &json!({})).is_err());
    }

    #[test]
    fn schedule_uses_the_formula_engine() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let lvl = info["levels"][0]["id"].as_u64().unwrap();
        let wt = info["wall_types"][3]["id"].as_u64().unwrap();
        let ft = info["floor_types"][0]["id"].as_u64().unwrap();
        // 10 m wall x 2.7 m = 27 m2, floor 5 x 4 = 20 m2
        call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [10000, 0]}));
        call(&mut s, "add_floor", json!({"type_id": ft, "level_id": lvl, "points": [[0, 0], [5000, 0], [5000, 4000], [0, 4000]]}));
        let r = call(&mut s, "schedule", json!({"wall_rate": 200, "floor_rate": 150}));
        assert_eq!(r["walls"]["rows"][0][5], "5400"); // 27 * 200
        assert_eq!(r["floors"]["rows"][0][3], "3000"); // 20 * 150
        assert_eq!(r["walls"]["totals"][5], "5400");
        assert_eq!(r["total_cost"], "8400");
    }

    #[test]
    fn save_and_open_through_the_api() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let (lvl, wt) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][0]["id"].as_u64().unwrap());
        call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [3000, 0]}));
        assert_eq!(call(&mut s, "doc_info", json!({}))["dirty"], true);
        let dir = std::env::temp_dir().join(format!("openarch-api-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.oab");
        call(&mut s, "save", json!({"path": path.to_string_lossy()}));
        assert_eq!(call(&mut s, "doc_info", json!({}))["dirty"], false);
        let mut s2 = Session::new();
        call(&mut s2, "open", json!({"path": path.to_string_lossy()}));
        assert_eq!(call(&mut s2, "plan", json!({"level_id": lvl}))["walls"].as_array().unwrap().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    fn join_text(s: &mut Session, wall: u64) -> String {
        let d = call(s, "describe", json!({"id": wall}));
        let g = d["groups"].as_array().unwrap().iter().find(|g| g["name"] == "Joins").unwrap();
        g["rows"][0]["value"].as_str().unwrap().to_string()
    }

    #[test]
    fn join_configuration_reports_kinds_and_locks_t_joins_to_butt() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let (lvl, wt) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][3]["id"].as_u64().unwrap());
        let a = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0]}))["id"].as_u64().unwrap();
        let b = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [4000, 0], "end": [4000, 3000]}))["id"].as_u64().unwrap();
        // corner: all three kinds available, previous/next apply to butt and square off but not miter
        let ji = call(&mut s, "wall_join_info", json!({"id": a, "end": 1}));
        assert_eq!(ji["kind"], "butt");
        assert_eq!(ji["steps"], 2);
        assert!(ji["kinds"].as_array().unwrap().iter().all(|k| k["enabled"] == true));
        call(&mut s, "set_wall_join", json!({"id": a, "end": 1, "kind": "miter"}));
        let ji = call(&mut s, "wall_join_info", json!({"id": a, "end": 1}));
        assert_eq!((ji["kind"].as_str(), ji["steps"].as_u64()), (Some("miter"), Some(0)));
        // a wall ending on the face of another is a T join: butt only, host reported
        let t = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [2000, 3000], "end": [2000, 30]}))["id"].as_u64().unwrap();
        let ji = call(&mut s, "wall_join_info", json!({"id": t, "end": 1}));
        assert_eq!(ji["mid"].as_u64(), Some(a));
        assert_eq!(ji["kind"], "butt");
        assert_eq!(ji["editable"], false);
        let enabled: Vec<&str> = ji["kinds"].as_array().unwrap().iter().filter(|k| k["enabled"] == true).map(|k| k["kind"].as_str().unwrap()).collect();
        assert_eq!(enabled, ["butt"]);
        // disallowing the join frees the end
        call(&mut s, "set_wall_join", json!({"id": t, "end": 1, "allowed": false}));
        assert!(call(&mut s, "wall_join_info", json!({"id": t, "end": 1}))["mid"].is_null());
        let _ = b;
    }

    #[test]
    fn join_tool_can_step_through_options_and_survive_save() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let (lvl, wt) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][3]["id"].as_u64().unwrap());
        let a = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0]}))["id"].as_u64().unwrap();
        let b = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [4000, 0], "end": [4000, 3000]}))["id"].as_u64().unwrap();
        let ji = call(&mut s, "wall_join_info", json!({"id": a, "end": 1}));
        assert_eq!(ji["walls"].as_array().unwrap().len(), 2);
        assert_eq!(ji["options"].as_array().unwrap().len(), 5);
        assert_eq!(ji["current"], 0); // butt, older wall through
        // step to option 2 (butt, b through), then option 3 (miter)
        for want in [1usize, 2] {
            let opts = call(&mut s, "wall_join_info", json!({"id": a, "end": 1}))["options"].clone();
            let o = &opts[want];
            let mut args = json!({"id": a, "end": 1, "kind": o["kind"]});
            if !o["through"].is_null() {
                args["through"] = o["through"].clone();
            }
            call(&mut s, "set_wall_join", args);
            assert_eq!(call(&mut s, "wall_join_info", json!({"id": b, "end": 0}))["current"], want);
        }
        // disallow, then check the plan flags and the properties panel text
        call(&mut s, "set_wall_join", json!({"id": b, "end": 0, "allowed": false, "cleanup": "dont_clean"}));
        let plan = call(&mut s, "plan", json!({"level_id": lvl}));
        assert!(plan["walls"].as_array().unwrap().iter().all(|w| w["dont_clean"] == true));
        assert_eq!(join_text(&mut s, b), "Not allowed");
        // settings are saved
        let dir = std::env::temp_dir().join(format!("openarch-join-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("j.oab");
        call(&mut s, "save", json!({"path": path.to_string_lossy()}));
        let mut s2 = Session::new();
        call(&mut s2, "open", json!({"path": path.to_string_lossy()}));
        assert_eq!(join_text(&mut s2, b), "Not allowed");
        std::fs::remove_dir_all(dir).ok();
    }

    fn setup() -> (Session, u64, u64, u64) {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let (lvl, wt) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][3]["id"].as_u64().unwrap());
        let view = info["views"][0]["id"].as_u64().unwrap();
        (s, lvl, wt, view)
    }

    #[test]
    fn round_dimensions_preview_place_and_follow_a_curved_wall_over_the_api() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let lvl = info["levels"][0]["id"].as_u64().unwrap();
        let wt = info["wall_types"].as_array().unwrap().iter().find(|t| t["name"] == "Brick Veneer").unwrap()["id"].as_u64().unwrap();
        let view = info["views"].as_array().unwrap().iter().find(|v| v["kind"] == "plan").map(|v| v["id"].as_u64().unwrap()).unwrap_or_else(|| call(&mut s, "doc_info", json!({}))["views"][0]["id"].as_u64().unwrap());
        let w = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "curve": {"type": "arc", "start": {"x": 4000, "y": 0}, "end": {"x": 0, "y": 4000}, "centre": {"x": 0, "y": 0}, "ccw": true}, "height_mm": 2700}))["id"].as_u64().unwrap();
        let cands = call(&mut s, "dimension_candidates", json!({"view_id": view, "point": [2828, 2828], "tol": 150, "settings": {"wall_ref": "centreline"}, "want": "arc"}));
        let c = &cands["candidates"][0];
        assert!(c["arc"].is_array(), "{cands}");
        let refs = json!([{"spec": c["spec"], "pick": [2828, 2828]}]);
        let ghost = call(&mut s, "preview_dimension", json!({"view_id": view, "kind": "radial", "refs": refs, "line_point": [1414, 1414]}));
        assert_eq!(ghost["dimension"]["segments"][0]["text"], "R4000");
        assert!(call(&mut s, "plan", json!({"view_id": view}))["dimensions"].as_array().unwrap().is_empty(), "a preview stores nothing");
        let d = call(&mut s, "add_dimension", json!({"view_id": view, "kind": "diameter", "refs": refs, "line_point": [1414, 1414]}))["id"].as_u64().unwrap();
        let moved = call(&mut s, "preview_dimension", json!({"id": d, "line_point": [-1000, 3000]}));
        assert_eq!(moved["dimension"]["segments"][0]["text"], "\u{00D8}8000");
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let dim = &plan["dimensions"][0];
        assert_eq!(dim["kind"], "diameter");
        assert!(dim["round"]["line"].is_array() && dim["type"]["tick"].is_string());
        assert!(s.handle("add_dimension", &json!({"view_id": view, "kind": "angular", "refs": refs, "line_point": [0, 0]})).is_err(), "one curved reference is not an angle");
        call(&mut s, "delete", json!({"ids": [w]}));
        assert!(call(&mut s, "plan", json!({"view_id": view}))["dimensions"].as_array().unwrap().is_empty());
    }

    #[test]
    fn sections_are_drawn_in_plan_cut_styled_moved_copied_and_deleted() {
        let (mut s, lvl, wt, view) = setup();
        for (a, b) in [([0, 0], [8000, 0]), ([8000, 0], [8000, 6000]), ([8000, 6000], [0, 6000]), ([0, 6000], [0, 0])] {
            call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": a, "end": b, "height": 2700.0}));
        }
        let made = call(&mut s, "add_section", json!({"level_id": lvl, "start": [-1000, 3000], "end": [9000, 3000]}));
        let (sec, sv) = (made["id"].as_u64().unwrap(), made["view_id"].as_u64().unwrap());
        // the plan shows the section with its head and the view's name
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let secs = plan["sections"].as_array().unwrap();
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0]["id"].as_u64(), Some(sec));
        assert!(secs[0]["family"]["pointer"]["fills"].as_array().unwrap().len() >= 1);
        let labels: Vec<String> = secs[0]["labels"].as_array().unwrap().iter().flat_map(|l| l["lines"].as_array().unwrap().iter().flat_map(|x| x.as_array().unwrap().iter().map(|p| p.as_str().unwrap().trim().to_string())).collect::<Vec<_>>()).collect();
        assert!(labels.contains(&"Section 1".to_string()) && labels.contains(&"-".to_string()), "{labels:?}");
        // the view is listed and draws cut walls with the material's cut style
        let info = call(&mut s, "doc_info", json!({}));
        let v = info["views"].as_array().unwrap().iter().find(|v| v["id"].as_u64() == Some(sv)).unwrap();
        assert_eq!(v["view_kind"], "section");
        assert_eq!(v["section_id"].as_u64(), Some(sec));
        let data = call(&mut s, "elevation", json!({"view_id": sv}));
        let cuts: Vec<&Value> = data["items"].as_array().unwrap().iter().filter(|i| i["cut"] == true).collect();
        assert!(!cuts.is_empty() && cuts.iter().all(|c| c["style"]["line"]["width_mm"].as_f64().unwrap() > 0.0), "{data}");
        assert!(data["fill_patterns"]["Solid fill"]["solid"] == true);
        // properties
        let props = call(&mut s, "describe", json!({"id": sec}));
        assert_eq!(props["target"]["method"], "patch_section");
        call(&mut s, "patch_section", json!({"id": sec, "patch": {"far_clip_mm": 2000.0, "far_clip": "without_line"}}));
        assert_eq!(call(&mut s, "elevation", json!({"view_id": sv}))["frame"]["depth_mm"].as_f64(), Some(2000.0));
        // flipping looks the other way
        call(&mut s, "patch_section", json!({"id": sec, "patch": {"flipped": true}}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"][0]["look"][1].as_f64().unwrap().round(), -1.0);
        assert!(s.handle("add_section", &json!({"level_id": lvl, "start": [0, 0], "end": [10, 0]})).is_err());
        // move, copy and mirror with the transform command
        call(&mut s, "transform", json!({"ids": [sec], "kind": "move", "dx": 0.0, "dy": 1000.0}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"][0]["start"][1].as_f64(), Some(4000.0));
        let copy = call(&mut s, "transform", json!({"ids": [sec], "kind": "move", "copy": true, "dx": 0.0, "dy": 500.0}));
        assert_eq!(copy["ids"].as_array().unwrap().len(), 1);
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"].as_array().unwrap().len(), 2);
        // deleting the section view deletes the section; so does deleting the section
        call(&mut s, "delete", json!({"ids": [sv]}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"].as_array().unwrap().len(), 1);
        let left = call(&mut s, "plan", json!({"view_id": view}))["sections"][0]["id"].as_u64().unwrap();
        call(&mut s, "delete", json!({"ids": [left]}));
        assert!(call(&mut s, "plan", json!({"view_id": view}))["sections"].as_array().unwrap().is_empty());
        call(&mut s, "undo", json!({}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn sections_jog_break_reference_other_views_and_fade_with_depth() {
        let (mut s, lvl, wt, view) = setup();
        for (a, b) in [([0, 0], [8000, 0]), ([8000, 0], [8000, 6000]), ([8000, 6000], [0, 6000]), ([0, 6000], [0, 0])] {
            call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": a, "end": b, "height": 2700.0}));
        }
        let made = call(&mut s, "add_section", json!({"level_id": lvl, "start": [-1000, 3000], "end": [9000, 3000]}));
        let (sec, sv) = (made["id"].as_u64().unwrap(), made["view_id"].as_u64().unwrap());
        // Split Segment: a preview first, then the split; the plan shows the runs and the jog
        let pv = call(&mut s, "section_edit", json!({"id": sec, "op": "split", "at": [4000, 3000], "to": [6500, 1000], "preview": true}));
        assert_eq!(pv["pts"].as_array().unwrap().len(), 4);
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"][0]["runs"].as_array().unwrap().len(), 1);
        call(&mut s, "section_edit", json!({"id": sec, "op": "split", "at": [4000, 3000], "to": [6500, 1000]}));
        let p = &call(&mut s, "plan", json!({"view_id": view}))["sections"][0];
        assert_eq!(p["runs"].as_array().unwrap().len(), 2);
        assert_eq!(p["jogs"][0]["a"], json!([4000.0, 3000.0]));
        assert_eq!(p["pts"].as_array().unwrap().len(), 4);
        assert!(p["gap"].is_null());
        let data = call(&mut s, "elevation", json!({"view_id": sv}));
        assert_eq!(data["folds"], json!([5000.0]));
        assert_eq!(data["section"]["runs"].as_array().unwrap().len(), 2);
        assert!(s.handle("section_edit", &json!({"id": sec, "op": "move_jog", "index": 3, "to": [0, 0]})).is_err());
        call(&mut s, "section_edit", json!({"id": sec, "op": "move_run", "index": 1, "to": [6000, 500]}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"][0]["end"], json!([9000.0, 500.0]));
        // the break control: on, lengths, off
        assert_eq!(call(&mut s, "section_break", json!({"id": sec, "view_id": view}))["broken"], true);
        call(&mut s, "section_break", json!({"id": sec, "view_id": view, "head_mm": 2000.0, "tail_mm": 1500.0}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"][0]["gap"], json!({"head_mm": 2000.0, "tail_mm": 1500.0}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["sections"][0]["broken_style"], "gapped");
        assert_eq!(call(&mut s, "section_break", json!({"id": sec, "view_id": view}))["broken"], false);
        // a reference section points at the section view: no view of its own, it opens the one it points at
        let targets = call(&mut s, "reference_targets", json!({}));
        assert!(targets.as_array().unwrap().iter().any(|t| t["id"].as_u64() == Some(sv)));
        let n_views = call(&mut s, "doc_info", json!({}))["views"].as_array().unwrap().len();
        let r = call(&mut s, "add_section", json!({"level_id": lvl, "start": [2000, -1000], "end": [2000, 7000], "reference": sv}));
        assert!(r["view_id"].is_null());
        assert_eq!(call(&mut s, "doc_info", json!({}))["views"].as_array().unwrap().len(), n_views);
        let rid = r["id"].as_u64().unwrap();
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let rp = plan["sections"].as_array().unwrap().iter().find(|x| x["id"].as_u64() == Some(rid)).unwrap();
        assert!(rp["reference"] == true && rp["view_id"].as_u64() == Some(sv) && rp["depth"].is_null());
        let props = call(&mut s, "describe", json!({"id": rid}));
        assert_eq!(props["title"], "Reference Section");
        // a copy of it is a reference section too
        let copy = call(&mut s, "transform", json!({"ids": [rid], "kind": "move", "copy": true, "dx": 500.0, "dy": 0.0}));
        let cid = copy["ids"][0].as_u64().unwrap();
        assert_eq!(call(&mut s, "describe", json!({"id": cid}))["title"], "Reference Section");
        assert_eq!(call(&mut s, "doc_info", json!({}))["views"].as_array().unwrap().len(), n_views);
        // depth cueing: rows on the section view, values checked, items fade
        let vp = call(&mut s, "describe", json!({"id": sv}));
        assert!(vp.to_string().contains("Show Depth"));
        assert!(s.handle("patch_view", &json!({"id": sv, "patch": {"settings": {"graphic_display": {"depth_cue_start": 80, "depth_cue_end": 20}}}})).is_err());
        call(&mut s, "patch_view", json!({"id": sv, "patch": {"settings": {"discipline": "architectural", "graphic_display": {"depth_cue": true, "depth_cue_fade": 40}}}}));
        let items = call(&mut s, "elevation", json!({"view_id": sv}))["items"].as_array().unwrap().clone();
        assert!(items.iter().any(|i| i["fade"].as_f64().unwrap() < 1.0));
        assert!(items.iter().filter(|i| i["cut"] == true).all(|i| i["fade"].as_f64() == Some(1.0)));
    }

    #[test]
    fn spots_are_found_previewed_placed_edited_and_follow_their_floor() {
        let (mut s, lvl, _wt, view) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let ft = info["floor_types"][0]["id"].as_u64().unwrap();
        let floor = call(&mut s, "add_floor", json!({"type_id": ft, "level_id": lvl, "points": [[0, 0], [5000, 0], [5000, 4000], [0, 4000]]}))["id"].as_u64().unwrap();
        let hits = call(&mut s, "spot_surfaces", json!({"view_id": view, "point": [2500, 2000]}));
        let list = hits["surfaces"].as_array().unwrap();
        assert!(list.iter().any(|h| h["host"].as_u64() == Some(floor) && h["surface"] == "top"), "{hits}");
        let args = json!({"view_id": view, "kind": "elevation", "target": [2500, 2000], "host": floor, "surface": "top", "shoulder": [600, 400]});
        let ghost = call(&mut s, "preview_spot", args.clone());
        assert!(ghost["spot"]["lines"][0].as_str().unwrap().len() > 0 && ghost["spot"]["type"]["text_size_mm"].is_number(), "{ghost}");
        assert!(call(&mut s, "plan", json!({"view_id": view}))["spots"].as_array().unwrap().is_empty(), "a preview stores nothing");
        let sp = call(&mut s, "add_spot", args)["id"].as_u64().unwrap();
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        assert_eq!(plan["spots"][0]["id"].as_u64(), Some(sp));
        assert_eq!(plan["spots"][0]["category"], "Spot Elevations");
        let props = call(&mut s, "describe", json!({"id": sp}));
        assert_eq!(props["target"]["method"], "patch_spot");
        call(&mut s, "patch_spot", json!({"id": sp, "patch": {"surface": "bottom", "prefix": "FFL "}}));
        let after = call(&mut s, "plan", json!({"view_id": view}));
        assert!(after["spots"][0]["lines"][0].as_str().unwrap().contains("FFL "), "{after}");
        let before = after["spots"][0]["target"].clone();
        call(&mut s, "transform", json!({"ids": [floor], "kind": "move", "dx": 1000.0, "dy": 0.0}));
        let moved = call(&mut s, "plan", json!({"view_id": view}));
        assert_ne!(moved["spots"][0]["target"], before, "the spot goes with its floor");
        call(&mut s, "delete", json!({"ids": [floor]}));
        assert!(call(&mut s, "plan", json!({"view_id": view}))["spots"].as_array().unwrap().is_empty(), "no floor, no spot");
    }

    #[test]
    fn permanent_dimensions_are_placed_drawn_edited_locked_and_deleted() {
        let (mut s, lvl, wt, view) = setup();
        let wall = |s: &mut Session, x: f64| call(s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [x, 0], "end": [x, 4000], "height_mm": 2700}))["id"].as_u64().unwrap();
        let (a, b, c) = (wall(&mut s, 0.0), wall(&mut s, 3000.0), wall(&mut s, 6000.0));
        // the pick finds a wall line near the cursor
        let cands = call(&mut s, "dimension_candidates", json!({"view_id": view, "point": [3000, 2000], "tol": 150, "settings": {"wall_ref": "centreline"}}));
        let first = &cands["candidates"][0];
        assert_eq!(first["spec"]["id"].as_u64().unwrap(), b);
        let r = |w: u64| json!({"spec": {"kind": "wall", "id": w, "offset": 0.0}, "pick": [0, 3500]});
        let d = call(&mut s, "add_dimension", json!({"view_id": view, "kind": "aligned", "refs": [r(a), r(b), r(c)], "line_point": [3000, 4500]}))["id"].as_u64().unwrap();
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let dim = &plan["dimensions"][0];
        assert_eq!(dim["id"].as_u64().unwrap(), d);
        assert_eq!(dim["segments"].as_array().unwrap().len(), 2);
        assert_eq!(dim["segments"][0]["text"], "3000");
        assert_eq!(dim["type"]["text_size_mm"], 2.5);
        // type a value, lock a segment, delete a wall
        call(&mut s, "set_dimension_value", json!({"id": d, "segment": 1, "value": 3500}));
        call(&mut s, "patch_dimension", json!({"id": d, "patch": {"segments": [{"index": 0, "locked": true}]}}));
        call(&mut s, "transform", json!({"ids": [a], "kind": "move", "dx": 200, "dy": 0}));
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        assert_eq!(plan["dimensions"][0]["segments"][0]["text"], "3000", "the locked segment kept its distance");
        let props = call(&mut s, "describe", json!({"id": d}));
        assert_eq!(props["kind"], "Dimension");
        call(&mut s, "delete", json!({"ids": [b]}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["dimensions"][0]["segments"].as_array().unwrap().len(), 1);
        call(&mut s, "delete", json!({"ids": [d]}));
        assert!(call(&mut s, "plan", json!({"view_id": view}))["dimensions"].as_array().unwrap().is_empty());
        // hidden when the category is turned off
        let d2 = call(&mut s, "add_dimension", json!({"view_id": view, "kind": "linear", "axis": [1, 0], "refs": [r(a), r(c)], "line_point": [3000, -500]}))["id"].as_u64().unwrap();
        assert!(d2 > 0);
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"overrides": {"annotation": {"Dimensions": {"visible": false}}}}}}));
        assert!(call(&mut s, "plan", json!({"view_id": view}))["dimensions"].as_array().unwrap().is_empty());
    }

    #[test]
    fn view_range_decides_what_is_cut_and_what_is_projection() {
        let (mut s, lvl, wt, view) = setup();
        let full = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0], "height_mm": 2700}))["id"].as_u64().unwrap();
        let low = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 1000], "end": [4000, 1000], "height_mm": 900}))["id"].as_u64().unwrap();
        let high = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 2000], "end": [4000, 2000], "height_mm": 600}))["id"].as_u64().unwrap();
        // lift the high wall above the cut plane
        call(&mut s, "patch_wall", json!({"id": high, "patch": {"base_offset_mm": 1500}}));
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let walls = plan["walls"].as_array().unwrap();
        let disp = |id: u64| walls.iter().find(|w| w["id"] == id).map(|w| w["display"].as_str().unwrap().to_string());
        assert_eq!(disp(full).as_deref(), Some("cut"));
        assert_eq!(disp(low).as_deref(), Some("projection"));
        assert_eq!(disp(high), None); // above the cut plane
        assert_eq!(plan["range"]["cut"], 1200.0);
        // move the cut plane to 1800 so it passes through the lifted wall (1500 to 2100)
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"view_range": {"cut_offset_mm": 1800.0, "top": {"offset_mm": 2300.0}}}}}));
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let walls = plan["walls"].as_array().unwrap();
        assert_eq!(walls.iter().find(|w| w["id"] == high).map(|w| w["display"].as_str().unwrap()), Some("cut"));
    }

    #[test]
    fn graphics_overrides_change_the_resolved_style_and_can_hide() {
        let (mut s, lvl, wt, view) = setup();
        let w = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0]}))["id"].as_u64().unwrap();
        let style = |s: &mut Session| call(s, "plan", json!({"view_id": view}))["walls"][0]["style"].clone();
        assert_eq!(style(&mut s)["line"]["weight"], 3);
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"overrides": {"model": {"Walls": {
            "cut_lines": {"color": [200, 0, 0], "weight": 6}, "cut_foreground": {"color": [250, 220, 220]}, "halftone": true }}}}}}));
        let st = style(&mut s);
        assert_eq!((st["line"]["weight"].as_u64(), st["line"]["color"].as_str(), st["fill"]["color"].as_str(), st["halftone"].as_bool()),
                   (Some(6), Some("#c80000"), Some("#fadcdc"), Some(true)));
        // per-element override wins over the category
        let vv = view;
        call(&mut s, "patch_view", json!({"id": vv, "patch": {"element_overrides": {w.to_string(): {"cut_lines": {"weight": 9}}}}}));
        assert_eq!(style(&mut s)["line"]["weight"], 9);
        // switch the category off, then hide just the element
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"overrides": {"model": {"Walls": {"visible": false}}}}}}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["walls"].as_array().unwrap().len(), 0);
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"overrides": {"model": {"Walls": {"visible": true}}}}}}));
        call(&mut s, "hide_elements", json!({"view_id": view, "ids": [w]}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["walls"].as_array().unwrap().len(), 0);
        call(&mut s, "hide_elements", json!({"view_id": view, "ids": [w], "hide": false}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["walls"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn underlay_shows_the_level_below_halftone() {
        let (mut s, lvl, wt, _) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let (l2, v2) = (info["levels"][1]["id"].as_u64().unwrap(), info["views"][1]["id"].as_u64().unwrap());
        call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0]}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": v2}))["walls"].as_array().unwrap().len(), 0);
        call(&mut s, "patch_view", json!({"id": v2, "patch": {"underlay": {"base_level": lvl}}}));
        let plan = call(&mut s, "plan", json!({"view_id": v2}));
        assert_eq!(plan["walls"][0]["display"], "underlay");
        assert_eq!(plan["walls"][0]["style"]["halftone"], true);
        let _ = l2;
    }

    #[test]
    fn properties_rows_edit_through_patches_and_templates_lock_them() {
        let (mut s, lvl, wt, view) = setup();
        let w = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0]}))["id"].as_u64().unwrap();
        let d = call(&mut s, "describe", json!({"id": w}));
        assert_eq!(d["target"]["method"], "patch_wall");
        let row = |d: &Value, label: &str| d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label).unwrap();
        // pick "Finish Face: Exterior" from the Location Line select and apply its patch
        let opt = row(&d, "Location Line")["options"].as_array().unwrap().iter().find(|o| o["label"] == "Finish Face: Exterior").unwrap().clone();
        call(&mut s, "patch_wall", json!({"id": w, "patch": opt["patch"]}));
        assert_eq!(row(&call(&mut s, "describe", json!({"id": w})), "Location Line")["value"], "Finish Face: Exterior");
        assert_eq!(row(&d, "IfcGUID")["value"].as_str().unwrap().len(), 22);
        // view properties; a template locks what it includes
        let vd = call(&mut s, "describe", json!({"id": view}));
        assert_eq!(row(&vd, "View Scale")["value"], "1 : 100");
        assert!(row(&vd, "View Scale").get("locked").is_none());
        let t = call(&mut s, "create_view_template", json!({"name": "Plan 1:50", "view_id": view}))["id"].as_u64().unwrap();
        call(&mut s, "patch_view_template", json!({"id": t, "patch": {"included": ["scale"], "settings": {"scale": 50}}}));
        call(&mut s, "patch_view", json!({"id": view, "patch": {"template": t}}));
        let vd = call(&mut s, "describe", json!({"id": view}));
        assert_eq!(row(&vd, "View Scale")["value"], "1 : 50");
        assert_eq!(row(&vd, "View Scale")["locked"], true);
        assert!(row(&vd, "Detail Level").get("locked").is_none());
        assert_eq!(row(&vd, "View Template")["value"], "Plan 1:50");
        // the V/G dialog data lists every model category
        let g = call(&mut s, "view_graphics", json!({"view_id": view}));
        assert!(g["model"].as_array().unwrap().iter().any(|c| c["name"] == "Walls"));
        // and a plan can still be drawn through a template
        assert_eq!(call(&mut s, "plan", json!({"view_id": view}))["view"]["scale"], 50);
    }

    #[test]
    fn spacebar_flips_walls_and_doors_as_single_undo_steps() {
        let (mut s, lvl, wt, _view) = setup();
        let w = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
        let info = call(&mut s, "doc_info", json!({}));
        let door = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Single-Flush: 820 x 2040").unwrap()["id"].as_u64().unwrap();
        let d = call(&mut s, "place_instance", json!({ "type_id": door, "host_id": w, "point": [1500, 200], "flip_hand": true }))["id"].as_u64().unwrap();
        let rows = |s: &mut Session, id: u64| -> Vec<Value> {
            let d = call(s, "describe", json!({ "id": id }));
            d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect()
        };
        let flag = |s: &mut Session, id: u64, label: &str| rows(s, id).iter().find(|r| r["label"] == label).unwrap()["raw"].as_bool().unwrap();
        assert!(flag(&mut s, d, "Flip Hand"), "placing with the hand flip set is stored");

        // Space on a selected door steps (facing, hand): 01 -> 11 -> 10 -> 00
        call(&mut s, "flip_instances", json!({ "ids": [d] }));
        assert!(flag(&mut s, d, "Flip Facing") && flag(&mut s, d, "Flip Hand"));
        call(&mut s, "flip_instances", json!({ "ids": [d] }));
        assert!(flag(&mut s, d, "Flip Facing") && !flag(&mut s, d, "Flip Hand"));
        call(&mut s, "undo", json!({}));
        assert!(flag(&mut s, d, "Flip Facing") && flag(&mut s, d, "Flip Hand"), "each flip is one undo step");

        // walls flip as one undo step (the layer mirroring itself is covered in openarch-model with a layered type)
        call(&mut s, "flip_walls", json!({ "ids": [w] }));
        assert!(rows(&mut s, w).iter().any(|r| r["label"] == "Flipped" && r["raw"] == true));
        call(&mut s, "undo", json!({}));
        assert!(rows(&mut s, w).iter().any(|r| r["label"] == "Flipped" && r["raw"] == false));
        call(&mut s, "patch_wall", json!({ "id": w, "patch": { "flipped": true } }));
        assert!(rows(&mut s, w).iter().any(|r| r["label"] == "Flipped" && r["raw"] == true));
        let f = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 5000], "end": [6000, 5000], "flipped": true }))["id"].as_u64().unwrap();
        call(&mut s, "flip_walls", json!({ "ids": [f] }));
        assert!(s.handle("flip_instances", &json!({ "ids": [w] })).is_err(), "walls are not doors");
    }

    #[test]
    fn pinned_elements_refuse_delete_and_flip_until_unpinned() {
        let (mut s, lvl, wt, view) = setup();
        let w = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
        let o = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 3000], "end": [6000, 3000], "height_mm": 2700 }))["id"].as_u64().unwrap();
        call(&mut s, "pin", json!({ "ids": [w] }));
        let plan = call(&mut s, "plan", json!({ "view_id": view }));
        let pinned: Vec<(u64, bool)> = plan["walls"].as_array().unwrap().iter().map(|x| (x["id"].as_u64().unwrap(), x["pinned"].as_bool().unwrap())).collect();
        assert!(pinned.contains(&(w, true)) && pinned.contains(&(o, false)));
        assert!(s.handle("delete", &json!({ "ids": [w] })).unwrap_err().contains("pinned"));
        assert!(s.handle("flip_walls", &json!({ "ids": [w, o] })).unwrap_err().contains("pinned"));
        call(&mut s, "delete", json!({ "ids": [o] }));
        // unpin through the Properties row, which is a plain patch, and the wall is free again
        call(&mut s, "patch_wall", json!({ "id": w, "patch": { "pinned": false } }));
        call(&mut s, "flip_walls", json!({ "ids": [w] }));
        call(&mut s, "pin", json!({ "ids": [w] }));
        call(&mut s, "undo", json!({}));
        assert!(s.handle("delete", &json!({ "ids": [w] })).is_ok(), "undo of Pin unpins");
    }

    #[test]
    fn modify_commands_move_copy_rotate_mirror_offset_trim_and_align() {
        let (mut s, lvl, wt, view) = setup();
        let add = |s: &mut Session, a: [i32; 2], b: [i32; 2]| call(s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": a, "end": b, "height_mm": 2700 }))["id"].as_u64().unwrap();
        let a = add(&mut s, [0, 0], [4000, 0]);
        let b = add(&mut s, [4000, 0], [4000, 3000]);
        let ends = |s: &mut Session, id: u64| -> (Value, Value) {
            let plan = call(s, "plan", json!({ "view_id": view }));
            let w = plan["walls"].as_array().unwrap().iter().find(|w| w["id"] == id).unwrap().clone();
            (w["start"].clone(), w["end"].clone())
        };
        call(&mut s, "transform", json!({ "ids": [a], "kind": "move", "dx": 0, "dy": 500 }));
        assert_eq!(ends(&mut s, b).0, json!([4000.0, 500.0]), "the joined wall follows");
        let copies = call(&mut s, "transform", json!({ "ids": [a, b], "kind": "move", "copy": true, "dx": 0, "dy": 6000 }))["ids"].as_array().unwrap().len();
        assert_eq!(copies, 2);
        let r = call(&mut s, "transform", json!({ "ids": [a], "kind": "rotate", "cx": 0, "cy": 500, "angle_deg": 90 }));
        assert_eq!(r["ids"], json!([a]));
        call(&mut s, "undo", json!({}));
        let m = call(&mut s, "transform", json!({ "ids": [a], "kind": "mirror", "a": [2000, 0], "b": [2000, 1], "copy": true }))["ids"][0].as_u64().unwrap();
        assert_eq!(ends(&mut s, m).0, json!([4000.0, 500.0]));
        assert!(s.handle("transform", &json!({ "ids": [a], "kind": "scale" })).is_err());
        let o = call(&mut s, "offset", json!({ "ids": [a], "dist": 250, "copy": true }))["ids"][0].as_u64().unwrap();
        assert_eq!(ends(&mut s, o).0, json!([0.0, 750.0]));
        // trim the offset copy back to the second wall's line, then align it onto y = 900
        call(&mut s, "trim", json!({ "mode": "single", "wall": o, "pick": [100, 750], "boundary": b }));
        call(&mut s, "align", json!({ "ids": [o], "reference": [[0, 900], [10, 900]], "target": [[0, 750], [4000, 750]] }));
        assert_eq!(ends(&mut s, o).0, json!([0.0, 900.0]));
        assert!(s.handle("align", &json!({ "ids": [o], "reference": [[0, 900], [10, 900]], "target": [[0, 900], [4000, 900]] })).is_err());
    }

    #[test]
    fn annotation_families_drive_the_level_head_and_marker_through_the_api() {
        let (mut s, _lvl, _wt, _view) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let fams = info["annotation_families"].as_array().unwrap();
        assert!(fams.iter().any(|f| f["kind"] == "level_head") && fams.iter().any(|f| f["kind"] == "elevation_marker") && fams.iter().any(|f| f["kind"] == "section_marker"));
        let head = fams.iter().find(|f| f["name"] == "Level Head - AHD").unwrap()["id"].as_u64().unwrap();
        let ty = info["level_types"][0]["id"].as_u64().unwrap();
        // the type picks the head; the elevation view sends it with its labels worked out
        call(&mut s, "patch_level_type", json!({ "id": ty, "patch": { "head": head } }));
        let ev = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "elevation").unwrap()["id"].as_u64().unwrap();
        let data = call(&mut s, "elevation", json!({ "view_id": ev }));
        assert_eq!(data["scale"], 100.0);
        let l1 = data["levels"].as_array().unwrap().iter().find(|l| l["name"] == "Level 1").unwrap();
        assert_eq!(l1["head"]["labels"][0]["lines"][0][0], "Level 1");
        assert_eq!(l1["head"]["labels"][1]["lines"][0][0], "3000 AHD");
        // the level number is a label parameter; the family editor saves the whole family
        let mut f = call(&mut s, "annotation_family", json!({ "id": head }));
        assert!(f["params"].as_array().unwrap().iter().any(|p| p == "Level Number"));
        f["labels"][0]["params"] = json!([{ "param": "Level Number", "prefix": "L", "suffix": "", "sample": "1", "break": true }, { "param": "Name", "prefix": "", "suffix": "", "sample": "Ground" }]);
        call(&mut s, "set_annotation_family", json!({ "id": head, "family": f }));
        let lid = info["levels"].as_array().unwrap().iter().find(|l| l["name"] == "Level 1").unwrap()["id"].as_u64().unwrap();
        call(&mut s, "patch_level", json!({ "id": lid, "patch": { "number": "2" } }));
        let data = call(&mut s, "elevation", json!({ "view_id": ev }));
        let l1 = data["levels"].as_array().unwrap().iter().find(|l| l["name"] == "Level 1").unwrap();
        assert_eq!(l1["head"]["labels"][0]["lines"], json!([["L2"], ["Level 1"]]));
        let bad = f.clone();
        let mut bad = bad;
        bad["labels"][0]["params"][0]["param"] = json!("Width");
        assert!(s.handle("set_annotation_family", &json!({ "id": head, "family": bad })).is_err());
        // the family editor draws a family it is changing, groups a region's sketch into loops, and describes curves
        let paths = call(&mut s, "annotation_paths", json!({ "family": f }));
        assert!(!paths["body"]["strokes"].as_array().unwrap().is_empty());
        let edge = |a: [f64; 2], b: [f64; 2]| json!({ "curve": { "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] } }, "subcategory": "" });
        let loops = call(&mut s, "annotation_region_loops", json!({ "edges": [edge([0.0, 0.0], [4.0, 0.0]), edge([4.0, 3.0], [4.0, 0.0]), edge([4.0, 3.0], [0.0, 0.0])] }));
        assert_eq!(loops["ok"], true);
        assert_eq!(loops["loops"][0].as_array().unwrap().len(), 3);
        let open = call(&mut s, "annotation_region_loops", json!({ "edges": [edge([0.0, 0.0], [4.0, 0.0]), edge([4.0, 0.0], [4.0, 3.0])] }));
        assert_eq!(open["ok"], false);
        let described = call(&mut s, "sketch_describe", json!({ "curves": [{ "type": "circle", "centre": { "x": 0.0, "y": 0.0 }, "radius": 400.0 }] }));
        assert!(described["curves"][0]["closed"] == true && described["curves"][0]["poly"].as_array().unwrap().len() > 8);
        // plan markers carry the family they are drawn with
        let plan = call(&mut s, "plan", json!({ "view_id": info["views"][0]["id"] }));
        assert!(plan["markers"][0]["family"]["pointer"]["fills"].as_array().unwrap().len() >= 1);
        // a level type is chosen in Properties
        let d = call(&mut s, "describe", json!({ "id": lid }));
        assert_eq!(d["type_id"], ty);
    }

    #[test]
    fn elevation_moves_lift_walls_windows_and_copies_in_one_step() {
        let (mut s, lvl, wt, _view) = setup();
        let w = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
        let info = call(&mut s, "doc_info", json!({}));
        let win = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Fixed: 1200 x 1200").unwrap()["id"].as_u64().unwrap();
        let i = call(&mut s, "place_instance", json!({ "type_id": win, "host_id": w, "point": [4000, -50] }))["id"].as_u64().unwrap();
        let sill = |s: &mut Session| call(s, "describe", json!({ "id": i }));
        let before = sill(&mut s).to_string();
        call(&mut s, "move_vertical", json!({ "ids": [i], "dz": 150 }));
        assert_ne!(sill(&mut s).to_string(), before);
        call(&mut s, "undo", json!({}));
        assert_eq!(sill(&mut s).to_string(), before);
        // a wall lifts by its base offset, and a copy moves sideways and up as one step
        call(&mut s, "move_vertical", json!({ "ids": [w], "dz": 300 }));
        assert_eq!(call(&mut s, "describe", json!({ "id": w })).to_string().contains("300"), true);
        let n = call(&mut s, "move_vertical", json!({ "ids": [w], "dz": 0, "dx": 0, "dy": 5000, "copy": true }))["ids"].as_array().unwrap().len();
        assert_eq!(n, 1);
        call(&mut s, "undo", json!({}));
        // pinned elements refuse, and an element with no height to change says so
        call(&mut s, "pin", json!({ "ids": [w] }));
        assert!(s.handle("move_vertical", &json!({ "ids": [w], "dz": 100 })).unwrap_err().contains("pinned"));
    }

    #[test]
    fn doors_and_windows_place_cut_and_edit_through_the_api() {
        let (mut s, lvl, wt, view) = setup();
        let w = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
        let info = call(&mut s, "doc_info", json!({}));
        let find = |name: &str| info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == name).unwrap()["id"].as_u64().unwrap();
        let (door, win) = (find("Single-Flush: 820 x 2040"), find("Fixed: 1200 x 1200"));
        let d1 = call(&mut s, "place_instance", json!({ "type_id": door, "host_id": w, "point": [1500, 200] }))["id"].as_u64().unwrap();
        call(&mut s, "place_instance", json!({ "type_id": win, "host_id": w, "point": [4000, -50] }));
        assert!(s.handle("place_instance", &json!({ "type_id": door, "host_id": w, "point": [1600, 0] })).unwrap_err().contains("overlaps"));

        // plan: the wall breaks at both openings (cut plane 1200 is inside 0..2040 and 900..2100)
        let plan = call(&mut s, "plan", json!({ "view_id": view }));
        assert_eq!(plan["walls"][0]["pieces"].as_array().unwrap().len(), 3);
        let inserts = plan["inserts"].as_array().unwrap();
        assert_eq!(inserts.len(), 2);
        let dr = inserts.iter().find(|i| i["category"] == "Doors").unwrap();
        assert_eq!(dr["parts"].as_array().unwrap().iter().filter(|p| p["sub"] == "Plan Swing").count(), 1);
        assert_eq!(dr["display"], "cut");

        // hiding the Doors category hides the symbol but the opening stays
        call(&mut s, "patch_view", json!({ "id": view, "patch": { "settings": { "overrides": { "model": { "Doors": { "visible": false } } } } } }));
        let plan = call(&mut s, "plan", json!({ "view_id": view }));
        assert_eq!(plan["inserts"].as_array().unwrap().len(), 1);
        assert_eq!(plan["walls"][0]["pieces"].as_array().unwrap().len(), 3);

        // 3D: the wall mesh has pieces over and under the openings, plus door and window meshes
        let scene = call(&mut s, "scene", json!({}));
        let kinds: Vec<&str> = scene["meshes"].as_array().unwrap().iter().map(|m| m["kind"].as_str().unwrap()).collect();
        assert!(kinds.contains(&"Wall") && kinds.contains(&"Door") && kinds.contains(&"Window") && kinds.contains(&"Glass"));

        // properties: instance rows edit through patch_instance, type rows through patch_family_type
        let d = call(&mut s, "describe", json!({ "id": d1 }));
        assert_eq!(d["target"]["method"], "patch_instance");
        let rows: Vec<&Value> = d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap()).collect();
        assert!(rows.iter().any(|r| r["label"] == "Sill Height" && r["kind"] == "number"));
        assert!(rows.iter().any(|r| r["label"] == "Width" && r["kind"] == "ro"), "type values are shown read only");
        call(&mut s, "patch_instance", json!({ "id": d1, "patch": { "mark": "D01" } }));
        let t = call(&mut s, "describe_type", json!({ "id": door }));
        assert_eq!(t["target"]["method"], "patch_family_type");
        assert!(s.handle("patch_family_type", &json!({ "id": door, "patch": { "values": { "Rough Width": 9000.0 } } })).is_err());
        let dup = call(&mut s, "duplicate_family_type", json!({ "id": door, "name": "Custom" }))["id"].as_u64().unwrap();
        call(&mut s, "patch_family_type", json!({ "id": dup, "patch": { "values": { "Width": 1000.0 } } }));

        // quantities: wall area is net of the openings, door and window schedules list the instances
        let sch = call(&mut s, "schedule", json!({}));
        let area: f64 = sch["walls"]["rows"][0][4].as_str().unwrap().parse().unwrap();
        // the door's hole follows its frame (reveal plus the frame's material)
        let (dw, dh, _) = match s.doc.get(d1) { Some(Element::FamilyInstance(i)) => s.doc.instance_opening_size(i), _ => panic!() };
        let expect = 6.0 * 2.7 - dw * dh / 1e6 - 1.2 * 1.2;
        assert!((area - expect).abs() < 0.011, "{area} vs {expect}");
        assert_eq!(sch["doors"]["count"], 1);
        assert_eq!(sch["doors"]["rows"][0][0], "D01");
        assert_eq!(sch["windows"]["count"], 1);

        // delete the wall: hosted instances go too, and undo brings everything back
        call(&mut s, "delete", json!({ "ids": [w] }));
        assert_eq!(call(&mut s, "schedule", json!({}))["doors"]["count"], 0);
        call(&mut s, "undo", json!({}));
        assert_eq!(call(&mut s, "schedule", json!({}))["doors"]["count"], 1);
    }

    #[test]
    fn object_styles_set_the_base_look_and_view_overrides_sit_on_top() {
        let (mut s, lvl, wt, view) = setup();
        let w = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700}))["id"].as_u64().unwrap();
        let style = |s: &mut Session| call(s, "plan", json!({"view_id": view}))["walls"][0]["style"].clone();
        // Walls: cut weight 3 is 0.35 mm at 1:100; Concrete 200 cuts with the Concrete material's pattern
        let st = style(&mut s);
        assert_eq!((st["line"]["weight"].as_u64(), st["line"]["width_mm"].as_f64(), st["fill"]["pattern"].as_str()), (Some(3), Some(0.35), Some("Concrete")));
        // the Object Style changes every wall; the view override still wins
        call(&mut s, "patch_object_styles", json!({"patch": {"model": {"Walls": {"cut_weight": 6, "color": [10, 20, 30], "pattern": "Dash"}}}}));
        let st = style(&mut s);
        assert_eq!((st["line"]["weight"].as_u64(), st["line"]["color"].as_str(), st["line"]["pattern"].as_str()), (Some(6), Some("#0a141e"), Some("Dash")));
        assert_eq!(st["line"]["width_mm"], 1.0);
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"overrides": {"model": {"Walls": {"cut_lines": {"weight": 2}}}}}}}));
        assert_eq!(style(&mut s)["line"]["weight"], 2);
        // a coarse view uses the wall type's coarse fill; the material pattern is for medium and fine
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"detail_level": "coarse"}}}));
        let st = style(&mut s);
        assert_eq!((st["fill"]["pattern"].as_str(), st["fill"]["color"].as_str()), (Some("Solid fill"), Some("#969ba2")));
        // line weights follow the view scale
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"scale": 500}}}));
        assert!(style(&mut s)["line"]["width_mm"].as_f64().unwrap() < 0.35);
        // changing a material changes the cut look of walls made of it
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"detail_level": "medium", "scale": 100}}}));
        let st = call(&mut s, "standards", json!({}));
        let concrete = st["materials"].as_array().unwrap().iter().find(|m| m["material"]["name"] == "Concrete").unwrap()["id"].as_u64().unwrap();
        call(&mut s, "patch_style_item", json!({"id": concrete, "patch": {"graphics": {"cut_foreground": {"pattern": "Horizontal", "color": [200, 0, 0]}}}}));
        let st = style(&mut s);
        assert_eq!((st["fill"]["pattern"].as_str(), st["fill"]["color"].as_str()), (Some("Horizontal"), Some("#c80000")));
        // errors come back as messages, and one undo reverts the last change
        assert!(s.handle("patch_object_styles", &json!({"patch": {"model": {"Walls": {"cut_weight": 40}}}})).is_err());
        call(&mut s, "undo", json!({}));
        assert_eq!(style(&mut s)["fill"]["pattern"], "Concrete");

        // subcategories: the door's swing arc has its own style
        let info = call(&mut s, "doc_info", json!({}));
        let door = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Single-Flush: 820 x 2040").unwrap()["id"].as_u64().unwrap();
        call(&mut s, "place_instance", json!({"type_id": door, "host_id": w, "point": [1500, 200]}));
        call(&mut s, "patch_object_styles", json!({"patch": {"model": {"Doors": {"subcategories": {"Plan Swing": {"color": [0, 128, 0], "projection_weight": 2}}}}}}));
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let swing = &plan["inserts"][0]["sub_styles"]["Plan Swing"]["line"];
        assert_eq!((swing["color"].as_str(), swing["weight"].as_u64()), (Some("#008000"), Some(2)));
        // pattern and hatch definitions travel with the plan for the canvas
        assert!(plan["line_patterns"]["Dash"].as_array().unwrap().len() >= 2);
        assert_eq!(plan["fill_patterns"]["Solid fill"]["solid"], true);
        assert!(!plan["fill_patterns"]["Diagonal crosshatch"]["lines"].as_array().unwrap().is_empty());
        // the dialogs' data
        let std = call(&mut s, "standards", json!({}));
        assert!(std["object_styles"]["model"]["Walls"].is_object() && std["line_weights"]["model"].as_array().unwrap().len() == 16);
        assert!(std["materials"].as_array().unwrap().iter().any(|m| !m["used_by"].as_array().unwrap().is_empty()));
    }

    #[test]
    fn dragging_a_wall_end_moves_only_that_wall_and_a_shared_grip_moves_both() {
        let (mut s, lvl, wt, _view) = setup();
        let mk = |s: &mut Session, a: [i64; 2], b: [i64; 2]| call(s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": a, "end": b }))["id"].as_u64().unwrap();
        let a = mk(&mut s, [0, 0], [4000, 0]);
        let b = mk(&mut s, [4000, 0], [4000, 3000]);
        call(&mut s, "move_wall_end", json!({ "id": a, "end": 1, "point": [5000, 500] }));
        let ends = |s: &mut Session, id: u64| -> Vec<f64> {
            let plan = call(s, "plan", json!({ "view_id": _view }));
            let w = plan["walls"].as_array().unwrap().iter().find(|w| w["id"] == id).unwrap().clone();
            vec![w["start"][0].as_f64().unwrap(), w["start"][1].as_f64().unwrap(), w["end"][0].as_f64().unwrap(), w["end"][1].as_f64().unwrap()]
        };
        assert_eq!(ends(&mut s, a), vec![0.0, 0.0, 5000.0, 500.0]);
        assert_eq!(ends(&mut s, b)[..2], [4000.0, 0.0], "the joined wall stays where it was");
        call(&mut s, "undo", json!({}));
        assert_eq!(ends(&mut s, a), vec![0.0, 0.0, 4000.0, 0.0]);
        // the shared grip at the corner moves both walls
        call(&mut s, "move_wall_ends", json!({ "ends": [{ "id": a, "end": 1 }, { "id": b, "end": 0 }], "point": [4500, 500] }));
        assert_eq!(ends(&mut s, a)[2..], [4500.0, 500.0]);
        assert_eq!(ends(&mut s, b)[..2], [4500.0, 500.0]);
        assert_eq!(ends(&mut s, b)[2..], [4000.0, 3000.0], "the far end of the second wall stays");
        call(&mut s, "undo", json!({}));
        assert!(s.handle("move_wall_end", &json!({ "id": a, "end": 1, "point": [0, 0] })).is_err(), "too short");
        call(&mut s, "pin", json!({ "ids": [a] }));
        assert!(s.handle("move_wall_end", &json!({ "id": a, "end": 1, "point": [5000, 0] })).is_err(), "pinned");
    }

    #[test]
    fn a_shape_of_walls_is_one_undo_step_and_joins_up() {
        let (mut s, lvl, wt, _view) = setup();
        let seg = |a: [i64; 2], b: [i64; 2]| json!([a, b]);
        let r = call(&mut s, "add_walls", json!({ "type_id": wt, "level_id": lvl, "segments": [seg([0, 0], [0, 3000]), seg([0, 3000], [4000, 3000]), seg([4000, 3000], [4000, 0]), seg([4000, 0], [0, 0])] }));
        assert_eq!(r["ids"].as_array().unwrap().len(), 4);
        let walls = |s: &mut Session| call(s, "plan", json!({"view_id": _view}))["walls"].as_array().unwrap().len();
        assert_eq!(walls(&mut s), 4);
        call(&mut s, "undo", json!({}));
        assert_eq!(walls(&mut s), 0, "the whole shape goes in one undo");
        assert!(s.handle("add_walls", &json!({ "type_id": wt, "level_id": lvl, "segments": [seg([0, 0], [0, 0])] })).is_err());
        assert!(s.handle("add_walls", &json!({ "type_id": wt, "level_id": lvl, "segments": [] })).is_err());
    }

    #[test]
    fn framed_door_symbol_changes_with_detail_level() {
        let (mut s, lvl, _wt, view) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let stud = info["wall_types"].as_array().unwrap().iter().find(|t| t["name"] == "Internal Stud 90").unwrap()["id"].as_u64().unwrap();
        let w = call(&mut s, "add_wall", json!({ "type_id": stud, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
        let door = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Single-Flush: 820 x 2040").unwrap()["id"].as_u64().unwrap();
        let placed = call(&mut s, "place_instance", json!({"type_id": door, "host_id": w, "point": [2000, 200]}));
        assert!(placed["warning"].is_null(), "a 112 back opening covers a 116 stud wall: {placed}");
        let id = placed["id"].as_u64().unwrap();
        // the door is a stored body: its frame comes through as parts, cut at Fine, outlined at Medium
        let frame_parts = |s: &mut Session, level: &str| -> Vec<bool> {
            call(s, "patch_view", json!({"id": view, "patch": {"settings": {"detail_level": level}}}));
            let plan = call(s, "plan", json!({"view_id": view}));
            plan["inserts"][0]["parts"].as_array().unwrap().iter().filter(|p| p["sub"] == "Frame/Mullion").map(|p| p["cut"].as_bool().unwrap()).collect()
        };
        assert!(frame_parts(&mut s, "coarse").is_empty());
        assert_eq!(frame_parts(&mut s, "medium"), vec![false, false], "one outline per jamb, not cut");
        let fine = frame_parts(&mut s, "fine");
        assert!(!fine.is_empty() && fine.iter().all(|c| *c), "the frame's material is cut at Fine: {fine:?}");
        // the leaf and swing are there at every level
        let swings = |plan: &Value| plan["inserts"][0]["parts"].as_array().unwrap().iter().filter(|p| p["sub"] == "Plan Swing").count();
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        assert_eq!(swings(&plan), 1);
        // a category override on Doors beats the view's level
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"detail_level": "coarse", "overrides": {"model": {"Doors": {"detail_level": "fine"}}}}}}));
        assert_eq!(frame_parts(&mut s, "coarse"), fine);
        // a shallow back opening is reported in Properties, and Frame Type is a drop-down
        call(&mut s, "patch_instance", json!({"id": id, "patch": {"values": {"Jamb Depth": 92.0}}}));
        let d = call(&mut s, "describe", json!({"id": id}));
        let rows: Vec<Value> = d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect();
        assert!(rows.iter().any(|r| r["label"] == "Frame Fit"));
        let ft = rows.iter().find(|r| r["label"] == "Frame Type").unwrap();
        assert_eq!(ft["kind"], "select");
        assert_eq!(ft["options"].as_array().unwrap().len(), 2);
        // the catalogue lists real sizes and adds a type
        let fam = info["families"].as_array().unwrap().iter().find(|f| f["name"] == "Single-Flush").unwrap();
        assert!(fam["catalog"].as_array().unwrap().iter().any(|n| n == "1020 x 2340"));
        call(&mut s, "add_catalog_type", json!({"family_id": fam["id"], "name": "1020 x 2340"}));
        assert!(s.handle("add_catalog_type", &json!({"family_id": fam["id"], "name": "1020 x 2340"})).is_err());
        assert!(s.handle("add_catalog_type", &json!({"family_id": fam["id"], "name": "999 x 999"})).is_err());
    }

    #[test]
    fn temporary_dimensions_over_the_api_edit_the_model_and_the_distance_row_is_gone() {
        let (mut s, lvl, wt, _view) = setup();
        let mk = |s: &mut Session, a: [i64; 2], b: [i64; 2]| call(s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": a, "end": b, "height_mm": 2700 }))["id"].as_u64().unwrap();
        let w = mk(&mut s, [0, 0], [6000, 0]);
        let _n = mk(&mut s, [0, 4000], [6000, 4000]);
        let dims = call(&mut s, "temp_dimensions", json!({ "id": w }))["dims"].as_array().unwrap().clone();
        let up = dims.iter().find(|d| d["key"] == "up").unwrap().clone();
        assert_eq!(up["value"].as_f64().unwrap(), 4000.0);
        call(&mut s, "set_temp_dimension", json!({ "edit": up["edit"], "current": 4000, "value": 3500 }));
        let after = call(&mut s, "temp_dimensions", json!({ "id": w }))["dims"].as_array().unwrap().clone();
        assert_eq!(after.iter().find(|d| d["key"] == "up").unwrap()["value"].as_f64().unwrap(), 3500.0);
        // faces setting
        let faces = call(&mut s, "temp_dimensions", json!({ "id": w, "settings": { "wall_ref": "faces" } }))["dims"].as_array().unwrap().clone();
        assert!(faces.iter().find(|d| d["key"] == "up").unwrap()["value"].as_f64().unwrap() < 3500.0);
        // a door: dimensions while placing (no edit) and once placed (editable), with no Distance from Start row
        let info = call(&mut s, "doc_info", json!({}));
        let door = info["family_types"].as_array().unwrap().iter().find(|t| t["label"] == "Single-Flush: 820 x 2040").unwrap()["id"].as_u64().unwrap();
        let pre = call(&mut s, "temp_dimensions", json!({ "type_id": door, "host_id": w, "point": [2000, 100] }))["dims"].as_array().unwrap().clone();
        assert!(pre.iter().any(|d| d["key"] == "left") && pre.iter().all(|d| d["edit"].is_null()));
        let d = call(&mut s, "place_instance", json!({ "type_id": door, "host_id": w, "point": [2000, 100] }))["id"].as_u64().unwrap();
        let desc = call(&mut s, "describe", json!({ "id": d }));
        assert!(!desc.to_string().contains("Distance from Start"));
        let placed = call(&mut s, "temp_dimensions", json!({ "id": d }))["dims"].as_array().unwrap().clone();
        assert!(!placed.iter().find(|d| d["key"] == "left").unwrap()["edit"].is_null());
    }

    #[test]
    fn several_selected_elements_share_rows_change_type_together_and_undo_as_one_step() {
        let (mut s, lvl, wt, _view) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let other = info["wall_types"].as_array().unwrap().iter().find(|t| t["id"].as_u64().unwrap() != wt).unwrap()["id"].as_u64().unwrap();
        let a = call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0], "height_mm": 2700}))["id"].as_u64().unwrap();
        let b = call(&mut s, "add_wall", json!({"type_id": other, "level_id": lvl, "start": [0, 3000], "end": [4000, 3000], "height_mm": 3000}))["id"].as_u64().unwrap();
        let row = |d: &Value, label: &str| d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).find(|r| r["label"] == label);
        let d = call(&mut s, "describe_multi", json!({"ids": [a, b]}));
        assert_eq!(d["title"], "Walls (2)");
        assert_eq!(d["type_kind"], "wall");
        assert!(d["type_id"].is_null(), "different types");
        // heights differ, so the row varies, and one edit sets both
        let h = row(&d, "Unconnected Height").unwrap();
        assert_eq!(h["varies"], true);
        let target = d["target"].clone();
        assert_eq!(target["method"], "patch_multi");
        call(&mut s, "patch_multi", json!({"ids": [a, b], "patch": {"height_mm": 3300}}));
        let d = call(&mut s, "describe_multi", json!({"ids": [a, b]}));
        assert!(row(&d, "Unconnected Height").unwrap().get("varies").is_none());
        assert_eq!(row(&d, "Unconnected Height").unwrap()["raw"], 3300.0);
        // one type for all
        call(&mut s, "set_type_multi", json!({"ids": [a, b], "type_id": wt}));
        let d = call(&mut s, "describe_multi", json!({"ids": [a, b]}));
        assert_eq!(d["type_id"], wt);
        // two steps, each undone in one go
        call(&mut s, "undo", json!({}));
        assert!(call(&mut s, "describe_multi", json!({"ids": [a, b]}))["type_id"].is_null());
        call(&mut s, "undo", json!({}));
        let h = call(&mut s, "describe", json!({"id": b}));
        assert_eq!(row(&h, "Unconnected Height").unwrap()["raw"], 3000.0);
        // a failing patch changes nothing
        assert!(s.handle("patch_multi", &json!({"ids": [a, b], "patch": {"base_level": 99999}})).is_err());
        assert_eq!(row(&call(&mut s, "describe", json!({"id": a})), "Unconnected Height").unwrap()["raw"], 2700.0);
        // a wall and a floor share only what both have
        let ft = info["floor_types"][0]["id"].as_u64().unwrap();
        let f = call(&mut s, "add_floor", json!({"type_id": ft, "level_id": lvl, "points": [[0, 0], [3000, 0], [3000, 3000], [0, 3000]]}))["id"].as_u64().unwrap();
        let d = call(&mut s, "describe_multi", json!({"ids": [a, f]}));
        assert_eq!(d["title"], "Multiple Categories (2)");
        assert!(d["type_kind"].is_null());
    }

    #[test]
    fn a_new_project_has_a_site_plan_and_four_elevations_and_levels_and_points_edit_through_the_palette() {
        let (mut s, lvl, wt, _view) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let names: Vec<String> = info["views"].as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap().to_string()).collect();
        for n in ["Site", "North", "South", "East", "West"] {
            assert!(names.iter().any(|x| x == n), "missing view {n}");
        }
        let site = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Site").unwrap()["id"].as_u64().unwrap();
        let plan = call(&mut s, "plan", json!({"view_id": site}));
        assert_eq!(plan["markers"].as_array().unwrap().len(), 4);
        for m in plan["markers"].as_array().unwrap() {
            assert_eq!(m["slots"].as_array().unwrap().iter().filter(|x| !x["view_id"].is_null()).count(), 1);
        }
        assert_eq!(plan["site_points"]["base"]["id"], crate::SITE_BASE_ID);
        // every other plan hides both points until its Visibility/Graphics turns them on, one at a time
        let ground = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "Ground").unwrap()["id"].as_u64().unwrap();
        assert!(call(&mut s, "plan", json!({"view_id": ground}))["site_points"].is_null());
        call(&mut s, "patch_view", json!({"id": ground, "patch": {"settings": {"overrides": {"model": {"Survey Point": {"visible": true}}}}}}));
        let gp = call(&mut s, "plan", json!({"view_id": ground}));
        assert!(gp["site_points"]["base"].is_null() && gp["site_points"]["survey"]["id"] == crate::SITE_SURVEY_ID);
        // an elevation draws a wall in front of it
        call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700}));
        let north = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "North").unwrap()["id"].as_u64().unwrap();
        let e = call(&mut s, "elevation", json!({"view_id": north}));
        assert!(!e["items"].as_array().unwrap().is_empty());
        assert!(e["levels"].as_array().unwrap().len() >= 2);
        // levels: add, rename through the palette, refuse a duplicate name
        let l3 = call(&mut s, "add_level", json!({"elevation": 6000.0}))["id"].as_u64().unwrap();
        let d = call(&mut s, "describe", json!({"id": l3}));
        assert_eq!(d["kind"], "Level");
        call(&mut s, "patch_level", json!({"id": l3, "patch": {"name": "Roof"}}));
        assert!(s.handle("patch_level", &json!({"id": l3, "patch": {"name": "Level 1"}})).is_err());
        assert!(call(&mut s, "doc_info", json!({}))["levels"].as_array().unwrap().iter().any(|l| l["name"] == "Roof"));
        call(&mut s, "delete_level", json!({"id": l3}));
        // the base point reads shared coordinates; typing new ones moves the shared origin, not the point
        let d = call(&mut s, "describe", json!({"id": crate::SITE_BASE_ID}));
        assert_eq!(d["kind"], "SitePoint");
        call(&mut s, "patch_project_location", json!({"patch": {"base_shared_x": 5000.0, "base_shared_y": 7000.0, "angle_true_north_deg": 12.0}}));
        let d = call(&mut s, "describe", json!({"id": crate::SITE_BASE_ID}));
        let rows: Vec<Value> = d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect();
        let raw = |l: &str| rows.iter().find(|r| r["label"] == l).unwrap()["raw"].as_f64().unwrap();
        assert_eq!(raw("E/W"), 5000.0);
        assert_eq!(raw("N/S"), 7000.0);
        assert_eq!(raw("Angle to True North"), 12.0);
        let plan = call(&mut s, "plan", json!({"view_id": site}));
        assert_eq!(plan["site_points"]["base"]["x"], 0.0);
        // pinned points refuse to move; undo walks back
        call(&mut s, "patch_project_location", json!({"patch": {"base_pinned": true}}));
        assert!(s.handle("move_site_point", &json!({"which": "base", "point": [100.0, 100.0]})).is_err());
        call(&mut s, "patch_project_location", json!({"patch": {"base_pinned": false}}));
        call(&mut s, "move_site_point", json!({"which": "base", "point": [100.0, 100.0]}));
        let plan = call(&mut s, "plan", json!({"view_id": site}));
        assert_eq!(plan["site_points"]["base"]["x"], 100.0);
        // an unclipped survey point moves by typing shared coordinates; a clipped one refuses
        assert!(s.handle("patch_project_location", &json!({"patch": {"survey_shared_x": 10.0}})).is_err());
        call(&mut s, "patch_project_location", json!({"patch": {"survey_clipped": false}}));
        call(&mut s, "patch_project_location", json!({"patch": {"survey_shared_x": 2500.0}}));
        let plan = call(&mut s, "plan", json!({"view_id": site}));
        assert_eq!(plan["site_points"]["survey"]["x"], -2500.0); // shared origin x is -5000 after the base point edit above
        // marker: turn a pointer off through the palette, then rotate it
        let m = call(&mut s, "add_elevation_marker", json!({"level_id": lvl, "point": [3000.0, -9000.0], "slots": [true, true, true, true]}))["id"].as_u64().unwrap();
        let find = |plan: &Value| plan["markers"].as_array().unwrap().iter().find(|x| x["id"] == m).unwrap().clone();
        let plan = call(&mut s, "plan", json!({"view_id": site}));
        assert!(!find(&plan)["slots"][2]["view_id"].is_null());
        call(&mut s, "patch_marker", json!({"id": m, "patch": {"slot_on": {"2": false}}}));
        let plan = call(&mut s, "plan", json!({"view_id": site}));
        assert!(find(&plan)["slots"][2]["view_id"].is_null());
        call(&mut s, "patch_marker", json!({"id": m, "patch": {"rotation_deg": 30.0}}));
        let plan = call(&mut s, "plan", json!({"view_id": site}));
        assert_eq!(find(&plan)["rotation_deg"], 30.0);
    }

    #[test]
    fn level_and_grid_ends_lock_through_the_api() {
        let (mut s, _lvl, _wt, _view) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let north = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "North").unwrap()["id"].as_u64().unwrap();
        let levels: Vec<u64> = info["levels"].as_array().unwrap().iter().map(|l| l["id"].as_u64().unwrap()).collect();
        let (l1, l2) = (levels[0], levels[1]);
        let ext = json!([[-2000.0, 0.0], [9000.0, 0.0]]);
        // the first drag of a level that fits the view gives it an extent from the view's ends
        call(&mut s, "set_level_end", json!({"id": l1, "end": 1, "point": [10000.0, 0.0], "extent": ext}));
        call(&mut s, "set_level_end", json!({"id": l2, "end": 1, "point": [10000.0, 0.0], "extent": ext}));
        call(&mut s, "lock_ends", json!({"a": {"id": l1, "end": 1}, "b": {"id": l2, "end": 1}}));
        call(&mut s, "set_level_end", json!({"id": l1, "end": 1, "point": [12000.0, 0.0]}));
        let e = call(&mut s, "elevation", json!({"view_id": north}));
        let line = |id: u64| e["levels"].as_array().unwrap().iter().find(|l| l["id"] == id).unwrap().clone();
        assert_eq!(line(l1)["u0"].as_f64().unwrap().abs(), line(l2)["u0"].as_f64().unwrap().abs());
        assert_eq!(line(l1)["locks"][1][0]["id"], l2);
        call(&mut s, "unlock_ends", json!({"a": {"id": l1, "end": 1}, "b": {"id": l2, "end": 1}}));
        assert!(call(&mut s, "elevation", json!({"view_id": north}))["levels"].as_array().unwrap().iter().all(|l| l["locks"][1].as_array().unwrap().is_empty()));
    }

    #[test]
    fn deleting_markers_views_and_levels_and_pinning_them() {
        let (mut s, lvl, _wt, _view) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let view_named = |info: &Value, n: &str| info["views"].as_array().unwrap().iter().find(|v| v["name"] == n).map(|v| v["id"].as_u64().unwrap());
        // deleting one elevation view turns that pointer off; the other three stay
        let north = view_named(&info, "North").unwrap();
        call(&mut s, "delete", json!({"ids": [north]}));
        let after = call(&mut s, "doc_info", json!({}));
        assert!(view_named(&after, "North").is_none());
        assert!(view_named(&after, "South").is_some());
        // one undo brings it back
        call(&mut s, "undo", json!({}));
        assert!(view_named(&call(&mut s, "doc_info", json!({})), "North").is_some());
        // a marker goes with all its views, and pinning it blocks that
        let m = call(&mut s, "add_elevation_marker", json!({"level_id": lvl, "point": [0.0, -8000.0], "slots": [true, true, false, false]}))["id"].as_u64().unwrap();
        call(&mut s, "pin", json!({"ids": [m], "pinned": true}));
        assert!(s.handle("delete", &json!({"ids": [m]})).is_err());
        assert!(s.handle("patch_marker", &json!({"id": m, "patch": {"rotation_deg": 90.0}})).is_err());
        call(&mut s, "pin", json!({"ids": [m], "pinned": false}));
        let n_views = call(&mut s, "doc_info", json!({}))["views"].as_array().unwrap().len();
        call(&mut s, "delete", json!({"ids": [m]}));
        assert_eq!(call(&mut s, "doc_info", json!({}))["views"].as_array().unwrap().len(), n_views - 2);
        // base and survey points pin through the same command and cannot be deleted
        call(&mut s, "pin", json!({"ids": [crate::SITE_BASE_ID], "pinned": true}));
        assert!(s.handle("move_site_point", &json!({"which": "base", "point": [5.0, 5.0]})).is_err());
        assert!(s.handle("delete", &json!({"ids": [crate::SITE_BASE_ID]})).is_err());
        // a pinned level cannot move or go
        let l2 = call(&mut s, "add_level", json!({"elevation": 3000.0}))["id"].as_u64().unwrap();
        call(&mut s, "pin", json!({"ids": [l2], "pinned": true}));
        assert!(s.handle("patch_level", &json!({"id": l2, "patch": {"display_elevation_mm": 3300.0}})).is_err());
        assert!(s.handle("delete", &json!({"ids": [l2]})).is_err());
        call(&mut s, "pin", json!({"ids": [l2], "pinned": false}));
        call(&mut s, "delete", json!({"ids": [l2]}));
    }

    #[test]
    fn grids_draw_in_plan_and_elevation_and_edit_through_the_palette() {
        let (mut s, lvl, wt, view) = setup();
        let line = |a: [f64; 2], b: [f64; 2]| json!({"type": "line", "a": {"x": a[0], "y": a[1]}, "b": {"x": b[0], "y": b[1]}});
        let g1 = call(&mut s, "add_grid", json!({"curves": [line([0.0, -1000.0], [0.0, 9000.0])]}))["id"].as_u64().unwrap();
        let g2 = call(&mut s, "add_grid", json!({"curves": [line([6000.0, -1000.0], [6000.0, 9000.0])]}))["id"].as_u64().unwrap();
        call(&mut s, "add_wall", json!({"type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700}));
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let grids = plan["grids"].as_array().unwrap();
        assert_eq!(grids.len(), 2);
        assert_eq!(grids[0]["name"], "1");
        assert_eq!(grids[1]["name"], "2");
        assert_eq!(grids[0]["path"].as_array().unwrap().len(), 2);
        assert_eq!(grids[0]["style"]["line"]["pattern"], "Centre");
        // properties: name, type selector, length
        let d = call(&mut s, "describe", json!({"id": g1}));
        assert_eq!(d["kind"], "Grid");
        assert!(d["type_id"].is_u64());
        let info = call(&mut s, "doc_info", json!({}));
        assert_eq!(info["grid_types"].as_array().unwrap().len(), 1);
        call(&mut s, "patch_grid", json!({"id": g1, "patch": {"name": "A"}}));
        assert!(s.handle("patch_grid", &json!({"id": g2, "patch": {"name": "A"}})).is_err());
        // type properties edit the head size and the default heads
        let t = info["grid_types"][0]["id"].as_u64().unwrap();
        let td = call(&mut s, "describe_grid_type", json!({"id": t}));
        assert_eq!(td["kind"], "GridType");
        call(&mut s, "patch_grid_type", json!({"id": t, "patch": {"bubble_dia_mm": 10.0, "bubble_default": {"0": true}}}));
        let g3 = call(&mut s, "add_grid", json!({"curves": [line([9000.0, 0.0], [9000.0, 8000.0])]}))["id"].as_u64().unwrap();
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let new = plan["grids"].as_array().unwrap().iter().find(|g| g["id"] == g3).unwrap();
        assert_eq!(new["bubbles"], json!([true, true]));
        assert_eq!(new["bubble_dia_mm"], 10.0);
        let dup = call(&mut s, "duplicate_grid_type", json!({"id": t, "name": "Grid 12mm Bubble"}))["id"].as_u64().unwrap();
        call(&mut s, "patch_grid", json!({"id": g3, "patch": {"grid_type": dup}}));
        // several grids edit together
        call(&mut s, "patch_multi", json!({"ids": [g1, g2], "patch": {"pinned": true}}));
        assert!(s.handle("set_grid_end", &json!({"id": g1, "end": 1, "point": [0.0, 12000.0]})).is_err());
        call(&mut s, "patch_multi", json!({"ids": [g1, g2], "patch": {"pinned": false}}));
        call(&mut s, "set_grid_end", json!({"id": g1, "end": 1, "point": [0.0, 12000.0]}));
        let m = call(&mut s, "describe_multi", json!({"ids": [g1, g2]}));
        assert_eq!(m["title"], "Grids (2)");
        assert_eq!(m["type_kind"], "grid");
        // pick lines: a grid along the wall, offset to the north
        let w = call(&mut s, "plan", json!({"view_id": view}))["walls"][0]["id"].as_u64().unwrap();
        let g4 = call(&mut s, "grid_from_line", json!({"source": w, "offset": 2000.0, "point": [3000.0, 500.0]}))["id"].as_u64().unwrap();
        let plan = call(&mut s, "plan", json!({"view_id": view}));
        let p4 = plan["grids"].as_array().unwrap().iter().find(|g| g["id"] == g4).unwrap()["path"].clone();
        assert_eq!(p4[0][1], 2000.0);
        // elevations show the north-south grids
        let north = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "North").unwrap()["id"].as_u64().unwrap();
        let e = call(&mut s, "elevation", json!({"view_id": north}));
        assert_eq!(e["grids"].as_array().unwrap().len(), 3);
        // hide the Grids category in a view: they leave the plan
        call(&mut s, "patch_view", json!({"id": view, "patch": {"settings": {"overrides": {"annotation": {"Grids": {"visible": false}}}}}}));
        assert!(call(&mut s, "plan", json!({"view_id": view}))["grids"].as_array().unwrap().is_empty());
        // delete and undo
        call(&mut s, "delete", json!({"ids": [g4]}));
        call(&mut s, "undo", json!({}));
        assert!(call(&mut s, "describe", json!({"id": g4}))["kind"] == "Grid");
    }

    #[test]
    fn roofs_draw_in_plan_3d_and_elevation_and_edit_through_the_palette() {
        let (mut s, _lvl, wt, _view) = setup();
        let info = call(&mut s, "doc_info", json!({}));
        let l2 = info["levels"][1]["id"].as_u64().unwrap();
        let rt = info["roof_types"][0]["id"].as_u64().unwrap();
        assert!(info["roof_types"].as_array().unwrap().len() >= 3);
        let line = |a: [f64; 2], b: [f64; 2]| json!({ "curve": {"type": "line", "a": {"x": a[0], "y": a[1]}, "b": {"x": b[0], "y": b[1]}}, "defines_slope": true, "slope_deg": 22.5, "overhang_mm": 0.0 });
        let edges = vec![line([0.0, 0.0], [10000.0, 0.0]), line([10000.0, 0.0], [10000.0, 6000.0]), line([10000.0, 6000.0], [0.0, 6000.0]), line([0.0, 6000.0], [0.0, 0.0])];
        // a wall for the roof to sit on, so the model has something under it
        call(&mut s, "add_wall", json!({"type_id": wt, "level_id": l2, "start": [0, 0], "end": [10000, 0], "height_mm": 2700}));
        let id = call(&mut s, "add_roof_sketch", json!({"type_id": rt, "level_id": l2, "edges": edges}))["id"].as_u64().unwrap();
        // plan: the outline, hips and ridge, and a slope symbol on each edge
        let views = call(&mut s, "doc_info", json!({}))["views"].clone();
        let v2 = views.as_array().unwrap().iter().find(|v| v["name"] == "Level 1").unwrap()["id"].as_u64().unwrap();
        let plan = call(&mut s, "plan", json!({"view_id": v2}));
        let roofs = plan["roofs"].as_array().unwrap();
        assert_eq!(roofs.len(), 1);
        assert_eq!(roofs[0]["ridges"].as_array().unwrap().len(), 5);
        assert_eq!(roofs[0]["marks"].as_array().unwrap().len(), 4);
        assert_eq!(roofs[0]["rings"].as_array().unwrap().len(), 1);
        // 3D: the roof has a mesh
        let scene = call(&mut s, "scene", json!({}));
        assert!(scene["meshes"].as_array().unwrap().iter().any(|m| m["id"] == id && m["kind"] == "Roof"));
        // properties
        let d = call(&mut s, "describe", json!({"id": id}));
        assert_eq!(d["kind"], "Roof");
        assert_eq!(d["target"]["method"], "patch_roof");
        let row = |label: &str| d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().iter()).find(|r| r["label"] == label).unwrap().clone();
        assert_eq!(row("Slope (degrees)")["raw"], 22.5);
        assert_eq!(row("Base Level")["value"], "Level 1");
        call(&mut s, "patch_roof", json!({"id": id, "patch": {"slope_deg": 35.0, "base_offset_mm": 100.0}}));
        let d = call(&mut s, "describe", json!({"id": id}));
        let slope = d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().iter()).find(|r| r["label"] == "Slope (degrees)").unwrap().clone();
        assert_eq!(slope["raw"], 35.0);
        // the footprint comes back as a sketch with the edge settings, and edits as one
        let sk = call(&mut s, "roof_sketch", json!({"id": id}));
        assert_eq!(sk["curves"].as_array().unwrap().len(), 4);
        assert_eq!(sk["curves"][0]["attr"]["slope_deg"], 35.0);
        let mut edges: Vec<Value> = sk["curves"].as_array().unwrap().iter().map(|c| json!({"curve": c["curve"], "defines_slope": c["attr"]["defines_slope"], "slope_deg": c["attr"]["slope_deg"], "overhang_mm": 0.0})).collect();
        edges[1]["defines_slope"] = json!(false);
        edges[3]["defines_slope"] = json!(false);
        call(&mut s, "set_roof_sketch", json!({"id": id, "edges": edges}));
        let plan = call(&mut s, "plan", json!({"view_id": v2}));
        assert_eq!(plan["roofs"][0]["marks"].as_array().unwrap().len(), 2);
        // a roof shows in elevations, from the east end the gable is face on
        let north = call(&mut s, "doc_info", json!({}))["views"].as_array().unwrap().iter().find(|v| v["name"] == "North").unwrap()["id"].as_u64().unwrap();
        let e = call(&mut s, "elevation", json!({"view_id": north}));
        assert!(e["items"].as_array().unwrap().iter().any(|i| i["category"] == "Roofs"));
        // the roof type changes, alone or with others
        let rt2 = call(&mut s, "doc_info", json!({}))["roof_types"][1]["id"].as_u64().unwrap();
        call(&mut s, "set_type_multi", json!({"ids": [id], "type_id": rt2}));
        let m = call(&mut s, "describe_multi", json!({"ids": [id]}));
        assert_eq!(m["type_kind"], "roof");
        // pin, move, delete and undo
        call(&mut s, "pin", json!({"ids": [id], "pinned": true}));
        assert!(s.handle("delete", &json!({"ids": [id]})).is_err());
        call(&mut s, "pin", json!({"ids": [id], "pinned": false}));
        call(&mut s, "transform", json!({"ids": [id], "kind": "move", "dx": 1000.0, "dy": 0.0}));
        let moved = call(&mut s, "plan", json!({"view_id": v2}));
        assert_eq!(moved["roofs"][0]["points"][0][0], 1000.0);
        call(&mut s, "delete", json!({"ids": [id]}));
        assert!(call(&mut s, "plan", json!({"view_id": v2}))["roofs"].as_array().unwrap().is_empty());
        call(&mut s, "undo", json!({}));
        assert_eq!(call(&mut s, "plan", json!({"view_id": v2}))["roofs"].as_array().unwrap().len(), 1);
        // hiding the Roofs category takes it out of plan and 3D
        call(&mut s, "patch_view", json!({"id": v2, "patch": {"settings": {"overrides": {"model": {"Roofs": {"visible": false}}}}}}));
        assert!(call(&mut s, "plan", json!({"view_id": v2}))["roofs"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_custom_door_family_is_made_edited_placed_and_exported_through_the_api() {
        let mut s = Session::new();
        let fid = call(&mut s, "new_family", json!({ "name": "Cottage Door", "category": "doors", "host": "wall" }))["id"].as_u64().unwrap();
        let def = call(&mut s, "family_def", json!({ "id": fid }));
        assert_eq!(def["types"].as_array().unwrap().len(), 1);
        let tid = def["types"][0]["id"].as_u64().unwrap();
        let mut body = def["body"].clone();
        let planes = body["planes"].as_array_mut().unwrap();
        planes.push(json!({ "id": 20, "name": "Left", "axis": "x", "anchor": { "type": "offset", "from": 1, "by": "-[Width]/2" } }));
        planes.push(json!({ "id": 21, "name": "Right", "axis": "x", "anchor": { "type": "offset", "from": 1, "by": "[Width]/2" } }));
        planes.push(json!({ "id": 22, "name": "Top", "axis": "z", "anchor": { "type": "offset", "from": 3, "by": "[Height]" } }));
        let rect = json!([{ "nodes": [
            { "u": { "plane": 20 }, "v": { "plane": 3 } }, { "u": { "plane": 21 }, "v": { "plane": 3 } },
            { "u": { "plane": 21 }, "v": { "plane": 22 } }, { "u": { "plane": 20 }, "v": { "plane": 22 } } ] }]);
        body["forms"] = json!([
            { "id": 30, "name": "Leaf", "shape": { "type": "extrusion", "plane": 2, "start": "0", "end": "35", "profile": rect }, "material": "Timber", "subcategory": "Panel" },
            { "id": 31, "name": "Opening", "void": true, "shape": { "type": "extrusion", "plane": 2, "start": "-120", "end": "120", "profile": rect } }
        ]);
        // the editor previews an unsaved body, flexed to another size
        let pv = call(&mut s, "family_preview", json!({ "id": fid, "type": tid, "body": body, "values": { "Width": 1000.0 } }));
        assert!(pv.get("error").is_none(), "{pv}");
        assert_eq!(pv["cut_box"][0][0], -500.0);
        assert_eq!(pv["cut_box"][1][2], 2040.0);
        assert_eq!(pv["views"]["front"].as_array().unwrap().len(), 2);
        // a body with a bad reference reports instead of failing
        let mut bad = body.clone();
        bad["forms"][0]["shape"]["plane"] = json!(99);
        assert!(call(&mut s, "family_preview", json!({ "id": fid, "body": bad }))["error"].is_string());
        // save it, place it in a wall and read the opening from the plan
        call(&mut s, "set_family_def", json!({ "id": fid, "body": body }));
        let info = call(&mut s, "doc_info", json!({}));
        let (lvl, wt) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][0]["id"].as_u64().unwrap());
        let w = call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 0], "end": [6000, 0], "height_mm": 2700 }))["id"].as_u64().unwrap();
        let inst = call(&mut s, "place_instance", json!({ "type_id": tid, "host_id": w, "point": [3000, 0] }))["id"].as_u64().unwrap();
        let view = call(&mut s, "plan", json!({ "level_id": lvl }));
        let ins = view["inserts"].as_array().unwrap().iter().find(|i| i["id"] == inst).unwrap();
        assert!(ins["parts"].as_array().unwrap().iter().any(|p| p["sub"] == "Panel" && p["cut"] == true), "{ins}");
        // export and load back under another name
        let mut file = call(&mut s, "export_family", json!({ "id": fid }));
        assert_eq!(file["openarch_family"], 1);
        assert!(s.handle("import_family", &json!({ "file": file.clone() })).is_err(), "same name is refused");
        file["family"]["name"] = json!("Cottage Door 2");
        let id2 = call(&mut s, "import_family", json!({ "file": file }))["id"].as_u64().unwrap();
        assert!(call(&mut s, "family_def", json!({ "id": id2 }))["body"]["forms"].as_array().unwrap().len() == 2);
        // a file written before the rename carries the old format marker and still loads
        let mut old = call(&mut s, "export_family", json!({ "id": fid }));
        let n = old.as_object_mut().unwrap();
        let marker = n.remove("openarch_family").unwrap();
        n.insert("ironbark_family".into(), marker);
        old["family"]["name"] = json!("Cottage Door 3");
        assert!(s.handle("import_family", &json!({ "file": old })).is_ok());
    }

    #[test]
    fn a_family_sketch_turns_into_loops_with_its_locks_and_back() {
        let mut s = Session::new();
        let planes = json!({ "1": 0.0, "2": 0.0, "3": 0.0, "50": 300.0 });
        let line = |a: [f64; 2], b: [f64; 2]| json!({ "type": "line", "a": { "x": a[0], "y": a[1] }, "b": { "x": b[0], "y": b[1] } });
        let curves = json!([line([0.0, 0.0], [300.0, 0.0]), line([300.0, 0.0], [300.0, 200.0]), line([300.0, 200.0], [0.0, 200.0]), line([0.0, 200.0], [0.0, 0.0])]);
        let r = call(&mut s, "family_sketch_loops", json!({ "curves": curves, "locks": [{ "curve": 1, "plane": 50, "coord": "u", "on": true }], "planes": planes }));
        assert_eq!(r["ok"], true);
        let nodes = r["loops"][0]["nodes"].as_array().unwrap();
        assert_eq!(nodes.iter().filter(|n| n["u"]["plane"] == 50).count(), 2);
        let back = call(&mut s, "family_sketch_of", json!({ "loops": r["loops"], "planes": planes }));
        assert_eq!(back["curves"].as_array().unwrap().len(), 4);
        assert_eq!(back["locks"].as_array().unwrap().len(), 1);
        // an open sketch is refused with the place it is open
        let open = call(&mut s, "family_sketch_loops", json!({ "curves": [line([0.0, 0.0], [300.0, 0.0]), line([300.0, 0.0], [300.0, 200.0])], "planes": planes }));
        assert_eq!(open["ok"], false);
        assert!(open["error"]["message"].as_str().unwrap().len() > 3);
    }

    #[test]
    fn wall_floor_and_roof_types_edit_in_type_properties_and_cancel_drops_the_edits() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let wt = info["wall_types"].as_array().unwrap().iter().find(|t| t["name"] == "Brick Veneer").unwrap()["id"].as_u64().unwrap();
        let d = call(&mut s, "describe_any_type", json!({ "id": wt }));
        assert_eq!((d["family"].as_str(), d["patch"].as_str()), (Some("Basic Wall"), Some("patch_system_type")));
        let rows: Vec<&str> = d["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap())).collect();
        for want in ["Structure", "Wrapping at Inserts", "Wrapping at Ends", "Width", "Function", "Coarse Scale Fill Pattern", "Thermal Resistance (R)", "Thermal Mass", "Type Mark", "Fire Rating"] {
            assert!(rows.contains(&want), "{want} missing from {rows:?}");
        }
        assert!(d["values"]["r_m2k_w"].as_f64().unwrap() > 0.5);
        // the preview of a wrapped assembly: the face brick turns into the opening
        let mut layers = d["layers"].clone();
        let flat = call(&mut s, "assembly_info", json!({ "kind": "wall", "layers": layers, "wrap_inserts": "none" }));
        let wrapped = call(&mut s, "assembly_info", json!({ "kind": "wall", "layers": layers, "wrap_inserts": "exterior" }));
        assert!(flat["error"].is_null() && flat["plan"]["layers"].as_array().unwrap().len() == 4);
        let pts = |v: &Value| v["plan"]["layers"][0]["shapes"].to_string().len();
        assert_ne!(pts(&flat), pts(&wrapped), "wrapping changes the outer leaf");
        layers[0]["thickness_mm"] = json!(0.0);
        assert!(call(&mut s, "assembly_info", json!({ "kind": "wall", "layers": layers }))["error"].as_str().unwrap().contains("membrane"));
        // edits in a session: Cancel puts everything back, including a duplicate and a rename
        let undo_before = call(&mut s, "doc_info", json!({}))["undo"].clone();
        let mark = call(&mut s, "edit_begin", json!({}))["mark"].clone();
        call(&mut s, "patch_system_type", json!({ "id": wt, "patch": { "info": { "type_mark": "W1" }, "wrap_ends": "interior" } }));
        let dup = call(&mut s, "duplicate_system_type", json!({ "id": wt, "name": "Brick Veneer 2" }))["id"].as_u64().unwrap();
        call(&mut s, "rename_type", json!({ "id": dup, "name": "Brick Veneer Custom" }));
        call(&mut s, "edit_cancel", json!({ "mark": mark }));
        let info = call(&mut s, "doc_info", json!({}));
        assert!(!info["wall_types"].as_array().unwrap().iter().any(|t| t["name"] == "Brick Veneer Custom"));
        assert_eq!(info["undo"], undo_before);
        assert_eq!(call(&mut s, "describe_any_type", json!({ "id": wt }))["wall"]["wrap_ends"], "none");
        // Apply then OK: one undo step per commit
        let mark = call(&mut s, "edit_begin", json!({}))["mark"].clone();
        call(&mut s, "patch_system_type", json!({ "id": wt, "patch": { "info": { "type_mark": "W1" } } }));
        call(&mut s, "patch_system_type", json!({ "id": wt, "patch": { "function": "interior" } }));
        call(&mut s, "edit_commit", json!({ "mark": mark, "name": "Type Properties" }));
        assert_eq!(call(&mut s, "doc_info", json!({}))["undo"], "Type Properties");
        call(&mut s, "undo", json!({}));
        assert_eq!(call(&mut s, "describe_any_type", json!({ "id": wt }))["groups"][4]["rows"][8]["value"], "");
        // floors and roofs
        let ft = info["floor_types"][0]["id"].as_u64().unwrap();
        let fd = call(&mut s, "describe_any_type", json!({ "id": ft }));
        assert_eq!(fd["family"], "Floor");
        let rt = info["roof_types"][0]["id"].as_u64().unwrap();
        assert_eq!(call(&mut s, "describe_any_type", json!({ "id": rt }))["family"], "Basic Roof");
        // every kind of type answers the one describe call
        let lt = info["level_types"][0]["id"].as_u64().unwrap();
        assert_eq!(call(&mut s, "describe_any_type", json!({ "id": lt }))["patch"], "patch_level_type");
        let ty = info["family_types"][0]["id"].as_u64().unwrap();
        assert_eq!(call(&mut s, "describe_any_type", json!({ "id": ty }))["duplicate"], "duplicate_family_type");
    }

    #[test]
    fn components_place_free_and_plans_are_made_on_request() {
        let mut s = Session::new();
        let info = call(&mut s, "doc_info", json!({}));
        let lvl = info["levels"][0]["id"].as_u64().unwrap();
        let fid = call(&mut s, "new_family", json!({ "name": "Bench", "category": "furniture", "host": "none" }))["id"].as_u64().unwrap();
        let mut body = call(&mut s, "family_def", json!({ "id": fid }))["body"].clone();
        let pt = |x: f64, y: f64| json!({ "u": { "plane": null, "off": x }, "v": { "plane": null, "off": y } });
        body["forms"] = json!([{ "id": 50, "name": "Seat", "shape": { "type": "extrusion", "plane": 3, "start": "0", "end": "450", "profile": [{ "nodes": [pt(-900.0, -200.0), pt(900.0, -200.0), pt(900.0, 200.0), pt(-900.0, 200.0)], "bulges": [], "closed": true }] }, "void": false, "cuts_host": false, "cut_by": [], "material": "", "subcategory": "", "vis": {} }]);
        call(&mut s, "set_family_def", json!({ "id": fid, "body": body }));
        let info = call(&mut s, "doc_info", json!({}));
        let tid = info["family_types"].as_array().unwrap().iter().find(|t| t["family_id"].as_u64() == Some(fid)).unwrap()["id"].as_u64().unwrap();
        let pv = call(&mut s, "type_plan_preview", json!({ "type_id": tid }));
        assert_eq!(pv["footprint"].as_array().unwrap().len(), 4, "{pv}");
        let id = call(&mut s, "place_free_instance", json!({ "type_id": tid, "level_id": lvl, "point": [1000, 2000], "rotation": 0 }))["id"].as_u64().unwrap();
        let plan_view = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "floor_plan" && v["level_id"].as_u64() == Some(lvl)).unwrap()["id"].as_u64().unwrap();
        let plan = call(&mut s, "plan", json!({ "view_id": plan_view }));
        assert!(plan["inserts"].as_array().unwrap().iter().any(|i| i["id"].as_u64() == Some(id) && i["category"] == "Furniture"));
        let d = call(&mut s, "describe", json!({ "id": id }));
        assert!(d.to_string().contains("Elevation from Level"));
        call(&mut s, "patch_instance", json!({ "id": id, "patch": { "offset_mm": 300.0 } }));
        let scene = call(&mut s, "scene", json!({}));
        assert!(scene["meshes"].as_array().unwrap().iter().any(|m| m["id"].as_u64() == Some(id) && m["kind"] == "Component"));
        // plans: a level makes none until asked
        let l2 = call(&mut s, "add_level", json!({ "elevation": 7000.0, "name": "Roof" }))["id"].as_u64().unwrap();
        let info = call(&mut s, "doc_info", json!({}));
        assert!(!info["views"].as_array().unwrap().iter().any(|v| v["level_id"].as_u64() == Some(l2)));
        let fp = info["view_types"].as_array().unwrap().iter().find(|t| t["view_kind"] == "floor_plan").unwrap()["id"].as_u64().unwrap();
        let made = call(&mut s, "create_plan_views", json!({ "view_type": fp, "levels": [l2], "scale": 50 }));
        assert_eq!(made["ids"].as_array().unwrap().len(), 1);
        assert!(s.handle("create_plan_views", &json!({ "view_type": fp, "levels": [l2] })).is_err());
    }

    #[test]
    fn components_on_offer_leave_out_the_family_itself_and_any_that_hold_it() {
        let mut s = Session::new();
        let a = call(&mut s, "new_family", json!({ "name": "Alpha", "category": "generic_models", "host": "none" }))["id"].as_u64().unwrap();
        let b = call(&mut s, "new_family", json!({ "name": "Beta", "category": "generic_models", "host": "none" }))["id"].as_u64().unwrap();
        let names = |s: &mut Session, id: u64| -> Vec<String> { call(s, "family_components", json!({ "id": id }))["families"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect() };
        assert!(names(&mut s, a).contains(&"Beta".to_string()) && !names(&mut s, a).contains(&"Alpha".to_string()));
        // Beta holds Alpha: Alpha cannot be offered Beta any more
        let mut body = call(&mut s, "family_def", json!({ "id": b }))["body"].clone();
        body["nested"] = json!([{ "id": 40, "family": "Alpha", "origin": [{ "off": 0.0 }, { "off": 0.0 }, { "off": 0.0 }] }]);
        call(&mut s, "set_family_def", json!({ "id": b, "body": body }));
        assert!(!names(&mut s, a).contains(&"Beta".to_string()));
        assert!(names(&mut s, b).contains(&"Alpha".to_string()));
        let one = call(&mut s, "family_components", json!({ "id": b }))["families"].as_array().unwrap().iter().find(|f| f["name"] == "Alpha").unwrap().clone();
        assert!(one["params"].is_array() && one["types"].is_array());
    }

    #[test]
    fn shared_parameter_files_are_made_edited_and_read() {
        let mut s = Session::new();
        let dir = std::env::temp_dir().join(format!("openarch_shared_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Shared").to_string_lossy().to_string();
        let r = call(&mut s, "shared_params", json!({ "path": path, "op": "create" }));
        let file = r["path"].as_str().unwrap().to_string();
        assert!(file.ends_with("Shared.txt") && r["groups"].as_array().unwrap().is_empty());
        assert!(s.handle("shared_params", &json!({ "path": file, "op": "create" })).unwrap_err().contains("already exists"));
        let g = call(&mut s, "shared_params", json!({ "path": file, "op": "add_group", "name": "Dimensions" }))["made"].as_u64().unwrap();
        let r = call(&mut s, "shared_params", json!({ "path": file, "op": "add_param", "group": g, "name": "Rough Width", "kind": "length", "description": "Opening width" }));
        let guid = r["made"].as_str().unwrap().to_string();
        let q = &r["params"][0];
        assert_eq!(q["name"], "Rough Width");
        assert_eq!(q["kind"], "length");
        assert_eq!(q["datatype"], "LENGTH");
        assert!(s.handle("shared_params", &json!({ "path": file, "op": "add_param", "group": g, "name": "rough width", "kind": "text" })).unwrap_err().contains("already"));
        let r = call(&mut s, "shared_params", json!({ "path": file, "op": "edit_param", "guid": guid, "name": "Opening Width" }));
        assert_eq!(r["params"][0]["name"], "Opening Width");
        // read again from disk: the same
        let again = call(&mut s, "shared_params", json!({ "path": file }));
        assert_eq!(again["params"][0]["guid"], guid.as_str());
        assert!(s.handle("shared_params", &json!({ "path": file, "op": "remove_group", "group": g })).is_err());
        call(&mut s, "shared_params", json!({ "path": file, "op": "remove_param", "guid": guid }));
        call(&mut s, "shared_params", json!({ "path": file, "op": "remove_group", "group": g }));
        assert!(s.handle("shared_params", &json!({ "path": dir.join("Missing.txt").to_string_lossy(), "op": "read" })).unwrap_err().contains("could not read"));
    }

    #[test]
    fn lookup_tables_import_export_and_drive_a_plane() {
        let mut s = Session::new();
        let dir = std::env::temp_dir().join(format!("openarch_lookup_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("Sizes.csv");
        std::fs::write(&csv, "Name,Pick##number,Width##length##centimeters\nA,1,70\nB,2,100\n").unwrap();
        let r = call(&mut s, "read_lookup_csv", json!({ "path": csv.to_string_lossy() }));
        assert_eq!(r["table"]["name"], "Sizes", "the file name is the table name");
        assert_eq!(r["table"]["rows"][1][2], 1000.0, "centimetres come in as millimetres");
        let bad = dir.join("Bad.csv");
        std::fs::write(&bad, "Name,Pick##number\nA,x\n").unwrap();
        assert!(s.handle("read_lookup_csv", &json!({ "path": bad.to_string_lossy() })).unwrap_err().contains("not a number"));
        let out = call(&mut s, "write_lookup_csv", json!({ "path": dir.join("Out").to_string_lossy(), "table": r["table"] }));
        assert!(out["path"].as_str().unwrap().ends_with("Out.csv"));
        let back = std::fs::read_to_string(dir.join("Out.csv")).unwrap();
        assert!(back.contains("Width##length##centimeters") && back.contains("A,1,70"), "{back}");
        // a body that holds the table solves a plane from size_lookup
        let fid = call(&mut s, "new_family", json!({ "name": "Tbl", "category": "generic_models", "host": "none" }))["id"].as_u64().unwrap();
        let mut body = call(&mut s, "family_def", json!({ "id": fid }))["body"].clone();
        body["planes"].as_array_mut().unwrap().push(json!({ "id": 90, "name": "P", "axis": "x", "anchor": { "type": "offset", "from": 1, "by": "size_lookup(Sizes, \"Width\", 5, 2)" }, "ref_kind": "weak" }));
        body["lookup_tables"] = json!([r["table"]]);
        let pv = call(&mut s, "family_preview", json!({ "id": fid, "body": body }));
        assert_eq!(pv["planes"]["90"], 1000.0, "{pv}");
        body["lookup_tables"] = json!([]);
        assert!(call(&mut s, "family_preview", json!({ "id": fid, "body": body }))["error"].as_str().unwrap().contains("no lookup table"));
    }

    #[test]
    fn family_files_save_and_load_without_linking() {
        let mut s = Session::new();
        let fid = call(&mut s, "new_family", json!({ "name": "Gate", "category": "doors", "host": "wall" }))["id"].as_u64().unwrap();
        let def = call(&mut s, "family_def", json!({ "id": fid }));
        assert!(def["source_path"].is_null(), "a new family has no file yet");
        let tid = def["types"][0]["id"].as_u64().unwrap();
        let dir = std::env::temp_dir().join(format!("openarch_famfile_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Gate");
        // save: the editor's unsaved value goes into the file, the project copy is not changed, the path is remembered
        let r = call(&mut s, "save_family_file", json!({ "id": fid, "path": path.to_string_lossy(), "values": { tid.to_string(): { "Width": 1234 } }, "loaded_at": "2026-10-20" }));
        let written = std::path::PathBuf::from(r["path"].as_str().unwrap());
        assert_eq!(written.extension().unwrap(), "oaf", "the extension is added");
        let text = std::fs::read_to_string(&written).unwrap();
        assert!(text.contains("1234") && !text.contains("source_path\": \"") , "the file holds the value and not the path");
        let def = call(&mut s, "family_def", json!({ "id": fid }));
        assert_eq!(def["source_path"].as_str().unwrap(), written.to_string_lossy());
        assert_ne!(def["types"][0]["values"]["Width"], json!(1234.0), "the project copy is untouched by Save");
        let st = call(&mut s, "family_file_status", json!({ "id": fid }));
        assert_eq!(st["found"], json!(true));
        // loading the same file over the copy is refused until a mode is given
        let info = call(&mut s, "family_file_info", json!({ "path": written.to_string_lossy() }));
        assert_eq!(info["name"], json!("Gate"));
        assert_eq!(info["exists"].as_u64(), Some(fid));
        assert!(s.handle("import_family_file", &json!({ "path": written.to_string_lossy() })).is_err());
        let r = call(&mut s, "import_family_file", json!({ "path": written.to_string_lossy(), "mode": "overwrite" }));
        assert_eq!(r["replaced"], json!(true));
        assert_eq!(call(&mut s, "family_def", json!({ "id": fid }))["types"][0]["values"]["Width"], json!(1234.0));
        // a missing file is reported, and the status says it cannot be found
        std::fs::remove_file(&written).unwrap();
        assert_eq!(call(&mut s, "family_file_status", json!({ "id": fid }))["found"], json!(false));
        assert!(s.handle("family_file_info", &json!({ "path": written.to_string_lossy() })).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn family_edit_adds_labels_and_moves_planes() {
        let mut s = Session::new();
        let id = call(&mut s, "new_family", json!({"name": "Panel", "category": "generic_models", "host": "none"}))["id"].as_u64().unwrap();
        let def = call(&mut s, "family_def", json!({"id": id}));
        let mut body = def["body"].clone();
        let mut params = def["params"].clone();
        // two planes 600 apart, then a dimension across both
        let planes = body["planes"].as_array_mut().unwrap();
        planes.push(json!({"id": 50, "name": "A", "axis": "x", "anchor": {"type": "offset", "from": 1, "by": "100"}}));
        planes.push(json!({"id": 51, "name": "B", "axis": "x", "anchor": {"type": "offset", "from": 1, "by": "700"}}));
        let r = call(&mut s, "family_edit", json!({"id": id, "body": body, "params": params, "op": "add_dim", "refs": [51, 50], "view": "plan", "at": -400}));
        let dim = r["dim"].as_u64().unwrap();
        assert_eq!(r["body"]["dims"][0]["a"], 50);
        assert_eq!(r["body"]["dims"][0]["at"], -400.0);
        body = r["body"].clone();
        // label it with a new length parameter: the far plane follows the parameter
        params.as_array_mut().unwrap().push(json!({"name": "Gap", "param_kind": "length", "default": 250.0}));
        let r = call(&mut s, "family_edit", json!({"id": id, "body": body, "params": params, "op": "patch_dim", "dim": dim, "patch": {"label": "Gap"}}));
        assert_eq!(r["body"]["dims"][0]["drives"], json!([51]));
        body = r["body"].clone();
        let pv = call(&mut s, "family_preview", json!({"id": id, "body": body, "params": params, "values": {"Gap": 300.0}}));
        assert_eq!(pv["planes"]["51"], 400.0);
        // a plane held by the dimension cannot be moved; a free one can
        let held = s.handle("family_edit", &json!({"id": id, "body": body, "params": params, "op": "move_plane", "plane": 51, "to": 900.0}));
        assert!(held.unwrap_err().contains("held by a dimension"));
        let r = call(&mut s, "family_edit", json!({"id": id, "body": body, "params": params, "op": "move_plane", "plane": 50, "to": -100.0}));
        assert_eq!(r["body"]["planes"][3]["anchor"]["by"], "-100");
        // an unknown label is refused
        assert!(s.handle("family_edit", &json!({"id": id, "body": body, "params": params, "op": "patch_dim", "dim": dim, "patch": {"label": "Nope"}})).is_err());
    }

    #[test]
    fn family_parameters_rename_and_report_their_uses() {
        let mut s = Session::new();
        let id = call(&mut s, "new_family", json!({"name": "Panel", "category": "generic_models", "host": "none"}))["id"].as_u64().unwrap();
        let def = call(&mut s, "family_def", json!({"id": id}));
        let mut body = def["body"].clone();
        let mut params = def["params"].clone();
        assert_eq!(def["settings"]["part_type"], "Normal");
        assert!(def["categories"].as_array().unwrap().iter().any(|c| c["discipline"] == "structure"));
        body["planes"].as_array_mut().unwrap().push(json!({"id": 50, "name": "A", "axis": "x", "anchor": {"type": "offset", "from": 1, "by": "Width/2"}}));
        let u = call(&mut s, "family_param_uses", json!({"params": params, "body": body, "name": "Width"}));
        assert_eq!(u["uses"], json!(["reference plane 'A'"]));
        // renaming moves the type value, the expression and a formula with it
        params.as_array_mut().unwrap().push(json!({"name": "Half", "param_kind": "length", "formula": "Width / 2", "default": 0.0}));
        let r = call(&mut s, "family_rename_param", json!({"params": params, "body": body, "old": "Width", "new": "Overall Width"}));
        assert_eq!(r["body"]["planes"][3]["anchor"]["by"], "[Overall Width]/2");
        let half = r["params"].as_array().unwrap().iter().find(|p| p["name"] == "Half").unwrap();
        assert_eq!(half["formula"], "[Overall Width] / 2");
        assert!(s.handle("family_rename_param", &json!({"params": params, "body": body, "old": "Width", "new": "Height"})).is_err());
        let tid = def["types"][0]["id"].as_u64().unwrap();
        call(&mut s, "patch_family_type", json!({"id": tid, "patch": {"values": {"Width": 700.0}}}));
        call(&mut s, "set_family_def", json!({"id": id, "params": r["params"], "body": r["body"], "renames": [["Width", "Overall Width"]]}));
        let def = call(&mut s, "family_def", json!({"id": id}));
        assert_eq!(def["types"][0]["values"]["Overall Width"], 700.0);
        assert!(def["types"][0]["values"].get("Width").is_none());
        let v = call(&mut s, "family_param_values", json!({"id": id, "type": tid}));
        let get = |n: &str| v["values"].as_array().unwrap().iter().find(|x| x["name"] == n).unwrap()["value"].clone();
        assert_eq!(get("Overall Width"), 700.0);
        assert_eq!(get("Half"), 350.0);
    }
}
