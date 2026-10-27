//! Tests of the snap of near-touches in the solid tools (MOD-19).
//!
//! The model of most tests is the one that the solid tools refused on a CI runner (see
//! `tests_solid`): the box `[0, 1000]^3` and a turned, tilted box whose corner lies 0.0058 mm off
//! the plane `y = 1000` of the first box, inside its face.

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::solid::snap;
use super::tests_solid::{Box3, K, P, assert_sound, close, crossing, draw, total, turned_model};
use super::*;

const OPS: [SolidOp; 5] = SolidOp::ALL;
const OFF: f64 = 0.005;

/// The refused model of `tests_solid`: the model and the faces of the two solids.
fn refused() -> (SurfaceModel, Vec<FaceId>, Vec<FaceId>) {
    let (m, _, fa, fb) = turned_model(
        1.539_623_148_363_857_6,
        0.394_573_062_789_783_6,
        200.0,
        796.195_805_697_005_2,
        200.0,
    )
    .unwrap();
    (m, fa, fb)
}

fn solid(
    m: &mut SurfaceModel,
    o: SolidOp,
    a: &[FaceId],
    b: &[FaceId],
    snap_near: bool,
) -> Result<Vec<String>, SurfaceError> {
    m.apply_with_notes(&Edit::Solid {
        op: o,
        first: a.to_vec(),
        second: b.to_vec(),
        snap_near,
    })
}

fn set(f: &[FaceId]) -> BTreeSet<FaceId> {
    f.iter().copied().collect()
}

#[test]
fn the_refusal_without_the_flag_names_the_snap_and_changes_nothing() {
    let (m, fa, fb) = refused();
    for o in OPS {
        let mut w = m.clone();
        let e = solid(&mut w, o, &fa, &fb, false).unwrap_err();
        assert_eq!(w, m, "{o}: nothing changes");
        let SurfaceError::Solid(SolidError::NearTouch { reason, moves }) = &e else {
            panic!("{o}: {e:?}");
        };
        // The text of the refusal stays, and the snap is added to it.
        assert!(
            reason.contains("lies 0.0058 mm from the plane of face"),
            "{reason}"
        );
        assert!(reason.contains("too close to cut"), "{reason}");
        assert!(e.to_string().starts_with(reason.as_str()), "{e}");
        assert!(e.to_string().contains("snap_near"), "{e}");
        assert_eq!(moves.len(), 1, "{o}");
        let mv = &moves[0];
        assert!((mv.distance - 0.005_793_698_856_905_394).abs() < 1e-12);
        assert_eq!(mv.faces.len(), 1);
        assert!((mv.from[1] - 1_000.005_793_698_856_9).abs() < 1e-9);
        assert!((mv.to[1] - 1000.0).abs() < 1e-9);
    }
}

#[test]
fn the_snap_makes_every_operation_work_and_lists_the_moved_vertex() {
    let (m, fa, fb) = refused();
    let r = m.solid_report();
    let (va, vb) = (
        r.solids().next().unwrap().volume,
        r.solids().nth(1).unwrap().volume,
    );
    let mut volumes = Vec::new();
    for o in OPS {
        let mut w = m.clone();
        let notes = solid(&mut w, o, &fa, &fb, true).unwrap_or_else(|e| panic!("{o}: {e}"));
        assert_sound(&w);
        assert!(w.solid_report().is_closed(), "{o}");
        assert_eq!(notes.len(), 1, "{o}: {notes:?}");
        assert!(notes[0].starts_with("snapped vertex "), "{}", notes[0]);
        assert!(notes[0].contains("1000.0058"), "{}", notes[0]);
        assert!(notes[0].contains("moved 0.0058 mm"), "{}", notes[0]);
        let out = m.solid_run(o, &fa, &fb, true).unwrap();
        assert_eq!(out.snapped.len(), 1);
        assert_eq!(out.notes, notes);
        volumes.push((o, total(&w)));
    }
    // The volumes are those of the exact model (the snap moved a vertex by 0.0058 mm).
    let get = |o: SolidOp| volumes.iter().find(|(x, _)| *x == o).unwrap().1;
    let inter = get(SolidOp::Intersect);
    assert!(close(get(SolidOp::Union) + inter, va + vb), "{volumes:?}");
    assert!(close(get(SolidOp::Subtract) + inter, va), "{volumes:?}");
    assert!(
        (get(SolidOp::Split) - (va + vb - inter)).abs() < 1e-4 * (va + vb),
        "{volumes:?}"
    );
}

#[test]
fn the_snapped_vertex_lies_on_the_plane_and_the_rest_stays() {
    let (m, fa, fb) = refused();
    let out = m.solid_run(SolidOp::Union, &fa, &fb, true).unwrap();
    let mv = &out.snapped[0];
    let mut w = m.clone();
    solid(&mut w, SolidOp::Union, &fa, &fb, true).unwrap();
    // The vertex keeps its id if it is in the result, and then lies on the plane.
    if let Some(p) = w.vertex(mv.vertex) {
        assert!((p.y - 1000.0).abs() < 1e-9, "{p}");
    }
    // Only that vertex moved: every other vertex of the model is where it was.
    for (v, p) in m.vertices() {
        if v == mv.vertex {
            continue;
        }
        if let Some(q) = w.vertex(v) {
            assert!(p.distance(q) < 1e-9, "{v}");
        }
    }
}

#[test]
fn a_model_without_a_near_touch_gives_the_same_result_with_the_flag() {
    let a: Box3 = ([0.0; 3], [K; 3]);
    let b: Box3 = ([500.0; 3], [1500.0; 3]);
    let (m, fa, fb) = crossing(a, b);
    for o in OPS {
        let (mut x, mut y) = (m.clone(), m.clone());
        let nx = solid(&mut x, o, &fa, &fb, false);
        let ny = solid(&mut y, o, &fa, &fb, true);
        assert_eq!(nx, ny, "{o}");
        assert_eq!(x, y, "{o}: the same ids and positions");
        let out = m.solid_run(o, &fa, &fb, true);
        assert!(out.is_ok_and(|o| o.snapped.is_empty()), "{o}");
    }
}

#[test]
fn a_refusal_for_another_reason_stays_with_the_flag() {
    let a: Box3 = ([0.0; 3], [K; 3]);
    let b: Box3 = ([500.0; 3], [1500.0; 3]);
    let (m, fa, fb) = crossing(a, b);
    for flag in [false, true] {
        let mut w = m.clone();
        let e = solid(&mut w, SolidOp::Subtract, &fa[..3], &fb, flag).unwrap_err();
        assert!(
            matches!(e, SurfaceError::Solid(SolidError::Open { .. })),
            "{e:?}"
        );
        assert_eq!(w, m);
    }
}

#[test]
fn the_find_lists_the_vertex_with_its_signed_distance() {
    let (m, fa, fb) = refused();
    let found = snap::find(&m, &set(&fa), &set(&fb));
    assert_eq!(found.len(), 1, "{found:?}");
    let t = &found[0];
    assert!(
        (t.distance - 0.005_793_698_856_905_394).abs() < 1e-12,
        "{t:?}"
    );
    // The vertex belongs to the second solid (the turned box), the face to the first.
    assert_eq!(t.operand.to_string(), "second");
    assert!(fa.contains(&t.face));
    // Seen the other way round, the vertex belongs to the first operand.
    let swapped = snap::find(&m, &set(&fb), &set(&fa));
    assert_eq!(swapped.len(), 1);
    assert_eq!(swapped[0].operand.to_string(), "first");
    assert_eq!(swapped[0].vertex, t.vertex);
}

#[test]
fn nothing_is_found_when_the_vertex_is_on_the_plane_or_far_off() {
    // On the plane: the cut works, there is nothing to snap.
    let a: Box3 = ([0.0; 3], [K; 3]);
    let (m, fa, fb) = crossing(a, ([500.0; 3], [1500.0; 3]));
    assert!(snap::find(&m, &set(&fa), &set(&fb)).is_empty());
    // A vertex 0.02 mm off (more than the length tolerance) is not a near-touch.
    let (m, fa, fb) = crossing(
        a,
        ([250.0, 250.0, 3.0 * K], [750.0, 750.0, 3.0 * K + 500.0]),
    );
    assert!(snap::find(&m, &set(&fa), &set(&fb)).is_empty());
    let (m, fa, fb) = crossing(a, ([250.0, 250.0, K - 0.02], [750.0, 750.0, K + 500.0]));
    assert!(snap::find(&m, &set(&fa), &set(&fb)).is_empty());
}

#[test]
fn a_vertex_whose_foot_is_outside_the_face_is_not_a_near_touch() {
    let a: Box3 = ([0.0; 3], [K; 3]);
    // Off the plane z = K by 0.005 mm, but at x = 2000, far from the face.
    let b: Box3 = ([1500.0, 250.0, K - 0.005], [2500.0, 750.0, K + 500.0]);
    let (m, fa, fb) = crossing(a, b);
    assert!(snap::find(&m, &set(&fa), &set(&fb)).is_empty());
}

/// A post that sinks 0.005 mm into the top of the box: the snap puts it on the face, and then the
/// cut meets a face that lies inside a coplanar face without crossing its border. That is refused
/// at exact contact too, so the operation is still refused, with the reason, after the snap.
#[test]
fn a_snap_that_does_not_help_the_cut_is_a_refusal_of_its_own() {
    let a: Box3 = ([0.0; 3], [K; 3]);
    let b: Box3 = ([250.0, 250.0, K - OFF], [750.0, 750.0, K + 500.0]);
    let (m, fa, fb) = crossing(a, b);
    // Without the flag the plain refusal stays: a snap does not resolve it.
    let mut w = m.clone();
    let e = solid(&mut w, SolidOp::Union, &fa, &fb, false).unwrap_err();
    assert!(
        matches!(&e, SurfaceError::Solid(SolidError::Cut(t)) if t.contains("too close to cut")),
        "{e:?}"
    );
    assert_eq!(w, m);
    // With the flag the snap is refused with its own id.
    let e = solid(&mut w, SolidOp::Union, &fa, &fb, true).unwrap_err();
    let SurfaceError::Solid(SolidError::SnapRefused(r)) = &e else {
        panic!("{e:?}");
    };
    assert_eq!(r.code(), "snap_still_refused");
    assert!(
        r.to_string().contains("still refused after the snap"),
        "{r}"
    );
    assert_eq!(w, m);
}

/// A thin plate of 0.012 mm in the box, poking 0.005 mm out of its top: the snap would put the
/// top and the bottom of the plate on one plane.
#[test]
fn a_snap_that_makes_a_face_degenerate_is_refused() {
    let a: Box3 = ([0.0; 3], [K; 3]);
    let b: Box3 = ([250.0, 250.0, K - 0.007], [750.0, 750.0, K + 0.005]);
    let (m, fa, fb) = crossing(a, b);
    let mut sub = m.clone();
    let e = snap::snap(&mut sub, &set(&fa), &set(&fb), &BTreeSet::new()).unwrap_err();
    let snap::SnapRefusal::Degenerate { reason, .. } = &e else {
        panic!("{e:?}");
    };
    assert!(
        reason.contains("shorter than the length tolerance"),
        "{reason}"
    );
    assert_eq!(e.code(), "snap_degenerate_face");
    // Through the edit: the refusal is typed, with the flag; without it the plain refusal stands.
    let mut w = m.clone();
    let err = solid(&mut w, SolidOp::Union, &fa, &fb, true).unwrap_err();
    assert!(
        matches!(
            &err,
            SurfaceError::Solid(SolidError::SnapRefused(
                snap::SnapRefusal::Degenerate { .. }
            ))
        ),
        "{err:?}"
    );
    assert_eq!(w, m);
    let err = solid(&mut w, SolidOp::Union, &fa, &fb, false).unwrap_err();
    assert!(
        matches!(&err, SurfaceError::Solid(SolidError::Cut(_))),
        "{err:?}"
    );
}

/// A vertex 0.006 mm from two parallel planes of a plate of 0.012 mm: it cannot be on both.
#[test]
fn a_vertex_between_two_planes_that_it_cannot_be_on_is_refused() {
    let a: Box3 = ([0.0, 0.0, 0.0], [K, K, 0.012]);
    let b: Box3 = ([250.0, 250.0, 0.006], [750.0, 750.0, 500.0]);
    let (m, fa, fb) = crossing(a, b);
    let mut sub = m.clone();
    let e = snap::snap(&mut sub, &set(&fa), &set(&fb), &BTreeSet::new()).unwrap_err();
    assert!(matches!(e, snap::SnapRefusal::NewNearTouch { .. }), "{e:?}");
    assert_eq!(e.code(), "snap_new_near_touch");
}

/// A vertex 0.009 mm from the planes of two faces at a right angle (near their shared edge): it
/// cannot go onto both along one normal, and the move onto one leaves it near the other.
#[test]
fn a_corner_that_is_near_two_planes_is_refused() {
    let a: Box3 = ([0.0; 3], [K; 3]);
    let b: Box3 = ([K - 0.009, 300.0, K + 0.009], [K + 500.0, 700.0, K + 600.0]);
    let (m, fa, fb) = crossing(a, b);
    let mut sub = m.clone();
    let e = snap::snap(&mut sub, &set(&fa), &set(&fb), &BTreeSet::new()).unwrap_err();
    assert!(matches!(e, snap::SnapRefusal::NewNearTouch { .. }), "{e:?}");
    assert!(e.to_string().contains("a new near-touch"), "{e}");
}

/// The two solids of a model whose faces may look inward: turned over, with the faces of each.
fn solids_of(mut m: SurfaceModel) -> (SurfaceModel, Vec<FaceId>, Vec<FaceId>) {
    let inward: Vec<FaceId> = m
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted)
        .flat_map(|s| s.faces.clone())
        .collect();
    m.apply(&Edit::ReverseFaces { faces: inward }).unwrap();
    let r = m.solid_report();
    let solids: Vec<_> = r.solids().collect();
    assert_eq!(solids.len(), 2, "{r:?}");
    let (x, y) = (solids[0].faces.clone(), solids[1].faces.clone());
    (m, x, y)
}

/// Faces in one plane count once: the top of the box is split in two parts, and the bottom of the
/// post, 0.005 mm under it, is near both (its corners are 0.0088 mm from the split). The corners
/// would land within the length tolerance of the split line, which snaps and welds them in the
/// model: the move is refused, with the reason.
#[test]
fn a_vertex_that_would_land_on_other_geometry_is_refused() {
    let mut m = SurfaceModel::new();
    draw(&mut m, ([0.0; 3], [K; 3]));
    m.apply(&Edit::AddEdges {
        edges: vec![[[500.0, 0.0, K], [500.0, K, K]]],
    })
    .unwrap();
    draw(
        &mut m,
        ([500.0088, 250.0, K - 0.005], [900.0, 750.0, K + 500.0]),
    );
    let (m, fa, fb) = solids_of(m);
    let found = snap::find(&m, &set(&fa), &set(&fb));
    let near_split = found
        .iter()
        .filter(|t| m.vertex(t.vertex).is_some_and(|p| p.x < 501.0))
        .count();
    assert_eq!(near_split, 4, "both parts, for both corners: {found:?}");
    let mut sub = m.clone();
    let e = snap::snap(&mut sub, &set(&fa), &set(&fb), &BTreeSet::new()).unwrap_err();
    let snap::SnapRefusal::MoveRefused { reason } = &e else {
        panic!("{e:?}");
    };
    assert!(
        reason.contains("would not stay where the snap puts it"),
        "{reason}"
    );
    assert_eq!(e.code(), "snap_move_refused");
}

#[test]
fn a_vertex_that_other_geometry_uses_is_not_moved() {
    let (m, fa, fb) = refused();
    let t = snap::find(&m, &set(&fa), &set(&fb))[0].vertex;
    let mut sub = m.clone();
    let pinned: BTreeSet<VertexId> = [t].into();
    let e = snap::snap(&mut sub, &set(&fa), &set(&fb), &pinned).unwrap_err();
    assert_eq!(e, snap::SnapRefusal::Shared { vertex: t });
    assert_eq!(e.code(), "snap_vertex_shared");
    assert!(e.to_string().contains("outside the operation"));
}

#[test]
fn the_snap_leaves_both_solids_closed_and_gives_their_faces() {
    let (m, fa, fb) = refused();
    let mut sub = m.clone();
    let s = snap::snap(&mut sub, &set(&fa), &set(&fb), &BTreeSet::new()).unwrap();
    for faces in [&s.a, &s.b] {
        let faces: Vec<FaceId> = faces.iter().copied().collect();
        let shell = sub.solid_state(&faces).unwrap();
        assert_eq!(shell.state, ShellState::Solid);
        assert!(shell.volume > 0.0);
    }
    // No near-touch is left.
    assert!(snap::find(&sub, &s.a, &s.b).is_empty());
    // Nothing to snap: the answer is empty and the model stays.
    let before = sub.clone();
    let again = snap::snap(&mut sub, &s.a, &s.b, &BTreeSet::new()).unwrap();
    assert!(again.moves.is_empty());
    assert_eq!(again.a, s.a);
    assert_eq!(sub, before);
}

/// The preview with the flag runs on a copy.
#[test]
fn the_preview_with_the_flag_does_not_change_the_model() {
    let (m, fa, fb) = refused();
    let copy = m.clone();
    assert!(m.solid_preview(SolidOp::Union, &fa, &fb).is_err());
    let out = m.solid_run(SolidOp::Union, &fa, &fb, true).unwrap();
    assert_eq!(out.volumes.len(), 1);
    assert_eq!(m, copy);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Random solids with an injected offset of 0.005 mm: the highest vertex of a turned and
    /// tilted box is put 0.005 mm above or below the plane `y = 1000` of the box `[0, 1000]^3`,
    /// inside its face. Where the operation is refused for the near-touch, the refusal names the
    /// snap and changes nothing, and with the flag the operation works, gives a valid model with
    /// the volumes of the inputs (to 1 part in 10,000) and lists the moved vertices. Where it is
    /// not refused, the flag changes nothing.
    #[test]
    fn an_injected_offset_snaps_and_the_rest_is_as_before(
        angle in 0.05f64..3.0,
        tilt in 0.0f64..0.5,
        dx in 400.0f64..600.0,
        dz in 400.0f64..600.0,
        up in prop::bool::ANY,
    ) {
        let (_, corners, _, _) = turned_model(angle, tilt, dx, 500.0, dz)?;
        let top = corners.iter().map(|c| c[1]).fold(f64::MIN, f64::max);
        let offset = if up { 0.005 } else { -0.005 };
        let (m, corners, fa, fb) = turned_model(angle, tilt, dx, 500.0 + K + offset - top, dz)?;
        let mut ys: Vec<(f64, P)> = corners.iter().map(|c| (c[1], *c)).collect();
        ys.sort_by(|a, b| b.0.total_cmp(&a.0));
        let apex = ys[0].1;
        prop_assume!(apex[0] > 20.0 && apex[0] < K - 20.0 && apex[2] > 20.0 && apex[2] < K - 20.0);
        prop_assume!(ys[1].0 < K - 5.0);
        let found = snap::find(&m, &set(&fa), &set(&fb));
        prop_assert_eq!(found.len(), 1, "{:?}", found);
        let r = m.solid_report();
        let (va, vb) = (r.solids().next().unwrap().volume, r.solids().nth(1).unwrap().volume);
        for o in OPS {
            let (mut plain, mut with) = (m.clone(), m.clone());
            let a = solid(&mut plain, o, &fa, &fb, false);
            let b = solid(&mut with, o, &fa, &fb, true);
            match a {
                Ok(notes) => {
                    // Not refused: the flag changes nothing.
                    prop_assert_eq!(&b, &Ok(notes));
                    prop_assert_eq!(&plain, &with);
                }
                Err(SurfaceError::Solid(SolidError::NearTouch { moves, .. })) => {
                    prop_assert_eq!(&plain, &m);
                    // A snap resolves it: with the flag the operation works.
                    let notes = b.map_err(|e| TestCaseError::fail(format!("{o}: {e}")))?;
                    prop_assert_eq!(notes.len(), moves.len());
                    prop_assert!(moves.iter().all(|mv| mv.distance <= 0.01 && mv.distance > 0.0025));
                    prop_assert!(with.check().is_valid(), "{o}: {:?}", with.check().problems);
                    prop_assert!(with.problem().is_none());
                    let t = total(&with);
                    prop_assert!(t > 0.0 && t <= (va + vb) * (1.0 + 1e-4), "{o}: {t} {va} {vb}");
                    if o == SolidOp::Union {
                        prop_assert!(t >= va.max(vb) * (1.0 - 1e-4), "{t} {va} {vb}");
                    }
                }
                Err(e) => {
                    // Any other refusal stays, with or without the flag, and changes nothing.
                    prop_assert_eq!(&plain, &m);
                    prop_assert_eq!(&with, &m);
                    prop_assert!(b.is_err(), "{o}: {e:?}");
                }
            }
        }
    }
}
