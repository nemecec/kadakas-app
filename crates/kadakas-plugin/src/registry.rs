//! The plug-ins that a session has installed, and the trust of the user.

use crate::plugin::Plugin;
use crate::runtime::{Limits, PluginError, PluginRuntime, default_runtime};
use crate::trust::ModuleTrust;

/// The installed plug-ins of one session, with the engine and the limits they run under.
pub struct Registry {
    runtime: Box<dyn PluginRuntime>,
    limits: Limits,
    plugins: Vec<Plugin>,
    /// The modules that the user trusted.
    pub trust: ModuleTrust,
}

impl Default for Registry {
    fn default() -> Self {
        Registry::new(default_runtime(), Limits::default())
    }
}

impl Registry {
    pub fn new(runtime: Box<dyn PluginRuntime>, limits: Limits) -> Self {
        Registry {
            runtime,
            limits,
            plugins: Vec::new(),
            trust: ModuleTrust::in_memory(),
        }
    }

    /// The name of the engine.
    pub fn engine(&self) -> &'static str {
        self.runtime.name()
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Changes the limits for modules that are loaded from now on.
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// Loads a module and adds it. A plug-in with the same id is replaced if no one uses it:
    /// `in_use` tells which ids are used by objects of the document. Installing the same module
    /// again changes nothing.
    pub fn install(
        &mut self,
        wasm: &[u8],
        in_use: impl Fn(&str) -> bool,
    ) -> Result<&Plugin, PluginError> {
        let plugin = Plugin::load(self.runtime.as_ref(), wasm, &self.limits)?;
        let id = plugin.manifest().id.clone();
        match self.plugins.iter().position(|p| p.manifest().id == id) {
            Some(i) if self.plugins[i].hash() == plugin.hash() => {}
            Some(_) if in_use(&id) => {
                return Err(PluginError::Refused(format!(
                    "objects use the installed plug-in '{id}'. Remove them before you install another build of it"
                )));
            }
            Some(i) => self.plugins[i] = plugin,
            None => self.plugins.push(plugin),
        }
        let i = self
            .plugins
            .iter()
            .position(|p| p.manifest().id == id)
            .expect("the plug-in is installed");
        Ok(&self.plugins[i])
    }

    /// Removes a plug-in. Returns false if there is none with this id.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.plugins.len();
        self.plugins.retain(|p| p.manifest().id != id);
        self.plugins.len() != before
    }

    /// The plug-in with this id.
    pub fn get(&self, id: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.manifest().id == id)
    }

    /// The plug-in with this module hash.
    pub fn by_hash(&self, hash: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.hash() == hash)
    }

    /// All plug-ins, in the order of installation.
    pub fn iter(&self) -> impl Iterator<Item = &Plugin> {
        self.plugins.iter()
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}
