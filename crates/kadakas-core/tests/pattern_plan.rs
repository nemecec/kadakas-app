//! Properties of zones and courses on a hardscape (PAT-07, PAT-08, PAT-10): area is conserved, no
//! two units overlap, and the layout does not depend on where the drawing is.

use proptest::prelude::*;

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{Vertex, corners};
use kadakas_core::hardscape::style::{HardscapeStyle, JointKind, JointPattern};
use kadakas_core::hardscape::{self, HardscapePatch};
use kadakas_core::layout::check::{CheckOptions, check};
use kadakas_core::layout::course::{self, Corner, Side};
use kadakas_core::layout::object::{pattern_of, region_of};
use kadakas_core::layout::plan::{self, CourseDef, Plan, SplitLine, groups_of};
use kadakas_core::layout::summary::SummaryOptions;
use kadakas_core::layout::{MAX_UNITS, area_shared};
use kadakas_core::model::{Attributes, Document, ObjectId};
use kadakas_core::resource::{ResourceBody, ResourceId};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

#[derive(Debug, Clone)]
struct Case {
    w: f64,
    h: f64,
    kind: usize,
    angle: f64,
    cuts_x: Vec<f64>,
    cuts_y: Vec<f64>,
    course: Option<(f64, f64, f64, usize, usize)>,
    shift: (f64, f64),
}

fn case() -> impl Strategy<Value = Case> {
    (
        (6000.0f64..14000.0, 4000.0f64..8000.0),
        0usize..4,
        prop_oneof![Just(0.0), Just(90.0), 0.0f64..180.0],
        proptest::collection::vec(0.15f64..0.85, 0..3),
        proptest::collection::vec(0.15f64..0.85, 0..3),
        proptest::option::of((
            80.0f64..250.0,
            100.0f64..300.0,
            3.0f64..8.0,
            0usize..3,
            0usize..2,
        )),
        (-1.0e6f64..1.0e6, -1.0e6f64..1.0e6),
    )
        .prop_map(|((w, h), kind, angle, cx, cy, course, shift)| Case {
            w,
            h,
            kind,
            angle,
            cuts_x: cx.iter().map(|f| f * w).collect(),
            cuts_y: cy.iter().map(|f| f * h).collect(),
            course,
            shift,
        })
}

fn build(c: &Case, shift: DVec2) -> (Editor, ObjectId) {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let kinds = [
        JointKind::RunningBond,
        JointKind::Stack,
        JointKind::Herringbone,
        JointKind::RunningBond,
    ];
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "P".into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                joints: Some(JointPattern {
                    kind: kinds[c.kind],
                    unit_length: 200.0,
                    unit_width: 100.0,
                    spacing: 5.0,
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
                shift,
                shift + v(c.w, 0.0),
                shift + v(c.w, c.h),
                shift + v(0.0, c.h),
            ])),
            patch: Box::new(HardscapePatch {
                style: Some(style),
                origin: Some(shift + v(13.0, 7.0)),
                angle: Some(c.angle),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let mut plan = Plan::default();
    for x in &c.cuts_x {
        plan.splits.push(SplitLine {
            source: None,
            points: vec![
                Vertex::corner(shift + v(*x, -50.0)),
                Vertex::corner(shift + v(*x, c.h + 50.0)),
            ],
        });
    }
    for y in &c.cuts_y {
        plan.splits.push(SplitLine {
            source: None,
            points: vec![
                Vertex::corner(shift + v(-50.0, *y)),
                Vertex::corner(shift + v(c.w + 50.0, *y)),
            ],
        });
    }
    if let Some((along, depth, gap, corner, side)) = c.course {
        plan.courses.push(CourseDef {
            name: "Kerb".into(),
            path: vec![
                Vertex::corner(shift + v(0.0, 0.0)),
                Vertex::corner(shift + v(c.w, 0.0)),
                Vertex::corner(shift + v(c.w, c.h)),
            ],
            side: [Side::Left, Side::Right][side],
            along,
            depth,
            gap: Some(gap),
            corner: [Corner::Miter, Corner::Butt, Corner::CornerUnit][corner],
            ..Default::default()
        });
    }
    if !plan.is_empty() {
        ed.execute(&Command::SetData {
            id,
            set: [(plan::KEY.to_string(), serde_json::to_value(&plan).unwrap())]
                .into_iter()
                .collect(),
            remove: vec![],
        })
        .unwrap();
    }
    (ed, id)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(48)
    ))]

    #[test]
    fn zones_and_courses_conserve_area_and_never_overlap(c in case()) {
        let (ed, id) = build(&c, DVec2::ZERO);
        let doc = ed.doc();
        let o = doc.object(id).unwrap();
        let region = region_of(doc, o, 1.0).unwrap();
        let Ok(Some(planned)) = groups_of(doc, o, 1.0, MAX_UNITS) else {
            // A corner that is too sharp, or a course that takes everything, is refused: fine.
            return Ok(());
        };
        // The field and its zones add up to the area less what the courses take.
        let field: f64 = planned
            .groups
            .iter()
            .filter(|g| g.role != plan::Role::Course)
            .map(|g| g.region.area)
            .sum();
        let mut taken = 0.0;
        if let Some((along, depth, gap, corner, side)) = c.course {
            let spec = course::Spec {
                path: vec![Vertex::corner(v(0.0, 0.0)), Vertex::corner(v(c.w, 0.0)), Vertex::corner(v(c.w, c.h))],
                closed: false,
                side: [Side::Left, Side::Right][side],
                offset: 0.0,
                along,
                depth,
                gap,
                corner: [Corner::Miter, Corner::Butt, Corner::CornerUnit][corner],
            };
            let laid = course::lay(&spec, MAX_UNITS).unwrap();
            let outer = vec![v(0.0, 0.0), v(c.w, 0.0), v(c.w, c.h), v(0.0, c.h)];
            for s in &laid.band {
                taken += area_shared(&s[0], &outer);
                for hole in &s[1..] {
                    taken -= area_shared(hole, &outer);
                }
            }
        }
        prop_assert!(
            (field + taken - region.area).abs() <= 1e-4 * region.area + 50.0,
            "field {} + course {} = {} against {}",
            field,
            taken,
            field + taken,
            region.area
        );
        // No overlaps, no units outside their area, no joints below the gap.
        let r = check(
            &planned.groups,
            &region,
            &CheckOptions { max_findings: 5, ..Default::default() },
        )
        .unwrap();
        prop_assert_eq!(r.counts["overlap"], 0, "{:?}", r.findings.iter().filter(|f| f.kind.id() == "overlap").collect::<Vec<_>>());
        prop_assert_eq!(r.counts["outside_area"], 0, "{:?}", r.findings);
        prop_assert_eq!(r.counts["wrong_size"], 0);
        // The units cover no more than the area.
        prop_assert!(r.units_area_m2 <= region.area / 1e6 + 1e-6);
    }

    #[test]
    fn the_layout_does_not_depend_on_where_the_drawing_is(c in case()) {
        let (ed0, id0) = build(&c, DVec2::ZERO);
        let shift = v(c.shift.0, c.shift.1);
        let (ed1, id1) = build(&c, shift);
        let opts = SummaryOptions::default();
        let count = |ed: &Editor, id: ObjectId| -> Option<(usize, usize, usize)> {
            let doc = ed.doc();
            let o = doc.object(id).unwrap();
            pattern_of(doc, o).unwrap();
            let planned = groups_of(doc, o, 1.0, MAX_UNITS).ok()??;
            let mut whole = 0;
            let mut cut = 0;
            for g in &planned.groups {
                let s = g.summarize(&opts).ok()?;
                whole += s.whole_units;
                cut += s.cut_units;
            }
            Some((planned.groups.len(), whole, cut))
        };
        let a = count(&ed0, id0);
        let b = count(&ed1, id1);
        match (a, b) {
            (Some(a), Some(b)) => {
                prop_assert_eq!(a.0, b.0);
                // Units that are cut by a hair at a far-away place can tip over the trim of 0.1 mm:
                // the totals may differ by the rounding of coordinates up to 1e6 mm.
                let units = |t: (usize, usize, usize)| t.1 + t.2;
                prop_assert!(
                    (units(a) as i64 - units(b) as i64).abs() <= 3 + units(a) as i64 / 2000,
                    "{:?} against {:?}", a, b
                );
            }
            (None, None) => {}
            (a, b) => prop_assert!(false, "{:?} against {:?}", a, b),
        }
        let _ = hardscape::data::KEY;
    }
}

#[test]
fn a_case_with_lines_and_a_course_has_the_groups_it_should() {
    let c = Case {
        w: 9000.0,
        h: 5000.0,
        kind: 0,
        angle: 0.0,
        cuts_x: vec![3000.0, 6000.0],
        cuts_y: vec![2500.0],
        course: Some((100.0, 200.0, 5.0, 0, 0)),
        shift: (0.0, 0.0),
    };
    let (ed, id) = build(&c, DVec2::ZERO);
    let doc = ed.doc();
    let o = doc.object(id).unwrap();
    let planned = groups_of(doc, o, 1.0, MAX_UNITS).unwrap().unwrap();
    // Six zones and a course.
    assert_eq!(planned.groups.len(), 7, "{:?}", planned.notes);
    assert_eq!(planned.zones.len(), 6);
    assert_eq!(planned.courses.len(), 1);
}
