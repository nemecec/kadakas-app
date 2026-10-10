//! Headless pipelines: what a UI program does between a pointer event and a finished object,
//! driven without any window. These tests use only the public API of the crate.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, Shape};
use kadakas_core::snapping::{Kind, Settings, Smart, SnapSet};
use kadakas_ui_core::databar::{self, DataEntry};
use kadakas_ui_core::effects::apply;
use kadakas_ui_core::snap::snap_cursor;
use kadakas_ui_core::tools::{Ctx, Input, Tool, ToolMachine, ToolSettings};
use kadakas_ui_core::view::View;

fn ctx<'a>(ed: &'a Editor, settings: &'a ToolSettings) -> Ctx<'a> {
    Ctx {
        doc: ed.doc(),
        selection: &[],
        settings,
        tol: 5.0,
        context: None,
        annot: Default::default(),
    }
}

fn shapes(ed: &Editor) -> Vec<Shape> {
    ed.doc().objects.values().map(|o| o.shape.clone()).collect()
}

/// A screen point becomes a model point, snaps to the end of an existing line, becomes a click
/// of the Line tool, and the effects of the tool make the finished line.
#[test]
fn a_pointer_press_becomes_a_snapped_line() {
    let mut ed = Editor::default();
    ed.execute(&Command::AddObject {
        shape: Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(1000.0, 0.0),
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let view = View::default();
    let tool_settings = ToolSettings::default();
    // Snap to the ends and the grid is off, so the second click stays where the pointer is.
    let snap_settings = Settings {
        set: SnapSet {
            grid: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let tol = 8.0 / view.zoom;
    let mut m = ToolMachine::default();
    m.set_tool(Tool::Line);
    let mut selection = Vec::new();

    // First click: the pointer is 2 px from the end (1000, 0) of the old line.
    let end_on_screen = view.screen_of(DVec2::new(1000.0, 0.0));
    let first_raw = view.model_of(end_on_screen + DVec2::new(2.0, -2.0));
    assert_ne!(first_raw, DVec2::new(1000.0, 0.0));
    let snap = snap_cursor(
        ed.doc(),
        first_raw,
        tol,
        200.0,
        &snap_settings,
        &Smart::default(),
        m.anchor(),
        m.prior_direction(),
        &[],
    )
    .expect("the end of the line attracts the pointer");
    assert_eq!(snap.kind, Kind::Endpoint);
    assert_eq!(snap.point, DVec2::new(1000.0, 0.0));
    let fx = m.handle(
        Input::Press {
            p: snap.point,
            shift: false,
        },
        &ctx(&ed, &tool_settings),
    );
    assert!(fx.is_empty());
    assert_eq!(m.anchor(), Some(DVec2::new(1000.0, 0.0)));

    // Second click: far from every point, so no snap and the raw model point is used.
    let second_raw = view.model_of(DVec2::new(450.0, 100.0));
    assert!(
        snap_cursor(
            ed.doc(),
            second_raw,
            tol,
            200.0,
            &snap_settings,
            &Smart::default(),
            m.anchor(),
            m.prior_direction(),
            &[],
        )
        .is_none()
    );
    let fx = m.handle(
        Input::Press {
            p: second_raw,
            shift: false,
        },
        &ctx(&ed, &tool_settings),
    );
    let messages = apply(fx, &mut ed, &mut selection, None);
    assert!(messages.is_empty());
    assert_eq!(selection.len(), 1, "the new line is selected");
    let Some(Shape::Line { a, b }) = shapes(&ed).pop() else {
        panic!("a line was made");
    };
    assert_eq!(a, DVec2::new(1000.0, 0.0));
    assert!(b.distance(second_raw) < 1e-9);
    assert!(!m.is_busy());
}

/// The Data bar flow: type X and Y, press Enter for the first point, then Length and Angle for the
/// second point. Each Enter is a click at the typed point.
#[test]
fn typing_values_in_the_data_bar_draws_an_exact_line() {
    let mut ed = Editor::default();
    let settings = ToolSettings::default();
    let mut m = ToolMachine::default();
    m.set_tool(Tool::Line);
    let mut entry = DataEntry::default();
    let mut selection = Vec::new();
    let cursor = DVec2::new(7.0, 7.0);

    // Tab activates the entry. Before the first click the fields are X and Y.
    entry.sync(m.tool(), m.anchor().is_some(), false);
    entry.activate();
    assert!(entry.active && entry.want_focus);
    assert_eq!(entry.text.len(), 2);
    assert!(!entry.any_typed());
    entry.text[0] = "100".into();
    entry.text[1] = "200,5".into(); // a comma is a decimal point
    assert!(entry.any_typed());
    let p = databar::target_point(m.tool(), m.anchor(), cursor, &entry.values(), false).unwrap();
    assert_eq!(p, DVec2::new(100.0, 200.5));
    m.handle(Input::Press { p, shift: false }, &ctx(&ed, &settings));
    entry.clear_text();
    assert_eq!(m.anchor(), Some(p));

    // After the click the fields are Length and Angle. An empty field follows the cursor.
    entry.sync(m.tool(), m.anchor().is_some(), false);
    assert_eq!(entry.text.len(), 2);
    entry.cycle(false);
    assert_eq!(entry.focus, 1);
    entry.cycle(false);
    assert_eq!(entry.focus, 0);
    entry.cycle(true);
    assert_eq!(entry.focus, 1);
    entry.text[0] = "500".into();
    entry.text[1] = "90".into();
    let q = databar::target_point(m.tool(), m.anchor(), cursor, &entry.values(), false).unwrap();
    assert!(q.distance(DVec2::new(100.0, 700.5)) < 1e-9, "{q}");
    let fx = m.handle(Input::Press { p: q, shift: false }, &ctx(&ed, &settings));
    apply(fx, &mut ed, &mut selection, None);
    let Some(Shape::Line { a, b }) = shapes(&ed).pop() else {
        panic!("a line was made");
    };
    assert_eq!(a, DVec2::new(100.0, 200.5));
    assert!(b.distance(DVec2::new(100.0, 700.5)) < 1e-9);

    // The operation is over: the entry leaves its mode.
    entry.deactivate();
    assert!(!entry.active && !entry.want_focus);
}
