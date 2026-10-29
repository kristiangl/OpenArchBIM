//! Wall, floor and roof types (the system families): checking a layer assembly before it is
//! stored, the assembly's thermal values, and the Type Properties edits (change, duplicate, rename).
//!
//! An assembly is the type's layer list, exterior face first for walls and top face first for
//! floors and roofs. Layers flagged `core` sit between the two core boundaries and must be
//! contiguous.

use serde::{Deserialize, Serialize};

use crate::doc::{apply_patch, Document};
use crate::element::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SysKind {
    Wall,
    Floor,
    Roof,
}

impl SysKind {
    /// The system family the type belongs to, as Type Properties names it.
    pub fn family(self) -> &'static str {
        match self {
            SysKind::Wall => "Basic Wall",
            SysKind::Floor => "Floor",
            SysKind::Roof => "Basic Roof",
        }
    }

    /// The two faces the layer list runs between.
    pub fn sides(self) -> (&'static str, &'static str) {
        match self {
            SysKind::Wall => ("EXTERIOR SIDE", "INTERIOR SIDE"),
            _ => ("TOP", "BOTTOM"),
        }
    }

    fn element(self) -> &'static str {
        match self {
            SysKind::Wall => "wall",
            SysKind::Floor => "floor",
            SysKind::Roof => "roof",
        }
    }
}

/// Thermal and size values of an assembly, as Type Properties reports them (Analytical Properties).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssemblyValues {
    pub thickness_mm: f64,
    /// Thermal Resistance (R): the sum of thickness over conductivity, m²K/W. Air films are not included.
    pub r_m2k_w: Option<f64>,
    /// Heat Transfer Coefficient (U): 1 / R, W/(m²K).
    pub u_w_m2k: Option<f64>,
    /// Thermal Mass: the sum of density times specific heat times thickness, kJ/(m²K).
    pub thermal_mass_kj_m2k: Option<f64>,
    /// Materials with a thickness but no conductivity or no specific heat: the values above leave them out.
    pub missing_thermal: Vec<String>,
    /// The material of the layer marked Structural Material (or the first core Structure layer).
    pub structural_material: Option<String>,
}

/// Thermal values of a layer list. Void layers (cavities) carry no material and add nothing.
pub fn assembly_values(d: &Document, layers: &[Layer]) -> AssemblyValues {
    let (mut r, mut mass, mut any_r, mut any_mass) = (0.0, 0.0, false, false);
    let mut missing: Vec<String> = vec![];
    for l in layers.iter().filter(|l| !l.is_void && l.thickness_mm > 0.0) {
        let t = l.thickness_mm / 1000.0;
        let phys = d.material(&l.material).map(|m| &m.physical);
        let k = phys.and_then(|p| p.conductivity_w_mk).filter(|k| *k > 0.0);
        let rho_c = phys
            .and_then(|p| Some(p.density_kg_m3? * p.specific_heat_j_kgk?))
            .filter(|v| *v > 0.0);
        match k {
            Some(k) => {
                r += t / k;
                any_r = true;
            }
            None => missing.push(l.material.clone()),
        }
        match rho_c {
            Some(rc) => {
                mass += rc * t / 1000.0;
                any_mass = true;
            }
            None if k.is_some() => missing.push(l.material.clone()),
            None => {}
        }
    }
    missing.dedup();
    let structural = layers
        .iter()
        .find(|l| l.structural_material)
        .or_else(|| {
            layers
                .iter()
                .find(|l| l.core && !l.is_void && l.function == LayerFunction::Structure)
        })
        .map(|l| l.material.clone());
    AssemblyValues {
        thickness_mm: layers.iter().map(|l| l.thickness_mm).sum(),
        r_m2k_w: any_r.then_some(r),
        u_w_m2k: (any_r && r > 0.0).then(|| 1.0 / r),
        thermal_mass_kj_m2k: any_mass.then_some(mass),
        missing_thermal: missing,
        structural_material: structural,
    }
}

/// Check a layer list the way Edit Assembly does before OK. Membranes have no thickness, every
/// other layer has some; the core is one run of layers with at least one layer in it; at most one
/// layer is the Structural Material (in the core) and, for floors and roofs, at most one is Variable.
pub fn check_assembly(d: &Document, kind: SysKind, layers: &[Layer]) -> Result<(), String> {
    if layers.is_empty() {
        return Err("an assembly needs at least one layer".into());
    }
    for (i, l) in layers.iter().enumerate() {
        let n = i + 1;
        if !l.thickness_mm.is_finite() || l.thickness_mm < 0.0 {
            return Err(format!("layer {n}: thickness must be zero or more"));
        }
        if l.function == LayerFunction::Membrane {
            if l.thickness_mm != 0.0 {
                return Err(format!(
                    "layer {n}: a membrane layer must have zero thickness"
                ));
            }
        } else if l.thickness_mm < 0.1 {
            return Err(format!(
                "layer {n}: only membrane layers can have zero thickness"
            ));
        }
        if !l.is_void && !l.material.is_empty() && d.material(&l.material).is_none() {
            return Err(format!("layer {n}: no material named '{}'", l.material));
        }
        if l.structural_material && (!l.core || l.is_void) {
            return Err(format!(
                "layer {n}: the structural material must be a material layer inside the core"
            ));
        }
        if l.variable && kind == SysKind::Wall {
            return Err("wall layers cannot be variable".into());
        }
        if l.variable && l.function == LayerFunction::Membrane {
            return Err(format!(
                "layer {n}: a membrane cannot be the variable layer"
            ));
        }
        if let Some(p) = l.priority {
            if !(1..=5).contains(&p) {
                return Err(format!("layer {n}: priority runs from 1 to 5"));
            }
        }
    }
    let total: f64 = layers.iter().map(|l| l.thickness_mm).sum();
    if total < 1.0 {
        return Err(format!("a {} type needs some thickness", kind.element()));
    }
    let core: Vec<usize> = layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.core)
        .map(|(i, _)| i)
        .collect();
    if core.is_empty() || core.iter().all(|&i| layers[i].thickness_mm == 0.0) {
        return Err("the core needs at least one layer with a thickness".into());
    }
    if core.windows(2).any(|w| w[1] != w[0] + 1) {
        return Err("core layers must sit together between the core boundaries".into());
    }
    if layers.iter().filter(|l| l.structural_material).count() > 1 {
        return Err("only one layer can be the structural material".into());
    }
    if layers.iter().filter(|l| l.variable).count() > 1 {
        return Err("only one layer can be variable".into());
    }
    Ok(())
}

impl Document {
    /// Which system family a type belongs to, if it is a wall, floor or roof type.
    pub fn sys_kind(&self, id: Id) -> Option<SysKind> {
        match self.get(id)? {
            Element::WallType(_) => Some(SysKind::Wall),
            Element::FloorType(_) => Some(SysKind::Floor),
            Element::RoofType(_) => Some(SysKind::Roof),
            _ => None,
        }
    }

    /// The layers of a wall, floor or roof type.
    pub fn sys_layers(&self, id: Id) -> Option<&[Layer]> {
        match self.get(id)? {
            Element::WallType(t) => Some(&t.layers),
            Element::FloorType(t) => Some(&t.layers),
            Element::RoofType(t) => Some(&t.layers),
            _ => None,
        }
    }

    fn sys_name_taken(&self, kind: SysKind, name: &str, except: Option<Id>) -> bool {
        self.iter().any(|(i, e)| {
            Some(i) != except
                && match (kind, e) {
                    (SysKind::Wall, Element::WallType(t)) => t.name == name,
                    (SysKind::Floor, Element::FloorType(t)) => t.name == name,
                    (SysKind::Roof, Element::RoofType(t)) => t.name == name,
                    _ => false,
                }
        })
    }

    /// Change a wall, floor or roof type with a JSON merge patch (layers, wrapping, function,
    /// coarse fill, identity data...). The layer list is replaced whole and checked first.
    pub fn patch_system_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let kind = self.sys_kind(id).ok_or("not a wall, floor or roof type")?;
        let old = self.get(id).cloned().ok_or("no such type")?;
        let new = match &old {
            Element::WallType(t) => {
                let n: WallType = apply_patch(t, patch)?;
                if n.wrap_ends == Wrap::Both {
                    return Err("wall ends wrap on the exterior or the interior, not both".into());
                }
                if !(n.sample_height_mm.is_finite() && n.sample_height_mm >= 100.0) {
                    return Err("the sample height must be at least 100 mm".into());
                }
                let mut n = n;
                // a curtain type's one layer follows its glazing zone
                if let Some(c) = &n.curtain {
                    for r in [&c.u, &c.v] {
                        if !(r.spacing_mm.is_finite() && r.spacing_mm >= 10.0) {
                            return Err("a grid spacing must be at least 10 mm".into());
                        }
                        if r.number > 500 || !r.offset_mm.is_finite() {
                            return Err("a grid takes at most 500 lines".into());
                        }
                    }
                    if !(c.panel.thickness_mm.is_finite() && c.panel.thickness_mm >= 0.0) {
                        return Err("a panel cannot be less than nothing thick".into());
                    }
                    n.layers = vec![c.layer()];
                }
                Element::WallType(n)
            }
            Element::FloorType(t) => Element::FloorType(apply_patch(t, patch)?),
            Element::RoofType(t) => Element::RoofType(apply_patch(t, patch)?),
            _ => unreachable!(),
        };
        if new == old {
            return Ok(());
        }
        let (name, layers) = match &new {
            Element::WallType(t) => (&t.name, &t.layers),
            Element::FloorType(t) => (&t.name, &t.layers),
            Element::RoofType(t) => (&t.name, &t.layers),
            _ => unreachable!(),
        };
        if name.trim().is_empty() {
            return Err("a type needs a name".into());
        }
        if self.sys_name_taken(kind, name, Some(id)) {
            return Err(format!(
                "a {} type named '{name}' already exists",
                kind.element()
            ));
        }
        check_assembly(self, kind, layers)?;
        let label = match kind {
            SysKind::Wall => "Change wall type",
            SysKind::Floor => "Change floor type",
            SysKind::Roof => "Change roof type",
        };
        self.transact(label, |tx| tx.update(id, new))
    }

    /// A copy of a wall, floor or roof type under a new name (Duplicate in Type Properties).
    pub fn duplicate_system_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let kind = self.sys_kind(id).ok_or("not a wall, floor or roof type")?;
        let name = name.trim();
        if name.is_empty() {
            return Err("a type needs a name".into());
        }
        if self.sys_name_taken(kind, name, None) {
            return Err(format!(
                "a {} type named '{name}' already exists",
                kind.element()
            ));
        }
        let e = match self.get(id).cloned().unwrap() {
            Element::WallType(t) => Element::WallType(WallType {
                name: name.into(),
                ..t
            }),
            Element::FloorType(t) => Element::FloorType(FloorType {
                name: name.into(),
                ..t
            }),
            Element::RoofType(t) => Element::RoofType(RoofType {
                name: name.into(),
                ..t
            }),
            _ => unreachable!(),
        };
        self.transact("Duplicate type", |tx| Ok(tx.add(e)))
    }

    /// Rename any type (Rename in Type Properties). Names are unique among types of the same kind,
    /// and family types only within their family.
    pub fn rename_type(&mut self, id: Id, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a type needs a name".into());
        }
        let e = self.get(id).cloned().ok_or("no such type")?;
        let kind = e.kind();
        let family = match &e {
            Element::FamilyType(t) => Some(t.family),
            _ => None,
        };
        let is_type = matches!(
            e,
            Element::WallType(_)
                | Element::FloorType(_)
                | Element::RoofType(_)
                | Element::FamilyType(_)
                | Element::GridType(_)
                | Element::LevelType(_)
                | Element::DimensionType(_)
                | Element::SpotType(_)
                | Element::ViewType(_)
                | Element::FilledRegionType(_)
                | Element::TextNoteType(_)
        );
        if !is_type {
            return Err("not a type".into());
        }
        let mut v = serde_json::to_value(&e).map_err(|e| e.to_string())?;
        if v["name"].as_str() == Some(name) {
            return Ok(());
        }
        let taken = self.iter().any(|(i, o)| {
            i != id
                && o.kind() == kind
                && serde_json::to_value(o)
                    .ok()
                    .and_then(|v| v["name"].as_str().map(|s| s == name))
                    .unwrap_or(false)
                && match (family, o) {
                    (Some(f), Element::FamilyType(t)) => t.family == f,
                    _ => true,
                }
        });
        if taken {
            return Err(format!("a type named '{name}' already exists"));
        }
        v["name"] = serde_json::json!(name);
        let renamed: Element = serde_json::from_value(v).map_err(|e| e.to_string())?;
        self.transact("Rename type", |tx| tx.update(id, renamed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn first(d: &Document, kind: SysKind) -> Id {
        d.iter()
            .find(|(i, _)| d.sys_kind(*i) == Some(kind))
            .map(|(i, _)| i)
            .unwrap()
    }

    #[test]
    fn an_assembly_is_checked_before_it_is_stored() {
        let mut d = Document::new_project();
        let wt = first(&d, SysKind::Wall);
        let core = |m: &str, t: f64| Layer::new(m, t, LayerFunction::Structure, true);
        let skin = |m: &str, t: f64| Layer::new(m, t, LayerFunction::Finish1, false);
        let membrane = Layer::new("", 5.0, LayerFunction::Membrane, false);
        let bad = |d: &mut Document, layers: Vec<Layer>| {
            d.patch_system_type(wt, &json!({ "layers": layers }))
                .unwrap_err()
        };
        assert!(bad(&mut d, vec![]).contains("at least one layer"));
        assert!(
            bad(&mut d, vec![membrane.clone(), core("Concrete", 200.0)]).contains("zero thickness")
        );
        assert!(bad(
            &mut d,
            vec![skin("Plasterboard", 0.0), core("Concrete", 200.0)]
        )
        .contains("only membrane"));
        assert!(bad(&mut d, vec![skin("Plasterboard", 13.0)]).contains("core"));
        assert!(bad(
            &mut d,
            vec![
                core("Concrete", 100.0),
                skin("Plasterboard", 13.0),
                core("Concrete", 100.0)
            ]
        )
        .contains("together"));
        assert!(bad(&mut d, vec![core("Unobtainium", 100.0)]).contains("no material"));
        let mut sm = skin("Plasterboard", 13.0);
        sm.structural_material = true;
        assert!(bad(&mut d, vec![sm, core("Concrete", 100.0)]).contains("inside the core"));
        let mut var = core("Concrete", 100.0);
        var.variable = true;
        assert!(bad(&mut d, vec![var.clone()]).contains("cannot be variable"));
        assert!(d
            .patch_system_type(wt, &json!({ "wrap_ends": "both" }))
            .unwrap_err()
            .contains("not both"));
        // a membrane with no thickness is fine, and the wall takes the new layers
        let mut m0 = membrane;
        m0.thickness_mm = 0.0;
        let good = vec![skin("Plasterboard", 13.0), m0, core("Concrete", 150.0)];
        d.patch_system_type(
            wt,
            &json!({ "layers": good, "wrap_inserts": "both", "function": "retaining" }),
        )
        .unwrap();
        let t = d.wall_type(wt).unwrap();
        assert_eq!(
            (t.thickness_mm(), t.wrap_inserts, t.function),
            (163.0, Wrap::Both, WallFunction::Retaining)
        );
        // a floor may have one variable layer
        let ft = first(&d, SysKind::Floor);
        d.patch_system_type(ft, &json!({ "layers": [var.clone()] }))
            .unwrap();
        let mut var2 = var.clone();
        var2.core = true;
        assert!(d
            .patch_system_type(ft, &json!({ "layers": [var, var2] }))
            .unwrap_err()
            .contains("only one layer can be variable"));
    }

    #[test]
    fn thermal_values_add_up_over_the_layers() {
        let d = Document::new_project();
        // 13 plasterboard (k 0.25, 700 kg/m³, 1090 J/kgK) + 200 concrete (k 1.6, 2400, 880)
        let layers = vec![
            Layer::new("Plasterboard", 13.0, LayerFunction::Finish1, false),
            Layer::new("Concrete", 200.0, LayerFunction::Structure, true),
            Layer {
                is_void: true,
                ..Layer::new("Cavity", 40.0, LayerFunction::ThermalAir, false)
            },
        ];
        let v = assembly_values(&d, &layers);
        let r = 0.013 / 0.25 + 0.2 / 1.6;
        assert!((v.r_m2k_w.unwrap() - r).abs() < 1e-9);
        assert!((v.u_w_m2k.unwrap() - 1.0 / r).abs() < 1e-9);
        let mass = (700.0 * 1090.0 * 0.013 + 2400.0 * 880.0 * 0.2) / 1000.0;
        assert!((v.thermal_mass_kj_m2k.unwrap() - mass).abs() < 1e-9);
        assert_eq!(v.thickness_mm, 253.0);
        assert_eq!(v.structural_material.as_deref(), Some("Concrete"));
        assert!(v.missing_thermal.is_empty());
    }

    #[test]
    fn a_layer_priority_overrides_the_function_and_is_checked() {
        let mut d = Document::new_project();
        let mut l = Layer::new("Plasterboard", 13.0, LayerFunction::Finish2, false);
        assert_eq!(l.join_priority(), 5);
        l.priority = Some(2);
        assert_eq!(l.join_priority(), 2);
        let mut m = Layer::new("", 0.0, LayerFunction::Membrane, false);
        m.priority = Some(1);
        assert_eq!(m.join_priority(), crate::layers::priority(LayerFunction::Membrane));
        l.priority = Some(7);
        let wt = first(&d, SysKind::Wall);
        let err = d.patch_system_type(wt, &json!({ "layers": [l, Layer::new("Concrete", 100.0, LayerFunction::Structure, true)] })).unwrap_err();
        assert!(err.contains("1 to 5"), "{err}");
    }

    #[test]
    fn types_duplicate_and_rename_with_unique_names() {
        let mut d = Document::new_project();
        let wt = first(&d, SysKind::Wall);
        let name = d.wall_type(wt).unwrap().name.clone();
        assert!(d.duplicate_system_type(wt, &name).is_err());
        let copy = d.duplicate_system_type(wt, "Copy").unwrap();
        assert_eq!(
            d.wall_type(copy).unwrap().layers,
            d.wall_type(wt).unwrap().layers
        );
        assert!(d.rename_type(copy, &name).is_err());
        d.rename_type(copy, "Renamed").unwrap();
        assert_eq!(d.wall_type(copy).unwrap().name, "Renamed");
        // a floor type may share a wall type's name
        let ft = first(&d, SysKind::Floor);
        d.rename_type(ft, "Renamed").unwrap();
        // family types are unique only within their family
        let (tid, fam) = d
            .family_types()
            .iter()
            .map(|(i, t)| (*i, t.family))
            .next()
            .unwrap();
        let other = d
            .family_types()
            .iter()
            .find(|(_, t)| t.family != fam)
            .map(|(_, t)| t.name.clone())
            .unwrap();
        d.rename_type(tid, &other).unwrap();
    }
}
