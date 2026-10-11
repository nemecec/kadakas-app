//! Tests of the persistence of installed modules and of trust. Every test uses a folder of its own
//! in the temporary folder, or a backend in memory. No test reads or writes the folders of the user.

use std::sync::Arc;

use super::*;
use crate::store::{MemoryBackend, StoreBackend};

fn temp_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kadakas-registry-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn installed_modules_and_trust_come_back_in_the_next_session() {
    let home = temp_home("a");
    let hash = trust::hash(PLANTER);
    {
        let mut reg = Registry::default();
        let report = reg.open_home(&home);
        assert!(report.loaded.is_empty() && report.rejected.is_empty());
        assert!(reg.persistent());
        reg.install(PLANTER).unwrap();
        reg.install(PLANTER).unwrap();
        reg.trust.trust(&hash, "Raised planter").unwrap();
        assert!(reg.take_store_warning().is_none());
    }
    assert!(home.join("modules").join(format!("{hash}.wasm")).is_file());
    // A new session: the module is there, and so is the trust.
    let mut reg = Registry::default();
    let report = reg.open_home(&home);
    assert_eq!(
        report.loaded,
        vec!["org.kadakas.raised-planter".to_string()]
    );
    assert!(reg.get("org.kadakas.raised-planter").is_some());
    assert!(reg.trust.is_trusted(&hash));
    // Removing the plug-in removes the stored copy.
    assert!(reg.remove("org.kadakas.raised-planter"));
    assert!(!home.join("modules").join(format!("{hash}.wasm")).exists());
    let mut again = Registry::default();
    assert!(again.open_home(&home).loaded.is_empty());
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_tampered_or_broken_stored_module_is_not_loaded() {
    let home = temp_home("b");
    let modules = home.join("modules");
    {
        let mut reg = Registry::default();
        reg.open_home(&home);
        reg.install(PLANTER).unwrap();
    }
    let hash = trust::hash(PLANTER);
    let file = modules.join(format!("{hash}.wasm"));
    // One changed byte: the hash no longer fits the name.
    let mut bytes = std::fs::read(&file).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(&file, &bytes).unwrap();
    // A file with a good name and hash that is not a plug-in module.
    let junk = b"not a module".to_vec();
    std::fs::write(modules.join(format!("{}.wasm", trust::hash(&junk))), &junk).unwrap();
    let mut reg = Registry::default();
    let report = reg.open_home(&home);
    assert!(report.loaded.is_empty() && reg.is_empty());
    assert_eq!(report.rejected.len(), 2);
    assert!(report.rejected.iter().any(|r| r.reason.contains("changed")));
    assert!(
        report
            .rejected
            .iter()
            .any(|r| r.reason.contains("no description"))
    );
    // The files stay. The next start reports them again.
    assert!(file.is_file());
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn an_upgrade_replaces_the_stored_build_and_a_failing_medium_is_a_warning() {
    let mem = Arc::new(MemoryBackend::new());
    let mut reg = Registry::default();
    reg.attach(mem.clone(), 2 << 20, 2 << 20);
    assert!(!reg.persistent(), "a memory backend is not persistent");
    let id = reg.install(PLANTER).unwrap().manifest().id.clone();
    let v2 = format!(
        r#"{{"id":"{id}","name":"Raised planter","version":"2","abi_version":2,"kinds":["tool"],"tool":{{}}}}"#
    );
    let other = module_with(&v2, ONE_CIRCLE, None, "");
    reg.install(&other).unwrap();
    let names: Vec<String> = mem.list().unwrap().into_iter().map(|(n, _)| n).collect();
    let h = trust::hash(&other);
    assert_eq!(
        names,
        vec![format!("{h}.manifest.json"), format!("{h}.wasm")]
    );
    // The medium refuses writes: the install works, with a warning that is read once.
    *mem.fail_writes.lock().unwrap() = Some("quota".into());
    reg.install(PLANTER).unwrap();
    let w = reg.take_store_warning().unwrap();
    assert!(w.contains("not kept") && w.contains("quota"));
    assert!(reg.take_store_warning().is_none());
    // A module over the cap of the store is a warning too.
    let mut small = Registry::default();
    small.attach(Arc::new(MemoryBackend::new()), 1000, 1000);
    small.install(PLANTER).unwrap();
    assert!(small.take_store_warning().unwrap().contains("limit"));
    // A medium that fails on removal is a warning.
    *mem.fail_writes.lock().unwrap() = None;
    *mem.fail_all.lock().unwrap() = Some("disk gone".into());
    assert!(reg.remove(&id));
    assert!(reg.take_store_warning().unwrap().contains("disk gone"));
    // An upgrade whose old copy cannot be removed is a warning.
    let mut reg2 = Registry::default();
    let mem2 = Arc::new(MemoryBackend::new());
    reg2.attach(mem2.clone(), 2 << 20, 2 << 20);
    reg2.install(PLANTER).unwrap();
    *mem2.fail_all.lock().unwrap() = Some("gone".into());
    reg2.install(&other).unwrap();
    assert!(reg2.take_store_warning().unwrap().contains("gone"));
}

#[test]
fn a_stored_module_is_checked_again_with_the_limits_of_now() {
    let mem = Arc::new(MemoryBackend::new());
    let mut first = Registry::default();
    first.attach(mem.clone(), 2 << 20, 2 << 20);
    first.install(PLANTER).unwrap();
    // A host with stricter limits refuses the stored module and says why.
    let mut strict = Registry::new(
        crate::default_runtime(),
        Limits {
            max_functions: 1,
            ..Limits::default()
        },
    );
    let report = strict.attach(mem, 2 << 20, 2 << 20);
    assert!(strict.is_empty() && report.loaded.is_empty());
    assert!(report.rejected[0].reason.contains("functions"));
}

#[test]
fn a_stored_module_is_listed_without_running_and_checked_when_it_is_first_needed() {
    let mem = Arc::new(MemoryBackend::new());
    let mut first = Registry::default();
    first.attach(mem.clone(), 2 << 20, 2 << 20);
    first.install(PLANTER).unwrap();
    let mut next = Registry::default();
    next.attach(mem.clone(), 2 << 20, 2 << 20);
    let p = next.get("org.kadakas.raised-planter").unwrap();
    assert_eq!(p.manifest().name, "Raised planter");
    assert!(!p.is_active(), "listing must not compile or run the module");
    // A refused call (no trust) does not run it either.
    let other = Permit::for_test("not this module");
    assert!(p.reset(&other, &Map::new(), &rect(2000.0, 800.0)).is_err());
    assert!(!p.is_active());
    // A trusted call activates it, and the answer is the same as for a fresh install.
    let hash = p.hash().to_string();
    let permit = ModuleTrust::in_memory().permit(&hash, true).unwrap();
    assert_eq!(
        p.reset(&permit, &Map::new(), &rect(2000.0, 800.0))
            .unwrap()
            .len(),
        3
    );
    assert!(p.is_active());
}

#[test]
fn a_changed_stored_description_stops_the_module() {
    let mem = Arc::new(MemoryBackend::new());
    let mut first = Registry::default();
    first.attach(mem.clone(), 2 << 20, 2 << 20);
    first.install(PLANTER).unwrap();
    let hash = trust::hash(PLANTER);
    // Someone edits the record: another name.
    let record = String::from_utf8(
        mem.read(&format!("{hash}.manifest.json"), 1 << 20)
            .unwrap()
            .unwrap(),
    )
    .unwrap()
    .replace("Raised planter", "Something else");
    mem.write(&format!("{hash}.manifest.json"), record.as_bytes())
        .unwrap();
    let mut next = Registry::default();
    next.attach(mem.clone(), 2 << 20, 2 << 20);
    let p = next.get("org.kadakas.raised-planter").unwrap();
    assert_eq!(p.manifest().name, "Something else");
    let permit = ModuleTrust::in_memory().permit(&hash, true).unwrap();
    let e = p
        .reset(&permit, &Map::new(), &rect(2000.0, 800.0))
        .unwrap_err();
    assert!(matches!(e, PluginError::Invalid(m) if m.contains("stored description")));
    assert!(!p.is_active() && p.activate().is_err());
    // A record that is not a manifest, or too big, is a stored problem, not a crash.
    mem.write(&format!("{hash}.manifest.json"), b"{").unwrap();
    let mut broken = Registry::default();
    let report = broken.attach(mem.clone(), 2 << 20, 2 << 20);
    assert!(broken.is_empty() && report.rejected.len() == 1);
    mem.write(&format!("{hash}.manifest.json"), &vec![b' '; 300_000])
        .unwrap();
    let mut huge = Registry::default();
    assert_eq!(huge.attach(mem, 2 << 20, 2 << 20).rejected.len(), 1);
}
