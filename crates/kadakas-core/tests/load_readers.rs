//! The readers of a document (`from_reader`, `from_reader_with`) and the questions about pictures
//! that stay in a file, for a document with no picture and for a text that is not a document.

use std::io::Cursor;

use kadakas_core::command::Editor;
use kadakas_core::model::Document;

fn text() -> String {
    let ed = Editor::with_sequential_ids();
    let mut v = Vec::new();
    ed.doc().write_compact(&mut v).unwrap();
    String::from_utf8(v).unwrap()
}

#[test]
fn a_document_is_read_from_a_reader_with_or_without_seek() {
    let t = text();
    let a = Document::from_reader(Cursor::new(t.clone().into_bytes())).unwrap();
    let b = Document::from_reader_with(
        t.as_bytes(),
        || Ok(Cursor::new(t.clone().into_bytes())),
        |doc| {
            assert!(doc.objects.is_empty());
            Ok(vec!["a note from the container".to_string()])
        },
    )
    .unwrap();
    assert_eq!(a.objects.len(), b.objects.len());
    assert!(
        b.load_notes()
            .iter()
            .any(|n| n.contains("note from the container"))
    );
}

#[test]
fn a_text_that_is_not_a_document_gives_an_error_from_both_readers() {
    let bad = br#"{"format_version": "x", "layers": 5}"#.to_vec();
    assert!(Document::from_reader(Cursor::new(bad.clone())).is_err());
    let r = Document::from_reader_with(
        &bad[..],
        || Ok(Cursor::new(bad.clone())),
        |_| Ok(Vec::new()),
    );
    assert!(r.is_err());
    // A text that stops in the middle.
    let cut = text().as_bytes()[..20].to_vec();
    assert!(Document::from_reader(Cursor::new(cut.clone())).is_err());
    assert!(Document::from_slice(&cut).is_err());
}

#[test]
fn a_document_without_pictures_has_nothing_to_relink_or_to_check() {
    let doc = Document::from_json(&text()).unwrap();
    assert!(doc.placeholder_note().is_none());
    assert!(doc.placeholder_hashes().is_empty());
    assert!(doc.unreadable_pictures().is_empty());
    assert!(doc.check_blobs().is_ok());
    let mut n = 0;
    doc.for_each_blob(&mut |_| n += 1);
    assert_eq!(n, 0);
}
