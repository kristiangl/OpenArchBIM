//! Annotation families: the level head, the elevation marker and the section marker are drawings
//! with label parameters, as annotation families are, so a firm can draw its own.
//!
//! A family is drawn on paper, in millimetres, round the crossing of its two reference lines. The
//! horizontal one is the line the symbol belongs to (a level line, say) and the vertical one is where
//! the symbol flips: the head lies on the positive side of it for the right hand end of a line and is
//! mirrored about it for the left hand end. Sizes are paper sizes, so a symbol keeps its printed size
//! whatever the zoom and grows with the view scale in model terms.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::{Element, Id, Level};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    #[default]
    LevelHead,
    ElevationMarker,
    SectionMarker,
}

impl AnnotationKind {
    pub fn label(self) -> &'static str {
        match self {
            AnnotationKind::LevelHead => "Level Head",
            AnnotationKind::ElevationMarker => "Elevation Marker",
            AnnotationKind::SectionMarker => "Section Marker",
        }
    }

    /// The label parameters a family of this kind can show.
    pub fn params(self) -> &'static [&'static str] {
        match self {
            AnnotationKind::LevelHead => LEVEL_PARAMS,
            AnnotationKind::ElevationMarker | AnnotationKind::SectionMarker => VIEW_PARAMS,
        }
    }
}

pub const LEVEL_PARAMS: &[&str] = &["Name", "Elevation", "Survey Elevation", "Project Elevation", "Level Number"];
pub const VIEW_PARAMS: &[&str] = &["View Name", "View Number", "Sheet Number"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnnShape {
    Line { a: [f64; 2], b: [f64; 2] },
    Circle { c: [f64; 2], r: f64, #[serde(default)] filled: bool },
    /// Counter-clockwise from `start_deg` to `end_deg`.
    Arc { c: [f64; 2], r: f64, start_deg: f64, end_deg: f64 },
    Polygon { pts: Vec<[f64; 2]>, #[serde(default)] filled: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

/// One line of a label: a parameter with the text round it. `sample` is what the family editor shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelLine {
    pub param: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub sample: String,
}

impl LabelLine {
    pub fn new(param: &str, prefix: &str, suffix: &str, sample: &str) -> LabelLine {
        LabelLine { param: param.into(), prefix: prefix.into(), suffix: suffix.into(), sample: sample.into() }
    }
}

/// A block of text, one line per label parameter, anchored at (`x`, `y`) on paper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnLabel {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub halign: HAlign,
    #[serde(default)]
    pub valign: VAlign,
    /// Text height on paper, mm.
    #[serde(default = "text_size")]
    pub size_mm: f64,
    #[serde(default)]
    pub bold: bool,
    pub lines: Vec<LabelLine>,
}

fn text_size() -> f64 {
    2.5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationFamily {
    pub name: String,
    /// Saved as `family_kind`: elements are tagged with `kind` in the project file.
    #[serde(rename = "family_kind")]
    pub kind: AnnotationKind,
    /// The fixed drawing.
    #[serde(default)]
    pub shapes: Vec<AnnShape>,
    /// Elevation and section markers: drawn once for each pointer, turned to point along it
    /// (pointing along +x in the family).
    #[serde(default)]
    pub pointer: Vec<AnnShape>,
    #[serde(default)]
    pub labels: Vec<AnnLabel>,
    /// Pen width on paper, mm.
    #[serde(default = "pen")]
    pub pen_mm: f64,
}

fn pen() -> f64 {
    0.25
}

/// A level type: which head its levels wear, and which ends show it by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelType {
    pub name: String,
    /// The level head family. `None` falls back to the first level head in the project.
    #[serde(default)]
    pub head: Option<Id>,
    #[serde(default = "end_default")]
    pub end_default: [bool; 2],
}

fn end_default() -> [bool; 2] {
    [false, true]
}

fn line(a: [f64; 2], b: [f64; 2]) -> AnnShape {
    AnnShape::Line { a, b }
}

impl AnnotationFamily {
    /// Name above the line and the elevation below it, with a triangle marking the level.
    pub fn level_head(name: &str, elevation_param: &str, suffix: &str) -> AnnotationFamily {
        AnnotationFamily {
            name: name.into(),
            kind: AnnotationKind::LevelHead,
            shapes: vec![
                line([0.0, 0.0], [30.0, 0.0]),
                AnnShape::Polygon { pts: vec![[1.5, 3.0], [4.5, 3.0], [3.0, 0.0]], filled: true },
            ],
            pointer: vec![],
            labels: vec![
                AnnLabel { x: 6.5, y: 0.8, halign: HAlign::Left, valign: VAlign::Bottom, size_mm: 2.5, bold: true, lines: vec![LabelLine::new("Name", "", "", "Level 1")] },
                AnnLabel { x: 6.5, y: -0.8, halign: HAlign::Left, valign: VAlign::Top, size_mm: 2.5, bold: false, lines: vec![LabelLine::new(elevation_param, "", suffix, "3000")] },
            ],
            pen_mm: pen(),
        }
    }

    /// A circle quartered on the diagonals with a pointer for each view it makes.
    pub fn elevation_marker(name: &str) -> AnnotationFamily {
        AnnotationFamily {
            name: name.into(),
            kind: AnnotationKind::ElevationMarker,
            shapes: vec![
                AnnShape::Circle { c: [0.0, 0.0], r: 3.5, filled: false },
                line([-2.475, -2.475], [2.475, 2.475]),
                line([-2.475, 2.475], [2.475, -2.475]),
            ],
            pointer: vec![AnnShape::Polygon { pts: vec![[6.65, 0.0], [3.5, 2.17], [3.5, -2.17]], filled: true }],
            labels: vec![],
            pen_mm: pen(),
        }
    }

    /// A line head with a pointer along the view direction.
    pub fn section_marker(name: &str) -> AnnotationFamily {
        AnnotationFamily {
            name: name.into(),
            kind: AnnotationKind::SectionMarker,
            shapes: vec![AnnShape::Circle { c: [0.0, 0.0], r: 4.0, filled: false }, line([-4.0, 0.0], [4.0, 0.0])],
            pointer: vec![AnnShape::Polygon { pts: vec![[7.0, 0.0], [4.0, 2.0], [4.0, -2.0]], filled: true }],
            labels: vec![
                AnnLabel { x: -6.0, y: 0.0, halign: HAlign::Right, valign: VAlign::Middle, size_mm: 2.5, bold: false, lines: vec![LabelLine::new("View Name", "", "", "Section 1")] },
                AnnLabel { x: 0.0, y: 2.0, halign: HAlign::Center, valign: VAlign::Middle, size_mm: 2.0, bold: true, lines: vec![LabelLine::new("View Number", "", "", "1")] },
                AnnLabel { x: 0.0, y: -2.0, halign: HAlign::Center, valign: VAlign::Middle, size_mm: 2.0, bold: false, lines: vec![LabelLine::new("Sheet Number", "", "", "A101")] },
            ],
            pen_mm: pen(),
        }
    }
}

/// A level head with its label text worked out, ready to draw.
#[derive(Debug, Clone, Serialize)]
pub struct DrawnLabel {
    pub x: f64,
    pub y: f64,
    pub halign: HAlign,
    pub valign: VAlign,
    pub size_mm: f64,
    pub bold: bool,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrawnHead {
    pub family: Id,
    pub shapes: Vec<AnnShape>,
    pub labels: Vec<DrawnLabel>,
    pub pen_mm: f64,
}

impl Document {
    pub fn annotation_families(&self) -> Vec<(Id, &AnnotationFamily)> {
        self.iter().filter_map(|(id, e)| if let Element::AnnotationFamily(f) = e { Some((id, f)) } else { None }).collect()
    }

    pub fn annotation_family(&self, id: Id) -> Option<&AnnotationFamily> {
        match self.get(id) {
            Some(Element::AnnotationFamily(f)) => Some(f),
            _ => None,
        }
    }

    pub fn level_types(&self) -> Vec<(Id, &LevelType)> {
        self.iter().filter_map(|(id, e)| if let Element::LevelType(t) = e { Some((id, t)) } else { None }).collect()
    }

    pub fn level_type(&self, id: Id) -> Option<&LevelType> {
        match self.get(id) {
            Some(Element::LevelType(t)) => Some(t),
            _ => None,
        }
    }

    pub(crate) fn first_family(&self, kind: AnnotationKind) -> Option<Id> {
        self.annotation_families().into_iter().find(|(_, f)| f.kind == kind).map(|(id, _)| id)
    }

    /// The type a level uses: its own, or the first in the project.
    pub fn level_type_of(&self, l: &Level) -> Option<(Id, &LevelType)> {
        l.level_type.and_then(|t| self.level_type(t).map(|x| (t, x))).or_else(|| self.level_types().into_iter().next())
    }

    /// The head family a level draws with.
    pub fn level_head_family(&self, l: &Level) -> Option<(Id, &AnnotationFamily)> {
        let id = self.level_type_of(l).and_then(|(_, t)| t.head).filter(|f| self.annotation_family(*f).is_some_and(|f| f.kind == AnnotationKind::LevelHead)).or_else(|| self.first_family(AnnotationKind::LevelHead))?;
        self.annotation_family(id).map(|f| (id, f))
    }

    /// The family an elevation marker draws with.
    pub fn marker_family(&self, family: Option<Id>) -> Option<(Id, &AnnotationFamily)> {
        let id = family.filter(|f| self.annotation_family(*f).is_some_and(|f| f.kind == AnnotationKind::ElevationMarker)).or_else(|| self.first_family(AnnotationKind::ElevationMarker))?;
        self.annotation_family(id).map(|f| (id, f))
    }

    /// What a label parameter reads for a level (`None` when it has no value, and its text is left out).
    pub fn level_param(&self, l: &Level, param: &str) -> Option<String> {
        let loc = self.project_location().map(|(_, p)| p.clone());
        let (base_z, survey_z) = loc.as_ref().map_or((0.0, 0.0), |p| (p.base.z, p.shared_origin.z));
        let mm = |v: f64| format!("{}", v.round() as i64);
        match param {
            "Name" => Some(l.name.clone()),
            "Elevation" => Some(mm(self.level_display_elevation(l))),
            "Project Elevation" => Some(mm(l.elevation_mm - base_z)),
            "Survey Elevation" => Some(mm(l.elevation_mm - survey_z)),
            "Level Number" => Some(l.number.clone()).filter(|n| !n.is_empty()),
            _ => None,
        }
    }

    /// The head a level is drawn with, its labels filled in.
    pub fn drawn_head(&self, l: &Level) -> Option<DrawnHead> {
        let (id, f) = self.level_head_family(l)?;
        let labels = f
            .labels
            .iter()
            .map(|lb| DrawnLabel {
                x: lb.x,
                y: lb.y,
                halign: lb.halign,
                valign: lb.valign,
                size_mm: lb.size_mm,
                bold: lb.bold,
                lines: lb.lines.iter().filter_map(|ln| self.level_param(l, &ln.param).map(|v| format!("{}{}{}", ln.prefix, v, ln.suffix))).collect(),
            })
            .collect();
        Some(DrawnHead { family: id, shapes: f.shapes.clone(), labels, pen_mm: f.pen_mm })
    }

    /// Fill in the families and the level type a project needs (new projects, and projects saved before
    /// annotation families), and point levels that have no type at the first one.
    pub fn ensure_annotation_defaults(&mut self) {
        self.upgrade_section_markers();
        if !self.annotation_families().is_empty() && !self.level_types().is_empty() {
            return;
        }
        let _ = self.transact("Annotation defaults", |tx| {
            let have = |tx: &crate::doc::Tx, kind: AnnotationKind| tx.doc_ref().annotation_families().iter().any(|(_, f)| f.kind == kind);
            let mut head = tx.doc_ref().first_family(AnnotationKind::LevelHead);
            if head.is_none() {
                head = Some(tx.add(Element::AnnotationFamily(AnnotationFamily::level_head("Level Head", "Elevation", ""))));
                tx.add(Element::AnnotationFamily(AnnotationFamily::level_head("Level Head - AHD", "Survey Elevation", " AHD")));
            }
            if !have(tx, AnnotationKind::ElevationMarker) {
                tx.add(Element::AnnotationFamily(AnnotationFamily::elevation_marker("Elevation Marker")));
            }
            if !have(tx, AnnotationKind::SectionMarker) {
                tx.add(Element::AnnotationFamily(AnnotationFamily::section_marker("Section Marker")));
            }
            if tx.doc_ref().level_types().is_empty() {
                tx.add(Element::LevelType(LevelType { name: "Level".into(), head, end_default: end_default() }));
            }
            Ok(())
        });
        self.clear_history();
    }

    /// Section markers saved before sections existed point their arrow at the head and have no view name. Put the
    /// arrow the right way round and add the name, unless the person has changed the family themselves.
    fn upgrade_section_markers(&mut self) {
        let old_pointer = vec![AnnShape::Polygon { pts: vec![[4.0, 0.0], [7.0, 2.0], [7.0, -2.0]], filled: true }];
        let fixes: Vec<(Id, AnnotationFamily)> = self
            .annotation_families()
            .into_iter()
            .filter(|(_, f)| f.kind == AnnotationKind::SectionMarker && f.pointer == old_pointer)
            .map(|(id, f)| {
                let mut n = f.clone();
                n.pointer = AnnotationFamily::section_marker("").pointer;
                if !n.labels.iter().any(|l| l.lines.iter().any(|x| x.param == "View Name")) {
                    n.labels.insert(0, AnnotationFamily::section_marker("").labels.remove(0));
                }
                (id, n)
            })
            .collect();
        if fixes.is_empty() {
            return;
        }
        let _ = self.transact("Section marker defaults", |tx| {
            for (id, f) in fixes {
                tx.update(id, Element::AnnotationFamily(f))?;
            }
            Ok(())
        });
        self.clear_history();
    }

    // ---- editing ---------------------------------------------------------------------

    /// Replace a family's drawing and labels (the family editor saves the whole family).
    pub fn set_annotation_family(&mut self, id: Id, new: AnnotationFamily) -> Result<(), String> {
        let Some(old) = self.annotation_family(id) else { return Err("not an annotation family".into()) };
        if new.kind != old.kind {
            return Err("a family keeps its kind".into());
        }
        if new.name.trim().is_empty() {
            return Err("a family needs a name".into());
        }
        if self.annotation_families().iter().any(|(i, f)| *i != id && f.name == new.name) {
            return Err(format!("a family named '{}' already exists", new.name));
        }
        let allowed = new.kind.params();
        for lb in &new.labels {
            for ln in &lb.lines {
                if !allowed.contains(&ln.param.as_str()) {
                    return Err(format!("'{}' is not a label parameter of a {}", ln.param, new.kind.label().to_lowercase()));
                }
            }
            if !(lb.size_mm.is_finite() && lb.size_mm > 0.2 && lb.size_mm < 20.0) {
                return Err("text height must be between 0.2 and 20 mm".into());
            }
        }
        if !(new.pen_mm.is_finite() && new.pen_mm >= 0.0 && new.pen_mm < 2.0) {
            return Err("the pen width must be between 0 and 2 mm".into());
        }
        if new == *old {
            return Ok(());
        }
        self.transact("Edit family", |tx| tx.update(id, Element::AnnotationFamily(new)))
    }

    /// A copy of a family under a new name, ready to edit.
    pub fn duplicate_annotation_family(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let Some(f) = self.annotation_family(id).cloned() else { return Err("not an annotation family".into()) };
        if name.trim().is_empty() || self.annotation_families().iter().any(|(_, g)| g.name == name) {
            return Err(format!("a family named '{name}' already exists"));
        }
        self.transact("Duplicate family", |tx| Ok(tx.add(Element::AnnotationFamily(AnnotationFamily { name: name.into(), ..f }))))
    }

    /// Draw an elevation marker with another family.
    pub fn set_marker_family(&mut self, id: Id, family: Option<Id>) -> Result<(), String> {
        let Some(mut m) = self.elevation_marker(id).cloned() else { return Err("not an elevation marker".into()) };
        if m.family == family {
            return Ok(());
        }
        m.family = family;
        self.transact("Change marker family", |tx| tx.update(id, Element::ElevationMarker(m)))
    }

    /// Delete a family nothing uses, keeping at least one of each kind.
    pub fn delete_annotation_family(&mut self, id: Id) -> Result<(), String> {
        let Some(f) = self.annotation_family(id) else { return Err("not an annotation family".into()) };
        if self.annotation_families().iter().filter(|(_, g)| g.kind == f.kind).count() < 2 {
            return Err("the project needs at least one family of each kind".into());
        }
        if self.level_types().iter().any(|(_, t)| t.head == Some(id)) || self.elevation_markers().iter().any(|(_, m)| m.family == Some(id)) {
            return Err("that family is in use; choose another for the types or markers that use it first".into());
        }
        self.transact("Delete family", |tx| tx.remove(id))
    }

    /// Edit a level type (its name, head family or default ends).
    pub fn patch_level_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(old) = self.level_type(id).cloned() else { return Err("not a level type".into()) };
        let new: LevelType = crate::doc::apply_patch(&old, patch)?;
        if let Some(h) = new.head {
            if !self.annotation_family(h).is_some_and(|f| f.kind == AnnotationKind::LevelHead) {
                return Err("the head must be a level head family".into());
            }
        }
        if new == old {
            return Ok(());
        }
        self.transact("Change level type", |tx| tx.update(id, Element::LevelType(new)))
    }

    pub fn duplicate_level_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let Some(t) = self.level_type(id).cloned() else { return Err("not a level type".into()) };
        if name.trim().is_empty() || self.level_types().iter().any(|(_, o)| o.name == name) {
            return Err(format!("a level type named '{name}' already exists"));
        }
        self.transact("Duplicate level type", |tx| Ok(tx.add(Element::LevelType(LevelType { name: name.into(), ..t }))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_project_has_a_head_for_its_levels_and_a_marker_family() {
        let d = Document::new_project();
        assert_eq!(d.annotation_families().iter().filter(|(_, f)| f.kind == AnnotationKind::LevelHead).count(), 2);
        let l = d.levels()[1].1.clone();
        let head = d.drawn_head(&l).unwrap();
        assert_eq!(head.labels[0].lines, vec!["Level 1".to_string()]);
        assert_eq!(head.labels[1].lines, vec!["3000".to_string()]);
        assert!(d.marker_family(None).is_some());
    }

    #[test]
    fn the_ahd_head_reads_the_survey_elevation_with_its_suffix() {
        let mut d = Document::new_project();
        let ahd = d.annotation_families().into_iter().find(|(_, f)| f.name == "Level Head - AHD").unwrap().0;
        let t = d.level_types()[0].0;
        d.patch_level_type(t, &serde_json::json!({ "head": ahd })).unwrap();
        let l = d.levels()[1].1.clone();
        let head = d.drawn_head(&l).unwrap();
        assert_eq!(head.labels[1].lines, vec!["3000 AHD".to_string()]);
    }

    #[test]
    fn a_label_with_no_value_leaves_its_text_out() {
        let d = Document::new_project();
        let l = d.levels()[0].1.clone();
        assert_eq!(d.level_param(&l, "Level Number"), None);
    }

    #[test]
    fn families_check_their_labels_and_keep_one_of_each_kind() {
        let mut d = Document::new_project();
        let (id, mut f) = d.annotation_families().into_iter().find(|(_, f)| f.kind == AnnotationKind::LevelHead).map(|(i, f)| (i, f.clone())).unwrap();
        f.labels[0].lines[0].param = "Width".into();
        assert!(d.set_annotation_family(id, f.clone()).unwrap_err().contains("not a label parameter"));
        f.labels[0].lines[0].param = "Elevation".into();
        f.labels[0].lines[0].prefix = "RL ".into();
        d.set_annotation_family(id, f).unwrap();
        let l = d.levels()[0].1.clone();
        assert_eq!(d.drawn_head(&l).unwrap().labels[0].lines, vec!["RL 0".to_string()]);
        let marker = d.annotation_families().into_iter().find(|(_, f)| f.kind == AnnotationKind::ElevationMarker).unwrap().0;
        assert!(d.delete_annotation_family(marker).is_err());
        let copy = d.duplicate_annotation_family(marker, "Marker 2").unwrap();
        assert!(d.delete_annotation_family(copy).is_ok());
    }
}
