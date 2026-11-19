//! Revit family files (.rfa) read into OpenArch family files, through rvt-rs (THIRD-PARTY-NOTICES.md).
//!
//! What comes across: the family's name, category and host, each type with its parameter values (the file's
//! PartAtom), and each solid form (extrusion, blend, revolve, sweep, swept blend, combined geometry) as the mesh
//! Revit saved to display it, in mm, one form per material. Voids are already cut out of those meshes.
//!
//! What does not: the forms as sketches, reference planes, dimensions, formulas, nested families and symbolic
//! lines. So the geometry is the one the file was saved with and does not flex when a type's values change.

use std::collections::BTreeMap;
use std::path::Path;

use openarch_model::famgeom::{FamilyBody, Form, FormShape, Visibility};
use openarch_model::family::{Family, FamilyCategory, FamilySettings, HostKind, ParamDef, ParamKind, ParamValue, PlanSymbol};
use openarch_model::faminst::{FamilyFile, FamilyFileType};
use rvt::RevitFile;
use rvt::research::native_form_geometry;
use rvt::research::native_parameters::{self, GraphLimits};
use rvt::research::native_saved_mesh;
use serde_json::Value;

/// Revit's internal length unit is the foot.
const FT: f64 = 304.8;
const LIMITS: GraphLimits = GraphLimits { max_values: 4_000_000, max_objects: 4_000_000, max_depth: 160 };
/// The records whose saved meshes are a family's solids.
const FORMS: [&str; 6] = ["ExtrusionElem", "BlendElem", "RevolutionElem", "SweepElem", "SweptBlendElem", "FamilyGeomCombination"];

/// Which way up a face-based family goes. A face-based family is drawn with the host face as its own XY plane,
/// so whether it stands against a wall or sits on a floor is not in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FaceAs {
    /// The face is a wall: the family's z runs out of the wall and its y up (a wall-faced toilet, a basin).
    #[default]
    Wall,
    /// The face is a floor: the family's z runs up.
    Floor,
}

/// What a family file is, read from its PartAtom only (quick).
#[derive(Debug, Clone)]
pub struct RfaInfo {
    pub name: String,
    pub category: String,
    /// Revit's host: "Face", "Wall", "Floor", "Ceiling", "Roof" or "" for none.
    pub host: String,
    pub types: Vec<String>,
    pub revit_version: Option<String>,
}

/// A family file read, with what was left out or guessed.
#[derive(Debug, Clone)]
pub struct RfaFamily {
    pub file: FamilyFile,
    pub info: RfaInfo,
    pub notes: Vec<String>,
}

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

fn open(path: &Path) -> Result<RevitFile, String> {
    RevitFile::open(path).map_err(|e| format!("{} is not a Revit file this can read: {}", path.display(), err(e)))
}

/// Name, category, host and types of a family file.
pub fn info(path: &Path) -> Result<RfaInfo, String> {
    let mut rf = open(path)?;
    let pa = part_atom(&mut rf, path)?;
    Ok(info_of(&pa, &mut rf))
}

fn info_of(pa: &PartAtom, rf: &mut RevitFile) -> RfaInfo {
    RfaInfo {
        name: pa.name.clone(),
        category: pa.category.clone(),
        host: pa.host.clone(),
        types: pa.types.iter().map(|t| t.name.clone()).collect(),
        revit_version: rf.basic_file_info().ok().and_then(|b| serde_json::to_value(&b).ok()).and_then(|v| v.get("version").and_then(|x| x.as_str().map(String::from).or_else(|| x.as_u64().map(|n| n.to_string())))),
    }
}

/// Read a family file into an OpenArch family file.
pub fn read(path: &Path, face: FaceAs) -> Result<RfaFamily, String> {
    let mut rf = open(path)?;
    let pa = part_atom(&mut rf, path)?;
    let info = info_of(&pa, &mut rf);
    // said every time: what an imported Revit family is and is not
    let mut notes = vec!["Revit family import is experimental and may give unexpected results. It does not work for parametric families: the family comes in at the size it was saved, and its types do not change its shape.".to_string()];

    // the category, where OpenArch has it; doors and windows need their own templates to sit in walls
    let mut category = FamilyCategory::all().iter().copied().find(|c| c.label().eq_ignore_ascii_case(&pa.category)).unwrap_or_default();
    if matches!(category, FamilyCategory::Doors | FamilyCategory::Windows | FamilyCategory::Profiles) {
        notes.push(format!("{} come in as Generic Models: an imported one cannot cut a wall yet.", category.label()));
        category = FamilyCategory::GenericModels;
    } else if category == FamilyCategory::GenericModels && !pa.category.is_empty() && !pa.category.eq_ignore_ascii_case("Generic Models") {
        notes.push(format!("OpenArch has no category {}: it comes in as Generic Models.", pa.category));
    }
    let host = match pa.host.as_str() {
        "Ceiling" => HostKind::Ceiling,
        "Wall" => {
            notes.push("A wall-based family comes in free standing: it does not attach to or cut a wall yet.".into());
            HostKind::None
        }
        "Face" => {
            notes.push(match face {
                FaceAs::Wall => "A face-based family: it comes in free standing, stood up with its face as the wall behind it.".into(),
                FaceAs::Floor => "A face-based family: it comes in free standing, with its face as the floor under it.".into(),
            });
            HostKind::None
        }
        _ => HostKind::None,
    };
    let turn = |p: [f64; 3]| -> [f64; 3] {
        let q = if pa.host == "Face" && face == FaceAs::Wall { [p[0], -p[2], p[1]] } else { p };
        [round(q[0] * FT), round(q[1] * FT), round(q[2] * FT)]
    };

    // the solid forms' saved meshes, one form per material
    let parts = meshes(&mut rf, &mut notes)?;
    let mut body = FamilyBody::template();
    let mut drawn = 0;
    for part in parts {
        // a face-based family's template carries a block standing for the host, under its face
        if pa.host == "Face" && part.verts.iter().all(|v| v[2] <= 0.003) {
            continue;
        }
        let verts: Vec<f64> = part.verts.iter().flat_map(|v| turn(*v)).collect();
        body.add_form(Form {
            id: 0,
            name: format!("{} {}", part.class.trim_end_matches("Elem"), part.id),
            shape: FormShape::Mesh { verts, tris: part.tris },
            void: false,
            cuts_host: false,
            opening: false,
            cut_by: vec![],
            material: part.material.unwrap_or_default(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        });
        drawn += 1;
    }
    if drawn == 0 {
        notes.push("No solid could be read: the family comes in with nothing drawn.".into());
    }

    // the parameters the types set, beside the identity data every family has
    let mut params = openarch_model::famtemplate::identity_params();
    let mut types = vec![];
    for t in &pa.types {
        let mut values = BTreeMap::new();
        for p in &t.params {
            if p.name == "Default Elevation" {
                continue; // where Revit places it; OpenArch has Offset for that
            }
            if !params.iter().any(|d| d.name == p.name) {
                params.push(ParamDef {
                    name: p.name.clone(),
                    param_kind: p.kind,
                    instance: false,
                    group: match p.kind {
                        ParamKind::Material => "Materials and Finishes",
                        ParamKind::Length | ParamKind::Area | ParamKind::Volume | ParamKind::Angle => "Dimensions",
                        _ => "Identity Data",
                    }
                    .into(),
                    formula: None,
                    reporting: false,
                    shared_guid: p.guid.clone(),
                    choices: vec![],
                    discipline: "common".into(),
                    tooltip: String::new(),
                    locked: false,
                    family_category: None,
                    default: default_of(p.kind),
                });
            }
            values.insert(p.name.clone(), p.value.clone());
        }
        types.push(FamilyFileType { name: t.name.clone(), values });
    }
    if types.is_empty() {
        types.push(FamilyFileType { name: pa.name.clone(), values: BTreeMap::new() });
    }
    let family = Family {
        name: pa.name.clone(),
        category,
        host,
        plan_symbol: PlanSymbol::None,
        params,
        built_in: false,
        catalog: vec![],
        body,
        settings: FamilySettings::default(),
        source_path: Some(path.to_string_lossy().into()),
        loaded_at: None,
    };
    Ok(RfaFamily { file: FamilyFile { openarch_family: 1, family, types, uses: vec![] }, info, notes })
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn default_of(kind: ParamKind) -> ParamValue {
    match kind {
        ParamKind::YesNo => ParamValue::Bool(false),
        ParamKind::Length | ParamKind::Number | ParamKind::Integer | ParamKind::Angle | ParamKind::Area | ParamKind::Volume => ParamValue::Number(0.0),
        _ => ParamValue::Text(String::new()),
    }
}

/// One form's triangles in one material, in Revit's axes and feet.
struct Part {
    id: u64,
    class: String,
    material: Option<String>,
    verts: Vec<[f64; 3]>,
    tris: Vec<u32>,
}

fn meshes(rf: &mut RevitFile, notes: &mut Vec<String>) -> Result<Vec<Part>, String> {
    let document = native_form_geometry::open(rf).map_err(|e| format!("its geometry cannot be read: {}", err(e)))?;
    // the solid forms (not voids), the material each names, and the materials' names
    let mut forms: BTreeMap<u64, (String, Option<i64>)> = BTreeMap::new();
    let mut materials: BTreeMap<i64, String> = BTreeMap::new();
    let mut nested = 0;
    native_form_geometry::walk(rf, &document, &[102], |e| {
        let class = e.class.unwrap_or("");
        if class == "FamilyInstance" {
            nested += 1;
        }
        if class != "MaterialElem" && !FORMS.contains(&class) {
            return Ok(());
        }
        let Ok(graph) = native_parameters::decode_graph_with_limits(e.body, &document.registry, &LIMITS) else { return Ok(()) };
        let objects: Vec<Value> = graph.objects.iter().map(|o| o.fields.to_json()).collect();
        if class == "MaterialElem" {
            if let Some(name) = objects.iter().find_map(|o| o.get("m_name").and_then(|v| v.as_str()).filter(|s| !s.is_empty())) {
                materials.insert(e.id as i64, name.to_string());
            }
            return Ok(());
        }
        let root = objects.first().cloned().unwrap_or(Value::Null);
        if root.get("m_cutting").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(());
        }
        let material = root.pointer("/m_materialId/m_id/m_id64").and_then(|v| v.as_i64()).filter(|m| *m >= 0);
        forms.insert(e.id, (class.to_string(), material));
        Ok(())
    })
    .map_err(|e| format!("its records cannot be read: {}", err(e)))?;
    if nested > 0 {
        notes.push(format!("{nested} nested famil{} left out.", if nested == 1 { "y is" } else { "ies are" }));
    }

    // each form's saved display mesh, split by material
    let mut parts = vec![];
    let mut unread = 0;
    native_form_geometry::walk(rf, &document, &[103], |e| {
        if e.class != Some("GElement") {
            return Ok(());
        }
        let Some((class, form_material)) = forms.get(&e.id) else { return Ok(()) };
        let Ok(graph) = native_parameters::decode_graph_with_limits(e.body, &document.registry, &LIMITS) else {
            unread += 1;
            return Ok(());
        };
        let m = native_saved_mesh::graphics(&graph);
        // an appearance's bitmap is not geometry; anything else left out is a surface missing
        if m.diagnostics.iter().any(|d| !d.contains("GBitmap")) {
            unread += 1;
        }
        let mut by_material: BTreeMap<Option<i64>, Part> = BTreeMap::new();
        for p in &m.primitives {
            let key = p.material_id.filter(|m| *m >= 0).or(*form_material);
            let part = by_material.entry(key).or_insert_with(|| Part { id: e.id, class: class.clone(), material: key.and_then(|k| materials.get(&k).cloned()), verts: vec![], tris: vec![] });
            let base = part.verts.len() as u32;
            part.verts.extend(p.vertices.iter().copied());
            part.tris.extend(p.triangles.iter().flat_map(|t| [base + t[0], base + t[1], base + t[2]]));
        }
        parts.extend(by_material.into_values().filter(|p| !p.tris.is_empty()));
        Ok(())
    })
    .map_err(|e| format!("its geometry cannot be read: {}", err(e)))?;
    if unread > 0 {
        notes.push(format!("{unread} form{} only partly read (a surface the reader cannot draw yet): look for gaps.", if unread == 1 { " was" } else { "s were" }));
    }
    Ok(parts)
}

// ---- the PartAtom: plain XML naming the family, its category and host, and each type's values ----

struct PartAtom {
    name: String,
    category: String,
    host: String,
    types: Vec<PaType>,
}

struct PaType {
    name: String,
    params: Vec<PaParam>,
}

struct PaParam {
    name: String,
    kind: ParamKind,
    value: ParamValue,
    guid: Option<String>,
}

fn part_atom(rf: &mut RevitFile, path: &Path) -> Result<PartAtom, String> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Revit family".into());
    let xml = rf.part_atom().map(|p| p.raw_xml).unwrap_or_default();
    let root = Xml::parse(&xml);
    let entry = root.as_ref().and_then(|r| if r.name == "entry" { Some(r) } else { r.child("entry") });
    let Some(entry) = entry else {
        return Ok(PartAtom { name: stem, category: String::new(), host: String::new(), types: vec![] });
    };
    let name = entry.child("title").map(|t| t.text.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or(stem);
    let category = entry
        .kids
        .iter()
        .filter(|k| k.name == "category")
        .find(|k| k.child("scheme").is_some_and(|s| s.text.trim() == "adsk:revit:grouping"))
        .and_then(|k| k.child("term"))
        .map(|t| t.text.trim().to_string())
        .unwrap_or_default();
    let family = entry.child("A:family");
    let host = family.and_then(|f| f.child("Host")).map(|h| h.text.trim().to_string()).unwrap_or_default();
    let types = family
        .map(|f| f.kids.iter().filter(|k| k.name == "A:part").map(part_type).collect())
        .unwrap_or_default();
    Ok(PartAtom { name, category, host, types })
}

fn part_type(part: &Xml) -> PaType {
    let name = part.child("title").map(|t| t.text.trim().to_string()).unwrap_or_default();
    let params = part
        .kids
        .iter()
        .filter(|k| k.name != "title")
        .map(|k| {
            let name = k.attr("displayName").map(String::from).unwrap_or_else(|| k.name.replace('_', " "));
            let text = k.text.trim();
            let units = k.attr("units").unwrap_or("");
            let num = || text.parse::<f64>().unwrap_or(0.0);
            let (kind, value) = match k.attr("typeOfParameter").unwrap_or("Text") {
                "Length" => {
                    let f = match units { "mm" => 1.0, "cm" => 10.0, "m" => 1000.0, "in" => 25.4, "ft" => FT, _ => 1.0 };
                    (ParamKind::Length, ParamValue::Number(num() * f))
                }
                "Yes/No" => (ParamKind::YesNo, ParamValue::Bool(matches!(text, "Yes" | "yes" | "1" | "true" | "True"))),
                "Material" => (ParamKind::Material, ParamValue::Text(text.to_string())),
                "Number" => (ParamKind::Number, ParamValue::Number(num())),
                "Integer" => (ParamKind::Integer, ParamValue::Number(num().round())),
                "Angle" => (ParamKind::Angle, ParamValue::Number(num())),
                "Area" => (ParamKind::Area, ParamValue::Number(num())),
                "Volume" => (ParamKind::Volume, ParamValue::Number(num())),
                _ => (ParamKind::Text, ParamValue::Text(text.to_string())),
            };
            let guid = (k.attr("type") == Some("shared")).then(|| k.attr("id").map(String::from)).flatten();
            PaParam { name, kind, value, guid }
        })
        .collect();
    PaType { name, params }
}

/// Just enough XML for the PartAtom: elements, attributes and text, entities decoded.
#[derive(Debug, Default)]
struct Xml {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    kids: Vec<Xml>,
}

impl Xml {
    fn child(&self, name: &str) -> Option<&Xml> {
        self.kids.iter().find(|k| k.name == name)
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn parse(s: &str) -> Option<Xml> {
        let mut stack: Vec<Xml> = vec![Xml::default()];
        let mut i = 0;
        let b = s.as_bytes();
        while i < b.len() {
            if b[i] != b'<' {
                let j = s[i..].find('<').map_or(s.len(), |k| i + k);
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&unescape(&s[i..j]));
                }
                i = j;
                continue;
            }
            let rest = &s[i..];
            if rest.starts_with("<?") {
                i += rest.find("?>").map_or(rest.len(), |k| k + 2);
            } else if rest.starts_with("<!--") {
                i += rest.find("-->").map_or(rest.len(), |k| k + 3);
            } else if rest.starts_with("<![CDATA[") {
                let end = rest.find("]]>").unwrap_or(rest.len());
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&rest[9..end]);
                }
                i += (end + 3).min(rest.len());
            } else if rest.starts_with("</") {
                i += rest.find('>').map_or(rest.len(), |k| k + 1);
                if stack.len() > 1 {
                    let done = stack.pop()?;
                    stack.last_mut()?.kids.push(done);
                }
            } else {
                let end = rest.find('>')?;
                let tag = &rest[1..end];
                let closed = tag.ends_with('/');
                let tag = tag.trim_end_matches('/');
                let name_end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
                let mut el = Xml { name: tag[..name_end].to_string(), ..Default::default() };
                let mut a = &tag[name_end..];
                while let Some(eq) = a.find('=') {
                    let key = a[..eq].trim().to_string();
                    let after = a[eq + 1..].trim_start();
                    let q = after.chars().next()?;
                    let close = after[1..].find(q)? + 1;
                    el.attrs.push((key, unescape(&after[1..close])));
                    a = &after[close + 1..];
                }
                i += end + 1;
                if closed {
                    stack.last_mut()?.kids.push(el);
                } else {
                    stack.push(el);
                }
            }
        }
        let mut top = stack.into_iter().next()?;
        if top.kids.len() == 1 { top.kids.pop() } else { Some(top) }
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_part_atom_gives_the_name_category_host_and_each_types_values() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<entry xmlns="http://www.w3.org/2005/Atom" xmlns:A="urn:schemas-autodesk-com:partatom"><title>Toilet</title><category><term>23.45</term><scheme>std:oc1</scheme></category><category><term>Specialty Equipment</term><scheme>adsk:revit:grouping</scheme></category><A:family type="user"><Host>Face</Host><A:part type="user"><title>SUITE - WHITE</title><URL type="system" typeOfParameter="URL">https://x/</URL><Material_Seat type="custom" typeOfParameter="Material">Acrylic</Material_Seat><Manufacturer type="system" typeOfParameter="Text">GWA Bathrooms &amp; Kitchens</Manufacturer><Default_Elevation displayName="Default Elevation" type="system" typeOfParameter="Length" units="mm">450</Default_Elevation><Seat_Height type="custom" typeOfParameter="Length" units="ft">1.5</Seat_Height><Concealed type="custom" typeOfParameter="Yes/No">Yes</Concealed></A:part></A:family></entry>"#;
        let root = Xml::parse(xml).unwrap();
        assert_eq!(root.name, "entry");
        let family = root.child("A:family").unwrap();
        assert_eq!(family.child("Host").unwrap().text, "Face");
        let t = part_type(family.child("A:part").unwrap());
        assert_eq!(t.name, "SUITE - WHITE");
        let get = |n: &str| t.params.iter().find(|p| p.name == n).map(|p| (p.kind, p.value.clone()));
        assert_eq!(get("Manufacturer"), Some((ParamKind::Text, ParamValue::Text("GWA Bathrooms & Kitchens".into()))));
        assert_eq!(get("Material Seat"), Some((ParamKind::Material, ParamValue::Text("Acrylic".into()))));
        assert_eq!(get("Default Elevation"), Some((ParamKind::Length, ParamValue::Number(450.0))));
        assert_eq!(get("Seat Height"), Some((ParamKind::Length, ParamValue::Number(1.5 * FT))));
        assert_eq!(get("Concealed"), Some((ParamKind::YesNo, ParamValue::Bool(true))));
    }

    /// The toilet in the project folder (a manufacturer's family, so not in the repository): set OPENARCH_RFA to
    /// its path to run this.
    #[test]
    #[ignore]
    fn a_real_family_comes_in_with_its_types_and_solids() {
        let Ok(path) = std::env::var("OPENARCH_RFA") else { return };
        let f = read(Path::new(&path), FaceAs::Wall).unwrap();
        eprintln!("{:?} notes {:?}", f.info, f.notes);
        let tris: usize = f.file.family.body.forms.iter().map(|x| match &x.shape { FormShape::Mesh { tris, .. } => tris.len() / 3, _ => 0 }).sum();
        let by: std::collections::BTreeMap<String, usize> = f.file.family.body.forms.iter().fold(Default::default(), |mut m, x| { *m.entry(x.name.split(' ').next().unwrap_or("").to_string()).or_default() += 1; m });
        eprintln!("forms {} triangles {tris} by kind {by:?}", f.file.family.body.forms.len());
        assert!(!f.file.types.is_empty());
        assert!(f.file.family.body.forms.len() > 5, "{} forms", f.file.family.body.forms.len());
    }
}
