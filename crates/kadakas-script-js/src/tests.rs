use super::*;

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
    let engine = ScriptEngine::new(ScriptLimits::native()).unwrap();
    let mut host = Echo { seen: vec![] };
    engine.run(
        &Request {
            code,
            args: &json!({"a": 1}),
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
