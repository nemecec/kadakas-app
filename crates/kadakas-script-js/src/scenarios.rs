
struct Echo {
    seen: Vec<(String, Value)>,
}

impl ScriptHost for Echo {
    fn call(&mut self, method: &str, params: Value, _typed: bool) -> Result<Value, HostError> {
        self.seen.push((method.to_string(), params.clone()));
        match method {
            "fail" => Err(HostError::Failure(CallFailure {
                code: 3,
                message: "no way".into(),
                data: Some(json!({"why": "test"})),
            })),
            "stop" => Err(HostError::Stop("the host said stop".into())),
            _ => Ok(json!({"method": method, "params": params})),
        }
    }
}

fn run(code: &str) -> Result<Output, Failure> {
    let engine = engine(ScriptLimits::native());
    let mut host = Echo { seen: vec![] };
    engine.run(
        &Request {
            code,
            args: &json!({"a": 1}),
            buffers: &[],
            seed: 0,
        },
        &mut host,
    )
}

#[test]
fn a_script_returns_a_value_and_sees_its_arguments() {
    let out = run("return {sum: args.a + 41};").unwrap();
    assert_eq!(out.value, json!({"sum": 42}));
    assert_eq!(out.calls, 0);
}

fn run_with(limits: ScriptLimits, code: &str) -> (Result<Output, Failure>, Vec<(String, Value)>) {
    let engine = engine(limits);
    let mut host = Echo { seen: vec![] };
    let r = engine.run(
        &Request {
            code,
            args: &json!(null),
            buffers: &[],
            seed: 7,
        },
        &mut host,
    );
    (r, host.seen)
}

fn small() -> ScriptLimits {
    ScriptLimits {
        fuel: 500_000_000,
        memory_bytes: 48 << 20,
        output_bytes: 1 << 20,
        ..ScriptLimits::native()
    }
}

#[test]
fn a_call_reaches_the_host_with_its_parameters_and_returns_the_answer() {
    let (r, seen) = run_with(
        small(),
        r#"const r = kadakas.call("objects.list", {layer: 2, tags: ["a", "b"]});
           return r.params.tags.length + r.params.layer;"#,
    );
    let out = r.unwrap();
    assert_eq!(out.value, json!(4));
    assert_eq!(out.calls, 1);
    assert_eq!(
        seen,
        vec![("objects.list".to_string(), json!({"layer": 2, "tags": ["a", "b"]}))]
    );
}

#[test]
fn a_failed_call_is_an_error_that_the_script_can_catch() {
    let (r, _) = run_with(
        small(),
        r#"try { kadakas.call("fail", {}); } catch (e) { return [e.message, e.code, e.data.why, e.method]; }"#,
    );
    assert_eq!(r.unwrap().value, json!(["no way", 3, "test", "fail"]));
}

#[test]
fn an_error_that_is_not_caught_fails_the_run_and_names_the_method() {
    let (r, _) = run_with(small(), r#"kadakas.call("fail", {});"#);
    let f = r.unwrap_err();
    match &f.error {
        ScriptError::Script {
            message,
            method,
            code,
            ..
        } => {
            assert_eq!(message, "no way");
            assert_eq!(method.as_deref(), Some("fail"));
            assert_eq!(*code, Some(3));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(f.calls, 1);
}

#[test]
fn a_syntax_error_and_a_thrown_value_are_script_errors_with_the_log() {
    let (r, _) = run_with(small(), "console.log('before'); return (;");
    let f = r.unwrap_err();
    assert!(matches!(&f.error, ScriptError::Script { name, .. } if name == "SyntaxError"), "{f:?}");
    let (r, _) = run_with(small(), "console.log('before'); throw 42;");
    let f = r.unwrap_err();
    assert!(matches!(&f.error, ScriptError::Script { message, .. } if message.contains("42")));
    assert_eq!(f.log, "before\n");
}

#[test]
fn the_host_can_stop_the_run_and_the_script_cannot_catch_it() {
    let (r, _) = run_with(small(), r#"try { kadakas.call("stop", {}); } catch (e) { return "caught"; } return "went on";"#);
    let f = r.unwrap_err();
    assert_eq!(f.error, ScriptError::Limit("the host said stop".into()));
}

#[test]
fn an_endless_loop_is_stopped_by_the_fuel() {
    let limits = ScriptLimits {
        fuel: 20_000_000,
        ..small()
    };
    let (r, _) = run_with(limits, "for (;;) {}");
    match r.unwrap_err().error {
        ScriptError::Limit(m) => assert!(m.contains("fuel"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_memory_bomb_is_stopped() {
    let (r, _) = run_with(
        small(),
        "const keep = []; for (;;) keep.push(new Array(100000).fill(1));",
    );
    let e = r.unwrap_err().error;
    assert!(matches!(&e, ScriptError::Limit(_)), "{e:?}");
    let (r, _) = run_with(small(), "return new ArrayBuffer(1 << 30).byteLength;");
    let e = r.unwrap_err().error;
    assert!(matches!(&e, ScriptError::Limit(_)), "{e:?}");
    let (r, _) = run_with(small(), r#"let s = "x"; for (;;) s += s;"#);
    let e = r.unwrap_err().error;
    assert!(matches!(&e, ScriptError::Limit(_)), "{e:?}");
}

#[test]
fn deep_recursion_is_a_limit_not_a_crash() {
    let (r, _) = run_with(small(), "function f(n) { return f(n + 1) + 1; } return f(0);");
    let e = r.unwrap_err().error;
    assert!(matches!(&e, ScriptError::Limit(_)), "{e:?}");
}

#[test]
fn a_result_over_the_output_size_is_refused() {
    let limits = ScriptLimits {
        output_bytes: 100_000,
        ..small()
    };
    let (r, _) = run_with(limits, r#"return "x".repeat(500000);"#);
    match r.unwrap_err().error {
        ScriptError::Limit(m) => assert!(m.contains("answer"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_log_is_bounded_and_says_how_many_lines_did_not_fit() {
    let (r, _) = run_with(
        small(),
        r#"for (let i = 0; i < 20000; i++) console.log("line " + i + " of the log"); return 1;"#,
    );
    let out = r.unwrap();
    assert!(out.log.len() <= 70_000, "{}", out.log.len());
    assert!(out.log.starts_with("line 0 of the log\n"));
    assert!(out.log.contains("more lines were not kept]"), "{}", &out.log[out.log.len() - 80..]);
}

#[test]
fn the_number_of_calls_is_limited() {
    let limits = ScriptLimits {
        max_calls: 5,
        ..small()
    };
    let (r, seen) = run_with(limits, r#"for (let i = 0; i < 100; i++) kadakas.call("x", {});"#);
    assert!(matches!(r.unwrap_err().error, ScriptError::Limit(m) if m.contains("5 calls")));
    assert_eq!(seen.len(), 5);
}

#[test]
fn a_run_is_deterministic_and_has_no_clock() {
    let code = r#"const r = []; for (let i = 0; i < 5; i++) r.push(Math.random());
                  return {r, now: Date.now(), year: new Date().getFullYear()};"#;
    let (a, _) = run_with(small(), code);
    let (b, _) = run_with(small(), code);
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.value, b.value, "the same script gives the same result");
    assert_eq!(a.value["now"], json!(0), "the clock always says 0");
    assert!(a.value["r"][0].as_f64().unwrap() < 1.0);
}

#[test]
fn a_script_has_no_files_and_no_network() {
    let (r, _) = run_with(
        small(),
        "return [typeof require, typeof std, typeof os, typeof fetch, typeof process, typeof XMLHttpRequest, typeof print];",
    );
    assert_eq!(r.unwrap().value, json!(vec!["undefined"; 7]));
}

#[test]
fn typed_arrays_go_out_as_buffers_and_come_back_as_the_same_numbers() {
    let (r, seen) = run_with(
        small(),
        r#"const p = kadakas.pointsFrom([[0, 0], [10, 0], [10, 5.5]]);
           const echoed = kadakas.call("echo", {points: p, name: "tri"}, {typed: false});
           return {n: p.length, stride: p.stride, echoed: echoed.params.points};"#,
    );
    let out = r.unwrap();
    assert_eq!(seen[0].1["points"], json!([[0.0, 0.0], [10.0, 0.0], [10.0, 5.5]]));
    assert_eq!(out.value, json!({"n": 6, "stride": 2, "echoed": [[0, 0], [10, 0], [10, 5.5]]}));
}

#[test]
fn a_typed_answer_arrives_as_a_typed_array_with_its_stride() {
    struct Ring;
    impl ScriptHost for Ring {
        fn call(&mut self, _m: &str, _p: Value, typed: bool) -> Result<Value, HostError> {
            assert!(typed);
            let pts: Vec<Value> = (0..10).map(|i| json!([i as f64, 2.0 * i as f64])).collect();
            Ok(json!({"ring": pts}))
        }
    }
    let engine = engine(small());
    let out = engine
        .run(
            &Request {
                code: r#"const r = kadakas.call("ring", {}, {typed: true}).ring;
                         return {kind: r.constructor.name, len: r.length, stride: r.stride, last: r[19], same: r};"#,
                args: &json!(null),
                buffers: &[],
                seed: 0,
            },
            &mut Ring,
        )
        .unwrap();
    assert_eq!(out.value["kind"], json!("Float64Array"));
    assert_eq!(out.value["len"], json!(20));
    assert_eq!(out.value["stride"], json!(2));
    assert_eq!(out.value["last"].as_f64(), Some(18.0));
    // The returned typed array leaves as a buffer, and `json()` writes it out again.
    assert_eq!(out.buffers.len(), 1);
    let j = out.json().unwrap();
    assert_eq!(j["same"][9], json!([9.0, 18.0]));
}

#[test]
fn a_new_engine_per_run_leaves_no_state_between_runs() {
    let engine = engine(small());
    let mut host = Echo { seen: vec![] };
    let mut go = |code: &str| {
        engine.run(
            &Request {
                code,
                args: &json!(null),
                buffers: &[],
                seed: 0,
            },
            &mut host,
        )
    };
    assert_eq!(go("globalThis.leak = 1; return 1;").unwrap().value, json!(1));
    assert_eq!(go("return typeof globalThis.leak;").unwrap().value, json!("undefined"));
}

#[test]
fn typed_arrays_can_be_arguments() {
    let engine = engine(small());
    let mut host = Echo { seen: vec![] };
    let ring: Vec<u8> = [0.0f64, 0.0, 4.0, 0.0, 4.0, 3.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let out = engine
        .run(
            &Request {
                code: "const r = args.ring; return {n: r.length, stride: r.stride, last: r[5], tag: args.tag};",
                args: &json!({"ring": {"$t": "f64", "i": 0, "n": 2}, "tag": "x"}),
                buffers: &[ring],
                seed: 0,
            },
            &mut host,
        )
        .unwrap();
    assert_eq!(out.value, json!({"n": 6, "stride": 2, "last": 3, "tag": "x"}));
}
