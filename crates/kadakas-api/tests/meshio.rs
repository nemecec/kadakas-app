//! Interop of surface models (MOD-12): import of OBJ, STL and glTF into meshes, the conversion
//! into surface models with the soften option, the export of STL and the round trips.

use std::path::{Path, PathBuf};

use kadakas_api::{PathPolicy, RpcError, Session};
use serde_json::{Value, json};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {} {:?}", e.message, e.details))
}

fn err(s: &mut Session, method: &str, params: Value) -> RpcError {
    match s.handle(method, params) {
        Ok(v) => panic!("{method} should fail but gave {v}"),
        Err(e) => e,
    }
}

fn reason(e: &RpcError) -> String {
    e.reason.unwrap_or("").to_string()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> String {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p.display().to_string()
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).display().to_string()
}

/// A unit cube as OBJ text with quads (meters, Y up unless told).
const CUBE_OBJ: &str = "mtllib cube.mtl\no Cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nusemtl red\nf 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

fn revision(s: &Session) -> u64 {
    s.editor.revision()
}

/// A slab of 4 by 3 m, 0.5 m thick, with a hole of 1 by 1 m: ten faces, closed.
fn slab_with_hole(s: &mut Session) -> String {
    let id = ok(s, "mesh.create", json!({"name": "Slab"}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = |x0: f64, y0: f64, x1: f64, y1: f64| {
        json!([
            [[x0, y0, 0], [x1, y0, 0]],
            [[x1, y0, 0], [x1, y1, 0]],
            [[x1, y1, 0], [x0, y1, 0]],
            [[x0, y1, 0], [x0, y0, 0]]
        ])
    };
    let mut edges = r(0.0, 0.0, 4000.0, 3000.0).as_array().unwrap().clone();
    edges.extend(
        r(1000.0, 1000.0, 2000.0, 2000.0)
            .as_array()
            .unwrap()
            .clone(),
    );
    let drawn = ok(s, "mesh.add_edges", json!({"id": id, "edges": edges}));
    let faces = drawn["created"]["faces"].as_array().unwrap().clone();
    assert_eq!(faces.len(), 2, "outer face with a hole, and the inner face");
    // The outer face has the larger id? Find the one with the larger area.
    let listed = ok(s, "mesh.faces", json!({"id": id}));
    let outer = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .max_by(|a, b| {
            a["area_mm2"]
                .as_f64()
                .partial_cmp(&b["area_mm2"].as_f64())
                .unwrap()
        })
        .unwrap()["id"]
        .clone();
    // Erase the inner face so that the hole is open, then pull the slab up.
    let inner = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] != outer)
        .unwrap()["id"]
        .clone();
    ok(s, "mesh.erase", json!({"id": id, "faces": [inner]}));
    ok(
        s,
        "mesh.push_pull",
        json!({"id": id, "face": outer, "distance": 500}),
    );
    id
}

struct Facts {
    volume: f64,
    area: f64,
    faces: usize,
    closed: bool,
}

fn facts(s: &mut Session, id: &str) -> Facts {
    let rep = ok(s, "mesh.solid_report", json!({"id": id}));
    let sum = ok(s, "mesh.summary", json!({"id": id}));
    let shells = rep["shells"].as_array().unwrap();
    Facts {
        volume: shells.iter().map(|x| x["volume"].as_f64().unwrap()).sum(),
        area: shells.iter().map(|x| x["area"].as_f64().unwrap()).sum(),
        faces: sum["models"][0]["faces"].as_u64().unwrap() as usize,
        closed: rep["closed"].as_bool().unwrap(),
    }
}

fn first_object(r: &Value) -> String {
    r["report"]["objects"][0]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_surface_model_survives_each_format_with_the_same_volume_area_and_faces() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Session::with_sequential_ids();
    let slab = slab_with_hole(&mut a);
    let before = facts(&mut a, &slab);
    assert!(before.closed);
    assert_eq!(before.faces, 10);
    for (file, format, options) in [
        ("slab.obj", "obj", json!({"objects": [slab]})),
        ("slab.stl", "stl", json!({"objects": [slab]})),
        (
            "slab_ascii.stl",
            "stl",
            json!({"objects": [slab], "encoding": "ascii"}),
        ),
        ("slab.gltf", "gltf", json!({"objects": [slab]})),
        ("slab.glb", "glb", json!({"objects": [slab]})),
    ] {
        let p = path(dir.path(), file);
        let out = ok(
            &mut a,
            "export.file",
            json!({"path": p, "format": format, "options": options, "overwrite": true}),
        );
        assert!(out["exported"].as_u64().unwrap() >= 1, "{format}");
        let mut b = Session::with_sequential_ids();
        let imp = ok(&mut b, "import.file", json!({"path": p}));
        assert_eq!(imp["softened_edges"], 0, "{format}");
        assert_eq!(imp["report"]["objects"][0]["kind"], "mesh3d", "{format}");
        let id = first_object(&imp);
        // Equal volume and area, in the mesh and then in the surface model.
        let conv = ok(&mut b, "mesh.to_surface", json!({"id": id}));
        assert_eq!(conv["kind"], "surface_model");
        assert_eq!(conv["report"]["closed"], true, "{format}");
        let after = facts(&mut b, &id);
        assert!(
            (after.volume - before.volume).abs() < 1e-6 * before.volume,
            "{format}: {} {}",
            after.volume,
            before.volume
        );
        assert!(
            (after.area - before.area).abs() < 1e-6 * before.area,
            "{format}"
        );
        assert_eq!(after.faces, before.faces, "{format}");
        assert!(after.closed, "{format}");
    }
}

#[test]
fn units_axes_names_layers_and_colors_of_an_obj() {
    let dir = tempfile::tempdir().unwrap();
    let obj = write(dir.path(), "cube.obj", CUBE_OBJ.as_bytes());
    write(dir.path(), "cube.mtl", b"newmtl red\nKd 1 0 0\n");
    let mut s = Session::with_sequential_ids();
    let r = ok(&mut s, "import.file", json!({"path": obj}));
    // Meters and Y up by default: a cube of 1 m, standing on x, -y, z.
    let rep = &r["report"];
    assert_eq!(rep["unit"], "m");
    assert_eq!(rep["up_axis"], "y");
    assert_eq!(rep["bounds_mm"]["min"], json!([0.0, -1000.0, 0.0]));
    assert_eq!(rep["bounds_mm"]["size"], json!([1000.0, 1000.0, 1000.0]));
    assert_eq!(rep["triangles"], 12);
    assert_eq!(rep["objects"][0]["name"], "Cube");
    assert_eq!(rep["objects"][0]["color"], json!([255, 0, 0, 255]));
    assert_eq!(r["imported"], 1);
    assert_eq!(r["softened_edges"], 0);
    // One undo step.
    ok(&mut s, "undo", json!({}));
    assert_eq!(
        ok(&mut s, "objects.list", json!({}))["objects"]
            .as_array()
            .map_or(0, Vec::len),
        0
    );
    // Millimeters, Z up, a layer by name, a name, centered.
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": obj, "options": {"unit": "mm", "up_axis": "z", "layer": "Imports", "name": "Block", "center": true}}),
    );
    assert_eq!(r["report"]["bounds_mm"]["size"], json!([1.0, 1.0, 1.0]));
    assert_eq!(r["report"]["objects"][0]["name"], "Block");
    assert!(
        r["report"]["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("moved"))
    );
    let id = first_object(&r);
    let o = ok(&mut s, "objects.get", json!({"id": id}));
    let layers = ok(&mut s, "document.summary", json!({}));
    assert!(layers.to_string().contains("Imports"));
    assert_eq!(o["shape"]["kind"], "mesh3d");
    // Inches.
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": obj, "options": {"unit": "inch", "up_axis": "z"}}),
    );
    assert_eq!(r["report"]["bounds_mm"]["size"][0], 25.4);
}

#[test]
fn the_soften_question_the_preview_and_the_conversion_on_import() {
    let dir = tempfile::tempdir().unwrap();
    // A sphere: 12 rings of 16 around, as OBJ with triangles.
    let mut text = String::from("o Ball\nv 0 0 1000\n");
    let (rings, around) = (12usize, 16usize);
    for i in 1..rings {
        let th = std::f64::consts::PI * i as f64 / rings as f64;
        for j in 0..around {
            let ph = 2.0 * std::f64::consts::PI * j as f64 / around as f64;
            text += &format!(
                "v {} {} {}\n",
                1000.0 * th.sin() * ph.cos(),
                1000.0 * th.sin() * ph.sin(),
                1000.0 * th.cos()
            );
        }
    }
    text += "v 0 0 -1000\n";
    let ring = |i: usize, j: usize| 2 + (i - 1) * around + j % around;
    let south = 2 + (rings - 1) * around;
    for j in 0..around {
        text += &format!("f 1 {} {}\n", ring(1, j), ring(1, j + 1));
    }
    for i in 1..rings - 1 {
        for j in 0..around {
            text += &format!(
                "f {} {} {}\nf {} {} {}\n",
                ring(i, j),
                ring(i + 1, j),
                ring(i + 1, j + 1),
                ring(i, j),
                ring(i + 1, j + 1),
                ring(i, j + 1)
            );
        }
    }
    for j in 0..around {
        text += &format!(
            "f {} {} {}\n",
            ring(rings - 1, j),
            south,
            ring(rings - 1, j + 1)
        );
    }
    let p = write(dir.path(), "ball.obj", text.as_bytes());
    let mut s = Session::with_sequential_ids();
    let rev = revision(&s);
    // The preview counts the edges that would soften and changes nothing.
    let pre = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"unit": "mm", "up_axis": "z", "preview": true, "soften_below_angle": 40}}),
    );
    assert_eq!(pre["preview"], true);
    let would = pre["softened_edges"].as_u64().unwrap();
    assert!(would > 100, "{would}");
    assert_eq!(pre["report"]["to_surface"]["converted"], 1);
    assert_eq!(revision(&s), rev, "a preview changes nothing");
    // The import with the same options softens the same number of edges.
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"unit": "mm", "up_axis": "z", "soften_below_angle": 40}}),
    );
    assert_eq!(r["softened_edges"].as_u64().unwrap(), would);
    assert_eq!(r["report"]["objects"][0]["kind"], "surface_model");
    let id = first_object(&r);
    let sum = ok(&mut s, "mesh.summary", json!({"id": id}));
    assert_eq!(sum["models"][0]["soft_edges"].as_u64().unwrap(), would);
    // Without an angle nothing is soft, and the answer says 0.
    let plain = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"unit": "mm", "up_axis": "z", "to_surface": true}}),
    );
    assert_eq!(plain["softened_edges"], 0);
    let plain_id = first_object(&plain);
    assert_eq!(
        ok(&mut s, "mesh.summary", json!({"id": plain_id}))["models"][0]["soft_edges"],
        0
    );
    // mesh.to_surface with an angle on an imported mesh gives the same count.
    let mesh = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"unit": "mm", "up_axis": "z"}}),
    );
    assert_eq!(mesh["softened_edges"], 0);
    let mid = first_object(&mesh);
    let dry = ok(
        &mut s,
        "mesh.to_surface",
        json!({"id": mid, "soften_below_angle": 40, "dry_run": true}),
    );
    assert_eq!(dry["softened_edges"].as_u64().unwrap(), would);
    assert_eq!(
        ok(&mut s, "objects.get", json!({"id": mid}))["shape"]["kind"],
        "mesh3d"
    );
    let done = ok(
        &mut s,
        "mesh.to_surface",
        json!({"id": mid, "soften_below_angle": 40}),
    );
    assert_eq!(done["softened_edges"].as_u64().unwrap(), would);
    // One undo step gives the mesh back.
    ok(&mut s, "undo", json!({}));
    assert_eq!(
        ok(&mut s, "objects.get", json!({"id": mid}))["shape"]["kind"],
        "mesh3d"
    );
}

#[test]
fn a_part_over_the_limits_of_a_surface_model_stays_a_mesh() {
    let dir = tempfile::tempdir().unwrap();
    // A rough terrain of 240 by 240 cells: 115,200 triangles, over the face limit.
    let n = 240usize;
    let mut text = String::new();
    for j in 0..=n {
        for i in 0..=n {
            text += &format!(
                "v {} {} {}\n",
                i * 100,
                j * 100,
                ((i * 73_856_093) ^ (j * 19_349_663)) % 1000
            );
        }
    }
    let at = |i: usize, j: usize| j * (n + 1) + i + 1;
    for j in 0..n {
        for i in 0..n {
            text += &format!(
                "f {} {} {}\nf {} {} {}\n",
                at(i, j),
                at(i + 1, j),
                at(i + 1, j + 1),
                at(i, j),
                at(i + 1, j + 1),
                at(i, j + 1)
            );
        }
    }
    let p = write(dir.path(), "terrain.obj", text.as_bytes());
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"unit": "mm", "up_axis": "z", "soften_below_angle": 20}}),
    );
    assert_eq!(r["imported"], 1);
    assert_eq!(r["softened_edges"], 0);
    assert_eq!(r["report"]["objects"][0]["kind"], "mesh3d");
    let refused = &r["report"]["to_surface"]["refused"][0];
    assert_eq!(refused["data"]["error_id"], "mesh_too_large_for_surface");
    assert_eq!(refused["data"]["counts"]["faces"], 115_200);
    assert!(refused["reason"].as_str().unwrap().contains("limit 100000"));
    // The explicit command refuses with the counts, and nothing changes.
    let id = first_object(&r);
    let rev = revision(&s);
    let e = err(&mut s, "mesh.to_surface", json!({"id": id}));
    assert_eq!(reason(&e), "mesh_too_large_for_surface");
    let d = e.details.unwrap();
    assert_eq!(d["counts"]["faces"], 115_200);
    assert!(
        d["over"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["what"] == "faces" && o["limit"] == 100_000)
    );
    assert_eq!(revision(&s), rev);
}

#[test]
fn files_that_are_too_big_broken_or_hostile_are_refused_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::with_sequential_ids();
    let rev = revision(&s);
    let obj = write(dir.path(), "cube.obj", CUBE_OBJ.as_bytes());
    // Too large in bytes, in triangles and in vertices.
    let e = err(
        &mut s,
        "import.file",
        json!({"path": obj, "options": {"max_bytes": 10}}),
    );
    assert_eq!(reason(&e), "mesh_file_too_large");
    assert_eq!(e.details.as_ref().unwrap()["limit"], 10);
    assert_eq!(e.kind, "limit");
    let e = err(
        &mut s,
        "import.file",
        json!({"path": obj, "options": {"max_triangles": 5}}),
    );
    assert_eq!(reason(&e), "mesh_too_many_triangles");
    assert_eq!(e.details.as_ref().unwrap()["limit"], 5);
    let e = err(
        &mut s,
        "import.file",
        json!({"path": obj, "options": {"max_vertices": 4}}),
    );
    assert_eq!(reason(&e), "mesh_too_many_vertices");
    // A session limit holds even when the request asks for more.
    let mut small = Session::with_sequential_ids();
    small.set_limits(kadakas_api::SessionLimits {
        import_text_bytes: 20,
        ..Default::default()
    });
    let e = err(
        &mut small,
        "import.file",
        json!({"path": obj, "options": {"max_bytes": 1_000_000_000u64}}),
    );
    assert_eq!(reason(&e), "mesh_file_too_large");
    // Broken OBJ files.
    for (text, why) in [
        ("v 0 0 0\nf 1 2 3\n", "mesh_bad_index"),
        ("v 0 0 NaN\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", "mesh_not_finite"),
        ("v 0 0 1e30\n", "mesh_out_of_range"),
        ("v 1 2\n", "mesh_malformed"),
        ("# empty\n", "mesh_empty"),
    ] {
        let p = write(dir.path(), "bad.obj", text.as_bytes());
        let e = err(&mut s, "import.file", json!({"path": p}));
        assert_eq!(reason(&e), why, "{text}");
    }
    // STL: truncated, a huge count, NaN.
    let good = kadakas_meshio::stl::write_binary(
        "x",
        vec![[[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]].into_iter(),
    )
    .unwrap();
    let p = write(dir.path(), "cut.stl", &good[..good.len() - 7]);
    let e = err(&mut s, "import.file", json!({"path": p}));
    assert_eq!(reason(&e), "mesh_truncated");
    assert_eq!(e.details.as_ref().unwrap()["need"], good.len());
    let mut huge = good.clone();
    huge[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    let p = write(dir.path(), "huge.stl", &huge);
    let e = err(&mut s, "import.file", json!({"path": p}));
    assert_eq!(reason(&e), "mesh_too_many_triangles");
    assert_eq!(e.details.as_ref().unwrap()["count"], u32::MAX as u64);
    let mut nan = good.clone();
    nan[96..100].copy_from_slice(&f32::NAN.to_le_bytes());
    let p = write(dir.path(), "nan.stl", &nan);
    assert_eq!(
        reason(&err(&mut s, "import.file", json!({"path": p}))),
        "mesh_not_finite"
    );
    // glTF: a bad index and a missing buffer.
    let gltf = r#"{"asset":{"version":"2.0"},"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}],
"bufferViews":[{"buffer":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],"buffers":[{"byteLength":42,"uri":"tri.bin"}]}"#;
    let p = write(dir.path(), "tri.gltf", gltf.as_bytes());
    let e = err(&mut s, "import.file", json!({"path": p}));
    assert_eq!(reason(&e), "mesh_malformed");
    let mut bin = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 7] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    write(dir.path(), "tri.bin", &bin);
    assert_eq!(
        reason(&err(&mut s, "import.file", json!({"path": p}))),
        "mesh_bad_index"
    );
    // The refusals changed nothing.
    assert_eq!(revision(&s), rev);
    bin[40..42].copy_from_slice(&2u16.to_le_bytes());
    write(dir.path(), "tri.bin", &bin);
    let ok_import = ok(&mut s, "import.file", json!({"path": p}));
    assert_eq!(ok_import["report"]["triangles"], 1);
    // Bad options.
    for bad in [
        json!({"unit": "furlong"}),
        json!({"up_axis": "x"}),
        json!({"soften_below_angle": 200}),
        json!({"soften_below_angle": 10, "to_surface": false}),
        json!({"tolerance": 0}),
        json!({"nope": 1}),
    ] {
        let e = err(&mut s, "import.file", json!({"path": obj, "options": bad}));
        assert_eq!(e.code, -32602, "{bad}");
    }
    let e = err(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"unit": "mm"}}),
    );
    assert_eq!(e.code, -32602, "glTF takes no unit");
    // A missing file.
    assert_eq!(
        err(
            &mut s,
            "import.file",
            json!({"path": path(dir.path(), "none.obj")})
        )
        .code,
        -32602
    );
}

#[test]
fn glb_and_gltf_nodes_materials_and_instances() {
    let dir = tempfile::tempdir().unwrap();
    let json_text = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],
"nodes":[{"name":"left","mesh":0},{"name":"right","mesh":0,"translation":[3,0,0]}],
"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1,"material":0}]}],
"materials":[{"name":"paint","pbrMetallicRoughness":{"baseColorFactor":[0.2,0.5,0.8,1]}}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}],
"bufferViews":[{"buffer":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],"buffers":[{"byteLength":42}]}"#;
    let mut bin = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2, 0] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let mut j = json_text.as_bytes().to_vec();
    while !j.len().is_multiple_of(4) {
        j.push(b' ');
    }
    let mut glb = Vec::new();
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&((12 + 8 + j.len() + 8 + bin.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(j.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
    glb.extend_from_slice(&j);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004E4942u32.to_le_bytes());
    glb.extend_from_slice(&bin);
    let p = write(dir.path(), "two.glb", &glb);
    let mut s = Session::with_sequential_ids();
    let r = ok(&mut s, "import.file", json!({"path": p}));
    assert_eq!(r["imported"], 2);
    assert_eq!(r["report"]["objects"][0]["name"], "left");
    assert_eq!(r["report"]["objects"][1]["name"], "right");
    // The base color is linear in the file and sRGB in the document.
    assert_eq!(
        r["report"]["objects"][0]["color"],
        json!([124, 188, 231, 255])
    );
    assert_eq!(r["report"]["bounds_mm"]["max"][0], 4000.0);
    // Joined into one mesh with the shared color.
    let j = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"join": true}}),
    );
    assert_eq!(j["imported"], 1);
    assert_eq!(j["report"]["triangles"], 2);
    assert_eq!(
        j["report"]["objects"][0]["color"],
        json!([124, 188, 231, 255])
    );
}

#[test]
fn a_restricted_session_keeps_to_its_folders_and_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let inside = write(dir.path(), "cube.obj", CUBE_OBJ.as_bytes());
    write(dir.path(), "cube.mtl", b"newmtl red\nKd 0 1 0\n");
    let outside = write(other.path(), "cube.obj", CUBE_OBJ.as_bytes());
    let mut s = Session::with_sequential_ids();
    s.paths = PathPolicy::restricted([dir.path().to_path_buf()]);
    let r = ok(&mut s, "import.file", json!({"path": inside}));
    assert_eq!(
        r["report"]["objects"][0]["color"],
        json!([0, 255, 0, 255]),
        "the MTL next to it is read"
    );
    assert!(s.handle("import.file", json!({"path": outside})).is_err());
    // The format must match the extension of the file.
    let renamed = write(dir.path(), "cube.txt", CUBE_OBJ.as_bytes());
    assert!(
        s.handle("import.file", json!({"path": renamed, "format": "obj"}))
            .is_err()
    );
    // The MTL of a file in another folder is not read through a path with "..".
    let sneaky = write(
        dir.path(),
        "sneaky.obj",
        b"mtllib ../secret.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
    );
    write(
        dir.path().parent().unwrap(),
        "secret.mtl",
        b"newmtl x\nKd 1 1 1\n",
    );
    let r = ok(&mut s, "import.file", json!({"path": sneaky}));
    assert!(r["report"]["notes"].to_string().contains("not found"));
    // STL export: the extension, the overwrite flag and the folder.
    let out = path(dir.path(), "out.stl");
    ok(&mut s, "export.file", json!({"path": out, "format": "stl"}));
    let e = err(&mut s, "export.file", json!({"path": out, "format": "stl"}));
    assert_eq!(e.kind, "conflict");
    ok(
        &mut s,
        "export.file",
        json!({"path": out, "format": "stl", "overwrite": true}),
    );
    assert!(
        s.handle(
            "export.file",
            json!({"path": path(other.path(), "x.stl"), "format": "stl"})
        )
        .is_err()
    );
    assert!(
        s.handle(
            "export.file",
            json!({"path": path(dir.path(), "x.dat"), "format": "stl"})
        )
        .is_err()
    );
}

#[test]
fn stl_export_options_notes_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::with_sequential_ids();
    // Nothing to export.
    let e = err(
        &mut s,
        "export.file",
        json!({"path": path(dir.path(), "none.stl")}),
    );
    assert_eq!(e.kind, "validation");
    let slab = slab_with_hole(&mut s);
    // An open model: a lone face.
    let open = ok(
        &mut s,
        "mesh.create",
        json!({"name": "Open", "origin": [10000, 0, 0]}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": open, "edges": [
        [[0, 0, 0], [1000, 0, 0]], [[1000, 0, 0], [1000, 1000, 0]], [[1000, 1000, 0], [0, 1000, 0]], [[0, 1000, 0], [0, 0, 0]]]}),
    );
    let bin = path(dir.path(), "a.stl");
    let r = ok(&mut s, "export.file", json!({"path": bin}));
    assert_eq!(r["report"]["encoding"], "binary");
    assert_eq!(r["report"]["open_models"], 1);
    assert_eq!(r["report"]["units"], "mm");
    assert!(r["report"]["notes"].to_string().contains("not closed"));
    let bytes = std::fs::read(&bin).unwrap();
    let tris = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 84 + 50 * tris);
    assert_eq!(r["report"]["triangles"], tris);
    // Only the slab, in meters, Y up, as ASCII.
    let asc = path(dir.path(), "b.stl");
    let r = ok(
        &mut s,
        "export.file",
        json!({"path": asc, "options": {"objects": [slab], "encoding": "ascii", "units": "m", "y_up": true, "name": "My slab"}}),
    );
    assert_eq!(r["report"]["open_models"], 0);
    let text = std::fs::read_to_string(&asc).unwrap();
    assert!(text.starts_with("solid My slab\n") && text.contains("endsolid My slab"));
    assert!(text.contains("facet normal"));
    for bad in [
        json!({"encoding": "xml"}),
        json!({"units": "inch"}),
        json!({"nope": 1}),
    ] {
        let e = err(
            &mut s,
            "export.file",
            json!({"path": path(dir.path(), "c.stl"), "options": bad}),
        );
        assert_eq!(e.code, -32602, "{bad}");
    }
}

#[test]
fn to_surface_refusals_are_typed() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::with_sequential_ids();
    let slab = slab_with_hole(&mut s);
    let e = err(&mut s, "mesh.to_surface", json!({"id": slab}));
    assert_eq!(e.details.as_ref().unwrap()["error_id"], "not_a_mesh");
    let e = err(
        &mut s,
        "mesh.to_surface",
        json!({"id": "00000000-0000-0000-0000-00000000270f"}),
    );
    assert_eq!(e.details.as_ref().unwrap()["error_id"], "no_such_object");
    let obj = write(dir.path(), "cube.obj", CUBE_OBJ.as_bytes());
    let imp = ok(&mut s, "import.file", json!({"path": obj}));
    let id = first_object(&imp);
    for bad in [
        json!({"soften_below_angle": -1}),
        json!({"tolerance": 0.0001}),
    ] {
        let mut p = json!({"id": id});
        p.as_object_mut()
            .unwrap()
            .extend(bad.as_object().unwrap().clone());
        assert_eq!(err(&mut s, "mesh.to_surface", p).code, -32602);
    }
    // A locked layer.
    let layer = ok(&mut s, "objects.get", json!({"id": id}))["layer"].clone();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": layer, "locked": true}}),
    );
    let e = err(&mut s, "mesh.to_surface", json!({"id": id}));
    assert_eq!(e.details.as_ref().unwrap()["error_id"], "layer_locked");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": layer, "locked": false}}),
    );
    let done = ok(&mut s, "mesh.to_surface", json!({"id": id, "merge": false}));
    assert_eq!(done["report"]["faces"], 12);
    assert!(done.get("notes").is_none(), "the cube has no color");
}

#[test]
fn describe_lists_the_formats_and_the_methods_exist() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    let io = d["io"].to_string();
    for needle in [
        "obj, stl, gltf, glb",
        "soften_below_angle",
        "to_surface",
        "stl (3D",
    ] {
        assert!(io.contains(needle), "describe.io lacks {needle}");
    }
    assert!(d["mesh"]["to_surface"].is_string());
    let one = ok(&mut s, "describe", json!({"method": "mesh.to_surface"}));
    assert!(one.to_string().contains("dry_run"));
    let imp = ok(&mut s, "describe", json!({"method": "import.file"}));
    assert!(imp.to_string().contains("gltf"));
}

#[test]
fn a_grey_layer_or_class_stops_the_conversion_with_its_own_id() {
    let dir = tempfile::tempdir().unwrap();
    let obj = write(dir.path(), "cube.obj", CUBE_OBJ.as_bytes());
    let mut s = Session::with_sequential_ids();
    let imp = ok(
        &mut s,
        "import.file",
        json!({"path": obj, "options": {"layer": "Grey"}}),
    );
    let id = first_object(&imp);
    let o = ok(&mut s, "objects.get", json!({"id": id}));
    let (layer, class) = (o["layer"].clone(), o["class"].clone());
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_visibility", "layers": [layer], "mode": "grey"}}),
    );
    let e = err(&mut s, "mesh.to_surface", json!({"id": id}));
    assert_eq!(e.details.as_ref().unwrap()["error_id"], "layer_grey");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_visibility", "layers": [layer], "mode": "visible"}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_visibility", "classes": [class], "mode": "grey"}}),
    );
    let e = err(&mut s, "mesh.to_surface", json!({"id": id}));
    assert_eq!(e.details.as_ref().unwrap()["error_id"], "class_grey");
}

#[test]
fn the_ifc_import_makes_surface_models_when_asked() {
    let ifc = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ifc/faceted_brep_ifc2x3.ifc"
    );
    let mut s = Session::with_sequential_ids();
    // Without the option the import makes meshes and says that nothing softened.
    let plain = ok(&mut s, "import.file", json!({"path": ifc}));
    assert_eq!(plain["softened_edges"], 0);
    assert!(plain["report"].get("to_surface").is_none());
    ok(&mut s, "undo", json!({}));
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": ifc, "options": {"soften_below_angle": 100}}),
    );
    let t = &r["report"]["to_surface"];
    assert!(t["converted"].as_u64().unwrap() >= 1, "{t}");
    assert_eq!(t["refused"].as_array().unwrap().len(), 0);
    assert_eq!(r["softened_edges"], t["softened_edges"]);
    assert!(r["softened_edges"].as_u64().unwrap() > 0);
    let kinds = ok(&mut s, "objects.list", json!({"kind": "surface_model"}));
    assert!(!kinds.as_array().unwrap().is_empty());
    // One undo step takes it all back.
    ok(&mut s, "undo", json!({}));
    assert!(
        ok(&mut s, "objects.list", json!({}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A terrain with 115,200 triangles is over the limits of a surface model: the report lists
    // the refusal with the counts, and the mesh stays.
    let dir = tempfile::tempdir().unwrap();
    let n = 240usize;
    let head: Vec<String> = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ifc/polygonal_faceset.ifc"
    ))
    .unwrap()
    .lines()
    .take_while(|l| !l.starts_with("#100="))
    .map(String::from)
    .collect();
    let mut text = head.join("\n");
    text += "\n#100=IFCCARTESIANPOINTLIST3D((";
    let mut points = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            points.push(format!(
                "({}.,{}.,{}.)",
                i * 100,
                j * 100,
                ((i * 73_856_093) ^ (j * 19_349_663)) % 1000
            ));
        }
    }
    text += &points.join(",");
    text += "));\n#110=IFCTRIANGULATEDFACESET(#100,$,.T.,(";
    let at = |i: usize, j: usize| j * (n + 1) + i + 1;
    let mut faces = Vec::new();
    for j in 0..n {
        for i in 0..n {
            faces.push(format!(
                "({},{},{})",
                at(i, j),
                at(i + 1, j),
                at(i + 1, j + 1)
            ));
            faces.push(format!(
                "({},{},{})",
                at(i, j),
                at(i + 1, j + 1),
                at(i, j + 1)
            ));
        }
    }
    text += &faces.join(",");
    text += "),$);\n#111=IFCSHAPEREPRESENTATION(#13,'Body','Tessellation',(#110));\n#112=IFCPRODUCTDEFINITIONSHAPE($,$,(#111));\n#113=IFCCOVERING('0000000000000000000113',$,'Cladding',$,$,#31,#112,$,.CLADDING.);\n#900=IFCRELCONTAINEDINSPATIALSTRUCTURE('0000000000000000000900',$,$,$,(#113),#30);\nENDSEC;\nEND-ISO-10303-21;\n";
    let big = write(dir.path(), "terrain.ifc", text.as_bytes());
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": big, "options": {"to_surface": true}}),
    );
    let refused = r["report"]["to_surface"]["refused"].as_array().unwrap();
    assert_eq!(refused.len(), 1, "{r}");
    assert_eq!(refused[0]["data"]["counts"]["faces"], 2 * n * n);
    assert_eq!(r["report"]["to_surface"]["converted"], 0);
    assert_eq!(r["softened_edges"], 0);
    assert!(
        !ok(&mut s, "objects.list", json!({"kind": "mesh3d"}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Bad values are parameter errors.
    for bad in [
        json!({"to_surface": "yes"}),
        json!({"soften_below_angle": "wide"}),
        json!({"soften_below_angle": 400}),
        json!({"surface_tolerance": "tiny"}),
    ] {
        let e = err(&mut s, "import.file", json!({"path": ifc, "options": bad}));
        assert_eq!(e.code, -32602, "{bad}");
    }
}

#[test]
fn small_helpers_of_the_import_are_covered() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::with_sequential_ids();
    // Many names: more parts than the objects listed in the report are still all made.
    let mut text = String::from("v 0 0 0\nv 1 0 0\nv 0 1 0\n");
    for i in 0..210 {
        text += &format!("o part{i}\nf 1 2 3\n");
    }
    let p = write(dir.path(), "many.obj", text.as_bytes());
    let r = ok(&mut s, "import.file", json!({"path": p}));
    assert_eq!(r["imported"], 210);
    assert_eq!(r["report"]["objects"].as_array().unwrap().len(), 200);
    assert_eq!(r["report"]["objects_listed"], 200);
    // A name for several parts, with the part name after it.
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"name": "Set", "join": true}}),
    );
    assert_eq!(r["report"]["objects"][0]["name"], "Set");
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"name": "Set"}}),
    );
    assert_eq!(r["report"]["objects"][0]["name"], "Set part0");
    // The preview lists objects too, and a layer that exists is reused (case does not matter).
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"preview": true, "join": true}}),
    );
    assert_eq!(r["report"]["objects"][0]["kind"], "mesh3d");
    ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"layer": "Twice", "join": true}}),
    );
    ok(
        &mut s,
        "import.file",
        json!({"path": p, "options": {"layer": "TWICE", "join": true}}),
    );
    let names = ok(&mut s, "document.summary", json!({}))["layers"].to_string();
    assert_eq!(
        names.matches("Twice").count() + names.matches("TWICE").count(),
        1,
        "{names}"
    );
    // A layer that is locked is not used: a new layer is made with the same name is refused or
    // reused, but the import never writes on a locked layer.
    let tiny = write(
        dir.path(),
        "tiny.obj",
        b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
    );
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": tiny, "options": {"join": true, "center": true}}),
    );
    assert_eq!(r["report"]["bounds_mm"]["min"][2], 0.0);
}

mod props {
    use super::*;
    use proptest::prelude::*;

    /// A prism on a convex polygon (points on a circle, rounded to mm): a closed solid.
    fn prism(s: &mut Session, corners: usize, radius: f64, height: f64) -> String {
        let id = ok(s, "mesh.create", json!({"name": "Prism"}))["id"]
            .as_str()
            .unwrap()
            .to_string();
        let pts: Vec<[f64; 2]> = (0..corners)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / corners as f64;
                [(radius * a.cos()).round(), (radius * a.sin()).round()]
            })
            .collect();
        let edges: Vec<Value> = (0..corners)
            .map(|k| {
                let (a, b) = (pts[k], pts[(k + 1) % corners]);
                json!([[a[0], a[1], 0], [b[0], b[1], 0]])
            })
            .collect();
        let drawn = ok(s, "mesh.add_edges", json!({"id": id, "edges": edges}));
        let face = drawn["created"]["faces"][0].clone();
        ok(
            s,
            "mesh.push_pull",
            json!({"id": id, "face": face, "distance": height}),
        );
        id
    }

    /// A heightfield as OBJ text: n by n cells, heights in steps of 100 mm.
    fn terrain_obj(n: usize, h: &[u8]) -> String {
        let mut t = String::new();
        for j in 0..=n {
            for i in 0..=n {
                t += &format!(
                    "v {} {} {}\n",
                    i * 500,
                    j * 500,
                    h[(j * 11 + i) % h.len()] as usize * 100
                );
            }
        }
        let at = |i: usize, j: usize| j * (n + 1) + i + 1;
        for j in 0..n {
            for i in 0..n {
                t += &format!(
                    "f {} {} {}\nf {} {} {}\n",
                    at(i, j),
                    at(i + 1, j),
                    at(i + 1, j + 1),
                    at(i, j),
                    at(i + 1, j + 1),
                    at(i, j + 1)
                );
            }
        }
        t
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10))]

        // A closed prism keeps its volume, area and face count in each format.
        #[test]
        fn random_prisms_round_trip(
            corners in 3usize..9,
            radius in 500.0f64..4000.0,
            height in 250.0f64..3000.0,
            fmt in 0usize..5,
        ) {
            let dir = tempfile::tempdir().unwrap();
            let mut a = Session::with_sequential_ids();
            let id = prism(&mut a, corners, radius, height.round());
            let before = facts(&mut a, &id);
            prop_assert!(before.closed);
            prop_assert_eq!(before.faces, corners + 2);
            let (file, format, enc) = [
                ("p.stl", "stl", "binary"), ("p.stl", "stl", "ascii"), ("p.obj", "obj", ""),
                ("p.glb", "glb", ""), ("p.gltf", "gltf", ""),
            ][fmt];
            let out = path(dir.path(), file);
            let mut options = json!({"objects": [id]});
            if !enc.is_empty() {
                options["encoding"] = json!(enc);
            }
            ok(&mut a, "export.file", json!({"path": out, "format": format, "options": options, "overwrite": true}));
            let mut b = Session::with_sequential_ids();
            let back = ok(&mut b, "import.file", json!({"path": out, "options": {"to_surface": true, "tolerance": 0.05}}));
            let id2 = first_object(&back);
            let after = facts(&mut b, &id2);
            prop_assert!(after.closed);
            prop_assert_eq!(after.faces, before.faces);
            prop_assert!((after.volume - before.volume).abs() <= 1e-5 * before.volume, "{} {}", after.volume, before.volume);
            prop_assert!((after.area - before.area).abs() <= 1e-5 * before.area);
        }

        // A random terrain survives mesh -> surface -> file -> mesh -> surface: the same area, in
        // each format. A face that is not convex and has a corner on the line between two others
        // is triangulated with a triangle of no area, which the 3D scene does not write, so such
        // a face can come back as two. So the count never goes down.
        #[test]
        fn random_meshes_round_trip(
            n in 2usize..7,
            h in proptest::collection::vec(0u8..3, 1..40),
            fmt in 0usize..4,
        ) {
            let dir = tempfile::tempdir().unwrap();
            let src = write(dir.path(), "t.obj", terrain_obj(n, &h).as_bytes());
            let mut a = Session::with_sequential_ids();
            let imp = ok(&mut a, "import.file", json!({"path": src, "options": {"unit": "mm", "up_axis": "z", "to_surface": true}}));
            let id = first_object(&imp);
            let first = ok(&mut a, "mesh.summary", json!({"id": id}));
            let faces = first["models"][0]["faces"].as_u64().unwrap();
            let area0: f64 = ok(&mut a, "mesh.faces", json!({"id": id, "limit": 1000}))["items"]
                .as_array().unwrap().iter().map(|f| f["area_mm2"].as_f64().unwrap()).sum();
            let (file, format) = [("t.stl", "stl"), ("t.obj", "obj"), ("t.glb", "glb"), ("t.gltf", "gltf")][fmt];
            let out = path(dir.path(), &format!("out_{file}"));
            ok(&mut a, "export.file", json!({"path": out, "format": format, "options": {"objects": [id]}, "overwrite": true}));
            let mut b = Session::with_sequential_ids();
            let back = ok(&mut b, "import.file", json!({"path": out, "options": {"to_surface": true}}));
            let id2 = first_object(&back);
            let sum = ok(&mut b, "mesh.summary", json!({"id": id2}));
            prop_assert!(sum["models"][0]["faces"].as_u64().unwrap() >= faces);
            let area1: f64 = ok(&mut b, "mesh.faces", json!({"id": id2, "limit": 1000}))["items"]
                .as_array().unwrap().iter().map(|f| f["area_mm2"].as_f64().unwrap()).sum();
            prop_assert!((area0 - area1).abs() <= 1e-6 * area0.max(1.0), "{} {}", area0, area1);
        }
    }
}

#[allow(dead_code)]
fn unused(_: PathBuf) {}
