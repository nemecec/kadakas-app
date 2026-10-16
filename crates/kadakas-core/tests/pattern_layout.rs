//! Hardscape holes, origin and angle, the pattern summary, `from_shapes` and the object adapters
//! of the layout engine (PAT-01, PAT-04, HRD-12, HRD-13).

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{Vertex, VertexKind, corners};
use kadakas_core::hardscape::batch::{self, FromShapesOptions};
use kadakas_core::hardscape::style::{HardscapeStyle, JointKind, JointPattern};
use kadakas_core::hardscape::{self, HardscapePatch};
use kadakas_core::layout::object::{outline_of, pattern_of, region_of};
use kadakas_core::layout::summary::{SummaryOptions, summarize};
use kadakas_core::layout::{self, Bond, LayoutError, Limits, Pattern, Placement, Region, gaps};
use kadakas_core::mesh;
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};

fn rect(w: f64, h: f64) -> Vec<DVec2> {
    vec![
        DVec2::ZERO,
        DVec2::new(w, 0.0),
        DVec2::new(w, h),
        DVec2::new(0.0, h),
    ]
}

fn hole() -> Vec<DVec2> {
    vec![
        DVec2::new(1000.0, 800.0),
        DVec2::new(1600.0, 800.0),
        DVec2::new(1600.0, 1300.0),
        DVec2::new(1000.0, 1300.0),
    ]
}

fn add_style(ed: &mut Editor, kind: JointKind) -> ResourceId {
    ResourceId(
        ed.execute(&Command::AddResource {
            name: format!("P {kind:?}"),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                joints: Some(JointPattern {
                    kind,
                    unit_length: 200.0,
                    unit_width: 100.0,
                    spacing: 5.0,
                    angle: 10.0,
                    ..Default::default()
                }),
                border: Some(hardscape::Border::default()),
                ..Default::default()
            })),
        })
        .unwrap()
        .created[0],
    )
}

fn add_slab(ed: &mut Editor, patch: HardscapePatch) -> Result<ObjectId, String> {
    ed.execute(&Command::AddHardscape {
        boundary: Some(corners(&rect(4000.0, 2000.0))),
        patch: Box::new(patch),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .map(|o| ObjectId(o.created[0]))
    .map_err(|e| e.to_string())
}

fn set(ed: &mut Editor, id: ObjectId, patch: HardscapePatch) -> Result<(), String> {
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch,
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[test]
fn patch_validates_holes_origin_angle_and_edge() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let st = add_style(&mut ed, JointKind::Stack);
    let id = add_slab(
        &mut ed,
        HardscapePatch {
            style: Some(st),
            holes: Some(vec![corners(&hole())]),
            origin: Some(DVec2::new(5.0, 6.0)),
            angle: Some(15.0),
            ..Default::default()
        },
    )
    .unwrap();
    let d = hardscape::data(ed.doc().object(id).unwrap()).unwrap();
    assert_eq!(d.holes.len(), 1);
    assert_eq!(
        (d.origin, d.angle),
        (Some(DVec2::new(5.0, 6.0)), Some(15.0))
    );
    // Bad input is refused and nothing changes.
    let mut bad = |p: HardscapePatch| set(&mut ed, id, p).unwrap_err();
    assert!(
        bad(HardscapePatch {
            holes: Some(vec![corners(&[DVec2::ZERO, DVec2::X])]),
            ..Default::default()
        })
        .contains("3 or more")
    );
    assert!(
        bad(HardscapePatch {
            holes: Some(vec![corners(&[
                DVec2::new(9000.0, 0.0),
                DVec2::new(9100.0, 0.0),
                DVec2::new(9100.0, 100.0)
            ])]),
            ..Default::default()
        })
        .contains("not inside")
    );
    assert!(
        bad(HardscapePatch {
            holes: Some(vec![corners(&hole()); 1001]),
            ..Default::default()
        })
        .contains("at most")
    );
    assert!(
        bad(HardscapePatch {
            holes: Some(vec![vec![
                Vertex::corner(DVec2::new(f64::NAN, 0.0)),
                Vertex::corner(DVec2::X),
                Vertex::corner(DVec2::Y)
            ]]),
            ..Default::default()
        })
        .contains("finite")
    );
    assert!(
        bad(HardscapePatch {
            origin: Some(DVec2::new(f64::INFINITY, 0.0)),
            ..Default::default()
        })
        .contains("origin")
    );
    assert!(
        bad(HardscapePatch {
            angle: Some(f64::NAN),
            ..Default::default()
        })
        .contains("angle")
    );
    assert!(
        bad(HardscapePatch {
            align_to_edge: Some(7),
            ..Default::default()
        })
        .contains("edge 7")
    );
    // Clearing, and the edge: vertex 1 runs up (90 degrees) from (4000, 0).
    set(
        &mut ed,
        id,
        HardscapePatch {
            clear_origin: true,
            clear_angle: true,
            holes: Some(vec![]),
            ..Default::default()
        },
    )
    .unwrap();
    let d = hardscape::data(ed.doc().object(id).unwrap()).unwrap();
    assert!(d.holes.is_empty() && d.origin.is_none() && d.angle.is_none());
    set(
        &mut ed,
        id,
        HardscapePatch {
            align_to_edge: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    let d = hardscape::data(ed.doc().object(id).unwrap()).unwrap();
    // The style has a kerb border of 150 mm, so the pattern starts on the inner border line.
    assert_eq!(d.origin, Some(DVec2::new(3850.0, 150.0)));
    assert!((d.angle.unwrap() - 90.0).abs() < 1e-9);
    // On a path the edge is an edge of the outline from the center line.
    let path = ed
        .execute(&Command::AddHardscape {
            boundary: None,
            patch: Box::new(HardscapePatch {
                center: Some(corners(&[DVec2::ZERO, DVec2::new(5000.0, 0.0)])),
                width: Some(1000.0),
                style: Some(st),
                align_to_edge: Some(0),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0];
    let d = hardscape::data(ed.doc().object(ObjectId(path)).unwrap()).unwrap();
    assert!(d.origin.is_some() && d.angle.unwrap().abs() < 1e-6 || d.angle.is_some());
}

#[test]
fn holes_move_with_the_object_and_guard_the_outline() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let id = add_slab(
        &mut ed,
        HardscapePatch {
            holes: Some(vec![corners(&hole())]),
            origin: Some(DVec2::new(10.0, 20.0)),
            ..Default::default()
        },
    )
    .unwrap();
    ed.execute(&Command::MoveObjects {
        ids: vec![id],
        delta: DVec2::new(100.0, -50.0),
    })
    .unwrap();
    let d = hardscape::data(ed.doc().object(id).unwrap()).unwrap();
    assert_eq!(d.holes[0][0].p, DVec2::new(1100.0, 750.0));
    assert_eq!(d.origin, Some(DVec2::new(110.0, -30.0)));
    // Moving a corner so that the hole falls outside is refused; a small move is fine.
    let corner = |index, p| Command::SetVertex {
        id,
        index,
        kind: None,
        p: Some(p),
    };
    let err = ed
        .execute(&corner(2, DVec2::new(1200.0, 1100.0)))
        .unwrap_err();
    assert!(err.to_string().contains("holes"), "{err}");
    ed.execute(&corner(2, DVec2::new(4100.0, 2050.0))).unwrap();
    // Replacing the whole shape is checked the same way.
    let shape = Shape::Hardscape {
        boundary: corners(&rect(500.0, 500.0)),
        material: "x".into(),
        thickness: None,
        elevation: None,
    };
    let err = ed
        .execute(&Command::ModifyObject {
            id,
            shape: Some(shape),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("holes"), "{err}");
}

#[test]
fn report_svg_and_scene_leave_the_holes_open() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let st = add_style(&mut ed, JointKind::RunningBond);
    let plain = add_slab(
        &mut ed,
        HardscapePatch {
            style: Some(st),
            ..Default::default()
        },
    )
    .unwrap();
    let holed = add_slab(
        &mut ed,
        HardscapePatch {
            style: Some(st),
            holes: Some(vec![corners(&hole())]),
            ..Default::default()
        },
    )
    .unwrap();
    let q =
        |id| hardscape::report::quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
    let (a, b) = (q(plain), q(holed));
    let hole_m2 = 0.6 * 0.5;
    assert!((a.plan_area_m2 - b.plan_area_m2 - hole_m2).abs() < 1e-9);
    assert_eq!((b.holes, a.holes), (1, 0));
    assert!((b.holes_area_m2 - hole_m2).abs() < 1e-9);
    assert!((b.perimeter_m - a.perimeter_m - 2.2).abs() < 1e-6);
    assert!(b.volume_m3 < a.volume_m3);
    // The plan: the hole is drawn, the joints stay out of it, and the border follows it.
    let svg = hardscape::svg_extras(ed.doc(), ed.doc().object(holed).unwrap(), 100.0, "black");
    assert!(svg.contains(r#"data-part="hole""#) && svg.contains("evenodd"));
    assert_eq!(svg.matches(r#"data-part="border""#).count(), 2);
    let prepared = hardscape::draw::prepared(ed.doc(), ed.doc().object(holed).unwrap())
        .unwrap()
        .unwrap();
    let joints = prepared.draw(None, &hardscape::draw::DrawOptions::canvas());
    assert!(!joints.is_preview());
    let joints = joints
        .groups
        .iter()
        .flat_map(|g| g.joints.clone())
        .collect::<Vec<_>>();
    let h = hole();
    assert!(!joints.is_empty());
    for (p, q) in &joints {
        let mid = (*p + *q) / 2.0;
        assert!(
            !(mid.x > 1001.0 && mid.x < 1599.0 && mid.y > 801.0 && mid.y < 1299.0),
            "a joint crosses the hole: {mid:?} {h:?}"
        );
    }
    // The slab in 3D has more walls and fewer top triangles over the hole.
    let scene_a = mesh::build(ed.doc());
    let (lo, hi) = scene_a.object_bounds(holed).unwrap();
    assert!(hi.x > lo.x);
    let tris = |id| {
        let s = mesh::build(ed.doc());
        s.mesh.tri_object.iter().filter(|o| **o == id).count()
    };
    assert!(tris(holed) > tris(plain));
}

#[test]
fn summary_counts_options_and_errors() {
    let region = Region::from_rings(&rect(2050.0, 1050.0), &[hole()]).unwrap();
    let pattern = Pattern::new(Bond::Stack, 200.0, 100.0, 5.0);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    let s = summarize(
        &region,
        &pattern,
        &place,
        &SummaryOptions {
            gap_length: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(s.order_units, s.whole_units + s.cut_units);
    assert!(s.waste_percent.is_none() && s.order_units_with_waste.is_none());
    assert!(s.gap_length_m.unwrap() > 10.0);
    let w = summarize(
        &region,
        &pattern,
        &place,
        &SummaryOptions {
            waste_percent: Some(10.0),
            sliver_share: 0.9,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        w.order_units_with_waste,
        Some((w.order_units as f64 * 1.1).ceil() as usize)
    );
    assert!(w.slivers >= s.slivers);
    // Over the limit: an error with the estimate.
    let e = summarize(
        &region,
        &pattern,
        &place,
        &SummaryOptions {
            max_units: 10,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        matches!(e, LayoutError::TooMany(ref t) if t.estimated > 10),
        "{e:?}"
    );
    // Gap lines over a limit: none, and the length is unknown.
    let g = gaps::gap_lines(&region, &pattern, &place, 5, false);
    assert!(g.truncated && g.segments.is_empty());
    let s = summarize(
        &region,
        &pattern,
        &place,
        &SummaryOptions {
            max_gap_segments: 5,
            gap_length: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s.gap_length_m.is_none());
    // The on-screen drawing may use larger units. A fine pattern over a big region does.
    let fine = Pattern::new(Bond::Stack, 10.0, 10.0, 0.0);
    let big = Region::from_rings(&rect(400_000.0, 400_000.0), &[]).unwrap();
    let coarse = gaps::gap_lines(&big, &fine, &Placement::default(), 30_000, true);
    assert!(coarse.coarsened && !coarse.segments.is_empty());
    // Invalid pattern: nothing.
    let bad = Pattern::new(Bond::Stack, 0.0, 100.0, 5.0);
    assert_eq!(
        gaps::gap_lines(&region, &bad, &place, 100, true),
        gaps::Gaps::default()
    );
    assert!(layout::lay_units(&region, &bad, &place, &Limits::new(100)).is_err());
    assert!(bad.validate().is_err());
    assert!(
        Pattern::new(Bond::Stack, 200.0, 100.0, -1.0)
            .validate()
            .is_err()
    );
}

#[test]
fn bonds_have_their_unit_areas_and_all_fill_a_region() {
    let region = Region::from_rings(&rect(3000.0, 2000.0), &[]).unwrap();
    for bond in Bond::ALL {
        let p = Pattern::new(bond, 200.0, 100.0, 0.0);
        let (units, _) =
            layout::lay_units(&region, &p, &Placement::default(), &Limits::new(100_000)).unwrap();
        let sum: f64 = units.iter().map(|u| u.area).sum();
        assert!((sum - 6.0e6).abs() < 1e-3, "{bond:?}");
        assert!((p.full_unit_area() - 20_000.0).abs() < 1e-9, "{bond:?}");
        assert_eq!(Bond::from_name(bond.id()), Some(bond));
    }
    assert_eq!(Bond::from_name("Basket-Weave"), Some(Bond::BasketWeave));
    assert_eq!(Bond::from_name("zigzag"), None);
    // The unit keeps its size in a herringbone.
    let h = Pattern::new(Bond::Herringbone, 200.0, 100.0, 5.0);
    assert!(
        (h.full_unit_area() - 200.0 * 100.0).abs() < 1e-9,
        "the unit keeps its size"
    );
}

#[test]
fn region_errors_and_normal_forms() {
    assert!(Region::from_rings(&[DVec2::ZERO, DVec2::X], &[]).is_err());
    assert!(Region::from_rings(&[DVec2::ZERO, DVec2::X, DVec2::new(f64::NAN, 1.0)], &[]).is_err());
    assert!(Region::from_rings(&rect(10.0, 10.0), &[rect(20.0, 20.0)]).is_err());
    // Overlapping holes merge.
    let r = Region::from_rings(
        &rect(1000.0, 1000.0),
        &[
            vec![
                DVec2::new(100.0, 100.0),
                DVec2::new(500.0, 100.0),
                DVec2::new(500.0, 500.0),
                DVec2::new(100.0, 500.0),
            ],
            vec![
                DVec2::new(300.0, 300.0),
                DVec2::new(700.0, 300.0),
                DVec2::new(700.0, 700.0),
                DVec2::new(300.0, 700.0),
            ],
        ],
    )
    .unwrap();
    assert!((r.area - (1.0e6 - 160_000.0 * 2.0 + 40_000.0)).abs() < 1e-3);
    // A curved outline within the tolerance.
    let round = vec![
        Vertex::corner(DVec2::new(-1000.0, 0.0)),
        Vertex::new(DVec2::new(1000.0, 0.0), VertexKind::PointOnArc),
        Vertex::corner(DVec2::new(0.0, 1000.0)),
    ];
    let r = Region::new(&round, &[], 1.0).unwrap();
    assert!(r.area > 1.0e6);
    assert!(Region::new(&corners(&rect(10.0, 10.0)), &[], 1.0).is_ok());
    let many = vec![Vertex::corner(DVec2::ZERO); layout::MAX_VERTICES + 1];
    assert!(Region::new(&many, &[], 1.0).is_err());
    // Placement from an edge.
    let outline = corners(&rect(100.0, 50.0));
    let p = Placement::along_edge(&outline, 3).unwrap();
    assert_eq!(p.origin, Some(DVec2::new(0.0, 50.0)));
    assert!((p.angle + 90.0).abs() < 1e-9);
    assert!(Placement::along_edge(&corners(&[DVec2::ZERO, DVec2::ZERO, DVec2::X]), 0).is_err());
    assert!(Placement::along_edge(&outline, 4).is_err());
}

#[test]
fn objects_work_as_regions() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let mut add = |shape: Shape| {
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
    };
    let poly = add(Shape::Polyline {
        points: corners(&rect(1000.0, 500.0)),
        closed: true,
    });
    let open = add(Shape::Polyline {
        points: corners(&rect(1000.0, 500.0)),
        closed: false,
    });
    let r = add(Shape::Rect {
        min: DVec2::new(10.0, 10.0),
        max: DVec2::new(0.0, 0.0),
    });
    let c = add(Shape::Circle {
        center: DVec2::ZERO,
        radius: 1000.0,
    });
    let line = add(Shape::Line {
        a: DVec2::ZERO,
        b: DVec2::X,
    });
    let doc = ed.doc();
    let area = |id| region_of(doc, doc.object(id).unwrap(), 1.0).unwrap().area;
    assert!((area(poly) - 500_000.0).abs() < 1e-6);
    assert!((area(r) - 100.0).abs() < 1e-6);
    assert!((area(c) - std::f64::consts::PI * 1.0e6).abs() / 1.0e6 < 0.001);
    assert!(
        outline_of(doc.object(open).unwrap())
            .unwrap_err()
            .contains("open")
    );
    assert!(
        outline_of(doc.object(line).unwrap())
            .unwrap_err()
            .contains("cannot be filled")
    );
    assert!(pattern_of(doc, doc.object(poly).unwrap()).is_none());
}

#[test]
fn from_shapes_plans_nested_outlines() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let st = add_style(&mut ed, JointKind::Stack);
    let mut add = |pts: Vec<DVec2>, closed: bool| {
        ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Polyline {
                    points: corners(&pts),
                    closed,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        )
    };
    let outer = add(rect(10_000.0, 8000.0), true);
    let island = add(
        vec![
            DVec2::new(2000.0, 2000.0),
            DVec2::new(4000.0, 2000.0),
            DVec2::new(4000.0, 4000.0),
        ],
        true,
    );
    let open = add(rect(10.0, 10.0), false);
    let ghost = ObjectId::from_num(999);
    let opts = FromShapesOptions {
        style: Some(st),
        delete_source: true,
        ..Default::default()
    };
    let made = batch::from_shapes(ed.doc(), &[outer, island, island, open, ghost], &opts);
    assert_eq!(made.created.len(), 1);
    assert_eq!(made.created[0].2, 1);
    assert_eq!(made.holes, vec![(island, outer)]);
    assert_eq!(made.skipped.len(), 2);
    ed.execute(&made.command.unwrap()).unwrap();
    let new = made.created[0].1;
    let d = hardscape::data(ed.doc().object(new).unwrap()).unwrap();
    assert_eq!((d.holes.len(), d.style), (1, Some(st)));
    assert!(ed.doc().object(outer).is_none() && ed.doc().object(island).is_none());
    // Without nesting, and keeping the sources.
    let mut ed2 = Editor::new(Document::with_sequential_ids());
    let a = ObjectId(
        ed2.execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(100.0, 100.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let made = batch::from_shapes(
        ed2.doc(),
        &[a],
        &FromShapesOptions {
            holes_from_inner: false,
            delete_source: false,
            ..Default::default()
        },
    );
    ed2.execute(&made.command.unwrap()).unwrap();
    assert!(ed2.doc().object(a).is_some());
    // A hardscape and nothing at all give no command.
    let h = made.created[0].1;
    let made = batch::from_shapes(ed2.doc(), &[h], &FromShapesOptions::default());
    assert!(made.command.is_none() && made.skipped.len() == 1);
    // Too many holes: the extra shapes stay hardscapes of their own, with a message.
    let mut ed3 = Editor::new(Document::with_sequential_ids());
    let mut ids = vec![ObjectId(
        ed3.execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(100_000.0, 100_000.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )];
    for i in 0..(hardscape::data::MAX_HOLES + 2) {
        let x = 10.0 + (i % 40) as f64 * 2000.0;
        let y = 10.0 + (i / 40) as f64 * 2000.0;
        ids.push(ObjectId(
            ed3.execute(&Command::AddObject {
                shape: Shape::Rect {
                    min: DVec2::new(x, y),
                    max: DVec2::new(x + 500.0, y + 500.0),
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        ));
    }
    let made = batch::from_shapes(ed3.doc(), &ids, &FromShapesOptions::default());
    assert_eq!(made.holes.len(), hardscape::data::MAX_HOLES);
    assert_eq!(made.skipped.len(), 2);
    assert_eq!(made.created.len(), 3);
    ed3.execute(&made.command.unwrap()).unwrap();
}

#[test]
fn unit_polygons_carry_holes_and_dust_is_counted() {
    // A hole inside a unit gives a ring that is a hole; a tiny sliver is dust.
    let region = Region::from_rings(
        &rect(400.0, 200.0),
        &[vec![
            DVec2::new(50.0, 20.0),
            DVec2::new(100.0, 20.0),
            DVec2::new(100.0, 70.0),
            DVec2::new(50.0, 70.0),
        ]],
    )
    .unwrap();
    let p = Pattern::new(Bond::Stack, 200.0, 100.0, 0.0);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    let (units, stats) = layout::lay_units(&region, &p, &place, &Limits::new(100)).unwrap();
    assert_eq!(units.len(), 4);
    assert!(units.iter().any(|u| u.rings.len() == 2 && u.rings[1].hole));
    assert_eq!(stats.dust_pieces, 0);
    // The region edge 0.5 mm inside a unit cuts a sliver of 0.5 x 100 mm = 50 mm2: a unit, not dust.
    let thin = Region::from_rings(&rect(400.5, 200.0), &[]).unwrap();
    let (units, _) = layout::lay_units(&thin, &p, &place, &Limits::new(100)).unwrap();
    assert_eq!(units.len(), 6);
    // 0.005 mm wide: 0.5 mm2, dust.
    let dust = Region::from_rings(&rect(400.005, 200.0), &[]).unwrap();
    let (units, stats) = layout::lay_units(&dust, &p, &place, &Limits::new(100)).unwrap();
    assert_eq!(units.len(), 4);
    assert_eq!(stats.dust_pieces, 2);
    // The tally.
    let mut t = layout::Tally::default();
    for u in &units {
        t.add(u, 0.25, &layout::units::Unfillable::default());
    }
    assert_eq!((t.whole(), t.cut()), (4, 0));
}

fn add_poly(ed: &mut Editor, pts: &[DVec2]) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Polyline {
                points: corners(pts),
                closed: true,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

#[test]
fn an_island_that_touches_or_crosses_the_outline_by_noise_is_still_a_hole() {
    for (name, island) in [
        // On the kerb line: the island shares an edge with the lot.
        (
            "touching",
            vec![
                DVec2::new(0.0, 500.0),
                DVec2::new(800.0, 500.0),
                DVec2::new(800.0, 900.0),
                DVec2::new(0.0, 900.0),
            ],
        ),
        // A vertex 0.01 mm outside after a snap.
        (
            "noise",
            vec![
                DVec2::new(-0.01, 500.0),
                DVec2::new(800.0, 500.0),
                DVec2::new(800.0, 900.0),
                DVec2::new(-0.01, 900.0),
            ],
        ),
    ] {
        let mut ed = Editor::new(Document::with_sequential_ids());
        let lot = add_poly(&mut ed, &rect(4000.0, 2000.0));
        let isl = add_poly(&mut ed, &island);
        let opts = FromShapesOptions {
            style: Some(add_style(&mut ed, JointKind::Stack)),
            ..Default::default()
        };
        let made = batch::from_shapes(ed.doc(), &[lot, isl], &opts);
        assert_eq!(made.created.len(), 1, "{name}: {:?}", made.skipped);
        assert_eq!(made.holes.len(), 1, "{name}");
        ed.execute(&made.command.unwrap()).unwrap();
        let rows = hardscape::report::report(ed.doc(), &kadakas_core::ground::NoTerrain).rows;
        assert_eq!(rows.len(), 1, "{name}: one hardscape, not two");
        let area = rows[0].quantities.plan_area_m2;
        assert!((area - (8.0 - 0.8 * 0.4)).abs() < 1e-3, "{name}: {area}");
    }
}

#[test]
fn a_shape_that_overlaps_in_part_is_left_out_and_named() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let lot = add_poly(&mut ed, &rect(4000.0, 2000.0));
    // Half in, half out.
    let half = add_poly(
        &mut ed,
        &[
            DVec2::new(3500.0, 500.0),
            DVec2::new(4500.0, 500.0),
            DVec2::new(4500.0, 900.0),
            DVec2::new(3500.0, 900.0),
        ],
    );
    // A clean neighbour.
    let near = add_poly(
        &mut ed,
        &[
            DVec2::new(8000.0, 0.0),
            DVec2::new(9000.0, 0.0),
            DVec2::new(9000.0, 500.0),
            DVec2::new(8000.0, 500.0),
        ],
    );
    let made = batch::from_shapes(ed.doc(), &[lot, half, near], &FromShapesOptions::default());
    let skipped: Vec<ObjectId> = made.skipped.iter().map(|s| s.0).collect();
    assert_eq!(skipped, vec![half], "{:?}", made.skipped);
    assert!(made.skipped[0].1.contains("overlaps"));
    assert_eq!(made.created.len(), 2);
    ed.execute(&made.command.unwrap()).unwrap();
    let rows = hardscape::report::report(ed.doc(), &kadakas_core::ground::NoTerrain).rows;
    let total: f64 = rows.iter().map(|r| r.quantities.plan_area_m2).sum();
    assert!(
        (total - (8.0 + 0.5)).abs() < 1e-6,
        "no area counted twice: {total}"
    );
    // Without holes_from_inner two lots that overlap in part are also not both made.
    let made = batch::from_shapes(
        ed.doc(),
        &[lot, half],
        &FromShapesOptions {
            holes_from_inner: false,
            ..Default::default()
        },
    );
    assert!(made.created.len() <= 1);
}

#[test]
fn an_island_on_an_arc_lot_works_and_a_closing_point_is_dropped() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    // A lot with an arc side: a half disc.
    let lot = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Polyline {
                points: vec![
                    Vertex::corner(DVec2::new(-2000.0, 0.0)),
                    Vertex::new(DVec2::new(0.0, 2000.0), VertexKind::PointOnArc),
                    Vertex::corner(DVec2::new(2000.0, 0.0)),
                ],
                closed: true,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    // An island on the flat side, closed with a repeated first point.
    let isl = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Polyline {
                points: corners(&[
                    DVec2::new(-300.0, 0.0),
                    DVec2::new(300.0, 0.0),
                    DVec2::new(300.0, 400.0),
                    DVec2::new(-300.0, 400.0),
                    DVec2::new(-300.0, 0.0),
                ]),
                closed: true,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let made = batch::from_shapes(ed.doc(), &[lot, isl], &FromShapesOptions::default());
    assert_eq!(made.holes.len(), 1, "{:?}", made.skipped);
    ed.execute(&made.command.unwrap()).unwrap();
    let h = made.created[0].1;
    let d = hardscape::data(ed.doc().object(h).unwrap()).unwrap();
    assert_eq!(d.holes[0].len(), 4, "the repeated point is not a corner");
    let o = ed.doc().object(h).unwrap();
    let region = layout::object::region_of(ed.doc(), o, 1.0).unwrap();
    let want = std::f64::consts::PI * 2000.0 * 2000.0 / 2.0 - 600.0 * 400.0;
    assert!((region.area - want).abs() / want < 0.001);
}

#[test]
fn the_kerb_border_is_not_paved() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let st = add_style(&mut ed, JointKind::Stack); // has a border of 150 mm
    let id = add_slab(
        &mut ed,
        HardscapePatch {
            style: Some(st),
            holes: Some(vec![corners(&hole())]),
            ..Default::default()
        },
    )
    .unwrap();
    let o = ed.doc().object(id).unwrap();
    let region = layout::object::region_of(ed.doc(), o, 1.0).unwrap();
    // 4000 x 2000 less 150 mm all round, less the hole grown by 150 mm all round.
    let want = (4000.0 - 300.0) * (2000.0 - 300.0) - (600.0 + 300.0) * (500.0 + 300.0);
    assert!(
        (region.area - want).abs() < 1e-3,
        "{} vs {want}",
        region.area
    );
    let (pattern, place) = layout::object::pattern_of(ed.doc(), o).unwrap();
    let s = summarize(&region, &pattern, &place, &SummaryOptions::default()).unwrap();
    assert!((s.area_laid_m2 + s.gap_area_m2 - region.area / 1e6).abs() < 1e-9);
    // align_to_edge starts the pattern on the inner border line, not on the outline.
    let p = layout::object::align_placement(ed.doc(), o, 0).unwrap();
    assert!(
        (p.origin.unwrap() - DVec2::new(150.0, 150.0)).length() < 1e-9,
        "{:?}",
        p.origin
    );
    set(
        &mut ed,
        id,
        HardscapePatch {
            align_to_edge: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    let d = hardscape::data(ed.doc().object(id).unwrap()).unwrap();
    assert!(
        (d.origin.unwrap() - DVec2::new(3850.0, 1850.0)).length() < 1e-9,
        "{:?}",
        d.origin
    );
    // A path loses the border only on the sides that have it.
    let path = ed
        .execute(&Command::AddHardscape {
            boundary: None,
            patch: Box::new(HardscapePatch {
                center: Some(corners(&[DVec2::ZERO, DVec2::new(5000.0, 0.0)])),
                width: Some(1000.0),
                style: Some(st),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0];
    let o = ed.doc().object(ObjectId(path)).unwrap();
    let region = layout::object::region_of(ed.doc(), o, 1.0).unwrap();
    assert!(
        (region.area - 5000.0 * 700.0).abs() < 1e-3,
        "{}",
        region.area
    );
}

#[test]
fn holes_are_checked_by_area_merged_and_repaired() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    // A concave outline: an L. A hole across the notch must be refused, though its corners are
    // inside or on the outline.
    let l = vec![
        DVec2::new(0.0, 0.0),
        DVec2::new(3000.0, 0.0),
        DVec2::new(3000.0, 1000.0),
        DVec2::new(1000.0, 1000.0),
        DVec2::new(1000.0, 3000.0),
        DVec2::new(0.0, 3000.0),
    ];
    let id = ObjectId(
        ed.execute(&Command::AddHardscape {
            boundary: Some(corners(&l)),
            patch: Box::new(HardscapePatch::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let across_notch = vec![
        DVec2::new(800.0, 800.0),
        DVec2::new(1600.0, 800.0),
        DVec2::new(1600.0, 1600.0),
        DVec2::new(800.0, 1600.0),
    ];
    let e = set(
        &mut ed,
        id,
        HardscapePatch {
            holes: Some(vec![corners(&across_notch)]),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(e.contains("not inside"), "{e}");
    // A hole that leaves the outline by 0.01 mm is fine.
    let noisy = vec![
        DVec2::new(-0.01, 200.0),
        DVec2::new(500.0, 200.0),
        DVec2::new(500.0, 600.0),
        DVec2::new(-0.01, 600.0),
    ];
    set(
        &mut ed,
        id,
        HardscapePatch {
            holes: Some(vec![corners(&noisy)]),
            ..Default::default()
        },
    )
    .unwrap();
    // Overlapping holes are merged for the perimeter and the 3D slab.
    let a = vec![
        DVec2::new(1500.0, 200.0),
        DVec2::new(2000.0, 200.0),
        DVec2::new(2000.0, 600.0),
        DVec2::new(1500.0, 600.0),
    ];
    let b = vec![
        DVec2::new(1800.0, 300.0),
        DVec2::new(2300.0, 300.0),
        DVec2::new(2300.0, 700.0),
        DVec2::new(1800.0, 700.0),
    ];
    set(
        &mut ed,
        id,
        HardscapePatch {
            holes: Some(vec![corners(&a), corners(&b)]),
            ..Default::default()
        },
    )
    .unwrap();
    let merged =
        hardscape::data::merged_holes(&corners(&l), &[corners(&a), corners(&b)], 1.0).unwrap();
    assert_eq!(merged.len(), 1);
    let q = hardscape::report::quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
    // The union of the two holes: 500 x 400 + 500 x 400 less the 200 x 300 overlap.
    assert!((q.holes_area_m2 - (0.2 + 0.2 - 0.06)).abs() < 1e-9);
    let outline = 3000.0 + 1000.0 + 2000.0 + 2000.0 + 1000.0 + 3000.0;
    // The union is two 500 x 400 rectangles that share a 200 x 300 part: its edge is the sum of
    // both perimeters less the parts inside each other.
    let union_perimeter = 2.0 * (500.0 + 400.0) * 2.0 - 4.0 * 300.0 + 0.0 - 4.0 * 0.0;
    let _ = union_perimeter;
    let union_edge = 2.0 * (500.0 + 400.0) + 2.0 * (500.0 + 400.0) - 2.0 * 200.0 - 2.0 * 300.0;
    assert!(
        (q.perimeter_m * 1000.0 - outline - union_edge).abs() < 1e-3,
        "{}",
        q.perimeter_m
    );
    // A merge that left a hole outside the outline is cut to it.
    let mut o = ed.doc().object(id).unwrap().clone();
    let mut d = hardscape::data(&o).unwrap();
    d.holes = vec![corners(&[
        DVec2::new(2000.0, 500.0),
        DVec2::new(4000.0, 500.0),
        DVec2::new(4000.0, 900.0),
        DVec2::new(2000.0, 900.0),
    ])];
    hardscape::data::set_data(&mut o, &d);
    let note = hardscape::data::repair_holes(&mut o).unwrap();
    assert!(note.contains("cut to it"));
    let d = hardscape::data(&o).unwrap();
    assert!(
        d.holes
            .iter()
            .all(|h| h.iter().all(|v| v.p.x <= 3000.0 + 1e-6))
    );
    assert!(hardscape::data::repair_holes(&mut o).is_none());
}

#[test]
fn a_path_with_a_border_starts_on_the_inner_border_line() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let st = add_style(&mut ed, JointKind::Stack); // a border of 150 mm on both sides
    let path = ObjectId(
        ed.execute(&Command::AddHardscape {
            boundary: None,
            patch: Box::new(HardscapePatch {
                center: Some(corners(&[DVec2::ZERO, DVec2::new(5000.0, 0.0)])),
                width: Some(1000.0),
                style: Some(st),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let o = ed.doc().object(path).unwrap();
    let outline = hardscape::data(o).unwrap().outline();
    let pts = kadakas_core::curve::positions(&outline);
    // Every long edge of the outline has the border, the ends do not.
    for i in 0..pts.len() {
        let p = layout::object::align_placement(ed.doc(), o, i).unwrap();
        let start = p.origin.unwrap();
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let along_x = (b - a).y.abs() < 1e-9;
        if along_x {
            // On the outline's long side: moved 150 mm in, across the path.
            assert!((start.y.abs() - 350.0).abs() < 1e-6, "edge {i}: {start:?}");
        } else {
            // An end: not moved along the path, but the corner is inside the border of the long side.
            assert!(start.y.abs() <= 500.0 + 1e-6);
        }
    }
    // A border on the left only: the right edge keeps the outline.
    let mut style = match &ed.doc().resources.get(st).unwrap().body {
        ResourceBody::HardscapeStyle(s) => (**s).clone(),
        _ => panic!(),
    };
    style.border.as_mut().unwrap().sides = hardscape::Sides::Left;
    ed.execute(&Command::ModifyResource {
        id: st,
        name: None,
        body: Some(ResourceBody::HardscapeStyle(Box::new(style))),
    })
    .unwrap();
    let o = ed.doc().object(path).unwrap();
    let ys: Vec<f64> = (0..pts.len())
        .filter(|i| (pts[(i + 1) % pts.len()] - pts[*i]).y.abs() < 1e-9)
        .map(|i| {
            layout::object::align_placement(ed.doc(), o, i)
                .unwrap()
                .origin
                .unwrap()
                .y
        })
        .collect();
    assert!(ys.iter().any(|y| (y.abs() - 500.0).abs() < 1e-6), "{ys:?}");
    assert!(ys.iter().any(|y| (y.abs() - 350.0).abs() < 1e-6), "{ys:?}");
}

#[test]
fn the_inner_corner_next_to_an_arc_uses_the_tangent() {
    // A half disc: the corner at (2000, 0) joins the flat side and the arc. The arc leaves
    // upward (tangent), the flat side comes in along x. The corner moved in by 100 mm on both
    // edges is at (1900, 100) only when the arc is taken as a tangent, not as the chord.
    let outline = vec![
        Vertex::corner(DVec2::new(-2000.0, 0.0)),
        Vertex::corner(DVec2::new(2000.0, 0.0)),
        Vertex::new(DVec2::new(0.0, 2000.0), VertexKind::PointOnArc),
    ];
    let c = hardscape::data::inset_corner(&outline, 1, 100.0, 100.0);
    assert!((c - DVec2::new(1900.0, 100.0)).length() < 1e-6, "{c:?}");
    // No border on the edge before: only the arc moves.
    let c = hardscape::data::inset_corner(&outline, 1, 0.0, 100.0);
    assert!((c.x - 1900.0).abs() < 1e-6 && c.y.abs() < 1e-6, "{c:?}");
    assert_eq!(
        hardscape::data::inset_corner(&outline, 1, 0.0, 0.0),
        DVec2::new(2000.0, 0.0)
    );
}

#[test]
fn a_bar_across_a_notch_is_a_part_overlap_and_not_a_hole() {
    // An island that leaves the lot at both ends of a notch: two pieces stay inside.
    let mut ed = Editor::new(Document::with_sequential_ids());
    let lot = add_poly(
        &mut ed,
        &[
            DVec2::new(0.0, 0.0),
            DVec2::new(4000.0, 0.0),
            DVec2::new(4000.0, 2000.0),
            DVec2::new(2200.0, 2000.0),
            DVec2::new(2200.0, 1000.0),
            DVec2::new(1800.0, 1000.0),
            DVec2::new(1800.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ],
    );
    // A bar across the notch that is inside the lot at both sides of the notch, 0.02 mm past the
    // outline at the top (noise, so it counts as inside) and crossing the notch.
    let bar = add_poly(
        &mut ed,
        &[
            DVec2::new(1000.0, 500.0),
            DVec2::new(3000.0, 500.0),
            DVec2::new(3000.0, 1500.0),
            DVec2::new(1000.0, 1500.0),
        ],
    );
    let made = batch::from_shapes(ed.doc(), &[lot, bar], &FromShapesOptions::default());
    // The bar leaves the notch part of the lot: it is a part overlap, not a hole.
    assert_eq!(made.holes.len(), 0, "{:?}", made.holes);
    assert!(
        made.skipped
            .iter()
            .any(|s| s.0 == bar && s.1.contains("overlaps"))
    );
}
