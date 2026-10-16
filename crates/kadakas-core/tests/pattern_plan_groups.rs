//! The groups of a hardscape with a plan: zone settings, notes, followed lines, the counts of all
//! groups added up, and the refusals (PAT-07, PAT-08).

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{Vertex, VertexKind, corners};
use kadakas_core::hardscape::HardscapePatch;
use kadakas_core::hardscape::style::{Border, HardscapeStyle, JointKind, JointPattern};
use kadakas_core::layout::course::{Corner, Side};
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
