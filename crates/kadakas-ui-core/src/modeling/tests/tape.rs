//! Tests of the Tape Measure (MOD-05): measure, parallel guide from an edge, guide through a
//! point, a typed distance, Ctrl, Esc, and one undo step for each guide.

use glam::{DVec2, DVec3};
use kadakas_core::command::Command;
use kadakas_core::guide::Guide;
use kadakas_core::model::{Attributes, ObjectId, Shape};
use kadakas_core::surface::{EdgeId, Edit};

use super::*;
use crate::infer3d::Constraint;
use crate::measure_box::Phase;

fn guides(r: &Rig) -> Vec<Guide> {
    r.ed.doc().guides().map(|(_, g)| g.clone()).collect()
}

/// A model with one edge along X from the origin to (1000, 0, 0). Its number in the scenes is 0.
pub(super) fn with_edge(r: &mut Rig) -> EdgeId {
    let id = ObjectId(
        r.ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    r.ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges {
            edges: vec![[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]]],
        },
    })
    .unwrap();
    r.model().edges().next().unwrap().0
}

/// The inference of a pointer that is on the edge at `p`.
pub(super) fn on_edge(edge: EdgeId, p: DVec3) -> Inference {
    Inference {
        kind: Kind::OnEdge,
        source: Tag::edge(0, edge),
        constraint: Some(Constraint::Line {
            origin: DVec3::ZERO,
            dir: DVec3::X,
        }),
        ..free(p)
    }
}

fn units() -> kadakas_units::Units {
    kadakas_units::current()
}

#[test]
fn the_tape_measure_makes_a_guide_line_through_a_point_in_one_undo_step() {
    let mut r = rig(ModelerTool::TapeMeasure);
    assert!(r.tools.tape_creates_guides(), "guides are the default");
    let depth = r.ed.undo_depth();
    let out = r.click(DVec3::new(100.0, 200.0, 0.0));
    assert!(out.actions.is_empty() && r.tools.in_progress());
    assert_eq!(r.tools.reference(), Some(DVec3::new(100.0, 200.0, 0.0)));
    assert_eq!(r.tools.phase(), Phase::Step(1));
    let out = r.click(DVec3::new(100.0, 1200.0, 0.0));
    assert!(matches!(out.actions[..], [Action::AddGuides { .. }]));
    assert!(!r.tools.in_progress());
    assert_eq!(
        r.ed.undo_depth(),
        depth + 1,
        "one undo step for the line and its end point"
    );
    let g = guides(&r);
    assert_eq!(g.len(), 2);
    assert_eq!(g[0].at, DVec3::new(100.0, 200.0, 0.0));
    assert!(g[0].unit_dir().unwrap().distance(DVec3::Y) < 1e-9);
    assert_eq!(
        g[0].length,
        Some(1000.0),
        "a finite line from the start to the end"
    );
    assert_eq!(g[1], Guide::point(DVec3::new(100.0, 1200.0, 0.0)));
    assert!(r.ed.undo());
    assert!(guides(&r).is_empty());
}

#[test]
fn two_clicks_on_one_point_make_a_guide_point() {
    let mut r = rig(ModelerTool::TapeMeasure);
    let p = DVec3::new(5.0, 6.0, 7.0);
    r.click(p);
    let out = r.click(p);
    assert!(matches!(out.actions[..], [Action::AddGuides { .. }]));
    assert_eq!(guides(&r), vec![Guide::point(p)]);
}

#[test]
fn a_click_on_an_edge_and_one_away_from_it_make_a_parallel_guide() {
    let mut r = rig(ModelerTool::TapeMeasure);
    let e = with_edge(&mut r);
    let depth = r.ed.undo_depth();
    r.click_inf(
        on_edge(e, DVec3::new(300.0, 0.0, 0.0)),
        down_ray(DVec3::ZERO),
    );
    // The offset shows while the pointer moves: the distance from the line of the edge.
    r.hover(DVec3::new(700.0, 400.0, 0.0));
    let (_, text) = r.tools.live(&units()).unwrap();
    assert!(text.contains("400"), "{text}");
    let out = r.click(DVec3::new(700.0, 400.0, 0.0));
    assert!(matches!(out.actions[..], [Action::AddGuides { .. }]));
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let g = guides(&r);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].at, DVec3::new(700.0, 400.0, 0.0));
    assert!(
        g[0].unit_dir().unwrap().dot(DVec3::X).abs() > 0.999999,
        "parallel"
    );
}

#[test]
fn a_second_click_on_the_line_of_the_edge_makes_no_guide() {
    let mut r = rig(ModelerTool::TapeMeasure);
    let e = with_edge(&mut r);
    r.click_inf(
        on_edge(e, DVec3::new(300.0, 0.0, 0.0)),
        down_ray(DVec3::ZERO),
    );
    let out = r.click(DVec3::new(900.0, 0.0, 0.0));
    assert!(out.actions.is_empty());
    assert!(out.message.is_some());
    assert!(guides(&r).is_empty());
}

#[test]
fn a_typed_distance_sets_the_offset_from_an_edge_on_the_side_of_the_pointer() {
    let mut r = rig(ModelerTool::TapeMeasure);
    let e = with_edge(&mut r);
    r.click_inf(
        on_edge(e, DVec3::new(200.0, 0.0, 0.0)),
        down_ray(DVec3::ZERO),
    );
    r.hover(DVec3::new(250.0, -30.0, 0.0));
    r.type_text("250");
    assert_eq!(guides(&r).len(), 1);
    let g = &guides(&r)[0];
    assert!(
        g.at.distance(DVec3::new(200.0, -250.0, 0.0)) < 1e-9,
        "{:?}",
        g.at
    );
    assert!(!r.tools.in_progress());
    // A negative distance takes the other side.
    r.click_inf(
        on_edge(e, DVec3::new(200.0, 0.0, 0.0)),
        down_ray(DVec3::ZERO),
    );
    r.hover(DVec3::new(250.0, -30.0, 0.0));
    r.type_text("-250");
    let all = guides(&r);
    assert!(all[1].at.distance(DVec3::new(200.0, 250.0, 0.0)) < 1e-9);
}

#[test]
fn a_typed_distance_makes_a_finite_guide_from_a_point() {
    let mut r = rig(ModelerTool::TapeMeasure);
    r.click(DVec3::ZERO);
    r.hover(DVec3::new(10.0, 0.0, 0.0));
    r.type_text("750");
    let g = guides(&r);
    assert_eq!(g.len(), 2, "the line and a point at its end");
    assert_eq!(g[0].at, DVec3::ZERO);
    assert_eq!(g[0].length, Some(750.0));
    assert!(g[0].unit_dir().unwrap().distance(DVec3::X) < 1e-9);
    assert_eq!(g[1], Guide::point(DVec3::new(750.0, 0.0, 0.0)));
    // A negative length points the other way.
    r.click(DVec3::ZERO);
    r.hover(DVec3::new(10.0, 0.0, 0.0));
    r.type_text("-100");
    let g = guides(&r);
    assert!(g[2].unit_dir().unwrap().distance(-DVec3::X) < 1e-9);
    assert_eq!(g[2].length, Some(100.0));
    assert_eq!(g[3], Guide::point(DVec3::new(-100.0, 0.0, 0.0)));
}

#[test]
fn ctrl_measures_only_and_shows_the_distance() {
    let mut r = rig(ModelerTool::TapeMeasure);
    r.tools.toggle_ctrl();
    assert!(!r.tools.tape_creates_guides());
    let depth = r.ed.undo_depth();
    r.click(DVec3::new(0.0, 0.0, 0.0));
    r.hover(DVec3::new(3000.0, 4000.0, 0.0));
    let (label, text) = r.tools.live(&units()).unwrap();
    assert_eq!(label, LiveLabel::Distance);
    assert!(text.contains("5000") || text.contains('5'), "{text}");
    let out = r.click(DVec3::new(3000.0, 4000.0, 0.0));
    assert!(out.actions.is_empty());
    assert_eq!(r.ed.undo_depth(), depth, "a measure changes nothing");
    assert!(guides(&r).is_empty());
    // The value stays after the second click, until the next one.
    assert_eq!(r.tools.phase(), Phase::Done);
    let (_, kept) = r.tools.live(&units()).unwrap();
    assert_eq!(kept, text);
    r.click(DVec3::new(1.0, 1.0, 0.0));
    assert_eq!(r.tools.phase(), Phase::Step(1));
    // Ctrl again: guides come back.
    r.tools.toggle_ctrl();
    assert!(r.tools.tape_creates_guides());
}

#[test]
fn a_typed_length_after_a_measure_asks_to_resize_the_model() {
    let mut r = rig(ModelerTool::TapeMeasure);
    r.tools.toggle_ctrl();
    r.click(DVec3::new(1000.0, 0.0, 0.0));
    r.click(DVec3::new(3000.0, 0.0, 0.0));
    let depth = r.ed.undo_depth();
    let out = r.type_text("4000");
    let resize = out.resize.expect("a question");
    assert!((resize.factor - 2.0).abs() < 1e-12);
    assert_eq!(resize.about, DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!((resize.measured, resize.typed), (2000.0, 4000.0));
    assert!(out.actions.is_empty());
    assert_eq!(
        r.ed.undo_depth(),
        depth,
        "the tool changes nothing by itself"
    );
    // A factor out of range is refused with a reason.
    let out = r.type_text("40000000");
    assert!(out.resize.is_none() && out.message.is_some());
    // Nothing measured: nothing to resize.
    r.tools.choose(ModelerTool::TapeMeasure);
    r.tools.toggle_ctrl();
    let out = r.type_text("5");
    assert!(out.resize.is_none() && out.message.is_some());
}

#[test]
fn escape_cancels_and_nothing_is_made() {
    let mut r = rig(ModelerTool::TapeMeasure);
    r.click(DVec3::ZERO);
    assert!(r.tools.in_progress());
    assert!(r.tools.cancel());
    assert!(!r.tools.in_progress());
    assert_eq!(r.tools.reference(), None);
    assert!(!r.tools.cancel(), "nothing more to cancel");
    // After Esc the next click starts again.
    let out = r.click(DVec3::new(5.0, 5.0, 0.0));
    assert!(out.actions.is_empty() && r.tools.in_progress());
    // A typed value is cleared by Esc first.
    assert!(r.tools.type_char('5'));
    assert!(r.tools.cancel());
    assert_eq!(r.tools.typed(), "");
    assert!(r.tools.in_progress(), "the second Esc ends the measure");
    r.tools.abort();
    assert!(!r.tools.in_progress());
}

#[test]
fn hovering_an_edge_shows_its_length() {
    let mut r = rig(ModelerTool::TapeMeasure);
    let e = with_edge(&mut r);
    assert!(r.tools.live(&units()).is_none());
    let inf = on_edge(e, DVec3::new(400.0, 0.0, 0.0));
    r.act(|t, w| {
        t.moved(
            w,
            &Pointer {
                inf: &inf,
                ray: down_ray(DVec3::ZERO),
                selected: None,
            },
        );
        Out::default()
    });
    let (label, text) = r.tools.live(&units()).unwrap();
    assert_eq!(label, LiveLabel::Distance);
    assert!(text.contains("1000") || text.contains('1'), "{text}");
    // Away from the edge the value goes.
    r.hover(DVec3::new(0.0, 900.0, 0.0));
    assert!(r.tools.live(&units()).is_none());
}

#[test]
fn a_guide_goes_on_the_active_layer_in_layer_coordinates() {
    let mut r = rig(ModelerTool::TapeMeasure);
    let layer = kadakas_core::model::LayerId(
        r.ed.execute(&Command::AddLayer {
            name: "Upper".into(),
            elevation: 1000.0,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0],
    );
    r.ed.execute(&Command::SetActive {
        layer: Some(layer),
        class: None,
    })
    .unwrap();
    r.click(DVec3::new(0.0, 0.0, 1500.0));
    r.click(DVec3::new(10.0, 0.0, 1500.0));
    let (o, g) = r.ed.doc().guides().next().unwrap();
    assert_eq!(o.layer, layer);
    assert_eq!(g.at.z, 500.0, "measured from the layer");
}

#[test]
fn the_preview_shows_the_measure_and_the_guide() {
    let mut r = rig(ModelerTool::TapeMeasure);
    let e = with_edge(&mut r);
    let w_preview = |r: &mut Rig| {
        let rev = r.ed.revision();
        let scenes = r.cache.get(r.ed.doc(), rev);
        let w = World {
            doc: r.ed.doc(),
            scenes,
        };
        r.tools.preview(&w)
    };
    assert!(w_preview(&mut r).shape.is_empty());
    r.click_inf(
        on_edge(e, DVec3::new(300.0, 0.0, 0.0)),
        down_ray(DVec3::ZERO),
    );
    r.hover(DVec3::new(700.0, 400.0, 0.0));
    let pv = w_preview(&mut r);
    assert_eq!(pv.guides.len(), 1);
    assert_eq!(pv.guides[0].at, DVec3::new(300.0, 400.0, 0.0));
    assert!(pv.guides[0].dir.dot(DVec3::X).abs() > 0.999999);
    assert_eq!(
        pv.shape,
        vec![[DVec3::new(300.0, 0.0, 0.0), DVec3::new(300.0, 400.0, 0.0)]]
    );
    // Measure only: a plain line and no guide.
    r.tools.toggle_ctrl();
    let pv = w_preview(&mut r);
    assert!(pv.guides.is_empty());
    assert_eq!(pv.shape.len(), 1);
    let _ = DVec2::ZERO;
}

#[test]
fn the_eraser_removes_guides_in_one_undo_step() {
    let mut r = rig(ModelerTool::Eraser);
    for x in [0.0, 500.0] {
        r.ed.execute(&Command::AddObject {
            shape: Shape::Guide(Box::new(Guide::point(DVec3::new(x, 0.0, 0.0)))),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    assert_eq!(guides(&r).len(), 2);
    let depth = r.ed.undo_depth();
    let on_guide = |n: u32| Inference {
        kind: Kind::GuidePoint,
        source: Tag::guide(n),
        ..free(DVec3::ZERO)
    };
    // Hovering shows which guide goes.
    r.act_no_run(|t, w| {
        let inf = on_guide(0);
        t.moved(
            w,
            &Pointer {
                inf: &inf,
                ray: down_ray(DVec3::ZERO),
                selected: None,
            },
        );
        assert_eq!(t.preview(w).erase_guides.len(), 1);
        Out::default()
    });
    // A click on one guide erases it.
    let out = r.act(|t, w| {
        let inf = on_guide(0);
        let p = Pointer {
            inf: &inf,
            ray: down_ray(DVec3::ZERO),
            selected: None,
        };
        t.press(w, &p);
        t.release()
    });
    assert!(matches!(out.actions[..], [Action::EraseGuides { .. }]));
    assert_eq!(guides(&r).len(), 1);
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert!(r.ed.undo());
    assert_eq!(guides(&r).len(), 2);
    // A drag over both erases both, still one step.
    let out = r.act(|t, w| {
        for n in [0, 1] {
            let inf = on_guide(n);
            let p = Pointer {
                inf: &inf,
                ray: down_ray(DVec3::ZERO),
                selected: None,
            };
            if n == 0 {
                t.press(w, &p);
            }
            t.moved(w, &p);
        }
        t.release()
    });
    assert!(matches!(&out.actions[..], [Action::EraseGuides { ids }] if ids.len() == 2));
    assert!(guides(&r).is_empty());
    assert_eq!(r.ed.undo_depth(), depth + 1);
}
