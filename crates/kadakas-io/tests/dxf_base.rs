//! What a tracked DXF import keeps for a later update: the summaries (`base: "auto"`), a link to the
//! original file (`link`) or nothing (`none`). A drawing over the cap is never imported without
//! update data unless the caller chose `link` or `none` (IOP-22).

use kadakas_core::command::Editor;
use kadakas_core::model::Document;
use kadakas_core::provenance::{self, FileLink, LayerSource};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::dxf::link::{self, LinkError, ReadOptions};
use kadakas_io::dxf::revision::{self, Rec};
use kadakas_io::{BaseMode, Context, ImportOptions, Importer, IoError, Source};

/// A drawing of `n` points, with a fingerprint and a version.
fn drawing(n: usize, version: &str) -> String {
    let mut s = format!(
        "0\nSECTION\n2\nHEADER\n9\n$FINGERPRINTGUID\n2\n{{BASE-1}}\n9\n$VERSIONGUID\n2\n{{{version}}}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n"
    );
    for k in 0..n {
        s.push_str(&format!(
            "0\nPOINT\n5\n{:X}\n8\nP\n10\n{k}\n20\n0\n",
            k + 16
        ));
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

fn source(text: &str) -> Source {
    Source {
        name: "survey.dxf".into(),
        data: text.as_bytes().to_vec(),
        extra: Default::default(),
    }
}

fn opts(base: BaseMode) -> ImportOptions {
    ImportOptions {
        base,
        // The survey points are far apart in the test files only by their count.
        max_objects: 1_000_000,
        ..ImportOptions::default()
    }
}

fn sources(doc: &Document) -> Vec<LayerSource> {
    doc.layers
        .iter()
        .filter_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
        .collect()
}

fn bases(doc: &Document) -> usize {
    doc.resources
        .iter()
        .filter(|r| matches!(r.body, kadakas_core::resource::ResourceBody::ImportBase(_)))
        .count()
}

fn run(
    ed: &mut Editor,
    text: &str,
    o: &ImportOptions,
    link: Option<&FileLink>,
) -> Result<kadakas_io::Report, IoError> {
    let r = kadakas_io::dxf::import_linked(&source(text), &Context::of(ed.doc()), o, link)?;
    ed.execute(&r.command).unwrap();
    Ok(r.report)
}

fn anywhere(p: &Path) -> Result<std::path::PathBuf, String> {
    Ok(p.to_path_buf())
}

fn temp(name: &str) -> kadakas_sys::tmp::PrivateTempDir {
    kadakas_sys::tmp::PrivateTempDir::new(name).unwrap()
}

#[test]
fn a_drawing_within_the_cap_keeps_its_summaries_and_says_auto() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let report = run(&mut ed, &drawing(50, "V1"), &opts(BaseMode::Auto), None).unwrap();
    assert_eq!(report.base.as_deref(), Some("auto"));
    assert_eq!(bases(ed.doc()), 1);
    let src = sources(ed.doc());
    assert_eq!(src.len(), 1);
    assert!(src[0].link.is_none());
    assert_eq!(BaseMode::default(), BaseMode::Auto);
    assert_eq!(ImportOptions::default().base, BaseMode::Auto);
}

#[test]
fn a_drawing_over_the_cap_is_refused_in_auto_mode_with_the_choices() {
    let n = provenance::MAX_BASE_RECS + 1;
    let mut ed = Editor::new(Document::with_sequential_ids());
    let before = ed.doc().clone();
    let err = run(&mut ed, &drawing(n, "V1"), &opts(BaseMode::Auto), None).unwrap_err();
    assert_eq!(
        err,
        IoError::BaseChoice {
            entities: n,
            limit: kadakas_io::BaseLimit::TooMany {
                cap: provenance::MAX_BASE_RECS
            }
        }
    );
    let text = err.to_string();
    for want in ["200000", "\"link\"", "\"none\"", "base"] {
        assert!(text.contains(want), "{text}");
    }
    assert_eq!(ed.doc(), &before, "nothing was imported");
    // Exactly at the cap is within it (checked on the count, without a 200000 entity import).
    let sample = kadakas_io::dxf::parse::parse(drawing(5, "V1").as_bytes()).unwrap();
    assert!(!revision::records(&sample, 5).1);
    assert!(revision::records(&sample, 4).1);
}

#[test]
fn link_keeps_the_path_the_size_and_the_hash_instead_of_the_summaries() {
    let dir = temp("kadakas-base-link");
    let path = dir.path().join("survey.dxf");
    let text = drawing(20, "V1");
    std::fs::write(&path, &text).unwrap();
    let mut ed = Editor::new(Document::with_sequential_ids());
    let o = ImportOptions {
        base: BaseMode::Link,
        ..ImportOptions::default()
    };
    let r = kadakas_io::import_file(&path, None, &o, ed.doc()).unwrap();
    ed.execute(&r.command).unwrap();
    assert_eq!(r.report.base.as_deref(), Some("link"));
    assert_eq!(bases(ed.doc()), 0, "the link is instead of the summaries");
    let src = sources(ed.doc());
    let link = src[0].link.clone().expect("the layer source has the link");
    assert_eq!(link.bytes, text.len() as u64);
    assert_eq!(
        link.sha256,
        kadakas_core::picture::sha256_hex(text.as_bytes())
    );
    let canonical = std::fs::canonicalize(&path).unwrap();
    assert_eq!(link.path, canonical.to_str().unwrap());
    assert_eq!(link.problem(), None);
    assert!(r.report.notes.iter().any(|t| t.contains("original file")));

    // The link survives a save and a load.
    let saved = serde_json::to_string(ed.doc()).unwrap();
    let loaded = Document::from_json(&saved).unwrap();
    assert!(loaded.load_notes().is_empty(), "{:?}", loaded.load_notes());
    assert_eq!(sources(&loaded)[0].link, Some(link.clone()));

    // An update gets the old summaries back from the file.
    let recs = link::read_records(&link, &anywhere, &ReadOptions::new(1 << 20, 1000)).unwrap();
    let parsed = kadakas_io::dxf::parse::parse(text.as_bytes()).unwrap();
    let (mut want, _) = revision::records(&parsed, 1000);
    want.iter_mut().for_each(Rec::compact);
    assert_eq!(recs, want);
    // And says so when the file is gone or has changed.
    std::fs::write(&path, drawing(20, "V2")).unwrap();
    assert!(matches!(
        link::read_records(&link, &anywhere, &ReadOptions::new(1 << 20, 1000)),
        Err(LinkError::Changed(_))
    ));
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        link::read_records(&link, &anywhere, &ReadOptions::new(1 << 20, 1000)),
        Err(LinkError::Gone(_))
    ));
}

use std::path::Path;

#[test]
fn link_needs_a_file_and_a_valid_path() {
    // From bytes alone the importer cannot make a link.
    let mut ed = Editor::new(Document::with_sequential_ids());
    let e = run(&mut ed, &drawing(3, "V1"), &opts(BaseMode::Link), None).unwrap_err();
    assert!(matches!(e, IoError::Option(_)), "{e}");
    let e = DxfFormat
        .import(
            &source(&drawing(3, "V1")),
            &Context::of(ed.doc()),
            &opts(BaseMode::Link),
        )
        .unwrap_err();
    assert!(e.to_string().contains("file on disk"), "{e}");
    // A path that is not valid (a control character in the name) is refused, not stored.
    if cfg!(unix) {
        let dir = temp("kadakas-base-link-name");
        let path = dir.path().join("bad\u{7}name.dxf");
        std::fs::write(&path, drawing(3, "V1")).unwrap();
        let e = kadakas_io::import_file(&path, None, &opts(BaseMode::Link), ed.doc()).unwrap_err();
        assert!(e.to_string().contains("cannot keep a link"), "{e}");
    }
    // Without source tracking there is no update data to decide.
    let o = ImportOptions {
        track_source: false,
        ..opts(BaseMode::Link)
    };
    let report = run(&mut ed, &drawing(3, "V1"), &o, None).unwrap();
    assert_eq!(report.base, None);
}

#[test]
fn a_later_import_in_auto_mode_replaces_the_link_with_the_summaries() {
    let dir = temp("kadakas-base-relink");
    let path = dir.path().join("survey.dxf");
    std::fs::write(&path, drawing(10, "V1")).unwrap();
    let mut ed = Editor::new(Document::with_sequential_ids());
    let linked = ImportOptions {
        base: BaseMode::Link,
        ..ImportOptions::default()
    };
    let r = kadakas_io::import_file(&path, None, &linked, ed.doc()).unwrap();
    ed.execute(&r.command).unwrap();
    assert!(sources(ed.doc())[0].link.is_some());
    // A new revision, imported with the summaries: the layer is reused and the link is gone.
    std::fs::write(&path, drawing(10, "V2")).unwrap();
    let r = kadakas_io::import_file(&path, None, &ImportOptions::default(), ed.doc()).unwrap();
    ed.execute(&r.command).unwrap();
    assert_eq!(r.report.base.as_deref(), Some("auto"));
    let src = sources(ed.doc());
    assert_eq!(src.len(), 1, "the layer of the drawing is reused");
    assert!(src[0].link.is_none());
    assert_eq!(bases(ed.doc()), 1);
}

#[test]
fn the_option_reads_from_json() {
    for (text, want) in [
        ("auto", BaseMode::Auto),
        ("link", BaseMode::Link),
        ("none", BaseMode::Off),
    ] {
        let o = ImportOptions::from_json(&serde_json::json!({"base": text})).unwrap();
        assert_eq!(o.base, want);
        assert_eq!(want.name(), text);
    }
    assert!(ImportOptions::from_json(&serde_json::json!({"base": "maybe"})).is_err());
    assert_eq!(
        ImportOptions::from_json(&serde_json::Value::Null)
            .unwrap()
            .base,
        BaseMode::Auto
    );
}
