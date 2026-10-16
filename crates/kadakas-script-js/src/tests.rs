//! The scenarios of `scenarios.rs`, once on the native engine (wasmtime: exact fuel, a deadline,
//! a memory cap) and once on the metered engine of the browser (a rewrite of the module that
//! counts its own fuel and caps its own memory), run on the desktop with a plain compiler. The
//! same script must give the same answer on both.

use super::*;

mod native {
    use super::*;

    fn engine(limits: ScriptLimits) -> ScriptEngine {
        ScriptEngine::new(limits).unwrap()
    }

    include!("scenarios.rs");
}

mod metered {
    use super::*;
    use kadakas_plugin::metered::MeteredRuntime;
    use kadakas_plugin::testkit::PlainCompiler;

    fn engine(limits: ScriptLimits) -> ScriptEngine {
        ScriptEngine::with_runtime(&MeteredRuntime::new(PlainCompiler::new()), limits).unwrap()
    }

    include!("scenarios.rs");
}

/// The same scripts on both engines give the same result and the same log.
#[test]
fn both_engines_give_the_same_answers() {
    use kadakas_plugin::metered::MeteredRuntime;
    use kadakas_plugin::testkit::PlainCompiler;
    let scripts = [
        "return 1 + 1;",
        "const a = []; for (let i = 0; i < 50; i++) a.push(Math.random()); console.log('x'); return a;",
        "return [JSON.stringify({a: [1, 2, {b: 3}]}), 'é'.length, [3, 1, 2].sort(), 0.1 + 0.2, 2 ** 60];",
        "const p = kadakas.pointsFrom([[1, 2], [3, 4]]); return kadakas.call('echo', {p}).params.p;",
        "return Array.from({length: 5}, (_, i) => i * i).map(String).join(',');",
    ];
    let limits = ScriptLimits {
        fuel: 500_000_000,
        memory_bytes: 48 << 20,
        ..ScriptLimits::native()
    };
    let native = ScriptEngine::new(limits).unwrap();
    let metered =
        ScriptEngine::with_runtime(&MeteredRuntime::new(PlainCompiler::new()), limits).unwrap();
    struct H;
    impl ScriptHost for H {
        fn call(&mut self, m: &str, p: Value, _: bool) -> Result<Value, HostError> {
            Ok(json!({"method": m, "params": p}))
        }
    }
    for code in scripts {
        let go = |e: &ScriptEngine| {
            e.run(
                &Request {
                    code,
                    args: &json!(null),
                    buffers: &[],
                    seed: 11,
                },
                &mut H,
            )
            .unwrap()
        };
        assert_eq!(go(&native), go(&metered), "{code}");
    }
}
