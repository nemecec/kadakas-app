//! An engine for hosts that run modules but cannot limit them: the browser.
//!
//! [`MeteredRuntime`] does everything that does not depend on the `WebAssembly` API of the
//! browser. It rewrites every module with [`crate::harden`] (fuel and memory cap, no imports), asks
//! a [`Compiler`] to compile the result, and makes the calls: it asks the instance for memory,
//! writes the request, calls the export, reads the answer, and tells "out of fuel" from other
//! traps. The `Compiler` and the instance it makes are small: they only have to touch the
//! `WebAssembly` API. The browser has one (`crate::web`), and the tests have one on wasmtime. So
//! the code that matters is the same code in both places, and it is tested on the desktop.
//!
//! The limits, compared to the native engine:
//!
//! | Limit | Native (`wasmtime`) | Metered (browser) |
//! |---|---|---|
//! | Fuel | exact, per instruction | per instruction, counted by the rewritten module |
//! | Memory | cap on growth | maximum set in the module, growth over it returns -1 |
//! | Module size, functions, locals | checked before compiling | checked before compiling (same code) |
//! | Answer size, shapes, state | same code | same code |
//! | Wall-clock deadline (1.5 s) | epoch deadline | **none** |
//!
//! There is no deadline because a browser page cannot interrupt a running module from its own
//! thread, and a worker cannot be called without waiting for it. The fuel is the bound: a module
//! stops after its fuel. How long that takes depends on the computer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::harden::{FUEL_EXPORT, HardenLimits, PAGE, harden};
use crate::runtime::{
    CallKind, HOSTED_EXPORTS, HOSTED_MEMORY_BYTES, HostCall, HostReply, HostedModule, Limits,
    LoadedModule, MaybeSend, PluginError, PluginRuntime, REQUIRED_EXPORTS,
};
use kadakas_plugin_abi::guest::unpack;

/// A failure of one call, as the engine reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawError {
    /// The module ran out of stack (the host stack, in a browser).
    Stack(String),
    /// The module trapped, or the engine refused the call.
    Trap(String),
}

/// What the two imports of a hosted module reach (`kadakas.call` and `kadakas.read`). The compiler
/// builds the imports around it.
pub trait HostLink {
    /// The import `call`: the bytes of a request. Returns the length of the reply, which `take_reply`
    /// then gives. An error stops the module (the compiler makes it a trap).
    fn call(&self, request: &[u8]) -> Result<u32, RawError>;
    /// The import `read`: the reply of the last call.
    fn take_reply(&self) -> Vec<u8>;
}

/// A running instance of a rewritten module.
pub trait RawInstance {
    /// Calls `_initialize` if the module exports it (a hosted module made with a C library does).
    fn initialize(&mut self) -> Result<(), RawError> {
        Ok(())
    }
    /// Calls the export `alloc` and returns the address.
    fn alloc(&mut self, len: u32) -> Result<u32, RawError>;
    /// Writes bytes into the linear memory. An address outside the memory is an error.
    fn write(&mut self, ptr: u32, bytes: &[u8]) -> Result<(), RawError>;
    /// Calls an export with `u32` arguments that returns the packed `u64`.
    fn call(&mut self, export: &str, args: &[u32]) -> Result<u64, RawError>;
    /// Reads bytes from the linear memory, or `None` if the range is outside the memory.
    fn read(&mut self, ptr: u32, len: u32) -> Option<Vec<u8>>;
    /// Sets the fuel of this call (the exported fuel global). The host does this before the first
    /// call into the instance.
    fn set_fuel(&mut self, fuel: i64) -> Result<(), RawError>;
    /// The fuel that is left (the value of the exported fuel global), if it can be read.
    fn fuel_left(&mut self) -> Option<i64>;
    /// The size of the linear memory in bytes.
    fn memory_bytes(&mut self) -> usize;
}

/// A compiled module.
pub trait RawModule: MaybeSend {
    /// The names of the exports.
    fn exports(&self) -> Vec<String>;
    /// The number of imports.
    fn import_count(&self) -> usize;
    /// Makes a new instance. Every call uses a new one.
    fn instantiate(&self) -> Result<Box<dyn RawInstance>, RawError>;
    /// Makes a new instance of a hosted module, whose imports reach `link`.
    fn instantiate_hosted(
        &self,
        _link: Rc<dyn HostLink>,
    ) -> Result<Box<dyn RawInstance>, RawError> {
        Err(RawError::Trap(
            "this compiler cannot run hosted modules".into(),
        ))
    }
}

/// Compiles the bytes of a rewritten module with some engine.
pub trait Compiler: MaybeSend {
    fn name(&self) -> &'static str;
    fn compile(&self, wasm: &[u8]) -> Result<Box<dyn RawModule>, PluginError>;
}

/// The runtime of a [`Compiler`]. See the module documentation.
pub struct MeteredRuntime<C: Compiler> {
    compiler: C,
}

impl<C: Compiler> MeteredRuntime<C> {
    pub fn new(compiler: C) -> Self {
        MeteredRuntime { compiler }
    }
}

impl<C: Compiler> PluginRuntime for MeteredRuntime<C> {
    fn name(&self) -> &'static str {
        self.compiler.name()
    }

    fn load_hosted(
        &self,
        wasm: &[u8],
        limits: &Limits,
    ) -> Result<Box<dyn HostedModule>, PluginError> {
        // The same bytes for any limits, so that the module can be compiled in advance. The host
        // sets the fuel of each run.
        let hardened = harden(
            wasm,
            &HardenLimits {
                fuel: i64::MAX as u64,
                memory_bytes: HOSTED_MEMORY_BYTES,
                host_imports: true,
            },
        )?;
        let module = self.compiler.compile(&hardened)?;
        let exports = module.exports();
        for name in HOSTED_EXPORTS.iter().chain(&[FUEL_EXPORT]) {
            if !exports.iter().any(|e| e == name) {
                return Err(PluginError::Invalid(format!(
                    "the module does not export '{name}'"
                )));
            }
        }
        Ok(Box::new(MeteredHosted {
            module,
            limits: *limits,
        }))
    }

    fn load(&self, wasm: &[u8], limits: &Limits) -> Result<Box<dyn LoadedModule>, PluginError> {
        let hardened = harden(
            wasm,
            &HardenLimits {
                fuel: limits.start_fuel(),
                memory_bytes: limits.memory_bytes,
                host_imports: false,
            },
        )?;
        let module = self.compiler.compile(&hardened)?;
        if module.import_count() > 0 {
            return Err(PluginError::Invalid(
                "the module has imports. A plug-in may import nothing".into(),
            ));
        }
        let exports = module.exports();
        for name in REQUIRED_EXPORTS {
            if !exports.iter().any(|e| e == name) {
                return Err(PluginError::Invalid(format!(
                    "the module does not export '{name}'"
                )));
            }
        }
        if !exports.iter().any(|e| e == FUEL_EXPORT) {
            return Err(PluginError::Invalid(
                "the rewritten module has no fuel counter".into(),
            ));
        }
        Ok(Box::new(Metered {
            module,
            limits: *limits,
        }))
    }
}

/// Largest manifest that the host reads, as for the native engine.
const MAX_MANIFEST_BYTES: usize = 256 << 10;

struct Metered {
    module: Box<dyn RawModule>,
    limits: Limits,
}

/// Tells a limit from a trap after a call failed.
fn classify(limits: &Limits, inst: &mut dyn RawInstance, e: RawError, budget: u64) -> PluginError {
    if inst.fuel_left().is_some_and(|f| f < 0) {
        return PluginError::Limit(format!(
            "the module used all of its {budget} fuel units (an endless loop?)"
        ));
    }
    match e {
        RawError::Stack(_) => PluginError::Limit("the module overflowed its stack".into()),
        RawError::Trap(m) => {
            // A memory.grow over the cap returns -1. A module that then fails ends up here
            // with its memory at the cap.
            if inst.memory_bytes() as u64 / PAGE >= limits.memory_bytes as u64 / PAGE {
                PluginError::Limit(format!(
                    "the module needs more than {} bytes of memory ({m})",
                    limits.memory_bytes
                ))
            } else {
                PluginError::Trap(m)
            }
        }
    }
}

/// The host side of a hosted run: the function that serves the calls, the reply that waits for
/// `read`, and why the host stopped the run.
struct Link {
    host: Cell<Option<*mut (dyn FnMut(&[u8]) -> HostReply + 'static)>>,
    pending: RefCell<Vec<u8>>,
    aborted: RefCell<Option<PluginError>>,
}

impl HostLink for Link {
    fn call(&self, request: &[u8]) -> Result<u32, RawError> {
        let Some(host) = self.host.get() else {
            return Err(RawError::Trap(
                "the module called the host after the run".into(),
            ));
        };
        // SAFETY: the pointer is set in `MeteredHosted::run` from a `&mut` that lives until the
        // run ends, and cleared before `run` returns. The browser has one thread, and the native
        // test engine calls from the thread of `run`.
        let reply = unsafe { (*host)(request) };
        match reply {
            HostReply::Reply(bytes) => {
                let n = u32::try_from(bytes.len())
                    .map_err(|_| RawError::Trap("the reply is too large".into()))?;
                *self.pending.borrow_mut() = bytes;
                Ok(n)
            }
            HostReply::Abort(e) => {
                *self.aborted.borrow_mut() = Some(e);
                Err(RawError::Trap("the host stopped the run".into()))
            }
        }
    }

    fn take_reply(&self) -> Vec<u8> {
        std::mem::take(&mut self.pending.borrow_mut())
    }
}

struct MeteredHosted {
    module: Box<dyn RawModule>,
    limits: Limits,
}

impl HostedModule for MeteredHosted {
    fn run(&self, request: &[u8], host: HostCall<'_>) -> Result<Vec<u8>, PluginError> {
        let link = Rc::new(Link {
            // SAFETY: only the lifetime changes; see `Link::call`.
            host: Cell::new(Some(unsafe {
                std::mem::transmute::<
                    *mut (dyn FnMut(&[u8]) -> HostReply + '_),
                    *mut (dyn FnMut(&[u8]) -> HostReply + 'static),
                >(host as *mut _)
            })),
            pending: RefCell::new(Vec::new()),
            aborted: RefCell::new(None),
        });
        let result = self.run_with(&link, request);
        link.host.set(None);
        result
    }
}

impl MeteredHosted {
    fn run_with(&self, link: &Rc<Link>, request: &[u8]) -> Result<Vec<u8>, PluginError> {
        let mut inst = self
            .module
            .instantiate_hosted(link.clone())
            .map_err(|e| match e {
                RawError::Stack(m) => PluginError::Limit(m),
                RawError::Trap(m) => PluginError::Trap(m),
            })?;
        let inst = inst.as_mut();
        let budget = self.limits.fuel;
        let go = |inst: &mut dyn RawInstance| -> Result<u64, RawError> {
            inst.set_fuel(i64::try_from(budget).unwrap_or(i64::MAX))?;
            inst.initialize()?;
            let len = u32::try_from(request.len())
                .map_err(|_| RawError::Trap("the request is too large".into()))?;
            let ptr = inst.alloc(len)?;
            inst.write(ptr, request)?;
            inst.call("run", &[ptr, len])
        };
        let packed = match go(inst) {
            Ok(p) => p,
            Err(e) => {
                if let Some(stop) = link.aborted.borrow_mut().take() {
                    return Err(stop);
                }
                return Err(classify(&self.limits, inst, e, budget));
            }
        };
        if packed == 0 {
            return Err(PluginError::BadOutput("the module gave no answer".into()));
        }
        let (ptr, len) = unpack(packed);
        if len as usize > self.limits.max_output_bytes {
            return Err(PluginError::Limit(format!(
                "the answer has {len} bytes. The limit is {}",
                self.limits.max_output_bytes
            )));
        }
        inst.read(ptr, len)
            .ok_or_else(|| PluginError::BadOutput("the answer points outside the memory".into()))
    }
}

impl Metered {
    fn classify(&self, inst: &mut dyn RawInstance, e: RawError, budget: u64) -> PluginError {
        classify(&self.limits, inst, e, budget)
    }

    fn run(
        &self,
        export: &str,
        request: Option<&[u8]>,
        max: usize,
        kind: CallKind,
    ) -> Result<Vec<u8>, PluginError> {
        let mut inst = self.module.instantiate().map_err(|e| match e {
            RawError::Stack(m) => PluginError::Limit(m),
            RawError::Trap(m) => PluginError::Trap(m),
        })?;
        let inst = inst.as_mut();
        let budget = self.limits.fuel_for(kind);
        let packed = match inst
            .set_fuel(i64::try_from(budget).unwrap_or(i64::MAX))
            .and_then(|()| self.call(inst, export, request))
        {
            Ok(p) => p,
            Err(e) => return Err(self.classify(inst, e, budget)),
        };
        if packed == 0 {
            return Err(PluginError::BadOutput("the module gave no answer".into()));
        }
        let (ptr, len) = unpack(packed);
        if len as usize > max {
            return Err(PluginError::Limit(format!(
                "the answer has {len} bytes. The limit is {max}"
            )));
        }
        inst.read(ptr, len)
            .ok_or_else(|| PluginError::BadOutput("the answer points outside the memory".into()))
    }

    fn call(
        &self,
        inst: &mut dyn RawInstance,
        export: &str,
        request: Option<&[u8]>,
    ) -> Result<u64, RawError> {
        match request {
            None => inst.call(export, &[]),
            Some(bytes) => {
                let len = u32::try_from(bytes.len())
                    .map_err(|_| RawError::Trap("the request is too large".into()))?;
                let ptr = inst.alloc(len)?;
                inst.write(ptr, bytes)?;
                inst.call(export, &[ptr, len])
            }
        }
    }
}

impl LoadedModule for Metered {
    fn describe(&self) -> Result<Vec<u8>, PluginError> {
        self.run(
            "describe",
            None,
            MAX_MANIFEST_BYTES.min(self.limits.max_output_bytes),
            CallKind::Describe,
        )
    }

    fn reset(&self, request: &[u8]) -> Result<Vec<u8>, PluginError> {
        self.run(
            "reset",
            Some(request),
            self.limits.max_output_bytes,
            CallKind::Reset,
        )
    }

    fn tool(&self, request: &[u8], light: bool) -> Result<Vec<u8>, PluginError> {
        let kind = if light {
            CallKind::ToolMove
        } else {
            CallKind::ToolStep
        };
        self.run("tool", Some(request), self.limits.max_output_bytes, kind)
    }
}
