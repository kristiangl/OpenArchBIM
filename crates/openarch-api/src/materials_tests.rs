use super::*;

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, &p).unwrap_or_else(|e| panic!("{m} failed: {e}"))
}

fn material_id(s: &mut Session, name: &str) -> u64 {
    let st = call(s, "standards", json!({}));
    st["materials"].as_array().unwrap().iter().find(|m| m["material"]["name"] == name).unwrap()["id"].as_u64().unwrap()
}

fn material(s: &mut Session, name: &str) -> Value {
    let st = call(s, "standards", json!({}));
    st["materials"].as_array().unwrap().iter().find(|m| m["material"]["name"] == name).map(|m| m["material"].clone()).unwrap_or(Value::Null)
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("openarch-mat-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn a_parametric_material_follows_its_generator_and_keeps_a_matching_pattern() {
    let mut s = Session::new();
    let presets = call(&mut s, "material_presets", json!({}));
    let masonry = presets["archetypes"].as_array().unwrap().iter().find(|a| a["id"] == "masonry").unwrap()["preset"].clone();
    call(&mut s, "add_style_item", json!({ "item": { "kind": "material", "name": "Red Brick", "class": "Masonry",
        "graphics": { "shading_color": [1, 2, 3], "sync_with_appearance": true, "pattern_from_appearance": true },
        "appearance": { "kind": "parametric", "generator": masonry } } }));
    let m = material(&mut s, "Red Brick");
    // stretcher bond 230 x 76 with 10 joints: 240 x 172 tile, colour from the generator, its own pattern
    assert_eq!(m["appearance"]["uv_size_mm"], json!([240.0, 172.0]));
    assert_eq!(m["graphics"]["shading_color"], json!([168, 84, 58]));
    assert_eq!(m["graphics"]["surface_foreground"]["pattern"], "Red Brick (generated)");
    let st = call(&mut s, "standards", json!({}));
    let pat = st["fill_patterns"].as_array().unwrap().iter().find(|p| p["pattern"]["name"] == "Red Brick (generated)").unwrap().clone();
    assert_eq!(pat["pattern"]["pattern_type"], "model");
    // bed joints every 86, perpends of each course
    let lines = pat["pattern"]["lines"].as_array().unwrap();
    assert!(lines.iter().any(|l| l["angle_deg"] == 0.0 && l["spacing"] == 172.0 && l["dashes"].as_array().unwrap().is_empty()));
    assert!(lines.iter().any(|l| l["angle_deg"] == 90.0 && l["spacing"] == 240.0));

    // a change to the generator changes the colour and the pattern in the same undo step
    let id = material_id(&mut s, "Red Brick");
    call(&mut s, "patch_style_item", json!({ "id": id, "patch": { "appearance": { "generator": { "primary_color": [100, 100, 100], "layout": "stack_bond" } } } }));
    let m = material(&mut s, "Red Brick");
    assert_eq!(m["graphics"]["shading_color"], json!([100, 100, 100]));
    assert_eq!(m["appearance"]["uv_size_mm"], json!([240.0, 86.0]));
    let st = call(&mut s, "standards", json!({}));
    let pat = st["fill_patterns"].as_array().unwrap().iter().find(|p| p["pattern"]["name"] == "Red Brick (generated)").unwrap().clone();
    assert!(pat["pattern"]["lines"].as_array().unwrap().iter().all(|l| l["dashes"].as_array().unwrap().is_empty()), "stack bond joints run straight through");
    call(&mut s, "undo", json!({}));
    assert_eq!(material(&mut s, "Red Brick")["graphics"]["shading_color"], json!([168, 84, 58]));

    // a custom colour once the sync is off
    call(&mut s, "patch_style_item", json!({ "id": id, "patch": { "graphics": { "sync_with_appearance": false, "shading_color": [255, 0, 0] } } }));
    call(&mut s, "patch_style_item", json!({ "id": id, "patch": { "appearance": { "generator": { "primary_color": [10, 10, 10] } } } }));
    assert_eq!(material(&mut s, "Red Brick")["graphics"]["shading_color"], json!([255, 0, 0]));

    // renaming takes the generated pattern along
    call(&mut s, "patch_style_item", json!({ "id": id, "patch": { "name": "Face Brick Red" } }));
    let m = material(&mut s, "Face Brick Red");
    assert_eq!(m["graphics"]["surface_foreground"]["pattern"], "Face Brick Red (generated)");
    let st = call(&mut s, "standards", json!({}));
    assert!(st["fill_patterns"].as_array().unwrap().iter().any(|p| p["pattern"]["name"] == "Face Brick Red (generated)"));
    assert!(!st["fill_patterns"].as_array().unwrap().iter().any(|p| p["pattern"]["name"] == "Red Brick (generated)"));

    // maps for the preview and the 3D view
    let maps = call(&mut s, "material_maps", json!({ "name": "Face Brick Red", "res": 128, "maps": ["albedo", "normal", "height"] }));
    assert!(maps["albedo"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert_eq!(maps["width_mm"], 240.0);
    let tex = call(&mut s, "material_textures", json!({ "name": "Face Brick Red", "res": 128 }));
    assert!(tex["surface"].as_str().is_some() && tex["textured"] == true);
    // a bad layout for the archetype is refused
    assert!(s.handle("patch_style_item", &json!({ "id": id, "patch": { "appearance": { "generator": { "layout": "corrugated" } } } })).is_err());
}

#[test]
fn the_3d_scene_lists_textured_materials_with_a_key_that_changes_with_them() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let (lvl, wt) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"][3]["id"].as_u64().unwrap());
    call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0], "height_mm": 2700 }));
    let scene = call(&mut s, "scene", json!({}));
    let used: Vec<String> = scene["meshes"].as_array().unwrap().iter().filter_map(|m| m["material"].as_str().map(String::from)).collect();
    assert!(!used.is_empty());
    assert!(scene["appearances"].as_object().unwrap().is_empty(), "nothing has maps yet");
    let name = used[0].clone();
    let id = material_id(&mut s, &name);
    let gen = serde_json::to_value(openarch_model::matgen::GeneratorParams::preset(openarch_model::matgen::Archetype::Masonry)).unwrap();
    call(&mut s, "patch_style_item", json!({ "id": id, "patch": { "appearance": { "kind": "parametric", "generator": gen } } }));
    let k1 = call(&mut s, "scene", json!({}))["appearances"][&name].as_str().unwrap().to_string();
    call(&mut s, "patch_style_item", json!({ "id": id, "patch": { "appearance": { "generator": { "seed": 7 } } } }));
    let k2 = call(&mut s, "scene", json!({}))["appearances"][&name].as_str().unwrap().to_string();
    assert_ne!(k1, k2);
}

#[test]
fn standard_maps_load_from_files_and_parametric_materials_bake_to_images() {
    let mut s = Session::new();
    let dir = temp("bake");
    std::fs::create_dir_all(&dir).unwrap();
    // a tiny PNG made by the generator stands in for a photo
    let g = openarch_model::matgen::generate(&openarch_model::matgen::GeneratorParams::preset(openarch_model::matgen::Archetype::Tile), 32, false).unwrap();
    let img = dir.join("tile.png");
    std::fs::write(&img, g.albedo_png().unwrap()).unwrap();
    let id = material_id(&mut s, "Concrete");
    call(&mut s, "material_load_map", json!({ "id": id, "slot": "albedo", "path": img.to_string_lossy() }));
    let m = material(&mut s, "Concrete");
    assert_eq!(m["appearance"]["maps"]["albedo"]["file"], "tile.png");
    assert_eq!(m["appearance"]["maps"]["albedo"]["px"], json!([32, 32]));
    let tex = call(&mut s, "material_textures", json!({ "id": id }));
    assert!(tex["albedo"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert!(s.handle("material_load_map", &json!({ "id": id, "slot": "albedo", "path": dir.join("missing.png").to_string_lossy() })).is_err());
    // only parametric materials bake
    assert!(s.handle("material_export", &json!({ "id": id, "dir": dir.to_string_lossy() })).is_err());
    let gen = serde_json::to_value(openarch_model::matgen::GeneratorParams::preset(openarch_model::matgen::Archetype::Metal)).unwrap();
    call(&mut s, "add_style_item", json!({ "item": { "kind": "material", "name": "Cladding / Grey", "class": "Metal", "appearance": { "kind": "parametric", "generator": gen } } }));
    let out = call(&mut s, "material_export", json!({ "name": "Cladding / Grey", "dir": dir.to_string_lossy(), "res": 128, "square": true }));
    let files: Vec<String> = out["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
    assert_eq!(files.len(), 6);
    for suffix in ["Albedo.png", "Normal.png", "Height_POM.png", "Roughness.png", "Metalness.png", "Metadata.json"] {
        let f = dir.join(format!("Cladding _ Grey_{suffix}"));
        assert!(f.is_file(), "{f:?}");
    }
    let meta: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("Cladding _ Grey_Metadata.json")).unwrap()).unwrap();
    assert_eq!(meta["uv_width_mm"], 76.0);
    assert_eq!(meta["pom_depth_mm"], 18.0);
    let h = std::fs::read(dir.join("Cladding _ Grey_Height_POM.png")).unwrap();
    assert_eq!(h[24], 16, "the height map is 16-bit");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_user_library_saves_imports_and_updates_independent_copies() {
    let lib = temp("lib");
    let d = lib.to_string_lossy().to_string();
    let mut a = Session::new();
    let gen = serde_json::to_value(openarch_model::matgen::GeneratorParams::preset(openarch_model::matgen::Archetype::WoodPlank)).unwrap();
    call(&mut a, "add_style_item", json!({ "item": { "kind": "material", "name": "Spotted Gum", "class": "Wood",
        "graphics": { "sync_with_appearance": true, "pattern_from_appearance": true }, "appearance": { "kind": "parametric", "generator": gen } } }));
    call(&mut a, "material_library_save", json!({ "dir": d, "name": "Spotted Gum", "category": "Timber" }));
    call(&mut a, "material_library_save", json!({ "dir": d, "name": "Face Brick", "category": "Masonry" }));
    assert!(a.handle("material_library_save", &json!({ "dir": d, "name": "Face Brick", "category": "Masonry" })).is_err(), "no silent overwrite");
    let list = call(&mut a, "material_library_list", json!({ "dir": d }));
    assert_eq!(list["categories"], json!(["Masonry", "Timber"]));
    assert_eq!(list["items"].as_array().unwrap().len(), 2);
    assert_eq!(material(&mut a, "Spotted Gum")["library_source"], json!({ "category": "Timber", "name": "Spotted Gum" }));

    // a second project takes both in one step; the one it already has is left alone
    let mut b = Session::new();
    let before = material(&mut b, "Face Brick");
    let r = call(&mut b, "material_library_import", json!({ "dir": d, "items": [{ "category": "Timber", "name": "Spotted Gum" }, { "category": "Masonry", "name": "Face Brick" }] }));
    assert_eq!(r["added"], json!(["Spotted Gum"]));
    assert_eq!(r["skipped"], json!(["Face Brick"]));
    assert_eq!(material(&mut b, "Face Brick"), before);
    let st = call(&mut b, "standards", json!({}));
    assert!(st["fill_patterns"].as_array().unwrap().iter().any(|p| p["pattern"]["name"] == "Spotted Gum (generated)"), "the generated pattern is made again");
    assert_eq!(b.doc.undo_name(), Some("Import materials from library"));
    call(&mut b, "undo", json!({}));
    assert!(material(&mut b, "Spotted Gum").is_null());
    call(&mut b, "redo", json!({}));

    // editing the copy leaves the library alone until Update Library Asset
    let id = material_id(&mut b, "Spotted Gum");
    call(&mut b, "patch_style_item", json!({ "id": id, "patch": { "appearance": { "generator": { "primary_color": [90, 60, 40] } } } }));
    let lib_colour = |s: &mut Session| call(s, "material_library_list", json!({ "dir": d }))["items"].as_array().unwrap().iter().find(|i| i["name"] == "Spotted Gum").unwrap()["color"].clone();
    assert_eq!(lib_colour(&mut b), json!([176, 128, 82]));
    call(&mut b, "material_library_update", json!({ "dir": d, "id": id }));
    assert_eq!(lib_colour(&mut b), json!([90, 60, 40]));
    // a material that never came from the library cannot update it
    assert!(b.handle("material_library_update", &json!({ "dir": d, "name": "Concrete" })).is_err());
    call(&mut b, "material_library_delete", json!({ "dir": d, "category": "Masonry", "name": "Face Brick" }));
    assert_eq!(call(&mut b, "material_library_list", json!({ "dir": d }))["items"].as_array().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&lib);
}

#[test]
fn the_3d_scene_gives_faces_their_surface_patterns() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let (lvl, wt) = (info["levels"][0]["id"].as_u64().unwrap(), info["wall_types"].as_array().unwrap().iter().find(|t| t["name"].as_str().unwrap().contains("Brick Veneer")).unwrap()["id"].as_u64().unwrap());
    let v3 = info["views"].as_array().unwrap().iter().find(|v| v["view_kind"] == "three_d").unwrap()["id"].as_u64().unwrap();
    call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0], "height_mm": 2700 }));
    let gen = serde_json::to_value(openarch_model::matgen::GeneratorParams::preset(openarch_model::matgen::Archetype::Masonry)).unwrap();
    let brick = material_id(&mut s, "Face Brick");
    call(&mut s, "patch_style_item", json!({ "id": brick, "patch": { "appearance": { "kind": "parametric", "generator": gen }, "graphics": { "pattern_from_appearance": true } } }));
    // a drafting pattern on the plasterboard: paper mm, laid square to the screen
    let board = material_id(&mut s, "Plasterboard");
    call(&mut s, "patch_style_item", json!({ "id": board, "patch": { "graphics": { "surface_foreground": { "pattern": "Diagonal up", "color": [200, 0, 0] } } } }));
    let scene = call(&mut s, "scene", json!({ "view_id": v3 }));
    let surf = |sc: &Value, mat: &str| sc["meshes"].as_array().unwrap().iter().find(|m| m["material"] == mat).map(|m| m["surface"].clone()).unwrap_or(Value::Null);
    assert_eq!(surf(&scene, "Face Brick")["pattern"], "Face Brick (generated)");
    assert_eq!(surf(&scene, "Plasterboard"), json!({ "pattern": "Diagonal up", "color": [200, 0, 0], "background": null }));
    assert!(surf(&scene, "Timber Frame").is_null(), "no surface pattern, nothing sent");
    let bed = scene["patterns"]["Face Brick (generated)"]["lines"].as_array().unwrap().iter().find(|l| l["angle"] == 0.0).unwrap().clone();
    assert_eq!(bed["spacing"], 172.0);
    assert_eq!(scene["patterns"]["Diagonal up"]["lines"][0]["spacing"], 2.5);
    assert_eq!(scene["patterns"]["Diagonal up"]["drafting"], true);
    assert_eq!(scene["patterns"]["Face Brick (generated)"]["drafting"], false);
    // Visibility/Graphics: surface patterns of walls off in this view
    call(&mut s, "patch_view", json!({ "id": v3, "patch": { "settings": { "overrides": { "model": { "Walls": { "surface_foreground": { "visible": false } } } } } } }));
    let scene = call(&mut s, "scene", json!({ "view_id": v3 }));
    assert!(surf(&scene, "Face Brick").is_null() && surf(&scene, "Plasterboard").is_null());
}

#[test]
fn elevations_lay_a_model_pattern_in_each_face_s_own_frame() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let lvl = info["levels"][0]["id"].as_u64().unwrap();
    let wt = info["wall_types"].as_array().unwrap().iter().find(|t| t["name"].as_str().unwrap().contains("Brick Veneer")).unwrap()["id"].as_u64().unwrap();
    let south = info["views"].as_array().unwrap().iter().find(|v| v["name"] == "South").unwrap()["id"].as_u64().unwrap();
    let gen = serde_json::to_value(openarch_model::matgen::GeneratorParams::preset(openarch_model::matgen::Archetype::Masonry)).unwrap();
    let brick = material_id(&mut s, "Face Brick");
    call(&mut s, "patch_style_item", json!({ "id": brick, "patch": { "appearance": { "kind": "parametric", "generator": gen }, "graphics": { "pattern_from_appearance": true } } }));
    // a wall square to the view and one at 30 degrees to it
    call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [0, 0], "end": [4000, 0], "height_mm": 2700 }));
    let (c, sn) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    call(&mut s, "add_wall", json!({ "type_id": wt, "level_id": lvl, "start": [8000, -3000], "end": [8000.0 + 4000.0 * c, -3000.0 + 4000.0 * sn], "height_mm": 2700 }));
    let el = call(&mut s, "elevation", json!({ "view_id": south }));
    let fr = &el["frame"];
    let o_r = fr["origin"][0].as_f64().unwrap() * fr["right"][0].as_f64().unwrap() + fr["origin"][1].as_f64().unwrap() * fr["right"][1].as_f64().unwrap();
    let maps: Vec<Vec<f64>> = el["items"].as_array().unwrap().iter().filter(|i| i["material"] == "Face Brick").filter_map(|i| i["pattern_map"].as_array()).map(|m| m.iter().map(|x| x.as_f64().unwrap()).collect()).collect();
    // along the face, a mm of pattern is a mm across the view on the square wall and cos 30 on the other; up is up
    let square = maps.iter().find(|m| (m[0] - 1.0).abs() < 1e-9).expect("the square wall's face");
    assert_eq!(&square[1..4], &[0.0, 0.0, 1.0]);
    assert!((square[4] + o_r).abs() < 1e-6, "from the project origin: {square:?}, marker at {o_r}");
    let angled = maps.iter().find(|m| (m[0] - c).abs() < 1e-9).expect("the angled wall's face");
    assert_eq!(&angled[1..4], &[0.0, 0.0, 1.0]);
    // a roof plane facing the view: along the eave across, up the slope rising by its sine
    let rt = info["roof_types"].as_array().unwrap()[0]["id"].as_u64().unwrap();
    let pts = [[0.0, 3000.0], [6000.0, 3000.0], [6000.0, 8000.0], [0.0, 8000.0]];
    let edges: Vec<Value> = (0..4).map(|i| json!({ "curve": { "type": "line", "a": { "x": pts[i][0], "y": pts[i][1] }, "b": { "x": pts[(i + 1) % 4][0], "y": pts[(i + 1) % 4][1] } }, "defines_slope": i % 2 == 0, "slope_deg": 30.0 })).collect();
    call(&mut s, "add_roof_sketch", json!({ "type_id": rt, "level_id": lvl, "edges": edges }));
    let el = call(&mut s, "elevation", json!({ "view_id": south }));
    let roof: Vec<f64> = el["items"].as_array().unwrap().iter().filter(|i| i["category"] == "Roofs").filter_map(|i| i["pattern_map"].as_array())
        .map(|m| m.iter().map(|x| x.as_f64().unwrap()).collect::<Vec<f64>>()).find(|m| (m[0] - 1.0).abs() < 1e-9).expect("the plane facing the view");
    assert!(roof[1].abs() < 1e-9 && roof[2].abs() < 1e-9 && (roof[3] - 0.5).abs() < 1e-9, "{roof:?}");
}
