//! `import.update` and `dxf.compare` with DWG files, through a stand-in converter: the "DWG" files
//! of this test are DXF text, and the converter copies them. One test, because it sets the
//! environment of the process.

#![cfg(unix)]

use kadakas_api::{PathPolicy, Session};
use serde_json::{Value, json};

fn file(version: &str, ents: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$FINGERPRINTGUID\n2\nA1B2C3D4-0001\n9\n$VERSIONGUID\n2\n{version}\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n"
    )
}

fn line(handle: u32, y: i64) -> String {
    format!("0\nLINE\n5\n{handle:X}\n8\nA\n10\n0\n20\n{y}\n11\n1000\n21\n{y}\n")
}

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

#[test]
fn dwg_drawings_are_updated_and_compared_through_the_converter() {
    use std::os::unix::fs::PermissionsExt;
    let dir = kadakas_sys::tmp::PrivateTempDir::new("kadakas-api-dwg-update").unwrap();
    let converter = dir.path().join("dwg2dxf");
    std::fs::write(&converter, "#!/bin/sh\ncp \"$4\" \"$3\"\n").unwrap();
    std::fs::set_permissions(&converter, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: this test file has one test, and no other thread reads the environment here.
    unsafe {
        std::env::set_var("KADAKAS_DWG_CONVERTER", &converter);
    }
    let v1 = dir.path().join("site_v1.dwg");
    let v2 = dir.path().join("site_v2.dwg");
    std::fs::write(&v1, file("V1", &[line(0x10, 0), line(0x11, 100)].concat())).unwrap();
    std::fs::write(
        &v2,
        file(
            "V2",
            &[line(0x10, 0), line(0x11, 160), line(0x12, 300)].concat(),
        ),
    )
    .unwrap();

    // Compare: DWG against DWG, with a picture.
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "dxf.compare",
        json!({"base": v1, "new": v2, "render": {"format": "svg"}}),
    );
    assert_eq!(r["counts"]["changed"], 1);
    assert_eq!(r["counts"]["added"], 1);
    assert!(r["image"]["svg"].as_str().unwrap().contains("<svg"));

    // Update: the drawing was imported as a DWG file and kept as a link to it.
    ok(
        &mut s,
        "import.file",
        json!({"path": v1, "options": {"base": "link"}}),
    );
    let r = ok(&mut s, "import.update", json!({"path": v2}));
    assert_eq!(r["old_from"], "link");
    assert_eq!(r["counts"]["changed"], 1);
    ok(
        &mut s,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all"}),
    );
    // The link moved to the new DWG file, and the next update reads it through the converter.
    let link = s
        .editor
        .doc()
        .layers
        .iter()
        .find_map(|l| {
            kadakas_core::provenance::LayerSource::of(
                l.extra.get(kadakas_core::provenance::LAYER_KEY)?,
            )
        })
        .and_then(|l| l.link)
        .unwrap();
    assert!(link.path.ends_with("site_v2.dwg"), "{}", link.path);
    let v3 = dir.path().join("site_v3.dwg");
    std::fs::write(&v3, file("V3", &[line(0x10, 0), line(0x11, 160)].concat())).unwrap();
    let r = ok(
        &mut s,
        "import.update",
        json!({"path": v3, "dry_run": true}),
    );
    assert_eq!(r["old_from"], "link");
    assert_eq!(r["counts"]["removed"], 1);

    // A host that does not let agents start a converter refuses DWG files, and the linked DWG.
    let mut agent = Session::with_sequential_ids();
    ok(
        &mut agent,
        "import.file",
        json!({"path": v1, "options": {"base": "link"}}),
    );
    agent.paths = PathPolicy::restricted([dir.path().to_path_buf()]);
    let e = agent
        .handle("import.update", json!({"path": v2}))
        .unwrap_err();
    assert_eq!(e.kind, "policy", "{}", e.message);
    let e = agent
        .handle("dxf.compare", json!({"base": v1, "new": v2}))
        .unwrap_err();
    assert_eq!(e.kind, "policy");
    // With the converters allowed, the new file is read, and the linked DWG goes through the
    // path policy as well.
    agent.paths = PathPolicy::restricted([dir.path().to_path_buf()]).with_converters(true);
    let r = ok(
        &mut agent,
        "import.update",
        json!({"path": v2, "dry_run": true}),
    );
    assert_eq!(r["old_from"], "link");
    // The user's own dialog needs no flag.
    let mut user = Session::with_sequential_ids();
    ok(
        &mut user,
        "import.file",
        json!({"path": v1, "options": {"base": "link"}}),
    );
    user.paths = PathPolicy::restricted([dir.path().to_path_buf()]);
    let r = user
        .import_update_trusted(json!({"path": v2, "dry_run": true}))
        .unwrap();
    assert_eq!(r["old_from"], "link");

    // A restricted session without the flag cannot read the linked DWG either.
    let mut linked_only = Session::with_sequential_ids();
    ok(
        &mut linked_only,
        "import.file",
        json!({"path": v1, "options": {"base": "link"}}),
    );
    let plain = dir.path().join("plain.dxf");
    std::fs::write(
        &plain,
        file("V2", &[line(0x10, 0), line(0x11, 160)].concat()),
    )
    .unwrap();
    linked_only.paths = PathPolicy::restricted([dir.path().to_path_buf()]);
    let e = linked_only
        .handle("import.update", json!({"path": plain}))
        .unwrap_err();
    assert_eq!(e.kind, "conflict", "{}", e.message);
    assert!(e.message.contains("converter"), "{}", e.message);

    // No converter at all.
    // SAFETY: as above.
    unsafe {
        std::env::set_var("KADAKAS_DWG_CONVERTER", dir.path().join("missing"));
        std::env::set_var("PATH", "");
    }
    let mut t = Session::with_sequential_ids();
    let e = t
        .handle("dxf.compare", json!({"base": v1, "new": v2}))
        .unwrap_err();
    assert!(e.message.contains("converter"), "{}", e.message);
}
