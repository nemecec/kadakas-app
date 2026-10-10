//! Error codes, converter policy, atomic exports with `.bak`, the shared SVG function and
//! unknown shapes in the API.

use cadektor_api::{PathPolicy, Session};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cdk-audit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(d).unwrap()
}

fn raw(s: &mut Session, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    serde_json::from_str(&s.handle_line(&line).unwrap()).unwrap()
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    let r = raw(s, method, params);
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn err(s: &mut Session, method: &str, params: Value) -> (i64, String, String) {
    let r = raw(s, method, params);
    let e = &r["error"];
    assert!(e.is_object(), "{method} should fail: {r}");
    (
        e["code"].as_i64().unwrap(),
        e["data"]["kind"].as_str().unwrap_or_default().to_string(),
        e["message"].as_str().unwrap_or_default().to_string(),
    )
}

fn circle(s: &mut Session) -> u64 {
    let r = ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 10}}}),
    );
    r["created"][0].as_u64().unwrap()
}

fn restricted(dir: &Path) -> Session {
    Session {
        paths: PathPolicy::restricted([dir.to_path_buf()]),
        ..Session::default()
    }
}

#[test]
fn errors_have_distinct_codes_and_a_stable_kind() {
    let mut s = Session::default();
    assert_eq!(
        err(&mut s, "objects.get", json!({"id": 999})).0,
        2,
        "not found"
    );
    assert_eq!(
        err(&mut s, "objects.get", json!({"id": 999})).1,
        "not_found"
    );
    let (c, k, _) = err(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 0}}}),
    );
    assert_eq!((c, k.as_str()), (3, "validation"));
    let (c, k, _) = err(&mut s, "no.such.method", json!({}));
    assert_eq!((c, k.as_str()), (-32601, "method_not_found"));
    let (c, k, _) = err(&mut s, "objects.get", json!({}));
    assert_eq!((c, k.as_str()), (-32602, "invalid_params"));
    // A layer that still has objects is a conflict.
    let id = circle(&mut s);
    assert!(id > 0);
    let (c, k, _) = err(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_layer", "id": 1}}),
    );
    assert!(matches!(c, 3 | 5), "{c} {k}");
    // A parse error has a kind, too.
    let r: Value = serde_json::from_str(&s.handle_line("{nope").unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32700);
    assert_eq!(r["error"]["data"]["kind"], "parse_error");
}

#[test]
fn describe_documents_the_error_codes() {
    let mut s = Session::default();
    let d = ok(&mut s, "describe", json!({}));
    let codes = d["errors"]["codes"].as_array().unwrap();
    let have: Vec<i64> = codes.iter().map(|c| c["code"].as_i64().unwrap()).collect();
    for c in [1, 2, 3, 4, 5, 6, -32601, -32602] {
        assert!(have.contains(&c), "code {c} is described");
    }
    assert!(d["errors"]["shape"].as_str().unwrap().contains("data.kind"));
    let text = d.to_string();
    assert!(
        !text.contains("Vectorworks"),
        "describe uses neutral wording"
    );
}

#[test]
fn files_open_and_save_cdk_and_old_names_through_the_session() {
    let dir = tmp("files");
    let mut s = restricted(&dir);
    circle(&mut s);
    // `file.save` and `file.open` are not Session methods: a session reports them as unknown. The
    // hosts (cli, app) route them to `files::handle_file_request`.
    let f = dir.join("plan.cdk");
    let req = json!({"jsonrpc": "2.0", "id": 4, "method": "file.save", "params": {"path": f}});
    let reply: Value =
        serde_json::from_str(&cadektor_api::files::handle_file_request(&mut s, &req)).unwrap();
    assert_eq!(reply["result"]["ok"], true, "{reply}");
    assert!(f.exists());
    let old = dir.join("plan.cdk.json");
    let req = json!({"jsonrpc": "2.0", "id": 5, "method": "file.save", "params": {"path": old}});
    let reply: Value =
        serde_json::from_str(&cadektor_api::files::handle_file_request(&mut s, &req)).unwrap();
    assert_eq!(reply["result"]["ok"], true, "{reply}");
    let mut t = restricted(&dir);
    for p in [&f, &old] {
        let req = json!({"jsonrpc": "2.0", "id": 6, "method": "file.open", "params": {"path": p}});
        let reply: Value =
            serde_json::from_str(&cadektor_api::files::handle_file_request(&mut t, &req)).unwrap();
        assert_eq!(reply["result"]["ok"], true, "{reply}");
        assert_eq!(ok(&mut t, "document.summary", json!({}))["object_count"], 1);
    }
}

#[test]
fn the_dwg_converter_is_off_for_agents_unless_allowed() {
    let dir = tmp("dwg");
    let dwg = dir.join("a.dwg");
    std::fs::write(&dwg, "x").unwrap();
    let mut s = restricted(&dir);
    for (m, p) in [
        ("import.file", json!({"path": dwg})),
        (
            "export.file",
            json!({"path": dir.join("b.dwg"), "format": "dwg"}),
        ),
    ] {
        let (c, k, msg) = err(&mut s, m, p.clone());
        assert_eq!((c, k.as_str()), (4, "policy"), "{m}: {msg}");
        assert!(msg.contains("--allow-converters"), "{msg}");
    }
    // With the flag the request goes on. It then fails for another reason, not for the policy.
    s.paths = PathPolicy::restricted([dir.clone()]).with_converters(true);
    let r = raw(&mut s, "import.file", json!({"path": dwg}));
    let msg = r["error"]["message"].as_str().unwrap_or_default();
    assert!(!msg.contains("--allow-converters"), "{r}");
    // The user's own dialogs are not limited by the flag.
    s.paths = PathPolicy::restricted([dir.clone()]);
    let e = s
        .import_file_trusted(json!({"path": dwg}))
        .err()
        .map(|e| e.message)
        .unwrap_or_default();
    assert!(!e.contains("--allow-converters"), "{e}");
    // A library session (no policy) is not limited either.
    assert!(Session::default().paths.converters_allowed());
}

#[test]
fn library_and_favorites_cannot_be_redirected() {
    let lib = tmp("lib");
    let outside = tmp("outside");
    // Safe here: this program has one test that sets the variable, and no other test reads it.
    unsafe { std::env::set_var("CADEKTOR_LIBRARY_DIR", &lib) };
    let mut s = restricted(&outside);
    // The favorites file is always inside the library folder, whatever the request says.
    ok(
        &mut s,
        "library.favorite_add",
        json!({"library": "L", "type": "hatch", "name": "H", "path": outside.join("evil.json")}),
    );
    assert!(lib.join("favorites.json").exists());
    assert!(!outside.join("evil.json").exists());
    let list = ok(&mut s, "library.favorites", json!({}));
    assert_eq!(list["favorites"].as_array().unwrap().len(), 1);
    // A link in place of the file is refused.
    #[cfg(unix)]
    {
        std::fs::remove_file(lib.join("favorites.json")).unwrap();
        let target = outside.join("target.json");
        std::fs::write(&target, "[]").unwrap();
        std::os::unix::fs::symlink(&target, lib.join("favorites.json")).unwrap();
        let (c, k, _) = err(
            &mut s,
            "library.favorite_add",
            json!({"library": "L", "type": "hatch", "name": "H2"}),
        );
        assert_eq!((c, k.as_str()), (4, "policy"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "[]");
    }
    // Path requests need a document extension, and a path outside the folders is refused.
    let (c, _, _) = err(
        &mut s,
        "resource.list_file",
        json!({"path": outside.join("x.txt")}),
    );
    assert_eq!(c, 4);
}

#[test]
fn worksheet_csv_export_is_atomic_keeps_a_bak_and_defuses_formulas() {
    let dir = tmp("csv");
    let mut s = Session::default();
    let id = ok(
        &mut s,
        "worksheet.create",
        json!({"name": "T", "cells": {"A1": "'=evil()", "A2": "'-5 cm", "B1": 12}}),
    )["id"]
        .clone();
    let out = dir.join("t.csv");
    ok(
        &mut s,
        "worksheet.export",
        json!({"id": id, "format": "csv", "path": out}),
    );
    let first = std::fs::read_to_string(&out).unwrap();
    assert!(first.contains("'=evil()"), "{first}");
    assert!(first.contains("'-5 cm"), "{first}");
    ok(
        &mut s,
        "worksheet.create",
        json!({"name": "U", "cells": {"A1": "second"}}),
    );
    let id2 = ok(&mut s, "resource.get", json!({"id": "U"}))["id"].clone();
    ok(
        &mut s,
        "worksheet.export",
        json!({"id": id2, "format": "csv", "path": out}),
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("t.csv.bak")).unwrap(),
        first,
        "the old file is kept"
    );
    assert!(std::fs::read_to_string(&out).unwrap().contains("second"));
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "no temporary file stays");
}

#[test]
fn glb_export_keeps_a_bak() {
    let dir = tmp("glb");
    let mut s = Session::default();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0,0],[3000,0],[3000,1200],[0,1200]], "material": "Granite"}}}),
    );
    let out = dir.join("m.glb");
    ok(&mut s, "export.file", json!({"path": out}));
    let first = std::fs::read(&out).unwrap();
    ok(&mut s, "export.file", json!({"path": out}));
    assert_eq!(std::fs::read(dir.join("m.glb.bak")).unwrap(), first);
}

#[test]
fn render_svg_is_one_function() {
    let mut s = Session::default();
    circle(&mut s);
    let via_method = ok(&mut s, "render.svg", json!({"width": 500}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(via_method, s.render_svg(500));
    // The width is clamped like the method does.
    assert_eq!(s.render_svg(1), s.render_svg(16));
}

#[test]
fn unknown_shapes_are_listed_and_only_deletable() {
    let mut s = Session::default();
    let id = circle(&mut s);
    let mut doc = ok(&mut s, "document.get", json!({}));
    doc["objects"][0]["shape"] = json!({"kind": "spiral", "turns": 2});
    doc["objects"][0]["future"] = json!(true);
    ok(&mut s, "document.load", json!({"document": doc.clone()}));
    let sum = ok(&mut s, "document.summary", json!({}));
    assert_eq!(sum["objects_by_kind"]["unknown"], 1);
    assert_eq!(sum["unknown_shapes"], json!([{"id": id, "kind": "spiral"}]));
    let (c, k, m) = err(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [1, 1]}}),
    );
    assert_eq!((c, k.as_str()), (3, "validation"), "{m}");
    // The document comes back unchanged.
    assert_eq!(ok(&mut s, "document.get", json!({})), doc);
    let d = ok(&mut s, "describe", json!({}));
    assert!(d["unknown_shapes"].as_str().unwrap().contains("unknown"));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [id]}}),
    );
    assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 0);
}
