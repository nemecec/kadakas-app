//! Tests of the writers: structure, units and axes, instancing, grouping, determinism and OBJ.

use glam::DVec2;
use proptest::prelude::*;
use serde_json::Value;

use cadektor_core::command::{Command, Editor};
use cadektor_core::mesh;
use cadektor_core::model::{Attributes, Document, Object, Shape, Vertex};

use crate::material::{Material, MaterialSource, Texture};
use crate::*;

pub(crate) fn rect_ring(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vertex> {
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
        .iter()
        .map(|(x, y)| Vertex::corner(DVec2::new(*x, *y)))
        .collect()
}

pub(crate) fn slab(x0: f64, y0: f64, x1: f64, y1: f64, thickness: f64, top: f64) -> Shape {
    Shape::Hardscape {
        boundary: rect_ring(x0, y0, x1, y1),
        material: "paving".into(),
        thickness: Some(thickness),
        elevation: Some(top),
    }
}

pub(crate) fn plant(x: f64, y: f64, species: &str) -> Shape {
    Shape::Plant {
        at: DVec2::new(x, y),
        spread: 3000.0,
        height: 6000.0,
        species: species.into(),
    }
}

pub(crate) fn document(shapes: Vec<Shape>) -> Document {
    let mut ed = Editor::default();
    for shape in shapes {
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    ed.doc().clone()
}

fn glb(doc: &Document, opts: &ExportOptions) -> (Vec<u8>, ExportStats) {
    let scene = mesh::build(doc);
    export_glb(doc, &scene, opts, &BaseColors).unwrap()
}

/// Checks the rules of glTF that a reader relies on, on the JSON and the binary buffer.
pub(crate) fn check_structure(root: &Value, bin: &[u8]) {
    assert_eq!(root["asset"]["version"], "2.0");
    let views = root["bufferViews"].as_array().cloned().unwrap_or_default();
    let accessors = root["accessors"].as_array().cloned().unwrap_or_default();
    let buffer_len = root["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
    assert!(
        buffer_len <= bin.len() && bin.len() - buffer_len < 4,
        "buffer length"
    );
    assert_eq!(buffer_len % 4, 0);
    for v in &views {
        let (off, len) = (
            v["byteOffset"].as_u64().unwrap() as usize,
            v["byteLength"].as_u64().unwrap() as usize,
        );
        assert!(off + len <= buffer_len, "view inside the buffer");
        assert_eq!(off % 4, 0, "view aligned");
    }
    let size = |c: u64| match c {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        _ => 4,
    };
    let comps = |t: &str| match t {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        _ => panic!("type {t}"),
    };
    for a in &accessors {
        let v = &views[a["bufferView"].as_u64().unwrap() as usize];
        let count = a["count"].as_u64().unwrap() as usize;
        let need =
            count * comps(a["type"].as_str().unwrap()) * size(a["componentType"].as_u64().unwrap());
        assert!(
            need <= v["byteLength"].as_u64().unwrap() as usize,
            "accessor inside its view"
        );
    }
    let meshes = root["meshes"].as_array().cloned().unwrap_or_default();
    for m in &meshes {
        for p in m["primitives"].as_array().unwrap() {
            let pos = &accessors[p["attributes"]["POSITION"].as_u64().unwrap() as usize];
            let n = pos["count"].as_u64().unwrap() as usize;
            assert!(pos["min"].is_array() && pos["max"].is_array());
            let idx = &accessors[p["indices"].as_u64().unwrap() as usize];
            let ic = idx["count"].as_u64().unwrap() as usize;
            let mode = p["mode"].as_u64().unwrap_or(4);
            assert_eq!(
                ic % if mode == 1 { 2 } else { 3 },
                0,
                "whole triangles or segments"
            );
            // Every index is below the vertex count, and the normals match the positions.
            let v = &views[idx["bufferView"].as_u64().unwrap() as usize];
            let off = v["byteOffset"].as_u64().unwrap() as usize;
            let comp = idx["componentType"].as_u64().unwrap();
            for k in 0..ic {
                let i = if comp == 5123 {
                    u16::from_le_bytes([bin[off + 2 * k], bin[off + 2 * k + 1]]) as usize
                } else {
                    u32::from_le_bytes(bin[off + 4 * k..off + 4 * k + 4].try_into().unwrap())
                        as usize
                };
                assert!(i < n, "index {i} below {n}");
            }
            if let Some(nrm) = p["attributes"].get("NORMAL") {
                assert_eq!(
                    accessors[nrm.as_u64().unwrap() as usize]["count"]
                        .as_u64()
                        .unwrap() as usize,
                    n
                );
            }
            let mat = p["material"].as_u64().unwrap() as usize;
            assert!(mat < root["materials"].as_array().unwrap().len());
        }
    }
    for n in root["nodes"].as_array().cloned().unwrap_or_default() {
        assert!((n["mesh"].as_u64().unwrap() as usize) < meshes.len());
    }
    for s in root["scenes"].as_array().unwrap() {
        for n in s["nodes"].as_array().unwrap() {
            assert!((n.as_u64().unwrap() as usize) < root["nodes"].as_array().unwrap().len());
        }
    }
}

/// Splits a GLB into its JSON and its binary chunk.
pub(crate) fn split_glb(bytes: &[u8]) -> (Value, Vec<u8>) {
    assert_eq!(&bytes[0..4], b"glTF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
        bytes.len()
    );
    let jl = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(jl % 4, 0);
    let json: Value = serde_json::from_slice(&bytes[20..20 + jl]).unwrap();
    let b0 = 20 + jl;
    let bl = u32::from_le_bytes(bytes[b0..b0 + 4].try_into().unwrap()) as usize;
    assert_eq!(bl % 4, 0);
    assert_eq!(&bytes[b0 + 4..b0 + 8], &0x004E_4942u32.to_le_bytes());
    (json, bytes[b0 + 8..b0 + 8 + bl].to_vec())
}

fn node_by_object(root: &Value, id: u64) -> Value {
    root["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["extras"]["id"] == id)
        .cloned()
        .unwrap()
}

#[test]
fn glb_is_valid_and_readable_by_a_gltf_reader() {
    let doc = document(vec![
        slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0),
        plant(1000.0, 1000.0, "Acer"),
        Shape::Polyline3d {
            points: vec![
                glam::DVec3::new(0.0, 0.0, 0.0),
                glam::DVec3::new(1000.0, 0.0, 500.0),
            ],
            closed: false,
        },
    ]);
    let (bytes, stats) = glb(&doc, &ExportOptions::default());
    let (root, bin) = split_glb(&bytes);
    check_structure(&root, &bin);
    assert_eq!(stats.objects, 3);
    assert_eq!(stats.bytes, bytes.len());
    assert!(stats.triangles > 0 && stats.lines == 1);
    // An independent reader accepts it and finds the same things.
    let g = gltf::Gltf::from_slice(&bytes).unwrap();
    assert_eq!(g.document.nodes().count(), 3);
    assert_eq!(g.document.meshes().count(), 3);
    let blob = g.blob.clone().unwrap();
    let mut tris = 0;
    let mut lines = 0;
    for m in g.document.meshes() {
        for p in m.primitives() {
            let r = p.reader(|_| Some(blob.as_slice()));
            let pos: Vec<[f32; 3]> = r.read_positions().unwrap().collect();
            let idx: Vec<u32> = r.read_indices().unwrap().into_u32().collect();
            assert!(idx.iter().all(|i| (*i as usize) < pos.len()));
            match p.mode() {
                gltf::mesh::Mode::Triangles => tris += idx.len() / 3,
                gltf::mesh::Mode::Lines => lines += idx.len() / 2,
                m => panic!("mode {m:?}"),
            }
        }
    }
    assert_eq!(tris, stats.triangles);
    assert_eq!(lines, 1);
    // Names and extras.
    let slab_node = g
        .document
        .nodes()
        .find(|n| n.name().is_some_and(|x| x.starts_with("hardscape")))
        .unwrap();
    let extras: Value = serde_json::from_str(slab_node.extras().as_ref().unwrap().get()).unwrap();
    assert_eq!(extras["kind"], "hardscape");
    assert!(extras["id"].as_u64().is_some());
}

#[test]
fn gltf_text_embeds_the_buffer_and_matches_the_glb() {
    let doc = document(vec![slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0)]);
    let scene = mesh::build(&doc);
    let (text, stats) = export_gltf(&doc, &scene, &ExportOptions::default(), &BaseColors).unwrap();
    let root: Value = serde_json::from_str(&text).unwrap();
    let uri = root["buffers"][0]["uri"].as_str().unwrap();
    let b64 = uri
        .strip_prefix("data:application/octet-stream;base64,")
        .unwrap();
    use base64::Engine;
    let bin = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    check_structure(&root, &bin);
    assert_eq!(
        bin.len(),
        root["buffers"][0]["byteLength"].as_u64().unwrap() as usize
    );
    let g = gltf::Gltf::from_slice(text.as_bytes()).unwrap();
    assert_eq!(g.document.nodes().count(), 1);
    assert_eq!(stats.bytes, text.len() + bin.len());
    let (bytes, _) = glb(&doc, &ExportOptions::default());
    let (r2, b2) = split_glb(&bytes);
    assert_eq!(b2, bin);
    let mut r1 = root.clone();
    r1["buffers"][0].as_object_mut().unwrap().remove("uri");
    assert_eq!(r1, r2);
}

#[test]
fn units_and_axes_follow_the_options() {
    // A slab from x 1000 to 3000 mm, y 2000 to 5000 mm, top at 0, 300 mm thick.
    let doc = document(vec![slab(1000.0, 2000.0, 3000.0, 5000.0, 300.0, 0.0)]);
    let extent = |opts: &ExportOptions| {
        let (bytes, _) = glb(&doc, opts);
        let (root, bin) = split_glb(&bytes);
        check_structure(&root, &bin);
        let node = &root["nodes"][0];
        let t: Vec<f64> = node
            .get("translation")
            .map(|t| {
                t.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect()
            })
            .unwrap_or(vec![0.0; 3]);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in root["meshes"][0]["primitives"].as_array().unwrap() {
            let a = &root["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize];
            for k in 0..3 {
                lo[k] = lo[k].min(a["min"][k].as_f64().unwrap() + t[k]);
                hi[k] = hi[k].max(a["max"][k].as_f64().unwrap() + t[k]);
            }
        }
        (lo, hi)
    };
    // The scene origin is the middle of the drawing, so compare sizes, which do not depend on it.
    let (lo, hi) = extent(&ExportOptions::default());
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    // Y up: x is x (2 m), y is the height (0.3 m), z is minus y (3 m).
    assert!((size[0] - 2.0).abs() < 1e-5, "{size:?}");
    assert!((size[1] - 0.3).abs() < 1e-5, "{size:?}");
    assert!((size[2] - 3.0).abs() < 1e-5, "{size:?}");
    let (lo, hi) = extent(&ExportOptions {
        y_up: false,
        ..ExportOptions::default()
    });
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    assert!(
        (size[0] - 2.0).abs() < 1e-5
            && (size[1] - 3.0).abs() < 1e-5
            && (size[2] - 0.3).abs() < 1e-5,
        "{size:?}"
    );
    let (lo, hi) = extent(&ExportOptions {
        units: Units::Millimetres,
        ..ExportOptions::default()
    });
    assert!((hi[0] - lo[0] - 2000.0).abs() < 1e-2 && (hi[1] - lo[1] - 300.0).abs() < 1e-2);
    // The top of the slab is at height 0 in the model, and the scene origin has Z zero.
    assert!((hi[1] - 0.0).abs() < 1e-3, "top at zero: {hi:?}");
}

#[test]
fn faces_keep_their_outward_direction_after_the_axis_change() {
    let doc = document(vec![slab(0.0, 0.0, 2000.0, 2000.0, 200.0, 0.0)]);
    let (bytes, _) = glb(&doc, &ExportOptions::default());
    let g = gltf::Gltf::from_slice(&bytes).unwrap();
    let blob = g.blob.clone().unwrap();
    let prim = g
        .document
        .meshes()
        .next()
        .unwrap()
        .primitives()
        .next()
        .unwrap();
    let r = prim.reader(|_| Some(blob.as_slice()));
    let pos: Vec<[f32; 3]> = r.read_positions().unwrap().collect();
    let nrm: Vec<[f32; 3]> = r.read_normals().unwrap().collect();
    let idx: Vec<u32> = r.read_indices().unwrap().into_u32().collect();
    let mid = {
        let mut m = [0.0f32; 3];
        for p in &pos {
            for k in 0..3 {
                m[k] += p[k] / pos.len() as f32;
            }
        }
        m
    };
    for t in idx.chunks(3) {
        let [a, b, c] = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let out = [a[0] - mid[0], a[1] - mid[1], a[2] - mid[2]];
        assert!(
            n[0] * out[0] + n[1] * out[1] + n[2] * out[2] >= -1e-9,
            "triangle faces inward"
        );
        let vn = nrm[t[0] as usize];
        assert!(
            vn[0] * n[0] + vn[1] * n[1] + vn[2] * n[2] >= -1e-9,
            "normal disagrees with winding"
        );
    }
}

#[test]
fn equal_plants_share_one_mesh() {
    let mut shapes: Vec<Shape> = (0..6)
        .map(|i| plant(i as f64 * 5000.0, 1000.0 + i as f64 * 777.0, "Acer"))
        .collect();
    shapes.push(plant(0.0, 20000.0, "Betula"));
    shapes.push(slab(0.0, 30000.0, 4000.0, 34000.0, 150.0, 0.0));
    let doc = document(shapes);
    let (bytes, stats) = glb(&doc, &ExportOptions::default());
    let (root, bin) = split_glb(&bytes);
    check_structure(&root, &bin);
    assert_eq!(stats.nodes, 8);
    assert_eq!(stats.instances, 5, "{stats:?}");
    assert_eq!(stats.meshes, 3);
    assert_eq!(root["meshes"].as_array().unwrap().len(), 3);
    // Every node still has its own place.
    let mut places: Vec<String> = root["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["translation"].to_string())
        .collect();
    places.sort();
    places.dedup();
    assert_eq!(places.len(), 8);
    // Without instancing every node has a mesh, and the file is larger.
    let (plain, s2) = glb(
        &doc,
        &ExportOptions {
            instancing: false,
            ..ExportOptions::default()
        },
    );
    assert_eq!(s2.instances, 0);
    assert_eq!(s2.meshes, 8);
    assert!(plain.len() > bytes.len());
    // Instanced triangles are stored once.
    assert!(s2.triangles > stats.triangles);
}

#[test]
fn output_is_deterministic() {
    let doc = document(vec![
        slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0),
        plant(1000.0, 1000.0, "Acer"),
        plant(5000.0, 1000.0, "Acer"),
    ]);
    let a = glb(&doc, &ExportOptions::default()).0;
    let b = glb(&doc, &ExportOptions::default()).0;
    assert_eq!(a, b);
    let scene = mesh::build(&doc);
    let t1 = export_gltf(&doc, &scene, &ExportOptions::default(), &BaseColors)
        .unwrap()
        .0;
    let t2 = export_gltf(
        &doc,
        &mesh::build(&doc),
        &ExportOptions::default(),
        &BaseColors,
    )
    .unwrap()
    .0;
    assert_eq!(t1, t2);
    let o1 = export_obj(
        &doc,
        &scene,
        &ExportOptions::default(),
        &BaseColors,
        "m.mtl",
    )
    .unwrap()
    .0;
    let o2 = export_obj(
        &doc,
        &scene,
        &ExportOptions::default(),
        &BaseColors,
        "m.mtl",
    )
    .unwrap()
    .0;
    assert_eq!(o1, o2);
}

#[test]
fn grouping_by_layer_and_class_joins_the_meshes() {
    let mut ed = Editor::default();
    let l2 = ed
        .execute(&Command::AddLayer {
            name: "Upper".into(),
            elevation: 1000.0,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0];
    for (i, layer) in [None, None, Some(cadektor_core::model::LayerId(l2))]
        .into_iter()
        .enumerate()
    {
        ed.execute(&Command::AddObject {
            shape: slab(
                i as f64 * 5000.0,
                0.0,
                i as f64 * 5000.0 + 3000.0,
                3000.0,
                150.0,
                0.0,
            ),
            layer,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    let doc = ed.doc().clone();
    let scene = mesh::build(&doc);
    let layered = ExportOptions {
        grouping: Grouping::Layer,
        ..ExportOptions::default()
    };
    let (bytes, stats) = export_glb(&doc, &scene, &layered, &BaseColors).unwrap();
    let (root, bin) = split_glb(&bytes);
    check_structure(&root, &bin);
    assert_eq!(stats.nodes, 2);
    assert_eq!(stats.objects, 3);
    let total: usize = root["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["extras"]["object_ids"].as_array().unwrap().len())
        .sum();
    assert_eq!(total, 3);
    let names: Vec<&str> = root["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Upper"));
    let classed = ExportOptions {
        grouping: Grouping::Class,
        ..ExportOptions::default()
    };
    let (bytes, stats) = export_glb(&doc, &scene, &classed, &BaseColors).unwrap();
    let (root, bin) = split_glb(&bytes);
    check_structure(&root, &bin);
    assert_eq!(stats.nodes, 1);
    assert_eq!(root["nodes"][0]["extras"]["group"], "class");
    // Joined meshes keep the triangle count.
    let (_, per_object) = glb(
        &doc,
        &ExportOptions {
            instancing: false,
            ..ExportOptions::default()
        },
    );
    assert_eq!(stats.triangles, per_object.triangles);
}

#[test]
fn extras_carry_the_data_and_can_be_left_out() {
    let mut doc = document(vec![slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0)]);
    let id = *doc.objects.keys().next().unwrap();
    doc.objects
        .get_mut(&id)
        .unwrap()
        .data
        .insert("Record".into(), serde_json::json!({"Owner": "City"}));
    let (bytes, _) = glb(&doc, &ExportOptions::default());
    let (root, _) = split_glb(&bytes);
    let n = node_by_object(&root, id.0);
    assert_eq!(n["extras"]["data"]["Record"]["Owner"], "City");
    assert_eq!(n["extras"]["layer"], doc.layers[0].name);
    let (bytes, _) = glb(
        &doc,
        &ExportOptions {
            data: false,
            ..ExportOptions::default()
        },
    );
    let (root, _) = split_glb(&bytes);
    assert!(node_by_object(&root, id.0)["extras"].get("data").is_none());
}

#[test]
fn georeference_goes_into_an_extension() {
    let mut doc = document(vec![slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0)]);
    doc.georef = Some(cadektor_core::georef::Georeference {
        crs: "EPSG:3067".into(),
        origin: DVec2::new(385_000.0, 6_672_000.0),
        rotation: 0.0,
        user_origin: DVec2::ZERO,
    });
    let (bytes, _) = glb(&doc, &ExportOptions::default());
    let (root, _) = split_glb(&bytes);
    let ext = &root["scenes"][0]["extensions"][crate::gltf_writer::GEOREF_EXTENSION];
    assert_eq!(ext["crs"], "EPSG:3067");
    assert_eq!(ext["map_origin"][0], 385_000.0);
    assert_eq!(ext["axes"], "y_up");
    assert_eq!(
        root["extensionsUsed"][0],
        crate::gltf_writer::GEOREF_EXTENSION
    );
    // The positions stay local.
    let acc = &root["accessors"][0];
    assert!(acc["max"][0].as_f64().unwrap().abs() < 100.0);
    let (bytes, _) = glb(
        &doc,
        &ExportOptions {
            georef: false,
            ..ExportOptions::default()
        },
    );
    let (root, _) = split_glb(&bytes);
    assert!(root.get("extensionsUsed").is_none());
    assert!(root["scenes"][0].get("extensions").is_none());
}

#[test]
fn a_selection_limits_the_export() {
    let doc = document(vec![
        slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0),
        slab(8000.0, 0.0, 12000.0, 3000.0, 150.0, 0.0),
    ]);
    let ids: Vec<_> = doc.objects.keys().copied().collect();
    let (bytes, stats) = glb(
        &doc,
        &ExportOptions {
            objects: Some(vec![ids[1]]),
            ..ExportOptions::default()
        },
    );
    assert_eq!(stats.objects, 1);
    let (root, _) = split_glb(&bytes);
    assert_eq!(root["nodes"][0]["extras"]["id"], ids[1].0);
    let scene = mesh::build(&doc);
    let none = export_glb(
        &doc,
        &scene,
        &ExportOptions {
            objects: Some(vec![]),
            ..ExportOptions::default()
        },
        &BaseColors,
    );
    assert_eq!(none.unwrap_err(), X3dError::Empty);
}

#[test]
fn bad_input_gives_errors() {
    let empty = Document::default();
    let scene = mesh::build(&empty);
    for r in [
        export_glb(&empty, &scene, &ExportOptions::default(), &BaseColors).map(|_| ()),
        export_gltf(&empty, &scene, &ExportOptions::default(), &BaseColors).map(|_| ()),
        export_obj(
            &empty,
            &scene,
            &ExportOptions::default(),
            &BaseColors,
            "a.mtl",
        )
        .map(|_| ()),
    ] {
        assert_eq!(r.unwrap_err(), X3dError::Empty);
    }
    assert!(X3dError::Empty.to_string().contains("no 3D objects"));
    let doc = document(vec![slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0)]);
    let scene = mesh::build(&doc);
    let bad = ExportOptions {
        instance_tolerance: f64::NAN,
        ..ExportOptions::default()
    };
    assert!(matches!(
        export_glb(&doc, &scene, &bad, &BaseColors),
        Err(X3dError::Options(_))
    ));
    // Unknown option names are refused when options come from JSON.
    assert!(
        serde_json::from_value::<ExportOptions>(
            serde_json::json!({"grouping": "layer", "units": "mm"})
        )
        .is_ok()
    );
    assert!(serde_json::from_value::<ExportOptions>(serde_json::json!({"bogus": 1})).is_err());
    assert!(
        serde_json::from_value::<ExportOptions>(serde_json::json!({"units": "yards"})).is_err()
    );
}

/// A source that gives a metal, a transparent material and a textured one.
struct Rich;

impl MaterialSource for Rich {
    fn material(&self, _doc: &Document, obj: &Object, color: [u8; 4]) -> Material {
        let mut m = Material::from_color(color);
        match obj.shape.kind() {
            "hardscape" => {
                m.name = "Paving".into();
                m.texture = Some(Texture {
                    name: "paving stones".into(),
                    mime: "image/png".into(),
                    data: vec![1, 2, 3, 4, 5],
                    repeat: 0.5,
                });
                m.roughness = 0.4;
            }
            _ => {
                m.metallic = 1.0;
                m.emissive = [0.1, 0.0, 0.0];
                m.double_sided = true;
                m.base_color[3] = 0.5;
            }
        }
        m
    }
}

#[test]
fn materials_and_textures_flow_through_the_hook() {
    let doc = document(vec![
        slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0),
        slab(8000.0, 0.0, 12000.0, 3000.0, 150.0, 0.0),
        plant(2000.0, 8000.0, "Acer"),
    ]);
    let scene = mesh::build(&doc);
    let (bytes, stats) = export_glb(&doc, &scene, &ExportOptions::default(), &Rich).unwrap();
    let (root, bin) = split_glb(&bytes);
    check_structure(&root, &bin);
    assert_eq!(stats.textures, 1, "one image for two slabs");
    assert_eq!(root["images"].as_array().unwrap().len(), 1);
    assert_eq!(root["images"][0]["mimeType"], "image/png");
    let paving = root["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "Paving")
        .unwrap();
    assert!(
        (paving["pbrMetallicRoughness"]["roughnessFactor"]
            .as_f64()
            .unwrap()
            - 0.4)
            .abs()
            < 1e-6
    );
    assert_eq!(
        paving["pbrMetallicRoughness"]["baseColorTexture"]["index"],
        0
    );
    let other = root["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] != "Paving")
        .unwrap();
    assert_eq!(other["alphaMode"], "BLEND");
    assert_eq!(other["doubleSided"], true);
    assert_eq!(other["pbrMetallicRoughness"]["metallicFactor"], 1.0);
    assert!(other["emissiveFactor"].is_array());
    // Textured primitives have texture coordinates.
    let textured = root["meshes"][0]["primitives"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["attributes"].get("TEXCOORD_0").is_some());
    assert!(textured);
    gltf::Gltf::from_slice(&bytes).unwrap();
    // OBJ gets the texture as a file.
    let (files, s) =
        export_obj(&doc, &scene, &ExportOptions::default(), &Rich, "scene.mtl").unwrap();
    assert_eq!(files.textures.len(), 1);
    assert!(
        files
            .mtl
            .contains(&format!("map_Kd {}", files.textures[0].0))
    );
    assert!(files.mtl.contains("Ke 0.1000"));
    assert!(files.obj.contains("vt "));
    assert_eq!(s.textures, 1);
}

#[test]
fn obj_has_valid_indices_and_materials() {
    let doc = document(vec![
        slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0),
        plant(1000.0, 1000.0, "Acer"),
        Shape::Polyline3d {
            points: vec![glam::DVec3::ZERO, glam::DVec3::new(1000.0, 0.0, 500.0)],
            closed: false,
        },
    ]);
    let scene = mesh::build(&doc);
    let (files, stats) = export_obj(
        &doc,
        &scene,
        &ExportOptions::default(),
        &BaseColors,
        "model.mtl",
    )
    .unwrap();
    assert!(files.obj.contains("mtllib model.mtl"));
    let count = |p: &str| files.obj.lines().filter(|l| l.starts_with(p)).count();
    let (v, vn) = (count("v "), count("vn "));
    assert!(v > 0 && vn > 0 && vn <= v);
    assert_eq!(count("f "), stats.triangles);
    assert_eq!(count("l "), 1);
    assert_eq!(count("o "), 3);
    for line in files.obj.lines().filter(|l| l.starts_with("f ")) {
        for corner in line[2..].split(' ') {
            let parts: Vec<&str> = corner.split('/').collect();
            assert!(parts[0].parse::<usize>().unwrap() <= v);
            assert!(parts[2].parse::<usize>().unwrap() <= vn);
        }
    }
    for line in files.obj.lines().filter(|l| l.starts_with("usemtl ")) {
        assert!(files.mtl.contains(&format!("newmtl {}", &line[7..])));
    }
    assert!(files.mtl.contains("Kd "));
    assert!(files.mtl.contains("d 1.0000"));
    // Millimetres in Z up keeps the model numbers: the slab is 4000 wide.
    let (files, _) = export_obj(
        &doc,
        &scene,
        &ExportOptions {
            units: Units::Millimetres,
            y_up: false,
            ..ExportOptions::default()
        },
        &BaseColors,
        "model.mtl",
    )
    .unwrap();
    let xs: Vec<f64> = files
        .obj
        .lines()
        .filter(|l| l.starts_with("v "))
        .map(|l| l.split(' ').nth(1).unwrap().parse().unwrap())
        .collect();
    let width =
        xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
    assert!(width >= 4000.0 - 1e-3, "{width}");
    // Group names do not hold spaces, and a name that is empty gets a replacement.
    assert!(
        files
            .obj
            .lines()
            .filter(|l| l.starts_with("o "))
            .all(|l| !l[2..].contains(' '))
    );
    assert_eq!(crate::obj::clean(""), "_");
    // The georeference is a comment.
    let mut g = doc.clone();
    g.georef = Some(cadektor_core::georef::Georeference {
        crs: "EPSG:3067".into(),
        origin: DVec2::new(1.0, 2.0),
        rotation: 0.0,
        user_origin: DVec2::ZERO,
    });
    let (files, _) = export_obj(
        &g,
        &scene,
        &ExportOptions::default(),
        &BaseColors,
        "model.mtl",
    )
    .unwrap();
    assert!(files.obj.contains("# crs EPSG:3067"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn random_documents_give_valid_gltf(
        slabs in proptest::collection::vec((-50_000.0f64..50_000.0, -50_000.0f64..50_000.0, 500.0f64..9_000.0, 500.0f64..9_000.0, 20.0f64..800.0), 0..6),
        plants in proptest::collection::vec((-50_000.0f64..50_000.0, -50_000.0f64..50_000.0, 0usize..3), 0..30),
        y_up in any::<bool>(),
        mm in any::<bool>(),
        grouping in 0usize..3,
        instancing in any::<bool>(),
    ) {
        let mut shapes: Vec<Shape> = slabs.iter().map(|(x, y, w, h, t)| slab(*x, *y, x + w, y + h, *t, 0.0)).collect();
        shapes.extend(plants.iter().map(|(x, y, s)| plant(*x, *y, ["Acer", "Betula", "Pinus"][*s])));
        prop_assume!(!shapes.is_empty());
        let doc = document(shapes);
        let opts = ExportOptions {
            grouping: [Grouping::Object, Grouping::Layer, Grouping::Class][grouping],
            units: if mm { Units::Millimetres } else { Units::Metres },
            y_up,
            instancing,
            ..ExportOptions::default()
        };
        let scene = mesh::build(&doc);
        let (bytes, stats) = export_glb(&doc, &scene, &opts, &BaseColors).unwrap();
        let (root, bin) = split_glb(&bytes);
        check_structure(&root, &bin);
        prop_assert!(gltf::Gltf::from_slice(&bytes).is_ok());
        prop_assert_eq!(stats.objects, doc.objects.len());
        // Instancing never changes the number of nodes, only the number of meshes.
        if grouping == 0 {
            prop_assert_eq!(stats.nodes, doc.objects.len());
            prop_assert_eq!(stats.meshes + stats.instances, stats.nodes);
        }
        // The same input gives the same bytes.
        let (again, _) = export_glb(&doc, &scene, &opts, &BaseColors).unwrap();
        prop_assert_eq!(bytes, again);
        // OBJ indexes stay in range.
        let (files, s) = export_obj(&doc, &scene, &opts, &BaseColors, "a.mtl").unwrap();
        let v = files.obj.lines().filter(|l| l.starts_with("v ")).count();
        for line in files.obj.lines().filter(|l| l.starts_with("f ")) {
            for corner in line[2..].split(' ') {
                prop_assert!(corner.split('/').next().unwrap().parse::<usize>().unwrap() <= v);
            }
        }
        prop_assert!(s.triangles >= stats.triangles);
    }
}

#[test]
fn document_materials_give_colors_textures_and_uvs_to_the_exports() {
    use cadektor_core::model::{Color, ObjectId};
    use cadektor_core::present::{
        Material as DocMaterial, Procedural, RenderResource, SlotTarget, Texture as DocTexture,
    };
    use cadektor_core::resource::{ResourceBody, ResourceId};

    let mut ed = Editor::default();
    let mut ids = Vec::new();
    for shape in [
        slab(0.0, 0.0, 4000.0, 3000.0, 150.0, 0.0),
        slab(5000.0, 0.0, 9000.0, 3000.0, 150.0, 0.0),
        slab(10_000.0, 0.0, 14_000.0, 3000.0, 150.0, 0.0),
    ] {
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        ids.push(ObjectId(out.created[0]));
    }
    let mut add_material = |name: &str, m: DocMaterial| {
        let out = ed
            .execute(&Command::AddResource {
                name: name.into(),
                body: ResourceBody::Render(Box::new(RenderResource::Material(m))),
            })
            .unwrap();
        ResourceId(out.created[0])
    };
    let glass = add_material(
        "Glass",
        DocMaterial {
            base_color: Color([100, 150, 200, 255]),
            opacity: 0.4,
            metallic: 0.2,
            ..Default::default()
        },
    );
    let timber = add_material(
        "Timber",
        DocMaterial {
            base_color: Color([150, 100, 60, 255]),
            texture: Some(DocTexture::Procedural {
                kind: Procedural::Timber,
                seed: 3,
            }),
            ..Default::default()
        },
    );
    for (id, m) in [(ids[0], glass), (ids[1], timber)] {
        ed.execute(&Command::SetMaterialSlot {
            target: SlotTarget::Object {
                object: id,
                slot: None,
            },
            material: Some(m),
        })
        .unwrap();
    }
    let doc = ed.doc().clone();
    let scene = mesh::build(&doc);
    let src = DocumentMaterials::new(&doc, &scene);
    assert!(!src.is_plain());
    let opts = ExportOptions::default();

    let (bytes, _) = export_glb(&doc, &scene, &opts, &src).unwrap();
    let (root, bin) = split_glb(&bytes);
    check_structure(&root, &bin);
    let materials = root["materials"].as_array().unwrap();
    assert!(
        materials
            .iter()
            .any(|m| m["name"] == "Glass" && m["alphaMode"] == "BLEND")
    );
    assert!(materials.iter().any(|m| m["name"] == "Timber"));
    assert_eq!(root["images"].as_array().unwrap().len(), 1);
    assert!(root["textures"].is_array());

    let (text, _) = export_gltf(&doc, &scene, &opts, &src).unwrap();
    assert!(text.contains("\"Timber\""));
    let (files, _) = export_obj(&doc, &scene, &opts, &src, "m.mtl").unwrap();
    assert!(files.mtl.contains("Timber") && files.mtl.contains("Glass"));
    assert!(files.obj.contains("vt "), "texture coordinates");

    // The trait answers for triangles without a slot, for a vertex outside the scene and for
    // a group out of range.
    let obj = doc.object(ids[2]).unwrap();
    let plain = src.material_for(&doc, obj, 0, [9, 9, 9, 255]);
    assert!(plain.name.starts_with("color_") || !plain.name.is_empty());
    assert_eq!(
        src.material(&doc, obj, [1, 2, 3, 255]),
        Material::from_color([1, 2, 3, 255])
    );
    assert_eq!(src.group(usize::MAX), u32::MAX);
    assert_eq!(
        src.material_for(&doc, obj, usize::MAX, [1, 2, 3, 255]),
        Material::from_color([1, 2, 3, 255])
    );
    assert_eq!(src.uv(usize::MAX), None);
    // The base colors give no uv and one group.
    assert_eq!(BaseColors.group(3), 0);
    assert_eq!(BaseColors.uv(3), None);
    assert_eq!(
        BaseColors.material_for(&doc, obj, 0, [1, 2, 3, 255]),
        Material::from_color([1, 2, 3, 255])
    );
}
