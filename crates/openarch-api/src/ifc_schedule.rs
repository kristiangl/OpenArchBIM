//! Per-element schedule values for IFC, without compressed rows, subtotals or display rounding.
use std::collections::BTreeMap;
use serde_json::{json, Value};
use openarch_model::ifc_properties::{IfcField, IfcProperty, IfcPropertySet};
use openarch_model::schedule::{SchedField, Unit};
use crate::Session;

impl Session {
    pub(crate) fn ifc_schedule_properties(&self, options: &Value) -> Result<Vec<IfcPropertySet>, String> {
        if !options["schedules"].as_bool().unwrap_or(false) { return Ok(vec![]); }
        let selected = options["selected_only"].as_bool().unwrap_or(true);
        let named = options["named_only"].as_bool().unwrap_or(false);
        let mut sets: BTreeMap<(u64,String,bool), IfcPropertySet> = BTreeMap::new();
        for (id, view) in self.doc.schedules() {
            let def = view.schedule.as_ref().unwrap();
            if def.key.is_some() || matches!(def.category.as_str(), "Sheets" | "Views" | "Revisions" | "Revision Clouds") { continue; }
            if selected && !def.ifc.enabled { continue; }
            if named && !["ifc","pset","common"].iter().any(|part| view.name.to_lowercase().contains(part)) { continue; }
            let mappings: Vec<IfcField> = if !def.ifc.explicit && def.ifc.fields.is_empty() {
                def.fields.iter().filter(|f| !f.hidden || def.ifc.include_hidden).map(|f| IfcField { field:f.name.clone(),name:f.heading().into(),quantity:false,unit:None }).collect()
            } else { def.ifc.fields.clone() };
            let mut individual = def.clone();
            individual.embedded=None;
            individual.itemize=true; individual.grand_totals=false; individual.sort.clear();
            for f in &mut individual.fields { f.hidden=false; }
            if individual.takeoff && !individual.fields.iter().any(|f| f.name=="Material: Name") {
                individual.fields.push(serde_json::from_value::<SchedField>(json!({"name":"Material: Name","kind":"material"})).map_err(|e|e.to_string())?);
            }
            let table=self.schedule_table_definition(id,None,Some(&individual))?;
            let columns=table["columns"].as_array().ok_or("Missing schedule columns")?;
            for row in table["rows"].as_array().into_iter().flatten().filter(|r|r["kind"]=="item") {
                let Some(element)=row["ids"][0].as_u64() else {continue};
                if self.doc.room(element).is_some() && self.doc.room_calculation(element).is_none_or(|r|r.status!=openarch_model::rooms::RoomStatus::Placed){continue;}
                let material=columns.iter().position(|c|c["name"]=="Material: Name").and_then(|i|row["cells"][i]["value"].as_str()).unwrap_or("");
                let base=if def.ifc.name.trim().is_empty(){view.name.as_str()}else{def.ifc.name.trim()};
                let qbase=if def.ifc.quantity_name.trim().is_empty(){format!("Qto_{base}")}else{def.ifc.quantity_name.trim().into()};
                for mapping in &mappings {
                    let Some(i)=columns.iter().position(|c|c["name"]==mapping.field) else{return Err(format!("{}: {} is not a field.",view.name,mapping.field))};
                    let cell=&row["cells"][i]; if cell["value"].is_null(){continue;}
                    let field=def.field(&mapping.field).unwrap();
                    let name=if mapping.name.trim().is_empty(){field.heading()}else{mapping.name.trim()};
                    let unit:Unit=mapping.unit.unwrap_or(serde_json::from_value(columns[i]["unit"].clone()).map_err(|e|e.to_string())?);
                    let value=match unit {
                        // a blank text value has no value: it is left out, as a missing parameter is
                        Unit::Text=>{let text=cell["text"].as_str().unwrap_or("");if text.trim().is_empty(){continue;}json!(text)},
                        Unit::YesNo=>json!(cell["value"].as_bool().or_else(||match cell["text"].as_str(){Some("Yes")=>Some(true),Some("No")=>Some(false),_=>None}).ok_or_else(||format!("{}: {name} needs a boolean.",view.name))?),
                        Unit::Integer=>{let n=cell["value"].as_f64().ok_or_else(||format!("{}: {name} needs a number.",view.name))?;if n.fract()!=0.0||n<i64::MIN as f64||n>=i64::MAX as f64{return Err(format!("{name} is not an integer."));}json!(n as i64)},
                        _=>json!(cell["value"].as_f64().filter(|n|n.is_finite()).ok_or_else(||format!("{}: {name} needs a number.",view.name))?),
                    };
                    if mapping.quantity&&(!openarch_model::ifc_properties::quantity_unit(unit)||value.as_f64().is_none_or(|n|n<0.0)){return Err(format!("{name} is not a non-negative IFC quantity."));}
                    let set_name=if mapping.quantity{qbase.clone()}else{base.into()};
                    let set_name=if individual.takeoff&&!material.is_empty(){format!("{set_name} [{material}]")}else{set_name};
                    let set=sets.entry((element,set_name.clone(),mapping.quantity)).or_insert_with(||IfcPropertySet{source:id,element,name:set_name,quantities:mapping.quantity,properties:vec![]});
                    let property=IfcProperty{name:name.into(),value,unit};
                    if let Some(old)=set.properties.iter().find(|p|p.name==name){if old!=&property{return Err(format!("Conflicting IFC property {name} in {}.",set.name));}}else{set.properties.push(property);}
                }
            }
        }
        Ok(sets.into_values().collect())
    }
}
