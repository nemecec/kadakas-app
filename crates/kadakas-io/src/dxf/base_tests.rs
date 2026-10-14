//! The choice of the update data (`base`) with a small cap, so that no test makes 200000 entities.
//! The tests with the real cap are in `tests/dxf_base.rs`.

use kadakas_core::command::Editor;
use kadakas_core::model::Document;
use kadakas_core::provenance::{self, FileLink};

use super::import::import_capped;
use crate::{BaseMode, Context, ImportOptions, IoError, Source};

fn drawing(n: usize, version: &str) -> Source {
    let mut s = format!(
        "0\nSECTION\n2\nHEADER\n9\n$FINGERPRINTGUID\n2\n{{CAP-1}}\n9\n$VERSIONGUID\n2\n{{{version}}}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n"
    );
    for k in 0..n {
        s.push_str(&format!(
            "0\nPOINT\n5\n{:X}\n8\nP\n10\n{k}\n20\n0\n",
            k + 16
        ));
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    Source {
        name: "cap.dxf".into(),
        data: s.into_bytes(),
        extra: Default::default(),
    }
}

fn opts(base: BaseMode) -> ImportOptions {
    ImportOptions {
        base,
        ..ImportOptions::default()
    }
}

fn run(
    ed: &mut Editor,
    src: &Source,
    base: BaseMode,
    link: Option<&FileLink>,
    cap: usize,
) -> Result<crate::Report, IoError> {
    let r = import_capped(src, &Context::of(ed.doc()), &opts(base), link, cap)?;
    ed.execute(&r.command).unwrap();
    Ok(r.report)
}

fn base_count(doc: &Document) -> usize {
    doc.resources
        .iter()
        .filter(|r| matches!(r.body, kadakas_core::resource::ResourceBody::ImportBase(_)))
        .count()
}

fn link_of(doc: &Document) -> Option<FileLink> {
    doc.layers
        .iter()
        .filter_map(|l| provenance::LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
        .find_map(|s| s.link)
}

fn fresh() -> Editor {
    Editor::new(Document::with_sequential_ids())
}

#[test]
fn auto_stores_up_to_the_cap_and_refuses_above_it() {
    let mut ed = fresh();
    let report = run(&mut ed, &drawing(10, "V1"), BaseMode::Auto, None, 10).unwrap();
    assert_eq!(report.base.as_deref(), Some("auto"));
    assert_eq!(base_count(ed.doc()), 1);

    let mut ed = fresh();
    let before = ed.doc().clone();
    let e = run(&mut ed, &drawing(11, "V1"), BaseMode::Auto, None, 10).unwrap_err();
    assert_eq!(
        e,
        IoError::BaseChoice {
            entities: 11,
            cap: 10
        }
    );
    assert_eq!(ed.doc(), &before);
}

#[test]
fn the_count_includes_the_entities_of_blocks() {
    let text = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n0\n20\n0\n\
0\nPOINT\n10\n1\n20\n1\n0\nPOINT\n10\n2\n20\n2\n0\nENDBLK\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n0\nPOINT\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n";
    let src = Source {
        name: "blocks.dxf".into(),
        data: text.as_bytes().to_vec(),
        extra: Default::default(),
    };
    let e = run(&mut fresh(), &src, BaseMode::Auto, None, 2).unwrap_err();
    assert_eq!(
        e,
        IoError::BaseChoice {
            entities: 3,
            cap: 2
        }
    );
    assert!(run(&mut fresh(), &src, BaseMode::Auto, None, 3).is_ok());
}

#[test]
fn none_imports_a_big_drawing_without_update_data() {
    let mut ed = fresh();
    let report = run(&mut ed, &drawing(11, "V1"), BaseMode::Off, None, 10).unwrap();
    assert_eq!(report.base.as_deref(), Some("none"));
    assert_eq!(report.total(), 11);
    assert_eq!(base_count(ed.doc()), 0);
    assert!(link_of(ed.doc()).is_none());
    assert!(report.notes.iter().any(|n| n.contains("no update data")));
    assert!(
        ed.doc()
            .objects
            .values()
            .all(|o| provenance::tag(&o.data).is_some())
    );
}

#[test]
fn link_imports_a_big_drawing_with_the_link_on_the_layer_source() {
    let link = FileLink::of_bytes("/data/big.dxf", b"the file");
    let mut ed = fresh();
    let report = run(&mut ed, &drawing(11, "V1"), BaseMode::Link, Some(&link), 10).unwrap();
    assert_eq!(report.base.as_deref(), Some("link"));
    assert_eq!(report.total(), 11);
    assert_eq!(base_count(ed.doc()), 0);
    assert_eq!(link_of(ed.doc()), Some(link));
    assert!(report.notes.iter().any(|n| n.contains("original file")));
}

#[test]
fn a_revision_that_the_document_keeps_is_not_refused_again() {
    let mut ed = fresh();
    run(&mut ed, &drawing(5, "V1"), BaseMode::Auto, None, 10).unwrap();
    // The same revision again, with a cap that it would not fit: its base is in the document.
    let report = run(&mut ed, &drawing(5, "V1"), BaseMode::Auto, None, 3).unwrap();
    assert_eq!(report.base.as_deref(), Some("auto"));
    assert_eq!(base_count(ed.doc()), 1);
    // A new revision is a new base, and the cap applies.
    assert!(run(&mut ed, &drawing(5, "V2"), BaseMode::Auto, None, 3).is_err());
}
