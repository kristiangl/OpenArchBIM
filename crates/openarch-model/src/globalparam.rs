//! Global parameters (Manage > Global Parameters).
//!
//! A global parameter is a named value kept by the project. It can label dimensions and stand in for the value of a
//! family's parameter on a type or an instance (associated in Properties), so one change reaches everything it labels
//! or is associated with.
//!
//! Driving parameters push their value out: every dimension segment labelled with one takes its value (the geometry
//! moves), and the segment then holds that length as a lock does. They can have a formula over other globals, written
//! the way family formulas are (`[Building Width] / 3`), worked out in dependency order; a cycle is refused.
//! Reporting parameters read the one dimension segment they label after every edit and cannot be typed into; they take
//! no formula and no formula may name them. Changing a parameter's mode lets go of its labels.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::*;
use crate::family::{ParamKind, ParamValue};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalParam {
    pub name: String,
    #[serde(default = "length")]
    pub kind: ParamKind,
    pub value: ParamValue,
    /// Over other global parameters; the value then comes from it.
    #[serde(default)]
    pub formula: Option<String>,
    /// Reads its value from the one dimension segment it labels.
    #[serde(default)]
    pub reporting: bool,
    /// Where the parameter is listed ("Dimensions", "Constraints" ...); for organising only.
    #[serde(default)]
    pub group: String,
}

fn length() -> ParamKind {
    ParamKind::Length
}

/// A dimension segment a parameter labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Labelled {
    pub dimension: Id,
    pub segment: usize,
}

/// Which kinds label which: lengths label aligned, linear, radial and diameter dimensions; angles label angular ones.
pub fn labels_lengths(kind: ParamKind) -> bool {
    kind == ParamKind::Length
}

/// Whether a parameter of `kind` can label a dimension of `dk`.
pub fn labels_kind(kind: ParamKind, dk: crate::dimension::DimKind) -> bool {
    use crate::dimension::DimKind as K;
    match dk {
        K::Aligned | K::Linear | K::Radial | K::Diameter => kind == ParamKind::Length,
        K::Angular => kind == ParamKind::Angle,
        K::ArcLength => false,
    }
}

fn to_eval(v: &ParamValue) -> openarch_params::Value {
    match v {
        ParamValue::Bool(b) => openarch_params::Value::Bool(*b),
        ParamValue::Number(n) => openarch_params::Value::Num(*n),
        ParamValue::Text(t) => openarch_params::Value::Text(t.clone()),
    }
}

fn from_eval(kind: ParamKind, v: openarch_params::Value) -> Result<ParamValue, String> {
    Ok(match kind {
        ParamKind::YesNo => ParamValue::Bool(v.as_bool().map_err(|e| e.to_string())?),
        ParamKind::Text | ParamKind::Material => ParamValue::Text(v.to_string()),
        ParamKind::Integer => ParamValue::Number(v.as_num().map_err(|e| e.to_string())?.round()),
        _ => ParamValue::Number(v.as_num().map_err(|e| e.to_string())?),
    })
}

/// The names a formula reads.
fn names_of(src: &str) -> Result<Vec<String>, String> {
    let e = openarch_params::expr::parse(src.trim()).map_err(|e| e.to_string())?;
    let mut out = vec![];
    openarch_params::expr::names_in(&e, &mut out);
    Ok(out)
}

impl Document {
    pub fn global_params(&self) -> Vec<(Id, &GlobalParam)> {
        self.iter().filter_map(|(id, e)| if let Element::GlobalParam(g) = e { Some((id, g)) } else { None }).collect()
    }

    pub fn global_param(&self, id: Id) -> Option<&GlobalParam> {
        match self.get(id) {
            Some(Element::GlobalParam(g)) => Some(g),
            _ => None,
        }
    }

    pub fn global_by_name(&self, name: &str) -> Option<(Id, &GlobalParam)> {
        self.global_params().into_iter().find(|(_, g)| g.name.eq_ignore_ascii_case(name.trim()))
    }

    /// The dimension segments labelled with a parameter.
    pub fn labelled_by(&self, gid: Id) -> Vec<Labelled> {
        let mut out = vec![];
        for (id, d) in self.dimensions() {
            for (k, s) in d.segments.iter().enumerate() {
                if s.param == Some(gid) {
                    out.push(Labelled { dimension: id, segment: k });
                }
            }
        }
        out
    }

    /// Family types and instances whose parameter takes this global's value: (element, parameter name).
    pub fn associated_with(&self, gid: Id) -> Vec<(Id, String)> {
        let mut out = vec![];
        for (id, e) in self.iter() {
            let map = match e {
                Element::FamilyType(t) => &t.globals,
                Element::FamilyInstance(i) => &i.globals,
                _ => continue,
            };
            for (name, g) in map {
                if *g == gid {
                    out.push((id, name.clone()));
                }
            }
        }
        out
    }

    /// Why a global's formula cannot be worked out now (a name it reads is gone, a cycle), if it cannot.
    pub fn global_formula_error(&self, gid: Id) -> Option<String> {
        let g = self.global_param(gid)?;
        let f = g.formula.as_ref()?;
        self.evaluate_globals().err().filter(|(id, _)| *id == gid).map(|(_, e)| e).or_else(|| {
            names_of(f).err().or_else(|| names_of(f).ok()?.into_iter().find(|n| self.global_by_name(n).is_none()).map(|n| format!("there is no global parameter called {n}")))
        })
    }

    /// Every formula's value, worked out in dependency order. A cycle or a broken formula is reported against the
    /// parameter it starts from.
    fn evaluate_globals(&self) -> Result<BTreeMap<Id, ParamValue>, (Id, String)> {
        let all = self.global_params();
        let by_name: BTreeMap<String, Id> = all.iter().map(|(id, g)| (g.name.to_ascii_lowercase(), *id)).collect();
        let mut done: BTreeMap<Id, ParamValue> = all.iter().filter(|(_, g)| g.formula.is_none()).map(|(id, g)| (*id, g.value.clone())).collect();
        fn visit(doc: &Document, id: Id, by_name: &BTreeMap<String, Id>, done: &mut BTreeMap<Id, ParamValue>, path: &mut Vec<Id>) -> Result<ParamValue, (Id, String)> {
            if let Some(v) = done.get(&id) {
                return Ok(v.clone());
            }
            let g = doc.global_param(id).ok_or((id, "gone".to_string()))?;
            if path.contains(&id) {
                let names: Vec<String> = path.iter().chain(std::iter::once(&id)).filter_map(|i| doc.global_param(*i).map(|g| g.name.clone())).collect();
                return Err((path[0], format!("the formulas go round in a circle: {}", names.join(" -> "))));
            }
            path.push(id);
            let src = g.formula.clone().unwrap_or_default();
            let expr = openarch_params::expr::parse(src.trim()).map_err(|e| (id, format!("{}: {e}", g.name)))?;
            let mut err: Option<(Id, String)> = None;
            let mut resolve = |n: &str| -> Result<openarch_params::Value, openarch_params::EvalError> {
                let Some(other) = by_name.get(&n.to_ascii_lowercase()) else { return Err(openarch_params::EvalError::UnknownName(n.to_string())) };
                if doc.global_param(*other).is_some_and(|o| o.reporting) {
                    err = Some((id, format!("{} reads a reporting parameter ({n}), which formulas cannot use", g.name)));
                    return Err(openarch_params::EvalError::UnknownName(n.to_string()));
                }
                match visit(doc, *other, by_name, done, path) {
                    Ok(v) => Ok(to_eval(&v)),
                    Err(e) => {
                        err.get_or_insert(e);
                        Err(openarch_params::EvalError::UnknownName(n.to_string()))
                    }
                }
            };
            let got = openarch_params::expr::eval(&expr, &mut resolve);
            if let Some(e) = err {
                return Err(e);
            }
            let v = got.map_err(|e| (id, format!("{}: {e}", g.name))).and_then(|v| from_eval(g.kind, v).map_err(|e| (id, format!("{}: {e}", g.name))))?;
            path.pop();
            done.insert(id, v.clone());
            Ok(v)
        }
        for (id, g) in &all {
            if g.formula.is_some() {
                visit(self, *id, &by_name, &mut done, &mut vec![])?;
            }
        }
        Ok(done)
    }

    /// Add a global parameter.
    pub fn add_global_param(&mut self, g: GlobalParam) -> Result<Id, String> {
        let name = g.name.trim().to_string();
        if name.is_empty() {
            return Err("a global parameter needs a name".into());
        }
        if self.global_by_name(&name).is_some() {
            return Err(format!("there is already a global parameter called {name}"));
        }
        if name.contains(['[', ']']) {
            return Err("a name cannot contain [ or ]".into());
        }
        if g.reporting && !matches!(g.kind, ParamKind::Length | ParamKind::Angle) {
            return Err("a reporting parameter is a length or an angle".into());
        }
        if g.reporting && g.formula.is_some() {
            return Err("a reporting parameter takes its value from a dimension, so it cannot have a formula".into());
        }
        let g = GlobalParam { name, ..g };
        self.grouped("Add global parameter", |d| {
            let id = d.transact("Add global parameter", |tx| Ok(tx.add(Element::GlobalParam(g))))?;
            d.apply_globals()?;
            Ok(id)
        })
    }

    /// Change a global parameter: `name`, `value`, `formula` (null or "" clears it), `reporting`, `group`.
    pub fn patch_global_param(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.global_param(id).cloned().ok_or("not a global parameter")?;
        let mut g = old.clone();
        if let Some(n) = patch.get("name").and_then(|v| v.as_str()) {
            let n = n.trim();
            if n.is_empty() {
                return Err("a global parameter needs a name".into());
            }
            if n.contains(['[', ']']) {
                return Err("a name cannot contain [ or ]".into());
            }
            if self.global_by_name(n).is_some_and(|(o, _)| o != id) {
                return Err(format!("there is already a global parameter called {n}"));
            }
            g.name = n.to_string();
        }
        if let Some(gr) = patch.get("group").and_then(|v| v.as_str()) {
            g.group = gr.trim().to_string();
        }
        if let Some(r) = patch.get("reporting").and_then(|v| v.as_bool()) {
            if r && !matches!(g.kind, ParamKind::Length | ParamKind::Angle) {
                return Err("a reporting parameter is a length or an angle".into());
            }
            if r && self.global_params().iter().any(|(o, x)| *o != id && x.formula.as_deref().is_some_and(|f| names_of(f).unwrap_or_default().iter().any(|n| n.eq_ignore_ascii_case(&old.name)))) {
                return Err(format!("a formula reads {}, and formulas cannot use a reporting parameter", old.name));
            }
            if r {
                g.formula = None;
            }
            g.reporting = r;
        }
        if let Some(f) = patch.get("formula") {
            let f = f.as_str().map(str::trim).filter(|s| !s.is_empty());
            if let Some(src) = f {
                if g.reporting {
                    return Err("a reporting parameter takes its value from a dimension, so it cannot have a formula".into());
                }
                for n in names_of(src)? {
                    match self.global_by_name(&n) {
                        None => return Err(format!("there is no global parameter called {n}")),
                        Some((_, o)) if o.reporting => return Err(format!("{n} is a reporting parameter, which formulas cannot use")),
                        _ => {}
                    }
                }
            }
            g.formula = f.map(String::from);
        }
        if let Some(v) = patch.get("value") {
            if g.reporting {
                return Err(format!("{} is a reporting parameter: its value comes from the dimension it labels", g.name));
            }
            if g.formula.is_some() && patch.get("formula").is_none() {
                return Err(format!("{} has a formula; change the formula, or clear it to type a value", g.name));
            }
            g.value = match (g.kind, v) {
                (ParamKind::YesNo, serde_json::Value::Bool(b)) => ParamValue::Bool(*b),
                (ParamKind::Text | ParamKind::Material, serde_json::Value::String(s)) => ParamValue::Text(s.clone()),
                (ParamKind::Integer, _) => ParamValue::Number(v.as_f64().ok_or("the value must be a number")?.round()),
                (_, _) => ParamValue::Number(v.as_f64().ok_or("the value must be a number")?),
            };
            if g.kind == ParamKind::Length && g.value.as_f64().is_some_and(|x| x <= 0.0) && !self.labelled_by(id).is_empty() {
                return Err("a length that labels a dimension must be greater than zero".into());
            }
        }
        let renamed = g.name != old.name;
        let mode_changed = g.reporting != old.reporting;
        self.grouped("Change global parameter", |d| {
            d.transact("Change global parameter", |tx| tx.update(id, Element::GlobalParam(g.clone())))?;
            if renamed {
                // formulas follow the new name
                for (o, x) in d.global_params().into_iter().map(|(o, x)| (o, x.clone())).collect::<Vec<_>>() {
                    if let Some(f) = &x.formula {
                        let nf = crate::famgeom::rename_in_expr(f, &old.name, &g.name);
                        if &nf != f {
                            d.transact("Change global parameter", |tx| tx.update(o, Element::GlobalParam(GlobalParam { formula: Some(nf), ..x.clone() })))?;
                        }
                    }
                }
            }
            if mode_changed {
                // switching between driving and reporting lets go of the labels
                d.unlabel_all(id)?;
            }
            d.apply_globals()
        })
    }

    /// Delete a global parameter: its labels let go (the geometry stays), associated family values keep their last
    /// value, and formulas that read it stop working until they are corrected.
    pub fn delete_global_param(&mut self, id: Id) -> Result<(), String> {
        self.global_param(id).ok_or("not a global parameter")?;
        self.grouped("Delete global parameter", |d| {
            d.unlabel_all(id)?;
            for (eid, name) in d.associated_with(id) {
                d.associate_global(eid, &name, None)?;
            }
            d.transact("Delete global parameter", |tx| tx.remove(id))
        })
    }

    fn unlabel_all(&mut self, gid: Id) -> Result<(), String> {
        for l in self.labelled_by(gid) {
            let Some(Element::Dimension(mut dm)) = self.get(l.dimension).cloned() else { continue };
            dm.segments[l.segment].param = None;
            self.transact("Unlabel dimension", |tx| tx.update(l.dimension, Element::Dimension(dm)))?;
        }
        Ok(())
    }

    /// Label one segment of a dimension with a global parameter, or take its label off (`None`).
    pub fn label_dimension(&mut self, did: Id, segment: usize, gid: Option<Id>) -> Result<(), String> {
        let Some(Element::Dimension(dm)) = self.get(did).cloned() else { return Err("not a dimension".into()) };
        let seg = dm.segments.get(segment).ok_or("no such segment")?.clone();
        let Some(gid) = gid else {
            // the geometry stays where it is; a reporting parameter keeps its last value
            let mut nd = dm.clone();
            nd.segments[segment].param = None;
            return self.transact("Unlabel dimension", |tx| tx.update(did, Element::Dimension(nd)));
        };
        let g = self.global_param(gid).cloned().ok_or("not a global parameter")?;
        if dm.kind == crate::dimension::DimKind::ArcLength {
            return Err("an arc length dimension takes no label yet".into());
        }
        if !labels_kind(g.kind, dm.kind) {
            return Err(if dm.kind == crate::dimension::DimKind::Angular { format!("{} is not an angle, so it cannot label an angular dimension", g.name) } else { format!("{} is not a length, so it cannot label this dimension", g.name) });
        }
        if seg.value_text.is_some() {
            return Err("this segment shows words in place of its value; clear them first".into());
        }
        // the dimension has to measure something now to take a label
        self.segment_value(&dm, segment)?;
        if g.reporting {
            if dm.segments.len() != 1 {
                return Err("a reporting parameter labels a dimension with one segment only".into());
            }
            if seg.param.is_some() {
                return Err("the dimension already has a label; take it off first".into());
            }
            if self.labelled_by(gid).iter().any(|l| l.dimension != did) {
                return Err(format!("{} already reports another dimension", g.name));
            }
        }
        self.grouped("Label dimension", |d| {
            let mut nd = dm.clone();
            nd.segments[segment].param = Some(gid);
            nd.segments[segment].locked = false;
            d.transact("Label dimension", |tx| tx.update(did, Element::Dimension(nd)))?;
            if g.reporting {
                d.read_reporting()?;
            } else {
                // the geometry takes the parameter's value at once
                let v = g.value.as_f64().ok_or("the parameter has no value")?;
                d.drive_segment(did, segment, v)?;
            }
            Ok(())
        })
    }

    /// Set a dimension segment to `value`, lifting its own hold while it moves.
    fn drive_segment(&mut self, did: Id, segment: usize, value: f64) -> Result<(), String> {
        let Some(Element::Dimension(dm)) = self.get(did).cloned() else { return Ok(()) };
        if (self.segment_value(&dm, segment)? - value).abs() < 1e-3 {
            return Ok(());
        }
        if value <= 0.0 {
            return Err("a labelled dimension must be greater than zero".into());
        }
        self.set_segment_value_raw(did, segment, value)
    }

    /// Push every driving parameter out: formulas worked out, labelled dimensions set, associated family values
    /// written. Refused whole when the geometry cannot take it.
    pub(crate) fn apply_globals(&mut self) -> Result<(), String> {
        let values = self.evaluate_globals().map_err(|(_, e)| e)?;
        for (id, v) in &values {
            let Some(g) = self.global_param(*id).cloned() else { continue };
            if g.formula.is_some() && &g.value != v {
                self.transact("Change global parameter", |tx| tx.update(*id, Element::GlobalParam(GlobalParam { value: v.clone(), ..g.clone() })))?;
            }
        }
        for (id, g) in self.global_params().into_iter().map(|(i, g)| (i, g.clone())).collect::<Vec<_>>() {
            if !g.reporting {
                if let Some(v) = g.value.as_f64().filter(|_| matches!(g.kind, ParamKind::Length | ParamKind::Angle)) {
                    for l in self.labelled_by(id) {
                        self.drive_segment(l.dimension, l.segment, v).map_err(|e| format!("{} cannot be {}: {e}", g.name, crate::famgeom::fmt_num(v)))?;
                    }
                }
            }
            for (eid, name) in self.associated_with(id) {
                self.write_associated(eid, &name, &g.value)?;
            }
        }
        Ok(())
    }

    /// Reporting parameters take the value their dimension measures now.
    pub(crate) fn read_reporting(&mut self) -> Result<(), String> {
        let mut writes = vec![];
        for (id, g) in self.global_params() {
            if !g.reporting {
                continue;
            }
            let Some(l) = self.labelled_by(id).into_iter().next() else { continue };
            let Some(dm) = self.dimension(l.dimension) else { continue };
            let Ok(v) = self.segment_value(dm, l.segment) else { continue };
            if g.value.as_f64().is_none_or(|old| (old - v).abs() > 1e-6) {
                writes.push((id, GlobalParam { value: ParamValue::Number((v * 1000.0).round() / 1000.0), ..g.clone() }));
            }
        }
        for (id, g) in writes {
            self.transact("Report dimension", |tx| tx.update(id, Element::GlobalParam(g)))?;
        }
        Ok(())
    }

    pub(crate) fn has_global_links(&self) -> bool {
        self.dimensions().iter().any(|(_, d)| d.segments.iter().any(|s| s.param.is_some()))
    }

    /// Make a family type's or instance's parameter take a global's value (`Some`), or let it go (`None`, it keeps
    /// the value it has).
    pub fn associate_global(&mut self, eid: Id, name: &str, gid: Option<Id>) -> Result<(), String> {
        let (family, instance) = match self.get(eid) {
            Some(Element::FamilyType(t)) => (t.family, false),
            Some(Element::FamilyInstance(i)) => (self.family_type(i.type_id).map(|t| t.family).ok_or("the instance has no type")?, true),
            _ => return Err("only a family type or a placed family has parameters to associate".into()),
        };
        if let Some(gid) = gid {
            let g = self.global_param(gid).cloned().ok_or("not a global parameter")?;
            let fam = self.family(family).ok_or("the family is gone")?;
            let p = fam.params.iter().find(|p| p.name == name).ok_or_else(|| format!("the family has no parameter called {name}"))?;
            if p.instance != instance {
                return Err(if instance { format!("{name} is a type parameter: associate it in Type Properties") } else { format!("{name} is an instance parameter: associate it on a placed one") });
            }
            if p.reporting || p.formula.is_some() {
                return Err(format!("{name} takes its value from the family itself (a formula or a dimension)"));
            }
            let same = p.param_kind == g.kind || (matches!(p.param_kind, ParamKind::Number | ParamKind::Integer) && matches!(g.kind, ParamKind::Number | ParamKind::Integer));
            if !same {
                return Err(format!("{} is a {:?} parameter and {name} is a {:?} one", g.name, g.kind, p.param_kind));
            }
        }
        let name = name.to_string();
        self.grouped("Associate global parameter", |d| {
            match d.get(eid).cloned() {
                Some(Element::FamilyType(mut t)) => {
                    match gid {
                        Some(g) => t.globals.insert(name.clone(), g),
                        None => t.globals.remove(&name),
                    };
                    d.transact("Associate global parameter", |tx| tx.update(eid, Element::FamilyType(t)))?;
                }
                Some(Element::FamilyInstance(mut i)) => {
                    match gid {
                        Some(g) => i.globals.insert(name.clone(), g),
                        None => i.globals.remove(&name),
                    };
                    d.transact("Associate global parameter", |tx| tx.update(eid, Element::FamilyInstance(i)))?;
                }
                _ => {}
            }
            if let Some(g) = gid.and_then(|g| d.global_param(g).cloned()) {
                d.write_associated(eid, &name, &g.value)?;
            }
            Ok(())
        })
    }

    fn write_associated(&mut self, eid: Id, name: &str, v: &ParamValue) -> Result<(), String> {
        match self.get(eid).cloned() {
            Some(Element::FamilyType(mut t)) if t.values.get(name) != Some(v) => {
                t.values.insert(name.to_string(), v.clone());
                self.transact("Global parameter", |tx| tx.update(eid, Element::FamilyType(t)))?;
            }
            Some(Element::FamilyInstance(i)) if i.values.get(name) != Some(v) => {
                let mut vals = serde_json::Map::new();
                vals.insert(name.to_string(), serde_json::to_value(v).map_err(|e| e.to_string())?);
                self.patch_instance(eid, &serde_json::json!({ "values": vals }))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The parameters of a family type or instance that a global sets (they cannot be typed into while it does).
    pub fn global_held(&self, eid: Id) -> BTreeSet<String> {
        match self.get(eid) {
            Some(Element::FamilyType(t)) => t.globals.keys().cloned().collect(),
            Some(Element::FamilyInstance(i)) => i.globals.keys().cloned().collect(),
            _ => BTreeSet::new(),
        }
    }
}
