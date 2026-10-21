//! Views, view templates and graphics overrides.
//!
//! Views are stored elements. Everything a view template can
//! control lives in `ViewSettings`; a template lists which properties it
//! `included`, and a view that points at a template takes those properties from
//! it (`Document::effective_settings`). Per-view things that templates do not
//! carry (crop, element overrides, name, level) stay on `View`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::element::Id;
use crate::geom::Point;

pub type Color = [u8; 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    #[default]
    FloorPlan,
    CeilingPlan,
    StructuralPlan,
    AreaPlan,
    Elevation,
    Section,
    Callout,
    Drafting,
    Legend,
    ThreeD,
}

impl ViewKind {
    pub fn is_plan(self) -> bool {
        matches!(self, ViewKind::FloorPlan | ViewKind::CeilingPlan | ViewKind::StructuralPlan | ViewKind::AreaPlan)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    Coarse,
    #[default]
    Medium,
    Fine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayModel {
    #[default]
    Normal,
    DoNotDisplay,
    Halftone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartsVisibility {
    #[default]
    ShowOriginal,
    ShowParts,
    ShowBoth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualStyle {
    Wireframe,
    HiddenLine,
    #[default]
    Shaded,
    ConsistentColors,
    Realistic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Discipline {
    Architectural,
    Structural,
    Mechanical,
    Electrical,
    Plumbing,
    #[default]
    Coordination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShowHiddenLines {
    #[default]
    ByDiscipline,
    None,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallJoinDisplay {
    /// Clean all wall joins, whatever the wall types.
    #[default]
    CleanAll,
    CleanSameType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    ProjectNorth,
    TrueNorth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnderlayOrientation {
    #[default]
    LookDown,
    LookUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSchemeLocation {
    #[default]
    Background,
    Foreground,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepthClipping {
    #[default]
    NoClip,
    ClipWithLine,
    ClipWithoutLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationOnSheet {
    #[default]
    None,
    Clockwise90,
    CounterClockwise90,
}

/// Where a view range plane is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeLevel {
    /// The view's own level.
    Associated,
    /// The next level up (unlimited when there is none).
    Above,
    /// The next level down.
    Below,
    Level(Id),
    Unlimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RangePlane {
    pub level: RangeLevel,
    pub offset_mm: f64,
}

/// View range of a plan view (Top, Cut plane, Bottom, View depth).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewRange {
    pub top: RangePlane,
    /// Offset from the associated level.
    pub cut_offset_mm: f64,
    pub bottom: RangePlane,
    pub depth: RangePlane,
}

impl Default for ViewRange {
    /// Standard defaults for a floor plan.
    fn default() -> Self {
        let at = |offset_mm| RangePlane { level: RangeLevel::Associated, offset_mm };
        ViewRange { top: at(2300.0), cut_offset_mm: 1200.0, bottom: at(0.0), depth: at(0.0) }
    }
}

/// Elevations (mm) of a resolved view range. Unlimited planes are infinite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedRange {
    pub top: f64,
    pub cut: f64,
    pub bottom: f64,
    pub depth: f64,
}

/// How an element at a given height shows in a plan view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanDisplay {
    /// Cut by the cut plane: drawn with cut lines and cut pattern.
    Cut,
    /// Below the cut plane inside the primary range: projection lines.
    Projection,
    /// Below the bottom plane but inside the view depth: drawn as beyond.
    Beyond,
    Hidden,
}

impl ResolvedRange {
    pub fn classify(&self, z0: f64, z1: f64) -> PlanDisplay {
        if z0 > self.top || z1 < self.depth {
            return PlanDisplay::Hidden;
        }
        if z0 <= self.cut && self.cut <= z1 {
            return PlanDisplay::Cut;
        }
        if z1 < self.cut {
            return if z1 >= self.bottom { PlanDisplay::Projection } else { PlanDisplay::Beyond };
        }
        PlanDisplay::Hidden // wholly above the cut plane
    }

    /// `classify` for an element of `category`. Windows, casework and generic models wholly above the cut plane but
    /// below the top are still shown, as seen from above (wall cupboards, high windows, a shelf modelled as a
    /// generic model); everything else up there is hidden.
    pub fn classify_category(&self, category: &str, z0: f64, z1: f64) -> PlanDisplay {
        match self.classify(z0, z1) {
            PlanDisplay::Hidden
                if z0 > self.cut
                    && z0 <= self.top
                    && matches!(category, "Windows" | "Casework" | "Generic Models") =>
            {
                PlanDisplay::Projection
            }
            d => d,
        }
    }
}

/// Line override. `None` fields keep the default.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LineOverride {
    pub pattern: Option<String>,
    pub color: Option<Color>,
    pub weight: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillOverride {
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub pattern: Option<String>,
}

impl Default for FillOverride {
    fn default() -> Self {
        FillOverride { visible: true, color: None, pattern: None }
    }
}

/// Everything the Visibility/Graphics dialog (and "Override Graphics in View")
/// can set for a category, subcategory, filter or element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryOverride {
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub halftone: bool,
    #[serde(default)]
    pub projection_lines: LineOverride,
    #[serde(default)]
    pub surface_foreground: FillOverride,
    #[serde(default)]
    pub surface_background: FillOverride,
    /// 0 (opaque) to 100 (invisible surfaces).
    #[serde(default)]
    pub transparency: u8,
    #[serde(default)]
    pub cut_lines: LineOverride,
    #[serde(default)]
    pub cut_foreground: FillOverride,
    #[serde(default)]
    pub cut_background: FillOverride,
    /// Model categories only. `None` follows the view.
    #[serde(default)]
    pub detail_level: Option<DetailLevel>,
    /// Subcategory overrides, by subcategory name (model categories).
    #[serde(default)]
    pub subcategories: BTreeMap<String, CategoryOverride>,
}

impl Default for CategoryOverride {
    fn default() -> Self {
        CategoryOverride {
            visible: true,
            halftone: false,
            projection_lines: LineOverride::default(),
            surface_foreground: FillOverride::default(),
            surface_background: FillOverride::default(),
            transparency: 0,
            cut_lines: LineOverride::default(),
            cut_foreground: FillOverride::default(),
            cut_background: FillOverride::default(),
            detail_level: None,
            subcategories: BTreeMap::new(),
        }
    }
}

/// A filter applied to a view with its own overrides (filters themselves are
/// elements, added later).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterApplication {
    pub filter: Id,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub overrides: CategoryOverride,
}

/// The tabs of the Visibility/Graphics dialog. Keys are category names.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GraphicsOverrides {
    #[serde(default)]
    pub model: BTreeMap<String, CategoryOverride>,
    #[serde(default)]
    pub annotation: BTreeMap<String, CategoryOverride>,
    #[serde(default)]
    pub imported: BTreeMap<String, CategoryOverride>,
    /// Per linked model, by link element id.
    #[serde(default)]
    pub links: BTreeMap<Id, GraphicsOverrides>,
    #[serde(default)]
    pub filters: Vec<FilterApplication>,
}

/// Properties a view template can control (the Include checkboxes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateProp {
    Scale,
    DisplayModel,
    DetailLevel,
    PartsVisibility,
    VgModel,
    VgAnnotation,
    VgImport,
    VgFilters,
    VisualStyle,
    ShowHiddenLines,
    ViewRange,
    Orientation,
    PhaseFilter,
    Discipline,
    WallJoinDisplay,
    ColorScheme,
    UnderlayOrientation,
    DepthClipping,
    SunPath,
    GraphicDisplay,
}

impl TemplateProp {
    pub const ALL: [TemplateProp; 20] = [
        TemplateProp::Scale,
        TemplateProp::DisplayModel,
        TemplateProp::DetailLevel,
        TemplateProp::PartsVisibility,
        TemplateProp::VgModel,
        TemplateProp::VgAnnotation,
        TemplateProp::VgImport,
        TemplateProp::VgFilters,
        TemplateProp::VisualStyle,
        TemplateProp::ShowHiddenLines,
        TemplateProp::ViewRange,
        TemplateProp::Orientation,
        TemplateProp::PhaseFilter,
        TemplateProp::Discipline,
        TemplateProp::WallJoinDisplay,
        TemplateProp::ColorScheme,
        TemplateProp::UnderlayOrientation,
        TemplateProp::DepthClipping,
        TemplateProp::SunPath,
        TemplateProp::GraphicDisplay,
    ];
}

/// Properties that a view template can carry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewSettings {
    /// Scale denominator: 100 means 1:100.
    pub scale: u32,
    /// True when the scale is a custom value rather than a preset.
    #[serde(default)]
    pub custom_scale: bool,
    #[serde(default)]
    pub display_model: DisplayModel,
    #[serde(default)]
    pub detail_level: DetailLevel,
    #[serde(default)]
    pub parts_visibility: PartsVisibility,
    #[serde(default)]
    pub visual_style: VisualStyle,
    #[serde(default)]
    pub show_hidden_lines: ShowHiddenLines,
    #[serde(default)]
    pub view_range: ViewRange,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub phase_filter: Option<Id>,
    #[serde(default)]
    pub discipline: Discipline,
    #[serde(default)]
    pub wall_join_display: WallJoinDisplay,
    #[serde(default)]
    pub color_scheme_location: ColorSchemeLocation,
    #[serde(default)]
    pub color_scheme: Option<Id>,
    #[serde(default)]
    pub underlay_orientation: UnderlayOrientation,
    #[serde(default)]
    pub depth_clipping: DepthClipping,
    #[serde(default)]
    pub sun_path: bool,
    #[serde(default)]
    pub graphic_display: GraphicDisplay,
    #[serde(default)]
    pub overrides: GraphicsOverrides,
}

/// The Graphic Display Options dialog. Stored now so views carry it from the
/// start; the renderers pick up each option as they are built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicDisplay {
    pub show_edges: bool,
    pub transparency: u8,
    pub silhouettes: bool,
    pub cast_shadows: bool,
    pub ambient_shadows: bool,
    pub sketchy_lines: bool,
    pub sketchy_jitter: u8,
    pub sketchy_extension: u8,
    pub depth_cue: bool,
    pub depth_cue_start: u8,
    pub depth_cue_end: u8,
    pub depth_cue_fade: u8,
    pub sun_azimuth_deg: f64,
    pub sun_altitude_deg: f64,
    pub sun_intensity: u8,
    pub ambient_intensity: u8,
    pub background: Background,
}

impl Default for GraphicDisplay {
    fn default() -> Self {
        GraphicDisplay {
            show_edges: true,
            transparency: 0,
            silhouettes: false,
            cast_shadows: false,
            ambient_shadows: false,
            sketchy_lines: false,
            sketchy_jitter: 2,
            sketchy_extension: 0,
            depth_cue: false,
            depth_cue_start: 0,
            depth_cue_end: 100,
            depth_cue_fade: 0,
            sun_azimuth_deg: 135.0,
            sun_altitude_deg: 45.0,
            sun_intensity: 50,
            ambient_intensity: 25,
            background: Background::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    #[default]
    None,
    Sky,
    Gradient,
    Image,
}

impl Default for ViewSettings {
    fn default() -> Self {
        ViewSettings {
            scale: 100,
            custom_scale: false,
            display_model: DisplayModel::default(),
            detail_level: DetailLevel::default(),
            parts_visibility: PartsVisibility::default(),
            visual_style: VisualStyle::default(),
            show_hidden_lines: ShowHiddenLines::default(),
            view_range: ViewRange::default(),
            orientation: Orientation::default(),
            phase_filter: None,
            discipline: Discipline::default(),
            wall_join_display: WallJoinDisplay::default(),
            color_scheme_location: ColorSchemeLocation::default(),
            color_scheme: None,
            underlay_orientation: UnderlayOrientation::default(),
            depth_clipping: DepthClipping::default(),
            sun_path: false,
            graphic_display: GraphicDisplay::default(),
            overrides: GraphicsOverrides::default(),
        }
    }
}

impl ViewSettings {
    /// Copy one template-controlled property from `src`.
    pub fn take(&mut self, src: &ViewSettings, p: TemplateProp) {
        match p {
            TemplateProp::Scale => {
                self.scale = src.scale;
                self.custom_scale = src.custom_scale;
            }
            TemplateProp::DisplayModel => self.display_model = src.display_model,
            TemplateProp::DetailLevel => self.detail_level = src.detail_level,
            TemplateProp::PartsVisibility => self.parts_visibility = src.parts_visibility,
            TemplateProp::VgModel => self.overrides.model = src.overrides.model.clone(),
            TemplateProp::VgAnnotation => self.overrides.annotation = src.overrides.annotation.clone(),
            TemplateProp::VgImport => {
                self.overrides.imported = src.overrides.imported.clone();
                self.overrides.links = src.overrides.links.clone();
            }
            TemplateProp::VgFilters => self.overrides.filters = src.overrides.filters.clone(),
            TemplateProp::VisualStyle => self.visual_style = src.visual_style,
            TemplateProp::ShowHiddenLines => self.show_hidden_lines = src.show_hidden_lines,
            TemplateProp::ViewRange => self.view_range = src.view_range,
            TemplateProp::Orientation => self.orientation = src.orientation,
            TemplateProp::PhaseFilter => self.phase_filter = src.phase_filter,
            TemplateProp::Discipline => self.discipline = src.discipline,
            TemplateProp::WallJoinDisplay => self.wall_join_display = src.wall_join_display,
            TemplateProp::ColorScheme => {
                self.color_scheme = src.color_scheme;
                self.color_scheme_location = src.color_scheme_location;
            }
            TemplateProp::UnderlayOrientation => self.underlay_orientation = src.underlay_orientation,
            TemplateProp::DepthClipping => self.depth_clipping = src.depth_clipping,
            TemplateProp::SunPath => self.sun_path = src.sun_path,
            TemplateProp::GraphicDisplay => self.graphic_display = src.graphic_display.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewTemplate {
    pub name: String,
    // not `kind`: that name is the element type tag in the project file
    #[serde(default)]
    pub view_kind: ViewKind,
    pub included: BTreeSet<TemplateProp>,
    pub settings: ViewSettings,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Crop {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub region_visible: bool,
    #[serde(default)]
    pub annotation_crop: bool,
    /// Crop region corners (min, max) in model coordinates, mm.
    #[serde(default)]
    pub region: Option<(Point, Point)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Underlay {
    /// Level at the bottom of the underlay range. `None` = no underlay.
    #[serde(default)]
    pub base_level: Option<Id>,
    /// Level at the top of the range. `None` = the level above the base level.
    #[serde(default)]
    pub top_level: Option<Id>,
}

impl Default for Underlay {
    fn default() -> Self {
        Underlay { base_level: None, top_level: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    #[serde(default)]
    pub view_kind: ViewKind,
    /// Associated level of a plan view.
    #[serde(default)]
    pub level: Option<Id>,
    #[serde(default)]
    pub title_on_sheet: String,
    #[serde(default)]
    pub template: Option<Id>,
    /// Primary view when this is a dependent view.
    #[serde(default)]
    pub parent: Option<Id>,
    #[serde(default)]
    pub settings: ViewSettings,
    #[serde(default)]
    pub underlay: Underlay,
    #[serde(default)]
    pub crop: Crop,
    #[serde(default)]
    pub scope_box: Option<Id>,
    #[serde(default = "default_column_offset")]
    pub column_symbolic_offset_mm: f64,
    #[serde(default)]
    pub phase: Option<Id>,
    #[serde(default)]
    pub rotation_on_sheet: RotationOnSheet,
    /// Per-element graphics overrides in this view (not part of templates).
    #[serde(default)]
    pub element_overrides: BTreeMap<Id, CategoryOverride>,
    /// Elements hidden in this view.
    #[serde(default)]
    pub hidden_elements: BTreeSet<Id>,
    /// What an elevation view looks at (marker, which of its pointers, crop and depth).
    #[serde(default)]
    pub elevation: Option<ElevationDef>,
    /// The section a section view shows.
    #[serde(default)]
    pub section: Option<Id>,
    /// The view type it was made from (Floor Plan, Ceiling Plan and so on), when it has one.
    #[serde(default)]
    pub view_type: Option<Id>,
}

/// A view type: what the New Plan dialog's Type list offers. Each kind of plan has at least one, and a type can
/// give its new views a view template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewType {
    pub name: String,
    pub view_kind: ViewKind,
    /// View Template applied to new views of this type.
    #[serde(default)]
    pub template_for_new: Option<Id>,
    /// New views stay linked to that template (dependent on it) rather than taking a copy of its settings.
    #[serde(default = "yes")]
    pub new_views_dependent: bool,
}

fn yes() -> bool {
    true
}

/// An elevation view's place in an elevation marker. `crop` is how far the view reaches to the
/// left and right of the marker (looking along the view) and `far_clip_mm` how deep it looks; None
/// means the view fits itself to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElevationDef {
    pub marker: Id,
    pub slot: u8,
    #[serde(default)]
    pub crop: Option<(f64, f64)>,
    #[serde(default)]
    pub far_clip_mm: Option<f64>,
}

fn default_column_offset() -> f64 {
    304.8
}

impl View {
    /// The pale translucent wash plans gave floors before format 3. Floors now draw from their materials and
    /// Object Styles like everything else; older projects lose the wash once when it is still exactly this.
    pub(crate) fn old_floor_wash() -> CategoryOverride {
        let wash = FillOverride { visible: true, color: Some([120, 160, 200]), pattern: Some("Solid fill".into()) };
        CategoryOverride { surface_foreground: wash.clone(), cut_foreground: wash, transparency: 75, ..CategoryOverride::default() }
    }

    pub fn floor_plan(name: &str, level: Id) -> View {
        let mut settings = ViewSettings::default();
        for cat in SITE_POINT_CATEGORIES {
            settings.overrides.model.insert(cat.into(), CategoryOverride { visible: false, ..CategoryOverride::default() });
        }
        View {
            name: name.into(),
            view_kind: ViewKind::FloorPlan,
            level: Some(level),
            title_on_sheet: String::new(),
            template: None,
            parent: None,
            settings,
            underlay: Underlay::default(),
            crop: Crop::default(),
            scope_box: None,
            column_symbolic_offset_mm: default_column_offset(),
            phase: None,
            rotation_on_sheet: RotationOnSheet::None,
            element_overrides: BTreeMap::new(),
            hidden_elements: BTreeSet::new(),
            elevation: None,
            section: None,
            view_type: None,
        }
    }

    /// A reflected ceiling plan of a level. It draws as a plan for now; looking up through the cut plane comes with ceilings.
    pub fn ceiling_plan(name: &str, level: Id) -> View {
        let mut v = View::floor_plan(name, level);
        v.view_kind = ViewKind::CeilingPlan;
        v.settings.overrides.model.remove("Floors");
        v
    }

    /// An elevation looking out of slot `slot` of `marker`.
    pub fn elevation(name: &str, marker: Id, slot: u8) -> View {
        let mut v = View::floor_plan(name, 0);
        v.view_kind = ViewKind::Elevation;
        v.level = None;
        v.settings.overrides.model.remove("Floors");
        v.settings.visual_style = VisualStyle::HiddenLine;
        v.elevation = Some(ElevationDef { marker, slot, crop: None, far_clip_mm: None });
        v
    }

    /// Show the project base point and survey point in this view (the Site plan does).
    pub fn show_site_points(&mut self) {
        for cat in SITE_POINT_CATEGORIES {
            self.settings.overrides.model.insert(cat.into(), CategoryOverride::default());
        }
    }

    pub fn three_d(name: &str) -> View {
        let mut v = View { view_kind: ViewKind::ThreeD, level: None, ..View::floor_plan(name, 0) };
        v.settings.visual_style = VisualStyle::HiddenLine;
        v
    }
}

/// Model categories listed in the Visibility/Graphics dialog. Categories are
/// plain names so that families and links can add their own later.
pub const MODEL_CATEGORIES: &[&str] = &[
    "Air Terminals", "Casework", "Ceilings", "Columns", "Communication Devices", "Curtain Panels", "Curtain Systems",
    "Curtain Wall Mullions", "Data Devices", "Doors", "Electrical Equipment", "Electrical Fixtures", "Entourage", "Fire Alarm Devices",
    "Fire Protection", "Floors", "Food Service Equipment", "Furniture", "Furniture Systems", "Generic Models", "Hardscape",
    "Lighting Devices", "Lighting Fixtures", "Mass", "Mechanical Control Devices", "Mechanical Equipment", "Parking", "Planting",
    "Plumbing Fixtures", "Project Base Point", "Railings", "Ramps", "Roofs", "Rooms", "Security Devices", "Signage", "Site",
    "Specialty Equipment", "Stairs", "Structural Columns", "Structural Foundations", "Structural Framing", "Survey Point", "Topography",
    "Walls", "Windows",
];

/// The project base point and survey point are non-printing symbols: each has its own category, and they are
/// shown only in the Site plan until a view turns them on in Visibility/Graphics.
pub const SITE_POINT_CATEGORIES: [&str; 2] = ["Project Base Point", "Survey Point"];

pub const ANNOTATION_CATEGORIES: &[&str] = &[
    "Dimensions", "Elevations", "Grids", "Levels", "Matchline", "Room Tags", "Scope Boxes", "Sections", "Spot Coordinates", "Spot Elevations", "Spot Slopes", "Text Notes",
    "Reference Planes", "Revision Clouds", "Detail Items", "Generic Annotations", "Keynote Tags", "Door Tags", "Window Tags", "Wall Tags",
];

/// The categories of annotation families (level heads, elevation and section marks): Object Styles rows whose
/// subcategories are the line styles those families draw with.
pub const SYMBOL_CATEGORIES: &[&str] = &["Level Heads", "Elevation Marks", "Section Marks"];

#[cfg(test)]
mod tests {
    use super::*;

    fn range() -> ResolvedRange {
        ResolvedRange { top: 2300.0, cut: 1200.0, bottom: 0.0, depth: 0.0 }
    }

    #[test]
    fn classify_follows_the_view_range() {
        let r = range();
        assert_eq!(r.classify(0.0, 2700.0), PlanDisplay::Cut); // normal wall
        assert_eq!(r.classify(0.0, 900.0), PlanDisplay::Projection); // low wall under the cut plane
        assert_eq!(r.classify(1500.0, 2700.0), PlanDisplay::Hidden); // high-level wall above the cut plane
        assert_eq!(r.classify(2400.0, 2700.0), PlanDisplay::Hidden); // above the top
        assert_eq!(r.classify(-200.0, 0.0), PlanDisplay::Projection); // slab touching bottom plane
    }

    #[test]
    fn windows_casework_and_generic_models_above_the_cut_are_seen() {
        let r = range();
        assert_eq!(
            r.classify_category("Casework", 1500.0, 2100.0),
            PlanDisplay::Projection
        );
        assert_eq!(
            r.classify_category("Generic Models", 1500.0, 2100.0),
            PlanDisplay::Projection
        );
        assert_eq!(
            r.classify_category("Lighting Fixtures", 1500.0, 2100.0),
            PlanDisplay::Hidden
        );
        assert_eq!(
            r.classify_category("Casework", 0.0, 900.0),
            PlanDisplay::Projection
        );
    }

    #[test]
    fn view_depth_shows_things_below_as_beyond() {
        let r = ResolvedRange { top: 2300.0, cut: 1200.0, bottom: 0.0, depth: -600.0 };
        assert_eq!(r.classify(-500.0, -300.0), PlanDisplay::Beyond);
        assert_eq!(r.classify(-1000.0, -800.0), PlanDisplay::Hidden);
    }

    #[test]
    fn template_properties_are_copied_only_when_included() {
        let mut view = ViewSettings::default();
        let mut t = ViewSettings::default();
        t.scale = 50;
        t.detail_level = DetailLevel::Fine;
        view.take(&t, TemplateProp::Scale);
        assert_eq!((view.scale, view.detail_level), (50, DetailLevel::Medium));
    }

    #[test]
    fn views_load_with_missing_fields() {
        let v: View = serde_json::from_str(r#"{"name":"Level 1","settings":{"scale":50}}"#).unwrap();
        assert_eq!(v.settings.view_range, ViewRange::default());
        assert_eq!(v.column_symbolic_offset_mm, 304.8);
    }
}
