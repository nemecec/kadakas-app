//! Export to DXF, import again, and compare the geometry.

use cadektor_core::command::Editor;
use cadektor_core::curve::{self, Vertex, VertexKind, corners};
use cadektor_core::model::{Document, Object, ObjectId, Shape, arc_sweep};
use cadektor_core::{DVec2, DVec3};
use cadektor_io::dxf::DxfFormat;
use cadektor_io::{Context, ExportOptions, Exporter, ImportOptions, Importer, Source, Unit};
use proptest::prelude::*;

fn add(doc: &mut Document, shape: Shape) {
    let id = ObjectId(doc.alloc_id());
    doc.objects.insert(
        id,
        Object::new(id, doc.layers[0].id, doc.classes[0].id, shape),
    );
}

fn roundtrip(doc: &Document, eo: &ExportOptions, io: &ImportOptions) -> Document {
    let bytes = DxfFormat.export(doc, eo).unwrap().bytes;
    let src = Source {
        name: "rt".into(),
        data: bytes,
        extra: Default::default(),
    };
    let r = DxfFormat
        .import(&src, &Context::of(&Document::with_sequential_ids()), io)
        .unwrap();
    assert_eq!(r.report.skipped_total(), 0, "{:?}", r.report.skipped);
    let mut ed = Editor::new(Document::with_sequential_ids());
    ed.execute(&r.command).unwrap();
    ed.doc().clone()
}

/// Largest distance from a point of `a` to the polyline `b`.
fn deviation(a: &[DVec2], b: &[DVec2]) -> f64 {
    let dist = |p: DVec2| {
        b.windows(2)
            .map(|w| {
                let (s, e) = (w[0], w[1]);
                let d = e - s;
                let t = if d.length_squared() == 0.0 {
                    0.0
                } else {
                    ((p - s).dot(d) / d.length_squared()).clamp(0.0, 1.0)
                };
                p.distance(s + d * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    a.iter().map(|p| dist(*p)).fold(0.0, f64::max)
}

fn outline(points: &[Vertex], closed: bool) -> Vec<DVec2> {
    let mut pts = curve::flatten(points, closed, 0.25);
    if closed {
        pts.push(pts[0]);
    }
    pts
}

fn assert_same_curve(a: &[Vertex], ca: bool, b: &[Vertex], cb: bool, tol: f64) {
    assert_eq!(ca, cb);
    let (pa, pb) = (outline(a, ca), outline(b, cb));
    assert!(deviation(&pa, &pb) < tol, "a to b: {}", deviation(&pa, &pb));
    assert!(deviation(&pb, &pa) < tol, "b to a: {}", deviation(&pb, &pa));
}

#[test]
fn all_shapes_round_trip() {
    let mut doc = Document::with_sequential_ids();
    let arc_poly = vec![
        Vertex::corner(DVec2::new(0.0, 0.0)),
        Vertex::new(DVec2::new(1500.0, -800.0), VertexKind::PointOnArc),
        Vertex::corner(DVec2::new(3000.0, 0.0)),
        Vertex::new(
            DVec2::new(3000.0, 1000.0),
            VertexKind::Arc { radius: 300.0 },
        ),
        Vertex::corner(DVec2::new(0.0, 2000.0)),
    ];
    let shapes = vec![
        Shape::Line {
            a: DVec2::new(1.5, 2.5),
            b: DVec2::new(1000.0, -2000.25),
        },
        Shape::Rect {
            min: DVec2::new(-500.0, 100.0),
            max: DVec2::new(2500.0, 900.0),
        },
        Shape::Circle {
            center: DVec2::new(100.0, 200.0),
            radius: 750.5,
        },
        Shape::Arc {
            center: DVec2::new(-100.0, 50.0),
            radius: 400.0,
            start: 5.5,
            end: 1.0,
        },
        Shape::Polyline {
            points: arc_poly.clone(),
            closed: false,
        },
        Shape::Polyline {
            points: arc_poly.clone(),
            closed: true,
        },
        Shape::Polyline {
            points: vec![
                Vertex::corner(DVec2::new(0.0, 0.0)),
                Vertex::new(DVec2::new(500.0, 800.0), VertexKind::Bezier),
                Vertex::corner(DVec2::new(1000.0, 0.0)),
            ],
            closed: false,
        },
        Shape::Text {
            at: DVec2::new(10.0, 20.0),
            text: "Hello, W\u{f6}rld \u{b0}".into(),
            height: 250.0,
        },
        Shape::Text {
            at: DVec2::new(10.0, 20.0),
            text: "two\nlines".into(),
            height: 100.0,
        },
        Shape::LandscapeArea {
            boundary: arc_poly.clone(),
            name: "Lawn".into(),
        },
        Shape::Hardscape {
            boundary: corners(&[
                DVec2::ZERO,
                DVec2::new(3000.0, 0.0),
                DVec2::new(3000.0, 2000.0),
            ]),
            material: "Granite".into(),
            thickness: None,
            elevation: None,
        },
        Shape::Locus3d {
            p: DVec3::new(12_345.678, -2_000.5, 4_321.25),
        },
        Shape::Polyline3d {
            points: vec![
                DVec3::new(0.0, 0.0, 10.0),
                DVec3::new(1000.0, 0.0, 20.5),
                DVec3::new(0.0, 1000.0, -3.0),
            ],
            closed: true,
        },
        Shape::Polyline3d {
            points: vec![DVec3::new(0.0, 0.0, 1.0), DVec3::new(10.0, 10.0, 2.0)],
            closed: false,
        },
        Shape::Plant {
            at: DVec2::new(5000.0, 5000.0),
            species: "Betula pendula".into(),
            spread: 3000.0,
            height: 8000.0,
        },
    ];
    for s in &shapes {
        add(&mut doc, s.clone());
    }
    let back = roundtrip(&doc, &ExportOptions::default(), &ImportOptions::default());
    let b: Vec<&Shape> = back.objects.values().map(|o| &o.shape).collect();
    // The plant is a circle and the two lines of its block.
    assert_eq!(b.len(), shapes.len() + 2);
    let tol = 0.05;
    let mut i = 0;
    for s in &shapes {
        match (s, b[i]) {
            (Shape::Line { a, b: e }, Shape::Line { a: a2, b: e2 }) => {
                assert!(a.distance(*a2) < tol && e.distance(*e2) < tol)
            }
            (
                Shape::Rect { min, max },
                Shape::Polyline {
                    points,
                    closed: true,
                },
            ) => {
                let want = [
                    *min,
                    DVec2::new(max.x, min.y),
                    *max,
                    DVec2::new(min.x, max.y),
                ];
                assert_eq!(points.len(), 4);
                assert!(points.iter().zip(want).all(|(v, w)| v.p.distance(w) < tol));
            }
            (
                Shape::Circle { center, radius },
                Shape::Circle {
                    center: c2,
                    radius: r2,
                },
            ) => assert!(center.distance(*c2) < tol && (radius - r2).abs() < tol),
            (
                Shape::Arc {
                    center,
                    radius,
                    start,
                    end,
                },
                Shape::Arc {
                    center: c2,
                    radius: r2,
                    start: s2,
                    end: e2,
                },
            ) => {
                assert!(center.distance(*c2) < tol && (radius - r2).abs() < tol);
                assert!(
                    (start - s2).rem_euclid(std::f64::consts::TAU).min(
                        std::f64::consts::TAU - (start - s2).rem_euclid(std::f64::consts::TAU)
                    ) < 1e-9
                );
                assert!((arc_sweep(*start, *end) - arc_sweep(*s2, *e2)).abs() < 1e-9);
            }
            (
                Shape::Polyline { points, closed },
                Shape::Polyline {
                    points: p2,
                    closed: c2,
                },
            ) => assert_same_curve(points, *closed, p2, *c2, 1.2),
            (
                Shape::LandscapeArea { boundary, .. },
                Shape::Polyline {
                    points: p2,
                    closed: true,
                },
            ) => assert_same_curve(boundary, true, p2, true, 1.2),
            (
                Shape::Hardscape { boundary, .. },
                Shape::Polyline {
                    points: p2,
                    closed: true,
                },
            ) => assert_same_curve(boundary, true, p2, true, 1.2),
            (
                Shape::Text { at, text, height },
                Shape::Text {
                    at: a2,
                    text: t2,
                    height: h2,
                },
            ) => {
                assert!(at.distance(*a2) < tol && (height - h2).abs() < tol);
                assert_eq!(text, t2);
            }
            (Shape::Locus3d { p }, Shape::Locus3d { p: p2 }) => assert!(p.distance(*p2) < tol),
            (
                Shape::Polyline3d { points, closed },
                Shape::Polyline3d {
                    points: p2,
                    closed: c2,
                },
            ) => {
                assert_eq!(closed, c2);
                assert!(points.iter().zip(p2).all(|(a, b)| a.distance(*b) < tol));
            }
            (Shape::Plant { at, spread, .. }, Shape::Circle { center, radius }) => {
                assert!(at.distance(*center) < tol && (spread / 2.0 - radius).abs() < tol);
                assert!(
                    matches!(b[i + 1], Shape::Line { .. })
                        && matches!(b[i + 2], Shape::Line { .. })
                );
                i += 2;
            }
            (a, b) => panic!("{a:?} came back as {b:?}"),
        }
        i += 1;
    }
}

#[test]
fn classes_and_layers_round_trip_by_name() {
    let mut doc = Document::with_sequential_ids();
    let walls = cadektor_core::model::ClassId(doc.alloc_id());
    doc.classes.push(cadektor_core::model::Class {
        extra: Default::default(),
        id: walls,
        name: "Walls & fences".into(),
        visible: false,
        attrs: cadektor_core::model::Attributes {
            pen: Some(cadektor_core::model::Color([200, 10, 30, 255])),
            fill: None,
            line_weight: Some(0.5),
            ..Default::default()
        },
    });
    add(
        &mut doc,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    );
    let id = ObjectId(doc.alloc_id());
    let mut o = Object::new(
        id,
        doc.layers[0].id,
        walls,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::Y,
        },
    );
    o.attrs.pen = Some(cadektor_core::model::Color([1, 2, 3, 255]));
    o.attrs.line_weight = Some(0.35);
    doc.objects.insert(id, o);
    let back = roundtrip(&doc, &ExportOptions::default(), &ImportOptions::default());
    let c = back
        .classes
        .iter()
        .find(|c| c.name == "Walls & fences")
        .unwrap();
    assert!(!c.visible);
    assert_eq!(
        c.attrs.pen,
        Some(cadektor_core::model::Color([200, 10, 30, 255]))
    );
    assert_eq!(c.attrs.line_weight, Some(0.5));
    let objs: Vec<_> = back.objects.values().collect();
    assert_eq!(objs[1].class, c.id);
    assert_eq!(
        objs[1].attrs.pen,
        Some(cadektor_core::model::Color([1, 2, 3, 255]))
    );
    assert_eq!(objs[1].attrs.line_weight, Some(0.35));
    // The class "None" is layer 0.
    assert!(back.classes.iter().any(|c| c.name == "0"));
}

#[test]
fn units_scale_both_ways() {
    let mut doc = Document::with_sequential_ids();
    add(
        &mut doc,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::new(1000.0, 0.0),
        },
    );
    add(
        &mut doc,
        Shape::Locus3d {
            p: DVec3::new(10.0, 20.0, 3000.0),
        },
    );
    for unit in [Unit::M, Unit::Foot, Unit::Inch, Unit::Cm] {
        let back = roundtrip(
            &doc,
            &ExportOptions {
                unit,
                ..Default::default()
            },
            &ImportOptions::default(),
        );
        let Shape::Line { b, .. } = back.objects.values().next().unwrap().shape else {
            panic!()
        };
        assert!((b.x - 1000.0).abs() < 1e-6, "{unit:?}");
        let Shape::Locus3d { p } = back.objects.values().nth(1).unwrap().shape else {
            panic!()
        };
        assert!((p.z - 3000.0).abs() < 1e-6, "{unit:?}");
    }
}

fn vertex_strategy() -> impl Strategy<Value = (i32, i32, bool)> {
    (-2000..2000i32, -2000..2000i32, any::<bool>())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn random_polylines_round_trip(raw in proptest::collection::vec(vertex_strategy(), 2..8), closed in any::<bool>()) {
        // Distinct points on a grid. Arc-through vertices only in the middle, never next to each other.
        let mut pts: Vec<Vertex> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (i, (x, y, arc)) in raw.iter().enumerate() {
            if !seen.insert((*x, *y)) { continue; }
            let kind = if *arc && i > 0 && i + 1 < raw.len() && pts.last().is_none_or(|l| l.kind == VertexKind::Corner) {
                VertexKind::PointOnArc
            } else {
                VertexKind::Corner
            };
            pts.push(Vertex::new(DVec2::new(*x as f64 * 10.0, *y as f64 * 10.0), kind));
        }
        prop_assume!(pts.len() >= 3);
        // The last vertex of an open path is a corner.
        if let Some(last) = pts.last_mut() { last.kind = VertexKind::Corner; }
        // Skip near-straight arcs and near-duplicate segments that make the arc ill-defined.
        for w in 1..pts.len() - 1 {
            if pts[w].kind == VertexKind::PointOnArc {
                let (a, b, c) = (pts[w - 1].p, pts[w].p, pts[w + 1].p);
                prop_assume!((b - a).perp_dot(c - b).abs() > 1.0e4);
                // The arc must not turn back: otherwise it runs around a huge circle (kilometers wide), which
                // makes the comparison slow and says nothing about the writer.
                prop_assume!((b - a).dot(c - b) > 0.0);
            }
        }
        let mut doc = Document::with_sequential_ids();
        add(&mut doc, Shape::Polyline { points: pts.clone(), closed });
        // A nearly straight arc has a huge bounding box, which could make the default `center` shift the drawing.
        let keep = ImportOptions { origin: cadektor_io::OriginMode::File, ..ImportOptions::default() };
        let back = roundtrip(&doc, &ExportOptions::default(), &keep);
        let Shape::Polyline { points: p2, closed: c2 } = &back.objects.values().next().unwrap().shape else { panic!() };
        let (pa, pb) = (outline(&pts, closed), outline(p2, *c2));
        prop_assert!(deviation(&pa, &pb) < 1.5, "{}", deviation(&pa, &pb));
        prop_assert!(deviation(&pb, &pa) < 1.5, "{}", deviation(&pb, &pa));
    }

    #[test]
    fn random_survey_points_round_trip(x in -1.0e7..1.0e7f64, y in -1.0e7..1.0e7f64, z in -1.0e5..1.0e5f64) {
        let mut doc = Document::with_sequential_ids();
        add(&mut doc, Shape::Locus3d { p: DVec3::new(x, y, z) });
        // `file` keeps the coordinates. With the default (`center`) a point 6 km from the origin is shifted.
        let keep = ImportOptions { origin: cadektor_io::OriginMode::File, ..ImportOptions::default() };
        let back = roundtrip(&doc, &ExportOptions::default(), &keep);
        let Shape::Locus3d { p } = back.objects.values().next().unwrap().shape else { panic!() };
        prop_assert!(p.distance(DVec3::new(x, y, z)) < 1e-6);
    }
}

#[test]
fn survey_in_lest97_keeps_its_real_world_coordinates() {
    let text = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n6\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
0\nLINE\n8\nA\n10\n6589000\n20\n540000\n11\n6589100\n21\n540050\n\
0\nPOINT\n8\nA\n10\n6589050.25\n20\n540025.5\n30\n12.5\n0\nENDSEC\n0\nEOF\n";
    let src = Source {
        name: "s".into(),
        data: text.as_bytes().to_vec(),
        extra: Default::default(),
    };
    let r = DxfFormat
        .import(
            &src,
            &Context::of(&Document::with_sequential_ids()),
            &ImportOptions::default(),
        )
        .unwrap();
    let mut ed = Editor::new(Document::with_sequential_ids());
    ed.execute(&r.command).unwrap();
    let doc = ed.doc().clone();
    assert!(doc.georef.is_some());
    // Model coordinates are small.
    for o in doc.objects.values() {
        let (lo, hi) = o.shape.bounds();
        assert!(lo.abs().max_element() < 1.0e6 && hi.abs().max_element() < 1.0e6);
    }
    // Default export: real-world numbers in meters. `document`: model numbers.
    let meters = ExportOptions {
        unit: Unit::M,
        ..ExportOptions::default()
    };
    let out = String::from_utf8(DxfFormat.export(&doc, &meters).unwrap().bytes).unwrap();
    for v in [
        "6589000",
        "540050",
        "6589100",
        "6589050.25",
        "540025.5",
        "12.5",
    ] {
        assert!(
            out.lines()
                .any(|l| l
                    .trim()
                    .parse::<f64>()
                    .is_ok_and(|x| (x - v.parse::<f64>().unwrap()).abs() < 1e-6)),
            "{v}"
        );
    }
    let local = ExportOptions {
        unit: Unit::M,
        origin: cadektor_io::ExportOrigin::Document,
        ..ExportOptions::default()
    };
    let out = String::from_utf8(DxfFormat.export(&doc, &local).unwrap().bytes).unwrap();
    assert!(!out.lines().any(|l| l.trim() == "6589000"));
    // Import the export again: the same georeference and the same shapes.
    let again = DxfFormat.export(&doc, &meters).unwrap().bytes;
    let src = Source {
        name: "s".into(),
        data: again,
        extra: Default::default(),
    };
    let r = DxfFormat
        .import(
            &src,
            &Context::of(&Document::with_sequential_ids()),
            &ImportOptions::default(),
        )
        .unwrap();
    let mut ed2 = Editor::new(Document::with_sequential_ids());
    ed2.execute(&r.command).unwrap();
    assert_eq!(ed2.doc().georef, doc.georef);
    let back: Vec<_> = ed2
        .doc()
        .objects
        .values()
        .map(|o| o.shape.clone())
        .collect();
    let orig: Vec<_> = doc.objects.values().map(|o| o.shape.clone()).collect();
    assert_eq!(back.len(), orig.len());
    for (a, b) in back.iter().zip(&orig) {
        let (la, ha) = a.bounds();
        let (lb, hb) = b.bounds();
        assert!(la.distance(lb) < 1e-3 && ha.distance(hb) < 1e-3);
    }
}
