//! Tests of the flush cases of Push/Pull (the face lands on the model and merges), of the
//! attributes of merged faces, and of faces back to back in the model check.

use super::*;

/// The box with a circle of 12 sides (r = 300) at the middle of the top. Returns the model, the
/// top (with its hole) and the disc.
fn box_with_circle() -> (SurfaceModel, FaceId, FaceId) {
    let mut m = box_model();
    let top = top_face(&m);
    let points = circle_points(12, 300.0)
        .iter()
        .map(|p| [p[0] + 500.0, p[1] + 500.0, 500.0])
        .collect();
    m.apply(&Edit::AddCurve {
        points,
        closed: true,
        faceted: false,
    })
    .unwrap();
    let disc = m
        .faces()
        .find(|(id, f)| *id != top && f.plane.normal.z > 0.9)
        .unwrap()
        .0;
    assert_eq!(m.curves().count(), 1);
    (m, top, disc)
}

fn ids_of(m: &SurfaceModel) -> (Vec<VertexId>, Vec<EdgeId>, Vec<FaceId>) {
    (
        m.vertices().map(|(id, _)| id).collect(),
        m.edges().map(|(id, _)| id).collect(),
        m.faces().map(|(id, _)| id).collect(),
    )
}

fn set_material(m: &mut SurfaceModel, f: FaceId, name: &str) {
    m.faces
        .get_mut(&f)
        .unwrap()
        .attrs
        .insert("material".into(), serde_json::json!(name));
}

fn material(m: &SurfaceModel, f: FaceId) -> Option<&str> {
    m.face(f)?.attrs.get("material")?.as_str()
}

#[test]
fn a_cylinder_pushed_back_flush_and_a_round_pocket_pulled_back_up_give_the_box_again() {
    let plain = box_model();
    for d in [300.0, -200.0] {
        let (mut m, top, disc) = box_with_circle();
        pull(&mut m, disc, d).unwrap();
        assert_eq!(m.curves().count(), 2, "{d}: the ring and its copy");
        pull(&mut m, disc, -d).unwrap();
        assert!(m.check().is_closed(), "{d}: {:?}", m.check());
        assert_eq!(counts(&m), (8, 12, 6), "{d}");
        assert!(m.face(disc).is_none(), "{d}: the disc merged into the top");
        assert!(m.face(top).unwrap().holes.is_empty());
        // The circle goes with the edges between the two faces of one plane, as in Erase.
        assert_eq!(m.curves().count(), 0);
        assert_eq!(
            ids_of(&m).0,
            ids_of(&plain).0,
            "{d}: the box keeps its vertices"
        );
    }
}

/// The box with the top cut into three strips by lines at x = 300 and x = 700. Returns the model
/// and the strips from left to right.
fn three_strips() -> (SurfaceModel, [FaceId; 3]) {
    let mut m = box_model();
    m.apply(&Edit::AddEdges {
        edges: vec![
            [[300.0, 0.0, 500.0], [300.0, 1000.0, 500.0]],
            [[700.0, 0.0, 500.0], [700.0, 1000.0, 500.0]],
        ],
    })
    .unwrap();
    let mut strips: Vec<(f64, FaceId)> = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z > 0.9)
        .map(|(id, f)| {
            let x =
                f.outer.iter().map(|v| m.vertex(*v).unwrap().x).sum::<f64>() / f.outer.len() as f64;
            (x, id)
        })
        .collect();
    strips.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(strips.len(), 3);
    (m, [strips[0].1, strips[1].1, strips[2].1])
}

#[test]
fn the_middle_strip_pushed_down_and_back_up_merges_into_one_top_the_same_way_every_time() {
    let (mut m, strips) = three_strips();
    // The middle strip is the largest, so its material is the look of the merged top.
    set_material(&mut m, strips[1], "brick");
    pull(&mut m, strips[1], -200.0).unwrap();
    let (mut a, mut b) = (m.clone(), m.clone());
    let notes = a
        .apply_with_notes(&Edit::PushPull {
            face: strips[1],
            distance: 200.0,
        })
        .unwrap();
    b.apply(&Edit::PushPull {
        face: strips[1],
        distance: 200.0,
    })
    .unwrap();
    assert_eq!(a, b, "the same edit gives the same model");
    assert!(a.check().is_closed(), "{:?}", a.check());
    assert_eq!(counts(&a), (8, 12, 6));
    assert_eq!(ids_of(&a).0, ids_of(&box_model()).0);
    let keep = *strips.iter().min().unwrap();
    assert_eq!(
        ids_of(&a).2.iter().filter(|f| strips.contains(f)).count(),
        1
    );
    assert!(a.face(keep).is_some(), "the strip with the lowest id stays");
    assert_eq!(
        material(&a, keep),
        Some("brick"),
        "the material of the largest strip is the look of the merged top"
    );
    assert!(
        !notes.iter().any(|n| n.contains("dropped")),
        "nothing was dropped: {notes:?}"
    );
    assert_eq!(a.curves().count(), 0);
}

/// The split top with the left half pushed down 200 (a step at x = 500) and the square
/// (200, 300)-(500, 700) drawn on the low half. With `lines` the step wall is cut at y = 300 and
/// y = 700 first. Returns the model, the square and the high (right) half.
fn square_at_the_step(lines: bool) -> (SurfaceModel, FaceId, FaceId) {
    let (mut m, left, right) = split_top();
    pull(&mut m, left, -200.0).unwrap();
    if lines {
        m.apply(&Edit::AddEdges {
            edges: vec![
                [[500.0, 300.0, 300.0], [500.0, 300.0, 500.0]],
                [[500.0, 700.0, 300.0], [500.0, 700.0, 500.0]],
            ],
        })
        .unwrap();
    }
    poly(
        &mut m,
        &[
            [200.0, 300.0, 300.0],
            [500.0, 300.0, 300.0],
            [500.0, 700.0, 300.0],
            [200.0, 700.0, 300.0],
        ],
    );
    assert!(m.check().is_valid(), "{:?}", m.check());
    let sq = m
        .faces()
        .find(|(_, f)| {
            f.plane.normal.z > 0.9
                && f.outer.len() == 4
                && f.outer.iter().all(|v| {
                    let p = m.vertex(*v).unwrap();
                    p.x >= 200.0 && (300.0..=700.0).contains(&p.y) && p.z == 300.0
                })
        })
        .unwrap()
        .0;
    (m, sq, right)
}

#[test]
fn a_square_at_the_step_pulled_up_to_the_high_half_merges_with_it() {
    for lines in [true, false] {
        let (mut m, sq, right) = square_at_the_step(lines);
        pull(&mut m, sq, 200.0).unwrap();
        assert!(m.check().is_closed(), "{lines}: {:?}", m.check());
        // 5e8 less the low half (500 x 1000 x 200) plus the block (300 x 400 x 200).
        assert!((volume(&m) - 4.24e8).abs() < 1.0, "{lines}: {}", volume(&m));
        assert!(m.face(sq.min(right)).is_some() && m.face(sq.max(right)).is_none());
        // The step wall is left only beside the block: no face at x = 500 between y 300 and 700.
        let at_step: Vec<FaceId> = m
            .faces()
            .filter(|(_, f)| {
                f.plane.normal.x.abs() > 0.9 && (f.plane.offset.abs() - 500.0).abs() < 1e-9
            })
            .filter(|(_, f)| f.outer.iter().any(|v| m.vertex(*v).unwrap().z == 300.0))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(at_step.len(), 2, "{lines}: {at_step:?}");
        for f in at_step {
            let ys: Vec<f64> = m
                .face(f)
                .unwrap()
                .outer
                .iter()
                .map(|v| m.vertex(*v).unwrap().y)
                .collect();
            assert!(
                ys.iter().all(|y| *y <= 300.0) || ys.iter().all(|y| *y >= 700.0),
                "{lines}: {ys:?}"
            );
        }
    }
}

#[test]
fn a_bump_pushed_nearly_flush_snaps_within_the_tolerance_and_leaves_a_sliver_beyond_it() {
    for erase_top in [false, true] {
        for (d, flush) in [(-299.995, true), (-299.98, false)] {
            let (mut m, top, inner) = box_with_inner_square();
            pull(&mut m, inner, 300.0).unwrap();
            if erase_top {
                m.apply(&Edit::EraseFaces { faces: vec![top] }).unwrap();
            }
            let before = counts(&m);
            let r = pull(&mut m, inner, d);
            assert!(r.is_ok(), "{erase_top} {d}: {r:?}");
            match (flush, erase_top) {
                (true, false) => {
                    assert_eq!(counts(&m), (8, 12, 6));
                    assert!(m.face(inner).is_none());
                    assert_eq!(m.face(top).unwrap().plane.offset, 500.0);
                }
                (true, true) => {
                    // The cap closes the opening of the erased top, exactly in its plane.
                    assert_eq!(counts(&m), (12, 16, 6));
                    assert_eq!(m.face(inner).unwrap().plane.offset, 500.0);
                    assert!(m.vertices().all(|(_, p)| p.z == 0.0 || p.z == 500.0));
                }
                (false, _) => {
                    assert_eq!(counts(&m), before, "a sliver wall of 0.02 mm stays");
                    let z = m.face(inner).unwrap().plane.offset;
                    assert!((z - 500.02).abs() < 1e-9, "{z}");
                }
            }
        }
    }
}

#[test]
fn a_pocket_floor_with_a_bump_pulled_to_the_top_goes_in_two_steps() {
    let (mut m, top, floor) = box_with_inner_square();
    pull(&mut m, floor, -200.0).unwrap();
    poly(&mut m, &square(400.0, 400.0, 200.0, 300.0));
    let small = m
        .faces()
        .find(|(id, f)| *id != floor && f.plane.normal.z > 0.9 && f.plane.offset == 300.0)
        .unwrap()
        .0;
    pull(&mut m, small, 100.0).unwrap();
    // The floor would meet the top of the bump on the way: a merge on the way is a later step.
    refused_with(&mut m, floor, 200.0, "later step");
    pull(&mut m, floor, 100.0).unwrap();
    let floor = floor.min(small);
    assert!(m.face(floor).unwrap().holes.is_empty());
    pull(&mut m, floor, 100.0).unwrap();
    assert!(m.check().is_closed());
    assert_eq!(counts(&m), (8, 12, 6));
    assert!(m.face(top).is_some());
}

/// The faces of `b` put into `a` with their own ids (after those of `a`).
fn combine(a: &SurfaceModel, b: &SurfaceModel) -> SurfaceModel {
    let off = a.next_id;
    let mut vs = a.vertices.clone();
    let mut es = a.edges.clone();
    let mut fs = a.faces.clone();
    for (id, p) in &b.vertices {
        vs.insert(VertexId(id.0 + off), *p);
    }
    for (id, e) in &b.edges {
        es.insert(
            EdgeId(id.0 + off),
            Edge {
                v: e.v.map(|v| VertexId(v.0 + off)),
            },
        );
    }
    let shift = |l: &Vec<VertexId>| l.iter().map(|v| VertexId(v.0 + off)).collect::<Vec<_>>();
    for (id, f) in &b.faces {
        fs.insert(
            FaceId(id.0 + off),
            Face::new(
                f.plane,
                shift(&f.outer),
                f.holes.iter().map(shift).collect(),
            ),
        );
    }
    SurfaceModel::from_parts(vs, es, fs, off + b.next_id)
}

fn back_to_back(r: &check::Report) -> usize {
    r.warnings
        .iter()
        .filter(|p| matches!(p, Problem::BackToBack(..)))
        .count()
}

fn overlap_errors(r: &check::Report) -> usize {
    r.problems
        .iter()
        .filter(|p| {
            matches!(
                p,
                Problem::FacesOverlap(..) | Problem::EdgeInFace(..) | Problem::Overlap(..)
            )
        })
        .count()
}

#[test]
fn faces_back_to_back_are_a_warning_and_faces_on_the_same_side_an_error() {
    // A box standing on a slab, each with its own vertices: the bottom of the box lies in the
    // top of the slab, the two look opposite ways.
    let mut slab = SurfaceModel::new();
    poly(&mut slab, &square(0.0, 0.0, 1000.0, 0.0));
    let f = only_face(&slab);
    pull(&mut slab, f, 200.0).unwrap();
    let mut cube = SurfaceModel::new();
    poly(&mut cube, &square(250.0, 250.0, 500.0, 200.0));
    let f = only_face(&cube);
    pull(&mut cube, f, 300.0).unwrap();
    let m = combine(&slab, &cube);
    assert!(m.problem().is_none());
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert!(r.is_closed());
    assert_eq!(back_to_back(&r), 1, "{:?}", r.warnings);

    // The bottom of the box turned to look up: the two faces lie on the same side, an error.
    let bottom = m
        .faces()
        .find(|(id, f)| id.0 > slab.next_id && f.plane.normal.z < -0.9)
        .unwrap()
        .0;
    let mut wrong = m.clone();
    wrong
        .apply(&Edit::ReverseFaces {
            faces: vec![bottom],
        })
        .unwrap();
    let r = wrong.check();
    assert!(overlap_errors(&r) > 0, "{:?}", r.problems);
    assert_eq!(back_to_back(&r), 0, "{:?}", r.warnings);

    // A face and its reverse on the same edges: back to back at an edge, for the fast check too.
    let mut one = SurfaceModel::new();
    poly(&mut one, &square(0.0, 0.0, 1000.0, 0.0));
    let (id, f) = one.faces().next().map(|(id, f)| (id, f.clone())).unwrap();
    let mut twin = f.clone();
    twin.plane = twin.plane.flipped();
    twin.outer = super::super::arrange::reversed(&twin.outer);
    let mut fs = one.faces.clone();
    fs.insert(FaceId(one.next_id), twin);
    let m = SurfaceModel::from_parts(one.vertices.clone(), one.edges.clone(), fs, one.next_id + 1);
    assert!(m.problem().is_none(), "{:?}", m.problem());
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert!(
        r.warnings
            .contains(&Problem::BackToBack(id, FaceId(one.next_id))),
        "{:?}",
        r.warnings
    );
}

/// A loose square face at z = 0 for [`loose_faces`]: `(x0, x1, hole, up)` is the square x0..x1
/// (in x and y) with an optional square hole, facing up or down.
type Loose = (f64, f64, Option<(f64, f64)>, bool);

/// A model of loose faces, each with its own vertices.
fn loose_faces(fs: &[Loose]) -> SurfaceModel {
    let mut m = SurfaceModel::new();
    for &(x0, x1, hole, up) in fs {
        let mut one = SurfaceModel::new();
        let ring = |a: f64, b: f64| vec![[a, a, 0.0], [b, a, 0.0], [b, b, 0.0], [a, b, 0.0]];
        let mut edges: Vec<[P; 2]> = Vec::new();
        for r in std::iter::once(ring(x0, x1)).chain(hole.map(|(a, b)| ring(a, b))) {
            edges.extend((0..4).map(|i| [r[i], r[(i + 1) % 4]]));
        }
        one.apply(&Edit::AddEdges { edges }).unwrap();
        if let Some((a, b)) = hole {
            let inner = one
                .faces()
                .find(|(_, f)| {
                    f.outer.iter().all(|v| {
                        let p = one.vertex(*v).unwrap();
                        p.x >= a && p.x <= b
                    })
                })
                .unwrap()
                .0;
            one.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
        }
        if !up {
            let all: Vec<FaceId> = one.faces().map(|(id, _)| id).collect();
            one.apply(&Edit::ReverseFaces { faces: all }).unwrap();
        }
        m = combine(&m, &one);
    }
    m
}

#[test]
fn a_hole_under_the_sample_point_does_not_hide_an_overlap() {
    let holed = (0.0, 1000.0, Some((400.0, 600.0)), true);
    // B (0..1000) has a hole 400..600 under the middle of A (300..700): the edges of A still lie
    // in B.
    let r = loose_faces(&[holed, (300.0, 700.0, None, true)]).check();
    assert!(
        r.problems
            .iter()
            .any(|p| matches!(p, Problem::FacesOverlap(..))),
        "{:?}",
        r.problems
    );
    // A small face inside the hole overlaps nothing.
    let r = loose_faces(&[holed, (450.0, 550.0, None, true)]).check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert_eq!(back_to_back(&r), 0);
    // A looks down: back to back with B, a warning only.
    let r = loose_faces(&[holed, (300.0, 700.0, None, false)]).check();
    assert_eq!(overlap_errors(&r), 0, "{:?}", r.problems);
    assert_eq!(back_to_back(&r), 1, "{:?}", r.warnings);
}

#[test]
fn merging_faces_gives_the_larger_face_the_look_and_says_what_was_dropped() {
    // A bump with a material pushed flat: the top is larger and has none, so the top stays
    // plain; the material of the cap is dropped, and a note says so (it is not spread).
    let (mut m, top, inner) = box_with_inner_square();
    pull(&mut m, inner, 300.0).unwrap();
    set_material(&mut m, inner, "brick");
    let notes = m
        .apply_with_notes(&Edit::PushPull {
            face: inner,
            distance: -300.0,
        })
        .unwrap();
    assert_eq!(
        material(&m, top),
        None,
        "a small face does not paint a large one"
    );
    assert!(
        notes
            .iter()
            .any(|n| n.contains("dropped") && n.contains("brick") && n.contains("has none")),
        "{notes:?}"
    );
    // A bump with no material on a painted top: the top keeps its look, no note.
    let (mut m, top, inner) = box_with_inner_square();
    set_material(&mut m, top, "stone");
    pull(&mut m, inner, 300.0).unwrap();
    let notes = m
        .apply_with_notes(&Edit::PushPull {
            face: inner,
            distance: -300.0,
        })
        .unwrap();
    assert_eq!(material(&m, top), Some("stone"));
    assert!(notes.is_empty(), "{notes:?}");
    // Two materials: the larger face keeps its own and the note names the dropped one.
    let (mut m, top, inner) = box_with_inner_square();
    pull(&mut m, inner, 300.0).unwrap();
    set_material(&mut m, inner, "brick");
    set_material(&mut m, top, "stone");
    let notes = m
        .apply_with_notes(&Edit::PushPull {
            face: inner,
            distance: -300.0,
        })
        .unwrap();
    assert_eq!(material(&m, top), Some("stone"));
    assert!(
        notes
            .iter()
            .any(|n| n.contains("dropped") && n.contains("brick")),
        "{notes:?}"
    );
    // The same rule for Erase. Two halves of equal area: the lower id decides.
    let (mut m, left, right) = split_top();
    set_material(&mut m, left.max(right), "brick");
    let line = eid(&m, [500.0, 0.0, 500.0], [500.0, 1000.0, 500.0]);
    let notes = m
        .apply_with_notes(&Edit::EraseEdges { edges: vec![line] })
        .unwrap();
    assert_eq!(
        material(&m, left.min(right)),
        None,
        "the lower id decides a tie"
    );
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("dropped") && notes[0].contains("brick"),
        "{notes:?}"
    );
    let (mut m, left, right) = split_top();
    set_material(&mut m, left.min(right), "brick");
    let line = eid(&m, [500.0, 0.0, 500.0], [500.0, 1000.0, 500.0]);
    let notes = m
        .apply_with_notes(&Edit::EraseEdges { edges: vec![line] })
        .unwrap();
    assert_eq!(material(&m, left.min(right)), Some("brick"));
    assert!(notes.is_empty(), "{notes:?}");
    // Unequal areas: the larger strip decides, whatever the ids.
    let (mut m, strips) = three_strips();
    set_material(&mut m, strips[0], "brick");
    set_material(&mut m, strips[1], "stone");
    let line = eid(&m, [300.0, 0.0, 500.0], [300.0, 1000.0, 500.0]);
    let notes = m
        .apply_with_notes(&Edit::EraseEdges { edges: vec![line] })
        .unwrap();
    let kept = strips[0].min(strips[1]);
    assert_eq!(
        material(&m, kept),
        Some("stone"),
        "the middle strip is larger"
    );
    assert!(
        notes
            .iter()
            .any(|n| n.contains("dropped") && n.contains("brick")),
        "{notes:?}"
    );
    let (mut m, strips) = three_strips();
    set_material(&mut m, strips[1], "stone");
    let line = eid(&m, [300.0, 0.0, 500.0], [300.0, 1000.0, 500.0]);
    let notes = m
        .apply_with_notes(&Edit::EraseEdges { edges: vec![line] })
        .unwrap();
    assert_eq!(material(&m, strips[0].min(strips[1])), Some("stone"));
    assert!(notes.is_empty(), "{notes:?}");
    // Equal attributes need no note.
    let (mut m, _, _) = split_top();
    let line = eid(&m, [500.0, 0.0, 500.0], [500.0, 1000.0, 500.0]);
    let notes = m
        .apply_with_notes(&Edit::EraseEdges { edges: vec![line] })
        .unwrap();
    assert!(notes.is_empty(), "{notes:?}");
}
