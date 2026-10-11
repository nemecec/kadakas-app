//! Tests of the fuel of each kind of call (the browser gives a pointer move less than a click).

use super::*;
use crate::PluginRuntime;
use crate::metered::{Compiler, MeteredRuntime, RawError, RawInstance, RawModule};
use crate::runtime::CallKind;

#[test]
fn each_kind_of_call_has_its_own_fuel() {
    let l = Limits {
        fuel: 2_000_000,
        fuel_describe: 100_000,
        fuel_move: 500_000,
        ..Limits::default()
    };
    assert_eq!(l.fuel_for(CallKind::Describe), 100_000);
    assert_eq!(l.fuel_for(CallKind::ToolMove), 500_000);
    assert_eq!(l.fuel_for(CallKind::Reset), 2_000_000);
    assert_eq!(l.fuel_for(CallKind::ToolStep), 2_000_000);
    assert_eq!(l.start_fuel(), 100_000);
    // A kind never gets more than the plain fuel.
    let capped = Limits {
        fuel: 10,
        ..Limits::default()
    };
    assert_eq!(capped.fuel_for(CallKind::Describe), 10);
    let b = Limits::browser();
    assert!(b.fuel_describe < b.fuel_move && b.fuel_move < b.fuel);
    assert!(b.fuel < Limits::default().fuel);
    assert_eq!(crate::runtime::default_limits(), Limits::default());
}

/// A compiler for the metered runtime that runs the rewritten module on wasmtime.
struct Plain(wasmtime::Engine);

struct PlainModule(wasmtime::Engine, wasmtime::Module);

struct PlainInstance {
    store: wasmtime::Store<()>,
    instance: wasmtime::Instance,
}

fn raw(e: wasmtime::Error) -> RawError {
    RawError::Trap(format!("{e:#}"))
}

impl Compiler for Plain {
    fn name(&self) -> &'static str {
        "plain"
    }
    fn compile(&self, wasm: &[u8]) -> Result<Box<dyn RawModule>, PluginError> {
        let m = wasmtime::Module::new(&self.0, wasm)
            .map_err(|e| PluginError::Invalid(e.to_string()))?;
        Ok(Box::new(PlainModule(self.0.clone(), m)))
    }
}

impl RawModule for PlainModule {
    fn exports(&self) -> Vec<String> {
        self.1.exports().map(|e| e.name().to_string()).collect()
    }
    fn import_count(&self) -> usize {
        0
    }
    fn instantiate(&self) -> Result<Box<dyn RawInstance>, RawError> {
        let mut store = wasmtime::Store::new(&self.0, ());
        let instance = wasmtime::Linker::new(&self.0)
            .instantiate(&mut store, &self.1)
            .map_err(raw)?;
        Ok(Box::new(PlainInstance { store, instance }))
    }
}

impl RawInstance for PlainInstance {
    fn alloc(&mut self, len: u32) -> Result<u32, RawError> {
        self.instance
            .get_typed_func::<u32, u32>(&mut self.store, "alloc")
            .map_err(raw)?
            .call(&mut self.store, len)
            .map_err(raw)
    }
    fn write(&mut self, ptr: u32, bytes: &[u8]) -> Result<(), RawError> {
        let memory = self.instance.get_memory(&mut self.store, "memory").unwrap();
        memory
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
            _ => unreachable!(),
        }
    }
    fn read(&mut self, ptr: u32, len: u32) -> Option<Vec<u8>> {
        let memory = self.instance.get_memory(&mut self.store, "memory")?;
        let end = (ptr as usize).checked_add(len as usize)?;
        memory
            .data(&self.store)
            .get(ptr as usize..end)
            .map(<[u8]>::to_vec)
    }
    fn set_fuel(&mut self, fuel: i64) -> Result<(), RawError> {
        self.instance
            .get_global(&mut self.store, "kadakas_fuel")
            .ok_or_else(|| RawError::Trap("no fuel".into()))?
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
        let memory = self.instance.get_memory(&mut self.store, "memory").unwrap();
        memory.data_size(&self.store)
    }
}

#[test]
fn a_pointer_move_gets_less_fuel_than_a_click() {
    let l = Limits {
        fuel: 2_000_000,
        fuel_describe: 100_000,
        fuel_move: 500_000,
        ..Limits::default()
    };
    let manifest = r#"{"id":"test.slow","name":"Slow","version":"1","abi_version":2,"kinds":["tool"],"tool":{}}"#;
    let answer = r#"{"status":"ok"}"#;
    // A tool that needs about 1.4M instructions: a pointer move may not, a click may.
    let wasm = wat::parse_str(format!(
        r#"(module
          (memory (export "memory") 2)
          (data (i32.const 1024) "{m}")
          (data (i32.const 2048) "{a}")
          (func (export "alloc") (param i32) (result i32) (i32.const 65536))
          (func (export "dealloc") (param i32 i32))
          (func (export "describe") (result i64)
            (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {ml})))
          (func (export "reset") (param i32 i32) (result i64) (i64.const 0))
          (func (export "tool") (param i32 i32) (result i64) (local $i i32)
            (loop $l
              (local.set $i (i32.add (local.get $i) (i32.const 1)))
              (br_if $l (i32.lt_u (local.get $i) (i32.const 200000))))
            (i64.or (i64.shl (i64.const 2048) (i64.const 32)) (i64.const {al}))))"#,
        m = manifest.replace('"', "\\\""),
        a = answer.replace('"', "\\\""),
        ml = manifest.len(),
        al = answer.len()
    ))
    .unwrap();
    let engine = wasmtime::Engine::new(&wasmtime::Config::new()).unwrap();
    let loaded = MeteredRuntime::new(Plain(engine)).load(&wasm, &l).unwrap();
    let e = loaded.tool(b"{}", true).unwrap_err();
    assert!(
        matches!(&e, PluginError::Limit(m) if m.contains("500000")),
        "{e}"
    );
    assert!(loaded.tool(b"{}", false).is_ok());
    assert!(loaded.describe().is_ok());
}
