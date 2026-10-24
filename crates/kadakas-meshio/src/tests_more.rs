//! More tests of the readers: the paths of the refusals, the rare options of the formats and
//! the small helpers.

use super::*;

fn limits() -> Limits {
    Limits::default()
}

fn none(_: &str) -> Option<Vec<u8>> {
    None
}

#[test]
fn every_error_has_a_kind_details_and_text() {
    let all = [
        MeshError::FileTooLarge { bytes: 5, limit: 4 },
        MeshError::TooManyTriangles { count: 5, limit: 4 },
        MeshError::TooManyVertices { count: 5, limit: 4 },
        MeshError::TooMuchMemory { bytes: 5, limit: 4 },
        MeshError::TooManyParts { count: 5, limit: 4 },
        MeshError::Truncated {
            what: "x".into(),
            have: 1,
            need: 2,
        },
        MeshError::Malformed("m".into()),
        MeshError::NotFinite { what: "v".into() },
        MeshError::OutOfRange {
            what: "v".into(),
            value: 2.0,
            limit: 1.0,
        },
        MeshError::BadIndex {
            what: "f".into(),
            index: 9,
            count: 3,
        },
        MeshError::Unsupported("u".into()),
        MeshError::Empty,
        MeshError::TooMuchWork { units: 5, limit: 4 },
        MeshError::TooManyEntities { count: 5, limit: 4 },
        MeshError::TooManyInstances { count: 5, limit: 4 },
        MeshError::TooManyCopies { count: 5, limit: 4 },
        MeshError::TooMuchTime {
            millis: 5,
            limit: 4,
        },
    ];
    let mut kinds = std::collections::BTreeSet::new();
    for e in &all {
        assert!(!e.to_string().is_empty());
        assert!(e.kind().starts_with("mesh_"));
        kinds.insert(e.kind());
        assert!(e.details().is_object());
    }
    assert_eq!(kinds.len(), all.len(), "each kind has its own name");
    assert_eq!(all.iter().filter(|e| e.is_limit()).count(), 10);
    assert_eq!(all[12].details()["units"], 5);
    assert_eq!(all[13].details()["count"], 5);
    assert_eq!(all[16].details()["millis"], 5);
    assert_eq!(all[0].details()["limit"], 4);
    assert_eq!(all[5].details()["need"], 2);
    assert_eq!(all[9].details()["index"], 9);
    assert_eq!(all[8].details()["value"], 2.0);
    assert_eq!(all[1].details()["count"], 5);
    assert_eq!(all[3].details()["bytes"], 5);
    assert_eq!(all[6].details(), serde_json::json!({}));
}

#[test]
fn limits_name_the_counts_they_stop_at() {
    let l = Limits {
        max_vertices: 10,
        max_triangles: 10,
        max_memory: 500,
        ..limits()
    };
    assert!(l.check_counts(5, 5).is_ok());
    assert!(matches!(
        l.check_counts(11, 5),
        Err(MeshError::TooManyVertices {
            count: 11,
            limit: 10
        })
    ));
    assert!(matches!(
        l.check_counts(5, 11),
        Err(MeshError::TooManyTriangles { .. })
    ));
    assert!(matches!(
        l.check_counts(10, 10),
        Err(MeshError::TooMuchMemory { .. })
    ));
    assert!(l.check_file(l.max_bytes).is_ok());
    assert!(l.check_file(l.max_bytes + 1).is_err());
    assert_eq!(Limits::memory_of(1, 1), (24 + 12) * 2);
}

#[test]
fn formats_units_and_axes_have_names() {
    for f in [Format::Obj, Format::Stl, Format::Gltf, Format::Glb] {
        assert_eq!(Format::from_name(f.name()), Some(f));
        let frame = f.default_frame();
        assert_eq!(frame.up == Up::Y, f != Format::Stl);
    }
    assert_eq!(Format::from_name("dxf"), None);
    for u in Unit::ALL {
        assert_eq!(Unit::from_name(u.id()), Some(u));
        assert!(u.mm() > 0.0);
    }
    assert_eq!(Unit::from_name("meters"), Some(Unit::Metre));
    assert_eq!(Unit::from_name("ft"), Some(Unit::Foot));
    assert_eq!(Unit::from_name("yd"), Some(Unit::Yard));
    assert_eq!(Unit::from_name("mi"), Some(Unit::Mile));
    assert_eq!(Unit::from_name("km"), Some(Unit::Kilometre));
    assert_eq!(Unit::from_name("centimeter"), Some(Unit::Centimetre));
    assert_eq!(Unit::from_name("millimeter"), Some(Unit::Millimetre));
    assert_eq!(Up::Y.id(), "y");
    assert_eq!(Up::Z.id(), "z");
    assert_eq!(Up::from_name("Y_UP"), Some(Up::Y));
    assert_eq!(Up::from_name("z_up"), Some(Up::Z));
    assert_eq!(Up::from_name("x"), None);
}

#[test]
fn obj_covers_the_rare_lines() {
    let o = obj::ObjOptions {
        frame: Frame {
            unit: Unit::Millimetre,
            up: Up::Z,
        },
        limits: limits(),
    };
    // A bow tie falls back to a fan and is noted. Groups name the parts. Normals, texture
    // coordinates and smoothing lines are ignored. A face with a repeated corner is counted.
    let text = "g Top\nv 0 0 0\nv 2 2 0\nv 2 0 0\nv 0 2 0\nvn 0 0 1\nvt 0 0\ns off\nl 1 2\nf 1/1/1 2/1/1 3/1/1 4/1/1\nf 1 1 2\n";
    let m = obj::read(text.as_bytes(), &o, &none).unwrap();
    assert_eq!(m.parts[0].name, "Top");
    assert!(m.notes.iter().any(|n| n.contains("cross themselves")));
    assert_eq!(m.totals.degenerate, 1);
    // A face that is too big.
    let mut big = String::from("v 0 0 0\nf");
    for _ in 0..10_001 {
        big += " 1";
    }
    assert!(matches!(
        obj::read(big.as_bytes(), &o, &none),
        Err(MeshError::Malformed(_))
    ));
    // Too many parts while reading.
    let mut many = String::from("v 0 0 0\nv 1 0 0\nv 0 1 0\n");
    for i in 0..100_001 {
        many += &format!("o part{i}\nf 1 2 3\n");
    }
    assert!(matches!(
        obj::read(many.as_bytes(), &o, &none),
        Err(MeshError::TooManyParts { .. })
    ));
    // The material file with spaces, found by the whole text; a missing one is noted once.
    let spaced =
        "mtllib my file.mtl\nmtllib my file.mtl\nusemtl a\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let found = |n: &str| (n == "my file.mtl").then(|| b"newmtl a\nKd 0 0 1\n".to_vec());
    let m = obj::read(spaced.as_bytes(), &o, &found).unwrap();
    assert_eq!(m.parts[0].color, Some([0, 0, 255, 255]));
    let m = obj::read(spaced.as_bytes(), &o, &none).unwrap();
    assert_eq!(
        m.notes.iter().filter(|n| n.contains("my file.mtl")).count(),
        1
    );
    // Bytes that are not UTF-8 in a name do not matter.
    let latin = b"o caf\xe9\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec();
    let m = obj::read(&latin, &o, &none).unwrap();
    assert!(m.parts[0].name.starts_with("caf"));
    // Colors: Tr, a bad Kd, a big MTL.
    let c = obj::parse_mtl(b"newmtl t\nKd 0.2 0.4 0.6\nTr 0.5\nnewmtl bad\nd x\nKd 1 1 1\n");
    assert_eq!(c["t"], [51, 102, 153, 128]);
    assert_eq!(c["bad"], [255, 255, 255, 255]);
    assert!(obj::parse_mtl(&vec![b'a'; 17 * 1024 * 1024]).is_empty());
    // A part that is merged by color gets the name of the material.
    let two = "usemtl a\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\no b\nf 1 2 3\n";
    let few = obj::ObjOptions {
        limits: Limits {
            max_parts: 1,
            ..limits()
        },
        ..o.clone()
    };
    let m = obj::read(two.as_bytes(), &few, &|_| {
        Some(b"newmtl a\nKd 1 0 0\n".to_vec())
    })
    .unwrap();
    assert_eq!(m.parts.len(), 1);
    assert_eq!(m.parts[0].name, "a");
    // Parts that cannot be joined (different colors) are refused with the count.
    let colors = "mtllib c.mtl\nusemtl a\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\nusemtl b\nf 1 2 3\n";
    let mtl = |_: &str| Some(b"newmtl a\nKd 1 0 0\nnewmtl b\nKd 0 1 0\n".to_vec());
    assert!(matches!(
        obj::read(colors.as_bytes(), &few, &mtl),
        Err(MeshError::TooManyParts { count: 2, limit: 1 })
    ));
}

fn write_ascii_facet(corners: &[[f32; 3]]) -> String {
    let mut s = String::from("facet normal 0 0 1\nouter loop\n");
    for c in corners {
        s += &format!("vertex {} {} {}\n", c[0], c[1], c[2]);
    }
    s + "endloop\nendfacet\n"
}

#[test]
fn stl_covers_the_ascii_paths() {
    let o = stl::StlOptions::default();
    let tri = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    // A facet inside a facet, a vertex with two numbers, a facet with two vertices.
    for text in [
        format!("solid x\nfacet normal 0 0 1\n{}endsolid\n", write_ascii_facet(&tri)),
        "solid x\nfacet normal 0 0 1\nouter loop\nvertex 1 2\nendloop\nendfacet\nendsolid\n".into(),
        "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nendloop\nendfacet\nendsolid\n".into(),
        "solid x\nvertex 0 0 0\nendsolid\n".into(),
        "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendsolid\n".into(),
    ] {
        assert!(stl::read(text.as_bytes(), &o).is_err(), "{text}");
    }
    // Empty lines, an unnamed solid, a degenerate facet, uppercase keywords.
    let text = format!(
        "solid\n\n{}{}ENDSOLID\n",
        write_ascii_facet(&tri),
        write_ascii_facet(&[[0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]])
    );
    let m = stl::read(text.as_bytes(), &o).unwrap();
    assert_eq!(m.parts[0].name, "mesh");
    assert_eq!((m.totals.triangles, m.totals.degenerate), (1, 1));
    // A binary file with a degenerate triangle only is empty.
    let bin = stl::write_binary("x", vec![[[0.0f32; 3]; 3]].into_iter()).unwrap();
    assert!(matches!(stl::read(&bin, &o), Err(MeshError::Empty)));
    // A file that is neither: short bytes.
    assert!(matches!(
        stl::read(b"abc", &o),
        Err(MeshError::Truncated { .. })
    ));
    // A name with control characters, and an empty name, in the ASCII writer.
    let asc = stl::write_ascii("a\nb", std::iter::empty());
    assert!(asc.starts_with("solid a b\n"));
    assert!(stl::write_ascii("  ", std::iter::empty()).starts_with("solid mesh\n"));
    // A triangle with no area gets a zero normal.
    let flat = stl::write_binary(
        "x",
        vec![[[0.0f32; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]].into_iter(),
    )
    .unwrap();
    assert_eq!(&flat[84..96], &[0u8; 12]);
}

fn simple(json_extra: &str) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0"}},{json_extra}"nodes":[{{"name":"n","mesh":0}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5121,"count":3,"type":"SCALAR"}}],
"bufferViews":[{{"buffer":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":3}}],"buffers":[{{"byteLength":39,"uri":"data:application/octet-stream;base64,BASE"}}]}}"#
    )
}

fn tri_data() -> Vec<u8> {
    let mut b = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&[0, 1, 2]);
    b
}

fn with_data(json: &str, data: &[u8]) -> String {
    use base64::Engine;
    json.replace(
        "BASE",
        &base64::engine::general_purpose::STANDARD.encode(data),
    )
}

#[test]
fn gltf_covers_matrices_rotations_strides_and_materials() {
    let g = gltf::GltfOptions::default();
    // Unsigned byte indices work.
    let text = with_data(&simple(""), &tri_data());
    let m = gltf::read(text.as_bytes(), &g, &none).unwrap();
    assert_eq!(m.totals.triangles, 1);
    // A node matrix: a translation of 2 m along x.
    let moved = with_data(
        &simple("").replace(
            r#""nodes":[{"name":"n","mesh":0}]"#,
            r#""nodes":[{"name":"n","mesh":0,"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,2,0,0,1]}]"#,
        ),
        &tri_data(),
    );
    let m = gltf::read(moved.as_bytes(), &g, &none).unwrap();
    assert_eq!(m.parts[0].positions[0], [2000.0, 0.0, 0.0]);
    // A turn of 90 degrees about Y (the up axis): x becomes -z, which is y in the model.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let turned = with_data(
        &simple("").replace(
            r#""nodes":[{"name":"n","mesh":0}]"#,
            &format!(r#""nodes":[{{"name":"n","mesh":0,"rotation":[0,{s},0,{s}]}}]"#),
        ),
        &tri_data(),
    );
    let m = gltf::read(turned.as_bytes(), &g, &none).unwrap();
    let p = m.parts[0].positions[1];
    assert!((p[0]).abs() < 1e-6 && (p[1] - 1000.0).abs() < 1e-6, "{p:?}");
    // Bad scale, bad matrix values.
    for node in [
        r#"{"mesh":0,"scale":[1,1]}"#,
        r#"{"mesh":0,"translation":[1,"a",3]}"#,
        r#"{"mesh":0,"matrix":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,1e999]}"#,
    ] {
        let t = with_data(
            &simple("").replace(r#"{"name":"n","mesh":0}"#, node),
            &tri_data(),
        );
        assert!(gltf::read(t.as_bytes(), &g, &none).is_err(), "{node}");
    }
    // Interleaved positions with a stride, and 32 bit indices.
    let mut inter = Vec::new();
    for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for v in p {
            inter.extend_from_slice(&v.to_le_bytes());
        }
        inter.extend_from_slice(&0.5f32.to_le_bytes());
    }
    for i in [0u32, 1, 2] {
        inter.extend_from_slice(&i.to_le_bytes());
    }
    let strided = r#"{"asset":{"version":"2.0"},"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"},{"bufferView":1,"componentType":5125,"count":3,"type":"SCALAR"}],
"bufferViews":[{"buffer":0,"byteLength":48,"byteStride":16},{"buffer":0,"byteOffset":48,"byteLength":12}],"buffers":[{"byteLength":60,"uri":"data:;base64,BASE"}]}"#;
    let t = with_data(strided, &inter);
    let m = gltf::read(t.as_bytes(), &g, &none).unwrap();
    assert_eq!(m.parts[0].positions[2], [0.0, 0.0, 1000.0]);
    assert_eq!(m.parts[0].triangles, vec![[0, 1, 2]]);
    // Materials: a texture is noted, the old diffuse factor is used, a bad factor gives no color.
    let mat = |m: &str| {
        with_data(
            &simple("")
                .replace(r#""nodes""#, &format!(r#""materials":[{m}],"nodes""#))
                .replace(r#""indices":1"#, r#""indices":1,"material":0"#),
            &tri_data(),
        )
    };
    let t = mat(r#"{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}"#);
    let m = gltf::read(t.as_bytes(), &g, &none).unwrap();
    assert!(m.notes.iter().any(|n| n.contains("texture")));
    assert_eq!(m.parts[0].color, Some([255, 255, 255, 255]));
    let t = mat(
        r#"{"extensions":{"KHR_materials_pbrSpecularGlossiness":{"diffuseFactor":[1,0,0,0.5]}}}"#,
    );
    let m = gltf::read(t.as_bytes(), &g, &none).unwrap();
    assert_eq!(m.parts[0].color, Some([255, 0, 0, 128]));
    let t = mat(r#"{"pbrMetallicRoughness":{"baseColorFactor":[1,0]}}"#);
    assert_eq!(
        gltf::read(t.as_bytes(), &g, &none).unwrap().parts[0].color,
        None
    );
    // Nodes without scenes: the roots are the nodes that no node names as a child.
    let t = with_data(
        &simple("").replace(
            r#""nodes":[{"name":"n","mesh":0}]"#,
            r#""nodes":[{"children":[1]},{"name":"n","mesh":0}]"#,
        ),
        &tri_data(),
    );
    assert_eq!(
        gltf::read(t.as_bytes(), &g, &none)
            .unwrap()
            .totals
            .triangles,
        1
    );
    // A scene that does not exist.
    let t = with_data(
        &simple(r#""scene":4,"scenes":[{"nodes":[0]}],"#),
        &tri_data(),
    );
    assert!(gltf::read(t.as_bytes(), &g, &none).is_err());
}

#[test]
fn gltf_covers_the_refusals_of_accessors_and_buffers() {
    let g = gltf::GltfOptions::default();
    let base = with_data(&simple(""), &tri_data());
    let cases: Vec<(String, &str)> = vec![
        (
            base.replace(r#""POSITION":0"#, r#""POSITION":9"#),
            "missing accessor",
        ),
        (
            base.replace(r#""indices":1"#, r#""indices":9"#),
            "missing index accessor",
        ),
        (
            base.replace(r#""attributes":{"POSITION":0},"#, r#""attributes":{},"#),
            "no position",
        ),
        (
            base.replace(
                r#""bufferView":0,"componentType":5126,"count":3,"type":"VEC3""#,
                r#""componentType":5126,"count":3,"type":"VEC3""#,
            ),
            "no view",
        ),
        (
            base.replace(
                r#""bufferView":0,"componentType":5126"#,
                r#""bufferView":7,"componentType":5126"#,
            ),
            "bad view",
        ),
        (
            base.replace(r#""componentType":5121"#, r#""componentType":5126"#),
            "float indices",
        ),
        (
            base.replace(
                r#""componentType":5126,"count":3,"type":"VEC3""#,
                r#""componentType":5126,"count":3,"type":"VEC3","normalized":true"#,
            ),
            "normalized",
        ),
        (
            base.replace(
                r#""primitives":[{"#,
                r#""primitives":[{"extensions":{"KHR_draco_mesh_compression":{}},"#,
            ),
            "draco primitive",
        ),
        (
            base.replace(
                r#""asset""#,
                r#""extensionsRequired":["KHR_mesh_quantization"],"asset""#,
            ),
            "quantization",
        ),
        (
            base.replace(
                "data:application/octet-stream;base64,",
                "data:application/octet-stream;base64",
            ),
            "no comma",
        ),
        (
            base.replace(
                "data:application/octet-stream;base64,",
                "data:application/octet-stream;base64,@@@",
            ),
            "bad base64",
        ),
    ];
    for (text, why) in cases {
        assert!(gltf::read(text.as_bytes(), &g, &none).is_err(), "{why}");
    }
    // A data URI that claims more than the limit, an external file that is too big.
    let small = gltf::GltfOptions {
        limits: Limits {
            max_bytes: 1000,
            ..limits()
        },
    };
    let ext = simple("").replace("data:application/octet-stream;base64,BASE", "a.bin");
    let big_file = |_: &str| Some(vec![0u8; 5000]);
    assert!(matches!(
        gltf::read(ext.as_bytes(), &small, &big_file),
        Err(MeshError::FileTooLarge { bytes: 5000, .. })
    ));
    // Part and position limits.
    let tiny = gltf::GltfOptions {
        limits: Limits {
            max_coord_mm: 10.0,
            ..limits()
        },
    };
    assert!(matches!(
        gltf::read(base.as_bytes(), &tiny, &none),
        Err(MeshError::OutOfRange { .. })
    ));
    // Many nodes with different names exceed the part limit.
    let many_nodes: Vec<String> = (0..30)
        .map(|i| format!(r#"{{"name":"n{i}","mesh":0}}"#))
        .collect();
    let t = with_data(
        &simple("").replace(
            r#""nodes":[{"name":"n","mesh":0}]"#,
            &format!(r#""nodes":[{}]"#, many_nodes.join(",")),
        ),
        &tri_data(),
    );
    let few = gltf::GltfOptions {
        limits: Limits {
            max_parts: 1,
            ..limits()
        },
    };
    assert!(matches!(
        gltf::read(t.as_bytes(), &few, &none),
        Err(MeshError::TooManyParts { .. })
    ));
    // A node that is used by two parents is read twice (an instance), and too many triangles stop.
    let two = gltf::GltfOptions {
        limits: Limits {
            max_triangles: 1,
            ..limits()
        },
    };
    let t = with_data(
        &simple("").replace(
            r#""nodes":[{"name":"n","mesh":0}]"#,
            r#""nodes":[{"children":[1,1]},{"name":"n","mesh":0}]"#,
        ),
        &tri_data(),
    );
    assert!(matches!(
        gltf::read(t.as_bytes(), &two, &none),
        Err(MeshError::TooManyTriangles { .. })
    ));
}

#[test]
fn glb_covers_the_chunk_rules() {
    let g = gltf::GltfOptions::default();
    let json = br#"{"asset":{"version":"2.0"}}"#;
    // No JSON chunk.
    let mut glb = b"glTF".to_vec();
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&20u32.to_le_bytes());
    glb.extend_from_slice(&0u32.to_le_bytes());
    glb.extend_from_slice(&0x004E4942u32.to_le_bytes());
    assert!(gltf::read(&glb, &g, &none).is_err());
    // A chunk longer than the file.
    let mut bad = b"glTF".to_vec();
    bad.extend_from_slice(&2u32.to_le_bytes());
    bad.extend_from_slice(&40u32.to_le_bytes());
    bad.extend_from_slice(&1000u32.to_le_bytes());
    bad.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
    bad.extend_from_slice(json);
    bad.resize(40, 0);
    assert!(matches!(
        gltf::read(&bad, &g, &none),
        Err(MeshError::Truncated { .. })
    ));
    // A model with no meshes is empty, and a buffer without a URI uses the binary chunk.
    let mut ok = b"glTF".to_vec();
    let body = {
        let mut j = json.to_vec();
        while !j.len().is_multiple_of(4) {
            j.push(b' ');
        }
        j
    };
    ok.extend_from_slice(&2u32.to_le_bytes());
    ok.extend_from_slice(&((12 + 8 + body.len()) as u32).to_le_bytes());
    ok.extend_from_slice(&(body.len() as u32).to_le_bytes());
    ok.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
    ok.extend_from_slice(&body);
    assert!(matches!(gltf::read(&ok, &g, &none), Err(MeshError::Empty)));
}

#[test]
fn model_points_are_checked_after_the_frame() {
    let frame = Format::Gltf.default_frame();
    let p = mesh::model_point(frame, [1.0, 2.0, 3.0], &limits(), &|| "p".into()).unwrap();
    assert_eq!(p, [1000.0, -3000.0, 2000.0]);
    assert!(mesh::model_point(frame, [f64::NAN, 0.0, 0.0], &limits(), &|| "p".into()).is_err());
    assert!(mesh::model_point(frame, [1.0e300, 0.0, 0.0], &limits(), &|| "p".into()).is_err());
    let set = MeshSet::finish(
        Format::Obj,
        frame,
        vec![Part {
            name: "a".into(),
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            color: Some([1, 2, 3, 4]),
            ..Default::default()
        }],
        0,
        vec![],
        &limits(),
    )
    .unwrap();
    assert_eq!(set.bounds(), ([0.0; 3], [1.0, 1.0, 0.0]));
    assert_eq!(set.joined().color, Some([1, 2, 3, 4]));
    assert_eq!(set.joined().name, "mesh");
}

#[test]
fn a_node_tree_cannot_use_more_than_its_budget() {
    let g = gltf::GltfOptions::default();
    let head = r#"{"asset":{"version":"2.0"},"nodes":["#;
    // Wide: one node names the same child 150,000 times. The stack must not grow first.
    let kids = vec!["1"; 150_000].join(",");
    let wide = format!(r#"{head}{{"children":[{kids}]}},{{}}]}}"#);
    let start = std::time::Instant::now();
    assert!(matches!(
        gltf::read(wide.as_bytes(), &g, &none),
        Err(MeshError::Malformed(m)) if m.contains("used more than")
    ));
    // Deep and fat: every node names the next one 20 times. The depth stops it at once.
    let mut deep = String::from(head);
    for i in 0..400 {
        if i > 0 {
            deep.push(',');
        }
        deep += &format!(
            r#"{{"children":[{}]}}"#,
            vec![(i + 1).to_string(); 20].join(",")
        );
    }
    deep += r#",{}]}"#;
    assert!(deep.len() < 200_000);
    assert!(gltf::read(deep.as_bytes(), &g, &none).is_err());
    // A loop through a parent, and a node that names itself.
    for nodes in [
        r#"{"children":[1]},{"children":[0]}"#,
        r#"{"children":[0]}"#,
        r#"{"children":[1]},{"children":[2]},{"children":[0]}"#,
    ] {
        let t = format!(
            r#"{{"asset":{{"version":"2.0"}},"scenes":[{{"nodes":[0]}}],"nodes":[{nodes}]}}"#
        );
        let e = gltf::read(t.as_bytes(), &g, &none).unwrap_err();
        assert!(e.to_string().contains("loop"), "{e}");
    }
    assert!(start.elapsed().as_secs() < 20);
}

#[test]
fn buffer_files_are_read_once_and_together_stay_within_the_budget() {
    let g = gltf::GltfOptions {
        limits: Limits {
            max_bytes: 5000,
            ..limits()
        },
    };
    let buffers = |names: &[String]| -> String {
        let list: Vec<String> = names
            .iter()
            .map(|n| format!(r#"{{"byteLength":39,"uri":"{n}"}}"#))
            .collect();
        format!(
            r#"{{"asset":{{"version":"2.0"}},"nodes":[{{"mesh":0}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5121,"count":3,"type":"SCALAR"}}],
"bufferViews":[{{"buffer":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":3}}],"buffers":[{}]}}"#,
            list.join(",")
        )
    };
    // Twelve buffers name one file of 4 KB: it is read once and counts once.
    let calls = std::cell::Cell::new(0);
    let mut data = tri_data();
    data.resize(4000, 0);
    let one = |_: &str| {
        calls.set(calls.get() + 1);
        Some(data.clone())
    };
    let t = buffers(&vec!["a.bin".to_string(); 12]);
    let m = gltf::read(t.as_bytes(), &g, &one).unwrap();
    assert_eq!(m.totals.triangles, 1);
    assert_eq!(calls.get(), 1);
    // Twelve different files of 4 KB are 48 KB: over the budget of 5 KB.
    let names: Vec<String> = (0..12).map(|i| format!("f{i}.bin")).collect();
    let calls2 = std::cell::Cell::new(0);
    let many = |_: &str| {
        calls2.set(calls2.get() + 1);
        Some(data.clone())
    };
    let t = buffers(&names);
    assert!(matches!(
        gltf::read(t.as_bytes(), &g, &many),
        Err(MeshError::FileTooLarge { limit: 5000, .. })
    ));
    assert_eq!(
        calls2.get(),
        2,
        "it stops at the file that passes the budget"
    );
    // The memory limit counts the files too.
    let tight = gltf::GltfOptions {
        limits: Limits {
            max_memory: 100,
            ..limits()
        },
    };
    assert!(matches!(
        gltf::read(
            buffers(&["a.bin".to_string()]).as_bytes(),
            &tight,
            &|_: &str| Some(data.clone())
        ),
        Err(MeshError::TooMuchMemory { .. })
    ));
}

#[test]
fn buffer_names_with_letters_outside_ascii() {
    let g = gltf::GltfOptions::default();
    let text = |uri: &str| {
        format!(
            r#"{{"asset":{{"version":"2.0"}},"nodes":[{{"mesh":0}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5121,"count":3,"type":"SCALAR"}}],
"bufferViews":[{{"buffer":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":3}}],"buffers":[{{"byteLength":39,"uri":"{uri}"}}]}}"#
        )
    };
    let load = |n: &str| (n == "café.bin").then(tri_data);
    for uri in ["caf%C3%A9.bin", "café.bin", "sub/caf%C3%A9.bin"] {
        let r = gltf::read(text(uri).as_bytes(), &g, &|n: &str| {
            load(n.trim_start_matches("sub/"))
        });
        assert!(r.is_ok(), "{uri}: {r:?}");
    }
    // Bytes that are not UTF-8 are refused.
    assert!(gltf::read(text("%FF.bin").as_bytes(), &g, &none).is_err());
}

#[test]
fn glb_origin_and_leftover_indices_are_read() {
    let g = gltf::GltfOptions::default();
    let t = with_data(
        &simple(
            r#""scenes":[{"nodes":[0],"extensions":{"KADAKAS_georeference":{"local_origin":[1000,2000,300]}}}],"#,
        ),
        &tri_data(),
    );
    let mut set = gltf::read(t.as_bytes(), &g, &none).unwrap();
    assert_eq!(set.origin_mm, Some([1000.0, 2000.0, 300.0]));
    set.move_by([1000.0, 2000.0, 300.0]);
    assert_eq!(set.parts[0].positions[0], [1000.0, 2000.0, 300.0]);
    // Four indices: one triangle and one index left over.
    let four = with_data(
        &simple("")
            .replace(
                r#""count":3,"type":"SCALAR""#,
                r#""count":4,"type":"SCALAR""#,
            )
            .replace(r#""byteLength":3}"#, r#""byteLength":4}"#)
            .replace(r#""byteLength":39,"#, r#""byteLength":40,"#),
        &[tri_data(), vec![0]].concat(),
    );
    let m = gltf::read(four.as_bytes(), &g, &none).unwrap();
    assert_eq!(m.totals.triangles, 1);
    assert!(
        m.notes.iter().any(|n| n.contains("whole triangle")),
        "{:?}",
        m.notes
    );
    // Instancing is noted.
    let inst = with_data(
        &simple("").replace(
            r#"{"name":"n","mesh":0}"#,
            r#"{"name":"n","mesh":0,"extensions":{"EXT_mesh_gpu_instancing":{}}}"#,
        ),
        &tri_data(),
    );
    let m = gltf::read(inst.as_bytes(), &g, &none).unwrap();
    assert!(
        m.notes
            .iter()
            .any(|n| n.contains("EXT_mesh_gpu_instancing"))
    );
}

#[test]
fn obj_notes_budgets_origin_and_big_material_files() {
    let o = obj::ObjOptions {
        frame: Frame {
            unit: Unit::Millimetre,
            up: Up::Z,
        },
        limits: limits(),
    };
    // The origin that Kadakas writes.
    let text = "# local origin in model mm: 100 200 -5\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let mut m = obj::read(text.as_bytes(), &o, &none).unwrap();
    assert_eq!(m.origin_mm, Some([100.0, 200.0, -5.0]));
    m.move_by([100.0, 200.0, -5.0]);
    assert_eq!(m.parts[0].positions[0], [100.0, 200.0, -5.0]);
    // A comment that is not numbers is a comment.
    let m = obj::read(
        b"# local origin in model mm: a b c\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
        &o,
        &none,
    )
    .unwrap();
    assert_eq!(m.origin_mm, None);
    // A big star is not convex: it is cut as a fan, and the note says so.
    let mut star = String::new();
    for i in 0..600 {
        let a = std::f64::consts::TAU * i as f64 / 600.0;
        let r = if i % 2 == 0 { 100.0 } else { 60.0 };
        star += &format!("v {} {} 0\n", r * a.cos(), r * a.sin());
    }
    star += "f";
    for i in 1..=600 {
        star += &format!(" {i}");
    }
    star += "\n";
    let m = obj::read(star.as_bytes(), &o, &none).unwrap();
    assert_eq!(m.totals.triangles, 598);
    assert!(m.notes.iter().any(|n| n.contains("fans")), "{:?}", m.notes);
    // The triangle budget is checked before the work.
    let few = obj::ObjOptions {
        limits: Limits {
            max_triangles: 100,
            ..limits()
        },
        ..o.clone()
    };
    assert!(matches!(
        obj::read(star.as_bytes(), &few, &none),
        Err(MeshError::TooManyTriangles {
            count: 598,
            limit: 100
        })
    ));
    // A material file over 16 MB is noted and not read.
    let big = |_: &str| Some(vec![b'#'; 17 * 1024 * 1024]);
    let m = obj::read(
        b"mtllib big.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
        &o,
        &big,
    )
    .unwrap();
    assert!(m.notes.iter().any(|n| n.contains("larger than 16 MB")));
}

#[test]
fn stl_covers_late_facets_precision_and_the_error_text() {
    let o = stl::StlOptions::default();
    let tri = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    // Facets after endsolid are kept in a part of their own, and the note says so.
    let text = format!(
        "solid a\n{}endsolid a\n{}",
        write_ascii_facet(&tri),
        write_ascii_facet(&tri)
    );
    let m = stl::read(text.as_bytes(), &o).unwrap();
    assert_eq!(m.parts.len(), 2);
    assert_eq!(m.totals.triangles, 2);
    assert!(m.notes.iter().any(|n| n.contains("after an endsolid")));
    // Digits beyond a float are kept.
    let fine = "solid p\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1.0000000001 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid p\n";
    let m = stl::read(fine.as_bytes(), &o).unwrap();
    assert_eq!(m.parts[0].positions[1][0], 1.0000000001);
    // The text of a short file does not have a stray comma and colon.
    let good = stl::write_binary("x", vec![tri].into_iter()).unwrap();
    let e = stl::read(&good[..good.len() - 7], &o).unwrap_err();
    let text = e.to_string();
    assert!(
        text.contains("ends too soon") && !text.contains(", and"),
        "{text}"
    );
}
