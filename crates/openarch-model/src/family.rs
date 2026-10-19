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

/// Categories a family can belong to. Doors and windows can be placed today; the rest
/// can be authored and saved and are placed once the Component tool exists.
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
        }
    }
    /// Every category in the order the family editor lists them.
    pub fn all() -> &'static [FamilyCategory] {
        use FamilyCategory::*;
        &[
            GenericModels, Doors, Windows, Furniture, FurnitureSystems, Casework, PlumbingFixtures, LightingFixtures, LightingDevices, ElectricalFixtures, ElectricalEquipment, FireAlarmDevices,
            CommunicationDevices, DataDevices, SecurityDevices, MechanicalEquipment, MechanicalControlDevices, AirTerminals, FireProtection, SpecialtyEquipment, FoodServiceEquipment, Columns,
            StructuralColumns, StructuralFraming, StructuralFoundations, Planting, Entourage, Parking, Site, Hardscape, Signage, Mass,
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
            FamilyCategory::SpecialtyEquipment | FamilyCategory::FoodServiceEquipment | FamilyCategory::Entourage | FamilyCategory::Parking | FamilyCategory::Signage | FamilyCategory::Mass => "IfcBuildingElementProxy",
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

/// How the family draws in a plan view.
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
    /// Value when a type or instance does not set one.
    pub default: ParamValue,
}

fn common() -> String {
    "common".into()
}

impl ParamDef {
    fn new(name: &str, kind: ParamKind, instance: bool, group: &str, default: ParamValue) -> ParamDef {
        ParamDef { name: name.into(), param_kind: kind, instance, group: group.into(), formula: None, reporting: false, shared_guid: None, choices: vec![], discipline: common(), tooltip: String::new(), locked: false, default }
    }
    /// A text parameter chosen from a list.
    fn choice(name: &str, instance: bool, group: &str, choices: &[&str]) -> ParamDef {
        ParamDef { choices: choices.iter().map(|c| c.to_string()).collect(), ..ParamDef::new(name, ParamKind::Text, instance, group, ParamValue::Text(choices[0].to_string())) }
    }
    /// A type parameter worked out from others by a formula. It cannot be set.
    fn formula(name: &str, group: &str, formula: &str) -> ParamDef {
        ParamDef { formula: Some(formula.into()), reporting: true, ..ParamDef::new(name, ParamKind::Length, false, group, ParamValue::Number(0.0)) }
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
    pub maintain_annotation_orientation: bool,
}

fn normal_part() -> String {
    "Normal".into()
}

impl Default for FamilySettings {
    fn default() -> Self {
        FamilySettings { can_host_rebar: false, part_type: normal_part(), shared: false, room_calculation_point: false, room_point: [0.0; 3], maintain_annotation_orientation: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Family {
    pub name: String,
    #[serde(default)]
    pub category: FamilyCategory,
    #[serde(default)]
    pub host: HostKind,
    #[serde(default)]
    pub plan_symbol: PlanSymbol,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    /// Shipped with OpenArch BIM rather than loaded from a file.
    #[serde(default)]
    pub built_in: bool,
    /// The sizes the family is made in (the type catalogue). Only these can
    /// be added as types with "Add from catalogue"; a type can still be edited freely.
    #[serde(default)]
    pub catalog: Vec<CatalogRow>,
    /// The family's geometry. Empty for families drawn by code (older files and custom
    /// parameter-only families); see `famgeom`.
    #[serde(default)]
    pub body: crate::famgeom::FamilyBody,
    #[serde(default)]
    pub settings: FamilySettings,
}

/// One row of a family's type catalogue: a type name and its type parameter values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogRow {
    pub name: String,
    #[serde(default)]
    pub values: BTreeMap<String, ParamValue>,
}

impl Family {
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
}

/// Where a hosted instance sits on its host wall.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HostRef {
    pub wall: Id,
    /// Distance of the instance centre from the wall start, measured along the
    /// wall's drawn path (mm). The instance follows the wall if it moves.
    pub u_mm: f64,
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
}

impl FamilyInstance {
    pub fn new(type_id: Id, level: Id) -> FamilyInstance {
        FamilyInstance {
            type_id,
            level,
            host: None,
            location: Point::new(0.0, 0.0),
            rotation_deg: 0.0,
            flip_facing: false,
            flip_hand: false,
            values: BTreeMap::new(),
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

// ---- built-in families ------------------------------------------------------

use ParamKind as K;
use ParamValue as V;

fn len(x: f64) -> ParamValue {
    V::Number(x)
}
fn txt(s: &str) -> ParamValue {
    V::Text(s.into())
}

/// Parameters shared by every door and window family.
fn common_params(sill_default: f64) -> Vec<ParamDef> {
    vec![
        ParamDef::new("Sill Height", K::Length, true, "Constraints", len(sill_default)),
        ParamDef::new("Width", K::Length, false, "Dimensions", len(820.0)),
        ParamDef::new("Height", K::Length, false, "Dimensions", len(2040.0)),
        ParamDef::new("Rough Width", K::Length, false, "Dimensions", len(820.0)),
        ParamDef::new("Rough Height", K::Length, false, "Dimensions", len(2040.0)),
        ParamDef::new("Frame Width", K::Length, false, "Construction", len(70.0)),
        ParamDef::new("Frame Depth", K::Length, false, "Construction", len(90.0)),
        ParamDef::new("Wall Closure", K::Text, false, "Construction", txt("By host")),
        ParamDef::new("Construction Type", K::Text, false, "Construction", txt("")),
        ParamDef::new("Trim Projection Ext", K::Length, false, "Dimensions", len(0.0)),
        ParamDef::new("Trim Projection Int", K::Length, false, "Dimensions", len(0.0)),
        ParamDef::new("Trim Width", K::Length, false, "Dimensions", len(0.0)),
        ParamDef::new("Material", K::Material, false, "Materials and Finishes", txt("")),
        ParamDef::new("Frame Material", K::Material, false, "Materials and Finishes", txt("")),
        ParamDef::new("Finish", K::Text, false, "Materials and Finishes", txt("")),
        ParamDef::new("Keynote", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Model", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Manufacturer", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Type Comments", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("URL", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Description", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Assembly Description", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Assembly Code", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Type Mark", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Fire Rating", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Cost", K::Number, false, "Identity Data", V::Number(0.0)),
        ParamDef::new("OmniClass Number", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("OmniClass Title", K::Text, false, "Identity Data", txt("")),
        ParamDef::new("Operation", K::Text, false, "IFC Parameters", txt("")),
        // thermal values matter for NCC energy provisions; stored now, used by analysis later
        ParamDef::new("Heat Transfer Coefficient (U)", K::Number, false, "Analytical Properties", V::Number(0.0)),
        ParamDef::new("Thermal Resistance (R)", K::Number, false, "Analytical Properties", V::Number(0.0)),
        ParamDef::new("Solar Heat Gain Coefficient", K::Number, false, "Analytical Properties", V::Number(0.0)),
        ParamDef::new("Visual Light Transmittance", K::Number, false, "Analytical Properties", V::Number(0.0)),
    ]
}

fn door_params() -> Vec<ParamDef> {
    let mut p = common_params(0.0);
    p.push(ParamDef::new("Thickness", K::Length, false, "Dimensions", len(40.0)));
    p.push(ParamDef::new("Function", K::Text, false, "Construction", txt("Interior")));
    p
}

fn window_params() -> Vec<ParamDef> {
    let mut p = common_params(900.0);
    // windows are sized by the opening; the frame sits inside it
    for d in &mut p {
        match d.name.as_str() {
            "Width" | "Rough Width" => d.default = len(1200.0),
            "Height" | "Rough Height" => d.default = len(1200.0),
            _ => {}
        }
    }
    p.push(ParamDef::new("Glazing Thickness", K::Length, false, "Dimensions", len(6.0)));
    p.push(ParamDef::new("Window Inset", K::Length, false, "Dimensions", len(0.0)));
    p.push(ParamDef::new("Glass Material", K::Material, false, "Materials and Finishes", txt("")));
    p.push(ParamDef::new("Sash Material", K::Material, false, "Materials and Finishes", txt("")));
    p.push(ParamDef::new("Default Sill Height", K::Length, false, "Constraints", len(900.0)));
    p
}

/// Built-in doors come with a frame. Internal and thin walls get the pressed metal
/// frame most used in Australia: a double rebate section (a centre rib with a recess
/// for the leaf on each side) with the architrave pressed into the profile. Thick
/// external walls get a timber jamb built into the opening, rebated, with no
/// architrave. Which one a door has is the instance's `Frame Type`, chosen from the
/// wall at placement and editable afterwards. Custom families carry none of this and
/// behave like the standard families.
///
/// The leaf is sized by `Width` (all leaves together) and `Height`. The reveal (the
/// clear opening the leaf sits in) and the hole in the wall follow by formula from
/// the frame's own parameters. Defaults come from supplier brochures and still need
/// checking against the catalogue Lawrence buys from.
fn framed_door_params() -> Vec<ParamDef> {
    let mut p = door_params();
    p.retain(|d| !matches!(d.name.as_str(), "Frame Width" | "Frame Depth" | "Rough Width" | "Rough Height"));
    for d in &mut p {
        if d.name == "Thickness" {
            d.default = len(35.0);
        }
        if d.name == "Wall Closure" {
            d.default = txt("By host");
        }
    }
    p.push(ParamDef::choice("Frame Type", true, "Frame", &["Pressed Metal", "Timber"]));
    // depth of the frame across the wall: the back opening of a metal frame, the width of a timber jamb
    p.push(ParamDef::new("Jamb Depth", K::Length, true, "Frame", len(112.0)));
    p.push(ParamDef::choice("Rebate Style", false, "Frame (Pressed Metal)", &["Double Rebate"]));
    p.push(ParamDef::new("Steel Gauge", K::Length, false, "Frame (Pressed Metal)", len(1.2)));
    p.push(ParamDef::new("Rib Width", K::Length, false, "Frame (Pressed Metal)", len(34.0)));
    p.push(ParamDef::new("Rib Height", K::Length, false, "Frame (Pressed Metal)", len(9.0)));
    p.push(ParamDef::new("Architrave Width", K::Length, false, "Frame (Pressed Metal)", len(40.0)));
    p.push(ParamDef::new("Architrave Projection", K::Length, false, "Frame (Pressed Metal)", len(14.0)));
    p.push(ParamDef::new("Jamb Thickness", K::Length, false, "Frame (Timber)", len(42.0)));
    p.push(ParamDef::new("Stop Width", K::Length, false, "Frame (Timber)", len(38.0)));
    p.push(ParamDef::new("Stop Height", K::Length, false, "Frame (Timber)", len(12.0)));
    p.push(ParamDef::new("Stop Offset", K::Length, false, "Frame (Timber)", len(20.0)));
    p.push(ParamDef::new("Frame Clearance Width", K::Length, false, "Frame", len(4.0)));
    p.push(ParamDef::new("Frame Clearance Height", K::Length, false, "Frame", len(15.0)));
    p.push(ParamDef::new("Leaf Bottom Gap", K::Length, false, "Frame", len(20.0)));
    p.push(ParamDef::formula("Reveal Width", "Dimensions", "[Width] + [Frame Clearance Width]"));
    p.push(ParamDef::formula("Reveal Height", "Dimensions", "[Height] + [Frame Clearance Height]"));
    // the hole in the wall: the reveal plus the frame's material each side
    p.push(ParamDef::formula("Rough Width", "Dimensions", "[Reveal Width] + 2 * if([Frame Type] == \"Timber\", [Jamb Thickness], [Steel Gauge])"));
    p.push(ParamDef::formula("Rough Height", "Dimensions", "[Reveal Height] + if([Frame Type] == \"Timber\", [Jamb Thickness], [Steel Gauge])"));
    p
}

/// Back openings pressed metal frames are made in, mm (the depth that fits over the
/// wall: stud walls, 110 mm masonry, 190 mm block).
pub const METAL_BACK_OPENINGS: [f64; 3] = [92.0, 112.0, 192.0];
/// Widths timber jambs are cut in, mm.
pub const TIMBER_JAMBS: [f64; 3] = [138.0, 163.0, 185.0];
/// Walls at or above this finished thickness get a timber jamb instead of a pressed
/// metal frame.
pub const TIMBER_FRAME_FROM: f64 = 195.0;
/// How near a wall's thickness a standard back opening must be to be used as is.
const BACK_OPENING_SNAP: f64 = 4.0;

/// The frame a door gets in a wall: its type and depth across the wall.
/// `thickness` is the finished wall, `core` the structural core inside it.
pub fn frame_for_wall(thickness: f64, core: f64) -> (&'static str, f64) {
    if thickness >= TIMBER_FRAME_FROM {
        let want = core + 26.0; // the core and its lining
        let jamb = TIMBER_JAMBS.iter().copied().find(|j| *j >= want - 1e-6).unwrap_or(TIMBER_JAMBS[TIMBER_JAMBS.len() - 1]);
        ("Timber", jamb)
    } else {
        let near = METAL_BACK_OPENINGS.iter().copied().find(|b| (b - thickness).abs() <= BACK_OPENING_SNAP);
        ("Pressed Metal", near.unwrap_or(thickness))
    }
}

/// Sizes framed door sets are made in: leaf widths, then the two common leaf
/// heights. Doubles are two equal leaves.
const FRAMED_SINGLE_WIDTHS: [f64; 9] = [520.0, 620.0, 720.0, 770.0, 820.0, 870.0, 920.0, 970.0, 1020.0];
const FRAMED_DOUBLE_LEAVES: [f64; 4] = [520.0, 620.0, 720.0, 820.0];
const FRAMED_LEAF_HEIGHTS: [f64; 2] = [2040.0, 2340.0];

fn framed_catalog(double: bool) -> Vec<CatalogRow> {
    let mut rows = vec![];
    let sizes: Vec<f64> = if double { FRAMED_DOUBLE_LEAVES.iter().map(|w| w * 2.0).collect() } else { FRAMED_SINGLE_WIDTHS.to_vec() };
    for w in sizes {
        for h in FRAMED_LEAF_HEIGHTS {
            let name = if double { format!("2 x {:.0} x {:.0}", w / 2.0, h) } else { format!("{w:.0} x {h:.0}") };
            let mut values = BTreeMap::new();
            values.insert("Width".to_string(), len(w));
            values.insert("Height".to_string(), len(h));
            values.insert("Frame Clearance Width".to_string(), len(if double { 8.0 } else { 4.0 }));
            rows.push(CatalogRow { name, values });
        }
    }
    rows
}

/// A starter family with its types: `(name, width, height, sill)`.
pub struct StarterFamily {
    pub family: Family,
    pub types: Vec<(String, BTreeMap<String, ParamValue>)>,
}

fn sized(w: f64, h: f64, extra: &[(&str, ParamValue)]) -> BTreeMap<String, ParamValue> {
    let mut m = BTreeMap::new();
    m.insert("Width".to_string(), len(w));
    m.insert("Height".to_string(), len(h));
    m.insert("Rough Width".to_string(), len(w));
    m.insert("Rough Height".to_string(), len(h));
    for (k, v) in extra {
        m.insert((*k).to_string(), v.clone());
    }
    m
}

/// The Australian starter set of door and window families.
pub fn starter_families() -> Vec<StarterFamily> {
    let door = |name: &str, symbol: PlanSymbol, types: Vec<(String, BTreeMap<String, ParamValue>)>| StarterFamily {
        family: Family { name: name.into(), category: FamilyCategory::Doors, host: HostKind::Wall, plan_symbol: symbol, params: door_params(), built_in: true, catalog: vec![], settings: Default::default(), body: if symbol == PlanSymbol::Sliding { crate::famdoor::sliding_door_body() } else { Default::default() } },
        types,
    };
    let window = |name: &str, types: Vec<(String, BTreeMap<String, ParamValue>)>| StarterFamily {
        family: Family { name: name.into(), category: FamilyCategory::Windows, host: HostKind::Wall, plan_symbol: PlanSymbol::Window, params: window_params(), built_in: true, catalog: vec![], settings: Default::default(), body: crate::famdoor::window_body() },
        types,
    };
    let ext = |w: f64, h: f64| (format!("{w:.0} x {h:.0} Exterior"), sized(w, h, &[("Function", txt("Exterior"))]));
    let wt = |w: f64, h: f64, sill: f64| (format!("{w:.0} x {h:.0}"), sized(w, h, &[("Default Sill Height", len(sill))]));
    // framed sets start with the sizes most used (820 and 920 singles, 2 x 820 doubles)
    let framed = |name: &str, symbol: PlanSymbol, double: bool, first: &[&str]| {
        let catalog = framed_catalog(double);
        let types = catalog.iter().filter(|r| first.contains(&r.name.as_str())).map(|r| (r.name.clone(), r.values.clone())).collect();
        StarterFamily {
            family: Family { name: name.into(), category: FamilyCategory::Doors, host: HostKind::Wall, plan_symbol: symbol, params: framed_door_params(), built_in: true, catalog, settings: Default::default(), body: crate::famdoor::flush_door_body(double) },
            types,
        }
    };
    vec![
        framed("Single-Flush", PlanSymbol::SingleSwing, false, &["720 x 2040", "820 x 2040", "920 x 2040", "820 x 2340"]),
        framed("Double-Flush", PlanSymbol::DoubleSwing, true, &["2 x 720 x 2040", "2 x 820 x 2040"]),
        door("Sliding-Glass", PlanSymbol::Sliding, vec![ext(1800.0, 2100.0), ext(2400.0, 2100.0), ext(3000.0, 2100.0)]),
        window("Fixed", vec![wt(600.0, 1200.0, 900.0), wt(1200.0, 1200.0, 900.0), wt(1800.0, 1200.0, 900.0), wt(2400.0, 2100.0, 0.0)]),
        window("Awning", vec![wt(600.0, 600.0, 1500.0), wt(900.0, 600.0, 1500.0), wt(1200.0, 900.0, 1200.0), wt(1800.0, 900.0, 1200.0)]),
        window("Sliding", vec![wt(1200.0, 1200.0, 900.0), wt(1800.0, 1200.0, 900.0), wt(2400.0, 1200.0, 900.0)]),
    ]
}
