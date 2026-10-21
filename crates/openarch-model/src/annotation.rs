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
            broken_style: BrokenStyle::default(),
            reference_label: reference_label(),
            old_shapes: shapes,
            old_pointer: pointer,
        };
        f.read_old_shapes();
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
                AnnLabel { x: 6.5, y: 0.8, halign: HAlign::Left, valign: VAlign::Bottom, size_mm: 2.5, bold: true, lines: vec![LabelLine::new("Name", "", "", "Level 1")] },
                AnnLabel { x: 6.5, y: -0.8, halign: HAlign::Left, valign: VAlign::Top, size_mm: 2.5, bold: false, lines: vec![LabelLine::new(elevation_param, "", suffix, "3000")] },
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
                AnnLabel { x: -6.0, y: 0.0, halign: HAlign::Right, valign: VAlign::Middle, size_mm: 2.5, bold: false, lines: vec![LabelLine::new("View Name", "", "", "Section 1")] },
                AnnLabel { x: 0.0, y: 2.0, halign: HAlign::Center, valign: VAlign::Middle, size_mm: 2.0, bold: true, lines: vec![LabelLine::new("View Number", "", "", "1")] },
                AnnLabel { x: 0.0, y: -2.0, halign: HAlign::Center, valign: VAlign::Middle, size_mm: 2.0, bold: false, lines: vec![LabelLine::new("Sheet Number", "", "", "A101")] },
                // shows on reference sections only
                AnnLabel { x: 0.0, y: -5.0, halign: HAlign::Center, valign: VAlign::Top, size_mm: 2.0, bold: false, lines: vec![LabelLine::new("Reference Label", "", "", "SIM")] },
            ],
        )
    }

    /// Read the shapes of an older project into lines and regions. A filled shape becomes a solid filled region with
    /// invisible edges; an outline (which used to hide what lay behind it) becomes lines over a masking region.
    /// Returns whether there was anything to read.
    pub fn read_old_shapes(&mut self) -> bool {
        if self.old_shapes.is_empty() && self.old_pointer.is_empty() {
            return false;
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

    /// A filled region type by name (the first type when there is none of that name).
    pub fn region_type(&self, name: &str) -> Option<&RegionType> {
        self.region_types.iter().find(|t| t.name == name).or_else(|| self.region_types.first())
    }
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
    let scaled: Vec<Curve> = edges.iter().map(|e| e.curve.scaled(SKETCH_SCALE)).collect();
    let order = curve::sketch_loop_order(&scaled).map_err(|e| SketchError { at: e.at.map(|a| [a[0] / SKETCH_SCALE, a[1] / SKETCH_SCALE]), ..e })?;
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
    pub paths: SymbolPaths,
    pub labels: Vec<DrawnLabel>,
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
        Some(DrawnHead { family: id, paths: self.symbol_drawing(f, false).body, labels })
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

    /// Families saved before lines and regions get them from their shapes, and Object Styles gets the categories
    /// their line styles belong to.
    fn upgrade_annotation_drawings(&mut self) {
        let fixes: Vec<(Id, AnnotationFamily)> = self
            .annotation_families()
            .into_iter()
            .filter(|(_, f)| !f.old_shapes.is_empty() || !f.old_pointer.is_empty())
            .map(|(id, f)| {
                let mut n = f.clone();
                n.read_old_shapes();
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
        let mut new = new;
        new.read_old_shapes();
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
}
