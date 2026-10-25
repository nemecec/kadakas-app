//! DXF releases (IOP-15): the export option `version`, and the release and unit of an import.

mod support;

use kadakas_core::curve::{Vertex, corners};
use kadakas_core::model::Shape;
use kadakas_core::{DVec2, DVec3};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::{DxfVersion, ExportOptions, Exporter, ImportOptions, Unit};
use support::{dxf, editor_with, import, import_with};

fn plain() -> Vec<Shape> {
    vec![
        Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(1000.0, 0.0),
        },
        Shape::Circle {
            center: DVec2::new(500.0, 500.0),
            radius: 250.0,
        },
        Shape::Arc {
            center: DVec2::new(0.0, 0.0),
            radius: 100.0,
            start: 0.0,
            end: 1.5,
        },
        Shape::Polyline {
            points: vec![
                Vertex::corner(DVec2::new(0.0, 0.0)),
                Vertex::corner(DVec2::new(100.0, 0.0)),
                Vertex::corner(DVec2::new(100.0, 100.0)),
            ],
            closed: true,
        },
        Shape::Text {
            at: DVec2::new(10.0, 10.0),
            text: "K\u{e4}ed".into(),
            height: 50.0,
        },
        Shape::Locus3d {
            p: DVec3::new(1.0, 2.0, 3.0),
        },
        Shape::Polyline3d {
            points: vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(1.0, 1.0, 1.0)],
            closed: false,
        },
    ]
}

fn export(shapes: Vec<Shape>, version: DxfVersion) -> (Vec<u8>, kadakas_io::Report) {
    let ed = editor_with(shapes);
    let r = DxfFormat
        .export(
            ed.doc(),
            &ExportOptions {
                version,
                ..ExportOptions::default()
            },
        )
        .unwrap();
    (r.bytes, r.report)
}

fn acadver(bytes: &[u8]) -> String {
    let t = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = t.lines().collect();
    let at = lines.iter().position(|l| l.trim() == "$ACADVER").unwrap();
    lines[at + 2].trim().to_string()
}

#[test]
fn every_release_is_written_and_read_again() {
    for v in [
        DxfVersion::R12,
        DxfVersion::R2000,
        DxfVersion::R2004,
        DxfVersion::R2013,
        DxfVersion::R2018,
    ] {
        let (bytes, report) = export(plain(), v);
        assert_eq!(acadver(&bytes), v.code());
        assert!(report.release.as_deref().unwrap().contains(v.code()));
        let (_, back) = import(&bytes).unwrap_or_else(|e| panic!("{v:?}: {e}"));
        assert_eq!(
            back.release.as_deref().unwrap().split(' ').next(),
            Some(v.code())
        );
        assert_eq!(back.total(), plain().len(), "{v:?} {:?}", back.objects);
        assert!(back.skipped.is_empty(), "{v:?}: {:?}", back.skipped);
    }
}

#[test]
fn release_2000_has_no_true_color_and_no_mesh_entity() {
    let (b2000, _) = export(plain(), DxfVersion::R2000);
    let (b2004, _) = export(plain(), DxfVersion::R2004);
    let has_420 = |b: &[u8]| {
        support::pairs(&String::from_utf8_lossy(b))
            .iter()
            .any(|(c, _)| *c == 420)
    };
    assert!(!has_420(&b2000));
    assert!(has_420(&b2004));
    let t = String::from_utf8_lossy(&b2000);
    assert!(
        t.contains("\n62\n1\n"),
        "the nearest indexed color is written"
    );
}

#[test]
fn mesh_entity_falls_back_before_release_2010() {
    let mesh = Shape::Mesh3d(Box::new(kadakas_core::mesh3d::Mesh3d {
        name: String::new(),
        vertices: vec![DVec3::ZERO, DVec3::X * 100.0, DVec3::Y * 100.0],
        triangles: vec![[0, 1, 2]],
        color: None,
        exact: None,
        exact_unusable: None,
    }));
    for (v, has_mesh) in [
        (DxfVersion::R2000, false),
        (DxfVersion::R2004, true),
        (DxfVersion::R2013, true),
    ] {
        let ed = editor_with(vec![mesh.clone()]);
        let r = DxfFormat
            .export(
                ed.doc(),
                &ExportOptions {
                    version: v,
                    mesh_entity: kadakas_io::MeshEntity::Mesh,
                    ..ExportOptions::default()
                },
            )
            .unwrap();
        let t = String::from_utf8(r.bytes).unwrap();
        assert_eq!(t.contains("\nMESH\n"), has_mesh, "{v:?}");
        let said = r.report.notes.iter().any(|n| n.contains("no MESH entity"));
        assert_eq!(said, !has_mesh, "{v:?} {:?}", r.report.notes);
    }
}

#[test]
fn release_12_writes_plain_entities_and_counts_what_it_drops() {
    let mut shapes = plain();
    shapes.push(Shape::Mesh3d(Box::new(kadakas_core::mesh3d::Mesh3d {
        name: String::new(),
        vertices: vec![DVec3::ZERO, DVec3::X, DVec3::Y],
        triangles: vec![[0, 1, 2]],
        color: None,
        exact: None,
        exact_unusable: None,
    })));
    let (bytes, report) = export(shapes, DxfVersion::R12);
    let t = String::from_utf8_lossy(&bytes).into_owned();
    for bad in [
        "LWPOLYLINE",
        "MTEXT",
        "HATCH",
        "AcDb",
        "\n5\n",
        "\n330\n",
        "\n420\n",
        "\n370\n",
        "OBJECTS",
        "$INSUNITS",
    ] {
        assert!(!t.contains(bad), "{bad}");
    }
    for ent in [
        "\nLINE\n",
        "\nCIRCLE\n",
        "\nARC\n",
        "\nPOLYLINE\n",
        "\nVERTEX\n",
        "\nSEQEND\n",
        "\nTEXT\n",
        "\nPOINT\n",
    ] {
        assert!(t.contains(ent), "{ent}");
    }
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].entity, "mesh3d");
    assert_eq!(report.skipped[0].count, 1);
    assert!(
        report.notes.iter().any(|n| n.contains("no unit")),
        "{:?}",
        report.notes
    );
    // The ä is one byte of the code page, not UTF-8.
    assert!(bytes.windows(4).any(|w| w == b"K\xe4ed"));
}

#[test]
fn release_12_keeps_bulges_as_arcs() {
    let shapes = vec![Shape::Polyline {
        points: vec![
            Vertex::corner(DVec2::new(0.0, 0.0)),
            Vertex::new(
                DVec2::new(50.0, 20.0),
                kadakas_core::curve::VertexKind::PointOnArc,
            ),
            Vertex::corner(DVec2::new(100.0, 0.0)),
        ],
        closed: false,
    }];
    let (bytes, _) = export(shapes, DxfVersion::R12);
    let t = String::from_utf8(bytes).unwrap();
    assert!(t.lines().any(|l| l.trim() == "42"), "a bulge");
    let (ed, rep) = import(t.as_bytes()).unwrap();
    assert_eq!(rep.total(), 1);
    let o = ed.doc().objects.values().next().unwrap();
    assert!(
        matches!(&o.shape, Shape::Polyline { points, .. } if points.iter().any(|p| matches!(p.kind, kadakas_core::curve::VertexKind::PointOnArc)))
    );
}

#[test]
fn release_12_names_and_units() {
    let mut ed = editor_with(vec![Shape::Line {
        a: DVec2::ZERO,
        b: DVec2::new(2000.0, 0.0),
    }]);
    ed.execute(&kadakas_core::command::Command::AddClass {
        name: "Walls & Paths".into(),
        attrs: kadakas_core::model::Attributes::default(),
    })
    .unwrap();
    let r = DxfFormat
        .export(
            ed.doc(),
            &ExportOptions {
                version: DxfVersion::R12,
                unit: Unit::M,
                ..ExportOptions::default()
            },
        )
        .unwrap();
    let t = String::from_utf8(r.bytes).unwrap();
    assert!(t.contains("\n11\n2\n"), "2000 mm in meters");
    assert!(r.report.notes.iter().any(|n| n.contains("meters")));
}

#[test]
fn an_import_reads_the_release_and_guesses_the_unit_of_old_files() {
    let r12 = dxf(
        "9\n$ACADVER\n1\nAC1009\n",
        "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1000\n21\n0\n",
    );
    let (ed, rep) = import(r12.as_bytes()).unwrap();
    assert_eq!(rep.release.as_deref(), Some("AC1009 (release 12)"));
    let note = rep
        .notes
        .iter()
        .find(|n| n.contains("no unit"))
        .expect("a note");
    assert!(note.contains("release 12 stores none") && note.contains("assuming millimeters"));
    let o = ed.doc().objects.values().next().unwrap();
    assert!(matches!(o.shape, Shape::Line { b, .. } if (b.x - 1000.0).abs() < 1e-9));
    // With the option the number is in the unit.
    let (ed, rep) = import_with(
        r12.as_bytes(),
        &ImportOptions {
            unit: Some(Unit::M),
            ..ImportOptions::default()
        },
    )
    .unwrap();
    assert!(!rep.notes.iter().any(|n| n.contains("no unit")));
    let o = ed.doc().objects.values().next().unwrap();
    assert!(
        matches!(o.shape, Shape::Line { b, a } if ((b - a).length() - 1_000_000.0).abs() < 1e-6)
    );
    // An English drawing without unit is read as inches.
    let en = dxf(
        "9\n$ACADVER\n1\nAC1009\n9\n$MEASUREMENT\n70\n0\n",
        "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n",
    );
    let (ed, rep) = import(en.as_bytes()).unwrap();
    assert!(rep.notes.iter().any(|n| n.contains("assuming inches")));
    let o = ed.doc().objects.values().next().unwrap();
    assert!(matches!(o.shape, Shape::Line { a, b } if ((b - a).length() - 254.0).abs() < 1e-9));
    // A file that names a unit needs no note.
    let m = dxf(
        "9\n$ACADVER\n1\nAC1018\n9\n$INSUNITS\n70\n6\n",
        "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n",
    );
    let (ed, rep) = import(m.as_bytes()).unwrap();
    assert!(rep.notes.is_empty(), "{:?}", rep.notes);
    let o = ed.doc().objects.values().next().unwrap();
    assert!(matches!(o.shape, Shape::Line { a, b } if ((b - a).length() - 10_000.0).abs() < 1e-9));
}

#[test]
fn other_units_of_insunits_are_read() {
    // 21 is the US survey foot.
    let t = dxf(
        "9\n$ACADVER\n1\nAC1032\n9\n$INSUNITS\n70\n21\n",
        "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n0\n",
    );
    let (ed, _) = import(t.as_bytes()).unwrap();
    let o = ed.doc().objects.values().next().unwrap();
    assert!(
        matches!(o.shape, Shape::Line { a, b } if ((b - a).length() - 304.8006096).abs() < 1e-5)
    );
}

#[test]
fn the_option_unit_that_differs_from_the_file_is_said() {
    let m = dxf(
        "9\n$ACADVER\n1\nAC1018\n9\n$INSUNITS\n70\n6\n",
        "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n",
    );
    let (_, rep) = import_with(
        m.as_bytes(),
        &ImportOptions {
            unit: Some(Unit::Mm),
            ..ImportOptions::default()
        },
    )
    .unwrap();
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("replaces the unit of the file")),
        "{:?}",
        rep.notes
    );
}

fn text_of_first(bytes: &[u8]) -> String {
    let (ed, _) = import(bytes).unwrap();
    match &ed.doc().objects.values().next().unwrap().shape {
        Shape::Text { text, .. } => text.clone(),
        other => panic!("{other:?}"),
    }
}

/// An old file keeps its text in the code page of `$DWGCODEPAGE`. A file of release 2007 or newer
/// has UTF-8.
#[test]
fn text_follows_the_code_page_of_old_files_and_utf8_of_new_ones() {
    let make = |version: &str, page: &str, text: &[u8]| {
        let mut b = format!(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{version}\n9\n$DWGCODEPAGE\n3\n{page}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n8\n0\n10\n0\n20\n0\n40\n10\n1\n"
        )
        .into_bytes();
        b.extend_from_slice(text);
        b.extend_from_slice(b"\n0\nENDSEC\n0\nEOF\n");
        b
    };
    let enc = |e: &'static encoding_rs::Encoding, s: &str| e.encode(s).0.into_owned();
    for (page, e, s) in [
        (
            "ANSI_1250",
            encoding_rs::WINDOWS_1250,
            "\u{141}\u{f3}d\u{17a}",
        ),
        (
            "ANSI_1251",
            encoding_rs::WINDOWS_1251,
            "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}",
        ),
        (
            "ANSI_1253",
            encoding_rs::WINDOWS_1253,
            "\u{391}\u{3b8}\u{3ae}\u{3bd}\u{3b1}",
        ),
        ("ANSI_932", encoding_rs::SHIFT_JIS, "\u{6771}\u{4eac}"),
        (
            "DOS866",
            encoding_rs::IBM866,
            "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}",
        ),
        (
            "ISO8859-2",
            encoding_rs::ISO_8859_2,
            "\u{141}\u{f3}d\u{17a}",
        ),
    ] {
        assert_eq!(
            text_of_first(&make("AC1009", page, &enc(e, s))),
            s,
            "{page}"
        );
        assert_eq!(
            text_of_first(&make("AC1018", page, &enc(e, s))),
            s,
            "{page}"
        );
    }
    // DOS 437 and 852 have their own tables.
    assert_eq!(
        text_of_first(&make("AC1009", "DOS437", &[0x84, 0x94, 0x81])),
        "\u{e4}\u{f6}\u{fc}"
    );
    assert_eq!(
        text_of_first(&make("AC1009", "DOS852", &[0xD0, 0xE7])),
        "\u{111}\u{161}"
    );
    // Release 2007 and newer: UTF-8, whatever the page says.
    assert_eq!(
        text_of_first(&make(
            "AC1021",
            "ANSI_1252",
            "\u{141}\u{f3}d\u{17a} \u{4e2d}".as_bytes()
        )),
        "\u{141}\u{f3}d\u{17a} \u{4e2d}"
    );
    assert_eq!(
        text_of_first(&make("AC1032", "ANSI_1252", "K\u{e4}ed".as_bytes())),
        "K\u{e4}ed"
    );
    // The escape \U+ works in every release.
    assert_eq!(
        text_of_first(&make("AC1009", "ANSI_1252", b"A\\U+00E4B")),
        "A\u{e4}B"
    );
}

/// Files of several releases from the ezdxf examples (MIT): the objects equal the entities that
/// the ezdxf reader finds in the model space.
#[test]
fn public_files_of_old_and_new_releases() {
    use support::corpus;
    // (file, release code, entities in the model space by the ezdxf reader)
    let cases: [(&str, &str, usize); 4] = [
        ("Minimal_DXF_AC1006.dxf", "AC1006", 3),
        ("Minimal_DXF_AC1009.dxf", "", 0),
        ("Minimal_DXF_AC1021.dxf", "AC1021", 0),
        ("mtext_columns_R2018.dxf", "AC1032", 3),
    ];
    for (name, release, n) in cases {
        let Some(data) = corpus::ezdxf(name) else {
            continue;
        };
        let (_, rep) = import(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            rep.total(),
            n,
            "{name}: {:?} {:?}",
            rep.objects,
            rep.skipped
        );
        match rep.release.as_deref() {
            Some(r) => assert!(r.starts_with(release), "{name}: {r}"),
            None => assert!(release.is_empty(), "{name}"),
        }
    }
}

#[test]
fn release_12_handles_classes_text_georeference_and_hidden_layers() {
    use kadakas_core::command::Command;
    use kadakas_core::model::{Attributes, ClassId, Color};
    let mut ed = editor_with(vec![]);
    let class =
        |ed: &mut kadakas_core::command::Editor, name: &str, visible: bool, pen: Option<Color>| {
            let id = ClassId(
                ed.execute(&Command::AddClass {
                    name: name.into(),
                    attrs: Attributes {
                        pen,
                        ..Attributes::default()
                    },
                })
                .unwrap()
                .created[0],
            );
            if !visible {
                ed.execute(&Command::ModifyClass {
                    id,
                    name: None,
                    visible: Some(false),
                    attrs: None,
                })
                .unwrap();
            }
            id
        };
    let walls = class(
        &mut ed,
        "Walls & paths",
        true,
        Some(Color([0, 0, 255, 255])),
    );
    let same = class(&mut ed, "WALLS _ PATHS", true, None);
    let hidden = class(&mut ed, "Old stuff", false, None);
    let add = |ed: &mut kadakas_core::command::Editor, shape: Shape, class: ClassId| {
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: Some(class),
            attrs: Attributes::default(),
        })
        .unwrap();
    };
    add(
        &mut ed,
        Shape::Rect {
            min: DVec2::ZERO,
            max: DVec2::new(100.0, 50.0),
        },
        walls,
    );
    add(
        &mut ed,
        Shape::Arc {
            center: DVec2::ZERO,
            radius: 10.0,
            start: 0.0,
            end: std::f64::consts::TAU,
        },
        same,
    );
    add(
        &mut ed,
        Shape::Plant {
            at: DVec2::new(5.0, 5.0),
            species: "Oak".into(),
            spread: 40.0,
            height: 100.0,
        },
        hidden,
    );
    add(
        &mut ed,
        Shape::Text {
            at: DVec2::new(0.0, 0.0),
            text: format!("one\n\ntwo \u{20ac} \u{4e2d}\n{}", "x".repeat(300)),
            height: 10.0,
        },
        walls,
    );
    add(
        &mut ed,
        Shape::Hardscape {
            boundary: corners(&[DVec2::ZERO, DVec2::new(10.0, 0.0), DVec2::new(0.0, 10.0)]),
            material: "x".into(),
            thickness: None,
            elevation: None,
        },
        walls,
    );
    let mut doc = ed.doc().clone();
    doc.georef = Some(kadakas_core::georef::Georeference {
        crs: "EPSG:3067".into(),
        origin: DVec2::new(385.0, 6672.0),
        rotation: 0.1,
        user_origin: DVec2::ZERO,
    });
    for mode in [
        kadakas_io::LayerMode::Classes,
        kadakas_io::LayerMode::Layers,
    ] {
        let r = DxfFormat
            .export(
                &doc,
                &ExportOptions {
                    version: DxfVersion::R12,
                    layer_mode: mode,
                    ..ExportOptions::default()
                },
            )
            .unwrap();
        let n = r.report.notes.join("\n");
        assert!(
            n.contains("rotation is not applied") && n.contains("no georeference"),
            "{n}"
        );
        assert!(
            n.contains("cut to 250 characters") && n.contains("have no letter"),
            "{n}"
        );
        if mode == kadakas_io::LayerMode::Classes {
            assert!(n.contains("share a name"), "{n}");
        }
        let t = String::from_utf8_lossy(&r.bytes).into_owned();
        // The multiline text is three TEXT entities (the empty line is left out), the plant a circle.
        assert_eq!(t.matches("\nTEXT\n").count(), 3, "{mode:?}");
        assert!(t.contains("\nCIRCLE\n"));
        // The hidden class is a layer with a negative color.
        if mode == kadakas_io::LayerMode::Classes {
            assert!(t.contains("\n-7\n"), "{t}");
        }
        // The file reads again.
        import(&r.bytes).unwrap();
    }
    // Only the visible objects.
    let r = DxfFormat
        .export(
            &doc,
            &ExportOptions {
                version: DxfVersion::R12,
                visible_only: true,
                ..ExportOptions::default()
            },
        )
        .unwrap();
    assert_eq!(r.report.objects.get("plant"), None);
}

#[test]
fn release_12_writes_an_annotation_as_lines_and_text_and_notes_the_patterns() {
    use kadakas_core::annot::{Annot, Axis, DimMode, Dimension};
    let mut ed = editor_with(vec![]);
    ed.execute(&kadakas_core::command::Command::AddObject {
        shape: Shape::Annotation(Box::new(Annot::Dimension(Dimension {
            mode: DimMode::Linear {
                axis: Axis::Horizontal,
            },
            points: vec![DVec2::new(0.0, 0.0), DVec2::new(3000.0, 0.0)],
            offset: 5.0,
            anchors: vec![],
            style: None,
            text: None,
            detached: false,
        }))),
        layer: None,
        class: None,
        attrs: kadakas_core::model::Attributes::default(),
    })
    .unwrap();
    let r = DxfFormat
        .export(
            ed.doc(),
            &ExportOptions {
                version: DxfVersion::R12,
                patterns: kadakas_io::PatternExport::Gaps,
                ..ExportOptions::default()
            },
        )
        .unwrap();
    let t = String::from_utf8_lossy(&r.bytes).into_owned();
    assert!(
        t.contains("\nTEXT\n") && (t.contains("\nLINE\n") || t.contains("\nPOLYLINE\n")),
        "{t}"
    );
    assert!(r.report.skipped.is_empty(), "{:?}", r.report.skipped);
    assert!(
        r.report.notes.iter().any(|n| n.contains("unit patterns")),
        "{:?}",
        r.report.notes
    );
}
