#![allow(clippy::field_reassign_with_default)]
//! The groups of a hardscape with a plan: zone settings, notes, followed lines, the counts of all
//! groups added up, and the refusals (PAT-07, PAT-08).

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{Vertex, VertexKind, corners};
use kadakas_core::hardscape::HardscapePatch;
use kadakas_core::hardscape::style::{Border, HardscapeStyle, JointKind, JointPattern};
use kadakas_core::layout::check::{CheckOptions, check};
use kadakas_core::layout::course::{Corner, Side};
use kadakas_core::layout::object::region_of;
use kadakas_core::layout::plan::{
    self, Body, CourseDef, Group, Plan, Role, SplitLine, ZoneSpec, groups_of, merge,
};
use kadakas_core::layout::summary::SummaryOptions;
use kadakas_core::layout::{Bond, LayoutError, Limits, MAX_UNITS, Pattern};
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn vx(x: f64, y: f64) -> Vertex {
    Vertex::corner(v(x, y))
}

fn slab(border: bool) -> (Editor, ObjectId) {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Pavers".into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                joints: Some(JointPattern {
                    kind: JointKind::RunningBond,
                    unit_length: 200.0,
                    unit_width: 100.0,
                    spacing: 5.0,
                    ..Default::default()
                }),
                border: border.then(|| Border {
                    width: 150.0,
                    ..Default::default()
                }),
                ..Default::default()
            })),
        })
        .unwrap()
        .created[0],
    );
    let id = ObjectId(
        ed.execute(&Command::AddHardscape {
            boundary: Some(corners(&[
                v(0.0, 0.0),
                v(8000.0, 0.0),
                v(8000.0, 4000.0),
                v(0.0, 4000.0),
            ])),
            patch: Box::new(HardscapePatch {
                style: Some(style),
                origin: Some(v(0.0, 0.0)),
                angle: Some(0.0),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    (ed, id)
}

fn set_plan(ed: &mut Editor, id: ObjectId, p: &Plan) {
    ed.execute(&Command::SetData {
        id,
        set: [(plan::KEY.to_string(), serde_json::to_value(p).unwrap())]
            .into_iter()
            .collect(),
        remove: vec![],
    })
    .unwrap();
}

fn vline(x: f64) -> SplitLine {
    SplitLine {
        source: None,
        points: vec![vx(x, -50.0), vx(x, 4050.0)],
    }
}

fn planned(ed: &Editor, id: ObjectId) -> Result<plan::Planned, String> {
    let doc = ed.doc();
    groups_of(doc, doc.object(id).unwrap(), 1.0, MAX_UNITS).map(|p| p.unwrap())
}

#[test]
fn zone_settings_are_tied_by_seed_and_change_the_pattern() {
    let (mut ed, id) = slab(false);
    let mut p = Plan::default();
    p.splits = vec![vline(3000.0), vline(6000.0)];
    p.zones = vec![
        ZoneSpec {
            name: Some("Stalls".into()),
            seed: v(1000.0, 1000.0),
            pattern: Some(Pattern::new(Bond::Stack, 300.0, 150.0, 4.0)),
            origin: Some(v(10.0, 20.0)),
            angle: Some(90.0),
            ..Default::default()
        },
        ZoneSpec {
            name: Some("Aisle".into()),
            seed: v(4500.0, 1000.0),
            align_to_edge: Some(0),
            ..Default::default()
        },
        ZoneSpec {
            seed: v(7000.0, 1000.0),
            align_to_split: Some(1),
            ..Default::default()
        },
        // Two settings for one zone, and one in no zone: notes.
        ZoneSpec {
            seed: v(1100.0, 1100.0),
            ..Default::default()
        },
        ZoneSpec {
            name: Some("Lost".into()),
            seed: v(9000.0, 9000.0),
            ..Default::default()
        },
    ];
    set_plan(&mut ed, id, &p);
    let r = planned(&ed, id).unwrap();
    assert_eq!(r.groups.len(), 3);
    assert_eq!(r.zones.len(), 3);
    let names: Vec<&str> = r.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Stalls", "Aisle", "zone 3"]);
    assert!(r.groups.iter().all(|g| g.role == Role::Zone));
    assert_eq!(r.groups[0].slug, "ZONE-STALLS");
    let Body::Pattern { pattern, place } = &r.groups[0].body else {
        panic!("a pattern")
    };
    assert_eq!(pattern.bond, Bond::Stack);
    assert_eq!((place.origin, place.angle), (Some(v(10.0, 20.0)), 90.0));
    // Aligned to the edge: starts at a vertex of the zone, along its first edge.
    let Body::Pattern { place, .. } = &r.groups[1].body else {
        panic!("a pattern")
    };
    assert!(place.origin.is_some());
    // Aligned to a split line: its first point and the direction of its first segment.
    let Body::Pattern { place, .. } = &r.groups[2].body else {
        panic!("a pattern")
    };
    assert_eq!(place.origin, Some(v(6000.0, -50.0)));
    assert!((place.angle - 90.0).abs() < 1e-9);
    assert_eq!(r.notes.len(), 2, "{:?}", r.notes);
    assert!(r.notes.iter().any(|n| n.contains("both tied")));
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("Lost") && n.contains("no zone"))
    );
    assert!(r.zones[0].has_setting && !r.zones[2].has_setting || r.zones[2].has_setting);
    // The counts of the groups add up in the merge, and a pattern of its own is "mixed".
    let opts = SummaryOptions {
        waste_percent: Some(10.0),
        ..Default::default()
    };
    let parts: Vec<_> = r
        .groups
        .iter()
        .map(|g| g.summarize(&opts).unwrap())
        .collect();
    let total = merge(&parts, 8000.0 * 4000.0, &opts);
    assert_eq!(total.bond, "mixed");
    assert_eq!(
        total.whole_units,
        parts.iter().map(|s| s.whole_units).sum::<usize>()
    );
    assert_eq!(
        total.cut_units,
        parts.iter().map(|s| s.cut_units).sum::<usize>()
    );
    assert_eq!(total.by_unit.len(), 2);
    assert!(total.unit_length_mm.is_none());
    assert!(total.order_units_with_waste.unwrap() >= total.order_units);
    assert!((total.region_area_m2 - 32.0).abs() < 1e-9);
    // Gap length is added when every part has it.
    let with_gaps = SummaryOptions {
        gap_length: true,
        ..Default::default()
    };
    let parts: Vec<_> = r
        .groups
        .iter()
        .map(|g| g.summarize(&with_gaps).unwrap())
        .collect();
    assert!(merge(&parts, 32e6, &with_gaps).gap_length_m.is_some());
}

#[test]
fn bad_alignments_and_too_many_zones_are_refused() {
    let (mut ed, id) = slab(false);
    let mut p = Plan::default();
    p.splits = vec![vline(4000.0)];
    p.zones = vec![ZoneSpec {
        seed: v(1000.0, 1000.0),
        align_to_edge: Some(40),
        ..Default::default()
    }];
    set_plan(&mut ed, id, &p);
    assert!(planned(&ed, id).unwrap_err().contains("zone 'zone 1'"));
    // A pattern that cannot be laid.
    p.zones[0].align_to_edge = None;
    p.zones[0].pattern = Some(Pattern::new(Bond::Stack, 300.0, 150.0, 4.0));
    p.zones[0].pattern.as_mut().unwrap().unit_length = 5.0;
    set_plan(&mut ed, id, &p);
    assert!(planned(&ed, id).unwrap_err().contains("at least"));
    // More zones than the limit.
    let mut p = Plan::default();
    for k in 1..40 {
        p.splits.push(vline(k as f64 * 200.0));
        p.splits.push(SplitLine {
            source: None,
            points: vec![vx(-50.0, k as f64 * 100.0), vx(8050.0, k as f64 * 100.0)],
        });
    }
    set_plan(&mut ed, id, &p);
    let e = planned(&ed, id).unwrap_err();
    assert!(e.contains("more than the limit of 1000"), "{e}");
    // A plan that cannot be read is an error that names the way out.
    ed.execute(&Command::SetData {
        id,
        set: [(plan::KEY.to_string(), serde_json::json!({"splits": 5}))]
            .into_iter()
            .collect(),
        remove: vec![],
    })
    .unwrap();
    let o = ed.doc().object(id).unwrap();
    assert!(plan::has_unreadable_data(o));
    assert!(plan::data(o).is_none());
    // ... and is not ignored: the layout says so.
    assert!(planned(&ed, id).unwrap_err().contains("cannot be read"));
}

#[test]
fn followed_lines_notes_and_courses_that_take_everything() {
    let (mut ed, id) = slab(true);
    // A drawn line that is followed, one that is gone, one that is not a line.
    let line = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Line {
                a: v(4000.0, -100.0),
                b: v(4000.0, 4100.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let circle = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Circle {
                center: v(0.0, 0.0),
                radius: 5.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let mut p = Plan::default();
    p.splits = vec![
        SplitLine {
            source: Some(line),
            points: vec![],
        },
        SplitLine {
            source: Some(ObjectId::from_num(9999)),
            points: vec![vx(2000.0, -50.0), vx(2000.0, 4050.0)],
        },
        SplitLine {
            source: Some(circle),
            points: vec![vx(6000.0, -50.0), vx(6000.0, 4050.0)],
        },
        // A closed polyline as a line, and a line that crosses nothing.
        SplitLine {
            source: None,
            points: vec![
                vx(100.0, 100.0),
                vx(300.0, 100.0),
                vx(300.0, 300.0),
                vx(100.0, 300.0),
            ],
        },
        SplitLine {
            source: None,
            points: vec![vx(20_000.0, 0.0), vx(20_000.0, 100.0)],
        },
        // Stops short.
        SplitLine {
            source: None,
            points: vec![vx(7000.0, 0.0), vx(7000.0, 3000.0)],
        },
    ];
    set_plan(&mut ed, id, &p);
    let r = planned(&ed, id).unwrap();
    assert!(r.zones.len() >= 4, "{:?}", r.notes);
    let notes = r.notes.join("\n");
    assert!(notes.contains("is gone"), "{notes}");
    assert!(notes.contains("not a line, arc or polyline"), "{notes}");
    assert!(notes.contains("stops inside the area"), "{notes}");
    assert!(notes.contains("cuts nothing"), "{notes}");
    // Courses: a path that follows an object, along the border of a style, and one that takes
    // everything.
    let mut p = Plan::default();
    p.courses = vec![
        CourseDef {
            name: "Followed".into(),
            source: Some(line),
            along: 100.0,
            depth: 200.0,
            gap: 5.0,
            ..Default::default()
        },
        CourseDef {
            name: "Gone".into(),
            source: Some(ObjectId::from_num(9999)),
            path: vec![vx(0.0, 100.0), vx(8000.0, 100.0)],
            along: 100.0,
            depth: 200.0,
            gap: 5.0,
            ..Default::default()
        },
    ];
    set_plan(&mut ed, id, &p);
    let r = planned(&ed, id).unwrap();
    assert_eq!(r.courses.len(), 2);
    assert!(
        r.notes.iter().any(|n| n.contains("which is gone")),
        "{:?}",
        r.notes
    );
    let course = r.groups.iter().find(|g| g.name == "Gone").unwrap();
    assert_eq!(course.role, Role::Course);
    assert_eq!(course.slug, "COURSE-GONE");
    assert!(course.gap() == 5.0 && course.widest_joint() >= 5.0);
    assert!(course.pattern().is_none());
    assert_eq!(course.kind_names(), vec!["100x200".to_string()]);
    // A course too big for the limit of units is refused with a reason.
    let big = Limits::new(5);
    let e = course.for_each_unit(&big, &mut |_| {}).unwrap_err();
    assert!(matches!(e, LayoutError::TooMany(_)));
    let e = groups_of(ed.doc(), ed.doc().object(id).unwrap(), 1.0, 5).unwrap_err();
    assert!(e.contains("course 'Followed'"), "{e}");
    // A course that covers the whole area leaves no field.
    let mut p = Plan::default();
    p.courses = vec![CourseDef {
        name: "All".into(),
        path: vec![vx(-100.0, 2000.0), vx(8100.0, 2000.0)],
        side: Side::Center,
        along: 200.0,
        depth: 5000.0,
        gap: 5.0,
        corner: Corner::Butt,
        ..Default::default()
    }];
    set_plan(&mut ed, id, &p);
    let r = planned(&ed, id).unwrap();
    assert!(
        r.notes.iter().any(|n| n.contains("no field is left")),
        "{:?}",
        r.notes
    );
    assert_eq!(r.groups.len(), 1);
    // A hardscape without a joint pattern has no groups.
    let mut doc = Document::with_sequential_ids();
    let mut ed2 = Editor::new(std::mem::take(&mut doc));
    let bare = ObjectId(
        ed2.execute(&Command::AddHardscape {
            boundary: Some(corners(&[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0)])),
            patch: Box::default(),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    assert!(
        groups_of(ed2.doc(), ed2.doc().object(bare).unwrap(), 1.0, 100)
            .unwrap()
            .is_none()
    );
}

#[test]
fn course_groups_count_units_by_the_stone_and_corner_units_by_shape() {
    let (mut ed, id) = slab(false);
    let mut p = Plan::default();
    p.courses = vec![CourseDef {
        name: "Edge".into(),
        path: vec![vx(0.0, 0.0), vx(8000.0, 0.0), vx(8000.0, 4000.0)],
        along: 100.0,
        depth: 200.0,
        gap: 5.0,
        corner: Corner::CornerUnit,
        unit: "200x100".into(),
        ..Default::default()
    }];
    set_plan(&mut ed, id, &p);
    let r = planned(&ed, id).unwrap();
    let opts = SummaryOptions::default();
    let parts: Vec<_> = r
        .groups
        .iter()
        .map(|g| g.summarize(&opts).unwrap())
        .collect();
    let course = parts.last().unwrap();
    let names: Vec<&str> = course.by_unit.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, ["200x100", "corner 200x200"]);
    assert_eq!(course.by_unit[1].whole_units, 1);
    assert_eq!(course.bond, "course");
    assert_eq!(course.narrowest_joint_mm, Some(5.0));
    // The field and the course share the stone: one row in the total.
    let total = merge(&parts, 32e6, &opts);
    assert_eq!(
        total.by_unit.iter().filter(|u| u.name == "200x100").count(),
        1
    );
    assert_eq!(total.narrowest_joint_mm, Some(5.0));
    // A wide joint on a bend is a warning.
    let mut p = Plan::default();
    p.courses = vec![CourseDef {
        name: "Bend".into(),
        path: vec![
            vx(2000.0, 500.0),
            Vertex::new(v(1000.0, 1500.0), VertexKind::PointOnArc),
            vx(0.0, 500.0),
        ],
        along: 100.0,
        depth: 300.0,
        gap: 5.0,
        ..Default::default()
    }];
    set_plan(&mut ed, id, &p);
    let r = planned(&ed, id).unwrap();
    let course: &Group = r.groups.iter().find(|g| g.name == "Bend").unwrap();
    let s = course.summarize(&opts).unwrap();
    assert!(
        s.joint_warning.is_some() || s.widest_joint_mm < 7.0,
        "{:?}",
        s.widest_joint_mm
    );
}

fn course_def(name: &str, path: &[(f64, f64)], side: Side) -> CourseDef {
    CourseDef {
        name: name.into(),
        path: path.iter().map(|p| vx(p.0, p.1)).collect(),
        side,
        along: 100.0,
        depth: 200.0,
        gap: 5.0,
        ..Default::default()
    }
}

fn clean(ed: &Editor, id: ObjectId, groups: &[Group]) {
    let doc = ed.doc();
    let region = region_of(doc, doc.object(id).unwrap(), 1.0).unwrap();
    let r = check(groups, &region, &CheckOptions::default()).unwrap();
    for k in [
        "overlap",
        "joint_too_narrow",
        "outside_area",
        "coverage",
        "wrong_size",
    ] {
        assert_eq!(
            r.counts[k],
            0,
            "{k}: {:?}",
            r.findings
                .iter()
                .filter(|f| f.kind.id() == k)
                .take(3)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn courses_that_cross_or_meet_never_overlap_each_other_or_the_field() {
    // An aisle course that starts on the kerb course and crosses it.
    for (a, b) in [
        (
            course_def("Kerb", &[(0.0, 0.0), (8000.0, 0.0)], Side::Left),
            course_def("Aisle", &[(4000.0, 0.0), (4000.0, 4000.0)], Side::Left),
        ),
        // The same two courses the other way round, and an aisle that goes right through the kerb.
        (
            course_def("Aisle", &[(4000.0, -300.0), (4000.0, 4000.0)], Side::Left),
            course_def("Kerb", &[(0.0, 0.0), (8000.0, 0.0)], Side::Left),
        ),
        // Two courses that meet at a corner of the lot.
        (
            course_def("Bottom", &[(0.0, 0.0), (8000.0, 0.0)], Side::Left),
            course_def("Side", &[(0.0, 0.0), (0.0, 4000.0)], Side::Right),
        ),
        // The same course twice: the second has no units.
        (
            course_def("One", &[(0.0, 0.0), (8000.0, 0.0)], Side::Left),
            course_def("Two", &[(0.0, 0.0), (8000.0, 0.0)], Side::Left),
        ),
    ] {
        let (mut ed, id) = slab(false);
        let mut p = Plan::default();
        p.courses = vec![a, b];
        p.splits = vec![vline(2500.0)];
        set_plan(&mut ed, id, &p);
        let r = planned(&ed, id).unwrap();
        assert!(
            r.notes
                .iter()
                .any(|n| n.contains("overlaps an earlier course") || n.contains("entirely inside")),
            "{:?}",
            r.notes
        );
        clean(&ed, id, &r.groups);
        // The field and the courses add up to the area: nothing is taken twice or lost.
        let doc = ed.doc();
        let region = region_of(doc, doc.object(id).unwrap(), 1.0).unwrap();
        let field: f64 = r
            .groups
            .iter()
            .filter(|g| g.role != Role::Course)
            .map(|g| g.area())
            .sum();
        assert!(field < region.area && field > 0.5 * region.area);
    }
}

#[test]
fn zones_by_center_have_no_false_cuts_and_the_cuts_are_counted() {
    let (mut ed, id) = slab(false);
    let mut p = Plan::default();
    p.splits = vec![vline(3010.0), vline(6020.0)];
    set_plan(&mut ed, id, &p);
    let cut = planned(&ed, id).unwrap();
    let opts = SummaryOptions::default();
    let edge_cuts = cut
        .zone_edge_cuts(&opts)
        .expect("zones that continue the pattern");
    assert!(
        edge_cuts.whole_lost >= 10 && edge_cuts.extra_pieces >= 10,
        "{edge_cuts:?}"
    );
    let whole_cut: usize = cut
        .groups
        .iter()
        .map(|g| g.summarize(&opts).unwrap().whole_units)
        .sum();
    p.zone_units = plan::ZoneUnits::Centroid;
    set_plan(&mut ed, id, &p);
    let centered = planned(&ed, id).unwrap();
    assert!(centered.zone_edge_cuts(&opts).is_none());
    let parts: Vec<_> = centered
        .groups
        .iter()
        .map(|g| g.summarize(&opts).unwrap())
        .collect();
    // The units are the units of one lattice: exactly the whole units of the field as one area.
    let one = kadakas_core::layout::summary::summarize(
        &region_of(ed.doc(), ed.doc().object(id).unwrap(), 1.0).unwrap(),
        &Pattern::new(Bond::RunningBond, 200.0, 100.0, 5.0),
        &kadakas_core::layout::Placement {
            origin: Some(v(0.0, 0.0)),
            angle: 0.0,
        },
        &opts,
    )
    .unwrap();
    let whole: usize = parts.iter().map(|s| s.whole_units).sum();
    let cutu: usize = parts.iter().map(|s| s.cut_units).sum();
    assert_eq!((whole, cutu), (one.whole_units, one.cut_units));
    assert!(whole > whole_cut);
    // The units are not counted twice, and the check finds nothing.
    clean(&ed, id, &centered.groups);
    assert_eq!(centered.groups.len(), 3);
    assert!((centered.groups.iter().map(|g| g.area()).sum::<f64>() - 32e6).abs() < 1.0);
}

#[test]
fn an_edge_course_follows_the_outline_and_the_plan_follows_a_turn() {
    let (mut ed, id) = slab(true);
    let mut c = course_def("Kerb", &[], Side::Left);
    c.edge = Some("outline".into());
    let mut p = Plan::default();
    p.courses = vec![c];
    set_plan(&mut ed, id, &p);
    let before = planned(&ed, id).unwrap();
    assert_eq!(before.courses[0].corners, 4);
    // The border of 150 mm is not paved: the course starts at its inner edge.
    let Body::Course(cb) = &before.groups[1].body else {
        panic!("a course")
    };
    assert!(
        cb.pieces
            .iter()
            .flat_map(|q| &q.poly)
            .all(|q| q.x >= 150.0 - 1e-6 && q.y >= 150.0 - 1e-6)
    );
    // A turn of the hardscape moves the plan with it.
    use kadakas_core::edit2d::Xf;
    let mut o = ed.doc().object(id).unwrap().clone();
    let mut q = Plan::default();
    q.splits = vec![vline(100.0)];
    q.zones = vec![ZoneSpec {
        seed: v(50.0, 50.0),
        origin: Some(v(5.0, 6.0)),
        ..Default::default()
    }];
    q.courses = vec![course_def("K", &[(0.0, 0.0), (1000.0, 0.0)], Side::Left)];
    o.data
        .insert(plan::KEY.into(), serde_json::to_value(&q).unwrap());
    plan::transform_data(
        &mut o,
        &Xf::rotate(v(0.0, 0.0), std::f64::consts::FRAC_PI_2),
    );
    let r = plan::data(&o).unwrap();
    assert!((r.splits[0].points[0].p - v(50.0, 100.0)).length() < 1e-9);
    assert!((r.zones[0].seed - v(-50.0, 50.0)).length() < 1e-9);
    assert!((r.courses[0].path[1].p - v(0.0, 1000.0)).length() < 1e-9);
    assert_eq!(r.courses[0].side, Side::Left);
    // A mirror puts the course on the other side of its path.
    let mut o2 = ed.doc().object(id).unwrap().clone();
    o2.data
        .insert(plan::KEY.into(), serde_json::to_value(&q).unwrap());
    plan::transform_data(&mut o2, &Xf::mirror(v(0.0, 0.0), v(1.0, 0.0)).unwrap());
    let r = plan::data(&o2).unwrap();
    assert_eq!(r.courses[0].side, Side::Right);
    assert!((r.zones[0].seed - v(50.0, -50.0)).length() < 1e-9);
}
