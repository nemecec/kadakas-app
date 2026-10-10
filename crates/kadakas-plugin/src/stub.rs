//! The stub engine for the browser build (`wasm32`).
//!
//! The browser has its own `WebAssembly` API. A browser backend can implement
//! [`PluginRuntime`](crate::runtime::PluginRuntime) with it later, because the rest of the host
//! (manifest checks, trust, shape checks) does not name an engine. Until then, this engine says
//! that plug-ins are not available, so a document with plug-in objects still opens and draws:
//! the shapes are stored in the document as plain data.

use crate::runtime::{Limits, LoadedModule, PluginError, PluginRuntime};

/// An engine that refuses every module.
pub struct UnavailableRuntime;

impl PluginRuntime for UnavailableRuntime {
    fn name(&self) -> &'static str {
        "unavailable"
    }

    fn load(&self, _wasm: &[u8], _limits: &Limits) -> Result<Box<dyn LoadedModule>, PluginError> {
        Err(PluginError::Unsupported(
            "WebAssembly plug-ins do not run in the browser build yet. The shapes of plug-in objects still show".into(),
        ))
    }
}

// The stub has a test that runs on every target; see the tests of `plugin`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_refuses_modules() {
        let e = UnavailableRuntime
            .load(b"\0asm\x01\0\0\0", &Limits::default())
            .err()
            .unwrap();
        assert!(matches!(e, PluginError::Unsupported(_)));
        assert!(e.to_string().contains("browser"));
        assert_eq!(UnavailableRuntime.name(), "unavailable");
    }
}
