//! Schedule-to-IFC mappings. Values are per element, independent of table grouping and totals.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::{Id, schedule::Unit};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IfcSchedule {
    #[serde(default)] pub enabled: bool,
    #[serde(default)] pub name: String,
    #[serde(default)] pub quantity_name: String,
    #[serde(default)] pub include_hidden: bool,
    /// Empty means all visible fields with their current column headings.
    #[serde(default)] pub fields: Vec<IfcField>,
    #[serde(default)] pub explicit: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IfcField {
    pub field: String,
    #[serde(default)] pub name: String,
    #[serde(default)] pub quantity: bool,
    #[serde(default)] pub unit: Option<Unit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IfcProperty {
    pub name: String,
    pub value: Value,
    pub unit: Unit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IfcPropertySet {
    pub source: Id,
    pub element: Id,
    pub name: String,
    pub quantities: bool,
    pub properties: Vec<IfcProperty>,
}

pub fn quantity_unit(unit: Unit) -> bool {
    matches!(unit, Unit::Length | Unit::Area | Unit::Volume | Unit::Number | Unit::Integer)
}
