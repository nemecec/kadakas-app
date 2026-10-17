use serde_json::{Value, json};

use kadakas_core::model::Document;

use crate::{RpcError, Session, SessionLimits};

fn eval(s: &mut Session, params: Value) -> Result<Value, RpcError> {
    s.handle("script.eval", params)
}

fn ok(s: &mut Session, params: Value) -> Value {
    eval(s, params).unwrap_or_else(|e| panic!("{e:?}"))
}

fn objects(s: &mut Session) -> usize {
    s.handle("objects.list", json!({}))
        .unwrap()
        .as_array()
        .map_or(0, Vec::len)
}

const ADD_CIRCLES: &str = r#"
    const ids = [];
    for (let i = 0; i < args.n; i++) {
        const r = kadakas.call("execute", {command: {op: "add_object",
            shape: {kind: "circle", center: [i * 1000, 0], radius: 300}}});
        ids.push(r.created[0]);
    }
"#;

#[test]
fn a_script_returns_a_value_and_reads_the_document() {
    let mut s = Session::default();
    let r = ok(
        &mut s,
        json!({"code": "const d = kadakas.call('document.summary', {}); console.log('hi'); return {layers: d.layers.length, a: args.a};", "args": {"a": 5}, "mode": "read"}),
    );
    assert_eq!(r["result"]["a"], json!(5));
    assert_eq!(r["calls"], json!(1));
    assert_eq!(r["log"], json!("hi\n"));
    assert_eq!(r["changed"], json!(false));
    assert_eq!(r["undo_steps"], json!(0));
}

#[test]
fn a_run_with_many_changes_is_one_undo_step() {
    let mut s = Session::default();
    let r = ok(
        &mut s,
        json!({"code": format!("{ADD_CIRCLES} return ids.length;"), "args": {"n": 5}}),
    );
    assert_eq!(r["result"], json!(5));
    assert_eq!(r["calls"], json!(5));
    assert_eq!(r["undo_steps"], json!(1));
    assert_eq!(objects(&mut s), 5);
    s.handle("undo", json!({})).unwrap();
    assert_eq!(objects(&mut s), 0, "one undo takes back the whole run");
    s.handle("redo", json!({})).unwrap();
    assert_eq!(objects(&mut s), 5, "and one redo brings it back");
}

#[test]
fn an_error_takes_back_the_whole_run() {
    let mut s = Session::default();
    let e = eval(
        &mut s,
        json!({"code": format!("{ADD_CIRCLES} throw new Error('boom');"), "args": {"n": 3}}),
    )
    .unwrap_err();
    assert!(e.message.contains("boom"), "{}", e.message);
    assert_eq!(e.details.as_ref().unwrap()["calls"], json!(3));
    assert_eq!(objects(&mut s), 0, "nothing is left");
    assert!(!s.editor.can_undo(), "and the history has no trace of it");
}

#[test]
fn a_failed_call_is_an_error_that_a_script_can_catch_and_the_rest_stays() {
    let mut s = Session::default();
    let r = ok(
        &mut s,
        json!({"code": "try { kadakas.call('objects.get', {id: '00000000-0000-0000-0000-0000000000ff'}); } catch (e) { return {code: e.code, method: e.method}; }"}),
    );
    assert_eq!(r["result"]["method"], json!("objects.get"));
    assert_eq!(r["result"]["code"], json!(2), "not found");
}

#[test]
fn read_mode_refuses_every_call_that_changes_the_document() {
    let mut s = Session::default();
    // Caught: the script sees a policy error, and the document is as it was.
    let r = ok(
        &mut s,
        json!({"mode": "read", "code": r#"
            try {
                kadakas.call("execute", {command: {op: "add_object", shape: {kind: "circle", center: [0, 0], radius: 5}}});
                return "not refused";
            } catch (e) { return [e.code, e.message.includes("mode read")]; }
        "#}),
    );
    assert_eq!(r["result"], json!([4, true]));
    assert_eq!(objects(&mut s), 0);
    assert!(!s.editor.can_undo());
    // Not caught: the run fails.
    let e = eval(
        &mut s,
        json!({"mode": "read", "code": "kadakas.call('execute', {command: {op: 'add_layer', name: 'X'}});"}),
    )
    .unwrap_err();
    assert!(e.message.contains("mode read"), "{}", e.message);
    // A method that writes a file is refused before it runs.
    let e = eval(
        &mut s,
        json!({"mode": "read", "code": "kadakas.call('export.file', {path: '/tmp/never.dxf'});"}),
    )
    .unwrap_err();
    assert!(e.message.contains("read-only"), "{}", e.message);
    // The same call is fine in edit mode.
    ok(
        &mut s,
        json!({"code": "kadakas.call('execute', {command: {op: 'add_layer', name: 'X'}});"}),
    );
}

#[test]
fn some_methods_are_never_called_by_a_script() {
    let mut s = Session::default();
    for m in [
        "script.eval",
        "undo",
        "document.load",
        "collab.connect",
        "events.poll",
    ] {
        let e = eval(
            &mut s,
            json!({"code": format!("kadakas.call('{m}', {{}});")}),
        )
        .unwrap_err();
        assert!(e.message.contains("cannot call"), "{m}: {}", e.message);
    }
}

#[test]
fn the_parameters_of_script_eval_are_checked() {
    let mut s = Session::default();
    for p in [
        json!({}),
        json!({"code": "1", "script": "x"}),
        json!({"code": "1", "mode": "write"}),
        json!({"code": "1", "bogus": 1}),
        json!({"code": 5}),
        json!({"code": "1", "seed": -1}),
    ] {
        let e = eval(&mut s, p.clone()).unwrap_err();
        assert_eq!(e.code, -32602, "{p}: {e:?}");
    }
    let e = eval(&mut s, json!({"code": "return (;"})).unwrap_err();
    assert!(
        e.message.contains("SyntaxError") || e.details.is_some(),
        "{e:?}"
    );
}

#[test]
fn the_same_script_gives_the_same_result() {
    let code = "const a = []; for (let i = 0; i < 4; i++) a.push(Math.random()); return a;";
    let mut s = Session::default();
    let r1 = ok(&mut s, json!({"code": code, "seed": 3}))["result"].clone();
    let r2 = ok(&mut Session::default(), json!({"code": code, "seed": 3}))["result"].clone();
    let r3 = ok(&mut s, json!({"code": code, "seed": 4}))["result"].clone();
    assert_eq!(r1, r2);
    assert_ne!(r1, r3);
}

#[test]
fn limits_of_the_session_stop_a_script() {
    let mut s = Session::default();
    s.set_limits(SessionLimits {
        script_js_fuel: 30_000_000,
        script_js_calls: 10,
        ..SessionLimits::default()
    });
    let e = eval(&mut s, json!({"code": "for (;;) {}"})).unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
    assert!(e.message.contains("fuel"), "{}", e.message);
    let e = eval(
        &mut s,
        json!({"code": "for (let i = 0; i < 50; i++) kadakas.call('document.summary', {});"}),
    )
    .unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
    assert_eq!(e.details.as_ref().unwrap()["calls"], json!(10));
    // The session still works.
    s.set_limits(SessionLimits::default());
    ok(&mut s, json!({"code": "return 1;"}));
}

#[test]
fn a_whole_run_has_a_time_limit_that_counts_the_calls() {
    let mut s = Session::default();
    s.set_limits(SessionLimits {
        script_js_time: std::time::Duration::from_millis(1),
        ..SessionLimits::default()
    });
    // The start of the engine takes longer than that, so the first call is over the limit.
    let e = eval(
        &mut s,
        json!({"code": "kadakas.call('document.summary', {}); let x = 0; for (let i = 0; i < 100000; i++) x += i; kadakas.call('document.summary', {});"}),
    )
    .unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
}

#[test]
fn script_info_says_how_scripts_run() {
    let mut s = Session::default();
    let i = s.handle("script.js_info", json!({})).unwrap();
    assert_eq!(i["available"], json!(true));
    assert!(i["upstream"].as_str().unwrap().contains("quickjs-ng"));
    assert!(i["limits"]["fuel"].as_u64().unwrap() > 0);
}

#[test]
fn a_typed_array_in_the_answer_of_a_call_and_in_the_result_is_written_out() {
    let mut s = Session::default();
    let r = ok(
        &mut s,
        json!({"mode": "read", "code": r#"
            const pts = kadakas.pointsFrom([[0, 0], [4000, 0], [4000, 3000], [0, 3000]]);
            const l = kadakas.call("unit_pattern.layout", {
                region: {outline: pts}, pattern: {unit_length: 1000, unit_width: 500, gap: 0, bond: "stack"},
                origin: [0, 0], limit: 0});
            return {summary: l.summary, pts};
        "#}),
    );
    assert_eq!(
        r["result"]["pts"],
        json!([[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [0.0, 3000.0]])
    );
    assert!(
        r["result"]["summary"].is_object() || r["result"]["summary"].is_null(),
        "{r}"
    );
}

/// Runs the body of a generator on `args` and passes its result on to `call`, a program of an
/// agent: the generator makes the shapes, the program writes them where they are stored.
fn with_generator(generator: &str, call: &str) -> String {
    format!(
        "const generate = (args) => {{ {generator}\n}};\nconst result = generate(args);\n{call}"
    )
}

/// A grid of rectangles over the box of the region; the rows alternate between two unit types.
const GRID: &str = r#"
    const b = args.region.box;
    const w = args.params.w, h = args.params.h;
    const cols = Math.ceil((b[2] - b[0]) / w), rows = Math.ceil((b[3] - b[1]) / h);
    const data = new Float64Array(cols * rows * 5), kind = new Uint32Array(cols * rows);
    let n = 0;
    for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) {
        data.set([b[0] + c * w + w / 2, b[1] + r * h + h / 2, w, h, 0], n * 5);
        kind[n++] = r % 2;
    }
    return {types: ["even", "odd"], rects: {kind, data}};
"#;

const ASHLAR: &str = include_str!("../../../../docs/scripts/random-ashlar.js");
const FAN: &str = include_str!("../../../../docs/scripts/radial-fan.js");

/// The program of an agent: make the shapes with `generator` for the region `outline` (a
/// rectangle from 0, 0), then lay them with `unit_pattern.layout` and return the answer.
fn layout_program(generator: &str) -> String {
    with_generator(
        generator,
        r#"return kadakas.call("unit_pattern.layout", {
            region: {outline: args.outline},
            pattern: {kind: "generator", label: "test", params: args.params, gap: args.gap || 0,
                      types: result.types, rects: result.rects, polygons: result.polygons},
            origin: args.origin, limit: args.limit === undefined ? 1000 : args.limit});"#,
    )
}

fn args(w: f64, h: f64, params: Value) -> Value {
    json!({
        "outline": [[0, 0], [w, 0], [w, h], [0, h]],
        "params": params,
        "origin": [w / 2.0, h / 2.0],
        "angle": 0,
        "region": {"box": [0, 0, w, h]},
    })
}

fn lay(s: &mut Session, generator: &str, a: Value) -> Result<Value, RpcError> {
    eval(
        s,
        json!({"code": layout_program(generator), "args": a, "mode": "read"}),
    )
    .map(|r| r["result"].clone())
}

#[test]
fn a_program_makes_shapes_and_the_engine_lays_clips_and_counts_them() {
    let mut s = Session::default();
    // 1000 x 250 with units of 200 x 100: five columns, two whole rows, and a third row cut in
    // half (hand-checked: 15 units, 10 whole, 5 cut with half a unit each).
    let l = lay(
        &mut s,
        GRID,
        args(1000.0, 250.0, json!({"w": 200, "h": 100})),
    )
    .unwrap();
    let sum = &l["summary"];
    assert_eq!(l["units_total"], json!(15), "{l}");
    assert_eq!(
        (sum["whole_units"].clone(), sum["cut_units"].clone()),
        (json!(10), json!(5))
    );
    assert_eq!(sum["by_unit"][0]["name"], json!("even"));
    assert_eq!(sum["by_unit"][0]["whole_units"], json!(5));
    assert_eq!(sum["by_unit"][0]["cut_units"], json!(5));
    assert_eq!(sum["by_unit"][1]["whole_units"], json!(5));
    let units = l["units"].as_array().unwrap();
    let cut: Vec<&Value> = units
        .iter()
        .filter(|u| u["whole"] == json!(false))
        .collect();
    assert_eq!(cut.len(), 5);
    assert!(
        (cut[0]["area_m2"].as_f64().unwrap() - 0.01).abs() < 1e-9,
        "{}",
        cut[0]
    );
    assert!((cut[0]["share"].as_f64().unwrap() - 0.5).abs() < 1e-9);
    // Other parameters, another result; the same parameters, the same result.
    let again = lay(
        &mut s,
        GRID,
        args(1000.0, 250.0, json!({"w": 200, "h": 100})),
    )
    .unwrap();
    assert_eq!(l, again);
    let other = lay(
        &mut s,
        GRID,
        args(1000.0, 250.0, json!({"w": 250, "h": 125})),
    )
    .unwrap();
    assert_eq!(other["units_total"], json!(8), "4 columns and 2 rows");
}

#[test]
fn the_unit_count_of_the_session_bounds_a_stored_result() {
    let mut s = Session::default();
    s.set_limits(SessionLimits {
        pattern_units: 10,
        ..SessionLimits::default()
    });
    let e = lay(
        &mut s,
        GRID,
        args(1000.0, 250.0, json!({"w": 200, "h": 100})),
    )
    .unwrap_err();
    assert!(
        e.message.contains("limit of 10") || e.message.contains("more than"),
        "{}",
        e.message
    );
}

#[test]
fn a_bad_result_is_refused_with_the_reason() {
    let mut s = Session::default();
    let e = lay(
        &mut s,
        "return {types: ['a'], rects: {data: [1, 2, 3]}};",
        args(100.0, 100.0, json!({})),
    )
    .unwrap_err();
    assert!(e.message.contains("takes 5"), "{}", e.message);
    let e = lay(
        &mut s,
        "return {rects: {kind: [4], data: [50, 50, 10, 10, 0]}};",
        args(100.0, 100.0, json!({})),
    )
    .unwrap_err();
    assert!(e.message.contains("unit 0"), "{}", e.message);
    // No shapes at all: the pattern has no result.
    let e = lay(&mut s, "return {};", args(100.0, 100.0, json!({}))).unwrap_err();
    assert!(e.message.contains("no stored result"), "{}", e.message);
}

#[test]
fn the_example_ashlar_covers_the_region_in_courses_of_three_lengths() {
    let mut s = Session::default();
    let run = |s: &mut Session, seed: u32| {
        lay(
            s,
            ASHLAR,
            args(
                3000.0,
                1000.0,
                json!({"seed": seed, "course": 200, "lengths": [300, 450, 600]}),
            ),
        )
        .unwrap()
    };
    let a = run(&mut s, 1);
    let sum = &a["summary"];
    // Five courses of 200 mm over 1000 mm: every cut unit is cut at the left or right edge, at
    // most two per course.
    assert!(sum["cut_units"].as_u64().unwrap() <= 10, "{sum}");
    let area = sum["area_laid_m2"].as_f64().unwrap();
    // A unit that sticks out of the region by less than the trim (0.1 mm) counts as whole, and the
    // random start of a course can leave one so: at most 0.1 mm over 200 mm, twice per course.
    assert!(
        (area - 3.0).abs() < 2e-4,
        "the courses fill the region: {area}"
    );
    let names: Vec<&str> = sum["by_unit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["300x200", "450x200", "600x200"]);
    assert_eq!(a, run(&mut s, 1), "the same seed, the same wall");
    assert_ne!(a["units"], run(&mut s, 2)["units"]);
}

#[test]
fn the_example_fan_has_the_counts_of_a_hand_checked_small_case() {
    let mut s = Session::default();
    // A 2000 x 2000 mm square with the fan at its middle: rings of 500 mm, 8 wedges in each.
    // Radius 500 and 1000 lie inside the square (a wedge of the second ring touches the sides at
    // the axes), so rings 1 and 2 give 16 whole units. The farthest corner is 1414 mm away: a
    // third ring of 8 wedges, and each of them is cut by a side of the square.
    let l = lay(
        &mut s,
        FAN,
        args(2000.0, 2000.0, json!({"ring": 500, "count": 8})),
    )
    .unwrap();
    let sum = &l["summary"];
    assert_eq!(l["units_total"], json!(24), "{sum}");
    assert_eq!(sum["whole_units"], json!(16), "{sum}");
    assert_eq!(sum["cut_units"], json!(8), "{sum}");
    // The wedges tile the square up to the sag of the arcs (about 1 mm at 1 m).
    let area = sum["area_laid_m2"].as_f64().unwrap();
    assert!((area - 4.0).abs() < 4.0 * 2e-3, "{area}");
    let by: Vec<(u64, u64)> = sum["by_unit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            (
                u["whole_units"].as_u64().unwrap(),
                u["cut_units"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(by, [(8, 8), (8, 0)]);
}

#[test]
fn a_program_stores_its_result_in_a_style_and_the_hardscape_uses_it_without_code() {
    let mut s = Session::default();
    // The agent makes the shapes and writes them into a style, in one run (one undo step).
    let code = with_generator(
        GRID,
        r#"kadakas.call("hardscape_style.create", {name: "Generated", joints: {kind: "generator", spacing: 0,
             generator: {label: "grid", params: args.params,
               units: {types: result.types, rects: result.rects}}}});
           return kadakas.call("hardscape.add", {boundary: args.outline, style: "Generated"}).id;"#,
    );
    let r = ok(
        &mut s,
        json!({"code": code, "args": args(1000.0, 250.0, json!({"w": 200, "h": 100}))}),
    );
    assert_eq!(r["undo_steps"], json!(1));
    let h = r["result"].clone();
    // From here on no code runs: the engine is not even asked. A session without an engine
    // reports, checks and exports the same.
    let rep = s
        .handle("report.unit_pattern", json!({"ids": [h]}))
        .unwrap();
    let row = &rep["rows"][0];
    assert_eq!(row["bond"], json!("generator"));
    assert_eq!(
        (row["whole_units"].clone(), row["cut_units"].clone()),
        (json!(10), json!(5))
    );
    assert_eq!(row["order_units"], json!(15));
    let c = s.handle("unit_pattern.check", json!({"ids": [h]})).unwrap();
    assert_eq!(c["objects"][0]["findings"], json!([]), "{c}");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.dxf");
    let e = s
        .handle(
            "export.file",
            json!({"path": path, "format": "dxf", "options": {"patterns": "units"}}),
        )
        .unwrap();
    assert!(
        e["report"]["notes"][0]
            .as_str()
            .unwrap()
            .contains("10 whole and 5 cut"),
        "{e}"
    );
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap()
            .matches("LWPOLYLINE")
            .count(),
        16
    );
    let made = s
        .handle("unit_pattern.to_objects", json!({"id": h}))
        .unwrap();
    assert_eq!(made["units"], json!(15), "{made}");
    // The stored result survives a save and a load of the document.
    let doc = s.handle("document.get", json!({})).unwrap();
    let mut s2 = Session::default();
    s2.handle("document.load", json!({"document": doc}))
        .unwrap();
    let rep2 = s2
        .handle("report.unit_pattern", json!({"ids": [h]}))
        .unwrap();
    assert_eq!(rep2["rows"][0]["whole_units"], json!(10));
    // A style without a result says so.
    s2.handle(
        "hardscape_style.create",
        json!({"name": "Empty", "joints": {"kind": "generator", "generator": {"label": "x"}}}),
    )
    .unwrap();
    let h2 = s2
        .handle(
            "hardscape.add",
            json!({"boundary": [[0,0],[100,0],[100,100],[0,100]], "style": "Empty"}),
        )
        .unwrap()["id"]
        .clone();
    let e = s2
        .handle("report.unit_pattern", json!({"ids": [h2]}))
        .unwrap();
    assert!(
        e["rows"][0]["error"].as_str().is_some()
            || e["rows"][0]["pattern"]["error"].as_str().is_some()
            || e.to_string().contains("no stored result"),
        "{e}"
    );
}

#[test]
fn read_mode_refuses_a_method_that_is_not_read_only_before_it_runs() {
    let mut s = Session::default();
    // The call is refused up front, so nothing ran: not even a rolled back change.
    let revision = s.editor.revision();
    let r = ok(
        &mut s,
        json!({"mode": "read", "code": r#"
        const out = [];
        for (const m of ["execute", "export.file", "selection.set", "gis.sources", "plugins.install"]) {
            try { kadakas.call(m, {}); out.push("ran"); } catch (e) { out.push([e.code, e.message.includes("read-only")]); }
        }
        return out;"#}),
    );
    assert_eq!(r["result"], json!(vec![json!([4, true]); 5]));
    assert_eq!(
        s.editor.revision(),
        revision,
        "nothing ran, not even to be taken back"
    );
}

#[test]
fn a_method_that_is_not_classified_is_denied() {
    let mut s = Session::default();
    let e = eval(
        &mut s,
        json!({"code": "kadakas.call('made.up_method', {});"}),
    )
    .unwrap_err();
    assert!(e.message.contains("not classified"), "{}", e.message);
}

#[test]
fn while_a_run_is_open_a_call_from_outside_is_refused() {
    let mut s = Session::default();
    // A script cannot make a call from outside, so open a run the way a window does.
    let mut run = s
        .script_start(&json!({"code": "let n = 0; for (let i = 0; i < 3; i++) n += kadakas.call('document.summary', {}).layers.length; return n;"}))
        .unwrap();
    let e = s.handle("document.summary", json!({})).unwrap_err();
    assert_eq!(e.kind, "conflict", "{e:?}");
    assert!(
        s.script_start(&json!({"code": "1"})).is_err(),
        "one run at a time"
    );
    let answer = loop {
        if let Some(a) = s.script_pump(&mut run, std::time::Duration::from_millis(8)) {
            break a;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(answer.unwrap()["result"], json!(3));
    assert_eq!(run.calls(), 3);
    s.handle("document.summary", json!({})).unwrap();
}

#[test]
fn a_worker_run_can_be_stopped_also_when_it_makes_no_calls() {
    let mut s = Session::default();
    s.set_limits(SessionLimits {
        script_js_fuel: u64::MAX / 2,
        script_timeout: std::time::Duration::from_secs(60),
        script_js_time: std::time::Duration::from_secs(600),
        ..SessionLimits::default()
    });
    s.handle(
        "execute",
        json!({"command": {"op": "add_layer", "name": "Before"}}),
    )
    .unwrap();
    let depth = s.editor.undo_depth();
    let mut run = s
        .script_start(&json!({"code": "kadakas.call('execute', {command: {op: 'add_layer', name: 'In the run'}}); for (;;) {}"}))
        .unwrap();
    let started = std::time::Instant::now();
    let mut stopped = false;
    let answer = loop {
        if let Some(a) = s.script_pump(&mut run, std::time::Duration::from_millis(8)) {
            break a;
        }
        if !stopped && run.calls() >= 1 && started.elapsed() > std::time::Duration::from_millis(300)
        {
            run.stop();
            stopped = true;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "the run did not stop"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    let e = answer.unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
    assert!(e.message.contains("stopped by the person"), "{}", e.message);
    assert_eq!(s.editor.undo_depth(), depth, "the layer of the run is gone");
    assert!(s.editor.doc().layers.iter().all(|l| l.name != "In the run"));
}

#[test]
fn the_whole_run_is_bounded_by_its_time_also_without_calls() {
    let mut s = Session::default();
    s.set_limits(SessionLimits {
        script_js_fuel: u64::MAX / 2,
        script_timeout: std::time::Duration::from_secs(600),
        script_js_time: std::time::Duration::from_millis(300),
        ..SessionLimits::default()
    });
    let started = std::time::Instant::now();
    let e = eval(&mut s, json!({"code": "for (;;) {}"})).unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
    assert!(e.message.contains("in all"), "{}", e.message);
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

/// Measures what a call from a script costs against the same call from Rust. Run it with
/// `cargo test -p kadakas-api --release --lib measure_call_overhead -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a test"]
fn measure_call_overhead() {
    use std::time::Instant;
    const N: usize = 10_000;
    let mut s = Session::default();
    let t = Instant::now();
    ok(&mut s, json!({"code": "return 1;", "mode": "read"}));
    println!(
        "first run (compile of the module, start): {:?}",
        t.elapsed()
    );
    let t = Instant::now();
    for _ in 0..20 {
        ok(&mut s, json!({"code": "return 1;", "mode": "read"}));
    }
    println!("an empty run, mean of 20: {:?}", t.elapsed() / 20);

    let t = Instant::now();
    for _ in 0..N {
        s.handle("selection.get", json!({})).unwrap();
    }
    let rust = t.elapsed();
    println!(
        "{N} calls of selection.get from Rust: {rust:?} ({:?} per call)",
        rust / N as u32
    );

    let code = format!("for (let i = 0; i < {N}; i++) kadakas.call('selection.get', {{}});");
    let t = Instant::now();
    let r = ok(&mut s, json!({"code": code, "mode": "read"}));
    let js = t.elapsed();
    println!(
        "{N} calls of selection.get from a script: {js:?} ({:?} per call, {} calls)",
        js / N as u32,
        r["calls"]
    );

    // A call with parameters and a bigger answer: document.summary.
    let t = Instant::now();
    for _ in 0..N {
        s.handle("document.summary", json!({})).unwrap();
    }
    let rust = t.elapsed();
    let code = format!("for (let i = 0; i < {N}; i++) kadakas.call('document.summary', {{}});");
    let t = Instant::now();
    ok(&mut s, json!({"code": code, "mode": "read"}));
    let js = t.elapsed();
    println!(
        "{N} calls of document.summary: Rust {rust:?} ({:?}), script {js:?} ({:?})",
        rust / N as u32,
        js / N as u32
    );

    // Fuel: how much does a loop of calls cost? Use a small budget and find the limit.
    let code = "let i = 0; try { for (;;) { kadakas.call('selection.get', {}); i++; } } catch (e) {} return i;";
    s.set_limits(SessionLimits {
        script_js_fuel: 100_000_000,
        ..SessionLimits::default()
    });
    match eval(&mut s, json!({"code": code, "mode": "read"})) {
        Err(e) => println!(
            "100M fuel stops the loop of calls: {}",
            e.details.unwrap()["calls"]
        ),
        Ok(r) => println!("{r}"),
    }
}

fn pump_to_end(s: &mut Session, run: &mut crate::ScriptRun) -> Result<Value, RpcError> {
    let started = std::time::Instant::now();
    loop {
        if let Some(a) = s.script_pump(run, std::time::Duration::from_millis(8)) {
            return a;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "the run did not end"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

const ADD_FOUR_IN_ONE: &str = r#"
    const add = {op: "add_object", shape: {kind: "circle", center: [0, 0], radius: 5}};
    let code = null;
    try { kadakas.call("execute", {command: {op: "batch", commands: [add, add, add, add]}}); }
    catch (e) { code = e.code; }
"#;

#[test]
fn an_edit_before_the_run_survives_a_limit_inside_the_run_and_a_later_error() {
    let strict = || SessionLimits {
        max_objects: 3,
        ..SessionLimits::default()
    };
    // The person's edit: one circle.
    let person = |s: &mut Session| {
        s.handle(
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [9, 9], "radius": 1}}}),
        )
        .unwrap();
    };
    // The run: a call that goes over the limit and is taken back by the session (inside the
    // run), then an error. The edit before the run must stay.
    let mut s = Session::default();
    s.set_limits(strict());
    person(&mut s);
    let e = eval(
        &mut s,
        json!({"code": format!("{ADD_FOUR_IN_ONE} throw new Error('later');")}),
    )
    .unwrap_err();
    assert!(e.message.contains("later"), "{}", e.message);
    assert_eq!(objects(&mut s), 1, "the edit of the person stays");
    s.handle("undo", json!({})).unwrap();
    assert_eq!(objects(&mut s), 0, "and is still an undo step of its own");
    // The same run with no error: the run is one step, and the person's edit is another.
    let mut s = Session::default();
    s.set_limits(strict());
    person(&mut s);
    let r = ok(
        &mut s,
        json!({"code": format!("{ADD_FOUR_IN_ONE} kadakas.call('execute', {{command: {{op: 'add_object', shape: {{kind: 'circle', center: [1, 1], radius: 1}}}}}}); return code;")}),
    );
    assert_eq!(
        r["result"],
        json!(6),
        "the limit error reached the program (code 6)"
    );
    assert_eq!(r["undo_steps"], json!(1));
    assert_eq!(objects(&mut s), 2);
    s.handle("undo", json!({})).unwrap();
    assert_eq!(objects(&mut s), 1, "one undo takes back the run only");
    s.handle("undo", json!({})).unwrap();
    assert_eq!(objects(&mut s), 0);
}

#[test]
fn subscribers_hear_of_a_run_once_and_not_of_its_steps() {
    let mut s = Session::default();
    s.handle_as(
        7,
        kadakas_core::change::Origin::Agent,
        "events.subscribe",
        json!({}),
    )
    .unwrap();
    let mut run = s
        .script_start(&json!({"code": "for (let i = 0; i < 3; i++) kadakas.call('execute', {command: {op: 'add_object', shape: {kind: 'circle', center: [i, 0], radius: 1}}}); return 1;"}))
        .unwrap();
    // While the run is open nothing is announced, also after steps were made.
    let started = std::time::Instant::now();
    loop {
        assert!(s.take_notifications(7).is_empty());
        if let Some(a) = s.script_pump(&mut run, std::time::Duration::from_millis(2)) {
            a.unwrap();
            break;
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(60));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let notes = s.take_notifications(7);
    assert_eq!(notes.len(), 1, "one change for the run: {notes:?}");
    assert_eq!(
        notes[0]["params"]["added"]["objects"]
            .as_array()
            .map_or(0, Vec::len),
        3,
        "{notes:?}"
    );
    // A run that is taken back is not announced at all.
    let mut run = s
        .script_start(&json!({"code": "kadakas.call('execute', {command: {op: 'add_layer', name: 'L'}}); throw new Error('no');"}))
        .unwrap();
    assert!(pump_to_end(&mut s, &mut run).is_err());
    assert!(s.take_notifications(7).is_empty());
}

#[test]
fn a_dropped_run_is_taken_back_and_the_session_is_whole_again() {
    let mut s = Session::default();
    let origin = s.editor.origin();
    let mut run = s
        .script_start(&json!({"code": "kadakas.call('execute', {command: {op: 'add_layer', name: 'Half'}}); for (;;) {}"}))
        .unwrap();
    // Let it make its change.
    let started = std::time::Instant::now();
    while run.calls() == 0 {
        s.script_pump(&mut run, std::time::Duration::from_millis(8));
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(s.script_running());
    drop(run);
    // The next call of the session takes the run back first.
    let summary = s.handle("document.summary", json!({})).unwrap();
    assert_eq!(
        summary["layers"].as_array().unwrap().len(),
        1,
        "the layer of the run is gone"
    );
    assert!(!s.script_running());
    assert_eq!(s.editor.origin(), origin);
    // And a new run can start.
    ok(&mut s, json!({"code": "return 1;"}));
}

#[test]
fn a_run_cannot_reach_a_document_that_replaced_its_own() {
    let mut s = Session::default();
    let mut run = s
        .script_start(&json!({"code": "let n = 0; for (let i = 0; i < 100000; i++) { kadakas.call('document.summary', {}); n++; } return n;"}))
        .unwrap();
    s.script_pump(&mut run, std::time::Duration::from_millis(2));
    // The document is replaced under the run (a path that goes around `handle`).
    s.replace_document(Document::default());
    assert!(!s.script_running(), "the new session state has no run");
    let answer = pump_to_end(&mut s, &mut run);
    let e = answer.unwrap_err();
    assert_eq!(e.kind, "conflict", "{e:?}");
    assert!(e.message.contains("replaced"), "{}", e.message);
    // The new document is whole and usable.
    s.handle("document.summary", json!({})).unwrap();
    ok(&mut s, json!({"code": "return 2;"}));
}

#[test]
fn a_run_that_holds_more_undo_data_than_the_session_allows_ends() {
    let mut s = Session::default();
    s.set_limits(SessionLimits {
        max_history_bytes: 3_000,
        ..SessionLimits::default()
    });
    let e = eval(
        &mut s,
        json!({"code": "for (let i = 0; i < 200; i++) kadakas.call('execute', {command: {op: 'add_object', shape: {kind: 'circle', center: [i, 0], radius: 1}}});"}),
    )
    .unwrap_err();
    assert_eq!(e.kind, "limit", "{e:?}");
    assert!(e.message.contains("max_history_bytes"), "{}", e.message);
    assert_eq!(objects(&mut s), 0, "and the run is taken back");
}

#[test]
fn a_hostile_stored_result_is_refused_with_a_reason() {
    let mut s = Session::default();
    let layout = |s: &mut Session, pattern: Value| {
        s.handle(
            "unit_pattern.layout",
            json!({"region": {"outline": [[0, 0], [10, 0], [10, 10], [0, 10]]}, "pattern": pattern, "origin": [0, 0]}),
        )
    };
    let e = layout(
        &mut s,
        json!({"kind": "generator", "gap": 0, "rects": {"data": [1e12, 0, 5, 5, 0]}}),
    )
    .unwrap_err();
    assert!(e.message.contains("up to 1e9"), "{}", e.message);
    let e = layout(&mut s, json!({"kind": "generator", "gap": 0, "polygons": {"start": [0, 3], "points": [0, 0, 1, 1, 2, 2]}})).unwrap_err();
    assert!(e.message.contains("no area"), "{}", e.message);
    let e = layout(
        &mut s,
        json!({"kind": "generator", "gap": 0, "label": "a\nb", "rects": {"data": [5, 5, 5, 5, 0]}}),
    )
    .unwrap_err();
    assert!(e.message.contains("label"), "{}", e.message);
    let ok_one = layout(
        &mut s,
        json!({"kind": "generator", "gap": 0, "label": "fine", "rects": {"data": [5, 5, 5, 5, 0]}}),
    );
    assert!(ok_one.is_ok(), "{ok_one:?}");
}

#[test]
fn the_entry_of_the_window_checks_the_parameters_like_the_api_does() {
    let mut s = Session::default();
    for (params, word) in [
        (json!({"code": "1", "bogus": 1}), "bogus"),
        (json!({"code": "1", "mode": "write"}), "mode"),
        (json!({"code": "1", "seed": -1}), "seed"),
        (json!({"code": "1", "seed": 4294967296u64}), "seed"),
        (json!({"code": 5}), "code"),
        (json!({}), "code"),
        (json!("code"), "code"),
        (
            json!({"code": "x".repeat(super::MAX_CODE_BYTES + 1)}),
            "at most",
        ),
    ] {
        let e = s.script_start(&params).err().expect("refused");
        assert_eq!(e.code, -32602, "{params}: {}", e.message);
        assert!(e.message.contains(word), "{params}: {}", e.message);
    }
    // The call of the API takes null and refuses what is no object.
    assert!(s.handle("script.js_info", Value::Null).is_ok());
    let e = s.handle("script.eval", json!([1])).unwrap_err();
    assert!(e.message.contains("object"), "{}", e.message);
}
