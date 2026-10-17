//! What the agent changed (AGR-05, AGR-06): the activity log of a session, the version at the
//! start of an agent session, the revert of an old step and `agent.activity`.

use kadakas_api::{METHODS, Session, SessionLimits, StepError};
use kadakas_core::change::Origin;
use kadakas_core::command::Refusal;
use serde_json::{Value, json};

const AGENT: u64 = 7;

fn agent(s: &mut Session, conn: u64, m: &str, p: Value) -> Value {
    match s.handle_as(conn, Origin::Agent, m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {}", e.message),
    }
}

fn person(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle_as(0, Origin::User, m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {}", e.message),
    }
}

fn add_line(x: f64) -> Value {
    json!({"command": {"op": "add_object", "shape": {"kind": "line", "a": [x, 0.0], "b": [x + 1.0, 1.0]}}})
}

fn created(v: &Value) -> String {
    v["created"][0].as_str().unwrap().to_string()
}

fn steps(s: &mut Session) -> Vec<Value> {
    agent(s, 99, "agent.activity", json!({}))["steps"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn the_method_is_listed_and_reads_only() {
    assert!(
        METHODS
            .iter()
            .any(|(n, d)| *n == "agent.activity" && !d.is_empty())
    );
    let mut s = Session::with_sequential_ids();
    let r = agent(&mut s, AGENT, "agent.activity", json!({}));
    assert_eq!(r["total"], 0);
    assert!(!s.editor.can_undo());
    let err = s
        .handle_as(AGENT, Origin::Agent, "agent.activity", json!({"stepp": 1}))
        .unwrap_err();
    assert_eq!(err.code, -32602);
    let err = s
        .handle_as(AGENT, Origin::Agent, "agent.activity", json!({"step": 5}))
        .unwrap_err();
    assert_eq!(err.kind, "not_found");
}

#[test]
fn a_call_of_the_agent_is_a_step_and_a_call_of_the_person_is_not() {
    let mut s = Session::with_sequential_ids();
    agent(&mut s, AGENT, "document.summary", json!({}));
    person(&mut s, "execute", add_line(0.0));
    assert!(
        steps(&mut s).is_empty(),
        "reads and the person are no steps"
    );
    agent(&mut s, AGENT, "execute", add_line(10.0));
    agent(&mut s, AGENT, "execute", add_line(20.0));
    let list = steps(&mut s);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["method"], "execute");
    assert_eq!(list[0]["counts"]["objects"]["added"], 1);
    assert_eq!(list[0]["summary"], "Added 1 object (1 line)");
    assert_eq!(list[1]["state"], "active");
    assert!(list[0]["time_ms"].as_u64().unwrap() > 0);
    // A failed call is no step.
    let failed = s.handle_as(
        AGENT,
        Origin::Agent,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": ["00000000-0000-0000-0000-0000000000ff"]}}),
    );
    assert!(failed.is_err());
    assert_eq!(steps(&mut s).len(), 2);
    // The list can be cut: only the newest, or the ones after an id.
    let one = agent(&mut s, 99, "agent.activity", json!({"limit": 1}));
    assert_eq!(one["steps"].as_array().unwrap().len(), 1);
    assert_eq!(one["steps"][0]["id"], list[1]["id"]);
    let after = agent(
        &mut s,
        99,
        "agent.activity",
        json!({"since": list[0]["id"]}),
    );
    assert_eq!(after["steps"].as_array().unwrap().len(), 1);
}

#[test]
fn a_step_lists_its_objects_by_how_they_changed() {
    let mut s = Session::with_sequential_ids();
    let a = created(&person(&mut s, "execute", add_line(0.0)));
    let b = created(&person(&mut s, "execute", add_line(5.0)));
    agent(
        &mut s,
        AGENT,
        "execute",
        json!({"command": {"op": "batch", "commands": [
            {"op": "add_object", "shape": {"kind": "line", "a": [50.0, 0.0], "b": [51.0, 1.0]}},
            {"op": "move_objects", "ids": [a], "delta": [3.0, 0.0]},
            {"op": "delete_objects", "ids": [b]},
        ]}}),
    );
    let list = steps(&mut s);
    assert_eq!(list.len(), 1, "a batch is one call and one step");
    let id = list[0]["id"].as_u64().unwrap();
    let one = agent(&mut s, 99, "agent.activity", json!({"step": id}));
    assert_eq!(one["objects"]["added"].as_array().unwrap().len(), 1);
    assert_eq!(one["objects"]["changed"], json!([a]));
    assert_eq!(one["objects"]["removed"], json!([b]));
    assert_eq!(
        one["summary"],
        "Added 1 object; changed 1 object; removed 1 object"
    );
}

#[test]
fn a_version_is_saved_when_a_session_starts_and_not_for_every_call() {
    let mut s = Session::with_sequential_ids();
    assert!(!s.session_version_enabled(), "off unless the host asks");
    s.set_session_version(true);
    // A read does not start a session that needs a version.
    agent(&mut s, AGENT, "document.summary", json!({}));
    assert_eq!(person(&mut s, "version.list", json!({}))["count"], 0);
    agent(&mut s, 11, "execute", add_line(0.0));
    let versions = person(&mut s, "version.list", json!({}));
    assert_eq!(versions["count"], 1);
    assert_eq!(versions["versions"][0]["name"], "Before agent session");
    assert_eq!(versions["versions"][0]["origin"], "agent");
    // The version holds the document as it was before the change.
    let id = versions["versions"][0]["id"].as_u64().unwrap();
    let got = person(&mut s, "version.get", json!({"id": id}));
    assert_eq!(
        got["document"]["objects"].as_array().map_or(0, Vec::len),
        0,
        "{got}"
    );
    agent(&mut s, 11, "execute", add_line(5.0));
    assert_eq!(person(&mut s, "version.list", json!({}))["count"], 1);
    // Another connection is another session.
    agent(&mut s, 12, "execute", add_line(9.0));
    assert_eq!(person(&mut s, "version.list", json!({}))["count"], 2);
    let list = steps(&mut s);
    assert_eq!(list[0]["session_version"], id);
    assert_ne!(list[0]["session"], list[2]["session"]);
    // The versions are no agent steps.
    assert_eq!(list.len(), 3);
    // A closed connection starts a new session at its next call.
    s.drop_connection(11);
    agent(&mut s, 11, "execute", add_line(15.0));
    assert_eq!(person(&mut s, "version.list", json!({}))["count"], 3);
    // An agent that starts with reads still gets its version before its first change.
    agent(&mut s, 20, "describe", json!({}));
    agent(&mut s, 20, "objects.list", json!({}));
    assert_eq!(person(&mut s, "version.list", json!({}))["count"], 3);
    agent(&mut s, 20, "execute", add_line(30.0));
    assert_eq!(person(&mut s, "version.list", json!({}))["count"], 4);
    agent(&mut s, 20, "execute", add_line(35.0));
    assert_eq!(person(&mut s, "version.list", json!({}))["count"], 4);
}

#[test]
fn a_refused_version_does_not_stop_the_agent_and_the_step_says_so() {
    let mut s = Session::with_sequential_ids();
    s.set_session_version(true);
    person(&mut s, "version.create", json!({"name": "Mine"}));
    s.set_limits(SessionLimits {
        max_versions: 1,
        ..SessionLimits::default()
    });
    agent(&mut s, AGENT, "execute", add_line(0.0));
    assert_eq!(s.editor.doc().objects.len(), 1, "the agent worked");
    let list = steps(&mut s);
    assert_eq!(list.len(), 1);
    assert!(
        list[0]["version_note"]
            .as_str()
            .unwrap()
            .contains("no version was saved"),
        "{}",
        list[0]
    );
}

#[test]
fn a_script_run_is_one_step_and_reverts_as_one() {
    let mut s = Session::with_sequential_ids();
    agent(
        &mut s,
        AGENT,
        "script.eval",
        json!({"code": "for (let i = 0; i < 4; i++) kadakas.call('execute', {command: {op: 'add_object', shape: {kind: 'circle', center: [i * 10, 0], radius: 2}}}); return 1;"}),
    );
    let list = steps(&mut s);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["counts"]["objects"]["added"], 4);
    assert_eq!(list[0]["method"], "script.eval");
    // The person draws something; the run reverts alone.
    person(&mut s, "execute", add_line(100.0));
    let id = list[0]["id"].as_u64().unwrap();
    let r = s.revert_agent_step(id, false).unwrap();
    assert_eq!(r.reverted.len(), 4);
    assert_eq!(s.editor.doc().objects.len(), 1);
    // Undo takes the revert back: the run is there again.
    assert!(s.editor.undo());
    assert_eq!(s.editor.doc().objects.len(), 5);
}

#[test]
fn a_revert_is_refused_when_the_person_changed_an_object_since() {
    let mut s = Session::with_sequential_ids();
    let a = created(&agent(&mut s, AGENT, "execute", add_line(0.0)));
    agent(&mut s, AGENT, "execute", add_line(10.0));
    let first = steps(&mut s)[0]["id"].as_u64().unwrap();
    person(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [a], "delta": [5.0, 0.0]}}),
    );
    let check = s.revert_check(first).unwrap();
    assert_eq!(check.conflicts.len(), 1);
    assert!(check.blocked().unwrap().contains("changed later"));
    let count = s.editor.doc().objects.len();
    match s.revert_agent_step(first, false) {
        Err(StepError::Refused(r)) => assert!(r.to_string().contains("changed later")),
        other => panic!("{other:?}"),
    }
    assert_eq!(s.editor.doc().objects.len(), count, "nothing changed");
    let listed = steps(&mut s);
    assert_eq!(listed[0]["revertible"], false);
    let detail = agent(&mut s, 99, "agent.activity", json!({"step": first}));
    assert!(
        detail["blocked"]
            .as_str()
            .unwrap()
            .contains("changed later")
    );
    // The second step is untouched by the person and reverts although a step of the person is
    // above it.
    let second = listed[1]["id"].as_u64().unwrap();
    assert!(s.revert_check(second).unwrap().blocked().is_none());
    s.revert_agent_step(second, false).unwrap();
    assert_eq!(s.editor.doc().objects.len(), 1);
}

#[test]
fn undo_back_to_a_step_takes_the_steps_above_it_and_redo_brings_them_back() {
    let mut s = Session::with_sequential_ids();
    agent(&mut s, AGENT, "execute", add_line(0.0));
    agent(&mut s, AGENT, "execute", add_line(10.0));
    person(&mut s, "execute", add_line(20.0));
    let first = steps(&mut s)[0]["id"].as_u64().unwrap();
    let check = s.revert_check(first).unwrap();
    assert_eq!((check.above, check.above_other), (2, 1));
    assert_eq!(s.undo_back_to_agent_step(first).unwrap(), 3);
    assert_eq!(s.editor.doc().objects.len(), 0);
    assert_eq!(steps(&mut s)[0]["state"], "undone");
    assert_eq!(
        s.revert_agent_step(first, false),
        Err(StepError::Refused(Refusal::AlreadyUndone))
    );
    for _ in 0..3 {
        assert!(s.editor.redo());
    }
    assert_eq!(s.editor.doc().objects.len(), 3);
    assert_eq!(steps(&mut s)[0]["state"], "active");
}

#[test]
fn a_step_that_the_history_forgot_says_so() {
    let mut s = Session::with_sequential_ids();
    agent(&mut s, AGENT, "execute", add_line(0.0));
    let id = steps(&mut s)[0]["id"].as_u64().unwrap();
    // Undo, then a new command: the redo steps are gone.
    s.editor.undo();
    person(&mut s, "execute", add_line(5.0));
    let check = s.revert_check(id).unwrap();
    assert!(check.blocked().unwrap().contains("not in the undo history"));
    assert_eq!(steps(&mut s)[0]["state"], "gone");
}

#[test]
fn a_new_document_empties_the_log() {
    let mut s = Session::with_sequential_ids();
    s.set_session_version(true);
    agent(&mut s, AGENT, "execute", add_line(0.0));
    assert_eq!(steps(&mut s).len(), 1);
    s.replace_document(kadakas_core::model::Document::default());
    assert!(steps(&mut s).is_empty());
    assert!(s.session_version_enabled(), "the setting stays");
}

#[test]
fn the_advice_follows_the_state_of_the_step_and_of_the_session() {
    use kadakas_api::{Advice, StepCheck};
    use kadakas_core::command::{Conflict, ConflictKind, StepPlace};
    let base = StepCheck {
        place: StepPlace::Undo(2),
        details: true,
        above: 2,
        above_other: 1,
        conflicts: vec![],
        already_gone: 0,
        snapshot_blocked: false,
        free: 3,
        shared: false,
    };
    assert_eq!(base.advice(), Advice::Revert);
    assert!(base.blocked().is_none());
    let top = StepCheck {
        place: StepPlace::Undo(0),
        ..base.clone()
    };
    assert_eq!(top.advice(), Advice::Undo);
    let c = Conflict {
        id: kadakas_core::model::ObjectId::from_num(5),
        kind: ConflictKind::ChangedLater,
    };
    let part = StepCheck {
        conflicts: vec![c],
        free: 2,
        ..base.clone()
    };
    assert_eq!(
        part.advice(),
        Advice::Partial {
            free: 2,
            blocked: 1
        }
    );
    assert!(part.blocked().unwrap().contains("changed later"));
    let all_blocked = StepCheck {
        conflicts: vec![c],
        free: 0,
        ..base.clone()
    };
    assert_eq!(all_blocked.advice(), Advice::Nothing);
    let snap = StepCheck {
        snapshot_blocked: true,
        ..base.clone()
    };
    assert_eq!(snap.advice(), Advice::UndoBackOnly);
    // A shared document: never the undo of the editor, and no undo back.
    let shared_top = StepCheck {
        shared: true,
        place: StepPlace::Undo(0),
        ..base.clone()
    };
    assert_eq!(shared_top.advice(), Advice::Revert);
    let shared_snap = StepCheck {
        shared: true,
        snapshot_blocked: true,
        ..base.clone()
    };
    assert_eq!(shared_snap.advice(), Advice::Nothing);
    assert!(shared_snap.blocked().unwrap().contains("shared"));
    for place in [StepPlace::Redo, StepPlace::Gone] {
        let c = StepCheck {
            place,
            ..base.clone()
        };
        assert_eq!(c.advice(), Advice::Nothing);
        assert!(c.blocked().is_some());
    }
}

#[test]
fn undo_by_the_agent_through_the_api_is_no_step() {
    let mut s = Session::with_sequential_ids();
    agent(&mut s, AGENT, "execute", add_line(0.0));
    agent(&mut s, AGENT, "undo", json!({}));
    let list = steps(&mut s);
    assert_eq!(list.len(), 1, "an undo is no step");
    assert_eq!(list[0]["state"], "undone");
}
