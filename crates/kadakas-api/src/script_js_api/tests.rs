use serde_json::{Value, json};

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
        json!({"code": format!("{ADD_CIRCLES} throw new Error('boom');"), "args": {"$kn": 3}}),
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
    assert!(e.message.contains("writes a file"), "{}", e.message);
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
fn a_script_of_the_document_runs_only_after_a_person_trusted_its_text() {
    let mut s = Session::default();
    let made = s
        .handle(
            "script_resource.create",
            json!({"name": "Count", "code": "return 41 + 1;"}),
        )
        .unwrap();
    assert_eq!(made["trusted"], json!(false));
    let sha = made["sha256"].as_str().unwrap().to_string();
    let e = eval(&mut s, json!({"script": "Count"})).unwrap_err();
    assert_eq!(e.kind, "policy", "{e:?}");
    assert!(e.message.contains(&sha));
    // A wrong hash trusts nothing.
    assert!(crate::trust_script(&mut s, "Count", "00").is_err());
    assert!(eval(&mut s, json!({"script": "Count"})).is_err());
    crate::trust_script(&mut s, "Count", &sha).unwrap();
    assert_eq!(ok(&mut s, json!({"script": "Count"}))["result"], json!(42));
    // A new text is a new hash and needs trust again.
    let upd = s
        .handle(
            "script_resource.update",
            json!({"id": "Count", "code": "return 43;"}),
        )
        .unwrap();
    assert_eq!(upd["trusted"], json!(false));
    assert!(eval(&mut s, json!({"script": "Count"})).is_err());
    // The list shows it.
    let l = s.handle("script_resource.list", json!({})).unwrap();
    assert_eq!(l["scripts"].as_array().unwrap().len(), 1);
    // The text an agent sends itself needs no trust.
    assert_eq!(ok(&mut s, json!({"code": "return 7;"}))["result"], json!(7));
    // A second script with the same name is refused.
    let e = s
        .handle(
            "script_resource.create",
            json!({"name": "count", "code": "1"}),
        )
        .unwrap_err();
    assert_eq!(e.kind, "conflict");
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

/// A script resource that is trusted, for the generator tests.
fn trusted_script(s: &mut Session, name: &str, code: &str) {
    let made = s
        .handle(
            "script_resource.create",
            json!({"name": name, "code": code}),
        )
        .unwrap();
    crate::trust_script(s, name, made["sha256"].as_str().unwrap()).unwrap();
}

/// A grid of rectangles 200 x 100 over the box of the region; the rows alternate between two
/// unit types.
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

fn layout(s: &mut Session, extra: Value) -> Result<Value, RpcError> {
    let mut p = json!({
        "region": {"outline": [[0, 0], [1000, 0], [1000, 250], [0, 250]]},
        "pattern": {"kind": "generator", "script": "Grid", "params": {"w": 200, "h": 100}, "gap": 0},
        "origin": [0, 0],
        "limit": 100,
    });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    s.handle("unit_pattern.layout", p)
}

#[test]
fn a_generator_pattern_is_laid_clipped_and_counted_like_any_other_kind() {
    let mut s = Session::default();
    trusted_script(&mut s, "Grid", GRID);
    let l = layout(&mut s, json!({})).unwrap();
    let sum = &l["summary"];
    // 5 columns and 3 rows of 200 x 100: two whole rows, and a third row cut at y = 250 (half).
    assert_eq!(l["units_total"], json!(15), "{l}");
    assert_eq!(sum["whole_units"], json!(10), "{l}");
    assert_eq!(sum["cut_units"], json!(5), "{l}");
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
}

#[test]
fn the_same_input_gives_the_same_layout_and_a_change_makes_a_new_one() {
    let mut s = Session::default();
    trusted_script(&mut s, "Grid", GRID);
    let a = layout(&mut s, json!({})).unwrap();
    let b = layout(&mut s, json!({})).unwrap();
    assert_eq!(a, b, "deterministic");
    // Other parameters, the script, and the region each give a new answer.
    let c = layout(
        &mut s,
        json!({"pattern": {"kind": "generator", "script": "Grid", "params": {"w": 250, "h": 125}, "gap": 0}}),
    )
    .unwrap();
    assert_eq!(c["units_total"], json!(8), "4 columns and 2 rows");
    s.handle(
        "script_resource.update",
        json!({"id": "Grid", "code": GRID.replace("r % 2", "0")}),
    )
    .unwrap();
    let e = layout(&mut s, json!({})).unwrap_err();
    assert!(
        e.message.contains("not trusted"),
        "a new text needs trust: {}",
        e.message
    );
}

#[test]
fn a_generator_that_is_not_trusted_or_missing_or_wrong_is_an_error_with_the_reason() {
    let mut s = Session::default();
    let e = layout(&mut s, json!({})).unwrap_err();
    assert!(e.message.contains("none with that name"), "{}", e.message);
    s.handle(
        "script_resource.create",
        json!({"name": "Grid", "code": GRID}),
    )
    .unwrap();
    let e = layout(&mut s, json!({})).unwrap_err();
    assert!(e.message.contains("not trusted"), "{}", e.message);
    trusted_script(&mut s, "Bad", "return 5;");
    let e = layout(
        &mut s,
        json!({"pattern": {"kind": "generator", "script": "Bad", "gap": 0}}),
    )
    .unwrap_err();
    assert!(e.message.contains("must return an object"), "{}", e.message);
    trusted_script(
        &mut s,
        "Calls",
        "kadakas.call('document.summary', {}); return {};",
    );
    let e = layout(
        &mut s,
        json!({"pattern": {"kind": "generator", "script": "Calls", "gap": 0}}),
    )
    .unwrap_err();
    assert!(e.message.contains("cannot call"), "{}", e.message);
    trusted_script(&mut s, "Loop", "for (;;) {}");
    s.set_limits(SessionLimits {
        script_js_fuel: 20_000_000,
        ..SessionLimits::default()
    });
    let e = layout(
        &mut s,
        json!({"pattern": {"kind": "generator", "script": "Loop", "gap": 0}}),
    )
    .unwrap_err();
    assert!(e.message.contains("fuel"), "{}", e.message);
}

#[test]
fn the_unit_count_of_the_session_bounds_a_generator() {
    let mut s = Session::default();
    trusted_script(&mut s, "Grid", GRID);
    s.set_limits(SessionLimits {
        pattern_units: 10,
        ..SessionLimits::default()
    });
    let e = layout(&mut s, json!({})).unwrap_err();
    assert!(
        e.message.contains("limit of 10") || e.message.contains("more than"),
        "{}",
        e.message
    );
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

const ASHLAR: &str = include_str!("../../../../docs/scripts/random-ashlar.js");
const FAN: &str = include_str!("../../../../docs/scripts/radial-fan.js");

fn pattern_of(script: &str, params: Value) -> Value {
    json!({"kind": "generator", "script": script, "params": params, "gap": 0})
}

#[test]
fn the_example_ashlar_covers_the_region_in_courses_of_three_lengths() {
    let mut s = Session::default();
    trusted_script(&mut s, "Ashlar", ASHLAR);
    let run = |s: &mut Session, seed: u32| {
        layout(
            s,
            json!({
                "region": {"outline": [[0, 0], [3000, 0], [3000, 1000], [0, 1000]]},
                "pattern": pattern_of("Ashlar", json!({"seed": seed})),
                "limit": 1000,
            }),
        )
        .unwrap()
    };
    let a = run(&mut s, 1);
    let sum = &a["summary"];
    // Five courses of 200 mm over 1000 mm: no unit is cut at the top or the bottom, so every
    // cut unit is cut at the left or right edge, at most two per course.
    assert!(sum["cut_units"].as_u64().unwrap() <= 10, "{sum}");
    let area = sum["area_laid_m2"].as_f64().unwrap();
    assert!(
        (area - 3.0).abs() < 1e-9,
        "the courses fill the region: {area}"
    );
    // Three unit types, named by their size.
    let names: Vec<&str> = sum["by_unit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["300x200", "450x200", "600x200"]);
    let total: u64 = sum["by_unit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["whole_units"].as_u64().unwrap() + u["cut_units"].as_u64().unwrap())
        .sum();
    assert_eq!(total, a["units_total"].as_u64().unwrap());
    // The same seed gives the same wall, another seed another one.
    assert_eq!(a, run(&mut s, 1));
    assert_ne!(a["units"], run(&mut s, 2)["units"]);
}

#[test]
fn the_example_fan_has_the_counts_of_a_hand_checked_small_case() {
    let mut s = Session::default();
    trusted_script(&mut s, "Fan", FAN);
    // A 2000 x 2000 mm square with the fan at its middle: rings of 500 mm, 8 wedges in each.
    // Radius 500 and 1000 lie inside the square (a wedge of the second ring touches the sides at
    // the axes), so rings 1 and 2 give 16 whole units. The farthest corner is 1414 mm away: a
    // third ring of 8 wedges, and each of them is cut by a side of the square.
    let l = layout(
        &mut s,
        json!({
            "region": {"outline": [[-1000, -1000], [1000, -1000], [1000, 1000], [-1000, 1000]]},
            "pattern": pattern_of("Fan", json!({"ring": 500, "count": 8})),
            "origin": [0, 0],
            "limit": 0,
        }),
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
    // Even rings: the first and the third (8 + 8 wedges, the third cut); odd: the second.
    assert_eq!(by, [(8, 8), (8, 0)]);
}

#[test]
fn a_hardscape_with_a_generator_style_is_reported_checked_and_exported() {
    let mut s = Session::default();
    trusted_script(&mut s, "Grid", GRID);
    s.handle(
        "hardscape_style.create",
        json!({"name": "Generated", "joints": {"kind": "generator",
            "generator": {"script": "Grid", "params": {"w": 200, "h": 100}}, "spacing": 0}}),
    )
    .unwrap();
    let h = s
        .handle(
            "hardscape.add",
            json!({"boundary": [[0, 0], [1000, 0], [1000, 250], [0, 250]], "style": "Generated"}),
        )
        .unwrap()["id"]
        .clone();
    let r = s
        .handle("report.unit_pattern", json!({"ids": [h]}))
        .unwrap();
    let row = &r["rows"][0];
    assert_eq!(row["bond"], json!("generator"));
    assert_eq!(
        (row["whole_units"].clone(), row["cut_units"].clone()),
        (json!(10), json!(5))
    );
    assert_eq!(row["order_units"], json!(15));
    // The check finds nothing wrong in a clean generated pattern.
    let c = s.handle("unit_pattern.check", json!({"ids": [h]})).unwrap();
    assert_eq!(c["objects"][0]["findings"], json!([]), "{c}");
    // The DXF has every unit as a polyline (and the outline of the hardscape).
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
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("LWPOLYLINE").count(), 16);
    // The units can be made ordinary objects too.
    let made = s
        .handle("unit_pattern.to_objects", json!({"id": h}))
        .unwrap();
    assert_eq!(made["units"], json!(15), "{made}");
}
