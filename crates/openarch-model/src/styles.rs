//! Project-level graphics standards: line patterns, fill patterns, materials,
//! Object Styles (line weight, colour, pattern and material per category and
//! subcategory) and the line weight table. View overrides sit on top of these.
//!
//! Everything here is stored as elements, so it is saved, undoable and (later)
//! transferable between projects like Transfer Project Standards.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::doc::{apply_patch, Document, Tx};
use crate::element::*;
use crate::family::{ParamKind, ParamValue};
use crate::view::{Color, ANNOTATION_CATEGORIES, MODEL_CATEGORIES, SYMBOL_CATEGORIES};

pub const SOLID: &str = "Solid";
pub const SOLID_FILL: &str = "Solid fill";
/// Line weights are numbered 1 to 16.
pub const MAX_WEIGHT: u8 = 16;
/// The view scales (denominators) the line weight table has a column for.
pub const WEIGHT_SCALES: [u32; 12] = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000];

// ---------------------------------------------------------------- line patterns

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    Dash,
    Space,
    Dot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternSegment {
    pub kind: SegmentKind,
    /// Length on paper in mm. Ignored for dots.
    #[serde(default)]
    pub length_mm: f64,
}

/// A line pattern (Manage > Additional Settings > Line Patterns). No segments
/// means a solid line. Lengths are paper lengths, so they scale with the view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinePattern {
    pub name: String,
    #[serde(default)]
    pub segments: Vec<PatternSegment>,
    #[serde(default)]
    pub built_in: bool,
}

impl LinePattern {
    /// Dash and space lengths for a canvas or PDF dash array, in paper mm. Dots
    /// are drawn as very short dashes.
    pub fn dash_array(&self) -> Vec<f64> {
        let mut out: Vec<f64> = vec![];
        for s in &self.segments {
            let (is_dash, len) = match s.kind {
                SegmentKind::Dash => (true, s.length_mm),
                SegmentKind::Dot => (true, 0.15),
                SegmentKind::Space => (false, s.length_mm),
            };
            let expecting_dash = out.len() % 2 == 0;
            if expecting_dash != is_dash {
                // two dashes or two spaces in a row: put a zero-length one between
                out.push(0.0);
            }
            out.push(len);
        }
        if out.len() % 2 == 1 {
            out.push(1.0);
        }
        out
    }
}

fn seg(kind: SegmentKind, l: f64) -> PatternSegment {
    PatternSegment { kind, length_mm: l }
}

pub fn builtin_line_patterns() -> Vec<LinePattern> {
    use SegmentKind::{Dash as D, Dot as O, Space as S};
    let mk = |name: &str, segs: &[(SegmentKind, f64)]| LinePattern {
        name: name.into(),
        segments: segs.iter().map(|(k, l)| seg(*k, *l)).collect(),
        built_in: true,
    };
    vec![
        mk(SOLID, &[]),
        mk("Dash", &[(D, 3.0), (S, 1.5)]),
        mk("Dot", &[(O, 0.0), (S, 1.2)]),
        mk("Dash dot", &[(D, 3.0), (S, 1.2), (O, 0.0), (S, 1.2)]),
        mk("Dash dot dot", &[(D, 3.0), (S, 1.2), (O, 0.0), (S, 1.2), (O, 0.0), (S, 1.2)]),
        mk("Hidden", &[(D, 1.5), (S, 1.0)]),
        mk("Centre", &[(D, 12.0), (S, 2.5), (D, 2.5), (S, 2.5)]),
        mk("Divide", &[(D, 10.0), (S, 2.5), (D, 2.5), (S, 2.5), (D, 2.5), (S, 2.5)]),
        mk("Phantom", &[(D, 12.0), (S, 2.0), (D, 2.0), (S, 2.0), (D, 2.0), (S, 2.0)]),
    ]
}

// ---------------------------------------------------------------- fill patterns

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    /// Fixed size on paper; the spacing scales with the view scale.
    #[default]
    Drafting,
    /// Fixed size in the model, as a real brick or tile would be.
    Model,
}

/// One family of parallel lines in a fill pattern (the pattern line: angle,
/// origin, shift, spacing and dashes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HatchLine {
    pub angle_deg: f64,
    #[serde(default)]
    pub origin: [f64; 2],
    /// Offset along the line between one line and the next.
    #[serde(default)]
    pub shift: f64,
    /// Distance between neighbouring lines, across them.
    pub spacing: f64,
    /// Dash, space, dash, space ... along the line. Empty is continuous.
    #[serde(default)]
    pub dashes: Vec<f64>,
}

/// A fill pattern. Lengths are paper mm for drafting patterns and model mm for
/// model patterns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillPattern {
    pub name: String,
    #[serde(default)]
    pub pattern_type: PatternKind,
    #[serde(default)]
    pub solid: bool,
    #[serde(default)]
    pub lines: Vec<HatchLine>,
    #[serde(default)]
    pub built_in: bool,
}

pub fn builtin_fill_patterns() -> Vec<FillPattern> {
    let line = |angle: f64, spacing: f64, shift: f64, dashes: &[f64]| HatchLine { angle_deg: angle, origin: [0.0, 0.0], shift, spacing, dashes: dashes.to_vec() };
    let drafting = |name: &str, lines: Vec<HatchLine>| FillPattern { name: name.into(), pattern_type: PatternKind::Drafting, solid: false, lines, built_in: true };
    vec![
        FillPattern { name: SOLID_FILL.into(), pattern_type: PatternKind::Drafting, solid: true, lines: vec![], built_in: true },
        drafting("Diagonal up", vec![line(45.0, 2.5, 0.0, &[])]),
        drafting("Diagonal down", vec![line(135.0, 2.5, 0.0, &[])]),
        drafting("Diagonal crosshatch", vec![line(45.0, 2.5, 0.0, &[]), line(135.0, 2.5, 0.0, &[])]),
        drafting("Horizontal", vec![line(0.0, 2.0, 0.0, &[])]),
        drafting("Vertical", vec![line(90.0, 2.0, 0.0, &[])]),
        drafting("Crosshatch", vec![line(0.0, 2.5, 0.0, &[]), line(90.0, 2.5, 0.0, &[])]),
        // aggregate speckle for concrete: short dashes, each row shifted
        drafting("Concrete", vec![line(0.0, 1.6, 0.9, &[0.12, 1.8]), line(0.0, 1.6, 0.0, &[0.12, 3.9])]),
        drafting("Sand", vec![line(0.0, 1.0, 0.7, &[0.1, 1.1])]),
        drafting("Insulation", vec![line(45.0, 1.8, 0.0, &[3.0, 1.0]), line(135.0, 1.8, 0.0, &[3.0, 1.0])]),
        // courses of brick at 76 mm (model size), stretcher bond
        FillPattern {
            name: "Brick".into(),
            pattern_type: PatternKind::Model,
            solid: false,
            lines: vec![line(0.0, 76.0, 0.0, &[]), line(90.0, 230.0, 115.0, &[76.0, 76.0])],
            built_in: true,
        },
    ]
}

// ---------------------------------------------------------------- materials

/// How a material's surface or cut pattern draws.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PatternGraphics {
    /// Fill pattern name. `None` draws no pattern.
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub color: Option<Color>,
}

/// The Graphics tab of the Material Browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialGraphics {
    /// Shading colour used in shaded views.
    pub shading_color: Color,
    /// 0 opaque to 100 clear.
    #[serde(default)]
    pub shading_transparency: u8,
    #[serde(default)]
    pub surface_foreground: PatternGraphics,
    #[serde(default)]
    pub surface_background: PatternGraphics,
    #[serde(default)]
    pub cut_foreground: PatternGraphics,
    #[serde(default)]
    pub cut_background: PatternGraphics,
}

impl Default for MaterialGraphics {
    fn default() -> Self {
        MaterialGraphics {
            shading_color: [190, 190, 190],
            shading_transparency: 0,
            surface_foreground: PatternGraphics::default(),
            surface_background: PatternGraphics::default(),
            cut_foreground: PatternGraphics::default(),
            cut_background: PatternGraphics::default(),
        }
    }
}

/// The Physical tab (structural and thermal values used by analysis and by
/// the schedule engine). All optional.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PhysicalAsset {
    #[serde(default)]
    pub density_kg_m3: Option<f64>,
    #[serde(default)]
    pub conductivity_w_mk: Option<f64>,
    #[serde(default)]
    pub specific_heat_j_kgk: Option<f64>,
}

/// The Identity tab.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MaterialIdentity {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub keynote: String,
    #[serde(default)]
    pub mark: String,
}

/// A project material. Layers, object styles and family parameters refer to
/// materials by name, so names are unique and renaming updates every user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Material class, e.g. Concrete, Masonry, Wood, Metal, Glass, Generic.
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub graphics: MaterialGraphics,
    #[serde(default)]
    pub physical: PhysicalAsset,
    #[serde(default)]
    pub identity: MaterialIdentity,
}

fn mat(name: &str, class: &str, shade: Color, cut: Option<&str>, surface: Option<&str>, density: f64, k: f64) -> Material {
    let pat = |p: Option<&str>| PatternGraphics { pattern: p.map(String::from), color: None };
    Material {
        name: name.into(),
        class: class.into(),
        graphics: MaterialGraphics {
            shading_color: shade,
            shading_transparency: 0,
            surface_foreground: pat(surface),
            surface_background: PatternGraphics::default(),
            cut_foreground: pat(cut),
            cut_background: PatternGraphics::default(),
        },
        // typical specific heats by class, so a new project's assemblies report a thermal mass
        physical: PhysicalAsset {
            density_kg_m3: Some(density),
            conductivity_w_mk: Some(k),
            specific_heat_j_kgk: match class {
                "Gypsum" => Some(1090.0),
                "Wood" => Some(1600.0),
                "Masonry" | "Glass" => Some(840.0),
                "Concrete" => Some(880.0),
                "Metal" => Some(450.0),
                _ => None,
            },
        },
        identity: MaterialIdentity::default(),
    }
}

/// Starter materials for the default wall and floor types, with plausible
/// Australian values for density and conductivity.
pub fn starter_materials() -> Vec<Material> {
    vec![
        mat("Plasterboard", "Gypsum", [235, 233, 226], None, None, 700.0, 0.25),
        mat("Timber Frame", "Wood", [196, 160, 112], Some("Diagonal up"), None, 500.0, 0.13),
        mat("Face Brick", "Masonry", [176, 98, 72], Some("Diagonal crosshatch"), Some("Brick"), 1800.0, 0.8),
        mat("Concrete", "Concrete", [168, 170, 172], Some("Concrete"), None, 2400.0, 1.6),
        mat("Concrete Block", "Masonry", [180, 180, 176], Some("Crosshatch"), None, 1900.0, 0.9),
        mat("Timber Flooring", "Wood", [206, 170, 120], Some("Horizontal"), Some("Horizontal"), 650.0, 0.15),
        mat("Timber Joist", "Wood", [190, 150, 100], Some("Diagonal up"), None, 500.0, 0.13),
        mat("Glass", "Glass", [170, 205, 225], None, None, 2500.0, 1.0),
        mat("Roof Sheeting", "Metal", [138, 146, 152], None, None, 7850.0, 50.0),
        mat("Roof Tile", "Masonry", [150, 92, 76], Some("Diagonal crosshatch"), None, 2100.0, 1.0),
        mat("Timber Batten", "Wood", [200, 164, 116], Some("Diagonal up"), None, 500.0, 0.13),
        mat("Timber Rafter", "Wood", [190, 150, 100], Some("Diagonal up"), None, 500.0, 0.13),
        mat("Steel Purlin", "Metal", [150, 156, 164], Some("Diagonal crosshatch"), None, 7850.0, 50.0),
    ]
}

// ---------------------------------------------------------------- object styles

/// One row of the Object Styles dialog: a category or one of its subcategories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectStyle {
    /// Line weight (1 to 16) when the object is not cut.
    pub projection_weight: u8,
    /// Line weight when cut. `None` for objects that are never cut.
    #[serde(default)]
    pub cut_weight: Option<u8>,
    pub color: Color,
    /// Line pattern name.
    #[serde(default = "solid_name")]
    pub pattern: String,
    /// Material name. `None` for annotation objects, which carry none.
    #[serde(default)]
    pub material: Option<String>,
    /// Subcategories (one level only).
    #[serde(default)]
    pub subcategories: BTreeMap<String, ObjectStyle>,
    /// Built-in subcategories cannot be deleted.
    #[serde(default)]
    pub built_in: bool,
}

fn solid_name() -> String {
    SOLID.into()
}

pub const INK: Color = [29, 34, 41];

impl ObjectStyle {
    pub fn new(projection: u8, cut: Option<u8>) -> ObjectStyle {
        ObjectStyle { projection_weight: projection, cut_weight: cut, color: INK, pattern: SOLID.into(), material: None, subcategories: BTreeMap::new(), built_in: false }
    }
    /// The weight for a cut or uncut object.
    pub fn weight(&self, cut: bool) -> u8 {
        if cut {
            self.cut_weight.unwrap_or(self.projection_weight)
        } else {
            self.projection_weight
        }
    }
}

/// The three tabs of the Object Styles dialog.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ObjectStyles {
    #[serde(default)]
    pub model: BTreeMap<String, ObjectStyle>,
    #[serde(default)]
    pub annotation: BTreeMap<String, ObjectStyle>,
    /// One entry per imported CAD layer (arrives with DXF import).
    #[serde(default)]
    pub imported: BTreeMap<String, ObjectStyle>,
}

impl ObjectStyles {
    pub fn category(&self, name: &str) -> Option<&ObjectStyle> {
        self.model.get(name).or_else(|| self.annotation.get(name)).or_else(|| self.imported.get(name))
    }

    /// The style for a subcategory, falling back to its category.
    pub fn resolve(&self, category: &str, sub: Option<&str>) -> Option<&ObjectStyle> {
        let c = self.category(category)?;
        sub.and_then(|s| c.subcategories.get(s)).or(Some(c))
    }

    pub fn defaults() -> ObjectStyles {
        let sub = |p: u8, c: Option<u8>, pat: &str| ObjectStyle { pattern: pat.into(), built_in: true, ..ObjectStyle::new(p, c) };
        let mut model = BTreeMap::new();
        for &name in MODEL_CATEGORIES {
            let (p, c) = match name {
                "Walls" | "Floors" | "Roofs" | "Columns" | "Structural Columns" | "Structural Foundations" | "Ramps" | "Stairs" | "Curtain Systems" => (1, Some(3)),
                "Site" | "Topography" | "Planting" | "Parking" | "Rooms" | "Mass" | "Lighting Fixtures" | "Electrical Fixtures" | "Electrical Equipment" => (1, None),
                _ => (1, Some(2)),
            };
            let mut s = ObjectStyle::new(p, c);
            let hidden = || sub(1, None, "Hidden");
            match name {
                "Walls" => {
                    s.subcategories.insert("Common Edges".into(), sub(1, None, SOLID));
                    s.subcategories.insert("Hidden Lines".into(), hidden());
                }
                "Floors" => {
                    s.subcategories.insert("Interior Edges".into(), sub(1, None, SOLID));
                    s.subcategories.insert("Hidden Lines".into(), hidden());
                }
                "Doors" => {
                    for n in ["Frame/Mullion", "Glass", "Panel", "Plan Swing", "Elevation Swing", "Opening"] {
                        s.subcategories.insert(n.into(), sub(1, if n == "Panel" { Some(2) } else { None }, SOLID));
                    }
                    s.subcategories.insert("Hidden Lines".into(), hidden());
                }
                "Windows" => {
                    for n in ["Frame/Mullion", "Glass", "Sash", "Elevation Swing"] {
                        s.subcategories.insert(n.into(), sub(1, if n == "Frame/Mullion" { Some(2) } else { None }, SOLID));
                    }
                    s.subcategories.insert("Hidden Lines".into(), hidden());
                }
                "Roofs" | "Ceilings" | "Stairs" | "Columns" | "Casework" | "Furniture" => {
                    s.subcategories.insert("Hidden Lines".into(), hidden());
                }
                _ => {}
            }
            model.insert(name.to_string(), s);
        }
        // glass reads lighter than its frame
        if let Some(g) = model.get_mut("Doors").and_then(|d| d.subcategories.get_mut("Glass")) {
            g.material = Some("Glass".into());
        }
        if let Some(g) = model.get_mut("Windows").and_then(|d| d.subcategories.get_mut("Glass")) {
            g.material = Some("Glass".into());
        }
        let mut annotation = BTreeMap::new();
        for &name in ANNOTATION_CATEGORIES.iter().chain(SYMBOL_CATEGORIES) {
            annotation.insert(name.to_string(), ObjectStyle::new(1, None));
        }
        // line styles: the Lines category and its styles
        let mut lines = ObjectStyle::new(1, None);
        for (n, w, pat) in [
            ("Thin Lines", 1, SOLID),
            ("Medium Lines", 3, SOLID),
            ("Wide Lines", 5, SOLID),
            ("Hidden Lines", 1, "Hidden"),
            ("Axis of Rotation", 1, "Centre"),
            // the styles views use for elements seen beyond the cut plane or overhead
            ("<Beyond>", 1, "Dash"),
            ("<Overhead>", 1, "Dash"),
        ] {
            let mut s = sub(w, None, pat);
            if n == "<Beyond>" {
                s.color = [120, 126, 135];
            }
            lines.subcategories.insert(n.into(), s);
        }
        annotation.insert("Lines".into(), lines);
        annotation.insert("Grids".into(), ObjectStyle { pattern: "Centre".into(), ..ObjectStyle::new(1, None) });
        annotation.insert("Dimensions".into(), ObjectStyle::new(1, None));
        ObjectStyles { model, annotation, imported: BTreeMap::new() }
    }

    /// Names of every material the styles refer to.
    fn materials(&self) -> impl Iterator<Item = &String> {
        [&self.model, &self.annotation, &self.imported]
            .into_iter()
            .flat_map(|m| m.values())
            .flat_map(|c| c.material.iter().chain(c.subcategories.values().flat_map(|s| s.material.iter())))
    }
}

// ---------------------------------------------------------------- line weights

/// Manage > Additional Settings > Line Weights: the paper width in mm of each
/// of the 16 weights at each view scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineWeights {
    /// Scale denominators, one per column.
    pub scales: Vec<u32>,
    /// `model[weight - 1][scale column]`, mm.
    pub model: Vec<Vec<f64>>,
    /// Perspective views, one width per weight, mm.
    pub perspective: Vec<f64>,
    /// Annotation lines (dimensions, tags), one width per weight, mm.
    pub annotation: Vec<f64>,
}

const PENS: [f64; 16] = [0.18, 0.25, 0.35, 0.50, 0.70, 1.00, 1.40, 2.00, 2.50, 3.00, 3.50, 4.00, 5.00, 6.00, 7.00, 8.00];

impl Default for LineWeights {
    fn default() -> Self {
        // pens follow the ISO 128 series; at small scales (1:200 and smaller)
        // heavy weights draw thinner so a dense drawing stays readable
        let shrink = |scale: u32| match scale {
            0..=100 => 1.0,
            200 => 0.8,
            500 => 0.65,
            1000 => 0.5,
            2000 => 0.4,
            _ => 0.3,
        };
        let model = PENS
            .iter()
            .enumerate()
            .map(|(i, pen)| WEIGHT_SCALES.iter().map(|s| if i < 2 { *pen } else { (pen * shrink(*s)).max(0.13) }).map(|w| (w * 100.0_f64).round() / 100.0).collect())
            .collect();
        LineWeights { scales: WEIGHT_SCALES.to_vec(), model, perspective: PENS.to_vec(), annotation: PENS.to_vec() }
    }
}

impl LineWeights {
    /// Paper width in mm of `weight` (1 to 16) in a view at 1:`scale`. Scales
    /// between columns use the nearest column on a log scale.
    pub fn model_width(&self, weight: u8, scale: u32) -> f64 {
        let row = &self.model[(weight.clamp(1, MAX_WEIGHT) - 1) as usize];
        let target = (scale.max(1) as f64).ln();
        let col = self
            .scales
            .iter()
            .enumerate()
            .min_by(|a, b| (target - (*a.1 as f64).ln()).abs().total_cmp(&(target - (*b.1 as f64).ln()).abs()))
            .map_or(0, |(i, _)| i);
        row.get(col).copied().unwrap_or(0.18)
    }

    pub fn annotation_width(&self, weight: u8) -> f64 {
        self.annotation.get((weight.clamp(1, MAX_WEIGHT) - 1) as usize).copied().unwrap_or(0.18)
    }

    fn validate(&self) -> Result<(), String> {
        if self.scales.is_empty() || self.scales.windows(2).any(|w| w[0] >= w[1]) {
            return Err("scales must be increasing".into());
        }
        if self.model.len() != MAX_WEIGHT as usize || self.model.iter().any(|r| r.len() != self.scales.len()) {
            return Err("the model table needs 16 weights with one width per scale".into());
        }
        if self.perspective.len() != MAX_WEIGHT as usize || self.annotation.len() != MAX_WEIGHT as usize {
            return Err("perspective and annotation need 16 widths".into());
        }
        let all = self.model.iter().flatten().chain(&self.perspective).chain(&self.annotation);
        if all.clone().any(|w| !w.is_finite() || *w <= 0.0 || *w > 20.0) {
            return Err("line widths must be between 0 and 20 mm".into());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- document

/// What `Document::style_item` hands back.
pub enum StyleItem<'a> {
    LinePattern(&'a LinePattern),
    FillPattern(&'a FillPattern),
    Material(&'a Material),
}

impl Tx<'_> {
    /// Add the built-in standards that are missing. Safe to call on any project.
    pub fn add_style_defaults(&mut self) {
        let have = |tx: &Tx, f: &dyn Fn(&Element) -> Option<String>| -> Vec<String> { tx.doc_elements().filter_map(|(_, e)| f(e)).collect() };
        let lp = have(self, &|e| if let Element::LinePattern(p) = e { Some(p.name.clone()) } else { None });
        for p in builtin_line_patterns().into_iter().filter(|p| !lp.contains(&p.name)) {
            self.add(Element::LinePattern(p));
        }
        let fp = have(self, &|e| if let Element::FillPattern(p) = e { Some(p.name.clone()) } else { None });
        for p in builtin_fill_patterns().into_iter().filter(|p| !fp.contains(&p.name)) {
            self.add(Element::FillPattern(p));
        }
        let ms = have(self, &|e| if let Element::Material(m) = e { Some(m.name.clone()) } else { None });
        let mut known = ms.clone();
        for m in starter_materials().into_iter().filter(|m| !ms.contains(&m.name)) {
            known.push(m.name.clone());
            self.add(Element::Material(m));
        }
        // materials used by layers in older projects get a material of their own
        let mut used: Vec<(String, bool)> = vec![];
        for (_, e) in self.doc_elements() {
            let layers = match e {
                Element::WallType(t) => &t.layers,
                Element::FloorType(t) => &t.layers,
                Element::RoofType(t) => &t.layers,
                _ => continue,
            };
            used.extend(layers.iter().filter(|l| !l.is_void).map(|l| (l.material.clone(), l.core)));
        }
        for (name, _) in used {
            if !known.contains(&name) && !name.trim().is_empty() {
                known.push(name.clone());
                self.add(Element::Material(guess_material(&name)));
            }
        }
        if !self.doc_elements().any(|(_, e)| matches!(e, Element::ObjectStyles(_))) {
            self.add(Element::ObjectStyles(ObjectStyles::defaults()));
        }
        if !self.doc_elements().any(|(_, e)| matches!(e, Element::LineWeights(_))) {
            self.add(Element::LineWeights(LineWeights::default()));
        }
    }

    fn doc_elements(&self) -> impl Iterator<Item = (&Id, &Element)> {
        self.doc_ref().elements.iter()
    }
}

/// A material for a layer name found in an older project, classed by keyword.
fn guess_material(name: &str) -> Material {
    let l = name.to_lowercase();
    let (class, shade, cut): (&str, Color, Option<&str>) = if l.contains("concrete block") || l.contains("blockwork") || l.contains("brick") {
        ("Masonry", [176, 98, 72], Some("Diagonal crosshatch"))
    } else if l.contains("concrete") || l.contains("slab") {
        ("Concrete", [168, 170, 172], Some("Concrete"))
    } else if l.contains("timber") || l.contains("wood") || l.contains("ply") {
        ("Wood", [196, 160, 112], Some("Diagonal up"))
    } else if l.contains("plaster") || l.contains("gyp") {
        ("Gypsum", [235, 233, 226], None)
    } else if l.contains("steel") || l.contains("metal") {
        ("Metal", [150, 156, 164], Some("Diagonal crosshatch"))
    } else {
        ("Generic", [190, 190, 190], None)
    };
    let mut m = mat(name, class, shade, cut, None, 0.0, 0.0);
    m.physical = PhysicalAsset::default();
    m
}

impl Document {
    pub fn object_styles(&self) -> Option<(Id, &ObjectStyles)> {
        self.elements.iter().find_map(|(id, e)| if let Element::ObjectStyles(s) = e { Some((*id, s)) } else { None })
    }

    pub fn line_weights(&self) -> Option<(Id, &LineWeights)> {
        self.elements.iter().find_map(|(id, e)| if let Element::LineWeights(s) = e { Some((*id, s)) } else { None })
    }

    pub fn line_patterns(&self) -> Vec<(Id, &LinePattern)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::LinePattern(p) = e { Some((*id, p)) } else { None }).collect()
    }

    pub fn fill_patterns(&self) -> Vec<(Id, &FillPattern)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::FillPattern(p) = e { Some((*id, p)) } else { None }).collect()
    }

    pub fn materials(&self) -> Vec<(Id, &Material)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::Material(m) = e { Some((*id, m)) } else { None }).collect()
    }

    pub fn line_pattern(&self, name: &str) -> Option<&LinePattern> {
        self.line_patterns().into_iter().map(|(_, p)| p).find(|p| p.name == name)
    }

    pub fn fill_pattern(&self, name: &str) -> Option<&FillPattern> {
        self.fill_patterns().into_iter().map(|(_, p)| p).find(|p| p.name == name)
    }

    pub fn material(&self, name: &str) -> Option<&Material> {
        self.elements.iter().find_map(|(_, e)| match e {
            Element::Material(m) if m.name == name => Some(m),
            _ => None,
        })
    }

    pub fn material_id(&self, name: &str) -> Option<Id> {
        self.elements.iter().find_map(|(id, e)| match e {
            Element::Material(m) if m.name == name => Some(*id),
            _ => None,
        })
    }

    pub fn style_item(&self, id: Id) -> Option<StyleItem<'_>> {
        match self.get(id)? {
            Element::LinePattern(p) => Some(StyleItem::LinePattern(p)),
            Element::FillPattern(p) => Some(StyleItem::FillPattern(p)),
            Element::Material(m) => Some(StyleItem::Material(m)),
            _ => None,
        }
    }

    /// Add built-in patterns, starter materials, object styles and the line
    /// weight table where a project lacks them (older files). Not an undo step.
    pub fn ensure_style_defaults(&mut self) {
        let _ = self.transact("Load graphics standards", |tx| {
            tx.add_style_defaults();
            Ok(())
        });
        self.clear_history();
    }

    /// The first layer of a wall type that carries a material the plan can
    /// draw: its structural core material, else any non-void layer.
    pub fn core_material(&self, layers: &[Layer]) -> Option<&Material> {
        layers
            .iter()
            .find(|l| l.core && !l.is_void)
            .or_else(|| layers.iter().find(|l| l.function == LayerFunction::Structure && !l.is_void))
            .or_else(|| layers.iter().find(|l| !l.is_void))
            .and_then(|l| self.material(&l.material))
    }

    // ----- editing

    /// Apply a merge patch to the Object Styles as one undo step.
    pub fn patch_object_styles(&mut self, patch: &serde_json::Value) -> Result<(), String> {
        let (id, cur) = self.object_styles().map(|(i, s)| (i, s.clone())).ok_or("project has no object styles")?;
        let new: ObjectStyles = apply_patch(&cur, patch)?;
        if new == cur {
            return Ok(());
        }
        self.validate_object_styles(&cur, &new)?;
        self.transact("Change object styles", |tx| tx.update(id, Element::ObjectStyles(new)))
    }

    fn validate_object_styles(&self, old: &ObjectStyles, new: &ObjectStyles) -> Result<(), String> {
        let check = |tab: &str, o: &BTreeMap<String, ObjectStyle>, n: &BTreeMap<String, ObjectStyle>, fixed: bool| -> Result<(), String> {
            if fixed {
                if let Some(k) = o.keys().find(|k| !n.contains_key(*k)) {
                    return Err(format!("the {tab} category '{k}' is built in and cannot be deleted"));
                }
                if let Some(k) = n.keys().find(|k| !o.contains_key(*k)) {
                    return Err(format!("'{k}' is not a {tab} category; add a subcategory instead"));
                }
            }
            for (name, c) in n {
                let mut rows: Vec<(String, &ObjectStyle, bool)> = vec![(name.clone(), c, false)];
                for (sn, s) in &c.subcategories {
                    if sn.trim().is_empty() {
                        return Err(format!("a subcategory of '{name}' needs a name"));
                    }
                    if !s.subcategories.is_empty() {
                        return Err(format!("'{sn}' cannot have its own subcategories"));
                    }
                    rows.push((format!("{name}: {sn}"), s, true));
                }
                if let Some(oc) = o.get(name) {
                    for (sn, os) in &oc.subcategories {
                        match c.subcategories.get(sn) {
                            None if os.built_in => return Err(format!("'{name}: {sn}' is built in and cannot be deleted")),
                            Some(s) if s.built_in != os.built_in => return Err(format!("'{name}: {sn}' built-in flag cannot change")),
                            _ => {}
                        }
                    }
                    if let Some(sn) = c.subcategories.iter().find(|(k, s)| s.built_in && !oc.subcategories.contains_key(*k)).map(|(k, _)| k) {
                        return Err(format!("'{name}: {sn}' cannot be marked built in"));
                    }
                } else if let Some(sn) = c.subcategories.iter().find(|(_, s)| s.built_in).map(|(k, _)| k) {
                    return Err(format!("'{name}: {sn}' cannot be marked built in"));
                }
                for (label, s, _) in rows {
                    for w in [Some(s.projection_weight), s.cut_weight].into_iter().flatten() {
                        if !(1..=MAX_WEIGHT).contains(&w) {
                            return Err(format!("{label}: line weight must be 1 to {MAX_WEIGHT}"));
                        }
                    }
                    if self.line_pattern(&s.pattern).is_none() {
                        return Err(format!("{label}: no line pattern named '{}'", s.pattern));
                    }
                    if let Some(m) = &s.material {
                        if self.material(m).is_none() {
                            return Err(format!("{label}: no material named '{m}'"));
                        }
                    }
                    if tab != "model" && s.material.is_some() {
                        return Err(format!("{label}: annotation and imported objects have no material"));
                    }
                }
            }
            Ok(())
        };
        check("model", &old.model, &new.model, true)?;
        check("annotation", &old.annotation, &new.annotation, true)?;
        check("imported", &old.imported, &new.imported, false)
    }

    pub fn patch_line_weights(&mut self, patch: &serde_json::Value) -> Result<(), String> {
        let (id, cur) = self.line_weights().map(|(i, s)| (i, s.clone())).ok_or("project has no line weight table")?;
        let new: LineWeights = apply_patch(&cur, patch)?;
        if new == cur {
            return Ok(());
        }
        new.validate()?;
        self.transact("Change line weights", |tx| tx.update(id, Element::LineWeights(new)))
    }

    /// Add a line pattern, fill pattern or material. A material may be copied
    /// from an existing one by `from`.
    pub fn add_style_item(&mut self, item: Element) -> Result<Id, String> {
        let name = match &item {
            Element::LinePattern(p) => p.name.clone(),
            Element::FillPattern(p) => p.name.clone(),
            Element::Material(m) => m.name.clone(),
            _ => return Err("not a graphics standard".into()),
        };
        if name.trim().is_empty() {
            return Err("it needs a name".into());
        }
        let taken = match &item {
            Element::LinePattern(_) => self.line_pattern(&name).is_some(),
            Element::FillPattern(_) => self.fill_pattern(&name).is_some(),
            _ => self.material(&name).is_some(),
        };
        if taken {
            return Err(format!("'{name}' already exists"));
        }
        self.validate_item(&item)?;
        let label = match &item {
            Element::LinePattern(_) => "Create line pattern",
            Element::FillPattern(_) => "Create fill pattern",
            _ => "Create material",
        };
        self.transact(label, |tx| Ok(tx.add(item)))
    }

    fn validate_item(&self, item: &Element) -> Result<(), String> {
        match item {
            Element::LinePattern(p) => {
                if p.segments.iter().any(|s| s.kind != SegmentKind::Dot && !(s.length_mm.is_finite() && s.length_mm > 0.0)) {
                    return Err("dashes and spaces need a length above zero".into());
                }
                if !p.segments.is_empty() && !p.segments.iter().any(|s| s.kind != SegmentKind::Space) {
                    return Err("a pattern of only spaces draws nothing".into());
                }
            }
            Element::FillPattern(p) => {
                if !p.solid && p.lines.is_empty() {
                    return Err("a fill pattern needs at least one line, or be solid".into());
                }
                for l in &p.lines {
                    if !(l.spacing.is_finite() && l.spacing > 0.0) || !l.angle_deg.is_finite() {
                        return Err("pattern lines need an angle and a spacing above zero".into());
                    }
                    if l.dashes.iter().any(|d| !d.is_finite() || *d < 0.0) || (!l.dashes.is_empty() && l.dashes.iter().sum::<f64>() <= 0.0) {
                        return Err("pattern dashes must be lengths".into());
                    }
                }
            }
            Element::Material(m) => {
                let g = &m.graphics;
                if g.shading_transparency > 100 {
                    return Err("transparency is 0 to 100".into());
                }
                for p in [&g.surface_foreground, &g.surface_background, &g.cut_foreground, &g.cut_background] {
                    if let Some(n) = &p.pattern {
                        if self.fill_pattern(n).is_none() {
                            return Err(format!("no fill pattern named '{n}'"));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Edit a line pattern, fill pattern or material with a merge patch. Built-in
    /// patterns are read only. Renaming a material updates everything that uses it.
    pub fn patch_style_item(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let cur = self.get(id).cloned().ok_or("no such item")?;
        let new = match &cur {
            Element::LinePattern(p) => Element::LinePattern(apply_patch(p, patch)?),
            Element::FillPattern(p) => Element::FillPattern(apply_patch(p, patch)?),
            Element::Material(m) => Element::Material(apply_patch(m, patch)?),
            _ => return Err("not a graphics standard".into()),
        };
        if new == cur {
            return Ok(());
        }
        match (&cur, &new) {
            (Element::LinePattern(a), _) if a.built_in => return Err(format!("'{}' is a built-in line pattern; duplicate it to change it", a.name)),
            (Element::FillPattern(a), _) if a.built_in => return Err(format!("'{}' is a built-in fill pattern; duplicate it to change it", a.name)),
            (_, Element::LinePattern(b)) if b.built_in => return Err("a pattern cannot be marked built in".into()),
            (_, Element::FillPattern(b)) if b.built_in => return Err("a pattern cannot be marked built in".into()),
            _ => {}
        }
        let (old_name, new_name) = match (&cur, &new) {
            (Element::LinePattern(a), Element::LinePattern(b)) => (a.name.clone(), b.name.clone()),
            (Element::FillPattern(a), Element::FillPattern(b)) => (a.name.clone(), b.name.clone()),
            (Element::Material(a), Element::Material(b)) => (a.name.clone(), b.name.clone()),
            _ => unreachable!(),
        };
        if old_name != new_name {
            if new_name.trim().is_empty() {
                return Err("it needs a name".into());
            }
            let clash = match &cur {
                Element::LinePattern(_) => self.line_pattern(&new_name).is_some(),
                Element::FillPattern(_) => self.fill_pattern(&new_name).is_some(),
                _ => self.material(&new_name).is_some(),
            };
            if clash {
                return Err(format!("'{new_name}' already exists"));
            }
        }
        self.validate_item(&new)?;
        let is_material = matches!(cur, Element::Material(_));
        let (is_line, is_fill) = (matches!(cur, Element::LinePattern(_)), matches!(cur, Element::FillPattern(_)));
        self.transact("Change graphics standard", |tx| {
            tx.update(id, new)?;
            if old_name != new_name {
                tx.rename_references(&old_name, &new_name, is_material, is_line, is_fill)?;
            }
            Ok(())
        })
    }

    /// Delete a pattern or material that nothing uses.
    pub fn delete_style_item(&mut self, id: Id) -> Result<(), String> {
        let (name, kind) = match self.get(id) {
            Some(Element::LinePattern(p)) if p.built_in => return Err(format!("'{}' is built in", p.name)),
            Some(Element::FillPattern(p)) if p.built_in => return Err(format!("'{}' is built in", p.name)),
            Some(Element::LinePattern(p)) => (p.name.clone(), "line pattern"),
            Some(Element::FillPattern(p)) => (p.name.clone(), "fill pattern"),
            Some(Element::Material(m)) => (m.name.clone(), "material"),
            _ => return Err("not a graphics standard".into()),
        };
        let users = self.style_item_users(id);
        if !users.is_empty() {
            return Err(format!("the {kind} '{name}' is used by {}", users.join(", ")));
        }
        self.transact("Delete graphics standard", |tx| tx.remove(id))
    }

    /// Short descriptions of what uses a pattern or material (at most 6).
    pub fn style_item_users(&self, id: Id) -> Vec<String> {
        let mut out = vec![];
        let mut add = |s: String| {
            if out.len() < 6 && !out.contains(&s) {
                out.push(s)
            }
        };
        match self.get(id) {
            Some(Element::Material(m)) => {
                for (_, e) in self.iter() {
                    match e {
                        Element::WallType(t) if t.layers.iter().any(|l| l.material == m.name) => add(format!("wall type {}", t.name)),
                        Element::FloorType(t) if t.layers.iter().any(|l| l.material == m.name) => add(format!("floor type {}", t.name)),
                        Element::RoofType(t) if t.layers.iter().any(|l| l.material == m.name) => add(format!("roof type {}", t.name)),
                        Element::ObjectStyles(s) if s.materials().any(|n| *n == m.name) => add("Object Styles".into()),
                        Element::FamilyType(t) => {
                            if let Some(f) = self.family(t.family) {
                                if f.params.iter().any(|p| p.param_kind == ParamKind::Material && t.values.get(&p.name).and_then(|v| v.as_text()) == Some(&m.name)) {
                                    add(format!("family type {}: {}", f.name, t.name));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some(Element::LinePattern(p)) => {
                if self.object_styles().is_some_and(|(_, s)| [&s.model, &s.annotation, &s.imported].iter().any(|m| m.values().any(|c| c.pattern == p.name || c.subcategories.values().any(|s| s.pattern == p.name)))) {
                    add("Object Styles".into());
                }
                for (_, e) in self.iter() {
                    if let Element::View(v) = e {
                        if view_uses_pattern(v, &p.name) {
                            add(format!("view {}", v.name));
                        }
                    }
                }
            }
            Some(Element::FillPattern(p)) => {
                for (_, e) in self.iter() {
                    match e {
                        Element::Material(m) => {
                            let g = &m.graphics;
                            if [&g.surface_foreground, &g.surface_background, &g.cut_foreground, &g.cut_background].iter().any(|x| x.pattern.as_deref() == Some(&p.name)) {
                                add(format!("material {}", m.name));
                            }
                        }
                        Element::WallType(t) if t.coarse_fill.pattern.as_deref() == Some(&p.name) => add(format!("wall type {}", t.name)),
                        Element::FloorType(t) if t.coarse_fill.pattern.as_deref() == Some(&p.name) => add(format!("floor type {}", t.name)),
                        Element::RoofType(t) if t.coarse_fill.pattern.as_deref() == Some(&p.name) => add(format!("roof type {}", t.name)),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        out
    }
}

fn view_uses_pattern(v: &crate::view::View, name: &str) -> bool {
    let o = &v.settings.overrides;
    let hit = |c: &crate::view::CategoryOverride| {
        [&c.projection_lines, &c.cut_lines].iter().any(|l| l.pattern.as_deref() == Some(name))
            || [&c.surface_foreground, &c.surface_background, &c.cut_foreground, &c.cut_background].iter().any(|f| f.pattern.as_deref() == Some(name))
    };
    [&o.model, &o.annotation, &o.imported].iter().any(|m| m.values().any(|c| hit(c) || c.subcategories.values().any(hit)))
        || v.element_overrides.values().any(hit)
}

impl Tx<'_> {
    /// After a rename: point everything that used the old name at the new one.
    fn rename_references(&mut self, old: &str, new: &str, material: bool, line: bool, fill: bool) -> Result<(), String> {
        let ids: Vec<Id> = self.doc_elements().map(|(i, _)| *i).collect();
        for id in ids {
            let Some(e) = self.get(id).cloned() else { continue };
            let mut e2 = e.clone();
            let fix = |s: &mut String| {
                if s == old {
                    *s = new.to_string();
                }
            };
            let fix_opt = |s: &mut Option<String>| {
                if s.as_deref() == Some(old) {
                    *s = Some(new.to_string());
                }
            };
            match &mut e2 {
                Element::WallType(t) if material => t.layers.iter_mut().for_each(|l| fix(&mut l.material)),
                Element::FloorType(t) if material => t.layers.iter_mut().for_each(|l| fix(&mut l.material)),
                Element::RoofType(t) if material => t.layers.iter_mut().for_each(|l| fix(&mut l.material)),
                Element::WallType(t) if fill => fix_opt(&mut t.coarse_fill.pattern),
                Element::FloorType(t) if fill => fix_opt(&mut t.coarse_fill.pattern),
                Element::RoofType(t) if fill => fix_opt(&mut t.coarse_fill.pattern),
                Element::ObjectStyles(s) => {
                    for tab in [&mut s.model, &mut s.annotation, &mut s.imported] {
                        for c in tab.values_mut() {
                            if material {
                                fix_opt(&mut c.material);
                            }
                            if line {
                                fix(&mut c.pattern);
                            }
                            for s in c.subcategories.values_mut() {
                                if material {
                                    fix_opt(&mut s.material);
                                }
                                if line {
                                    fix(&mut s.pattern);
                                }
                            }
                        }
                    }
                }
                Element::Material(m) if fill => {
                    let g = &mut m.graphics;
                    for p in [&mut g.surface_foreground, &mut g.surface_background, &mut g.cut_foreground, &mut g.cut_background] {
                        fix_opt(&mut p.pattern);
                    }
                }
                Element::FamilyType(t) if material => {
                    let names: Vec<String> = match self.doc_ref().elements.get(&t.family) {
                        Some(Element::Family(f)) => f.params.iter().filter(|p| p.param_kind == ParamKind::Material).map(|p| p.name.clone()).collect(),
                        _ => vec![],
                    };
                    for n in names {
                        if let Some(ParamValue::Text(s)) = t.values.get_mut(&n) {
                            fix(s);
                        }
                    }
                }
                Element::View(v) if line || fill => {
                    let o = &mut v.settings.overrides;
                    let mut fix_over = |c: &mut crate::view::CategoryOverride| {
                        if line {
                            fix_opt(&mut c.projection_lines.pattern);
                            fix_opt(&mut c.cut_lines.pattern);
                        }
                        if fill {
                            for f in [&mut c.surface_foreground, &mut c.surface_background, &mut c.cut_foreground, &mut c.cut_background] {
                                fix_opt(&mut f.pattern);
                            }
                        }
                    };
                    for tab in [&mut o.model, &mut o.annotation, &mut o.imported] {
                        for c in tab.values_mut() {
                            fix_over(c);
                            c.subcategories.values_mut().for_each(&mut fix_over);
                        }
                    }
                    v.element_overrides.values_mut().for_each(&mut fix_over);
                }
                _ => {}
            }
            if e2 != e {
                self.update(id, e2)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_projects_carry_the_graphics_standards() {
        let d = Document::new_project();
        assert!(d.line_pattern("Dash").is_some() && d.line_pattern(SOLID).is_some());
        assert!(d.fill_pattern("Diagonal crosshatch").is_some() && d.fill_pattern(SOLID_FILL).is_some());
        assert!(d.material("Face Brick").is_some());
        let (_, os) = d.object_styles().unwrap();
        assert_eq!(os.model["Walls"].cut_weight, Some(3));
        assert!(os.model["Doors"].subcategories.contains_key("Plan Swing"));
        assert_eq!(os.annotation["Lines"].subcategories["<Beyond>"].pattern, "Dash");
        assert_eq!(os.resolve("Walls", Some("Nope")).unwrap().cut_weight, Some(3));
        // every layer in the starter wall and floor types has a material
        for (_, t) in d.wall_types() {
            for l in t.layers.iter().filter(|l| !l.is_void) {
                assert!(d.material(&l.material).is_some(), "{}", l.material);
            }
        }
    }

    #[test]
    fn old_projects_get_the_standards_and_layer_materials_on_open() {
        let mut d = Document::new_project();
        // simulate an old file: drop the standards and use a layer name no material has
        let ids: Vec<Id> = d.iter().filter(|(_, e)| matches!(e, Element::Material(_) | Element::ObjectStyles(_) | Element::LineWeights(_) | Element::LinePattern(_) | Element::FillPattern(_))).map(|(i, _)| i).collect();
        d.transact("strip", |tx| ids.iter().try_for_each(|i| tx.remove(*i))).unwrap();
        let (wt, mut t) = d.wall_types().into_iter().map(|(i, t)| (i, t.clone())).next().unwrap();
        t.layers[0].material = "Gyprock 10".into();
        d.transact("old layer", |tx| tx.update(wt, Element::WallType(t))).unwrap();
        d.ensure_style_defaults();
        assert!(d.object_styles().is_some() && d.line_weights().is_some());
        assert_eq!(d.material("Gyprock 10").unwrap().class, "Gypsum");
        assert!(d.undo_name().is_none(), "loading the standards is not an undo step");
    }

    #[test]
    fn line_weights_depend_on_the_view_scale() {
        let lw = LineWeights::default();
        assert_eq!(lw.model_width(3, 100), 0.35);
        assert_eq!(lw.model_width(1, 100), 0.18);
        assert!(lw.model_width(7, 1000) < lw.model_width(7, 50));
        // a custom scale uses the nearest column
        assert_eq!(lw.model_width(3, 110), lw.model_width(3, 100));
        assert_eq!(lw.model_width(99, 100), lw.model_width(16, 100));
    }

    #[test]
    fn object_style_edits_are_validated_and_undoable() {
        let mut d = Document::new_project();
        d.patch_object_styles(&json!({"model": {"Walls": {"cut_weight": 5, "color": [10, 20, 30], "pattern": "Dash"}}})).unwrap();
        assert_eq!(d.object_styles().unwrap().1.model["Walls"].cut_weight, Some(5));
        assert_eq!(d.undo_name(), Some("Change object styles"));
        d.undo();
        assert_eq!(d.object_styles().unwrap().1.model["Walls"].cut_weight, Some(3));
        for bad in [
            json!({"model": {"Walls": {"cut_weight": 0}}}),
            json!({"model": {"Walls": {"cut_weight": 17}}}),
            json!({"model": {"Walls": {"pattern": "Zigzag"}}}),
            json!({"model": {"Walls": {"material": "Unobtainium"}}}),
            json!({"model": {"Walls": null}}),
            json!({"model": {"Spaceships": {"projection_weight": 1, "color": [0, 0, 0]}}}),
            json!({"model": {"Doors": {"subcategories": {"Glass": null}}}}),
            json!({"annotation": {"Grids": {"material": "Concrete"}}}),
        ] {
            assert!(d.patch_object_styles(&bad).is_err(), "{bad}");
        }
        // user subcategories can be added and removed, but not marked built in
        d.patch_object_styles(&json!({"model": {"Walls": {"subcategories": {"Skirting": {"projection_weight": 2, "color": [0, 0, 0]}}}}})).unwrap();
        assert!(!d.object_styles().unwrap().1.model["Walls"].subcategories["Skirting"].built_in);
        d.patch_object_styles(&json!({"model": {"Walls": {"subcategories": {"Skirting": null}}}})).unwrap();
        assert!(d.patch_object_styles(&json!({"model": {"Walls": {"subcategories": {"Trim": {"projection_weight": 2, "color": [0, 0, 0], "built_in": true}}}}})).is_err());
    }

    #[test]
    fn materials_rename_everywhere_and_refuse_deletion_while_used() {
        let mut d = Document::new_project();
        let id = d.material_id("Face Brick").unwrap();
        assert!(d.delete_style_item(id).is_err());
        d.patch_style_item(id, &json!({"name": "Brick, Common", "graphics": {"cut_foreground": {"pattern": "Diagonal up"}}})).unwrap();
        assert!(d.material("Face Brick").is_none());
        let t = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().1;
        assert!(t.layers.iter().any(|l| l.material == "Brick, Common"));
        assert!(d.patch_style_item(id, &json!({"name": "Concrete"})).is_err(), "names stay unique");
        assert!(d.patch_style_item(id, &json!({"graphics": {"cut_foreground": {"pattern": "Nope"}}})).is_err());
        d.undo();
        assert!(d.material("Face Brick").is_some(), "one undo step restores every user");
        // a new material is free to delete
        let m = d.add_style_item(Element::Material(Material { name: "Spare".into(), class: "Generic".into(), graphics: Default::default(), physical: Default::default(), identity: Default::default() })).unwrap();
        d.delete_style_item(m).unwrap();
    }

    #[test]
    fn patterns_are_validated_and_built_ins_are_protected() {
        let mut d = Document::new_project();
        let dash = d.line_patterns().into_iter().find(|(_, p)| p.name == "Dash").unwrap().0;
        assert!(d.patch_style_item(dash, &json!({"name": "Dash2"})).is_err());
        assert!(d.delete_style_item(dash).is_err());
        let bad = LinePattern { name: "Bad".into(), segments: vec![seg(SegmentKind::Space, 2.0)], built_in: false };
        assert!(d.add_style_item(Element::LinePattern(bad)).is_err());
        let mine = LinePattern { name: "Long dash".into(), segments: vec![seg(SegmentKind::Dash, 8.0), seg(SegmentKind::Space, 2.0)], built_in: false };
        let id = d.add_style_item(Element::LinePattern(mine)).unwrap();
        d.patch_object_styles(&json!({"model": {"Walls": {"pattern": "Long dash"}}})).unwrap();
        assert!(d.delete_style_item(id).is_err(), "used by Object Styles");
        d.patch_style_item(id, &json!({"name": "Long dash 8"})).unwrap();
        assert_eq!(d.object_styles().unwrap().1.model["Walls"].pattern, "Long dash 8");
        let fill = FillPattern { name: "Empty".into(), pattern_type: PatternKind::Drafting, solid: false, lines: vec![], built_in: false };
        assert!(d.add_style_item(Element::FillPattern(fill)).is_err());
    }

    #[test]
    fn dash_arrays_alternate_dash_and_space() {
        let p = |name: &str| builtin_line_patterns().into_iter().find(|p| p.name == name).unwrap();
        assert_eq!(p("Dash").dash_array(), vec![3.0, 1.5]);
        assert!(p(SOLID).dash_array().is_empty());
        let dd = p("Dash dot").dash_array();
        assert_eq!(dd.len() % 2, 0);
        assert_eq!(dd[0], 3.0);
    }

    #[test]
    fn standards_survive_save_and_open() {
        let mut d = Document::new_project();
        d.patch_object_styles(&json!({"model": {"Floors": {"cut_weight": 6}}})).unwrap();
        let dir = std::env::temp_dir().join(format!("openarch-styles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.oab");
        d.save(&path).unwrap();
        let o = Document::open(&path).unwrap();
        assert_eq!(o.object_styles().unwrap().1.model["Floors"].cut_weight, Some(6));
        assert_eq!(o.materials().len(), d.materials().len(), "no duplicates added on open");
        std::fs::remove_dir_all(dir).ok();
    }
}
