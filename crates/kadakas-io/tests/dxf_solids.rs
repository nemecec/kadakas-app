//! Extrudes and sweeps (3D-03) in the DXF export: meshes at the elevation of their layer.

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::model::{Attributes, Document, Shape};
use kadakas_core::solid3d::{Extrude, Sweep};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::{ExportOptions, Exporter, MeshEntity};

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
fn extrudes_and_sweeps_are_meshes_at_the_layer_elevation() {
    let ring = corners(&[
        DVec2::ZERO,
        DVec2::new(1000.0, 0.0),
        DVec2::new(1000.0, 500.0),
        DVec2::new(0.0, 500.0),
    ]);
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::AddLayer {
        name: "Up".into(),
        elevation: 3000.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let layer = ed.doc().layers.last().unwrap().id;
    for shape in [
        Shape::Extrude(Box::new(Extrude::new(ring.clone(), 200.0))),
        Shape::Sweep(Box::new(Sweep::new(ring))),
    ] {
        ed.execute(&Command::AddObject {
            shape,
            layer: Some(layer),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    let (text, _) = export(ed.doc(), &ExportOptions::default());
    assert_eq!(count(&text, "POLYLINE"), 2);
    assert!(text.contains("AcDbPolyFaceMesh"));
    let (faces, _) = export(
        ed.doc(),
        &ExportOptions {
            mesh_entity: MeshEntity::Face3d,
            ..ExportOptions::default()
        },
    );
    // 12 triangles in the box. The sweep has more.
    assert!(count(&faces, "3DFACE") > 12);
    // A solid with a problem (a file written by hand) is left out.
    let mut doc = ed.doc().clone();
    let id = *doc.objects.keys().next().unwrap();
    doc.object_mut(id).unwrap().shape = Shape::Extrude(Box::new(Extrude::new(vec![], 0.0)));
    let (text, _) = export(&doc, &ExportOptions::default());
    assert_eq!(count(&text, "POLYLINE"), 1);
}

#[test]
fn components_and_empty_surface_models_are_skipped_with_a_reason() {
    use kadakas_core::model::{Object, ObjectId};
    use kadakas_core::resource::ResourceId;
    let mut doc = Editor::with_sequential_ids().doc().clone();
    let (layer, class) = (doc.active_layer, doc.active_class);
    for shape in [
        Shape::Component {
            symbol: ResourceId::from_num(1),
            transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        },
        Shape::SurfaceModel(Box::default()),
    ] {
        let id = ObjectId(doc.alloc_id());
        doc.objects.insert(id, Object::new(id, layer, class, shape));
    }
    let (_, report) = export(&doc, &ExportOptions::default());
    let kinds: Vec<&str> = report.skipped.iter().map(|s| s.entity.as_str()).collect();
    assert!(
        kinds.contains(&"component") && kinds.contains(&"surface_model"),
        "{kinds:?}"
    );
    // A component is skipped with the way out. An empty surface model has nothing to write.
    assert!(
        report
            .skipped
            .iter()
            .all(|s| s.reason.contains("explode") || s.reason.contains("without a visible face")),
        "{:?}",
        report.skipped
    );
}
