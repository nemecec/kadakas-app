//! Extrudes and sweeps (3D-03) in the IFC export.

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve;
use kadakas_core::model::*;
use kadakas_core::solid3d::{Extrude, Sweep};

use crate::step::{self, StepFile};
use crate::{IfcOptions, export_bytes};

fn parsed(doc: &Document) -> StepFile {
    let (bytes, _) = export_bytes(doc, &IfcOptions::default()).unwrap();
    step::parse(&String::from_utf8(bytes).unwrap()).unwrap()
}

fn named<'a>(f: &'a StepFile, name: &str) -> Vec<&'a step::Entity> {
    f.entities.iter().filter(|e| e.name == name).collect()
}

fn rect(x: f64, y: f64, w: f64, d: f64) -> Vec<Vertex> {
    curve::corners(&[
        DVec2::new(x, y),
        DVec2::new(x + w, y),
        DVec2::new(x + w, y + d),
        DVec2::new(x, y + d),
    ])
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

#[test]
fn an_extrude_is_an_extruded_solid_with_voids_and_a_sweep_is_a_face_set() {
    let mut ed = Editor::with_sequential_ids();
    let mut e = Extrude::new(rect(0.0, 0.0, 4000.0, 3000.0), 600.0);
    e.holes = vec![rect(1000.0, 1000.0, 500.0, 500.0)];
    e.base = 200.0;
    e.name = "Planter".into();
    add(&mut ed, Shape::Extrude(Box::new(e)));
    let mut s = Sweep::new(rect(0.0, 0.0, 400.0, 900.0));
    s.origin = DVec3::new(8000.0, 0.0, 0.0);
    s.name = "Column".into();
    add(&mut ed, Shape::Sweep(Box::new(s)));
    // An open sweep is a surface and has no volume in the quantities.
    let mut open = Sweep::new(curve::corners(&[
        DVec2::new(100.0, 0.0),
        DVec2::new(100.0, 500.0),
    ]));
    open.closed = false;
    add(&mut ed, Shape::Sweep(Box::new(open)));
    let f = parsed(ed.doc());
    assert!(f.dangling().is_empty());
    assert_eq!(named(&f, "IFCBUILDINGELEMENTPROXY").len(), 3);
    assert_eq!(named(&f, "IFCEXTRUDEDAREASOLID").len(), 1);
    assert_eq!(named(&f, "IFCARBITRARYPROFILEDEFWITHVOIDS").len(), 1);
    assert_eq!(named(&f, "IFCTRIANGULATEDFACESET").len(), 2);
    let names: Vec<String> = named(&f, "IFCBUILDINGELEMENTPROXY")
        .iter()
        .map(|e| format!("{:?}", e.attrs[2]))
        .collect();
    assert!(
        names.iter().any(|n| n.contains("Planter")) && names.iter().any(|n| n.contains("Column"))
    );
    // The extrusion starts at the base and has the height as depth.
    let solid = named(&f, "IFCEXTRUDEDAREASOLID")[0];
    assert!(format!("{:?}", solid.attrs[3]).contains("600"));
    // Two sets of quantities have a volume, the open sweep has none.
    let volumes = named(&f, "IFCQUANTITYVOLUME").len();
    assert_eq!(volumes, 2);
}

#[test]
fn a_solid_with_a_problem_is_skipped_with_a_reason() {
    let mut ed = Editor::with_sequential_ids();
    let id = add(
        &mut ed,
        Shape::Extrude(Box::new(Extrude::new(rect(0.0, 0.0, 10.0, 10.0), 5.0))),
    );
    let mut doc = ed.doc().clone();
    doc.object_mut(id).unwrap().shape =
        Shape::Extrude(Box::new(Extrude::new(rect(0.0, 0.0, 10.0, 10.0), 0.0)));
    let (bytes, report) = export_bytes(&doc, &IfcOptions::default()).unwrap();
    let f = step::parse(&String::from_utf8(bytes).unwrap()).unwrap();
    assert!(named(&f, "IFCBUILDINGELEMENTPROXY").is_empty());
    assert!(format!("{report:?}").contains("zero"), "{report:?}");
}

#[test]
fn ifc2x3_writes_a_sweep_as_a_surface_model() {
    let mut ed = Editor::with_sequential_ids();
    add(
        &mut ed,
        Shape::Sweep(Box::new(Sweep::new(rect(100.0, 0.0, 400.0, 900.0)))),
    );
    add(
        &mut ed,
        Shape::Extrude(Box::new(Extrude::new(rect(0.0, 0.0, 1000.0, 500.0), 300.0))),
    );
    let opts = IfcOptions {
        version: "IFC2X3".into(),
        ..IfcOptions::default()
    };
    let (bytes, _) = export_bytes(ed.doc(), &opts).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let f = step::parse(&text).unwrap();
    assert!(text.contains("'SurfaceModel'") && !text.contains("'Tessellation'"));
    assert_eq!(named(&f, "IFCEXTRUDEDAREASOLID").len(), 1);
    assert!(f.dangling().is_empty());
}
