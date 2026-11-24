//! The Material Manager's appearance side: the parametric generator's choices and previews, image maps for a
//! standard appearance, the maps the 3D view draws Realistic with, baking a generated material to image files, and
//! the user material library (a folder of material files shared between projects).

use std::path::{Path, PathBuf};

use openarch_model::image::{base64_encode, image_kind};
use openarch_model::matgen::{self, Archetype, Finish, GeneratorParams, Layout, MortarProfile};
use openarch_model::styles::{AppearanceKind, FillPattern, LibraryRef, Material, TextureImage};
use openarch_model::{Document, Element, Id};
use serde_json::{json, Value};

use crate::Res;

fn s<'a>(p: &'a Value, k: &str) -> Result<&'a str, String> {
    p.get(k).and_then(|v| v.as_str()).ok_or_else(|| format!("missing {k}"))
}

fn material_of(d: &Document, p: &Value) -> Result<(Id, Material), String> {
    if let Some(id) = p.get("id").and_then(|v| v.as_u64()) {
        return match d.get(id) {
            Some(Element::Material(m)) => Ok((id, m.clone())),
            _ => Err("not a material".into()),
        };
    }
    let name = s(p, "name")?;
    d.materials().into_iter().find(|(_, m)| m.name == name).map(|(id, m)| (id, m.clone())).ok_or_else(|| format!("no material '{name}'"))
}

/// The generator's choices: archetypes with their layouts and starting values, finishes, mortar profiles.
pub fn presets() -> Res {
    let archetypes: Vec<Value> = [Archetype::Tile, Archetype::WoodPlank, Archetype::WoodSolid, Archetype::Masonry, Archetype::Metal].iter().map(|a| {
        json!({ "id": a, "layouts": Layout::for_archetype(*a), "preset": GeneratorParams::preset(*a) })
    }).collect();
    let finishes: Vec<Value> = [Finish::Matte, Finish::Satin, Finish::HighGloss, Finish::TexturedStone, Finish::Natural, Finish::Lacquered, Finish::MillFinish, Finish::Brushed, Finish::PowderCoated, Finish::Patina]
        .iter().map(|f| json!({ "id": f, "roughness": f.roughness() })).collect();
    Ok(json!({
        "archetypes": archetypes,
        "finishes": finishes,
        "mortar_profiles": [MortarProfile::Flush, MortarProfile::Raked, MortarProfile::Concave, MortarProfile::Weathered],
        // unit size presets (mm): name, length, height
        "unit_presets": {
            "masonry": [["Standard brick 230 x 76", 230, 76], ["Metric brick 215 x 65", 215, 65], ["Modular brick 194 x 57", 194, 57], ["Concrete block 390 x 190", 390, 190], ["Half-height block 390 x 90", 390, 90]],
            "tile": [["200 x 200", 200, 200], ["300 x 300", 300, 300], ["600 x 300", 600, 300], ["600 x 600", 600, 600], ["Subway 75 x 150", 150, 75], ["Plank 1200 x 200", 1200, 200]],
            "wood_plank": [["Flooring 130 x 1800", 1800, 130], ["Cladding 180 x 3000", 3000, 180], ["Decking 90 x 5400", 5400, 90], ["Parquet 70 x 350", 350, 70]],
        },
    }))
}

fn png_url(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64_encode(bytes))
}

/// Maps of a generator definition (`params`) or of a parametric material (`id` or `name`): `res` pixels along the
/// longer side (default 512), `square`, and which `maps` (albedo, normal, height, roughness, metalness, surface: the
/// 3D view's packed normal, height and roughness). Each comes back as a PNG data URL.
pub fn maps(d: &Document, p: &Value) -> Res {
    let params: GeneratorParams = if let Some(v) = p.get("params") {
        serde_json::from_value(v.clone()).map_err(|e| format!("bad generator parameters: {e}"))?
    } else {
        let (_, m) = material_of(d, p)?;
        m.appearance.generator.clone().filter(|_| m.appearance.kind == AppearanceKind::Parametric).ok_or("the material is not parametric")?
    };
    let square = p.get("square").and_then(|v| v.as_bool()).unwrap_or(false);
    let res = match p.get("res").and_then(|v| v.as_u64()) {
        Some(r) => r.clamp(16, 4096) as u32,
        None => auto_res(&params, 1024)?,
    };
    let g = matgen::generate(&params, res, square)?;
    let want: Vec<String> = p.get("maps").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_else(|| vec!["albedo".into()]);
    let mut out = json!({ "width_mm": g.width_mm, "height_mm": g.height_mm, "px": g.px, "depth_mm": g.depth_mm, "notes": g.notes, "base_color": params.base_color(), "hatch_lines": g.hatch.len(), "hatch": g.hatch });
    for k in want {
        let bytes = match k.as_str() {
            "albedo" => g.albedo_png()?,
            "normal" => g.normal_png()?,
            "height" => g.height_png()?,
            "roughness" => g.roughness_png()?,
            "metalness" => g.metalness_png()?,
            "surface" => g.surface_png()?,
            _ => return Err(format!("no map '{k}'")),
        };
        out[k] = json!(png_url(&bytes));
    }
    Ok(out)
}

/// Pixels along the longer side for about 2 mm a pixel, between 512 and `cap`.
fn auto_res(g: &GeneratorParams, cap: u32) -> Result<u32, String> {
    let (w, h) = matgen::domain(g)?;
    Ok(((w.max(h) / 2.0).ceil() as u32).clamp(512, cap))
}

/// A key that changes whenever what the 3D view draws for a material changes.
pub fn appearance_key(m: &Material) -> String {
    let text = serde_json::to_string(&(&m.appearance, m.graphics.shading_color)).unwrap_or_default();
    // FNV-1a
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// What the 3D view needs to draw a material in Realistic: its shader values and, when it has them, its maps
/// (`albedo` and, for a generated material, `surface` with normal, height and roughness packed; for a standard one
/// the linked images by slot, which the view packs itself).
pub fn textures(d: &Document, p: &Value) -> Res {
    let (_, m) = material_of(d, p)?;
    let a = &m.appearance;
    let res = p.get("res").and_then(|v| v.as_u64()).map(|r| r.clamp(64, 4096) as u32);
    let mut out = json!({
        "key": appearance_key(&m), "kind": a.kind, "textured": a.textured(), "color": a.color, "uv_size_mm": a.uv_size_mm,
        "uv_offset_mm": a.uv_offset_mm, "uv_rotation_deg": a.uv_rotation_deg, "roughness": a.roughness, "metalness": a.metalness,
        "reflectivity": a.reflectivity, "pom_depth_mm": a.pom_depth_mm,
    });
    match a.kind {
        AppearanceKind::Parametric => {
            if let Some(g) = &a.generator {
                let gen = matgen::generate(g, match res { Some(r) => r, None => auto_res(g, 2048)? }, false)?;
                out["albedo"] = json!(png_url(&gen.albedo_png()?));
                out["surface"] = json!(png_url(&gen.surface_png()?));
                out["roughness_map"] = json!(png_url(&gen.roughness_png()?));
            }
        }
        AppearanceKind::Standard => {
            let url = |t: &Option<TextureImage>| t.as_ref().map(|t| format!("data:{};base64,{}", t.mime, t.data));
            out["albedo"] = json!(url(&a.maps.albedo));
            out["normal"] = json!(url(&a.maps.normal));
            out["bump_is_height"] = json!(a.maps.bump_is_height);
            out["height"] = json!(url(&a.maps.height));
            out["roughness_map"] = json!(url(&a.maps.roughness));
            out["cutout"] = json!(url(&a.maps.cutout));
        }
    }
    Ok(out)
}

/// Link an image file into a standard appearance's slot (albedo, normal, height, roughness, cutout). The image is
/// kept in the project; the sample size stays as the user set it.
pub fn load_map(d: &mut Document, p: &Value) -> Res {
    let (id, m) = material_of(d, p)?;
    let slot = s(p, "slot")?;
    if !["albedo", "normal", "height", "roughness", "cutout"].contains(&slot) {
        return Err(format!("no slot '{slot}'"));
    }
    let path = PathBuf::from(s(p, "path")?);
    let bytes = std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let (mime, px) = image_kind(&bytes)?;
    let img = TextureImage { file: path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), mime: mime.into(), data: base64_encode(&bytes), px };
    let mut patch = json!({ "appearance": { "maps": { slot: img } } });
    if m.appearance.kind != AppearanceKind::Standard {
        patch["appearance"]["kind"] = json!("standard");
    }
    d.patch_style_item(id, &patch)?;
    Ok(json!({ "px": px, "mime": mime }))
}

fn safe(name: &str) -> String {
    let t: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') { c } else { '_' }).collect();
    let t = t.trim().trim_matches('.').to_string();
    if t.is_empty() { "Material".into() } else { t }
}

/// Bake a parametric material to a folder of images: {Name}_Albedo, _Normal, _Height_POM (16-bit), _Roughness,
/// _Metalness (PNG) and {Name}_Metadata.json with the real tile size and the parallax depth.
pub fn export(d: &Document, p: &Value) -> Res {
    let (_, m) = material_of(d, p)?;
    let g = m.appearance.generator.clone().filter(|_| m.appearance.kind == AppearanceKind::Parametric).ok_or("only a parametric material can be baked to images")?;
    let dir = PathBuf::from(s(p, "dir")?);
    let res = p.get("res").and_then(|v| v.as_u64()).unwrap_or(2048).clamp(64, 8192) as u32;
    let square = p.get("square").and_then(|v| v.as_bool()).unwrap_or(true);
    let format = p.get("format").and_then(|v| v.as_str()).unwrap_or("png");
    if !["png", "tiff", "exr"].contains(&format) {
        return Err(format!("no export to {format}: PNG, TIFF or EXR"));
    }
    let gen = matgen::generate(&g, res, square)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    let base = safe(&m.name);
    let mut files = vec![];
    for (suffix, map) in [("Albedo", "albedo"), ("Normal", "normal"), ("Height_POM", "height"), ("Roughness", "roughness"), ("Metalness", "metalness")] {
        // EXR is for the height data: the colour maps go alongside it as PNG
        let f = if format == "exr" && map != "height" { "png" } else { format };
        let path = dir.join(format!("{base}_{suffix}.{f}"));
        std::fs::write(&path, gen.map_file(map, f)?).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        files.push(path.to_string_lossy().into_owned());
    }
    let height = if format == "exr" { "32-bit float, 1 is the face, 0 is pom_depth_mm below it" } else { "16-bit, white is the face, black is pom_depth_mm below it" };
    let meta = json!({
        "material": m.name, "generator": "OpenArch BIM parametric material", "uv_width_mm": gen.width_mm, "uv_height_mm": gen.height_mm,
        "uv_rotation_deg": m.appearance.uv_rotation_deg, "pom_depth_mm": gen.depth_mm, "pixels": gen.px, "format": format, "height_map": height,
        "normal_map": "tangent space, +Y up (OpenGL)", "base_color": g.base_color(), "parameters": g,
    });
    let path = dir.join(format!("{base}_Metadata.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&meta).unwrap_or_default()).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    files.push(path.to_string_lossy().into_owned());
    Ok(json!({ "files": files, "px": gen.px }))
}

// ---------------------------------------------------------------- the user library

const EXT: &str = "oamat";

/// Where the library is unless the user picks another folder: Documents\OpenArch BIM\Material Library.
pub fn default_library_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let docs = home.join("Documents");
    if docs.is_dir() { docs } else { home }.join("OpenArch BIM").join("Material Library")
}

fn lib_dir(p: &Value) -> PathBuf {
    p.get("dir").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(default_library_dir)
}

fn asset_path(dir: &Path, category: &str, name: &str) -> Result<PathBuf, String> {
    let cat = category.trim();
    if cat.is_empty() {
        return Err("a library folder needs a name".into());
    }
    Ok(dir.join(safe(cat)).join(format!("{}.{EXT}", safe(name))))
}

/// One library file: the material and the patterns it draws with that are not built in (generated ones are made
/// again from the material).
fn asset(d: &Document, m: &Material) -> Value {
    let g = &m.graphics;
    let names = [&g.surface_foreground, &g.surface_background, &g.cut_foreground, &g.cut_background].iter().filter_map(|x| x.pattern.clone()).collect::<Vec<_>>();
    let patterns: Vec<&FillPattern> = d.fill_patterns().into_iter().map(|(_, p)| p).filter(|p| !p.built_in && names.contains(&p.name) && p.name != openarch_model::styles::generated_pattern_name(&m.name)).collect();
    let mut m = m.clone();
    m.library_source = None;
    // the list's picture, kept with the file so listing doesn't make it again
    json!({ "format": 1, "material": m, "patterns": patterns, "thumbnail": thumbnail(&m) })
}

/// The library's folders and materials: [{ category, name, class, kind, color, path }].
pub fn library_list(p: &Value) -> Res {
    let dir = lib_dir(p);
    let mut items = vec![];
    let mut categories = vec![];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut cats: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).collect();
        cats.sort_by_key(|e| e.file_name());
        for c in cats {
            let cat = c.file_name().to_string_lossy().into_owned();
            categories.push(cat.clone());
            let Ok(files) = std::fs::read_dir(c.path()) else { continue };
            let mut files: Vec<_> = files.flatten().filter(|f| f.path().extension().is_some_and(|x| x == EXT)).collect();
            files.sort_by_key(|f| f.file_name());
            for f in files {
                let Ok(text) = std::fs::read_to_string(f.path()) else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                let Ok(m) = serde_json::from_value::<Material>(v["material"].clone()) else { continue };
                // files saved before thumbnails were kept get one made now
                let thumb = v.get("thumbnail").and_then(|t| t.as_str()).map(String::from).or_else(|| if v.get("thumbnail").is_some() { None } else { thumbnail(&m) });
                items.push(json!({ "category": cat, "name": m.name, "class": m.class, "kind": m.appearance.kind, "color": m.graphics.shading_color, "description": m.identity.description, "path": f.path().to_string_lossy(), "thumbnail": thumb }));
            }
        }
    }
    Ok(json!({ "dir": dir.to_string_lossy(), "exists": dir.is_dir(), "categories": categories, "items": items }))
}

/// A small picture of a library material: a generated material's tile, or a standard one's albedo image when it is
/// small enough to send with the list (otherwise the list shows its colour).
fn thumbnail(m: &Material) -> Option<String> {
    match m.appearance.kind {
        AppearanceKind::Parametric => {
            let g = matgen::generate(m.appearance.generator.as_ref()?, 64, false).ok()?;
            Some(png_url(&g.albedo_png().ok()?))
        }
        AppearanceKind::Standard => {
            let a = m.appearance.maps.albedo.as_ref()?;
            (a.data.len() < 300_000).then(|| format!("data:{};base64,{}", a.mime, a.data))
        }
    }
}

/// Move library materials (`items`: category and name) into the folder `to`. One whose name is already there is left
/// where it is. Project materials that came from a moved one still name its old folder.
pub fn library_move(p: &Value) -> Res {
    let dir = lib_dir(p);
    let to = s(p, "to")?;
    let (mut moved, mut skipped) = (vec![], vec![]);
    for it in p.get("items").and_then(|v| v.as_array()).ok_or("missing items")? {
        let (cat, name) = (s(it, "category")?, s(it, "name")?);
        let (from, dest) = (asset_path(&dir, cat, name)?, asset_path(&dir, to, name)?);
        if from == dest {
            continue;
        }
        if dest.exists() {
            skipped.push(name.to_string());
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not make {}: {e}", parent.display()))?;
        }
        std::fs::rename(&from, &dest).map_err(|e| format!("could not move {}: {e}", from.display()))?;
        moved.push(name.to_string());
    }
    Ok(json!({ "moved": moved, "skipped": skipped }))
}

/// Save a project material to the library under `category` (as `as_name`, or its own name). An asset of that name
/// in that folder is replaced only with `replace`. The project material remembers where it went.
pub fn library_save(d: &mut Document, p: &Value) -> Res {
    let (id, m) = material_of(d, p)?;
    let dir = lib_dir(p);
    let category = s(p, "category")?;
    let name = p.get("as_name").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).unwrap_or(&m.name).trim().to_string();
    let path = asset_path(&dir, category, &name)?;
    if path.exists() && !p.get("replace").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err(format!("'{name}' is already in the library folder '{category}'"));
    }
    let mut m2 = m.clone();
    m2.name = name.clone();
    write_asset(&path, &asset(d, &m2))?;
    let src = LibraryRef { category: category.trim().into(), name };
    if m.library_source.as_ref() != Some(&src) {
        d.patch_style_item(id, &json!({ "library_source": src }))?;
    }
    Ok(json!({ "path": path.to_string_lossy() }))
}

fn write_asset(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    }
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap_or_default()).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Update Library Asset: write the project material over the library asset it came from.
pub fn library_update(d: &mut Document, p: &Value) -> Res {
    let (_, m) = material_of(d, p)?;
    let src = m.library_source.clone().ok_or("this material did not come from the library; save it to the library instead")?;
    let path = asset_path(&lib_dir(p), &src.category, &src.name)?;
    let mut m2 = m.clone();
    m2.name = src.name.clone();
    write_asset(&path, &asset(d, &m2))?;
    Ok(json!({ "path": path.to_string_lossy() }))
}

/// Import library materials into the project, all in one undo step: `items` [{ category, name }]. A material whose
/// name is already in the project is left out (or, with `replace`, overwritten), and said so.
pub fn library_import(d: &mut Document, p: &Value) -> Res {
    let dir = lib_dir(p);
    let replace = p.get("replace").and_then(|v| v.as_bool()).unwrap_or(false);
    let items = p.get("items").and_then(|v| v.as_array()).ok_or("missing items")?;
    let mark = d.edit_mark();
    let (mut added, mut skipped, mut replaced) = (vec![], vec![], vec![]);
    let mut result = Ok(());
    for it in items {
        let r = (|| -> Result<(), String> {
            let (cat, name) = (s(it, "category")?, s(it, "name")?);
            let path = asset_path(&dir, cat, name)?;
            let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not a material file: {e}", path.display()))?;
            let mut m: Material = serde_json::from_value(v["material"].clone()).map_err(|e| format!("{} is not a material file: {e}", path.display()))?;
            m.library_source = Some(LibraryRef { category: cat.into(), name: m.name.clone() });
            // patterns it needs that the project lacks
            for fp in v["patterns"].as_array().into_iter().flatten() {
                if let Ok(fp) = serde_json::from_value::<FillPattern>(fp.clone()) {
                    if d.fill_pattern(&fp.name).is_none() {
                        d.add_style_item(Element::FillPattern(fp))?;
                    }
                }
            }
            match d.materials().into_iter().find(|(_, x)| x.name == m.name).map(|(id, _)| id) {
                Some(id) if replace => {
                    let patch = serde_json::to_value(&m).map_err(|e| e.to_string())?;
                    d.patch_style_item(id, &patch)?;
                    replaced.push(m.name.clone());
                }
                Some(_) => skipped.push(m.name.clone()),
                None => {
                    let n = m.name.clone();
                    d.add_style_item(Element::Material(m))?;
                    added.push(n);
                }
            }
            Ok(())
        })();
        if let Err(e) = r {
            result = Err(e);
            break;
        }
    }
    if let Err(e) = result {
        d.roll_back_to(mark);
        return Err(e);
    }
    d.fold_since(mark, "Import materials from library");
    Ok(json!({ "added": added, "skipped": skipped, "replaced": replaced }))
}

/// Remove an asset from the library (the project's materials are not touched).
pub fn library_delete(p: &Value) -> Res {
    let path = asset_path(&lib_dir(p), s(p, "category")?, s(p, "name")?)?;
    std::fs::remove_file(&path).map_err(|e| format!("could not delete {}: {e}", path.display()))?;
    Ok(json!({}))
}

/// Make a library folder.
pub fn library_add_category(p: &Value) -> Res {
    let cat = s(p, "category")?.trim();
    if cat.is_empty() {
        return Err("a library folder needs a name".into());
    }
    let path = lib_dir(p).join(safe(cat));
    std::fs::create_dir_all(&path).map_err(|e| format!("could not make {}: {e}", path.display()))?;
    Ok(json!({ "path": path.to_string_lossy() }))
}
