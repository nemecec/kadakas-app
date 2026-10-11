//! Extrude, sweep and 3D polygon (3D-03) in the core: commands, undo, files, 2D (SVG), 3D (mesh)
//! and snapping.

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::curve::corners;
use kadakas_core::edit2d::Transform;
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::snapping::Geometry;
use kadakas_core::solid3d::{Extrude, Sweep};
use kadakas_core::svg;

fn rect(w: f64, d: f64) -> Vec<Vertex> {
    corners(&[
        DVec2::ZERO,
        DVec2::new(w, 0.0),
        DVec2::new(w, d),
        DVec2::new(0.0, d),
    ])
}

fn add(ed: &mut Editor, shape: Shape) -> Result<ObjectId, CommandError> {
    ed.execute(&Command::AddObject {
        shape,
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .map(|o| ObjectId(o.created[0]))
}

fn box_shape() -> Shape {
    let mut e = Extrude::new(rect(2000.0, 1000.0), 500.0);
    e.holes = vec![corners(&[
        DVec2::new(500.0, 250.0),
        DVec2::new(900.0, 250.0),
        DVec2::new(900.0, 650.0),
        DVec2::new(500.0, 650.0),
    ])];
    Shape::Extrude(Box::new(e))
}

fn vase() -> Shape {
    let mut s = Sweep::new(corners(&[
        DVec2::new(0.0, 0.0),
        DVec2::new(400.0, 0.0),
        DVec2::new(300.0, 800.0),
        DVec2::new(0.0, 800.0),
    ]));
    s.origin = DVec3::new(5000.0, 5000.0, 100.0);
    Shape::Sweep(Box::new(s))
}

#[test]
fn commands_check_the_values_and_undo_works() {
    let mut ed = Editor::default();
    let id = add(&mut ed, box_shape()).unwrap();
    let s = add(&mut ed, vase()).unwrap();
    assert_eq!(ed.doc().object(id).unwrap().shape.kind(), "extrude");
    assert_eq!(ed.doc().object(s).unwrap().shape.kind(), "sweep");
    let mut flat = Extrude::new(rect(10.0, 10.0), 0.0);
    let e = add(&mut ed, Shape::Extrude(Box::new(flat.clone()))).unwrap_err();
    assert!(e.to_string().contains("zero"), "{e}");
    flat.height = f64::NAN;
    assert!(add(&mut ed, Shape::Extrude(Box::new(flat))).is_err());
    let mut bad = Sweep::new(rect(-10.0, 10.0));
    bad.origin.x = 1.0e12;
    assert!(add(&mut ed, Shape::Sweep(Box::new(bad))).is_err());
    let bow = Extrude::new(
        corners(&[
            DVec2::ZERO,
            DVec2::new(10.0, 10.0),
            DVec2::new(10.0, 0.0),
            DVec2::new(0.0, 10.0),
        ]),
        5.0,
    );
    let e = add(&mut ed, Shape::Extrude(Box::new(bow))).unwrap_err();
    assert!(e.to_string().contains("itself"), "{e}");
    // Changing the height is one undo step.
    let mut e = Extrude::new(rect(2000.0, 1000.0), 900.0);
    e.holes.clear();
    ed.execute(&Command::ModifyObject {
        id,
        shape: Some(Shape::Extrude(Box::new(e))),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    let Shape::Extrude(now) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    assert_eq!(now.height, 900.0);
    assert!(ed.undo());
    let Shape::Extrude(back) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    assert_eq!((back.height, back.holes.len()), (500.0, 1));
}

#[test]
fn files_round_trip_and_bad_files_are_refused() {
    let mut ed = Editor::default();
    let e = add(&mut ed, box_shape()).unwrap();
    let s = add(&mut ed, vase()).unwrap();
    let text = serde_json::to_string(ed.doc()).unwrap();
    assert!(text.contains("\"kind\":\"extrude\"") && text.contains("\"kind\":\"sweep\""));
    let back = Document::from_json(&text).unwrap();
    for id in [e, s] {
        assert_eq!(
            back.object(id).unwrap().shape,
            ed.doc().object(id).unwrap().shape
        );
    }
    let broken = text.replace("\"height\":500.0", "\"height\":0.0");
    assert!(Document::from_json(&broken).is_err());
    let broken = text.replace("\"sweep_angle_deg\":360.0", "\"sweep_angle_deg\":-5.0");
    assert!(Document::from_json(&broken).is_err());
    // A hand-written file needs only the profile and the height.
    let by_hand = text.replace(
        ",\"holes\":[[[500.0,250.0],[900.0,250.0],[900.0,650.0],[500.0,650.0]]]",
        "",
    );
    assert!(Document::from_json(&by_hand).is_ok());
}

#[test]
fn objects_move_turn_and_transform() {
    let mut ed = Editor::default();
    let e = add(&mut ed, box_shape()).unwrap();
    let s = add(&mut ed, vase()).unwrap();
    ed.execute(&Command::MoveObjects {
        ids: vec![e, s],
        delta: DVec2::new(100.0, 200.0),
    })
    .unwrap();
    let Shape::Extrude(x) = &ed.doc().object(e).unwrap().shape else {
        panic!()
    };
    assert_eq!(x.profile[0].p, DVec2::new(100.0, 200.0));
    assert_eq!(x.holes[0][0].p, DVec2::new(600.0, 450.0));
    let Shape::Sweep(w) = &ed.doc().object(s).unwrap().shape else {
        panic!()
    };
    assert_eq!(w.origin, DVec3::new(5100.0, 5200.0, 100.0));
    // Turning keeps the volume.
    let volume = |ed: &Editor, id| {
        ed.doc()
            .object(id)
            .unwrap()
            .shape
            .solid_mesh()
            .unwrap()
            .volume()
    };
    let (v0, v1) = (volume(&ed, e), volume(&ed, s));
    ed.execute(&Command::RotateObjects {
        ids: vec![e, s],
        center: DVec2::new(10.0, -20.0),
        angle: 0.7,
    })
    .unwrap();
    assert!((volume(&ed, e) - v0).abs() < 1e-3 * v0);
    assert!((volume(&ed, s) - v1).abs() < 1e-3 * v1);
    // Scale and mirror: an extrude follows (the height stays), a sweep refuses.
    ed.execute(&Command::TransformObjects {
        ids: vec![e],
        transform: Transform::Scale {
            center: DVec2::ZERO,
            sx: 2.0,
            sy: Some(0.5),
        },
        copy: false,
    })
    .unwrap();
    assert!((volume(&ed, e) - v0).abs() < 1e-3 * v0);
    ed.execute(&Command::TransformObjects {
        ids: vec![e],
        transform: Transform::Mirror {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
        copy: false,
    })
    .unwrap();
    assert!(volume(&ed, e) > 0.0);
    // A mirror and a uniform scale keep a sweep (the volume grows with the cube of the scale).
    let v_before = volume(&ed, s);
    ed.execute(&Command::TransformObjects {
        ids: vec![s],
        transform: Transform::Mirror {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
        copy: false,
    })
    .unwrap();
    assert!((volume(&ed, s) - v_before).abs() < 2e-3 * v_before);
    ed.execute(&Command::TransformObjects {
        ids: vec![s, e],
        transform: Transform::Scale {
            center: DVec2::ZERO,
            sx: 2.0,
            sy: None,
        },
        copy: false,
    })
    .unwrap();
    assert!((volume(&ed, s) - 8.0 * v_before).abs() < 2e-2 * v_before);
    let Shape::Extrude(x) = &ed.doc().object(e).unwrap().shape else {
        panic!()
    };
    assert_eq!(x.height, 1000.0, "a uniform scale scales the height too");
    // A stretch in the plan is refused for a sweep.
    let err = ed
        .execute(&Command::TransformObjects {
            ids: vec![s],
            transform: Transform::Scale {
                center: DVec2::ZERO,
                sx: 2.0,
                sy: Some(1.0),
            },
            copy: false,
        })
        .unwrap_err();
    assert!(err.to_string().contains("sweep"), "{err}");
    // A move as a transform and a copy work for a sweep.
    ed.execute(&Command::TransformObjects {
        ids: vec![s],
        transform: Transform::Move {
            delta: DVec2::new(1.0, 1.0),
        },
        copy: true,
    })
    .unwrap();
    ed.execute(&Command::TransformObjects {
        ids: vec![s],
        transform: Transform::Rotate {
            center: DVec2::ZERO,
            angle: 0.3,
        },
        copy: false,
    })
    .unwrap();
}

#[test]
fn the_3d_scene_has_a_closed_solid_at_the_layer_elevation() {
    let mut ed = Editor::default();
    ed.execute(&Command::AddLayer {
        name: "Upper".into(),
        elevation: 1000.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let layer = ed.doc().layers.last().unwrap().id;
    let out = ed
        .execute(&Command::AddObject {
            shape: box_shape(),
            layer: Some(layer),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let e = ObjectId(out.created[0]);
    let s = add(&mut ed, vase()).unwrap();
    let scene = mesh::build(ed.doc());
    let (lo, hi) = scene.object_bounds(e).expect("geometry");
    let o = scene.origin;
    assert_eq!(
        (lo.z as f64 + o.z, hi.z as f64 + o.z),
        (1000.0, 1500.0),
        "the extrusion stands on the layer"
    );
    let (lo, hi) = scene.object_bounds(s).expect("geometry");
    assert_eq!((lo.z as f64 + o.z, hi.z as f64 + o.z), (100.0, 900.0));
    // The bounds of the shape agree with the scene.
    let Shape::Sweep(w) = &ed.doc().object(s).unwrap().shape else {
        panic!()
    };
    let (blo, bhi) = w.bounds3();
    assert!((bhi.x - 5400.0).abs() < 1e-6 && (blo.x - 4600.0).abs() < 1e-6);
    // A bad solid from a file draws nothing.
    let mut ed = Editor::default();
    let bad = Extrude::new(rect(10.0, 10.0), 0.0);
    let id = add(&mut ed, box_shape()).unwrap();
    let mut doc = ed.doc().clone();
    doc.object_mut(id).unwrap().shape = Shape::Extrude(Box::new(bad));
    assert!(mesh::build(&doc).object_bounds(id).is_none());
}

#[test]
fn a_filled_3d_polygon_is_a_face_and_an_unfilled_one_is_lines() {
    let mut ed = Editor::default();
    let pts = vec![
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(1000.0, 0.0, 200.0),
        DVec3::new(1000.0, 1000.0, 400.0),
        DVec3::new(0.0, 1000.0, 200.0),
    ];
    let plain = add(
        &mut ed,
        Shape::Polyline3d {
            points: pts.clone(),
            closed: true,
        },
    )
    .unwrap();
    let filled = ed
        .execute(&Command::AddObject {
            shape: Shape::Polyline3d {
                points: pts,
                closed: true,
            },
            layer: None,
            class: None,
            attrs: Attributes {
                fill: Some(Color([200, 100, 50, 255])),
                ..Attributes::default()
            },
        })
        .unwrap();
    let filled = ObjectId(filled.created[0]);
    let scene = mesh::build(ed.doc());
    assert!(scene.object_bounds(plain).is_some(), "its lines");
    assert!(
        scene.mesh.triangle_count() == 2,
        "only the filled one has a face"
    );
    assert!(scene.object_bounds(filled).is_some());
}

#[test]
fn svg_and_snapping_follow_the_profile() {
    let mut ed = Editor::default();
    let e = add(&mut ed, box_shape()).unwrap();
    let s = add(&mut ed, vase()).unwrap();
    let out = svg::render(ed.doc(), 800);
    assert!(out.contains("data-kind=\"extrude\"") && out.contains("fill-rule=\"evenodd\""));
    assert!(out.contains("data-kind=\"sweep\""));
    let mut g = Geometry::default();
    g.add_shape(&ed.doc().object(e).unwrap().shape);
    // Four corners of the profile and four of the hole, and the middles.
    assert!(g.points.iter().any(|p| p.p == DVec2::new(2000.0, 1000.0)));
    assert!(g.points.iter().any(|p| p.p == DVec2::new(900.0, 650.0)));
    let mut g = Geometry::default();
    g.add_shape(&ed.doc().object(s).unwrap().shape);
    assert!(g.points.iter().any(|p| p.p == DVec2::new(5400.0, 4600.0)));
    // Shape helpers.
    let sh = &ed.doc().object(e).unwrap().shape;
    assert_eq!(sh.area(), Some(2.0e6 - 1.6e5));
    assert_eq!(sh.bounds(), (DVec2::ZERO, DVec2::new(2000.0, 1000.0)));
    assert!(sh.vertices().is_some() && ed.doc().object(s).unwrap().shape.vertices().is_none());
    assert!(
        sh.solid_bounds3().is_some()
            && Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X
            }
            .solid_bounds3()
            .is_none()
    );
    assert!(
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X
        }
        .solid_mesh()
        .is_none()
    );
}

#[test]
fn the_profile_vertices_can_be_edited() {
    let mut ed = Editor::default();
    let e = add(
        &mut ed,
        Shape::Extrude(Box::new(Extrude::new(rect(1000.0, 1000.0), 300.0))),
    )
    .unwrap();
    ed.execute(&Command::SetVertex {
        id: e,
        index: 2,
        kind: None,
        p: Some(DVec2::new(1500.0, 1500.0)),
    })
    .unwrap();
    let Shape::Extrude(x) = &ed.doc().object(e).unwrap().shape else {
        panic!()
    };
    assert_eq!(x.profile[2].p, DVec2::new(1500.0, 1500.0));
    assert!(x.mesh().unwrap().is_closed());
}

#[test]
fn properties_for_reports() {
    use kadakas_core::props::{Prop, property};
    use serde_json::json;
    let mut ed = Editor::default();
    let mut x = Extrude::new(rect(2000.0, 1000.0), -500.0);
    x.name = "Bench".into();
    let e = add(&mut ed, Shape::Extrude(Box::new(x))).unwrap();
    let s = add(&mut ed, vase()).unwrap();
    let mut open = Sweep::new(corners(&[DVec2::new(100.0, 0.0), DVec2::new(100.0, 500.0)]));
    open.closed = false;
    let o = add(&mut ed, Shape::Sweep(Box::new(open))).unwrap();
    let p = |id: ObjectId, name: &str| property(ed.doc(), ed.doc().object(id).unwrap(), &[name]);
    assert_eq!(p(e, "Volume"), Prop::Value(json!(1.0)));
    assert_eq!(p(e, "Height"), Prop::Value(json!(500.0)));
    assert_eq!(p(e, "Name"), Prop::Value(json!("Bench")));
    assert_eq!(p(e, "Area"), Prop::Value(json!(2.0)));
    assert_eq!(p(e, "Perimeter"), Prop::Value(json!(6.0)));
    assert!(matches!(p(s, "Volume"), Prop::Value(v) if v.as_f64().unwrap() > 0.0));
    assert_eq!(p(s, "Name"), Prop::Value(json!("")));
    assert_eq!(p(o, "Volume"), Prop::NotApplicable);
}
