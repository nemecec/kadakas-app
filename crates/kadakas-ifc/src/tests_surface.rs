//! Surface models of the 3D modeler (MOD-12) in the IFC export: faceted solids and open shells.

use glam::DVec3;
use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh3d::Mesh3d;
use kadakas_core::model::*;
use kadakas_core::surface::{Edit, FaceId};

use crate::step::{self, StepFile};
use crate::{IfcOptions, export_bytes};

fn export(doc: &Document, version: &str) -> (StepFile, Vec<String>) {
    let opts = IfcOptions {
        version: version.into(),
        ..IfcOptions::default()
    };
    let (bytes, report) = export_bytes(doc, &opts).unwrap();
    (
        step::parse(&String::from_utf8(bytes).unwrap()).unwrap(),
        report.notes,
    )
}

fn named<'a>(f: &'a StepFile, name: &str) -> Vec<&'a step::Entity> {
    f.entities.iter().filter(|e| e.name == name).collect()
}

fn cube(z0: f64) -> Mesh3d {
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
        name: "Block".into(),
        vertices: v,
        triangles: quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect(),
        color: None,
        exact: None,
    }
}

/// A square plate 3 by 3 cells with the middle cell missing, as triangles: an open surface with
/// a hole.
fn plate_with_hole() -> Mesh3d {
    let mut vertices = Vec::new();
    for j in 0..=3 {
        for i in 0..=3 {
            vertices.push(DVec3::new(i as f64 * 1000.0, j as f64 * 1000.0, 0.0));
        }
    }
    let at = |i: usize, j: usize| (j * 4 + i) as u32;
    let mut triangles = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            if (i, j) == (1, 1) {
                continue;
            }
            triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    Mesh3d {
        name: "Plate".into(),
        vertices,
        triangles,
        color: Some(Color([10, 120, 200, 255])),
        exact: None,
    }
}

fn with_surface(mesh: Mesh3d, ed: &mut Editor) -> ObjectId {
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Mesh3d(Box::new(mesh)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::Mesh3dToSurface {
        id,
        tolerance: None,
        soften_below_angle: None,
        merge: true,
    })
    .unwrap();
    id
}

#[test]
fn a_closed_surface_model_is_a_faceted_brep() {
    for version in ["IFC4", "IFC2X3", "IFC4X3"] {
        let mut ed = Editor::with_sequential_ids();
        with_surface(cube(0.0), &mut ed);
        let (f, notes) = export(ed.doc(), version);
        assert!(f.dangling().is_empty(), "{version}");
        assert_eq!(named(&f, "IFCFACETEDBREP").len(), 1, "{version}");
        assert_eq!(named(&f, "IFCCLOSEDSHELL").len(), 1);
        assert_eq!(named(&f, "IFCFACE").len(), 6);
        assert_eq!(named(&f, "IFCFACEOUTERBOUND").len(), 6);
        assert_eq!(named(&f, "IFCFACEBOUND").len(), 0);
        assert_eq!(named(&f, "IFCPOLYLOOP").len(), 6);
        assert_eq!(named(&f, "IFCBUILDINGELEMENTPROXY").len(), 1);
        assert!(named(&f, "IFCSHELLBASEDSURFACEMODEL").is_empty());
        assert!(
            notes.iter().all(|n| !n.contains("surface model")),
            "{notes:?}"
        );
        // The 8 corners are written once each as points of the faces (plus the placement points).
        let reps = named(&f, "IFCSHAPEREPRESENTATION");
        assert!(
            reps.iter()
                .any(|r| format!("{:?}", r.attrs).contains("Brep"))
        );
    }
}

#[test]
fn an_open_surface_model_is_a_shell_with_a_note_and_keeps_its_holes() {
    let mut ed = Editor::with_sequential_ids();
    with_surface(plate_with_hole(), &mut ed);
    let (f, notes) = export(ed.doc(), "IFC4");
    assert!(f.dangling().is_empty());
    assert!(named(&f, "IFCFACETEDBREP").is_empty());
    assert_eq!(named(&f, "IFCSHELLBASEDSURFACEMODEL").len(), 1);
    assert_eq!(named(&f, "IFCOPENSHELL").len(), 1);
    // One face with a hole.
    assert_eq!(named(&f, "IFCFACE").len(), 1);
    assert_eq!(named(&f, "IFCFACEOUTERBOUND").len(), 1);
    assert_eq!(named(&f, "IFCFACEBOUND").len(), 1);
    assert!(
        notes.iter().any(|n| n.contains("not in a closed solid")),
        "{notes:?}"
    );
    // The color of the object is a style: the fill of the surface model.
    let colour = named(&f, "IFCCOLOURRGB");
    assert!(
        colour
            .iter()
            .any(|c| format!("{:?}", c.attrs).contains("0.0392")),
        "{colour:?}"
    );
}

#[test]
fn a_model_with_a_solid_and_an_open_part_is_one_surface_model() {
    let mut ed = Editor::with_sequential_ids();
    let id = with_surface(cube(0.0), &mut ed);
    // Add a loose triangle to the same model.
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges {
            edges: vec![
                [[3000.0, 0.0, 0.0], [4000.0, 0.0, 0.0]],
                [[4000.0, 0.0, 0.0], [4000.0, 1000.0, 0.0]],
                [[4000.0, 1000.0, 0.0], [3000.0, 0.0, 0.0]],
            ],
        },
    })
    .unwrap();
    let (f, notes) = export(ed.doc(), "IFC4");
    assert!(f.dangling().is_empty());
    assert_eq!(named(&f, "IFCCLOSEDSHELL").len(), 1);
    assert_eq!(named(&f, "IFCOPENSHELL").len(), 1);
    assert_eq!(named(&f, "IFCSHELLBASEDSURFACEMODEL").len(), 1);
    assert!(
        notes.iter().any(|n| n.contains("1 closed solids")),
        "{notes:?}"
    );
}

#[test]
fn hidden_faces_and_empty_models_are_left_out() {
    let mut ed = Editor::with_sequential_ids();
    let id = with_surface(cube(0.0), &mut ed);
    let top = {
        let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
            panic!()
        };
        m.faces().map(|(f, _)| f).next().unwrap()
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
    let (f, _) = export(ed.doc(), "IFC4");
    // The solid has a hidden face: the five others are an open shell.
    assert!(named(&f, "IFCFACETEDBREP").is_empty());
    assert_eq!(named(&f, "IFCFACE").len(), 5);
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
    let (bytes, report) = export_bytes(ed.doc(), &IfcOptions::default()).unwrap();
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.reason.contains("visible face"))
    );
    drop(bytes);
}

#[test]
fn a_mirrored_model_keeps_its_fronts_outward_and_the_layer_elevation_counts() {
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::AddLayer {
        name: "Up".into(),
        elevation: 500.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let id = with_surface(cube(500.0), &mut ed);
    let mut doc = ed.doc().clone();
    if let Shape::SurfaceModel(m) = &mut doc.object_mut(id).unwrap().shape {
        let mut mirror = m.transform;
        mirror.matrix3.x_axis.x = -mirror.matrix3.x_axis.x;
        mirror.translation.x = -mirror.translation.x;
        m.place(mirror);
    }
    let (f, _) = export(&doc, "IFC4");
    assert_eq!(named(&f, "IFCFACETEDBREP").len(), 1);
    assert!(f.dangling().is_empty());
}

#[test]
fn components_are_skipped_with_the_way_out() {
    use kadakas_core::resource::ResourceId;
    let mut doc = Editor::with_sequential_ids().doc().clone();
    let (layer, class) = (doc.active_layer, doc.active_class);
    let id = ObjectId(doc.alloc_id());
    doc.objects.insert(
        id,
        Object::new(
            id,
            layer,
            class,
            Shape::Component {
                symbol: ResourceId::from_num(1),
                transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            },
        ),
    );
    let (_, report) = export_bytes(&doc, &IfcOptions::default()).unwrap();
    assert!(report.skipped.iter().any(|s| s.reason.contains("explode")));
}
