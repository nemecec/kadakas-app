//! Native DXF entities: dimensions, leaders, MTEXT, 3D meshes and GEODATA, and the import of
//! polyface meshes, polygon meshes and 3D faces.

use kadakas_core::annot::style::{Justify, Terminator, TextStyle};
use kadakas_core::annot::{Annot, Axis, Callout, DimMode, Dimension, Leader, StyledText};
use kadakas_core::command::{Command, Editor};
use kadakas_core::georef::Georeference;
use kadakas_core::mesh3d::Mesh3d;
use kadakas_core::model::{Attributes, Document, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::{DVec2, DVec3};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::{
    Context, ExportOptions, ExportOrigin, Exporter, FaceMode, ImportOptions, Importer, MeshEntity,
    SiteMesh, Source, Unit,
};

fn add(ed: &mut Editor, shape: Shape) -> kadakas_core::uid::Uid {
    ed.execute(&Command::AddObject {
        shape,
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap()
    .created[0]
}

fn dim(mode: DimMode, points: &[(f64, f64)], offset: f64) -> Shape {
    Shape::Annotation(Box::new(Annot::Dimension(Dimension {
        mode,
        points: points.iter().map(|p| DVec2::new(p.0, p.1)).collect(),
        anchors: vec![],
        offset,
        style: None,
        text: None,
        detached: false,
        area: None,
    })))
}

fn callout(tip: (f64, f64), at: (f64, f64), text: &str, terminator: Terminator) -> Shape {
    Shape::Annotation(Box::new(Annot::Callout(Callout {
        at: DVec2::new(at.0, at.1),
        text: text.into(),
        note: None,
        notes_db: None,
        style: None,
        leaders: vec![Leader {
            points: vec![DVec2::new(tip.0, tip.1)],
            anchor: None,
        }],
        shoulder_mm: 5.0,
        terminator,
        detached: false,
    })))
}

fn export(doc: &Document, opts: &ExportOptions) -> (String, kadakas_io::Report) {
    let r = DxfFormat.export(doc, opts).unwrap();
    (String::from_utf8(r.bytes).unwrap(), r.report)
}

fn count(text: &str, entity: &str) -> usize {
    text.lines()
        .zip(text.lines().skip(1))
        .filter(|(a, b)| a.trim() == "0" && b.trim() == entity)
        .count()
}

#[test]
fn dimensions_callouts_and_text_are_native() {
    let mut ed = Editor::with_sequential_ids();
    let h = DimMode::Linear {
        axis: Axis::Horizontal,
    };
    add(&mut ed, dim(h, &[(0.0, 0.0), (5000.0, 0.0)], 8.0));
    add(
        &mut ed,
        dim(
            DimMode::Linear {
                axis: Axis::Vertical,
            },
            &[(0.0, 0.0), (0.0, 3000.0)],
            -8.0,
        ),
    );
    add(
        &mut ed,
        dim(
            DimMode::Linear {
                axis: Axis::Aligned,
            },
            &[(0.0, 0.0), (3000.0, 4000.0)],
            8.0,
        ),
    );
    add(
        &mut ed,
        dim(DimMode::Radial, &[(9000.0, 0.0), (10000.0, 0.0)], 0.0),
    );
    add(
        &mut ed,
        dim(
            DimMode::Diameter,
            &[(9000.0, 5000.0), (10000.0, 5000.0)],
            0.0,
        ),
    );
    // These have no native form here.
    add(
        &mut ed,
        dim(
            DimMode::Chain {
                axis: Axis::Horizontal,
            },
            &[(0.0, 8000.0), (2000.0, 8000.0), (4000.0, 8000.0)],
            8.0,
        ),
    );
    add(
        &mut ed,
        dim(
            DimMode::Angular,
            &[(0.0, 0.0), (1000.0, 0.0), (0.0, 1000.0)],
            0.0,
        ),
    );
    // Callouts on the right and on the left of the tip, with the arrow and with other end marks.
    add(
        &mut ed,
        callout(
            (0.0, 0.0),
            (3000.0, 3000.0),
            "Right\nnote",
            Terminator::Arrow,
        ),
    );
    add(
        &mut ed,
        callout((6000.0, 0.0), (3000.0, 3000.0), "Left", Terminator::Dot),
    );
    add(
        &mut ed,
        callout((6000.0, 0.0), (3000.0, 3000.0), "  ", Terminator::OpenArrow),
    );
    add(
        &mut ed,
        callout((6000.0, 0.0), (3000.0, 3000.0), "No mark", Terminator::None),
    );
    // Styled text in the three justifications.
    for (i, j) in [Justify::Left, Justify::Center, Justify::Right]
        .into_iter()
        .enumerate()
    {
        let style = TextStyle {
            justify: j,
            ..TextStyle::default()
        };
        let sid = ResourceId(
            ed.execute(&Command::AddResource {
                name: format!("S{i}"),
                body: ResourceBody::Annot(Box::new(
                    kadakas_core::annot::style::AnnotResource::TextStyle(style),
                )),
            })
            .unwrap()
            .created[0],
        );
        add(
            &mut ed,
            Shape::Annotation(Box::new(Annot::Text(StyledText {
                at: DVec2::new(0.0, -2000.0 - i as f64 * 1000.0),
                text: "Planting\nnotes".into(),
                style: Some(sid),
            }))),
        );
    }
    let (text, report) = export(ed.doc(), &ExportOptions::default());
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(count(&text, "DIMENSION"), 5);
    for sub in [
        "AcDbRotatedDimension",
        "AcDbRadialDimension",
        "AcDbDiameterDimension",
        "AcDbLeader",
    ] {
        assert!(text.contains(sub), "{sub}");
    }
    // Each dimension has an anonymous block with its drawing.
    for n in 1..=5 {
        assert!(text.contains(&format!("\n*D{n}\n")), "*D{n}");
    }
    assert!(text.contains("DSTYLE"), "the arrow size override");
    assert_eq!(count(&text, "LEADER"), 4);
    assert!(count(&text, "MTEXT") >= 5, "{}", count(&text, "MTEXT"));
    assert!(text.contains("Right\\Pnote"));
    // The chain dimension and the angular dimension are exploded into lines and text.
    assert!(count(&text, "TEXT") >= 2);
    // Native annotations off: nothing native.
    let (plain, _) = export(
        ed.doc(),
        &ExportOptions {
            native_annotations: false,
            ..ExportOptions::default()
        },
    );
    assert_eq!(
        count(&plain, "DIMENSION") + count(&plain, "LEADER") + count(&plain, "MTEXT"),
        0
    );
    // In meters the numbers follow the unit.
    let (m, _) = export(
        ed.doc(),
        &ExportOptions {
            unit: Unit::M,
            ..ExportOptions::default()
        },
    );
    assert!(m.contains("\n5.0\n") || m.contains("\n5\n"));
}

fn mesh(n: usize) -> Mesh3d {
    let mut vertices = Vec::new();
    for i in 0..=n {
        for j in 0..=n {
            vertices.push(DVec3::new(
                i as f64 * 100.0,
                j as f64 * 100.0,
                (i + j) as f64,
            ));
        }
    }
    let mut triangles = Vec::new();
    let w = (n + 1) as u32;
    for i in 0..n as u32 {
        for j in 0..n as u32 {
            let a = i * w + j;
            triangles.push([a, a + w, a + w + 1]);
            triangles.push([a, a + w + 1, a + 1]);
        }
    }
    Mesh3d {
        name: String::new(),
        vertices,
        triangles,
        color: None,
    }
}

#[test]
fn meshes_are_polyface_meshes_or_3d_faces_and_big_ones_are_cut() {
    let mut ed = Editor::with_sequential_ids();
    add(&mut ed, Shape::Mesh3d(Box::new(mesh(2))));
    let (text, _) = export(ed.doc(), &ExportOptions::default());
    assert_eq!(count(&text, "POLYLINE"), 1);
    assert!(text.contains("AcDbPolyFaceMesh") && text.contains("AcDbFaceRecord"));
    let (faces, _) = export(
        ed.doc(),
        &ExportOptions {
            mesh_entity: MeshEntity::Face3d,
            ..ExportOptions::default()
        },
    );
    assert_eq!(count(&faces, "3DFACE"), 8);
    // 100 by 100 cells: 20 000 triangles in one piece. 200 by 200: 80 000 in three pieces.
    let mut big = Editor::with_sequential_ids();
    add(&mut big, Shape::Mesh3d(Box::new(mesh(200))));
    let (text, _) = export(big.doc(), &ExportOptions::default());
    assert_eq!(count(&text, "POLYLINE"), 3);
}

#[test]
fn the_site_surface_can_be_written_as_a_mesh() {
    let mut ed = Editor::with_sequential_ids();
    let mut points = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            points.push(DVec3::new(
                i as f64 * 1000.0,
                j as f64 * 1000.0,
                i as f64 * 100.0,
            ));
        }
    }
    add(
        &mut ed,
        Shape::SiteModel {
            points,
            lines: vec![],
            settings: Default::default(),
        },
    );
    let (none, _) = export(ed.doc(), &ExportOptions::default());
    assert_eq!(
        count(&none, "POLYLINE") + count(&none, "3DFACE"),
        count(&none, "POLYLINE")
    );
    assert!(!none.contains("AcDbPolyFaceMesh"));
    for which in [SiteMesh::Existing, SiteMesh::Proposed] {
        let (text, _) = export(
            ed.doc(),
            &ExportOptions {
                site_mesh: which,
                ..ExportOptions::default()
            },
        );
        assert!(text.contains("AcDbPolyFaceMesh"));
    }
}

fn georef(crs: &str, rotation: f64) -> Editor {
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::SetGeoreference {
        georef: Some(Georeference {
            crs: crs.into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation,
            user_origin: DVec2::ZERO,
        }),
    })
    .unwrap();
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::new(1000.0, 0.0),
        },
    );
    ed
}

#[test]
fn the_georeference_is_a_geodata_object() {
    let ed = georef("EPSG:3067", 0.0);
    let (text, report) = export(
        ed.doc(),
        &ExportOptions {
            unit: Unit::M,
            ..ExportOptions::default()
        },
    );
    assert_eq!(count(&text, "GEODATA"), 1, "one object (and its class)");
    assert!(text.contains("AcDbGeoData") && text.contains("EPSG:3067"));
    assert!(text.contains("\n385000\n") && text.contains("\n6672000\n"));
    assert!(report.notes.iter().all(|n| !n.contains("EPSG")));
    // Document coordinates: the design point is the origin, and a rotation turns the north direction.
    let ed = georef("EPSG:3067", 0.5);
    let (text, _) = export(
        ed.doc(),
        &ExportOptions {
            origin: ExportOrigin::Document,
            unit: Unit::M,
            ..ExportOptions::default()
        },
    );
    assert!(text.contains(&format!("{:?}\n", 0.5f64.sin())) || text.contains("0.479"));
    // A CRS that is not an EPSG code gets a note and no GEODATA. No CRS: nothing.
    let ed = georef("PROJCS[\"x\"]", 0.0);
    let (text, report) = export(ed.doc(), &ExportOptions::default());
    assert_eq!(count(&text, "GEODATA"), 0);
    assert!(report.notes.iter().any(|n| n.contains("not an EPSG code")));
    let ed = georef("", 0.0);
    let (text, report) = export(ed.doc(), &ExportOptions::default());
    assert_eq!(count(&text, "GEODATA"), 0);
    assert!(report.notes.iter().all(|n| !n.contains("EPSG")));
    let ed = georef("epsg:25833", 0.0);
    assert_eq!(
        count(&export(ed.doc(), &ExportOptions::default()).0, "GEODATA"),
        1
    );
    let ed = georef("EPSG:", 0.0);
    assert_eq!(
        count(&export(ed.doc(), &ExportOptions::default()).0, "GEODATA"),
        0
    );
}

fn import(entities: &str, opts: &ImportOptions) -> (Document, kadakas_io::Report) {
    let dxf = format!("0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n");
    let src = Source {
        name: "t".into(),
        data: dxf.into_bytes(),
        extra: Default::default(),
    };
    let r = DxfFormat
        .import(&src, &Context::of(&Document::with_sequential_ids()), opts)
        .unwrap();
    let mut ed = Editor::new(Document::with_sequential_ids());
    ed.execute(&r.command).unwrap();
    (ed.doc().clone(), r.report)
}

fn meshes(doc: &Document) -> Vec<Mesh3d> {
    doc.objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::Mesh3d(m) => Some((**m).clone()),
            _ => None,
        })
        .collect()
}

const POLYFACE: &str = "0\nPOLYLINE\n8\nP\n70\n64\n71\n4\n72\n2\n\
0\nVERTEX\n10\n0\n20\n0\n30\n0\n70\n192\n\
0\nVERTEX\n10\n10\n20\n0\n30\n0\n70\n192\n\
0\nVERTEX\n10\n10\n20\n10\n30\n5\n70\n192\n\
0\nVERTEX\n10\n0\n20\n10\n30\n5\n70\n192\n\
0\nVERTEX\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n2\n73\n-3\n74\n4\n\
0\nVERTEX\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n3\n73\n4\n74\n4\n\
0\nVERTEX\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n9\n73\n2\n\
0\nSEQEND\n";

#[test]
fn polyface_meshes_become_meshes() {
    let o = ImportOptions {
        unit: Some(Unit::Mm),
        ..ImportOptions::default()
    };
    let (doc, report) = import(POLYFACE, &o);
    let m = meshes(&doc);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].vertices.len(), 4);
    // A quad (2 triangles) and a triangle with a repeated corner (1); the face with a bad index is dropped.
    assert_eq!(m[0].triangles, vec![[0, 1, 2], [0, 2, 3], [0, 2, 3]]);
    assert_eq!(report.skipped_total(), 0);
    // A polyface with no faces is skipped with a reason.
    let (doc, report) = import(
        "0\nPOLYLINE\n70\n64\n0\nVERTEX\n10\n0\n20\n0\n30\n0\n70\n192\n0\nSEQEND\n",
        &o,
    );
    assert!(meshes(&doc).is_empty());
    assert_eq!(report.skipped_total(), 1);
}

fn grid(flags: u32, m: u32, n: u32) -> String {
    let mut s = format!("0\nPOLYLINE\n70\n{flags}\n71\n{m}\n72\n{n}\n");
    for i in 0..m {
        for j in 0..n {
            s += &format!("0\nVERTEX\n10\n{i}\n20\n{j}\n30\n{}\n70\n64\n", i + j);
        }
    }
    s + "0\nSEQEND\n"
}

#[test]
fn polygon_meshes_become_meshes() {
    let o = ImportOptions {
        unit: Some(Unit::Mm),
        ..ImportOptions::default()
    };
    let (doc, _) = import(&grid(16, 3, 3), &o);
    let m = meshes(&doc);
    assert_eq!((m[0].vertices.len(), m[0].triangles.len()), (9, 8));
    // Closed in both directions: the last row and column join the first.
    let (doc, _) = import(&grid(16 | 1 | 32, 3, 3), &o);
    assert_eq!(meshes(&doc)[0].triangles.len(), 18);
    // A wrong count is skipped.
    let (doc, report) = import(&grid(16, 3, 3).replace("72\n3", "72\n4"), &o);
    assert!(meshes(&doc).is_empty());
    assert_eq!(report.skipped_total(), 1);
}

#[test]
fn faces_are_polygons_or_one_mesh_per_layer() {
    let faces = "0\n3DFACE\n8\nF\n10\n0\n20\n0\n30\n0\n11\n10\n21\n0\n31\n0\n12\n10\n22\n10\n32\n5\n13\n0\n23\n10\n33\n5\n\
0\n3DFACE\n8\nF\n10\n10\n20\n0\n30\n0\n11\n20\n21\n0\n31\n0\n12\n10\n22\n10\n32\n5\n13\n10\n23\n10\n33\n5\n\
0\n3DFACE\n8\nG\n10\n0\n20\n0\n30\n0\n11\n1\n21\n0\n31\n0\n12\n1\n22\n1\n32\n0\n13\n1\n23\n1\n33\n0\n";
    let polygons = ImportOptions {
        unit: Some(Unit::Mm),
        ..ImportOptions::default()
    };
    let (doc, _) = import(faces, &polygons);
    assert!(meshes(&doc).is_empty());
    assert_eq!(doc.objects.len(), 3);
    let merged = ImportOptions {
        faces: FaceMode::Mesh,
        ..polygons
    };
    let (doc, report) = import(faces, &merged);
    let mut m = meshes(&doc);
    m.sort_by_key(|m| m.triangles.len());
    assert_eq!(m.len(), 2);
    assert_eq!(m[0].triangles.len(), 1);
    // Two faces share the edge (10, 0)-(10, 10): five vertices, three triangles.
    assert_eq!((m[1].vertices.len(), m[1].triangles.len()), (5, 3));
    assert_eq!(report.skipped_total(), 0);
}

#[test]
fn an_area_dimension_exports_as_text() {
    let mut ed = Editor::with_sequential_ids();
    let rect = add(
        &mut ed,
        Shape::Rect {
            min: DVec2::new(0.0, 0.0),
            max: DVec2::new(4000.0, 5000.0),
        },
    );
    add(
        &mut ed,
        Shape::Annotation(Box::new(Annot::Dimension(Dimension {
            mode: DimMode::Area,
            points: vec![DVec2::ZERO],
            anchors: vec![Some(kadakas_core::annot::Anchor::new(
                kadakas_core::model::ObjectId(rect),
                kadakas_core::annot::AnchorTarget::Center,
            ))],
            offset: 0.0,
            style: None,
            text: None,
            detached: false,
            area: None,
        }))),
    );
    let (text, report) = export(ed.doc(), &ExportOptions::default());
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    // No native DIMENSION for an area: the label is a text.
    assert_eq!(count(&text, "DIMENSION"), 0);
    assert!(text.contains("20.00 m"), "the area text is in the file");
}
