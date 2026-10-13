//! Helpers for the tests of this crate: random edits of a document.

use kadakas_core::change::Origin;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::ObjectId;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::history::NewVersion;

pub fn cmd(v: Value) -> Command {
    serde_json::from_value(v).unwrap()
}

pub fn editor() -> Editor {
    Editor::with_sequential_ids()
}

pub fn circle(ed: &mut Editor, x: f64) -> ObjectId {
    let out = ed
        .execute(&cmd(json!({
            "op": "add_object",
            "shape": {"kind": "circle", "center": [x, 0.0], "radius": 2.0}
        })))
        .unwrap();
    ObjectId(out.created[0])
}

pub fn new_version(name: &str) -> NewVersion {
    NewVersion {
        name: name.into(),
        note: None,
        author: "tester".into(),
        time_ms: 0,
        origin: Origin::User,
        base_revision: 0,
        auto: false,
    }
}

/// A small edit of a document.
#[derive(Clone, Debug)]
pub enum Step {
    Add(i8),
    Delete(u8),
    Move(u8, i8),
    Back(u8),
    Front(u8),
    Layer(u8),
    Class(u8),
    Recolor(u8, u8),
    Georef,
    Project(u8),
}

pub fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => any::<i8>().prop_map(Step::Add),
        2 => any::<u8>().prop_map(Step::Delete),
        3 => (any::<u8>(), any::<i8>()).prop_map(|(i, d)| Step::Move(i, d)),
        1 => any::<u8>().prop_map(Step::Back),
        1 => any::<u8>().prop_map(Step::Front),
        1 => any::<u8>().prop_map(Step::Layer),
        1 => any::<u8>().prop_map(Step::Class),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(i, c)| Step::Recolor(i, c)),
        1 => Just(Step::Georef),
        1 => any::<u8>().prop_map(Step::Project),
    ]
}

pub fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 0..10)
}

/// Runs the steps; a step that a document refuses is skipped.
pub fn run(ed: &mut Editor, steps: &[Step]) {
    for s in steps {
        let ids: Vec<ObjectId> = ed.doc().objects.keys().copied().collect();
        let pick = |i: &u8| (!ids.is_empty()).then(|| ids[*i as usize % ids.len()]);
        let c = match s {
            Step::Add(x) => Some(json!({"op": "add_object", "shape": {"kind": "circle",
                "center": [f64::from(*x), 1.0], "radius": 2.0}})),
            Step::Delete(i) => pick(i).map(|id| json!({"op": "delete_objects", "ids": [id]})),
            Step::Move(i, d) => pick(i).map(
                |id| json!({"op": "move_objects", "ids": [id], "delta": [f64::from(*d), 0.0]}),
            ),
            Step::Back(i) => pick(i).map(|id| json!({"op": "arrange", "ids": [id], "how": "back"})),
            Step::Front(i) => {
                pick(i).map(|id| json!({"op": "arrange", "ids": [id], "how": "front"}))
            }
            Step::Layer(n) => Some(json!({"op": "add_layer", "name": format!("L{n}")})),
            Step::Class(n) => Some(json!({"op": "add_class", "name": format!("C{n}")})),
            Step::Recolor(i, c) => pick(i).map(|id| {
                json!({"op": "modify_object", "id": id,
                    "attrs": {"pen": [*c, 0, 0, 255]}})
            }),
            Step::Georef => None,
            Step::Project(n) => Some(json!({"op": "set_project", "name": format!("P{n}")})),
        };
        if let Some(c) = c
            && let Ok(c) = serde_json::from_value::<Command>(c)
        {
            let _ = ed.execute(&c);
        }
    }
}
