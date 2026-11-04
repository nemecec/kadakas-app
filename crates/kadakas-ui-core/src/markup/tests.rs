use super::*;
use kadakas_core::command::Editor;
use std::collections::BTreeMap;

const CANVAS: [f64; 2] = [800.0, 600.0];

fn vp(view: ViewSpec, canvas: [f64; 2]) -> MarkupViewpoint {
    MarkupViewpoint {
        view,
        fov: None,
        canvas,
        layers: BTreeMap::new(),
        classes: BTreeMap::new(),
    }
}

fn plan(center: [f64; 2], zoom: f64) -> MarkupViewpoint {
    vp(ViewSpec::Plan { center, zoom }, CANVAS)
}

fn camera(distance: f64, yaw: f64, canvas: [f64; 2]) -> MarkupViewpoint {
    vp(
        ViewSpec::Camera {
            target: [0.0, 0.0, 0.0],
            yaw,
            pitch: 0.5,
            distance,
            perspective: false,
        },
        canvas,
    )
}

fn mapped(stored: &MarkupViewpoint, now: &MarkupViewpoint) -> FrameMap {
    match frame_state(stored, now) {
        FrameState::Mapped(m) => m,
        other => panic!("{other:?}"),
    }
}

/// The map of a markup on the canvas it was drawn on.
fn same_canvas() -> FrameMap {
    mapped(&plan([0.0, 0.0], 1.0), &plan([0.0, 0.0], 1.0))
}

#[test]
fn on_its_own_canvas_the_frame_is_the_canvas() {
    let m = same_canvas();
    assert_eq!((m.origin, m.size, m.scale), ([0.0, 0.0], CANVAS, 1.0));
    assert_eq!(m.to_screen([0.5, 0.25]), [400.0, 150.0]);
    assert_eq!(m.to_rel([400.0, 150.0]), [0.5, 0.25]);
    // A place outside the frame is held at its edge.
    assert_eq!(m.to_rel([-30.0, 700.0]), [0.0, 1.0]);
}

#[test]
fn in_the_plan_the_frame_keeps_its_place_in_the_drawing() {
    let stored = plan([100.0, 200.0], 0.5);
    // A window of another height: the middle of the frame stays at the drawing point.
    let mut now = plan([100.0, 200.0], 0.5);
    now.canvas = [800.0, 400.0];
    let m = mapped(&stored, &now);
    assert_eq!(m.size, CANVAS);
    assert_eq!(m.to_screen([0.5, 0.5]), [400.0, 200.0]);
    // The view pans by 40 points to the right: the frame moves 40 points to the left.
    let panned = plan([100.0 + 40.0 / 0.5, 200.0], 0.5);
    let p = mapped(&stored, &panned);
    assert!(
        (p.origin[0] + 40.0).abs() < 1e-9 && p.origin[1].abs() < 1e-9,
        "{p:?}"
    );
    // A zoom of 2 about the middle doubles the frame and the width of lines.
    let z = mapped(&stored, &plan([100.0, 200.0], 1.0));
    assert_eq!((z.size, z.scale), ([1600.0, 1200.0], 2.0));
    assert_eq!(z.to_screen([0.5, 0.5]), [400.0, 300.0]);
}

#[test]
fn in_3d_the_frame_has_the_height_of_the_canvas_and_is_centered() {
    let stored = camera(1000.0, 0.0, [1200.0, 800.0]);
    let m = mapped(&stored, &stored);
    assert_eq!(
        (m.origin, m.size, m.scale),
        ([0.0, 0.0], [1200.0, 800.0], 1.0)
    );
    // A window as wide but lower: the frame is lower and narrower, centered, and fits.
    let m = mapped(&stored, &camera(1000.0, 0.0, [1200.0, 600.0]));
    assert_eq!(m.size, [900.0, 600.0]);
    assert_eq!(m.origin, [150.0, 0.0]);
    assert_eq!(m.scale, 0.75);
    assert_eq!(m.to_screen([0.5, 0.5]), [600.0, 300.0]);
    // A window that is too narrow for the frame: marks wait.
    assert_eq!(
        frame_state(&stored, &camera(1000.0, 0.0, [700.0, 800.0])),
        FrameState::DoesNotFit
    );
    // Another camera, and the plan against 3D.
    assert_eq!(
        frame_state(&stored, &camera(1100.0, 0.0, [1200.0, 800.0])),
        FrameState::ViewDiffers
    );
    assert_eq!(
        frame_state(&stored, &plan([0.0, 0.0], 1.0)),
        FrameState::ViewDiffers
    );
    let mut persp = camera(1000.0, 0.0, [1200.0, 800.0]);
    if let ViewSpec::Camera { perspective, .. } = &mut persp.view {
        *perspective = true;
    }
    assert_eq!(frame_state(&stored, &persp), FrameState::ViewDiffers);
}

#[test]
fn a_pen_stroke_keeps_points_that_are_far_enough_apart() {
    let map = same_canvas();
    let mut d = Draft::begin(Tool::Pen, Brush::default(), [10.0, 10.0]).unwrap();
    for i in 0..1000 {
        d.add([10.0 + f64::from(i) * 0.1, 10.0]);
    }
    assert!(d.len() < 80, "{}", d.len());
    let marks = d.finish(&map);
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].kind, MarkKind::Pen);
    assert_eq!(marks[0].points[0], [0.0125, 0.016_67]);
    marks[0].validate().unwrap();
    // The last point is where the pen was lifted.
    let last = marks[0].points.last().unwrap();
    assert!((last[0] * CANVAS[0] - 109.9).abs() < 0.01, "{last:?}");
}

#[test]
fn points_are_stored_in_the_frame_of_the_markup_after_a_resize() {
    // The markup was drawn on 800 by 600. The window is now 800 by 400.
    let stored = plan([0.0, 0.0], 1.0);
    let mut now = plan([0.0, 0.0], 1.0);
    now.canvas = [800.0, 400.0];
    let map = mapped(&stored, &now);
    let mut d = Draft::begin(Tool::Pen, Brush::default(), [400.0, 200.0]).unwrap();
    d.add([480.0, 200.0]);
    let m = &d.finish(&map)[0];
    assert_eq!(m.points[0], [0.5, 0.5]);
    assert_eq!(m.points[1], [0.6, 0.5]);
    // 3D: the window is lower, the frame narrower. 100 points left of the middle is 100/900.
    let map = mapped(
        &camera(1000.0, 0.0, [1200.0, 800.0]),
        &camera(1000.0, 0.0, [1200.0, 600.0]),
    );
    let mark = note_at(Brush::default(), &map, [500.0, 300.0], "x");
    assert!(
        (mark.points[0][0] - (0.5 - 100.0 / 900.0)).abs() < 1e-5,
        "{:?}",
        mark.points
    );
    assert_eq!(mark.points[0][1], 0.5);
    let back = map.to_screen(mark.points[0]);
    assert!((back[0] - 500.0).abs() < 0.01 && (back[1] - 300.0).abs() < 0.01);
}

#[test]
fn a_click_makes_a_dot_and_a_click_with_a_shape_tool_makes_nothing() {
    let map = same_canvas();
    let dot = Draft::begin(Tool::Pen, Brush::default(), [400.0, 300.0]).unwrap();
    let marks = dot.finish(&map);
    assert_eq!((marks.len(), marks[0].points.len()), (1, 1));
    for tool in [Tool::Arrow, Tool::Rectangle, Tool::Ellipse] {
        let d = Draft::begin(tool, Brush::default(), [400.0, 300.0]).unwrap();
        assert!(d.finish(&map).is_empty(), "{tool:?}");
    }
    assert!(Draft::begin(Tool::Select, Brush::default(), [0.0, 0.0]).is_none());
    assert!(Draft::begin(Tool::Eraser, Brush::default(), [0.0, 0.0]).is_none());
}

#[test]
fn a_shape_has_its_first_point_and_the_current_one_and_the_width_scales_back() {
    let brush = Brush {
        color: PALETTE[4],
        width: 7.0,
    };
    let map = mapped(&plan([0.0, 0.0], 1.0), &plan([0.0, 0.0], 2.0));
    let mut d = Draft::begin(Tool::Rectangle, brush, [100.0, 100.0]).unwrap();
    for x in [120.0, 200.0, 300.0] {
        d.add([x, 250.0]);
    }
    assert_eq!(d.len(), 2);
    let m = &d.finish(&map)[0];
    assert_eq!(m.points.len(), 2);
    assert_eq!(
        (m.color, m.width),
        (PALETTE[4], 3.5),
        "7 points at a zoom of 2"
    );
    assert!(!d.preview(&map).is_empty());
}

#[test]
fn a_very_long_stroke_is_cut_into_marks_and_nothing_is_lost() {
    let big = vp(
        ViewSpec::Plan {
            center: [0.0, 0.0],
            zoom: 1.0,
        },
        [5000.0, 5000.0],
    );
    let map = mapped(&big, &big);
    let mut d = Draft::begin(Tool::Highlighter, Brush::default(), [0.0, 0.0]).unwrap();
    for i in 1..2600 {
        d.add([f64::from(i) * 2.0, 100.0]);
    }
    let marks = d.finish(&map);
    assert!(marks.len() >= 3);
    for m in &marks {
        assert!(m.points.len() <= MAX_POINTS);
        m.validate().unwrap();
    }
    for w in marks.windows(2) {
        assert_eq!(w[0].points.last(), w[1].points.first());
    }
    let total: usize = marks.iter().map(|m| m.points.len()).sum();
    assert_eq!(total, d.len() + marks.len() - 1);
}

#[test]
fn the_eraser_and_select_find_the_topmost_mark_under_the_pointer() {
    let map = same_canvas();
    let mut m = Markup::new(plan([0.0, 0.0], 1.0));
    let mut a = Mark::new(MarkKind::Pen, vec![[0.1, 0.5], [0.9, 0.5]]);
    a.width = 6.0;
    let mut b = Mark::new(MarkKind::Rectangle, vec![[0.4, 0.4], [0.6, 0.6]]);
    b.width = 3.0;
    let mut n = Mark::new(MarkKind::Note, vec![[0.1, 0.1]]);
    n.text = "tree".into();
    m.marks = vec![a, b, n];
    m.prepare();
    assert_eq!(hit_mark(&m, &map, [160.0, 300.0], 4.0), Some(1));
    assert_eq!(hit_mark(&m, &map, [320.0, 300.0], 4.0), Some(2));
    assert_eq!(hit_mark(&m, &map, [400.0, 260.0], 4.0), None);
    assert_eq!(hit_mark(&m, &map, [100.0, 60.0], 2.0), Some(3));
    assert_eq!(hit_mark(&m, &map, [700.0, 50.0], 4.0), None);
    // A bigger frame (a zoom of 2): the same marks at the same relative places.
    let zoomed = mapped(&plan([0.0, 0.0], 1.0), &plan([0.0, 0.0], 2.0));
    assert_eq!(
        hit_mark(&m, &zoomed, zoomed.to_screen([0.2, 0.5]), 4.0),
        Some(1)
    );
    // A mark that is not drawable is not hit.
    m.marks[0].width = -5.0;
    assert_eq!(hit_mark(&m, &map, [160.0, 300.0], 4.0), None);
}

#[test]
fn a_new_markup_and_a_drawn_mark_are_commands_with_one_undo_step_each() {
    let map = same_canvas();
    let mut ed = Editor::with_sequential_ids();
    let id = MarkupId(
        ed.execute(&new_markup(
            plan([0.0, 0.0], 1.0),
            "Ann",
            5,
            None,
            "m".into(),
        ))
        .unwrap()
        .created[0],
    );
    assert_eq!(ed.doc().markups.get(id).unwrap().author, "Ann");
    let depth = ed.undo_depth();
    let mut d = Draft::begin(Tool::Pen, Brush::default(), [10.0, 10.0]).unwrap();
    d.add([50.0, 50.0]);
    for m in d.finish(&map) {
        ed.execute(&add_mark(id, m)).unwrap();
    }
    assert_eq!(ed.undo_depth(), depth + 1);
    ed.execute(&add_mark(
        id,
        note_at(Brush::default(), &map, [400.0, 300.0], " hi "),
    ))
    .unwrap();
    assert_eq!(ed.doc().markups.get(id).unwrap().marks[1].text, "hi");
    let mut v = plan([0.0, 0.0], 1.0);
    v.layers.insert(kadakas_core::model::FIRST_LAYER, false);
    let cmds = visibility_commands(ed.doc(), &v);
    assert_eq!(cmds.len(), 1);
}
