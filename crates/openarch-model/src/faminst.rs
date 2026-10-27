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

/// Rename a parameter in a family's parameter list and body: every formula, expression, dimension label
/// and material link that names it follows. `names` are the other names in use, which must not clash.
pub fn rename_family_param(params: &mut [crate::family::ParamDef], body: &mut crate::famgeom::FamilyBody, old: &str, new: &str) -> Result<(), String> {
    let new = new.trim();
    if new.is_empty() {
        return Err("a parameter needs a name".into());
    }
    if new.contains('[') || new.contains(']') || new.contains('"') {
        return Err("a parameter name cannot contain [ ] or quotes".into());
    }
    if params.iter().any(|p| p.name != old && p.name.eq_ignore_ascii_case(new)) {
        return Err(format!("a parameter called '{new}' already exists"));
    }
    let Some(p) = params.iter_mut().find(|p| p.name == old) else { return Err(format!("no parameter called '{old}'")) };
    p.name = new.to_string();
    for q in params.iter_mut() {
        if let Some(f) = &q.formula {
            if crate::famgeom::expr_names(f).iter().any(|n| n == old) {
                q.formula = Some(crate::famgeom::rename_in_expr(f, old, new));
            }
        }
    }
    body.rename_param(old, new);
    Ok(())
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

    /// Work out a body with its components: each nested family is solved with its own type and the
    /// values its associations give it, then placed in this body. `stack` holds the families being
    /// solved, so a family that holds itself (directly or through another) is stopped.
    pub(crate) fn solve_family_body(&self, name: &str, body: &crate::famgeom::FamilyBody, env: &mut crate::famgeom::Env, stack: &mut Vec<String>) -> Result<Solved, String> {
        if body.nested.is_empty() {
            return body.solve(env);
        }
        stack.push(name.to_string());
        let r = body.solve_with(env, &mut |n, values, type_name| self.solve_nested(n, values, type_name, stack));
        stack.pop();
        r
    }

    fn solve_nested(&self, n: &crate::famgeom::Nested, values: &std::collections::BTreeMap<String, Value>, type_name: &str, stack: &mut Vec<String>) -> Result<Option<crate::famgeom::NestedSolved>, String> {
        let Some((fid, fam)) = self.families().into_iter().find(|(_, f)| f.name == n.family) else {
            return Err(format!("the family '{}' is not in the project", n.family));
        };
        if stack.iter().any(|s| *s == fam.name) || stack.len() >= 6 {
            return Err(format!("'{}' is nested inside itself", fam.name));
        }
        if fam.body.is_empty() {
            return Ok(None);
        }
        let mut types: Vec<&crate::family::FamilyType> = self.family_types().into_iter().filter(|(_, t)| t.family == fid).map(|(_, t)| t).collect();
        types.sort_by(|a, b| a.name.cmp(&b.name));
        let mut t = types.iter().find(|t| t.name == type_name).or_else(|| types.first()).map(|t| (*t).clone()).unwrap_or(crate::family::FamilyType { family: fid, name: String::new(), values: Default::default(), globals: Default::default() });
        for (k, v) in values {
            let pv = match v {
                Value::Num(x) => ParamValue::Number(*x),
                Value::Bool(b) => ParamValue::Bool(*b),
                Value::Text(s) => ParamValue::Text(s.clone()),
            };
            t.values.insert(k.clone(), pv);
        }
        let mut env = |name: &str| -> Result<Value, EvalError> {
            match name {
                "Host Thickness" | "Host Core Thickness" => return Ok(Value::Num(100.0)),
                "Host Core Offset" => return Ok(Value::Num(0.0)),
                _ if fam.body.reports(name) => return Err(EvalError::UnknownName(name.to_string())),
                _ => {}
            }
            match self.type_param(&t, name) {
                Some(ParamValue::Number(x)) => Ok(Value::Num(x)),
                Some(ParamValue::Bool(b)) => Ok(Value::Bool(b)),
                Some(ParamValue::Text(s)) => Ok(Value::Text(s)),
                None => Err(EvalError::UnknownName(name.to_string())),
            }
        };
        let solved = self.solve_family_body(&fam.name, &fam.body, &mut env, stack)?;
        Ok(Some(crate::famgeom::NestedSolved { solved, work_plane_based: fam.host == crate::family::HostKind::Face }))
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
        let solved = Arc::new(self.solve_family_body(&fam.name, &fam.body, &mut self.body_env(inst), &mut Vec::new()));
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
            let ids = types.into_iter().map(|(name, values)| tx.add(Element::FamilyType(crate::family::FamilyType { family: fid, name, values, globals: Default::default() }))).collect();
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
        Form { id: 0, name: String::new(), shape, void, cuts_host: true, cut_by: vec![], opening: false, material: "Timber".into(), material_param: None, subcategory: subcategory.into(), vis: Visibility::default() }
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
            path: Loop { nodes: vec![Node::new(Coord::on(left), Coord::on(PLANE_FB)), Node::new(Coord::on(right), Coord::on(PLANE_FB))], bulges: vec![], closed: false, shapes: vec![] },
            filled: false,
            masking: false,
            subcategory: "Plan Swing".into(),
            vis: Visibility::default(),
        });
        let p = |name: &str, kind, instance, default| ParamDef { name: name.into(), param_kind: kind, instance, group: "Dimensions".into(), formula: None, reporting: false, shared_guid: None, choices: vec![], discipline: "common".into(), tooltip: String::new(), locked: false, default };
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
            settings: Default::default(),
            source_path: None,
            loaded_at: None,
            body: b,
        }
    }

    #[test]
    fn loading_a_family_file_over_the_project_copy_keeps_placed_instances() {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let mut vals = BTreeMap::new();
        vals.insert("Width".to_string(), ParamValue::Number(800.0));
        let (fid, types) = d.add_family(hatch_family(), vec![("800".into(), vals)]).unwrap();
        let inst = d.place_on_wall(types[0], w, Point::new(3000., 0.)).unwrap();
        // the file: the same family with a second type, a new parameter, and Width 1000 on type "800"
        let mut file = d.export_family(fid).unwrap();
        assert!(file.family.source_path.is_none(), "a file never carries the project's path");
        file.types[0].values.insert("Width".into(), ParamValue::Number(1000.0));
        file.types.push(FamilyFileType { name: "1200".into(), values: BTreeMap::from([("Width".to_string(), ParamValue::Number(1200.0))]) });
        file.family.params.push(ParamDef { name: "Note".into(), param_kind: ParamKind::Text, instance: false, group: "Identity Data".into(), formula: None, reporting: false, shared_guid: None, choices: vec![], discipline: "common".into(), tooltip: String::new(), locked: false, default: ParamValue::Text(String::new()) });
        let src = Some(("C:/families/Hatch.oaf".to_string(), Some("2026-10-20".to_string())));
        assert!(d.import_family_with(file.clone(), ImportMode::Refuse, src.clone()).is_err(), "same name is refused");
        // keep the project's values: type "800" stays 800, "1200" is added
        let (id, replaced) = d.import_family_with(file.clone(), ImportMode::KeepValues, src.clone()).unwrap();
        assert!(id == fid && replaced);
        let ts: Vec<_> = d.family_types().into_iter().filter(|(_, t)| t.family == fid).map(|(_, t)| (t.name.clone(), t.values.get("Width").and_then(|v| v.as_f64()))).collect();
        assert!(ts.contains(&("800".to_string(), Some(800.0))) && ts.contains(&("1200".to_string(), Some(1200.0))), "{ts:?}");
        assert!(d.family(fid).unwrap().param("Note").is_some());
        assert_eq!(d.family(fid).unwrap().source_path.as_deref(), Some("C:/families/Hatch.oaf"));
        assert!(d.get(inst).is_some(), "the placed instance is still there");
        // overwrite with the file's values: type "800" becomes 1000
        d.import_family_with(file, ImportMode::Overwrite, src).unwrap();
        let w800 = d.family_types().into_iter().find(|(_, t)| t.family == fid && t.name == "800").unwrap().1.values.get("Width").and_then(|v| v.as_f64());
        assert_eq!(w800, Some(1000.0));
        // one undo step brings the earlier copy back
        d.undo().unwrap();
        let w800 = d.family_types().into_iter().find(|(_, t)| t.family == fid && t.name == "800").unwrap().1.values.get("Width").and_then(|v| v.as_f64());
        assert_eq!(w800, Some(800.0));
        // Save from the editor: the file holds the editor's values without touching the project
        let tid = types[0];
        let f2 = d.export_family_with(fid, None, None, None, &BTreeMap::from([(tid, BTreeMap::from([("Width".to_string(), ParamValue::Number(950.0))]))])).unwrap();
        assert_eq!(f2.types.iter().find(|t| t.name == "800").unwrap().values.get("Width").and_then(|v| v.as_f64()), Some(950.0));
        assert_eq!(d.family_type(tid).unwrap().values.get("Width").and_then(|v| v.as_f64()), Some(800.0));
        d.set_family_source(fid, Some("D:/x.oaf".into()), None).unwrap();
        assert_eq!(d.family(fid).unwrap().source_path.as_deref(), Some("D:/x.oaf"));
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
    fn model_lines_show_in_plan_flattened_and_an_opening_sets_the_hole() {
        let mut fam = hatch_family();
        let left = fam.body.planes.iter().find(|p| p.name == "Left").unwrap().id;
        let right = fam.body.planes.iter().find(|p| p.name == "Right").unwrap().id;
        // a rail drawn on the front plane at height 1000 runs across the plan; a post is a point in plan and is left out
        let rail = Loop { nodes: vec![Node::new(Coord::on(left), Coord::from(PLANE_LEVEL, 1000.0)), Node::new(Coord::on(right), Coord::from(PLANE_LEVEL, 1000.0))], bulges: vec![], closed: false, shapes: vec![] };
        let post = Loop { nodes: vec![Node::new(Coord::from(PLANE_LR, 100.0), Coord::from(PLANE_LEVEL, 0.0)), Node::new(Coord::from(PLANE_LR, 100.0), Coord::from(PLANE_LEVEL, 500.0))], bulges: vec![], closed: false, shapes: vec![] };
        for (name, path) in [("Rail", rail), ("Post", post)] {
            let id = fam.body.take_id();
            fam.body.model_lines.push(ModelLine { id, name: name.into(), plane: PLANE_FB, path, subcategory: "Model Lines".into(), vis: Visibility::default() });
        }
        // the rectangular void is replaced by an opening: a narrower profile whose depth no longer matters
        fam.body.forms.retain(|f| !f.void);
        let hole = vec![Loop::rect(Coord::from(PLANE_LR, -300.0), Coord::from(PLANE_LR, 300.0), Coord::on(PLANE_LEVEL), Coord::from(PLANE_LEVEL, 1800.0))];
        let id = fam.body.take_id();
        fam.body.forms.push(Form { id, name: "Opening 1".into(), shape: FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_FB, start: "0".into(), end: "1".into(), profile: hole }, void: true, cuts_host: true, opening: true, cut_by: vec![], material: String::new(), material_param: None, subcategory: String::new(), vis: Visibility::default() });
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let (_, types) = d.add_family(fam, vec![("t".into(), BTreeMap::new())]).unwrap();
        let inst = d.place_on_wall(types[0], w, Point::new(3000., 0.)).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(inst) else { panic!() };
        assert_eq!(d.instance_opening_size(i), (600.0, 1800.0, 0.0));
        let sym = crate::symbol::plan_symbol_at(&d, inst, crate::view::DetailLevel::Medium, Some(1200.0)).unwrap();
        let lines: Vec<_> = sym.parts.iter().filter(|p| p.subcategory == "Model Lines").collect();
        assert_eq!(lines.len(), 1, "the rail shows, the post flattens to a point and is dropped");
        assert_eq!(lines[0].rings[0].len(), 2);
    }

    fn tri_box(f: &SolvedForm) -> [[f64; 3]; 2] {
        let mut b = [[f64::MAX; 3], [f64::MIN; 3]];
        for t in &f.solid.tris {
            for p in t {
                for i in 0..3 {
                    b[0][i] = b[0][i].min(p[i]);
                    b[1][i] = b[1][i].max(p[i]);
                }
            }
        }
        b
    }

    #[test]
    fn a_component_is_solved_with_its_own_type_and_placed_in_the_parent() {
        let mut leaf = hatch_family();
        leaf.name = "Leaf".into();
        leaf.host = HostKind::None;
        let mut frame = hatch_family();
        frame.name = "Frame".into();
        let nid = frame.body.take_id();
        frame.body.nested.push(Nested {
            id: nid,
            family: "Leaf".into(),
            type_name: "t".into(),
            type_param: None,
            name: String::new(),
            origin: [Coord::at(2000.0), Coord::at(0.0), Coord::at(0.0)],
            rotation_deg: 0.0,
            work_plane: None,
            flip: false,
            mirror: false,
            params: BTreeMap::from([("Width".to_string(), "[Width] / 2".to_string())]),
            vis: Visibility::default(),
        });
        let mut d = Document::new_project();
        d.add_family(leaf, vec![("t".into(), BTreeMap::new())]).unwrap();
        let (fid, _) = d.add_family(frame, vec![("t".into(), BTreeMap::new())]).unwrap();
        let none = BTreeMap::new();
        let s = d.family_preview(fid, None, &none).unwrap();
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let mine: Vec<_> = s.forms.iter().filter(|f| f.owner == Some(nid)).collect();
        assert_eq!(mine.len(), 2, "the leaf's panel and its void");
        let panel = mine.iter().find(|f| !f.void).unwrap();
        let b = tri_box(panel);
        // the leaf is 450 wide here (half of 900) and its centre stands at x = 2000
        assert!((b[0][0] - 1775.0).abs() < 1e-6 && (b[1][0] - 2225.0).abs() < 1e-6, "{b:?}");
        assert!(panel.cuts_host == false && panel.solid.axis.is_some(), "a plain move keeps the exact prism");
        assert!(s.forms.iter().any(|f| f.owner.is_none()), "the parent's own pieces are still there");
        assert!(mine.iter().all(|f| f.id >= 1_000_000));
        // the leaf's plan line is carried into the parent's plan
        assert!(s.syms.iter().any(|y| y.owner == Some(nid) && y.view == SymView::Plan && (y.points[0].x - 1775.0).abs() < 1e-6));
        // a quarter turn: the 450 width now runs along Y
        let mut f2 = d.family(fid).unwrap().clone();
        f2.body.nested[0].rotation_deg = 90.0;
        d.set_family_def(fid, None, Some(f2.body.clone())).unwrap();
        let s = d.family_preview(fid, None, &none).unwrap();
        let panel = s.forms.iter().find(|f| f.owner == Some(nid) && !f.void).unwrap();
        let b = tri_box(panel);
        assert!((b[0][1] + 225.0).abs() < 1e-6 && (b[1][1] - 225.0).abs() < 1e-6, "{b:?}");
        assert!(panel.solid.axis.is_none(), "a turned piece is a mesh");
        // not visible: gone
        f2.body.nested[0].vis.visible = false;
        d.set_family_def(fid, None, Some(f2.body.clone())).unwrap();
        assert!(d.family_preview(fid, None, &none).unwrap().forms.iter().all(|f| f.owner.is_none()));
        // a family that is not in the project is reported and the rest still solves
        f2.body.nested[0].vis.visible = true;
        f2.body.nested[0].family = "Nothing".into();
        d.set_family_def(fid, None, Some(f2.body.clone())).unwrap();
        let s = d.family_preview(fid, None, &none).unwrap();
        assert!(s.warnings.iter().any(|w| w.contains("Nothing")), "{:?}", s.warnings);
        assert!(!s.forms.is_empty());
    }

    #[test]
    fn a_placed_family_with_a_component_draws_it_in_plan_and_survives_a_file_round_trip() {
        let mut leaf = hatch_family();
        leaf.name = "Leaf".into();
        leaf.host = HostKind::None;
        let mut frame = hatch_family();
        frame.name = "Frame".into();
        let nid = frame.body.take_id();
        frame.body.nested.push(Nested { id: nid, family: "Leaf".into(), type_name: "t".into(), type_param: None, name: String::new(), origin: [Coord::at(0.0), Coord::at(0.0), Coord::at(0.0)], rotation_deg: 0.0, work_plane: None, flip: false, mirror: false, params: BTreeMap::from([("Width".to_string(), "300".to_string())]), vis: Visibility::default() });
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        d.add_family(leaf, vec![("t".into(), BTreeMap::new())]).unwrap();
        let (fid, types) = d.add_family(frame, vec![("t".into(), BTreeMap::new())]).unwrap();
        let inst = d.place_on_wall(types[0], w, Point::new(3000., 0.)).unwrap();
        let solved = d.instance_solved(inst).unwrap();
        let s = solved.as_ref().as_ref().unwrap();
        assert!(s.forms.iter().any(|f| f.owner.is_some()), "the component is in the placed instance");
        assert!(crate::symbol::plan_symbol_at(&d, inst, crate::view::DetailLevel::Medium, Some(1200.0)).is_some());
        // the nested type's values follow an edit to the nested family
        let narrow = |d: &Document| tri_box(d.instance_solved(inst).unwrap().as_ref().as_ref().unwrap().forms.iter().find(|f| f.owner.is_some() && !f.void).unwrap());
        let b = narrow(&d);
        assert!((b[1][0] - b[0][0] - 300.0).abs() < 1e-6, "{b:?}");
        // a file round trip keeps the component
        let file = d.export_family(fid).unwrap();
        assert_eq!(file.family.body.nested.len(), 1);
        let json = serde_json::to_string(&file.family.body).unwrap();
        let back: crate::famgeom::FamilyBody = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nested, file.family.body.nested);
        // an older file that stored a component with only the first few fields still opens
        let old: Nested = serde_json::from_str(r#"{"id":5,"family":"Leaf","origin":[{"off":0.0},{"off":0.0},{"off":0.0}]}"#).unwrap();
        assert!(old.type_param.is_none() && old.work_plane.is_none() && !old.flip && !old.mirror && old.name.is_empty());
    }

    /// A free standing desk: a 1200 x 600 top 750 high.
    fn desk_family() -> Family {
        let mut b = FamilyBody::template();
        let rect = vec![Loop::rect(Coord::at(-600.0), Coord::at(600.0), Coord::at(-300.0), Coord::at(300.0))];
        b.add_form(form(FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_LEVEL, start: "0".into(), end: "750".into(), profile: rect }, false, ""));
        Family { name: "Desk".into(), category: FamilyCategory::Furniture, host: HostKind::None, plan_symbol: PlanSymbol::None, params: vec![], built_in: false, catalog: vec![], settings: Default::default(), source_path: None, loaded_at: None, body: b }
    }

    #[test]
    fn a_free_standing_family_places_draws_turns_mirrors_and_moves_up() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let (_, types) = d.add_family(desk_family(), vec![("Standard".into(), BTreeMap::new())]).unwrap();
        let id = d.place_free(types[0], lvl, Point::new(2000.0, 1000.0), 90.0, 0.0).unwrap();
        // plan: cut at 1200 is above the desk, so it is seen; the footprint is the desk turned a quarter
        let sym = crate::symbol::plan_symbol_at(&d, id, crate::view::DetailLevel::Medium, None).unwrap();
        let xs: Vec<f64> = sym.footprint.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = sym.footprint.iter().map(|p| p.y).collect();
        let span = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min);
        assert!((span(&xs) - 600.0).abs() < 1e-6 && (span(&ys) - 1200.0).abs() < 1e-6, "{:?}", sym.footprint);
        assert!(sym.lines.is_empty(), "no jamb lines without a wall");
        assert!(!sym.parts.is_empty());
        let f = d.instance_frame(id).unwrap();
        assert!(f.free && (f.z1 - f.z0 - 750.0).abs() < 1e-6);
        let (cat, solid, _) = crate::mesh::instance_meshes(&d, id).unwrap();
        assert_eq!(cat, FamilyCategory::Furniture);
        assert!(!solid.is_empty());
        // mirror across a vertical line through x = 0: it lands at x = -2000, turned to 90 and flipped
        d.transform_elements(&[id], crate::edit::Xform::Mirror { ax: 0.0, ay: 0.0, bx: 0.0, by: 1.0 }, Default::default()).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(id) else { panic!() };
        assert!((i.location.x + 2000.0).abs() < 1e-6 && i.flip_facing);
        assert!((i.rotation_deg - 90.0).abs() < 1e-6, "{}", i.rotation_deg);
        // up and down in an elevation: Elevation from Level
        d.move_vertical(&[id], 450.0).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(id) else { panic!() };
        assert!((i.offset_mm - 450.0).abs() < 1e-6);
        assert!((d.instance_frame(id).unwrap().z0 - 450.0).abs() < 1e-6);
        // a wall-hosted family is placed on a wall, not free
        let (_, door) = d.add_family(hatch_family(), vec![("t".into(), BTreeMap::new())]).unwrap();
        assert!(d.place_free(door[0], lvl, Point::new(0.0, 0.0), 0.0, 0.0).unwrap_err().contains("walls"));
        assert!(d.patch_instance(id, &json!({ "type_id": door[0] })).is_err(), "a free desk cannot become a door");
        // elevations draw it
        let south = d.views().into_iter().filter(|(_, v)| v.view_kind == crate::view::ViewKind::Elevation).map(|(vid, _)| vid).collect::<Vec<_>>();
        assert!(south.iter().any(|vid| d.elevation_data(*vid).unwrap().items.iter().any(|it| it.id == id && it.category == "Furniture")));
    }

    #[test]
    fn a_family_nested_inside_itself_is_stopped() {
        let mut a = hatch_family();
        a.name = "A".into();
        let mut b = hatch_family();
        b.name = "B".into();
        let mk = |id: u32, fam: &str| Nested { id, family: fam.into(), type_name: String::new(), type_param: None, name: String::new(), origin: [Coord::at(0.0), Coord::at(0.0), Coord::at(0.0)], rotation_deg: 0.0, work_plane: None, flip: false, mirror: false, params: BTreeMap::new(), vis: Visibility::default() };
        let (ida, idb) = (a.body.take_id(), b.body.take_id());
        a.body.nested.push(mk(ida, "B"));
        b.body.nested.push(mk(idb, "A"));
        let mut d = Document::new_project();
        let (fa, _) = d.add_family(a, vec![("t".into(), BTreeMap::new())]).unwrap();
        d.add_family(b, vec![("t".into(), BTreeMap::new())]).unwrap();
        let s = d.family_preview(fa, None, &BTreeMap::new()).unwrap();
        assert!(s.warnings.iter().any(|w| w.contains("inside itself")), "{:?}", s.warnings);
    }

    #[test]
    fn a_lookup_table_sets_a_parameter_and_follows_the_type() {
        let mut fam = hatch_family();
        let p = |name: &str, default| ParamDef { name: name.into(), param_kind: ParamKind::Number, instance: false, group: "Dimensions".into(), formula: None, reporting: false, shared_guid: None, choices: vec![], discipline: "common".into(), tooltip: String::new(), locked: false, default };
        fam.params.push(p("Pick", ParamValue::Number(2.0)));
        fam.params.iter_mut().find(|q| q.name == "Width").unwrap().formula = Some("size_lookup(Sizes, \"Width\", 800, [Pick])".into());
        fam.body.lookup_tables.push(openarch_params::lookup::LookupTable::from_csv("Sizes", "Name,Pick##number,Width##length##meters\nA,1,0.7\nB,2,1.0\nC,3,1.2\n").unwrap());
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let w = d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(6000., 0.), 2700.).unwrap();
        let (_, types) = d.add_family(fam, vec![("t".into(), BTreeMap::new())]).unwrap();
        let id = d.place_on_wall(types[0], w, Point::new(3000., 0.)).unwrap();
        let width = |d: &Document| { let Some(Element::FamilyInstance(i)) = d.get(id) else { panic!() }; d.instance_opening_size(i).0 };
        assert_eq!(width(&d), 1000.0);
        d.patch_family_type(types[0], &json!({ "values": { "Pick": 3.0 } })).unwrap();
        assert_eq!(width(&d), 1200.0);
        d.patch_family_type(types[0], &json!({ "values": { "Pick": 9.0 } })).unwrap();
        assert_eq!(width(&d), 800.0, "nothing matches: the default");
        // the tables are stored with the family and survive a file round trip
        let (file, _) = d.export_family(d.family_type(types[0]).unwrap().family).map(|f| (f, ())).unwrap();
        assert_eq!(file.family.body.lookup_tables.len(), 1);
        let old: crate::famgeom::FamilyBody = serde_json::from_str(r#"{"planes":[]}"#).unwrap();
        assert!(old.lookup_tables.is_empty());
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
        fam.params.push(ParamDef { name: "Clear Width".into(), param_kind: ParamKind::Length, instance: false, group: "Dimensions".into(), formula: None, reporting: true, shared_guid: None, choices: vec![], discipline: "common".into(), tooltip: String::new(), locked: false, default: ParamValue::Number(0.0) });
        fam.body.dims.push(DimDef { a: left, b: right, reports_to: Some("Clear Width".into()), ..Default::default() });
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
    fn every_category_maps_to_an_ifc43_class() {
        // Entities checked against the IFC4.3 schema listing.
        const KNOWN: &[&str] = &[
            "IfcBuildingElementProxy", "IfcDoor", "IfcWindow", "IfcFurniture", "IfcSystemFurnitureElement",
            "IfcSanitaryTerminal", "IfcLightFixture", "IfcElectricAppliance", "IfcElectricDistributionBoard",
            "IfcUnitaryEquipment", "IfcColumn", "IfcBeam", "IfcFooting", "IfcGeographicElement", "IfcSensor",
            "IfcSwitchingDevice", "IfcCommunicationsAppliance", "IfcController", "IfcAirTerminal",
            "IfcFireSuppressionTerminal", "IfcSlab",
        ];
        for c in FamilyCategory::all() {
            assert!(KNOWN.contains(&c.ifc_class()), "{:?} maps to {}", c, c.ifc_class());
        }
        assert_eq!(FamilyCategory::Hardscape.ifc_predefined(), "PAVING");
        assert_eq!(FamilyCategory::DataDevices.ifc_class(), "IfcCommunicationsAppliance");
        assert_eq!(FamilyCategory::FurnitureSystems.ifc_class(), "IfcSystemFurnitureElement");
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

/// What loading a family file does when the project already has a family of that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// Refuse, so nothing is replaced silently.
    #[default]
    Refuse,
    /// Replace the project copy, including the values of types the file also has.
    Overwrite,
    /// Replace the project copy but keep the project's own type values.
    KeepValues,
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
            discipline: "common".into(),
            tooltip: String::new(),
            locked: false,
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
        let family = Family { name: name.into(), category, host, plan_symbol: PlanSymbol::None, params, built_in: false, catalog: vec![], settings: Default::default(), source_path: None, loaded_at: None, body };
        let (id, _) = self.add_family(family, vec![("Type 1".into(), Default::default())])?;
        Ok(id)
    }

    /// Replace a family's parameters and/or body in one undo step. Parameter names must be unique and
    /// every formula must read; the body must solve for the family's first type.
    pub fn set_family_def(&mut self, family: Id, params: Option<Vec<crate::family::ParamDef>>, body: Option<crate::famgeom::FamilyBody>) -> Result<(), String> {
        self.set_family_def_full(family, params, body, None, &[])
    }

    /// Replace a family's parameters, body and settings. `renames` are parameters renamed in the editor
    /// (old name, new name): the stored types and the placed instances keep their values under the new name.
    pub fn set_family_def_full(&mut self, family: Id, params: Option<Vec<crate::family::ParamDef>>, body: Option<crate::famgeom::FamilyBody>, settings: Option<crate::family::FamilySettings>, renames: &[(String, String)]) -> Result<(), String> {
        let Some(Element::Family(f)) = self.get(family) else { return Err("no such family".into()) };
        let mut f = f.clone();
        if let Some(st) = settings {
            f.settings = st;
        }
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
        for d in &f.body.dims {
            for n in d.label.iter().chain(d.seg_labels.iter()).filter(|n| !n.trim().is_empty()) {
                match f.param(n) {
                    Some(p) if !p.reporting => {}
                    Some(_) => return Err(format!("'{n}' is a reporting parameter, so it cannot drive a dimension")),
                    None => return Err(format!("a dimension is labelled '{n}', which is not a parameter")),
                }
            }
        }
        // values stored under a parameter's old name move to the new one
        let mut moved: Vec<(Id, Element)> = Vec::new();
        if !renames.is_empty() {
            let rename_keys = |m: &mut std::collections::BTreeMap<String, ParamValue>| -> bool {
                let mut changed = false;
                for (old, new) in renames {
                    if let Some(v) = m.remove(old) {
                        m.insert(new.clone(), v);
                        changed = true;
                    }
                }
                changed
            };
            for (tid, t) in self.family_types() {
                if t.family == family {
                    let mut t = t.clone();
                    if rename_keys(&mut t.values) {
                        moved.push((tid, Element::FamilyType(t)));
                    }
                }
            }
            for (iid, i) in self.instances() {
                if self.family_type(i.type_id).map_or(false, |t| t.family == family) {
                    let mut i = i.clone();
                    if rename_keys(&mut i.values) {
                        moved.push((iid, Element::FamilyInstance(i)));
                    }
                }
            }
        }
        self.transact("Edit family", |tx| {
            tx.update(family, Element::Family(f))?;
            for (id, e) in moved {
                tx.update(id, e)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Every parameter's value for one type, as the Family Types dialog shows them: the type's own
    /// value or the default, a formula worked out, a reporting parameter read from its dimension.
    pub fn family_param_values(&self, family: Id, type_id: Option<Id>, overrides: &std::collections::BTreeMap<String, ParamValue>) -> Result<Vec<(String, ParamValue)>, String> {
        let fam = self.family(family).ok_or("no such family")?;
        let mut t = match type_id {
            Some(t) => self.family_type(t).ok_or("no such type")?.clone(),
            None => self.family_types().into_iter().find(|(_, t)| t.family == family).map(|(_, t)| t.clone()).unwrap_or(crate::family::FamilyType { family, name: String::new(), values: Default::default(), globals: Default::default() }),
        };
        for (k, v) in overrides {
            t.values.insert(k.clone(), v.clone());
        }
        let reports = self.family_preview(family, type_id, overrides).map(|s| s.reports).unwrap_or_default();
        let mut out = Vec::new();
        for p in &fam.params {
            if let Some((_, v)) = reports.iter().find(|(n, _)| *n == p.name).filter(|_| p.formula.is_none()) {
                out.push((p.name.clone(), ParamValue::Number(*v)));
                continue;
            }
            // a formula that does not work out is reported, not left blank
            if let Some(v) = self.param_try(&t, None, &p.name, 0)? {
                out.push((p.name.clone(), v));
            }
        }
        Ok(out)
    }

    /// The family as it stands for one type, with `overrides` laid over the type's values (the
    /// "flex"). Host values take plain defaults: a 100 mm wall with its core centred.
    pub fn family_preview(&self, family: Id, type_id: Option<Id>, overrides: &std::collections::BTreeMap<String, ParamValue>) -> Result<Solved, String> {
        let fam = self.family(family).ok_or("no such family")?;
        let base = match type_id {
            Some(t) => self.family_type(t).ok_or("no such type")?.clone(),
            None => self.family_types().into_iter().find(|(_, t)| t.family == family).map(|(_, t)| t.clone()).unwrap_or(crate::family::FamilyType { family, name: String::new(), values: Default::default(), globals: Default::default() }),
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
        self.solve_family_body(&fam.name, &fam.body, &mut env, &mut Vec::new())
    }

    /// Run an edit on a copy of a family body with the parameter values of one type (and
    /// `overrides`) at hand, for edits that need to know where the planes stand.
    pub fn family_body_edit(&self, family: Id, type_id: Option<Id>, overrides: &std::collections::BTreeMap<String, ParamValue>, mut body: crate::famgeom::FamilyBody, edit: impl FnOnce(&mut crate::famgeom::FamilyBody, &mut crate::famgeom::Env) -> Result<(), String>) -> Result<crate::famgeom::FamilyBody, String> {
        self.family(family).ok_or("no such family")?;
        let mut t = match type_id {
            Some(t) => self.family_type(t).ok_or("no such type")?.clone(),
            None => self.family_types().into_iter().find(|(_, t)| t.family == family).map(|(_, t)| t.clone()).unwrap_or(crate::family::FamilyType { family, name: String::new(), values: Default::default(), globals: Default::default() }),
        };
        for (k, v) in overrides {
            t.values.insert(k.clone(), v.clone());
        }
        let reporting: Vec<String> = body.dims.iter().filter_map(|d| d.reports_to.clone()).collect();
        let mut env = |n: &str| -> Result<Value, EvalError> {
            match n {
                "Host Thickness" => return Ok(Value::Num(100.0)),
                "Host Core Thickness" => return Ok(Value::Num(100.0)),
                "Host Core Offset" => return Ok(Value::Num(0.0)),
                _ if reporting.iter().any(|r| r == n) => return Err(EvalError::UnknownName(n.to_string())),
                _ => {}
            }
            match self.type_param(&t, n) {
                Some(ParamValue::Number(x)) => Ok(Value::Num(x)),
                Some(ParamValue::Bool(b)) => Ok(Value::Bool(b)),
                Some(ParamValue::Text(s)) => Ok(Value::Text(s)),
                None => Err(EvalError::UnknownName(n.to_string())),
            }
        };
        edit(&mut body, &mut env)?;
        Ok(body)
    }

    pub fn export_family(&self, family: Id) -> Result<FamilyFile, String> {
        self.export_family_with(family, None, None, None, &Default::default())
    }

    /// The file for a family, with the editor's unsaved parameters, body, settings and type values laid
    /// over what the project holds (so Save writes what is on screen without touching the project copy).
    /// The file never carries the project's remembered path.
    pub fn export_family_with(
        &self,
        family: Id,
        params: Option<Vec<crate::family::ParamDef>>,
        body: Option<crate::famgeom::FamilyBody>,
        settings: Option<crate::family::FamilySettings>,
        values: &std::collections::BTreeMap<Id, std::collections::BTreeMap<String, ParamValue>>,
    ) -> Result<FamilyFile, String> {
        let fam = self.family(family).ok_or("no such family")?;
        let mut f = fam.clone();
        f.built_in = false;
        f.source_path = None;
        f.loaded_at = None;
        if let Some(p) = params {
            f.params = p;
        }
        if let Some(b) = body {
            f.body = b;
        }
        if let Some(s) = settings {
            f.settings = s;
        }
        let types = self
            .family_types()
            .into_iter()
            .filter(|(_, t)| t.family == family)
            .map(|(tid, t)| {
                let mut v = t.values.clone();
                for (k, x) in values.get(&tid).into_iter().flatten() {
                    v.insert(k.clone(), x.clone());
                }
                FamilyFileType { name: t.name.clone(), values: v }
            })
            .collect();
        Ok(FamilyFile { openarch_family: 1, family: f, types })
    }

    /// Record where a family's file sits. One undo step; nothing else about the family changes.
    pub fn set_family_source(&mut self, family: Id, path: Option<String>, loaded_at: Option<String>) -> Result<(), String> {
        let Some(Element::Family(f)) = self.get(family) else { return Err("no such family".into()) };
        if f.source_path == path && f.loaded_at == loaded_at {
            return Ok(());
        }
        let mut f = f.clone();
        f.source_path = path;
        f.loaded_at = loaded_at;
        self.transact("Family file location", |tx| tx.update(family, Element::Family(f)))
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
        Ok(self.import_family_with(file, ImportMode::Refuse, None)?.0)
    }

    /// Load a family file, replacing the project's copy when one of that name exists and `mode` allows it.
    /// `source` is the file's path and a note of when it was loaded; it is remembered on the project copy
    /// and nothing else is linked. Replacing keeps the placed instances: types of the same name take the
    /// file's values (`Overwrite`) or keep their own (`KeepValues`), types the file lacks stay, and the
    /// whole load is refused if a placed instance would no longer fit. One undo step.
    /// Returns the family and whether it replaced an existing copy.
    pub fn import_family_with(&mut self, file: FamilyFile, mode: ImportMode, source: Option<(String, Option<String>)>) -> Result<(Id, bool), String> {
        if file.openarch_family != 1 {
            return Err("this is not an OpenArch BIM family file".into());
        }
        let FamilyFile { family: mut nf, types, .. } = file;
        let types = if types.is_empty() { vec![FamilyFileType { name: "Type 1".into(), values: Default::default() }] } else { types };
        nf.built_in = false;
        let (path, at) = source.map_or((None, None), |(p, a)| (Some(p), a));
        nf.source_path = path;
        nf.loaded_at = at;
        let Some(existing) = self.families().into_iter().find(|(_, f)| f.name == nf.name).map(|(id, _)| id) else {
            let (id, _) = self.add_family(nf, types.into_iter().map(|t| (t.name, t.values)).collect())?;
            return Ok((id, false));
        };
        if mode == ImportMode::Refuse {
            return Err(format!("a family called '{}' already exists", nf.name));
        }
        for p in &nf.params {
            if let Some(src) = &p.formula {
                openarch_params::expr::parse(src).map_err(|e| format!("formula of '{}': {e:?}", p.name))?;
            }
        }
        let names: std::collections::BTreeSet<String> = nf.params.iter().map(|p| p.name.clone()).collect();
        let keep = |m: &std::collections::BTreeMap<String, ParamValue>| -> std::collections::BTreeMap<String, ParamValue> { m.iter().filter(|(k, _)| names.contains(*k)).map(|(k, v)| (k.clone(), v.clone())).collect() };
        let mut updates: Vec<(Id, Element)> = Vec::new();
        let mut adds: Vec<crate::family::FamilyType> = Vec::new();
        for (tid, t) in self.family_types().into_iter().filter(|(_, t)| t.family == existing) {
            let from_file = types.iter().find(|ft| ft.name == t.name);
            let values = match (mode, from_file) {
                (ImportMode::Overwrite, Some(ft)) => keep(&ft.values),
                _ => keep(&t.values),
            };
            updates.push((tid, Element::FamilyType(crate::family::FamilyType { family: existing, name: t.name.clone(), values, globals: t.globals.clone() })));
        }
        for ft in &types {
            if !self.family_types().iter().any(|(_, t)| t.family == existing && t.name == ft.name) {
                adds.push(crate::family::FamilyType { family: existing, name: ft.name.clone(), values: keep(&ft.values), globals: Default::default() });
            }
        }
        for (iid, i) in self.instances() {
            if self.family_type(i.type_id).map_or(false, |t| t.family == existing) {
                let mut i = i.clone();
                i.values = keep(&i.values);
                updates.push((iid, Element::FamilyInstance(i)));
            }
        }
        let mut probe = self.clone();
        probe.elements.insert(existing, Element::Family(nf.clone()));
        for (id, e) in &updates {
            probe.elements.insert(*id, e.clone());
        }
        for (iid, i) in probe.instances() {
            if probe.family_type(i.type_id).map_or(false, |t| t.family == existing) {
                probe.fits(i, Some(iid)).map_err(|e| format!("a placed instance would no longer fit the loaded family: {e}"))?;
            }
        }
        self.transact("Load family", |tx| {
            tx.update(existing, Element::Family(nf))?;
            for (id, e) in updates {
                tx.update(id, e)?;
            }
            for t in adds {
                tx.add(Element::FamilyType(t));
            }
            Ok(())
        })?;
        Ok((existing, true))
    }

}
