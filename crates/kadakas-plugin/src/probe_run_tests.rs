//! Probe modules that are valid but try to run long (`f_copy`, `g_tgrow`, `h_start`), and a test
//! that a module which fails to activate is compiled and run once, not again for every look.
//! The modules run through `harden` on a plain wasmtime engine with no limits of its own, so
//! whatever stops them is the rewrite itself, as in a browser.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use wasmtime::{Config, Engine, Linker, Module, Store};

use super::*;
use crate::harden::{HardenLimits, harden};
use crate::store::{MemoryBackend, StoreBackend};

const LIMITS: HardenLimits = HardenLimits {
    fuel: 100_000,
    memory_bytes: 64 << 20,
    host_imports: false,
};

fn probe(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/probes")
        .join(format!("{name}.wat"));
    wat::parse_file(path).unwrap()
}

fn instantiate(hard: &[u8]) -> wasmtime::Result<(Store<()>, wasmtime::Instance)> {
    let engine = Engine::new(&Config::new())?;
    let module = Module::new(&engine, hard)?;
    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine).instantiate(&mut store, &module)?;
    Ok((store, instance))
}

fn fuel(store: &mut Store<()>, instance: &wasmtime::Instance) -> i64 {
    instance
        .get_global(&mut *store, "kadakas_fuel")
        .unwrap()
        .get(&mut *store)
        .i64()
        .unwrap()
}

#[test]
fn probe_f_a_loop_of_big_memory_copies_runs_out_of_fuel() {
    // 64 MiB per copy is 4M units of fuel at one unit per 16 bytes: the first copy is more than
    // all the fuel, and the loop stops at its next start.
    let hard = harden(&probe("f_copy"), &LIMITS).unwrap();
    let (mut store, instance) = instantiate(&hard).unwrap();
    let run = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .unwrap();
    assert!(run.call(&mut store, ()).is_err());
    assert!(fuel(&mut store, &instance) < 0);
}

#[test]
fn probe_g_table_growth_stops_at_the_cap_and_the_loop_at_the_fuel() {
    let hard = harden(&probe("g_tgrow"), &LIMITS).unwrap();
    let (mut store, instance) = instantiate(&hard).unwrap();
    let run = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .unwrap();
    assert!(run.call(&mut store, ()).is_err());
    assert!(fuel(&mut store, &instance) < 0);
    // With enough fuel the table still cannot grow past the cap: it ends with exactly the cap.
    let rich = harden(
        &probe("g_tgrow"),
        &HardenLimits {
            fuel: i64::MAX as u64,
            ..LIMITS
        },
    )
    .unwrap();
    let engine = Engine::new(&Config::new()).unwrap();
    let module = Module::new(&engine, &rich).unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine)
        .instantiate(&mut store, &module)
        .unwrap();
    // Set the fuel so that the 1 000 000 iterations of the probe finish.
    instance
        .get_global(&mut store, "kadakas_fuel")
        .unwrap()
        .set(&mut store, wasmtime::Val::I64(i64::MAX))
        .unwrap();
    let run = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .unwrap();
    assert_eq!(
        run.call(&mut store, ()).unwrap(),
        crate::harden::MAX_TABLE_ELEMENTS as i32
    );
}

#[test]
fn probe_h_a_start_function_that_loops_traps_at_instantiation() {
    let hard = harden(&probe("h_start"), &LIMITS).unwrap();
    assert!(instantiate(&hard).is_err());
}

/// A runtime that counts how many modules it was asked to load.
struct Counting {
    loads: Arc<AtomicUsize>,
    inner: crate::native::WasmtimeRuntime,
}

impl PluginRuntime for Counting {
    fn name(&self) -> &'static str {
        "counting"
    }
    fn load(
        &self,
        wasm: &[u8],
        limits: &Limits,
    ) -> Result<Box<dyn crate::LoadedModule>, PluginError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        self.inner.load(wasm, limits)
    }
}

#[test]
fn a_module_that_fails_to_activate_is_compiled_and_run_once_not_again_for_each_look() {
    let mem = Arc::new(MemoryBackend::new());
    let mut first = Registry::default();
    first.attach(mem.clone(), 2 << 20, 2 << 20);
    first.install(PLANTER).unwrap();
    let hash = trust::hash(PLANTER);
    // The record is edited, so the module no longer matches it.
    let key = format!("{hash}.manifest.json");
    let record = String::from_utf8(mem.read(&key, 1 << 20).unwrap().unwrap())
        .unwrap()
        .replace("Raised planter", "Something else");
    mem.write(&key, record.as_bytes()).unwrap();

    let loads = Arc::new(AtomicUsize::new(0));
    let mut next = Registry::new(
        Box::new(Counting {
            loads: loads.clone(),
            inner: crate::native::WasmtimeRuntime::new(),
        }),
        Limits::default(),
    );
    next.attach(mem, 2 << 20, 2 << 20);
    let p = next.get("org.kadakas.raised-planter").unwrap();
    assert_eq!(
        loads.load(Ordering::SeqCst),
        0,
        "attach must compile nothing"
    );
    // The review dialog looks at the module in every frame.
    let first_error = p.activate().unwrap_err();
    for _ in 0..5 {
        assert_eq!(p.activate().unwrap_err(), first_error);
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1, "the failure is kept");
    // A trusted call gets the same error and does not compile again either.
    let permit = ModuleTrust::in_memory().permit(&hash, true).unwrap();
    assert_eq!(
        p.reset(&permit, &Map::new(), &rect(2000.0, 800.0))
            .unwrap_err(),
        first_error
    );
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert!(!p.is_active());
}

#[test]
fn a_module_that_activates_is_compiled_once() {
    let mem = Arc::new(MemoryBackend::new());
    let mut first = Registry::default();
    first.attach(mem.clone(), 2 << 20, 2 << 20);
    first.install(PLANTER).unwrap();
    let loads = Arc::new(AtomicUsize::new(0));
    let mut next = Registry::new(
        Box::new(Counting {
            loads: loads.clone(),
            inner: crate::native::WasmtimeRuntime::new(),
        }),
        Limits::default(),
    );
    next.attach(mem, 2 << 20, 2 << 20);
    let p = next.get("org.kadakas.raised-planter").unwrap();
    for _ in 0..4 {
        p.activate().unwrap();
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert!(p.is_active());
}
