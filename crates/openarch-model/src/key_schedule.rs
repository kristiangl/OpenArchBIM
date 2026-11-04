//! Reusable sets of instance parameter values, selected on elements of one category.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::{Document, Element, Id};
use crate::schedule::{ScheduleDef, Unit, FieldKind};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectParameter {
    pub name: String,
    pub categories: Vec<String>,
    pub unit: Unit,
    #[serde(default)] pub shared_guid: Option<String>,
    #[serde(default)] pub group: String,
    #[serde(default, deserialize_with = "id_map")] pub values: BTreeMap<Id, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyBinding {
    #[serde(default)] pub path: Vec<String>,
    #[serde(default)] pub project_parameter: Option<Id>,
    #[serde(default)] pub shared_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyRow {
    pub id: u64,
    pub name: String,
    #[serde(default)] pub values: BTreeMap<String, Value>,
}

fn one() -> u64 { 1 }

// Object keys stay text through the internally tagged Element deserializer.
fn id_map<'de, D, T>(d: D) -> Result<BTreeMap<Id, T>, D::Error>
where D: serde::Deserializer<'de>, T: Deserialize<'de> {
    let raw = BTreeMap::<String, T>::deserialize(d)?;
    raw.into_iter().map(|(key, value)| key.parse::<Id>().map(|key| (key, value)).map_err(serde::de::Error::custom)).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeySchedule {
    pub parameter_name: String,
    #[serde(default)] pub rows: Vec<KeyRow>,
    #[serde(default = "one")] pub next_row_id: u64,
    #[serde(default, deserialize_with = "id_map")] pub assignments: BTreeMap<Id, u64>,
    #[serde(default)] pub bindings: BTreeMap<String, KeyBinding>,
}

impl KeySchedule {
    pub fn new(name: &str) -> Self {
        Self { parameter_name: name.into(), rows: vec![], next_row_id: 1, assignments: BTreeMap::new(), bindings: BTreeMap::new() }
    }
}

pub fn path_value<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(value, |v, key| v.get(key))
}

pub fn nested(path: &[String], value: Value) -> Value {
    path.iter().rev().fold(value, |v, key| json!({ key: v }))
}

pub fn same_binding(a: &KeyBinding, b: &KeyBinding) -> bool {
    if a.project_parameter.is_some() || b.project_parameter.is_some() { return a.project_parameter.is_some() && a.project_parameter == b.project_parameter; }
    if a.shared_guid.is_some() && a.shared_guid == b.shared_guid { return true; }
    a.path == b.path
}

pub fn check_value(value: &Value, unit: Unit) -> Result<(), String> {
    if value.is_null() { return Ok(()); }
    let valid = match unit {
        Unit::Text => value.is_string(),
        Unit::YesNo => value.is_boolean(),
        Unit::Integer => value.as_f64().is_some_and(|v| v.is_finite() && v.fract() == 0.0),
        _ => value.as_f64().is_some_and(f64::is_finite),
    };
    if valid { Ok(()) } else { Err(format!("Enter a {} value, or leave it blank.", unit.label().to_lowercase())) }
}

impl Document {
    pub fn parameter_category(&self, id: Id) -> Option<String> {
        match self.get(id)? {
            Element::Wall(_) => Some("Walls".into()),
            Element::Floor(_) => Some("Floors".into()),
            Element::Roof(_) => Some("Roofs".into()),
            Element::Room(_) => Some("Rooms".into()),
            Element::FamilyInstance(i) => self.instance_family(i).map(|(f, _)| f.category.label().into()),
            _ => None,
        }
    }

    pub fn project_parameters(&self) -> Vec<(Id, &ProjectParameter)> {
        self.iter().filter_map(|(id, e)| if let Element::ProjectParameter(p) = e { Some((id, p)) } else { None }).collect()
    }

    pub fn project_parameter(&self, id: Id) -> Option<&ProjectParameter> {
        match self.get(id) { Some(Element::ProjectParameter(p)) => Some(p), _ => None }
    }

    pub fn add_project_parameter(&mut self, mut p: ProjectParameter) -> Result<Id, String> {
        p.name = p.name.trim().to_string();
        if p.name.is_empty() || p.categories.is_empty() { return Err("A project parameter needs a name and category.".into()); }
        if self.project_parameters().iter().any(|(_, other)| other.categories.iter().any(|c| p.categories.contains(c)) && (other.name == p.name || p.shared_guid.is_some() && p.shared_guid == other.shared_guid)) {
            return Err("That parameter is already defined for this category.".into());
        }
        for v in p.values.values() { check_value(v, p.unit)?; }
        self.transact("Add project parameter", |tx| Ok(tx.add(Element::ProjectParameter(p))))
    }

    pub fn set_project_value(&mut self, parameter: Id, element: Id, value: Value) -> Result<(), String> {
        let mut p = self.project_parameter(parameter).cloned().ok_or("Unknown project parameter")?;
        if !self.parameter_category(element).is_some_and(|c| p.categories.contains(&c)) { return Err("The parameter does not apply to that element.".into()); }
        check_value(&value, p.unit)?;
        p.values.insert(element, value);
        self.transact("Change project parameter", |tx| tx.update(parameter, Element::ProjectParameter(p)))
    }

    /// Resolve shared family parameter identity independently of the spelling used by another family.
    pub fn key_path(&self, element: Id, binding: &KeyBinding) -> Option<Vec<String>> {
        if let Some(guid) = &binding.shared_guid {
            if let Some(Element::FamilyInstance(i)) = self.get(element) {
                let (family, _) = self.instance_family(i)?;
                let p = family.params.iter().find(|p| p.instance && p.shared_guid.as_ref() == Some(guid))?;
                return Some(vec!["values".into(), p.name.clone()]);
            }
        }
        Some(binding.path.clone())
    }

    pub fn key_control(&self, element: Id, path: &[String], parameter: Option<Id>) -> Option<(Id, String, String)> {
        for (schedule, view) in self.schedules() {
            let def = view.schedule.as_ref()?;
            let Some(key) = &def.key else { continue };
            let Some(row_id) = key.assignments.get(&element) else { continue };
            if !key.rows.iter().any(|r| r.id == *row_id) { continue; }
            for (field, b) in &key.bindings {
                if !def.fields.iter().any(|f| f.name == *field) { continue; }
                let matches = if parameter.is_some() { parameter == b.project_parameter }
                    else { b.project_parameter.is_none() && self.key_path(element, b).as_deref() == Some(path) };
                if matches { return Some((schedule, view.name.clone(), field.clone())); }
            }
        }
        None
    }

    /// Enforce read-only values in core transactions too, including edits made without the UI.
    pub(crate) fn check_key_change(&self, id: Id, before: &Element, after: &Element) -> Result<(), String> {
        if self.key_writing { return Ok(()); }
        if let (Element::ProjectParameter(a), Element::ProjectParameter(b)) = (before, after) {
            for element in a.values.keys().chain(b.values.keys()) {
                if a.values.get(element) != b.values.get(element) && self.key_control(*element, &[], Some(id)).is_some() {
                    return Err(format!("{} is controlled by a key schedule.", a.name));
                }
            }
            return Ok(());
        }
        if self.parameter_category(id).is_none() { return Ok(()); }
        let (a, b) = (serde_json::to_value(before).map_err(|e| e.to_string())?, serde_json::to_value(after).map_err(|e| e.to_string())?);
        for (_, view) in self.schedules() {
            let Some(def) = &view.schedule else { continue };
            let Some(key) = &def.key else { continue };
            if !key.assignments.contains_key(&id) { continue; }
            for (name, binding) in &key.bindings {
                if binding.project_parameter.is_some() || !def.fields.iter().any(|f| f.name == *name) { continue; }
                if let Some(path) = self.key_path(id, binding) {
                    let global_changed = if path.first().is_some_and(|p| p == "values") {
                        path.get(1).is_some_and(|p| match (before, after) { (Element::FamilyInstance(a), Element::FamilyInstance(b)) => a.globals.get(p) != b.globals.get(p), _ => false })
                    } else { false };
                    if global_changed || path_value(&a, &path) != path_value(&b, &path) { return Err(format!("{name} is controlled by {}.", view.name)); }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn check_key_definition(&self, id: Id, def: &ScheduleDef) -> Result<(), String> {
        let Some(key) = &def.key else { return Ok(()) };
        if def.takeoff || matches!(def.category.as_str(), "Multi-Category" | "Sheets" | "Views") { return Err("A key schedule belongs to one model category.".into()); }
        if key.parameter_name.trim().is_empty() { return Err("Enter a name for the key selector.".into()); }
        for (sid, view) in self.schedules().into_iter().filter(|(sid, _)| *sid != id) {
            let other = view.schedule.as_ref().unwrap();
            if other.category != def.category { continue; }
            let Some(k) = &other.key else { continue };
            if k.parameter_name == key.parameter_name { return Err("That key selector name is already in use.".into()); }
            for (name, binding) in &key.bindings {
                if k.bindings.iter().any(|(n, b)| n == name || same_binding(b, binding)) { return Err(format!("{name} is already controlled by {} ({sid}).", view.name)); }
            }
        }
        let bindings: Vec<_> = key.bindings.iter().collect();
        for (i, (name, binding)) in bindings.iter().enumerate() {
            if bindings[..i].iter().any(|(_, other)| same_binding(binding, other)) { return Err(format!("{name} refers to a parameter already controlled by another field.")); }
        }
        for (i, row) in key.rows.iter().enumerate() {
            if row.id == 0 || row.name.trim().is_empty() || key.rows[..i].iter().any(|r| r.id == row.id || r.name.eq_ignore_ascii_case(&row.name)) { return Err("Each key needs a unique name and ID.".into()); }
            for (name, value) in &row.values {
                if let Some(f) = def.fields.iter().find(|f| f.name == *name) { check_value(value, f.unit)?; }
            }
        }
        if def.fields.iter().any(|f| f.kind != FieldKind::Instance || f.name != "Key Name" && !key.bindings.contains_key(&f.name)) { return Err("Keys can control only editable instance parameters.".into()); }
        if !def.fields.iter().any(|f| f.name == "Key Name") { return Err("A key schedule must include Key Name.".into()); }
        Ok(())
    }

    /// Write a definition and its assigned values in one atomic undo step.
    pub(crate) fn write_key_definition(&mut self, id: Id, mut def: ScheduleDef) -> Result<(), String> {
        self.check_key_definition(id, &def)?;
        let key = def.key.as_mut().ok_or("not a key schedule")?;
        key.assignments.retain(|element, row| self.parameter_category(*element).as_deref() == Some(&def.category) && key.rows.iter().any(|r| r.id == *row));
        let key = key.clone();
        let old_selector = self.schedule(id).and_then(|s| s.key.as_ref()).map(|k| k.parameter_name.clone());
        let mut view = self.view(id).cloned().ok_or("not a view")?;
        view.schedule = Some(def.clone());
        let prior = self.key_writing;
        self.key_writing = true;
        let result = self.grouped("Change key schedule", |d| {
            d.transact("Change key definition", |tx| tx.update(id, Element::View(view)))?;
            if let Some(old) = old_selector.filter(|old| *old != key.parameter_name) {
                let schedules: Vec<_> = d.schedules().into_iter().filter(|(sid,v)| *sid != id && v.schedule.as_ref().is_some_and(|s| s.category == def.category && s.key.is_none())).map(|(id,v)| (id,v.clone())).collect();
                for (sid, mut v) in schedules {
                    let s = v.schedule.as_mut().unwrap();
                    for f in &mut s.fields {
                        if f.name == old && f.kind == FieldKind::Instance { f.name = key.parameter_name.clone(); }
                        f.formula = f.formula.replace(&format!("[{old}]"), &format!("[{}]", key.parameter_name));
                        if f.of == old { f.of = key.parameter_name.clone(); }
                        for p in &mut f.parts { if p.field == old { p.field = key.parameter_name.clone(); } }
                        for h in &mut f.highlights { if h.field == old { h.field = key.parameter_name.clone(); } }
                    }
                    for f in &mut s.filters { if f.field == old { f.field = key.parameter_name.clone(); } }
                    for sort in &mut s.sort { if sort.field == old { sort.field = key.parameter_name.clone(); } }
                    d.transact("Rename key selector", |tx| tx.update(sid, Element::View(v)))?;
                }
            }
            for (&element, &row) in &key.assignments {
                let row = key.rows.iter().find(|r| r.id == row).ok_or("Unknown key")?;
                for field in &def.fields {
                    let Some(binding) = key.bindings.get(&field.name) else { continue };
                    let mut value = row.values.get(&field.name).cloned().unwrap_or(Value::Null);
                    if let Some(parameter) = binding.project_parameter {
                        if d.project_parameter(parameter).and_then(|p| p.values.get(&element)) != Some(&value) { d.set_project_value(parameter, element, value)?; }
                        continue;
                    }
                    let Some(path) = d.key_path(element, binding) else { continue };
                    let Some(source) = d.get(element).cloned() else { continue };
                    if path.first().is_some_and(|p| p == "values") {
                        let Element::FamilyInstance(i) = &source else { continue };
                        let Some((fam, typ)) = d.instance_family(i) else { continue };
                        let Some(p) = path.get(1).and_then(|name| fam.param(name)) else { continue };
                        if p.formula.is_some() || p.reporting || i.globals.contains_key(&p.name) { continue; }
                        if value.is_null() { value = serde_json::to_value(d.type_param(typ, &p.name).unwrap_or_else(|| p.default.clone())).map_err(|e| e.to_string())?; }
                        if !p.choices.is_empty() && !value.as_str().is_some_and(|s| p.choices.iter().any(|c| c == s)) { return Err(format!("Choose an available value for {}.", p.name)); }
                    } else if value.is_null() {
                        value = match field.unit { Unit::Text => json!(""), Unit::YesNo => json!(false), _ => json!(0) };
                    }
                    let raw = serde_json::to_value(&source).map_err(|e| e.to_string())?;
                    if path_value(&raw, &path).is_some_and(|v| v == &value || v.as_f64().zip(value.as_f64()).is_some_and(|(a,b)| (a-b).abs()<1e-9)) { continue; }
                    let patch = nested(&path, value);
                    match source {
                        Element::FamilyInstance(_) => d.patch_instance(element, &patch)?,
                        Element::Wall(_) => d.patch_wall(element, &patch)?,
                        Element::Floor(_) => d.patch_floor(element, &patch)?,
                        Element::Roof(_) => d.patch_roof(element, &patch)?,
                        Element::Room(_) => d.patch_room(element, &patch)?,
                        _ => return Err("That element cannot use a key.".into()),
                    }
                }
            }
            Ok(())
        });
        self.key_writing = prior;
        result
    }

    pub fn assign_key(&mut self, schedule: Id, ids: &[Id], row: Option<u64>) -> Result<(), String> {
        let mut def = self.schedule(schedule).cloned().ok_or("not a schedule")?;
        let key = def.key.as_mut().ok_or("not a key schedule")?;
        if row.is_some_and(|r| !key.rows.iter().any(|k| k.id == r)) { return Err("Unknown key.".into()); }
        for &id in ids {
            if self.parameter_category(id).as_deref() != Some(&def.category) { return Err("That element belongs to a different category.".into()); }
            if let Some(row) = row { key.assignments.insert(id, row); } else { key.assignments.remove(&id); }
        }
        self.write_key_definition(schedule, def)
    }

    pub fn add_key_row(&mut self, schedule: Id) -> Result<u64, String> {
        let mut def = self.schedule(schedule).cloned().ok_or("not a schedule")?;
        let key = def.key.as_mut().ok_or("not a key schedule")?;
        let id = key.next_row_id.max(key.rows.iter().map(|r| r.id).max().unwrap_or(0) + 1);
        let mut name = id.to_string();
        while key.rows.iter().any(|r| r.name == name) { name.push('a'); }
        key.rows.push(KeyRow { id, name, values: BTreeMap::new() }); key.next_row_id = id + 1;
        self.write_key_definition(schedule, def)?;
        Ok(id)
    }

    pub fn patch_key_row(&mut self, schedule: Id, row: u64, patch: &Value) -> Result<(), String> {
        let mut def = self.schedule(schedule).cloned().ok_or("not a schedule")?;
        let key = def.key.as_mut().ok_or("not a key schedule")?;
        let r = key.rows.iter_mut().find(|r| r.id == row).ok_or("Unknown key")?;
        if let Some(name) = patch.get("name") { r.name = name.as_str().ok_or("Key name must be text")?.trim().into(); }
        if let Some(values) = patch.get("values") {
            for (name, value) in values.as_object().ok_or("Key values must be an object")? {
                if !key.bindings.contains_key(name) { return Err(format!("{name} is not controlled by this key schedule.")); }
                r.values.insert(name.clone(), value.clone());
            }
        }
        self.write_key_definition(schedule, def)
    }

    pub fn delete_key_rows(&mut self, schedule: Id, rows: &[u64]) -> Result<(), String> {
        let mut def = self.schedule(schedule).cloned().ok_or("not a schedule")?;
        let key = def.key.as_mut().ok_or("not a key schedule")?;
        key.rows.retain(|r| !rows.contains(&r.id));
        self.write_key_definition(schedule, def)
    }
}

impl crate::doc::Tx<'_> {
    /// A copied element keeps its independent project values and stable key assignments.
    pub(crate) fn copy_parameter_data(&mut self, source: Id, target: Id) -> Result<(), String> {
        let params: Vec<_> = self.doc_ref().project_parameters().into_iter().filter_map(|(id,p)| {
            let value = p.values.get(&source)?.clone(); let mut p = p.clone(); p.values.insert(target,value); Some((id,p))
        }).collect();
        for (id,p) in params { self.update(id,Element::ProjectParameter(p))?; }
        let keys: Vec<_> = self.doc_ref().schedules().into_iter().filter_map(|(id,v)| {
            let mut v=v.clone(); let k=v.schedule.as_mut()?.key.as_mut()?;
            let row=*k.assignments.get(&source)?; k.assignments.insert(target,row); Some((id,v))
        }).collect();
        for (id,v) in keys { self.update(id,Element::View(v))?; }
        Ok(())
    }
}
