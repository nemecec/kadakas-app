//! The browser side of the plug-in host (`wasm32` only): the `WebAssembly` API as a
//! [`Compiler`], and the browser storage as a [`StoreBackend`].
//!
//! This file has no logic of its own that matters for safety. The limits are in
//! [`crate::harden`] (the module enforces its own fuel and memory cap) and in
//! [`crate::metered`] (the calls and the classification of failures). Both run in the tests on the
//! desktop. What is left here is the translation to the browser API, so every function is short
//! and every failure of the browser becomes a `Result` with text, never a panic.
//!
//! Chrome refuses to compile a module over 4 KB with the synchronous `WebAssembly.Module` on the
//! main thread. So the app calls [`precompile`] (asynchronous) before it installs a module, and
//! the compiler finds the result in a cache by the hash of the rewritten bytes. Firefox and Safari
//! can compile synchronously. Instances are made synchronously from a compiled module, which every
//! browser allows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use base64::Engine as _;
use js_sys::{Array, ArrayBuffer, BigInt, Function, Object, Reflect, Uint8Array, WebAssembly};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::harden::{HardenLimits, harden};
use crate::metered::{Compiler, HostLink, MeteredRuntime, RawError, RawInstance, RawModule};
use crate::runtime::{Limits, PluginError};
use crate::store::{MemoryBackend, ModuleStore, StoreBackend, safe_name};
use crate::trust;

/// The runtime of the browser build.
pub fn runtime() -> MeteredRuntime<JsCompiler> {
    MeteredRuntime::new(JsCompiler)
}

/// Most compiled modules that the cache keeps.
const MAX_PRECOMPILED: usize = 32;

thread_local! {
    /// Modules compiled in advance, by the SHA-256 of the rewritten bytes.
    static PRECOMPILED: RefCell<HashMap<String, WebAssembly::Module>> = RefCell::new(HashMap::new());
}

fn text(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| {
            Reflect::get(e, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| "unknown error".into())
}

fn raw_error(e: &JsValue) -> RawError {
    if e.dyn_ref::<js_sys::RangeError>().is_some() {
        RawError::Stack(text(e))
    } else {
        RawError::Trap(text(e))
    }
}

/// Compiles a module in advance, asynchronously, so that a later [`JsCompiler::compile`] needs no
/// synchronous compile. Applies the same checks as `Plugin::load` before the browser sees the
/// bytes: size, structure, and the rewrite with the limits.
pub async fn precompile(wasm: &[u8], limits: &Limits) -> Result<(), PluginError> {
    if wasm.len() > limits.max_module_bytes {
        return Err(PluginError::Limit(format!(
            "the module has {} bytes. The limit is {}",
            wasm.len(),
            limits.max_module_bytes
        )));
    }
    crate::plugin::scan(wasm, limits)?;
    let hardened = harden(
        wasm,
        &HardenLimits {
            fuel: limits.start_fuel(),
            memory_bytes: limits.memory_bytes,
            host_imports: false,
        },
    )?;
    compile_in_advance(hardened).await
}

/// Like [`precompile`], for a hosted module (the script engine, see `crate::runtime::HostedModule`):
/// it may import `kadakas.call` and `kadakas.read`. The app
/// calls it once, before the first script runs, because the synchronous compile of a module of
/// this size is refused on the main thread of Chrome.
pub async fn precompile_hosted(wasm: &[u8]) -> Result<(), PluginError> {
    // The same rewrite as `MeteredRuntime::load_hosted`, so the key is the same.
    let hardened = harden(
        wasm,
        &HardenLimits {
            fuel: i64::MAX as u64,
            memory_bytes: crate::runtime::HOSTED_MEMORY_BYTES,
            host_imports: true,
        },
    )?;
    compile_in_advance(hardened).await
}

async fn compile_in_advance(hardened: Vec<u8>) -> Result<(), PluginError> {
    let key = trust::hash(&hardened);
    if PRECOMPILED.with(|p| p.borrow().contains_key(&key)) {
        return Ok(());
    }
    let promise = WebAssembly::compile(&Uint8Array::from(hardened.as_slice()));
    let module = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| PluginError::Invalid(format!("the browser refused the module: {}", text(&e))))?
        .dyn_into::<WebAssembly::Module>()
        .map_err(|_| PluginError::Invalid("the browser gave no module".into()))?;
    PRECOMPILED.with(|p| {
        let mut p = p.borrow_mut();
        if p.len() >= MAX_PRECOMPILED {
            p.clear();
        }
        p.insert(key, module);
    });
    Ok(())
}

/// Compiles all modules in a store in advance. A module that fails is left out: loading the
/// registry reports it.
pub async fn precompile_stored(store: &ModuleStore, limits: &Limits) {
    for bytes in store.load_all().modules {
        let _ = precompile(&bytes, limits).await;
    }
}

/// The compiler of the browser.
pub struct JsCompiler;

impl Compiler for JsCompiler {
    fn name(&self) -> &'static str {
        "webassembly"
    }

    fn compile(&self, wasm: &[u8]) -> Result<Box<dyn RawModule>, PluginError> {
        let key = trust::hash(wasm);
        if let Some(module) = PRECOMPILED.with(|p| p.borrow().get(&key).cloned()) {
            return Ok(Box::new(JsModule { module }));
        }
        let module = WebAssembly::Module::new(&Uint8Array::from(wasm)).map_err(|e| {
            PluginError::Invalid(format!(
                "the browser could not compile the module: {}. Some browsers compile large modules only asynchronously, so install it through the app",
                text(&e)
            ))
        })?;
        Ok(Box::new(JsModule { module }))
    }
}

struct JsModule {
    module: WebAssembly::Module,
}

fn names(list: &Array) -> Vec<String> {
    list.iter()
        .filter_map(|item| {
            Reflect::get(&item, &JsValue::from_str("name"))
                .ok()?
                .as_string()
        })
        .collect()
}

impl RawModule for JsModule {
    fn exports(&self) -> Vec<String> {
        names(&WebAssembly::Module::exports(&self.module))
    }

    fn import_count(&self) -> usize {
        WebAssembly::Module::imports(&self.module).length() as usize
    }

    fn instantiate(&self) -> Result<Box<dyn RawInstance>, RawError> {
        // No imports: the import object is empty.
        self.build(&Object::new(), Vec::new(), Rc::default())
    }

    fn instantiate_hosted(&self, link: Rc<dyn HostLink>) -> Result<Box<dyn RawInstance>, RawError> {
        // The imports need the memory of the instance, which exists only after the instance. The
        // closures find it in this cell.
        let memory: Rc<RefCell<Option<WebAssembly::Memory>>> = Rc::default();
        let bytes_at = |memory: &Rc<RefCell<Option<WebAssembly::Memory>>>,
                        ptr: u32,
                        len: u32|
         -> Result<Uint8Array, JsValue> {
            let mem = memory.borrow();
            let buffer = mem
                .as_ref()
                .ok_or_else(|| JsValue::from_str("the module called the host too early"))?
                .buffer()
                .dyn_into::<ArrayBuffer>()?;
            let end = u64::from(ptr) + u64::from(len);
            if end > u64::from(buffer.byte_length()) {
                return Err(JsValue::from_str("the request is outside the memory"));
            }
            Ok(Uint8Array::new_with_byte_offset_and_length(
                &buffer, ptr, len,
            ))
        };
        let call = {
            let (link, memory) = (link.clone(), memory.clone());
            Closure::<dyn FnMut(u32, u32) -> Result<u32, JsValue>>::new(move |ptr, len| {
                let request = bytes_at(&memory, ptr, len)?.to_vec();
                link.call(&request)
                    .map_err(|e| JsValue::from_str(&format!("{e:?}")))
            })
        };
        let read = {
            let (link, memory) = (link.clone(), memory.clone());
            Closure::<dyn FnMut(u32) -> Result<(), JsValue>>::new(move |dst| {
                let reply = link.take_reply();
                let n = u32::try_from(reply.len())
                    .map_err(|_| JsValue::from_str("the reply is too large"))?;
                bytes_at(&memory, dst, n)?.copy_from(&reply);
                Ok(())
            })
        };
        let host = Object::new();
        for (name, f) in [("call", call.as_ref()), ("read", read.as_ref())] {
            Reflect::set(&host, &JsValue::from_str(name), f).map_err(|e| raw_error(&e))?;
        }
        let imports = Object::new();
        Reflect::set(
            &imports,
            &JsValue::from_str(crate::runtime::HOST_MODULE),
            &host,
        )
        .map_err(|e| raw_error(&e))?;
        self.build(&imports, vec![Box::new(call), Box::new(read)], memory)
    }
}

impl JsModule {
    /// Makes an instance. `keep` holds what the imports need until the instance is dropped;
    /// `memory_cell` receives the memory of the instance for the imports.
    fn build(
        &self,
        imports: &Object,
        keep: Vec<Box<dyn std::any::Any>>,
        memory_cell: Rc<RefCell<Option<WebAssembly::Memory>>>,
    ) -> Result<Box<dyn RawInstance>, RawError> {
        let instance =
            WebAssembly::Instance::new(&self.module, imports).map_err(|e| raw_error(&e))?;
        let exports = instance.exports();
        let get = |name: &str| {
            Reflect::get(&exports, &JsValue::from_str(name)).map_err(|e| RawError::Trap(text(&e)))
        };
        let memory = get("memory")?
            .dyn_into::<WebAssembly::Memory>()
            .map_err(|_| RawError::Trap("'memory' is not a memory".into()))?;
        *memory_cell.borrow_mut() = Some(memory.clone());
        let fuel = get(crate::harden::FUEL_EXPORT)
            .ok()
            .and_then(|g| g.dyn_into::<WebAssembly::Global>().ok());
        Ok(Box::new(JsInstance {
            exports,
            memory,
            fuel,
            _keep: keep,
        }))
    }
}

struct JsInstance {
    exports: Object,
    memory: WebAssembly::Memory,
    fuel: Option<WebAssembly::Global>,
    /// The closures of the imports of a hosted module: they must live as long as the instance.
    _keep: Vec<Box<dyn std::any::Any>>,
}

/// The packed `u64` of an export comes back as a signed `BigInt`.
fn packed(v: &JsValue) -> Result<u64, RawError> {
    let big = v
        .dyn_ref::<BigInt>()
        .ok_or_else(|| RawError::Trap("the export did not return an i64".into()))?;
    let digits = big
        .to_string(10)
        .map_err(|_| RawError::Trap("bad i64".into()))?
        .as_string()
        .unwrap_or_default();
    digits
        .parse::<i128>()
        .map(|n| n as u64)
        .map_err(|_| RawError::Trap("bad i64".into()))
}

impl JsInstance {
    fn function(&self, name: &str) -> Result<Function, RawError> {
        Reflect::get(&self.exports, &JsValue::from_str(name))
            .map_err(|e| RawError::Trap(text(&e)))?
            .dyn_into::<Function>()
            .map_err(|_| RawError::Trap(format!("'{name}' is not a function")))
    }

    /// The memory right now. A call may grow the memory, which replaces the buffer.
    fn buffer(&self) -> Option<ArrayBuffer> {
        self.memory.buffer().dyn_into::<ArrayBuffer>().ok()
    }

    fn range(&self, ptr: u32, len: u32) -> Option<Uint8Array> {
        let buffer = self.buffer()?;
        let end = u64::from(ptr) + u64::from(len);
        (end <= u64::from(buffer.byte_length()))
            .then(|| Uint8Array::new_with_byte_offset_and_length(&buffer, ptr, len))
    }
}

impl RawInstance for JsInstance {
    fn initialize(&mut self) -> Result<(), RawError> {
        let init = Reflect::get(&self.exports, &JsValue::from_str("_initialize"))
            .map_err(|e| RawError::Trap(text(&e)))?;
        if let Some(f) = init.dyn_ref::<Function>() {
            f.call0(&JsValue::NULL).map_err(|e| raw_error(&e))?;
        }
        Ok(())
    }

    fn alloc(&mut self, len: u32) -> Result<u32, RawError> {
        let v = self
            .function("alloc")?
            .call1(&JsValue::NULL, &JsValue::from_f64(f64::from(len)))
            .map_err(|e| raw_error(&e))?;
        v.as_f64()
            .map(|n| n as u32)
            .ok_or_else(|| RawError::Trap("alloc did not return a number".into()))
    }

    fn write(&mut self, ptr: u32, bytes: &[u8]) -> Result<(), RawError> {
        let len = u32::try_from(bytes.len())
            .map_err(|_| RawError::Trap("the request is too large".into()))?;
        self.range(ptr, len)
            .ok_or_else(|| RawError::Trap("alloc returned an address outside the memory".into()))?
            .copy_from(bytes);
        Ok(())
    }

    fn call(&mut self, export: &str, args: &[u32]) -> Result<u64, RawError> {
        let f = self.function(export)?;
        let v = match args {
            [] => f.call0(&JsValue::NULL),
            [a, b] => f.call2(
                &JsValue::NULL,
                &JsValue::from_f64(f64::from(*a)),
                &JsValue::from_f64(f64::from(*b)),
            ),
            _ => {
                return Err(RawError::Trap(
                    "the host calls with none or two arguments".into(),
                ));
            }
        }
        .map_err(|e| raw_error(&e))?;
        packed(&v)
    }

    fn read(&mut self, ptr: u32, len: u32) -> Option<Vec<u8>> {
        self.range(ptr, len).map(|a| a.to_vec())
    }

    fn set_fuel(&mut self, fuel: i64) -> Result<(), RawError> {
        let global = self
            .fuel
            .as_ref()
            .ok_or_else(|| RawError::Trap("no fuel counter".into()))?;
        global.set_value(&BigInt::from(fuel).into());
        Ok(())
    }

    fn fuel_left(&mut self) -> Option<i64> {
        let v = self.fuel.as_ref()?.value();
        let digits = v.dyn_ref::<BigInt>()?.to_string(10).ok()?.as_string()?;
        digits.parse::<i64>().ok()
    }

    fn memory_bytes(&mut self) -> usize {
        self.buffer().map_or(0, |b| b.byte_length() as usize)
    }
}

// ------------------------------------------------------------------ storage

const KEY_PREFIX: &str = "kadakas.plugins/";

/// Blobs in `localStorage`, as base64 text. All errors (storage off, a full quota, a private
/// window) are returned as text.
pub struct BrowserBackend {
    storage: web_sys::Storage,
}

impl BrowserBackend {
    /// The storage of the page, if it works. Writes and removes a test entry, because some
    /// browsers give a storage object that throws on every write.
    pub fn open() -> Option<BrowserBackend> {
        let storage = web_sys::window()?.local_storage().ok()??;
        let probe = format!("{KEY_PREFIX}.probe");
        storage.set_item(&probe, "1").ok()?;
        storage.remove_item(&probe).ok()?;
        Some(BrowserBackend { storage })
    }

    fn key(name: &str) -> Result<String, String> {
        if safe_name(name) {
            Ok(format!("{KEY_PREFIX}{name}"))
        } else {
            Err(format!("'{name}' is not a name that the store uses"))
        }
    }
}

fn decoded_len(encoded: usize, tail: &str) -> u64 {
    let padding = tail.bytes().rev().take(2).filter(|b| *b == b'=').count();
    (encoded as u64 / 4 * 3).saturating_sub(padding as u64)
}

impl StoreBackend for BrowserBackend {
    fn medium(&self) -> &'static str {
        "browser storage"
    }

    fn list(&self) -> Result<Vec<(String, u64)>, String> {
        let n = self.storage.length().map_err(|e| text(&e))?;
        let mut out = Vec::new();
        for i in 0..n {
            let Some(key) = self.storage.key(i).map_err(|e| text(&e))? else {
                continue;
            };
            let Some(name) = key.strip_prefix(KEY_PREFIX).filter(|n| safe_name(n)) else {
                continue;
            };
            let value = self
                .storage
                .get_item(&key)
                .map_err(|e| text(&e))?
                .unwrap_or_default();
            out.push((name.to_string(), decoded_len(value.len(), &value)));
        }
        out.sort();
        Ok(out)
    }

    fn read(&self, name: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
        let Some(value) = self
            .storage
            .get_item(&Self::key(name)?)
            .map_err(|e| text(&e))?
        else {
            return Ok(None);
        };
        if value.len() > max / 3 * 4 + 8 {
            return Err(format!("{name} has more than {max} bytes"));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(value.as_bytes())
            .map_err(|e| format!("{name} is not valid: {e}"))?;
        if bytes.len() > max {
            return Err(format!("{name} has more than {max} bytes"));
        }
        Ok(Some(bytes))
    }

    fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let value = base64::engine::general_purpose::STANDARD.encode(bytes);
        self.storage
            .set_item(&Self::key(name)?, &value)
            .map_err(|e| format!("the browser storage refused {name}: {}", text(&e)))
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        self.storage
            .remove_item(&Self::key(name)?)
            .map_err(|e| text(&e))
    }
}

/// The best storage that the page has: `localStorage` if it works, else memory. A memory backend
/// is not persistent, and the registry and the app say so.
pub fn backend() -> Arc<dyn StoreBackend> {
    match BrowserBackend::open() {
        Some(b) => Arc::new(b),
        None => Arc::new(MemoryBackend::new()),
    }
}
