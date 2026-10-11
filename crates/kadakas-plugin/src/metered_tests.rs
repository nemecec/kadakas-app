//! Tests of the metered engine (the engine of the browser build) on a test compiler that uses
//! wasmtime without fuel and without a deadline. Whatever stops a module here is the rewritten
//! module itself (see `crate::harden`), as it is in a browser.

use kadakas_plugin_abi::{Kind, Modifiers, ToolEvent};
use serde_json::{Map, Value, json};
use wasmtime::{Config, Engine, Instance, Linker, Memory, Module, Store, Trap};

use super::*;
use crate::metered::{Compiler, MeteredRuntime, RawError, RawInstance, RawModule};
use crate::{PluginRuntime, ToolCall};

/// A compiler on wasmtime that has no limits of its own.
struct PlainCompiler {
    engine: Engine,
}

impl PlainCompiler {
    fn new() -> Self {
        PlainCompiler {
            engine: Engine::new(&Config::new()).unwrap(),
        }
    }
}

struct PlainModule {
    engine: Engine,
    module: Module,
}

struct PlainInstance {
    store: Store<()>,
    instance: Instance,
    memory: Memory,
}

fn raw(e: wasmtime::Error) -> RawError {
    match e.downcast_ref::<Trap>() {
        Some(Trap::StackOverflow) => RawError::Stack(e.to_string()),
        _ => RawError::Trap(format!("{e:#}")),
    }
}

impl Compiler for PlainCompiler {
    fn name(&self) -> &'static str {
        "plain"
    }

    fn compile(&self, wasm: &[u8]) -> Result<Box<dyn RawModule>, PluginError> {
        let module =
            Module::new(&self.engine, wasm).map_err(|e| PluginError::Invalid(format!("{e:#}")))?;
        Ok(Box::new(PlainModule {
            engine: self.engine.clone(),
            module,
        }))
    }
}

impl RawModule for PlainModule {
    fn exports(&self) -> Vec<String> {
        self.module
            .exports()
            .map(|e| e.name().to_string())
            .collect()
    }

    fn import_count(&self) -> usize {
        self.module.imports().count()
    }

    fn instantiate(&self) -> Result<Box<dyn RawInstance>, RawError> {
        let mut store = Store::new(&self.engine, ());
        let instance = Linker::new(&self.engine)
            .instantiate(&mut store, &self.module)
            .map_err(raw)?;
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        Ok(Box::new(PlainInstance {
            store,
            instance,
            memory,
        }))
    }
}

impl RawInstance for PlainInstance {
    fn alloc(&mut self, len: u32) -> Result<u32, RawError> {
        let f = self
            .instance
            .get_typed_func::<u32, u32>(&mut self.store, "alloc")
            .map_err(raw)?;
        f.call(&mut self.store, len).map_err(raw)
    }

    fn write(&mut self, ptr: u32, bytes: &[u8]) -> Result<(), RawError> {
        self.memory
            .write(&mut self.store, ptr as usize, bytes)
            .map_err(|e| RawError::Trap(e.to_string()))
    }

    fn call(&mut self, export: &str, args: &[u32]) -> Result<u64, RawError> {
        match args {
            [] => self
                .instance
                .get_typed_func::<(), u64>(&mut self.store, export)
                .map_err(raw)?
                .call(&mut self.store, ())
                .map_err(raw),
            [a, b] => self
                .instance
                .get_typed_func::<(u32, u32), u64>(&mut self.store, export)
                .map_err(raw)?
                .call(&mut self.store, (*a, *b))
                .map_err(raw),
            _ => unreachable!("the host calls with none or two arguments"),
        }
    }

    fn read(&mut self, ptr: u32, len: u32) -> Option<Vec<u8>> {
        let data = self.memory.data(&self.store);
        let end = (ptr as usize).checked_add(len as usize)?;
        data.get(ptr as usize..end).map(<[u8]>::to_vec)
    }

    fn set_fuel(&mut self, fuel: i64) -> Result<(), RawError> {
        self.instance
            .get_global(&mut self.store, "kadakas_fuel")
            .ok_or_else(|| RawError::Trap("no fuel counter".into()))?
            .set(&mut self.store, wasmtime::Val::I64(fuel))
            .map_err(raw)
    }

    fn fuel_left(&mut self) -> Option<i64> {
        self.instance
            .get_global(&mut self.store, "kadakas_fuel")?
            .get(&mut self.store)
            .i64()
    }

    fn memory_bytes(&mut self) -> usize {
        self.memory.data_size(&self.store)
    }
}

fn runtime() -> MeteredRuntime<PlainCompiler> {
    MeteredRuntime::new(PlainCompiler::new())
}

fn mload(wasm: &[u8], limits: &Limits) -> Result<Plugin, PluginError> {
    Plugin::load(&runtime(), wasm, limits)
}

fn mrun(wasm: &[u8]) -> Result<Vec<crate::Generated>, PluginError> {
    let p = mload(wasm, &quick())?;
    p.reset(
        &permit_for(&p),
        &Map::new(),
        &Geometry::Point {
            at: [1.0, 2.0],
            rotation: 0.0,
        },
    )
}

#[test]
fn the_runtime_has_the_name_of_its_compiler() {
    assert_eq!(runtime().name(), "plain");
}

#[test]
fn the_examples_give_the_same_shapes_as_on_the_native_engine() {
    let native = Plugin::load(&WasmtimeRuntime::new(), PLANTER, &Limits::default()).unwrap();
    let metered = mload(PLANTER, &Limits::default()).unwrap();
    assert_eq!(native.manifest(), metered.manifest());
    assert_eq!(native.hash(), metered.hash());
    let given = params(json!({"wall": 100, "corners": "Cut"}));
    let a = native
        .reset(&permit_for(&native), &given, &rect(2400.0, 900.0))
        .unwrap();
    let b = metered
        .reset(&permit_for(&metered), &given, &rect(2400.0, 900.0))
        .unwrap();
    assert_eq!(a, b);
}

#[test]
fn the_example_tool_works_on_the_metered_engine() {
    let stones: &[u8] = include_bytes!("../tests/fixtures/stone_row.wasm");
    let p = mload(stones, &Limits::default()).unwrap();
    assert!(p.manifest().provides(Kind::Tool));
    let state = json!({"start": [0.0, 0.0]});
    let s = p
        .tool_step(
            &permit_for(&p),
            &ToolCall {
                event: &ToolEvent::Move { at: [3000.0, 0.0] },
                modifiers: Modifiers::default(),
                state: &state,
            },
            &Map::new(),
        )
        .unwrap();
    assert_eq!(s.preview.len(), 4);
}

#[test]
fn an_endless_loop_runs_out_of_fuel() {
    let wasm = module_with(MANIFEST, "", Some("(loop $l (br $l)) (i64.const 0)"), "");
    let e = mrun(&wasm).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("fuel")),
        "{e}"
    );
}

#[test]
fn endless_recursion_is_a_limit_error() {
    let wasm = module_with(
        MANIFEST,
        "",
        Some("(call $deep) (i64.const 0)"),
        "(func $deep (call $deep))",
    );
    let e = mrun(&wasm).unwrap_err();
    assert!(matches!(&e, PluginError::Limit(_)), "{e}");
}

#[test]
fn a_bulk_memory_loop_is_stopped_by_fuel() {
    let wasm = module_with(
        MANIFEST,
        "",
        Some(
            "(loop $l (memory.fill (i32.const 0) (i32.const 1) (i32.const 400000)) (br $l)) (i64.const 0)",
        ),
        "",
    );
    let e = mrun(&wasm).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("fuel")),
        "{e}"
    );
}

#[test]
fn a_memory_bomb_hits_the_cap() {
    // memory.grow answers -1, and this module traps when it sees that. The memory is not at the
    // cap yet, so this is a plain crash...
    let wasm = module_with(
        MANIFEST,
        "",
        Some(
            "(if (i32.eq (memory.grow (i32.const 100000)) (i32.const -1)) (then (unreachable))) (i64.const 0)",
        ),
        "",
    );
    let e = mrun(&wasm).unwrap_err();
    assert!(matches!(&e, PluginError::Trap(_)), "{e}");
    // ...but a module that grows to the cap in steps and then fails is a limit error.
    let wasm = module_with(
        MANIFEST,
        "",
        Some(
            "(loop $l (if (i32.ne (memory.grow (i32.const 1)) (i32.const -1)) (then (br $l)))) (unreachable)",
        ),
        "",
    );
    let limits = Limits {
        memory_bytes: 16 << 20,
        ..quick()
    };
    let p = mload(&wasm, &limits).unwrap();
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
        matches!(&e, PluginError::Limit(m) if m.contains("16777216")),
        "{e}"
    );
}

#[test]
fn a_module_that_declares_too_much_memory_does_not_load() {
    let wasm = module_with(MANIFEST, ONE_CIRCLE, None, "");
    let limits = Limits {
        memory_bytes: 2 << 20,
        ..Limits::default()
    };
    // The test modules ask for 8 pages (512 KiB) at the start; 2 MiB is enough.
    assert!(mload(&wasm, &limits).is_ok());
    let tight = Limits {
        memory_bytes: 256 << 10,
        ..Limits::default()
    };
    let e = mload(&wasm, &tight).unwrap_err();
    assert!(matches!(e, PluginError::Limit(m) if m.contains("memory")));
}

#[test]
fn traps_and_bad_pointers_are_reported() {
    let wasm = module_with(MANIFEST, "", Some("(unreachable)"), "");
    assert!(matches!(mrun(&wasm), Err(PluginError::Trap(_))));
    let wasm = module_with(MANIFEST, "", Some("(i64.const 0)"), "");
    assert!(matches!(mrun(&wasm), Err(PluginError::BadOutput(m)) if m.contains("no answer")));
    let wasm = module_with(
        MANIFEST,
        "",
        Some("(i64.or (i64.shl (i64.const 524000) (i64.const 32)) (i64.const 5000))"),
        "",
    );
    assert!(matches!(mrun(&wasm), Err(PluginError::BadOutput(m)) if m.contains("outside")));
    let wasm = module_with(
        MANIFEST,
        "",
        Some("(i64.or (i64.shl (i64.const 1000) (i64.const 32)) (i64.const 3000000))"),
        "",
    );
    assert!(matches!(mrun(&wasm), Err(PluginError::Limit(_))));
}

#[test]
fn the_request_reaches_the_module_as_json() {
    let echo = module_with(
        MANIFEST,
        "",
        Some(
            "(i64.or (i64.shl (i64.extend_i32_u (local.get 0)) (i64.const 32)) (i64.extend_i32_u (local.get 1)))",
        ),
        "",
    );
    let loaded = runtime().load(&echo, &Limits::default()).unwrap();
    let req = br#"{"hello":"world"}"#;
    assert_eq!(loaded.reset(req).unwrap(), req);
}

#[test]
fn modules_with_imports_or_missing_exports_do_not_load() {
    let imports = wat::parse_str(
        r#"(module
          (import "env" "f" (func))
          (memory (export "memory") 1)
          (func (export "alloc") (param i32) (result i32) (i32.const 0))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64) (i64.const 0))
          (func (export "reset") (param i32 i32) (result i64) (i64.const 0))
          (func (export "tool") (param i32 i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    assert!(matches!(
        mload(&imports, &Limits::default()),
        Err(PluginError::Invalid(m)) if m.contains("imports")
    ));
    let missing = wat::parse_str(r#"(module (memory (export "memory") 1))"#).unwrap();
    assert!(matches!(
        mload(&missing, &Limits::default()),
        Err(PluginError::Invalid(m)) if m.contains("does not export")
    ));
    assert!(matches!(
        mload(b"\0asm\x01\0\0\0junk", &Limits::default()),
        Err(PluginError::Invalid(_))
    ));
}

#[test]
fn a_start_function_that_never_ends_cannot_hang_the_load() {
    let wasm = wat::parse_str(
        r#"(module
          (memory (export "memory") 1)
          (func $init (loop $l (br $l)))
          (start $init)
          (func (export "alloc") (param i32) (result i32) (i32.const 0))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64) (i64.const 0))
          (func (export "reset") (param i32 i32) (result i64) (i64.const 0))
          (func (export "tool") (param i32 i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    let e = mload(&wasm, &quick()).unwrap_err();
    // The start function runs when the instance is made; the fuel stops it.
    assert!(
        matches!(e, PluginError::Trap(_) | PluginError::Limit(_)),
        "{e}"
    );
}

#[test]
fn fuel_is_per_call() {
    let p = mload(PLANTER, &Limits::default()).unwrap();
    for _ in 0..3 {
        p.reset(&permit_for(&p), &Map::new(), &rect(2000.0, 800.0))
            .unwrap();
    }
    let starved = Limits {
        fuel: 50,
        ..Limits::default()
    };
    assert!(mload(PLANTER, &starved).is_err());
    let _: Value = json!(null);
}
