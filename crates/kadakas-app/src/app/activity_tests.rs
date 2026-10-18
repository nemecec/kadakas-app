//! What the agent changed (AGR-05) and the Activity panel (AGR-06) in the window: headless egui
//! frames, with the same session calls that the agent server makes.

use egui::{Pos2, RawInput, Rect, vec2};
use kadakas_core::change::Origin;
use kadakas_ui_core::agent_activity::SeenTarget;
use serde_json::{Value, json};

use super::*;
use crate::agent_activity_ui::{Act, summary_text};

const W: f32 = 1000.0;
const H: f32 = 700.0;

struct Rig {
    app: KadakasApp,
    ctx: egui::Context,
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
        rig.frame();
        rig.time += 1.0;
        rig.frame();
        rig
    }

    fn frame(&mut self) -> egui::FullOutput {
        let input = RawInput {
            time: Some(self.time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(W, H))),
            ..Default::default()
        };
        let ctx = self.ctx.clone();
        let app = &mut self.app;
        ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            app.agent_view_step(&ctx);
            app.activity_step(&ctx);
            egui::CentralPanel::no_frame().show(ui, |ui| app.show_dock(ui));
        })
    }

    fn agent(&mut self, method: &str, params: Value) -> Value {
        match self.app.session.handle_as(7, Origin::Agent, method, params) {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {}", e.message),
        }
    }

    fn person(&mut self, method: &str, params: Value) -> Value {
        match self.app.session.handle_as(0, Origin::User, method, params) {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {}", e.message),
        }
    }

    fn add_rect(&mut self, who: Origin, x: f64) -> ObjectId {
        let cmd = json!({"command": {"op": "add_object", "shape":
            {"kind": "rect", "min": [x, 0.0], "max": [x + 1000.0, 1000.0]}}});
        let r = match who {
            Origin::Agent => self.agent("execute", cmd),
            _ => self.person("execute", cmd),
        };
        ObjectId(kadakas_core::uid::Uid::parse(r["created"][0].as_str().unwrap()).unwrap())
    }

    fn act(&mut self, a: Act) {
        let ctx = self.ctx.clone();
        self.app.activity_act(&ctx, a);
    }

    fn review(&self) -> std::sync::Arc<kadakas_ui_core::agent_activity::Review> {
        self.app.activity_review.clone().expect("a frame ran")
    }

    fn step_ids(&self) -> Vec<u64> {
        self.app
            .session
            .activity
            .steps()
            .iter()
            .map(|s| s.id)
            .collect()
    }
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

#[test]
fn a_change_of_the_agent_is_highlighted_until_the_person_marks_it_as_seen() {
    let mut r = Rig::new();
    assert!(r.app.activity_draw().is_none(), "nothing to review yet");
    let mine = r.add_rect(Origin::User, 0.0);
    let theirs = r.add_rect(Origin::Agent, 5000.0);
    r.frame();
    let review = r.review();
    assert_eq!(review.set.counts().added, 1);
    assert!(review.set.get(theirs).is_some() && review.set.get(mine).is_none());
    // The plan gets the overlay of the compare: the same colors, patterns and marks.
    let draw = r.app.activity_draw().expect("the changes are drawn");
    assert_eq!(
        draw.overlay.status_of(theirs),
        Some(kadakas_review::overlay::Status::Added)
    );
    assert_eq!(
        draw.style.unchanged,
        kadakas_review::overlay::UnchangedMode::Show
    );
    // The person can switch the highlight off and on.
    r.app.activity_ui.model.show = false;
    assert!(r.app.activity_draw().is_none());
    r.app.activity_ui.model.show = true;
    // Seen: the highlight goes, the step stays in the list.
    r.act(Act::Seen(SeenTarget::All));
    r.frame();
    assert!(r.app.activity_draw().is_none());
    assert_eq!(r.step_ids().len(), 1);
    assert!(r.app.session.activity.steps()[0].seen);
}

#[test]
fn the_highlight_follows_the_document_when_the_person_edits_afterwards() {
    let mut r = Rig::new();
    let a = r.add_rect(Origin::Agent, 0.0);
    r.frame();
    assert_eq!(r.review().set.counts().added, 1);
    r.person(
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [a.to_string()]}}),
    );
    r.time += 1.0;
    r.frame();
    assert!(
        r.review().set.is_empty(),
        "the person deleted it: not the agent's removal"
    );
}

#[test]
fn the_panel_lists_the_steps_in_both_languages() {
    let mut r = Rig::new();
    r.add_rect(Origin::Agent, 0.0);
    r.agent(
        "execute",
        json!({"command": {"op": "add_layer", "name": "Planting"}}),
    );
    r.app.activity_ui.model.open = true;
    for (lang, step, empty_title) in [
        ("en-US", "Step 2", "Agent Activity"),
        ("et", "Samm 2", "Agendi tegevus"),
    ] {
        kadakas_l10n::with_language(lang, || r.frame());
        let out = kadakas_l10n::with_language(lang, || r.frame());
        let t = texts(&out);
        assert!(t.iter().any(|x| x.contains(step)), "{lang}: {t:?}");
        assert!(t.iter().any(|x| x.contains(empty_title)), "{lang}: {t:?}");
        // The method and the summary of the step are shown.
        assert!(t.iter().any(|x| x.contains("execute")), "{lang}: {t:?}");
    }
    let out = kadakas_l10n::with_language("en-US", || r.frame());
    let t = texts(&out);
    assert!(t.iter().any(|x| x == "Added layer Planting"), "{t:?}");
}

#[test]
fn the_summary_in_the_window_tells_what_the_summary_of_the_api_tells() {
    let mut r = Rig::new();
    let a = r.add_rect(Origin::Agent, 0.0);
    let b = r.add_rect(Origin::Agent, 5000.0);
    r.agent(
        "execute",
        json!({"command": {"op": "batch", "commands": [
            {"op": "add_object", "shape": {"kind": "circle", "center": [0.0, 0.0], "radius": 10.0}},
            {"op": "move_objects", "ids": [a.to_string()], "delta": [10.0, 0.0]},
            {"op": "delete_objects", "ids": [b.to_string()]},
            {"op": "add_layer", "name": "Trees"},
        ]}}),
    );
    r.agent(
        "execute",
        json!({"command": {"op": "add_layer", "name": "Beds"}}),
    );
    let steps: Vec<_> = r.app.session.activity.steps().iter().cloned().collect();
    assert_eq!(steps.len(), 4);
    for step in &steps {
        let en = kadakas_l10n::with_language("en-US", || summary_text(&step.summary));
        assert_eq!(en, step.summary.text(), "step {}", step.id);
        let et = kadakas_l10n::with_language("et", || summary_text(&step.summary));
        assert!(
            !et.is_empty() && et != en || step.summary.text() == et,
            "{et}"
        );
    }
}

#[test]
fn revert_takes_an_old_step_back_and_keeps_what_came_after() {
    let mut r = Rig::new();
    let first = r.add_rect(Origin::Agent, 0.0);
    r.add_rect(Origin::Agent, 5000.0);
    r.frame();
    let id = r.step_ids()[0];
    r.act(Act::Revert {
        step: id,
        skip: false,
    });
    assert!(r.app.session.editor.doc().object(first).is_none());
    assert_eq!(
        r.app.session.editor.doc().objects.len(),
        1,
        "the later step stays"
    );
    assert!(r.app.status.contains(&id.to_string()), "{}", r.app.status);
    // One undo of the person brings it back.
    r.app.undo();
    assert!(r.app.session.editor.doc().object(first).is_some());
}

#[test]
fn revert_is_refused_and_says_why_when_the_person_changed_the_object() {
    let mut r = Rig::new();
    let a = r.add_rect(Origin::Agent, 0.0);
    r.add_rect(Origin::Agent, 5000.0);
    r.person(
        "execute",
        json!({"command": {"op": "move_objects", "ids": [a.to_string()], "delta": [100.0, 0.0]}}),
    );
    r.frame();
    let id = r.step_ids()[0];
    let count = r.app.session.editor.doc().objects.len();
    let depth = r.app.session.editor.undo_depth();
    r.act(Act::Revert {
        step: id,
        skip: false,
    });
    assert_eq!(r.app.session.editor.doc().objects.len(), count);
    assert_eq!(r.app.session.editor.undo_depth(), depth, "nothing changed");
    assert!(r.app.status.contains("after this step"), "{}", r.app.status);
    // In Estonian too.
    kadakas_l10n::with_language("et", || {
        r.act(Act::Revert {
            step: id,
            skip: false,
        });
    });
    assert!(
        r.app.status.contains("pärast seda sammu"),
        "{}",
        r.app.status
    );
}

#[test]
fn undo_back_to_a_step_asks_nothing_when_only_agent_steps_are_above() {
    let mut r = Rig::new();
    r.add_rect(Origin::Agent, 0.0);
    r.add_rect(Origin::Agent, 5000.0);
    r.frame();
    let id = r.step_ids()[0];
    r.act(Act::UndoBack {
        step: id,
        expect: 2,
    });
    assert!(r.app.session.editor.doc().objects.is_empty());
    assert!(r.app.status.contains(&id.to_string()), "{}", r.app.status);
    r.time += 1.0;
    r.frame();
    assert!(r.review().set.is_empty() || r.review().set.counts().added == 0);
}

#[test]
fn the_next_change_selects_the_object_and_moves_the_view_to_it() {
    let mut r = Rig::new();
    let far = r.add_rect(Origin::Agent, 90_000.0);
    r.frame();
    let before = r.app.view;
    r.act(Act::Next);
    r.time += 1.0;
    r.frame();
    assert_eq!(r.app.session.selection, vec![far]);
    assert!(
        r.app.view.origin != before.origin || r.app.view.zoom != before.zoom,
        "the view moved"
    );
    assert!(r.app.status.contains("Step 1"), "{}", r.app.status);
    // A removed object: the view goes to where it was, nothing is selected.
    let gone = r.add_rect(Origin::Agent, -90_000.0);
    r.agent(
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [gone.to_string()]}}),
    );
    r.frame();
    let review = r.review();
    let removed_only = review.set.get(gone);
    assert!(
        removed_only.is_none(),
        "added and removed again is no change"
    );
}

#[test]
fn the_notice_offers_the_review_and_the_panel_opens_from_it() {
    let mut r = Rig::new();
    r.add_rect(Origin::Agent, 0.0);
    kadakas_l10n::with_language("en-US", || r.frame());
    let out = kadakas_l10n::with_language("en-US", || r.frame());
    assert!(
        texts(&out)
            .iter()
            .any(|t| t.contains("changed the drawing in 1 step")),
        "{:?}",
        texts(&out)
    );
    r.act(Act::Open);
    assert!(r.app.activity_ui.model.open);
    kadakas_l10n::with_language("et", || r.frame());
    let out = kadakas_l10n::with_language("et", || r.frame());
    assert!(texts(&out).iter().any(|t| t.contains("Agendi tegevus")));
}

#[test]
fn the_changes_are_drawn_in_3d_with_a_box_a_pattern_and_a_mark() {
    let mut r = Rig::new();
    let id = r.add_rect(Origin::Agent, 0.0);
    r.frame();
    let review = r.review();
    let scene = r.app.session.scene_for_pixel_size(10.0);
    r.app.view3d.fit(scene.bounds, 1.4);
    let vp = r.app.view3d.camera.view_proj(1.4, scene.bounds);
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(700.0, 500.0));
    let ctx = egui::Context::default();
    let out = kadakas_l10n::with_language("en-US", || {
        ctx.run_ui(
            RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                crate::agent_activity_ui::paint_3d(
                    &ui.painter().clone(),
                    rect,
                    vp,
                    &scene,
                    r.app.session.editor.doc(),
                    &review,
                );
            },
        )
    });
    let t = texts(&out);
    assert!(
        t.iter().any(|x| x == "+"),
        "the mark of an added object: {t:?}"
    );
    assert!(t.iter().any(|x| x == "Added 1"), "{t:?}");
    let _ = id;
    // Nothing to draw: nothing drawn.
    let none = kadakas_ui_core::agent_activity::ActivityModel::default().review(
        &Default::default(),
        r.app.session.editor.doc(),
        1,
        false,
        0.0,
    );
    let out = ctx.run_ui(
        RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ui| {
            crate::agent_activity_ui::paint_3d(
                &ui.painter().clone(),
                rect,
                vp,
                &scene,
                r.app.session.editor.doc(),
                &none,
            );
        },
    );
    assert!(texts(&out).is_empty());
}

#[test]
fn a_reverted_step_says_so_and_the_hold_notice_offers_redo_and_release() {
    let mut r = Rig::new();
    r.add_rect(Origin::Agent, 0.0);
    r.add_rect(Origin::Agent, 5000.0);
    r.frame();
    let first = r.step_ids()[0];
    r.act(Act::Revert {
        step: first,
        skip: false,
    });
    r.app.activity_ui.model.open = true;
    r.frame();
    kadakas_l10n::with_language("en-US", || r.frame());
    // The checks wait for a quiet moment, then the panel shows the state.
    r.time += 1.0;
    kadakas_l10n::with_language("en-US", || r.frame());
    r.time += 1.0;
    let out = kadakas_l10n::with_language("en-US", || r.frame());
    assert!(
        texts(&out).iter().any(|t| t == "reverted"),
        "{:?}",
        texts(&out)
    );
    // The person undoes back to the second step; a step of their own is above it.
    let second = r.step_ids()[1];
    r.person(
        "execute",
        json!({"command": {"op": "add_object", "shape":
            {"kind": "rect", "min": [0.0, 9000.0], "max": [10.0, 9010.0]}}}),
    );
    r.time += 1.0;
    r.frame();
    let expect = r.app.session.revert_check(second).unwrap().above + 1;
    r.act(Act::UndoBack {
        step: second,
        expect,
    });
    assert!(r.app.session.agent_hold());
    kadakas_l10n::with_language("en-US", || r.frame());
    let out = kadakas_l10n::with_language("en-US", || r.frame());
    assert!(
        texts(&out).iter().any(|t| t.contains("The agent waits")),
        "{:?}",
        texts(&out)
    );
    let out = kadakas_l10n::with_language("et", || r.frame());
    assert!(texts(&out).iter().any(|t| t.contains("Agent ootab")));
    // Asking with a number that is not the history any more does nothing.
    r.act(Act::ReleaseHold);
    assert!(!r.app.session.agent_hold());
    let depth = r.app.session.editor.undo_depth();
    r.act(Act::UndoBack {
        step: second,
        expect: 99,
    });
    assert!(
        r.app.status.contains("undo history changed"),
        "{}",
        r.app.status
    );
    assert_eq!(r.app.session.editor.undo_depth(), depth);
    // Redo everything ends a hold at once.
    let before = r.app.session.editor.doc().objects.len();
    r.act(Act::RedoAll);
    assert!(r.app.session.editor.doc().objects.len() >= before);
    assert!(!r.app.session.editor.can_redo());
}

#[test]
fn the_keys_open_the_panel_and_step_through_the_changes() {
    let mut r = Rig::new();
    r.add_rect(Origin::Agent, 0.0);
    r.add_rect(Origin::Agent, 8000.0);
    r.frame();
    let ctx = r.ctx.clone();
    r.app.run_action(&ctx, Action::ViewAgentActivity);
    assert!(r.app.activity_ui.model.open);
    r.app.run_action(&ctx, Action::ViewAgentChangeNext);
    let first = r.app.session.selection.clone();
    r.app.run_action(&ctx, Action::ViewAgentChangeNext);
    assert_ne!(first, r.app.session.selection);
    r.app.run_action(&ctx, Action::ViewAgentChangePrevious);
    assert_eq!(first, r.app.session.selection);
    r.app.run_action(&ctx, Action::ViewAgentChangesToggle);
    assert!(!r.app.activity_ui.model.show);
    r.app.run_action(&ctx, Action::ViewAgentChangesSeen);
    r.frame();
    assert!(r.review().set.is_empty());
    for a in [
        Action::ViewAgentActivity,
        Action::ViewAgentChangeNext,
        Action::ViewAgentChangePrevious,
        Action::ViewAgentChangesSeen,
        Action::ViewAgentChangesToggle,
    ] {
        assert!(!r.app.keys.hint(a).is_empty(), "{a:?} has a key");
    }
}

#[test]
fn a_one_object_step_names_what_changed_in_both_languages() {
    let mut r = Rig::new();
    let st = r.person(
        "hardscape_style.create",
        json!({"name": "Pavers", "thickness": 80,
               "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 5}}),
    )["id"]
        .clone();
    let id = r.person(
        "hardscape.add",
        json!({"boundary": [[0, 0], [8000, 0], [8000, 4000], [0, 4000]], "style": st}),
    )["id"]
        .clone();
    r.person(
        "hardscape.split",
        json!({"id": id, "lines": [[[3000, -50], [3000, 4050]]],
               "zones": [{"seed": [6000, 2000], "name": "Aisle", "angle": 0}]}),
    );
    r.agent(
        "hardscape.zone_set",
        json!({"id": id, "zone": 1, "angle": 45}),
    );
    let step = r.app.session.activity.steps().back().unwrap().clone();
    let en = kadakas_l10n::with_language("en-US", || summary_text(&step.summary));
    assert_eq!(en, step.summary.text());
    assert_eq!(
        en,
        "Changed hardscape 'Pavers': zone 'Aisle' angle 0° → 45°"
    );
    let et = kadakas_l10n::with_language("et", || summary_text(&step.summary));
    assert!(et.contains("tsoon 'Aisle' nurk 0° → 45°"), "{et}");
}

#[test]
fn a_style_step_names_the_style_and_the_field_in_both_languages() {
    let mut r = Rig::new();
    let st = r.person(
        "hardscape_style.create",
        json!({"name": "Pavers", "thickness": 80,
               "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 3}}),
    )["id"]
        .clone();
    r.agent(
        "hardscape_style.update",
        json!({"id": st, "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 5}}),
    );
    let step = r.app.session.activity.steps().back().unwrap().clone();
    let en = kadakas_l10n::with_language("en-US", || summary_text(&step.summary));
    assert_eq!(en, step.summary.text());
    assert_eq!(en, "Changed hardscape style 'Pavers': joint 3 → 5 mm");
    let et = kadakas_l10n::with_language("et", || summary_text(&step.summary));
    assert!(
        et.contains("kõvakatte stiil 'Pavers'") && et.contains("vuuk 3 → 5 mm"),
        "{et}"
    );
}
