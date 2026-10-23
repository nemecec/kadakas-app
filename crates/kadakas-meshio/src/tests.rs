use super::*;

fn limits() -> Limits {
    Limits::default()
}

#[test]
fn units_and_axes_round_trip() {
    for u in Unit::ALL {
        for up in [Up::Y, Up::Z] {
            let f = Frame { unit: u, up };
            let p = [1.5, -2.25, 3.0];
            let q = f.from_model(f.to_model(p));
            for k in 0..3 {
                assert!((p[k] - q[k]).abs() < 1e-9);
            }
        }
    }
    // Y up: the file (x, y, z) is the model (x, -z, y).
    let f = Format::Gltf.default_frame();
    assert_eq!(f.to_model([1.0, 2.0, 3.0]), [1000.0, -3000.0, 2000.0]);
    assert_eq!(Unit::from_name("Inches"), Some(Unit::Inch));
    assert_eq!(Unit::from_name("furlong"), None);
    assert_eq!(Up::from_name("Z"), Some(Up::Z));
    assert_eq!(Format::from_name(".GLB"), Some(Format::Glb));
    assert_eq!(Format::Stl.name(), "stl");
}

const CUBE_OBJ: &str = "# cube\nmtllib cube.mtl\no Cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nusemtl red\nf 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

fn no_mtl(_: &str) -> Option<Vec<u8>> {
    None
}

#[test]
fn obj_reads_quads_units_and_colors() {
    let mtl = |n: &str| (n == "cube.mtl").then(|| b"newmtl red\nKd 1 0 0\nd 0.5\n".to_vec());
    let opts = obj::ObjOptions {
        frame: Frame {
            unit: Unit::Millimetre,
            up: Up::Z,
        },
        limits: limits(),
    };
    let m = obj::read(CUBE_OBJ.as_bytes(), &opts, &mtl).unwrap();
    assert_eq!(m.parts.len(), 1);
    assert_eq!(m.totals.triangles, 12);
    assert_eq!(m.totals.vertices, 8);
    assert_eq!(m.parts[0].name, "Cube");
    assert_eq!(m.parts[0].color, Some([255, 0, 0, 128]));
    let (lo, hi) = m.bounds();
    assert_eq!((lo, hi), ([0.0; 3], [1.0; 3]));
    // Default frame: meters, Y up.
    let m = obj::read(CUBE_OBJ.as_bytes(), &obj::ObjOptions::default(), &no_mtl).unwrap();
    let (lo, hi) = m.bounds();
    assert_eq!(lo, [0.0, -1000.0, 0.0]);
    assert_eq!(hi, [1000.0, 0.0, 1000.0]);
    assert!(m.notes.iter().any(|n| n.contains("cube.mtl")));
    assert_eq!(m.parts[0].color, None);
}

#[test]
fn obj_negative_indices_slashes_and_concave_faces() {
    let text =
        "v 0 0 0\nv 4 0 0\nv 4 4 0\nv 2 1 0\nv 0 4 0\nf -5/1/1 -4//2 -3 -2 -1\nf 1 1 2\nf 1 2\n";
    let o = obj::ObjOptions {
        frame: Frame {
            unit: Unit::Millimetre,
            up: Up::Z,
        },
        limits: limits(),
    };
    let m = obj::read(text.as_bytes(), &o, &no_mtl).unwrap();
    // A concave pentagon makes three triangles with the area of the polygon.
    assert_eq!(m.totals.triangles, 3);
    assert_eq!(m.totals.degenerate, 1);
    assert!(m.notes.iter().any(|n| n.contains("fewer than three")));
    let p = &m.parts[0];
    let area: f64 = p
        .triangles
        .iter()
        .map(|t| {
            let (a, b, c) = (
                p.positions[t[0] as usize],
                p.positions[t[1] as usize],
                p.positions[t[2] as usize],
            );
            0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
        })
        .sum();
    assert!((area - 10.0).abs() < 1e-9, "{area}");
}

#[test]
fn obj_refuses_bad_input_cleanly() {
    let o = obj::ObjOptions::default();
    for (text, kind) in [
        ("v 0 0 0\nf 1 2 3\n", "mesh_bad_index"),
        ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 0 1 2\n", "mesh_bad_index"),
        ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf -4 1 2\n", "mesh_bad_index"),
        ("v 0 0 NaN\n", "mesh_not_finite"),
        ("v 0 0 inf\n", "mesh_not_finite"),
        ("v 0 0 1e300\n", "mesh_out_of_range"),
        ("v 0 0\n", "mesh_malformed"),
        ("v a b c\n", "mesh_malformed"),
        ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 x 3\n", "mesh_malformed"),
        ("# nothing\n", "mesh_empty"),
        ("v 0 0 0\n", "mesh_empty"),
    ] {
        let e = obj::read(text.as_bytes(), &o, &no_mtl).unwrap_err();
        assert_eq!(e.kind(), kind, "{text}: {e}");
    }
    let small = obj::ObjOptions {
        limits: Limits {
            max_triangles: 1,
            ..limits()
        },
        ..Default::default()
    };
    let e = obj::read(CUBE_OBJ.as_bytes(), &small, &no_mtl).unwrap_err();
    assert!(
        matches!(e, MeshError::TooManyTriangles { limit: 1, .. }),
        "{e}"
    );
    assert!(e.is_limit());
    let tiny = obj::ObjOptions {
        limits: Limits {
            max_bytes: 10,
            ..limits()
        },
        ..Default::default()
    };
    assert!(matches!(
        obj::read(CUBE_OBJ.as_bytes(), &tiny, &no_mtl),
        Err(MeshError::FileTooLarge { limit: 10, .. })
    ));
    let few = obj::ObjOptions {
        limits: Limits {
            max_vertices: 3,
            ..limits()
        },
        ..Default::default()
    };
    assert!(matches!(
        obj::read(CUBE_OBJ.as_bytes(), &few, &no_mtl),
        Err(MeshError::TooManyVertices { .. })
    ));
    let mem = obj::ObjOptions {
        limits: Limits {
            max_memory: 100,
            ..limits()
        },
        ..Default::default()
    };
    assert!(matches!(
        obj::read(CUBE_OBJ.as_bytes(), &mem, &no_mtl),
        Err(MeshError::TooMuchMemory { .. })
    ));
    // Bytes that are not text do not panic.
    let junk: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    let _ = obj::read(&junk, &o, &no_mtl);
}

#[test]
fn obj_parts_by_object_and_material_and_merge_when_many() {
    let text = "v 0 0 0\nv 1 0 0\nv 0 1 0\no A\nusemtl m1\nf 1 2 3\nusemtl m2\nf 1 2 3\no B\nusemtl m1\nf 1 2 3\n";
    let m = obj::read(text.as_bytes(), &obj::ObjOptions::default(), &no_mtl).unwrap();
    assert_eq!(m.parts.len(), 3);
    let few = obj::ObjOptions {
        limits: Limits {
            max_parts: 2,
            ..limits()
        },
        ..Default::default()
    };
    // No colors: all three have the same (no) color and join into one.
    let m = obj::read(text.as_bytes(), &few, &no_mtl).unwrap();
    assert_eq!(m.parts.len(), 1);
    assert!(m.notes.iter().any(|n| n.contains("joined")));
    assert_eq!(m.joined().triangles.len(), 3);
}

#[test]
fn mtl_parses_colors() {
    let c =
        obj::parse_mtl(b"newmtl a\nKd 0 1 0\nTr 0.25\nnewmtl b\nKd x\nnewmtl c\nKd 0.5 0.5 0.5\n");
    assert_eq!(c["a"], [0, 255, 0, 191]);
    assert!(!c.contains_key("b"));
    assert_eq!(c["c"], [128, 128, 128, 255]);
}

fn tri_list() -> Vec<[[f32; 3]; 3]> {
    vec![
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
    ]
}

#[test]
fn stl_binary_and_ascii_round_trip() {
    let t = tri_list();
    let bin = stl::write_binary("a test", t.clone().into_iter()).unwrap();
    assert_eq!(bin.len(), 84 + 100);
    let o = stl::StlOptions::default();
    let m = stl::read(&bin, &o).unwrap();
    assert_eq!(m.totals.triangles, 2);
    assert_eq!(m.totals.vertices, 4, "equal corners are joined");
    let asc = stl::write_ascii("a test", t.into_iter());
    assert!(asc.starts_with("solid a test"));
    let m2 = stl::read(asc.as_bytes(), &o).unwrap();
    assert_eq!(m2.parts[0].triangles, m.parts[0].triangles);
    assert_eq!(m2.parts[0].positions, m.parts[0].positions);
    // A header that starts with solid is still binary when the size agrees.
    let mut tricky = bin.clone();
    tricky[..5].copy_from_slice(b"solid");
    assert_eq!(stl::read(&tricky, &o).unwrap().totals.triangles, 2);
    // The writer never starts the header with "solid".
    let h = stl::write_binary("solid thing", tri_list().into_iter()).unwrap();
    assert!(!h.starts_with(b"solid"));
}

#[test]
fn stl_color_in_header() {
    let mut bin = stl::write_binary("x", tri_list().into_iter()).unwrap();
    bin[..80].fill(b' ');
    bin[2..8].copy_from_slice(b"COLOR=");
    bin[8..12].copy_from_slice(&[10, 20, 30, 0]);
    let m = stl::read(&bin, &stl::StlOptions::default()).unwrap();
    assert_eq!(m.parts[0].color, Some([10, 20, 30, 255]));
}

#[test]
fn stl_refuses_bad_files_cleanly() {
    let o = stl::StlOptions::default();
    let good = stl::write_binary("x", tri_list().into_iter()).unwrap();
    // Truncated.
    for cut in [0, 10, 83, 84, 100, good.len() - 1] {
        let e = stl::read(&good[..cut], &o).unwrap_err();
        assert!(
            matches!(
                e,
                MeshError::Truncated { .. } | MeshError::TooManyTriangles { .. }
            ),
            "{cut}: {e}"
        );
    }
    // A huge count in a small file.
    let mut huge = good.clone();
    huge[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    let e = stl::read(&huge, &o).unwrap_err();
    assert!(matches!(e, MeshError::TooManyTriangles { count, .. } if count == u32::MAX as u64));
    // A count that is within the limit but larger than the file.
    let mut big = good.clone();
    big[80..84].copy_from_slice(&1_000_000u32.to_le_bytes());
    assert!(matches!(
        stl::read(&big, &o),
        Err(MeshError::Truncated { .. })
    ));
    // NaN and infinity.
    let mut nan = good.clone();
    nan[96..100].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(matches!(
        stl::read(&nan, &o),
        Err(MeshError::NotFinite { .. })
    ));
    let mut inf = good.clone();
    inf[96..100].copy_from_slice(&f32::INFINITY.to_le_bytes());
    assert!(matches!(
        stl::read(&inf, &o),
        Err(MeshError::NotFinite { .. })
    ));
    let mut far = good.clone();
    far[96..100].copy_from_slice(&1.0e30f32.to_le_bytes());
    assert!(matches!(
        stl::read(&far, &o),
        Err(MeshError::OutOfRange { .. })
    ));
    // Extra bytes are noted.
    let mut extra = good.clone();
    extra.extend_from_slice(&[0; 7]);
    assert!(stl::read(&extra, &o).unwrap().notes[0].contains("7 bytes"));
    // ASCII problems.
    for text in [
        "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\n",
        "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nfacet\nouter loop\nvertex 0 0 0\n",
        "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nvertex 1 1 1\nendloop\nendfacet\nendsolid\n",
        "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 nan\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n",
        "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 a\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n",
        "solid x\nendsolid x\n",
    ] {
        assert!(stl::read(text.as_bytes(), &o).is_err(), "{text}");
    }
    // Many solids make many parts; a missing endsolid is a note.
    let two = "solid a\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid a\nsolid b\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\n";
    let m = stl::read(two.as_bytes(), &o).unwrap();
    assert_eq!(m.parts.len(), 2);
    assert!(m.notes.iter().any(|n| n.contains("endsolid")));
    // Limits.
    let tiny = stl::StlOptions {
        limits: Limits {
            max_triangles: 1,
            ..limits()
        },
        ..Default::default()
    };
    assert!(matches!(
        stl::read(&good, &tiny),
        Err(MeshError::TooManyTriangles { .. })
    ));
    assert!(matches!(
        stl::read(
            two.as_bytes(),
            &stl::StlOptions {
                limits: Limits {
                    max_triangles: 1,
                    ..limits()
                },
                ..Default::default()
            }
        ),
        Err(MeshError::TooManyTriangles { .. })
    ));
    assert!(matches!(
        stl::read(
            &good,
            &stl::StlOptions {
                limits: Limits {
                    max_vertices: 2,
                    ..limits()
                },
                ..Default::default()
            }
        ),
        Err(MeshError::TooManyVertices { .. })
    ));
    assert!(matches!(
        stl::read(
            two.as_bytes(),
            &stl::StlOptions {
                limits: Limits {
                    max_vertices: 2,
                    ..limits()
                },
                ..Default::default()
            }
        ),
        Err(MeshError::TooManyVertices { .. })
    ));
}

#[test]
fn stl_degenerate_triangles_are_counted() {
    let t = vec![
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    ];
    let bin = stl::write_binary("x", t.into_iter()).unwrap();
    let m = stl::read(&bin, &stl::StlOptions::default()).unwrap();
    assert_eq!((m.totals.triangles, m.totals.degenerate), (1, 1));
}

fn glb_of(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut j = json.as_bytes().to_vec();
    while !j.len().is_multiple_of(4) {
        j.push(b' ');
    }
    let mut b = bin.to_vec();
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
    let total = 12 + 8 + j.len() + if bin.is_empty() { 0 } else { 8 + b.len() };
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
    out.extend_from_slice(&j);
    if !bin.is_empty() {
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E4942u32.to_le_bytes());
        out.extend_from_slice(&b);
    }
    out
}

/// One triangle in the XY plane of glTF (Y up), with indices.
fn tri_bin() -> Vec<u8> {
    let mut b = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2] {
        b.extend_from_slice(&i.to_le_bytes());
    }
    b
}

fn tri_json(extra_node: &str, mat: &str) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[{{"name":"tri","mesh":0{extra_node}}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}}],
"materials":[{mat}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},
{{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":6}}],
"buffers":[{{"byteLength":42}}]}}"#
    )
}

#[test]
fn glb_reads_transform_and_color() {
    let json = tri_json(
        r#","translation":[1,0,0],"scale":[2,2,2]"#,
        r#"{"name":"blue","pbrMetallicRoughness":{"baseColorFactor":[0,0,1,1]}}"#,
    );
    let m = gltf::read(
        &glb_of(&json, &tri_bin()),
        &gltf::GltfOptions::default(),
        &|_| None,
    )
    .unwrap();
    assert_eq!(m.parts.len(), 1);
    assert_eq!(m.parts[0].name, "tri");
    assert_eq!(m.parts[0].color, Some([0, 0, 255, 255]));
    assert_eq!(m.parts[0].material, "blue");
    // Meters, Y up to millimeters, Z up: the corner (1, 0, 0) is at x = 1 m + 2 m.
    let p = &m.parts[0].positions;
    assert_eq!(p[0], [1000.0, 0.0, 0.0]);
    assert_eq!(p[1], [3000.0, 0.0, 0.0]);
    assert_eq!(p[2], [1000.0, 0.0, 2000.0]);
    assert_eq!(m.parts[0].triangles, vec![[0, 1, 2]]);
}

#[test]
fn gltf_with_data_uri_and_external_buffer() {
    use base64::Engine;
    let bin = tri_bin();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = tri_json("", r#"{}"#).replace(
        r#""buffers":[{"byteLength":42}]"#,
        &format!(
            r#""buffers":[{{"byteLength":42,"uri":"data:application/octet-stream;base64,{b64}"}}]"#
        ),
    );
    let m = gltf::read(json.as_bytes(), &gltf::GltfOptions::default(), &|_| None).unwrap();
    assert_eq!(m.totals.triangles, 1);
    assert_eq!(m.parts[0].color, Some([255, 255, 255, 255]));
    let json = tri_json("", r#"{}"#).replace(
        r#""buffers":[{"byteLength":42}]"#,
        r#""buffers":[{"byteLength":42,"uri":"tri%20.bin"}]"#,
    );
    let ext = |n: &str| (n == "tri .bin").then(|| bin.clone());
    let m = gltf::read(json.as_bytes(), &gltf::GltfOptions::default(), &ext).unwrap();
    assert_eq!(m.totals.triangles, 1);
    // A missing file and a name with a folder are refused.
    assert!(gltf::read(json.as_bytes(), &gltf::GltfOptions::default(), &|_| None).is_err());
    for bad in ["../x.bin", "/etc/passwd", "a:b", "sub/../../x"] {
        let j = tri_json("", r#"{}"#).replace(
            r#""buffers":[{"byteLength":42}]"#,
            &format!(r#""buffers":[{{"byteLength":42,"uri":"{bad}"}}]"#),
        );
        let called = std::cell::Cell::new(false);
        let e = gltf::read(j.as_bytes(), &gltf::GltfOptions::default(), &|_| {
            called.set(true);
            Some(Vec::new())
        });
        assert!(e.is_err() && !called.get(), "{bad}");
    }
}

#[test]
fn gltf_hierarchy_mirror_strip_fan_and_instances() {
    // Two nodes use one mesh, one is mirrored (scale -1 in x): its winding is turned back.
    let json = r#"{"asset":{"version":"2.0"},
"nodes":[{"name":"root","children":[1,2]},{"name":"a","mesh":0},{"name":"b","mesh":0,"scale":[-1,1,1]}],
"meshes":[{"primitives":[{"attributes":{"POSITION":0},"mode":5}]}],
"accessors":[{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"}],
"bufferViews":[{"buffer":0,"byteLength":48}],"buffers":[{"byteLength":48}]}"#;
    let mut bin = Vec::new();
    for v in [
        0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0,
    ] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let m = gltf::read(&glb_of(json, &bin), &gltf::GltfOptions::default(), &|_| {
        None
    })
    .unwrap();
    assert_eq!(m.totals.triangles, 4);
    assert_eq!(m.parts.len(), 2);
    // Both copies face the same way (the same sign of the area in the plan).
    let nz = |p: &Part, t: &[u32; 3]| {
        let (a, b, c) = (
            p.positions[t[0] as usize],
            p.positions[t[1] as usize],
            p.positions[t[2] as usize],
        );
        (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2])
    };
    let s0 = nz(&m.parts[0], &m.parts[0].triangles[0]);
    let s1 = nz(&m.parts[1], &m.parts[1].triangles[0]);
    assert!(s0 * s1 > 0.0, "{s0} {s1}");
    // The strip alternates: the second triangle faces the same way as the first.
    let s0b = nz(&m.parts[0], &m.parts[0].triangles[1]);
    assert!(s0 * s0b > 0.0);
    // A fan.
    let fan = json.replace(r#""mode":5"#, r#""mode":6"#);
    let m = gltf::read(&glb_of(&fan, &bin), &gltf::GltfOptions::default(), &|_| {
        None
    })
    .unwrap();
    assert_eq!(m.totals.triangles, 4);
    // Lines are skipped with a note, and then nothing is left.
    let lines = json.replace(r#""mode":5"#, r#""mode":1"#);
    assert!(matches!(
        gltf::read(
            &glb_of(&lines, &bin),
            &gltf::GltfOptions::default(),
            &|_| None
        ),
        Err(MeshError::Empty)
    ));
}

#[test]
fn gltf_refuses_bad_files_cleanly() {
    let o = gltf::GltfOptions::default();
    let none = |_: &str| None;
    let good = glb_of(&tri_json("", "{}"), &tri_bin());
    assert!(gltf::read(&good, &o, &none).is_ok());
    // Truncated at every length.
    for cut in 0..good.len() {
        assert!(gltf::read(&good[..cut], &o, &none).is_err(), "{cut}");
    }
    // Bad index.
    let mut bin = tri_bin();
    bin[36..38].copy_from_slice(&9u16.to_le_bytes());
    assert!(matches!(
        gltf::read(&glb_of(&tri_json("", "{}"), &bin), &o, &none),
        Err(MeshError::BadIndex { index: 9, .. })
    ));
    // NaN position.
    let mut bin = tri_bin();
    bin[4..8].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(matches!(
        gltf::read(&glb_of(&tri_json("", "{}"), &bin), &o, &none),
        Err(MeshError::NotFinite { .. })
    ));
    // A huge count.
    let huge = tri_json("", "{}").replace(
        r#""count":3,"type":"VEC3""#,
        r#""count":4000000000,"type":"VEC3""#,
    );
    assert!(matches!(
        gltf::read(&glb_of(&huge, &tri_bin()), &o, &none),
        Err(MeshError::TooManyVertices { .. })
    ));
    let huge = tri_json("", "{}").replace(
        r#""count":3,"type":"SCALAR""#,
        r#""count":4000000000,"type":"SCALAR""#,
    );
    assert!(matches!(
        gltf::read(&glb_of(&huge, &tri_bin()), &o, &none),
        Err(MeshError::TooManyTriangles { .. })
    ));
    // A count that is small but reads past the view.
    let past =
        tri_json("", "{}").replace(r#""count":3,"type":"VEC3""#, r#""count":5,"type":"VEC3""#);
    assert!(matches!(
        gltf::read(&glb_of(&past, &tri_bin()), &o, &none),
        Err(MeshError::Truncated { .. })
    ));
    // A buffer shorter than it says.
    let short = tri_json("", "{}").replace(r#""byteLength":42}]"#, r#""byteLength":4200}]"#);
    assert!(matches!(
        gltf::read(&glb_of(&short, &tri_bin()), &o, &none),
        Err(MeshError::Truncated { .. })
    ));
    // Wrong version, not JSON, refused extension, sparse, cycle.
    for (text, why) in [
        (tri_json("", "{}").replace("2.0", "1.0"), "version"),
        ("not json".to_string(), "json"),
        (
            tri_json("", "{}").replace(
                r#""scene":0"#,
                r#""extensionsRequired":["KHR_draco_mesh_compression"],"scene":0"#,
            ),
            "draco",
        ),
        (
            tri_json("", "{}").replace(
                r#""count":3,"type":"VEC3""#,
                r#""count":3,"type":"VEC3","sparse":{}"#,
            ),
            "sparse",
        ),
        (
            tri_json("", "{}").replace(r#""mesh":0"#, r#""mesh":7"#),
            "mesh",
        ),
        (tri_json(r#","matrix":[1,2]"#, "{}"), "matrix"),
        (tri_json(r#","rotation":[0,0,0,0]"#, "{}"), "rotation"),
        (
            tri_json("", "{}").replace(r#""componentType":5126"#, r#""componentType":5123"#),
            "positions type",
        ),
        (
            tri_json("", "{}").replace(r#""componentType":5123"#, r#""componentType":9999"#),
            "component type",
        ),
    ] {
        let r = gltf::read(&glb_of(&text, &tri_bin()), &o, &none);
        assert!(r.is_err(), "{why}");
        let r = gltf::read(text.as_bytes(), &o, &none);
        assert!(r.is_err(), "{why} as text");
    }
    let cycle = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"children":[0]}]}"#;
    assert!(gltf::read(cycle.as_bytes(), &o, &none).is_err());
    // A bad GLB header.
    let mut bad = good.clone();
    bad[4] = 1;
    assert!(matches!(
        gltf::read(&bad, &o, &none),
        Err(MeshError::Unsupported(_))
    ));
    let mut bad = good.clone();
    bad[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        gltf::read(&bad, &o, &none),
        Err(MeshError::Truncated { .. })
    ));
    // Limits.
    let tiny = gltf::GltfOptions {
        limits: Limits {
            max_triangles: 0,
            ..limits()
        },
    };
    assert!(gltf::read(&good, &tiny, &none).is_err());
    let small = gltf::GltfOptions {
        limits: Limits {
            max_bytes: 10,
            ..limits()
        },
    };
    assert!(matches!(
        gltf::read(&good, &small, &none),
        Err(MeshError::FileTooLarge { .. })
    ));
}

#[test]
fn triangulation_keeps_the_area_and_winding() {
    // A clockwise concave polygon keeps its winding.
    let pts = [
        [0.0, 0.0, 0.0],
        [0.0, 4.0, 0.0],
        [2.0, 1.0, 0.0],
        [4.0, 4.0, 0.0],
        [4.0, 0.0, 0.0],
    ];
    let (t, ok) = triangulate::polygon(&pts);
    assert!(ok);
    assert_eq!(t.len(), 3);
    let area: f64 = t
        .iter()
        .map(|t| {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
        })
        .sum();
    assert!((area + 10.0).abs() < 1e-9, "{area}");
    // A bow tie falls back to a fan.
    let bow = [
        [0.0, 0.0, 0.0],
        [2.0, 2.0, 0.0],
        [2.0, 0.0, 0.0],
        [0.0, 2.0, 0.0],
    ];
    let (t, _) = triangulate::polygon(&bow);
    assert_eq!(t.len(), 2);
    assert!(triangulate::polygon(&bow[..2]).0.is_empty());
    // A polygon with no area.
    let flat = [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]];
    assert_eq!(triangulate::polygon(&flat).0.len(), 2);
}

mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        // No input makes a reader panic.
        #[test]
        fn readers_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
            let _ = obj::read(&bytes, &obj::ObjOptions::default(), &|_| None);
            let _ = stl::read(&bytes, &stl::StlOptions::default());
            let _ = gltf::read(&bytes, &gltf::GltfOptions::default(), &|_| None);
            let mut glb = b"glTF".to_vec();
            glb.extend_from_slice(&2u32.to_le_bytes());
            glb.extend_from_slice(&(bytes.len() as u32 + 12).to_le_bytes());
            glb.extend_from_slice(&bytes);
            let _ = gltf::read(&glb, &gltf::GltfOptions::default(), &|_| None);
        }

        // Flipping bytes of a good STL or GLB never panics.
        #[test]
        fn damaged_files_never_panic(at in 0usize..200, v in any::<u8>()) {
            let mut s = stl::write_binary("x", tri_list().into_iter()).unwrap();
            let i = at % s.len();
            s[i] = v;
            let _ = stl::read(&s, &stl::StlOptions::default());
            let mut g = glb_of(&tri_json("", "{}"), &tri_bin());
            let i = at % g.len();
            g[i] = v;
            let _ = gltf::read(&g, &gltf::GltfOptions::default(), &|_| None);
        }

        // Random triangles survive an STL round trip (f32 values exactly).
        #[test]
        fn stl_round_trip(tris in proptest::collection::vec(
            proptest::array::uniform3(proptest::array::uniform3(-1000i32..1000)), 1..40)) {
            let list: Vec<[[f32; 3]; 3]> = tris
                .iter()
                .map(|t| t.map(|c| c.map(|v| v as f32 * 0.5)))
                .collect();
            let bin = stl::write_binary("p", list.clone().into_iter()).unwrap();
            let m = stl::read(&bin, &stl::StlOptions::default());
            let kept: Vec<&[[f32; 3]; 3]> = list
                .iter()
                .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
                .collect();
            if kept.is_empty() {
                prop_assert!(m.is_err());
            } else {
                let m = m.unwrap();
                let p = &m.parts[0];
                prop_assert_eq!(p.triangles.len(), kept.len());
                for (t, src) in p.triangles.iter().zip(kept) {
                    for k in 0..3 {
                        let q = p.positions[t[k] as usize];
                        for a in 0..3 {
                            prop_assert_eq!(q[a], src[k][a] as f64);
                        }
                    }
                }
                let asc = stl::write_ascii("p", list.into_iter());
                let m2 = stl::read(asc.as_bytes(), &stl::StlOptions::default()).unwrap();
                prop_assert_eq!(&m2.parts[0].positions, &p.positions);
                prop_assert_eq!(&m2.parts[0].triangles, &p.triangles);
            }
        }
    }
}
