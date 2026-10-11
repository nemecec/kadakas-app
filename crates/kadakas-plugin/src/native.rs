//! The wasmtime backend (native targets only).
//!
//! The sandbox:
//!
//! - **No imports.** The linker is empty, so a module cannot call the host, WASI, files or the
//!   network. A module that wants an import does not load.
//! - **Fuel.** Every call gets [`Limits::fuel`]. An endless loop stops with a limit error.
//! - **Wall-clock time.** Fuel counts instructions, and wasmtime 49 also charges bulk operations
//!   such as `memory.fill` and `memory.copy` by size. Other slow calls may still cost little fuel.
//!   So a ticker thread advances the epoch of the engine every [`TICK_MILLIS`] ms, and every call
//!   has a deadline of [`Limits::max_millis`]. The fuel stays as the exact limit that does not
//!   depend on the speed of the computer.
//! - **Determinism.** NaN results are made canonical, and relaxed SIMD (whose results depend on
//!   the processor) is off, so one module gives the same shapes on every computer.
//! - **Memory.** A call may use [`Limits::memory_bytes`] of linear memory. Growing past it stops
//!   the call with a limit error. A module that declares more at the start does not load.
//! - **One instance per call.** Nothing stays in memory between calls.
//! - Only the core WebAssembly features that plug-ins need: no threads, no 64-bit memories, no
//!   multiple memories, no component model.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use wasmtime::{Config, Engine, Linker, Module, ResourceLimiter, Store, Trap};

use crate::runtime::{Limits, LoadedModule, PluginError, PluginRuntime, REQUIRED_EXPORTS};
use kadakas_plugin_abi::guest::unpack;

/// Largest request or manifest that the host reads, as a guard: the manifest is small.
const MAX_MANIFEST_BYTES: usize = 256 << 10;

/// How often the ticker advances the epoch, in milliseconds.
pub const TICK_MILLIS: u64 = 50;

/// The wasmtime engine. One thread per engine advances the epoch; it ends when the engine is dropped.
pub struct WasmtimeRuntime {
    engine: Engine,
    ticker: Arc<Ticker>,
}

/// Stops the ticker thread when the last user of the engine (the runtime or a loaded module) is gone.
struct Ticker {
    stop: Arc<AtomicBool>,
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Default for WasmtimeRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmtimeRuntime {
    pub fn new() -> Self {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        config.cranelift_nan_canonicalization(true);
        config.wasm_relaxed_simd(false);
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);
        config.max_wasm_stack(512 << 10);
        let engine = Engine::new(&config).expect("the wasmtime configuration is valid");
        let stop = Arc::new(AtomicBool::new(false));
        let (ticker, flag) = (engine.clone(), stop.clone());
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(TICK_MILLIS));
                ticker.increment_epoch();
            }
        });
        WasmtimeRuntime {
            engine,
            ticker: Arc::new(Ticker { stop }),
        }
    }
}

impl PluginRuntime for WasmtimeRuntime {
    fn name(&self) -> &'static str {
        "wasmtime"
    }

    fn load(&self, wasm: &[u8], limits: &Limits) -> Result<Box<dyn LoadedModule>, PluginError> {
        let module =
            Module::new(&self.engine, wasm).map_err(|e| PluginError::Invalid(format!("{e:#}")))?;
        if let Some(i) = module.imports().next() {
            return Err(PluginError::Invalid(format!(
                "the module imports '{}.{}'. A plug-in may import nothing: it has no files, no network and no clock",
                i.module(),
                i.name()
            )));
        }
        for name in REQUIRED_EXPORTS {
            if module.get_export(name).is_none() {
                return Err(PluginError::Invalid(format!(
                    "the module does not export '{name}'"
                )));
            }
        }
        Ok(Box::new(Loaded {
            engine: self.engine.clone(),
            _ticker: self.ticker.clone(),
            module,
            limits: *limits,
        }))
    }
}

struct Loaded {
    engine: Engine,
    _ticker: Arc<Ticker>,
    module: Module,
    limits: Limits,
}

/// The state of one call: the memory limit and whether the module went over it.
struct CallState {
    max_memory: usize,
    hit_memory_limit: bool,
}

impl ResourceLimiter for CallState {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_memory {
            self.hit_memory_limit = true;
            // An error makes the growth a trap, so the call ends at once.
            return Err(wasmtime::Error::msg("the memory limit is reached"));
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // A table of 100 000 entries is far more than a plug-in needs.
        Ok(desired <= 100_000)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        4
    }

    fn memories(&self) -> usize {
        1
    }
}

impl Loaded {
    /// Runs `f` in a new instance with fuel and the memory limit.
    fn call<T>(
        &self,
        f: impl FnOnce(&mut Store<CallState>, &wasmtime::Instance) -> wasmtime::Result<T>,
    ) -> Result<T, PluginError> {
        let mut store = Store::new(
            &self.engine,
            CallState {
                max_memory: self.limits.memory_bytes,
                hit_memory_limit: false,
            },
        );
        store.limiter(|s| s);
        // One more tick than the limit: the first tick comes at any moment after the call starts.
        store.set_epoch_deadline(self.limits.max_millis.div_ceil(TICK_MILLIS) + 1);
        store
            .set_fuel(self.limits.fuel)
            .expect("fuel is switched on in the engine");
        let linker = Linker::<CallState>::new(&self.engine);
        let result = linker
            .instantiate(&mut store, &self.module)
            .and_then(|instance| f(&mut store, &instance));
        result.map_err(|e| self.classify(&store, e))
    }

    fn classify(&self, store: &Store<CallState>, e: wasmtime::Error) -> PluginError {
        if store.data().hit_memory_limit {
            return PluginError::Limit(format!(
                "the module needs more than {} bytes of memory",
                self.limits.memory_bytes
            ));
        }
        match e.downcast_ref::<Trap>() {
            Some(Trap::OutOfFuel) => PluginError::Limit(format!(
                "the module used all of its {} fuel units (an endless loop?)",
                self.limits.fuel
            )),
            Some(Trap::Interrupt) => PluginError::Limit(format!(
                "the module ran for more than {} ms",
                self.limits.max_millis
            )),
            Some(Trap::StackOverflow) => {
                PluginError::Limit("the module overflowed its stack".into())
            }
            Some(trap) => PluginError::Trap(trap.to_string()),
            None => PluginError::Trap(format!("{e:#}")),
        }
    }
}

/// Reads `len` bytes at `ptr` from the memory of the instance.
fn read_memory(
    store: &Store<CallState>,
    memory: &wasmtime::Memory,
    packed: u64,
    max: usize,
) -> Result<Vec<u8>, PluginError> {
    if packed == 0 {
        return Err(PluginError::BadOutput("the module gave no answer".into()));
    }
    let (ptr, len) = unpack(packed);
    let (ptr, len) = (ptr as usize, len as usize);
    if len > max {
        return Err(PluginError::Limit(format!(
            "the answer has {len} bytes. The limit is {max}"
        )));
    }
    let data = memory.data(store);
    ptr.checked_add(len)
        .and_then(|end| data.get(ptr..end))
        .map(<[u8]>::to_vec)
        .ok_or_else(|| PluginError::BadOutput("the answer points outside the memory".into()))
}

impl LoadedModule for Loaded {
    fn describe(&self) -> Result<Vec<u8>, PluginError> {
        let max = MAX_MANIFEST_BYTES.min(self.limits.max_output_bytes);
        self.call(|store, instance| {
            let describe = instance.get_typed_func::<(), u64>(&mut *store, "describe")?;
            let memory = memory_of(store, instance)?;
            let packed = describe.call(&mut *store, ())?;
            Ok(read_memory(store, &memory, packed, max))
        })?
    }

    fn reset(&self, request: &[u8]) -> Result<Vec<u8>, PluginError> {
        let max = self.limits.max_output_bytes;
        self.call(|store, instance| {
            let alloc = instance.get_typed_func::<u32, u32>(&mut *store, "alloc")?;
            let reset = instance.get_typed_func::<(u32, u32), u64>(&mut *store, "reset")?;
            let memory = memory_of(store, instance)?;
            let len = u32::try_from(request.len())
                .map_err(|_| wasmtime::Error::msg("the request is too large"))?;
            let ptr = alloc.call(&mut *store, len)?;
            memory.write(&mut *store, ptr as usize, request)?;
            let packed = reset.call(&mut *store, (ptr, len))?;
            Ok(read_memory(store, &memory, packed, max))
        })?
    }
}

fn memory_of(
    store: &mut Store<CallState>,
    instance: &wasmtime::Instance,
) -> wasmtime::Result<wasmtime::Memory> {
    instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| wasmtime::Error::msg("'memory' is not a memory"))
}
