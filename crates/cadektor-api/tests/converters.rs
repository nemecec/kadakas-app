//! DWG through a stand-in converter, and IFC round trips, through `import.file` and `export.file`.

use cadektor_api::{PathPolicy, Session};
use serde_json::{Value, json};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn err(s: &mut Session, method: &str, params: Value) -> String {
    s.handle(method, params).unwrap_err().message
}

fn work_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cadektor-api-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn add_site(s: &mut Session) {
    ok(
        s,
        "execute",
        json!({"command":{"op":"add_object","shape":{"kind":"hardscape","material":"paving",
            "boundary":[[0,0],[4000,0],[4000,3000],[0,3000]]}}}),
    );
    ok(
        s,
        "execute",
        json!({"command":{"op":"add_object","shape":{"kind":"line","a":[0,0],"b":[1000,0]}}}),
    );
}

#[cfg(unix)]
#[test]
fn dwg_files_go_through_the_converter_named_by_the_environment() {
    use std::os::unix::fs::PermissionsExt;
    let dir = work_dir("dwg");
    let dxf = "0\\nSECTION\\n2\\nENTITIES\\n0\\nLINE\\n8\\nWalls\\n10\\n0.0\\n20\\n0.0\\n30\\n0.0\\n11\\n1000.0\\n21\\n0.0\\n31\\n0.0\\n0\\nENDSEC\\n0\\nEOF\\n";
    // Stand-ins for dwg2dxf and dxf2dwg of LibreDWG.
    for (name, body) in [
        (
            "dwg2dxf",
            format!("grep -q FAKEDWG \"$4\" || exit 3\nprintf '{dxf}' > \"$3\"\n"),
        ),
        (
            "dxf2dwg",
            "grep -q ENTITIES \"$4\" || exit 3\necho FAKEDWG > \"$3\"\n".to_string(),
        ),
    ] {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // SAFETY: this test file has one test that touches the environment, and no other thread reads it here.
    unsafe {
        std::env::set_var("CADEKTOR_DWG_CONVERTER", dir.join("dwg2dxf"));
    }
    let input = dir.join("site.dwg");
    std::fs::write(&input, "FAKEDWG\n").unwrap();

    let mut s = Session::default();
    let d = ok(&mut s, "describe", json!({}));
    assert!(d["io"]["dwg"]["converter"]["name"].is_string(), "{d}");
    let r = ok(&mut s, "import.file", json!({"path": input}));
    assert_eq!(r["imported"], 1);
    let out = dir.join("out.dwg");
    let r = ok(&mut s, "export.file", json!({"path": out, "format": "dwg"}));
    assert_eq!(r["exported"], 1);
    assert!(out.exists());

    // A restricted session checks the path, the extension and the overwrite flag.
    let mut r = Session::default();
    r.paths = PathPolicy::restricted([dir.clone()]).with_converters(true);
    ok(&mut r, "import.file", json!({"path": input}));
    assert!(err(&mut r, "export.file", json!({"path": out})).contains("overwrite"));
    ok(
        &mut r,
        "export.file",
        json!({"path": out, "overwrite": true}),
    );
    let outside = std::env::temp_dir().join("elsewhere.dwg");
    assert!(!err(&mut r, "import.file", json!({"path": outside})).is_empty());
    assert!(!err(&mut r, "export.file", json!({"path": outside})).is_empty());
    // Bad options are invalid parameters, and a trusted import skips the policy.
    let e = r
        .handle(
            "import.file",
            json!({"path": input, "options": {"converter_timeout_s": -1}}),
        )
        .unwrap_err();
    assert!(!e.message.is_empty());
    let e = r
        .handle(
            "export.file",
            json!({"path": out, "options": {"dwg_version": 3}, "overwrite": true}),
        )
        .unwrap_err();
    assert!(!e.message.is_empty());
    let e = r
        .handle(
            "export.file",
            json!({"path": out, "options": {"layer_mode": 5}, "overwrite": true}),
        )
        .unwrap_err();
    assert!(!e.message.is_empty());
    ok(
        &mut r,
        "import.file",
        json!({"path": input.to_str().unwrap(), "options": {}}),
    );
    r.import_file_trusted(json!({"path": input})).unwrap();
    r.export_file_trusted(json!({"path": out, "overwrite": true}))
        .unwrap();

    // A converter that fails gives an application error.
    std::fs::write(&input, "NOT A DRAWING\n").unwrap();
    let e = s.handle("import.file", json!({"path": input})).unwrap_err();
    assert!(!e.message.is_empty());
    // Without any converter the error names the setup page.
    unsafe {
        std::env::set_var("CADEKTOR_DWG_CONVERTER", dir.join("missing"));
        std::env::set_var("PATH", "");
    }
    let m = err(&mut s, "import.file", json!({"path": input}));
    assert!(m.contains("converter"), "{m}");
    let m = err(&mut s, "export.file", json!({"path": dir.join("o2.dwg")}));
    assert!(m.contains("converter"), "{m}");
}

#[test]
fn ifc_files_round_trip_and_errors_are_reported() {
    let dir = work_dir("ifc");
    let mut s = Session::default();
    add_site(&mut s);
    let path = dir.join("site.ifc");
    let r = ok(
        &mut s,
        "export.file",
        json!({"path": path, "overwrite": true}),
    );
    assert!(r["exported"].as_u64().unwrap() >= 1, "{r}");
    assert!(path.exists());
    let mut t = Session::default();
    let r = ok(&mut t, "import.file", json!({"path": path}));
    assert!(r["imported"].as_u64().unwrap() >= 1, "{r}");

    // A restricted session checks the path and the overwrite flag.
    let mut r = Session::default();
    r.paths = PathPolicy::restricted([dir.clone()]);
    add_site(&mut r);
    assert!(err(&mut r, "export.file", json!({"path": path})).contains("overwrite"));
    ok(
        &mut r,
        "export.file",
        json!({"path": path, "overwrite": true}),
    );
    ok(&mut r, "import.file", json!({"path": path}));
    let outside = std::env::temp_dir().join("elsewhere.ifc");
    assert!(!err(&mut r, "export.file", json!({"path": outside})).is_empty());
    assert!(!err(&mut r, "import.file", json!({"path": outside})).is_empty());
    // Options that do not parse are invalid parameters.
    let e = r
        .handle(
            "export.file",
            json!({"path": path, "overwrite": true, "options": {"version": "IFC9"}}),
        )
        .unwrap_err();
    assert!(!e.message.is_empty());
    let e = r
        .handle(
            "import.file",
            json!({"path": path, "options": {"terrain": "nonsense"}}),
        )
        .unwrap_err();
    assert!(!e.message.is_empty());
    // A file that is no IFC file is an application error.
    let bad = dir.join("bad.ifc");
    std::fs::write(&bad, "this is not STEP").unwrap();
    let e = r.handle("import.file", json!({"path": bad})).unwrap_err();
    assert!(!e.message.is_empty());
    r.import_file_trusted(json!({"path": path})).unwrap();
    r.export_file_trusted(json!({"path": path, "overwrite": true}))
        .unwrap();
}
