//! Tests of the solid tools (MOD-09): the closed-solid check and the boolean operations.

use glam::DVec3;
use proptest::prelude::*;

use super::*;

type P = [f64; 3];
type Box3 = (P, P);

// ---- building ------------------------------------------------------------------------------

fn corners_edges(c: &[P; 8]) -> Vec<[P; 2]> {
    let mut out = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                out.push([c[i], c[i | bit]]);
            }
        }
    }
    out
}

fn box_corners(lo: P, hi: P) -> [P; 8] {
    let c = |i: usize| -> P {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    std::array::from_fn(c)
}

fn draw_corners(m: &mut SurfaceModel, c: &[P; 8]) {
    m.apply(&Edit::AddEdges {
        edges: corners_edges(c),
    })
    .unwrap();
}

fn draw(m: &mut SurfaceModel, b: Box3) {
    draw_corners(m, &box_corners(b.0, b.1));
}

fn op(m: &mut SurfaceModel, o: SolidOp, a: &[FaceId], b: &[FaceId]) -> Result<(), SurfaceError> {
    m.apply(&Edit::Solid {
        op: o,
        first: a.to_vec(),
        second: b.to_vec(),
    })
}

/// The volumes of the solids of the report, sorted.
fn volumes(m: &SurfaceModel) -> Vec<f64> {
    let mut v: Vec<f64> = m.solid_report().solids().map(|s| s.volume).collect();
    v.sort_by(f64::total_cmp);
    v
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

fn vol(b: Box3) -> f64 {
    (0..3).map(|i| b.1[i] - b.0[i]).product()
}

/// The common box of two boxes, if it has a volume.
fn common(a: Box3, b: Box3) -> Option<Box3> {
    let lo: P = std::array::from_fn(|i| a.0[i].max(b.0[i]));
    let hi: P = std::array::from_fn(|i| a.1[i].min(b.1[i]));
    (0..3).all(|i| hi[i] > lo[i]).then_some((lo, hi))
}

/// The faces of `m` that lie on the surface of the box.
fn on_box(m: &SurfaceModel, b: Box3) -> Vec<FaceId> {
    let (lo, hi) = (DVec3::from(b.0), DVec3::from(b.1));
    let eps = 1e-6;
    m.faces()
        .filter(|(_, f)| {
            let n = f.plane.normal;
            let Some(i) = (0..3).find(|&i| n[i].abs() > 0.99999) else {
                return false;
            };
            let d = f.plane.offset * n[i].signum();
            let on_side = (d - lo[i]).abs() < eps || (d - hi[i]).abs() < eps;
            m.loop_points(&f.outer)
                .iter()
                .all(|p| (0..3).all(|k| p[k] > lo[k] - eps && p[k] < hi[k] + eps))
                && on_side
        })
        .map(|(id, _)| id)
        .collect()
}

/// Turns the faces on the surface of the box so that they look out of it.
fn orient(m: &mut SurfaceModel, b: Box3) {
    let wrong: Vec<FaceId> = on_box(m, b)
        .into_iter()
        .filter(|id| {
            let n = m.face(*id).unwrap().plane;
            let i = (0..3).find(|&i| n.normal[i].abs() > 0.99999).unwrap();
            let d = n.offset * n.normal[i].signum();
            let plus = (d - b.1[i]).abs() < 1e-6;
            (n.normal[i] > 0.0) != plus
        })
        .collect();
    m.apply(&Edit::ReverseFaces { faces: wrong }).unwrap();
}

/// Two boxes drawn in one model (planes they share are shared by their faces), with the faces
/// of each. The faces look out of their boxes.
fn pair(a: Box3, b: Box3) -> (SurfaceModel, Vec<FaceId>, Vec<FaceId>) {
    let mut m = SurfaceModel::new();
    draw(&mut m, a);
    draw(&mut m, b);
    m.apply(&Edit::IntersectFaces {
        scope: IntersectScope::Model,
        faces: vec![],
        edges: vec![],
    })
    .unwrap();
    orient(&mut m, a);
    orient(&mut m, b);
    let (fa, fb) = (on_box(&m, a), on_box(&m, b));
    (m, fa, fb)
}

/// The same without Intersect Faces first (the faces cross).
fn crossing(a: Box3, b: Box3) -> (SurfaceModel, Vec<FaceId>, Vec<FaceId>) {
    let mut m = SurfaceModel::new();
    draw(&mut m, a);
    draw(&mut m, b);
    let inward: Vec<FaceId> = m
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted)
        .flat_map(|s| s.faces.clone())
        .collect();
    m.apply(&Edit::ReverseFaces { faces: inward }).unwrap();
    let r = m.solid_report();
    let solids: Vec<&Shell> = r.solids().collect();
    assert_eq!(solids.len(), 2, "{r:?}");
    let (x, y) = (solids[0].faces.clone(), solids[1].faces.clone());
    (m, x, y)
}

fn assert_sound(m: &SurfaceModel) {
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert!(m.problem().is_none(), "{:?}", m.problem());
}

// ---- the cases ------------------------------------------------------------------------------

const K: f64 = 1000.0;

/// The cases: both boxes in one model.
fn cases() -> Vec<(&'static str, Box3, Box3)> {
    vec![
        ("corner", ([0.0; 3], [K; 3]), ([500.0; 3], [1500.0; 3])),
        (
            "rooms sharing planes",
            ([0.0; 3], [K; 3]),
            ([500.0, 0.0, 0.0], [1500.0, K, K]),
        ),
        (
            "plus sign",
            ([0.0, K, K], [3.0 * K, 2.0 * K, 2.0 * K]),
            ([K, 0.0, K], [2.0 * K, 3.0 * K, 2.0 * K]),
        ),
        ("nested", ([0.0; 3], [3.0 * K; 3]), ([K; 3], [2.0 * K; 3])),
        (
            "nested sharing a corner",
            ([0.0; 3], [3.0 * K; 3]),
            ([0.0; 3], [K; 3]),
        ),
        (
            "nested sharing a wall",
            ([0.0; 3], [3.0 * K; 3]),
            ([0.0, K, K], [K, 2.0 * K, 2.0 * K]),
        ),
        ("disjoint", ([0.0; 3], [K; 3]), ([3.0 * K; 3], [4.0 * K; 3])),
        (
            "slab and post",
            ([0.0, 0.0, 0.0], [3.0 * K, 3.0 * K, 200.0]),
            ([K, K, -500.0], [2.0 * K, 2.0 * K, 500.0]),
        ),
        ("the same box", ([0.0; 3], [K; 3]), ([0.0; 3], [K; 3])),
    ]
}

fn expected(a: Box3, b: Box3) -> (f64, f64, f64, f64) {
    let (va, vb) = (vol(a), vol(b));
    let vi = common(a, b).map_or(0.0, vol);
    (va + vb - vi, vi, va - vi, vb - vi)
}

/// Builds a case in the way that gives the faces of the boxes: boxes that share a plane need
/// Intersect Faces first, boxes that only cross are cut by the operation itself.
fn build(a: Box3, b: Box3) -> (SurfaceModel, Vec<FaceId>, Vec<FaceId>) {
    pair(a, b)
}

#[test]
fn every_case_gives_the_volumes_of_the_boxes() {
    for (name, a, b) in cases() {
        let (union, inter, a_less_b, b_less_a) = expected(a, b);
        let (m, fa, fb) = build(a, b);
        let sa = m.solid_state(&fa).unwrap();
        let sb = m.solid_state(&fb).unwrap();
        assert!(
            sa.is_solid() && sb.is_solid(),
            "{name}: {:?} {:?}",
            sa.state,
            sb.state
        );
        let sum = |m: &SurfaceModel| volumes(m).iter().sum::<f64>();
        // Union.
        let mut u = m.clone();
        op(&mut u, SolidOp::Union, &fa, &fb).unwrap_or_else(|e| panic!("{name} union: {e}"));
        assert_sound(&u);
        assert!(
            close(sum(&u), union),
            "{name} union {:?} {union}",
            volumes(&u)
        );
        // Intersect.
        let mut i = m.clone();
        let r = op(&mut i, SolidOp::Intersect, &fa, &fb);
        if inter > 0.0 {
            r.unwrap_or_else(|e| panic!("{name} intersect: {e}"));
            assert_sound(&i);
            assert!(close(sum(&i), inter), "{name} intersect {:?}", volumes(&i));
        } else {
            assert_eq!(
                r,
                Err(SurfaceError::Solid(SolidError::NoResult(
                    SolidOp::Intersect
                ))),
                "{name}"
            );
            assert_eq!(i, m, "{name}: a refusal changes nothing");
        }
        // Subtract, both ways round.
        for (x, y, want) in [(&fa, &fb, a_less_b), (&fb, &fa, b_less_a)] {
            let mut s = m.clone();
            let r = op(&mut s, SolidOp::Subtract, x, y);
            if want > 0.0 {
                r.unwrap_or_else(|e| panic!("{name} subtract: {e}"));
                assert_sound(&s);
                assert!(
                    close(sum(&s), want),
                    "{name} subtract {:?} {want}",
                    volumes(&s)
                );
            } else {
                assert_eq!(
                    r,
                    Err(SurfaceError::Solid(SolidError::NoResult(SolidOp::Subtract))),
                    "{name}"
                );
                assert_eq!(s, m);
            }
        }
        // The previews give the volumes of what the edits make.
        assert_eq!(
            m.solid_preview(SolidOp::Trim, &fa, &fb).map(|v| v.len()),
            if a_less_b > 0.0 {
                Ok(2)
            } else {
                Err(SurfaceError::Solid(SolidError::NoResult(SolidOp::Trim)))
            },
            "{name}"
        );
        if inter > 0.0 {
            let v = m
                .solid_preview(SolidOp::Split, &fa, &fb)
                .unwrap_or_else(|e| panic!("{name} split: {e}"));
            assert!(
                close(v[0], a_less_b) && close(v[1], inter) && close(v[2], b_less_a),
                "{name} {v:?}"
            );
            // And Split is sound.
            let mut s = m.clone();
            op(&mut s, SolidOp::Split, &fa, &fb).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_sound(&s);
        }
    }
}

#[test]
fn trim_keeps_the_second_solid() {
    let (a, b) = (([0.0; 3], [K; 3]), ([500.0; 3], [1500.0; 3]));
    let (mut m, fa, fb) = crossing(a, b);
    op(&mut m, SolidOp::Trim, &fa, &fb).unwrap();
    assert_sound(&m);
    let v = m.solid_preview(SolidOp::Trim, &fa, &fb);
    assert!(v.is_err(), "the faces of the old solids are gone");
    // The second box is still a solid with its volume, and the first is cut.
    let vols = volumes(&m);
    assert!(vols.iter().any(|v| close(*v, vol(b))), "{vols:?}");
    assert!(
        vols.iter().any(|v| close(*v, vol(a) - 125_000_000.0)),
        "{vols:?}"
    );
}

#[test]
fn split_gives_three_solids_in_place() {
    let (a, b) = (([0.0; 3], [K; 3]), ([500.0; 3], [1500.0; 3]));
    let (mut m, fa, fb) = crossing(a, b);
    op(&mut m, SolidOp::Split, &fa, &fb).unwrap();
    assert_sound(&m);
    let vols = volumes(&m);
    assert_eq!(vols.len(), 3, "{vols:?}");
    assert!(close(vols[0], 125_000_000.0), "{vols:?}");
    assert!(
        close(vols[1], 875_000_000.0) && close(vols[2], 875_000_000.0),
        "{vols:?}"
    );
}

#[test]
fn nested_subtract_makes_a_cavity_that_belongs_to_its_solid() {
    let (a, b) = (([0.0; 3], [3.0 * K; 3]), ([K; 3], [2.0 * K; 3]));
    let (mut m, fa, fb) = crossing(a, b);
    op(&mut m, SolidOp::Subtract, &fa, &fb).unwrap();
    let r = m.solid_report();
    let solids: Vec<&Shell> = r.solids().collect();
    assert_eq!(solids.len(), 1, "{r:?}");
    assert!(close(solids[0].volume, 26.0 * 1e9));
    assert_eq!(solids[0].faces.len(), 12);
    assert_eq!(m.solid_of(solids[0].faces[0]).unwrap(), solids[0].faces);
}

// ---- the closed-solid check ---------------------------------------------------------------

/// One box with outward faces.
fn one_box() -> (SurfaceModel, Vec<FaceId>) {
    let b = ([0.0; 3], [K; 3]);
    let mut m = SurfaceModel::new();
    draw(&mut m, b);
    orient(&mut m, b);
    let f = on_box(&m, b);
    (m, f)
}

#[test]
fn a_box_is_one_closed_solid() {
    let (m, faces) = one_box();
    let r = m.solid_report();
    assert!(r.is_closed(), "{r:?}");
    assert_eq!(r.shells.len(), 1);
    assert_eq!(r.shells[0].faces, faces);
    assert!(close(r.shells[0].volume, 1e9));
    assert!(close(r.shells[0].area, 6e6));
    assert!(r.open_edges().is_empty() && r.wire_edges.is_empty());
    assert_eq!(m.solid_of(faces[3]).unwrap(), faces);
    assert_eq!(m.solid_of(FaceId(999)), None);
    assert!(close(m.solid_volume(&faces).unwrap(), 1e9));
    assert_eq!(m.solid_volume(&[]), None);
    assert_eq!(m.solid_state(&[FaceId(999)]), None);
    // The report is plain data that an agent can read as JSON.
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["shells"][0]["state"], "solid");
}

#[test]
fn an_erased_face_makes_four_open_border_edges() {
    let (mut m, faces) = one_box();
    m.apply(&Edit::EraseFaces {
        faces: vec![faces[5]],
    })
    .unwrap();
    let r = m.solid_report();
    assert!(!r.is_closed());
    assert_eq!(r.shells.len(), 1);
    assert_eq!(r.shells[0].state, ShellState::Open);
    let open = r.open_edges();
    assert_eq!(open.len(), 4, "{open:?}");
    assert!(open.iter().all(|o| o.kind == OpenKind::Border));
    // The edges are those of the face that went.
    for o in &open {
        assert_eq!(m.faces_of_edge(o.edge).len(), 1);
    }
    // The operation names an open edge.
    let (_, other) = {
        let b = ([3.0 * K; 3], [4.0 * K; 3]);
        let mut mm = m.clone();
        draw(&mut mm, b);
        orient(&mut mm, b);
        let f = on_box(&mm, b);
        (mm, f)
    };
    let mut mm = m.clone();
    draw(&mut mm, ([3.0 * K; 3], [4.0 * K; 3]));
    orient(&mut mm, ([3.0 * K; 3], [4.0 * K; 3]));
    let before = mm.clone();
    let err = op(&mut mm, SolidOp::Union, &faces[..5], &other).unwrap_err();
    assert!(
        matches!(
            err,
            SurfaceError::Solid(SolidError::Open {
                which: Operand::First,
                count: 4,
                ..
            })
        ),
        "{err}"
    );
    assert!(err.to_string().contains("is not a closed solid"), "{err}");
    assert_eq!(mm, before);
}

#[test]
fn a_fin_a_reversed_face_and_a_loose_edge_are_reported() {
    // A fin: a third face on an edge of the box.
    let (mut m, faces) = one_box();
    m.apply(&Edit::AddEdges {
        edges: vec![
            [[0.0, 0.0, 0.0], [0.0, -500.0, 0.0]],
            [[0.0, -500.0, 0.0], [0.0, -500.0, 1000.0]],
            [[0.0, -500.0, 1000.0], [0.0, 0.0, 1000.0]],
        ],
    })
    .unwrap();
    assert_eq!(m.face_count(), 7);
    let r = m.solid_report();
    // The box itself is closed; the fin is an open shell of its own with border edges.
    let solids: Vec<&Shell> = r.solids().collect();
    assert_eq!(solids.len(), 1, "{r:?}");
    assert_eq!(solids[0].faces, faces);
    assert!(r.shells.iter().any(|s| s.state == ShellState::Open));
    assert!(!r.is_closed());
    // A reversed face.
    let (mut m, faces) = one_box();
    m.apply(&Edit::ReverseFaces {
        faces: vec![faces[2]],
    })
    .unwrap();
    let s = m.solid_state(&faces).unwrap();
    assert_eq!(s.state, ShellState::Open);
    assert_eq!(s.open_edges.len(), 4);
    assert!(s.open_edges.iter().all(|o| o.kind == OpenKind::Reversed));
    // A box turned inside out looks inward.
    let (mut m, faces) = one_box();
    m.apply(&Edit::ReverseFaces {
        faces: faces.clone(),
    })
    .unwrap();
    let s = m.solid_state(&faces).unwrap();
    assert_eq!(s.state, ShellState::Inverted);
    assert!(close(s.volume, -1e9));
    // A loose edge.
    let (mut m, _) = one_box();
    m.apply(&Edit::AddEdges {
        edges: vec![[[5.0 * K, 0.0, 0.0], [6.0 * K, 0.0, 0.0]]],
    })
    .unwrap();
    let r = m.solid_report();
    assert_eq!(r.wire_edges.len(), 1);
    assert!(!r.is_closed());
    assert!(r.shells[0].is_solid());
}

#[test]
fn two_faces_back_to_back_close_no_volume() {
    // A square and its twin share all four edges and enclose nothing.
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddEdges {
        edges: vec![
            [[0.0, 0.0, 0.0], [K, 0.0, 0.0]],
            [[K, 0.0, 0.0], [K, K, 0.0]],
            [[K, K, 0.0], [0.0, K, 0.0]],
            [[0.0, K, 0.0], [0.0, 0.0, 0.0]],
        ],
    })
    .unwrap();
    let f = m.faces().next().unwrap().0;
    let g = {
        let face = m.face(f).unwrap().clone();
        let mut twin = face.clone();
        twin.plane = face.plane.flipped();
        twin.outer = face.outer.iter().rev().copied().collect();
        let mut vertices = std::collections::BTreeMap::new();
        for (id, p) in m.vertices() {
            vertices.insert(id, p);
        }
        let edges = m.edges().collect();
        let mut faces = std::collections::BTreeMap::new();
        faces.insert(f, face);
        faces.insert(FaceId(50), twin);
        SurfaceModel::from_parts(vertices, edges, faces, 60)
    };
    let s = g.solid_state(&[f, FaceId(50)]).unwrap();
    assert_eq!(s.state, ShellState::Flat, "{s:?}");
    let mut g2 = g.clone();
    let err = op(&mut g2, SolidOp::Union, &[f, FaceId(50)], &[f]).unwrap_err();
    assert!(
        matches!(err, SurfaceError::Solid(SolidError::Flat(Operand::First))),
        "{err}"
    );
}

#[test]
fn two_crossing_solids_taken_as_one_pass_through_themselves() {
    let (a, b) = (([0.0; 3], [K; 3]), ([500.0; 3], [1500.0; 3]));
    let (mut m, fa, fb) = crossing(a, b);
    let both: Vec<FaceId> = fa.iter().chain(fb.iter()).copied().collect();
    let s = m.solid_state(&both).unwrap();
    assert!(!s.crossings.is_empty());
    assert_eq!(s.state, ShellState::SelfCrossing);
    let before = m.clone();
    let err = op(&mut m, SolidOp::Union, &both, &fa[..1]).unwrap_err();
    assert!(
        matches!(
            err,
            SurfaceError::Solid(SolidError::SelfCrossing {
                which: Operand::First,
                ..
            })
        ),
        "{err}"
    );
    assert!(err.to_string().contains("passes through itself"));
    assert_eq!(m, before);
    // The crossing of two solids is counted by the report, not held against either.
    let r = before.solid_report();
    assert!(r.crossings > 0);
    assert_eq!(r.solids().count(), 2);
}

// ---- refusals -----------------------------------------------------------------------------

#[test]
fn bad_inputs_are_refused_and_change_nothing() {
    let (a, b) = (([0.0; 3], [K; 3]), ([500.0; 3], [1500.0; 3]));
    let (m, fa, fb) = crossing(a, b);
    let refuse = |first: &[FaceId], second: &[FaceId]| {
        let mut x = m.clone();
        let err = op(&mut x, SolidOp::Union, first, second).unwrap_err();
        assert_eq!(x, m, "{err}");
        err
    };
    assert_eq!(
        refuse(&[], &fb),
        SurfaceError::Solid(SolidError::Empty(Operand::First))
    );
    assert_eq!(
        refuse(&fa, &[]),
        SurfaceError::Solid(SolidError::Empty(Operand::Second))
    );
    assert_eq!(
        refuse(&fa, &[FaceId(9999)]),
        SurfaceError::NoFace(FaceId(9999))
    );
    // Three faces are not a solid.
    let e = refuse(&fa[..3], &fb);
    assert!(
        matches!(e, SurfaceError::Solid(SolidError::Open { .. })),
        "{e}"
    );
    let e = refuse(&fa, &fb[..2]);
    assert!(
        matches!(
            e,
            SurfaceError::Solid(SolidError::Open {
                which: Operand::Second,
                ..
            })
        ),
        "{e}"
    );
    // A solid that looks inward.
    let mut inward = m.clone();
    inward
        .apply(&Edit::ReverseFaces { faces: fb.clone() })
        .unwrap();
    let mut x = inward.clone();
    let e = op(&mut x, SolidOp::Union, &fa, &fb).unwrap_err();
    assert_eq!(
        e,
        SurfaceError::Solid(SolidError::Inverted(Operand::Second))
    );
    assert!(e.to_string().contains("looks inward"));
    assert_eq!(x, inward);
}

#[test]
fn a_solid_stuck_to_other_geometry_is_refused() {
    let (a, b) = (([0.0; 3], [K; 3]), ([500.0; 3], [1500.0; 3]));
    let (mut m, fa, fb) = crossing(a, b);
    // A loose edge from a corner of the first box.
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 0.0], [-K, 0.0, 0.0]]],
    })
    .unwrap();
    let before = m.clone();
    let e = op(&mut m, SolidOp::Union, &fa, &fb).unwrap_err();
    assert_eq!(e, SurfaceError::Solid(SolidError::Attached(Operand::First)));
    assert!(e.to_string().contains("is stuck to other geometry"));
    assert_eq!(m, before);
    // A third solid that shares the edge of the second.
    let (mut m, fa, fb) = crossing(a, b);
    m.apply(&Edit::AddEdges {
        edges: vec![[[1500.0, 1500.0, 1500.0], [1500.0, 1500.0, 2500.0]]],
    })
    .unwrap();
    let e = op(&mut m, SolidOp::Union, &fa, &fb).unwrap_err();
    assert_eq!(
        e,
        SurfaceError::Solid(SolidError::Attached(Operand::Second))
    );
}

#[test]
fn boxes_that_touch_are_refused_or_sound() {
    // Sharing a face, an edge or a corner makes one complex in the model. The tools either do
    // what is asked or refuse with a reason; they never leave a broken model.
    let touching: [(&str, Box3, Box3); 3] = [
        ("face", ([0.0; 3], [K; 3]), ([0.0, 0.0, K], [K, K, 2.0 * K])),
        (
            "edge",
            ([0.0; 3], [K; 3]),
            ([K, K, 0.0], [2.0 * K, 2.0 * K, K]),
        ),
        ("corner", ([0.0; 3], [K; 3]), ([K; 3], [2.0 * K; 3])),
    ];
    for (name, a, b) in touching {
        let (m, fa, fb) = pair(a, b);
        for o in SolidOp::ALL {
            let mut x = m.clone();
            match op(&mut x, o, &fa, &fb) {
                Ok(()) => assert_sound(&x),
                Err(e) => {
                    assert!(matches!(e, SurfaceError::Solid(_)), "{name} {o}: {e}");
                    assert_eq!(x, m, "{name} {o}");
                }
            }
        }
    }
}

#[test]
fn a_crossing_too_close_to_an_edge_is_refused_not_guessed() {
    // The second box starts 0.004 mm inside the first one: its edges are closer than the length
    // tolerance (0.01 mm) to the faces of the first one.
    let (a, b) = (
        ([0.0; 3], [K; 3]),
        ([0.004, 500.0, 500.0], [1500.0, 1500.0, 1500.0]),
    );
    let mut m = SurfaceModel::new();
    draw(&mut m, a);
    if m.apply(&Edit::AddEdges {
        edges: cuboid_edges(b),
    })
    .is_err()
    {
        return;
    }
    let inward: Vec<FaceId> = m
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted)
        .flat_map(|s| s.faces.clone())
        .collect();
    m.apply(&Edit::ReverseFaces { faces: inward }).unwrap();
    let r = m.solid_report();
    let solids: Vec<Vec<FaceId>> = r.solids().map(|s| s.faces.clone()).collect();
    if solids.len() != 2 {
        return;
    }
    let mut x = m.clone();
    match op(&mut x, SolidOp::Union, &solids[0], &solids[1]) {
        Ok(()) => assert_sound(&x),
        Err(_) => assert_eq!(x, m),
    }
}

fn cuboid_edges(b: Box3) -> Vec<[P; 2]> {
    corners_edges(&box_corners(b.0, b.1))
}

#[test]
fn an_operation_is_deterministic_and_leaves_the_solids_in_place() {
    let (a, b) = (([0.0; 3], [K; 3]), ([500.0; 3], [1500.0; 3]));
    let (m, fa, fb) = crossing(a, b);
    let mut x = m.clone();
    let mut y = m.clone();
    op(&mut x, SolidOp::Subtract, &fa, &fb).unwrap();
    op(&mut y, SolidOp::Subtract, &fa, &fb).unwrap();
    assert_eq!(x, y);
    // One edit: one revision more, and the log holds it.
    assert_eq!(x.rev(), m.rev() + 1);
    // The result can be walked from any of its faces.
    let f = x.faces().next().unwrap().0;
    let solid = x.solid_of(f).unwrap();
    assert!(x.solid_state(&solid).unwrap().is_solid());
    // The solid tools cannot be replayed on another model, and the log says so.
    assert!(matches!(
        sync::replay_intent(&mut m.clone(), &x.log().last().unwrap().intent),
        Err(SurfaceError::Unsolved(_))
    ));
}

#[test]
fn a_solid_with_soft_edges_keeps_them() {
    // A box with one edge made soft: the edge stays soft in the union.
    let (a, b) = (([0.0; 3], [K; 3]), ([2.0 * K, 0.0, 0.0], [3.0 * K, K, K]));
    let (mut m, fa, fb) = crossing(a, b);
    let e = m
        .edges()
        .find(|(_, e)| {
            let (p, q) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            p.distance(DVec3::ZERO) < 1e-9 && q.distance(DVec3::new(K, 0.0, 0.0)) < 1e-9
        })
        .map(|(id, _)| id)
        .unwrap();
    m.apply(&Edit::SetEdgeStyle {
        edges: vec![e],
        soft: Some(true),
        smooth: None,
    })
    .unwrap();
    op(&mut m, SolidOp::Union, &fa, &fb).unwrap();
    assert!(m.edge_soft(e));
    assert_eq!(m.solid_report().solids().count(), 2);
}

#[test]
fn paint_goes_with_the_pieces_and_a_cap_takes_the_paint_of_the_tool() {
    let (a, b) = (([0.0; 3], [K; 3]), ([500.0; 3], [1500.0; 3]));
    let (mut m, fa, fb) = crossing(a, b);
    m.apply(&Edit::SetFaceMaterial {
        faces: fb.clone(),
        side: PaintSide::Both,
        material: Some("m-tool".into()),
    })
    .unwrap();
    m.apply(&Edit::SetFaceMaterial {
        faces: fa.clone(),
        side: PaintSide::Both,
        material: Some("m-base".into()),
    })
    .unwrap();
    op(&mut m, SolidOp::Subtract, &fa, &fb).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for (_, f) in m.faces() {
        seen.insert(
            f.attrs
                .get(ATTR_MATERIAL)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        );
    }
    assert!(
        seen.contains("m-base") && seen.contains("m-tool"),
        "{seen:?}"
    );
}

// ---- properties ----------------------------------------------------------------------------

/// A box on a grid of 500 mm with sides of 500 to 2500 mm: boxes often share planes, edges
/// and corners.
fn grid_box() -> impl Strategy<Value = Box3> {
    (
        proptest::array::uniform3(0u32..5),
        proptest::array::uniform3(1u32..6),
    )
        .prop_map(|(lo, len)| {
            let l: P = lo.map(|x| f64::from(x) * 500.0);
            let h: P = std::array::from_fn(|i| l[i] + f64::from(len[i]) * 500.0);
            (l, h)
        })
}

/// The volume of the result of an operation as the sum of the solids of the model.
fn total(m: &SurfaceModel) -> f64 {
    volumes(m).iter().sum()
}

/// True if the boxes have a common volume, or are apart on some axis by a gap.
fn generic(a: Box3, b: Box3) -> bool {
    common(a, b).is_some() || (0..3).any(|i| a.1[i] < b.0[i] || b.1[i] < a.0[i])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Union and Intersect of two boxes give the volumes of the sum and of the common box, so
    /// that vol(A union B) + vol(A intersect B) = vol(A) + vol(B); Subtract gives vol(A) less
    /// vol(common). A refusal changes nothing, and boxes that overlap or are apart never refuse
    /// for another reason than an empty result.
    #[test]
    fn volumes_of_boxes_on_a_grid(a in grid_box(), b in grid_box()) {
        let (union, inter, a_less_b, b_less_a) = expected(a, b);
        let (m, fa, fb) = pair(a, b);
        let mut results = Vec::new();
        for (o, x, y, want) in [
            (SolidOp::Union, &fa, &fb, union),
            (SolidOp::Intersect, &fa, &fb, inter),
            (SolidOp::Subtract, &fa, &fb, a_less_b),
            (SolidOp::Subtract, &fb, &fa, b_less_a),
        ] {
            let mut w = m.clone();
            match op(&mut w, o, x, y) {
                Ok(()) => {
                    prop_assert!(w.check().is_valid(), "{o} {a:?} {b:?}: {:?}", w.check().problems);
                    prop_assert!(close(total(&w), want), "{o} {a:?} {b:?}: {:?} {want}", volumes(&w));
                    results.push(Some(total(&w)));
                }
                Err(e) => {
                    prop_assert_eq!(&w, &m, "{} {:?} {:?}: {}", o, a, b, e);
                    prop_assert!(matches!(e, SurfaceError::Solid(_)), "{e}");
                    if generic(a, b) {
                        let empty = matches!(e, SurfaceError::Solid(SolidError::NoResult(_))) && want <= 0.0;
                        let touching_sticky = false;
                        prop_assert!(empty || touching_sticky, "{o} {a:?} {b:?}: {e}");
                    }
                    results.push(None);
                }
            }
        }
        if let (Some(u), Some(i)) = (results[0], results[1]) {
            prop_assert!(close(u + i, vol(a) + vol(b)));
        }
        if generic(a, b) && common(a, b).is_some() {
            // Split gives the three parts.
            let v = m.solid_preview(SolidOp::Split, &fa, &fb).unwrap();
            prop_assert!(close(v[0], a_less_b) && close(v[1], inter) && close(v[2], b_less_a), "{v:?}");
        }
    }

    /// A box turned about the vertical axis and moved: no plane is shared. The identities of the
    /// volumes hold: union + intersection = sum, and subtract + intersection = the first.
    #[test]
    fn volumes_of_a_turned_box(angle in 0.05f64..3.0, tilt in 0.0f64..0.4, dx in 200.0f64..900.0, dy in 200.0f64..900.0, dz in 200.0f64..900.0) {
        let a = ([0.0; 3], [K; 3]);
        let (sin, cos) = angle.sin_cos();
        let (st, ct) = tilt.sin_cos();
        let corners: [P; 8] = std::array::from_fn(|i| {
            let p = DVec3::new(
                if i & 1 == 0 { -400.0 } else { 400.0 },
                if i & 2 == 0 { -400.0 } else { 400.0 },
                if i & 4 == 0 { -400.0 } else { 400.0 },
            );
            let z = DVec3::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos, p.z);
            let t = DVec3::new(z.x, z.y * ct - z.z * st, z.y * st + z.z * ct);
            [t.x + dx, t.y + dy, t.z + dz]
        });
        let mut m = SurfaceModel::new();
        draw(&mut m, a);
        draw_corners(&mut m, &corners);
        let inward: Vec<FaceId> = m
            .solid_report()
            .shells
            .iter()
            .filter(|s| s.state == ShellState::Inverted)
            .flat_map(|s| s.faces.clone())
            .collect();
        m.apply(&Edit::ReverseFaces { faces: inward }).unwrap();
        let r = m.solid_report();
        let solids: Vec<Shell> = r.solids().cloned().collect();
        prop_assume!(solids.len() == 2);
        let (fa, fb) = (solids[0].faces.clone(), solids[1].faces.clone());
        let (va, vb) = (solids[0].volume, solids[1].volume);
        let mut got = Vec::new();
        for (o, x, y) in [
            (SolidOp::Union, &fa, &fb),
            (SolidOp::Intersect, &fa, &fb),
            (SolidOp::Subtract, &fa, &fb),
        ] {
            let mut w = m.clone();
            match op(&mut w, o, x, y) {
                Ok(()) => {
                    prop_assert!(w.check().is_valid(), "{o}: {:?}", w.check().problems);
                    got.push(Some(total(&w)));
                }
                Err(e) => {
                    prop_assert_eq!(&w, &m, "{}: {}", o, e);
                    got.push(None);
                }
            }
        }
        if let (Some(u), Some(i)) = (got[0], got[1]) {
            prop_assert!(close(u + i, va + vb), "{u} {i} {va} {vb}");
        }
        if let (Some(s), Some(i)) = (got[2], got[1]) {
            prop_assert!(close(s + i, va), "{s} {i} {va}");
        }
        // Boxes that cross like this always can be cut.
        prop_assert!(got.iter().all(Option::is_some), "{got:?}");
    }
}

// ---- curved and holed solids -----------------------------------------------------------------

/// A closed solid pushed up from a polygon in the plane `z = z0`.
fn prism(m: &mut SurfaceModel, pts: &[P], height: f64) {
    let n = pts.len();
    m.apply(&Edit::AddEdges {
        edges: (0..n).map(|i| [pts[i], pts[(i + 1) % n]]).collect(),
    })
    .unwrap();
    let f = m
        .faces()
        .find(|(_, f)| f.plane.normal.z.abs() > 0.99 && f.outer.len() == n)
        .map(|(id, _)| id)
        .unwrap();
    let up = m.face(f).unwrap().plane.normal.z > 0.0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: if up { height } else { -height },
    })
    .unwrap();
}

fn ngon(cx: f64, cy: f64, z: f64, r: f64, n: usize) -> Vec<P> {
    (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            [cx + r * t.cos(), cy + r * t.sin(), z]
        })
        .collect()
}

fn ngon_area(r: f64, n: usize) -> f64 {
    n as f64 / 2.0 * r * r * (std::f64::consts::TAU / n as f64).sin()
}

fn solids_of(m: &SurfaceModel) -> Vec<Shell> {
    m.solid_report().solids().cloned().collect()
}

#[test]
fn a_round_post_through_a_box_is_subtracted_and_united() {
    let mut m = SurfaceModel::new();
    let box_pts = [[0.0, 0.0, 0.0], [K, 0.0, 0.0], [K, K, 0.0], [0.0, K, 0.0]];
    prism(&mut m, &box_pts, 400.0);
    let n = 24;
    m.apply(&Edit::AddCurve {
        points: ngon(3500.0, 500.0, -300.0, 200.0, n),
        closed: true,
        faceted: false,
    })
    .unwrap();
    let post = m
        .faces()
        .find(|(_, f)| f.outer.len() == n)
        .map(|(id, _)| id)
        .unwrap();
    let up = m.face(post).unwrap().plane.normal.z > 0.0;
    m.apply(&Edit::PushPull {
        face: post,
        distance: if up { 1000.0 } else { -1000.0 },
    })
    .unwrap();
    // Move the post into the box.
    let post_faces = m.solid_of(post).unwrap();
    m.apply(&Edit::TransformVertices {
        vertices: vec![],
        faces: post_faces,
        edges: vec![],
        matrix: [
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, -3000.0, 0.0, 0.0,
        ],
    })
    .unwrap();
    let solids = solids_of(&m);
    assert_eq!(solids.len(), 2, "{:?}", m.solid_report());
    // The post is the one with the 24 side faces.
    let (post_faces, box_faces) = if solids[0].faces.len() > solids[1].faces.len() {
        (solids[0].faces.clone(), solids[1].faces.clone())
    } else {
        (solids[1].faces.clone(), solids[0].faces.clone())
    };
    let hole = ngon_area(200.0, n) * 400.0;
    let mut sub = m.clone();
    op(&mut sub, SolidOp::Subtract, &box_faces, &post_faces).unwrap();
    assert_sound(&sub);
    assert!(
        close(total(&sub), 400.0 * K * K - hole),
        "{:?}",
        volumes(&sub)
    );
    // The curved wall of the hole keeps its soft and smooth edges.
    let soft = sub.edges().filter(|(id, _)| sub.edge_soft(*id)).count();
    assert!(soft >= n, "{soft}");
    let mut uni = m.clone();
    op(&mut uni, SolidOp::Union, &box_faces, &post_faces).unwrap();
    assert_sound(&uni);
    let post_total = ngon_area(200.0, n) * 1000.0;
    assert!(
        close(total(&uni), 400.0 * K * K + post_total - hole),
        "{:?}",
        volumes(&uni)
    );
    let mut int = m.clone();
    op(&mut int, SolidOp::Intersect, &box_faces, &post_faces).unwrap();
    assert!(close(total(&int), hole), "{:?}", volumes(&int));
    assert_sound(&int);
}

#[test]
fn a_bar_through_a_tube_and_a_concave_solid() {
    // A tube: a square prism with a square hole through it.
    let mut m = SurfaceModel::new();
    let outer = [
        [0.0, 0.0, 0.0],
        [2.0 * K, 0.0, 0.0],
        [2.0 * K, 2.0 * K, 0.0],
        [0.0, 2.0 * K, 0.0],
    ];
    let inner = [
        [500.0, 500.0, 0.0],
        [1500.0, 500.0, 0.0],
        [1500.0, 1500.0, 0.0],
        [500.0, 1500.0, 0.0],
    ];
    let segs: Vec<[P; 2]> = [&outer, &inner]
        .iter()
        .flat_map(|poly| (0..4).map(move |i| [poly[i], poly[(i + 1) % 4]]))
        .collect();
    m.apply(&Edit::AddEdges { edges: segs }).unwrap();
    let inner_face = m
        .faces()
        .find(|(_, f)| f.holes.is_empty() && m.face_area(f_id(&m, f)).unwrap() < 1.1e6)
        .map(|(id, _)| id)
        .unwrap();
    m.apply(&Edit::EraseFaces {
        faces: vec![inner_face],
    })
    .unwrap();
    let ring = m.faces().next().unwrap().0;
    let up = m.face(ring).unwrap().plane.normal.z > 0.0;
    m.apply(&Edit::PushPull {
        face: ring,
        distance: if up { 600.0 } else { -600.0 },
    })
    .unwrap();
    let tube = solids_of(&m);
    assert_eq!(tube.len(), 1, "{:?}", m.solid_report());
    assert!(
        close(tube[0].volume, (4.0e6 - 1.0e6) * 600.0),
        "{}",
        tube[0].volume
    );
    // A bar through the hole, longer than the tube, sharing no plane with it.
    let bar = ([700.0, 700.0, -300.0], [900.0, 900.0, 900.0]);
    draw(&mut m, bar);
    let inward: Vec<FaceId> = m
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted && s.faces.len() == 6)
        .flat_map(|s| s.faces.clone())
        .collect();
    m.apply(&Edit::ReverseFaces { faces: inward }).unwrap();
    let solids = solids_of(&m);
    assert_eq!(solids.len(), 2, "{:?}", m.solid_report());
    let (t, b) = if solids[0].faces.len() > 6 {
        (&solids[0], &solids[1])
    } else {
        (&solids[1], &solids[0])
    };
    // The bar lies in the hole: the two do not touch, and the union is the two solids.
    let mut u = m.clone();
    op(&mut u, SolidOp::Union, &t.faces, &b.faces).unwrap();
    assert!(close(total(&u), t.volume + b.volume), "{:?}", volumes(&u));
    // A second bar across the tube wall: a real cut.
    let mut w = m.clone();
    let cross = ([-200.0, 1100.0, 100.0], [2200.0, 1300.0, 400.0]);
    draw(&mut w, cross);
    let inward: Vec<FaceId> = w
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted && s.faces.len() == 6)
        .flat_map(|s| s.faces.clone())
        .collect();
    w.apply(&Edit::ReverseFaces { faces: inward }).unwrap();
    let all = solids_of(&w);
    assert_eq!(all.len(), 3, "{:?}", w.solid_report());
    let second = all
        .iter()
        .find(|s| s.faces.len() == 6 && close(s.volume, 2400.0 * 200.0 * 300.0))
        .unwrap();
    let tube_now = all.iter().find(|s| s.faces.len() > 6).unwrap();
    let mut cut = w.clone();
    op(&mut cut, SolidOp::Subtract, &tube_now.faces, &second.faces).unwrap();
    assert_sound(&cut);
    // The bar removes 2 x 500 mm of wall and the hole is open between: wall length 500 + 500.
    let removed = 200.0 * 300.0 * (500.0 + 500.0);
    assert!(
        close(
            volumes(&cut).iter().sum::<f64>(),
            tube_now.volume + 200.0 * 200.0 * 1200.0 - removed
        ),
        "{:?}",
        volumes(&cut)
    );
}

fn f_id(m: &SurfaceModel, f: &Face) -> FaceId {
    m.faces().find(|(_, g)| *g == f).map(|(id, _)| id).unwrap()
}
