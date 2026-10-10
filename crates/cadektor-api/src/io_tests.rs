use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::io::describe;
use crate::{RpcError, Session};

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cadektor-api-io-{}-{name}", std::process::id()))
}

fn call(s: &mut Session, method: &str, p: Value) -> Result<Value, RpcError> {
    s.handle(method, p)
}

#[test]
fn import_export_round_trip_through_the_api() {
    let dxf = tmp("in.dxf");
    std::fs::write(&dxf, "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nWalls\n10\n0\n20\n0\n11\n1000\n21\n0\n0\nENDSEC\n0\nEOF\n").unwrap();
    let mut s = Session::default();
    let r = call(&mut s, "import.file", json!({"path": dxf})).unwrap();
    assert_eq!(r["imported"], 1);
    assert_eq!(r["report"]["classes_created"][0], "Walls");
    assert!(r["georeference"].is_null());
    assert_eq!(s.editor.doc().objects.len(), 1);
    let out = tmp("out.geojson");
    let e = call(
        &mut s,
        "export.file",
        json!({"path": out, "format": "geojson"}),
    )
    .unwrap();
    assert_eq!(e["exported"], 1);
    let back = tmp("out.dxf");
    call(
        &mut s,
        "export.file",
        json!({"path": back, "options": {"unit": "m"}}),
    )
    .unwrap();
    let mut t = Session::default();
    let r = call(&mut t, "import.file", json!({"path": back})).unwrap();
    assert_eq!(r["imported"], 1);
    assert!(s.editor.undo());
    assert!(s.editor.doc().objects.is_empty());
    let geo = tmp("in.geojson");
    std::fs::write(&geo, r#"{"type":"Point","coordinates":[24.9,60.1]}"#).unwrap();
    let r = call(&mut s, "import.file", json!({"path": geo})).unwrap();
    assert_eq!(r["georeference"]["crs"], "EPSG:32635");
    assert!(
        call(&mut s, "document.summary", json!({})).unwrap()["georeference"]["crs"].is_string()
    );
    for f in [dxf, out, back, geo] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn errors_are_reported() {
    let mut s = Session::default();
    let e = call(&mut s, "import.file", json!({"path": "/nonexistent/x.dxf"})).unwrap_err();
    assert_eq!(e.code, 1);
    assert_eq!(
        call(
            &mut s,
            "import.file",
            json!({"path": "x.dxf", "format": "png"})
        )
        .unwrap_err()
        .code,
        -32602
    );
    assert_eq!(
        call(
            &mut s,
            "import.file",
            json!({"path": "x.dxf", "options": {"unit": "parsec"}})
        )
        .unwrap_err()
        .code,
        -32602
    );
    assert_eq!(
        call(&mut s, "import.file", json!({})).unwrap_err().code,
        -32602
    );
    assert_eq!(
        call(&mut s, "export.file", json!({"path": "x.shp"}))
            .unwrap_err()
            .code,
        1
    );
    assert_eq!(
        call(
            &mut s,
            "export.file",
            json!({"path": "x.dxf", "options": {"tolerance": -1}})
        )
        .unwrap_err()
        .code,
        -32602
    );
    assert_eq!(describe()["examples"].as_array().unwrap().len(), 4);
}

fn policy_dirs(name: &str) -> (PathBuf, PathBuf) {
    let base = tmp(name);
    let _ = std::fs::remove_dir_all(&base);
    let (inside, outside) = (base.join("inside"), base.join("outside"));
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    (inside, outside)
}

#[test]
fn restricted_sessions_only_use_allowed_folders() {
    let (inside, outside) = policy_dirs("policy");
    let line = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nA\n10\n0\n20\n0\n11\n1000\n21\n0\n0\nENDSEC\n0\nEOF\n";
    std::fs::write(inside.join("in.dxf"), line).unwrap();
    std::fs::write(outside.join("secret.dxf"), line).unwrap();
    std::fs::write(inside.join("notes.txt"), line).unwrap();
    let mut s = Session {
        paths: crate::PathPolicy::restricted([inside.clone()]),
        ..Session::default()
    };
    // Inside works. Outside, `..` and a wrong extension do not.
    assert_eq!(
        call(
            &mut s,
            "import.file",
            json!({"path": inside.join("in.dxf")})
        )
        .unwrap()["imported"],
        1
    );
    let e = call(
        &mut s,
        "import.file",
        json!({"path": outside.join("secret.dxf")}),
    )
    .unwrap_err();
    assert!(e.message.contains("--allow-dir"), "{}", e.message);
    assert!(
        call(
            &mut s,
            "import.file",
            json!({"path": inside.join("..").join("outside").join("secret.dxf")})
        )
        .is_err()
    );
    let e = call(
        &mut s,
        "import.file",
        json!({"path": inside.join("notes.txt")}),
    )
    .unwrap_err();
    assert!(e.message.contains("unknown file type"), "{}", e.message);
    let e = call(
        &mut s,
        "import.file",
        json!({"path": inside.join("notes.txt"), "format": "dxf"}),
    )
    .unwrap_err();
    assert!(e.message.contains("extension"), "{}", e.message);
    assert_eq!(s.editor.doc().objects.len(), 1);
    // Export: new files work, existing ones need overwrite, other folders and extensions do not.
    let out = inside.join("out.dxf");
    assert_eq!(
        call(&mut s, "export.file", json!({"path": out})).unwrap()["exported"],
        1
    );
    let first = std::fs::read(&out).unwrap();
    let e = call(&mut s, "export.file", json!({"path": out})).unwrap_err();
    assert!(e.message.contains("overwrite"), "{}", e.message);
    assert_eq!(
        call(
            &mut s,
            "export.file",
            json!({"path": out, "options": {"unit": "m"}, "overwrite": true})
        )
        .unwrap()["exported"],
        1
    );
    assert_ne!(std::fs::read(&out).unwrap(), first);
    // The write is atomic: no temporary files stay, and the old file is the backup.
    let names: Vec<String> = std::fs::read_dir(&inside)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    assert_eq!(std::fs::read(inside.join("out.dxf.bak")).unwrap(), first);
    assert!(
        call(
            &mut s,
            "export.file",
            json!({"path": outside.join("x.dxf")})
        )
        .is_err()
    );
    assert!(!outside.join("x.dxf").exists());
    assert!(
        call(
            &mut s,
            "export.file",
            json!({"path": inside.join("x.txt"), "format": "dxf"})
        )
        .is_err()
    );
    assert!(call(&mut s, "export.file", json!({"path": inside.join("x.shp")})).is_err());
    // The folder of the document is allowed too, and survives `document.load`.
    s.doc_dir = Some(outside.clone());
    assert!(
        call(
            &mut s,
            "import.file",
            json!({"path": outside.join("secret.dxf")})
        )
        .is_ok()
    );
    let doc = serde_json::to_value(s.editor.doc()).unwrap();
    call(&mut s, "document.load", json!({"document": doc})).unwrap();
    assert!(
        call(
            &mut s,
            "import.file",
            json!({"path": outside.join("secret.dxf")})
        )
        .is_ok()
    );
    assert!(s.paths.is_restricted());
    // The user's own dialogs are not limited.
    s.doc_dir = None;
    assert!(
        call(
            &mut s,
            "import.file",
            json!({"path": outside.join("secret.dxf")})
        )
        .is_err()
    );
    assert_eq!(
        s.import_file_trusted(json!({"path": outside.join("secret.dxf")}))
            .unwrap()["imported"],
        1
    );
    assert!(
        s.export_file_trusted(json!({"path": outside.join("by-user.dxf")}))
            .is_ok()
    );
    assert!(
        s.check_path(&outside.join("a.json"), true, &["json"])
            .is_err()
    );
    assert!(
        s.check_path(&inside.join("a.txt"), true, &["json"])
            .is_err()
    );
    assert!(
        s.check_path(&inside.join("a.json"), true, &["json"])
            .is_ok()
    );
    assert_eq!(
        Session::default()
            .check_path(Path::new("rel/x.txt"), true, &["json"])
            .unwrap(),
        PathBuf::from("rel/x.txt")
    );
    // Without a policy any path works.
    let mut free = Session::default();
    assert!(
        free.import_file_trusted(json!({"path": outside.join("secret.dxf")}))
            .is_ok()
    );
    assert!(
        call(
            &mut free,
            "export.file",
            json!({"path": outside.join("free.txt"), "format": "dxf"})
        )
        .is_ok()
    );
    let _ = std::fs::remove_dir_all(inside.parent().unwrap());
}

#[test]
fn ifc_export_follows_the_path_policy_and_never_changes_the_document() {
    let (inside, outside) = policy_dirs("ifc");
    let mut s = Session {
        paths: crate::PathPolicy::restricted([inside.clone()]),
        ..Session::default()
    };
    s.handle("execute", json!({"command": {"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "material": "Stone"}}})).unwrap();
    let guid = s.editor.doc().drawing.project.guid.clone();
    assert_eq!(guid.len(), 22, "a new session has a project GUID");
    let before = s.editor.doc().clone();
    // Other folders and extensions fail. Bad options are invalid params.
    assert!(
        call(
            &mut s,
            "export.file",
            json!({"path": outside.join("x.ifc")})
        )
        .is_err()
    );
    assert!(
        call(
            &mut s,
            "export.file",
            json!({"path": inside.join("x.txt"), "format": "ifc"})
        )
        .is_err()
    );
    assert_eq!(
        call(
            &mut s,
            "export.file",
            json!({"path": inside.join("x.ifc"), "options": {"version": "IFC2X3"}})
        )
        .unwrap_err()
        .code,
        -32602
    );
    assert!(!inside.join("x.ifc").exists());
    let out = inside.join("site.ifc");
    let r = call(&mut s, "export.file", json!({"path": out})).unwrap();
    assert_eq!(r["exported"], 1);
    assert_eq!(r["project_guid"], guid.as_str());
    assert_eq!(*s.editor.doc(), before, "no undo step, no change");
    let first = std::fs::read(&out).unwrap();
    assert!(String::from_utf8_lossy(&first).contains("FILE_SCHEMA(('IFC4'))"));
    let e = call(&mut s, "export.file", json!({"path": out, "format": "ifc"})).unwrap_err();
    assert!(e.message.contains("overwrite"), "{}", e.message);
    call(
        &mut s,
        "export.file",
        json!({"path": out, "format": "ifc", "overwrite": true}),
    )
    .unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), first);
    assert_eq!(s.editor.doc().drawing.project.guid, guid);
    // The user's own dialog is not limited.
    let free = outside.join("free.ifc");
    s.export_file_trusted(json!({"path": free, "format": "ifc"}))
        .unwrap();
    assert!(free.exists());
}
