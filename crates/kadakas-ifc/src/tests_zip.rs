//! ifcZIP and the encodings that are refused: files on disk and bytes in memory.

use std::io::Write;

use kadakas_core::command::Editor;
use kadakas_core::model::Document;

use super::tests::sample;
use super::*;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../tests/fixtures/ifc/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (n, d) in entries {
        z.start_file(*n, o).unwrap();
        z.write_all(d).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn run(bytes: Vec<u8>) -> Result<(Editor, kadakas_io::Report), IfcImportError> {
    let doc = Document::with_sequential_ids();
    let r = import_bytes(bytes, &doc, &IfcImportOptions::default())?;
    let mut ed = Editor::new(doc);
    ed.execute(&r.command).unwrap();
    Ok((ed, r.report))
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("kadakas-ifczip-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_zipped_file_imports_like_the_plain_file() {
    let plain = fixture("extruded_rect.ifc");
    let (a, ra) = run(plain.clone()).unwrap();
    let (b, rb) = run(zip_of(&[("extruded_rect.ifc", &plain)])).unwrap();
    assert_eq!(a.doc().objects.len(), b.doc().objects.len());
    assert_eq!(ra.total(), rb.total());
    assert_eq!(ra.notes, rb.notes);
    // With another entry: the report says it was ignored.
    let (_, r) = run(zip_of(&[("x.ifc", &plain), ("readme.txt", b"hello")])).unwrap();
    assert!(r.notes.iter().any(|n| n.contains("1 other entries")));
}

#[test]
fn a_zipped_file_on_disk_is_found_by_content_not_by_name() {
    let dir = temp_dir("disk");
    let plain = fixture("extruded_rect.ifc");
    // The zip has the name .ifc, a plain file has the name .ifczip: both work.
    std::fs::write(dir.join("a.ifc"), zip_of(&[("m.ifc", &plain)])).unwrap();
    std::fs::write(dir.join("b.ifczip"), &plain).unwrap();
    let doc = Document::with_sequential_ids();
    for n in ["a.ifc", "b.ifczip"] {
        let r = import_file(&dir.join(n), &doc, &IfcImportOptions::default()).unwrap();
        assert_eq!(r.report.total(), 1, "{n}");
    }
    // A zip over the size limit is refused before it is read.
    let mut o = IfcImportOptions::default();
    o.limits.max_bytes = 100;
    let e = import_file(&dir.join("a.ifc"), &doc, &o).unwrap_err();
    assert!(e.to_string().contains("larger than the limit"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn xml_and_json_are_refused_with_the_name_of_the_format() {
    let e = run(b"<?xml version=\"1.0\"?><ifcXML/>".to_vec())
        .err()
        .unwrap();
    assert_eq!(
        e.to_string(),
        "IFC XML is not supported; export STEP (.ifc) or ifcZIP"
    );
    let e = run(b"{\"type\":\"ifcJSON\"}".to_vec()).err().unwrap();
    assert!(e.to_string().starts_with("IFC JSON is not supported"));
    let dir = temp_dir("xml");
    std::fs::write(dir.join("a.ifcxml"), b"<ifc/>").unwrap();
    let e = import_file(
        &dir.join("a.ifcxml"),
        &Document::with_sequential_ids(),
        &IfcImportOptions::default(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("IFC XML"));
    assert!(matches!(
        import_file(
            &dir.join("missing.ifc"),
            &Document::with_sequential_ids(),
            &IfcImportOptions::default()
        ),
        Err(IfcImportError::Io(_))
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bad_zips_are_refused() {
    let plain = fixture("extruded_rect.ifc");
    for (z, want) in [
        (zip_of(&[("../x.ifc", &plain)]), "unsafe name"),
        (
            zip_of(&[("a.ifc", &plain), ("b.ifc", &plain)]),
            "more than one",
        ),
        (zip_of(&[("a.txt", b"x")]), "no .ifc file"),
        (zip_of(&[("a.ifcxml", b"<x/>")]), "IFC XML"),
        (b"PK\x03\x04broken".to_vec(), "zip"),
    ] {
        let e = run(z).err().unwrap().to_string();
        assert!(e.contains(want), "{e}");
    }
}

#[test]
fn an_export_to_ifczip_is_a_zip_with_one_ifc_file() {
    let dir = temp_dir("export");
    let doc = sample();
    let path = dir.join("Site plan.ifczip");
    let report = export_file(&path, &doc, &IfcOptions::default()).unwrap();
    assert!(report.total() > 10);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(z.len(), 1);
    let mut inner = String::new();
    {
        use std::io::Read;
        let mut f = z.by_index(0).unwrap();
        assert_eq!(f.name().unwrap(), "Site plan.ifc");
        f.read_to_string(&mut inner).unwrap();
    }
    assert!(inner.contains("FILE_NAME('Site plan.ifc'"));
    // The same bytes as the plain export, and it is read back by the importer.
    let (plain, _) = export_bytes(
        &doc,
        &IfcOptions {
            file_name: Some("Site plan.ifc".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(inner.as_bytes(), plain.as_slice());
    let r = import_file(
        &path,
        &Document::with_sequential_ids(),
        &IfcImportOptions::default(),
    )
    .unwrap();
    assert!(r.report.total() > 5);
    // In memory: the same bytes again. The same document gives the same zip.
    let (mem, _) = export_zip_bytes(&doc, &IfcOptions::default(), "Site plan.ifc").unwrap();
    assert_eq!(mem, bytes);
    assert!(is_ifczip_path(&path) && !is_ifczip_path(&dir.join("a.ifc")));
    // No temporary file is left. A bad folder is an error.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    assert!(matches!(
        export_file(
            &dir.join("no").join("x.ifczip"),
            &doc,
            &IfcOptions::default()
        ),
        Err(IfcError::Io(_))
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}
