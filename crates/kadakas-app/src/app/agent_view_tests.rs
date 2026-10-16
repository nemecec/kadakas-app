//! The agent drives the view and points (AGR-02, AGR-03) in the window: headless egui frames and
//! the same handler that the agent server calls.

use egui::{Event, PointerButton, Pos2, RawInput, Rect, vec2};
use kadakas_api::{REASON_PERSON_NAVIGATING, RpcError};
use kadakas_core::command::Command;
use kadakas_core::model::{ObjectId, Shape};
use kadakas_ui_core::agent_view::{ANIM_MS, BUSY_MS, MAX_HIGHLIGHTS};
use serde_json::{Value, json};

use super::*;

const W: f32 = 1000.0;
const H: f32 = 700.0;

struct Rig {
    app: KadakasApp,
    ctx: egui::Context,
    /// Seconds.
    time: f64,
}

impl Rig {
    fn new() -> Rig {
        let mut app = KadakasApp::blank();
        app.prefs.animate_agent_view = false;
        let mut rig = Rig {
            app,
            ctx: egui::Context::default(),
            time: 1.0,
        };
        rig.frame(vec![]);
        rig
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> ObjectId {
        let before = self.app.session.editor.doc().objects.len();
        self.app.exec(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::new(x, y),
                max: DVec2::new(x + w, y + h),
            },
            layer: None,
            class: None,
            attrs: Default::default(),
        });
        let doc = self.app.session.editor.doc();
        assert_eq!(doc.objects.len(), before + 1);
        *doc.objects.keys().last().unwrap()
    }

    /// One frame at the current time, like the window runs it.
    fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
        let input = RawInput {
            time: Some(self.time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(W, H))),
            events,
            ..Default::default()
        };
        let ctx = self.ctx.clone();
        let app = &mut self.app;
        ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            app.agent_view_step(&ctx);
            egui::CentralPanel::no_frame().show(ui, |ui| app.show_dock(ui));
        })
    }

    fn advance(&mut self, seconds: f64) -> egui::FullOutput {
        self.time += seconds;
        self.frame(vec![])
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        let ctx = self.ctx.clone();
        let mut env = self.app.agent_env(&ctx);
        crate::agent_view::call(&mut env, method, params)
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        match self.call(method, params) {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {}", e.message),
        }
    }

    fn err(&mut self, method: &str, params: Value) -> RpcError {
        match self.call(method, params) {
            Ok(v) => panic!("{method} should fail, got {v}"),
            Err(e) => e,
        }
    }

    fn canvas(&self) -> Rect {
        self.app.canvas_rect.expect("the canvas was drawn")
    }

    fn pointer(&self) -> Pos2 {
        self.canvas().center()
    }

    /// A middle-button drag over several frames: the person pans.
    fn drag_frames(&mut self, steps: usize) {
        let from = self.pointer();
        let button = |pressed, pos| Event::PointerButton {
            pos,
            button: PointerButton::Middle,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![Event::PointerMoved(from)]);
        self.frame(vec![button(true, from)]);
        for k in 1..=steps {
            self.time += 0.016;
            self.frame(vec![Event::PointerMoved(from + vec2(6.0 * k as f32, 0.0))]);
        }
    }

    fn release(&mut self) {
        let at = self.pointer() + vec2(60.0, 0.0);
        self.time += 0.016;
        self.frame(vec![Event::PointerButton {
            pos: at,
            button: PointerButton::Middle,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
    }
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn pair(v: &Value) -> (f64, f64) {
    (num(&v[0]), num(&v[1]))
}

fn texts(out: &egui::FullOutput) -> Vec<String> {
    fn walk(s: &egui::Shape, into: &mut Vec<String>) {
        match s {
            egui::Shape::Text(t) => into.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, into)),
            _ => {}
        }
    }
    let mut into = Vec::new();
    for c in &out.shapes {
        walk(&c.shape, &mut into);
    }
    into
}

fn shapes(out: &egui::FullOutput) -> usize {
    fn count(s: &egui::Shape) -> usize {
        match s {
            egui::Shape::Vec(v) => v.iter().map(count).sum(),
            _ => 1,
        }
    }
    out.shapes.iter().map(|c| count(&c.shape)).sum()
}

fn box_of(v: &Value) -> ((f64, f64), (f64, f64)) {
    (pair(&v["min"]), pair(&v["max"]))
}

#[test]
fn view_get_tells_what_the_person_sees_after_a_zoom() {
    let mut r = Rig::new();
    let a = r.rect(0.0, 0.0, 4000.0, 2000.0);
    r.app.session.selection = vec![a];
    r.frame(vec![]);
    let z = r.ok("view.zoom", json!({"box": [0, 0, 20000, 10000]}));
    assert_eq!(z["mode"], "plan");
    r.frame(vec![]);
    let g = r.ok("view.get", json!({}));
    assert_eq!(g["mode"], "plan");
    let (center, zoom) = (pair(&g["plan"]["center"]), num(&g["plan"]["zoom"]));
    assert!(
        (center.0 - 10000.0).abs() < 1e-6 && (center.1 - 5000.0).abs() < 1e-6,
        "{center:?}"
    );
    // 10 percent on each side: the wider side takes 1/1.2 of the canvas.
    let canvas = r.canvas();
    let want = (f64::from(canvas.width()) / 24000.0).min(f64::from(canvas.height()) / 12000.0);
    assert!((zoom - want).abs() < 1e-9, "{zoom} {want}");
    assert!((num(&g["plan"]["mm_per_px"]) * zoom - 1.0).abs() < 1e-9);
    // The visible box contains the box that was asked for.
    let (lo, hi) = box_of(&g["plan"]["visible_box"]);
    assert!(
        lo.0 <= 0.0 && lo.1 <= 0.0 && hi.0 >= 20000.0 && hi.1 >= 10000.0,
        "{lo:?} {hi:?}"
    );
    // The rest of what the person has.
    assert_eq!(g["selection"], json!([a.to_string()]));
    assert!(g["active_layer"]["name"].as_str().is_some());
    assert!(!g["visible_layers"].as_array().unwrap().is_empty());
    assert!(g["visible_classes"].is_array());
    assert_eq!(num(&g["canvas"]["width"]), f64::from(canvas.width()));
    assert_eq!(num(&g["window"]["width"]), f64::from(W));
    assert_eq!(g["back"], 1);
    assert_eq!(g["animating"], false);
    assert_eq!(g["camera"]["fov_deg"], 45.0);
    // The same numbers are in the app.
    assert!((r.app.view.zoom - zoom).abs() < 1e-12);
}

#[test]
fn zoom_to_objects_frames_them_and_names_a_missing_one() {
    let mut r = Rig::new();
    let a = r.rect(0.0, 0.0, 1000.0, 1000.0);
    let b = r.rect(30000.0, 20000.0, 1000.0, 1000.0);
    r.frame(vec![]);
    r.ok(
        "view.zoom",
        json!({"ids": [a.to_string(), b.to_string()], "margin": 0.0}),
    );
    r.frame(vec![]);
    let g = r.ok("view.get", json!({}));
    let (lo, hi) = box_of(&g["plan"]["visible_box"]);
    assert!(
        lo.0 <= 0.0 && lo.1 <= 0.0 && hi.0 >= 31000.0 && hi.1 >= 21000.0,
        "{lo:?} {hi:?}"
    );
    // With no margin the pair fills the canvas in one direction.
    let canvas = r.canvas();
    let fill_x = (hi.0 - lo.0) / (31000.0);
    let fill_y = (hi.1 - lo.1) / (21000.0);
    assert!(
        fill_x.min(fill_y) < 1.0 + 1e-6,
        "{fill_x} {fill_y} {canvas:?}"
    );
    // One object only: the middle is its middle.
    r.ok("view.zoom", json!({"ids": [b.to_string()]}));
    let g = r.ok("view.get", json!({}));
    let c = pair(&g["plan"]["center"]);
    assert!((c.0 - 30500.0).abs() < 1e-6 && (c.1 - 20500.0).abs() < 1e-6);
    // An object that is not there.
    let e = r.err(
        "view.zoom",
        json!({"ids": ["00000000-0000-0000-0000-0000000000ff"]}),
    );
    assert!(e.message.contains("no object with ID"), "{}", e.message);
    // Bad requests are parameter errors.
    assert_eq!(r.err("view.zoom", json!({})).code, -32602);
    assert_eq!(
        r.err("view.zoom", json!({"at": [0, 0], "box": [0, 0, 1, 1]}))
            .code,
        -32602
    );
    assert_eq!(
        r.err("view.zoom", json!({"at": [0, 0], "scale": 0})).code,
        -32602
    );
    assert_eq!(r.err("view.zoom", json!({"idz": []})).code, -32602);
}

#[test]
fn zoom_at_a_place_with_a_scale_and_pan_move_the_middle() {
    let mut r = Rig::new();
    r.rect(0.0, 0.0, 100.0, 100.0);
    r.frame(vec![]);
    r.ok("view.zoom", json!({"at": [5000, 3000], "scale": 0.25}));
    let g = r.ok("view.get", json!({}));
    assert_eq!(pair(&g["plan"]["center"]), (5000.0, 3000.0));
    assert_eq!(num(&g["plan"]["zoom"]), 0.25);
    // `at` alone keeps the zoom.
    r.ok("view.zoom", json!({"at": [100, 200]}));
    let g = r.ok("view.get", json!({}));
    assert_eq!(
        (pair(&g["plan"]["center"]), num(&g["plan"]["zoom"])),
        ((100.0, 200.0), 0.25)
    );
    r.ok("view.pan", json!({"by": [1000, -500]}));
    let g = r.ok("view.get", json!({}));
    assert_eq!(pair(&g["plan"]["center"]), (1100.0, -300.0));
    r.ok("view.pan", json!({"to": [7, 8]}));
    let g = r.ok("view.get", json!({}));
    assert_eq!(pair(&g["plan"]["center"]), (7.0, 8.0));
    assert_eq!(r.err("view.pan", json!({})).code, -32602);
    assert_eq!(r.err("view.pan", json!({"by": [1]})).code, -32602);
    // The zoom limit is reported.
    let z = r.ok("view.zoom", json!({"box": [0, 0, 1e9, 1e9]}));
    assert_eq!(z["clamped"], true);
}

#[test]
fn the_view_changes_do_not_touch_the_document() {
    let mut r = Rig::new();
    let a = r.rect(0.0, 0.0, 100.0, 100.0);
    r.frame(vec![]);
    let (rev, depth) = (
        r.app.session.editor.revision(),
        r.app.session.editor.undo_depth(),
    );
    r.ok("view.zoom", json!({"ids": [a.to_string()]}));
    r.ok("view.pan", json!({"by": [5, 5]}));
    r.ok(
        "view.camera",
        json!({"eye": [5000, -5000, 4000], "target": [0, 0, 0]}),
    );
    r.ok(
        "view.highlight",
        json!({"ids": [a.to_string()], "text": "x"}),
    );
    r.ok("view.highlight_clear", json!({}));
    assert_eq!(r.app.session.editor.revision(), rev);
    assert_eq!(r.app.session.editor.undo_depth(), depth);
}

#[test]
fn the_zoom_is_animated_for_about_250_ms_and_is_deterministic() {
    let run = || {
        let mut r = Rig::new();
        r.app.prefs.animate_agent_view = true;
        r.rect(0.0, 0.0, 100.0, 100.0);
        r.frame(vec![]);
        let from = r.app.view;
        r.ok("view.zoom", json!({"at": [40000, 30000], "scale": 0.5}));
        // Not moved yet: the animation starts with the next frame.
        assert!(r.app.agent_view.animating());
        let g = r.ok("view.get", json!({}));
        assert_eq!(g["animating"], true);
        assert_eq!(
            num(&g["plan"]["zoom"]),
            0.5,
            "view.get tells where the view is going"
        );
        let mut views = vec![];
        for _ in 0..4 {
            r.advance(ANIM_MS / 1000.0 / 3.0);
            views.push((r.app.view.zoom, r.app.view.origin));
        }
        assert!(!r.app.agent_view.animating());
        assert_eq!(r.app.view.zoom, 0.5);
        // In between, the zoom was neither at the start nor at the end.
        assert!(
            views[0].0 > from.zoom && views[0].0 < 0.5,
            "{:?} {}",
            views[0],
            from.zoom
        );
        views
    };
    assert_eq!(run(), run(), "the same calls give the same frames");
}

#[test]
fn the_person_dragging_wins_over_the_agent() {
    let mut r = Rig::new();
    r.rect(0.0, 0.0, 1000.0, 1000.0);
    r.frame(vec![]);
    r.drag_frames(3);
    let view = r.app.view;
    let back = r.app.agent_view.back_len();
    let e = r.err("view.zoom", json!({"at": [99999, 99999], "scale": 0.5}));
    assert_eq!(e.reason, Some(REASON_PERSON_NAVIGATING));
    assert!(e.message.contains("dropped"), "{}", e.message);
    assert_eq!(
        r.err("view.pan", json!({"by": [1, 1]})).reason,
        Some(REASON_PERSON_NAVIGATING)
    );
    assert_eq!(
        r.err(
            "view.camera",
            json!({"eye": [1000, 0, 0], "target": [0, 0, 0]})
        )
        .reason,
        Some(REASON_PERSON_NAVIGATING)
    );
    assert_eq!(r.app.view, view, "nothing moved");
    assert_eq!(r.app.agent_view.back_len(), back, "no view was kept");
    // The error line carries the reason for the agent.
    let line = crate::agent_view::error_line(
        r#"{"jsonrpc":"2.0","id":5,"method":"view.zoom","params":{}}"#,
        &e,
    )
    .unwrap();
    assert!(line.contains("person_navigating"), "{line}");
    // Reading and pointing are never held back.
    r.ok("view.get", json!({}));
    r.ok("view.highlight", json!({"at": [0, 0]}));
    // After the drag and a moment, the agent can move the view again.
    r.release();
    r.advance(BUSY_MS / 1000.0 + 0.1);
    r.ok("view.zoom", json!({"at": [99999, 99999], "scale": 0.5}));
    assert_eq!(
        pair(&r.ok("view.get", json!({}))["plan"]["center"]),
        (99999.0, 99999.0)
    );
}

#[test]
fn the_wheel_of_the_person_stops_a_running_animation() {
    let mut r = Rig::new();
    r.app.prefs.animate_agent_view = true;
    r.rect(0.0, 0.0, 1000.0, 1000.0);
    r.frame(vec![]);
    r.ok("view.zoom", json!({"at": [50000, 50000], "scale": 0.5}));
    r.advance(0.05);
    assert!(r.app.agent_view.animating());
    let at = r.pointer();
    r.time += 0.016;
    r.frame(vec![
        Event::PointerMoved(at),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 80.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    r.advance(0.016);
    assert!(
        !r.app.agent_view.animating(),
        "the person's input ended the animation"
    );
    // The wheel of the person goes on smoothly for a moment; then the view rests.
    for _ in 0..5 {
        r.advance(1.0);
    }
    let view = r.app.view;
    r.advance(1.0);
    assert_eq!(r.app.view, view, "the animation did not take the view back");
}

#[test]
fn back_returns_to_the_view_before_each_change() {
    let mut r = Rig::new();
    let a = r.rect(0.0, 0.0, 500.0, 500.0);
    r.frame(vec![]);
    let start = r.app.view;
    assert!(!r.app.agent_view.can_go_back());
    r.ok("view.zoom", json!({"ids": [a.to_string()]}));
    let first = r.app.view;
    r.ok("view.pan", json!({"by": [3000, 0]}));
    assert_eq!(r.app.agent_view.back_len(), 2);
    let ctx = r.ctx.clone();
    assert!(crate::agent_view::go_back(&mut r.app.agent_env(&ctx)));
    assert_eq!(r.app.view, first);
    assert!(crate::agent_view::go_back(&mut r.app.agent_env(&ctx)));
    assert_eq!(r.app.view, start);
    assert!(!crate::agent_view::go_back(&mut r.app.agent_env(&ctx)));
    // Back is a key and a button too.
    r.ok("view.zoom", json!({"at": [9, 9], "scale": 1.0}));
    r.app.agent_back(&ctx);
    assert_eq!(r.app.view, start);
    assert_eq!(r.app.status, tr!("agent-view-restored"));
    r.app.agent_back(&ctx);
    assert_eq!(r.app.status, tr!("agent-view-nothing-back"));
}

#[test]
fn back_returns_from_the_3d_camera_to_the_plan() {
    let mut r = Rig::new();
    r.rect(0.0, 0.0, 4000.0, 3000.0);
    r.frame(vec![]);
    let plan = r.app.view;
    assert!(r.app.plan());
    let c = r.ok(
        "view.camera",
        json!({"eye": [8000, -9000, 6000], "target": [2000, 1500, 0], "projection": "perspective", "up": [0, 0, 1], "fov": 45}),
    );
    assert_eq!(c["mode"], "3d");
    assert!(!r.app.plan());
    r.app.gpu_ready = true;
    r.frame(vec![]);
    let g = r.ok("view.get", json!({}));
    assert_eq!(g["mode"], "3d");
    assert_eq!(g["camera"]["projection"], "perspective");
    let eye = &g["camera"]["eye"];
    assert!(
        (num(&eye[0]) - 8000.0).abs() < 1.0
            && (num(&eye[1]) + 9000.0).abs() < 1.0
            && (num(&eye[2]) - 6000.0).abs() < 1.0,
        "{eye}"
    );
    assert!((num(&g["camera"]["target"][0]) - 2000.0).abs() < 1.0);
    assert!(g["camera"]["visible_box"].is_object(), "{g}");
    assert_eq!(g["plan"]["visible_box"], Value::Null);
    // A zoom to an object in 3D keeps the direction and frames it.
    let a = *r.app.session.editor.doc().objects.keys().next().unwrap();
    let before = r.app.view3d.camera;
    r.ok("view.zoom", json!({"ids": [a.to_string()]}));
    let after = r.app.view3d.camera;
    assert_eq!((after.yaw, after.pitch), (before.yaw, before.pitch));
    // The camera works in scene coordinates (the middle of the plan is its origin); the
    // answers are in drawing coordinates.
    assert!(after.target.length() < 50.0, "{:?}", after.target);
    let g = r.ok("view.get", json!({}));
    assert!(
        (num(&g["camera"]["target"][0]) - 2000.0).abs() < 50.0,
        "{g}"
    );
    assert!(
        (num(&g["camera"]["target"][1]) - 1500.0).abs() < 50.0,
        "{g}"
    );
    // The 3D camera has no roll and a fixed field of view.
    assert_eq!(
        r.err(
            "view.camera",
            json!({"eye": [1000, 0, 0], "target": [0, 0, 0], "fov": 90})
        )
        .code,
        -32602
    );
    assert_eq!(
        r.err(
            "view.camera",
            json!({"eye": [1000, 0, 0], "target": [0, 0, 0], "up": [0, 0, -1]})
        )
        .code,
        -32602
    );
    // Back, twice: the plan view again, with the same zoom.
    let ctx = r.ctx.clone();
    assert!(crate::agent_view::go_back(&mut r.app.agent_env(&ctx)));
    assert!(crate::agent_view::go_back(&mut r.app.agent_env(&ctx)));
    assert!(r.app.plan());
    assert_eq!(r.app.view, plan);
}

#[test]
fn a_view_set_through_the_session_is_kept_for_back() {
    let mut r = Rig::new();
    r.rect(0.0, 0.0, 500.0, 500.0);
    r.frame(vec![]);
    let start = r.app.view;
    r.app
        .session
        .handle(
            "saved_view.save",
            json!({"name": "Far", "zoom": 0.01, "pan": [90000, 90000]}),
        )
        .unwrap();
    let ctx = r.ctx.clone();
    // What the agent handler of the window does around a call of the session.
    let memo = crate::agent_view::memo_for_session_change(&r.app.agent_env(&ctx)).unwrap();
    r.app
        .session
        .handle("view.set", json!({"view": "Far"}))
        .unwrap();
    crate::agent_view::keep_if_changed(&mut r.app.agent_env(&ctx), memo);
    assert_eq!(r.app.agent_view.back_len(), 1);
    // A call that changes nothing keeps nothing.
    let memo = crate::agent_view::memo_for_session_change(&r.app.agent_env(&ctx)).unwrap();
    r.app.session.take_view_changed();
    r.app.session.take_applied_view();
    crate::agent_view::keep_if_changed(&mut r.app.agent_env(&ctx), memo);
    assert_eq!(r.app.agent_view.back_len(), 1);
    r.app.agent_back(&ctx);
    assert_eq!(r.app.view, start);
    // And the person's drag holds it back as well.
    r.drag_frames(2);
    assert_eq!(
        crate::agent_view::memo_for_session_change(&r.app.agent_env(&ctx))
            .err()
            .and_then(|e| e.reason),
        Some(REASON_PERSON_NAVIGATING)
    );
}

#[test]
fn pointers_are_drawn_and_end_by_their_until() {
    let mut r = Rig::new();
    let a = r.rect(1000.0, 1000.0, 3000.0, 2000.0);
    r.frame(vec![]);
    let base = shapes(&r.frame(vec![]));
    let rev = r.app.session.editor.revision();
    let timed = r.ok(
        "view.highlight",
        json!({"ids": [a.to_string()], "text": "Look", "color": "red", "until": 1500}),
    );
    let change = r.ok(
        "view.highlight",
        json!({"at": [2000, 2000], "text": "Here", "until": "next_change"}),
    );
    let kept = r.ok("view.highlight", json!({"box": [0, 0, 500, 500]}));
    assert_eq!(kept["count"], 3);
    let out = r.frame(vec![]);
    assert!(
        shapes(&out) > base + 10,
        "the pointers are drawn: {} vs {base}",
        shapes(&out)
    );
    let t = texts(&out);
    assert!(
        t.iter().any(|x| x.contains("Look")) && t.iter().any(|x| x.contains("Here")),
        "{t:?}"
    );
    // The timed one ends after its time, the others stay.
    r.advance(1.0);
    assert_eq!(r.app.agent_view.highlights.len(), 3);
    r.advance(1.0);
    assert!(
        r.app
            .agent_view
            .highlights
            .get(timed["id"].as_u64().unwrap() as u32)
            .is_none()
    );
    assert_eq!(r.app.agent_view.highlights.len(), 2);
    // The next change of the document ends `next_change`.
    // Any edit counts. This one draws nothing.
    r.app
        .session
        .handle("project.set", json!({"name": "A project"}))
        .unwrap();
    assert_ne!(r.app.session.editor.revision(), rev);
    r.advance(0.1);
    assert!(
        r.app
            .agent_view
            .highlights
            .get(change["id"].as_u64().unwrap() as u32)
            .is_none()
    );
    assert_eq!(r.app.agent_view.highlights.len(), 1, "dismissed stays");
    // The agent clears one, then all.
    let id = kept["id"].as_u64().unwrap();
    assert_eq!(
        r.ok("view.highlight_clear", json!({"id": id}))["cleared"],
        1
    );
    assert_eq!(
        r.err("view.highlight_clear", json!({"id": id})).code,
        kadakas_api::RpcError::not_found("x").code
    );
    r.ok("view.highlight", json!({"at": [1, 1]}));
    r.ok("view.highlight", json!({"at": [2, 2]}));
    assert_eq!(r.ok("view.highlight_clear", json!({}))["cleared"], 2);
    // The cursor of the agent stays for a while after its pointers; then nothing is left.
    r.advance(130.0);
    let out = r.frame(vec![]);
    assert_eq!(shapes(&out), base, "nothing is left on the screen");
}

#[test]
fn a_deleted_object_takes_its_pointer_with_it() {
    let mut r = Rig::new();
    let a = r.rect(0.0, 0.0, 1000.0, 1000.0);
    r.frame(vec![]);
    r.ok(
        "view.highlight",
        json!({"ids": [a.to_string()], "text": "Gone soon"}),
    );
    r.app.exec(&Command::DeleteObjects { ids: vec![a] });
    r.advance(0.1);
    assert!(r.app.agent_view.highlights.is_empty());
}

#[test]
fn pointers_have_bounds_and_say_so() {
    let mut r = Rig::new();
    let a = r.rect(0.0, 0.0, 1000.0, 1000.0);
    r.frame(vec![]);
    let e = r.err(
        "view.highlight",
        json!({"at": [0, 0], "text": "x".repeat(81)}),
    );
    assert_eq!(e.code, -32602);
    assert!(e.message.contains("80"), "{}", e.message);
    assert_eq!(
        r.err(
            "view.highlight",
            json!({"at": [0, 0], "text": "two\nlines"})
        )
        .code,
        -32602
    );
    assert_eq!(
        r.err("view.highlight", json!({"at": [0, 0], "color": "pink"}))
            .code,
        -32602
    );
    assert_eq!(
        r.err("view.highlight", json!({"at": [0, 0], "until": 0}))
            .code,
        -32602
    );
    assert_eq!(
        r.err(
            "view.highlight",
            json!({"ids": [a.to_string()], "box": [0, 0, 1, 1]})
        )
        .code,
        -32602
    );
    let missing = r.err(
        "view.highlight",
        json!({"ids": ["00000000-0000-0000-0000-0000000000ee"]}),
    );
    assert!(missing.message.contains("no object"), "{}", missing.message);
    assert!(
        r.app.agent_view.highlights.is_empty(),
        "a refused pointer leaves nothing"
    );
    for i in 0..MAX_HIGHLIGHTS {
        r.ok("view.highlight", json!({"at": [i, 0]}));
    }
    let full = r.err("view.highlight", json!({"at": [0, 0]}));
    assert!(full.message.contains("32"), "{}", full.message);
    assert_eq!(r.app.agent_view.highlights.len(), MAX_HIGHLIGHTS);
    // `view.get` lists them.
    let g = r.ok("view.get", json!({}));
    assert_eq!(g["highlights"].as_array().unwrap().len(), MAX_HIGHLIGHTS);
    assert_eq!(g["highlights"][0]["kind"], "place");
    assert_eq!(g["highlights"][0]["color"], "agent");
    assert_eq!(g["highlights"][0]["until"], "dismissed");
}

#[test]
fn the_agent_shows_in_the_people_palette_and_has_a_cursor() {
    let mut r = Rig::new();
    r.rect(0.0, 0.0, 1000.0, 1000.0);
    r.frame(vec![]);
    let people = |r: &mut Rig| {
        let ctx = r.ctx.clone();
        let now_ms = r.time * 1000.0;
        let mut found = Vec::new();
        let input = RawInput {
            time: Some(r.time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(W, H))),
            ..Default::default()
        };
        let app = &r.app;
        let out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::no_frame().show(ui, |ui| {
                app.collab.people_ui(ui, now_ms as u64);
                crate::agent_view_ui::people_row(ui, &app.agent_view, now_ms);
            });
        });
        found.extend(texts(&out));
        found
    };
    assert!(
        !people(&mut r).iter().any(|t| t.contains("Agent")),
        "before it did anything"
    );
    r.ok(
        "view.highlight",
        json!({"at": [4000, 3000], "text": "Check"}),
    );
    let t = people(&mut r);
    assert!(t.iter().any(|x| x == "Agent"), "{t:?}");
    assert!(t.iter().any(|x| x.contains("points at 1 pointer")), "{t:?}");
    // The cursor sits at the place and is drawn on the plan with the name.
    let peer = r
        .app
        .agent_view
        .presence
        .peer(r.time * 1000.0, "Agent".into())
        .unwrap();
    assert_eq!(peer.cursor, Some([4000.0, 3000.0]));
    assert_eq!(peer.color, kadakas_ui_core::agent_view::AGENT_COLOR);
    let out = r.frame(vec![]);
    assert!(
        texts(&out).iter().any(|x| x == "Agent"),
        "the cursor label is drawn"
    );
    // A zoom moves the cursor to the place that it shows.
    r.ok("view.zoom", json!({"at": [100, 200]}));
    let peer = r
        .app
        .agent_view
        .presence
        .peer(r.time * 1000.0, "Agent".into())
        .unwrap();
    assert_eq!(peer.cursor, Some([100.0, 200.0]));
    // Quiet after a while, gone after a longer while.
    r.time += 40.0;
    let t = people(&mut r);
    assert!(t.iter().any(|x| x.contains("quiet")), "{t:?}");
    r.time += 200.0;
    assert!(!people(&mut r).iter().any(|x| x.contains("Agent")));
}

#[test]
fn the_panel_lists_pointers_and_a_click_zooms_to_one() {
    let mut r = Rig::new();
    let a = r.rect(20000.0, 20000.0, 1000.0, 1000.0);
    r.frame(vec![]);
    r.ok("view.zoom", json!({"at": [0, 0], "scale": 0.1}));
    r.ok(
        "view.highlight",
        json!({"ids": [a.to_string()], "text": "Not paved", "color": "red"}),
    );
    r.ok("view.highlight", json!({"at": [5, 5], "color": "green"}));
    // An area shows after its first frame (it measures itself then).
    r.frame(vec![]);
    let out = r.frame(vec![]);
    let t = texts(&out);
    assert!(t.iter().any(|x| x.contains("Agent pointers (2)")), "{t:?}");
    // The color is named with its mark, so it is not the only sign.
    assert!(
        t.iter().any(|x| x.contains("1. Not paved (red triangle)")),
        "{t:?}"
    );
    assert!(
        t.iter().any(|x| x.contains("2. Place (green circle)")),
        "{t:?}"
    );
    assert!(
        t.iter().any(|x| x.starts_with("Back")),
        "Back shows after the zoom: {t:?}"
    );
    // The Estonian panel.
    let et = kadakas_l10n::with_language("et", || {
        r.frame(vec![]);
        texts(&r.frame(vec![]))
    });
    assert!(et.iter().any(|x| x.contains("Agendi osutid (2)")), "{et:?}");
    assert!(et.iter().any(|x| x.contains("punane kolmnurk")), "{et:?}");
    // A click on the first pointer zooms to its object; the next key goes on to the second.
    let ctx = r.ctx.clone();
    let first = r.app.agent_view.highlights.items()[0].id;
    r.app
        .agent_panel_action(&ctx, crate::agent_view_ui::PanelAction::Zoom(first));
    let c = pair(&r.ok("view.get", json!({}))["plan"]["center"]);
    assert!(
        (c.0 - 20500.0).abs() < 1e-6 && (c.1 - 20500.0).abs() < 1e-6,
        "{c:?}"
    );
    r.app.agent_next(&ctx);
    let c = pair(&r.ok("view.get", json!({}))["plan"]["center"]);
    assert!(
        (c.0 - 5.0).abs() < 1e-6 && (c.1 - 5.0).abs() < 1e-6,
        "{c:?}"
    );
    r.app.agent_next(&ctx);
    let c = pair(&r.ok("view.get", json!({}))["plan"]["center"]);
    assert!((c.0 - 20500.0).abs() < 1e-6, "the list starts again");
    // Dismiss one, clear the rest.
    r.app
        .agent_panel_action(&ctx, crate::agent_view_ui::PanelAction::Dismiss(first));
    assert_eq!(r.app.agent_view.highlights.len(), 1);
    r.app
        .agent_panel_action(&ctx, crate::agent_view_ui::PanelAction::ClearAll);
    assert!(r.app.agent_view.highlights.is_empty());
    // A pointer that is gone says so.
    r.app
        .agent_panel_action(&ctx, crate::agent_view_ui::PanelAction::Zoom(first));
    assert_eq!(r.app.status, tr!("agent-view-gone"));
}

#[test]
fn pointers_are_drawn_in_3d_too() {
    let mut r = Rig::new();
    let a = r.rect(0.0, 0.0, 4000.0, 3000.0);
    r.app.gpu_ready = true;
    r.ok(
        "view.camera",
        json!({"eye": [9000, -9000, 7000], "target": [2000, 1500, 0]}),
    );
    for _ in 0..3 {
        r.frame(vec![]);
    }
    let base = shapes(&r.frame(vec![]));
    r.ok(
        "view.highlight",
        json!({"ids": [a.to_string()], "text": "Slab"}),
    );
    r.ok(
        "view.highlight",
        json!({"at": [2000, 1500, 0], "text": "Middle"}),
    );
    let out = r.frame(vec![]);
    let t = texts(&out);
    assert!(
        t.iter().any(|x| x.contains("Slab")) && t.iter().any(|x| x.contains("Middle")),
        "{t:?}"
    );
    assert!(
        texts(&out).iter().any(|x| x == "Agent"),
        "the cursor of the agent is drawn in 3D"
    );
    assert!(shapes(&out) > base + 10);
}

#[test]
fn the_same_calls_give_the_same_answers() {
    let run = || {
        let mut r = Rig::new();
        let a = r.rect(0.0, 0.0, 3000.0, 1000.0);
        r.frame(vec![]);
        let mut answers = vec![];
        for (m, p) in [
            ("view.zoom", json!({"ids": [a.to_string()]})),
            ("view.pan", json!({"by": [100, 100]})),
            (
                "view.highlight",
                json!({"ids": [a.to_string()], "text": "A"}),
            ),
            ("view.get", json!({})),
        ] {
            answers.push(r.ok(m, p).to_string());
        }
        answers
    };
    // The IDs of the objects come from the session, so they match between the runs.
    let (a, b) = (run(), run());
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        let strip = |s: &str| {
            let mut v: Value = serde_json::from_str(s).unwrap();
            if let Some(h) = v.get_mut("highlights") {
                *h = json!(h.as_array().map(Vec::len));
            }
            v["selection"] = Value::Null;
            v.to_string()
        };
        assert_eq!(strip(x), strip(y));
    }
}

#[test]
fn the_handler_answers_lines_and_the_headless_methods_stay_with_the_server() {
    let mut r = Rig::new();
    r.rect(0.0, 0.0, 100.0, 100.0);
    r.frame(vec![]);
    let ctx = r.ctx.clone();
    let line = r#"{"jsonrpc":"2.0","id":3,"method":"view.get","params":{}}"#;
    assert_eq!(
        crate::agent_view::method_of(line).as_deref(),
        Some("view.get")
    );
    assert_eq!(
        crate::agent_view::method_of(r#"{"id":1,"method":"objects.list"}"#),
        None
    );
    let out = crate::agent_view::respond(&mut r.app.agent_env(&ctx), line).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["id"], 3);
    assert_eq!(v["result"]["mode"], "plan");
    let bad = crate::agent_view::respond(
        &mut r.app.agent_env(&ctx),
        r#"{"jsonrpc":"2.0","id":4,"method":"view.zoom","params":{}}"#,
    )
    .unwrap();
    let v: Value = serde_json::from_str(&bad).unwrap();
    assert_eq!(v["error"]["code"], -32602);
    // A notification gets no answer.
    assert!(
        crate::agent_view::respond(&mut r.app.agent_env(&ctx), r#"{"method":"view.get"}"#)
            .is_none()
    );
    assert!(crate::agent_view::is_app_method("view.highlight_clear"));
    assert!(!crate::agent_view::is_app_method("view.list"));
    assert!(crate::agent_view::is_session_view_change(
        "saved_view.restore"
    ));
}

#[test]
fn a_new_document_forgets_the_pointers_and_the_views_to_go_back_to() {
    let mut r = Rig::new();
    r.rect(0.0, 0.0, 100.0, 100.0);
    r.frame(vec![]);
    r.ok("view.zoom", json!({"at": [1, 1], "scale": 1.0}));
    r.ok("view.highlight", json!({"at": [1, 1]}));
    r.app.agent_view.reset();
    assert!(r.app.agent_view.highlights.is_empty() && !r.app.agent_view.can_go_back());
    assert!(
        r.app
            .agent_view
            .presence
            .peer(r.time * 1000.0, "Agent".into())
            .is_none()
    );
}

#[test]
fn the_agent_keys_exist_in_the_default_keymap_and_back_works_in_a_read_only_view() {
    let keys = Keymap::defaults();
    assert!(!keys.hint(Action::ViewAgentBack).is_empty());
    assert!(!keys.hint(Action::ViewAgentNext).is_empty());
    assert!(KadakasApp::allowed_when_read_only(Action::ViewAgentBack));
    // The preference is on by default and survives the file.
    assert!(Prefs::default().animate_agent_view);
    let off = Prefs {
        animate_agent_view: false,
        ..Prefs::default()
    };
    let text = serde_json::to_string(&off).unwrap();
    assert!(
        !serde_json::from_str::<Prefs>(&text)
            .unwrap()
            .animate_agent_view
    );
    assert!(
        serde_json::from_str::<Prefs>("{}")
            .unwrap()
            .animate_agent_view
    );
}
