//! Families: the unit of everything placed in a project (doors, windows and,
//! later, furniture, fixtures, columns and so on).
//!
//! A `Family` declares a category, what it hosts on, how it draws in plan and
//! which parameters it has. A `FamilyType` holds the type parameter values
//! (the rows of the family's type catalogue). A `FamilyInstance` is a
//! placed copy with its instance values. Doors and windows are the first
//! families; they host in walls and cut openings in them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::element::{new_ifc_guid, Id, IfcParams};
use crate::geom::Point;

/// Categories a family can belong to. Doors and windows have their own tools; the rest are placed with Component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FamilyCategory {
    #[default]
    GenericModels,
    Doors,
    Windows,
    Furniture,
    FurnitureSystems,
    Casework,
    PlumbingFixtures,
    LightingFixtures,
    LightingDevices,
    ElectricalFixtures,
    ElectricalEquipment,
    FireAlarmDevices,
    CommunicationDevices,
    DataDevices,
    SecurityDevices,
    MechanicalEquipment,
    MechanicalControlDevices,
    AirTerminals,
    FireProtection,
    SpecialtyEquipment,
    FoodServiceEquipment,
    Columns,
    StructuralColumns,
    StructuralFraming,
    StructuralFoundations,
    Planting,
    Entourage,
    Parking,
    Site,
    Hardscape,
    Signage,
    Mass,
    /// A closed outline drawn on reference planes, used by sweeps (door frames, fascias, reveals). Never placed.
    Profiles,
}

impl FamilyCategory {
    /// The category name, also the Visibility/Graphics category name.
    pub fn label(self) -> &'static str {
        match self {
            FamilyCategory::GenericModels => "Generic Models",
            FamilyCategory::Doors => "Doors",
            FamilyCategory::Windows => "Windows",
            FamilyCategory::Furniture => "Furniture",
            FamilyCategory::Casework => "Casework",
            FamilyCategory::PlumbingFixtures => "Plumbing Fixtures",
            FamilyCategory::LightingFixtures => "Lighting Fixtures",
            FamilyCategory::ElectricalFixtures => "Electrical Fixtures",
            FamilyCategory::ElectricalEquipment => "Electrical Equipment",
            FamilyCategory::MechanicalEquipment => "Mechanical Equipment",
            FamilyCategory::SpecialtyEquipment => "Specialty Equipment",
            FamilyCategory::Columns => "Columns",
            FamilyCategory::Planting => "Planting",
            FamilyCategory::Entourage => "Entourage",
            FamilyCategory::Parking => "Parking",
            FamilyCategory::Site => "Site",
            FamilyCategory::FurnitureSystems => "Furniture Systems",
            FamilyCategory::LightingDevices => "Lighting Devices",
            FamilyCategory::FireAlarmDevices => "Fire Alarm Devices",
            FamilyCategory::CommunicationDevices => "Communication Devices",
            FamilyCategory::DataDevices => "Data Devices",
            FamilyCategory::SecurityDevices => "Security Devices",
            FamilyCategory::MechanicalControlDevices => "Mechanical Control Devices",
            FamilyCategory::AirTerminals => "Air Terminals",
            FamilyCategory::FireProtection => "Fire Protection",
            FamilyCategory::FoodServiceEquipment => "Food Service Equipment",
            FamilyCategory::StructuralColumns => "Structural Columns",
            FamilyCategory::StructuralFraming => "Structural Framing",
            FamilyCategory::StructuralFoundations => "Structural Foundations",
            FamilyCategory::Hardscape => "Hardscape",
            FamilyCategory::Signage => "Signage",
            FamilyCategory::Mass => "Mass",
            FamilyCategory::Profiles => "Profiles",
        }
    }
    /// Every category in the order the family editor lists them.
    pub fn all() -> &'static [FamilyCategory] {
        use FamilyCategory::*;
        &[
            GenericModels, Doors, Windows, Furniture, FurnitureSystems, Casework, PlumbingFixtures, LightingFixtures, LightingDevices, ElectricalFixtures, ElectricalEquipment, FireAlarmDevices,
            CommunicationDevices, DataDevices, SecurityDevices, MechanicalEquipment, MechanicalControlDevices, AirTerminals, FireProtection, SpecialtyEquipment, FoodServiceEquipment, Columns,
            StructuralColumns, StructuralFraming, StructuralFoundations, Planting, Entourage, Parking, Site, Hardscape, Signage, Mass, Profiles,
        ]
    }
    /// The discipline filter the category belongs under in the Family Category dialog.
    pub fn discipline(self) -> &'static str {
        use FamilyCategory::*;
        match self {
            StructuralColumns | StructuralFraming | StructuralFoundations => "structure",
            ElectricalFixtures | ElectricalEquipment | LightingFixtures | LightingDevices | FireAlarmDevices | CommunicationDevices | DataDevices | SecurityDevices => "electrical",
            MechanicalEquipment | MechanicalControlDevices | AirTerminals => "mechanical",
            PlumbingFixtures | FireProtection => "piping",
            _ => "architecture",
        }
    }
    /// The IFC predefined type that goes with `ifc_class` for this category, or "" when
    /// the category covers several (the family can set its own).
    pub fn ifc_predefined(self) -> &'static str {
        match self {
            FamilyCategory::Hardscape => "PAVING",
            _ => "",
        }
    }
    pub fn ifc_class(self) -> &'static str {
        match self {
            FamilyCategory::GenericModels => "IfcBuildingElementProxy",
            FamilyCategory::Doors => "IfcDoor",
            FamilyCategory::Windows => "IfcWindow",
            FamilyCategory::Furniture | FamilyCategory::Casework => "IfcFurniture",
            FamilyCategory::PlumbingFixtures => "IfcSanitaryTerminal",
            FamilyCategory::LightingFixtures => "IfcLightFixture",
            FamilyCategory::ElectricalFixtures => "IfcElectricAppliance",
            FamilyCategory::ElectricalEquipment => "IfcElectricDistributionBoard",
            FamilyCategory::MechanicalEquipment => "IfcUnitaryEquipment",
            FamilyCategory::Columns => "IfcColumn",
            FamilyCategory::Planting | FamilyCategory::Site => "IfcGeographicElement",
            FamilyCategory::StructuralColumns => "IfcColumn",
            FamilyCategory::StructuralFraming => "IfcBeam",
            FamilyCategory::StructuralFoundations => "IfcFooting",
            FamilyCategory::FurnitureSystems => "IfcSystemFurnitureElement",
            FamilyCategory::LightingDevices => "IfcSwitchingDevice",
            FamilyCategory::FireAlarmDevices | FamilyCategory::SecurityDevices => "IfcSensor",
            FamilyCategory::CommunicationDevices | FamilyCategory::DataDevices => "IfcCommunicationsAppliance",
            FamilyCategory::MechanicalControlDevices => "IfcController",
            FamilyCategory::AirTerminals => "IfcAirTerminal",
            FamilyCategory::FireProtection => "IfcFireSuppressionTerminal",
            FamilyCategory::Hardscape => "IfcSlab",
            FamilyCategory::SpecialtyEquipment | FamilyCategory::FoodServiceEquipment | FamilyCategory::Entourage | FamilyCategory::Parking | FamilyCategory::Signage | FamilyCategory::Mass | FamilyCategory::Profiles => "IfcBuildingElementProxy",
        }
    }
}

/// What an instance of the family attaches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    /// Free standing, placed on a level.
    #[default]
    None,
    Wall,
    Floor,
    Ceiling,
    Roof,
    Face,
}

/// The plan symbol older files name, which code used to draw for families without a body. Nothing draws it now
/// (door-window-spec.md step 4): a family draws only its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanSymbol {
    #[default]
    None,
    SingleSwing,
    DoubleSwing,
    Sliding,
    /// Window: frame and glass lines.
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    #[default]
    Text,
    Length,
    Number,
    Integer,
    Angle,
    Area,
    Volume,
    YesNo,
    Material,
    /// Names a type of a loaded family of one category, as "Family : Type"; a nested component or a sweep's
    /// profile whose type follows it swaps to that family and type.
    FamilyType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl ParamValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ParamValue::Number(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ParamValue::Text(s) => Some(s),
            _ => None,
        }
    }
    pub fn display(&self) -> String {
        match self {
            ParamValue::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
            ParamValue::Number(n) => format!("{}", (n * 100.0).round() / 100.0),
            ParamValue::Text(s) => s.clone(),
        }
    }
}

/// A parameter the family declares (a family parameter).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    #[serde(default)]
    pub param_kind: ParamKind,
    /// Instance parameters can differ per placed copy; the rest are per type.
    #[serde(default)]
    pub instance: bool,
    /// Properties palette group, e.g. "Dimensions".
    #[serde(default)]
    pub group: String,
    /// Formula over other parameters. Stored for the formula engine; unused so far.
    #[serde(default)]
    pub formula: Option<String>,
    /// Reporting parameters read a dimension and cannot be set.
    #[serde(default)]
    pub reporting: bool,
    /// GUID when the parameter is a shared parameter.
    #[serde(default)]
    pub shared_guid: Option<String>,
    /// For text parameters: the values the palette offers in a drop-down.
    #[serde(default)]
    pub choices: Vec<String>,
    /// Discipline the data type belongs to (common, structural, hvac, electrical, piping, energy).
    #[serde(default = "common")]
    pub discipline: String,
    /// Help text shown when the pointer rests on the parameter in a palette.
    #[serde(default)]
    pub tooltip: String,
    /// Locked in the Family Types dialog: its value is not changed by flexing or typing there.
    #[serde(default)]
    pub locked: bool,
    /// For Family Type parameters: the category whose families it can name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_category: Option<FamilyCategory>,
    /// Value when a type or instance does not set one.
    pub default: ParamValue,
}

/// The values of the Wall Closure parameter, By Host first.
pub const WALL_CLOSURE: [&str; 5] = ["By Host", "Neither", "Interior", "Exterior", "Both"];

/// A Family Type parameter's value split into its family and type ("Panel : Glazed" gives the family "Panel" and the
/// type "Glazed"). A value without " : " is a type name alone, of whatever family the user already has.
pub fn split_family_type(v: &str) -> (Option<&str>, &str) {
    match v.split_once(" : ") {
        Some((f, t)) if !f.trim().is_empty() => (Some(f.trim()), t.trim()),
        _ => (None, v.trim()),
    }
}

fn common() -> String {
    "common".into()
}

impl ParamDef {
    fn new(name: &str, kind: ParamKind, instance: bool, group: &str, default: ParamValue) -> ParamDef {
        ParamDef { name: name.into(), param_kind: kind, instance, group: group.into(), formula: None, reporting: false, shared_guid: None, choices: vec![], discipline: common(), tooltip: String::new(), locked: false, family_category: None, default }
    }
    /// Wall Closure: which of the wall's layers wrap into the opening (By Host takes the wall type's Wrapping at
    /// Inserts). They wrap to the plane flagged Wall Closure, or round the core without one.
    pub fn wall_closure() -> ParamDef {
        ParamDef::choice("Wall Closure", false, "Construction", &WALL_CLOSURE)
    }
    /// A text parameter chosen from a list.
    fn choice(name: &str, instance: bool, group: &str, choices: &[&str]) -> ParamDef {
        ParamDef { choices: choices.iter().map(|c| c.to_string()).collect(), ..ParamDef::new(name, ParamKind::Text, instance, group, ParamValue::Text(choices[0].to_string())) }
    }
}

/// The yes/no and choice settings of the Family Category and Parameters dialog that are not a
/// category or host (those are `Family::category` and `Family::host`; Always vertical and Cut with
/// Voids When Loaded sit on the body).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FamilySettings {
    #[serde(default)]
    pub can_host_rebar: bool,
    #[serde(default = "normal_part")]
    pub part_type: String,
    /// Shared: nested copies in other families are counted and tagged on their own.
    #[serde(default)]
    pub shared: bool,
    /// Marks a point of the family that room calculations use (a door's point on either side).
    #[serde(default)]
    pub room_calculation_point: bool,
    /// Where the point is, in the family's own axes (mm).
    #[serde(default)]
    pub room_point: [f64; 3],
    #[serde(default)]
    pub room_from: Option<[f64; 3]>,
    #[serde(default)]
    pub room_to: Option<[f64; 3]>,
    #[serde(default)]
    pub room_bounding: Option<bool>,
    #[serde(default)]
    pub maintain_annotation_orientation: bool,
    /// Show dashed when above the cut plane: an instance wholly above a plan's cut plane draws with the Overhead
    /// line style. Off, it draws as seen, and its "Show only if above the cut plane" lines are the overhead drawing.
    #[serde(default = "yes")]
    pub dashed_above: bool,
    /// The family's own subcategories, offered first in the editor (the project's Object Styles hold their look).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subcategories: Vec<String>,
}

fn yes() -> bool {
    true
}

fn normal_part() -> String {
    "Normal".into()
}

impl Default for FamilySettings {
    fn default() -> Self {
        FamilySettings { can_host_rebar: false, part_type: normal_part(), shared: false, room_calculation_point: false, room_point: [0.0; 3], room_from: None, room_to: None, room_bounding: None, maintain_annotation_orientation: false, dashed_above: true, subcategories: vec![] }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Family {
    pub name: String,
    #[serde(default)]
    pub category: FamilyCategory,
    #[serde(default)]
    pub host: HostKind,
    /// Read from older files and kept; not drawn.
    #[serde(default)]
    pub plan_symbol: PlanSymbol,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    /// Made by code and shipped with OpenArch BIM. Only older projects' doors and windows carry it, and nothing
    /// treats them differently now: the starter families are bundled family files like any other.
    #[serde(default)]
    pub built_in: bool,
    /// The sizes the family is made in (the type catalogue). Only these can
    /// be added as types with "Add from catalogue"; a type can still be edited freely.
    #[serde(default)]
    pub catalog: Vec<CatalogRow>,
    /// The family's geometry; see `famgeom`. Empty for a family with nothing drawn, which shows only its
    /// footprint in plan and nothing in 3D.
    #[serde(default)]
    pub body: crate::famgeom::FamilyBody,
    #[serde(default)]
    pub settings: FamilySettings,
    /// Where the family file this copy was loaded from (or last saved to) sits. Nothing follows
    /// from it: the copy is not linked to the file. It is only offered again when the family is
    /// loaded back after an edit.
    #[serde(default)]
    pub source_path: Option<String>,
    /// A note of when the copy was loaded or saved (text the app supplies).
    #[serde(default)]
    pub loaded_at: Option<String>,
}

/// One row of a family's type catalogue: a type name and its type parameter values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogRow {
    pub name: String,
    #[serde(default)]
    pub values: BTreeMap<String, ParamValue>,
}

impl Family {
    /// Every family this one uses by name: its body's components and sweep profiles, and the families its Family
    /// Type parameters name, as defaults or in `types` (the values of its types).
    pub fn used_families<'a>(&self, types: impl IntoIterator<Item = &'a BTreeMap<String, ParamValue>>) -> Vec<String> {
        let mut out = self.body.used_families();
        let ft: Vec<&ParamDef> = self.params.iter().filter(|p| p.param_kind == ParamKind::FamilyType).collect();
        if !ft.is_empty() {
            let mut add = |v: Option<&ParamValue>| {
                if let Some((Some(f), _)) = v.and_then(|v| v.as_text()).map(split_family_type) {
                    out.push(f.to_string());
                }
            };
            for p in &ft {
                add(Some(&p.default));
            }
            for t in types {
                for p in &ft {
                    add(t.get(&p.name));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    pub fn param(&self, name: &str) -> Option<&ParamDef> {
        self.params.iter().find(|p| p.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FamilyType {
    pub family: Id,
    pub name: String,
    /// Type parameter values that differ from the family's defaults.
    #[serde(default)]
    pub values: BTreeMap<String, ParamValue>,
    /// Parameters whose value a global parameter sets (name to global).
    #[serde(default)]
    pub globals: BTreeMap<String, Id>,
}

/// Where a hosted instance sits on its host wall.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HostRef {
    pub wall: Id,
    /// Distance of the instance centre from the wall start, measured along the
    /// wall's drawn path (mm). The instance follows the wall if it moves.
    pub u_mm: f64,
    /// A door in a curtain wall: how far up the wall the point it holds is. It fills the panel over the cell holding
    /// (`u_mm`, `v_mm`), its width, height and sill set by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FamilyInstance {
    pub type_id: Id,
    #[serde(default)]
    pub level: Id,
    #[serde(default)]
    pub host: Option<HostRef>,
    /// Insertion point for free standing instances (hosted ones use `host`).
    #[serde(default)]
    pub location: Point,
    #[serde(default)]
    pub rotation_deg: f64,
    /// Elevation from Level: how far a free standing instance stands above its level (mm).
    /// Hosted doors and windows use their Sill Height instead.
    #[serde(default)]
    pub offset_mm: f64,
    /// Moves With Nearby Elements: follow a wall that moves near it. Stored; nothing moves it yet.
    #[serde(default)]
    pub moves_with_nearby: bool,
    /// Which way the instance faces across its host wall. False: towards the
    /// left of the wall (the exterior side).
    #[serde(default)]
    pub flip_facing: bool,
    /// Which side the hinge is on. False: the wall start side.
    #[serde(default)]
    pub flip_hand: bool,
    /// Instance parameter values that differ from the type's / family's.
    #[serde(default)]
    pub values: BTreeMap<String, ParamValue>,
    /// Instance parameters whose value a global parameter sets (name to global).
    #[serde(default)]
    pub globals: BTreeMap<String, Id>,
    #[serde(default)]
    pub mark: String,
    /// Pinned instances refuse move, delete and flip until unpinned.
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub phase_created: Option<Id>,
    #[serde(default)]
    pub phase_demolished: Option<Id>,
    #[serde(default)]
    pub ifc: IfcParams,
    /// A face host (a ceiling now; floors and roofs can follow): the instance stands at `location`, turned by
    /// `rotation_deg`, against the host's face there, `offset_mm` away from it (Offset from Host, out of the face).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_host: Option<FaceHost>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceHost {
    pub element: Id,
    pub kind: HostKind,
}

impl FamilyInstance {
    pub fn new(type_id: Id, level: Id) -> FamilyInstance {
        FamilyInstance {
            face_host: None,
            type_id,
            level,
            host: None,
            location: Point::new(0.0, 0.0),
            rotation_deg: 0.0,
            offset_mm: 0.0,
            moves_with_nearby: false,
            flip_facing: false,
            flip_hand: false,
            values: BTreeMap::new(),
            globals: BTreeMap::new(),
            mark: String::new(),
            pinned: false,
            comments: String::new(),
            phase_created: None,
            phase_demolished: None,
            ifc: IfcParams { guid: new_ifc_guid(), ..IfcParams::default() },
        }
    }
}

/// A hole in a wall, in the wall's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opening {
    pub instance: Id,
    /// Along the wall path from its start (mm).
    pub u0: f64,
    pub u1: f64,
    /// Elevations (mm).
    pub z0: f64,
    pub z1: f64,
}

// ---- starter families ------------------------------------------------------

/// A starter family with its types.
pub struct StarterFamily {
    pub family: Family,
    pub types: Vec<(String, BTreeMap<String, ParamValue>)>,
}

/// The starter families every new project has: the ceiling-hosted starters and the bundled family files (the
/// door and window profiles, panels, doors and windows).
pub fn starter_families() -> Vec<StarterFamily> {
    crate::ceiling::starter_ceiling_families()
        .into_iter()
        .chain(bundled_families())
        .collect()
}

/// The starter families shipped as family files (door-window-spec.md step 3), opened and changed in the family
/// editor like any other: the doors and windows first, then the panels and profiles they use.
const BUNDLED: [(&str, &str); 29] = [
    ("Single Flush Door - Metal Frame", include_str!("../families/Doors/Single Flush Door - Metal Frame.oaf")),
    ("Single Flush Door - Timber Frame", include_str!("../families/Doors/Single Flush Door - Timber Frame.oaf")),
    ("Double Flush Door - Metal Frame", include_str!("../families/Doors/Double Flush Door - Metal Frame.oaf")),
    ("Double Flush Door - Timber Frame", include_str!("../families/Doors/Double Flush Door - Timber Frame.oaf")),
    ("Glazed Hinged Door - Timber Frame", include_str!("../families/Doors/Glazed Hinged Door - Timber Frame.oaf")),
    ("Cavity Sliding Door", include_str!("../families/Doors/Cavity Sliding Door.oaf")),
    ("Sliding Glass Door - 2 Panel", include_str!("../families/Doors/Sliding Glass Door - 2 Panel.oaf")),
    ("Stacking Sliding Door - 3 Panel", include_str!("../families/Doors/Stacking Sliding Door - 3 Panel.oaf")),
    ("Stacking Sliding Door - 4 Panel", include_str!("../families/Doors/Stacking Sliding Door - 4 Panel.oaf")),
    ("Bifold Door - 3 Panel", include_str!("../families/Doors/Bifold Door - 3 Panel.oaf")),
    ("Bifold Door - 4 Panel", include_str!("../families/Doors/Bifold Door - 4 Panel.oaf")),
    ("Garage Door - Sectional", include_str!("../families/Doors/Garage Door - Sectional.oaf")),
    ("Garage Door - Roller", include_str!("../families/Doors/Garage Door - Roller.oaf")),
    ("Entry Door with Sidelight", include_str!("../families/Doors/Entry Door with Sidelight.oaf")),
    ("Curtain Wall Door - Single Glazed", include_str!("../families/Doors/Curtain Wall Door - Single Glazed.oaf")),
    ("Fixed Window", include_str!("../families/Windows/Fixed Window.oaf")),
    ("Sliding Window", include_str!("../families/Windows/Sliding Window.oaf")),
    ("Awning Window", include_str!("../families/Windows/Awning Window.oaf")),
    ("Double Hung Window", include_str!("../families/Windows/Double Hung Window.oaf")),
    ("Casement Window", include_str!("../families/Windows/Casement Window.oaf")),
    ("Louvre Window", include_str!("../families/Windows/Louvre Window.oaf")),
    ("Door Panel - Flush", include_str!("../families/Panels/Door Panel - Flush.oaf")),
    ("Door Panel - Flush Vision Panel", include_str!("../families/Panels/Door Panel - Flush Vision Panel.oaf")),
    ("Door Panel - Glazed", include_str!("../families/Panels/Door Panel - Glazed.oaf")),
    ("Metal Door Frame - Square Bend", include_str!("../families/Profiles/Metal Door Frame - Square Bend.oaf")),
    ("Metal Door Frame - Square Bend Slider", include_str!("../families/Profiles/Metal Door Frame - Square Bend Slider.oaf")),
    ("Timber Door Jamb - Rebated", include_str!("../families/Profiles/Timber Door Jamb - Rebated.oaf")),
    ("Aluminium Window Frame", include_str!("../families/Profiles/Aluminium Window Frame.oaf")),
    ("Aluminium Sliding Door Frame", include_str!("../families/Profiles/Aluminium Sliding Door Frame.oaf")),
];

/// The bundled family files read into starter families, with their types.
pub fn bundled_families() -> Vec<StarterFamily> {
    BUNDLED
        .iter()
        .map(|(name, text)| {
            let file: crate::faminst::FamilyFile = serde_json::from_str(text).unwrap_or_else(|e| panic!("bundled family {name}: {e}"));
            StarterFamily { family: file.family, types: file.types.into_iter().map(|t| (t.name, t.values)).collect() }
        })
        .collect()
}
