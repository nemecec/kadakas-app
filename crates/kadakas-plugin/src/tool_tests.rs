//! Tests of the tool kind: the example tool, and bad answers from modules written as text (WAT).

use kadakas_plugin_abi::{Kind, Modifiers, ToolEvent};
use serde_json::{Map, Value, json};

use super::*;
use crate::{Plugin, PluginError, ToolCall, ToolDone, ToolStep};

const STONES: &[u8] = include_bytes!("../tests/fixtures/stone_row.wasm");

/// A manifest with an object part and a tool part with one parameter.
const BOTH: &str = r#"{"id":"test.both","name":"Both","version":"1.0","abi_version":2,"kinds":["object","tool"],"object":{"shape_type":"linear","params":[{"name":"n","type":"number","default":1,"min":0,"max":10}]},"tool":{"params":[{"name":"t","type":"number","default":2}]}}"#;
const TOOL_ONLY: &str =
    r#"{"id":"test.tool","name":"T","version":"1.0","abi_version":2,"kinds":["tool"],"tool":{}}"#;

fn step(
    p: &Plugin,
    event: &ToolEvent,
    state: &Value,
    modifiers: Modifiers,
) -> Result<ToolStep, PluginError> {
    p.tool_step(
        &permit_for(p),
        &ToolCall {
            event,
            modifiers,
            state,
        },
        &Map::new(),
    )
}

fn answer(manifest: &str, answer: &str) -> Plugin {
    load(&module_with(manifest, answer, None, ""), &quick()).unwrap()
}

fn run_answer(manifest: &str, a: &str) -> Result<ToolStep, PluginError> {
    step(
        &answer(manifest, a),
        &ToolEvent::Start,
        &Value::Null,
        Modifiers::default(),
    )
}

fn bad_tool(a: &str, what: &str) {
    let e = run_answer(BOTH, a).unwrap_err();
    assert!(
        matches!(e, PluginError::BadOutput(_)) && e.to_string().contains(what),
        "{e}"
    );
}

#[test]
fn the_example_tool_draws_a_row_in_two_clicks() {
    let p = load(STONES, &Limits::default()).unwrap();
    let m = p.manifest();
    assert_eq!(m.id, "org.kadakas.stone-row");
    assert_eq!(m.author, "The Kadakas project");
    assert!(m.provides(Kind::Object) && m.provides(Kind::Tool));
    let none = Modifiers::default();

    let s = step(&p, &ToolEvent::Start, &Value::Null, none).unwrap();
    assert!(s.hint.contains("start") && s.state.is_null() && s.done.is_none());
    // Moving before the first click does nothing.
    let s = step(&p, &ToolEvent::Move { at: [5.0, 5.0] }, &s.state, none).unwrap();
    assert!(s.preview.is_empty());
    let s = step(&p, &ToolEvent::Click { at: [0.0, 0.0] }, &s.state, none).unwrap();
    assert_eq!(s.state, json!({"start": [0.0, 0.0]}));
    // The preview follows the pointer. 3000 mm fit 5 stones of 400 mm with gaps of 300 mm.
    let s = step(&p, &ToolEvent::Move { at: [3000.0, 0.0] }, &s.state, none).unwrap();
    assert_eq!(s.preview.len(), 4);
    assert!(s.done.is_none());
    // A pointer too close for one stone gives no preview and no error.
    let near = step(&p, &ToolEvent::Move { at: [100.0, 0.0] }, &s.state, none).unwrap();
    assert!(near.preview.is_empty());

    // The last click ends the run with a plug-in object.
    let end = step(&p, &ToolEvent::Click { at: [3000.0, 0.0] }, &s.state, none).unwrap();
    match end.done {
        Some(ToolDone::Object {
            params,
            geometry,
            shapes,
        }) => {
            assert_eq!(params["size"], json!(400.0));
            assert_eq!(params["gap"], json!(300.0));
            assert_eq!(
                geometry,
                Geometry::Linear {
                    a: [0.0, 0.0],
                    b: [3000.0, 0.0]
                }
            );
            assert_eq!(shapes.len(), 4);
        }
        other => panic!("{other:?}"),
    }
    // With Shift it ends with plain shapes.
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    let end = step(&p, &ToolEvent::Click { at: [3000.0, 0.0] }, &s.state, shift).unwrap();
    assert!(matches!(end.done, Some(ToolDone::Shapes(v)) if v.len() == 4));

    // A bad end point is the message of the module, and the run goes on (the caller keeps the state).
    let e = step(&p, &ToolEvent::Click { at: [10.0, 0.0] }, &s.state, none).unwrap_err();
    assert!(matches!(e, PluginError::Guest(m) if m.contains("shorter")));
    let e = step(&p, &ToolEvent::Finish, &s.state, none).unwrap_err();
    assert!(matches!(e, PluginError::Guest(_)));
    let s = step(&p, &ToolEvent::Finish, &Value::Null, none).unwrap();
    assert!(s.hint.contains("start"));
}

#[test]
fn the_example_tool_takes_its_options_from_the_parameters() {
    let p = load(STONES, &Limits::default()).unwrap();
    let given = params(json!({"size": 200, "gap": 0}));
    let state = json!({"start": [0.0, 0.0]});
    let s = p
        .tool_step(
            &permit_for(&p),
            &ToolCall {
                event: &ToolEvent::Move { at: [1000.0, 0.0] },
                modifiers: Modifiers::default(),
                state: &state,
            },
            &given,
        )
        .unwrap();
    assert_eq!(s.preview.len(), 5);
    // A parameter outside its range is refused before the module runs.
    let e = p
        .tool_step(
            &permit_for(&p),
            &ToolCall {
                event: &ToolEvent::Start,
                modifiers: Modifiers::default(),
                state: &Value::Null,
            },
            &params(json!({"size": 5})),
        )
        .unwrap_err();
    assert!(matches!(e, PluginError::Refused(_)));
}

#[test]
fn the_host_checks_permit_kind_point_and_state_before_it_runs_anything() {
    let none = Modifiers::default();
    let call = |p: &Plugin, permit: &Permit, event: &ToolEvent, state: &Value| {
        p.tool_step(
            permit,
            &ToolCall {
                event,
                modifiers: none,
                state,
            },
            &Map::new(),
        )
    };
    let p = answer(BOTH, r#"{"status":"ok"}"#);
    let other = Permit::for_test("not this module");
    assert!(
        matches!(call(&p, &other, &ToolEvent::Start, &Value::Null), Err(PluginError::Refused(m)) if m.contains("not trusted"))
    );
    // An object-only plug-in has no tool.
    let planter = load(PLANTER, &Limits::default()).unwrap();
    assert!(
        matches!(call(&planter, &permit_for(&planter), &ToolEvent::Start, &Value::Null), Err(PluginError::Refused(m)) if m.contains("no tool"))
    );
    // A point that is not a number is refused. JSON cannot carry NaN, so the host checks it.
    for bad in [[f64::NAN, 0.0], [0.0, f64::INFINITY], [2e9, 0.0]] {
        let e = call(
            &p,
            &permit_for(&p),
            &ToolEvent::Move { at: bad },
            &Value::Null,
        )
        .unwrap_err();
        assert!(matches!(e, PluginError::Refused(m) if m.contains("finite")));
    }
    // A state that is too big is not sent.
    let big = json!("x".repeat(70_000));
    let e = call(&p, &permit_for(&p), &ToolEvent::Start, &big).unwrap_err();
    assert!(matches!(e, PluginError::Limit(_)));
}

#[test]
fn a_good_answer_gives_state_preview_hint_and_a_result() {
    let a = r#"{"status":"ok","state":{"k":1},"preview":[{"kind":"line","a":[0,0],"b":[5,0]}],"hint":"Click\nhere","result":{"type":"shapes","shapes":[{"kind":"circle","center":[0,0],"radius":3}]}}"#;
    let s = run_answer(BOTH, a).unwrap();
    assert_eq!(s.state, json!({"k": 1}));
    assert_eq!(s.preview.len(), 1);
    // Control characters in a hint become spaces.
    assert_eq!(s.hint, "Click here");
    assert!(matches!(s.done, Some(ToolDone::Shapes(v)) if v.len() == 1));
    // A reply with no preview and no result is fine.
    let s = run_answer(BOTH, r#"{"status":"ok"}"#).unwrap();
    assert!(s.preview.is_empty() && s.done.is_none() && s.state.is_null());
}

#[test]
fn a_long_hint_is_cut() {
    let a = format!(r#"{{"status":"ok","hint":"{}"}}"#, "h".repeat(500));
    assert_eq!(run_answer(BOTH, &a).unwrap().hint.chars().count(), 200);
}

#[test]
fn a_result_object_is_run_through_reset_and_checked() {
    // The module of `answer` gives the same bytes to `tool` and `reset`, so build the object
    // result with a reset answer in a second module: here the shared answer is a tool answer, which
    // is not a valid reset answer. The host then reports the bad reset answer.
    let a = r#"{"status":"ok","result":{"type":"object","params":{"n":3},"geometry":{"type":"linear","a":[0,0],"b":[10,0]}}}"#;
    let e = run_answer(BOTH, a).unwrap_err();
    assert!(matches!(e, PluginError::BadOutput(m) if m.contains("not valid")));
    // Parameters that the schema refuses.
    bad_tool(
        r#"{"status":"ok","result":{"type":"object","params":{"n":99},"geometry":{"type":"linear","a":[0,0],"b":[10,0]}}}"#,
        "object parameters",
    );
    bad_tool(
        r#"{"status":"ok","result":{"type":"object","params":{"zzz":1},"geometry":{"type":"linear","a":[0,0],"b":[10,0]}}}"#,
        "object parameters",
    );
    // The wrong kind of geometry.
    bad_tool(
        r#"{"status":"ok","result":{"type":"object","geometry":{"type":"point","at":[0,0]}}}"#,
        "needs a linear one",
    );
    // A geometry out of range.
    bad_tool(
        r#"{"status":"ok","result":{"type":"object","geometry":{"type":"linear","a":[0,0],"b":[1e12,0]}}}"#,
        "object geometry",
    );
}

#[test]
fn a_tool_only_module_cannot_place_an_object() {
    let e = run_answer(
        TOOL_ONLY,
        r#"{"status":"ok","result":{"type":"object","geometry":{"type":"point","at":[0,0]}}}"#,
    )
    .unwrap_err();
    assert!(matches!(e, PluginError::BadOutput(m) if m.contains("provides no object")));
}

#[test]
fn bad_tool_answers_are_rejected() {
    bad_tool("{", "not valid");
    bad_tool(r#"{"status":"maybe"}"#, "not valid");
    bad_tool(
        r#"{"status":"ok","preview":[{"kind":"blob"}]}"#,
        "not valid",
    );
    bad_tool(
        r#"{"status":"ok","preview":[{"kind":"circle","center":[0,0],"radius":-1}]}"#,
        "radius",
    );
    // A result with no shapes is an error. A preview with no shapes is not.
    bad_tool(
        r#"{"status":"ok","result":{"type":"shapes","shapes":[]}}"#,
        "no shapes",
    );
    bad_tool(
        r#"{"status":"ok","result":{"type":"shapes","shapes":[{"kind":"text","at":[0,0],"text":"a","height":0}]}}"#,
        "height",
    );
    let e = run_answer(BOTH, r#"{"status":"error","message":"no\nway"}"#).unwrap_err();
    assert!(matches!(e, PluginError::Guest(m) if m == "no way"));
}

#[test]
fn the_state_and_the_preview_are_limited() {
    let a = format!(r#"{{"status":"ok","state":"{}"}}"#, "x".repeat(70_000));
    let e = run_answer(BOTH, &a).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("state")),
        "{e}"
    );
    let one = r#"{"kind":"line","a":[0,0],"b":[1,1]}"#;
    let many = vec![one; 2_100].join(",");
    let e = run_answer(BOTH, &format!(r#"{{"status":"ok","preview":[{many}]}}"#)).unwrap_err();
    assert!(matches!(e, PluginError::Limit(_)));
    // An answer over the byte limit stops before it is parsed.
    let p = load(
        &module_with(BOTH, r#"{"status":"ok","hint":"abcdefghij"}"#, None, ""),
        &Limits {
            max_output_bytes: 20,
            ..quick()
        },
    );
    // The manifest is longer than 20 bytes, so such a module does not even load.
    assert!(matches!(p, Err(PluginError::Limit(_))));
}

#[test]
fn an_endless_tool_loop_runs_out_of_fuel() {
    let wasm = wat::parse_str(
        r#"(module
          (memory (export "memory") 1)
          (func (export "alloc") (param i32) (result i32) (i32.const 0))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64) (i64.const 0))
          (func (export "reset") (param i32 i32) (result i64) (i64.const 0))
          (func (export "tool") (param i32 i32) (result i64) (loop $l (br $l)) (i64.const 0)))"#,
    )
    .unwrap();
    let loaded = WasmtimeRuntime::new().load(&wasm, &quick()).unwrap();
    let e = loaded.tool(b"{}", false).unwrap_err();
    assert!(matches!(e, PluginError::Limit(m) if m.contains("fuel")));
}
