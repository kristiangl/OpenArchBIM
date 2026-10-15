//! Resolving how things draw (Object Styles, materials, view overrides) and the
//! API for editing the project's graphics standards.

use openarch_model::styles::{FillPattern, Material, PatternGraphics, PatternKind, SOLID, SOLID_FILL};
use openarch_model::view::*;
use openarch_model::{Document, Element, Id};
use serde_json::{json, Value};

use crate::Res;

pub fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Pattern graphics an element brings from its material or type, used as the
/// base for its fill before the view's overrides apply.
#[derive(Default, Clone, Debug)]
pub struct Hint {
    pub cut: Option<PatternGraphics>,
    pub cut_bg: Option<PatternGraphics>,
    pub surface: Option<PatternGraphics>,
    pub surface_bg: Option<PatternGraphics>,
}

impl Hint {
    pub fn from_material(m: &Material) -> Hint {
        let g = &m.graphics;
        Hint {
            cut: Some(g.cut_foreground.clone()),
            cut_bg: Some(g.cut_background.clone()),
            surface: Some(g.surface_foreground.clone()),
            surface_bg: Some(g.surface_background.clone()),
        }
    }

    /// Cut fill comes from `cut` (a type's coarse fill), the rest from a material.
    pub fn with_cut(mut self, cut: PatternGraphics) -> Hint {
        self.cut = Some(cut);
        self.cut_bg = None;
        self
    }
}

const FALLBACK_INK: Color = [29, 34, 41];

/// Resolve how an element draws in a plan view. Order, weakest first: the
/// category (or subcategory) Object Style, the element's material or type, the
/// view's category override, its subcategory override, then the per-element
/// override. `None` means it is switched off.
#[allow(clippy::too_many_arguments)]
pub fn style_for(d: &Document, settings: &ViewSettings, category: &str, sub: Option<&str>, element: Option<&CategoryOverride>, display: &str, hint: &Hint) -> Option<Value> {
    let cat = settings.overrides.model.get(category).or_else(|| settings.overrides.annotation.get(category));
    let sub_ov = sub.and_then(|s| cat.and_then(|c| c.subcategories.get(s)));
    if cat.is_some_and(|c| !c.visible) || sub_ov.is_some_and(|c| !c.visible) || element.is_some_and(|e| !e.visible) {
        return None;
    }
    let cut = display == "cut";
    let underlay = display == "underlay";
    let os = d.object_styles().map(|(_, s)| s);
    let base = os.and_then(|s| s.resolve(category, sub));
    let beyond = (display == "beyond").then(|| os.and_then(|s| s.resolve("Lines", Some("<Beyond>")))).flatten();

    let mut line_color: Color = base.map_or(FALLBACK_INK, |b| b.color);
    let mut line_weight: u8 = base.map_or(if cut { 3 } else { 1 }, |b| b.weight(cut));
    let mut line_pattern: String = base.map_or(SOLID.to_string(), |b| b.pattern.clone());
    if let Some(b) = beyond {
        line_color = b.color;
        line_weight = line_weight.min(b.projection_weight);
        line_pattern = b.pattern.clone();
    }

    // fill: the element's own material or type first, else the category's material
    let cat_material = base.and_then(|b| b.material.as_deref()).and_then(|n| d.material(n)).map(Hint::from_material);
    let pick = |h: &Option<PatternGraphics>, fallback: Option<&PatternGraphics>| -> PatternGraphics {
        match h {
            Some(p) if p.pattern.is_some() || p.color.is_some() => p.clone(),
            _ => fallback.cloned().unwrap_or_default(),
        }
    };
    let cm = cat_material.unwrap_or_default();
    let (fg, bg) = if cut {
        (pick(&hint.cut, cm.cut.as_ref()), pick(&hint.cut_bg, cm.cut_bg.as_ref()))
    } else {
        (pick(&hint.surface, cm.surface.as_ref()), pick(&hint.surface_bg, cm.surface_bg.as_ref()))
    };
    let mut fill_pattern = fg.pattern.clone();
    let mut fill_color: Color = fg.color.unwrap_or(line_color);
    let mut fill_visible = fg.pattern.is_some();
    let mut bg_color: Option<Color> = bg.pattern.as_ref().map(|_| bg.color.unwrap_or([255, 255, 255]));
    let mut transparency = 0u8;
    let mut halftone = underlay || settings.display_model == DisplayModel::Halftone;

    for o in cat.into_iter().chain(sub_ov).chain(element) {
        halftone |= o.halftone;
        let (lines, f, b) = if cut { (&o.cut_lines, &o.cut_foreground, &o.cut_background) } else { (&o.projection_lines, &o.surface_foreground, &o.surface_background) };
        if let Some(c) = lines.color {
            line_color = c;
        }
        if let Some(w) = lines.weight {
            line_weight = w;
        }
        if let Some(p) = &lines.pattern {
            line_pattern = p.clone();
        }
        if let Some(c) = f.color {
            fill_color = c;
            fill_visible = true;
            fill_pattern.get_or_insert_with(|| SOLID_FILL.to_string());
        }
        if f.pattern.is_some() {
            fill_pattern = f.pattern.clone();
            fill_visible = true;
        }
        if !f.visible {
            fill_visible = false;
        }
        if let Some(c) = b.color {
            bg_color = Some(c);
        }
        if !b.visible {
            bg_color = None;
        }
        if o.transparency > 0 {
            transparency = o.transparency;
        }
    }
    let width_mm = d.line_weights().map_or(0.18, |(_, w)| w.model_width(line_weight, settings.scale));
    Some(json!({
        "halftone": halftone,
        "line": { "color": hex(line_color), "weight": line_weight, "width_mm": width_mm, "pattern": line_pattern },
        "fill": { "visible": fill_visible, "color": hex(fill_color), "pattern": fill_pattern, "background": bg_color.map(hex) },
        "transparency": transparency,
    }))
}

/// Dash arrays (paper mm) for every line pattern, keyed by name, for the canvas.
pub fn line_pattern_defs(d: &Document) -> Value {
    let mut m = serde_json::Map::new();
    for (_, p) in d.line_patterns() {
        m.insert(p.name.clone(), json!(p.dash_array()));
    }
    Value::Object(m)
}

fn fill_def(p: &FillPattern) -> Value {
    json!({
        "solid": p.solid, "model": p.pattern_type == PatternKind::Model,
        "lines": p.lines.iter().map(|l| json!({ "angle": l.angle_deg, "origin": l.origin, "shift": l.shift, "spacing": l.spacing, "dashes": l.dashes })).collect::<Vec<_>>(),
    })
}

/// Hatch definitions for every fill pattern, keyed by name, for the canvas.
pub fn fill_pattern_defs(d: &Document) -> Value {
    let mut m = serde_json::Map::new();
    for (_, p) in d.fill_patterns() {
        m.insert(p.name.clone(), fill_def(p));
    }
    Value::Object(m)
}

// ---------------------------------------------------------------- hints from model data

/// The fill an element brings with it: a wall or floor type's coarse fill at
/// coarse detail, otherwise its core material's cut pattern; the surface fill
/// comes from the outermost material.
pub fn layers_hint(d: &Document, layers: &[openarch_model::Layer], coarse_fill: &PatternGraphics, detail: DetailLevel, surface_first: bool) -> Hint {
    let core = d.core_material(layers).map(Hint::from_material).unwrap_or_default();
    let surface_layer = if surface_first { layers.iter().find(|l| !l.is_void) } else { layers.iter().rev().find(|l| !l.is_void) };
    let surf = surface_layer.and_then(|l| d.material(&l.material)).map(Hint::from_material).unwrap_or_default();
    let mut h = Hint { surface: surf.surface, surface_bg: surf.surface_bg, ..core };
    if detail == DetailLevel::Coarse {
        h = h.with_cut(coarse_fill.clone());
    }
    h
}

// ---------------------------------------------------------------- the Object Styles dialog

/// Everything the Object Styles, Materials, Line Patterns, Fill Patterns and
/// Line Weights dialogs show.
pub fn standards(d: &Document) -> Res {
    let (os_id, os) = d.object_styles().ok_or("project has no object styles")?;
    let (lw_id, lw) = d.line_weights().ok_or("project has no line weight table")?;
    let materials = d.materials();
    let used = |id: Id| d.style_item_users(id);
    Ok(json!({
        "object_styles": { "id": os_id, "model": os.model, "annotation": os.annotation, "imported": os.imported },
        "line_weights": { "id": lw_id, "scales": lw.scales, "model": lw.model, "perspective": lw.perspective, "annotation": lw.annotation },
        "line_patterns": d.line_patterns().iter().map(|(id, p)| json!({ "id": id, "pattern": p, "dash": p.dash_array(), "used_by": used(*id) })).collect::<Vec<_>>(),
        "fill_patterns": d.fill_patterns().iter().map(|(id, p)| json!({ "id": id, "pattern": p, "def": fill_def(p), "used_by": used(*id) })).collect::<Vec<_>>(),
        "materials": materials.iter().map(|(id, m)| json!({ "id": id, "material": m, "used_by": used(*id) })).collect::<Vec<_>>(),
    }))
}

pub fn patch_object_styles(d: &mut Document, p: &Value) -> Res {
    d.patch_object_styles(p.get("patch").ok_or("missing patch")?)?;
    Ok(json!({}))
}

pub fn patch_line_weights(d: &mut Document, p: &Value) -> Res {
    d.patch_line_weights(p.get("patch").ok_or("missing patch")?)?;
    Ok(json!({}))
}

/// `item` is `{ "kind": "material" | "line_pattern" | "fill_pattern", ... }` for
/// a new item, or `{ "copy_of": id, "name": ... }` to duplicate one.
pub fn add_style_item(d: &mut Document, p: &Value) -> Res {
    let name = p.get("name").and_then(|v| v.as_str()).map(String::from);
    let element = if let Some(src) = p.get("copy_of").and_then(|v| v.as_u64()) {
        let name = name.ok_or("missing name")?;
        match d.get(src).cloned() {
            Some(Element::LinePattern(mut x)) => {
                x.name = name;
                x.built_in = false;
                Element::LinePattern(x)
            }
            Some(Element::FillPattern(mut x)) => {
                x.name = name;
                x.built_in = false;
                Element::FillPattern(x)
            }
            Some(Element::Material(mut x)) => {
                x.name = name;
                Element::Material(x)
            }
            _ => return Err("nothing to copy".into()),
        }
    } else {
        let mut v = p.get("item").cloned().ok_or("missing item")?;
        let kind = v.get("kind").and_then(|k| k.as_str()).ok_or("missing kind")?.to_string();
        let tag = match kind.as_str() {
            "material" => "Material",
            "line_pattern" => "LinePattern",
            "fill_pattern" => "FillPattern",
            _ => return Err("unknown item kind".into()),
        };
        v["kind"] = json!(tag);
        let mut e: Element = serde_json::from_value(v).map_err(|e| e.to_string())?;
        match &mut e {
            Element::LinePattern(x) => x.built_in = false,
            Element::FillPattern(x) => x.built_in = false,
            _ => {}
        }
        e
    };
    Ok(json!({ "id": d.add_style_item(element)? }))
}

pub fn patch_style_item(d: &mut Document, p: &Value) -> Res {
    d.patch_style_item(crate::id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?;
    Ok(json!({}))
}

pub fn delete_style_item(d: &mut Document, p: &Value) -> Res {
    d.delete_style_item(crate::id_of(p, "id")?)?;
    Ok(json!({}))
}

// ---------------------------------------------------------------- layered walls

/// A wall drawn layer by layer.
pub struct LayeredWall {
    pub id: Id,
    pub display: &'static str,
    pub wall_type: Id,
    pub dont_clean: bool,
    pub layers: Vec<openarch_model::layers::LayerGeom>,
}

fn ring_json(r: &openarch_model::layers::Ring) -> Value {
    Value::Array(r.iter().map(|p| json!([p.x, p.y])).collect())
}

fn shapes_json(shapes: &[openarch_model::layers::Shape]) -> Value {
    Value::Array(shapes.iter().map(|s| Value::Array(s.iter().map(ring_json).collect())).collect())
}

/// Group layered walls that draw as one shape (same display, line style and
/// overrides; and, when the view only cleans joins of the same type, the same
/// type) and merge each group's layers: one outline, and one region per
/// material, so joined walls show no seam inside a material and a thin line
/// between materials.
pub fn wall_groups(d: &Document, settings: &ViewSettings, view: &View, walls: &[LayeredWall], same_type_only: bool) -> Vec<Value> {
    use openarch_model::layers::union_rings;
    let mut groups: std::collections::BTreeMap<String, Vec<&LayeredWall>> = Default::default();
    for w in walls {
        let eo = view.element_overrides.get(&w.id);
        let Some(st) = style_for(d, settings, "Walls", None, eo, w.display, &Hint::default()) else { continue };
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            w.display,
            st["line"],
            st["halftone"],
            st["transparency"],
            if same_type_only { w.wall_type } else { 0 },
            if w.dont_clean { w.id } else { 0 },
        );
        groups.entry(key).or_default().push(w);
    }
    let mut out = vec![];
    for (_, members) in groups {
        let first = members[0];
        let eo = view.element_overrides.get(&first.id);
        let Some(line_style) = style_for(d, settings, "Walls", None, eo, first.display, &Hint::default()) else { continue };
        let edge = style_for(d, settings, "Walls", Some("Common Edges"), eo, first.display, &Hint::default()).map(|s| s["line"].clone());
        // outer boundary of everything in the group
        let outer: Vec<&openarch_model::layers::Ring> = members.iter().flat_map(|w| w.layers.iter()).flat_map(|l| l.shapes.iter()).filter_map(|s| s.first()).collect();
        let outline = union_rings(outer);
        // one region per material and fill look
        let mut by_fill: std::collections::BTreeMap<String, (Value, Vec<openarch_model::layers::Ring>, String, bool)> = Default::default();
        for w in &members {
            let eo = view.element_overrides.get(&w.id);
            for l in &w.layers {
                let (fill, void) = if l.is_void {
                    (Value::Null, true)
                } else {
                    let hint = d.material(&l.material).map(Hint::from_material).unwrap_or_default();
                    (style_for(d, settings, "Walls", None, eo, w.display, &hint).map_or(Value::Null, |s| json!({ "fill": s["fill"], "transparency": s["transparency"], "halftone": s["halftone"] })), false)
                };
                let key = format!("{}|{}|{}", l.material, void, fill);
                let entry = by_fill.entry(key).or_insert_with(|| (fill, vec![], l.material.clone(), void));
                entry.1.extend(l.shapes.iter().filter_map(|s| s.first().cloned()));
            }
        }
        let fills: Vec<Value> = by_fill
            .into_values()
            .map(|(fill, rings, material, void)| {
                let shapes = union_rings(rings.iter());
                json!({ "material": material, "void": void, "style": fill, "shapes": shapes_json(&shapes) })
            })
            .collect();
        out.push(json!({
            "display": first.display, "walls": members.iter().map(|w| w.id).collect::<Vec<_>>(),
            "line": line_style["line"], "halftone": line_style["halftone"], "transparency": line_style["transparency"],
            "edge": edge, "outline": shapes_json(&outline), "fills": fills,
        }));
    }
    out
}
