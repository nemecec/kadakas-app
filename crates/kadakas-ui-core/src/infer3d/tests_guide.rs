//! The inference of guides (MOD-05): "Guide Point" and "On Guide", with the usual priorities.

use super::*;
use glam::{DVec2, DVec3};
use kadakas_core::guide::Guide;

fn cam() -> Camera {
    Camera::look_at(
        DVec3::new(0.0, -10000.0, 6000.0),
        DVec3::new(1000.0, 0.0, 0.0),
        DVec3::Z,
        Projection::Perspective { fov_y: 0.8 },
        DVec2::new(1000.0, 800.0),
    )
    .unwrap()
}

fn px(c: &Camera, p: DVec3) -> DVec2 {
    c.project(p).unwrap()
}

fn scene(f: impl FnOnce(&mut SceneBuilder)) -> Scene {
    let mut b = SceneBuilder::new();
    f(&mut b);
    b.finish()
}

#[test]
fn a_guide_point_is_a_point_with_its_own_label_and_color() {
    let p = DVec3::new(1500.0, 800.0, 0.0);
    let s = scene(|b| {
        b.add_guide(&Guide::point(p), 0.0, 3);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let r = i.infer(&s, &c, px(&c, p) + DVec2::new(5.0, 4.0), None);
    assert_eq!(r.kind, Kind::GuidePoint);
    assert!(r.kind.is_point());
    assert_eq!(r.point, p);
    assert_eq!(r.label, "Guide Point");
    assert_eq!(r.color, GUIDE);
    assert_eq!(r.source, Tag::guide(3));
    assert_eq!(r.source.kind(), TagKind::Guide);
    assert!(matches!(r.constraint, Some(Constraint::Point(q)) if q == p));
    // Too far from the point: nothing to snap to.
    let r = i.infer(&s, &c, px(&c, p) + DVec2::new(40.0, 0.0), None);
    assert_eq!(r.kind, Kind::Free);
}

#[test]
fn a_guide_line_snaps_along_its_whole_length() {
    // A line without ends along X at y = 2000, and one along Y that stops at 3000 mm.
    let s = scene(|b| {
        b.add_guide(&Guide::line(DVec3::new(0.0, 2000.0, 0.0), DVec3::X), 0.0, 0);
        b.add_guide(&Guide::segment(DVec3::new(500.0, 0.0, 0.0), DVec3::Y, 3000.0), 0.0, 1);
    });
    let c = cam();
    let mut i = Inferencer::default();
    for x in [-8000.0, 0.0, 1234.0, 30000.0] {
        let p = DVec3::new(x, 2000.0, 0.0);
        let r = i.infer(&s, &c, px(&c, p) + DVec2::new(0.0, 3.0), None);
        assert_eq!(r.kind, Kind::OnGuide, "{x}");
        assert!((r.point.y - 2000.0).abs() < 1e-6 && r.point.z.abs() < 1e-6);
        assert_eq!(r.label, "On Guide");
        assert_eq!(r.color, GUIDE);
        assert_eq!(r.source, Tag::guide(0));
        match r.constraint {
            Some(Constraint::Line { dir, .. }) => assert!(dir.dot(DVec3::X).abs() > 0.999),
            other => panic!("{other:?}"),
        }
        assert!(!r.kind.is_point());
    }
    // The finite line holds up to its end and not beyond.
    let mut on = |y: f64| i.infer(&s, &c, px(&c, DVec3::new(500.0, y, 0.0)), None);
    assert_eq!(on(1500.0).kind, Kind::OnGuide);
    assert_eq!(on(1500.0).source, Tag::guide(1));
    assert_eq!(on(9000.0).kind, Kind::Free, "past the end");
}

#[test]
fn the_ends_and_the_middle_of_a_guide_are_no_points() {
    let s = scene(|b| {
        b.add_guide(&Guide::segment(DVec3::ZERO, DVec3::X, 2000.0), 0.0, 0);
    });
    let c = cam();
    let mut i = Inferencer::default();
    for p in [DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(2000.0, 0.0, 0.0)] {
        let r = i.infer(&s, &c, px(&c, p), None);
        assert_eq!(r.kind, Kind::OnGuide, "{p}");
    }
}

#[test]
fn an_endpoint_wins_over_a_guide_point_and_an_edge_over_a_guide_line() {
    let at = DVec3::new(1000.0, 0.0, 0.0);
    let s = scene(|b| {
        b.add_edge(DVec3::ZERO, at, Tag(7));
        b.add_guide(&Guide::point(at), 0.0, 0);
        // A guide along the same line.
        b.add_guide(&Guide::line(DVec3::ZERO, DVec3::X), 0.0, 1);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let r = i.infer(&s, &c, px(&c, at), None);
    assert_eq!(r.kind, Kind::Endpoint, "the endpoint comes first");
    assert_eq!(r.source, Tag(7));
    // In the middle of the edge the guide line is at the same place: the edge wins.
    let r = i.infer(&s, &c, px(&c, DVec3::new(300.0, 0.0, 0.0)), None);
    assert_eq!(r.kind, Kind::OnEdge);
    assert_eq!(r.source, Tag(7));
    // Beyond the edge only the guide is left.
    let r = i.infer(&s, &c, px(&c, DVec3::new(3000.0, 0.0, 0.0)), None);
    assert_eq!(r.kind, Kind::OnGuide);
}

#[test]
fn a_guide_point_beats_a_guide_line_and_a_line_beats_a_face() {
    let p = DVec3::new(500.0, 0.0, 0.0);
    let s = scene(|b| {
        b.add_guide(&Guide::line(DVec3::ZERO, DVec3::X), 0.0, 0);
        b.add_guide(&Guide::point(p), 0.0, 1);
        b.add_face(
            vec![
                DVec3::new(-5000.0, -5000.0, 0.0),
                DVec3::new(5000.0, -5000.0, 0.0),
                DVec3::new(5000.0, 5000.0, 0.0),
                DVec3::new(-5000.0, 5000.0, 0.0),
            ],
            vec![],
            Tag(9),
        );
    });
    let c = cam();
    let mut i = Inferencer::default();
    assert_eq!(i.infer(&s, &c, px(&c, p), None).kind, Kind::GuidePoint);
    let r = i.infer(&s, &c, px(&c, DVec3::new(-2000.0, 0.0, 0.0)), None);
    assert_eq!(r.kind, Kind::OnGuide, "on the face and on the guide: the guide");
    let r = i.infer(&s, &c, px(&c, DVec3::new(-2000.0, 1500.0, 0.0)), None);
    assert_eq!(r.kind, Kind::OnFace);
}

#[test]
fn a_guide_crosses_an_edge_and_another_guide() {
    let s = scene(|b| {
        b.add_edge(DVec3::new(1000.0, -2000.0, 0.0), DVec3::new(1000.0, 3000.0, 0.0), Tag(4));
        b.add_guide(&Guide::line(DVec3::ZERO, DVec3::X), 0.0, 0);
        b.add_guide(&Guide::line(DVec3::new(-1500.0, 0.0, 0.0), DVec3::Y), 0.0, 1);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let r = i.infer(&s, &c, px(&c, DVec3::new(1000.0, 0.0, 0.0)) + DVec2::new(2.0, 1.0), None);
    assert_eq!(r.kind, Kind::Intersection);
    assert!(r.point.distance(DVec3::new(1000.0, 0.0, 0.0)) < 1e-3);
    assert!(r.source == Tag(4) || r.other == Tag(4));
    assert!(r.source == Tag::guide(0) || r.other == Tag::guide(0));
    let r = i.infer(&s, &c, px(&c, DVec3::new(-1500.0, 0.0, 0.0)), None);
    assert_eq!(r.kind, Kind::Intersection);
    assert!(r.point.distance(DVec3::new(-1500.0, 0.0, 0.0)) < 1e-3);
    assert_eq!(r.source, Tag::guide(0));
    assert_eq!(r.other, Tag::guide(1));
}

#[test]
fn an_axis_line_from_the_reference_point_crosses_a_guide() {
    let s = scene(|b| {
        b.add_guide(&Guide::line(DVec3::new(2500.0, 0.0, 0.0), DVec3::Y), 0.0, 0);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let r = i.infer(
        &s,
        &c,
        px(&c, DVec3::new(2500.0, 0.0, 0.0)),
        Some(DVec3::ZERO),
    );
    assert_eq!(r.kind, Kind::Intersection);
    assert!(r.point.distance(DVec3::new(2500.0, 0.0, 0.0)) < 1e-3, "{:?}", r.point);
    assert_eq!(r.source, Tag::guide(0));
}

#[test]
fn a_tool_can_skip_the_guides() {
    let s = scene(|b| {
        b.add_guide(&Guide::line(DVec3::ZERO, DVec3::X), 0.0, 0);
        b.add_guide(&Guide::point(DVec3::new(500.0, 0.0, 0.0)), 0.0, 1);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let skip_guides = |t: Tag| t.kind() == TagKind::Guide;
    let r = i.infer_scenes(&[&s], &c, px(&c, DVec3::new(500.0, 0.0, 0.0)), None, &skip_guides);
    assert_eq!(r.kind, Kind::Free);
    let r = i.infer_scenes(&[&s], &c, px(&c, DVec3::new(500.0, 0.0, 0.0)), None, &|_| false);
    assert_eq!(r.kind, Kind::GuidePoint);
}

#[test]
fn shift_keeps_a_guide_point_as_a_through_point_and_the_estonian_labels_exist() {
    let guide = DVec3::new(2000.0, 1000.0, 0.0);
    let s = scene(|b| {
        b.add_guide(&Guide::point(guide), 0.0, 0);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let reference = DVec3::ZERO;
    let r = i.infer(&s, &c, px(&c, guide), Some(reference));
    assert_eq!(r.kind, Kind::GuidePoint);
    assert!(i.key(Key::ShiftDown));
    // Far from the point, the lock keeps the line from the reference point through it.
    let r = i.infer(&s, &c, px(&c, DVec3::new(4000.0, 2000.0, 0.0)), Some(reference));
    assert_eq!(r.kind, Kind::ThroughPoint);
    assert!(r.locked);
    assert!(r.point.cross(guide).length() / guide.length() < 1.0);
    kadakas_l10n::with_language("et", || {
        assert_eq!(Kind::GuidePoint.label(), "Abipunkt");
        assert_eq!(Kind::OnGuide.label(), "Abijoonel");
    });
}

#[test]
fn a_line_without_ends_is_clipped_to_the_view_for_drawing() {
    let c = cam();
    // The view is 1000 x 800 pixels.
    let s = c
        .view_segment(
            DVec3::new(-GUIDE_REACH, 2000.0, 0.0),
            DVec3::new(GUIDE_REACH, 2000.0, 0.0),
        )
        .expect("the line crosses the view");
    for p in s {
        assert!(p.x >= -1e-6 && p.x <= 1000.0 + 1e-6 && p.y >= -1e-6 && p.y <= 800.0 + 1e-6, "{p}");
    }
    assert!(s[0].distance(s[1]) > 100.0);
    // A line far behind the camera has nothing to draw.
    assert!(
        c.view_segment(
            DVec3::new(-GUIDE_REACH, -20000.0, 0.0),
            DVec3::new(GUIDE_REACH, -20000.0, 0.0)
        )
        .is_none()
    );
    // The clip itself.
    let lo = DVec2::ZERO;
    let hi = DVec2::new(10.0, 10.0);
    assert_eq!(
        clip_segment(DVec2::new(-5.0, 5.0), DVec2::new(15.0, 5.0), lo, hi),
        Some([DVec2::new(0.0, 5.0), DVec2::new(10.0, 5.0)])
    );
    assert_eq!(clip_segment(DVec2::new(-5.0, -5.0), DVec2::new(-1.0, 20.0), lo, hi), None);
    assert_eq!(clip_segment(DVec2::new(20.0, 5.0), DVec2::new(30.0, 5.0), lo, hi), None);
    assert_eq!(clip_segment(DVec2::new(5.0, 20.0), DVec2::new(5.0, 30.0), lo, hi), None);
    assert!(clip_segment(DVec2::new(f64::NAN, 0.0), DVec2::ONE, lo, hi).is_none());
    let inside = clip_segment(DVec2::ONE, DVec2::new(2.0, 2.0), lo, hi).unwrap();
    assert_eq!(inside, [DVec2::ONE, DVec2::new(2.0, 2.0)]);
}
