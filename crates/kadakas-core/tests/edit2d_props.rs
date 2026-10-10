//! Property tests for the 2D editing functions: area stays when a shape is split, the inverse of
//! a transform gives the shape back, and no operation produces a number that is not finite.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::edit2d::boolean::{self, poly_area};
use kadakas_core::edit2d::modify::{self, ChamferSize, JoinKind, JoinMode};
use kadakas_core::edit2d::offset::{self, Side};
use kadakas_core::edit2d::plan::{self, Corner, OffsetTo, SurfaceOp};
use kadakas_core::edit2d::{ArrayParams, Chain, Transform, Xf};
use kadakas_core::model::{Attributes, ObjectId, Shape};
use proptest::prelude::*;

fn coord() -> impl Strategy<Value = f64> {
    -500.0f64..500.0
}

fn point() -> impl Strategy<Value = DVec2> {
    (coord(), coord()).prop_map(|(x, y)| DVec2::new(x, y))
}

fn size() -> impl Strategy<Value = f64> {
    1.0f64..300.0
}

fn rect() -> impl Strategy<Value = Shape> {
    (point(), size(), size()).prop_map(|(p, w, h)| Shape::Rect {
        min: p,
        max: p + DVec2::new(w, h),
    })
}

fn circle() -> impl Strategy<Value = Shape> {
    (point(), size()).prop_map(|(c, r)| Shape::Circle {
        center: c,
        radius: r,
    })
}

/// A convex polygon: points on a circle, in order.
fn convex() -> impl Strategy<Value = Shape> {
    (point(), 20.0f64..300.0, 3usize..9, 0.0f64..6.0).prop_map(|(c, r, n, rot)| Shape::Polyline {
        points: corners(
            &(0..n)
                .map(|i| {
                    c + DVec2::from_angle(rot + std::f64::consts::TAU * i as f64 / n as f64) * r
                })
                .collect::<Vec<_>>(),
        ),
        closed: true,
    })
}

fn closed_shape() -> impl Strategy<Value = Shape> {
    prop_oneof![rect(), circle(), convex()]
}

fn line() -> impl Strategy<Value = Shape> {
    (point(), point())
        .prop_filter("a line has a length", |(a, b)| a.distance(*b) > 1.0)
        .prop_map(|(a, b)| Shape::Line { a, b })
}

fn any_shape() -> impl Strategy<Value = Shape> {
    prop_oneof![line(), rect(), circle(), convex()]
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn total_area(ed: &Editor) -> f64 {
    ed.doc()
        .objects
        .values()
        .map(|o| o.shape.area().unwrap_or(0.0))
        .sum()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * (1.0 + a.abs().max(b.abs()))
}

fn same_shape(a: &Shape, b: &Shape) -> bool {
    match (a, b) {
        (Shape::Rect { min: a0, max: a1 }, Shape::Rect { min: b0, max: b1 }) => {
            a0.distance(*b0) < 1e-6 && a1.distance(*b1) < 1e-6
        }
        (Shape::Line { a: a0, b: a1 }, Shape::Line { a: b0, b: b1 }) => {
            a0.distance(*b0) < 1e-6 && a1.distance(*b1) < 1e-6
        }
        (
            Shape::Circle {
                center: c0,
                radius: r0,
            },
            Shape::Circle {
                center: c1,
                radius: r1,
            },
        ) => c0.distance(*c1) < 1e-6 && close(*r0, *r1, 1e-9),
        (Shape::Polyline { points: p0, .. }, Shape::Polyline { points: p1, .. }) => {
            p0.len() == p1.len() && p0.iter().zip(p1).all(|(a, b)| a.p.distance(b.p) < 1e-6)
        }
        _ => false,
    }
}

fn finite(shape: &Shape) -> bool {
    shape.is_finite()
}

/// A star-shaped polygon: alternating long and short radii at growing angles. Simple, mostly concave.
fn star() -> impl Strategy<Value = Shape> {
    (
        point(),
        5usize..10,
        100.0f64..300.0,
        30.0f64..100.0,
        0.0f64..6.0,
    )
        .prop_map(|(c, n, long, short, rot)| Shape::Polyline {
            points: corners(
                &(0..2 * n)
                    .map(|i| {
                        let r = if i % 2 == 0 { long } else { short };
                        c + DVec2::from_angle(
                            rot + std::f64::consts::TAU * i as f64 / (2 * n) as f64,
                        ) * r
                    })
                    .collect::<Vec<_>>(),
            ),
            closed: true,
        })
}

/// True if no two edges of the closed polygon cross (edges that share a corner may touch).
fn is_simple(shape: &Shape) -> bool {
    let Shape::Polyline { points, .. } = shape else {
        return true;
    };
    let p: Vec<DVec2> = points.iter().map(|v| v.p).collect();
    let n = p.len();
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (a, b, c, d) = (p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]);
            let (r, s) = (b - a, d - c);
            let den = r.perp_dot(s);
            if den.abs() < 1e-12 {
                continue;
            }
            let t = (c - a).perp_dot(s) / den;
            let u = (c - a).perp_dot(r) / den;
            if t > 1e-6 && t < 1.0 - 1e-6 && u > 1e-6 && u < 1.0 - 1e-6 {
                return false;
            }
        }
    }
    true
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// An inward offset is simple and smaller than the source, or it collapses with an error.
    #[test]
    fn inward_offsets_are_simple_and_smaller(shape in prop_oneof![star(), convex(), rect()], d in 1.0f64..140.0) {
        match offset::offset_shapes(&shape, d, Side::Inside) {
            Ok(parts) => {
                let total: f64 = parts.iter().map(|s| s.area().unwrap()).sum();
                prop_assert!(total < shape.area().unwrap(), "{total} vs {}", shape.area().unwrap());
                for p in &parts {
                    prop_assert!(is_simple(p), "{p:?}");
                    prop_assert!(p.is_finite());
                }
            }
            Err(e) => prop_assert!(e.contains("collapses"), "{e}"),
        }
    }

    /// An outward offset contains the source.
    #[test]
    fn outward_offsets_contain_the_source(shape in prop_oneof![star(), convex(), rect()], d in 1.0f64..200.0) {
        let parts = offset::offset_shapes(&shape, d, Side::Outside).unwrap();
        let rings: Vec<boolean::Ring> = parts.iter().filter_map(boolean::ring_of).collect();
        prop_assert!(!rings.is_empty());
        let out_area: f64 = boolean::union(&rings).iter().map(poly_area).sum();
        prop_assert!(out_area > shape.area().unwrap());
        let mut all = rings.clone();
        all.push(boolean::ring_of(&shape).unwrap());
        let both: f64 = boolean::union(&all).iter().map(poly_area).sum();
        prop_assert!(close(both, out_area, 1e-6), "{both} vs {out_area}");
    }

    /// Cutting a closed shape by a line keeps the area (the two pieces add up).
    #[test]
    fn split_by_line_conserves_area(shape in closed_shape(), a in point(), b in point()) {
        prop_assume!(a.distance(b) > 1.0);
        let mut ed = Editor::default();
        let id = add(&mut ed, shape);
        let before = total_area(&ed);
        if let Ok(cmd) = plan::split_by_line(ed.doc(), id, a, b) {
            ed.execute(&cmd).unwrap();
            prop_assert!(ed.doc().objects.len() >= 2);
            // A circle is flattened to a polygon with the same area (see `boolean::ring_of`).
            prop_assert!(close(total_area(&ed), before, 1e-6), "{} vs {}", total_area(&ed), before);
            prop_assert!(ed.doc().objects.values().all(|o| finite(&o.shape)));
            // One undo gives the shape back.
            ed.undo();
            prop_assert_eq!(ed.doc().objects.len(), 1);
        }
    }

    /// Splitting a chain at points keeps its length.
    #[test]
    fn split_chain_conserves_length(shape in any_shape(), p in point(), q in point()) {
        let chain = Chain::from_shape(&shape).unwrap();
        if let Ok(parts) = modify::split(&chain, &[p, q]) {
            let total: f64 = parts.iter().map(Chain::length).sum();
            prop_assert!(close(total, chain.length(), 1e-9), "{total} vs {}", chain.length());
            prop_assert!(parts.iter().all(|c| c.length().is_finite() && c.length() > 0.0));
        }
    }

    /// Mirroring twice, turning back, and scaling back give the same shape.
    #[test]
    fn transforms_have_inverses(shape in any_shape(), a in point(), b in point(), angle in -6.0f64..6.0, s in 0.1f64..8.0) {
        prop_assume!(a.distance(b) > 1.0);
        let mut ed = Editor::default();
        let id = add(&mut ed, shape.clone());
        let run = |ed: &mut Editor, t: Transform| ed.execute(&Command::TransformObjects { ids: vec![id], transform: t, copy: false });
        // Mirror twice.
        run(&mut ed, Transform::Mirror { a, b }).unwrap();
        run(&mut ed, Transform::Mirror { a, b }).unwrap();
        let now = ed.doc().object(id).unwrap().shape.clone();
        let (lo0, hi0) = shape.bounds();
        let (lo1, hi1) = now.bounds();
        prop_assert!(lo0.distance(lo1) < 1e-6 && hi0.distance(hi1) < 1e-6, "{shape:?} -> {now:?}");
        // Scale and scale back.
        run(&mut ed, Transform::Scale { center: a, sx: s, sy: None }).unwrap();
        run(&mut ed, Transform::Scale { center: a, sx: 1.0 / s, sy: None }).unwrap();
        let now = ed.doc().object(id).unwrap().shape.clone();
        let (lo1, hi1) = now.bounds();
        prop_assert!(lo0.distance(lo1) < 1e-6 && hi0.distance(hi1) < 1e-6);
        // Turn and turn back. A rectangle becomes a polyline when turned, so compare areas.
        run(&mut ed, Transform::Rotate { center: b, angle }).unwrap();
        run(&mut ed, Transform::Rotate { center: b, angle: -angle }).unwrap();
        let now = ed.doc().object(id).unwrap().shape.clone();
        prop_assert!(finite(&now));
        if let (Some(a0), Some(a1)) = (shape.area(), now.area()) {
            prop_assert!(close(a0, a1, 1e-9));
        }
        // Undo all six steps.
        for _ in 0..6 { prop_assert!(ed.undo()); }
        prop_assert!(same_shape(&ed.doc().object(id).unwrap().shape, &shape));
    }

    /// Scaling a closed shape by s multiplies the area by s squared.
    #[test]
    fn scale_changes_area_by_the_square(shape in closed_shape(), c in point(), s in 0.1f64..6.0) {
        let mut ed = Editor::default();
        let id = add(&mut ed, shape.clone());
        let a0 = shape.area().unwrap();
        ed.execute(&Command::TransformObjects { ids: vec![id], transform: Transform::Scale { center: c, sx: s, sy: None }, copy: false }).unwrap();
        let a1 = ed.doc().object(id).unwrap().shape.area().unwrap();
        prop_assert!(close(a1, a0 * s * s, 1e-9));
    }

    /// A mirror copy has the same area as the original.
    #[test]
    fn mirror_copy_keeps_area(shape in closed_shape(), a in point(), b in point()) {
        prop_assume!(a.distance(b) > 1.0);
        let mut ed = Editor::default();
        let id = add(&mut ed, shape.clone());
        let out = ed.execute(&Command::TransformObjects { ids: vec![id], transform: Transform::Mirror { a, b }, copy: true }).unwrap();
        let copy = ed.doc().object(ObjectId(out.created[0])).unwrap().shape.clone();
        prop_assert!(close(copy.area().unwrap(), shape.area().unwrap(), 1e-9));
    }

    /// Offset out and back in gives a convex shape back.
    #[test]
    fn offset_out_and_in(shape in prop_oneof![rect(), circle(), convex()], d in 0.5f64..30.0) {
        let out = offset::offset_shape(&shape, d, Side::Outside).unwrap();
        prop_assert!(out.area().unwrap() > shape.area().unwrap());
        let back = offset::offset_shape(&out, d, Side::Inside).unwrap();
        prop_assert!(close(back.area().unwrap(), shape.area().unwrap(), 1e-6), "{} vs {}", back.area().unwrap(), shape.area().unwrap());
    }

    /// Operations on random shapes either fail with a message or give finite shapes.
    #[test]
    fn nothing_produces_a_number_that_is_not_finite(
        s1 in any_shape(), s2 in any_shape(), p in point(), q in point(), r in 0.5f64..80.0, d in 0.5f64..80.0,
    ) {
        let mut ed = Editor::default();
        let a = add(&mut ed, s1);
        let b = add(&mut ed, s2);
        let kinds = [JoinKind::Fillet(r), JoinKind::Chamfer(ChamferSize::Distances(d, d)), JoinKind::Chamfer(ChamferSize::Length(d))];
        let mut cmds = Vec::new();
        for kind in kinds {
            for mode in [JoinMode::Standard, JoinMode::Trim, JoinMode::Split] {
                cmds.push(plan::join(ed.doc(), a, p, b, q, Corner { kind, mode }));
            }
        }
        cmds.push(plan::trim(ed.doc(), a, p, None));
        cmds.push(plan::extend(ed.doc(), a, p, None));
        cmds.push(plan::split(ed.doc(), a, &[p, q]));
        cmds.push(plan::offset(ed.doc(), a, OffsetTo::Distance(d, Side::Left), true));
        cmds.push(plan::offset(ed.doc(), a, OffsetTo::Through(p), true));
        cmds.push(plan::round_all(ed.doc(), a, None, JoinKind::Fillet(r)));
        for op in [SurfaceOp::Add, SurfaceOp::Intersect, SurfaceOp::Compose, SurfaceOp::Clip] {
            cmds.push(plan::surface(ed.doc(), op, &[a, b], None, false));
        }
        for c in cmds.into_iter().flatten() {
            let mut copy = Editor::new(ed.doc().clone());
            if copy.execute(&c).is_ok() {
                prop_assert!(copy.doc().objects.values().all(|o| finite(&o.shape)), "{c:?}");
            }
        }
        // Chamfer with an angle, extreme values.
        let size = plan::chamfer_size(Some(d), None, Some(r / 10.0), None).unwrap();
        let _ = plan::join(ed.doc(), a, p, b, q, Corner { kind: JoinKind::Chamfer(size), mode: JoinMode::Trim });
    }

    /// Inclusion and exclusion: |A union B| + |A intersect B| = |A| + |B|, and compose covers the union once.
    #[test]
    fn boolean_areas_add_up(s1 in closed_shape(), s2 in closed_shape()) {
        let (a, b) = (boolean::ring_of(&s1).unwrap(), boolean::ring_of(&s2).unwrap());
        let area = |p: &[boolean::Poly]| -> f64 { p.iter().map(poly_area).sum() };
        let (aa, ab) = (area(&boolean::union(std::slice::from_ref(&a))), area(&boolean::union(std::slice::from_ref(&b))));
        let u = area(&boolean::union(&[a.clone(), b.clone()]));
        let i = area(&boolean::intersect(&[a.clone(), b.clone()]));
        prop_assert!(close(u + i, aa + ab, 1e-6), "{u} + {i} vs {aa} + {ab}");
        let d = area(&boolean::subtract(&a, std::slice::from_ref(&b)));
        prop_assert!(close(d + i, aa, 1e-6));
        let regions = boolean::compose(&[a, b]);
        let total: f64 = regions.iter().map(|r| area(&r.polys)).sum();
        prop_assert!(close(total, u, 1e-6));
    }

    /// An array of n copies makes n new objects, and one undo removes them all.
    #[test]
    fn array_counts_and_undo(shape in any_shape(), n in 1usize..8, dx in -200.0f64..200.0) {
        let mut ed = Editor::default();
        let id = add(&mut ed, shape);
        let array: ArrayParams = serde_json::from_value(serde_json::json!({"shape": "linear", "count": n, "offset": [dx, 10.0]})).unwrap();
        let out = ed.execute(&Command::ArrayObjects { ids: vec![id], array }).unwrap();
        prop_assert_eq!(out.created.len(), n);
        prop_assert_eq!(ed.doc().objects.len(), n + 1);
        ed.undo();
        prop_assert_eq!(ed.doc().objects.len(), 1);
    }

    /// A circular array about the center of a shape keeps the radius of its copies.
    #[test]
    fn circular_copies_stay_on_the_circle(c in point(), p in point(), n in 1usize..9) {
        prop_assume!(c.distance(p) > 1.0);
        let mut ed = Editor::default();
        let id = add(&mut ed, Shape::Circle { center: p, radius: 2.0 });
        let array: ArrayParams = serde_json::from_value(serde_json::json!({"shape": "circular", "count": n, "center": [c.x, c.y]})).unwrap();
        let out = ed.execute(&Command::ArrayObjects { ids: vec![id], array }).unwrap();
        for id in out.created {
            let Shape::Circle { center, .. } = ed.doc().object(ObjectId(id)).unwrap().shape else { panic!() };
            prop_assert!(close(center.distance(c), p.distance(c), 1e-9));
        }
    }

    /// The transform maps points consistently: the composition with the inverse is the identity.
    #[test]
    fn xf_inverse(c in point(), angle in -6.0f64..6.0, p in point()) {
        let back = Xf::rotate(c, angle).then(Xf::rotate(c, -angle));
        prop_assert!(back.apply(p).distance(p) < 1e-6);
        let m = Xf::mirror(c, c + DVec2::from_angle(angle)).unwrap();
        prop_assert!(m.then(m).apply(p).distance(p) < 1e-6);
    }

    /// Fillet radius is exact: the center of the fillet arc is the given radius from both lines.
    #[test]
    fn fillet_is_tangent_to_both_lines(angle in 0.3f64..2.8, r in 1.0f64..20.0, len in 60.0f64..200.0) {
        use kadakas_core::edit2d::Piece;
        let a = Piece::line(DVec2::ZERO, DVec2::new(len, 0.0));
        let b = Piece::line(DVec2::ZERO, DVec2::from_angle(angle) * len);
        let j = modify::fillet_joint(&a, DVec2::new(len * 0.9, 0.0), &b, DVec2::from_angle(angle) * len * 0.9, r).unwrap();
        let Piece::Arc { c, r: rr, .. } = j.piece else { panic!() };
        prop_assert!(close(rr, r, 1e-12));
        prop_assert!(close(c.y.abs(), r, 1e-9));
        let db = (c - b.point(b.param_of(c))).length();
        prop_assert!(close(db, r, 1e-9));
    }
}
