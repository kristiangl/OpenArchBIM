//! Annotation families: the level head, the elevation marker and the section marker are drawings
//! with label parameters, as annotation families are, so a firm can draw its own.
//!
//! A family is drawn on paper, in millimetres, round the crossing of its two reference lines. The
//! horizontal one is the line the symbol belongs to (a level line, say) and the vertical one is where
//! the symbol flips: the head lies on the positive side of it for the right hand end of a line and is
//! mirrored about it for the left hand end. Sizes are paper sizes, so a symbol keeps its printed size
//! whatever the zoom and grows with the view scale in model terms.
//!
//! The drawing is lines (curves with a line style) and regions (closed loops of curves, filled with a
//! filled region type or masking what lies behind), as the family editor's Line, Filled Region and
//! Masking Region tools make them. Line styles are the subcategories of the family's category in
//! Object Styles.

use serde::{Deserialize, Serialize};

use crate::curve::{self, Curve, SketchError};
use crate::doc::Document;
use crate::element::{Element, Id, Level};
use crate::geom::Point;
use crate::styles::{ObjectStyle, PatternKind, SOLID_FILL};
use crate::view::Color;

/// Annotation sketches are checked at this many times paper size, as if drawn at 1:100, so the sketch
/// tolerances (made for model millimetres) suit symbols a few millimetres across.
pub const SKETCH_SCALE: f64 = 100.0;
/// The line style that hides a line, such as the edge of a region that should show only its fill.
pub const INVISIBLE_LINES: &str = "<Invisible lines>";
/// The filled region type every family starts with.
pub const SOLID_FILLED: &str = "Solid filled";

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

    /// The family category, as the family editor names it.
    pub fn category(self) -> &'static str {
        match self {
            AnnotationKind::LevelHead => "Level Heads",
            AnnotationKind::ElevationMarker => "Elevation Marks",
            AnnotationKind::SectionMarker => "Section Marks",
        }
    }

    /// The label parameters a family of this kind can show.
    pub fn params(self) -> &'static [&'static str] {
        match self {
            AnnotationKind::LevelHead => LEVEL_PARAMS,
            AnnotationKind::ElevationMarker => VIEW_PARAMS,
            AnnotationKind::SectionMarker => SECTION_PARAMS,
        }
    }
}

pub const LEVEL_PARAMS: &[&str] = &["Name", "Elevation", "Survey Elevation", "Project Elevation", "Level Number"];
pub const VIEW_PARAMS: &[&str] = &["View Name", "View Number", "Sheet Number"];
pub const SECTION_PARAMS: &[&str] = &["View Name", "View Number", "Sheet Number", "Reference Label"];

/// A line of an annotation family's drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnLine {
    pub curve: Curve,
    /// Line style: a subcategory of the family's category, empty for the category itself, or `<Invisible lines>`.
    #[serde(default)]
    pub subcategory: String,
    #[serde(default = "yes")]
    pub visible: bool,
    /// Elevation and section marks: drawn with the pointer (once for each view, turned to it) rather than the body.
    #[serde(default)]
    pub pointer: bool,
}

/// One edge of a region's boundary, with its own line style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionEdge {
    pub curve: Curve,
    #[serde(default)]
    pub subcategory: String,
}

/// A filled or masking region: closed loops of edges, head to tail. A loop inside another is a hole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnRegion {
    pub loops: Vec<Vec<RegionEdge>>,
    /// A masking region hides what lies behind it and has no type.
    #[serde(default)]
    pub masking: bool,
    /// The filled region type (by name) of a filled region.
    #[serde(default)]
    pub region_type: String,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub pointer: bool,
}

/// A filled region type: the patterns a filled region is filled with. Patterns are drafting patterns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionType {
    pub name: String,
    #[serde(default)]
    pub foreground: Option<String>,
    #[serde(default)]
    pub foreground_color: Color,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default = "white")]
    pub background_color: Color,
    /// Line weight (1 to 16) of the pattern lines.
    #[serde(default = "one")]
    pub line_weight: u8,
    /// Hides what lies behind the region; without it things behind show between the pattern lines.
    #[serde(default = "yes")]
    pub masking: bool,
}

impl RegionType {
    pub fn solid(name: &str) -> RegionType {
        RegionType { name: name.into(), foreground: Some(SOLID_FILL.into()), foreground_color: [0, 0, 0], background: None, background_color: white(), line_weight: 1, masking: true }
    }
}

fn yes() -> bool {
    true
}

fn one() -> u8 {
    1
}

fn white() -> Color {
    [255, 255, 255]
}

fn region_types() -> Vec<RegionType> {
    vec![RegionType::solid(SOLID_FILLED)]
}

/// The shapes annotation families were drawn with before lines and regions, read from older projects.
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

/// A line of a label as labels were saved before label types: one parameter with the text round it.
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

/// One parameter of a label, as the Edit Label dialog lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelParam {
    pub param: String,
    /// Spaces between this parameter and the next (none after a break).
    #[serde(default = "one")]
    pub spaces: u8,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    /// What the family editor shows in its place (the parameter's name when empty).
    #[serde(default)]
    pub sample: String,
    /// A new line starts after this parameter.
    #[serde(default, rename = "break")]
    pub brk: bool,
}

impl LabelParam {
    pub fn new(param: &str, prefix: &str, suffix: &str, sample: &str, brk: bool) -> LabelParam {
        LabelParam { param: param.into(), spaces: 1, prefix: prefix.into(), suffix: suffix.into(), sample: sample.into(), brk }
    }
}

/// A text type: how a label or a piece of text of an annotation family is written. Labels and text have types of
/// their own (`label_types`, `text_types`), with the same settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextType {
    pub name: String,
    #[serde(default = "arial")]
    pub font: String,
    /// Text height on paper, mm.
    pub size_mm: f64,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    /// Letters stretched (above 1) or narrowed (below 1); the height stays.
    #[serde(default = "unit")]
    pub width_factor: f64,
    #[serde(default)]
    pub color: Color,
    /// Line weight (1 to 16) of the border.
    #[serde(default = "one")]
    pub line_weight: u8,
    /// Background: opaque hides what is behind the text, transparent lets it show.
    #[serde(default)]
    pub opaque: bool,
    #[serde(default)]
    pub show_border: bool,
    /// Gap between the text and its border, mm.
    #[serde(default = "border_offset")]
    pub border_offset_mm: f64,
    #[serde(default = "tab_size")]
    pub tab_size_mm: f64,
}

fn arial() -> String {
    "Arial".into()
}

fn unit() -> f64 {
    1.0
}

fn border_offset() -> f64 {
    1.0
}

fn tab_size() -> f64 {
    12.7
}

impl TextType {
    pub fn new(name: &str, size_mm: f64, bold: bool) -> TextType {
        TextType {
            name: name.into(),
            font: arial(),
            size_mm,
            bold,
            italic: false,
            underline: false,
            width_factor: 1.0,
            color: [0, 0, 0],
            line_weight: 1,
            opaque: false,
            show_border: false,
            border_offset_mm: border_offset(),
            tab_size_mm: tab_size(),
        }
    }

    /// The name a type of this size and weight gets ("2.5mm", "2mm Bold").
    pub fn name_for(size_mm: f64, bold: bool) -> String {
        let size = format!("{}", (size_mm * 100.0).round() / 100.0);
        format!("{size}mm{}", if bold { " Bold" } else { "" })
    }
}

fn text_types() -> Vec<TextType> {
    vec![TextType::new("2.5mm", 2.5, false)]
}

/// A label: the values of parameters written in a box on paper, anchored at (`x`, `y`). The anchor is the box's
/// left edge, middle or right edge (`halign`) and its top, middle or bottom (`valign`); text wraps at the box's width.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnLabel {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub halign: HAlign,
    #[serde(default)]
    pub valign: VAlign,
    /// The label type, by name.
    #[serde(default)]
    pub label_type: String,
    #[serde(default)]
    pub width_mm: f64,
    #[serde(default)]
    pub params: Vec<LabelParam>,
    /// Wrap only between parameters, not between words.
    #[serde(default)]
    pub wrap_params_only: bool,
    /// Never upside-down when the label is turned.
    #[serde(default = "yes")]
    pub keep_readable: bool,
    #[serde(default = "yes")]
    pub visible: bool,
    /// A label as saved before label types: its text height, weight and lines (read into a type and parameters).
    #[serde(default, rename = "size_mm", skip_serializing)]
    pub old_size_mm: Option<f64>,
    #[serde(default, rename = "bold", skip_serializing)]
    pub old_bold: Option<bool>,
    #[serde(default, rename = "lines", skip_serializing)]
    pub old_lines: Vec<LabelLine>,
}

impl AnnLabel {
    fn has_old(&self) -> bool {
        self.old_size_mm.is_some() || !self.old_lines.is_empty()
    }

    /// A label written with `label_type`, its parameters on lines of their own.
    pub fn new(x: f64, y: f64, halign: HAlign, valign: VAlign, label_type: &str, width_mm: f64, params: Vec<LabelParam>) -> AnnLabel {
        AnnLabel { x, y, halign, valign, label_type: label_type.into(), width_mm, params, wrap_params_only: false, keep_readable: true, visible: true, old_size_mm: None, old_bold: None, old_lines: vec![] }
    }
}

/// Text written in the family: the same in every project view. `width_mm` 0 is as wide as its longest line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnText {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub text_type: String,
    #[serde(default)]
    pub halign: HAlign,
    #[serde(default)]
    pub valign: VAlign,
    #[serde(default)]
    pub width_mm: f64,
    #[serde(default = "yes")]
    pub keep_readable: bool,
    #[serde(default = "yes")]
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationFamily {
    pub name: String,
    /// Saved as `family_kind`: elements are tagged with `kind` in the project file.
    #[serde(rename = "family_kind")]
    pub kind: AnnotationKind,
    /// Lines, drawn over the regions. Elevation and section marks draw their pointer lines once for each
    /// pointer, turned to point along it (pointing along +x in the family).
    #[serde(default)]
    pub lines: Vec<AnnLine>,
    #[serde(default)]
    pub regions: Vec<AnnRegion>,
    #[serde(default = "region_types")]
    pub region_types: Vec<RegionType>,
    #[serde(default)]
    pub labels: Vec<AnnLabel>,
    #[serde(default)]
    pub texts: Vec<AnnText>,
    #[serde(default = "text_types")]
    pub label_types: Vec<TextType>,
    #[serde(default = "text_types")]
    pub text_types: Vec<TextType>,
    /// Section markers: how the hidden middle of a broken section line shows (Broken Section Display Style).
    #[serde(default)]
    pub broken_style: BrokenStyle,
    /// Section markers: the text a reference section shows for its Reference Label parameter.
    #[serde(default = "reference_label")]
    pub reference_label: String,
    /// The drawing of a project saved before lines and regions: read into them when the project opens.
    #[serde(default, rename = "shapes", skip_serializing)]
    pub old_shapes: Vec<AnnShape>,
    #[serde(default, rename = "pointer", skip_serializing)]
    pub old_pointer: Vec<AnnShape>,
}

fn reference_label() -> String {
    "SIM".into()
}

/// How a broken section line shows between its ends: not at all (a gap), or as a thin line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokenStyle {
    #[default]
    Gapped,
    Continuous,
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

fn pt(p: [f64; 2]) -> Point {
    Point::new(p[0], p[1])
}

impl AnnotationFamily {
    fn drawn_with(name: &str, kind: AnnotationKind, shapes: Vec<AnnShape>, pointer: Vec<AnnShape>, labels: Vec<AnnLabel>) -> AnnotationFamily {
        let mut f = AnnotationFamily {
            name: name.into(),
            kind,
            lines: vec![],
            regions: vec![],
            region_types: region_types(),
            labels,
            texts: vec![],
            label_types: text_types(),
            text_types: text_types(),
            broken_style: BrokenStyle::default(),
            reference_label: reference_label(),
            old_shapes: shapes,
            old_pointer: pointer,
        };
        f.read_old_drawing();
        f
    }

    /// Name above the line and the elevation below it, with a triangle marking the level.
    pub fn level_head(name: &str, elevation_param: &str, suffix: &str) -> AnnotationFamily {
        AnnotationFamily::drawn_with(
            name,
            AnnotationKind::LevelHead,
            vec![line([0.0, 0.0], [30.0, 0.0]), AnnShape::Polygon { pts: vec![[1.5, 3.0], [4.5, 3.0], [3.0, 0.0]], filled: true }],
            vec![],
            vec![
                old_label(6.5, 0.8, HAlign::Left, VAlign::Bottom, 2.5, true, vec![LabelLine::new("Name", "", "", "Level 1")]),
                old_label(6.5, -0.8, HAlign::Left, VAlign::Top, 2.5, false, vec![LabelLine::new(elevation_param, "", suffix, "3000")]),
            ],
        )
    }

    /// A circle quartered on the diagonals with a pointer for each view it makes.
    pub fn elevation_marker(name: &str) -> AnnotationFamily {
        AnnotationFamily::drawn_with(
            name,
            AnnotationKind::ElevationMarker,
            vec![AnnShape::Circle { c: [0.0, 0.0], r: 3.5, filled: false }, line([-2.475, -2.475], [2.475, 2.475]), line([-2.475, 2.475], [2.475, -2.475])],
            vec![AnnShape::Polygon { pts: vec![[6.65, 0.0], [3.5, 2.17], [3.5, -2.17]], filled: true }],
            vec![],
        )
    }

    /// A line head with a pointer along the view direction.
    pub fn section_marker(name: &str) -> AnnotationFamily {
        let (shapes, pointer) = section_marker_shapes();
        AnnotationFamily::drawn_with(
            name,
            AnnotationKind::SectionMarker,
            shapes,
            pointer,
            vec![
                old_label(-6.0, 0.0, HAlign::Right, VAlign::Middle, 2.5, false, vec![LabelLine::new("View Name", "", "", "Section 1")]),
                old_label(0.0, 2.0, HAlign::Center, VAlign::Middle, 2.0, true, vec![LabelLine::new("View Number", "", "", "1")]),
                old_label(0.0, -2.0, HAlign::Center, VAlign::Middle, 2.0, false, vec![LabelLine::new("Sheet Number", "", "", "A101")]),
                // shows on reference sections only
                old_label(0.0, -5.0, HAlign::Center, VAlign::Top, 2.0, false, vec![LabelLine::new("Reference Label", "", "", "SIM")]),
            ],
        )
    }

    /// Whether the family still has the shapes or labels of an older project.
    pub fn has_old_drawing(&self) -> bool {
        !self.old_shapes.is_empty() || !self.old_pointer.is_empty() || self.labels.iter().any(|l| l.has_old())
    }

    /// Read the drawing of an older project into lines, regions and labels with types. A filled shape becomes a
    /// solid filled region with invisible edges; an outline (which used to hide what lay behind it) becomes lines over
    /// a masking region; a label's text height and weight become a label type, its lines parameters with a break
    /// after each. Returns whether there was anything to read.
    pub fn read_old_drawing(&mut self) -> bool {
        let labels = self.read_old_labels();
        if self.old_shapes.is_empty() && self.old_pointer.is_empty() {
            return labels;
        }
        let solid = self.region_types.first().map_or_else(|| SOLID_FILLED.to_string(), |t| t.name.clone());
        let lists = [(std::mem::take(&mut self.old_shapes), false), (std::mem::take(&mut self.old_pointer), true)];
        for (list, on_pointer) in lists {
            for sh in list {
                let (curves, filled): (Vec<Curve>, Option<bool>) = match sh {
                    AnnShape::Line { a, b } => (vec![Curve::line(pt(a), pt(b))], None),
                    AnnShape::Circle { c, r, filled } => (vec![Curve::Circle { centre: pt(c), radius: r }], Some(filled)),
                    AnnShape::Arc { c, r, start_deg, end_deg } => {
                        let at = |deg: f64| Point::new(c[0] + r * deg.to_radians().cos(), c[1] + r * deg.to_radians().sin());
                        let curve = if (end_deg - start_deg).abs() >= 359.999 {
                            Curve::Circle { centre: pt(c), radius: r }
                        } else {
                            Curve::Arc { start: at(start_deg), end: at(end_deg), centre: pt(c), ccw: end_deg > start_deg }
                        };
                        (vec![curve], None)
                    }
                    AnnShape::Polygon { pts, filled } => {
                        let n = pts.len();
                        let edges = (0..n).map(|i| Curve::line(pt(pts[i]), pt(pts[(i + 1) % n]))).collect();
                        (edges, if n >= 3 { Some(filled) } else { None })
                    }
                };
                let curves: Vec<Curve> = curves.into_iter().filter(|c| c.length() > 1e-6).collect();
                if curves.is_empty() {
                    continue;
                }
                if let Some(filled) = filled {
                    let edges = curves.iter().map(|c| RegionEdge { curve: c.clone(), subcategory: INVISIBLE_LINES.into() }).collect();
                    self.regions.push(AnnRegion { loops: vec![edges], masking: !filled, region_type: solid.clone(), visible: true, pointer: on_pointer });
                    if filled {
                        continue;
                    }
                }
                self.lines.extend(curves.into_iter().map(|c| AnnLine { curve: c, subcategory: String::new(), visible: true, pointer: on_pointer }));
            }
        }
        true
    }

    fn read_old_labels(&mut self) -> bool {
        let mut types: Vec<(String, f64, bool)> = vec![];
        for lb in self.labels.iter_mut().filter(|l| l.has_old()) {
            let size = lb.old_size_mm.take().unwrap_or(2.5);
            let bold = lb.old_bold.take().unwrap_or(false);
            let lines = std::mem::take(&mut lb.old_lines);
            let n = lines.len();
            // labels had no box: names get a wide one, so long names do not wrap; numbers one round their sample
            let wide = lines.iter().any(|l| l.param == "Name" || l.param == "View Name");
            let chars = lines.iter().map(|l| (l.prefix.len() + l.sample.len() + l.suffix.len()) as f64).fold(1.0, f64::max);
            lb.width_mm = if wide { 40.0 } else { (chars * size * 0.7 + size).max(8.0) };
            lb.label_type = TextType::name_for(size, bold);
            lb.params = lines.into_iter().enumerate().map(|(k, l)| LabelParam::new(&l.param, &l.prefix, &l.suffix, &l.sample, k + 1 < n)).collect();
            types.push((lb.label_type.clone(), size, bold));
        }
        let read = !types.is_empty();
        for (name, size, bold) in types {
            if !self.label_types.iter().any(|t| t.name == name) {
                self.label_types.push(TextType::new(&name, size, bold));
            }
        }
        read
    }

    pub fn label_type(&self, name: &str) -> Option<&TextType> {
        self.label_types.iter().find(|t| t.name == name).or_else(|| self.label_types.first())
    }

    pub fn text_type(&self, name: &str) -> Option<&TextType> {
        self.text_types.iter().find(|t| t.name == name).or_else(|| self.text_types.first())
    }

    /// A filled region type by name (the first type when there is none of that name).
    pub fn region_type(&self, name: &str) -> Option<&RegionType> {
        self.region_types.iter().find(|t| t.name == name).or_else(|| self.region_types.first())
    }
}

/// A label as the shipped families wrote it before label types (read into a type and parameters).
fn old_label(x: f64, y: f64, halign: HAlign, valign: VAlign, size_mm: f64, bold: bool, lines: Vec<LabelLine>) -> AnnLabel {
    AnnLabel { old_size_mm: Some(size_mm), old_bold: Some(bold), old_lines: lines, ..AnnLabel::new(x, y, halign, valign, "", 0.0, vec![]) }
}

/// The section marker's drawing as it was before lines and regions.
fn section_marker_shapes() -> (Vec<AnnShape>, Vec<AnnShape>) {
    (
        vec![AnnShape::Circle { c: [0.0, 0.0], r: 4.0, filled: false }, line([-4.0, 0.0], [4.0, 0.0])],
        vec![AnnShape::Polygon { pts: vec![[7.0, 0.0], [4.0, 2.0], [4.0, -2.0]], filled: true }],
    )
}

/// Group a region's edges into loops, each head to tail and closed, as Finish does for a region's sketch.
pub fn region_loops(edges: &[RegionEdge]) -> Result<Vec<Vec<RegionEdge>>, SketchError> {
    edge_loops(edges, SKETCH_SCALE)
}

/// As `region_loops`, checking the edges at `scale` times their size (1 for model mm).
pub fn edge_loops(edges: &[RegionEdge], scale: f64) -> Result<Vec<Vec<RegionEdge>>, SketchError> {
    let scaled: Vec<Curve> = edges.iter().map(|e| e.curve.scaled(scale)).collect();
    let order = curve::sketch_loop_order(&scaled).map_err(|e| SketchError { at: e.at.map(|a| [a[0] / scale, a[1] / scale]), ..e })?;
    Ok(order
        .iter()
        .map(|l| l.iter().map(|&(i, rev)| RegionEdge { curve: if rev { edges[i].curve.reversed() } else { edges[i].curve.clone() }, subcategory: edges[i].subcategory.clone() }).collect())
        .collect())
}

/// How far apart two ends of a region's edges may be and still join, paper mm.
const JOIN_MM: f64 = curve::JOIN_TOL / SKETCH_SCALE;

/// Whether a loop runs head to tail and closes.
fn loop_closes(l: &[RegionEdge]) -> bool {
    match l {
        [] => false,
        [one] => one.curve.is_closed(),
        _ => (0..l.len()).all(|i| !l[i].curve.is_closed() && l[i].curve.end().dist(&l[(i + 1) % l.len()].curve.start()) <= JOIN_MM),
    }
}

/// A drawn line: points in paper mm, with the width, colour and dashes (paper mm) of its line style.
#[derive(Debug, Clone, Serialize)]
pub struct PathStroke {
    pub pts: Vec<[f64; 2]>,
    pub closed: bool,
    pub width_mm: f64,
    pub color: Color,
    pub dash: Vec<f64>,
    /// The line it draws, or the region whose edge it is.
    pub line: Option<usize>,
    pub region: Option<usize>,
    /// Not drawn in a project (a line that is not visible, or has invisible lines for its style); the family
    /// editor shows it greyed.
    pub hidden: bool,
}

/// A drawn region: its rings (holes inside outlines fill even-odd) and what fills it.
#[derive(Debug, Clone, Serialize)]
pub struct PathFill {
    pub rings: Vec<Vec<[f64; 2]>>,
    pub masking: bool,
    pub foreground: Option<String>,
    pub foreground_color: Color,
    pub background: Option<String>,
    pub background_color: Color,
    /// Width of the pattern lines.
    pub pen_mm: f64,
    pub region: usize,
    pub area_mm2: f64,
    pub hidden: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SymbolPaths {
    pub fills: Vec<PathFill>,
    pub strokes: Vec<PathStroke>,
}

/// A family's drawing ready to draw: the body, and the pointer drawn for each view of a mark.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SymbolDrawing {
    pub body: SymbolPaths,
    pub pointer: SymbolPaths,
}

/// A text type worked out for drawing: the border's width comes from its line weight.
#[derive(Debug, Clone, Serialize)]
pub struct TextStyle {
    pub font: String,
    pub size_mm: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub width_factor: f64,
    pub color: Color,
    pub opaque: bool,
    pub show_border: bool,
    pub border_offset_mm: f64,
    pub line_mm: f64,
    pub tab_size_mm: f64,
}

/// Where text in a box may wrap: between words, or only between a label's parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Wrap {
    Words,
    Pieces,
}

/// A label or a piece of text ready to draw: its lines (a label breaks them where a parameter says), each the pieces
/// it is made of (a parameter's value with its prefix, suffix and the spaces after it), in a box `width_mm` wide
/// (0: as wide as the text). The canvas wraps them in the box, since it knows the font.
#[derive(Debug, Clone, Serialize)]
pub struct DrawnText {
    pub x: f64,
    pub y: f64,
    pub halign: HAlign,
    pub valign: VAlign,
    pub width_mm: f64,
    pub lines: Vec<Vec<String>>,
    pub wrap: Wrap,
    pub style: TextStyle,
    pub label: Option<usize>,
    pub text: Option<usize>,
    /// Not shown in a project (not visible); the family editor shows it greyed.
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrawnHead {
    pub family: Id,
    pub paths: SymbolPaths,
    pub labels: Vec<DrawnText>,
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
        let labels = self.symbol_texts(f, &|p| self.level_param(l, p), false);
        Some(DrawnHead { family: id, paths: self.symbol_drawing(f, false).body, labels })
    }

    pub(crate) fn text_style(&self, t: Option<&TextType>) -> TextStyle {
        let t = t.cloned().unwrap_or_else(|| TextType::new("", 2.5, false));
        let line_mm = self.line_weights().map_or(0.18, |(_, w)| w.annotation_width(t.line_weight));
        TextStyle {
            font: t.font,
            size_mm: t.size_mm,
            bold: t.bold,
            italic: t.italic,
            underline: t.underline,
            width_factor: t.width_factor,
            color: t.color,
            opaque: t.opaque,
            show_border: t.show_border,
            border_offset_mm: t.border_offset_mm,
            line_mm,
            tab_size_mm: t.tab_size_mm,
        }
    }

    /// A family's labels, with `value` giving what each parameter reads (`None` leaves it and its spaces out, and a
    /// line left empty goes), then its text. A project leaves out what is not visible; the family editor (`editing`)
    /// keeps it, marked hidden.
    pub fn symbol_texts(&self, f: &AnnotationFamily, value: &dyn Fn(&str) -> Option<String>, editing: bool) -> Vec<DrawnText> {
        let mut out = vec![];
        for (i, lb) in f.labels.iter().enumerate() {
            if !lb.visible && !editing {
                continue;
            }
            let mut lines: Vec<Vec<String>> = vec![];
            let mut line: Vec<String> = vec![];
            for (k, p) in lb.params.iter().enumerate() {
                if let Some(v) = value(&p.param) {
                    let gap = if p.brk || k + 1 == lb.params.len() { 0 } else { p.spaces as usize };
                    line.push(format!("{}{}{}{}", p.prefix, v, p.suffix, " ".repeat(gap)));
                }
                if p.brk {
                    lines.push(std::mem::take(&mut line));
                }
            }
            lines.push(line);
            lines.retain(|l| !l.is_empty());
            out.push(DrawnText {
                x: lb.x,
                y: lb.y,
                halign: lb.halign,
                valign: lb.valign,
                width_mm: lb.width_mm,
                lines,
                wrap: if lb.wrap_params_only { Wrap::Pieces } else { Wrap::Words },
                style: self.text_style(f.label_type(&lb.label_type)),
                label: Some(i),
                text: None,
                hidden: !lb.visible,
            });
        }
        for (i, t) in f.texts.iter().enumerate() {
            if !t.visible && !editing {
                continue;
            }
            out.push(DrawnText {
                x: t.x,
                y: t.y,
                halign: t.halign,
                valign: t.valign,
                width_mm: t.width_mm,
                lines: t.text.split('\n').map(|l| vec![l.to_string()]).collect(),
                wrap: Wrap::Words,
                style: self.text_style(f.text_type(&t.text_type)),
                label: None,
                text: Some(i),
                hidden: !t.visible,
            });
        }
        out
    }

    /// The style a line of a family draws with: its subcategory of the family's category in Object Styles.
    fn symbol_style(&self, f: &AnnotationFamily, sub: &str) -> ObjectStyle {
        let os = self.object_styles().map(|(_, s)| s);
        let sub = (!sub.is_empty()).then_some(sub);
        os.and_then(|s| s.resolve(f.kind.category(), sub)).cloned().unwrap_or_else(|| ObjectStyle::new(1, None))
    }

    /// The subcategories a family's lines can use (its category's, from Object Styles).
    pub fn symbol_subcategories(&self, kind: AnnotationKind) -> Vec<String> {
        self.object_styles().and_then(|(_, s)| s.annotation.get(kind.category())).map(|c| c.subcategories.keys().cloned().collect()).unwrap_or_default()
    }

    /// A family's lines and regions as paths in paper mm, with their line styles and fills worked out. A project
    /// leaves out what is not visible; the family editor (`editing`) keeps it, marked hidden.
    pub fn symbol_drawing(&self, f: &AnnotationFamily, editing: bool) -> SymbolDrawing {
        const TOL: f64 = 0.01;
        let weights = self.line_weights().map(|(_, w)| w.clone()).unwrap_or_default();
        let stroke = |c: &Curve, sub: &str, line: Option<usize>, region: Option<usize>, visible: bool| -> Option<PathStroke> {
            let hidden = !visible || sub == INVISIBLE_LINES;
            if hidden && !editing {
                return None;
            }
            let st = self.symbol_style(f, if sub == INVISIBLE_LINES { "" } else { sub });
            let dash = self.line_pattern(&st.pattern).map(|p| p.dash_array()).unwrap_or_default();
            let pts = c.tessellate(TOL);
            Some(PathStroke {
                pts: pts.iter().map(|q| [q.x, q.y]).collect(),
                closed: c.is_closed(),
                width_mm: weights.annotation_width(st.projection_weight),
                color: st.color,
                dash,
                line,
                region,
                hidden,
            })
        };
        let mut out = SymbolDrawing::default();
        for (i, r) in f.regions.iter().enumerate() {
            if !r.visible && !editing {
                continue;
            }
            let polys: Vec<Vec<Point>> = r.loops.iter().map(|l| curve::loop_polygon(&l.iter().map(|e| e.curve.clone()).collect::<Vec<_>>(), TOL)).collect();
            let depth = curve::loop_depths(&polys);
            let area: f64 = polys.iter().zip(&depth).map(|(p, d)| if d % 2 == 0 { 1.0 } else { -1.0 } * crate::geom::polygon_area_abs(p)).sum();
            let ty = f.region_type(&r.region_type);
            let paths = if r.pointer { &mut out.pointer } else { &mut out.body };
            paths.fills.push(PathFill {
                rings: polys.iter().map(|p| p.iter().map(|q| [q.x, q.y]).collect()).collect(),
                masking: r.masking || ty.is_some_and(|t| t.masking),
                foreground: if r.masking { None } else { ty.and_then(|t| t.foreground.clone()) },
                foreground_color: ty.map_or([0, 0, 0], |t| t.foreground_color),
                background: if r.masking { None } else { ty.and_then(|t| t.background.clone()) },
                background_color: ty.map_or(white(), |t| t.background_color),
                pen_mm: weights.annotation_width(ty.map_or(1, |t| t.line_weight)),
                region: i,
                area_mm2: area,
                hidden: !r.visible,
            });
            for e in r.loops.iter().flatten() {
                paths.strokes.extend(stroke(&e.curve, &e.subcategory, None, Some(i), r.visible));
            }
        }
        for (i, l) in f.lines.iter().enumerate() {
            let paths = if l.pointer { &mut out.pointer } else { &mut out.body };
            paths.strokes.extend(stroke(&l.curve, &l.subcategory, Some(i), None, l.visible));
        }
        out
    }

    /// Fill in the families and the level type a project needs (new projects, and projects saved before
    /// annotation families), and point levels that have no type at the first one.
    pub fn ensure_annotation_defaults(&mut self) {
        self.upgrade_section_markers();
        self.upgrade_annotation_drawings();
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
            .filter(|(_, f)| f.kind == AnnotationKind::SectionMarker && f.old_pointer == old_pointer)
            .map(|(id, f)| {
                let mut n = f.clone();
                n.old_pointer = section_marker_shapes().1;
                if !n.labels.iter().any(|l| l.old_lines.iter().any(|x| x.param == "View Name") || l.params.iter().any(|x| x.param == "View Name")) {
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

    /// Families saved before lines and regions get them from their shapes, and Object Styles gets the categories
    /// their line styles belong to.
    fn upgrade_annotation_drawings(&mut self) {
        let fixes: Vec<(Id, AnnotationFamily)> = self
            .annotation_families()
            .into_iter()
            .filter(|(_, f)| f.has_old_drawing())
            .map(|(id, f)| {
                let mut n = f.clone();
                n.read_old_drawing();
                (id, n)
            })
            .collect();
        let styles = self.object_styles().map(|(id, s)| (id, s.clone())).filter(|(_, s)| crate::view::SYMBOL_CATEGORIES.iter().any(|c| !s.annotation.contains_key(*c)));
        if fixes.is_empty() && styles.is_none() {
            return;
        }
        let _ = self.transact("Annotation family lines and regions", |tx| {
            for (id, f) in fixes {
                tx.update(id, Element::AnnotationFamily(f))?;
            }
            if let Some((id, mut s)) = styles {
                for c in crate::view::SYMBOL_CATEGORIES {
                    s.annotation.entry(c.to_string()).or_insert_with(|| ObjectStyle::new(1, None));
                }
                tx.update(id, Element::ObjectStyles(s))?;
            }
            Ok(())
        });
        self.clear_history();
    }

    /// Check a family's drawing: line styles the category has, regions that close, types and patterns that exist.
    fn check_symbol(&self, f: &AnnotationFamily) -> Result<(), String> {
        let subs = self.symbol_subcategories(f.kind);
        let style_ok = |s: &str| s.is_empty() || s == INVISIBLE_LINES || subs.iter().any(|x| x == s);
        let curve_ok = |c: &Curve| {
            let pts = c.tessellate(0.05);
            pts.iter().all(|q| q.x.is_finite() && q.y.is_finite() && q.x.abs() < 1000.0 && q.y.abs() < 1000.0) && c.length() > 0.001
        };
        for l in &f.lines {
            if !curve_ok(&l.curve) {
                return Err("a line has no length or lies more than a metre from the reference lines".into());
            }
            if !style_ok(&l.subcategory) {
                return Err(format!("'{}' is not a subcategory of {}", l.subcategory, f.kind.category()));
            }
        }
        if f.region_types.is_empty() {
            return Err("a family needs at least one filled region type".into());
        }
        for (i, t) in f.region_types.iter().enumerate() {
            if t.name.trim().is_empty() || f.region_types[..i].iter().any(|o| o.name == t.name) {
                return Err(format!("filled region types need names of their own ('{}')", t.name));
            }
            if !(1..=crate::styles::MAX_WEIGHT).contains(&t.line_weight) {
                return Err(format!("{}: line weight must be 1 to {}", t.name, crate::styles::MAX_WEIGHT));
            }
            for p in [&t.foreground, &t.background].into_iter().flatten() {
                match self.fill_pattern(p) {
                    None => return Err(format!("{}: no fill pattern named '{p}'", t.name)),
                    Some(fp) if fp.pattern_type != PatternKind::Drafting => return Err(format!("{}: '{p}' is a model pattern; a filled region in an annotation family takes drafting patterns", t.name)),
                    _ => {}
                }
            }
        }
        for (what, types) in [("label", &f.label_types), ("text", &f.text_types)] {
            if types.is_empty() {
                return Err(format!("a family needs at least one {what} type"));
            }
            for (i, t) in types.iter().enumerate() {
                if t.name.trim().is_empty() || types[..i].iter().any(|o| o.name == t.name) {
                    return Err(format!("{what} types need names of their own ('{}')", t.name));
                }
                if t.font.trim().is_empty() {
                    return Err(format!("{}: choose a font", t.name));
                }
                if !(t.size_mm.is_finite() && t.size_mm >= 0.2 && t.size_mm <= 20.0) {
                    return Err(format!("{}: text size must be between 0.2 and 20 mm", t.name));
                }
                if !(t.width_factor.is_finite() && (0.1..=10.0).contains(&t.width_factor)) {
                    return Err(format!("{}: width factor must be between 0.1 and 10", t.name));
                }
                if !(1..=crate::styles::MAX_WEIGHT).contains(&t.line_weight) {
                    return Err(format!("{}: line weight must be 1 to {}", t.name, crate::styles::MAX_WEIGHT));
                }
                if !(t.border_offset_mm.is_finite() && (0.0..=20.0).contains(&t.border_offset_mm)) || !(t.tab_size_mm.is_finite() && (0.5..=100.0).contains(&t.tab_size_mm)) {
                    return Err(format!("{}: the border offset must be 0 to 20 mm and the tab size 0.5 to 100 mm", t.name));
                }
            }
        }
        let allowed = f.kind.params();
        let place_ok = |x: f64, y: f64, w: f64| x.is_finite() && y.is_finite() && x.abs() < 1000.0 && y.abs() < 1000.0 && w.is_finite() && (0.0..500.0).contains(&w);
        for lb in &f.labels {
            for p in &lb.params {
                if !allowed.contains(&p.param.as_str()) {
                    return Err(format!("'{}' is not a label parameter of a {}", p.param, f.kind.label().to_lowercase()));
                }
            }
            if !f.label_types.iter().any(|t| t.name == lb.label_type) {
                return Err(format!("no label type named '{}'", lb.label_type));
            }
            if !place_ok(lb.x, lb.y, lb.width_mm) || lb.width_mm < 0.5 {
                return Err("a label needs a place within a metre of the reference lines and a width of 0.5 to 500 mm".into());
            }
        }
        for t in &f.texts {
            if !f.text_types.iter().any(|x| x.name == t.text_type) {
                return Err(format!("no text type named '{}'", t.text_type));
            }
            if !place_ok(t.x, t.y, t.width_mm) {
                return Err("text needs a place within a metre of the reference lines and a width under 500 mm".into());
            }
        }
        for r in &f.regions {
            if r.loops.is_empty() || !r.loops.iter().all(|l| loop_closes(l)) {
                return Err("a region's boundary must be closed loops".into());
            }
            for e in r.loops.iter().flatten() {
                if !curve_ok(&e.curve) {
                    return Err("a region's edge has no length or lies more than a metre from the reference lines".into());
                }
                if !style_ok(&e.subcategory) {
                    return Err(format!("'{}' is not a subcategory of {}", e.subcategory, f.kind.category()));
                }
            }
            if !r.masking && !f.region_types.iter().any(|t| t.name == r.region_type) {
                return Err(format!("no filled region type named '{}'", r.region_type));
            }
        }
        Ok(())
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
        let mut new = new;
        new.read_old_drawing();
        self.check_symbol(&new)?;
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

    /// Whether a level type, an elevation marker or a section draws with this family.
    pub fn annotation_family_in_use(&self, id: Id) -> bool {
        self.level_types().iter().any(|(_, t)| t.head == Some(id)) || self.elevation_markers().iter().any(|(_, m)| m.family == Some(id)) || self.sections().iter().any(|(_, s)| s.family == Some(id))
    }

    /// Delete a family nothing uses, keeping at least one of each kind.
    pub fn delete_annotation_family(&mut self, id: Id) -> Result<(), String> {
        let Some(f) = self.annotation_family(id) else { return Err("not an annotation family".into()) };
        if self.annotation_families().iter().filter(|(_, g)| g.kind == f.kind).count() < 2 {
            return Err("the project needs at least one family of each kind".into());
        }
        if self.annotation_family_in_use(id) {
            return Err("that family is in use; choose another for the types, markers or sections that use it first".into());
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
        assert_eq!(head.labels[0].lines, vec![vec!["Level 1".to_string()]]);
        assert_eq!(head.labels[1].lines, vec![vec!["3000".to_string()]]);
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
        assert_eq!(head.labels[1].lines, vec![vec!["3000 AHD".to_string()]]);
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
        f.labels[0].params[0].param = "Width".into();
        assert!(d.set_annotation_family(id, f.clone()).unwrap_err().contains("not a label parameter"));
        f.labels[0].params[0].param = "Elevation".into();
        f.labels[0].params[0].prefix = "RL ".into();
        d.set_annotation_family(id, f).unwrap();
        let l = d.levels()[0].1.clone();
        assert_eq!(d.drawn_head(&l).unwrap().labels[0].lines, vec![vec!["RL 0".to_string()]]);
        let marker = d.annotation_families().into_iter().find(|(_, f)| f.kind == AnnotationKind::ElevationMarker).unwrap().0;
        assert!(d.delete_annotation_family(marker).is_err());
        let copy = d.duplicate_annotation_family(marker, "Marker 2").unwrap();
        assert!(d.delete_annotation_family(copy).is_ok());
    }

    #[test]
    fn a_section_mark_a_section_draws_with_cannot_be_deleted() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let (mark, _) = d.section_marker_family(None).map(|(i, f)| (i, f.clone())).unwrap();
        assert_eq!(d.annotation_family(mark).unwrap().kind.category(), "Section Marks");
        let copy = d.duplicate_annotation_family(mark, "Section Mark 2").unwrap();
        let s = d.add_section(lvl, crate::geom::Point::new(0.0, 0.0), crate::geom::Point::new(5000.0, 0.0), false).unwrap();
        d.patch_section(s, &serde_json::json!({ "family": copy })).unwrap();
        assert!(d.annotation_family_in_use(copy));
        assert!(d.delete_annotation_family(copy).unwrap_err().contains("in use"));
        d.patch_section(s, &serde_json::json!({ "family": mark })).unwrap();
        assert!(d.delete_annotation_family(copy).is_ok());
    }

    #[test]
    fn older_shapes_become_lines_over_masking_regions_and_solid_filled_regions() {
        let d = Document::new_project();
        let (_, f) = d.section_marker_family(None).unwrap();
        // the circle and the line across it are lines; the circle also masks what is behind it; the arrow is solid
        assert_eq!(f.lines.len(), 2);
        assert_eq!(f.regions.len(), 2);
        assert!(f.regions[0].masking && !f.regions[0].pointer);
        assert!(!f.regions[1].masking && f.regions[1].pointer && f.regions[1].region_type == SOLID_FILLED);
        let dr = d.symbol_drawing(f, false);
        assert_eq!(dr.body.fills.len(), 1);
        assert!(dr.body.fills[0].masking && dr.body.fills[0].foreground.is_none());
        assert_eq!(dr.body.strokes.len(), 2, "the regions' invisible edges are left out of a project");
        assert_eq!(dr.pointer.fills[0].foreground.as_deref(), Some(SOLID_FILL));
        assert!((dr.pointer.fills[0].area_mm2 - 6.0).abs() < 1e-6, "{}", dr.pointer.fills[0].area_mm2);
        // the family editor keeps them, marked hidden
        let ed = d.symbol_drawing(f, true);
        assert_eq!(ed.body.strokes.iter().filter(|s| s.hidden).count(), 1);
    }

    #[test]
    fn region_sketches_close_into_loops_with_holes_and_their_own_edge_styles() {
        let p = |x: f64, y: f64| Point::new(x, y);
        let e = |a: Point, b: Point, sub: &str| RegionEdge { curve: Curve::line(a, b), subcategory: sub.into() };
        // a 6 mm square drawn out of order, one edge backwards, with a circle inside it
        let edges = vec![
            e(p(0.0, 0.0), p(6.0, 0.0), ""),
            e(p(6.0, 6.0), p(0.0, 6.0), INVISIBLE_LINES),
            e(p(6.0, 6.0), p(6.0, 0.0), ""),
            e(p(0.0, 6.0), p(0.0, 0.0), ""),
            RegionEdge { curve: Curve::Circle { centre: p(3.0, 3.0), radius: 1.0 }, subcategory: String::new() },
        ];
        let loops = region_loops(&edges).unwrap();
        assert_eq!(loops.len(), 2);
        assert!(loops.iter().all(|l| loop_closes(l)));
        assert!(loops[0].iter().any(|x| x.subcategory == INVISIBLE_LINES), "an edge keeps its style when it runs the other way");
        // a gap is found where it is, on paper
        let err = region_loops(&edges[..3]).unwrap_err();
        let at = err.at.unwrap();
        assert!(at[0].abs() <= 6.0 && at[1].abs() <= 6.0, "{at:?}");
        let mut d = Document::new_project();
        let (id, mut f) = d.section_marker_family(None).map(|(i, f)| (i, f.clone())).unwrap();
        f.regions.push(AnnRegion { loops, masking: false, region_type: SOLID_FILLED.into(), visible: true, pointer: false });
        d.set_annotation_family(id, f.clone()).unwrap();
        let dr = d.symbol_drawing(d.annotation_family(id).unwrap(), false);
        let fill = dr.body.fills.last().unwrap();
        assert!((fill.area_mm2 - (36.0 - std::f64::consts::PI)).abs() < 0.05, "the circle is a hole: {}", fill.area_mm2);
        // an open loop, a model pattern and a style the category does not have are refused
        let mut open = f.clone();
        open.regions.last_mut().unwrap().loops[0].pop();
        assert!(d.set_annotation_family(id, open).unwrap_err().contains("closed"));
        let mut model = f.clone();
        let brick = d.fill_patterns().into_iter().find(|(_, p)| p.pattern_type == PatternKind::Model).map(|(_, p)| p.name.clone());
        if let Some(b) = brick {
            model.region_types[0].foreground = Some(b);
            assert!(d.set_annotation_family(id, model).unwrap_err().contains("drafting"));
        }
        let mut styled = f.clone();
        styled.lines[0].subcategory = "Heavy".into();
        assert!(d.set_annotation_family(id, styled.clone()).unwrap_err().contains("subcategory"));
        // once Object Styles has it, the line draws with its weight
        d.patch_object_styles(&serde_json::json!({ "annotation": { "Section Marks": { "subcategories": { "Heavy": { "projection_weight": 4, "color": [200, 0, 0], "pattern": "Solid", "subcategories": {} } } } } })).unwrap();
        d.set_annotation_family(id, styled).unwrap();
        let dr = d.symbol_drawing(d.annotation_family(id).unwrap(), false);
        let s = dr.body.strokes.iter().find(|s| s.line == Some(0)).unwrap();
        assert_eq!((s.width_mm, s.color), (0.5, [200, 0, 0]));
    }

    #[test]
    fn older_labels_become_label_types_and_parameters() {
        let d = Document::new_project();
        let (_, head) = d.annotation_families().into_iter().find(|(_, f)| f.name == "Level Head").unwrap();
        assert!(head.label_types.iter().any(|t| t.name == "2.5mm") && head.label_types.iter().any(|t| t.name == "2.5mm Bold" && t.bold));
        assert_eq!(head.labels[0].label_type, "2.5mm Bold");
        assert_eq!(head.labels[0].width_mm, 40.0, "a name gets a wide box");
        let (_, mark) = d.section_marker_family(None).unwrap();
        assert!(mark.label_types.iter().any(|t| t.name == "2mm Bold" && (t.size_mm - 2.0).abs() < 1e-9));
        assert!(mark.labels.iter().all(|l| !l.has_old() && l.params.len() == 1 && !l.params[0].brk));
        // a label saved with two lines reads as two parameters with a break between them
        let old: AnnLabel = serde_json::from_value(serde_json::json!({ "x": 0.0, "y": 0.0, "size_mm": 3.0, "lines": [{ "param": "Name" }, { "param": "Elevation", "suffix": " AHD" }] })).unwrap();
        let mut f = head.clone();
        f.labels = vec![old];
        assert!(f.has_old_drawing() && f.read_old_drawing());
        assert_eq!(f.labels[0].label_type, "3mm");
        assert!(f.labels[0].params[0].brk && !f.labels[0].params[1].brk);
        assert!(f.label_types.iter().any(|t| t.name == "3mm"));
    }

    #[test]
    fn labels_join_their_parameters_break_lines_and_leave_out_what_has_no_value() {
        let mut d = Document::new_project();
        let (id, mut f) = d.annotation_families().into_iter().find(|(_, f)| f.name == "Level Head").map(|(i, f)| (i, f.clone())).unwrap();
        let mut name = LabelParam::new("Name", "L-", "", "", false);
        name.spaces = 2;
        f.labels[0].params = vec![name, LabelParam::new("Elevation", "", "", "", true), LabelParam::new("Level Number", "No. ", "", "", false)];
        f.labels[0].wrap_params_only = true;
        d.set_annotation_family(id, f).unwrap();
        let t = d.level_types()[0].0;
        d.patch_level_type(t, &serde_json::json!({ "head": id })).unwrap();
        let l = d.levels()[1].1.clone();
        let head = d.drawn_head(&l).unwrap();
        assert_eq!(head.labels[0].lines, vec![vec!["L-Level 1  ".to_string(), "3000".to_string()]], "no number: its line goes");
        assert_eq!(head.labels[0].wrap, Wrap::Pieces);
        assert!(head.labels[0].style.bold && (head.labels[0].style.size_mm - 2.5).abs() < 1e-9);
    }

    #[test]
    fn text_draws_as_written_and_types_are_checked() {
        let mut d = Document::new_project();
        let (id, mut f) = d.section_marker_family(None).map(|(i, f)| (i, f.clone())).unwrap();
        f.texts.push(AnnText { x: 10.0, y: 0.0, text: "SEE\nDETAIL".into(), text_type: "2.5mm".into(), halign: HAlign::Left, valign: VAlign::Middle, width_mm: 0.0, keep_readable: true, visible: true });
        f.texts.push(AnnText { x: 10.0, y: -5.0, text: "hidden".into(), text_type: "2.5mm".into(), halign: HAlign::Left, valign: VAlign::Middle, width_mm: 0.0, keep_readable: true, visible: false });
        let mut bad = f.clone();
        bad.texts[0].text_type = "9mm".into();
        assert!(d.set_annotation_family(id, bad).unwrap_err().contains("text type"));
        let mut bad = f.clone();
        bad.text_types[0].size_mm = 30.0;
        assert!(d.set_annotation_family(id, bad).unwrap_err().contains("text size"));
        let mut bad = f.clone();
        bad.labels[0].label_type = "Huge".into();
        assert!(d.set_annotation_family(id, bad).unwrap_err().contains("label type"));
        d.set_annotation_family(id, f).unwrap();
        let lvl = d.levels()[0].0;
        let s = d.add_section(lvl, Point::new(0.0, 0.0), Point::new(5000.0, 0.0), false).unwrap();
        let drawn = d.section_labels(d.section(s).unwrap());
        let text: Vec<_> = drawn.iter().filter(|t| t.text.is_some()).collect();
        assert_eq!(text.len(), 1, "text that is not visible stays out of a project");
        assert_eq!(text[0].lines, vec![vec!["SEE".to_string()], vec!["DETAIL".to_string()]]);
        let f = d.annotation_family(id).unwrap();
        assert_eq!(d.symbol_texts(f, &|_| None, true).iter().filter(|t| t.text.is_some()).count(), 2, "the family editor shows it greyed");
    }
}
