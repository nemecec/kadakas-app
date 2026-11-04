use super::*;
use kadakas_core::command::Editor;
use std::collections::BTreeMap;

const CANVAS: [f64; 2] = [800.0, 600.0];

fn vp(view: ViewSpec) -> MarkupViewpoint {
    MarkupViewpoint {
        view,
        fov: None,
        canvas: CANVAS,
        layers: BTreeMap::new(),
        classes: BTreeMap::new(),
    }
}

fn plan(center: [f64; 2], zoom: f64) -> MarkupViewpoint {
    vp(ViewSpec::Plan { center, zoom })
}

fn camera(distance: f64, yaw: f64) -> MarkupViewpoint {
    vp(ViewSpec::Camera {
        target: [0.0, 0.0, 0.0],
        yaw,
        pitch: 0.5,
        distance,
        perspective: false,
    })
}

#[test]
fn a_pen_stroke_keeps_points_that_are_far_enough_apart() {
    let mut d = Draft::begin(Tool::Pen, Brush::default(), CANVAS, [10.0, 10.0]).unwrap();
    for i in 0..1000 {
        d.add([10.0 + f64::from(i) * 0.1, 10.0]);
    }
    assert!(d.len() < 80, "{}", d.len());
    let marks = d.finish();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].kind, MarkKind::Pen);
    assert_eq!(marks[0].points[0], [0.0125, 0.016_67]);
    marks[0].validate().unwrap();
    // The last point is where the pen was lifted, not 1.5 points before it.
    let last = marks[0].points.last().unwrap();
    assert!((last[0] * CANVAS[0] - 109.0).abs() < 1.6, "{last:?}");
}

#[test]
fn points_are_relative_to_the_canvas_and_inside_it() {
    let mut d = Draft::begin(Tool::Pen, Brush::default(), CANVAS, [-30.0, 700.0]).unwrap();
    d.add([900.0, -5.0]);
    let m = &d.finish()[0];
    assert_eq!(m.points[0], [0.0, 1.0]);
    assert_eq!(m.points[1], [1.0, 0.0]);
}

#[test]
fn a_click_makes_a_dot_and_a_click_with_a_shape_tool_makes_nothing() {
    let dot = Draft::begin(Tool::Pen, Brush::default(), CANVAS, [400.0, 300.0]).unwrap();
    let marks = dot.finish();
    assert_eq!((marks.len(), marks[0].points.len()), (1, 1));
    for tool in [Tool::Arrow, Tool::Rectangle, Tool::Ellipse] {
        let d = Draft::begin(tool, Brush::default(), CANVAS, [400.0, 300.0]).unwrap();
        assert!(d.finish().is_empty(), "{tool:?}");
    }
    assert!(Draft::begin(Tool::Select, Brush::default(), CANVAS, [0.0, 0.0]).is_none());
    assert!(Draft::begin(Tool::Eraser, Brush::default(), CANVAS, [0.0, 0.0]).is_none());
}

#[test]
fn a_shape_has_its_first_point_and_the_current_one() {
    let brush = Brush {
        color: PALETTE[4],
        width: 7.0,
    };
    let mut d = Draft::begin(Tool::Rectangle, brush, CANVAS, [100.0, 100.0]).unwrap();
    for x in [120.0, 200.0, 300.0] {
        d.add([x, 250.0]);
    }
    assert_eq!(d.len(), 2);
    let m = &d.finish()[0];
    assert_eq!(m.points, vec![[0.125, 0.166_67], [0.375, 0.416_67]]);
    assert_eq!((m.color, m.width), (PALETTE[4], 7.0));
    assert!(!d.preview().is_empty());
}

#[test]
fn a_very_long_stroke_is_cut_into_marks_and_nothing_is_lost() {
    let mut d = Draft::begin(
        Tool::Highlighter,
        Brush::default(),
        [5000.0, 5000.0],
        [0.0, 0.0],
    )
    .unwrap();
    for i in 1..2600 {
        d.add([f64::from(i) * 2.0, 100.0]);
    }
    let marks = d.finish();
    assert!(marks.len() >= 3);
    for m in &marks {
        assert!(m.points.len() <= MAX_POINTS);
        m.validate().unwrap();
    }
    // The pieces join: each starts where the one before ended.
    for w in marks.windows(2) {
        assert_eq!(w[0].points.last(), w[1].points.first());
    }
    let total: usize = marks.iter().map(|m| m.points.len()).sum();
    assert_eq!(total, d.len() + marks.len() - 1);
}

#[test]
fn the_eraser_and_select_find_the_topmost_mark_under_the_pointer() {
    let mut m = Markup::new(plan([0.0, 0.0], 1.0));
    let mut a = Mark::new(MarkKind::Pen, vec![[0.1, 0.5], [0.9, 0.5]]);
    a.width = 6.0;
    let mut b = Mark::new(MarkKind::Rectangle, vec![[0.4, 0.4], [0.6, 0.6]]);
    b.width = 3.0;
    let mut n = Mark::new(MarkKind::Note, vec![[0.1, 0.1]]);
    n.text = "tree".into();
    m.marks = vec![a, b, n];
    m.prepare();
    // On the stroke, away from the rectangle.
    assert_eq!(hit_mark(&m, CANVAS, [160.0, 300.0], 4.0), Some(1));
    // On the edge of the rectangle: the rectangle is later, so it is on top of the stroke.
    assert_eq!(hit_mark(&m, CANVAS, [320.0, 300.0], 4.0), Some(2));
    // Inside the rectangle, not near its edge or the stroke.
    assert_eq!(hit_mark(&m, CANVAS, [400.0, 260.0], 4.0), None);
    // The note is hit inside its box.
    assert_eq!(hit_mark(&m, CANVAS, [100.0, 60.0], 2.0), Some(3));
    assert_eq!(hit_mark(&m, CANVAS, [700.0, 50.0], 4.0), None);
    // A window of another size: the same marks at the same relative places.
    assert_eq!(hit_mark(&m, [1600.0, 1200.0], [320.0, 600.0], 4.0), Some(1));
}

#[test]
fn the_view_matches_when_it_is_the_view_of_the_markup() {
    let stored = plan([100.0, 200.0], 0.5);
    assert!(view_matches(&stored, &stored));
    // A bigger window with the same part of the drawing across the width.
    let mut wide = plan([100.0, 200.0], 1.0);
    wide.canvas = [1600.0, 1200.0];
    assert!(view_matches(&stored, &wide));
    assert!(!view_matches(&stored, &plan([100.0, 200.0], 0.6)));
    assert!(!view_matches(&stored, &plan([300.0, 200.0], 0.5)));
    assert!(!view_matches(&stored, &camera(1000.0, 0.0)));
    let cam = camera(1000.0, 0.5);
    assert!(view_matches(&cam, &camera(1000.0, 0.5)));
    assert!(!view_matches(&cam, &camera(1100.0, 0.5)));
    assert!(!view_matches(&cam, &camera(1000.0, 0.6)));
    let mut persp = camera(1000.0, 0.5);
    if let ViewSpec::Camera { perspective, .. } = &mut persp.view {
        *perspective = true;
    }
    assert!(!view_matches(&cam, &persp));
    // The zoom for a window of another width.
    assert_eq!(plan_zoom_for(&stored, 0.5, 1600.0), 1.0);
}

#[test]
fn a_new_markup_and_a_drawn_mark_are_commands_with_one_undo_step_each() {
    let mut ed = Editor::with_sequential_ids();
    let id = MarkupId(
        ed.execute(&new_markup(plan([0.0, 0.0], 1.0), "Ann", 5, None))
            .unwrap()
            .created[0],
    );
    assert_eq!(ed.doc().markups.get(id).unwrap().author, "Ann");
    let depth = ed.undo_depth();
    let mut d = Draft::begin(Tool::Pen, Brush::default(), CANVAS, [10.0, 10.0]).unwrap();
    d.add([50.0, 50.0]);
    for m in d.finish() {
        ed.execute(&add_mark(id, m)).unwrap();
    }
    assert_eq!(ed.undo_depth(), depth + 1);
    ed.execute(&add_mark(
        id,
        note_at(Brush::default(), CANVAS, [400.0, 300.0], " hi "),
    ))
    .unwrap();
    assert_eq!(ed.doc().markups.get(id).unwrap().marks[1].text, "hi");
    // The commands that show the layers as they were.
    let mut v = plan([0.0, 0.0], 1.0);
    v.layers.insert(kadakas_core::model::FIRST_LAYER, false);
    let cmds = visibility_commands(ed.doc(), &v);
    assert_eq!(cmds.len(), 1);
}
