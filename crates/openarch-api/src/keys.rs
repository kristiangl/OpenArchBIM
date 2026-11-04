//! Key-schedule commands and the instance parameter editors shared with Properties.
use std::collections::{BTreeMap, BTreeSet};
use serde_json::{json, Value};
use openarch_model::Id;
use openarch_model::key_schedule::{KeyBinding, KeySchedule, ProjectParameter};
use openarch_model::schedule::{ScheduleDef, SchedField, FieldKind, Unit};
use crate::{Session, Res};

pub(crate) fn unit(kind: openarch_model::family::ParamKind) -> Unit {
    use openarch_model::family::ParamKind as K;
    match kind { K::Length => Unit::Length, K::Number => Unit::Number, K::Integer => Unit::Integer, K::Angle => Unit::Angle, K::Area => Unit::Area, K::Volume => Unit::Volume, K::YesNo => Unit::YesNo, _ => Unit::Text }
}

pub(crate) fn editor(name: &str, path: &[String], value: Value, u: Unit) -> Value {
    let shown = if value.is_null() { String::new() } else if let Some(s) = value.as_str() { s.into() } else if let Some(b) = value.as_bool() { if b { "Yes".into() } else { "No".into() } } else { value.to_string() };
    json!({ "label": name, "path": path.join("."), "patch_path": path, "parameter_unit": u, "value": shown, "raw": value,
        "kind": if u == Unit::Text { "text" } else if u == Unit::YesNo { "bool" } else { "number" } })
}

impl Session {
    pub(crate) fn key_fields(&self, category: &str, current: Option<Id>) -> Vec<Value> {
        let mut out: BTreeMap<String, Value> = BTreeMap::new();
        if category=="Rooms" {
            for (name,path) in [("Comments","comments"),("Occupancy","occupancy"),("Department","department"),("Occupant","occupant"),("Base Finish","base_finish"),("Floor Finish","floor_finish"),("Wall Finish","wall_finish"),("Ceiling Finish","ceiling_finish")] {
                let b=KeyBinding{path:vec![path.into()],..Default::default()};out.insert(name.into(),json!({"name":name,"kind":"instance","unit":"text","binding":b,"editor":editor(name,&[path.into()],json!(""),Unit::Text)}));
            }
        }
        // Instance definitions remain available before anything is placed.
        for (_, fam) in self.doc.families().into_iter().filter(|(_, f)| f.category.label() == category) {
            for p in fam.params.iter().filter(|p| p.instance && !p.reporting && p.formula.is_none()) {
                let binding = KeyBinding { path: vec!["values".into(), p.name.clone()], shared_guid: p.shared_guid.clone(), ..Default::default() };
                let u = unit(p.param_kind);
                let mut row = editor(&p.name, &binding.path, serde_json::to_value(&p.default).unwrap_or(Value::Null), u);
                if !p.choices.is_empty() {
                    row["kind"] = json!("select");
                    row["options"] = json!(p.choices.iter().map(|choice| json!({ "label": choice, "patch": { "values": { p.name.clone(): choice } }, "selected": p.default.as_text() == Some(choice) })).collect::<Vec<_>>());
                }
                out.entry(p.name.clone()).or_insert(json!({ "name": p.name, "kind": "instance", "unit": u, "binding": binding, "editor": row }));
            }
        }
        let mut seen_types = BTreeSet::new();
        for (id, element) in self.doc.iter().filter(|(id, _)| self.doc.parameter_category(*id).as_deref() == Some(category)) {
            let ty = match element {
                openarch_model::Element::FamilyInstance(i) => i.type_id,
                openarch_model::Element::Wall(w) => w.wall_type,
                openarch_model::Element::Floor(f) => f.floor_type,
                openarch_model::Element::Roof(r) => r.roof_type,
                _ => id,
            };
            if !seen_types.insert(ty) { continue; }
            let Ok(props) = self.describe_base(id) else { continue };
            for r in props["groups"].as_array().into_iter().flatten().flat_map(|g| g["rows"].as_array().into_iter().flatten()) {
                let Some(path) = r["path"].as_str() else { continue };
                if !matches!(r["kind"].as_str(), Some("text" | "number" | "bool" | "select")) { continue; }
                // Keys control parameter values, not host, placement, pinning or type selection.
                if !(path.starts_with("values.") || matches!(path, "comments" | "mark" | "ifc.export_as" | "ifc.predefined_type")) { continue; }
                let name = r["label"].as_str().unwrap_or("");
                let mut binding = out.get(name).and_then(|f| serde_json::from_value::<KeyBinding>(f["binding"].clone()).ok()).unwrap_or_default();
                if binding.path.is_empty() { binding.path = path.split('.').map(str::to_string).collect(); }
                let u = out.get(name).map(|f| f["unit"].clone()).unwrap_or_else(|| json!(if r["kind"] == "number" { Unit::Number } else if r["kind"] == "bool" { Unit::YesNo } else { Unit::Text }));
                out.insert(name.into(), json!({ "name": name, "kind": "instance", "unit": u, "binding": binding, "editor": r }));
            }
        }
        for (id, p) in self.doc.project_parameters().into_iter().filter(|(_, p)| p.categories.iter().any(|c| c == category)) {
            let binding = KeyBinding { project_parameter: Some(id), shared_guid: p.shared_guid.clone(), ..Default::default() };
            out.insert(p.name.clone(), json!({ "name": p.name, "kind": "instance", "unit": p.unit, "binding": binding, "editor": editor(&p.name, &[], Value::Null, p.unit) }));
        }
        // A parameter is owned by at most one key schedule of its category.
        for (id, view) in self.doc.schedules().into_iter().filter(|(id, _)| Some(*id) != current) {
            let def = view.schedule.as_ref().unwrap();
            if def.category == category {
                if let Some(key) = &def.key { out.retain(|name, f| !key.bindings.iter().any(|(n,b)| n == name || serde_json::from_value::<KeyBinding>(f["binding"].clone()).is_ok_and(|own| openarch_model::key_schedule::same_binding(&own,b)))); }
            }
            let _ = id;
        }
        out.into_values().collect()
    }

    pub(crate) fn prepare_key_definition(&self, id: Id, mut def: ScheduleDef) -> Result<ScheduleDef, String> {
        def = self.prepare_embedded(def)?;
        let Some(key) = &mut def.key else { return Ok(def) };
        let old = self.doc.schedule(id).and_then(|d| d.key.as_ref());
        if let Some(old) = old { key.rows = old.rows.clone(); key.assignments = old.assignments.clone(); key.next_row_id = old.next_row_id; }
        let available = self.key_fields(&def.category, Some(id));
        if available.iter().any(|a| a["name"] == key.parameter_name) { return Err("The key selector name is already a parameter of this category.".into()); }
        key.bindings.clear();
        for field in &mut def.fields {
            if field.name == "Key Name" { field.kind = FieldKind::Instance; field.unit = Unit::Text; continue; }
            let a = available.iter().find(|a| a["name"] == field.name && field.kind == FieldKind::Instance).ok_or_else(|| format!("{} is not an available editable instance parameter.", field.name))?;
            field.unit = serde_json::from_value(a["unit"].clone()).map_err(|e| e.to_string())?;
            key.bindings.insert(field.name.clone(), serde_json::from_value(a["binding"].clone()).map_err(|e| e.to_string())?);
        }
        def.itemize = true;
        def.filter_by_sheet = false;
        Ok(def)
    }

    pub(crate) fn create_key_schedule(&mut self, p: &Value) -> Res {
        let category = p["category"].as_str().ok_or("Choose a category")?;
        if !crate::schedule::model_categories().iter().any(|c| c == category) { return Err("Keys belong to one model category.".into()); }
        let name = p["name"].as_str().unwrap_or("Key Schedule");
        let selector = p["key_name"].as_str().map(str::to_string).unwrap_or_else(|| format!("{} Style", category.trim_end_matches('s')));
        let id = self.doc.grouped("New Key Schedule", |d| {
            let id = d.add_schedule(category, name, false)?;
            let mut def = d.schedule(id).unwrap().clone();
            def.key = Some(KeySchedule::new(&selector));
            def.fields = vec![serde_json::from_value(json!({ "name": "Key Name", "kind": "instance", "unit": "text" })).unwrap()];
            d.set_schedule(id, def, &|_| Unit::Text)?;
            Ok(id)
        })?;
        Ok(json!({ "id": id }))
    }

    pub(crate) fn key_cell(&self, available: &[Value], key: &openarch_model::key_schedule::KeyRow, f: &SchedField) -> Value {
        if f.name == "Key Name" { return editor("Key Name", &["name".into()], json!(key.name), Unit::Text); }
        let value = key.values.get(&f.name).cloned().unwrap_or(Value::Null);
        let mut row = editor(&f.name, &["values".into(), f.name.clone()], value.clone(), f.unit);
        if f.unit == Unit::YesNo {
            row["kind"] = json!("select");
            row["options"] = json!([(Value::Null, "(blank)"), (json!(true), "Yes"), (json!(false), "No")].into_iter().map(|(raw,label)| json!({ "label": label, "selected": raw == value, "patch": { "values": { f.name.clone(): raw } } })).collect::<Vec<_>>());
        }
        if let Some(a) = available.iter().find(|a| a["name"] == f.name && a["editor"]["kind"] == "select") {
            row["kind"] = json!("select");
            row["options"] = json!(a["editor"]["options"].as_array().into_iter().flatten().map(|o| {
                let binding: KeyBinding = serde_json::from_value(a["binding"].clone()).unwrap_or_default();
                let raw = openarch_model::key_schedule::path_value(&o["patch"], &binding.path).cloned().unwrap_or(Value::Null);
                json!({ "label": o["label"], "selected": raw == value, "patch": { "values": { f.name.clone(): raw } } })
            }).collect::<Vec<_>>());
        }
        row
    }

    pub(crate) fn add_key_properties(&self, ids: &[Id], props: &mut Value) {
        let Some(&id) = ids.first() else { return };
        let Some(category) = self.doc.parameter_category(id) else { return };
        if !ids.iter().all(|id| self.doc.parameter_category(*id).as_deref() == Some(&category)) { return; }
        for group in props["groups"].as_array_mut().into_iter().flatten() {
            for row in group["rows"].as_array_mut().into_iter().flatten() {
                let path: Vec<String> = serde_json::from_value(row["patch_path"].clone()).unwrap_or_else(|_| row["path"].as_str().unwrap_or("").split('.').map(str::to_string).collect());
                if let Some((schedule, name, field)) = ids.iter().find_map(|id| self.doc.key_control(*id, &path, None)) {
                    let def = self.doc.schedule(schedule).unwrap(); let key = def.key.as_ref().unwrap();
                    let expected = key.assignments.get(&id).and_then(|key_id| key.rows.iter().find(|r| r.id == *key_id)).and_then(|r| r.values.get(&field));
                    let raw = if row["raw"].is_null() { row["value"].clone() } else { row["raw"].clone() };
                    let aligned = expected.is_none_or(|v| v.is_null() || v == &raw || v.as_f64().zip(raw.as_f64()).is_some_and(|(a,b)| (a-b).abs() < 1e-8));
                    row["kind"] = json!("ro"); row["locked"] = json!(true);
                    row["lock_reason"] = json!(format!("Controlled by {name}"));
                    row["key_control"] = json!({ "schedule": schedule, "name": name, "aligned": aligned });
                    row.as_object_mut().unwrap().remove("associate");
                }
            }
        }
        let Some(groups) = props["groups"].as_array_mut() else { return };
        let mut selectors = vec![];
        for (schedule, v) in self.doc.schedules() {
            let def = v.schedule.as_ref().unwrap(); if def.category != category { continue; }
            let Some(key) = &def.key else { continue };
            let current = key.assignments.get(&id).copied();
            let varies = ids.iter().any(|id| key.assignments.get(id).copied() != current);
            let mut options = vec![json!({ "label": "(none)", "selected": current.is_none(), "patch": { "keys": { schedule.to_string(): Value::Null } } })];
            options.extend(key.rows.iter().map(|r| json!({ "label": r.name, "selected": current == Some(r.id), "patch": { "keys": { schedule.to_string(): r.id } } })));
            let shown = current.and_then(|id| key.rows.iter().find(|r| r.id == id)).map(|r| r.name.as_str()).unwrap_or("(none)");
            selectors.push(json!({ "label": key.parameter_name, "value": shown, "kind": "select", "options": options, "varies": varies }));
        }
        if !selectors.is_empty() { groups.push(json!({ "name": "Key Schedules", "target": { "method": "patch_element_keys", "id": id, "extra": { "ids": ids } }, "rows": selectors })); }
        let mut parameters = vec![];
        for (parameter, p) in self.doc.project_parameters().into_iter().filter(|(_, p)| p.categories.contains(&category)) {
            let value = p.values.get(&id).cloned().unwrap_or(Value::Null);
            let mut row = editor(&p.name, &["values".into(), parameter.to_string()], value.clone(), p.unit);
            row["varies"] = json!(ids.iter().any(|id| p.values.get(id).cloned().unwrap_or(Value::Null) != value));
            if let Some((schedule, name, _)) = ids.iter().find_map(|id| self.doc.key_control(*id, &[], Some(parameter))) {
                row["kind"] = json!("ro"); row["locked"] = json!(true); row["lock_reason"] = json!(format!("Controlled by {name}")); row["key_control"] = json!({ "schedule": schedule, "name": name, "aligned": true });
            }
            parameters.push(row);
        }
        if !parameters.is_empty() { groups.push(json!({ "name": "Project Parameters", "target": { "method": "patch_project_values", "id": id, "extra": { "ids": ids } }, "rows": parameters })); }
    }

    pub(crate) fn create_project_parameter(&mut self, p: &Value) -> Res {
        let category = p["category"].as_str().ok_or("Choose a category")?;
        if !crate::schedule::model_categories().iter().any(|c| c == category) { return Err("Choose a model category.".into()); }
        let name = p["name"].as_str().ok_or("Enter a name")?.trim();
        if name == "Key Name" || self.key_fields(category, None).iter().any(|f| f["name"] == name) || self.doc.schedules().iter().any(|(_,v)| v.schedule.as_ref().is_some_and(|d| d.category == category && d.key.as_ref().is_some_and(|k| k.parameter_name == name || k.bindings.contains_key(name)))) { return Err("That parameter name is already in use.".into()); }
        let unit: Unit = serde_json::from_value(p.get("unit").cloned().unwrap_or(json!("text"))).map_err(|e| e.to_string())?;
        let id = self.doc.add_project_parameter(ProjectParameter { name: name.into(), categories: vec![category.into()], unit, shared_guid: p["shared_guid"].as_str().map(str::to_string), group: "Identity Data".into(), values: BTreeMap::new() })?;
        Ok(json!({ "id": id }))
    }
}
