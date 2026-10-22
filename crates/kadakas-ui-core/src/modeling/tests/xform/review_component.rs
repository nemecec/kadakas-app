//! Scale of a single turned component instance follows its own axes.

use super::*;
use glam::DAffine3;

/// A component instance made from the box, turned 30 degrees about the z axis.
fn turned_instance(r: &mut Rig) -> ObjectId {
    let m = make_box(r);
    r.ed.execute(&Command::MakeComponent {
        name: "Box".into(),
        model: None,
        faces: Vec::new(),
        edges: Vec::new(),
        objects: vec![m],
        replace: true,
    })
    .unwrap();
    let inst =
        r.ed.doc()
            .objects
            .values()
            .find(|o| matches!(o.shape, Shape::Component { .. }))
            .map(|o| o.id)
            .unwrap();
    r.ed.execute(&Command::TransformObjects3d {
        ids: vec![inst],
        matrix: DAffine3::from_rotation_z(30.0_f64.to_radians()).to_cols_array(),
        copy: false,
        count: 1,
    })
    .unwrap();
    inst
}

fn instance_transform(r: &Rig, id: ObjectId) -> DAffine3 {
    match &r.ed.doc().object(id).unwrap().shape {
        Shape::Component { transform, .. } => kadakas_core::component::affine(transform),
        _ => panic!("not an instance"),
    }
}

#[test]
fn a_scale_of_a_turned_component_follows_its_own_axes() {
    let mut r = rig(ModelerTool::Scale);
    let inst = turned_instance(&mut r);
    select_object(&mut r, inst);
    let own = DAffine3::from_rotation_z(30.0_f64.to_radians());
    // The grip in the middle of the instance's own +x side (the definition is 1000 x 1000 x 500).
    start_scale(&mut r, own.transform_point3(v(1000.0, 500.0, 250.0)));
    enter(&mut r, "2");
    let t = instance_transform(&r, inst);
    let (x, y, z) = (t.matrix3.x_axis, t.matrix3.y_axis, t.matrix3.z_axis);
    assert!((x.length() - 2.0).abs() < 1e-9 && (y.length() - 1.0).abs() < 1e-9);
    assert!(x.dot(y).abs() < 1e-9 && x.dot(z).abs() < 1e-9 && y.dot(z).abs() < 1e-9);
    // Entity Info: not skewed, scale 2 x 1 x 1, turned 30 degrees.
    let scene = kadakas_core::mesh::build(r.ed.doc());
    let info = crate::modeler::entity_info_with_parts(
        r.ed.doc(),
        &scene,
        &[inst],
        &SubSelection::default(),
    );
    match info {
        crate::modeler::EntityInfo::One {
            placement: Some(p), ..
        } => {
            assert!(!p.skewed, "{p:?}");
            assert!((p.scale[0] - 2.0).abs() < 1e-9 && (p.scale[1] - 1.0).abs() < 1e-9);
            assert!((p.rotation_deg[2] - 30.0).abs() < 1e-6);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_mirror_of_a_turned_component_along_its_own_axis_stays_square() {
    let mut r = rig(ModelerTool::Scale);
    let inst = turned_instance(&mut r);
    select_object(&mut r, inst);
    let own = DAffine3::from_rotation_z(30.0_f64.to_radians());
    start_scale(&mut r, own.transform_point3(v(1000.0, 500.0, 250.0)));
    enter(&mut r, "-1");
    let t = instance_transform(&r, inst);
    let (x, y, z) = (t.matrix3.x_axis, t.matrix3.y_axis, t.matrix3.z_axis);
    assert!(t.matrix3.determinant() < 0.0, "a mirror");
    assert!((x.length() - 1.0).abs() < 1e-9 && (y.length() - 1.0).abs() < 1e-9);
    assert!(x.dot(y).abs() < 1e-9 && x.dot(z).abs() < 1e-9 && y.dot(z).abs() < 1e-9);
    let p = crate::modeler::Placement::of_transform(&t);
    assert!(!p.skewed && p.scale[0] < 0.0, "{p:?}");
}
