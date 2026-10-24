//! Surface models in the DXF export (MOD-12): one 3D mesh per model, as a polyface mesh, as
//! 3DFACE entities or as a MESH entity, up at the elevation of the layer.

use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh3d::Mesh3d;
use kadakas_core::model::{Attributes, Document, Shape};
use kadakas_core::surface::{Edit, FaceId};
use kadakas_core::{DVec2, DVec3};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::{
    Context, ExportOptions, Exporter, FaceMode, ImportOptions, Importer, MeshEntity, Source, Unit,
};

fn cube_mesh(z0: f64) -> Mesh3d {
    let v: Vec<DVec3> = (0..8)
        .map(|i| {
            DVec3::new(
                (i & 1) as f64 * 1000.0,
                ((i >> 1) & 1) as f64 * 1000.0,
                z0 + ((i >> 2) & 1) as f64 * 1000.0,
            )
        })
        .collect();
    let quads: [[u32; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    Mesh3d {
        name: "Cube".into(),
        vertices: v,
        triangles: quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect(),
        color: None,
        exact: None,
    }
}

/// A surface model of a cube on a layer at 2000 mm. Its heights in the drawing are 2000 to 3000.
fn cube_document() -> (Editor, kadakas_core::uid::Uid) {
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::AddLayer {
        name: "Up".into(),
        elevation: 2000.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let layer = ed.doc().layers.last().unwrap().id;
    let id = ed
        .execute(&Command::AddObject {
            shape: Shape::Mesh3d(Box::new(cube_mesh(2000.0))),
            layer: Some(layer),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0];
    ed.execute(&Command::Mesh3dToSurface {
        id: kadakas_core::model::ObjectId(id),
        tolerance: None,
        soften_below_angle: None,
        merge: true,
    })
    .unwrap();
    (ed, id)
}

fn export(doc: &Document, entity: MeshEntity) -> (String, kadakas_io::Report) {
    let r = DxfFormat
        .export(
            doc,
            &ExportOptions {
                mesh_entity: entity,
                ..ExportOptions::default()
            },
        )
        .unwrap();
    (String::from_utf8(r.bytes).unwrap(), r.report)
}

fn count(text: &str, entity: &str) -> usize {
    text.lines()
        .zip(text.lines().skip(1))
        .filter(|(a, b)| a.trim() == "0" && b.trim() == entity)
        .count()
}

/// The values of a group code in the entities section, as numbers.
fn values(text: &str, code: &str) -> Vec<f64> {
    let start = text.find("ENTITIES").unwrap_or(0);
    let end = text[start..]
        .find("ENDSEC")
        .map_or(text.len(), |e| start + e);
    let lines: Vec<&str> = text[start..end].lines().skip(1).collect();
    lines
        .chunks(2)
        .filter(|c| c.len() == 2 && c[0].trim() == code)
        .filter_map(|c| c[1].trim().parse().ok())
        .collect()
}

#[test]
fn a_surface_model_is_written_as_each_kind_of_mesh_at_the_layer_elevation() {
    let (ed, _) = cube_document();
    let (poly, report) = export(ed.doc(), MeshEntity::Polyface);
    assert_eq!(count(&poly, "POLYLINE"), 1);
    assert!(poly.contains("AcDbPolyFaceMesh"));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let (faces, _) = export(ed.doc(), MeshEntity::Face3d);
    assert_eq!(count(&faces, "3DFACE"), 12);
    let z = values(&faces, "30");
    let (lo, hi) = z
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    assert_eq!((lo, hi), (2000.0, 3000.0), "{z:?}");
    let (mesh, _) = export(ed.doc(), MeshEntity::Mesh);
    assert_eq!(count(&mesh, "MESH"), 1);
    assert!(mesh.contains("AcDbSubDMesh"));
    // 8 vertices, and 12 faces of 3 corners: the list has 12 * 4 numbers.
    assert_eq!(values(&mesh, "92"), vec![8.0]);
    assert_eq!(values(&mesh, "93"), vec![48.0]);
    let z = values(&mesh, "30");
    assert_eq!(z.len(), 8);
    assert!(z.iter().all(|v| *v == 2000.0 || *v == 3000.0));
}

#[test]
fn hidden_faces_are_left_out_and_an_empty_model_is_skipped() {
    let (mut ed, id) = cube_document();
    let id = kadakas_core::model::ObjectId(id);
    let top = {
        let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
            panic!()
        };
        m.faces()
            .max_by(|a, b| a.1.plane.normal.z.total_cmp(&b.1.plane.normal.z))
            .map(|(f, _)| f)
            .unwrap()
    };
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: vec![FaceId(top.0)],
            edges: vec![],
            hidden: true,
        },
    })
    .unwrap();
    let (faces, _) = export(ed.doc(), MeshEntity::Face3d);
    assert_eq!(count(&faces, "3DFACE"), 10);
    // Hide the rest: nothing is left to write.
    let all: Vec<FaceId> = {
        let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
            panic!()
        };
        m.faces().map(|(f, _)| f).collect()
    };
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: all,
            edges: vec![],
            hidden: true,
        },
    })
    .unwrap();
    let (_, report) = export(ed.doc(), MeshEntity::Polyface);
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.entity == "surface_model" && s.reason.contains("visible face"))
    );
}

#[test]
fn a_mirrored_model_keeps_its_faces_outward() {
    let (mut ed, id) = cube_document();
    let id = kadakas_core::model::ObjectId(id);
    let mut doc = ed.doc().clone();
    if let Shape::SurfaceModel(m) = &mut doc.object_mut(id).unwrap().shape {
        let mut mirror = m.transform;
        mirror.matrix3.x_axis.x = -mirror.matrix3.x_axis.x;
        mirror.translation.x = -mirror.translation.x;
        m.place(mirror);
    }
    ed = Editor::new(doc);
    let (text, _) = export(ed.doc(), MeshEntity::Face3d);
    assert_eq!(count(&text, "3DFACE"), 12);
    // The signed volume of the written triangles is positive: they face outward.
    let lines: Vec<&str> = text.lines().collect();
    let mut vol = 0.0;
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() == "0" && lines[i + 1].trim() == "3DFACE" {
            let mut p = [DVec3::ZERO; 3];
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                let code: i32 = lines[j].trim().parse().unwrap_or(-1);
                let v: f64 = lines[j + 1].trim().parse().unwrap_or(0.0);
                for (k, pk) in p.iter_mut().enumerate() {
                    let base = 10 + k as i32;
                    match code {
                        c if c == base => pk.x = v,
                        c if c == base + 10 => pk.y = v,
                        c if c == base + 20 => pk.z = v,
                        _ => {}
                    }
                }
                j += 2;
            }
            vol += p[0].dot(p[1].cross(p[2])) / 6.0;
            i = j;
        } else {
            i += 1;
        }
    }
    assert!((vol - 1.0e9).abs() < 1.0, "{vol}");
}

#[test]
fn the_3d_faces_come_back_as_one_mesh_with_the_same_volume() {
    let (ed, _) = cube_document();
    let (text, _) = export(ed.doc(), MeshEntity::Face3d);
    let src = Source {
        name: "t".into(),
        data: text.into_bytes(),
        extra: Default::default(),
    };
    let r = DxfFormat
        .import(
            &src,
            &Context::of(&Document::with_sequential_ids()),
            &ImportOptions {
                faces: FaceMode::Mesh,
                unit: Some(Unit::Mm),
                ..ImportOptions::default()
            },
        )
        .unwrap();
    let mut back = Editor::new(Document::with_sequential_ids());
    back.execute(&r.command).unwrap();
    let meshes: Vec<&Mesh3d> = back
        .doc()
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::Mesh3d(m) => Some(&**m),
            _ => None,
        })
        .collect();
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0].triangles.len(), 12);
    assert!((meshes[0].volume() - 1.0e9).abs() < 1.0);
    let _ = DVec2::ZERO;
}

/// A U shaped prism: the top has corners on a line and is cut into triangles by diagonals.
fn u_prism() -> (Editor, kadakas_core::model::ObjectId) {
    let pts = [
        [0.0, 0.0],
        [1000.0, 0.0],
        [2000.0, 0.0],
        [3000.0, 0.0],
        [3000.0, 3000.0],
        [2000.0, 3000.0],
        [2000.0, 1000.0],
        [1000.0, 1000.0],
        [1000.0, 3000.0],
        [0.0, 3000.0],
    ];
    let mut ed = Editor::with_sequential_ids();
    let id = kadakas_core::model::ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let edges = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            [[a[0], a[1], 0.0], [b[0], b[1], 0.0]]
        })
        .collect();
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges { edges },
    })
    .unwrap();
    let face = {
        let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
            panic!()
        };
        m.faces().next().unwrap().0
    };
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::PushPull {
            face,
            distance: 500.0,
        },
    })
    .unwrap();
    (ed, id)
}

#[test]
fn the_diagonals_of_a_face_and_the_soft_edges_are_not_drawn() {
    let (mut ed, id) = u_prism();
    let edge_count = {
        let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
            panic!()
        };
        m.edge_count()
    };
    // Each edge of the model is the side of two triangles: so 2 x edges sides are drawn.
    let (faces, _) = export(ed.doc(), MeshEntity::Face3d);
    let triangles = count(&faces, "3DFACE");
    assert_eq!(triangles, 36);
    let hidden: u32 = values(&faces, "70")
        .iter()
        .map(|v| (*v as u32).count_ones())
        .sum();
    assert_eq!(3 * triangles as u32 - hidden, 2 * edge_count as u32);
    // The same in the polyface mesh: the indices of the face records that are negative.
    let (poly, _) = export(ed.doc(), MeshEntity::Polyface);
    let mut negative = 0;
    for code in ["71", "72", "73"] {
        negative += values(&poly, code).iter().filter(|v| **v < 0.0).count();
    }
    assert_eq!(3 * triangles - negative, 2 * edge_count);
    // A soft edge is not drawn either.
    let first_edge = {
        let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
            panic!()
        };
        m.edges().next().unwrap().0
    };
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetEdgeStyle {
            edges: vec![first_edge],
            soft: Some(true),
            smooth: None,
        },
    })
    .unwrap();
    let (soft, _) = export(ed.doc(), MeshEntity::Face3d);
    let hidden_soft: u32 = values(&soft, "70")
        .iter()
        .map(|v| (*v as u32).count_ones())
        .sum();
    assert_eq!(hidden_soft, hidden + 2);
}

#[test]
fn an_instance_of_a_component_is_written_as_a_mesh_through_its_transform() {
    use kadakas_core::model::ObjectId;
    let (mut ed, id) = cube_document();
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Cube".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![ObjectId(id)],
            replace: true,
        })
        .unwrap();
    let inst = ObjectId(out.created[1]);
    ed.execute(&Command::MoveObjects {
        ids: vec![inst],
        delta: DVec2::new(5000.0, 0.0),
    })
    .unwrap();
    let (faces, report) = export(ed.doc(), MeshEntity::Face3d);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(count(&faces, "3DFACE"), 12);
    // The corner of the definition was at x = 0: after the move the cube is at 5000 to 6000,
    // and it stands on the layer at 2000.
    let x = values(&faces, "10");
    let (lo, hi) = x
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    assert_eq!((lo, hi), (5000.0, 6000.0), "{x:?}");
    let z = values(&faces, "30");
    assert!(z.iter().all(|v| *v == 2000.0 || *v == 3000.0), "{z:?}");
    // A part in a class that is switched off is left out, and then nothing is left to write.
    let class = kadakas_core::model::ClassId(
        ed.execute(&Command::AddClass {
            name: "Parts".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let mut doc = ed.doc().clone();
    let Shape::Component { symbol, .. } = doc.object(inst).unwrap().shape.clone() else {
        panic!()
    };
    let res = doc.resources.get_mut(symbol).unwrap();
    let kadakas_core::resource::ResourceBody::Graphic(g) = &mut res.body else {
        panic!()
    };
    let kadakas_core::graphics::GraphicResource::Symbol(def) = g.as_mut() else {
        panic!()
    };
    def.parts[0].class = Some(class);
    doc.classes
        .iter_mut()
        .find(|c| c.id == class)
        .unwrap()
        .visible = false;
    // Everything is written, as an object on a hidden layer is: the part is flagged invisible and
    // nothing is dropped without a word.
    let (text, report) = export(&doc, MeshEntity::Face3d);
    assert_eq!(count(&text, "3DFACE"), 12);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(values(&text, "60").contains(&1.0), "flagged invisible");
    // Only the visible objects: the part is left out, and the instance says why it gave nothing.
    let r = DxfFormat
        .export(
            &doc,
            &ExportOptions {
                mesh_entity: MeshEntity::Face3d,
                visible_only: true,
                ..ExportOptions::default()
            },
        )
        .unwrap();
    let text = String::from_utf8(r.bytes).unwrap();
    assert_eq!(count(&text, "3DFACE"), 0);
    assert!(r.report.skipped.iter().any(|s| s.entity == "component"));
}
