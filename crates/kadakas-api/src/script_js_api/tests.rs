use serde_json::{Value, json};

use crate::{RpcError, Session, SessionLimits};

fn eval(s: &mut Session, params: Value) -> Result<Value, RpcError> {
    s.handle("script.eval", params)
}

fn ok(s: &mut Session, params: Value) -> Value {
    eval(s, params).unwrap_or_else(|e| panic!("{e:?}"))
}

fn objects(s: &mut Session) -> usize {
    s.handle("objects.list", json!({}))
        .unwrap()
        .as_array()
        .map_or(0, Vec::len)
}

const ADD_CIRCLES: &str = r#"
    const ids = [];
    for (let i = 0; i < args.n; i++) {
        const r = kadakas.call("execute", {command: {op: "add_object",
            shape: {kind: "circle", center: [i * 1000, 0], radius: 300}}});
        ids.push(r.created[0]);
    }
"#;

#[test]
fn a_script_returns_a_value_and_reads_the_document() {
    let mut s = Session::default();
    let r = ok(
        &mut s,
        json!({"code": "const d = kadakas.call('document.summary', {}); console.log('hi'); return {layers: d.layers.length, a: args.a};", "args": {"a": 5}, "mode": "read"}),
    );
    assert_eq!(r["result"]["a"], json!(5));
    assert_eq!(r["calls"], json!(1));
    assert_eq!(r["log"], json!("hi\n"));
    assert_eq!(r["changed"], json!(false));
    assert_eq!(r["undo_steps"], json!(0));
}

#[test]
fn a_run_with_many_changes_is_one_undo_step() {
    let mut s = Session::default();
    let r = ok(&mut s, json!({"code": format!("{ADD_CIRCLES} return ids.length;"), "args": {"n": 5}}));
    assert_eq!(r["result"], json!(5));
    assert_eq!(r["calls"], json!(5));
    assert_eq!(r["undo_steps"], json!(1));
    assert_eq!(objects(&mut s), 5);
    s.handle("undo", json!({})).unwrap();
    assert_eq!(objects(&mut s), 0, "one undo takes back the whole run");
    s.handle("redo", json!({})).unwrap();
    assert_eq!(objects(&mut s), 5, "and one redo brings it back");
}

#[test]
fn an_error_takes_back_the_whole_run() {
    let mut s = Session::default();
    let e = eval(
        &mut s,
        json!({"code": format!("{ADD_CIRCLES} throw new Error('boom');"), "args": {"n": 3}}),
    )
    .unwrap_err();
    assert!(e.message.contains("boom"), "{}", e.message);
    assert_eq!(e.details.as_ref().unwrap()["calls"], json!(3));
    assert_eq!(objects(&mut s), 0, "nothing is left");
    assert!(!s.editor.can_undo(), "and the history has no trace of it");
}

#[test]
fn a_failed_call_is_an_error_that_a_script_can_catch_and_the_rest_stays() {
    let mut s = Session::default();
    let r = ok(
        &mut s,
        json!({"code": "try { kadakas.call('objects.get', {id: '00000000-0000-0000-0000-0000000000ff'}); } catch (e) { return {code: e.code, method: e.method}; }"}),
    );
    assert_eq!(r["result"]["method"], json!("objects.get"));
    assert_eq!(r["result"]["code"], json!(2), "not found");
}

#[test]
fn read_mode_refuses_every_call_that_changes_the_document() {
    let mut s = Session::default();
    // Caught: the script sees a policy error, and the document is as it was.
    let r = ok(
        &mut s,
        json!({"mode": "read", "code": r#"
            try {
                kadakas.call("execute", {command: {op: "add_object", shape: {kind: "circle", center: [0, 0], radius: 5}}});
                return "not refused";
            } catch (e) { return [e.code, e.message.includes("mode read")]; }
        "#}),
    );
    assert_eq!(r["result"], json!([4, true]));
    assert_eq!(objects(&mut s), 0);
    assert!(!s.editor.can_undo());
    // Not caught: the run fails.
    let e = eval(
        &mut s,
        json!({"mode": "read", "code": "kadakas.call('execute', {command: {op: 'add_layer', name: 'X'}});"}),
    )
    .unwrap_err();
    assert!(e.message.contains("mode read"), "{}", e.message);
    // A method that writes a file is refused before it runs.
    let e = eval(
        &mut s,
        json!({"mode": "read", "code": "kadakas.call('export.file', {path: '/tmp/never.dxf'});"}),
    )
    .unwrap_err();
    assert!(e.message.contains("writes a file"), "{}", e.message);
    // The same call is fine in edit mode.
    ok(
        &mut s,
        json!({"code": "kadakas.call('execute', {command: {op: 'add_layer', name: 'X'}});"}),
    );
}

#[test]
fn some_methods_are_never_called_by_a_script() {
    let mut s = Session::default();
    for m in ["script.eval", "undo", "document.load", "collab.connect", "events.poll"] {
        let e = eval(&mut s, json!({"code": format!("kadakas.call('{m}', {{}});")})).unwrap_err();
        assert!(e.message.contains("cannot call"), "{m}: {}", e.message);
    }
}

#[test]
fn the_parameters_of_script_eval_are_checked() {
    let mut s = Session::default();
    for p in [
        json!({}),
        json!({"code": "1", "script": "x"}),
        json!({"code": "1", "mode": "write"}),
        json!({"code": "1", "bogus": 1}),
        json!({"code": 5}),
        json!({"code": "1", "seed": -1}),
    ] {
        let e = eval(&mut s, p.clone()).unwrap_err();
        assert_eq!(e.code, -32602, "{p}: {e:?}");
    }
    let e = eval(&mut s, json!({"code": "return (;"})).unwrap_err();
    assert!(e.message.contains("SyntaxError") || e.details.is_some(), "{e:?}");
}

#[test]
fn the_same_script_gives_the_same_result() {
    let code = "const a = []; for (let i = 0; i < 4; i++) a.push(Math.random()); return a;";
    let mut s = Session::default();
    let r1 = ok(&mut s, json!({"code": code, "seed": 3}))["result"].clone();
    let r2 = ok(&mut Session::default(), json!({"code": code, "seed": 3}))["result"].clone();
    let r3 = ok(&mut s, json!({"code": code, "seed": 4}))["result"].clone();
    assert_eq!(r1, r2);
    assert_ne!(r1, r3);
}

#[test]
fn limits_of_the_session_stop_a_script() {
    let mut s = Session::default();
    s.set_limits(SessionLimits {
        script_js_fuel: 30_000_000,
        script_js_calls: 10,
        ..SessionLimits::default()
    });
    let e = eval(&mut s, json!({"code": "for (;;) {}"})).unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
    assert!(e.message.contains("fuel"), "{}", e.message);
    let e = eval(&mut s, json!({"code": "for (let i = 0; i < 50; i++) kadakas.call('document.summary', {});"}))
        .unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
    assert_eq!(e.details.as_ref().unwrap()["calls"], json!(10));
    // The session still works.
    s.set_limits(SessionLimits::default());
    ok(&mut s, json!({"code": "return 1;"}));
}

#[test]
fn a_whole_run_has_a_time_limit_that_counts_the_calls() {
    let mut s = Session::default();
    s.set_limits(SessionLimits {
        script_js_time: std::time::Duration::from_millis(1),
        ..SessionLimits::default()
    });
    // The start of the engine takes longer than that, so the first call is over the limit.
    let e = eval(
        &mut s,
        json!({"code": "kadakas.call('document.summary', {}); let x = 0; for (let i = 0; i < 100000; i++) x += i; kadakas.call('document.summary', {});"}),
    )
    .unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
}

#[test]
fn script_info_says_how_scripts_run() {
    let mut s = Session::default();
    let i = s.handle("script.js_info", json!({})).unwrap();
    assert_eq!(i["available"], json!(true));
    assert!(i["upstream"].as_str().unwrap().contains("quickjs-ng"));
    assert!(i["limits"]["fuel"].as_u64().unwrap() > 0);
}

#[test]
fn a_script_of_the_document_runs_only_after_a_person_trusted_its_text() {
    let mut s = Session::default();
    let made = s
        .handle("script_resource.create", json!({"name": "Count", "code": "return 41 + 1;"}))
        .unwrap();
    assert_eq!(made["trusted"], json!(false));
    let sha = made["sha256"].as_str().unwrap().to_string();
    let e = eval(&mut s, json!({"script": "Count"})).unwrap_err();
    assert_eq!(e.kind, "policy", "{e:?}");
    assert!(e.message.contains(&sha));
    // A wrong hash trusts nothing.
    assert!(crate::trust_script(&mut s, "Count", "00").is_err());
    assert!(eval(&mut s, json!({"script": "Count"})).is_err());
    crate::trust_script(&mut s, "Count", &sha).unwrap();
    assert_eq!(ok(&mut s, json!({"script": "Count"}))["result"], json!(42));
    // A new text is a new hash and needs trust again.
    let upd = s
        .handle("script_resource.update", json!({"id": "Count", "code": "return 43;"}))
        .unwrap();
    assert_eq!(upd["trusted"], json!(false));
    assert!(eval(&mut s, json!({"script": "Count"})).is_err());
    // The list shows it.
    let l = s.handle("script_resource.list", json!({})).unwrap();
    assert_eq!(l["scripts"].as_array().unwrap().len(), 1);
    // The text an agent sends itself needs no trust.
    assert_eq!(ok(&mut s, json!({"code": "return 7;"}))["result"], json!(7));
    // A second script with the same name is refused.
    let e = s
        .handle("script_resource.create", json!({"name": "count", "code": "1"}))
        .unwrap_err();
    assert_eq!(e.kind, "conflict");
}

#[test]
fn a_typed_array_in_the_answer_of_a_call_and_in_the_result_is_written_out() {
    let mut s = Session::default();
    let r = ok(
        &mut s,
        json!({"mode": "read", "code": r#"
            const pts = kadakas.pointsFrom([[0, 0], [4000, 0], [4000, 3000], [0, 3000]]);
            const l = kadakas.call("unit_pattern.layout", {
                region: {outline: pts}, pattern: {unit_length: 1000, unit_width: 500, gap: 0, bond: "stack"},
                origin: [0, 0], limit: 0});
            return {summary: l.summary, pts};
        "#}),
    );
    assert_eq!(r["result"]["pts"], json!([[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [0.0, 3000.0]]));
    assert!(r["result"]["summary"].is_object() || r["result"]["summary"].is_null(), "{r}");
}
