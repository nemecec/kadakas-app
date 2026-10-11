use super::*;
use crate::command::Editor;
use crate::model::Attributes;
use serde_json::json;

fn add_with(ed: &mut Editor, shape: Shape, attrs: Attributes) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs,
        })
        .unwrap()
        .created[0],
    )
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    add_with(ed, shape, Attributes::default())
}

fn square(at: DVec2, w: f64) -> Shape {
    Shape::Polyline {
        points: curve::corners(&[
            at,
            at + DVec2::new(w, 0.0),
            at + DVec2::splat(w),
            at + DVec2::new(0.0, w),
        ]),
        closed: true,
    }
}

fn solid(ed: &Editor, id: ObjectId) -> &Shape {
    &ed.doc().object(id).unwrap().shape
}

#[test]
fn closed_shapes_become_extrudes_in_one_undo_step() {
    let mut ed = Editor::with_sequential_ids();
    let attrs = Attributes {
        fill: Some(crate::model::Color([10, 20, 30, 255])),
        ..Attributes::default()
    };
    let a = add_with(&mut ed, square(DVec2::ZERO, 1000.0), attrs);
    let r = add(
        &mut ed,
        Shape::Rect {
            min: DVec2::new(3000.0, 0.0),
            max: DVec2::new(2000.0, 500.0),
        },
    );
    let c = add(
        &mut ed,
        Shape::Circle {
            center: DVec2::new(5000.0, 0.0),
            radius: 400.0,
        },
    );
    ed.execute(&Command::SetData {
        id: a,
        set: [("tag".to_string(), json!("bench"))].into_iter().collect(),
        remove: vec![],
    })
    .unwrap();
    let open = add(
        &mut ed,
        Shape::Polyline {
            points: curve::corners(&[DVec2::ZERO, DVec2::X]),
            closed: false,
        },
    );
    let text = add(
        &mut ed,
        Shape::Text {
            at: DVec2::ZERO,
            text: "x".into(),
            height: 10.0,
        },
    );
    let ids = [
        a,
        r,
        c,
        open,
        text,
        a,
        ObjectId(crate::uid::Uid::from_num(999_999)),
    ];
    let opts = ExtrudeOptions {
        height: 450.0,
        base: 100.0,
        delete_source: true,
    };
    let made = extrude(ed.doc(), &ids, &opts);
    assert_eq!(made.created.len(), 3);
    let reasons: Vec<&str> = made.skipped.iter().map(|s| s.1.as_str()).collect();
    assert_eq!(
        reasons,
        [
            "the polyline is not closed",
            "a text is not a closed shape",
            "no such object"
        ]
    );
    let before = ed.doc().objects.len();
    ed.execute(made.command.as_ref().unwrap()).unwrap();
    assert_eq!(ed.doc().objects.len(), before, "three added, three removed");
    assert!(ed.doc().object(a).is_none());
    let new = made.created[0].1;
    let Shape::Extrude(e) = solid(&ed, new) else {
        panic!()
    };
    assert_eq!((e.height, e.base), (450.0, 100.0));
    let o = ed.doc().object(new).unwrap();
    assert_eq!(o.attrs.fill, Some(crate::model::Color([10, 20, 30, 255])));
    assert_eq!(o.data["tag"], json!("bench"));
    let Shape::Extrude(circle) = solid(&ed, made.created[2].1) else {
        panic!()
    };
    assert!((circle.profile_area() - std::f64::consts::PI * 400.0 * 400.0).abs() < 1.0e3);
    assert!(circle.mesh().unwrap().is_closed());
    assert!(ed.undo());
    assert_eq!(ed.doc().objects.len(), before);
    assert!(ed.doc().object(a).is_some());
}

#[test]
fn the_source_can_stay_and_a_group_member_stays_in_its_group() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(&mut ed, square(DVec2::ZERO, 1000.0));
    let b = add(&mut ed, square(DVec2::new(5000.0, 0.0), 1000.0));
    let g = ObjectId(
        ed.execute(&Command::Group {
            ids: vec![a, b],
            name: Some("G".into()),
        })
        .unwrap()
        .created[0],
    );
    let made = extrude(
        ed.doc(),
        &[a],
        &ExtrudeOptions {
            delete_source: false,
            ..ExtrudeOptions::default()
        },
    );
    ed.execute(made.command.as_ref().unwrap()).unwrap();
    assert!(ed.doc().object(a).is_some());
    assert_eq!(ed.doc().object(made.created[0].1).unwrap().parent, Some(g));
}

#[test]
fn a_landscape_area_brings_its_holes_and_bad_profiles_are_skipped() {
    let mut ed = Editor::with_sequential_ids();
    let hole = curve::corners(&[
        DVec2::new(100.0, 100.0),
        DVec2::new(300.0, 100.0),
        DVec2::new(300.0, 300.0),
        DVec2::new(100.0, 300.0),
    ]);
    let area = ObjectId(
        ed.execute(&Command::AddLandscapeArea {
            boundary: curve::corners(&[
                DVec2::ZERO,
                DVec2::new(1000.0, 0.0),
                DVec2::new(1000.0, 1000.0),
                DVec2::new(0.0, 1000.0),
            ]),
            name: "Bed".into(),
            patch: Box::new(landscape::LandscapePatch {
                holes: Some(vec![hole]),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let bow = add(
        &mut ed,
        Shape::Polyline {
            points: curve::corners(&[
                DVec2::ZERO,
                DVec2::new(10.0, 10.0),
                DVec2::new(10.0, 0.0),
                DVec2::new(0.0, 10.0),
            ]),
            closed: true,
        },
    );
    let flat = add(
        &mut ed,
        Shape::Polyline {
            points: curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::X * 2.0]),
            closed: true,
        },
    );
    let made = extrude(ed.doc(), &[area, bow, flat], &ExtrudeOptions::default());
    // A sweep of a shape with holes is skipped with a note: the holes would be lost.
    let swept = sweep(ed.doc(), &[area], &SweepOptions::default());
    assert!(swept.command.is_none() && swept.skipped[0].1.contains("holes"));
    assert_eq!(made.created.len(), 1);
    assert_eq!(made.skipped.len(), 2);
    assert!(made.skipped[0].1.contains("itself"), "{:?}", made.skipped);
    assert!(made.skipped[1].1.contains("no area"), "{:?}", made.skipped);
    ed.execute(made.command.as_ref().unwrap()).unwrap();
    let Shape::Extrude(e) = solid(&ed, made.created[0].1) else {
        panic!()
    };
    assert_eq!(e.holes.len(), 1);
    assert_eq!(e.profile_area(), 1.0e6 - 4.0e4);
    // Extruding a solid again is refused with a reason.
    let again = extrude(ed.doc(), &[made.created[0].1], &ExtrudeOptions::default());
    assert!(again.command.is_none() && again.skipped[0].1.contains("already"));
    // Polygon3d and a hardscape work too.
    let p3 = add(
        &mut ed,
        Shape::Polyline3d {
            points: vec![
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 5.0),
                DVec3::new(10.0, 10.0, 1.0),
            ],
            closed: true,
        },
    );
    assert_eq!(
        extrude(ed.doc(), &[p3], &ExtrudeOptions::default())
            .created
            .len(),
        1
    );
}

#[test]
fn options_are_checked() {
    assert!(ExtrudeOptions::default().check().is_ok());
    for h in [0.0, f64::NAN] {
        let o = ExtrudeOptions {
            height: h,
            ..ExtrudeOptions::default()
        };
        assert!(o.check().is_err());
    }
    assert!(SweepOptions::default().check().is_ok());
    let bad = |f: &dyn Fn(&mut SweepOptions)| {
        let mut o = SweepOptions::default();
        f(&mut o);
        o.check().unwrap_err()
    };
    assert!(bad(&|o| o.sweep_angle_deg = 0.0).contains("sweep_angle"));
    assert!(bad(&|o| o.sweep_angle_deg = 4000.0).contains("sweep_angle"));
    assert!(bad(&|o| o.segment_angle_deg = 0.0).contains("segment_angle"));
    assert!(bad(&|o| o.pitch = f64::INFINITY).contains("finite"));
    assert!(
        bad(&|o| {
            o.axis_id = Some(ObjectId(crate::uid::Uid::from_num(999_999)));
            o.axis_at = Some(DVec2::ZERO);
        })
        .contains("not both")
    );
}

#[test]
fn a_profile_without_an_axis_turns_around_its_left_edge() {
    let mut ed = Editor::with_sequential_ids();
    // A pot drawn as an elevation: 2 wide and 3 high, at x 5000.
    let pot = add(
        &mut ed,
        Shape::Polyline {
            points: curve::corners(&[
                DVec2::new(5000.0, 100.0),
                DVec2::new(5600.0, 100.0),
                DVec2::new(7000.0, 3100.0),
                DVec2::new(5000.0, 3100.0),
            ]),
            closed: true,
        },
    );
    let made = sweep(
        ed.doc(),
        &[pot],
        &SweepOptions {
            delete_source: false,
            ..SweepOptions::default()
        },
    );
    ed.execute(made.command.as_ref().unwrap()).unwrap();
    let Shape::Sweep(s) = solid(&ed, made.created[0].1) else {
        panic!()
    };
    assert_eq!(s.origin, DVec3::new(5000.0, 100.0, 0.0));
    assert_eq!(s.axis, DVec3::Z);
    assert_eq!(s.profile[2].p, DVec2::new(2000.0, 3000.0));
    let m = s.mesh().unwrap();
    assert!(m.is_closed() && m.signed_volume() > 0.0);
    let (lo, hi) = s.bounds3();
    assert!((hi.x - 7000.0).abs() < 1e-6 && (lo.z, hi.z) == (0.0, 3000.0));
    // With a chosen axis point.
    let made = sweep(
        ed.doc(),
        &[pot],
        &SweepOptions {
            axis_at: Some(DVec2::new(4000.0, 0.0)),
            delete_source: false,
            ..SweepOptions::default()
        },
    );
    ed.execute(made.command.as_ref().unwrap()).unwrap();
    let Shape::Sweep(s2) = solid(&ed, made.created[0].1) else {
        panic!()
    };
    assert_eq!(s2.origin, DVec3::new(4000.0, 0.0, 0.0));
    assert!(ed.doc().object(pot).is_some());
}

#[test]
fn a_profile_around_an_axis_line_lies_on_the_side_where_it_was_drawn() {
    let mut ed = Editor::with_sequential_ids();
    let axis = add(
        &mut ed,
        Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(4000.0, 0.0),
        },
    );
    let left = add(&mut ed, square(DVec2::new(1000.0, 500.0), 600.0));
    let right = add(&mut ed, square(DVec2::new(1000.0, -1100.0), 600.0));
    let across = add(&mut ed, square(DVec2::new(1000.0, -300.0), 600.0));
    let made = sweep(
        ed.doc(),
        &[axis, left, right, across],
        &SweepOptions {
            axis_id: Some(axis),
            sweep_angle_deg: 90.0,
            segment_angle_deg: 5.0,
            ..SweepOptions::default()
        },
    );
    assert_eq!(made.created.len(), 2);
    assert_eq!(made.skipped.len(), 1);
    assert!(made.skipped[0].1.contains("crosses the axis"));
    ed.execute(made.command.as_ref().unwrap()).unwrap();
    assert!(ed.doc().object(axis).is_some(), "the axis stays");
    for (n, side) in [(0, 1.0), (1, -1.0)] {
        let Shape::Sweep(s) = solid(&ed, made.created[n].1) else {
            panic!()
        };
        assert!(s.mesh().unwrap().is_closed());
        let (lo, hi) = s.bounds3();
        // The first quarter turn starts on the drawn side: in the plan, y of the same sign.
        let y = if side > 0.0 { hi.y } else { lo.y };
        assert!(y.abs() > 1000.0, "{lo} {hi}");
        assert!((lo.x - 1000.0).abs() < 1e-6 && (hi.x - 1600.0).abs() < 1e-6);
    }
    // An axis that is not a line, has no length, or does not exist.
    for (shape, text) in [
        (square(DVec2::ZERO, 10.0), "must be a line"),
        (
            Shape::Line {
                a: DVec2::ONE,
                b: DVec2::ONE,
            },
            "no length",
        ),
    ] {
        let id = add(&mut ed, shape);
        let m = sweep(
            ed.doc(),
            &[left],
            &SweepOptions {
                axis_id: Some(id),
                ..SweepOptions::default()
            },
        );
        assert!(m.command.is_none() && m.skipped[0].1.contains(text));
    }
    let m = sweep(
        ed.doc(),
        &[left],
        &SweepOptions {
            axis_id: Some(ObjectId(crate::uid::Uid::from_num(999_999))),
            ..SweepOptions::default()
        },
    );
    assert!(m.skipped[0].1.contains("does not exist"));
}

#[test]
fn open_shapes_sweep_to_surfaces_and_bad_ones_are_skipped() {
    let mut ed = Editor::with_sequential_ids();
    let line = add(
        &mut ed,
        Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(1000.0, 1000.0),
        },
    );
    let open = add(
        &mut ed,
        Shape::Polyline {
            points: curve::corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(500.0, 200.0),
                DVec2::new(500.0, 900.0),
            ]),
            closed: false,
        },
    );
    let text = add(
        &mut ed,
        Shape::Text {
            at: DVec2::ZERO,
            text: "t".into(),
            height: 5.0,
        },
    );
    let made = sweep(
        ed.doc(),
        &[
            line,
            open,
            text,
            ObjectId(crate::uid::Uid::from_num(999_999)),
        ],
        &SweepOptions {
            delete_source: false,
            ..SweepOptions::default()
        },
    );
    assert_eq!(made.created.len(), 2);
    assert_eq!(made.skipped.len(), 2);
    ed.execute(made.command.as_ref().unwrap()).unwrap();
    let Shape::Sweep(s) = solid(&ed, made.created[1].1) else {
        panic!()
    };
    assert!(!s.closed && s.mesh().unwrap().area() > 0.0);
    // A profile that cannot be swept (a huge number of segments for a big profile) is skipped.
    let m = sweep(
        ed.doc(),
        &[open],
        &SweepOptions {
            sweep_angle_deg: 3600.0,
            pitch: 5000.0,
            segment_angle_deg: solid3d::MIN_SEGMENT_ANGLE,
            ..SweepOptions::default()
        },
    );
    assert_eq!(
        m.created.len(),
        1,
        "the segment count is lowered, not refused"
    );
    let bounds = union_bounds(ed.doc(), &[made.created[0].1, made.created[1].1]);
    assert!(bounds.is_some());
    assert!(union_bounds(ed.doc(), &[text]).is_none());
}
