use std::path::PathBuf;

use serde_json::{json, Value};

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
    let e = call(&mut s, "export.file", json!({"path": out, "format": "geojson"})).unwrap();
    assert_eq!(e["exported"], 1);
    let back = tmp("out.dxf");
    call(&mut s, "export.file", json!({"path": back, "options": {"unit": "m"}})).unwrap();
    let mut t = Session::default();
    let r = call(&mut t, "import.file", json!({"path": back})).unwrap();
    assert_eq!(r["imported"], 1);
    assert!(s.editor.undo());
    assert!(s.editor.doc().objects.is_empty());
    let geo = tmp("in.geojson");
    std::fs::write(&geo, r#"{"type":"Point","coordinates":[24.9,60.1]}"#).unwrap();
    let r = call(&mut s, "import.file", json!({"path": geo})).unwrap();
    assert_eq!(r["georeference"]["crs"], "EPSG:32635");
    assert!(call(&mut s, "document.summary", json!({})).unwrap()["georeference"]["crs"].is_string());
    for f in [dxf, out, back, geo] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn errors_are_reported() {
    let mut s = Session::default();
    let e = call(&mut s, "import.file", json!({"path": "/nonexistent/x.dxf"})).unwrap_err();
    assert_eq!(e.code, 1);
    assert_eq!(call(&mut s, "import.file", json!({"path": "x.dxf", "format": "png"})).unwrap_err().code, -32602);
    assert_eq!(call(&mut s, "import.file", json!({"path": "x.dxf", "options": {"unit": "parsec"}})).unwrap_err().code, -32602);
    assert_eq!(call(&mut s, "import.file", json!({})).unwrap_err().code, -32602);
    assert_eq!(call(&mut s, "export.file", json!({"path": "x.shp"})).unwrap_err().code, 1);
    assert_eq!(call(&mut s, "export.file", json!({"path": "x.dxf", "options": {"tolerance": -1}})).unwrap_err().code, -32602);
    assert_eq!(describe()["examples"].as_array().unwrap().len(), 3);
}
