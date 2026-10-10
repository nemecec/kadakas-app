//! Massing models and material resources in glTF, GLB and OBJ exports and in sections.

use serde_json::{Value, json};

use crate::Session;
use crate::x3d_tests::{fail, glb_json, ok, slab, tmp};

fn massing(s: &mut Session, roof: &str) -> u64 {
    ok(
        s,
        "massing.add",
        json!({"footprint": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "floors": 2, "floor_height": 3000, "roof": roof, "pitch": 30, "name": "House"}),
    )["id"]
        .as_u64()
        .unwrap()
}

#[test]
fn massing_models_export_and_are_cut_by_sections() {
    let dir = tmp("massing");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    let id = massing(&mut s, "gable");
    let glb = dir.join("m.glb");
    let r = ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    assert_eq!(r["exported"], 1);
    let j = glb_json(&std::fs::read(&glb).unwrap());
    assert_eq!(j["nodes"][0]["extras"]["kind"], "massing");
    assert_eq!(j["nodes"][0]["extras"]["id"], id);
    // The solid stands 6 m under the eaves, and the gable roof is above that.
    let obj = dir.join("m.obj");
    ok(
        &mut s,
        "export.file",
        json!({"path": obj, "options": {"units": "m", "y_up": false}}),
    );
    let text = std::fs::read_to_string(&obj).unwrap();
    let zs: Vec<f64> = text
        .lines()
        .filter(|l| l.starts_with("v "))
        .map(|l| l.split_whitespace().nth(3).unwrap().parse().unwrap())
        .collect();
    let lo = zs.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    assert!(lo.abs() < 1e-6 && hi > 6.0 + 1.0, "z {lo} .. {hi}");
    // A section across the building cuts one face for the massing model.
    let line = ok(
        &mut s,
        "section_viewport.line_add",
        json!({"a": [-1000, 3000], "b": [9000, 3000], "name": "A"}),
    )["id"]
        .as_u64()
        .unwrap();
    let got = ok(
        &mut s,
        "section_viewport.get",
        json!({"id": line, "geometry": true}),
    );
    let faces = got["cut_faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1, "{got}");
    assert_eq!(faces[0]["object"], id);
    // The wall profile is 8 m by 6 m; the roof adds to it.
    let area = faces[0]["area_m2"].as_f64().unwrap();
    assert!(area > 8.0 * 6.0 - 1e-6, "{area}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn materials_flow_into_gltf_glb_and_obj() {
    let dir = tmp("materials");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    let id = massing(&mut s, "flat");
    let slab_id = slab(&mut s, 10_000, 0, 14_000, 3000, 150, 0);
    ok(&mut s, "material.add_defaults", json!({}));
    ok(
        &mut s,
        "material.create",
        json!({"name": "Steel", "base_color": [200, 210, 220, 255], "metallic": 1.0, "roughness": 0.2}),
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Brick", "object": id}),
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Steel", "object": slab_id}),
    );
    // GLB: materials with PBR values, one embedded image, texture coordinates.
    let glb = dir.join("m.glb");
    let r = ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    assert_eq!(r["report"]["textures"], 1);
    let j = glb_json(&std::fs::read(&glb).unwrap());
    let mats = j["materials"].as_array().unwrap();
    let steel = mats.iter().find(|m| m["name"] == "Steel").unwrap();
    assert_eq!(steel["pbrMetallicRoughness"]["metallicFactor"], 1.0);
    assert!(
        (steel["pbrMetallicRoughness"]["roughnessFactor"]
            .as_f64()
            .unwrap()
            - 0.2)
            .abs()
            < 1e-6
    );
    let brick = mats.iter().find(|m| m["name"] == "Brick").unwrap();
    assert!(brick["pbrMetallicRoughness"]["baseColorTexture"].is_object());
    assert_eq!(j["images"].as_array().unwrap().len(), 1);
    assert_eq!(j["images"][0]["mimeType"], "image/png");
    let uv_prims = j["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["primitives"].as_array().unwrap())
        .filter(|p| p["attributes"].get("TEXCOORD_0").is_some())
        .count();
    assert!(uv_prims >= 1);
    // GLTF text embeds the image.
    let gltf = dir.join("m.gltf");
    ok(&mut s, "export.file", json!({"path": gltf}));
    let text: Value = serde_json::from_slice(&std::fs::read(&gltf).unwrap()).unwrap();
    assert_eq!(text["images"].as_array().unwrap().len(), 1);
    // OBJ: the texture is a file next to it.
    let obj = dir.join("m.obj");
    let r = ok(&mut s, "export.file", json!({"path": obj}));
    let files = r["files"].as_array().unwrap();
    assert_eq!(files.len(), 3, "{files:?}");
    let mtl = std::fs::read_to_string(dir.join("m.mtl")).unwrap();
    assert!(mtl.contains("map_Kd Brick_0.png"), "{mtl}");
    assert!(
        std::fs::read(dir.join("Brick_0.png"))
            .unwrap()
            .starts_with(b"\x89PNG")
    );
    // Plants resolve per part: the trunk and the canopy get their own materials.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "plant", "at": [20000, 1000], "species": "Acer", "spread": 3000, "height": 6000}}}),
    );
    let plant = s.editor.doc().objects.keys().last().unwrap().0;
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Bark mulch", "object": plant, "slot": "trunk"}),
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Grass", "object": plant, "slot": "canopy"}),
    );
    let plants = dir.join("p.glb");
    ok(&mut s, "export.file", json!({"path": plants}));
    let j = glb_json(&std::fs::read(&plants).unwrap());
    let node = j["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["extras"]["id"] == plant)
        .unwrap();
    let prims = j["meshes"][node["mesh"].as_u64().unwrap() as usize]["primitives"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = prims
        .iter()
        .map(|p| {
            j["materials"][p["material"].as_u64().unwrap() as usize]["name"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert!(
        names.contains(&"Bark mulch") && names.contains(&"Grass"),
        "{names:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn obj_texture_files_follow_the_path_policy() {
    let base = tmp("texpolicy");
    let _ = std::fs::remove_dir_all(&base);
    let inside = base.join("inside");
    std::fs::create_dir_all(&inside).unwrap();
    let mut s = Session {
        paths: crate::PathPolicy::restricted([inside.clone()]),
        ..Session::default()
    };
    let id = massing(&mut s, "flat");
    ok(&mut s, "material.add_defaults", json!({}));
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Brick", "object": id}),
    );
    let out = inside.join("m.obj");
    ok(&mut s, "export.file", json!({"path": out}));
    assert!(inside.join("Brick_0.png").exists());
    // A texture file that exists blocks the export unless overwrite is set, and nothing is written.
    std::fs::remove_file(&out).unwrap();
    std::fs::remove_file(inside.join("m.mtl")).unwrap();
    assert_eq!(fail(&mut s, "export.file", json!({"path": out})).code, 1);
    assert!(!out.exists());
    ok(
        &mut s,
        "export.file",
        json!({"path": out, "overwrite": true}),
    );
    assert!(out.exists());
    let _ = std::fs::remove_dir_all(&base);
}

/// A park of `n` multi-stem plants on a grid, a section line through the middle of it.
fn park(s: &mut Session, n: i64) -> u64 {
    let style = ok(
        s,
        "plant_style.create",
        json!({"name": "Clump", "botanical_name": "Amelanchier", "id_code": "AM", "category": "tree", "spread": 3000, "height": 6000, "form": "multi_stem"}),
    )["id"]
        .as_u64()
        .unwrap();
    let side = (n as f64).sqrt().ceil() as i64;
    let positions: Vec<Value> = (0..n)
        .map(|i| json!([(i % side) * 3500, (i / side) * 3500]))
        .collect();
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_plants", "style": style, "positions": positions}}),
    );
    ok(
        s,
        "section_viewport.line_add",
        json!({"a": [-2000, (side / 2) * 3500], "b": [side * 3500 + 2000, (side / 2) * 3500], "name": "P", "depth": 12000}),
    )["id"]
        .as_u64()
        .unwrap()
}

#[test]
fn many_plants_instance_and_sections_are_fast_and_shared() {
    let dir = tmp("park");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    let line = park(&mut s, 3000);
    // Instancing: the plants of one style share a few meshes, not one each.
    let glb = dir.join("park.glb");
    let r = ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    let meshes = r["report"]["meshes"].as_u64().unwrap();
    // Eight looks (the variants), and the section line.
    assert!(
        meshes <= 8 + 1,
        "{meshes} meshes for 3000 plants of one style"
    );
    assert!(r["report"]["instances"].as_u64().unwrap() >= 3000 - 8);
    assert!(std::fs::metadata(&glb).unwrap().len() < 6_000_000);
    // A section of the park, in a debug build, in seconds; later calls and the sheet use the cache.
    let sheet = ok(&mut s, "sheet.create", json!({"name": "S", "paper": "a1"}))["layer"]
        .as_u64()
        .unwrap();
    let t = std::time::Instant::now();
    let vp = ok(
        &mut s,
        "section_viewport.create",
        json!({"sheet": sheet, "line": line, "scale": "fit"}),
    )["id"]
        .as_u64()
        .unwrap();
    let first = t.elapsed();
    assert!(first.as_secs_f64() < 5.0, "create took {first:?}");
    // The viewport was added after the drawing was made for it, so the first get is a new revision.
    ok(&mut s, "section_viewport.get", json!({"id": vp}));
    let builds = s.section_cache.lock().unwrap().builds;
    let t = std::time::Instant::now();
    let got = ok(&mut s, "section_viewport.get", json!({"id": vp}));
    assert!(
        got["edge_count"].as_u64().unwrap() > 100,
        "{}",
        got["edge_count"]
    );
    assert!(!got["silhouettes"].as_array().unwrap().is_empty());
    ok(&mut s, "sheet.render", json!({"sheet": sheet}));
    ok(
        &mut s,
        "export.file",
        json!({"path": dir.join("park.pdf"), "format": "pdf"}),
    );
    let later = t.elapsed();
    assert!(
        later.as_secs_f64() < 5.0,
        "get, render and pdf took {later:?}"
    );
    assert_eq!(
        s.section_cache.lock().unwrap().builds,
        builds,
        "get, the sheet and the PDF reuse the drawing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn views_far_from_the_plants_get_coarser_scenes_and_exports_choose_their_detail() {
    let dir = tmp("lod");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    park(&mut s, 50);
    let near = s.scene_for_pixel_size(5.0);
    let far = s.scene_for_pixel_size(500.0);
    assert!(
        std::sync::Arc::ptr_eq(&near, &s.scene()),
        "close: the normal scene"
    );
    assert!(far.mesh.triangle_count() * 3 < near.mesh.triangle_count());
    assert!(
        std::sync::Arc::ptr_eq(&far, &s.scene_for_pixel_size(500.0)),
        "kept"
    );
    // Exports use full meshes unless asked otherwise.
    let tris = |s: &mut Session, name: &str, options: Value| {
        let r = ok(
            s,
            "export.file",
            json!({"path": dir.join(name), "options": options}),
        );
        r["report"]["triangles"].as_u64().unwrap()
    };
    let fine = tris(&mut s, "fine.glb", json!({}));
    let billboard = tris(&mut s, "bb.glb", json!({"plant_detail": "billboard"}));
    let auto = tris(&mut s, "auto.glb", json!({"plant_detail": "auto"}));
    assert!(billboard * 3 < fine, "{billboard} {fine}");
    assert_eq!(auto, fine, "50 plants: the count rule gives full meshes");
    assert_eq!(
        fail(
            &mut s,
            "export.file",
            json!({"path": dir.join("x.glb"), "options": {"plant_detail": "huge"}})
        )
        .code,
        -32602
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn obj_names_with_spaces_and_equal_cleaned_names_stay_apart() {
    let dir = tmp("objnames");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    let a = massing(&mut s, "flat");
    let b = slab(&mut s, 10_000, 0, 14_000, 3000, 150, 0);
    // "Red brick" and "Red_brick" are the same text once spaces are cleaned.
    ok(
        &mut s,
        "material.create",
        json!({"name": "Red brick", "base_color": [200, 50, 50, 255]}),
    );
    ok(
        &mut s,
        "material.create",
        json!({"name": "Red_brick", "base_color": [50, 50, 200, 255]}),
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Red brick", "object": a}),
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Red_brick", "object": b}),
    );
    let obj = dir.join("my model.obj");
    let r = ok(&mut s, "export.file", json!({"path": obj}));
    let files: Vec<String> = r["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap().to_string())
        .collect();
    // The OBJ refers to the MTL by the name of the file that was written.
    let text = std::fs::read_to_string(&obj).unwrap();
    let mtllib = text
        .lines()
        .find_map(|l| l.strip_prefix("mtllib "))
        .unwrap()
        .to_string();
    assert!(!mtllib.contains(' '));
    assert!(dir.join(&mtllib).exists(), "{mtllib} not in {files:?}");
    let mtl = std::fs::read_to_string(dir.join(&mtllib)).unwrap();
    let mut names: Vec<&str> = mtl
        .lines()
        .filter_map(|l| l.strip_prefix("newmtl "))
        .collect();
    let n = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), n, "every material has its own name");
    // Every usemtl names a material of the MTL.
    for u in text.lines().filter_map(|l| l.strip_prefix("usemtl ")) {
        assert!(mtl.contains(&format!("newmtl {u}\n")), "{u}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
