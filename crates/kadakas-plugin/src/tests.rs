//! Tests of the host: the example plug-in, the limits of the sandbox and bad answers.
//!
//! The happy path uses the committed module `tests/fixtures/raised_planter.wasm` (built by
//! `scripts/build-plugin-fixtures.sh` from `plugins/raised-planter`). The tests of the limits and of
//! bad modules write small modules as text (WAT) and compile them with the `wat` crate.

use kadakas_plugin_abi::{Geometry, ShapeType};
use serde_json::{Map, Value, json};

use crate::native::WasmtimeRuntime;
use crate::trust::{self, ModuleTrust, Permit};
use crate::{Limits, Plugin, PluginError, PluginRuntime, Registry};

const PLANTER: &[u8] = include_bytes!("../tests/fixtures/raised_planter.wasm");

fn rect(w: f64, d: f64) -> Geometry {
    Geometry::Rectangle {
        min: [0.0, 0.0],
        max: [w, d],
    }
}

fn params(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn permit_for(p: &Plugin) -> Permit {
    ModuleTrust::in_memory().permit(p.hash(), true).unwrap()
}

fn load(wasm: &[u8], limits: &Limits) -> Result<Plugin, PluginError> {
    Plugin::load(&WasmtimeRuntime::new(), wasm, limits)
}

/// A small limit for fuel, so that the tests of endless loops are quick.
fn quick() -> Limits {
    Limits {
        fuel: 2_000_000,
        ..Limits::default()
    }
}

// ----------------------------------------------------------------- WAT modules

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

const MANIFEST: &str = r#"{"id":"test.mod","name":"Test","version":"1.0","abi_version":2,"kinds":["object"],"object":{"shape_type":"point","params":[{"name":"n","type":"number","default":1}]}}"#;
const ONE_CIRCLE: &str =
    r#"{"status":"ok","shapes":[{"kind":"circle","center":[0,0],"radius":10}]}"#;

/// A module with a fixed manifest and a fixed answer. `reset_body` replaces the body of `reset`
/// (a list of WAT instructions that leave an `i64` on the stack). `extra` adds top level items.
fn module_with(manifest: &str, answer: &str, reset_body: Option<&str>, extra: &str) -> Vec<u8> {
    let m_ptr = 1024;
    let a_ptr = m_ptr + manifest.len() + 16;
    let default_reset = format!(
        "(i64.or (i64.shl (i64.const {a_ptr}) (i64.const 32)) (i64.const {}))",
        answer.len()
    );
    let src = format!(
        r#"(module
  (memory (export "memory") 8)
  (global $next (mut i32) (i32.const 393216))
  (data (i32.const {m_ptr}) "{m}")
  (data (i32.const {a_ptr}) "{a}")
  (func (export "alloc") (param $len i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (local.get $p))
  (func (export "dealloc") (param i32 i32))
  (func (export "describe") (result i64)
    (i64.or (i64.shl (i64.const {m_ptr}) (i64.const 32)) (i64.const {mlen})))
  (func (export "reset") (param i32 i32) (result i64) {body})
  (func (export "tool") (param i32 i32) (result i64) {default_reset})
  {extra}
)"#,
        m = escape(manifest),
        a = escape(answer),
        mlen = manifest.len(),
        body = reset_body.unwrap_or(&default_reset),
    );
    wat::parse_str(src).expect("the test module is valid WAT")
}

fn module(answer: &str) -> Vec<u8> {
    module_with(MANIFEST, answer, None, "")
}

fn run(wasm: &[u8]) -> Result<Vec<crate::Generated>, PluginError> {
    let p = load(wasm, &quick())?;
    p.reset(
        &permit_for(&p),
        &Map::new(),
        &Geometry::Point {
            at: [1.0, 2.0],
            rotation: 0.0,
        },
    )
}

// ---------------------------------------------------------------- the example

#[test]
fn the_example_plugin_loads_and_describes_itself() {
    let p = load(PLANTER, &Limits::default()).unwrap();
    let m = p.manifest();
    assert_eq!(m.id, "org.kadakas.raised-planter");
    assert_eq!(m.object.as_ref().unwrap().shape_type, ShapeType::Rectangle);
    assert_eq!(p.hash(), trust::hash(PLANTER));
    assert_eq!(p.size(), PLANTER.len());
    assert!(format!("{p:?}").contains("raised-planter"));
    // The module is small.
    assert!(PLANTER.len() < 400_000, "{}", PLANTER.len());
}

#[test]
fn the_example_makes_wall_soil_and_label() {
    let p = load(PLANTER, &Limits::default()).unwrap();
    let shapes = p
        .reset(&permit_for(&p), &Map::new(), &rect(2000.0, 800.0))
        .unwrap();
    assert_eq!(shapes.len(), 3);
    assert!(matches!(
        shapes[2].shape,
        kadakas_core::model::Shape::Text { .. }
    ));
    // The default colors come through as attributes.
    assert_eq!(shapes[0].attrs.pen.unwrap().0, [0x6b, 0x4f, 0x3a, 255]);
    assert_eq!(shapes[1].attrs.fill.unwrap().0, [0x8a, 0x6a, 0x45, 255]);
    let no_label = p
        .reset(
            &permit_for(&p),
            &params(json!({"label": false, "corners": "Cut", "wall": 100})),
            &rect(2000.0, 800.0),
        )
        .unwrap();
    assert_eq!(no_label.len(), 2);
    match &no_label[0].shape {
        kadakas_core::model::Shape::Polyline { points, closed } => {
            assert!(*closed);
            assert_eq!(points.len(), 8);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_example_says_when_the_object_is_too_small() {
    let p = load(PLANTER, &Limits::default()).unwrap();
    let e = p
        .reset(&permit_for(&p), &Map::new(), &rect(200.0, 800.0))
        .unwrap_err();
    assert!(
        matches!(&e, PluginError::Guest(m) if m.contains("too small")),
        "{e}"
    );
    assert!(e.to_string().starts_with("the plug-in failed"));
}

#[test]
fn the_host_checks_permit_geometry_and_parameters_before_it_runs_anything() {
    let p = load(PLANTER, &Limits::default()).unwrap();
    let other = Permit::for_test("0000");
    let e = p
        .reset(&other, &Map::new(), &rect(2000.0, 800.0))
        .unwrap_err();
    assert!(matches!(e, PluginError::Refused(m) if m.contains("not trusted")));
    let permit = permit_for(&p);
    let e = p
        .reset(
            &permit,
            &Map::new(),
            &Geometry::Point {
                at: [0.0, 0.0],
                rotation: 0.0,
            },
        )
        .unwrap_err();
    assert!(matches!(e, PluginError::Refused(m) if m.contains("rectangle")));
    let e = p
        .reset(&permit, &params(json!({"wall": 5})), &rect(2000.0, 800.0))
        .unwrap_err();
    assert!(matches!(e, PluginError::Refused(m) if m.contains("outside the range")));
    let e = p
        .reset(&permit, &params(json!({"nope": 5})), &rect(2000.0, 800.0))
        .unwrap_err();
    assert!(matches!(e, PluginError::Refused(m) if m.contains("unknown parameter")));
}

#[test]
fn a_plugin_without_an_object_cannot_be_reset() {
    let manifest =
        r#"{"id":"test.tool","name":"T","version":"1","abi_version":2,"kinds":["tool"],"tool":{}}"#;
    let p = load(&module_with(manifest, ONE_CIRCLE, None, ""), &quick()).unwrap();
    let e = p
        .reset(
            &permit_for(&p),
            &Map::new(),
            &Geometry::Point {
                at: [0.0, 0.0],
                rotation: 0.0,
            },
        )
        .unwrap_err();
    assert!(matches!(e, PluginError::Refused(m) if m.contains("no object")));
}

#[test]
fn the_request_reaches_the_module_as_json() {
    // This module answers with the request itself.
    let echo = module_with(
        MANIFEST,
        "",
        Some(
            "(i64.or (i64.shl (i64.extend_i32_u (local.get 0)) (i64.const 32)) (i64.extend_i32_u (local.get 1)))",
        ),
        "",
    );
    let loaded = WasmtimeRuntime::new()
        .load(&echo, &Limits::default())
        .unwrap();
    let req = br#"{"hello":"world"}"#;
    assert_eq!(loaded.reset(req).unwrap(), req);
}

// ------------------------------------------------------------------- limits

#[test]
fn an_endless_loop_runs_out_of_fuel() {
    let wasm = module_with(MANIFEST, "", Some("(loop $l (br $l)) (i64.const 0)"), "");
    let e = run(&wasm).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("fuel")),
        "{e}"
    );
    assert!(e.to_string().contains("over a limit"));
}

#[test]
fn the_fuel_is_per_call() {
    // A module that needs some fuel works again and again.
    let p = load(PLANTER, &Limits::default()).unwrap();
    for _ in 0..3 {
        p.reset(&permit_for(&p), &Map::new(), &rect(2000.0, 800.0))
            .unwrap();
    }
    // With almost no fuel, even the example fails.
    let starved = Limits {
        fuel: 50,
        ..Limits::default()
    };
    assert!(matches!(
        load(PLANTER, &starved),
        Err(PluginError::Limit(_))
    ));
}

#[test]
fn a_memory_bomb_hits_the_cap() {
    // Asks for 100 000 pages (6.5 GB) while it runs.
    let wasm = module_with(
        MANIFEST,
        "",
        Some("(drop (memory.grow (i32.const 100000))) (i64.const 0)"),
        "",
    );
    let e = run(&wasm).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("memory")),
        "{e}"
    );
}

#[test]
fn growing_in_small_steps_stops_at_the_cap() {
    let wasm = module_with(
        MANIFEST,
        "",
        Some("(loop $l (drop (memory.grow (i32.const 16))) (br $l)) (i64.const 0)"),
        "",
    );
    let limits = Limits {
        memory_bytes: 8 << 20,
        ..Limits::default()
    };
    let p = load(&wasm, &limits).unwrap();
    let e = p
        .reset(
            &permit_for(&p),
            &Map::new(),
            &Geometry::Point {
                at: [0.0, 0.0],
                rotation: 0.0,
            },
        )
        .unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("8388608")),
        "{e}"
    );
}

#[test]
fn a_module_that_declares_too_much_memory_does_not_load() {
    let wasm = wat::parse_str(
        r#"(module (memory (export "memory") 5000)
          (func (export "alloc") (param i32) (result i32) (i32.const 0))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64) (i64.const 0))
          (func (export "reset") (param i32 i32) (result i64) (i64.const 0))
          (func (export "tool") (param i32 i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    let e = load(&wasm, &Limits::default()).unwrap_err();
    assert!(matches!(e, PluginError::Limit(m) if m.contains("memory")));
}

#[test]
fn tables_are_capped_without_a_crash() {
    // table.grow answers -1 when the table would be too large. The module then returns its answer.
    let body = format!(
        "(if (i32.eq (table.grow $t (ref.null func) (i32.const 200000)) (i32.const -1))
           (then (return (i64.or (i64.shl (i64.const {a}) (i64.const 32)) (i64.const {l})))))
         (unreachable)",
        a = 1024 + MANIFEST.len() + 16,
        l = ONE_CIRCLE.len()
    );
    let wasm = module_with(MANIFEST, ONE_CIRCLE, Some(&body), "(table $t 1 funcref)");
    assert_eq!(run(&wasm).unwrap().len(), 1);
}

#[test]
fn endless_recursion_overflows_the_stack() {
    let wasm = module_with(
        MANIFEST,
        "",
        Some("(call $deep) (i64.const 0)"),
        "(func $deep (call $deep))",
    );
    let e = run(&wasm).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("stack")),
        "{e}"
    );
}

#[test]
fn a_trap_is_reported_not_propagated() {
    let wasm = module_with(MANIFEST, "", Some("(unreachable)"), "");
    let e = run(&wasm).unwrap_err();
    assert!(matches!(&e, PluginError::Trap(_)), "{e}");
    assert!(e.to_string().starts_with("the plug-in crashed"));
    // Reading outside the memory is a trap too.
    let wasm = module_with(
        MANIFEST,
        "",
        Some("(drop (i32.load (i32.const 100000000))) (i64.const 0)"),
        "",
    );
    assert!(matches!(run(&wasm), Err(PluginError::Trap(_))));
}

#[test]
fn a_module_that_is_too_big_is_refused_before_it_is_compiled() {
    let limits = Limits {
        max_module_bytes: 100,
        ..Limits::default()
    };
    let e = load(PLANTER, &limits).unwrap_err();
    assert!(matches!(e, PluginError::Limit(m) if m.contains("The limit is 100")));
}

// ------------------------------------------------------------ bad modules

#[test]
fn a_module_with_imports_does_not_load() {
    // This is what a module built for WASI looks like. It would get files and a clock.
    let wasm = wat::parse_str(
        r#"(module
          (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "alloc") (param i32) (result i32) (i32.const 0))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64) (i64.const 0))
          (func (export "reset") (param i32 i32) (result i64) (i64.const 0))
          (func (export "tool") (param i32 i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    let e = load(&wasm, &Limits::default()).unwrap_err();
    assert!(
        matches!(&e, PluginError::Invalid(m) if m.contains("wasi_snapshot_preview1.fd_write") && m.contains("no files")),
        "{e}"
    );
}

#[test]
fn a_module_must_export_the_whole_interface() {
    let wasm = wat::parse_str(r#"(module (memory (export "memory") 1))"#).unwrap();
    let e = load(&wasm, &Limits::default()).unwrap_err();
    assert!(matches!(e, PluginError::Invalid(m) if m.contains("'alloc'")));
}

#[test]
fn bytes_that_are_not_a_module_are_refused() {
    assert!(matches!(
        load(b"hello", &Limits::default()),
        Err(PluginError::Invalid(m)) if m.contains("not a WebAssembly module")
    ));
    assert!(matches!(
        load(b"\0asm\x01\0\0\0garbage", &Limits::default()),
        Err(PluginError::Invalid(_))
    ));
}

#[test]
fn a_bad_manifest_is_refused() {
    let wasm = module_with("not json", ONE_CIRCLE, None, "");
    assert!(
        matches!(load(&wasm, &quick()), Err(PluginError::Invalid(m)) if m.contains("manifest"))
    );
    let wrong_abi = MANIFEST.replace("\"abi_version\":2", "\"abi_version\":9");
    let wasm = module_with(&wrong_abi, ONE_CIRCLE, None, "");
    assert!(
        matches!(load(&wasm, &quick()), Err(PluginError::Invalid(m)) if m.contains("ABI version 9"))
    );
}

#[test]
fn a_manifest_that_asks_for_permissions_is_refused() {
    let asking = MANIFEST.replace("\"kinds\"", "\"permissions\":[\"network\"],\"kinds\"");
    let wasm = module_with(&asking, ONE_CIRCLE, None, "");
    let e = load(&wasm, &quick()).unwrap_err();
    assert!(
        matches!(&e, PluginError::Refused(m) if m.contains("'network'")),
        "{e}"
    );
}

#[test]
fn a_manifest_that_is_too_big_is_refused() {
    let big = format!(
        r#"{{"id":"test.big","name":"{}","version":"1","abi_version":2,"kinds":["tool"],"tool":{{}}}}"#,
        "x".repeat(300_000)
    );
    // The manifest is read with a size limit of its own.
    let wasm = wat::parse_str(format!(
        r#"(module (memory (export "memory") 8)
          (data (i32.const 16) "{}")
          (func (export "alloc") (param i32) (result i32) (i32.const 0))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64) (i64.const {}))
          (func (export "reset") (param i32 i32) (result i64) (i64.const 0))
          (func (export "tool") (param i32 i32) (result i64) (i64.const 0)))"#,
        escape(&big),
        (16u64 << 32) | big.len() as u64
    ))
    .unwrap();
    assert!(matches!(
        load(&wasm, &Limits::default()),
        Err(PluginError::Limit(_))
    ));
}

// ------------------------------------------------------------- bad answers

fn bad_output(answer: &str, what: &str) {
    let e = run(&module(answer)).unwrap_err();
    assert!(
        matches!(&e, PluginError::BadOutput(m) if m.contains(what)),
        "{e}"
    );
    assert!(e.to_string().starts_with("the plug-in gave a bad answer"));
}

#[test]
fn bad_output_is_rejected() {
    bad_output("this is not json", "not valid");
    bad_output(r#"{"status":"ok","shapes":"many"}"#, "not valid");
    bad_output(
        r#"{"status":"ok","shapes":[{"kind":"spaceship"}]}"#,
        "not valid",
    );
    bad_output(r#"{"status":"ok","shapes":[]}"#, "no shapes");
    bad_output(
        r#"{"status":"ok","shapes":[{"kind":"circle","center":[0,0],"radius":-1}]}"#,
        "radius",
    );
    bad_output(
        r#"{"status":"ok","shapes":[{"kind":"arc","center":[0,0],"radius":0,"start":0,"end":1}]}"#,
        "radius",
    );
    bad_output(
        r#"{"status":"ok","shapes":[{"kind":"line","a":[0,0],"b":[1e12,0]}]}"#,
        "too large",
    );
    bad_output(
        r#"{"status":"ok","shapes":[{"kind":"polyline","points":[[0,0]],"closed":false}]}"#,
        "at least 2",
    );
    bad_output(
        r#"{"status":"ok","shapes":[{"kind":"text","at":[0,0],"text":"x","height":0}]}"#,
        "height",
    );
    bad_output(
        r#"{"status":"ok","shapes":[{"kind":"line","a":[0,0],"b":[1,0],"attrs":{"line_weight":1e12}}]}"#,
        "line weight",
    );
}

#[test]
fn a_text_that_is_too_long_is_rejected() {
    let long = "x".repeat(kadakas_core::annot::MAX_TEXT_CHARS + 1);
    let answer = format!(
        r#"{{"status":"ok","shapes":[{{"kind":"text","at":[0,0],"text":"{long}","height":10}}]}}"#
    );
    bad_output(&answer, "characters");
}

#[test]
fn a_pointer_outside_the_memory_is_rejected() {
    let wasm = module_with(
        MANIFEST,
        "",
        Some("(i64.or (i64.shl (i64.const 524000) (i64.const 32)) (i64.const 5000))"),
        "",
    );
    let e = run(&wasm).unwrap_err();
    assert!(
        matches!(&e, PluginError::BadOutput(m) if m.contains("outside")),
        "{e}"
    );
}

#[test]
fn no_answer_is_rejected() {
    let wasm = module_with(MANIFEST, "", Some("(i64.const 0)"), "");
    let e = run(&wasm).unwrap_err();
    assert!(
        matches!(&e, PluginError::BadOutput(m) if m.contains("no answer")),
        "{e}"
    );
}

#[test]
fn an_answer_over_the_size_limit_is_rejected() {
    let answer = format!(r#"{{"status":"error","message":"{}"}}"#, "x".repeat(5000));
    let wasm = module(&answer);
    let limits = Limits {
        max_output_bytes: 1000,
        ..quick()
    };
    let p = load(&wasm, &limits).unwrap();
    let e = p
        .reset(
            &permit_for(&p),
            &Map::new(),
            &Geometry::Point {
                at: [0.0, 0.0],
                rotation: 0.0,
            },
        )
        .unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("The limit is 1000")),
        "{e}"
    );
}

#[test]
fn too_many_shapes_or_vertices_are_rejected() {
    let two = r#"{"status":"ok","shapes":[{"kind":"circle","center":[0,0],"radius":1},{"kind":"circle","center":[0,0],"radius":2}]}"#;
    let p = load(
        &module(two),
        &Limits {
            max_shapes: 1,
            ..quick()
        },
    )
    .unwrap();
    let e = p
        .reset(
            &permit_for(&p),
            &Map::new(),
            &Geometry::Point {
                at: [0.0, 0.0],
                rotation: 0.0,
            },
        )
        .unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("2 shapes")),
        "{e}"
    );
    let poly = r#"{"status":"ok","shapes":[{"kind":"polyline","points":[[0,0],[1,0],[2,0]],"closed":false}]}"#;
    let p = load(
        &module(poly),
        &Limits {
            max_vertices: 2,
            ..quick()
        },
    )
    .unwrap();
    let e = p
        .reset(
            &permit_for(&p),
            &Map::new(),
            &Geometry::Point {
                at: [0.0, 0.0],
                rotation: 0.0,
            },
        )
        .unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("vertices")),
        "{e}"
    );
}

#[test]
fn a_good_answer_gives_core_shapes_with_attributes() {
    let answer = r##"{"status":"ok","shapes":[
        {"kind":"line","a":[0,0],"b":[10,0],"attrs":{"pen":[1,2,3,255],"line_weight":0.5}},
        {"kind":"rect","min":[0,0],"max":[5,5]},
        {"kind":"arc","center":[0,0],"radius":3,"start":0,"end":1.5},
        {"kind":"text","at":[1,1],"text":"hi","height":20}]}"##;
    let shapes = run(&module(answer)).unwrap();
    assert_eq!(shapes.len(), 4);
    assert_eq!(shapes[0].attrs.line_weight, Some(0.5));
    assert!(matches!(
        shapes[3].shape,
        kadakas_core::model::Shape::Text { .. }
    ));
}

// ------------------------------------------------- time, compile limits, determinism

const BULK_FILL_LOOP: &str =
    "(loop $l (memory.fill (i32.const 0) (i32.const 1) (i32.const 131072)) (br $l)) (i64.const 0)";

fn run_bulk_fill_loop(limits: &Limits) -> PluginError {
    let wasm = module_with(MANIFEST, "", Some(BULK_FILL_LOOP), "");
    let p = load(&wasm, limits).unwrap();
    p.reset(
        &permit_for(&p),
        &Map::new(),
        &Geometry::Point {
            at: [0.0, 0.0],
            rotation: 0.0,
        },
    )
    .unwrap_err()
}

#[test]
fn a_bulk_memory_loop_is_stopped_by_the_time_limit() {
    // Older wasmtime charged one fuel unit for a whole memory.fill, so fuel alone took minutes to
    // run out. Newer wasmtime charges by size. To test the epoch deadline on its own, this test
    // gives the call so much fuel that only the clock can stop it.
    let e = run_bulk_fill_loop(&Limits {
        max_millis: 300,
        fuel: u64::MAX / 2,
        ..Limits::default()
    });
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("300 ms")),
        "{e}"
    );
}

#[test]
fn a_bulk_memory_loop_is_also_stopped_by_fuel() {
    // With the default fuel, the size-based charge of memory.fill ends the loop long before the
    // time limit.
    let e = run_bulk_fill_loop(&Limits::default());
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("fuel")),
        "{e}"
    );
}

#[test]
fn a_module_with_a_huge_function_many_functions_or_many_locals_is_not_compiled() {
    let locals = format!("(func $big (local {}))", "i32 ".repeat(6000));
    let wasm = module_with(MANIFEST, ONE_CIRCLE, None, &locals);
    let e = load(&wasm, &Limits::default()).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("local variables")),
        "{e}"
    );

    let body = "(drop (i32.const 1)) ".repeat(100_000);
    let wasm = module_with(MANIFEST, ONE_CIRCLE, None, &format!("(func $huge {body})"));
    let e = load(&wasm, &Limits::default()).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("A function") || m.contains("a function")),
        "{e}"
    );

    let wasm = module_with(MANIFEST, ONE_CIRCLE, None, &"(func) ".repeat(5001));
    let e = load(&wasm, &Limits::default()).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("functions")),
        "{e}"
    );
    // The example is far below the limits.
    load(PLANTER, &Limits::default()).unwrap();
}

#[test]
fn nan_results_are_canonical_so_every_computer_gives_the_same_bytes() {
    let body = "(i32.store (i32.const 4096) (i32.const 0x7fc00001))
        (f32.store (i32.const 4100) (f32.add (f32.load (i32.const 4096)) (f32.const 0)))
        (i64.or (i64.shl (i64.const 4100) (i64.const 32)) (i64.const 4))";
    let wasm = module_with(MANIFEST, "", Some(body), "");
    let loaded = WasmtimeRuntime::new()
        .load(&wasm, &Limits::default())
        .unwrap();
    assert_eq!(loaded.reset(b"{}").unwrap(), [0x00, 0x00, 0xc0, 0x7f]);
}

#[test]
fn messages_from_a_plugin_are_cut_and_cleaned() {
    let long = format!("{}\n\u{7}end", "x".repeat(2000));
    let c = crate::plugin::clean_message(&long);
    assert_eq!(c.chars().count(), crate::plugin::MAX_MESSAGE_CHARS + 3);
    assert!(c.ends_with("..."));
    assert_eq!(
        crate::plugin::clean_message("a\nb\tc\u{1b}[31m "),
        "a b c [31m"
    );
    let answer = r#"{"status":"error","message":"line one\nline two\u0007"}"#;
    let e = run(&module(answer)).unwrap_err();
    assert!(matches!(e, PluginError::Guest(m) if m == "line one line two"));
}

// ------------------------------------------------------------------ registry

#[test]
fn the_registry_installs_replaces_and_removes() {
    let mut reg = Registry::default();
    assert!(reg.is_empty());
    assert_eq!(reg.engine(), "wasmtime");
    assert_eq!(reg.limits(), &Limits::default());
    let id = reg.install(PLANTER).unwrap().manifest().id.clone();
    assert_eq!(reg.len(), 1);
    // The same module again changes nothing and is not compiled again.
    reg.install(PLANTER).unwrap();
    assert_eq!(reg.len(), 1);
    assert!(reg.get(&id).is_some() && reg.get("nope").is_none());
    let hash = trust::hash(PLANTER);
    assert!(reg.by_hash(&hash).is_some() && reg.by_hash("00").is_none());
    assert_eq!(reg.iter().count(), 1);

    // Another build with the same id replaces the old one.
    let v2 = format!(
        r#"{{"id":"{id}","name":"Raised planter","version":"2","abi_version":2,"kinds":["tool"],"tool":{{}}}}"#
    );
    let other = module_with(&v2, ONE_CIRCLE, None, "");
    reg.install(&other).unwrap();
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.get(&id).unwrap().manifest().version, "2");
    assert!(reg.by_hash(&hash).is_none());
    // A broken module is an error and leaves the registry as it was.
    assert!(reg.install(b"junk").is_err());
    assert_eq!(reg.len(), 1);

    assert!(reg.remove(&id) && !reg.remove(&id));
    assert!(reg.is_empty());
    reg.set_limits(Limits {
        fuel: 5,
        ..Limits::default()
    });
    assert_eq!(reg.limits().fuel, 5);
    assert!(reg.trust.is_empty());
}

#[test]
fn an_installed_module_is_not_compiled_twice() {
    // With a limit that would refuse the module, the second install still works: no compile, no scan.
    let mut reg = Registry::default();
    reg.install(PLANTER).unwrap();
    reg.set_limits(Limits {
        max_functions: 1,
        ..Limits::default()
    });
    reg.install(PLANTER).unwrap();
    assert!(matches!(
        Registry::new(
            crate::default_runtime(),
            Limits {
                max_functions: 1,
                ..Limits::default()
            }
        )
        .install(PLANTER),
        Err(PluginError::Limit(_))
    ));
}

#[path = "tool_tests.rs"]
mod tool_tests;

#[path = "persist_tests.rs"]
mod persist_tests;

#[path = "metered_tests.rs"]
mod metered_tests;

#[path = "limits_tests.rs"]
mod limits_tests;

#[path = "format_tests.rs"]
mod format_tests;

#[path = "fixture_tests.rs"]
mod fixture_tests;

#[path = "probe_tests.rs"]
mod probe_tests;

#[path = "probe_run_tests.rs"]
mod probe_run_tests;
