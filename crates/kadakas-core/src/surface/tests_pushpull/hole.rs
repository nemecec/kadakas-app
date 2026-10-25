//! Tests of the hole through a slab (Push/Pull of a face on one side exactly to the other side).

use proptest::prelude::*;

use super::*;

/// The face drawn by `pts` (a closed polygon) on the plane at `z`: the face inside it.
fn face_inside(m: &SurfaceModel, pts: &[P]) -> FaceId {
    let c = pts.iter().fold(DVec3::ZERO, |a, p| a + DVec3::from(*p)) / pts.len() as f64;
    m.faces()
        .find(|(id, f)| {
            f.holes.is_empty()
                && on_plane(&f.plane, c)
                && super::super::sync::face_holds(m, f, c)
                && m.face(*id).is_some()
        })
        .map(|(id, _)| id)
        .expect("the face drawn")
}

fn bottom_face(m: &SurfaceModel) -> FaceId {
    m.faces()
        .filter(|(_, f)| f.plane.normal.z < -0.9)
        .min_by(|a, b| a.1.plane.offset.total_cmp(&b.1.plane.offset))
        .map(|(id, _)| id)
        .unwrap()
}

/// The box of `box_model` (1000 x 1000 x 500) with a square of `w` at (`x`, `y`) on the top.
fn box_with_square(x: f64, y: f64, w: f64) -> (SurfaceModel, FaceId) {
    let mut m = box_model();
    let sq = square(x, y, w, 500.0);
    poly(&mut m, &sq);
    let f = face_inside(&m, &sq);
    (m, f)
}

/// The face of the top with one hole whose outer loop lies between `lo` and `hi` in x and y.
fn ring_within(m: &SurfaceModel, lo: f64, hi: f64) -> FaceId {
    m.faces()
        .find(|(_, f)| {
            f.plane.normal.z > 0.9
                && f.holes.len() == 1
                && f.outer.iter().all(|v| {
                    let p = m.vertex(*v).unwrap();
                    (lo..=hi).contains(&p.x) && (lo..=hi).contains(&p.y)
                })
        })
        .map(|(id, _)| id)
        .expect("the ring")
}

fn assert_closed(m: &SurfaceModel) {
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
}

#[test]
fn a_square_pushed_to_the_bottom_cuts_a_hole_through() {
    let (mut m, f) = box_with_square(250.0, 250.0, 500.0);
    let top = m
        .faces()
        .find(|(_, g)| g.plane.normal.z > 0.9 && g.holes.len() == 1)
        .unwrap()
        .0;
    let bottom = bottom_face(&m);
    let rev = m.rev();
    pull(&mut m, f, -500.0).unwrap();
    assert_closed(&m);
    assert_eq!(m.rev(), rev + 1, "one edit");
    assert!(m.face(f).is_none(), "the cap goes");
    assert_eq!(m.face(top).unwrap().holes.len(), 1);
    assert_eq!(m.face(bottom).unwrap().holes.len(), 1);
    // 6 faces of the box and 4 walls of the hole; 8 + 8 vertices, 12 + 4 + 4 + 4 edges.
    assert_eq!(counts(&m), (16, 24, 10));
    let v = 1000.0 * 1000.0 * 500.0 - 500.0 * 500.0 * 500.0;
    assert!((volume(&m) - v).abs() < 1e-3, "{}", volume(&m));
    // The walls of the hole face into the hole.
    let mid = DVec3::new(500.0, 500.0, 250.0);
    let walls: Vec<_> = m
        .faces()
        .filter(|(_, f)| {
            f.plane.normal.z.abs() < 1e-9
                && f.outer.iter().all(|v| {
                    let p = m.vertex(*v).unwrap();
                    p.x > 200.0 && p.x < 800.0 && p.y > 200.0 && p.y < 800.0
                })
        })
        .collect();
    assert_eq!(walls.len(), 4);
    for (_, w) in walls {
        assert!(w.plane.distance(mid) > 0.0);
    }
}

#[test]
fn a_push_within_the_tolerance_of_the_bottom_goes_exactly_there() {
    for d in [-500.004, -499.996] {
        let (mut m, f) = box_with_square(250.0, 250.0, 500.0);
        pull(&mut m, f, d).unwrap();
        assert_closed(&m);
        assert!(m.face(f).is_none(), "{d}");
        assert!(m.vertices().all(|(_, p)| p.z == 0.0 || p.z == 500.0), "{d}");
    }
}

#[test]
fn a_push_short_of_the_bottom_is_a_pocket_and_past_it_is_refused() {
    let (m0, f) = box_with_square(250.0, 250.0, 500.0);
    let mut m = m0.clone();
    pull(&mut m, f, -499.0).unwrap();
    assert!(m.face(f).is_some(), "a pocket keeps its floor");
    assert_closed(&m);
    let mut m = m0.clone();
    refused_with(&mut m, f, -600.0, "exactly on the opposite face");
    // A pull out of the top is a bump, never a hole.
    let mut m = m0.clone();
    pull(&mut m, f, 500.0).unwrap();
    assert!(m.face(f).is_some());
}

#[test]
fn a_square_drawn_on_the_bottom_pushed_up_cuts_the_hole_too() {
    let mut m = box_model();
    let sq = square(100.0, 100.0, 300.0, 0.0);
    poly(&mut m, &sq);
    let f = face_inside(&m, &sq);
    assert!(m.face(f).unwrap().plane.normal.z < 0.0, "it looks down");
    let top = top_face(&m);
    pull(&mut m, f, -500.0).unwrap();
    assert_closed(&m);
    assert_eq!(m.face(top).unwrap().holes.len(), 1);
}

#[test]
fn a_face_that_touches_the_border_of_the_bottom_is_refused() {
    // A square at the edge of the top: its cap would touch the border of the bottom.
    let (mut m, f) = box_with_square(0.0, 250.0, 500.0);
    let why = refused_with(&mut m, f, -500.0, "not fully inside");
    assert!(why.contains("opposite face"), "{why}");
    // The whole top pushed to the bottom (zero thickness) stays refused.
    let mut m = box_model();
    let top = top_face(&m);
    assert!(pull(&mut m, top, -500.0).is_err());
}

#[test]
fn a_new_hole_may_not_overlap_an_old_one() {
    let (mut m, f) = box_with_square(400.0, 400.0, 200.0);
    pull(&mut m, f, -500.0).unwrap();
    // A larger square around the hole: its face is a ring around the hole of the top.
    let sq = square(300.0, 300.0, 400.0, 500.0);
    poly(&mut m, &sq);
    let ring = ring_within(&m, 300.0, 700.0);
    let before = m.clone();
    let e = m
        .apply(&Edit::PushPull {
            face: ring,
            distance: -500.0,
        })
        .unwrap_err();
    assert_eq!(m, before);
    assert!(e.to_string().contains("not fully inside"), "{e}");
    // A second hole elsewhere works: the bottom gets two holes.
    let sq = square(50.0, 50.0, 150.0, 500.0);
    poly(&mut m, &sq);
    let g = face_inside(&m, &sq);
    pull(&mut m, g, -500.0).unwrap();
    assert_closed(&m);
    assert_eq!(m.face(bottom_face(&m)).unwrap().holes.len(), 2);
}

#[test]
fn a_bottom_of_two_faces_takes_a_hole_inside_one_and_refuses_one_across_both() {
    let mut m = box_model();
    // A line across the bottom splits it in two faces.
    m.apply(&Edit::AddEdges {
        edges: vec![[[500.0, 0.0, 0.0], [500.0, 1000.0, 0.0]]],
    })
    .unwrap();
    let downs = m.faces().filter(|(_, f)| f.plane.normal.z < -0.9).count();
    assert_eq!(downs, 2);
    let sq = square(100.0, 100.0, 300.0, 500.0);
    poly(&mut m, &sq);
    let f = face_inside(&m, &sq);
    pull(&mut m, f, -500.0).unwrap();
    assert_closed(&m);
    let sq = square(400.0, 600.0, 200.0, 500.0);
    poly(&mut m, &sq);
    let g = face_inside(&m, &sq);
    refused_with(&mut m, g, -500.0, "lies across two faces");
}

#[test]
fn a_ring_pushed_through_leaves_a_column_with_an_island_at_the_bottom() {
    let mut m = box_model();
    let outer = square(200.0, 200.0, 600.0, 500.0);
    let inner = square(400.0, 400.0, 200.0, 500.0);
    poly(&mut m, &outer);
    poly(&mut m, &inner);
    let ring = ring_within(&m, 200.0, 800.0);
    pull(&mut m, ring, -500.0).unwrap();
    assert_closed(&m);
    // The island under the column looks down, like the bottom.
    let islands = m
        .faces()
        .filter(|(_, f)| {
            f.plane.normal.z < -0.9 && f.plane.offset.abs() < 1e-9 && f.holes.is_empty()
        })
        .count();
    assert_eq!(islands, 1);
    let v = 1000.0 * 1000.0 * 500.0 - (600.0 * 600.0 - 200.0 * 200.0) * 500.0;
    assert!((volume(&m) - v).abs() < 1e-3);
}

#[test]
fn a_round_hole_keeps_its_curves_and_smooth_walls() {
    let mut m = box_model();
    let points: Vec<P> = circle_points(24, 200.0)
        .iter()
        .map(|p| [p[0] + 500.0, p[1] + 500.0, 500.0])
        .collect();
    m.apply(&Edit::AddCurve {
        points: points.clone(),
        closed: true,
        faceted: false,
        analytic: None,
    })
    .unwrap();
    let disc = face_inside(&m, &points);
    pull(&mut m, disc, -500.0).unwrap();
    assert_closed(&m);
    assert_eq!(
        m.curves().count(),
        2,
        "the ring at the top and at the bottom"
    );
    let smooth = m
        .edges()
        .filter(|(id, _)| {
            m.edge_attrs(*id)
                .and_then(|a| a.get("smooth"))
                .is_some_and(|v| v == &serde_json::Value::Bool(true))
        })
        .count();
    assert_eq!(smooth, 24);
    let b = m.face(bottom_face(&m)).unwrap();
    assert_eq!(b.holes[0].len(), 24);
}

#[test]
fn the_walls_of_a_painted_hole_keep_the_paint_and_the_cap_gives_a_note() {
    let (mut m, f) = box_with_square(250.0, 250.0, 500.0);
    m.apply(&Edit::SetFaceMaterial {
        faces: vec![f],
        side: PaintSide::Front,
        material: Some("brick".into()),
    })
    .unwrap();
    let notes = m
        .apply_with_notes(&Edit::PushPull {
            face: f,
            distance: -500.0,
        })
        .unwrap();
    assert!(
        notes.iter().any(|n| n.contains("pushed through")),
        "{notes:?}"
    );
    let painted = m
        .faces()
        .filter(|(_, g)| g.attrs.get(ATTR_MATERIAL) == Some(&serde_json::json!("brick")))
        .count();
    assert_eq!(painted, 4, "the four walls");
    // An unpainted face gives no note.
    let (mut m, f) = box_with_square(250.0, 250.0, 500.0);
    let notes = m
        .apply_with_notes(&Edit::PushPull {
            face: f,
            distance: -500.0,
        })
        .unwrap();
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_hole_through_gives_the_same_ids_every_time_and_replays() {
    let (m0, f) = box_with_square(250.0, 250.0, 500.0);
    let edit = Edit::PushPull {
        face: f,
        distance: -500.0,
    };
    let mut a = m0.clone();
    let mut b = m0.clone();
    a.apply(&edit).unwrap();
    b.apply(&edit).unwrap();
    assert_eq!(a, b);
    let entry = a.log().last().unwrap().clone();
    let mut c = m0.clone();
    super::super::sync::replay_intent(&mut c, &entry.intent).unwrap();
    assert_eq!(
        (c.vertex_count(), c.edge_count(), c.face_count()),
        (a.vertex_count(), a.edge_count(), a.face_count())
    );
    assert!(c.check().is_closed());
}

#[test]
fn many_holes_in_one_slab_stay_closed() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 10_000.0, 0.0));
    let f = only_face(&m);
    pull(&mut m, f, 200.0).unwrap();
    for i in 0..6 {
        for j in 0..6 {
            let sq = square(
                500.0 + 1500.0 * i as f64,
                500.0 + 1500.0 * j as f64,
                800.0,
                200.0,
            );
            poly(&mut m, &sq);
            let g = face_inside(&m, &sq);
            m.apply(&Edit::PushPull {
                face: g,
                distance: -200.0,
            })
            .unwrap();
        }
    }
    assert_closed(&m);
    assert_eq!(m.face(bottom_face(&m)).unwrap().holes.len(), 36);
    let v = 1.0e8 * 200.0 - 36.0 * 640_000.0 * 200.0;
    assert!((volume(&m) - v).abs() < 1.0);
}

/// Release build, `--ignored --nocapture`: a hole through a slab whose top has thousands of
/// pockets.
#[test]
#[ignore = "timing"]
fn hole_through_timing_on_a_slab_with_many_pockets() {
    let mut m = slab_with(2500, 100_000.0, -200.0);
    let top = m
        .faces()
        .find(|(_, f)| f.holes.len() == 2500)
        .map(|(id, _)| id)
        .unwrap();
    let _ = top;
    // A square between the pockets, in the first gap.
    let sq = square(10.0, 10.0, 100.0, 500.0);
    poly(&mut m, &sq);
    let g = face_inside(&m, &sq);
    let t = std::time::Instant::now();
    m.apply(&Edit::PushPull {
        face: g,
        distance: -500.0,
    })
    .unwrap();
    let el = t.elapsed();
    println!("hole through: {:.0} ms", el.as_secs_f64() * 1e3);
    assert!(m.check().is_closed());
    assert!(el.as_secs_f64() < 5.0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Any rectangle well inside the top, pushed to the bottom, makes a closed solid with the
    /// volume of the box less the hole; the same edit gives the same model.
    #[test]
    fn any_rectangle_inside_the_top_cuts_a_closed_hole(
        x in 10.0f64..600.0,
        y in 10.0f64..600.0,
        w in 20.0f64..380.0,
        h in 20.0f64..380.0,
    ) {
        let mut m = box_model();
        let pts = vec![
            [x, y, 500.0],
            [x + w, y, 500.0],
            [x + w, y + h, 500.0],
            [x, y + h, 500.0],
        ];
        poly(&mut m, &pts);
        let f = face_inside(&m, &pts);
        let m0 = m.clone();
        pull(&mut m, f, -500.0).unwrap();
        prop_assert!(m.check().is_closed());
        let v = 1000.0 * 1000.0 * 500.0 - w * h * 500.0;
        prop_assert!((volume(&m) - v).abs() < 1e-3 * v.abs().max(1.0));
        let mut again = m0.clone();
        again.apply(&Edit::PushPull { face: f, distance: -500.0 }).unwrap();
        prop_assert_eq!(again, m);
    }
}
