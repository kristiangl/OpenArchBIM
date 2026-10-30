//! Schedules worked out from the live model: the elements of the schedule's category, each read the way the
//! Properties palette reads it (its own parameters and its type's), filtered, sorted and grouped, compressed when
//! not itemised, with formulas, percentages, combined fields and totals. A cell that the palette could edit carries
//! the same row and target, so an edit in the schedule changes the element (or its type) in the model.
//! Sheet and view lists read sheets and views the same way; a material takeoff gives each element a row for every
//! material in its type's layers.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use openarch_model::element::{Element, Id};
use openarch_model::schedule::*;
use serde_json::{json, Value};

use crate::{Res, Session};

/// A value as a schedule cell holds it: its text, its number in display units (mm, m², m³), what it measures,
/// and how it is edited (the palette's row and the targets that take the patch); whether the element has the
/// parameter at all, and the global parameter that sets it.
#[derive(Clone, Debug)]
struct Cell {
    text: String,
    num: Option<f64>,
    unit: Unit,
    edit: Option<(Value, Vec<Value>)>,
    present: bool,
    global: Option<String>,
}

impl Cell {
    fn text(s: impl Into<String>) -> Cell {
        Cell { text: s.into(), num: None, unit: Unit::Text, edit: None, present: true, global: None }
    }
    fn blank() -> Cell {
        Cell::text("")
    }
    /// A parameter the element doesn't have.
    fn missing() -> Cell {
        Cell { present: false, ..Cell::blank() }
    }
    fn num(n: f64, unit: Unit) -> Cell {
        Cell { text: show(n, unit), num: Some(n), unit, edit: None, present: true, global: None }
    }

    /// Whether the cell passes a filter or a highlight's test.
    fn passes(&self, op: FilterOp, against: &str) -> bool {
        op.test_cell(&self.text, self.num, self.present, self.global.as_deref(), against)
    }
}

/// A number written in its unit.
fn show(n: f64, unit: Unit) -> String {
    let trim = |s: String| if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    match unit {
        Unit::Length => trim(format!("{:.1}", n)),
        Unit::Area => format!("{n:.2} m²"),
        Unit::Volume => format!("{n:.2} m³"),
        Unit::Angle => format!("{}°", trim(format!("{n:.2}"))),
        Unit::Integer => format!("{}", n.round()),
        Unit::Currency => format!("{n:.2}"),
        Unit::YesNo => if n != 0.0 { "Yes".into() } else { "No".into() },
        Unit::Number | Unit::Text => trim(format!("{n:.3}")),
    }
}

/// Labels that name a length when the palette shows a bare number.
fn lengthy(label: &str) -> bool {
    ["Length", "Width", "Height", "Thickness", "Offset", "Depth", "Elevation", "Distance", "Projection", "Gap", "Clearance", "Sill", "Head", "Radius", "Diameter", "Extension"].iter().any(|w| label.contains(w))
}

/// One row of the Properties palette as a schedule value.
fn cell_of(row: &Value, target: &Value) -> Option<Cell> {
    let kind = row["kind"].as_str().unwrap_or("");
    if matches!(kind, "dialog" | "color" | "fill_pattern" | "line_pattern") {
        return None;
    }
    let text = row["value"].as_str().map(str::to_string).unwrap_or_else(|| row["value"].to_string());
    let editable = matches!(kind, "number" | "text" | "bool" | "select") && !row["locked"].as_bool().unwrap_or(false);
    let edit = editable.then(|| (row.clone(), vec![target.clone()]));
    let assoc = row["associate"]["kind"].as_str().unwrap_or("");
    let raw = row["raw"].as_f64();
    let path = row["path"].as_str().unwrap_or("");
    let label = row["label"].as_str().unwrap_or("");
    let mut c = match (kind, assoc) {
        ("bool", _) => Cell { num: Some(if row["raw"].as_bool().unwrap_or(false) { 1.0 } else { 0.0 }), unit: Unit::YesNo, ..Cell::text(text.clone()) },
        (_, "length") => Cell { num: raw.or_else(|| text.parse().ok()), unit: Unit::Length, ..Cell::text(text.clone()) },
        (_, "area") => Cell { num: raw, unit: Unit::Area, ..Cell::text(text.clone()) },
        (_, "volume") => Cell { num: raw, unit: Unit::Volume, ..Cell::text(text.clone()) },
        (_, "angle") => Cell { num: raw, unit: Unit::Angle, ..Cell::text(text.clone()) },
        (_, "integer") => Cell { num: raw, unit: Unit::Integer, ..Cell::text(text.clone()) },
        (_, "number") => Cell { num: raw, unit: Unit::Number, ..Cell::text(text.clone()) },
        (_, "text" | "material" | "url") => Cell::text(text.clone()),
        _ => {
            let t = text.trim();
            let (n, unit) = if let Some(x) = t.strip_suffix("m²") {
                (x.trim().parse().ok(), Unit::Area)
            } else if let Some(x) = t.strip_suffix("m³") {
                (x.trim().parse().ok(), Unit::Volume)
            } else if let Some(x) = t.strip_suffix('°') {
                (x.trim().parse().ok(), Unit::Angle)
            } else {
                let n = raw.or_else(|| t.parse().ok());
                (n, if n.is_some() && (path.ends_with("_mm") || lengthy(label)) { Unit::Length } else if n.is_some() { Unit::Number } else { Unit::Text })
            };
            match n {
                Some(n) if unit != Unit::Text => Cell { num: Some(n), unit, ..Cell::text(text.clone()) },
                _ => Cell::text(text.clone()),
            }
        }
    };
    if c.unit != Unit::Text && c.unit != Unit::YesNo {
        if let Some(n) = c.num {
            c.text = show(n, c.unit);
        }
    }
    c.edit = edit;
    c.global = row["global"]["name"].as_str().map(str::to_string);
    Some(c)
}

/// The model categories a schedule can list.
pub(crate) fn model_categories() -> Vec<String> {
    let mut cats: Vec<String> = openarch_model::view::MODEL_CATEGORIES.iter().map(|c| c.to_string()).collect();
    cats.retain(|c| !openarch_model::view::SITE_POINT_CATEGORIES.contains(&c.as_str()));
    cats
}

/// An element's parameters and its type's, by name.
struct Params {
    instance: BTreeMap<String, Cell>,
    of_type: BTreeMap<String, Cell>,
}

impl Session {
    /// The category an element is scheduled under.
    pub(crate) fn schedule_category(&self, id: Id) -> Option<String> {
        let d = &self.doc;
        match d.get(id)? {
            Element::Wall(_) => Some("Walls".into()),
            Element::Floor(_) => Some("Floors".into()),
            Element::Roof(_) => Some("Roofs".into()),
            Element::FamilyInstance(i) => d.instance_family(i).map(|(f, _)| f.category.label().to_string()),
            _ => None,
        }
    }

    /// The types of a category: family types, or wall, floor and roof types.
    fn category_types(&self, category: &str) -> Vec<Id> {
        let d = &self.doc;
        match category {
            "Walls" => d.wall_types().into_iter().map(|(i, _)| i).collect(),
            "Floors" => d.floor_types().into_iter().map(|(i, _)| i).collect(),
            "Roofs" => d.roof_types().into_iter().map(|(i, _)| i).collect(),
            c => d.family_types().into_iter().filter(|(_, t)| d.family(t.family).is_some_and(|f| f.category.label() == c)).map(|(i, _)| i).collect(),
        }
    }

    fn type_params(&self, type_id: Id) -> BTreeMap<String, Cell> {
        let mut out = BTreeMap::new();
        let Ok(t) = self.describe_any_type(type_id) else { return out };
        let target = if t["target"].is_object() { t["target"].clone() } else { json!({ "method": t["patch"], "id": type_id }) };
        for row in t["groups"].as_array().into_iter().flatten().flat_map(|g| g["rows"].as_array().into_iter().flatten()) {
            if let (Some(label), Some(c)) = (row["label"].as_str(), cell_of(row, &target)) {
                out.entry(label.to_string()).or_insert(c);
            }
        }
        let family = t["family"].as_str().unwrap_or("").to_string();
        let name = t["name"].as_str().map(str::to_string).or_else(|| t["title"].as_str().map(str::to_string)).unwrap_or_default();
        out.insert("Family".into(), Cell::text(family.clone()));
        out.insert("Type".into(), Cell::text(name.clone()));
        out.insert("Family and Type".into(), Cell::text(format!("{family}: {name}")));
        out
    }

    fn params(&self, id: Id, types: &mut BTreeMap<Id, BTreeMap<String, Cell>>) -> Params {
        let mut instance = BTreeMap::new();
        let mut of_type = BTreeMap::new();
        if let Some(c) = self.schedule_category(id) {
            instance.insert("Category".to_string(), Cell::text(c));
        }
        if let Ok(v) = self.describe(id) {
            if let (Some(Element::View(_)), Some(t)) = (self.doc.get(id), v["type_label"].as_str()) {
                instance.insert("View Type".to_string(), Cell::text(t));
            }
            let target = v["target"].clone();
            for row in v["groups"].as_array().into_iter().flatten().flat_map(|g| g["rows"].as_array().into_iter().flatten()) {
                if let (Some(label), Some(c)) = (row["label"].as_str(), cell_of(row, &target)) {
                    instance.entry(label.to_string()).or_insert(c);
                }
            }
            if let Some(t) = v["type_id"].as_u64() {
                of_type = types.entry(t).or_insert_with(|| self.type_params(t)).clone();
            }
        }
        Params { instance, of_type }
    }

    /// The elements a schedule of a category lists, in the model's order: every model element for Multi-Category,
    /// the sheets that appear in sheet lists, the views (but schedules).
    fn category_elements(&self, category: &str) -> Vec<Id> {
        let d = &self.doc;
        match category {
            MULTI_CATEGORY => d.iter().map(|(id, _)| id).filter(|id| self.schedule_category(*id).is_some()).collect(),
            SHEETS => d.sheets().into_iter().filter(|(_, s)| s.appears_in_list).map(|(id, _)| id).collect(),
            VIEWS => d.views().into_iter().filter(|(_, v)| v.view_kind != openarch_model::view::ViewKind::Schedule).map(|(id, _)| id).collect(),
            c => d.iter().map(|(id, _)| id).filter(|id| self.schedule_category(*id).as_deref() == Some(c)).collect(),
        }
    }

    /// An element's materials, from its type's layers (not the voids): each material's thickness, and its area and
    /// volume (the element's area for each layer of it, and that times the layer's thickness).
    fn materials_of(&self, id: Id, area_m2: f64) -> Vec<(String, f64, f64, f64)> {
        let d = &self.doc;
        let layers = match d.get(id) {
            Some(Element::Wall(w)) => d.wall_type(w.wall_type).map(|t| t.layers.clone()),
            Some(Element::Floor(f)) => d.floor_type(f.floor_type).map(|t| t.layers.clone()),
            Some(Element::Roof(r)) => d.roof_type(r.roof_type).map(|t| t.layers.clone()),
            _ => None,
        };
        let mut out: Vec<(String, f64, f64, f64)> = vec![];
        for l in layers.unwrap_or_default().iter().filter(|l| !l.is_void && !l.material.trim().is_empty()) {
            let v = area_m2 * l.thickness_mm / 1000.0;
            match out.iter_mut().find(|m| m.0 == l.material) {
                Some(m) => {
                    m.1 += l.thickness_mm;
                    m.2 += area_m2;
                    m.3 += v;
                }
                None => out.push((l.material.clone(), l.thickness_mm, area_m2, v)),
            }
        }
        out
    }

    /// A material field's value.
    fn material_cell(&self, field: &str, m: &(String, f64, f64, f64)) -> Cell {
        let mat = self.doc.material(&m.0);
        let id = |f: fn(&openarch_model::styles::MaterialIdentity) -> &String| Cell::text(mat.map_or(String::new(), |x| f(&x.identity).clone()));
        match field {
            "Material: Name" => Cell::text(m.0.clone()),
            "Material: Thickness" => Cell::num(m.1, Unit::Length),
            "Material: Area" => Cell::num(m.2, Unit::Area),
            "Material: Volume" => Cell::num(m.3, Unit::Volume),
            "Material: Class" => Cell::text(mat.map_or(String::new(), |x| x.class.clone())),
            "Material: Description" => id(|i| &i.description),
            "Material: Comments" => id(|i| &i.comments),
            "Material: Manufacturer" => id(|i| &i.manufacturer),
            "Material: Model" => id(|i| &i.model),
            "Material: Mark" => id(|i| &i.mark),
            "Material: Keynote" => id(|i| &i.keynote),
            "Material: Cost" => mat.map_or_else(Cell::blank, |x| Cell::num(x.identity.cost, Unit::Currency)),
            _ => Cell::missing(),
        }
    }

    /// The fields a category offers: its elements' parameters and its types', with what they measure. Multi-Category
    /// offers the parameters two or more categories share, and the category; a material takeoff adds the material's.
    pub(crate) fn schedule_fields(&self, category: &str, takeoff: bool) -> Value {
        let mut out = if category == MULTI_CATEGORY { self.shared_fields() } else { self.own_fields(category) };
        if takeoff {
            out.extend(MATERIAL_FIELDS.iter().map(|(n, u)| json!({ "name": n, "kind": "material", "unit": u })));
        }
        json!(out)
    }

    fn shared_fields(&self) -> Vec<Value> {
        let mut seen: BTreeMap<String, (Unit, BTreeSet<String>)> = BTreeMap::new();
        let mut types = BTreeMap::new();
        for id in self.category_elements(MULTI_CATEGORY) {
            let Some(cat) = self.schedule_category(id) else { continue };
            let p = self.params(id, &mut types);
            for (k, c) in p.instance.into_iter().chain(p.of_type) {
                seen.entry(k).or_insert((c.unit, BTreeSet::new())).1.insert(cat.clone());
            }
        }
        for cat in model_categories() {
            for t in self.category_types(&cat) {
                for (k, c) in self.type_params(t) {
                    seen.entry(k).or_insert((c.unit, BTreeSet::new())).1.insert(cat.clone());
                }
            }
        }
        let mut out = vec![json!({ "name": "Category", "kind": "instance", "unit": Unit::Text })];
        out.extend(seen.iter().filter(|(k, (_, cats))| cats.len() >= 2 && k.as_str() != "Category").map(|(k, (u, _))| json!({ "name": k, "kind": "instance", "unit": u })));
        out
    }

    fn own_fields(&self, category: &str) -> Vec<Value> {
        let mut types = BTreeMap::new();
        let mut inst: BTreeMap<String, Unit> = BTreeMap::new();
        for id in self.category_elements(category) {
            for (k, c) in self.params(id, &mut types).instance {
                inst.entry(k).or_insert(c.unit);
            }
        }
        let mut typ: BTreeMap<String, Unit> = BTreeMap::new();
        for t in self.category_types(category) {
            for (k, c) in self.type_params(t) {
                typ.entry(k).or_insert(c.unit);
            }
        }
        let mut out: Vec<Value> = inst.iter().filter(|(k, _)| !typ.contains_key(*k)).map(|(k, u)| json!({ "name": k, "kind": "instance", "unit": u })).collect();
        out.extend(typ.iter().map(|(k, u)| json!({ "name": k, "kind": "type", "unit": u })));
        out.retain(|f| f["name"] != "Category");
        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        out
    }

    /// What a parameter field of a schedule measures, from the category's elements and types.
    pub(crate) fn schedule_param_units(&self, category: &str) -> BTreeMap<String, Unit> {
        let f = self.schedule_fields(category, false);
        f.as_array().into_iter().flatten().filter_map(|x| Some((x["name"].as_str()?.to_string(), serde_json::from_value(x["unit"].clone()).ok()?))).collect()
    }

    /// What the views on a sheet show, for Filter by Sheet: a plan's elements inside its crop, an elevation's or
    /// section's items.
    pub(crate) fn sheet_visible_ids(&self, sheet: Id) -> HashSet<Id> {
        let mut out = HashSet::new();
        for (_, vp) in self.doc.viewports().into_iter().filter(|(_, v)| v.sheet == sheet) {
            let Some(v) = self.doc.view(vp.view) else { continue };
            if v.view_kind.is_plan() {
                let Ok(data) = self.plan(&json!({ "view_id": vp.view })) else { continue };
                let crop = if v.crop.enabled { v.crop.region } else { None };
                for (key, pts) in [("walls", "corners"), ("inserts", "footprint"), ("floors", "points"), ("roofs", "points"), ("curtains", "line")] {
                    for item in data[key].as_array().into_iter().flatten() {
                        let Some(id) = item["id"].as_u64() else { continue };
                        let xy: Vec<(f64, f64)> = item[pts].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_f64()?, p[1].as_f64()?))).collect();
                        let inside = match crop {
                            Some((lo, hi)) if !xy.is_empty() => {
                                let (x0, x1) = xy.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.0), b.max(p.0)));
                                let (y0, y1) = xy.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.1), b.max(p.1)));
                                x1 >= lo.x && x0 <= hi.x && y1 >= lo.y && y0 <= hi.y
                            }
                            _ => true,
                        };
                        if inside {
                            out.insert(id);
                        }
                    }
                }
            } else if let Ok(e) = self.doc.elevation_data(vp.view) {
                out.extend(e.items.iter().map(|it| it.id));
            }
        }
        out
    }

    /// The schedule as a table: { id, name, category, title, columns, rows: [{ kind, ids, cells }], count }.
    /// Row kinds: "header", "item", "footer", "blank", "grand".
    pub(crate) fn schedule_table(&self, view: Id) -> Res {
        self.schedule_table_for(view, None)
    }

    /// The table of only some elements (what a sheet's views show).
    pub(crate) fn schedule_table_for(&self, view: Id, only: Option<&HashSet<Id>>) -> Res {
        let v = self.doc.view(view).ok_or("not a view")?;
        let def = v.schedule.as_ref().ok_or("not a schedule")?;
        // sheets and views aren't shown by a sheet's views
        let only = only.filter(|_| !matches!(def.category.as_str(), SHEETS | VIEWS));
        let units = self.schedule_param_units(&def.category);
        let unit_of = |f: &SchedField| def.unit_of(f, &|g: &SchedField| units.get(&g.name).copied().unwrap_or(Unit::Text));
        // every element: its fields' values
        let mut types = BTreeMap::new();
        let mut rows: Vec<(Vec<Id>, Vec<Cell>)> = vec![];
        for id in self.category_elements(&def.category).into_iter().filter(|id| only.is_none_or(|o| o.contains(id))) {
            let p = self.params(id, &mut types);
            let base: Vec<Cell> = def
                .fields
                .iter()
                .map(|f| match f.kind {
                    FieldKind::Instance => p.instance.get(&f.name).or_else(|| p.of_type.get(&f.name)).cloned().unwrap_or_else(Cell::missing),
                    FieldKind::Type => p.of_type.get(&f.name).or_else(|| p.instance.get(&f.name)).cloned().unwrap_or_else(Cell::missing),
                    FieldKind::Count => Cell::num(1.0, Unit::Integer),
                    _ => Cell::blank(),
                })
                .collect();
            // a takeoff: a row for each material, with the material's fields
            let each: Vec<Vec<Cell>> = if def.takeoff {
                let area = p.instance.get("Area").and_then(|c| c.num.filter(|_| c.unit == Unit::Area)).unwrap_or(0.0);
                self.materials_of(id, area)
                    .iter()
                    .map(|m| def.fields.iter().zip(&base).map(|(f, c)| if f.kind == FieldKind::Material { self.material_cell(&f.name, m) } else { c.clone() }).collect())
                    .collect()
            } else {
                vec![base]
            };
            for mut cells in each {
                for (i, f) in def.fields.iter().enumerate() {
                    if f.kind == FieldKind::Formula {
                        cells[i] = formula(def, f, &cells, &unit_of, 0);
                    }
                }
                for (i, f) in def.fields.iter().enumerate() {
                    if f.kind == FieldKind::Combined {
                        let mut s = String::new();
                        for (k, part) in f.parts.iter().enumerate() {
                            let t = def.fields.iter().position(|g| g.name == part.field).map(|j| cells[j].text.clone()).unwrap_or_default();
                            s.push_str(&format!("{}{}{}", part.prefix, t, part.suffix));
                            if k + 1 < f.parts.len() {
                                s.push_str(&part.separator);
                            }
                        }
                        cells[i] = Cell::text(s);
                    }
                }
                rows.push((vec![id], cells));
            }
        }
        let col = |name: &str| def.fields.iter().position(|f| f.name == name);
        // filters
        rows.retain(|(_, cells)| def.filters.iter().all(|flt| col(&flt.field).is_some_and(|i| cells[i].passes(flt.op, &flt.value))));
        // percentages: each row's share of the column's total
        for (i, f) in def.fields.iter().enumerate() {
            if f.kind != FieldKind::Percentage {
                continue;
            }
            let Some(j) = col(&f.of) else { continue };
            let total: f64 = rows.iter().filter_map(|(_, c)| c[j].num).sum();
            for (_, cells) in rows.iter_mut() {
                cells[i] = match cells[j].num {
                    Some(n) if total.abs() > 1e-12 => Cell { num: Some(n / total * 100.0), unit: Unit::Number, ..Cell::text(format!("{:.1}%", n / total * 100.0)) },
                    _ => Cell::blank(),
                };
            }
        }
        // sort: numbers as numbers, text in natural order
        let keys: Vec<(usize, &SortKey)> = def.sort.iter().filter_map(|k| col(&k.field).map(|i| (i, k))).collect();
        let cmp = |a: &Cell, b: &Cell| match (a.num, b.num) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => natural(&a.text).cmp(&natural(&b.text)),
        };
        rows.sort_by(|(_, a), (_, b)| {
            for (i, k) in &keys {
                let o = cmp(&a[*i], &b[*i]);
                let o = if k.descending { o.reverse() } else { o };
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            std::cmp::Ordering::Equal
        });
        let same_keys = |a: &[Cell], b: &[Cell], upto: usize| keys.iter().take(upto).all(|(i, _)| a[*i].text == b[*i].text);
        // not itemised: elements that agree on every sort key share a row
        if !def.itemize {
            let mut merged: Vec<(Vec<Id>, Vec<Cell>)> = vec![];
            for (ids, cells) in rows {
                match merged.last_mut() {
                    Some((mids, mcells)) if same_keys(mcells, &cells, keys.len()) => {
                        mids.extend(ids);
                        for (i, f) in def.fields.iter().enumerate() {
                            let (a, b) = (&mcells[i], &cells[i]);
                            mcells[i] = if f.kind == FieldKind::Count {
                                Cell::num(a.num.unwrap_or(0.0) + 1.0, Unit::Integer)
                            } else if f.totals != Totals::None && a.num.is_some() && b.num.is_some() {
                                Cell::num(a.num.unwrap() + b.num.unwrap(), a.unit)
                            } else if a.text == b.text {
                                // the same everywhere: an edit goes to every element (or type) in the row
                                let edit = match (&a.edit, &b.edit) {
                                    (Some((row, t1)), Some((_, t2))) => Some((row.clone(), t1.iter().chain(t2).fold(vec![], |mut v: Vec<Value>, t| {
                                        if !v.contains(t) {
                                            v.push(t.clone());
                                        }
                                        v
                                    }))),
                                    _ => None,
                                };
                                Cell { edit, ..a.clone() }
                            } else {
                                Cell::blank()
                            };
                        }
                    }
                    _ => merged.push((ids, cells)),
                }
            }
            rows = merged;
        }
        // the table: headers where a key changes, footers and blank lines where a group ends
        let shown: Vec<usize> = (0..def.fields.len()).filter(|i| !def.fields[*i].hidden).collect();
        let count_of = |rs: &[(Vec<Id>, Vec<Cell>)]| rs.iter().map(|(ids, _)| ids.len()).sum::<usize>();
        let totals_row = |kind: &str, title: String, rs: &[(Vec<Id>, Vec<Cell>)]| -> Value {
            let mut cells: Vec<Value> = shown
                .iter()
                .map(|&i| {
                    let f = &def.fields[i];
                    let nums: Vec<f64> = rs.iter().filter_map(|(_, c)| c[i].num).collect();
                    let u = rs.first().map_or(Unit::Number, |(_, c)| c[i].unit);
                    let text = if f.kind == FieldKind::Count {
                        show(count_of(rs) as f64, Unit::Integer)
                    } else if nums.is_empty() {
                        String::new()
                    } else {
                        let (lo, hi) = nums.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| (l.min(*x), h.max(*x)));
                        match f.totals {
                            Totals::None => String::new(),
                            Totals::Totals => show(nums.iter().sum(), u),
                            Totals::Min => show(lo, u),
                            Totals::Max => show(hi, u),
                            Totals::MinMax => format!("{} - {}", show(lo, u), show(hi, u)),
                        }
                    };
                    json!({ "text": text })
                })
                .collect();
            if let Some(c) = cells.first_mut() {
                let t = c["text"].as_str().unwrap_or("").to_string();
                c["text"] = json!(if t.is_empty() { title } else { format!("{title}  {t}") });
            }
            json!({ "kind": kind, "ids": rs.iter().flat_map(|(ids, _)| ids.clone()).collect::<Vec<_>>(), "cells": cells })
        };
        // conditional formatting: the first rule the row passes colours the cell
        let bg = |i: usize, cells: &Vec<Cell>| -> Option<String> {
            def.fields[i].highlights.iter().find(|h| (if h.field.is_empty() { Some(i) } else { col(&h.field) }).is_some_and(|j| cells[j].passes(h.op, &h.value))).map(|h| format!("#{:02x}{:02x}{:02x}", h.colour[0], h.colour[1], h.colour[2]))
        };
        let item = |ids: &Vec<Id>, cells: &Vec<Cell>| -> Value {
            json!({ "kind": "item", "ids": ids, "cells": shown.iter().map(|&i| {
                let c = &cells[i];
                let mut v = match &c.edit {
                    Some((row, targets)) => json!({ "text": c.text, "row": row, "targets": targets }),
                    None => json!({ "text": c.text }),
                };
                if let Some(b) = bg(i, cells) {
                    v["bg"] = json!(b);
                }
                v
            }).collect::<Vec<_>>() })
        };
        let mut out: Vec<Value> = vec![];
        let mut start: Vec<usize> = vec![0; keys.len()];
        for (n, (ids, cells)) in rows.iter().enumerate() {
            // the first key that changes here: groups from it down end before this row and begin at it
            let changed = if n == 0 { Some(0) } else { (0..keys.len()).find(|&k| !same_keys(&rows[n - 1].1, cells, k + 1)) };
            if let Some(level) = changed {
                if n > 0 {
                    for k in (level..keys.len()).rev() {
                        let key = keys[k].1;
                        if key.footer {
                            out.push(totals_row("footer", format!("{}:", rows[n - 1].1[keys[k].0].text), &rows[start[k]..n]));
                        }
                        if key.blank_line {
                            out.push(json!({ "kind": "blank", "ids": [], "cells": [] }));
                        }
                    }
                }
                for k in level..keys.len() {
                    start[k] = n;
                    if keys[k].1.header {
                        out.push(json!({ "kind": "header", "level": k, "ids": [], "cells": [{ "text": cells[keys[k].0].text }] }));
                    }
                }
            }
            out.push(item(ids, cells));
        }
        if !rows.is_empty() {
            for k in (0..keys.len()).rev() {
                let key = keys[k].1;
                if key.footer {
                    out.push(totals_row("footer", format!("{}:", rows[rows.len() - 1].1[keys[k].0].text), &rows[start[k]..]));
                }
                if key.blank_line {
                    out.push(json!({ "kind": "blank", "ids": [], "cells": [] }));
                }
            }
        }
        if def.grand_totals {
            out.push(totals_row("grand", format!("{}:", def.grand_title), &rows));
        }
        let columns: Vec<Value> = shown.iter().map(|&i| { let f = &def.fields[i]; json!({ "name": f.name, "heading": f.heading(), "align": f.align, "unit": unit_of(f), "highlight_on_sheets": f.highlight_on_sheets }) }).collect();
        Ok(json!({
            "id": view, "name": v.name, "category": def.category, "title": v.name, "show_title": def.show_title, "show_headers": def.show_headers,
            "columns": columns, "rows": out, "count": count_of(&rows), "def": def,
        }))
    }
}

/// A formula field's value in a row: the other fields in base units (lengths in metres, so that a length times a
/// length is an area in m²), the answer in the field's unit.
fn formula(def: &ScheduleDef, f: &SchedField, cells: &[Cell], unit_of: &dyn Fn(&SchedField) -> Unit, depth: u8) -> Cell {
    use openarch_params::expr::{eval, parse};
    use openarch_params::value::Value as V;
    let Ok(e) = parse(&f.formula) else { return Cell::text("?") };
    let base = |c: &Cell| -> V {
        match (c.num, c.unit) {
            (Some(n), Unit::Length) => V::Num(n / 1000.0),
            (Some(n), Unit::YesNo) => V::Bool(n != 0.0),
            (Some(n), _) => V::Num(n),
            (None, _) => V::Text(c.text.clone()),
        }
    };
    let mut resolve = |name: &str| -> Result<V, openarch_params::value::EvalError> {
        let i = def.fields.iter().position(|g| g.name == name).ok_or_else(|| openarch_params::value::EvalError::UnknownName(name.into()))?;
        let g = &def.fields[i];
        if g.kind == FieldKind::Formula && depth < 8 {
            return Ok(base(&formula(def, g, cells, unit_of, depth + 1)));
        }
        Ok(base(&cells[i]))
    };
    match eval(&e, &mut resolve) {
        Ok(V::Num(n)) => {
            let u = unit_of(f);
            Cell::num(if u == Unit::Length { n * 1000.0 } else { n }, if u == Unit::Text { Unit::Number } else { u })
        }
        Ok(V::Text(s)) => Cell::text(s),
        Ok(V::Bool(b)) => Cell::num(if b { 1.0 } else { 0.0 }, Unit::YesNo),
        Err(_) => Cell::text(""),
    }
}

/// A sort key that puts D2 before D10.
fn natural(s: &str) -> Vec<(String, u64)> {
    let mut out = vec![];
    let (mut text, mut num) = (String::new(), String::new());
    for c in s.to_lowercase().chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                num.clear();
            }
            text.push(c);
        }
    }
    out.push((text, num.parse().unwrap_or(0)));
    out
}
