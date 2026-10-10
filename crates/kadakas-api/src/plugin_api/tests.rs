use kadakas_core::uid::AsUid;
use serde_json::{Value, json};

use super::*;

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, RpcError> {
    s.handle(method, params)
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn fail(s: &mut Session, method: &str, params: Value) -> String {
    call(s, method, params)
        .expect_err(&format!("{method} should fail"))
        .message
}

/// A session that lets "agents" run their own scripts.
fn open_session() -> Session {
    let mut s = Session::with_sequential_ids();
    s.scripting.allow_agent_scripts = true;
    s
}

const CIRCLES: &str = "def reset(params, geometry):\n    n = int(params.n)\n    return [circle([geometry.at[0] + 100 * i, geometry.at[1]], 10) for i in range(n)]\n";

fn define_circles(s: &mut Session) -> kadakas_core::uid::Uid {
    ok(
        s,
        "plugin.define",
        json!({"name": "Circles", "shape_type": "point", "script": CIRCLES,
               "params": [{"name": "n", "type": "number", "default": 3, "min": 1, "max": 20}]}),
    )["id"]
        .as_uid()
        .unwrap()
}

fn children(s: &Session, id: kadakas_core::uid::Uid) -> Vec<ObjectId> {
    s.editor.doc().children(ObjectId(id))
}

fn centers(s: &Session, id: kadakas_core::uid::Uid) -> Vec<f64> {
    children(s, id)
        .iter()
        .map(|c| match s.editor.doc().object(*c).unwrap().shape {
            Shape::Circle { center, .. } => center.x,
            _ => panic!("not a circle"),
        })
        .collect()
}

fn count(s: &Session) -> usize {
    s.editor.doc().objects.len()
}

#[test]
fn shipped_examples_run_without_the_agent_flag() {
    let mut s = Session::with_sequential_ids();
    let list = ok(&mut s, "plugin.examples", json!({}));
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["installed"], false);
    let bench = ok(&mut s, "plugin.install_example", json!({"name": "bench"}));
    assert_eq!(bench["existing"], false);
    assert_eq!(
        ok(&mut s, "plugin.install_example", json!({"name": "Bench"}))["existing"],
        true
    );
    assert!(fail(&mut s, "plugin.install_example", json!({"name": "Nope"})).contains("Available"));
    let placed = ok(
        &mut s,
        "plugin.place",
        json!({"definition": "Bench", "at": [1000, 2000], "rotation": 0.5, "params": {"slats": 4, "back": false}}),
    );
    // Seat outline and three slat lines.
    assert_eq!(placed["shapes"], 4);
    let id = placed["id"].as_uid().unwrap();
    assert!(s.editor.doc().is_group(ObjectId(id)));
    assert_eq!(s.selection, vec![ObjectId(id)]);
    let info = ok(&mut s, "plugin.object", json!({"id": id}));
    assert_eq!(info["params"]["slats"], 4.0);
    assert_eq!(info["params"]["length"], 1800.0);
    assert_eq!(info["params"]["supplier"]["name"], "");
    assert_eq!(info["error"], Value::Null);
    assert_eq!(info["schema"].as_array().unwrap().len(), 7);
    assert_eq!(
        ok(&mut s, "plugin.list", json!({}))[0]["instances"],
        json!(1)
    );
}

#[test]
fn placing_is_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "plugin.install_example", json!({"name": "Bench"}));
    let before = count(&s);
    ok(
        &mut s,
        "plugin.place",
        json!({"definition": "Bench", "at": [0, 0]}),
    );
    assert!(count(&s) > before + 3);
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    assert_eq!(count(&s), before);
    assert!(ok(&mut s, "redo", json!({}))["done"].as_bool().unwrap());
    assert!(count(&s) > before + 3);
}

#[test]
fn own_scripts_need_the_flag() {
    let mut s = Session::with_sequential_ids();
    let id = define_circles(&mut s);
    let msg = fail(
        &mut s,
        "plugin.place",
        json!({"definition": id, "at": [0, 0]}),
    );
    assert!(msg.contains("--allow-agent-scripts"), "{msg}");
    assert!(
        fail(
            &mut s,
            "plugin.preview",
            json!({"definition": id, "at": [0, 0]})
        )
        .contains("may not run")
    );
    assert!(fail(&mut s, "script.run", json!({"source": "print(1)"})).contains("may not run"));
    assert!(fail(&mut s, "script.stop", json!({"run": 1})).contains("may not run"));
    assert_eq!(
        ok(&mut s, "script.info", json!({}))["agent_scripts_allowed"],
        false
    );
    // The document stays as it was.
    assert_eq!(count(&s), 0);
    // A user, in the app, is allowed.
    s.scripting.allow_agent_scripts = true;
    let placed = ok(
        &mut s,
        "plugin.place",
        json!({"definition": id, "at": [0, 0]}),
    );
    s.scripting.allow_agent_scripts = false;
    let gid = placed["id"].as_uid().unwrap();
    assert!(
        fail(
            &mut s,
            "plugin.set_params",
            json!({"id": gid, "params": {"n": 2}})
        )
        .contains("may not run")
    );
    // The user's own action needs trust in the script, not the flag for agents.
    let err =
        user_set_params(&mut s, ObjectId(gid), json!({"n": 2}).as_object().unwrap()).unwrap_err();
    assert_eq!(err.code, UNTRUSTED_CODE);
    trust_it(&mut s, ObjectId(gid));
    let r = user_set_params(&mut s, ObjectId(gid), json!({"n": 2}).as_object().unwrap()).unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(centers(&s, gid).len(), 2);
    assert!(!s.scripting.allow_agent_scripts, "the flag is not touched");
}

#[test]
fn parameters_regenerate_the_shapes_and_undo_restores_them() {
    let mut s = open_session();
    let def = define_circles(&mut s);
    let g = ok(
        &mut s,
        "plugin.place",
        json!({"definition": def, "at": [500, 0]}),
    )["id"]
        .as_uid()
        .unwrap();
    assert_eq!(centers(&s, g), vec![500.0, 600.0, 700.0]);
    let r = ok(
        &mut s,
        "plugin.set_params",
        json!({"id": g, "params": {"n": 5}}),
    );
    assert_eq!(r["ok"], true);
    assert_eq!(centers(&s, g).len(), 5);
    assert!(s.editor.doc().is_group(ObjectId(g)), "the group stays");
    assert_eq!(count(&s), 6);
    ok(&mut s, "undo", json!({}));
    assert_eq!(centers(&s, g), vec![500.0, 600.0, 700.0]);
    assert_eq!(
        ok(&mut s, "plugin.object", json!({"id": g}))["params"]["n"],
        3.0
    );
    // Bad values are refused and change nothing.
    assert!(
        fail(
            &mut s,
            "plugin.set_params",
            json!({"id": g, "params": {"n": 99}})
        )
        .contains("range")
    );
    assert!(
        fail(
            &mut s,
            "plugin.set_params",
            json!({"id": g, "params": {"zzz": 1}})
        )
        .contains("unknown parameter")
    );
    assert!(
        fail(
            &mut s,
            "plugin.set_params",
            json!({"id": "00000000-0000-0000-0000-000000000001", "params": {}})
        )
        .contains("does not exist")
    );
    assert!(
        fail(
            &mut s,
            "plugin.place",
            json!({"definition": def, "at": [0, 0], "params": {"n": 0}})
        )
        .contains("range")
    );
    assert!(fail(&mut s, "plugin.place", json!({"definition": def})).contains("at"));
    assert!(
        fail(
            &mut s,
            "plugin.place",
            json!({"definition": "nothing", "at": [0, 0]})
        )
        .contains("no plug-in")
    );
    assert!(
        fail(
            &mut s,
            "plugin.place",
            json!({"definition": true, "at": [0, 0]})
        )
        .contains("ID or a name")
    );
    assert!(fail(&mut s, "plugin.place", json!({"at": [0, 0]})).contains("definition"));
}

#[test]
fn a_failing_script_keeps_the_last_good_shapes_and_shows_an_error() {
    let mut s = open_session();
    let def = define_circles(&mut s);
    let g = ok(
        &mut s,
        "plugin.place",
        json!({"definition": def, "at": [0, 0]}),
    )["id"]
        .as_uid()
        .unwrap();
    let shapes = centers(&s, g);
    // The script now fails for n = 7 only.
    let script = "def reset(params, geometry):\n    if params.n == 7:\n        raise ValueError('seven is unlucky')\n    return [circle([geometry.at[0], 0], 10)]\n";
    ok(
        &mut s,
        "plugin.update",
        json!({"id": def, "script": script}),
    );
    // Updating does not change objects by itself.
    assert_eq!(centers(&s, g), shapes);
    let r = ok(
        &mut s,
        "plugin.set_params",
        json!({"id": g, "params": {"n": 7}}),
    );
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("seven is unlucky"));
    assert!(r["traceback"].as_str().unwrap().contains("ValueError"));
    assert_eq!(centers(&s, g), shapes, "the last good shapes stay");
    let info = ok(&mut s, "plugin.object", json!({"id": g}));
    assert_eq!(info["params"]["n"], 7.0, "the parameters are saved");
    assert!(info["error"].as_str().unwrap().contains("unlucky"));
    // The error step can be undone, too.
    ok(&mut s, "undo", json!({}));
    assert_eq!(
        ok(&mut s, "plugin.object", json!({"id": g}))["error"],
        Value::Null
    );
    // A good value clears the error.
    ok(
        &mut s,
        "plugin.set_params",
        json!({"id": g, "params": {"n": 7}}),
    );
    let r = ok(
        &mut s,
        "plugin.set_params",
        json!({"id": g, "params": {"n": 2}}),
    );
    assert_eq!(r["ok"], true);
    assert_eq!(centers(&s, g).len(), 1);
    assert_eq!(
        ok(&mut s, "plugin.object", json!({"id": g}))["error"],
        Value::Null
    );
}

#[test]
fn a_timeout_keeps_the_last_good_shapes() {
    let mut s = open_session();
    let def = define_circles(&mut s);
    let g = ok(
        &mut s,
        "plugin.place",
        json!({"definition": def, "at": [0, 0]}),
    )["id"]
        .as_uid()
        .unwrap();
    let script = "import time\ndef reset(params, geometry):\n    if params.n > 3:\n        time.sleep(60)\n    return [circle(geometry.at, 1)] * int(params.n)\n";
    ok(
        &mut s,
        "plugin.update",
        json!({"id": def, "script": script, "timeout_ms": 400}),
    );
    let r = ok(
        &mut s,
        "plugin.set_params",
        json!({"id": g, "params": {"n": 9}}),
    );
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("400 ms"));
    assert_eq!(children(&s, g).len(), 3);
    // The definition cannot set a time limit outside the allowed range.
    assert!(
        fail(&mut s, "plugin.update", json!({"id": def, "timeout_ms": 1})).contains("timeout_ms")
    );
}

#[test]
fn a_moved_object_regenerates_where_it_is() {
    let mut s = open_session();
    let def = define_circles(&mut s);
    let g = ok(
        &mut s,
        "plugin.place",
        json!({"definition": def, "at": [500, 100]}),
    )["id"]
        .as_uid()
        .unwrap();
    s.editor
        .execute(&Command::MoveObjects {
            ids: vec![ObjectId(g)],
            delta: [1000.0, 50.0].into(),
        })
        .unwrap();
    ok(
        &mut s,
        "plugin.set_params",
        json!({"id": g, "params": {"n": 2}}),
    );
    assert_eq!(centers(&s, g), vec![1500.0, 1600.0]);
    assert_eq!(
        ok(&mut s, "plugin.object", json!({"id": g}))["geometry"]["at"],
        json!([1500.0, 150.0])
    );
}

#[test]
fn definitions_can_be_updated_listed_and_deleted() {
    let mut s = open_session();
    let def = define_circles(&mut s);
    assert!(
        fail(
            &mut s,
            "plugin.define",
            json!({"name": "circles", "shape_type": "point", "script": ""})
        )
        .contains("already exists")
    );
    assert!(
        fail(
            &mut s,
            "plugin.define",
            json!({"shape_type": "point", "script": ""})
        )
        .contains("name")
    );
    assert!(
        fail(
            &mut s,
            "plugin.define",
            json!({"name": "B", "shape_type": "blob", "script": ""})
        )
        .contains("variant")
    );
    let g1 = ok(
        &mut s,
        "plugin.place",
        json!({"definition": "Circles", "at": [0, 0]}),
    )["id"]
        .as_uid()
        .unwrap();
    let g2 = ok(
        &mut s,
        "plugin.place",
        json!({"definition": "Circles", "at": [0, 1000]}),
    )["id"]
        .as_uid()
        .unwrap();
    let got = ok(&mut s, "plugin.get", json!({"name": "CIRCLES"}));
    assert_eq!(got["instances"], 2);
    assert_eq!(got["shipped"], false);
    assert!(got["script"].as_str().unwrap().contains("def reset"));
    assert!(fail(&mut s, "plugin.delete", json!({"id": def})).contains("2 objects"));
    assert!(
        fail(
            &mut s,
            "plugin.update",
            json!({"id": def, "shape_type": "path"})
        )
        .contains("cannot change")
    );
    // Rename, change the script and regenerate both objects in one step.
    let script = "def reset(params, geometry):\n    return [circle(geometry.at, 99)]\n";
    let r = ok(
        &mut s,
        "plugin.update",
        json!({"id": def, "new_name": "Dots", "script": script, "reset": true}),
    );
    assert_eq!(r["reset"].as_array().unwrap().len(), 2);
    assert_eq!(children(&s, g1).len(), 1);
    assert_eq!(children(&s, g2).len(), 1);
    ok(&mut s, "undo", json!({}));
    assert_eq!(
        children(&s, g1).len(),
        3,
        "one undo step takes back update and reset"
    );
    assert_eq!(
        ok(&mut s, "plugin.get", json!({"id": def}))["name"],
        "Circles"
    );
    ok(
        &mut s,
        "plugin.update",
        json!({"id": def, "script": script}),
    );
    let r = ok(&mut s, "plugin.reset", json!({"definition": def}));
    assert_eq!(r["results"].as_array().unwrap().len(), 2);
    assert_eq!(children(&s, g1).len(), 1);
    ok(&mut s, "plugin.reset", json!({"ids": [g1]}));
    ok(&mut s, "plugin.reset", json!({"id": g2}));
    assert!(
        fail(
            &mut s,
            "plugin.reset",
            json!({"id": "00000000-0000-0000-0000-000000000001"})
        )
        .contains("does not exist")
    );
    assert!(fail(&mut s, "plugin.reset", json!({"ids": ["x"]})).contains("object IDs"));
    assert!(fail(&mut s, "plugin.reset", json!({})).contains("id must"));
    // Delete the objects, then the definition.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [g1, g2]}}),
    );
    ok(&mut s, "plugin.delete", json!({"id": def}));
    assert!(
        ok(&mut s, "plugin.list", json!({}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(fail(&mut s, "plugin.delete", json!({"id": def})).contains("no plug-in"));
    assert!(fail(&mut s, "plugin.get", json!({})).contains("give id or name"));
    assert!(fail(&mut s, "plugin.list_nothing", json!({})).contains("unknown method"));
}

#[test]
fn update_with_reset_needs_permission_when_objects_exist() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "plugin.install_example", json!({"name": "Bench"}));
    ok(
        &mut s,
        "plugin.place",
        json!({"definition": "Bench", "at": [0, 0]}),
    );
    let script = "def reset(p, g):\n    return []\n";
    assert!(
        fail(
            &mut s,
            "plugin.update",
            json!({"name": "Bench", "script": script, "reset": true})
        )
        .contains("may not run")
    );
    // Without a reset the update is only data.
    ok(
        &mut s,
        "plugin.update",
        json!({"name": "Bench", "script": script}),
    );
    assert_eq!(
        ok(&mut s, "plugin.get", json!({"name": "Bench"}))["shipped"],
        false
    );
}

#[test]
fn every_shape_type_places_with_its_geometry() {
    let mut s = open_session();
    let echo = "def reset(params, geometry):\n    return [text([0, 0], json_text(geometry), 10)]\n";
    let echo = format!("import json\ndef json_text(g):\n    return json.dumps(g)\n{echo}");
    for (ty, geo) in [
        ("point", json!({"at": [1, 2], "rotation": 1.0})),
        ("linear", json!({"a": [0, 0], "b": [10, 0]})),
        ("rectangle", json!({"min": [0, 0], "max": [10, 5]})),
        (
            "path",
            json!({"points": [[0, 0], [10, 0], [10, 10]], "closed": true}),
        ),
    ] {
        let name = format!("Echo {ty}");
        ok(
            &mut s,
            "plugin.define",
            json!({"name": name, "shape_type": ty, "script": echo}),
        );
        let mut p = geo.as_object().unwrap().clone();
        p.insert("definition".into(), json!(name));
        let prev = ok(&mut s, "plugin.preview", Value::Object(p.clone()));
        let text = prev["shapes"][0]["text"].as_str().unwrap();
        let echoed = serde_json::from_str::<Value>(text).unwrap();
        assert_eq!(
            echoed[ty_key(ty)].to_string().replace(".0", ""),
            geo[ty_key(ty)].to_string()
        );
        let placed = ok(&mut s, "plugin.place", Value::Object(p));
        assert_eq!(placed["shapes"], 1);
        assert_eq!(count(&s), 2);
        ok(&mut s, "undo", json!({}));
    }
    // Wrong geometry for the type is refused.
    assert!(
        fail(
            &mut s,
            "plugin.place",
            json!({"definition": "Echo path", "points": [[0, 0]]})
        )
        .contains("at least 2")
    );
    assert!(
        fail(
            &mut s,
            "plugin.place",
            json!({"definition": "Echo linear", "at": [0, 0]})
        )
        .contains("'a'")
    );
}

fn ty_key(ty: &str) -> &'static str {
    match ty {
        "point" => "at",
        "linear" => "a",
        "rectangle" => "max",
        _ => "points",
    }
}

#[test]
fn stepping_stones_follow_the_path() {
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "plugin.install_example",
        json!({"name": "Stepping stones"}),
    );
    let placed = ok(
        &mut s,
        "plugin.place",
        json!({"definition": "Stepping stones", "points": [[0, 0], [3000, 0]], "params": {"spacing": 1000, "stone": "Square", "zigzag": 100}}),
    );
    // Stones at 0, 1000, 2000 and 3000 mm.
    assert_eq!(placed["shapes"], 4);
    let g = placed["id"].as_uid().unwrap();
    let ys: Vec<f64> = children(&s, g)
        .iter()
        .map(|c| s.editor.doc().object(*c).unwrap().shape.bounds().0.y)
        .collect();
    assert!(
        ys[0] > ys[1] - 1.0 && (ys[0] - ys[1]).abs() > 100.0,
        "zigzag moves stones to both sides: {ys:?}"
    );
    // Changing the path regenerates the stones.
    assert!(placed["children"].as_array().unwrap().len() == 4);
    // A path too long for the shape limit fails and nothing is added.
    let before = count(&s);
    let msg = fail(
        &mut s,
        "plugin.place",
        json!({"definition": "Stepping stones", "points": [[0, 0], [1_500_000, 0]], "params": {"spacing": 500}}),
    );
    assert!(msg.contains("shapes"), "{msg}");
    assert_eq!(count(&s), before);
}

#[test]
fn a_document_with_plug_in_objects_saves_and_loads() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "plugin.install_example", json!({"name": "Bench"}));
    let g = ok(
        &mut s,
        "plugin.place",
        json!({"definition": "Bench", "at": [0, 0]}),
    )["id"]
        .as_uid()
        .unwrap();
    let doc = ok(&mut s, "document.get", json!({}));
    assert!(doc["resources"].to_string().contains("plugin_object"));
    let mut t = Session::with_sequential_ids();
    ok(&mut t, "document.load", json!({"document": doc}));
    assert_eq!(ok(&mut t, "plugin.list", json!({}))[0]["name"], "Bench");
    assert_eq!(
        ok(&mut t, "plugin.object", json!({"id": g}))["name"],
        "Bench"
    );
    // The shapes are stored, so a session without Python can still show and export them.
    assert_eq!(children(&t, g).len(), children(&s, g).len());
    assert_eq!(instance_info(&t, ObjectId(g)).unwrap()["name"], "Bench");
    assert!(instance_info(&t, ObjectId::from_num(1)).is_none());
    // Load keeps the scripting settings of the session.
    s.scripting.allow_agent_scripts = true;
    s.replace_document(Default::default());
    assert!(s.scripting.allow_agent_scripts);
}

#[test]
fn script_runs_need_a_live_session_and_report_their_output() {
    let mut s = open_session();
    assert!(
        fail(&mut s, "script.run", json!({"source": "print(1)"})).contains("running Kadakas app")
    );
    s.scripting.endpoint = Some(("127.0.0.1:9".into(), "tok".into()));
    assert_eq!(ok(&mut s, "script.info", json!({}))["live_session"], true);
    assert!(fail(&mut s, "script.run", json!({})).contains("path or source"));
    assert!(fail(&mut s, "script.run", json!({"source": "x", "args": 5})).contains("args"));
    assert!(fail(&mut s, "script.run", json!({"source": "x", "args": [1]})).contains("text"));
    let run = ok(
        &mut s,
        "script.run",
        json!({"source": "import os, sys\nprint(os.environ['KADAKAS_ADDR'], os.environ['KADAKAS_TOKEN'], sys.argv[1:])\nprint('bad', file=sys.stderr)\n", "args": ["a b"]}),
    )["run"]
        .as_u64()
        .unwrap();
    let mut status = Value::Null;
    for _ in 0..300 {
        status = ok(&mut s, "script.status", json!({"run": run}));
        if status["status"] != "running" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(status["status"], "finished", "{status}");
    assert_eq!(status["exit_code"], 0);
    let text = status["lines"].to_string();
    assert!(text.contains("127.0.0.1:9 tok ['a b']"), "{text}");
    assert!(text.contains("\"stream\":\"err\""), "{text}");
    let next = status["next"].as_u64().unwrap();
    assert_eq!(
        ok(&mut s, "script.status", json!({"run": run, "since": next}))["lines"],
        json!([])
    );
    assert_eq!(
        ok(&mut s, "script.list", json!({}))[0]["status"],
        "finished"
    );
    assert_eq!(
        ok(&mut s, "script.stop", json!({"run": run}))["stopped"],
        false
    );
    assert!(fail(&mut s, "script.status", json!({"run": 99})).contains("no script run"));
    assert!(fail(&mut s, "script.status", json!({})).contains("run number"));
    // A script file must be a .py file inside the allowed folders.
    s.paths = crate::PathPolicy::restricted(vec![std::env::temp_dir()]);
    assert!(fail(&mut s, "script.run", json!({"path": "/etc/hosts"})).contains(".py"));
    let m = fail(&mut s, "script.run", json!({"path": "/etc/evil.py"}));
    assert!(m.contains("cannot read") || m.contains("allowed"), "{m}");
}

#[test]
fn a_script_that_does_not_end_can_be_stopped() {
    let mut s = open_session();
    s.scripting.endpoint = Some(("127.0.0.1:9".into(), "tok".into()));
    let run = ok(
        &mut s,
        "script.run",
        json!({"source": "import time\ntime.sleep(60)\n"}),
    )["run"]
        .as_u64()
        .unwrap();
    assert_eq!(
        ok(&mut s, "script.stop", json!({"run": run}))["stopped"],
        true
    );
    let mut st = Value::Null;
    for _ in 0..300 {
        st = ok(&mut s, "script.status", json!({"run": run}));
        if st["status"] != "running" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(st["status"], "stopped");
}

#[test]
fn without_python_the_reset_fails_with_a_clear_message() {
    let mut s = open_session();
    let def = define_circles(&mut s);
    s.scripting.interpreter = Some(Interpreter {
        program: "/no/such/python".into(),
        prefix: vec![],
    });
    let msg = fail(
        &mut s,
        "plugin.place",
        json!({"definition": def, "at": [0, 0]}),
    );
    assert!(msg.contains("cannot start"), "{msg}");
    assert_eq!(count(&s), 0);
}

#[test]
fn describe_lists_the_scripting_methods() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    assert!(
        d["scripting"]["plugin_objects"]
            .as_str()
            .unwrap()
            .contains("reset(params, geometry)")
    );
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for n in [
        "plugin.place",
        "plugin.set_params",
        "script.run",
        "script.status",
    ] {
        assert!(names.contains(&n), "{n}");
    }
}

// ---------------------------------------------------------------------------------------------
// Trust: a script from a document never runs until the user trusted its exact text.

/// A program that records every start. If it is the interpreter and the marker stays missing,
/// no Python was started.
struct Spy {
    dir: std::path::PathBuf,
}

impl Spy {
    fn new(name: &str) -> Spy {
        let dir = std::env::temp_dir().join(format!("kadakas-spy-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let prog = dir.join("spy.sh");
        std::fs::write(
            &prog,
            format!(
                "#!/bin/sh\ntouch {}\nexit 1\n",
                dir.join("started").display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Spy { dir }
    }

    fn interpreter(&self) -> Interpreter {
        Interpreter {
            program: self.dir.join("spy.sh"),
            prefix: vec![],
        }
    }

    fn started(&self) -> bool {
        self.dir.join("started").exists()
    }
}

/// A session with a plug-in object from a "downloaded" document: it was made by a session with the
/// flag, and then loaded into a fresh session that has no flag, no trust, and a spy as interpreter.
fn downloaded(spy: &Spy) -> (Session, kadakas_core::uid::Uid, kadakas_core::uid::Uid) {
    let mut maker = open_session();
    let def = define_circles(&mut maker);
    let g = ok(
        &mut maker,
        "plugin.place",
        json!({"definition": def, "at": [0, 0]}),
    )["id"]
        .as_uid()
        .unwrap();
    let doc = ok(&mut maker, "document.get", json!({}));
    let mut s = Session::with_sequential_ids();
    s.scripting.interpreter = Some(spy.interpreter());
    ok(&mut s, "document.load", json!({"document": doc}));
    (s, def, g)
}

#[test]
fn an_untrusted_definition_never_starts_python() {
    let spy = Spy::new("paths");
    let (mut s, def, g) = downloaded(&spy);
    let gid = ObjectId(g);
    // Open, list, select (Object Info reads the object): no script.
    ok(&mut s, "plugin.list", json!({}));
    let info = instance_info(&s, gid).unwrap();
    assert_eq!(info["trusted"], false);
    assert_eq!(info["origin"], "this document");
    assert!(info["script"].as_str().unwrap().contains("def reset"));
    // Edit and Reset, as the user: refused with the code the app turns into the dialog.
    let one = json!({"n": 2});
    let one = one.as_object().unwrap();
    assert_eq!(
        user_set_params(&mut s, gid, one).unwrap_err().code,
        UNTRUSTED_CODE
    );
    assert_eq!(
        user_prepare(&s, gid, &serde_json::Map::new())
            .err()
            .unwrap()
            .code,
        UNTRUSTED_CODE
    );
    // Every API path, without the flag.
    for (m, p) in [
        ("plugin.place", json!({"definition": def, "at": [0, 0]})),
        ("plugin.set_params", json!({"id": g, "params": {"n": 2}})),
        ("plugin.reset", json!({"id": g})),
        ("plugin.reset", json!({"definition": def})),
        ("plugin.preview", json!({"definition": def, "at": [0, 0]})),
        (
            "plugin.update",
            json!({"id": def, "script": "x = 1", "reset": true}),
        ),
    ] {
        let e = call(&mut s, m, p).unwrap_err();
        assert_eq!(e.code, 4, "{m}: {}", e.message);
    }
    assert!(!spy.started(), "an untrusted script started Python");
    // Trust is by the exact text. The user trusts it; now Python starts (the spy fails the run).
    trust_it(&mut s, gid);
    let r = user_set_params(&mut s, gid, one).unwrap();
    assert_eq!(r["ok"], false);
    assert!(spy.started());
    std::fs::remove_dir_all(&spy.dir).unwrap();
}

#[test]
fn changing_a_script_through_the_api_drops_trust() {
    let spy = Spy::new("update");
    let (mut s, def, g) = downloaded(&spy);
    let gid = ObjectId(g);
    trust_it(&mut s, gid);
    assert_eq!(instance_info(&s, gid).unwrap()["trusted"], true);
    // Any agent can edit a definition (it is data) but the new text has a new hash.
    ok(
        &mut s,
        "plugin.update",
        json!({"id": def, "script": "import os\ndef reset(p, g):\n    return []\n"}),
    );
    let info = instance_info(&s, gid).unwrap();
    assert_eq!(info["trusted"], false);
    let err = user_set_params(&mut s, gid, &serde_json::Map::new()).unwrap_err();
    assert_eq!(err.code, UNTRUSTED_CODE);
    assert!(!spy.started());
    // Putting the old text back does not need a new dialog: the user trusted exactly that text.
    ok(
        &mut s,
        "plugin.update",
        json!({"id": def, "script": CIRCLES}),
    );
    assert_eq!(instance_info(&s, gid).unwrap()["trusted"], true);
    std::fs::remove_dir_all(&spy.dir).unwrap();
}

#[test]
fn nothing_in_the_api_adds_trust() {
    let mut s = open_session();
    let names: Vec<&str> = crate::METHODS.iter().map(|(n, _)| *n).collect();
    // The one exception is `plugins.trust` for sandboxed WebAssembly modules. It needs the flag for
    // agents, lasts for the session only and never writes the trust file (see wasm_plugin_api).
    assert!(
        !names
            .iter()
            .any(|n| n.contains("trust") && *n != "plugins.trust")
    );
    assert!(s.scripting.trust.is_empty());
    let def = define_circles(&mut s);
    ok(
        &mut s,
        "plugin.place",
        json!({"definition": def, "at": [0, 0]}),
    );
    assert!(
        s.scripting.trust.is_empty(),
        "running with the flag adds no trust"
    );
    let e = fail(&mut s, "plugin.trust", json!({"id": def}));
    assert!(e.contains("unknown method"));
}

#[test]
fn shipped_examples_are_trusted_by_hash() {
    let s = Session::with_sequential_ids();
    for (_, d) in kadakas_script::examples::all() {
        assert!(s.scripting.trust.is_trusted(&d.script));
    }
}

#[test]
fn agent_script_files_are_private_and_never_follow_a_link() {
    let mut s = open_session();
    s.scripting.endpoint = Some(("127.0.0.1:9".into(), "tok".into()));
    for _ in 0..2 {
        ok(&mut s, "script.run", json!({"source": "print(1)"}));
    }
    assert_eq!(s.scripting.temp_dirs.len(), 2);
    let dirs: Vec<_> = s
        .scripting
        .temp_dirs
        .iter()
        .map(|d| d.path().to_path_buf())
        .collect();
    assert_ne!(dirs[0], dirs[1], "each run has its own folder");
    for d in &dirs {
        assert!(d.join("agent.py").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(d).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert!(!d.starts_with(std::env::temp_dir().join("kadakas-scripts-0")));
    }
    // A planted link at the old fixed place is not used.
    #[cfg(unix)]
    {
        let old = std::env::temp_dir().join(format!("kadakas-scripts-{}", std::process::id()));
        std::fs::create_dir_all(&old).unwrap();
        let victim = old.join("victim.txt");
        std::fs::write(&victim, "keep").unwrap();
        let _ = std::os::unix::fs::symlink(&victim, old.join("agent-0.py"));
        ok(&mut s, "script.run", json!({"source": "print(2)"}));
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        std::fs::remove_dir_all(old).unwrap();
    }
}

#[test]
fn text_and_radius_limits_apply_to_generated_shapes() {
    let mut s = open_session();
    let d = ok(
        &mut s,
        "plugin.define",
        json!({"name": "Bad", "shape_type": "point",
               "script": "def reset(p, g):\n    return [circle(g.at, -5)]\n"}),
    )["id"]
        .as_uid()
        .unwrap();
    assert!(
        fail(
            &mut s,
            "plugin.place",
            json!({"definition": d, "at": [0, 0]})
        )
        .contains("radius")
    );
    ok(
        &mut s,
        "plugin.update",
        json!({"id": d, "script": "def reset(p, g):\n    return [text(g.at, 'x' * 20000, 10)]\n"}),
    );
    assert!(
        fail(
            &mut s,
            "plugin.place",
            json!({"definition": d, "at": [0, 0]})
        )
        .contains("at most")
    );
}

/// What the dialog does: trust the hash of the text on the screen.
fn trust_it(s: &mut Session, id: ObjectId) {
    let hash = instance_info(s, id).unwrap()["script_hash"]
        .as_str()
        .unwrap()
        .to_string();
    trust_plugin(s, id, &hash).unwrap();
}

#[test]
fn trust_is_only_for_the_hash_the_dialog_showed() {
    let spy = Spy::new("hash");
    let (mut s, def, g) = downloaded(&spy);
    let gid = ObjectId(g);
    let shown = instance_info(&s, gid).unwrap()["script_hash"]
        .as_str()
        .unwrap()
        .to_string();
    // After the user looked at the dialog, an agent swaps the script.
    ok(
        &mut s,
        "plugin.update",
        json!({"id": def, "script": "import os\ndef reset(p, g):\n    return []\n"}),
    );
    let e = trust_plugin(&mut s, gid, &shown).unwrap_err();
    assert!(e.contains("changed"), "{e}");
    assert!(s.scripting.trust.is_empty());
    assert_eq!(
        user_set_params(&mut s, gid, &serde_json::Map::new())
            .unwrap_err()
            .code,
        UNTRUSTED_CODE
    );
    assert!(trust_plugin(&mut s, gid, "not a hash").is_err());
    assert!(!spy.started());
    std::fs::remove_dir_all(&spy.dir).unwrap();
}

#[test]
fn a_script_with_a_hidden_return_cannot_be_defined() {
    let mut s = open_session();
    let e = fail(
        &mut s,
        "plugin.define",
        json!({"name": "Sneaky", "shape_type": "point",
               "script": "def reset(p, g):\n    return []  # docs\rimport os\n"}),
    );
    assert!(e.contains("U+000D"), "{e}");
}
