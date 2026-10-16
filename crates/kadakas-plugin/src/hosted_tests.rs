//! Tests of hosted modules (the script engine's kind of module): a module that calls the host
//! through `kadakas.call` and `kadakas.read`. They run on the native engine and on the metered
//! engine of the browser (on a plain compiler), and both must agree.

use crate::metered::MeteredRuntime;
use crate::runtime::{HostReply, HostedModule, Limits, PluginError, PluginRuntime};
use crate::testkit::PlainCompiler;
use crate::{native::WasmtimeRuntime, runtime::HOST_MODULE};

/// A module that sends its request to the host, reads the reply into memory at 8192 and returns it
/// as its answer. `extra` is the text of more functions or a different `run`.
fn echo_module(run_body: &str, extra: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
          (import "{HOST_MODULE}" "call" (func $call (param i32 i32) (result i32)))
          (import "{HOST_MODULE}" "read" (func $read (param i32)))
          (memory (export "memory") 2)
          (global $heap (mut i32) (i32.const 4096))
          (func (export "alloc") (param $n i32) (result i32)
            (local $p i32)
            (local.set $p (global.get $heap))
            (global.set $heap (i32.add (global.get $heap) (local.get $n)))
            (local.get $p))
          (func (export "dealloc") (param i32 i32))
          {extra}
          (func (export "run") (param $ptr i32) (param $len i32) (result i64)
            (local $n i32)
            {run_body}))"#
    ))
    .unwrap()
}

const ECHO: &str = r#"
    (local.set $n (call $call (local.get $ptr) (local.get $len)))
    (call $read (i32.const 8192))
    (i64.or (i64.shl (i64.const 8192) (i64.const 32)) (i64.extend_i32_u (local.get $n)))"#;

fn runtimes() -> Vec<(&'static str, Box<dyn PluginRuntime>)> {
    vec![
        ("native", Box::new(WasmtimeRuntime::new())),
        (
            "metered",
            Box::new(MeteredRuntime::new(PlainCompiler::new())),
        ),
    ]
}

fn small() -> Limits {
    Limits {
        fuel: 5_000_000,
        memory_bytes: 8 << 20,
        max_millis: 2_000,
        max_output_bytes: 64 << 10,
        ..Limits::default()
    }
}

fn load(rt: &dyn PluginRuntime, wasm: &[u8]) -> Result<Box<dyn HostedModule>, PluginError> {
    rt.load_hosted(wasm, &small())
}

#[test]
fn the_host_serves_the_calls_of_a_module_on_both_engines() {
    for (name, rt) in runtimes() {
        let m = load(&*rt, &echo_module(ECHO, "")).unwrap();
        let mut seen = Vec::new();
        let mut host = |req: &[u8]| {
            seen.push(req.to_vec());
            let mut reply = req.to_vec();
            reply.reverse();
            HostReply::Reply(reply)
        };
        let out = m.run(b"hello, host", &mut host, &|| None).unwrap();
        assert_eq!(out, b"tsoh ,olleh", "{name}");
        // A second run is a new instance: nothing stays.
        assert_eq!(m.run(b"ab", &mut host, &|| None).unwrap(), b"ba", "{name}");
        drop(host);
        assert_eq!(seen, [b"hello, host".to_vec(), b"ab".to_vec()], "{name}");
    }
}

#[test]
fn the_host_can_stop_a_run_with_its_own_error() {
    for (name, rt) in runtimes() {
        let m = load(&*rt, &echo_module(ECHO, "")).unwrap();
        let mut host = |_: &[u8]| HostReply::Abort(PluginError::Limit("the host said stop".into()));
        let e = m.run(b"x", &mut host, &|| None).unwrap_err();
        assert_eq!(e, PluginError::Limit("the host said stop".into()), "{name}");
    }
}

#[test]
fn an_endless_loop_and_a_memory_bomb_are_limits_on_both_engines() {
    let spin = echo_module("(loop $l (br $l)) (i64.const 0)", "");
    let grow = echo_module(
        // Ask for 5000 pages (320 MiB): wasmtime stops the growth at its cap of 8 MiB, and the
        // rewritten module (whose cap is 256 MiB for any limits, see HOSTED_MEMORY_BYTES) returns
        // -1. The module then writes past its memory and traps.
        "(drop (memory.grow (i32.const 5000))) (i32.store (i32.const 9000000) (i32.const 1)) (i64.const 0)",
        "",
    );
    for (name, rt) in runtimes() {
        let mut host = |_: &[u8]| HostReply::Reply(Vec::new());
        let m = load(&*rt, &spin).unwrap();
        let e = m.run(b"x", &mut host, &|| None).unwrap_err();
        assert!(
            matches!(&e, PluginError::Limit(m) if m.contains("fuel")),
            "{name}: {e:?}"
        );
        let m = load(&*rt, &grow).unwrap();
        let e = m.run(b"x", &mut host, &|| None).unwrap_err();
        assert!(
            matches!(e, PluginError::Limit(_) | PluginError::Trap(_)),
            "{name}: {e:?}"
        );
    }
}

#[test]
fn a_bad_answer_is_refused() {
    let none = echo_module("(i64.const 0)", "");
    let outside = echo_module("(i64.const 0x7fffffff00000010)", "");
    let long = echo_module("(i64.const 0x0000100000100000)", "");
    for (name, rt) in runtimes() {
        let mut host = |_: &[u8]| HostReply::Reply(Vec::new());
        let e = load(&*rt, &none)
            .unwrap()
            .run(b"x", &mut host, &|| None)
            .unwrap_err();
        assert!(matches!(e, PluginError::BadOutput(_)), "{name}: {e:?}");
        let e = load(&*rt, &outside)
            .unwrap()
            .run(b"x", &mut host, &|| None)
            .unwrap_err();
        assert!(matches!(e, PluginError::BadOutput(_)), "{name}: {e:?}");
        let e = load(&*rt, &long)
            .unwrap()
            .run(b"x", &mut host, &|| None)
            .unwrap_err();
        assert!(matches!(e, PluginError::Limit(_)), "{name}: {e:?}");
    }
}

#[test]
fn a_hosted_module_may_import_only_the_two_host_functions() {
    let wasi = wat::parse_str(
        r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) (i32.const 0))
            (func (export "dealloc") (param i32 i32))
            (func (export "run") (param i32 i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    let wrong_kind = wat::parse_str(
        r#"(module (import "kadakas" "call" (memory 1))
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) (i32.const 0))
            (func (export "dealloc") (param i32 i32))
            (func (export "run") (param i32 i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    let unknown_name = wat::parse_str(
        r#"(module (import "kadakas" "clock" (func (result i64)))
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) (i32.const 0))
            (func (export "dealloc") (param i32 i32))
            (func (export "run") (param i32 i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    let no_run = wat::parse_str(
        r#"(module (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) (i32.const 0))
            (func (export "dealloc") (param i32 i32)))"#,
    )
    .unwrap();
    for (name, rt) in runtimes() {
        for (what, wasm) in [
            ("wasi", &wasi),
            ("wrong kind", &wrong_kind),
            ("unknown name", &unknown_name),
            ("no run", &no_run),
        ] {
            let e = load(&*rt, wasm)
                .err()
                .unwrap_or_else(|| panic!("{name}: {what} loaded"));
            assert!(
                matches!(e, PluginError::Invalid(_)),
                "{name}: {what}: {e:?}"
            );
        }
        // Not a module at all.
        assert!(load(&*rt, b"not wasm").is_err(), "{name}");
    }
}

#[test]
fn a_plain_plugin_module_is_not_a_hosted_module_and_the_other_way_round() {
    let planter: &[u8] = include_bytes!("../tests/fixtures/raised_planter.wasm");
    for (name, rt) in runtimes() {
        // A plug-in has no `run`.
        assert!(load(&*rt, planter).is_err(), "{name}");
        // A hosted module is not a plug-in: it imports.
        assert!(rt.load(&echo_module(ECHO, ""), &small()).is_err(), "{name}");
    }
}

#[test]
fn an_engine_without_hosted_modules_says_so() {
    struct Bare;
    impl PluginRuntime for Bare {
        fn name(&self) -> &'static str {
            "bare"
        }
        fn load(
            &self,
            _: &[u8],
            _: &Limits,
        ) -> Result<Box<dyn crate::runtime::LoadedModule>, PluginError> {
            unreachable!()
        }
    }
    let e = Bare.load_hosted(b"", &small()).err().unwrap();
    assert!(matches!(e, PluginError::Unsupported(_)));
}

#[test]
fn the_deadline_starts_again_after_a_call_into_the_host() {
    // The host is slow (60 ms per call) and the module calls it 30 times: far more than the
    // deadline of 500 ms in all, but each stretch of the module is short.
    let looping = echo_module(
        r#"(local $i i32)
           (loop $again
             (drop (call $call (local.get $ptr) (local.get $len)))
             (local.set $i (i32.add (local.get $i) (i32.const 1)))
             (br_if $again (i32.lt_u (local.get $i) (i32.const 30))))
           (call $read (i32.const 8192))
           (i64.const 0x2000000000000)"#,
        "",
    );
    let rt = WasmtimeRuntime::new();
    let limits = Limits {
        max_millis: 500,
        ..small()
    };
    let m = rt.load_hosted(&looping, &limits).unwrap();
    let mut calls = 0;
    let mut host = |_: &[u8]| {
        calls += 1;
        std::thread::sleep(std::time::Duration::from_millis(60));
        HostReply::Reply(Vec::new())
    };
    // The answer has length 0 at address 8192: the module returns 0x2000_0000_0000 = ptr 0x2000, len 0.
    let out = m.run(b"x", &mut host, &|| None).unwrap();
    assert!(out.is_empty());
    assert_eq!(calls, 30);
}

#[test]
fn the_watcher_stops_a_run_at_a_call_and_while_the_module_runs_alone() {
    for (name, rt) in runtimes() {
        // At a call: the watcher says stop before the host is asked.
        let m = load(&*rt, &echo_module(ECHO, "")).unwrap();
        let mut asked = false;
        let mut host = |_: &[u8]| {
            asked = true;
            HostReply::Reply(Vec::new())
        };
        let stop = || Some(PluginError::Limit("stopped by the person".into()));
        let e = m.run(b"x", &mut host, &stop).unwrap_err();
        assert_eq!(
            e,
            PluginError::Limit("stopped by the person".into()),
            "{name}"
        );
        assert!(!asked, "{name}: the host was not asked");
    }
    // While the module runs alone (native only: the clock of the engine ticks): an endless loop
    // with a lot of fuel is stopped by the watcher, with its reason.
    let spin = echo_module("(loop $l (br $l)) (i64.const 0)", "");
    let rt = WasmtimeRuntime::new();
    let limits = Limits {
        fuel: u64::MAX / 2,
        max_millis: 60_000,
        ..small()
    };
    let m = rt.load_hosted(&spin, &limits).unwrap();
    let started = std::time::Instant::now();
    let watch = move || {
        (started.elapsed() > std::time::Duration::from_millis(200))
            .then(|| PluginError::Limit("the whole run took too long".into()))
    };
    let mut host = |_: &[u8]| HostReply::Reply(Vec::new());
    let e = m.run(b"x", &mut host, &watch).unwrap_err();
    assert_eq!(e, PluginError::Limit("the whole run took too long".into()));
}

#[test]
fn a_module_that_cannot_allocate_is_out_of_memory() {
    // An allocator that always answers 0.
    let nomem = wat::parse_str(format!(
        r#"(module
          (import "{HOST_MODULE}" "call" (func $call (param i32 i32) (result i32)))
          (import "{HOST_MODULE}" "read" (func $read (param i32)))
          (memory (export "memory") 2)
          (func (export "alloc") (param i32) (result i32) (i32.const 0))
          (func (export "dealloc") (param i32 i32))
          (func (export "run") (param i32 i32) (result i64) (i64.const 0)))"#
    ))
    .unwrap();
    for (name, rt) in runtimes() {
        let mut host = |_: &[u8]| HostReply::Reply(Vec::new());
        let e = load(&*rt, &nomem)
            .unwrap()
            .run(b"x", &mut host, &|| None)
            .unwrap_err();
        assert!(
            matches!(e, PluginError::Limit(_) | PluginError::Trap(_)),
            "{name}: {e:?}"
        );
        assert!(e.to_string().contains("out of memory"), "{name}: {e}");
    }
}
