//! Hosted modules on wasmtime: a module that calls back into the host (see
//! [`crate::runtime::HostedModule`]).
//!
//! The linker knows exactly two functions, `kadakas.call` and `kadakas.read`. A module that
//! imports anything else does not load. Fuel, the memory cap and the epoch deadline work as for
//! plug-ins. The deadline starts again after each call into the host, so the time that the host
//! spends in a call does not count against the module (the fuel bounds the module in all, and the
//! host can stop the run with `HostReply::Abort`).

use std::sync::Arc;

use wasmtime::{Caller, Engine, Extern, Linker, Module, Store, Trap};

use super::{TICK_MILLIS, Ticker, WasmtimeRuntime};
use crate::runtime::{
    HOST_IMPORTS, HOST_MODULE, HOSTED_EXPORTS, HostCall, HostReply, HostedModule, Limits,
    PluginError,
};
use kadakas_plugin_abi::guest::unpack;

/// The host function of a run, with its lifetime erased. It is called only while `run` is on the
/// stack (see `Hosted::run`).
struct HostPtr(*mut (dyn FnMut(&[u8]) -> HostReply + 'static));

// SAFETY: wasmtime asks for `Send` because a store may move to another thread. This one does not:
// it lives on the stack of `Hosted::run` and is dropped there, and the pointer is used only on
// that thread, while `run` is on the stack.
unsafe impl Send for HostPtr {}

/// The state of one run.
struct HostState {
    max_memory: usize,
    hit_memory_limit: bool,
    host: HostPtr,
    /// The reply of the last call, until the module reads it.
    pending: Vec<u8>,
    /// Why the host stopped the run.
    aborted: Option<PluginError>,
    /// Ticks of the deadline of one stretch of the module.
    ticks: u64,
}

impl wasmtime::ResourceLimiter for HostState {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_memory {
            self.hit_memory_limit = true;
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

pub(super) struct Hosted {
    engine: Engine,
    _ticker: Arc<Ticker>,
    module: Module,
    limits: Limits,
}

impl WasmtimeRuntime {
    pub(super) fn load_hosted_module(
        &self,
        wasm: &[u8],
        limits: &Limits,
    ) -> Result<Box<dyn HostedModule>, PluginError> {
        let module = self.compile(wasm)?;
        for i in module.imports() {
            let known = i.module() == HOST_MODULE && HOST_IMPORTS.contains(&i.name());
            if !known {
                return Err(PluginError::Invalid(format!(
                    "the module imports '{}.{}'. A script module may import only {HOST_MODULE}.call and {HOST_MODULE}.read: it has no files, no network and no clock",
                    i.module(),
                    i.name()
                )));
            }
        }
        for name in HOSTED_EXPORTS {
            if module.get_export(name).is_none() {
                return Err(PluginError::Invalid(format!(
                    "the module does not export '{name}'"
                )));
            }
        }
        Ok(Box::new(Hosted {
            engine: self.engine.clone(),
            _ticker: self.ticker.clone(),
            module,
            limits: *limits,
        }))
    }
}

fn memory_of(caller: &mut Caller<'_, HostState>) -> wasmtime::Result<wasmtime::Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Ok(m),
        _ => Err(wasmtime::Error::msg("'memory' is not a memory")),
    }
}

fn linker(engine: &Engine) -> Linker<HostState> {
    let mut linker = Linker::new(engine);
    linker
        .func_wrap(
            HOST_MODULE,
            "call",
            |mut caller: Caller<'_, HostState>, ptr: u32, len: u32| -> wasmtime::Result<i32> {
                let memory = memory_of(&mut caller)?;
                let request = {
                    let data = memory.data(&caller);
                    let (ptr, len) = (ptr as usize, len as usize);
                    ptr.checked_add(len)
                        .and_then(|end| data.get(ptr..end))
                        .ok_or_else(|| wasmtime::Error::msg("the request is outside the memory"))?
                        .to_vec()
                };
                // SAFETY: the pointer was made from the `&mut` of `Hosted::run`, which is alive
                // for as long as the store exists, and nothing else uses it meanwhile: the run
                // is on this thread and the store is dropped before `run` returns.
                let host = unsafe { &mut *caller.data().host.0 };
                let reply = host(&request);
                // The deadline of the stretch starts again.
                let ticks = caller.data().ticks;
                wasmtime::AsContextMut::as_context_mut(&mut caller).set_epoch_deadline(ticks);
                match reply {
                    HostReply::Reply(bytes) => {
                        let n = i32::try_from(bytes.len())
                            .map_err(|_| wasmtime::Error::msg("the reply is too large"))?;
                        caller.data_mut().pending = bytes;
                        Ok(n)
                    }
                    HostReply::Abort(e) => {
                        caller.data_mut().aborted = Some(e);
                        Err(wasmtime::Error::msg("the host stopped the run"))
                    }
                }
            },
        )
        .expect("the host function has a fresh name");
    linker
        .func_wrap(
            HOST_MODULE,
            "read",
            |mut caller: Caller<'_, HostState>, dst: u32| -> wasmtime::Result<()> {
                let memory = memory_of(&mut caller)?;
                let bytes = std::mem::take(&mut caller.data_mut().pending);
                memory.write(&mut caller, dst as usize, &bytes)?;
                Ok(())
            },
        )
        .expect("the host function has a fresh name");
    linker
}

impl HostedModule for Hosted {
    fn run(&self, request: &[u8], host: HostCall<'_>) -> Result<Vec<u8>, PluginError> {
        // SAFETY: only the lifetime changes. The pointer is used while this function runs: the
        // store, the instance and every host function that could use it are dropped before it
        // returns.
        let host: *mut (dyn FnMut(&[u8]) -> HostReply + 'static) = unsafe {
            std::mem::transmute::<
                *mut (dyn FnMut(&[u8]) -> HostReply + '_),
                *mut (dyn FnMut(&[u8]) -> HostReply + 'static),
            >(host as *mut _)
        };
        let ticks = self.limits.max_millis.div_ceil(TICK_MILLIS) + 1;
        let mut store = Store::new(
            &self.engine,
            HostState {
                max_memory: self.limits.memory_bytes,
                hit_memory_limit: false,
                host: HostPtr(host),
                pending: Vec::new(),
                aborted: None,
                ticks,
            },
        );
        store.limiter(|s| s);
        store.set_epoch_deadline(ticks);
        store
            .set_fuel(self.limits.fuel)
            .expect("fuel is switched on in the engine");
        let result = self.call(&mut store, request);
        match result {
            Ok(answer) => answer,
            Err(e) => Err(self.classify(&mut store, e)),
        }
    }
}

impl Hosted {
    fn call(
        &self,
        store: &mut Store<HostState>,
        request: &[u8],
    ) -> wasmtime::Result<Result<Vec<u8>, PluginError>> {
        let instance = linker(&self.engine).instantiate(&mut *store, &self.module)?;
        if let Ok(init) = instance.get_typed_func::<(), ()>(&mut *store, "_initialize") {
            init.call(&mut *store, ())?;
        }
        let alloc = instance.get_typed_func::<u32, u32>(&mut *store, "alloc")?;
        let run = instance.get_typed_func::<(u32, u32), u64>(&mut *store, "run")?;
        let memory = instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| wasmtime::Error::msg("'memory' is not a memory"))?;
        let len = u32::try_from(request.len())
            .map_err(|_| wasmtime::Error::msg("the request is too large"))?;
        let ptr = alloc.call(&mut *store, len)?;
        memory.write(&mut *store, ptr as usize, request)?;
        let packed = run.call(&mut *store, (ptr, len))?;
        if packed == 0 {
            return Ok(Err(PluginError::BadOutput("the module gave no answer".into())));
        }
        let (ptr, len) = unpack(packed);
        let (ptr, len) = (ptr as usize, len as usize);
        // The answer of a script is larger than that of a plug-in only by its log and its buffers;
        // the caller sets the bound for what it keeps.
        let max = self.limits.max_output_bytes;
        if len > max {
            return Ok(Err(PluginError::Limit(format!(
                "the answer has {len} bytes. The limit is {max}"
            ))));
        }
        Ok(memory
            .data(&*store)
            .get(ptr..ptr.saturating_add(len))
            .map(<[u8]>::to_vec)
            .ok_or_else(|| {
                PluginError::BadOutput("the answer points outside the memory".into())
            }))
    }

    fn classify(&self, store: &mut Store<HostState>, e: wasmtime::Error) -> PluginError {
        if let Some(stop) = store.data_mut().aborted.take() {
            return stop;
        }
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
                "the module ran for more than {} ms without a call to the host",
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

