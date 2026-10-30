use serde::{Deserialize, Serialize};

use crate::geom::Point;
use crate::styles::{LineWeights, LinePattern, FillPattern, Material, ObjectStyles, PatternGraphics};
use crate::view::{View, ViewTemplate};

pub type Id = u64;

/// What a level's elevation is measured from (the level type's Elevation Base).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElevationBase {
    #[default]
    ProjectBasePoint,
    SurveyPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub name: String,
    /// Height above the internal origin, mm. What the person sees is measured from the datum
    /// named by `elevation_base` (see `Document::level_display_elevation`).
    pub elevation_mm: f64,
    #[serde(default)]
    pub elevation_base: ElevationBase,
    /// A level that is a storey of the building (Building Story).
    #[serde(default = "yes")]
    pub building_story: bool,
    #[serde(default)]
    pub structural: bool,
    /// Whether the level's bubble shows at its left and right end in elevations.
    #[serde(default = "default_bubbles")]
    pub bubbles: [bool; 2],
    /// Model-space ends of the level line (the 3D extent). `None` fits it to the model.
    #[serde(default)]
    pub extent: Option<[Point; 2]>,
    #[serde(default)]
    pub pinned: bool,
    /// The level type, which names the head family. `None` uses the first type.
    #[serde(default)]
    pub level_type: Option<Id>,
    /// Shown by head label parameters (a shared parameter on levels).
    #[serde(default)]
    pub number: String,
    /// The ends of other levels each end of this one is locked in line with (the alignment pin).
    #[serde(default)]
    pub end_locks: crate::datum::EndLocks,
}

fn default_bubbles() -> [bool; 2] {
    [false, true]
}

impl Level {
    pub fn new(name: &str, elevation_mm: f64) -> Level {
        Level { name: name.into(), elevation_mm, elevation_base: ElevationBase::default(), building_story: true, structural: false, bubbles: default_bubbles(), extent: None, pinned: false, level_type: None, number: String::new(), end_locks: Default::default() }
    }
}

/// What a wall layer is for. Drives join priority between layers, wrapping at
/// ends and openings, and which layers count as the structural core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerFunction {
    #[default]
    Structure,
    Substrate,
    ThermalAir,
    Membrane,
    Finish1,
    Finish2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub material: String,
    pub thickness_mm: f64,
    /// Cavities and the like are drawn but carry no material in quantities.
    #[serde(default)]
    pub is_void: bool,
    #[serde(default)]
    pub function: LayerFunction,
    /// Inside the core boundaries (the structural part of the wall).
    #[serde(default)]
    pub core: bool,
    /// Wraps: this layer takes part in wrapping at ends and inserts (layers outside the core only).
    #[serde(default = "yes")]
    pub wraps: bool,
    /// Structural Material: the layer whose material is the type's structural material (one at most, in the core).
    #[serde(default)]
    pub structural_material: bool,
    /// Variable: the layer whose thickness changes when a floor or roof is shape edited (one at most).
    #[serde(default)]
    pub variable: bool,
    /// Priority (1 joins through everything, 5 least): set to override the one the function gives.
    #[serde(default)]
    pub priority: Option<u8>,
}

impl Layer {
    pub fn new(material: &str, thickness_mm: f64, function: LayerFunction, core: bool) -> Layer {
        Layer {
            material: material.into(),
            thickness_mm,
            is_void: false,
            function,
            core,
            wraps: true,
            structural_material: false,
            variable: false,
            priority: None,
        }
    }

    /// The join priority this layer uses: its own, or the one its function gives.
    pub fn join_priority(&self) -> u8 {
        match (self.function, self.priority) {
            (LayerFunction::Membrane, _) => crate::layers::priority(LayerFunction::Membrane),
            (_, Some(p)) => p,
            (f, None) => crate::layers::priority(f),
        }
    }
}

/// What a wall type is for (the Function type parameter): it feeds schedules, IFC and the default for new walls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallFunction {
    #[default]
    Exterior,
    Interior,
    Retaining,
    Foundation,
    Soffit,
    CoreShaft,
}

/// Interior or exterior floor (the Function type parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloorFunction {
    #[default]
    Interior,
    Exterior,
}

/// The type parameters every wall, floor and roof type carries besides its layers: analytical values and Identity Data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeInfo {
    /// Solar absorptance of the outer surface, 0 to 1.
    #[serde(default = "default_absorptance")]
    pub absorptance: f64,
    /// Surface roughness, 1 (very rough) to 6 (very smooth).
    #[serde(default = "default_roughness")]
    pub roughness: u8,
    #[serde(default)]
    pub keynote: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub type_comments: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub assembly_description: String,
    #[serde(default)]
    pub assembly_code: String,
    #[serde(default)]
    pub type_mark: String,
    #[serde(default)]
    pub fire_rating: String,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub ifc_export_as: String,
    #[serde(default)]
    pub ifc_predefined_type: String,
}

fn default_absorptance() -> f64 {
    0.7
}
fn default_roughness() -> u8 {
    3
}

impl Default for TypeInfo {
    fn default() -> Self {
        serde_json::from_str("{}").expect("defaults")
    }
}

/// How layers wrap round wall ends or openings (Wrapping at Ends / Inserts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wrap {
    #[default]
    None,
    Exterior,
    Interior,
    /// Both sides wrap.
    Both,
}

/// How two walls meet at a shared end. Square off differs from butt only in how
/// the layers join, so until layers join individually it draws like butt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinKind {
    #[default]
    Butt,
    Miter,
    SquareOff,
}

/// Plan display of a join (Clean Join / Don't Clean Join / Use View Setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinCleanup {
    #[default]
    UseView,
    Clean,
    DontClean,
}

/// Join settings for one end of a wall. Joins themselves are found from the
/// geometry (ends at the same point, or an end on another wall); what is stored
/// is how the user wants each end to behave, so it survives moving walls.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EndJoin {
    /// false = Disallow Join: this end stays independent wherever it is moved.
    #[serde(default = "yes")]
    pub allowed: bool,
    #[serde(default)]
    pub kind: JoinKind,
    /// Butt / square off order: Some(true) means this wall runs through the
    /// join, Some(false) means it butts into the other. None = automatic.
    #[serde(default)]
    pub through: Option<bool>,
    #[serde(default)]
    pub cleanup: JoinCleanup,
}

fn yes() -> bool {
    true
}

impl Default for EndJoin {
    fn default() -> Self {
        EndJoin { allowed: true, kind: JoinKind::Butt, through: None, cleanup: JoinCleanup::UseView }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallType {
    pub name: String,
    /// Exterior face first.
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub wrap_ends: Wrap,
    #[serde(default)]
    pub wrap_inserts: Wrap,
    /// Fill used in plan at coarse detail (Coarse Scale Fill Pattern and Color).
    #[serde(default = "default_coarse_fill")]
    pub coarse_fill: PatternGraphics,
    #[serde(default)]
    pub function: WallFunction,
    #[serde(default)]
    pub info: TypeInfo,
    /// Sample Height: how tall the wall is drawn in the Edit Assembly section preview (and the
    /// height vertically compound layers are laid out over).
    #[serde(default = "default_sample_height")]
    pub sample_height_mm: f64,
    /// A curtain wall type: its grids, panels and mullions (the one layer then stands for its glazing zone).
    #[serde(default)]
    pub curtain: Option<crate::curtain::CurtainSpec>,
}

fn default_sample_height() -> f64 {
    6000.0
}

/// A grey solid fill, so coarse plans read as filled walls.
pub fn default_coarse_fill() -> PatternGraphics {
    PatternGraphics { pattern: Some(crate::styles::SOLID_FILL.into()), color: Some([150, 155, 162]) }
}

impl WallType {
    pub fn thickness_mm(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness_mm).sum()
    }

    /// Where the core sits, as distances from the exterior face to its start and
    /// end. A wall with no core layer counts as all core.
    pub fn core_span(&self) -> (f64, f64) {
        let total = self.thickness_mm();
        let (mut at, mut core_start, mut core_end) = (0.0, None, None);
        for l in &self.layers {
            if l.core {
                core_start.get_or_insert(at);
                core_end = Some(at + l.thickness_mm);
            }
            at += l.thickness_mm;
        }
        (core_start.unwrap_or(0.0), core_end.unwrap_or(total))
    }

    /// How far the wall body reaches to the left (exterior side) and right of
    /// the chosen location line, in mm.
    pub fn extents(&self, line: LocationLine) -> (f64, f64) {
        let total = self.thickness_mm();
        let (cs, ce) = self.core_span();
        let pos = match line {
            LocationLine::WallCentreline => total / 2.0,
            LocationLine::CoreCentreline => (cs + ce) / 2.0,
            LocationLine::FinishFaceExterior => 0.0,
            LocationLine::FinishFaceInterior => total,
            LocationLine::CoreFaceExterior => cs,
            LocationLine::CoreFaceInterior => ce,
        };
        (pos, total - pos)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloorType {
    pub name: String,
    pub layers: Vec<Layer>,
    #[serde(default = "default_coarse_fill")]
    pub coarse_fill: PatternGraphics,
    #[serde(default)]
    pub function: FloorFunction,
    #[serde(default)]
    pub info: TypeInfo,
}

impl FloorType {
    pub fn thickness_mm(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness_mm).sum()
    }
}

/// A roof type: layers from the top surface down, as a basic roof.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoofType {
    pub name: String,
    /// Top layer first.
    pub layers: Vec<Layer>,
    #[serde(default = "default_coarse_fill")]
    pub coarse_fill: PatternGraphics,
    #[serde(default)]
    pub info: TypeInfo,
}

impl RoofType {
    pub fn thickness_mm(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness_mm).sum()
    }
}

/// Which line of the wall the drawn path follows (Location Line).
/// Positions are measured across the wall type's layers, exterior face first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationLine {
    #[default]
    WallCentreline,
    CoreCentreline,
    FinishFaceExterior,
    FinishFaceInterior,
    CoreFaceExterior,
    CoreFaceInterior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralUsage {
    #[default]
    NonBearing,
    Bearing,
    Shear,
    StructuralCombination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossSection {
    #[default]
    Vertical,
    Slanted,
    Tapered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IfcExport {
    #[default]
    ByType,
    Yes,
    No,
}

/// IFC parameters carried by every exportable element.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct IfcParams {
    #[serde(default)]
    pub export: IfcExport,
    /// Override of the IFC class, e.g. "IfcBeam".
    #[serde(default)]
    pub export_as: String,
    #[serde(default)]
    pub predefined_type: String,
    /// IFC GlobalId (22 characters). Assigned when the element is created and
    /// kept for the life of the element.
    #[serde(default)]
    pub guid: String,
}

fn yes_() -> bool {
    true
}

/// A new IFC GlobalId: a random UUID compressed to 22 characters.
pub fn new_ifc_guid() -> String {
    const CHARS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    let n = uuid::Uuid::new_v4().as_u128();
    let mut out = String::with_capacity(22);
    // 128 bits: the first character carries 2 bits, the other 21 carry 6 each
    out.push(CHARS[((n >> 126) & 0x3) as usize] as char);
    for i in (0..21).rev() {
        out.push(CHARS[((n >> (i * 6)) & 0x3f) as usize] as char);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wall {
    pub wall_type: Id,
    pub start: Point,
    pub end: Point,
    /// The location line when the wall is not straight (an arc, partial ellipse or spline).
    /// `start` and `end` stay the ends of the curve so everything that finds walls by their
    /// ends keeps working; older files have no curve and read as straight.
    #[serde(default)]
    pub curve: Option<crate::curve::Curve>,
    pub base_level: Id,
    /// When set, the wall runs up to this level (less `top_offset_mm`).
    pub top_level: Option<Id>,
    #[serde(default)]
    pub top_offset_mm: f64,
    /// Used when `top_level` is `None`.
    pub height_mm: f64,
    #[serde(default)]
    pub base_offset_mm: f64,
    #[serde(default)]
    pub start_join: EndJoin,
    #[serde(default)]
    pub end_join: EndJoin,
    #[serde(default)]
    pub location_line: LocationLine,
    /// How far the layers have been extended at the base / top (mm).
    #[serde(default)]
    pub base_extension_mm: f64,
    #[serde(default)]
    pub top_extension_mm: f64,
    #[serde(default = "yes_")]
    pub room_bounding: bool,
    #[serde(default)]
    pub cross_section: CrossSection,
    /// Slant angle from vertical in degrees, for `Slanted` walls.
    #[serde(default)]
    pub angle_from_vertical_deg: f64,
    #[serde(default)]
    pub structural: bool,
    #[serde(default)]
    pub structural_usage: StructuralUsage,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub phase_created: Option<Id>,
    #[serde(default)]
    pub phase_demolished: Option<Id>,
    #[serde(default)]
    pub ifc: IfcParams,
    /// Wall orientation: the layers run in reverse order across the
    /// location line, so the exterior side swaps from the left to the right.
    #[serde(default)]
    pub flipped: bool,
    /// Pinned elements refuse move, delete and flip until unpinned.
    #[serde(default)]
    pub pinned: bool,
    /// A curtain wall's own grid lines, beside those its type lays out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curtain_lines: Option<crate::curtain::CurtainLines>,
    /// An outline sketched with Edit Profile, in place of the wall's rectangle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<crate::wallprofile::WallProfile>,
}

impl Wall {
    pub fn new(wall_type: Id, base_level: Id, start: Point, end: Point, height_mm: f64) -> Wall {
        Wall {
            wall_type,
            start,
            end,
            curve: None,
            base_level,
            top_level: None,
            top_offset_mm: 0.0,
            height_mm,
            base_offset_mm: 0.0,
            start_join: EndJoin::default(),
            end_join: EndJoin::default(),
            location_line: LocationLine::default(),
            base_extension_mm: 0.0,
            top_extension_mm: 0.0,
            room_bounding: true,
            cross_section: CrossSection::default(),
            angle_from_vertical_deg: 0.0,
            structural: false,
            structural_usage: StructuralUsage::default(),
            mark: String::new(),
            comments: String::new(),
            phase_created: None,
            phase_demolished: None,
            ifc: IfcParams { guid: new_ifc_guid(), ..IfcParams::default() },
            flipped: false,
            pinned: false,
            curtain_lines: None,
            profile: None,
        }
    }
}

/// The slope arrow: the floor's top surface rises from the tail to the head.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlopeArrow {
    pub tail: Point,
    pub head: Point,
    pub spec: SlopeSpec,
    /// Level the tail offset is measured from (the floor's own level when `None`).
    #[serde(default)]
    pub tail_level: Option<Id>,
    /// Level the head offset is measured from (the floor's own level when `None`).
    #[serde(default)]
    pub head_level: Option<Id>,
}

/// How the slope is given, as the "Specify" property: by the heights at the tail and head,
/// or by the tail height and a slope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum SlopeSpec {
    HeightAtTail { tail_offset_mm: f64, head_offset_mm: f64 },
    Slope { tail_offset_mm: f64, percent: f64 },
}

/// The span direction symbol: which way the floor spans structurally. Stored for IFC and
/// structural export; nothing in the plan or 3D reads it yet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpanDirection {
    pub a: Point,
    pub b: Point,
}

/// A floor. The shape is `loops`: closed loops of curves, with loops inside others being
/// openings. Files written before curves existed hold a plain `boundary` polygon instead;
/// `loops()` reads that as one loop of lines, so older projects still open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Floor {
    pub floor_type: Id,
    pub level: Id,
    /// Legacy outline (older files). Empty for floors saved with `loops`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundary: Vec<Point>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<Vec<crate::curve::Curve>>,
    #[serde(default)]
    pub slope: Option<SlopeArrow>,
    #[serde(default)]
    pub span: Option<SpanDirection>,
    #[serde(default)]
    pub offset_mm: f64,
    /// Pinned elements refuse move, delete and flip until unpinned.
    #[serde(default)]
    pub pinned: bool,
    /// Structural (instance): the floor is a structural slab, for the analytical model and structural schedules.
    #[serde(default)]
    pub structural: bool,
}

impl Floor {
    pub fn new(floor_type: Id, level: Id, loops: Vec<Vec<crate::curve::Curve>>) -> Floor {
        Floor { floor_type, level, boundary: vec![], loops, slope: None, span: None, offset_mm: 0.0, pinned: false, structural: false }
    }

    /// The closed loops of the floor, whichever way it was stored.
    pub fn loops(&self) -> Vec<Vec<crate::curve::Curve>> {
        if !self.loops.is_empty() {
            return self.loops.clone();
        }
        let n = self.boundary.len();
        if n < 3 {
            return vec![];
        }
        vec![(0..n).map(|i| crate::curve::Curve::line(self.boundary[i], self.boundary[(i + 1) % n])).collect()]
    }

    /// Each loop as a polygon, curves flattened to within `tol` mm.
    pub fn polygons(&self, tol: f64) -> Vec<Vec<Point>> {
        self.loops().iter().map(|l| crate::curve::loop_polygon(l, tol)).collect()
    }

    pub fn regions(&self, tol: f64) -> Vec<crate::geom::Region> {
        crate::geom::regions(&self.polygons(tol))
    }

    /// Area in plan, net of openings, mm squared.
    pub fn area_mm2(&self) -> f64 {
        self.regions(0.5).iter().map(|r| r.area()).sum()
    }

    /// The outline of the first region (what the plan hit-tests and zooms to).
    pub fn outline(&self) -> Vec<Point> {
        self.regions(1.0).into_iter().next().map(|r| r.outer).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Element {
    Level(Level),
    WallType(WallType),
    FloorType(FloorType),
    Wall(Wall),
    Floor(Floor),
    RoofType(RoofType),
    Roof(crate::roof::Roof),
    View(View),
    ViewTemplate(ViewTemplate),
    Family(crate::family::Family),
    FamilyType(crate::family::FamilyType),
    FamilyInstance(crate::family::FamilyInstance),
    ObjectStyles(ObjectStyles),
    LineWeights(LineWeights),
    LinePattern(LinePattern),
    FillPattern(FillPattern),
    Material(Material),
    ProjectLocation(crate::site::ProjectLocation),
    ElevationMarker(crate::site::ElevationMarker),
    Grid(crate::grid::Grid),
    GridType(crate::grid::GridType),
    AnnotationFamily(crate::annotation::AnnotationFamily),
    LevelType(crate::annotation::LevelType),
    Dimension(crate::dimension::Dimension),
    DimensionType(crate::dimension::DimensionType),
    Spot(crate::spot::Spot),
    SpotType(crate::spot::SpotType),
    Section(crate::section::Section),
    ViewType(crate::view::ViewType),
    DetailLine(crate::detail::DetailLine),
    DetailRegion(crate::detail::DetailRegion),
    FilledRegionType(crate::annotation::RegionType),
    TextNote(crate::detail::TextNote),
    TextNoteType(crate::annotation::TextType),
    GlobalParam(crate::globalparam::GlobalParam),
    GroupType(crate::group::GroupType),
    GroupInstance(crate::group::GroupInstance),
    GroupArray(crate::array::GroupArray),
    ProjectInfo(crate::sheet::ProjectInfo),
    Sheet(crate::sheet::Sheet),
    Viewport(crate::sheet::Viewport),
}

impl Element {
    pub fn kind(&self) -> &'static str {
        match self {
            Element::Level(_) => "Level",
            Element::WallType(_) => "WallType",
            Element::FloorType(_) => "FloorType",
            Element::Wall(_) => "Wall",
            Element::Floor(_) => "Floor",
            Element::RoofType(_) => "RoofType",
            Element::Roof(_) => "Roof",
            Element::View(_) => "View",
            Element::ViewTemplate(_) => "ViewTemplate",
            Element::Family(_) => "Family",
            Element::FamilyType(_) => "FamilyType",
            Element::FamilyInstance(_) => "FamilyInstance",
            Element::ObjectStyles(_) => "ObjectStyles",
            Element::LineWeights(_) => "LineWeights",
            Element::LinePattern(_) => "LinePattern",
            Element::FillPattern(_) => "FillPattern",
            Element::Material(_) => "Material",
            Element::ProjectLocation(_) => "ProjectLocation",
            Element::ElevationMarker(_) => "ElevationMarker",
            Element::Grid(_) => "Grid",
            Element::GridType(_) => "GridType",
            Element::AnnotationFamily(_) => "AnnotationFamily",
            Element::LevelType(_) => "LevelType",
            Element::Dimension(_) => "Dimension",
            Element::DimensionType(_) => "DimensionType",
            Element::Spot(_) => "Spot",
            Element::SpotType(_) => "SpotType",
            Element::Section(_) => "Section",
            Element::ViewType(_) => "ViewType",
            Element::DetailLine(_) => "DetailLine",
            Element::DetailRegion(_) => "DetailRegion",
            Element::FilledRegionType(_) => "FilledRegionType",
            Element::TextNote(_) => "TextNote",
            Element::TextNoteType(_) => "TextNoteType",
            Element::GlobalParam(_) => "GlobalParam",
            Element::GroupType(_) => "GroupType",
            Element::GroupInstance(_) => "GroupInstance",
            Element::GroupArray(_) => "GroupArray",
            Element::ProjectInfo(_) => "ProjectInfo",
            Element::Sheet(_) => "Sheet",
            Element::Viewport(_) => "Viewport",
        }
    }
}
