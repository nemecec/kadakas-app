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

mod unit {
    use super::*;
    use crate::typed;

    #[test]
    fn the_limits_of_the_browser_are_smaller_and_the_default_follows_the_target() {
        let (n, b) = (ScriptLimits::native(), ScriptLimits::browser());
        assert!(b.fuel < n.fuel);
        assert_eq!(ScriptLimits::default(), n);
        assert!(ScriptEngine::built_in());
        assert_eq!(
            ScriptEngine::new(n).unwrap().limits(),
            &n,
            "the engine keeps its limits"
        );
    }

    #[test]
    fn errors_read_well() {
        let throw = ScriptError::Script(Box::new(ScriptThrow {
            message: "boom".into(),
            name: "Error".into(),
            stack: "    at f (<eval>:1:5)".into(),
            method: Some("objects.get".into()),
            code: Some(2),
            data: None,
        }));
        let text = throw.to_string();
        assert!(text.contains("failed at objects.get: boom"), "{text}");
        assert!(text.contains("<eval>:1:5"), "{text}");
        let plain = ScriptError::Script(Box::new(ScriptThrow {
            message: "x".into(),
            name: String::new(),
            stack: String::new(),
            method: None,
            code: None,
            data: None,
        }));
        assert_eq!(plain.to_string(), "the script failed: x");
        assert!(
            ScriptError::Limit("fuel".into())
                .to_string()
                .contains("over a limit")
        );
        assert!(
            ScriptError::Trap("t".into())
                .to_string()
                .contains("crashed")
        );
        assert_eq!(ScriptError::Unavailable("no".into()).to_string(), "no");
        let f = Failure::from(ScriptError::Limit("x".into()));
        assert_eq!((f.log.as_str(), f.calls), ("", 0));
    }

    #[test]
    fn an_engine_that_cannot_run_hosted_modules_is_unavailable() {
        struct Bare;
        impl kadakas_plugin::PluginRuntime for Bare {
            fn name(&self) -> &'static str {
                "bare"
            }
            fn load(
                &self,
                _: &[u8],
                _: &kadakas_plugin::Limits,
            ) -> Result<Box<dyn kadakas_plugin::LoadedModule>, PluginError> {
                unreachable!()
            }
        }
        let e = ScriptEngine::with_runtime(&Bare, ScriptLimits::native())
            .err()
            .unwrap();
        assert!(matches!(e, ScriptError::Unavailable(_)), "{e:?}");
        struct Broken;
        impl kadakas_plugin::PluginRuntime for Broken {
            fn name(&self) -> &'static str {
                "broken"
            }
            fn load(
                &self,
                _: &[u8],
                _: &kadakas_plugin::Limits,
            ) -> Result<Box<dyn kadakas_plugin::LoadedModule>, PluginError> {
                unreachable!()
            }
            fn load_hosted(
                &self,
                _: &[u8],
                _: &kadakas_plugin::Limits,
            ) -> Result<Box<dyn HostedModule>, PluginError> {
                Err(PluginError::Invalid("bad".into()))
            }
        }
        let e = ScriptEngine::with_runtime(&Broken, ScriptLimits::native())
            .err()
            .unwrap();
        assert!(matches!(e, ScriptError::Trap(m) if m.contains("bad")));
        // A limit or an unsupported engine become the matching run errors.
        assert!(matches!(
            plugin_failure(PluginError::Unsupported("u".into())).error,
            ScriptError::Unavailable(_)
        ));
        assert!(matches!(
            plugin_failure(PluginError::Limit("l".into())).error,
            ScriptError::Limit(_)
        ));
    }

    struct Nothing;
    impl ScriptHost for Nothing {
        fn call(&mut self, _: &str, _: Value, _: bool) -> Result<Value, HostError> {
            Ok(json!({"seen": true}))
        }
    }

    fn reply(r: HostReply) -> Vec<Vec<u8>> {
        match r {
            HostReply::Reply(b) => frame::decode(&b)
                .unwrap()
                .into_iter()
                .map(<[u8]>::to_vec)
                .collect(),
            HostReply::Abort(e) => panic!("{e:?}"),
        }
    }

    #[test]
    fn a_broken_call_gets_an_error_reply_not_a_crash() {
        let bad = |parts: &[&[u8]]| {
            let r = reply(serve_call(&frame::encode(parts), &mut Nothing, 1 << 20));
            assert_eq!(r[0], b"err");
            String::from_utf8(r[1].clone()).unwrap()
        };
        assert!(bad(&[]).contains("broken"));
        assert!(bad(&[b"only one part"]).contains("broken"));
        assert!(bad(&[&[0xff, 0xfe], b"{}"]).contains("not text"));
        assert!(bad(&[b"m", b"{not json"]).contains("not JSON"));
        assert!(bad(&[b"m", br#"{"x": {"$kt": "f64", "$ki": 4}}"#]).contains("not there"));
        let ok = reply(serve_call(
            &frame::encode(&[b"m", b"{}"]),
            &mut Nothing,
            1 << 20,
        ));
        assert_eq!(ok[0], b"ok");
    }

    #[test]
    fn a_broken_answer_is_a_trap_and_the_status_decides() {
        let f = |parts: &[&[u8]]| parse_answer(&frame::encode(parts));
        assert!(matches!(
            f(&[b"ok"]).unwrap_err().error,
            ScriptError::Trap(_)
        ));
        assert!(matches!(
            f(&[b"ok", b"{not json", b"0", b""]).unwrap_err().error,
            ScriptError::Trap(m) if m.contains("not JSON")
        ));
        let fatal = f(&[b"fatal", b"why", b"3", b"log"]).unwrap_err();
        assert!(matches!(fatal.error, ScriptError::Trap(m) if m.contains("why")));
        assert_eq!((fatal.log.as_str(), fatal.calls), ("log", 3));
        let calls = f(&[
            b"limit",
            br#"{"message": "the script made more than 5 calls"}"#,
            b"5",
            b"",
        ])
        .unwrap_err();
        assert!(matches!(calls.error, ScriptError::Limit(_)));
        let oom = f(&[b"limit", b"{\"message\":\"\"}", b"0", b""]).unwrap_err();
        assert!(matches!(&oom.error, ScriptError::Limit(m) if m == "out of memory"));
        // The text of a message decides nothing: the module says what is a limit.
        let text_only = f(&[
            b"error",
            br#"{"message": "out of memory", "name": "InternalError"}"#,
            b"0",
            b"",
        ])
        .unwrap_err();
        assert!(matches!(text_only.error, ScriptError::Script(_)));
        let not_json = f(&[b"error", b"???", b"0", b""]).unwrap_err();
        assert!(matches!(not_json.error, ScriptError::Script(_)));
    }

    #[test]
    fn typed_arrays_of_every_kind_and_the_odd_cases() {
        let f32s: Vec<u8> = [1.5f32, -2.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let i32s: Vec<u8> = [-7i32, 9].iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut v =
            json!([{"$kt": "f32", "$ki": 0}, {"$kt": "i32", "$ki": 1}, {"$kt": "u8", "$ki": 2}]);
        typed::resolve(&mut v, &[f32s, i32s, vec![1, 2, 255]]).unwrap();
        assert_eq!(v, json!([[1.5, -2.0], [-7, 9], [1, 2, 255]]));
        let mut cut = json!({"$kt": "f32", "$ki": 0});
        assert!(
            typed::resolve(&mut cut, &[vec![0u8; 5]]).is_err(),
            "5 bytes of 4-byte numbers"
        );
        // Pairs and triples are lists of points; a list of 4-tuples is not.
        let pts3: Vec<Value> = (0..9).map(|i| json!([i, i, i])).collect();
        let mut v =
            json!({"p": pts3, "q": (0..9).map(|i| json!([i, i, i, i])).collect::<Vec<_>>()});
        let bufs = typed::extract(&mut v);
        assert_eq!(v["p"], json!({"$kt": "f64", "$ki": 0, "$kn": 3}));
        assert!(v["q"].is_array());
        assert_eq!(bufs.len(), 1);
    }
}

mod caps {
    use super::*;

    struct Nothing;
    impl ScriptHost for Nothing {
        fn call(&mut self, _: &str, _: Value, _: bool) -> Result<Value, HostError> {
            Ok(json!(1))
        }
    }

    struct Big;
    impl ScriptHost for Big {
        fn call(&mut self, _: &str, _: Value, _: bool) -> Result<Value, HostError> {
            Ok(json!({"text": "x".repeat(10_000)}))
        }
    }

    #[test]
    fn a_request_and_a_reply_over_the_cap_are_refused_and_so_are_too_many_numbers() {
        let ok = |r: HostReply| {
            frame::decode(&match r {
                HostReply::Reply(b) => b,
                HostReply::Abort(e) => panic!("{e:?}"),
            })
            .unwrap()[0]
                .to_vec()
        };
        assert_eq!(
            ok(serve_call(
                &frame::encode(&[b"m", b"{}"]),
                &mut Big,
                1 << 20
            )),
            b"ok"
        );
        // The reply has 10,000 bytes and the cap is 1,000.
        assert_eq!(
            ok(serve_call(&frame::encode(&[b"m", b"{}"]), &mut Big, 1000)),
            b"err"
        );
        // The request has more bytes than the cap.
        let long = vec![b'x'; 2000];
        assert_eq!(
            ok(serve_call(&frame::encode(&[b"m", &long]), &mut Big, 1000)),
            b"err"
        );
        // 2,000 numbers of 8 bytes: with a cap of 1,000 bytes, at most 62 numbers may be written out.
        let nums = vec![0u8; 16_000];
        let req = frame::encode(&[b"m", br#"{"x": {"$kt": "f64", "$ki": 0}}"#, &nums]);
        assert_eq!(ok(serve_call(&req, &mut Big, 1 << 16)), b"ok");
        let r = serve_call(&req, &mut Big, 16_100);
        assert_eq!(ok(r), b"err");
    }

    #[test]
    fn the_number_of_calls_is_bounded_by_the_host_not_only_by_the_script() {
        let mut host = Nothing;
        let mut server = Server {
            host: &mut host,
            calls: 0,
            max_calls: 2,
            cap: 1 << 20,
        };
        let req = frame::encode(&[b"m", b"{}"]);
        assert!(matches!(server.serve(&req), HostReply::Reply(_)));
        assert!(matches!(server.serve(&req), HostReply::Reply(_)));
        assert!(matches!(
            server.serve(&req),
            HostReply::Abort(PluginError::Limit(_))
        ));
    }
}
