//! Placed families that carry a stored body: the parameter values and host data that
//! drive the solver, the opening a body cuts, and where family coordinates land in the
//! project.
//!
//! The family's own axes are x along the host, y across it (towards the side the
//! instance faces) and z up from the reference level (the level plus the instance's
//! Sill Height). Flipping hand mirrors x, flipping facing mirrors y. Besides its own
//! parameters a body can read these host values, which OpenArch BIM supplies:
//! `Host Thickness`, `Host Core Thickness` and `Host Core Offset` (how far the host's core
//! centre lies towards the facing side).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use openarch_params::{EvalError, Value};

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::famgeom::Solved;
use crate::family::{FamilyInstance, ParamValue};

/// Solved bodies by instance, valid for one `edit_count`.
#[derive(Default)]
pub struct SolveCache(pub(crate) Mutex<(u64, HashMap<Id, Arc<Result<Solved, String>>>)>);

impl Clone for SolveCache {
    fn clone(&self) -> Self {
        SolveCache::default()
    }
}

impl std::fmt::Debug for SolveCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SolveCache")
    }
}

impl Document {
    /// Values a body can read about its host, in the body's own direction.
    fn host_value(&self, inst: &FamilyInstance, name: &str) -> Option<f64> {
        let host = inst.host?;
        let Some(Element::Wall(w)) = self.get(host.wall) else { return None };
        let swing = if inst.flip_facing { -1.0 } else { 1.0 };
        let (wl, wr) = self.wall_extents(w);
        let (core, off) = self.wall_core(w);
        match name {
            "Host Thickness" => Some(wl + wr),
            "Host Core Thickness" => Some(core),
            "Host Core Offset" => Some(swing * off),
            _ => None,
        }
    }

    fn body_env<'a>(&'a self, inst: &'a FamilyInstance) -> impl FnMut(&str) -> Result<Value, EvalError> + 'a {
        move |n: &str| {
            if let Some(v) = self.host_value(inst, n) {
                return Ok(Value::Num(v));
            }
            // a parameter that reports a dimension is read after the solve, so geometry cannot use it
            if self.family_of(inst).map_or(false, |f| f.body.reports(n)) {
                return Err(EvalError::UnknownName(n.to_string()));
            }
            match self.instance_param(inst, n) {
                Some(ParamValue::Number(x)) => Ok(Value::Num(x)),
                Some(ParamValue::Bool(b)) => Ok(Value::Bool(b)),
                Some(ParamValue::Text(s)) => Ok(Value::Text(s)),
                None => Err(EvalError::UnknownName(n.to_string())),
            }
        }
    }

    fn family_of(&self, inst: &FamilyInstance) -> Option<&crate::family::Family> {
        self.instance_family(inst).map(|(f, _)| f)
    }

    /// The value of a reporting parameter that reads a dimension, measured on the solved body.
    pub(crate) fn reported_value(&self, inst: &FamilyInstance, name: &str) -> Option<f64> {
        let fam = self.family_of(inst)?;
        if !fam.body.reports(name) {
            return None;
        }
        let solved = fam.body.solve(&mut self.body_env(inst)).ok()?;
        solved.reports.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    /// The solved body of a placed instance, or `None` when its family has no body.
    pub fn instance_solved(&self, id: Id) -> Option<Arc<Result<Solved, String>>> {
        let Some(Element::FamilyInstance(inst)) = self.get(id) else { return None };
        let (fam, _) = self.instance_family(inst)?;
        if fam.body.is_empty() {
            return None;
        }
        {
            let mut c = self.solve_cache.0.lock().ok()?;
            if c.0 != self.edit_count {
                *c = (self.edit_count, HashMap::new());
            }
            if let Some(hit) = c.1.get(&id) {
                return Some(hit.clone());
            }
        }
        let solved = Arc::new(fam.body.solve(&mut self.body_env(inst)));
        if let Ok(mut c) = self.solve_cache.0.lock() {
            if c.0 == self.edit_count {
                c.1.insert(id, solved.clone());
            }
        }
        Some(solved)
    }

    /// The box the host-cutting voids of an instance span, in family coordinates.
    /// `None` when the family has no body or no host-cutting void.
    pub fn instance_cut_box(&self, inst: &FamilyInstance) -> Option<[[f64; 3]; 2]> {
        let (fam, _) = self.instance_family(inst)?;
        if fam.body.is_empty() {
            return None;
        }
        fam.body.opening_box(&mut self.body_env(inst)).ok().flatten()
    }
}

impl Document {
    /// Add a family and its types. One undo step.
    pub fn add_family(&mut self, family: crate::family::Family, types: Vec<(String, std::collections::BTreeMap<String, ParamValue>)>) -> Result<(Id, Vec<Id>), String> {
        if self.families().iter().any(|(_, f)| f.name == family.name) {
            return Err(format!("a family called '{}' already exists", family.name));
        }
        self.transact("Add family", |tx| {
            let fid = tx.add(Element::Family(family));
            let ids = types.into_iter().map(|(name, values)| tx.add(Element::FamilyType(crate::family::FamilyType { family: fid, name, values }))).collect();
            Ok((fid, ids))
        })
    }

    /// Replace a family's body (the family editor saves through this). One undo step. Instances
    /// that no longer fit their hosts are not checked here; the editor warns before saving.
    pub fn set_family_body(&mut self, family: Id, body: crate::famgeom::FamilyBody) -> Result<(), String> {
        let Some(Element::Family(f)) = self.get(family) else { return Err("no such family".into()) };
        let mut f = f.clone();
        f.body = body;
        self.transact("Edit family", |tx| tx.update(family, Element::Family(f)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::famgeom::*;
    use crate::family::*;
    use crate::geom::Point;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn form(shape: FormShape, void: bool, subcategory: &str) -> Form {
        Form { id: 0, name: String::new(), shape, void, cuts_host: true, cut_by: vec![], material: "Timber".into(), material_param: None, subcategory: subcategory.into(), vis: Visibility::default() }
    }

    /// A hand-made door: a slab leaf in an opening void, with a line across it in plan.
    fn hatch_family() -> Family {
        let mut b = FamilyBody::template();
        let left = b.add_offset_plane("Left", PLANE_LR, "-[Width]/2").unwrap();
        let right = b.add_offset_plane("Right", PLANE_LR, "[Width]/2").unwrap();
        let top = b.add_offset_plane("Top", PLANE_LEVEL, "[Height]").unwrap();
        let rect = || vec![Loop::rect(Coord::on(left), Coord::on(right), Coord::on(PLANE_LEVEL), Coord::on(top))];
        b.add_form(form(FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_FB, start: "0".into(), end: "40".into(), profile: rect() }, false, "Panel"));
        b.add_form(form(FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_FB, start: "-150".into(), end: "150".into(), profile: rect() }, true, ""));
        b.add_sym(SymElement {
            id: 0,
            view: SymView::Plan,
            path: Loop { nodes: vec![Node::new(Coord::on(left), Coord::on(PLANE_FB)), Node::new(Coord::on(right), Coord::on(PLANE_FB))], bulges: vec![], closed: false },
            filled: false,
            masking: false,
            subcategory: "Plan Swing".into(),
            vis: Visibility::default(),
        });
        let p = |name: &str, kind, instance, default| ParamDef { name: name.into(), param_kind: kind, instance, group: "Dimensions".into(), formula: None, reporting: false, shared_guid: None, choices: vec![], default };
        Family {
            name: "Hatch".into(),
            category: FamilyCategory::Doors,
            host: HostKind::Wall,
            plan_symbol: PlanSymbol::None,
            params: vec![
                p("Width", ParamKind::Length, false, ParamValue::Number(900.0)),
                p("Height", ParamKind::Length, false, ParamValue::Number(2100.0)),
                p("Sill Height", ParamKind::Length, true, ParamValue::Number(0.0)),
            ],
            built_in: false,
            catalog: vec![],
            body: b,
        }
    }

    #[test]
    fn a_hand_made_family_cuts_its_own_opening_and_draws_from_its_geometry() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let (_, types) = d.add_family(hatch_family(), vec![("900 x 2100".into(), BTreeMap::new())]).unwrap();
        let id = d.place_on_wall(types[0], w, Point::new(3000., 0.)).unwrap();
        let Some(Element::FamilyInstance(inst)) = d.get(id) else { panic!() };
        assert_eq!(d.instance_opening_size(inst), (900.0, 2100.0, 0.0));
        let ops = d.wall_openings(w);
        assert_eq!(ops.len(), 1);
        assert!((ops[0].u0 - 2550.0).abs() < 1e-6 && (ops[0].u1 - 3450.0).abs() < 1e-6 && ops[0].z1 > 2099.0);
        // plan: the leaf cut at 1200 is 900 x 40, and the symbolic line comes through
        let sym = crate::symbol::plan_symbol_at(&d, id, crate::view::DetailLevel::Medium, Some(1200.0)).unwrap();
        let leaf = sym.parts.iter().find(|p| p.subcategory == "Panel").unwrap();
        assert!(leaf.cut && (crate::geom::signed_area(&leaf.rings[0]).abs() - 900.0 * 40.0).abs() < 1e-6);
        assert!(sym.parts.iter().any(|p| p.subcategory == "Plan Swing" && !p.closed && p.rings[0].len() == 2));
        // 3D: the leaf is a closed solid of the right size (mesh is in metres)
        let (cat, solid, glass) = crate::mesh::instance_meshes(&d, id).unwrap();
        assert_eq!(cat, FamilyCategory::Doors);
        assert!(glass.is_empty() && !solid.is_empty());
        // changing the type makes a wider opening straight away (the solved body is not stale)
        d.patch_family_type(types[0], &json!({ "values": { "Width": 1200.0 } })).unwrap();
        let Some(Element::FamilyInstance(inst)) = d.get(id) else { panic!() };
        assert_eq!(d.instance_opening_size(inst).0, 1200.0);
        let ops = d.wall_openings(w);
        assert!((ops[0].u1 - ops[0].u0 - 1200.0).abs() < 1e-6);
        // undo brings the old size back
        d.undo();
        assert_eq!(d.wall_openings(w)[0].u1 - d.wall_openings(w)[0].u0, 900.0);
    }

    #[test]
    fn flipping_hand_and_facing_mirror_the_body() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let (_, types) = d.add_family(hatch_family(), vec![("900 x 2100".into(), BTreeMap::new())]).unwrap();
        let id = d.place_on_wall(types[0], w, Point::new(3000., 0.)).unwrap();
        let before = d.instance_frame(id).unwrap();
        d.flip_instances(&[id]).unwrap();
        let after = d.instance_frame(id).unwrap();
        assert!(before.flip_facing != after.flip_facing || before.flip_hand != after.flip_hand);
        // a point in front of the family lands on the other side of the wall after a facing flip
        let (a, b) = (before.fam_world(0.0, 100.0), after.fam_world(0.0, 100.0));
        assert!(before.flip_facing == after.flip_facing || (a.y - b.y).abs() > 1.0);
    }

    #[test]
    fn a_shipped_family_can_be_copied_and_the_copy_edited_alone() {
        let mut d = Document::new_project();
        let src = d.families().into_iter().find(|(_, f)| f.name == "Single-Flush").unwrap().0;
        assert!(d.duplicate_family(src, "Single-Flush").unwrap_err().contains("already exists"));
        assert!(d.duplicate_family(src, "  ").is_err());
        let copy = d.duplicate_family(src, "Cottage Door").unwrap();
        let (a, b) = (d.family(src).unwrap().clone(), d.family(copy).unwrap().clone());
        assert!(a.built_in && !b.built_in);
        assert_eq!((a.body.planes.len(), a.params.len()), (b.body.planes.len(), b.params.len()));
        let (na, nb) = (d.family_types().iter().filter(|(_, t)| t.family == src).count(), d.family_types().iter().filter(|(_, t)| t.family == copy).count());
        assert_eq!(na, nb);
        // editing the copy leaves the shipped door alone
        let mut body = b.body.clone();
        body.forms.truncate(1);
        d.set_family_body(copy, body).unwrap();
        assert_eq!(d.family(src).unwrap().body.forms.len(), a.body.forms.len());
        assert_eq!(d.family(copy).unwrap().body.forms.len(), 1);
    }

    #[test]
    fn a_reporting_parameter_reads_a_dimension_and_cannot_be_set_or_used_by_geometry() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let (fid, types) = d.add_family(hatch_family(), vec![("900 x 2100".into(), BTreeMap::new())]).unwrap();
        let mut fam = d.family(fid).unwrap().clone();
        let (left, right) = (fam.body.plane_named("Left").unwrap(), fam.body.plane_named("Right").unwrap());
        fam.params.push(ParamDef { name: "Clear Width".into(), param_kind: ParamKind::Length, instance: false, group: "Dimensions".into(), formula: None, reporting: true, shared_guid: None, choices: vec![], default: ParamValue::Number(0.0) });
        fam.body.dims.push(DimDef { a: left, b: right, reports_to: Some("Clear Width".into()) });
        d.set_family_def(fid, Some(fam.params.clone()), Some(fam.body.clone())).unwrap();
        let id = d.place_on_wall(types[0], w, Point::new(3000., 0.)).unwrap();
        let Some(Element::FamilyInstance(inst)) = d.get(id) else { panic!() };
        assert_eq!(d.instance_param(inst, "Clear Width").and_then(|v| v.as_f64()), Some(900.0));
        // it follows the type
        d.patch_family_type(types[0], &json!({ "values": { "Width": 1100.0 } })).unwrap();
        let Some(Element::FamilyInstance(inst)) = d.get(id) else { panic!() };
        assert_eq!(d.instance_param(inst, "Clear Width").and_then(|v| v.as_f64()), Some(1100.0));
        // it is read only
        assert!(d.patch_family_type(types[0], &json!({ "values": { "Clear Width": 5.0 } })).is_err());
        // a dimension cannot report to a missing or ordinary parameter
        let mut bad = fam.body.clone();
        bad.dims[0].reports_to = Some("Width".into());
        assert!(d.set_family_def(fid, None, Some(bad)).is_err());
        // geometry that uses the reported value is refused rather than looping
        let mut loopy = fam.body.clone();
        loopy.add_offset_plane("Loop", PLANE_LR, "[Clear Width]").unwrap();
        assert!(d.family_preview(fid, None, &BTreeMap::new()).is_ok());
        d.set_family_def(fid, None, Some(loopy)).unwrap();
        assert!(d.family_preview(fid, None, &BTreeMap::new()).is_err());
    }

    #[test]
    fn types_can_be_deleted_and_the_category_changed_only_while_nothing_uses_them() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let (fid, types) = d.add_family(hatch_family(), vec![("900 x 2100".into(), BTreeMap::new())]).unwrap();
        assert!(d.delete_family_type(types[0]).unwrap_err().contains("at least one"));
        let second = d.duplicate_family_type(types[0], "1200 x 2100").unwrap();
        let id = d.place_on_wall(second, w, Point::new(3000., 0.)).unwrap();
        assert!(d.delete_family_type(second).unwrap_err().contains("placed"));
        assert!(d.set_family_meta(fid, Some(FamilyCategory::Windows), None).is_err());
        d.delete(&[id]).unwrap();
        d.delete_family_type(second).unwrap();
        d.set_family_meta(fid, Some(FamilyCategory::Furniture), Some(HostKind::None)).unwrap();
        let f = d.family(fid).unwrap();
        assert_eq!((f.category, f.host), (FamilyCategory::Furniture, HostKind::None));
        assert_eq!(f.category.ifc_class(), "IfcFurniture");
    }

    #[test]
    fn reference_kinds_offer_the_named_references_of_their_axis_and_old_files_still_read() {
        use crate::famgeom::{Axis, RefKind};
        assert!(RefKind::choices(Axis::X).contains(&RefKind::CenterLeftRight) && !RefKind::choices(Axis::X).contains(&RefKind::Top));
        assert!(RefKind::choices(Axis::Z).contains(&RefKind::Top));
        let p: crate::famgeom::RefPlane = serde_json::from_value(json!({ "id": 9, "name": "Jamb", "axis": "x", "anchor": { "type": "origin" } })).unwrap();
        assert_eq!(p.ref_kind, RefKind::Weak);
        let q: crate::famgeom::RefPlane = serde_json::from_value(json!({ "id": 9, "name": "Jamb", "axis": "x", "anchor": { "type": "origin" }, "ref_kind": "center_left_right" })).unwrap();
        assert_eq!(q.ref_kind, RefKind::CenterLeftRight);
    }
}

/// What a family file holds: the family and its types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FamilyFile {
    /// Format marker so other files are not mistaken for one.
    #[serde(alias = "ironbark_family")] // files written before the rename
    pub openarch_family: u32,
    pub family: crate::family::Family,
    #[serde(default)]
    pub types: Vec<FamilyFileType>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FamilyFileType {
    pub name: String,
    #[serde(default)]
    pub values: std::collections::BTreeMap<String, ParamValue>,
}

impl Document {
    /// A new family of its own kind, with the origin planes, the usual size parameters and one type.
    /// Door and window families are hosted in walls and start with a Width, Height and Sill Height.
    pub fn new_family(&mut self, name: &str, category: crate::family::FamilyCategory, host: crate::family::HostKind) -> Result<Id, String> {
        use crate::family::{Family, ParamDef, ParamKind, PlanSymbol};
        let name = name.trim();
        if name.is_empty() {
            return Err("a family needs a name".into());
        }
        let p = |n: &str, instance: bool, group: &str, default: f64| ParamDef {
            name: n.into(),
            param_kind: ParamKind::Length,
            instance,
            group: group.into(),
            formula: None,
            reporting: false,
            shared_guid: None,
            choices: vec![],
            default: ParamValue::Number(default),
        };
        let hosted = host == crate::family::HostKind::Wall;
        let (w, h, sill) = match category {
            crate::family::FamilyCategory::Windows => (1200.0, 1200.0, 900.0),
            crate::family::FamilyCategory::Doors => (820.0, 2040.0, 0.0),
            _ => (600.0, 600.0, 0.0),
        };
        let mut params = vec![p("Width", false, "Dimensions", w), p("Height", false, "Dimensions", h)];
        if hosted {
            params.push(p("Sill Height", true, "Constraints", sill));
        }
        let mut body = crate::famgeom::FamilyBody::template();
        body.cut_with_voids = hosted;
        let family = Family { name: name.into(), category, host, plan_symbol: PlanSymbol::None, params, built_in: false, catalog: vec![], body };
        let (id, _) = self.add_family(family, vec![("Type 1".into(), Default::default())])?;
        Ok(id)
    }

    /// Replace a family's parameters and/or body in one undo step. Parameter names must be unique and
    /// every formula must read; the body must solve for the family's first type.
    pub fn set_family_def(&mut self, family: Id, params: Option<Vec<crate::family::ParamDef>>, body: Option<crate::famgeom::FamilyBody>) -> Result<(), String> {
        let Some(Element::Family(f)) = self.get(family) else { return Err("no such family".into()) };
        let mut f = f.clone();
        if let Some(ps) = params {
            let mut seen = std::collections::BTreeSet::new();
            for p in &ps {
                if p.name.trim().is_empty() || !seen.insert(p.name.clone()) {
                    return Err(format!("parameter '{}' is empty or used twice", p.name));
                }
                if let Some(src) = &p.formula {
                    openarch_params::expr::parse(src).map_err(|e| format!("formula of '{}': {e:?}", p.name))?;
                }
            }
            f.params = ps;
        }
        if let Some(b) = body {
            f.body = b;
        }
        for d in &f.body.dims {
            if let Some(n) = &d.reports_to {
                match f.param(n) {
                    Some(p) if p.reporting && p.formula.is_none() => {}
                    Some(_) => return Err(format!("'{n}' reports a dimension, so it must be a reporting parameter without a formula")),
                    None => return Err(format!("a dimension reports to '{n}', which is not a parameter")),
                }
            }
        }
        self.transact("Edit family", |tx| tx.update(family, Element::Family(f)))?;
        Ok(())
    }

    /// The family as it stands for one type, with `overrides` laid over the type's values (the
    /// "flex"). Host values take plain defaults: a 100 mm wall with its core centred.
    pub fn family_preview(&self, family: Id, type_id: Option<Id>, overrides: &std::collections::BTreeMap<String, ParamValue>) -> Result<Solved, String> {
        let fam = self.family(family).ok_or("no such family")?;
        let base = match type_id {
            Some(t) => self.family_type(t).ok_or("no such type")?.clone(),
            None => self.family_types().into_iter().find(|(_, t)| t.family == family).map(|(_, t)| t.clone()).unwrap_or(crate::family::FamilyType { family, name: String::new(), values: Default::default() }),
        };
        let mut t = base;
        for (k, v) in overrides {
            t.values.insert(k.clone(), v.clone());
        }
        let mut env = |n: &str| -> Result<Value, EvalError> {
            match n {
                "Host Thickness" => return Ok(Value::Num(100.0)),
                "Host Core Thickness" => return Ok(Value::Num(100.0)),
                "Host Core Offset" => return Ok(Value::Num(0.0)),
                _ if fam.body.reports(n) => return Err(EvalError::UnknownName(n.to_string())),
                _ => {}
            }
            match self.type_param(&t, n) {
                Some(ParamValue::Number(x)) => Ok(Value::Num(x)),
                Some(ParamValue::Bool(b)) => Ok(Value::Bool(b)),
                Some(ParamValue::Text(s)) => Ok(Value::Text(s)),
                None => Err(EvalError::UnknownName(n.to_string())),
            }
        };
        fam.body.solve(&mut env)
    }

    pub fn export_family(&self, family: Id) -> Result<FamilyFile, String> {
        let fam = self.family(family).ok_or("no such family")?;
        let mut f = fam.clone();
        f.built_in = false;
        let types = self.family_types().into_iter().filter(|(_, t)| t.family == family).map(|(_, t)| FamilyFileType { name: t.name.clone(), values: t.values.clone() }).collect();
        Ok(FamilyFile { openarch_family: 1, family: f, types })
    }

    /// A copy of a family and its types under a new name, to start a custom family from a shipped one.
    /// The copy is not built in, so it is never replaced when the project opens.
    pub fn duplicate_family(&mut self, family: Id, name: &str) -> Result<Id, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("give the copy a name".into());
        }
        let mut file = self.export_family(family)?;
        file.family.name = name.to_string();
        self.import_family(file)
    }

    /// Load a family file into the project. A family of the same name is refused so nothing is replaced silently.
    pub fn import_family(&mut self, file: FamilyFile) -> Result<Id, String> {
        if file.openarch_family != 1 {
            return Err("this is not an OpenArch BIM family file".into());
        }
        let FamilyFile { family, types, .. } = file;
        let types = if types.is_empty() { vec![FamilyFileType { name: "Type 1".into(), values: Default::default() }] } else { types };
        let (id, _) = self.add_family(family, types.into_iter().map(|t| (t.name, t.values)).collect())?;
        Ok(id)
    }


}
