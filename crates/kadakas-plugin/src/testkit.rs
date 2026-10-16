//! A test compiler for the metered engine: wasmtime without fuel and without a deadline, so that
//! whatever stops a module is the rewritten module itself (see `crate::harden`), as it is in a
//! browser. The tests of this crate and of the script engine (`kadakas-script-js`, feature
//! `testkit`) run the metered engine on it. Native targets only. Not for shipping.

use std::rc::Rc;

use wasmtime::{Caller, Config, Engine, Instance, Linker, Memory, Module, Store, Trap};

use crate::metered::{Compiler, HostLink, RawError, RawInstance, RawModule};
use crate::runtime::{HOST_MODULE, PluginError};

/// A compiler on wasmtime that has no limits of its own.
pub struct PlainCompiler {
    engine: Engine,
}

impl PlainCompiler {
    pub fn new() -> Self {
        PlainCompiler {
            engine: Engine::new(&Config::new()).expect("a plain wasmtime engine"),
        }
    }
}

impl Default for PlainCompiler {
    fn default() -> Self {
        Self::new()
    }
}

struct PlainModule {
    engine: Engine,
    module: Module,
}

/// The state of a store: the link of a hosted run, if there is one.
struct State {
    link: Option<Rc<dyn HostLink>>,
}

struct PlainInstance {
    store: Store<State>,
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

impl PlainModule {
    fn make(&self, link: Option<Rc<dyn HostLink>>) -> Result<Box<dyn RawInstance>, RawError> {
        let mut store = Store::new(&self.engine, State { link });
        let mut linker = Linker::<State>::new(&self.engine);
        linker
            .func_wrap(
                HOST_MODULE,
                "call",
                |mut caller: Caller<'_, State>, ptr: u32, len: u32| -> wasmtime::Result<i32> {
                    let memory = caller
                        .get_export("memory")
                        .and_then(wasmtime::Extern::into_memory)
                        .ok_or_else(|| wasmtime::Error::msg("no memory"))?;
                    let request = memory
                        .data(&caller)
                        .get(ptr as usize..(ptr as usize).saturating_add(len as usize))
                        .ok_or_else(|| wasmtime::Error::msg("request outside the memory"))?
                        .to_vec();
                    let link = caller
                        .data()
                        .link
                        .clone()
                        .ok_or_else(|| wasmtime::Error::msg("no host"))?;
                    link.call(&request)
                        .map(|n| n as i32)
                        .map_err(|e| wasmtime::Error::msg(format!("{e:?}")))
                },
            )
            .map_err(raw)?;
        linker
            .func_wrap(
                HOST_MODULE,
                "read",
                |mut caller: Caller<'_, State>, dst: u32| -> wasmtime::Result<()> {
                    let memory = caller
                        .get_export("memory")
                        .and_then(wasmtime::Extern::into_memory)
                        .ok_or_else(|| wasmtime::Error::msg("no memory"))?;
                    let link = caller
                        .data()
                        .link
                        .clone()
                        .ok_or_else(|| wasmtime::Error::msg("no host"))?;
                    let bytes = link.take_reply();
                    memory.write(&mut caller, dst as usize, &bytes)?;
                    Ok(())
                },
            )
            .map_err(raw)?;
        let instance = linker.instantiate(&mut store, &self.module).map_err(raw)?;
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        Ok(Box::new(PlainInstance {
            store,
            instance,
            memory,
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
        self.make(None)
    }

    fn instantiate_hosted(&self, link: Rc<dyn HostLink>) -> Result<Box<dyn RawInstance>, RawError> {
        self.make(Some(link))
    }
}

impl RawInstance for PlainInstance {
    fn initialize(&mut self) -> Result<(), RawError> {
        if let Ok(f) = self
            .instance
            .get_typed_func::<(), ()>(&mut self.store, "_initialize")
        {
            f.call(&mut self.store, ()).map_err(raw)?;
        }
        Ok(())
    }

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
