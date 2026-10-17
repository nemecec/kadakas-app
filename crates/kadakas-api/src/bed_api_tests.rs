//! Texture beds (3D-05): `bed.*`, the surface cut in the 3D scene, renders and exports.

use kadakas_core::uid::AsUid;
use serde_json::{Value, json};

use crate::Session;
use crate::x3d_tests::{fail, glb_json, ok, tmp};

/// A site model of 9 by 9 points, 1 m apart, rising 10 percent toward +X. Returns its ID.
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

const SQUARE: &str = "[[1500, 1500], [5500, 1500], [5500, 5500], [1500, 5500]]";

fn square() -> Value {
    serde_json::from_str(SQUARE).unwrap()
}

fn bed(s: &mut Session, boundary: Value, texture: &str) -> kadakas_core::uid::Uid {
    ok(
        s,
        "bed.add",
        json!({"boundary": boundary, "texture": texture}),
    )["id"]
        .as_uid()
        .unwrap()
}

#[test]
fn a_bed_is_added_read_changed_and_listed() {
    let mut s = Session::with_sequential_ids();
    let site = terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let a = bed(&mut s, square(), "grass");
    let r = ok(&mut s, "bed.get", json!({"id": a}));
    assert_eq!(r["texture"], "Grass");
    assert_eq!(r["material"], "Grass");
    assert_eq!(r["by_class"], false);
    assert_eq!(r["own_mapping"], false);
    assert_eq!(r["tile_size"], 2000.0, "the mapping of the site model");
    assert_eq!(r["points"], 4);
    assert_eq!(r["order"], 0);
    assert_eq!(r["active"], true);
    assert_eq!(r["acts_on"], json!([site]));
    // Its own mapping starts from the one of the site model.
    ok(
        &mut s,
        "site.texture",
        json!({"id": site, "tile_size": 700, "rotation": 12}),
    );
    let r = ok(&mut s, "bed.set", json!({"id": a, "offset": [10, 20]}));
    assert_eq!(r["own_mapping"], true);
    assert_eq!(r["tile_size"], 700.0);
    assert_eq!(r["offset"], json!([10.0, 20.0]));
    assert_eq!(r["rotation"], 12.0);
    let r = ok(&mut s, "bed.set", json!({"id": a, "tile_size": 300}));
    assert_eq!(r["tile_size"], 300.0);
    assert_eq!(r["offset"], json!([10.0, 20.0]));
    // Back to the mapping of the site model.
    let r = ok(&mut s, "bed.set", json!({"id": a, "own_mapping": false}));
    assert_eq!(r["own_mapping"], false);
    assert_eq!(r["tile_size"], 700.0);
    // By class: no material, so no effect, until the class has a material.
    let r = ok(&mut s, "bed.set", json!({"id": a, "texture": null}));
    assert_eq!(r["by_class"], true);
    assert!(r["material"].is_null());
    assert_eq!(r["active"], false);
    let class = r["class"].clone();
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Gravel", "class": class}),
    );
    let r = ok(&mut s, "bed.get", json!({"id": a}));
    assert_eq!(r["material"], "Gravel");
    assert_eq!(r["active"], true);
    // Order follows the drawing order; arrange changes it.
    let b = bed(&mut s, square(), "Grass");
    let list = ok(&mut s, "bed.list", json!({}));
    let order: Vec<Value> = list["beds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(order, vec![json!(a), json!(b)]);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "arrange", "ids": [a], "how": "front"}}),
    );
    let list = ok(&mut s, "bed.list", json!({"site": site}));
    let order: Vec<Value> = list["beds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(order, vec![json!(b), json!(a)]);
    assert_eq!(list["beds"][1]["order"], 1);
    // A change of the boundary and one undo step per call.
    let r = ok(
        &mut s,
        "bed.set",
        json!({"id": a, "boundary": [[0, 0], [1000, 0], [1000, 1000]]}),
    );
    assert_eq!(r["points"], 3);
    assert!(s.editor.undo());
    assert_eq!(ok(&mut s, "bed.get", json!({"id": a}))["points"], 4);
    // A bed away from every site model acts on nothing.
    let far = bed(
        &mut s,
        json!([[1.0e6, 1.0e6], [1.1e6, 1.0e6], [1.1e6, 1.1e6]]),
        "Grass",
    );
    let r = ok(&mut s, "bed.get", json!({"id": far}));
    assert_eq!(r["acts_on"], json!([]));
    assert_eq!(r["active"], false);
    assert_eq!(
        ok(&mut s, "bed.list", json!({"site": site}))["beds"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // The bed is an object of kind site_modifier with the texture bed kind.
    let o = ok(&mut s, "objects.get", json!({"id": a}));
    assert_eq!(o["shape"]["kind"], "site_modifier");
    assert_eq!(o["shape"]["modifier"]["type"], "texture_bed");
}

#[test]
fn closed_shapes_become_beds_and_keep_their_class_and_layer() {
    let mut s = Session::with_sequential_ids();
    terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let class = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_class", "name": "Lawn"}}),
    )["created"][0]
        .clone();
    let add = |s: &mut Session, shape: Value| {
        ok(
            s,
            "execute",
            json!({"command": {"op": "add_object", "shape": shape, "class": class}}),
        )["created"][0]
            .clone()
    };
    let poly = add(
        &mut s,
        json!({"kind": "polyline", "points": square(), "closed": true}),
    );
    let rect = add(
        &mut s,
        json!({"kind": "rect", "min": [0, 0], "max": [900, 900]}),
    );
    let circle = add(
        &mut s,
        json!({"kind": "circle", "center": [4000, 4000], "radius": 800}),
    );
    let area = add(
        &mut s,
        json!({"kind": "landscape_area", "boundary": square(), "name": "Meadow"}),
    );
    let r = ok(
        &mut s,
        "bed.add",
        json!({"from": [poly, rect, circle, area], "texture": "Grass"}),
    );
    let ids = r["ids"].as_array().unwrap().clone();
    assert_eq!(ids.len(), 4);
    assert_eq!(r["id"], ids[0]);
    let doc = ok(&mut s, "document.summary", json!({}));
    let _ = doc;
    // The polyline, rectangle and circle are replaced; the landscape area stays.
    for gone in [&poly, &rect, &circle] {
        assert!(
            s.handle("objects.get", json!({"id": gone})).is_err(),
            "{gone}"
        );
    }
    assert!(s.handle("objects.get", json!({"id": area})).is_ok());
    for bed in &ids {
        let row = ok(&mut s, "bed.get", json!({"id": bed}));
        assert_eq!(row["class"], class);
        assert_eq!(row["texture"], "Grass");
    }
    // The circle is a round bed: 4 points, arcs.
    let o = ok(&mut s, "objects.get", json!({"id": ids[2]}));
    assert_eq!(o["shape"]["boundary"].as_array().unwrap().len(), 4);
    // Undo restores the shapes and removes the beds in one step.
    assert!(s.editor.undo());
    assert!(s.handle("objects.get", json!({"id": poly})).is_ok());
    assert!(s.handle("bed.get", json!({"id": ids[0]})).is_err());
    // keep_source keeps the shapes; a layer can be given.
    let r = ok(
        &mut s,
        "bed.add",
        json!({"from": poly, "keep_source": true, "tile_size": 500}),
    );
    assert!(s.handle("objects.get", json!({"id": poly})).is_ok());
    let row = ok(&mut s, "bed.get", json!({"id": r["id"]}));
    assert_eq!(row["own_mapping"], true);
    assert_eq!(row["tile_size"], 500.0);
    assert!(row["texture"].is_null());
}

#[test]
fn bed_requests_with_bad_values_are_refused() {
    let mut s = Session::with_sequential_ids();
    let site = terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let a = bed(&mut s, square(), "Grass");
    let line = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [5, 5]}}}),
    )["created"][0]
        .clone();
    let open = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "polyline", "points": square(), "closed": false}}}),
    )["created"][0]
        .clone();
    for (method, p, word) in [
        ("bed.add", json!({}), "boundary"),
        (
            "bed.add",
            json!({"boundary": square(), "from": line}),
            "either",
        ),
        (
            "bed.add",
            json!({"boundary": [[0, 0], [1, 1]]}),
            "3 or more",
        ),
        ("bed.add", json!({"boundary": "x"}), "boundary"),
        (
            "bed.add",
            json!({"boundary": square(), "texture": "Nothing"}),
            "no material",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "tile_size": 1}),
            "tile",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "tile_size": "big"}),
            "tile_size",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "offset": [1]}),
            "offset",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "offset": [1, null]}),
            "offset",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "rotation": null}),
            "rotation",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "own_mapping": 3}),
            "own_mapping",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "keep_source": 3}),
            "keep_source",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "colour": 1}),
            "unknown parameter",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "layer": 5}),
            "layer",
        ),
        (
            "bed.add",
            json!({"boundary": square(), "class": 5}),
            "class",
        ),
        ("bed.add", json!({"from": 5}), "'from'"),
        ("bed.add", json!({"from": []}), "no IDs"),
        ("bed.add", json!({"from": line}), "not a closed shape"),
        ("bed.add", json!({"from": open}), "not closed"),
        (
            "bed.add",
            json!({"from": "00000000-0000-0000-0000-0000000003e7"}),
            "does not exist",
        ),
        ("bed.get", json!({}), "missing 'id'"),
        ("bed.get", json!({"id": line}), "not a texture bed"),
        ("bed.get", json!({"id": site}), "not a texture bed"),
        (
            "bed.get",
            json!({"id": "00000000-0000-0000-0000-0000000003e7"}),
            "does not exist",
        ),
        ("bed.set", json!({"id": a, "tile_size": 1}), "tile"),
        (
            "bed.set",
            json!({"id": a, "texture": "Nothing"}),
            "no material",
        ),
        (
            "bed.set",
            json!({"id": a, "boundary": [[0, 0]]}),
            "3 or more",
        ),
        ("bed.set", json!({"id": a, "boundary": 5}), "boundary"),
        ("bed.set", json!({"id": a, "mood": 5}), "unknown parameter"),
        ("bed.list", json!({"site": line}), "not a site model"),
        ("bed.list", json!({"site": 5}), "'site'"),
    ] {
        let e = fail(&mut s, method, p.clone());
        assert!(
            e.message.contains(word),
            "{method} {p}: {} (wanted {word})",
            e.message
        );
    }
    // Null params mean no parameters.
    assert!(s.handle("bed.list", Value::Null).is_ok());
    assert!(s.handle("bed.list", json!(5)).is_err());
    // Nothing changed.
    assert_eq!(
        ok(&mut s, "bed.list", json!({}))["beds"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn beds_do_not_change_the_grade() {
    let mut s = Session::with_sequential_ids();
    let site = terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let z = |s: &mut Session| {
        ok(
            s,
            "site.elevation",
            json!({"id": site, "x": 3000, "y": 3000, "surface": "proposed"}),
        )["z"]
            .clone()
    };
    let before = (z(&mut s), ok(&mut s, "site.cut_fill", json!({"id": site})));
    bed(&mut s, square(), "Grass");
    let after = (z(&mut s), ok(&mut s, "site.cut_fill", json!({"id": site})));
    assert_eq!(before, after);
    assert_eq!(
        ok(&mut s, "site.refresh", json!({"id": site}))["has_proposed"],
        false
    );
}

fn primitives(j: &Value) -> Vec<&Value> {
    j["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["primitives"].as_array().unwrap())
        .collect()
}

fn material_names(j: &Value) -> Vec<String> {
    j["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn a_bed_texture_reaches_the_render_and_the_exports() {
    let dir = tmp("bed-exports");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::with_sequential_ids();
    terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let plain_png = ok(&mut s, "render.png", json!({"width": 240, "height": 180}));
    let plain_glb = dir.join("plain.glb");
    ok(
        &mut s,
        "export.file",
        json!({"path": plain_glb, "format": "glb"}),
    );
    let plain = glb_json(&std::fs::read(&plain_glb).unwrap());
    assert!(
        plain
            .get("images")
            .is_none_or(|i| i.as_array().unwrap().is_empty())
    );
    let a = bed(&mut s, square(), "Grass");
    let with_bed = ok(&mut s, "render.png", json!({"width": 240, "height": 180}));
    assert_ne!(plain_png["png_base64"], with_bed["png_base64"]);
    // GLB: the texture of the bed is an image, its pieces have texture coordinates.
    let glb = dir.join("bed.glb");
    let r = ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    assert_eq!(r["report"]["textures"], 1);
    let j = glb_json(&std::fs::read(&glb).unwrap());
    assert_eq!(j["images"].as_array().unwrap().len(), 1);
    assert!(
        material_names(&j).iter().any(|n| n.contains("Grass")),
        "{:?}",
        material_names(&j)
    );
    assert!(
        primitives(&j)
            .iter()
            .any(|p| p["attributes"].get("TEXCOORD_0").is_some())
    );
    // gltf and OBJ with MTL.
    let gltf = dir.join("bed.gltf");
    ok(&mut s, "export.file", json!({"path": gltf}));
    let text: Value = serde_json::from_slice(&std::fs::read(&gltf).unwrap()).unwrap();
    assert_eq!(text["images"].as_array().unwrap().len(), 1);
    let obj = dir.join("bed.obj");
    ok(&mut s, "export.file", json!({"path": obj}));
    let mtl = std::fs::read_to_string(dir.join("bed.mtl")).unwrap();
    assert!(mtl.contains("map_Kd Grass_0.png"), "{mtl}");
    let text = std::fs::read_to_string(&obj).unwrap();
    assert!(text.lines().any(|l| l.starts_with("vt ")));
    assert!(text.contains("usemtl Grass"));
    // The same input gives the same file.
    let again = dir.join("bed2.glb");
    ok(
        &mut s,
        "export.file",
        json!({"path": again, "format": "glb"}),
    );
    assert_eq!(std::fs::read(&glb).unwrap(), std::fs::read(&again).unwrap());
    // A second material in front: a second texture.
    let front = bed(
        &mut s,
        json!([[3000, 3000], [7000, 3000], [7000, 7000], [3000, 7000]]),
        "Gravel",
    );
    let both = dir.join("both.glb");
    let r = ok(
        &mut s,
        "export.file",
        json!({"path": both, "format": "glb"}),
    );
    assert_eq!(r["report"]["textures"], 2);
    let _ = (a, front);
    // IFC has no textures: the bed is reported as skipped, the terrain stays.
    let ifc = dir.join("bed.ifc");
    let r = ok(&mut s, "export.file", json!({"path": ifc, "format": "ifc"}));
    let skipped = r["report"]["skipped"].to_string();
    assert!(skipped.contains("texture bed"), "{skipped}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn beds_work_with_the_elevation_look() {
    let dir = tmp("bed-elevation");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::with_sequential_ids();
    let site = terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    ok(
        &mut s,
        "site.texture",
        json!({"id": site, "look": "elevation", "bands": 4, "ramp": [[0, 0, 255], [255, 0, 0]]}),
    );
    bed(&mut s, square(), "Grass");
    let glb = dir.join("e.glb");
    ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    let j = glb_json(&std::fs::read(&glb).unwrap());
    // The bands outside the bed have colors; the bed has the texture.
    assert_eq!(j["images"].as_array().unwrap().len(), 1);
    let colors: std::collections::BTreeSet<String> = j["materials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["pbrMetallicRoughness"].get("baseColorTexture").is_none())
        .map(|m| m["pbrMetallicRoughness"]["baseColorFactor"].to_string())
        .collect();
    assert!(colors.len() >= 3, "{colors:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_texture_look_site_keeps_its_own_texture_outside_the_beds() {
    let dir = tmp("bed-texture-look");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::with_sequential_ids();
    let site = terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    ok(
        &mut s,
        "site.texture",
        json!({"id": site, "look": "texture", "material": "Grass", "tile_size": 1000}),
    );
    bed(&mut s, square(), "Gravel");
    let glb = dir.join("t.glb");
    let r = ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    assert_eq!(r["report"]["textures"], 2);
    let j = glb_json(&std::fs::read(&glb).unwrap());
    let names = material_names(&j);
    assert!(
        names.iter().any(|n| n.contains("Grass")) && names.iter().any(|n| n.contains("Gravel")),
        "{names:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_moved_bed_takes_its_own_mapping_along_and_old_documents_load() {
    let mut s = Session::with_sequential_ids();
    terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let a = ok(
        &mut s,
        "bed.add",
        json!({"boundary": square(), "texture": "Grass", "offset": [10, 20]}),
    )["id"]
        .as_uid()
        .unwrap();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [a], "delta": [3000, -500]}}),
    );
    assert_eq!(
        ok(&mut s, "bed.get", json!({"id": a}))["offset"],
        json!([1010.0, 1520.0]),
        "moved with the ground, then reduced to one tile (2000 mm)"
    );
    // A document with a bad mapping loads, and says what it repaired.
    let mut doc = ok(&mut s, "document.get", json!({}));
    for o in doc["objects"].as_array_mut().unwrap() {
        if o["shape"]["kind"] == "site_modifier" {
            o["shape"]["modifier"]["mapping"]["tile"] = json!(0.001);
        }
    }
    let r = ok(&mut s, "document.load", json!({"document": doc}));
    assert_eq!(r["notes"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(
        ok(&mut s, "bed.get", json!({"id": a}))["own_mapping"],
        false
    );
    // A command with the same bad value is refused.
    let e = fail(&mut s, "bed.set", json!({"id": a, "tile_size": 0.001}));
    assert!(e.message.contains("tile"), "{}", e.message);
    // The description lists the methods and examples.
    let d = ok(&mut s, "describe", json!({}));
    assert!(d["texture_beds"]["examples"].as_array().unwrap().len() >= 4);
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["name"].as_str())
        .filter(|n| n.starts_with("bed."))
        .collect();
    assert_eq!(names, vec!["bed.add", "bed.set", "bed.get", "bed.list"]);
}

#[test]
fn a_bed_from_a_landscape_area_with_holes_is_refused_and_a_closed_3d_polyline_is_accepted() {
    let mut s = Session::with_sequential_ids();
    terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let ring = square();
    let area = ok(
        &mut s,
        "landscape_area.add",
        json!({"boundary": ring, "holes": [[[2000, 2000], [3000, 2000], [3000, 3000]]]}),
    )["id"]
        .clone();
    let e = fail(&mut s, "bed.add", json!({"from": area, "texture": "Grass"}));
    assert!(e.message.contains("holes"), "{}", e.message);
    assert!(
        ok(&mut s, "bed.list", json!({}))["beds"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Without holes it works, and the area stays.
    let plain = ok(&mut s, "landscape_area.add", json!({"boundary": ring}))["id"].clone();
    ok(
        &mut s,
        "bed.add",
        json!({"from": plain, "texture": "Grass"}),
    );
    // A closed 3D polyline gives its outline in plan.
    let poly3 = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "polyline3d", "closed": true,
            "points": [[0, 0, 0], [3000, 0, 100], [3000, 3000, 200], [0, 3000, 50]]}}}),
    )["created"][0]
        .clone();
    let r = ok(
        &mut s,
        "bed.add",
        json!({"from": poly3, "texture": "Gravel"}),
    );
    assert_eq!(ok(&mut s, "bed.get", json!({"id": r["id"]}))["points"], 4);
}

#[test]
fn the_mapping_flags_must_agree() {
    let mut s = Session::with_sequential_ids();
    terrain(&mut s);
    ok(&mut s, "material.add_defaults", json!({}));
    let a = bed(&mut s, square(), "Grass");
    for p in [
        json!({"id": a, "own_mapping": false, "tile_size": 500}),
        json!({"id": a, "own_mapping": false, "offset": [1, 2]}),
        json!({"id": a, "own_mapping": false, "rotation": 5}),
    ] {
        let e = fail(&mut s, "bed.set", p);
        assert_eq!(e.code, 3, "{}", e.message);
        assert!(e.message.contains("own_mapping"));
    }
    let e = fail(
        &mut s,
        "bed.add",
        json!({"boundary": square(), "own_mapping": false, "tile_size": 500}),
    );
    assert_eq!(e.code, 3);
    // Alone, own_mapping false and true both work.
    ok(&mut s, "bed.set", json!({"id": a, "own_mapping": false}));
    assert_eq!(
        ok(&mut s, "bed.set", json!({"id": a, "own_mapping": true}))["own_mapping"],
        true
    );
}
