//! `site.texture` and the terrain look in renders and exports (3D-05).

use kadakas_core::uid::AsUid;
use serde_json::{Value, json};

use crate::Session;
use crate::x3d_tests::{fail, glb_json, ok, tmp};

/// A site model of 9 by 9 points, 1 m apart, rising 10 percent toward +X.
fn terrain(s: &mut Session) -> kadakas_core::uid::Uid {
    let mut ids = Vec::new();
    for i in 0..9 {
        for j in 0..9 {
            let r = ok(
                s,
                "execute",
                json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100]}}}),
            );
            ids.push(r["created"][0].clone());
        }
    }
    ok(s, "site.create", json!({"source_ids": ids}))["id"]
        .as_uid()
        .unwrap()
}

#[test]
fn site_texture_sets_reads_and_checks() {
    let mut s = Session::with_sequential_ids();
    let id = terrain(&mut s);
    let r = ok(&mut s, "site.texture", json!({"id": id}));
    assert_eq!(r["look"], "color");
    assert_eq!(r["tile_size"], 2000.0);
    assert_eq!(r["bands"], 12);
    assert_eq!(r["ramp"].as_array().unwrap().len(), 4);
    ok(&mut s, "material.add_defaults", json!({}));
    let r = ok(
        &mut s,
        "site.texture",
        json!({"id": id, "look": "texture", "material": "grass", "tile_size": 1500,
               "offset": [10, 20], "rotation": 30}),
    );
    assert_eq!(r["look"], "texture");
    assert_eq!(r["material"], "Grass");
    assert_eq!(r["offset"], json!([10.0, 20.0]));
    // The same fields are in the settings of the object.
    let o = ok(&mut s, "objects.get", json!({"id": id}));
    assert_eq!(o["shape"]["settings"]["surface_look"], "texture");
    assert_eq!(o["shape"]["settings"]["texture_tile"], 1500.0);
    // By class: no material of its own.
    let r = ok(&mut s, "site.texture", json!({"id": id, "material": null}));
    assert!(r["material"].is_null());
    let r = ok(
        &mut s,
        "site.texture",
        json!({"id": id, "look": "elevation", "bands": 5, "ramp": [[1, 2, 3], [4, 5, 6, 7]],
               "range": [0, 800]}),
    );
    assert_eq!(r["ramp"], json!([[1, 2, 3, 255], [4, 5, 6, 7]]));
    assert_eq!(r["range"], json!([0.0, 800.0]));
    // One undo step per change.
    assert!(s.editor.undo());
    assert_eq!(ok(&mut s, "site.texture", json!({"id": id}))["look"], "texture");

    for (p, word) in [
        (json!({"look": "shiny"}), "look must be"),
        (json!({"material": "Nothing"}), "no material"),
        (json!({"tile_size": 1}), "tile"),
        (json!({"tile_size": "big"}), "invalid"),
        (json!({"bands": 0}), "bands"),
        (json!({"ramp": []}), "ramp"),
        (json!({"ramp": [[1, 2]]}), "ramp"),
        (json!({"ramp": [[1, 2, 300]]}), "ramp"),
        (json!({"ramp": 5}), "ramp"),
        (json!({"range": [5, 5]}), "range"),
        (json!({"color": 1}), "unknown parameter"),
    ] {
        let mut p = p;
        p["id"] = json!(id);
        let e = fail(&mut s, "site.texture", p.clone());
        assert!(
            e.message.to_lowercase().contains(word),
            "{p}: {}",
            e.message
        );
    }
    let circle = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}),
    )["created"][0]
        .as_uid()
        .unwrap();
    let e = fail(&mut s, "site.texture", json!({"id": circle}));
    assert!(e.message.contains("not a site model"));
    let e = fail(
        &mut s,
        "site.texture",
        json!({"id": "00000000-0000-0000-0000-0000000003e7"}),
    );
    assert!(e.message.contains("does not exist"));
}

#[test]
fn the_terrain_texture_reaches_glb_gltf_and_obj() {
    let dir = tmp("terrain-texture");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::with_sequential_ids();
    let id = terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    ok(
        &mut s,
        "site.texture",
        json!({"id": id, "look": "texture", "material": "Grass", "tile_size": 1000}),
    );
    let glb = dir.join("t.glb");
    let r = ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    assert_eq!(r["report"]["textures"], 1);
    let j = glb_json(&std::fs::read(&glb).unwrap());
    assert_eq!(j["images"].as_array().unwrap().len(), 1);
    let prims: Vec<&Value> = j["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["primitives"].as_array().unwrap())
        .collect();
    assert!(
        prims
            .iter()
            .any(|p| p["attributes"].get("TEXCOORD_0").is_some())
    );
    let gltf = dir.join("t.gltf");
    ok(&mut s, "export.file", json!({"path": gltf}));
    let text: Value = serde_json::from_slice(&std::fs::read(&gltf).unwrap()).unwrap();
    assert_eq!(text["images"].as_array().unwrap().len(), 1);
    let obj = dir.join("t.obj");
    ok(&mut s, "export.file", json!({"path": obj}));
    let mtl = std::fs::read_to_string(dir.join("t.mtl")).unwrap();
    assert!(mtl.contains("map_Kd Grass_0.png"), "{mtl}");
    assert!(
        std::fs::read(dir.join("Grass_0.png"))
            .unwrap()
            .starts_with(b"\x89PNG")
    );
    let text = std::fs::read_to_string(&obj).unwrap();
    assert!(text.lines().any(|l| l.starts_with("vt ")));
    // Same input, same file.
    let again = dir.join("t2.glb");
    ok(&mut s, "export.file", json!({"path": again, "format": "glb"}));
    assert_eq!(std::fs::read(&glb).unwrap(), std::fs::read(&again).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn elevation_bands_reach_the_exports_as_colored_primitives() {
    let dir = tmp("terrain-bands");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::with_sequential_ids();
    let id = terrain(&mut s);
    ok(
        &mut s,
        "site.texture",
        json!({"id": id, "look": "elevation", "bands": 4, "ramp": [[0, 0, 255], [255, 0, 0]]}),
    );
    let glb = dir.join("b.glb");
    ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    let j = glb_json(&std::fs::read(&glb).unwrap());
    assert!(j.get("images").is_none_or(|i| i.as_array().unwrap().is_empty()));
    let colors: std::collections::BTreeSet<String> = j["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["pbrMetallicRoughness"]["baseColorFactor"].to_string())
        .collect();
    assert!(colors.len() >= 3, "{colors:?}");
    // A render of the bands differs from a render of the plain terrain.
    let banded = ok(&mut s, "render.png", json!({"width": 200, "height": 150, "view": "left_isometric"}));
    ok(&mut s, "site.texture", json!({"id": id, "look": "color"}));
    let plain = ok(&mut s, "render.png", json!({"width": 200, "height": 150, "view": "left_isometric"}));
    assert_ne!(banded["png_base64"], plain["png_base64"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_textured_terrain_renders_differently_from_a_plain_one() {
    let mut s = Session::with_sequential_ids();
    let id = terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let plain = ok(&mut s, "render.png", json!({"width": 200, "height": 150}));
    ok(
        &mut s,
        "site.texture",
        json!({"id": id, "look": "texture", "material": "Gravel", "tile_size": 500}),
    );
    let textured = ok(&mut s, "render.png", json!({"width": 200, "height": 150}));
    assert_ne!(plain["png_base64"], textured["png_base64"]);
    // A smaller tile gives a different picture again.
    ok(&mut s, "site.texture", json!({"id": id, "tile_size": 4000}));
    let big = ok(&mut s, "render.png", json!({"width": 200, "height": 150}));
    assert_ne!(big["png_base64"], textured["png_base64"]);
}
