//! Tests of `plugins.tool_step`, of the functions that the trust dialog calls, and of the
//! persistence that the app turns on. Uses the example "Stone row" (an object and a tool).

use std::sync::Arc;

use kadakas_plugin::MemoryBackend;
use kadakas_plugin::store::StoreBackend;

use super::*;

const STONES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../kadakas-plugin/tests/fixtures/stone_row.wasm"
);
const STONES_ID: &str = "org.kadakas.stone-row";

fn stones(s: &mut Session) -> Value {
    ok(s, "plugins.install", json!({"path": STONES}))
}

fn step(s: &mut Session, extra: Value) -> Value {
    let mut p = json!({"plugin": STONES_ID});
    p.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    ok(s, "plugins.tool_step", p)
}

#[test]
fn list_shows_the_author_the_kinds_and_the_tool_options() {
    let mut s = open_session();
    let r = stones(&mut s);
    assert_eq!(r["kinds"], json!(["object", "tool"]));
    assert_eq!(r["author"], "The Kadakas project");
    assert_eq!(r["shape_type"], "linear");
    assert_eq!(r["tool_params"].as_array().unwrap().len(), 2);
    // The object part still places with two points.
    let placed = ok(
        &mut s,
        "plugins.place",
        json!({"plugin": STONES_ID, "a": [0, 0], "b": [2000, 0]}),
    );
    assert_eq!(placed["shapes"], 3);
    // The planter has no tool options.
    installed(&mut s);
    let list = ok(&mut s, "plugins.list", json!({}));
    let planter = list
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == PLANTER_ID)
        .unwrap();
    assert_eq!(planter["tool_params"], Value::Null);
    assert_eq!(planter["author"], "");
}

#[test]
fn a_tool_run_ends_with_a_plug_in_object_in_one_undo_step() {
    let mut s = open_session();
    stones(&mut s);
    let first = step(&mut s, json!({"event": {"type": "start"}}));
    assert_eq!(first["done"], false);
    assert!(first["hint"].as_str().unwrap().contains("start"));
    let click = step(
        &mut s,
        json!({"event": {"type": "click", "at": [0, 0]}, "state": first["state"]}),
    );
    assert_eq!(click["state"], json!({"start": [0.0, 0.0]}));
    // The preview does not change the drawing.
    let moved = step(
        &mut s,
        json!({"event": {"type": "move", "at": [3000, 0]}, "state": click["state"]}),
    );
    assert_eq!(moved["preview"].as_array().unwrap().len(), 4);
    assert_eq!(moved["preview"][0]["kind"], "circle");
    assert_eq!(s.editor.doc().objects.len(), 0);

    let rev = s.editor.revision();
    let end = step(
        &mut s,
        json!({"event": {"type": "click", "at": [3000, 0]}, "state": moved["state"]}),
    );
    assert_eq!(end["done"], true);
    assert_eq!(end["result"]["kind"], "object");
    assert_eq!(end["result"]["committed"], true);
    let id = end["result"]["id"].as_uid().unwrap();
    assert_eq!(children(&s, id).len(), 4);
    let sets = s.editor.changes_since(rev);
    assert_eq!(sets.changes.len(), 1);
    assert_eq!(sets.changes[0].origin, Origin::Script);
    assert_eq!(s.selection, vec![ObjectId(id)]);
    // It is a real plug-in object: its parameters can change.
    let info = ok(&mut s, "plugins.object", json!({"id": id}));
    assert_eq!(info["params"]["size"], 400.0);
    assert_eq!(info["geometry"]["b"], json!([3000.0, 0.0]));
    ok(
        &mut s,
        "plugins.set_params",
        json!({"id": id, "params": {"gap": 100}}),
    );
    assert_eq!(children(&s, id).len(), 6);
    // Undo removes the regeneration, then the placement.
    ok(&mut s, "undo", json!({}));
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.editor.doc().objects.len(), 0);
}

#[test]
fn a_tool_run_can_end_with_plain_shapes_or_only_show_the_result() {
    let mut s = open_session();
    stones(&mut s);
    let state = json!({"start": [0, 0]});
    // Shift ends the run with plain shapes: no group, no plug-in data.
    let end = step(
        &mut s,
        json!({"event": {"type": "click", "at": [3000, 0]}, "state": state, "modifiers": {"shift": true}}),
    );
    assert_eq!(end["result"]["kind"], "shapes");
    assert_eq!(end["result"]["ids"].as_array().unwrap().len(), 4);
    assert_eq!(s.editor.doc().objects.len(), 4);
    assert!(
        s.editor
            .doc()
            .objects
            .values()
            .all(|o| !o.data.contains_key(DATA_KEY) && o.parent.is_none())
    );
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.editor.doc().objects.len(), 0);

    // With commit false nothing is added, and the result is in the answer.
    let shown = step(
        &mut s,
        json!({"event": {"type": "click", "at": [3000, 0]}, "state": state, "commit": false}),
    );
    assert_eq!(shown["result"]["committed"], false);
    assert_eq!(shown["result"]["kind"], "object");
    assert_eq!(shown["result"]["shapes"].as_array().unwrap().len(), 4);
    assert_eq!(shown["result"]["geometry"]["a"], json!([0.0, 0.0]));
    let shapes = step(
        &mut s,
        json!({"event": {"type": "click", "at": [3000, 0]}, "state": state, "commit": false, "modifiers": {"shift": true}}),
    );
    assert_eq!(shapes["result"]["kind"], "shapes");
    assert_eq!(shapes["result"]["shapes"].as_array().unwrap().len(), 4);
    assert_eq!(s.editor.doc().objects.len(), 0);
}

#[test]
fn tool_options_and_layout_choices_are_passed_on() {
    let mut s = open_session();
    stones(&mut s);
    let state = json!({"start": [0, 0]});
    let end = step(
        &mut s,
        json!({"event": {"type": "click", "at": [1000, 0]}, "state": state,
               "params": {"size": 200, "gap": 0}, "name": "Path"}),
    );
    let id = end["result"]["id"].as_uid().unwrap();
    assert_eq!(end["result"]["params"]["size"], 200.0);
    assert_eq!(children(&s, id).len(), 5);
    assert!(matches!(
        &s.editor.doc().object(ObjectId(id)).unwrap().shape,
        Shape::Group { name, .. } if name == "Path"
    ));
    // A value outside the range is refused before the module runs.
    let e = err(
        &mut s,
        "plugins.tool_step",
        json!({"plugin": STONES_ID, "event": {"type": "start"}, "params": {"size": 1}}),
    );
    assert_eq!(e.code, 4);
}

#[test]
fn bad_tool_requests_are_refused() {
    let mut s = open_session();
    stones(&mut s);
    let call = |s: &mut Session, p: Value| err(s, "plugins.tool_step", p);
    assert_eq!(call(&mut s, json!({"plugin": STONES_ID})).code, -32602);
    assert_eq!(
        call(
            &mut s,
            json!({"plugin": STONES_ID, "event": {"type": "dance"}})
        )
        .code,
        -32602
    );
    assert_eq!(
        call(
            &mut s,
            json!({"plugin": STONES_ID, "event": {"type": "move"}})
        )
        .code,
        -32602
    );
    assert_eq!(
        call(
            &mut s,
            json!({"plugin": STONES_ID, "event": {"type": "start"}, "modifiers": 3})
        )
        .code,
        -32602
    );
    assert_eq!(
        call(
            &mut s,
            json!({"plugin": STONES_ID, "event": {"type": "start"}, "params": 3})
        )
        .code,
        -32602
    );
    // A point that is too large is refused by the host.
    assert_eq!(
        call(
            &mut s,
            json!({"plugin": STONES_ID, "event": {"type": "move", "at": [1e12, 0]}})
        )
        .code,
        4
    );
    assert_eq!(
        call(
            &mut s,
            json!({"plugin": "nope", "event": {"type": "start"}})
        )
        .code,
        2
    );
    // The message of the module reaches the caller; the run goes on because nothing changed.
    let e = call(
        &mut s,
        json!({"plugin": STONES_ID, "event": {"type": "click", "at": [10, 0]}, "state": {"start": [0, 0]}}),
    );
    assert_eq!(e.code, 1);
    assert!(e.message.contains("shorter"), "{}", e.message);
    assert_eq!(s.editor.doc().objects.len(), 0);
    // A plug-in without a tool says so.
    installed(&mut s);
    let e = call(
        &mut s,
        json!({"plugin": PLANTER_ID, "event": {"type": "start"}}),
    );
    assert_eq!(e.code, 4);
    assert!(e.message.contains("no tool"), "{}", e.message);
}

#[test]
fn a_tool_does_not_run_without_trust_and_agents_cannot_give_trust() {
    let mut s = Session::with_sequential_ids();
    stones(&mut s);
    let e = err(
        &mut s,
        "plugins.tool_step",
        json!({"plugin": STONES_ID, "event": {"type": "start"}}),
    );
    assert_eq!(e.code, 4);
    assert!(e.message.contains("not trusted"), "{}", e.message);
    // No method gives or takes trust. The names are unknown to the API.
    for method in [
        "plugins.trust",
        "plugins.untrust",
        "plugins.forget",
        "plugins.allow",
    ] {
        assert_eq!(
            err(&mut s, method, json!({"plugin": STONES_ID})).code,
            -32601
        );
    }
    assert!(!crate::METHODS.iter().any(|(n, _)| n.contains("trust")));
    // The user trusts in the app. Then the tool runs.
    let hash = module_review(&s, STONES_ID).unwrap()["hash"]
        .as_str()
        .unwrap()
        .to_string();
    trust_module(&mut s, STONES_ID, &hash).unwrap();
    step(&mut s, json!({"event": {"type": "start"}}));
    // And the user takes the trust back.
    forget_module(&mut s, STONES_ID).unwrap();
    assert!(forget_module(&mut s, "nope").is_err());
    assert_eq!(
        err(
            &mut s,
            "plugins.tool_step",
            json!({"plugin": STONES_ID, "event": {"type": "start"}})
        )
        .code,
        4
    );
}

#[test]
fn the_app_drives_a_tool_with_the_raw_functions() {
    let mut s = Session::with_sequential_ids();
    stones(&mut s);
    let hash = module_review(&s, STONES_ID).unwrap()["hash"]
        .as_str()
        .unwrap()
        .to_string();
    trust_module(&mut s, STONES_ID, &hash).unwrap();
    let state = json!({"start": [0.0, 0.0]});
    let step = tool_step_raw(
        &s,
        STONES_ID,
        &ToolEvent::Click { at: [3000.0, 0.0] },
        Modifiers::default(),
        &state,
        &Map::new(),
    )
    .unwrap();
    let done = step.done.unwrap();
    assert_eq!(s.editor.doc().objects.len(), 0);
    let result = commit_tool_result(&mut s, STONES_ID, &done, None, None, None).unwrap();
    assert_eq!(result["kind"], "object");
    assert_eq!(s.editor.doc().objects.len(), 5);
    assert!(
        tool_step_raw(
            &s,
            "nope",
            &ToolEvent::Start,
            Modifiers::default(),
            &Value::Null,
            &Map::new()
        )
        .is_err()
    );
}

#[test]
fn the_trust_dialog_gets_everything_a_person_needs_to_decide() {
    let mut s = Session::with_sequential_ids();
    stones(&mut s);
    let r = module_review(&s, STONES_ID).unwrap();
    assert_eq!(r["name"], "Stone row");
    assert_eq!(r["version"], "1.0.0");
    assert_eq!(r["author"], "The Kadakas project");
    assert_eq!(r["hash"].as_str().unwrap().len(), 64);
    assert!(r["size"].as_u64().unwrap() > 1000);
    assert_eq!(r["kinds"], json!(["object", "tool"]));
    assert_eq!(r["trusted"], false);
    assert_eq!(r["instances"], 0);
    // There is no permission to show: the host grants none and refuses a module that asks.
    assert!(r.get("permissions").is_none());
    assert!(module_review(&s, "nope").is_none());
}

#[test]
fn install_from_bytes_keeps_the_module_for_the_next_session() {
    let bytes = std::fs::read(STONES).unwrap();
    let mem = Arc::new(MemoryBackend::new());
    let mut s = Session::with_sequential_ids();
    s.wasm_plugins
        .registry
        .attach(mem.clone(), 2 << 20, 2 << 20);
    assert_eq!(ok(&mut s, "plugins.info", json!({}))["persistent"], false);
    let r = install_module(&mut s, &bytes).unwrap();
    assert_eq!(r["id"], STONES_ID);
    assert!(r.get("warning").is_none());
    assert_eq!(mem.list().unwrap().len(), 1);
    // A medium that refuses writes: the module is installed, with a warning.
    *mem.fail_writes.lock().unwrap() = Some("quota".into());
    let other = std::fs::read(PLANTER).unwrap();
    let r = install_module(&mut s, &other).unwrap();
    assert!(r["warning"].as_str().unwrap().contains("quota"));
    assert!(take_store_warning(&mut s).is_none());
    // A new session on the same medium has the module.
    *mem.fail_writes.lock().unwrap() = None;
    let mut next = Session::with_sequential_ids();
    let report = next.wasm_plugins.registry.attach(mem, 2 << 20, 2 << 20);
    assert_eq!(report.loaded, vec![STONES_ID.to_string()]);
    assert_eq!(
        ok(&mut next, "plugins.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // A broken module is an error with the right code.
    assert_eq!(install_module(&mut next, b"junk").unwrap_err().code, -32602);
}

#[test]
fn info_names_the_time_limit_of_the_engine() {
    let mut s = open_session();
    let info = ok(&mut s, "plugins.info", json!({}));
    assert_eq!(info["time_limit_ms"], 1500);
}

#[test]
fn describe_and_the_method_list_know_the_tool_step() {
    assert!(
        crate::METHODS
            .iter()
            .any(|(n, _)| *n == "plugins.tool_step")
    );
    let d = describe();
    assert!(d["tools"].as_str().unwrap().contains("plugins.tool_step"));
    assert!(
        d["trust"]
            .as_str()
            .unwrap()
            .contains("No API method adds or removes trust")
    );
}
