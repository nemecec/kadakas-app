//! The plug-ins that a session has installed, and the trust of the user.

use std::sync::Arc;

use crate::plugin::Plugin;
use crate::runtime::{Limits, PluginError, PluginRuntime, default_runtime};
use crate::store::{ModuleStore, Rejected, StoreBackend, TRUST_FILE};
use crate::trust::{self, ModuleTrust};

/// What happened when a store was attached.
#[derive(Debug, Default)]
pub struct StoreReport {
    /// The ids of the plug-ins that were loaded from the store.
    pub loaded: Vec<String>,
    /// The stored modules that were not loaded, and why. They stay in the store.
    pub rejected: Vec<Rejected>,
}

/// The installed plug-ins of one session, with the engine and the limits they run under.
pub struct Registry {
    runtime: Box<dyn PluginRuntime>,
    limits: Limits,
    plugins: Vec<Plugin>,
    /// The modules that the user trusted.
    pub trust: ModuleTrust,
    /// Where installed modules are kept between sessions, if anywhere.
    store: Option<ModuleStore>,
    /// Why the last change could not be kept, until the caller takes it.
    store_warning: Option<String>,
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
            store: None,
            store_warning: None,
        }
    }

    /// Keeps installed modules and the trust list on a medium that the caller gives. Loads the
    /// modules that are stored already, each one checked again like a new install (hash, size,
    /// structure, manifest). A stored module that fails stays in the store and is reported. The
    /// trust list is read from the same medium.
    pub fn attach(
        &mut self,
        backend: Arc<dyn StoreBackend>,
        max_module: usize,
        max_total: usize,
    ) -> StoreReport {
        self.trust = ModuleTrust::with_backend(backend.clone(), TRUST_FILE);
        self.attach_store(ModuleStore::new(backend, max_module, max_total))
    }

    /// Like [`Registry::attach`], for a folder: modules in `<home>/modules`, the trust list in
    /// `<home>/trusted_wasm_plugins.json`. The caller names the folder; a test passes a temporary
    /// one.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_home(&mut self, home: &std::path::Path) -> StoreReport {
        use crate::store::{FileBackend, NATIVE_TOTAL_CAP};
        self.trust =
            ModuleTrust::with_backend(Arc::new(FileBackend::new(home.to_path_buf())), TRUST_FILE);
        let store = ModuleStore::new(
            Arc::new(FileBackend::new(home.join("modules"))),
            self.limits.max_module_bytes,
            NATIVE_TOTAL_CAP,
        );
        self.attach_store(store)
    }

    fn attach_store(&mut self, store: ModuleStore) -> StoreReport {
        let loaded = store.load_all();
        let mut report = StoreReport {
            loaded: Vec::new(),
            rejected: loaded.rejected,
        };
        for bytes in loaded.modules {
            let hash = trust::hash(&bytes);
            match Plugin::load(self.runtime.as_ref(), &bytes, &self.limits) {
                Ok(plugin) => {
                    report.loaded.push(plugin.manifest().id.clone());
                    // Two stored builds of one id: the later name wins, as for an upgrade.
                    self.insert(plugin);
                }
                Err(e) => report.rejected.push(Rejected {
                    name: ModuleStore::name_of(&hash),
                    reason: e.to_string(),
                }),
            }
        }
        self.store = Some(store);
        report
    }

    /// True if installed modules and trust outlive the session.
    pub fn persistent(&self) -> bool {
        self.store.as_ref().is_some_and(ModuleStore::persistent)
    }

    /// Why the last install or removal could not be kept. Reading it clears it.
    pub fn take_store_warning(&mut self) -> Option<String> {
        self.store_warning.take()
    }

    fn insert(&mut self, plugin: Plugin) -> Option<String> {
        let id = plugin.manifest().id.clone();
        match self.plugins.iter().position(|p| p.manifest().id == id) {
            Some(i) => Some(
                std::mem::replace(&mut self.plugins[i], plugin)
                    .hash()
                    .to_string(),
            ),
            None => {
                self.plugins.push(plugin);
                None
            }
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

    /// Loads a module and adds it. A module that is installed already (same hash) is not compiled
    /// again. A module with the id of an installed plug-in replaces that build: this is the
    /// upgrade path. Objects of the document keep their shapes, and the new build is not trusted
    /// until its hash is trusted. The caller regenerates the objects with a reset.
    pub fn install(&mut self, wasm: &[u8]) -> Result<&Plugin, PluginError> {
        let hash = trust::hash(wasm);
        if let Some(i) = self.plugins.iter().position(|p| p.hash() == hash) {
            self.keep(wasm, None);
            return Ok(&self.plugins[i]);
        }
        let plugin = Plugin::load(self.runtime.as_ref(), wasm, &self.limits)?;
        let id = plugin.manifest().id.clone();
        let replaced = self.insert(plugin);
        self.keep(wasm, replaced);
        Ok(self.get(&id).expect("just inserted"))
    }

    /// Saves a module that was installed, and removes the build it replaced. A failure becomes
    /// the store warning: the module works in this session, but it will not be there next time.
    fn keep(&mut self, wasm: &[u8], replaced: Option<String>) {
        let Some(store) = &self.store else { return };
        let mut warning = None;
        if let Err(e) = store.save(wasm) {
            warning = Some(format!(
                "the module works in this session, but it is not kept for the next one: {e}"
            ));
        }
        if let Some(old) = replaced
            && let Err(e) = store.remove(&old)
        {
            warning.get_or_insert(format!("cannot remove the old build from the store: {e}"));
        }
        if warning.is_some() {
            self.store_warning = warning;
        }
    }

    /// Removes a plug-in. Returns false if there is none with this id. The stored copy goes too.
    pub fn remove(&mut self, id: &str) -> bool {
        let hash = self.get(id).map(|p| p.hash().to_string());
        let before = self.plugins.len();
        self.plugins.retain(|p| p.manifest().id != id);
        if let (Some(hash), Some(store)) = (hash, &self.store)
            && let Err(e) = store.remove(&hash)
        {
            self.store_warning = Some(format!("cannot remove the stored module: {e}"));
        }
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
