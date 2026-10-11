//! The probe modules of the security review are kept as text (`tests/fixtures/probes/*.wat`) and
//! as binaries (`*.wasm`, for the browser test in the `kadakas-server` repository, which has no
//! WebAssembly text compiler). This test keeps the two in step:
//! `UPDATE_PROBES=1 cargo test -p kadakas-plugin probe_binaries` writes the binaries again.

use std::path::Path;

#[test]
fn probe_binaries_are_the_compiled_text() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/probes");
    let update = std::env::var_os("UPDATE_PROBES").is_some();
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "wat") {
            continue;
        }
        let compiled = wat::parse_file(&path).unwrap();
        let binary = path.with_extension("wasm");
        if update {
            std::fs::write(&binary, &compiled).unwrap();
        }
        assert_eq!(
            std::fs::read(&binary).unwrap(),
            compiled,
            "{} is not the compiled text. Run with UPDATE_PROBES=1",
            binary.display()
        );
        // Every probe is refused by the host, whatever the engine.
        assert!(
            crate::plugin::scan(&compiled, &crate::Limits::default()).is_err()
                || crate::harden::harden(
                    &compiled,
                    &crate::harden::HardenLimits {
                        fuel: 1000,
                        memory_bytes: 1 << 20
                    }
                )
                .is_err(),
            "{} was not refused",
            path.display()
        );
        n += 1;
    }
    assert!(n >= 5);
}

#[test]
fn the_endless_loop_module_is_valid_and_both_engines_stop_it() {
    use crate::metered::MeteredRuntime;
    use serde_json::Map;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/modules");
    let text = dir.join("endless_loop.wat");
    let compiled = wat::parse_file(&text).unwrap();
    let binary = text.with_extension("wasm");
    if std::env::var_os("UPDATE_PROBES").is_some() {
        std::fs::write(&binary, &compiled).unwrap();
    }
    assert_eq!(std::fs::read(&binary).unwrap(), compiled);
    let limits = crate::Limits {
        fuel: 1_000_000,
        ..crate::Limits::default()
    };
    let geometry = kadakas_plugin_abi::Geometry::Point {
        at: [0.0, 0.0],
        rotation: 0.0,
    };
    let native =
        crate::Plugin::load(&crate::native::WasmtimeRuntime::new(), &compiled, &limits).unwrap();
    let permit = crate::trust::ModuleTrust::in_memory()
        .permit(native.hash(), true)
        .unwrap();
    let e = native.reset(&permit, &Map::new(), &geometry).unwrap_err();
    assert!(matches!(e, crate::PluginError::Limit(_)), "{e}");
    // The browser engine, on the test compiler, stops it with its own fuel.
    let metered = crate::Plugin::load(
        &MeteredRuntime::new(crate::tests::limits_tests::plain_compiler()),
        &compiled,
        &limits,
    )
    .unwrap();
    let e = metered.reset(&permit, &Map::new(), &geometry).unwrap_err();
    assert!(
        matches!(&e, crate::PluginError::Limit(m) if m.contains("fuel")),
        "{e}"
    );
}
