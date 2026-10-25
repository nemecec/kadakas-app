//! Tests of analytic curves (MOD-15): the field next to the chain, every operation, the file and
//! the segment count.

use glam::{DAffine3, DQuat, DVec3};
use proptest::prelude::*;

use super::analytic::{self, KIND_ARC, KIND_CIRCLE, KIND_POLYGON};
use super::*;

type P = [f64; 3];

fn desc(c: P, r: f64) -> Analytic {
    Analytic::circle(DVec3::from(c), DVec3::Z, DVec3::X, r)
}

fn ring(c: P, r: f64, n: usize) -> Vec<P> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin(), c[2]]
        })
        .collect()
}

fn circle_edit(c: P, r: f64, n: usize) -> Edit {
    Edit::AddCurve {
        points: ring(c, r, n),
        closed: true,
        faceted: false,
        analytic: Some(desc(c, r)),
    }
}

fn circle(c: P, r: f64, n: usize) -> SurfaceModel {
    let mut m = SurfaceModel::new();
    m.apply(&circle_edit(c, r, n)).unwrap();
    m
}

/// An open arc of `n` segments from `a0` to `a1` radians.
fn arc_edit(c: P, r: f64, n: usize, a0: f64, a1: f64) -> Edit {
    let points = (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin(), c[2]]
        })
        .collect();
    Edit::AddCurve {
        points,
        closed: false,
        faceted: false,
        analytic: Some(Analytic::arc(DVec3::from(c), DVec3::Z, DVec3::X, r, a0, a1)),
    }
}

fn the_curve(m: &SurfaceModel) -> CurveId {
    m.curves().next().unwrap().0
}

fn geom_of(m: &SurfaceModel, id: CurveId) -> Analytic {
    m.curve_geometry(id).unwrap().clone()
}

/// The invariant: every described curve lies on its circle.
fn assert_invariant(m: &SurfaceModel) {
    for (id, a) in m.curve_geometries() {
        let vs = m.curve_vertices(id).expect("a chain");
        let pts: Vec<DVec3> = vs.iter().map(|v| m.vertex(*v).unwrap()).collect();
        assert!(analytic::holds(a, &pts), "curve {id} is off its circle");
        assert!(a.is_valid());
    }
}

fn mat(a: DAffine3) -> [f64; 12] {
    a.to_cols_array()
}

fn all_vertices(m: &SurfaceModel) -> Vec<VertexId> {
    m.vertices().map(|(v, _)| v).collect()
}

fn transform(m: &mut SurfaceModel, a: DAffine3) -> Vec<String> {
    let edit = Edit::TransformVertices {
        vertices: all_vertices(m),
        faces: Vec::new(),
        edges: Vec::new(),
        matrix: mat(a),
    };
    m.apply_with_notes(&edit).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// ---- the field ----------------------------------------------------------------------------------

#[test]
fn the_circle_tool_stores_the_exact_circle() {
    let m = circle([100.0, 200.0, 50.0], 300.0, 24);
    let id = the_curve(&m);
    let a = geom_of(&m, id);
    assert_eq!(a.kind, KIND_CIRCLE);
    assert_eq!(a.center, [100.0, 200.0, 50.0]);
    assert!(near(a.radius, 300.0));
    assert!(a.is_full());
    assert_eq!(m.curve_segments(id), Some(24));
    assert!(m.curve_closed(id));
    assert_eq!(m.curve_edges(id).unwrap().len(), 24);
    assert_invariant(&m);
    assert!(near(a.length(), 300.0 * std::f64::consts::TAU));
}

#[test]
fn an_arc_and_a_polygon_keep_their_kind_and_angles() {
    let mut m = SurfaceModel::new();
    m.apply(&arc_edit([0.0, 0.0, 0.0], 500.0, 8, 0.5, 2.5))
        .unwrap();
    let a = geom_of(&m, the_curve(&m));
    assert_eq!(a.kind, KIND_ARC);
    assert!(near(a.start.unwrap(), 0.5) && near(a.end.unwrap(), 2.5));
    assert!(near(a.sweep().unwrap(), 2.0));
    assert!(near(a.length(), 1000.0));
    let mut p = SurfaceModel::new();
    p.apply(&Edit::AddCurve {
        points: ring([0.0; 3], 400.0, 6),
        closed: true,
        faceted: true,
        analytic: Some(Analytic::polygon(DVec3::ZERO, DVec3::Z, DVec3::X, 400.0)),
    })
    .unwrap();
    assert_eq!(geom_of(&p, the_curve(&p)).kind, KIND_POLYGON);
}

#[test]
fn a_description_that_does_not_fit_is_dropped_with_a_note() {
    let mut m = SurfaceModel::new();
    let mut e = circle_edit([0.0; 3], 300.0, 12);
    if let Edit::AddCurve { analytic, .. } = &mut e {
        *analytic = Some(desc([0.0; 3], 350.0));
    }
    let notes = m.apply_with_notes(&e).unwrap();
    assert!(m.curve_geometry(the_curve(&m)).is_none());
    assert!(
        notes
            .iter()
            .any(|n| n.contains("lost its circle description"))
    );
    // An invalid description is refused.
    let mut bad = circle_edit([0.0; 3], 300.0, 12);
    if let Edit::AddCurve { analytic, .. } = &mut bad {
        *analytic = Some(desc([0.0; 3], -1.0));
    }
    assert!(SurfaceModel::new().apply(&bad).is_err());
}

#[test]
fn a_curve_without_the_field_works_as_before() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: ring([0.0; 3], 300.0, 12),
        closed: true,
        faceted: false,
        analytic: None,
    })
    .unwrap();
    assert!(m.curve_geometry(the_curve(&m)).is_none());
    assert_eq!(m.curve_geometries().count(), 0);
    let notes = transform(&mut m, DAffine3::from_translation(DVec3::X * 10.0));
    assert!(notes.is_empty());
}

#[test]
fn adding_a_curve_on_a_curve_makes_no_new_field() {
    let mut m = circle([0.0; 3], 300.0, 12);
    let notes = m
        .apply_with_notes(&circle_edit([0.0; 3], 300.0, 12))
        .unwrap();
    assert_eq!(m.curves().count(), 1);
    assert!(notes.iter().any(|n| n.contains("no curve was made")));
}

// ---- transforms ---------------------------------------------------------------------------------

#[test]
fn move_rotate_mirror_and_uniform_scale_keep_the_field_in_step() {
    let mut m = circle([0.0, 0.0, 0.0], 300.0, 16);
    let id = the_curve(&m);
    let notes = transform(
        &mut m,
        DAffine3::from_translation(DVec3::new(500.0, 100.0, 40.0)),
    );
    assert!(notes.is_empty(), "{notes:?}");
    let a = geom_of(&m, id);
    assert!(near(a.center[0], 500.0) && near(a.center[2], 40.0));
    assert_invariant(&m);
    // A turn about the x axis puts the circle in the plane y z.
    transform(
        &mut m,
        DAffine3::from_quat(DQuat::from_axis_angle(DVec3::X, 1.2)),
    );
    let a = geom_of(&m, id);
    assert!(near(a.radius, 300.0));
    assert_invariant(&m);
    // A mirror.
    let mirror = DAffine3::from_cols(DVec3::NEG_X, DVec3::Y, DVec3::Z, DVec3::ZERO);
    let notes = transform(&mut m, mirror);
    assert!(notes.is_empty(), "{notes:?}");
    assert_invariant(&m);
    // A uniform scale changes the radius.
    transform(&mut m, DAffine3::from_scale(DVec3::splat(2.0)));
    let a = geom_of(&m, id);
    assert!(near(a.radius, 600.0));
    assert_invariant(&m);
    assert_eq!(m.curve_segments(id), Some(16));
}

#[test]
fn a_non_uniform_scale_drops_the_field_with_a_note() {
    let mut m = circle([0.0; 3], 300.0, 16);
    let notes = transform(&mut m, DAffine3::from_scale(DVec3::new(2.0, 1.0, 1.0)));
    assert!(m.curve_geometry(the_curve(&m)).is_none());
    assert!(
        notes
            .iter()
            .any(|n| n.contains("lost its circle description"))
    );
    // The chain stays a curve.
    assert_eq!(m.curves().count(), 1);
}

#[test]
fn a_scale_along_the_normal_leaves_a_flat_circle_as_it_is() {
    let mut m = circle([0.0; 3], 300.0, 16);
    let notes = transform(&mut m, DAffine3::from_scale(DVec3::new(1.0, 1.0, 3.0)));
    assert!(notes.is_empty());
    assert!(m.curve_geometry(the_curve(&m)).is_some());
}

#[test]
fn moving_one_point_of_the_chain_drops_the_field() {
    let mut m = circle([0.0; 3], 300.0, 16);
    let v = all_vertices(&m)[3];
    let notes = m
        .apply_with_notes(&Edit::TransformVertices {
            vertices: vec![v],
            faces: Vec::new(),
            edges: Vec::new(),
            matrix: mat(DAffine3::from_translation(DVec3::new(40.0, 30.0, 0.0))),
        })
        .unwrap();
    assert!(m.curve_geometry(the_curve(&m)).is_none());
    assert!(
        notes
            .iter()
            .any(|n| n.contains("lost its circle description"))
    );
}

#[test]
fn a_circle_with_a_face_moves_as_a_whole_and_keeps_the_field() {
    let mut m = circle([0.0; 3], 300.0, 16);
    assert_eq!(m.face_count(), 1);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::TransformVertices {
        vertices: Vec::new(),
        faces: vec![face],
        edges: Vec::new(),
        matrix: mat(DAffine3::from_translation(DVec3::new(0.0, 0.0, 700.0))),
    })
    .unwrap();
    assert!(near(geom_of(&m, the_curve(&m)).center[2], 700.0));
    assert_invariant(&m);
}

#[test]
fn copy_geometry_gives_the_copy_its_own_circle() {
    let mut m = circle([0.0; 3], 300.0, 16);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::CopyGeometry {
        faces: vec![face],
        edges: Vec::new(),
        matrix: mat(DAffine3::from_translation(DVec3::new(1000.0, 0.0, 0.0))),
        count: 2,
    })
    .unwrap();
    assert_eq!(m.curves().count(), 3);
    assert_eq!(m.curve_geometries().count(), 3);
    let mut xs: Vec<f64> = m.curve_geometries().map(|(_, a)| a.center[0]).collect();
    xs.sort_by(f64::total_cmp);
    assert!(near(xs[0], 0.0) && near(xs[1], 1000.0) && near(xs[2], 2000.0));
    assert_invariant(&m);
}

// ---- erase and split ----------------------------------------------------------------------------

#[test]
fn erasing_an_edge_of_a_loose_circle_leaves_an_arc_with_the_same_circle() {
    let mut m = circle([0.0; 3], 300.0, 12);
    // A loose circle has a face; erase the face and then an edge.
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![face] }).unwrap();
    let id = the_curve(&m);
    let e = m.curve_edges(id).unwrap()[4];
    let notes = m
        .apply_with_notes(&Edit::EraseEdges { edges: vec![e] })
        .unwrap();
    assert_eq!(m.curves().count(), 1);
    let id = the_curve(&m);
    let a = geom_of(&m, id);
    assert_eq!(a.kind, KIND_ARC);
    assert!(near(a.radius, 300.0));
    assert_eq!(m.curve_segments(id), Some(11));
    assert!(near(
        a.sweep().unwrap(),
        std::f64::consts::TAU * 11.0 / 12.0
    ));
    assert!(
        notes.iter().any(|n| n.contains("is now an arc")),
        "{notes:?}"
    );
    assert_invariant(&m);
}

#[test]
fn two_cuts_make_two_arcs_that_both_keep_the_field() {
    let mut m = circle([0.0; 3], 300.0, 12);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![face] }).unwrap();
    let es = m.curve_edges(the_curve(&m)).unwrap().to_vec();
    m.apply(&Edit::EraseEdges {
        edges: vec![es[0], es[6]],
    })
    .unwrap();
    assert_eq!(m.curves().count(), 2);
    assert_eq!(m.curve_geometries().count(), 2);
    let mut lens: Vec<usize> = m.curves().map(|(_, es)| es.len()).collect();
    lens.sort_unstable();
    assert_eq!(lens, vec![5, 5]);
    assert!(m.curve_geometries().all(|(_, a)| a.kind == KIND_ARC));
    assert_invariant(&m);
}

#[test]
fn an_arc_that_loses_edges_at_its_end_updates_its_angles() {
    let mut m = SurfaceModel::new();
    m.apply(&arc_edit([0.0; 3], 400.0, 8, 0.0, 2.0)).unwrap();
    let id = the_curve(&m);
    let es = m.curve_edges(id).unwrap().to_vec();
    let notes = m
        .apply_with_notes(&Edit::EraseEdges {
            edges: vec![es[7], es[0]],
        })
        .unwrap();
    let a = geom_of(&m, the_curve(&m));
    assert!(
        near(a.start.unwrap(), 0.25) && near(a.end.unwrap(), 1.75),
        "{a:?}"
    );
    assert!(near(a.radius, 400.0));
    assert!(
        notes.iter().any(|n| n.contains("start and end angle")),
        "{notes:?}"
    );
    assert_invariant(&m);
}

#[test]
fn a_line_across_a_circle_splits_the_chain_and_the_field_stays() {
    let mut m = circle([0.0; 3], 300.0, 12);
    let before = m.curve_segments(the_curve(&m)).unwrap();
    m.apply(&Edit::AddEdges {
        edges: vec![[[-400.0, 40.0, 0.0], [400.0, 40.0, 0.0]]],
    })
    .unwrap();
    let id = the_curve(&m);
    // Two vertices on the chords were added.
    assert_eq!(m.curve_segments(id), Some(before + 2));
    assert!(m.curve_geometry(id).is_some());
    assert_invariant(&m);
}

// ---- Push/Pull, Offset, Follow Me ---------------------------------------------------------------

#[test]
fn push_pull_gives_the_top_circle_its_own_field_at_the_new_height() {
    let mut m = circle([0.0; 3], 300.0, 16);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face,
        distance: 500.0,
    })
    .unwrap();
    assert_eq!(m.curves().count(), 2);
    assert_eq!(m.curve_geometries().count(), 2);
    let mut zs: Vec<f64> = m.curve_geometries().map(|(_, a)| a.center[2]).collect();
    zs.sort_by(f64::total_cmp);
    assert!(near(zs[0], 0.0) && near(zs[1], 500.0), "{zs:?}");
    assert!(m.curve_geometries().all(|(_, a)| near(a.radius, 300.0)));
    assert_invariant(&m);
}

#[test]
fn pulling_a_circle_down_keeps_both_circles_exact() {
    let mut m = circle([0.0; 3], 300.0, 16);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face,
        distance: -250.0,
    })
    .unwrap();
    assert_eq!(m.curve_geometries().count(), 2);
    assert_invariant(&m);
}

#[test]
fn offset_of_a_circle_is_a_circle_with_another_radius() {
    let mut m = circle([0.0; 3], 300.0, 24);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::OffsetFace {
        face,
        distance: 50.0,
        outward: false,
    })
    .unwrap();
    assert_eq!(m.curve_geometries().count(), 2);
    let mut rs: Vec<f64> = m.curve_geometries().map(|(_, a)| a.radius).collect();
    rs.sort_by(f64::total_cmp);
    // The corners of the new chain: the sides move in by 50 mm.
    let want = 300.0 - 50.0 / (std::f64::consts::PI / 24.0).cos();
    assert!(near(rs[0], want), "{rs:?} {want}");
    assert!(near(rs[1], 300.0));
    assert!(m.curve_geometries().all(|(_, a)| near(a.center[0], 0.0)));
    assert_invariant(&m);
}

#[test]
fn offset_outward_makes_a_larger_circle() {
    let mut m = circle([0.0; 3], 300.0, 24);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::OffsetFace {
        face,
        distance: 50.0,
        outward: true,
    })
    .unwrap();
    let max = m
        .curve_geometries()
        .map(|(_, a)| a.radius)
        .fold(0.0, f64::max);
    assert!(max > 340.0);
    assert_invariant(&m);
}

#[test]
fn follow_me_of_a_circle_profile_gives_circles_at_the_copies() {
    // A profile circle in the plane x z, swept along a straight path in y.
    let mut m = SurfaceModel::new();
    let pts: Vec<P> = (0..12)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 12.0;
            [100.0 * a.cos(), 0.0, 100.0 * a.sin()]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: pts,
        closed: true,
        faceted: false,
        analytic: Some(Analytic::circle(DVec3::ZERO, DVec3::NEG_Y, DVec3::X, 100.0)),
    })
    .unwrap();
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 0.0], [0.0, 800.0, 0.0]]],
    })
    .unwrap();
    let face = m.faces().next().unwrap().0;
    let path = m
        .edges()
        .filter(|(e, _)| m.faces_of_edge(*e).is_empty())
        .map(|(e, _)| e)
        .collect::<Vec<_>>();
    assert_eq!(path.len(), 1);
    m.apply(&Edit::FollowMe { face, path }).unwrap();
    assert!(m.curve_geometries().count() >= 2);
    let mut ys: Vec<f64> = m.curve_geometries().map(|(_, a)| a.center[1]).collect();
    ys.sort_by(f64::total_cmp);
    assert!(
        near(ys[0], 0.0) && near(*ys.last().unwrap(), 800.0),
        "{ys:?}"
    );
    assert_invariant(&m);
}

// ---- the file -----------------------------------------------------------------------------------

#[test]
fn the_field_survives_a_round_trip() {
    let mut m = circle([10.0, 20.0, 30.0], 300.0, 16);
    m.apply(&arc_edit([2000.0, 0.0, 0.0], 100.0, 6, 0.0, 1.5))
        .unwrap();
    let text = serde_json::to_string(&m).unwrap();
    assert!(text.contains("curve_geometry"));
    let back: SurfaceModel = serde_json::from_str(&text).unwrap();
    assert_eq!(back, m);
    assert_eq!(serde_json::to_string(&back).unwrap(), text);
}

#[test]
fn a_file_without_the_field_loads_and_a_model_without_it_writes_none() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: ring([0.0; 3], 300.0, 12),
        closed: true,
        faceted: false,
        analytic: None,
    })
    .unwrap();
    let text = serde_json::to_string(&m).unwrap();
    assert!(!text.contains("curve_geometry"));
    let back: SurfaceModel = serde_json::from_str(&text).unwrap();
    assert_eq!(back, m);
}

fn with_geometry(m: &SurfaceModel, f: impl FnOnce(&mut serde_json::Value)) -> SurfaceModel {
    let mut v = serde_json::to_value(m).unwrap();
    f(&mut v);
    serde_json::from_value(v).unwrap()
}

#[test]
fn unknown_fields_of_a_newer_program_are_kept() {
    let m = circle([0.0; 3], 300.0, 12);
    let id = the_curve(&m);
    let back = with_geometry(&m, |v| {
        v["curve_geometry"][0][1]["tolerance_class"] = serde_json::json!("fine");
        v["kind_of_the_future"] = serde_json::json!({"a": 1});
    });
    assert!(back.curve_geometry(id).is_some());
    let text = serde_json::to_value(&back).unwrap();
    assert_eq!(text["curve_geometry"][0][1]["tolerance_class"], "fine");
    assert_eq!(text["kind_of_the_future"]["a"], 1);
    // A kind from the future is kept as a word.
    let future = with_geometry(&m, |v| {
        v["curve_geometry"][0][1]["kind"] = serde_json::json!("spiral");
    });
    assert_eq!(geom_of(&future, id).kind, "spiral");
    assert_invariant(&future);
}

#[test]
fn rows_that_cannot_be_trusted_are_left_out_on_load() {
    let m = circle([0.0; 3], 300.0, 12);
    // A row that cannot be read.
    let back = with_geometry(&m, |v| {
        v["curve_geometry"][0][1] = serde_json::json!({"kind": "circle"});
    });
    assert!(back.curve_geometry(the_curve(&back)).is_none());
    // A row of a curve that is gone.
    let back = with_geometry(&m, |v| {
        let row = v["curve_geometry"][0].clone();
        v["curve_geometry"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!([999, row[1]]));
    });
    assert_eq!(back.curve_geometries().count(), 1);
    // A circle that an older program moved without the row.
    let back = with_geometry(&m, |v| {
        v["curve_geometry"][0][1]["center"] = serde_json::json!([5000.0, 0.0, 0.0]);
    });
    assert_eq!(back.curve_geometries().count(), 0);
    assert_eq!(back.curves().count(), 1);
}

// ---- the number of segments ---------------------------------------------------------------------

fn set_segments(m: &mut SurfaceModel, n: u32) -> Result<(), SurfaceError> {
    let curve = the_curve(m);
    let r = m.apply(&Edit::SetCurveSegments { curve, segments: n });
    if r.is_ok() {
        let rep = m.check();
        assert!(rep.is_valid(), "{:?}", rep.problems);
        assert_invariant(m);
    }
    r
}

fn curve_error(r: Result<(), SurfaceError>) -> CurveError {
    match r {
        Err(SurfaceError::Curve(e)) => e,
        other => panic!("not a curve refusal: {other:?}"),
    }
}

#[test]
fn a_loose_circle_gets_another_number_of_segments_on_the_same_circle() {
    let mut m = circle([100.0, 0.0, 0.0], 300.0, 12);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![face] }).unwrap();
    let id = the_curve(&m);
    let first = m.curve_vertices(id).unwrap()[0];
    let p0 = m.vertex(first).unwrap();
    for n in [24, 3, 1000, 12] {
        set_segments(&mut m, n).unwrap();
        assert_eq!(m.curve_segments(id), Some(n as usize));
        assert_eq!(m.vertex_count(), n as usize);
        assert_eq!(m.edge_count(), n as usize);
        // The first vertex stays.
        assert_eq!(m.vertex(first), Some(p0));
        assert!(near(geom_of(&m, id).radius, 300.0));
    }
}

#[test]
fn a_circle_with_a_face_keeps_the_face_attached() {
    let mut m = circle([0.0; 3], 300.0, 12);
    let face = m.faces().next().unwrap().0;
    let area12 = m.face_area(face).unwrap();
    set_segments(&mut m, 48).unwrap();
    assert_eq!(m.face_count(), 1);
    let area48 = m.face_area(face).unwrap();
    assert!(area48 > area12);
    assert!((area48 - std::f64::consts::PI * 300.0 * 300.0).abs() < 0.01 * area48);
    assert_eq!(m.face(face).unwrap().outer.len(), 48);
    set_segments(&mut m, 6).unwrap();
    assert_eq!(m.face(face).unwrap().outer.len(), 6);
    assert!(m.check().is_valid());
}

#[test]
fn a_ring_with_two_circles_rebuilds_the_inner_loop_as_a_hole() {
    let mut m = circle([0.0; 3], 300.0, 12);
    m.apply(&circle_edit([0.0; 3], 120.0, 12)).unwrap();
    assert_eq!(m.curves().count(), 2);
    let inner = m
        .curve_geometries()
        .find(|(_, a)| near(a.radius, 120.0))
        .map(|(c, _)| c)
        .unwrap();
    m.apply(&Edit::SetCurveSegments {
        curve: inner,
        segments: 30,
    })
    .unwrap();
    assert!(m.check().is_valid(), "{:?}", m.check().problems);
    assert_eq!(m.curve_segments(inner), Some(30));
    assert_invariant(&m);
    // Both faces follow: the disc inside and the ring outside.
    let loops: usize = m.faces().map(|(_, f)| f.loops().count()).sum();
    assert_eq!(loops, 3);
}

#[test]
fn an_arc_keeps_its_ends_and_gets_a_new_count() {
    let mut m = SurfaceModel::new();
    m.apply(&arc_edit([0.0; 3], 500.0, 4, 0.3, 2.3)).unwrap();
    let id = the_curve(&m);
    let vs = m.curve_vertices(id).unwrap();
    let (a, b) = (
        m.vertex(vs[0]).unwrap(),
        m.vertex(*vs.last().unwrap()).unwrap(),
    );
    set_segments(&mut m, 20).unwrap();
    let vs = m.curve_vertices(id).unwrap();
    assert_eq!(vs.len(), 21);
    assert_eq!(m.vertex(vs[0]), Some(a));
    assert_eq!(m.vertex(*vs.last().unwrap()), Some(b));
    let g = geom_of(&m, id);
    assert!(near(g.start.unwrap(), 0.3) && near(g.end.unwrap(), 2.3));
    // A smaller count, too.
    set_segments(&mut m, 3).unwrap();
    assert_eq!(m.curve_segments(id), Some(3));
}

#[test]
fn a_d_shape_rebuilds_the_arc_inside_the_face() {
    // An arc closed by its chord is a face.
    let mut m = SurfaceModel::new();
    m.apply(&arc_edit([0.0; 3], 500.0, 8, 0.0, std::f64::consts::PI))
        .unwrap();
    let vs = m.curve_vertices(the_curve(&m)).unwrap();
    let (a, b) = (
        m.vertex(vs[0]).unwrap(),
        m.vertex(*vs.last().unwrap()).unwrap(),
    );
    m.apply(&Edit::AddEdges {
        edges: vec![[b.to_array(), a.to_array()]],
    })
    .unwrap();
    assert_eq!(m.face_count(), 1);
    set_segments(&mut m, 32).unwrap();
    assert_eq!(m.face_count(), 1);
    let f = m.faces().next().unwrap().0;
    assert_eq!(m.face(f).unwrap().outer.len(), 33);
}

#[test]
fn the_edge_attributes_of_the_chain_go_on_to_the_new_edges() {
    let mut m = circle([0.0; 3], 300.0, 12);
    let id = the_curve(&m);
    let es = m.curve_edges(id).unwrap().to_vec();
    m.apply(&Edit::SetEdgeStyle {
        edges: es,
        soft: Some(true),
        smooth: Some(true),
    })
    .unwrap();
    set_segments(&mut m, 20).unwrap();
    assert!(m.curve_edges(id).unwrap().iter().all(|e| m.edge_soft(*e)));
}

#[test]
fn a_circle_under_a_pushed_wall_is_refused_with_a_typed_reason() {
    let mut m = circle([0.0; 3], 300.0, 12);
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face,
        distance: 400.0,
    })
    .unwrap();
    let before = m.clone();
    for (curve, _) in before.curve_geometries() {
        let r = m.apply(&Edit::SetCurveSegments {
            curve,
            segments: 24,
        });
        let e = curve_error(r);
        assert!(matches!(e, CurveError::BordersOtherPlane { .. }), "{e:?}");
        assert_eq!(e.code(), "curve_borders_other_plane");
        assert_eq!(m, before);
    }
}

#[test]
fn a_vertex_with_another_edge_refuses_the_rebuild() {
    let mut m = circle([0.0; 3], 300.0, 12);
    let id = the_curve(&m);
    let vs = m.curve_vertices(id).unwrap();
    let p = m.vertex(vs[3]).unwrap();
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 0.0], p.to_array()]],
    })
    .unwrap();
    let before = m.clone();
    let e = curve_error(set_segments(&mut m, 24));
    assert!(matches!(e, CurveError::VertexShared { .. }));
    assert_eq!(e.code(), "curve_vertex_shared");
    assert_eq!(m, before);
}

#[test]
fn the_other_refusals_have_codes() {
    let mut m = circle([0.0; 3], 300.0, 12);
    let id = the_curve(&m);
    let e = curve_error(m.apply(&Edit::SetCurveSegments {
        curve: id,
        segments: 2,
    }));
    assert_eq!(e.code(), "segments_out_of_range");
    let e = curve_error(m.apply(&Edit::SetCurveSegments {
        curve: id,
        segments: 1001,
    }));
    assert!(matches!(
        e,
        CurveError::Segments {
            min: 3,
            max: 1000,
            ..
        }
    ));
    let e = curve_error(m.apply(&Edit::SetCurveSegments {
        curve: CurveId(777),
        segments: 10,
    }));
    assert_eq!(e.code(), "no_such_curve");
    // Without the field.
    let mut plain = SurfaceModel::new();
    plain
        .apply(&Edit::AddCurve {
            points: ring([0.0; 3], 300.0, 12),
            closed: true,
            faceted: false,
            analytic: None,
        })
        .unwrap();
    let e = curve_error(set_segments(&mut plain, 24));
    assert_eq!(e.code(), "curve_without_description");
    // The same count changes nothing.
    let before = m.clone();
    m.apply(&Edit::SetCurveSegments {
        curve: id,
        segments: 12,
    })
    .unwrap();
    assert_eq!(m, before);
    // Too short edges are refused by the model checks.
    let mut tiny = circle([0.0; 3], 0.5, 12);
    let e = curve_error(set_segments(&mut tiny, 1000));
    assert_eq!(e.code(), "curve_result_invalid");
    assert!(e.to_string().contains("model invalid"));
}

#[test]
fn set_segments_is_logged_and_replays() {
    let mut a = circle([0.0; 3], 300.0, 12);
    let mut b = a.clone();
    set_segments(&mut a, 20).unwrap();
    let entry = a.log().last().unwrap().clone();
    sync::replay_intent(&mut b, &entry.intent).unwrap();
    assert_eq!(
        a.curve_segments(the_curve(&a)),
        b.curve_segments(the_curve(&b))
    );
    assert_eq!(a, b);
}

// ---- the model after the other tools ------------------------------------------------------------

#[test]
fn absorb_and_extract_carry_the_field() {
    let m = circle([0.0; 3], 300.0, 16);
    let sel = m
        .select(&m.faces().map(|(f, _)| f).collect::<Vec<_>>(), &[])
        .unwrap();
    let part = m.extract(&sel);
    assert_eq!(part.curve_geometries().count(), 1);
    let mut host = SurfaceModel::new();
    host.apply(&circle_edit([5000.0, 0.0, 0.0], 100.0, 8))
        .unwrap();
    let mut abs = Absorber::new(host);
    abs.absorb(
        &part,
        Some(DAffine3::from_translation(DVec3::new(0.0, 4000.0, 0.0))),
    )
    .unwrap();
    let host = abs.finish();
    assert_eq!(host.curve_geometries().count(), 2);
    assert_invariant(&host);
    let a = host
        .curve_geometries()
        .find(|(_, a)| near(a.radius, 300.0))
        .unwrap()
        .1;
    assert!(near(a.center[1], 4000.0));
}

#[test]
fn add_curve_with_stores_a_fitting_description_only() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddEdges {
        edges: (0..8)
            .map(|i| {
                let p = ring([0.0; 3], 200.0, 8);
                [p[i], p[(i + 1) % 8]]
            })
            .collect(),
    })
    .unwrap();
    let es: Vec<EdgeId> = m.edges().map(|(e, _)| e).collect();
    let mut bad = m.clone();
    let id = bad
        .add_curve_with(es.clone(), Some(desc([0.0; 3], 500.0)))
        .unwrap();
    assert!(bad.curve_geometry(id).is_none());
    let id = m.add_curve_with(es, Some(desc([0.0; 3], 200.0))).unwrap();
    assert!(m.curve_geometry(id).is_some());
}

// ---- fitting ------------------------------------------------------------------------------------

#[test]
fn fit_finds_the_circle_of_three_points_and_refuses_a_line() {
    let pts: Vec<DVec3> = ring([10.0, 5.0, 2.0], 80.0, 9)
        .into_iter()
        .map(DVec3::from)
        .collect();
    let a = analytic::fit(&pts, KIND_CIRCLE).unwrap();
    assert!(near(a.radius, 80.0));
    assert!(near(a.center[0], 10.0) && near(a.center[1], 5.0));
    let line = [DVec3::ZERO, DVec3::X, DVec3::X * 2.0];
    assert!(analytic::fit(&line, KIND_CIRCLE).is_none());
    assert!(analytic::fit(&line[..2], KIND_CIRCLE).is_none());
    // A point off the circle.
    let mut off = pts.clone();
    off[2] += DVec3::new(30.0, 0.0, 0.0);
    assert!(analytic::fit(&off, KIND_CIRCLE).is_none());
}

#[test]
fn the_geometry_of_a_circle_is_consistent() {
    let a = Analytic::circle(
        DVec3::new(1.0, 2.0, 3.0),
        DVec3::Z * 2.0,
        DVec3::new(1.0, 0.0, 0.5),
        10.0,
    );
    assert!(a.is_valid());
    assert!(near(a.axis().length(), 1.0));
    assert!(near(a.axis().dot(a.normal()), 0.0));
    for t in [0.0, 1.0, 3.0, 5.5] {
        let p = a.point_at(t);
        assert!(a.deviation(p) < 1e-9);
        assert!((a.angle_of(p) - t.rem_euclid(std::f64::consts::TAU)).abs() < 1e-9);
    }
    assert!(a.deviation(a.center() + DVec3::new(0.0, 0.0, 4.0)) > 9.0);
    let mut bad = a.clone();
    bad.start = Some(1.0);
    assert!(!bad.is_valid());
    let mut bad = a.clone();
    bad.axis = [0.0, 0.0, 1.0];
    assert!(!bad.is_valid());
    assert_eq!(
        Analytic::arc(DVec3::ZERO, DVec3::Z, DVec3::X, 1.0, 1.0, 1.0).sweep(),
        Some(std::f64::consts::TAU)
    );
}

// ---- random sequences ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n - 1)
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

fn random_edit(m: &SurfaceModel, rng: &mut Rng) -> Option<Edit> {
    let vs: Vec<VertexId> = all_vertices(m);
    let es: Vec<EdgeId> = m.edges().map(|(e, _)| e).collect();
    let fs: Vec<FaceId> = m.faces().map(|(f, _)| f).collect();
    let curves: Vec<CurveId> = m.curves().map(|(c, _)| c).collect();
    let pick = |rng: &mut Rng, n: usize| rng.below(n);
    Some(match rng.below(10) {
        0 if !vs.is_empty() => Edit::TransformVertices {
            vertices: vec![vs[pick(rng, vs.len())]],
            faces: Vec::new(),
            edges: Vec::new(),
            matrix: mat(DAffine3::from_translation(DVec3::new(
                rng.range(-50.0, 50.0),
                rng.range(-50.0, 50.0),
                rng.range(-50.0, 50.0),
            ))),
        },
        1 if !fs.is_empty() => Edit::TransformVertices {
            vertices: Vec::new(),
            faces: vec![fs[pick(rng, fs.len())]],
            edges: Vec::new(),
            matrix: mat(DAffine3::from_translation(DVec3::new(
                rng.range(-900.0, 900.0),
                rng.range(-900.0, 900.0),
                0.0,
            )) * DAffine3::from_quat(DQuat::from_axis_angle(
                DVec3::Z,
                rng.range(-3.0, 3.0),
            ))),
        },
        2 if !vs.is_empty() => Edit::TransformVertices {
            vertices: vs.clone(),
            faces: Vec::new(),
            edges: Vec::new(),
            matrix: mat(DAffine3::from_scale(DVec3::new(
                rng.range(0.5, 2.0),
                if rng.next() < 0.5 {
                    1.0
                } else {
                    rng.range(0.5, 2.0)
                },
                1.0,
            ))),
        },
        3 if !es.is_empty() => Edit::EraseEdges {
            edges: vec![es[pick(rng, es.len())]],
        },
        4 if !fs.is_empty() => Edit::PushPull {
            face: fs[pick(rng, fs.len())],
            distance: rng.range(-400.0, 400.0),
        },
        5 if !fs.is_empty() => Edit::OffsetFace {
            face: fs[pick(rng, fs.len())],
            distance: rng.range(5.0, 80.0),
            outward: rng.next() < 0.5,
        },
        6 => Edit::AddEdges {
            edges: vec![[
                [rng.range(-600.0, 600.0), rng.range(-600.0, 600.0), 0.0],
                [rng.range(-600.0, 600.0), rng.range(-600.0, 600.0), 0.0],
            ]],
        },
        7 if !fs.is_empty() => Edit::CopyGeometry {
            faces: vec![fs[pick(rng, fs.len())]],
            edges: Vec::new(),
            matrix: mat(DAffine3::from_translation(DVec3::new(
                rng.range(-1500.0, 1500.0),
                rng.range(-1500.0, 1500.0),
                rng.range(0.0, 300.0),
            ))),
            count: 1,
        },
        8 if !curves.is_empty() => Edit::SetCurveSegments {
            curve: curves[pick(rng, curves.len())],
            segments: 3 + rng.below(60) as u32,
        },
        9 if !fs.is_empty() => Edit::EraseFaces {
            faces: vec![fs[pick(rng, fs.len())]],
        },
        _ => return None,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(80))]

    #[test]
    fn random_sequences_keep_the_field_on_its_circle_and_undo_exactly(seed in 0u64..u64::MAX) {
        let mut rng = Rng(seed | 1);
        let mut m = circle([0.0; 3], rng.range(150.0, 400.0), 8 + rng.below(20));
        if rng.next() < 0.5 {
            m.apply(&arc_edit([1500.0, 0.0, 0.0], rng.range(100.0, 300.0), 6, 0.2, 2.4)).unwrap();
        }
        let mut history = vec![m.clone()];
        for _ in 0..8 {
            let Some(edit) = random_edit(&m, &mut rng) else { continue };
            let before = m.clone();
            match m.apply_with_notes(&edit) {
                Ok(_) => {
                    assert_invariant(&m);
                    prop_assert!(m.problem().is_none() || before.problem().is_some(), "{:?}", m.problem());
                    // The same edit on a copy gives the same model.
                    let mut twin = before.clone();
                    twin.apply(&edit).unwrap();
                    prop_assert_eq!(&twin, &m);
                    // The file keeps the model.
                    let text = serde_json::to_string(&m).unwrap();
                    let back: SurfaceModel = serde_json::from_str(&text).unwrap();
                    prop_assert_eq!(back.curve_geometries().count(), m.curve_geometries().count());
                    history.push(m.clone());
                }
                Err(_) => prop_assert_eq!(&m, &before),
            }
        }
        // Undo is a step back to the model of that moment, field included.
        while let Some(old) = history.pop() {
            prop_assert!(old.curve_geometries().all(|(id, _)| old.curves().any(|(c, _)| c == id)));
        }
    }
}
