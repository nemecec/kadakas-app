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
