use std::time::Duration;

use serde_json::{Map, Value, json};

use super::*;
use crate::python;
use crate::trust::{Permit, TrustStore};

fn def(script: &str) -> PluginDef {
    PluginDef {
        shape_type: ShapeType::Point,
        params: vec![ParamDef {
            name: "n".into(),
            label: String::new(),
            ty: ParamType::Number,
            default: json!(3.0),
            min: Some(1.0),
            max: Some(50.0),
            options: vec![],
            fields: vec![],
        }],
        script: script.into(),
        description: String::new(),
        timeout_ms: None,
    }
}

fn point() -> Geometry {
    Geometry::Point {
        at: [10.0, 20.0],
        rotation: 0.0,
    }
}

fn run(script: &str, limits: ResetLimits) -> Result<Vec<Generated>, ResetError> {
    let py = python::discover().expect("python3 is needed for the tests");
    let d = def(script);
    let params = d.resolve_params(&Map::new()).unwrap();
    reset(
        &Permit::for_test(script),
        &py,
        "Stub",
        &d,
        &params,
        &point(),
        &limits,
    )
}

fn quick() -> ResetLimits {
    ResetLimits {
        timeout: Duration::from_secs(10),
        ..ResetLimits::default()
    }
}

fn err(script: &str, limits: ResetLimits) -> ResetError {
    run(script, limits).unwrap_err()
}

#[test]
fn a_stub_script_gets_params_and_geometry_and_returns_shapes() {
    let shapes = run(
        "def reset(params, geometry):\n    n = int(params.n)\n    print('this goes to stderr')\n    return [circle(geometry.at, 100 * n, fill='#ff0000'), line([0, 0], [n, n]), text([0, 0], 'hi', 50)]\n",
        quick(),
    )
    .unwrap();
    assert_eq!(shapes.len(), 3);
    assert_eq!(
        shapes[0].shape,
        Shape::Circle {
            center: [10.0, 20.0].into(),
            radius: 300.0
        }
    );
    assert_eq!(shapes[0].attrs.fill.unwrap().0, [255, 0, 0, 255]);
    assert!(matches!(shapes[2].shape, Shape::Text { .. }));
}

#[test]
fn a_script_error_is_reported_with_its_traceback() {
    match err("def reset(p, g):\n    return 1 / 0\n", quick()) {
        ResetError::Script { error, trace } => {
            assert!(error.contains("ZeroDivisionError"), "{error}");
            assert!(trace.contains("plug-in Stub"), "{trace}");
        }
        e => panic!("{e:?}"),
    }
    assert!(matches!(
        err("def reset(p, g:\n", quick()),
        ResetError::Script { error, .. } if error.contains("SyntaxError")
    ));
    let e = err("x = 1\n", quick());
    assert!(e.to_string().contains("must define reset"), "{e}");
}

#[test]
fn a_slow_script_times_out() {
    let limits = ResetLimits {
        timeout: Duration::from_millis(300),
        ..ResetLimits::default()
    };
    let t = std::time::Instant::now();
    let e = err(
        "import time\ndef reset(p, g):\n    time.sleep(60)\n",
        limits,
    );
    assert!(
        matches!(e, ResetError::Exec(ExecError::Timeout(_))),
        "{e:?}"
    );
    assert!(t.elapsed() < Duration::from_secs(8));
    assert!(e.to_string().contains("300 ms"));
}

#[test]
fn a_script_that_dies_gives_a_protocol_error() {
    let e = err("import os\ndef reset(p, g):\n    os._exit(7)\n", quick());
    assert!(
        matches!(&e, ResetError::Protocol(m) if m.contains("exit code 7")),
        "{e:?}"
    );
}

#[test]
fn big_answers_are_refused() {
    let limits = ResetLimits {
        max_output: 2000,
        ..quick()
    };
    let e = err(
        "def reset(p, g):\n    return [circle([i, 0], 1) for i in range(500)]\n",
        limits,
    );
    assert!(
        matches!(e, ResetError::Exec(ExecError::OutputTooLarge(2000))),
        "{e:?}"
    );
}

#[test]
fn too_many_shapes_and_vertices_are_refused() {
    let limits = ResetLimits {
        max_shapes: 10,
        max_vertices: 20,
        ..quick()
    };
    let e = err(
        "def reset(p, g):\n    return [circle([i, 0], 1) for i in range(11)]\n",
        limits,
    );
    assert!(
        matches!(&e, ResetError::Shapes(m) if m.contains("11 shapes")),
        "{e:?}"
    );
    let e = err(
        "def reset(p, g):\n    return [polyline([[0, 0], [i, 1], [i, 2]]) for i in range(8)]\n",
        limits,
    );
    assert!(
        matches!(&e, ResetError::Shapes(m) if m.contains("vertices")),
        "{e:?}"
    );
}

#[test]
fn bad_shapes_are_refused() {
    for (script, needle) in [
        ("def reset(p, g):\n    return []\n", "no shapes"),
        ("def reset(p, g):\n    return None\n", "no shapes"),
        (
            "def reset(p, g):\n    return [{'kind': 'plant', 'at': [0, 0]}]\n",
            "not allowed",
        ),
        (
            "def reset(p, g):\n    return [{'kind': 'group', 'name': 'x'}]\n",
            "not allowed",
        ),
        (
            "def reset(p, g):\n    return [{'kind': 'circle'}]\n",
            "shape 0",
        ),
        ("def reset(p, g):\n    return [5]\n", "must be an object"),
        (
            "def reset(p, g):\n    return [{'kind': 'circle', 'center': [0, 0], 'radius': 1e12}]\n",
            "too large",
        ),
        (
            "def reset(p, g):\n    return [{'kind': 'line', 'a': [0, 0], 'b': [1, 1], 'attrs': {'pen': 5}}]\n",
            "attrs",
        ),
        (
            "def reset(p, g):\n    return [circle([0, 0], 0)]\n",
            "radius",
        ),
        (
            "def reset(p, g):\n    return [circle([0, 0], -5)]\n",
            "radius",
        ),
        (
            "def reset(p, g):\n    return [arc([0, 0], -1, 0, 1)]\n",
            "radius",
        ),
        (
            "def reset(p, g):\n    return [text([0, 0], 'x' * 10001, 10)]\n",
            "at most",
        ),
        (
            "def reset(p, g):\n    return [text([0, 0], 'x', 0)]\n",
            "text height",
        ),
    ] {
        let e = err(script, quick());
        assert!(e.to_string().contains(needle), "{script} -> {e}");
    }
    // NaN never reaches the app: the runner refuses it.
    let e = err(
        "def reset(p, g):\n    return [circle([float('nan'), 0], 1)]\n",
        quick(),
    );
    assert!(matches!(e, ResetError::Script { .. }), "{e:?}");
}

#[test]
fn the_helpers_walk_and_rotate() {
    let shapes = run(
        "def reset(p, g):\n    pts = list(walk([[0, 0], [1000, 0], [1000, 1000]], 400))\n    assert len(pts) == 6, pts\n    assert abs(path_length([[0, 0], [3, 4]]) - 5) < 1e-9\n    r = rotate([1, 0], math.pi / 2)\n    return [circle([x, y], 10) for x, y, a in pts] + [circle(r, 1)]\n",
        quick(),
    )
    .unwrap();
    assert_eq!(shapes.len(), 7);
}

#[test]
fn the_runner_isolates_the_script_from_user_modules() {
    let e = run(
        "import kadakas_not_installed\ndef reset(p, g):\n    return []\n",
        quick(),
    );
    assert!(matches!(e, Err(ResetError::Script { .. })));
}

#[test]
fn params_are_checked_and_get_defaults() {
    let d = def("");
    let m = |v: Value| v.as_object().unwrap().clone();
    assert_eq!(d.resolve_params(&Map::new()).unwrap()["n"], json!(3.0));
    assert_eq!(
        d.resolve_params(&m(json!({"n": 7}))).unwrap()["n"],
        json!(7.0)
    );
    for bad in [
        json!({"n": 0}),
        json!({"n": 51}),
        json!({"n": "x"}),
        json!({"x": 1}),
        json!({"n": null}),
    ] {
        assert!(d.resolve_params(&m(bad.clone())).is_err(), "{bad}");
    }
    assert!(
        d.resolve_params(&m(json!({"x": 1})))
            .unwrap_err()
            .contains("Parameters: n")
    );
}

#[test]
fn every_param_type_is_checked() {
    let p = |v: Value| -> ParamDef { serde_json::from_value(v).unwrap() };
    let text = p(json!({"name": "t", "type": "text"}));
    assert_eq!(text.coerce(&json!("ab")).unwrap(), json!("ab"));
    assert!(text.coerce(&json!(1)).is_err());
    assert!(text.coerce(&json!("x".repeat(MAX_TEXT + 1))).is_err());
    let b = p(json!({"name": "b", "type": "bool"}));
    assert_eq!(b.default_value(), json!(false));
    assert!(b.coerce(&json!(1)).is_err());
    let c = p(json!({"name": "c", "type": "choice", "options": ["a", "b"]}));
    assert_eq!(c.default_value(), json!("a"));
    assert!(c.coerce(&json!("z")).unwrap_err().contains("a, b"));
    let col = p(json!({"name": "c", "type": "color"}));
    assert_eq!(col.coerce(&json!("#AABBCC")).unwrap(), json!("#aabbcc"));
    assert!(col.coerce(&json!("red")).is_err());
    assert!(col.coerce(&json!(5)).is_err());
    let len = p(json!({"name": "l", "type": "length", "min": 5, "max": 10}));
    assert_eq!(len.default_value(), json!(5.0));
    assert!(len.coerce(&json!(1e300)).is_err());
    let rec = p(json!({"name": "r", "type": "record", "fields": [
    {"name": "who", "type": "text", "default": "me"}, {"name": "n", "type": "number", "default": 2}]}));
    assert_eq!(rec.default_value(), json!({"who": "me", "n": 2}));
    assert_eq!(
        rec.coerce(&json!({"n": 5})).unwrap(),
        json!({"who": "me", "n": 5.0})
    );
    assert!(rec.coerce(&json!({"zzz": 5})).is_err());
    assert!(rec.coerce(&json!(5)).is_err());
}

#[test]
fn bad_definitions_are_refused() {
    let base = def("");
    let check = |f: &dyn Fn(&mut PluginDef)| {
        let mut d = base.clone();
        f(&mut d);
        d.validate().unwrap_err()
    };
    check(&|d| d.params[0].name = "1x".into());
    check(&|d| d.params[0].name = "a b".into());
    check(&|d| d.params.push(d.params[0].clone()));
    check(&|d| d.params[0].min = Some(100.0));
    check(&|d| d.params[0].default = json!(100));
    check(&|d| d.params[0].options = vec!["a".into()]);
    check(&|d| d.params[0].min = Some(f64::NAN));
    check(&|d| d.script = "x".repeat(MAX_SCRIPT + 1));
    check(&|d| d.timeout_ms = Some(5));
    check(&|d| d.timeout_ms = Some(70_000));
    check(&|d| {
        d.params = (0..=MAX_PARAMS)
            .map(|i| ParamDef {
                name: format!("p{i}"),
                ..d.params[0].clone()
            })
            .collect()
    });
    check(&|d| d.params[0].ty = ParamType::Choice);
    check(&|d| d.params[0].ty = ParamType::Record);
    let mut rec = base.params[0].clone();
    rec.ty = ParamType::Record;
    rec.default = Value::Null;
    let mut inner = rec.clone();
    inner.name = "inner".into();
    inner.fields = vec![base.params[0].clone()];
    rec.fields = vec![inner];
    check(&|d| d.params = vec![rec.clone()]);
    let mut dup = rec.clone();
    dup.fields = vec![base.params[0].clone(), base.params[0].clone()];
    check(&|d| d.params = vec![dup.clone()]);
    let mut f = base.params[0].clone();
    f.fields = vec![base.params[0].clone()];
    check(&|d| d.params = vec![f.clone()]);
}

#[test]
fn definition_json_round_trips() {
    for (_, d) in crate::examples::all() {
        let v = d.to_json();
        assert_eq!(v["type"], "plugin_object");
        assert_eq!(PluginDef::from_json(&v).unwrap(), d);
    }
    assert!(PluginDef::from_json(&json!({"shape_type": "blob", "script": ""})).is_err());
    assert!(PluginDef::from_json(&json!({"shape_type": "point"})).is_err());
}

#[test]
fn geometry_is_read_for_each_shape_type() {
    let g = |t, v: Value| Geometry::from_json(t, &v);
    assert_eq!(
        g(ShapeType::Point, json!({"at": [1, 2]})).unwrap(),
        Geometry::Point {
            at: [1.0, 2.0],
            rotation: 0.0
        }
    );
    assert!(g(ShapeType::Point, json!({"at": [1, 2], "rotation": "x"})).is_err());
    assert!(g(ShapeType::Point, json!({"at": [1]})).is_err());
    assert!(g(ShapeType::Point, json!({})).is_err());
    assert!(g(ShapeType::Point, json!(5)).is_err());
    assert_eq!(
        g(ShapeType::Linear, json!({"points": [[0, 0], [5, 5]]})).unwrap(),
        Geometry::Linear {
            a: [0.0, 0.0],
            b: [5.0, 5.0]
        }
    );
    assert!(g(ShapeType::Linear, json!({"points": [[0, 0]]})).is_err());
    assert!(g(ShapeType::Linear, json!({"a": [0, 0]})).is_err());
    assert_eq!(
        g(ShapeType::Rectangle, json!({"min": [5, 5], "max": [0, 1]})).unwrap(),
        Geometry::Rectangle {
            min: [0.0, 1.0],
            max: [5.0, 5.0]
        }
    );
    let mut path = g(
        ShapeType::Path,
        json!({"points": [[0, 0], [1, 0]], "closed": true}),
    )
    .unwrap();
    assert!(g(ShapeType::Path, json!({"points": [[0, 0]]})).is_err());
    assert!(
        g(
            ShapeType::Path,
            json!({"points": [[0, 0], [1, 0]], "closed": 1})
        )
        .is_err()
    );
    assert!(g(ShapeType::Path, json!({"points": 3})).is_err());
    assert!(g(ShapeType::Path, json!({"points": [[0, 0], [1e300, 0]]})).is_err());
    for mut x in [
        path.clone(),
        point(),
        Geometry::Linear {
            a: [0.0; 2],
            b: [1.0; 2],
        },
        Geometry::Rectangle {
            min: [0.0; 2],
            max: [1.0; 2],
        },
    ] {
        let before = x.to_json();
        x.translate(5.0, -5.0);
        assert_ne!(x.to_json(), before);
    }
    path.translate(1.0, 1.0);
    assert_eq!(path.to_json()["points"][0], json!([1.0, 1.0]));
    for t in [
        ShapeType::Point,
        ShapeType::Linear,
        ShapeType::Rectangle,
        ShapeType::Path,
    ] {
        assert!(!t.name().is_empty());
    }
}

#[test]
fn parse_answer_handles_noise() {
    let l = ResetLimits::default();
    let e = parse_answer(b"", "boom\n", Some(1), &l).unwrap_err();
    assert!(e.to_string().contains("boom"));
    let ok = parse_answer(
        b"{\"ok\": true, \"shapes\": [{\"kind\": \"line\", \"a\": [0, 0], \"b\": [1, 1]}]}\n\n",
        "",
        Some(0),
        &l,
    )
    .unwrap();
    assert_eq!(ok.len(), 1);
    assert!(parse_answer(b"{\"ok\": true, \"shapes\": 5}", "", Some(0), &l).is_err());
}

#[test]
fn a_background_process_does_not_stall_the_reset() {
    // The script starts a long job, returns its shapes at once, and must not hold the app.
    let limits = ResetLimits {
        timeout: Duration::from_millis(1000),
        ..ResetLimits::default()
    };
    let t = std::time::Instant::now();
    let shapes = run(
        "import subprocess\ndef reset(p, g):\n    subprocess.Popen(['sleep', '30'])\n    return [circle(g.at, 5)]\n",
        limits,
    )
    .unwrap();
    assert_eq!(shapes.len(), 1);
    assert!(
        t.elapsed() < Duration::from_millis(2500),
        "{:?}",
        t.elapsed()
    );
}

fn marker_interpreter(name: &str) -> (std::path::PathBuf, python::Interpreter) {
    let dir = std::env::temp_dir().join(format!("kadakas-exec-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let prog = dir.join("spy.sh");
    std::fs::write(
        &prog,
        format!(
            "#!/bin/sh\ntouch {m}\npwd > {c}\nenv > {e}\nexit 1\n",
            m = dir.join("started").display(),
            c = dir.join("cwd").display(),
            e = dir.join("env").display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (
        dir,
        python::Interpreter {
            program: prog,
            prefix: vec![],
        },
    )
}

#[test]
fn the_executor_refuses_an_untrusted_script_by_itself() {
    // A caller that skips every check still cannot run an untrusted script: no permit can be made.
    let (dir, spy) = marker_interpreter("untrusted");
    let store = TrustStore::in_memory();
    let evil = "def reset(p, g):\n    return []\n";
    assert!(store.permit(evil, false).is_none());
    let d = def(evil);
    let params = d.resolve_params(&Map::new()).unwrap();
    // A permit for another text does not help.
    let other = store
        .permit(&crate::examples::all()[0].1.script, false)
        .unwrap();
    let e = reset(
        &other,
        &spy,
        "X",
        &d,
        &params,
        &point(),
        &ResetLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(e, ResetError::Refused(_)), "{e:?}");
    assert!(
        !dir.join("started").exists(),
        "the executor started an untrusted script"
    );
    // A script with a hidden character is refused even with a fitting permit.
    let sneaky = "def reset(p, g):\n    return []  # docs\rimport os\n";
    let d = def(sneaky);
    let permit = TrustStore::in_memory().permit(sneaky, true).unwrap();
    let e = reset(
        &permit,
        &spy,
        "X",
        &d,
        &params,
        &point(),
        &ResetLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(e, ResetError::Refused(_)), "{e:?}");
    assert!(!dir.join("started").exists());
    // A fitting permit runs it, in an empty private folder, without the interpreter variables.
    let ok = def(evil);
    let permit = store.permit(evil, true).unwrap();
    let _ = reset(
        &permit,
        &spy,
        "X",
        &ok,
        &params,
        &point(),
        &ResetLimits::default(),
    );
    assert!(dir.join("started").exists());
    let cwd = std::fs::read_to_string(dir.join("cwd")).unwrap();
    assert!(
        cwd.contains("tmp") || cwd.contains("T/") || cwd.contains("var"),
        "{cwd}"
    );
    assert!(
        !cwd.trim().ends_with(&format!("{}", std::process::id())) || !cwd.contains("kadakas-exec")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn planted_interpreter_variables_are_not_passed_on() {
    let (dir, spy) = marker_interpreter("env");
    // Set in this process: the child must not see them.
    // SAFETY: only this test sets these names, and no other test reads them.
    unsafe {
        std::env::set_var("UV_PYTHON", "/tmp/evil-python");
        std::env::set_var("PYTHONSTARTUP", "/tmp/evil.py");
    }
    let script = "def reset(p, g):\n    return []\n";
    let d = def(script);
    let params = d.resolve_params(&Map::new()).unwrap();
    let permit = Permit::for_test(script);
    let _ = reset(
        &permit,
        &spy,
        "X",
        &d,
        &params,
        &point(),
        &ResetLimits::default(),
    );
    let env = std::fs::read_to_string(dir.join("env")).unwrap();
    assert!(
        !env.contains("UV_PYTHON") && !env.contains("PYTHONSTARTUP"),
        "{env}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hidden_and_control_characters_are_rejected_and_crlf_is_normal() {
    assert!(check_script_text("def reset(p, g):\r\n    return []\r\n").is_ok());
    assert!(check_script_text("a = 1\tb\n").is_ok());
    for bad in [
        "x = 1  # docs\rimport os",
        "x = '\u{202E}'",
        "x = 1\u{200B}",
        "x = 1\u{FEFF}",
        "x = 1\u{2066}",
        "x = 1\u{0}",
        "x = 1\u{1b}[2K",
        "x = 1\u{85}",
        "x = 1\u{2028}",
        "x = 1\u{200F}",
    ] {
        assert!(check_script_text(bad).is_err(), "{bad:?}");
        let mut d = def(bad);
        d.script = bad.into();
        assert!(d.validate().is_err(), "{bad:?}");
    }
    // \r\n and \n give the same hash, so the trust does not depend on the line ending.
    assert_eq!(crate::trust::hash("a\r\nb"), crate::trust::hash("a\nb"));
    assert_ne!(crate::trust::hash("a\rb"), crate::trust::hash("a\nb"));
}
