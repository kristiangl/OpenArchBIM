//! Model Text in the project (Architecture > Model Text): letters as solids, placed at a point on a level. Flat text
//! lies on the level (a name on a slab, a parking number); upright text stands on the level facing the way its
//! rotation says, its back on the line through its point (letters on a facade). The letters come from the family
//! editor's font layout (famtext.rs); 3D views, elevations and sections draw them in Generic Models, plans of the level
//! their outlines.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::famtext::TextAlign;
use crate::geom::Point;

fn d_font() -> String {
    crate::famtext::DEFAULT_FONT.into()
}
fn d_size() -> f64 {
    300.0
}
fn d_depth() -> f64 {
    50.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelText {
    pub level: Id,
    /// The text's anchor (its alignment point on the first baseline), plan mm.
    pub at: Point,
    #[serde(default)]
    pub rotation_deg: f64,
    pub text: String,
    #[serde(default = "d_font")]
    pub font: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    /// Height of a capital letter.
    #[serde(default = "d_size")]
    pub size_mm: f64,
    #[serde(default = "d_depth")]
    pub depth_mm: f64,
    /// Above the level.
    #[serde(default)]
    pub offset_mm: f64,
    #[serde(default)]
    pub upright: bool,
    #[serde(default)]
    pub align: TextAlign,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub pinned: bool,
}

impl ModelText {
    pub fn new(level: Id, at: Point, text: &str) -> ModelText {
        ModelText { level, at, rotation_deg: 0.0, text: text.into(), font: d_font(), bold: false, italic: false, size_mm: d_size(), depth_mm: d_depth(), offset_mm: 0.0, upright: false, align: TextAlign::Left, material: String::new(), pinned: false }
    }
}

impl Document {
    pub fn model_text(&self, id: Id) -> Option<&ModelText> {
        match self.get(id) { Some(Element::ModelText(t)) => Some(t), _ => None }
    }
    pub fn model_texts(&self) -> Vec<(Id, &ModelText)> {
        self.iter().filter_map(|(id, e)| match e { Element::ModelText(t) => Some((id, t)), _ => None }).collect()
    }

    fn check_model_text(&self, t: &ModelText) -> Result<(), String> {
        if self.level(t.level).is_none() {
            return Err("Unknown level".into());
        }
        if t.text.trim().is_empty() {
            return Err("Model text needs some text.".into());
        }
        let ok = |v: f64| v.is_finite() && v > 0.0 && v <= 20_000.0;
        if !ok(t.size_mm) || !ok(t.depth_mm) {
            return Err("Size and depth are more than 0 and up to 20 m.".into());
        }
        if !t.offset_mm.is_finite() || !t.rotation_deg.is_finite() || !t.at.x.is_finite() || !t.at.y.is_finite() {
            return Err("Offset, rotation and position are numbers.".into());
        }
        if !t.material.is_empty() && self.material(&t.material).is_none() {
            return Err(format!("There is no material named '{}'.", t.material));
        }
        crate::famtext::layout(&t.font, t.bold, t.italic, &t.text, t.size_mm, t.align)?;
        Ok(())
    }

    pub fn add_model_text(&mut self, t: ModelText) -> Result<Id, String> {
        self.check_model_text(&t)?;
        self.transact("Model text", |tx| Ok(tx.add(Element::ModelText(t))))
    }

    /// Text, font, size, depth, offset, rotation, upright, alignment, material, level and pinned.
    pub fn patch_model_text(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.model_text(id).cloned().ok_or("not model text")?;
        let new: ModelText = crate::doc::apply_patch(&old, patch)?;
        if new == old {
            return Ok(());
        }
        self.check_model_text(&new)?;
        self.transact("Change model text", |tx| tx.update(id, Element::ModelText(new)))
    }

    /// The letters in the text's own frame (x along the text, y up the letters), mm.
    fn model_text_letters(t: &ModelText) -> Vec<crate::layers::Shape> {
        let Ok(laid) = crate::famtext::layout(&t.font, t.bold, t.italic, &t.text, t.size_mm, t.align) else { return vec![] };
        crate::geom::regions(&laid.rings).into_iter().map(|r| std::iter::once(r.outer).chain(r.holes).collect()).collect()
    }

    /// What a plan draws of the text: the letters of flat text, or the strip upright text stands on (its length along
    /// the text by its depth), in plan mm.
    pub fn model_text_plan(&self, id: Id) -> Vec<Vec<Point>> {
        let Some(t) = self.model_text(id) else { return vec![] };
        let (s, c) = t.rotation_deg.to_radians().sin_cos();
        let world = |p: Point| Point::new(t.at.x + p.x * c - p.y * s, t.at.y + p.x * s + p.y * c);
        let letters = Self::model_text_letters(t);
        if !t.upright {
            return letters.iter().flatten().map(|r| r.iter().map(|p| world(*p)).collect()).collect();
        }
        let xs = letters.iter().flatten().flatten().map(|p| p.x);
        let (lo, hi) = xs.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
        if !lo.is_finite() {
            return vec![];
        }
        // the letters stand toward the viewer's side: their face is `depth` in front of the line through the point
        let d = -t.depth_mm;
        vec![[Point::new(lo, 0.0), Point::new(hi, 0.0), Point::new(hi, d), Point::new(lo, d)].iter().map(|p| world(*p)).collect()]
    }

    /// The text's solids, for 3D views, elevations, sections and IFC (kind "ModelText").
    pub fn model_text_meshes(&self) -> Vec<(Id, &'static str, [u8; 3], Vec<f32>)> {
        self.model_texts()
            .into_iter()
            .filter_map(|(id, t)| {
                let base = self.level(t.level)?.elevation_mm + t.offset_mm;
                let letters = Self::model_text_letters(t);
                if letters.is_empty() {
                    return None;
                }
                // made flat in the text's frame (metres), then laid on the level or stood up
                let local = crate::mesh::shapes_prism(&letters, 0.0, t.depth_mm);
                let (s, c) = t.rotation_deg.to_radians().sin_cos();
                let (ax, ay, bz) = (t.at.x / 1000.0, t.at.y / 1000.0, base / 1000.0);
                let map = |v: [f64; 3]| -> [f64; 3] {
                    if t.upright {
                        // x along the text, y up, z out of its face toward the viewer (right handed)
                        [c * v[0] + s * v[2], s * v[0] - c * v[2], v[1]]
                    } else {
                        [c * v[0] - s * v[1], s * v[0] + c * v[1], v[2]]
                    }
                };
                let mut out = Vec::with_capacity(local.len());
                for k in local.chunks(6) {
                    let p = map([k[0] as f64, k[1] as f64, k[2] as f64]);
                    let n = map([k[3] as f64, k[4] as f64, k[5] as f64]);
                    out.extend_from_slice(&[(ax + p[0]) as f32, (ay + p[1]) as f32, (bz + p[2]) as f32, n[0] as f32, n[1] as f32, n[2] as f32]);
                }
                let colour = self.material(&t.material).map_or([150, 150, 150], |m| m.graphics.shading_color);
                Some((id, "ModelText", colour, out))
            })
            .collect()
    }

    /// The model text a plan shows: on its level.
    pub fn model_texts_in(&self, view: Id) -> Vec<(Id, Vec<Vec<Point>>)> {
        let Some(v) = self.view(view) else { return vec![] };
        let Some(level) = v.level.filter(|_| v.view_kind.is_plan()) else { return vec![] };
        self.model_texts().into_iter().filter(|(id, t)| t.level == level && !self.hidden_in(view, *id)).map(|(id, _)| (id, self.model_text_plan(id))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_text_is_letters_flat_or_upright() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let id = d.add_model_text(ModelText::new(lvl, Point::new(1000.0, 1000.0), "AB")).unwrap();
        let plan = d.model_text_plan(id);
        // A has an opening, B two: outlines and holes
        assert!(plan.len() >= 5, "{}", plan.len());
        assert!(plan.iter().flatten().all(|p| p.x >= 999.0 && p.y >= 999.0 && p.y <= 1000.0 + 320.0));
        let m = d.model_text_meshes();
        let zs: Vec<f32> = m[0].3.chunks(6).map(|k| k[2]).collect();
        let base = d.level(lvl).unwrap().elevation_mm as f32 / 1000.0;
        let top = zs.iter().cloned().fold(f32::MIN, f32::max);
        assert!((top - (base + 0.05)).abs() < 1e-4);
        // stood up it is as tall as its letters and its footprint is a strip as deep as the text
        d.patch_model_text(id, &serde_json::json!({ "upright": true })).unwrap();
        let zs: Vec<f32> = d.model_text_meshes()[0].3.chunks(6).map(|k| k[2]).collect();
        let top = zs.iter().cloned().fold(f32::MIN, f32::max);
        assert!(top - base > 0.29 && top - base < 0.33, "{}", top - base);
        let strip = d.model_text_plan(id);
        assert_eq!((strip.len(), strip[0].len()), (1, 4));
        assert!(d.add_model_text(ModelText::new(lvl, Point::new(0.0, 0.0), "  ")).is_err());
        assert!(d.patch_model_text(id, &serde_json::json!({ "size_mm": 0.0 })).is_err());
        d.transform_elements(&[id], crate::edit::Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 }, crate::edit::EditOptions::default()).unwrap();
        assert_eq!(d.model_text(id).unwrap().rotation_deg, 90.0);
        assert!(d.transform_elements(&[id], crate::edit::Xform::Mirror { ax: 0.0, ay: 0.0, bx: 0.0, by: 1.0 }, crate::edit::EditOptions::default()).is_err());
        let plan_view = d.views().into_iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::FloorPlan && v.level == Some(lvl)).unwrap().0;
        assert_eq!(d.model_texts_in(plan_view).len(), 1);
        d.delete(&[id]).unwrap();
        assert!(d.model_texts().is_empty());
    }
}
