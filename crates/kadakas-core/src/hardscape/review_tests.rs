//! Regression tests for paths (alignment, offset, closed), slopes by angle and by rise and run,
//! edge elevations after an outline edit, partial draping, and the limits for agent input.

use super::*;
use crate::command::{Command, CommandError, Editor};
use crate::curve::{self, Vertex};
use crate::ground::{GroundSource, Surface};
use crate::model::{Attributes, ObjectId};
use crate::resource::{ResourceBody, ResourceId};

fn sq(w: f64) -> Vec<Vertex> {
    curve::corners(&[
        DVec2::ZERO,
        DVec2::new(w, 0.0),
        DVec2::new(w, w),
        DVec2::new(0.0, w),
    ])
}

fn add(ed: &mut Editor, boundary: Vec<Vertex>, patch: HardscapePatch) -> ObjectId {
    let c = Command::AddHardscape {
        boundary: Some(boundary),
        patch: Box::new(patch),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    };
    ObjectId(ed.execute(&c).unwrap().created[0])
}

fn add_path(ed: &mut Editor, center: &[DVec2], patch: HardscapePatch) -> ObjectId {
    let patch = HardscapePatch {
        center: Some(curve::corners(center)),
        ..patch
    };
    let c = Command::AddHardscape {
        boundary: None,
        patch: Box::new(patch),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    };
    ObjectId(ed.execute(&c).unwrap().created[0])
}

fn y_range(ed: &Editor, id: ObjectId) -> (f64, f64) {
    let (lo, hi) = ed.doc().object(id).unwrap().shape.bounds();
    (lo.y, hi.y)
}

#[test]
fn a_path_can_be_aligned_to_its_line_and_offset_from_it() {
    let mut ed = Editor::default();
    let line = [DVec2::ZERO, DVec2::new(10_000.0, 0.0)];
    let id = add_path(
        &mut ed,
        &line,
        HardscapePatch {
            width: Some(1000.0),
            ..Default::default()
        },
    );
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-2 && (a.1 - b.1).abs() < 1e-2;
    assert!(near(y_range(&ed, id), (-500.0, 500.0)));
    // The line is the left edge: the path lies to its right.
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            align: Some(offset::Align::Left),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(near(y_range(&ed, id), (-1000.0, 0.0)));
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            align: Some(offset::Align::Right),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(near(y_range(&ed, id), (0.0, 1000.0)));
    // An offset moves the path to the left of the line (positive) or to the right.
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            align: Some(offset::Align::Center),
            offset: Some(300.0),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(near(y_range(&ed, id), (-200.0, 800.0)));
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            offset: Some(-700.0),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(near(y_range(&ed, id), (-1200.0, -200.0)));
    // The area does not change, and the center line stays where it was drawn.
    let q = report::quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
    assert!((q.plan_area_m2 - 10.0).abs() < 1e-6);
    assert_eq!(
        data(ed.doc().object(id).unwrap()).unwrap().center[0].p,
        DVec2::ZERO
    );
    // A bad offset is refused.
    let bad = ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            offset: Some(f64::NAN),
            ..Default::default()
        },
    });
    assert!(bad.is_err());
}

#[test]
fn a_closed_path_is_a_ring_without_ends() {
    let mut ed = Editor::default();
    let ring = [
        DVec2::ZERO,
        DVec2::new(4000.0, 0.0),
        DVec2::new(4000.0, 4000.0),
        DVec2::new(0.0, 4000.0),
    ];
    let id = add_path(
        &mut ed,
        &ring,
        HardscapePatch {
            width: Some(1000.0),
            closed: Some(true),
            ..Default::default()
        },
    );
    let q = report::quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
    assert!(
        (q.plan_area_m2 - 16.0).abs() < 1e-6,
        "5 x 5 m minus 3 x 3 m: {}",
        q.plan_area_m2
    );
    // The 3D solid and the 2D drawing can use the ring.
    let scene = crate::mesh::build(ed.doc());
    assert!(scene.mesh.triangle_count() > 8);
    assert!(crate::svg::render(ed.doc(), 400).contains("polygon"));
    // Too few points for a ring are refused.
    let c = Command::AddHardscape {
        boundary: None,
        patch: Box::new(HardscapePatch {
            center: Some(curve::corners(&ring[..2])),
            closed: Some(true),
            ..Default::default()
        }),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    };
    assert!(ed.execute(&c).is_err());
}

#[test]
fn slopes_by_angle_and_by_rise_and_run() {
    let mut ed = Editor::default();
    let id = add(&mut ed, sq(2000.0), HardscapePatch::default());
    let z = |ed: &Editor, p: DVec2| {
        surface::surface_of(
            ed.doc().object(id).unwrap(),
            data(ed.doc().object(id).unwrap()).as_ref(),
            None,
        )
        .z(p)
    };
    // 1 over 20 is 5 percent. It falls toward +X.
    let rr = Slope::RiseRun {
        rise: 1.0,
        run: 20.0,
        angle_deg: 0.0,
    };
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            slope: Some(rr),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(
        (z(&ed, DVec2::new(1000.0, 1000.0)) - z(&ed, DVec2::new(2000.0, 1000.0)) - 50.0).abs()
            < 1e-9
    );
    // 45 degrees is 100 percent. It falls toward +Y.
    let by_angle = Slope::Angle {
        degrees: 45.0,
        angle_deg: 90.0,
    };
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            slope: Some(by_angle),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(
        (z(&ed, DVec2::new(1000.0, 1000.0)) - z(&ed, DVec2::new(1000.0, 2000.0)) - 1000.0).abs()
            < 1e-6
    );
    let q = report::quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
    assert!((q.surface_area_m2 - 4.0 * 2.0f64.sqrt()).abs() < 1e-6);
    // JSON names.
    let j: Slope =
        serde_json::from_value(serde_json::json!({"type": "rise_run", "rise": 1, "run": 12}))
            .unwrap();
    assert_eq!(j.id(), "rise_run");
    assert!((j.fall().unwrap().0 - 100.0 / 12.0).abs() < 1e-9);
    assert_eq!(Slope::None.fall(), None);
    // Bad values.
    for bad in [
        Slope::RiseRun {
            rise: 1.0,
            run: 0.0,
            angle_deg: 0.0,
        },
        Slope::RiseRun {
            rise: f64::NAN,
            run: 1.0,
            angle_deg: 0.0,
        },
        Slope::Angle {
            degrees: 60.0,
            angle_deg: 0.0,
        },
        Slope::Angle {
            degrees: f64::NAN,
            angle_deg: 0.0,
        },
        Slope::RiseRun {
            rise: 50.0,
            run: 1.0,
            angle_deg: 0.0,
        },
    ] {
        let r = ed.execute(&Command::SetHardscape {
            ids: vec![id],
            patch: HardscapePatch {
                slope: Some(bad.clone()),
                ..Default::default()
            },
        });
        assert!(r.is_err(), "{bad:?}");
    }
    // On a path the fall is to the right of the line, whatever the angle.
    let p = add_path(
        &mut ed,
        &[DVec2::ZERO, DVec2::new(5000.0, 0.0)],
        HardscapePatch {
            width: Some(1000.0),
            slope: Some(Slope::RiseRun {
                rise: 1.0,
                run: 10.0,
                angle_deg: 33.0,
            }),
            ..Default::default()
        },
    );
    let o = ed.doc().object(p).unwrap();
    let s = surface::surface_of(o, data(o).as_ref(), None);
    assert!((s.z(DVec2::new(2500.0, 0.0)) - s.z(DVec2::new(2500.0, -400.0)) - 40.0).abs() < 1e-9);
}

#[test]
fn edge_elevations_follow_an_outline_edit() {
    let mut ed = Editor::default();
    let id = add(
        &mut ed,
        sq(2000.0),
        HardscapePatch {
            slope: Some(Slope::Edges {
                elevations: vec![0.0, 0.0, 200.0, 200.0],
            }),
            ..Default::default()
        },
    );
    let before = surface::surface_of(
        ed.doc().object(id).unwrap(),
        data(ed.doc().object(id).unwrap()).as_ref(),
        None,
    );
    // A fifth corner on the bottom edge (ModifyObject with a new outline).
    let mut b = sq(2000.0);
    b.insert(
        1,
        Vertex::new(DVec2::new(1000.0, 0.0), curve::VertexKind::Corner),
    );
    let shape = Shape::Hardscape {
        boundary: b,
        material: "m".into(),
        thickness: None,
        elevation: None,
    };
    ed.execute(&Command::ModifyObject {
        id,
        shape: Some(shape),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    let o = ed.doc().object(id).unwrap();
    let Slope::Edges { elevations } = data(o).unwrap().slope else {
        panic!("the slope stays")
    };
    assert_eq!(elevations.len(), 5);
    assert!((elevations[1] - 0.0).abs() < 1e-9 && (elevations[3] - 200.0).abs() < 1e-9);
    let after = surface::surface_of(o, data(o).as_ref(), None);
    for p in [DVec2::new(100.0, 100.0), DVec2::new(1500.0, 1800.0)] {
        assert!(
            (before.z(p) - after.z(p)).abs() < 1e-9,
            "the surface is the same"
        );
    }
    // Later changes still work.
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            elevation: Some(10.0),
            ..Default::default()
        },
    })
    .unwrap();
    // A hand-made file with the wrong number of heights is repaired by the next change, not refused.
    let mut o = ed.doc().object(id).unwrap().clone();
    let mut d = data(&o).unwrap();
    d.slope = Slope::Edges {
        elevations: vec![1.0, 2.0],
    };
    data::set_data(&mut o, &d);
    let mut doc = ed.doc().clone();
    *doc.object_mut(id).unwrap() = o;
    let mut ed2 = Editor::new(doc);
    ed2.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            drape: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    let Slope::Edges { elevations } = data(ed2.doc().object(id).unwrap()).unwrap().slope else {
        panic!()
    };
    assert_eq!(elevations.len(), 5);
    // Moving a corner keeps the heights of the corners.
    ed.execute(&Command::SetVertex {
        id,
        index: 2,
        kind: None,
        p: Some(DVec2::new(2100.0, 50.0)),
    })
    .unwrap();
    let Slope::Edges {
        elevations: after_move,
    } = data(ed.doc().object(id).unwrap()).unwrap().slope
    else {
        panic!()
    };
    assert_eq!(after_move, vec![0.0, 0.0, 0.0, 200.0, 200.0]);
}

/// A ground that exists for x below a limit only, and levels off beyond it.
struct HalfGround;

impl GroundSource for HalfGround {
    fn exact(&self, _obj: Option<&Object>, p: DVec2, _: Surface) -> Option<f64> {
        (p.x <= 1000.0).then(|| p.x / 10.0)
    }

    fn clamped(&self, _obj: Option<&Object>, p: DVec2, _: Surface) -> Option<f64> {
        Some(p.x.min(1000.0) / 10.0)
    }
}

struct NoGround;

impl GroundSource for NoGround {
    fn exact(&self, _obj: Option<&Object>, _p: DVec2, _: Surface) -> Option<f64> {
        None
    }
}

#[test]
fn a_hardscape_half_on_the_ground_is_partially_draped() {
    let mut ed = Editor::default();
    // 2 m by 2 m, from x = 0 to 2000. The ground is a ramp of 10 percent up to x = 1000, then level.
    let id = add(
        &mut ed,
        sq(2000.0),
        HardscapePatch {
            thickness: Some(100.0),
            drape: Some(true),
            ..Default::default()
        },
    );
    let r = report::report(ed.doc(), &HalfGround);
    let q = &r.rows[0].quantities;
    assert!(q.draped && q.partially_draped);
    // 1 m by 2 m on the ramp (x 1.005) and 1 m by 2 m level: 4.01 m2, not a huge number.
    assert!(
        (q.surface_area_m2 - (2.0 * 1.00499 + 2.0)).abs() < 0.01,
        "{}",
        q.surface_area_m2
    );
    assert!((q.volume_m3 - q.surface_area_m2 * 0.1).abs() < 1e-9);
    // Fully on the ground: not partial. Without ground at all: not draped.
    let on = report::report(ed.doc(), &Everywhere);
    assert!(on.rows[0].quantities.draped && !on.rows[0].quantities.partially_draped);
    let off = report::report(ed.doc(), &NoGround);
    assert!(!off.rows[0].quantities.draped && !off.rows[0].quantities.partially_draped);
    assert!((off.rows[0].quantities.surface_area_m2 - 4.0).abs() < 1e-9);
    assert_eq!(r.rows[0].id, id);
}

struct Everywhere;

impl GroundSource for Everywhere {
    fn exact(&self, _obj: Option<&Object>, p: DVec2, _: Surface) -> Option<f64> {
        Some(p.x / 10.0)
    }
}

#[test]
fn draped_surfaces_lie_above_the_ground() {
    let mut ed = Editor::default();
    let id = add(
        &mut ed,
        sq(1000.0),
        HardscapePatch {
            drape: Some(true),
            elevation: Some(0.0),
            ..Default::default()
        },
    );
    let o = ed.doc().object(id).unwrap();
    let ground = |p: DVec2| Some(p.x / 10.0);
    let s = surface::surface_of(o, data(o).as_ref(), Some(&ground));
    assert_eq!(
        s.z(DVec2::new(500.0, 0.0)),
        50.0 + surface::LIFT,
        "never exactly on the terrain"
    );
}

#[test]
fn boundaries_from_agents_have_a_vertex_limit() {
    let mut ed = Editor::default();
    let many: Vec<Vertex> = (0..data::MAX_VERTICES + 1)
        .map(|i| {
            Vertex::new(
                DVec2::from_angle(i as f64 * 1e-4) * 1000.0,
                curve::VertexKind::Corner,
            )
        })
        .collect();
    for shape in [
        Shape::Hardscape {
            boundary: many.clone(),
            material: "m".into(),
            thickness: None,
            elevation: None,
        },
        Shape::LandscapeArea {
            boundary: many,
            name: "a".into(),
        },
    ] {
        let err = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap_err();
        assert!(matches!(err, CommandError::Invalid(m) if m.contains("at most")));
    }
    let center = curve::corners(
        &(0..data::MAX_VERTICES + 1)
            .map(|i| DVec2::new(i as f64, 0.0))
            .collect::<Vec<_>>(),
    );
    let c = Command::AddHardscape {
        boundary: None,
        patch: Box::new(HardscapePatch {
            center: Some(center),
            ..Default::default()
        }),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    };
    assert!(ed.execute(&c).is_err());
}

#[test]
fn joints_of_a_huge_boundary_are_bounded() {
    // 200,000 vertices on a circle and joints of 10 mm: the work is limited, not 6 seconds.
    let ring: Vec<DVec2> = (0..200_000)
        .map(|i| DVec2::from_angle(i as f64 / 200_000.0 * std::f64::consts::TAU) * 50_000.0)
        .collect();
    let pattern = JointPattern {
        unit_length: 20.0,
        unit_width: 10.0,
        spacing: 1.0,
        ..Default::default()
    };
    let j = joints::joint_lines(&ring, &pattern);
    assert!(j.truncated || !j.segments.is_empty());
    // A reasonable pattern on the same boundary still works.
    let j = joints::joint_lines(
        &ring,
        &JointPattern {
            unit_length: 4000.0,
            unit_width: 2000.0,
            ..pattern
        },
    );
    assert!(!j.truncated && j.segments.len() > 100);
    // An area that needs more joint lines than allowed is drawn with larger units.
    let rect = vec![
        DVec2::ZERO,
        DVec2::new(20_000.0, 0.0),
        DVec2::new(20_000.0, 20_000.0),
        DVec2::new(0.0, 20_000.0),
    ];
    let fine = joints::joint_lines(
        &rect,
        &JointPattern {
            kind: JointKind::Stack,
            unit_length: 100.0,
            unit_width: 100.0,
            spacing: 0.0,
            ..pattern
        },
    );
    assert!(
        !fine.truncated && !fine.segments.is_empty() && fine.segments.len() < joints::MAX_SEGMENTS
    );
}

#[test]
fn style_ids_in_a_file_that_do_not_exist_are_ignored() {
    // A file may name a style that is gone. The hardscape keeps its own fields.
    let mut ed = Editor::default();
    let id = add(&mut ed, sq(1000.0), HardscapePatch::default());
    let mut o = ed.doc().object(id).unwrap().clone();
    let d = HardscapeData {
        style: Some(ResourceId::from_num(999)),
        ..Default::default()
    };
    data::set_data(&mut o, &d);
    assert!(data::style_of(ed.doc(), &o).is_none());
    assert!(!matches!(
        ed.doc()
            .resources
            .get(ResourceId::from_num(999))
            .map(|r| &r.body),
        Some(ResourceBody::HardscapeStyle(_))
    ));
}
