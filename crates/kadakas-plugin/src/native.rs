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
//!
//! **One engine per process, and a cache of compiled modules.** Compiling a module with cranelift
//! takes far longer than a call. Every runtime of the process shares one engine (the configuration
//! above is fixed), so a module that one session compiled serves the others: the last
//! [`MODULE_CACHE_ENTRIES`] modules stay compiled, found by the SHA-256 of their bytes. A module
//! that does not compile is not kept. The checks of a module (no imports, the exports) run at every
//! load, also for a module from the cache.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use sha2::{Digest, Sha256};

use wasmtime::{Config, Engine, Linker, Module, ResourceLimiter, Store, Trap};

use crate::runtime::{Limits, LoadedModule, PluginError, PluginRuntime, REQUIRED_EXPORTS};
use kadakas_plugin_abi::guest::unpack;

/// Largest request or manifest that the host reads, as a guard: the manifest is small.
const MAX_MANIFEST_BYTES: usize = 256 << 10;

/// How often the ticker advances the epoch, in milliseconds.
pub const TICK_MILLIS: u64 = 50;

/// How many compiled modules the process keeps (see the module text). A compiled plug-in is a few
/// MiB at most, so the cache stays small.
pub const MODULE_CACHE_ENTRIES: usize = 16;

/// The last compiled values, found by a key, with at most `cap` entries: the oldest use goes
/// first.
struct Lru<T> {
    cap: usize,
    entries: VecDeque<([u8; 32], T)>,
}

impl<T: Clone> Lru<T> {
    fn new(cap: usize) -> Self {
        Lru {
            cap,
            entries: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &[u8; 32]) -> Option<T> {
        let at = self.entries.iter().position(|(k, _)| k == key)?;
        let e = self.entries.remove(at)?;
        let v = e.1.clone();
        self.entries.push_back(e);
        Some(v)
    }

    fn insert(&mut self, key: [u8; 32], value: T) {
        self.entries.retain(|(k, _)| *k != key);
        self.entries.push_back((key, value));
        while self.entries.len() > self.cap {
            self.entries.pop_front();
        }
    }
}

/// The engine of the process, the ticker that runs while a runtime or a loaded module uses it, and
/// the compiled modules.
struct Shared {
    engine: Engine,
    ticker: Weak<Ticker>,
    /// Keyed by the SHA-256 of the module bytes alone: the configuration of the engine is fixed
    /// (`WasmtimeRuntime::engine`) and there is one engine, so the bytes decide the code. A second
    /// configuration would need its own cache or a key with the configuration in it.
    modules: Lru<Module>,
    /// How many modules this engine compiled (for the tests).
    compiled: usize,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

fn shared() -> std::sync::MutexGuard<'static, Option<Shared>> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

/// The wasmtime engine. One thread advances the epoch of the engine of the process while a runtime
/// or a loaded module uses it.
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
        let mut guard = shared();
        let sh = guard.get_or_insert_with(|| Shared {
            engine: Self::engine(),
            ticker: Weak::new(),
            modules: Lru::new(MODULE_CACHE_ENTRIES),
            compiled: 0,
        });
        let engine = sh.engine.clone();
        // One ticker for the engine: a second one would make every deadline shorter.
        let ticker = sh.ticker.upgrade().unwrap_or_else(|| {
            let t = Arc::new(Self::start_ticker(&engine));
            sh.ticker = Arc::downgrade(&t);
            t
        });
        WasmtimeRuntime { engine, ticker }
    }

    fn engine() -> Engine {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        config.cranelift_nan_canonicalization(true);
        config.wasm_relaxed_simd(false);
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);
        config.max_wasm_stack(512 << 10);
        Engine::new(&config).expect("the wasmtime configuration is valid")
    }

    /// A thread that advances the epoch until the returned ticker is dropped. It looks at the stop
    /// flag after each wait, before it advances, so a stopped ticker ticks at most once more (when
    /// the flag is set between the look and the tick). A new ticker may start for the same engine
    /// at once, so a deadline can end at most one tick early; every deadline has one tick to spare
    /// (`Loaded::call`).
    fn start_ticker(engine: &Engine) -> Ticker {
        let stop = Arc::new(AtomicBool::new(false));
        let (ticker, flag) = (engine.clone(), stop.clone());
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(TICK_MILLIS));
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                ticker.increment_epoch();
            }
        });
        Ticker { stop }
    }

    /// The compiled module for these bytes: from the cache of the process, or compiled now.
    fn compile(&self, wasm: &[u8]) -> Result<Module, PluginError> {
        compile_in(&SHARED, &self.engine, wasm)
    }
}

/// The compiled module for these bytes from the cache in `shared`, or compiled now and kept there.
/// The cache serves only its own engine.
fn compile_in(
    shared: &Mutex<Option<Shared>>,
    engine: &Engine,
    wasm: &[u8],
) -> Result<Module, PluginError> {
    let lock = || shared.lock().unwrap_or_else(|e| e.into_inner());
    let key: [u8; 32] = Sha256::digest(wasm).into();
    let cached = lock()
        .as_mut()
        .filter(|sh| Engine::same(&sh.engine, engine))
        .and_then(|sh| sh.modules.get(&key));
    if let Some(m) = cached {
        return Ok(m);
    }
    // Compile without the lock: other sessions go on meanwhile.
    let module = Module::new(engine, wasm).map_err(|e| PluginError::Invalid(format!("{e:#}")))?;
    if let Some(sh) = lock()
        .as_mut()
        .filter(|sh| Engine::same(&sh.engine, engine))
    {
        sh.compiled += 1;
        sh.modules.insert(key, module.clone());
    }
    Ok(module)
}

impl PluginRuntime for WasmtimeRuntime {
    fn name(&self) -> &'static str {
        "wasmtime"
    }

    fn load(&self, wasm: &[u8], limits: &Limits) -> Result<Box<dyn LoadedModule>, PluginError> {
        let module = self.compile(wasm)?;
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
        self.call_with_request("reset", request)
    }

    fn tool(&self, request: &[u8], _light: bool) -> Result<Vec<u8>, PluginError> {
        self.call_with_request("tool", request)
    }
}

impl Loaded {
    /// Calls the export `name` (`reset` or `tool`) with a request and reads the answer.
    fn call_with_request(&self, name: &str, request: &[u8]) -> Result<Vec<u8>, PluginError> {
        let max = self.limits.max_output_bytes;
        self.call(|store, instance| {
            let alloc = instance.get_typed_func::<u32, u32>(&mut *store, "alloc")?;
            let reset = instance.get_typed_func::<(u32, u32), u64>(&mut *store, name)?;
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

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn the_cache_keeps_the_last_used_entries_up_to_its_bound() {
        let mut lru = Lru::new(2);
        let k = |n: u8| [n; 32];
        lru.insert(k(1), 10);
        lru.insert(k(2), 20);
        assert_eq!(lru.get(&k(1)), Some(10), "a hit makes the entry the newest");
        lru.insert(k(3), 30);
        assert_eq!(lru.get(&k(2)), None, "the oldest use went");
        assert_eq!((lru.get(&k(1)), lru.get(&k(3))), (Some(10), Some(30)));
        lru.insert(k(3), 31);
        assert_eq!(lru.entries.len(), 2, "a key is kept once");
        assert_eq!(lru.get(&k(3)), Some(31));
    }

    #[test]
    fn runtimes_share_the_engine() {
        let a = WasmtimeRuntime::new();
        let b = WasmtimeRuntime::new();
        assert!(Engine::same(&a.engine, &b.engine));
        let wasm = include_bytes!("../tests/fixtures/raised_planter.wasm");
        let limits = Limits::default();
        a.load(wasm, &limits).unwrap();
        b.load(wasm, &limits).unwrap();
    }

    #[test]
    fn a_compiled_module_is_kept_and_a_bad_one_is_not() {
        // A cache of its own, so the other tests of the process do not change what it holds.
        let engine = WasmtimeRuntime::engine();
        let cache = Mutex::new(Some(Shared {
            engine: engine.clone(),
            ticker: Weak::new(),
            modules: Lru::new(MODULE_CACHE_ENTRIES),
            compiled: 0,
        }));
        let compiled = || cache.lock().unwrap().as_ref().unwrap().compiled;
        let wasm = include_bytes!("../tests/fixtures/raised_planter.wasm");
        compile_in(&cache, &engine, wasm).unwrap();
        compile_in(&cache, &engine, wasm).unwrap();
        assert_eq!(compiled(), 1, "the second load comes from the cache");
        // A module that does not compile is refused every time and not kept.
        for _ in 0..2 {
            assert!(matches!(
                compile_in(&cache, &engine, b"not a module"),
                Err(PluginError::Invalid(_))
            ));
        }
        assert_eq!(compiled(), 1);
        assert_eq!(
            cache
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .modules
                .entries
                .len(),
            1
        );
        // Another engine does not use this cache.
        let other = WasmtimeRuntime::engine();
        compile_in(&cache, &other, wasm).unwrap();
        assert_eq!(compiled(), 1);
    }

    #[test]
    fn a_new_ticker_starts_when_the_old_one_stopped() {
        let first = WasmtimeRuntime::new();
        let engine = first.engine.clone();
        drop(first);
        // Another test may hold a runtime: then its ticker goes on. Either way, the new runtime
        // has a running ticker for the same engine, and the epoch moves.
        let second = WasmtimeRuntime::new();
        assert!(Engine::same(&second.engine, &engine));
        assert!(!second.ticker.stop.load(Ordering::Relaxed));
        // (module (func (export "spin") (loop br 0))): without a ticker the endless loop would
        // never stop (the fuel is endless); the epoch ends it. It runs in a thread, so a broken
        // ticker fails the test instead of hanging it.
        let spin_wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic, version
            0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type () -> ()
            0x03, 0x02, 0x01, 0x00, // one function
            0x07, 0x08, 0x01, 0x04, b's', b'p', b'i', b'n', 0x00, 0x00, // export "spin"
            0x0a, 0x09, 0x01, 0x07, 0x00, 0x03, 0x40, 0x0c, 0x00, 0x0b, 0x0b, // loop br 0
        ];
        let module = Module::new(&second.engine, spin_wasm).unwrap();
        let engine = second.engine.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut store = Store::new(&engine, ());
            store.set_fuel(u64::MAX).unwrap();
            store.set_epoch_deadline(1);
            let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
            let spin = instance
                .get_typed_func::<(), ()>(&mut store, "spin")
                .unwrap();
            let _ = tx.send(spin.call(&mut store, ()).is_err());
        });
        let stopped = rx.recv_timeout(Duration::from_secs(10));
        assert_eq!(stopped, Ok(true), "the epoch must end the endless loop");
        drop(second);
    }
}
