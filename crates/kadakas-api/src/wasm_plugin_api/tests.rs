//! Behavior tests of the `plugins.*` methods, with the example plug-in "Raised planter" and with
//! small modules written as text (WAT).

use kadakas_core::change::Origin;
use kadakas_core::model::Shape;
use kadakas_core::uid::{AsUid, Uid};
use serde_json::{Value, json};

use super::*;

const PLANTER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../kadakas-plugin/tests/fixtures/raised_planter.wasm"
);
const PLANTER_ID: &str = "org.kadakas.raised-planter";

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn err(s: &mut Session, method: &str, params: Value) -> RpcError {
    s.handle(method, params)
        .expect_err(&format!("{method} should fail"))
}

/// A session where agents may run plug-ins.
fn open_session() -> Session {
    let mut s = Session::with_sequential_ids();
    s.scripting.allow_agent_scripts = true;
    s
}

fn installed(s: &mut Session) -> Value {
    ok(s, "plugins.install", json!({"path": PLANTER}))
}

fn place(s: &mut Session, extra: Value) -> Uid {
    let mut p = json!({"plugin": PLANTER_ID, "min": [0, 0], "max": [2000, 800]});
    p.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    ok(s, "plugins.place", p)["id"].as_uid().unwrap()
}

fn children(s: &Session, id: Uid) -> Vec<ObjectId> {
    s.editor.doc().children(ObjectId(id))
}

fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kadakas-wasm-api-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A module with a fixed manifest whose `reset` runs `body` (WAT instructions that leave an i64).
fn wat_module(manifest: &str, body: &str) -> Vec<u8> {
    let esc = manifest.replace('\\', "\\\\").replace('"', "\\\"");
    wat::parse_str(format!(
        r#"(module
          (memory (export "memory") 2)
          (data (i32.const 1024) "{esc}")
          (func (export "alloc") (param i32) (result i32) (i32.const 65536))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64)
            (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {len})))
          (func (export "reset") (param i32 i32) (result i64) {body}))"#,
        len = manifest.len()
    ))
    .unwrap()
}

const POINT_MANIFEST: &str = r#"{"id":"test.spin","name":"Spin","version":"1","abi_version":1,"kinds":["object"],"object":{"shape_type":"point"}}"#;

#[test]
fn info_and_list_start_empty_and_name_the_engine() {
    let mut s = open_session();
    let info = ok(&mut s, "plugins.info", json!({}));
    assert_eq!(info["engine"], "wasmtime");
    assert_eq!(info["installed"], 0);
    assert_eq!(info["agent_plugins_allowed"], true);
    assert_eq!(info["abi_version"], 1);
    assert!(info["limits"]["fuel"].as_u64().unwrap() > 0);
    assert_eq!(ok(&mut s, "plugins.list", json!(null)), json!([]));
    // A wrong params type is refused.
    assert_eq!(err(&mut s, "plugins.list", json!([1])).code, -32602);
    assert!(handle(&mut s, "plugin.list", json!({})).is_none());
}

#[test]
fn install_reads_the_manifest_and_is_idempotent() {
    let mut s = open_session();
    let first = installed(&mut s);
    assert_eq!(first["id"], PLANTER_ID);
    assert_eq!(first["name"], "Raised planter");
    assert_eq!(first["kinds"], json!(["object"]));
    assert_eq!(first["shape_type"], "rectangle");
    assert_eq!(first["trusted"], false);
    assert_eq!(first["existing"], false);
    assert_eq!(first["hash"].as_str().unwrap().len(), 64);
    assert!(first["params"].as_array().unwrap().len() >= 5);
    let again = installed(&mut s);
    assert_eq!(again["existing"], true);
    assert_eq!(
        ok(&mut s, "plugins.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(ok(&mut s, "plugins.info", json!({}))["installed"], 1);
}

#[test]
fn install_refuses_bad_files() {
    let mut s = open_session();
    assert_eq!(err(&mut s, "plugins.install", json!({})).code, -32602);
    let junk = temp_file("junk.wasm", b"hello");
    let e = err(&mut s, "plugins.install", json!({"path": junk}));
    assert_eq!(e.code, -32602);
    assert!(
        e.message.contains("not a WebAssembly module"),
        "{}",
        e.message
    );
    let missing = std::env::temp_dir().join("kadakas-nothing-here.wasm");
    assert_eq!(
        err(&mut s, "plugins.install", json!({"path": missing})).code,
        1
    );
    // A module that imports something (what a WASI module does).
    let wasi = wat::parse_str(
        r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "alloc") (param i32) (result i32) (i32.const 0))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64) (i64.const 0))
          (func (export "reset") (param i32 i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    let path = temp_file("wasi.wasm", &wasi);
    let e = err(&mut s, "plugins.install", json!({"path": path}));
    assert_eq!(e.code, -32602);
    assert!(e.message.contains("imports"), "{}", e.message);
    // A module that asks for a permission is a policy matter.
    let asking = POINT_MANIFEST.replace("\"kinds\"", "\"permissions\":[\"filesystem\"],\"kinds\"");
    let path = temp_file("asking.wasm", &wat_module(&asking, "(i64.const 0)"));
    let e = err(&mut s, "plugins.install", json!({"path": path}));
    assert_eq!(e.kind, "policy");
    // A module over the size limit.
    let mut limits = *s.wasm_plugins.registry.limits();
    limits.max_module_bytes = 1000;
    s.wasm_plugins.registry.set_limits(limits);
    let e = err(&mut s, "plugins.install", json!({"path": PLANTER}));
    assert_eq!(e.kind, "limit");
    assert_eq!(ok(&mut s, "plugins.list", json!({})), json!([]));
}

#[test]
fn the_path_policy_applies_to_install() {
    let mut s = open_session();
    let dir = std::env::temp_dir().join(format!("kadakas-wasm-policy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.paths = crate::PathPolicy::restricted([dir.canonicalize().unwrap()]);
    // Outside the allowed folder.
    let e = err(&mut s, "plugins.install", json!({"path": PLANTER}));
    assert_eq!(e.kind, "policy");
    // Inside the folder, but the wrong extension.
    let wrong = dir.join("module.bin");
    std::fs::write(&wrong, std::fs::read(PLANTER).unwrap()).unwrap();
    let e = err(&mut s, "plugins.install", json!({"path": wrong}));
    assert!(e.message.contains(".wasm"), "{}", e.message);
    // Inside the folder with the right extension.
    let good = dir.join("planter.wasm");
    std::fs::write(&good, std::fs::read(PLANTER).unwrap()).unwrap();
    ok(&mut s, "plugins.install", json!({"path": good}));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_untrusted_module_does_not_run_without_the_flag() {
    let mut s = Session::with_sequential_ids();
    installed(&mut s);
    let hash = ok(&mut s, "plugins.list", json!({}))[0]["hash"]
        .as_str()
        .unwrap()
        .to_string();
    let e = err(
        &mut s,
        "plugins.place",
        json!({"plugin": PLANTER_ID, "min": [0, 0], "max": [2000, 800]}),
    );
    assert_eq!(e.code, 4);
    assert!(e.message.contains(&hash) && e.message.contains("--allow-agent-scripts"));
    assert_eq!(s.editor.doc().objects.len(), 0);
    // An agent cannot trust the module itself.
    let e = err(
        &mut s,
        "plugins.trust",
        json!({"plugin": PLANTER_ID, "hash": hash}),
    );
    assert_eq!(e.code, 4);
    assert!(
        err(
            &mut s,
            "plugins.preview",
            json!({"plugin": PLANTER_ID, "min": [0, 0], "max": [2000, 800]})
        )
        .code
            == 4
    );
    // The user can, in the dialog of the app.
    assert!(trust_module(&mut s, "nope", &hash).is_err());
    assert!(
        trust_module(&mut s, PLANTER_ID, "0000")
            .unwrap_err()
            .contains("changed")
    );
    trust_module(&mut s, PLANTER_ID, &hash).unwrap();
    assert_eq!(ok(&mut s, "plugins.list", json!({}))[0]["trusted"], true);
    let id = place(&mut s, json!({}));
    assert_eq!(children(&s, id).len(), 3);
}

#[test]
fn plugins_trust_checks_the_hash() {
    let mut s = open_session();
    installed(&mut s);
    let hash = ok(&mut s, "plugins.list", json!({}))[0]["hash"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        err(&mut s, "plugins.trust", json!({"plugin": PLANTER_ID})).code,
        -32602
    );
    let e = err(
        &mut s,
        "plugins.trust",
        json!({"plugin": PLANTER_ID, "hash": "ab"}),
    );
    assert_eq!(e.kind, "conflict");
    assert_eq!(
        err(
            &mut s,
            "plugins.trust",
            json!({"plugin": "nope", "hash": hash})
        )
        .code,
        2
    );
    // The name works as well as the id, with any case.
    let r = ok(
        &mut s,
        "plugins.trust",
        json!({"plugin": "RAISED PLANTER", "hash": hash.to_uppercase()}),
    );
    assert_eq!(r["trusted"], true);
    assert_eq!(r["scope"], "session");
    assert_eq!(ok(&mut s, "plugins.list", json!({}))[0]["trusted"], true);
    // The trust of an agent is not in the file of the user.
    assert!(s.wasm_plugins.registry.trust.is_empty());
}

#[test]
fn place_stores_plain_shapes_and_the_parameters() {
    let mut s = open_session();
    installed(&mut s);
    let id = place(
        &mut s,
        json!({"params": {"wall": 100, "label": false}, "name": "Bed 1"}),
    );
    let kids = children(&s, id);
    assert_eq!(kids.len(), 2);
    for k in &kids {
        assert!(matches!(
            s.editor.doc().object(*k).unwrap().shape,
            Shape::Polyline { .. }
        ));
    }
    assert_eq!(s.selection, vec![ObjectId(id)]);
    let info = ok(&mut s, "plugins.object", json!({"id": id}));
    assert_eq!(info["plugin"], PLANTER_ID);
    assert_eq!(info["params"]["wall"], 100.0);
    assert_eq!(info["params"]["label"], false);
    assert_eq!(info["geometry"]["max"], json!([2000.0, 800.0]));
    assert_eq!(info["installed"], true);
    // The host flag lets the module run, but the user did not trust it.
    assert_eq!(info["trusted"], false);
    assert_eq!(info["error"], Value::Null);
    assert_eq!(info["schema"].as_array().unwrap().len(), 6);
    assert_eq!(ok(&mut s, "plugins.list", json!({}))[0]["instances"], 1);
    // The group is a normal group in the document: the data is the only trace of the plug-in.
    let group = s.editor.doc().object(ObjectId(id)).unwrap();
    assert_eq!(group.data[DATA_KEY]["version"], "1.0.0");
}

#[test]
fn a_placement_is_one_undo_step_with_the_origin_script() {
    let mut s = open_session();
    installed(&mut s);
    let before = s.editor.revision();
    let id = place(&mut s, json!({}));
    let sets = s.editor.changes_since(before);
    assert_eq!(sets.changes.len(), 1);
    assert_eq!(sets.changes[0].origin, Origin::Script);
    assert!(sets.changes[0].added.objects.contains(&ObjectId(id)));
    assert_eq!(s.editor.origin(), Origin::User);
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    assert_eq!(s.editor.doc().objects.len(), 0);
}

#[test]
fn set_params_regenerates_through_one_undo_step() {
    let mut s = open_session();
    installed(&mut s);
    let id = place(&mut s, json!({}));
    let old = children(&s, id);
    let rev = s.editor.revision();
    let r = ok(
        &mut s,
        "plugins.set_params",
        json!({"id": id, "params": {"label": false, "corners": "Cut"}}),
    );
    assert_eq!(r["ok"], true);
    let now = children(&s, id);
    assert_eq!(now.len(), 2);
    assert!(old.iter().all(|o| !now.contains(o)));
    let sets = s.editor.changes_since(rev);
    assert_eq!(sets.changes.len(), 1);
    assert_eq!(sets.changes[0].origin, Origin::Script);
    // Undo brings the old shapes back.
    ok(&mut s, "undo", json!({}));
    assert_eq!(children(&s, id), old);
    assert_eq!(
        ok(&mut s, "plugins.object", json!({"id": id}))["params"]["corners"],
        "Square"
    );
    // Bad input is refused and changes nothing.
    assert_eq!(
        err(
            &mut s,
            "plugins.set_params",
            json!({"id": id, "params": {"wall": 1}})
        )
        .code,
        -32602
    );
    assert_eq!(
        err(&mut s, "plugins.set_params", json!({"id": id})).code,
        -32602
    );
    assert_eq!(children(&s, id), old);
}

#[test]
fn a_module_failure_keeps_the_old_shapes_and_shows_the_error() {
    let mut s = open_session();
    installed(&mut s);
    let id = place(&mut s, json!({}));
    let old = children(&s, id);
    // Walls of 500 mm do not fit into a planter that is 800 mm deep.
    let r = ok(
        &mut s,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 500}}),
    );
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("too small"));
    assert_eq!(children(&s, id), old);
    let info = ok(&mut s, "plugins.object", json!({"id": id}));
    assert!(info["error"].as_str().unwrap().contains("too small"));
    assert_eq!(info["params"]["wall"], 500.0);
    // Placing the same thing fails and adds nothing.
    let n = s.editor.doc().objects.len();
    let e = err(
        &mut s,
        "plugins.place",
        json!({"plugin": PLANTER_ID, "min": [0, 0], "max": [900, 300]}),
    );
    assert_eq!(e.code, 1);
    assert_eq!(s.editor.doc().objects.len(), n);
}

#[test]
fn reset_follows_a_moved_group() {
    let mut s = open_session();
    installed(&mut s);
    let id = place(&mut s, json!({}));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [5000, 0]}}),
    );
    ok(
        &mut s,
        "plugins.set_params",
        json!({"id": id, "params": {"height": 900}}),
    );
    let info = ok(&mut s, "plugins.object", json!({"id": id}));
    assert_eq!(info["geometry"]["min"][0], 5000.0);
    let lowest = children(&s, id)
        .iter()
        .map(|c| s.editor.doc().object(*c).unwrap().shape.bounds().0.x)
        .fold(f64::MAX, f64::min);
    assert!((lowest - 5000.0).abs() < 1e-6, "{lowest}");
}

#[test]
fn reset_many_by_plugin_ids_or_one_id() {
    let mut s = open_session();
    installed(&mut s);
    let a = place(&mut s, json!({}));
    let b = place(&mut s, json!({"min": [5000, 0], "max": [7000, 800]}));
    let all = ok(&mut s, "plugins.reset", json!({"plugin": PLANTER_ID}));
    assert_eq!(all["results"].as_array().unwrap().len(), 2);
    assert!(all["results"][0]["ok"].as_bool().unwrap());
    let some = ok(&mut s, "plugins.reset", json!({"ids": [a]}));
    assert_eq!(some["results"].as_array().unwrap().len(), 1);
    let one = ok(&mut s, "plugins.reset", json!({"id": b}));
    assert_eq!(one["results"][0]["id"], json!(b));
    assert_eq!(
        err(&mut s, "plugins.reset", json!({"ids": ["x"]})).code,
        -32602
    );
    assert_eq!(err(&mut s, "plugins.reset", json!({})).code, -32602);
    // An empty list changes nothing.
    let rev = s.editor.revision();
    ok(&mut s, "plugins.reset", json!({"ids": []}));
    assert_eq!(s.editor.revision(), rev);
    assert!(
        err(
            &mut s,
            "plugins.reset",
            json!({"id": "00000000-0000-0000-0000-0000000000ff"})
        )
        .code
            == 2
    );
}

#[test]
fn preview_runs_without_changing_the_document() {
    let mut s = open_session();
    installed(&mut s);
    let rev = s.editor.revision();
    let r = ok(
        &mut s,
        "plugins.preview",
        json!({"plugin": PLANTER_ID, "min": [0, 0], "max": [2000, 800], "params": {"label": false}}),
    );
    assert_eq!(r["shapes"].as_array().unwrap().len(), 2);
    assert_eq!(r["shapes"][0]["kind"], "polyline");
    assert_eq!(
        r["shapes"][0]["attrs"]["pen"],
        json!([0x6b, 0x4f, 0x3a, 255])
    );
    assert_eq!(s.editor.revision(), rev);
}

#[test]
fn bad_input_is_refused() {
    let mut s = open_session();
    installed(&mut s);
    let base = json!({"plugin": PLANTER_ID, "min": [0, 0], "max": [2000, 800]});
    let with = |k: &str, v: Value| {
        let mut b = base.clone();
        b[k] = v;
        b
    };
    assert_eq!(
        err(&mut s, "plugins.place", with("params", json!({"wall": 5}))).code,
        -32602
    );
    assert_eq!(
        err(&mut s, "plugins.place", with("params", json!({"nope": 5}))).code,
        -32602
    );
    assert_eq!(
        err(&mut s, "plugins.place", with("params", json!(3))).code,
        -32602
    );
    assert_eq!(
        err(&mut s, "plugins.place", with("min", json!("x"))).code,
        -32602
    );
    assert_eq!(
        err(
            &mut s,
            "plugins.place",
            json!({"plugin": PLANTER_ID, "at": [0, 0]})
        )
        .code,
        -32602
    );
    assert_eq!(
        err(&mut s, "plugins.place", json!({"plugin": 5})).code,
        -32602
    );
    assert_eq!(err(&mut s, "plugins.place", json!({})).code, -32602);
    assert_eq!(
        err(
            &mut s,
            "plugins.place",
            json!({"plugin": "nope", "at": [0, 0]})
        )
        .code,
        2
    );
    assert_eq!(
        err(&mut s, "plugins.object", json!({"id": "x"})).code,
        -32602
    );
    assert_eq!(
        err(
            &mut s,
            "plugins.object",
            json!({"id": "00000000-0000-0000-0000-0000000000ff"})
        )
        .code,
        2
    );
    let name = with("name", json!(""));
    assert_eq!(err(&mut s, "plugins.place", name).code, -32602);
    assert_eq!(err(&mut s, "plugins.nothing", json!({})).code, -32601);
    assert_eq!(s.editor.doc().objects.len(), 0);
    // An object that is not a plug-in object.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}),
    );
    let any = s.editor.doc().objects.keys().next().copied().unwrap();
    assert_eq!(err(&mut s, "plugins.object", json!({"id": any})).code, 3);
    assert_eq!(
        err(
            &mut s,
            "plugins.set_params",
            json!({"id": any, "params": {}})
        )
        .code,
        3
    );
}

#[test]
fn remove_needs_that_no_object_uses_the_plugin() {
    let mut s = open_session();
    installed(&mut s);
    let id = place(&mut s, json!({}));
    let e = err(&mut s, "plugins.remove", json!({"plugin": PLANTER_ID}));
    assert!(e.message.contains("1 objects use"), "{}", e.message);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [id]}}),
    );
    assert_eq!(
        ok(&mut s, "plugins.remove", json!({"plugin": PLANTER_ID}))["ok"],
        true
    );
    assert_eq!(ok(&mut s, "plugins.list", json!({})), json!([]));
    assert_eq!(
        err(&mut s, "plugins.remove", json!({"plugin": PLANTER_ID})).code,
        2
    );
}

#[test]
fn objects_survive_a_missing_plugin_as_plain_shapes() {
    let mut s = open_session();
    installed(&mut s);
    let id = place(&mut s, json!({}));
    let kids = children(&s, id);
    // A new session that has not installed the module, as after a restart.
    s.wasm_plugins = WasmPluginState::default();
    let info = ok(&mut s, "plugins.object", json!({"id": id}));
    assert_eq!(info["installed"], false);
    assert_eq!(info["trusted"], false);
    assert_eq!(info["schema"], Value::Null);
    assert_eq!(info["children"].as_array().unwrap().len(), kids.len());
    let e = err(
        &mut s,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 120}}),
    );
    assert_eq!(e.code, 2);
    assert!(e.message.contains("plugins.install"));
    assert_eq!(children(&s, id), kids);
    // The shapes are in the document and in its picture.
    assert!(s.render_svg(400).contains("<polygon"));
}

#[test]
fn installed_plugins_stay_when_the_document_is_replaced() {
    let mut s = open_session();
    installed(&mut s);
    s.replace_document(kadakas_core::model::Document::with_sequential_ids());
    assert_eq!(
        ok(&mut s, "plugins.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn another_build_of_a_used_plugin_is_refused() {
    let mut s = open_session();
    installed(&mut s);
    place(&mut s, json!({}));
    let other_manifest = format!(
        r#"{{"id":"{PLANTER_ID}","name":"Raised planter","version":"2","abi_version":1,"kinds":["tool"]}}"#
    );
    let path = temp_file("v2.wasm", &wat_module(&other_manifest, "(i64.const 0)"));
    let e = err(&mut s, "plugins.install", json!({"path": path}));
    assert_eq!(e.kind, "policy");
    assert!(e.message.contains("Remove them"));
}

#[test]
fn a_tool_only_plugin_installs_but_cannot_be_placed() {
    let mut s = open_session();
    let manifest =
        r#"{"id":"test.tool","name":"Tool","version":"1","abi_version":1,"kinds":["tool"]}"#;
    let path = temp_file("tool.wasm", &wat_module(manifest, "(i64.const 0)"));
    let r = ok(&mut s, "plugins.install", json!({"path": path}));
    assert_eq!(r["kinds"], json!(["tool"]));
    assert_eq!(r["shape_type"], Value::Null);
    let e = err(
        &mut s,
        "plugins.place",
        json!({"plugin": "test.tool", "at": [0, 0]}),
    );
    assert!(e.message.contains("provides no object"), "{}", e.message);
}

#[test]
fn an_endless_loop_stops_and_leaves_the_document_alone() {
    let mut s = open_session();
    let path = temp_file(
        "spin.wasm",
        &wat_module(POINT_MANIFEST, "(loop $l (br $l)) (i64.const 0)"),
    );
    let mut limits = *s.wasm_plugins.registry.limits();
    limits.fuel = 5_000_000;
    s.wasm_plugins.registry.set_limits(limits);
    ok(&mut s, "plugins.install", json!({"path": path}));
    let e = err(
        &mut s,
        "plugins.place",
        json!({"plugin": "test.spin", "at": [0, 0]}),
    );
    assert_eq!(e.kind, "limit");
    assert!(e.message.contains("fuel"), "{}", e.message);
    assert_eq!(s.editor.doc().objects.len(), 0);
}

#[test]
fn a_memory_bomb_and_a_crash_are_reported() {
    let mut s = open_session();
    let bomb = temp_file(
        "bomb.wasm",
        &wat_module(
            POINT_MANIFEST,
            "(drop (memory.grow (i32.const 100000))) (i64.const 0)",
        ),
    );
    ok(&mut s, "plugins.install", json!({"path": bomb}));
    let e = err(
        &mut s,
        "plugins.place",
        json!({"plugin": "test.spin", "at": [0, 0]}),
    );
    assert_eq!(e.kind, "limit");
    assert!(e.message.contains("memory"), "{}", e.message);
    // A module that traps. Remove the first one to reuse the id.
    ok(&mut s, "plugins.remove", json!({"plugin": "test.spin"}));
    let crash = temp_file("crash.wasm", &wat_module(POINT_MANIFEST, "(unreachable)"));
    ok(&mut s, "plugins.install", json!({"path": crash}));
    let e = err(
        &mut s,
        "plugins.place",
        json!({"plugin": "test.spin", "at": [0, 0]}),
    );
    assert_eq!(e.code, 1);
    assert!(e.message.contains("crashed"), "{}", e.message);
}

#[test]
fn bad_output_from_a_module_is_refused() {
    let mut s = open_session();
    // The answer is a string that is not JSON.
    let body = "(i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 20))";
    let path = temp_file("garbage.wasm", &wat_module(POINT_MANIFEST, body));
    ok(&mut s, "plugins.install", json!({"path": path}));
    let e = err(
        &mut s,
        "plugins.place",
        json!({"plugin": "test.spin", "at": [0, 0]}),
    );
    assert!(e.message.contains("bad answer"), "{}", e.message);
    assert_eq!(s.editor.doc().objects.len(), 0);
}

#[test]
fn describe_lists_the_methods_with_examples() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for n in [
        "plugins.info",
        "plugins.list",
        "plugins.install",
        "plugins.trust",
        "plugins.remove",
        "plugins.place",
        "plugins.set_params",
        "plugins.reset",
        "plugins.object",
        "plugins.preview",
    ] {
        assert!(names.contains(&n), "{n}");
    }
    let examples = d["plugins"]["examples"].as_array().unwrap();
    assert!(
        examples
            .iter()
            .all(|e| names.contains(&e["method"].as_str().unwrap()))
    );
    assert!(
        d["plugins"]["summary"]
            .as_str()
            .unwrap()
            .contains("sandboxed")
    );
}
