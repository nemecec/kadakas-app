//! Tests of the file forms: round trips, lazy pictures, raw copy, and hostile containers.

use std::io::Write as _;
use std::sync::Arc;

use glam::DVec2;
use kadakas_container::{Compression, Container, Limits, Writer, sha256_hex};
use kadakas_core::blob::{BlobText, text_hash};
use kadakas_core::history::HistoryData;
use kadakas_core::model::{Document, FIRST_LAYER, NONE_CLASS, Object, ObjectId, Shape};
use kadakas_core::picture::{ImageResource, PictureFormat, encode_base64, sha256_hex as file_hash};
use kadakas_core::resource::{Resource, ResourceBody, ResourceId};
use kadakas_core::underlay::{PngData, Underlay};
use serde_json::{Value, json};

use super::*;

fn png_bytes(seed: u8, len: usize) -> Vec<u8> {
    // The header of a PNG of 10 x 10 pixels, then bytes that are no image.
    let mut png = vec![
        0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 10,
        0, 0, 0, 10, 8, 6, 0, 0, 0,
    ];
    png.extend((0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)));
    png
}

fn underlay(seed: u8) -> Shape {
    Shape::Underlay(Box::new(Underlay {
        name: format!("u{seed}"),
        min: DVec2::ZERO,
        max: DVec2::new(100.0, 100.0),
        width: 10,
        height: 10,
        png: PngData::new(png_bytes(seed, 20_000)),
        opacity: 1.0,
        source: String::new(),
    }))
}

fn image(seed: u8) -> ImageResource {
    let mut file = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    file.extend(b"IHDR");
    file.extend([0, 0, 0, 2, 0, 0, 0, 2, 8, 6, 0, 0, 0]);
    file.extend((0..9000).map(|i| (i as u8).wrapping_add(seed)));
    let mut thumb = file[..33].to_vec();
    thumb.push(seed);
    ImageResource {
        sha256: file_hash(&file),
        format: PictureFormat::Png,
        width: 2,
        height: 2,
        data: encode_base64(&file).into(),
        thumb: encode_base64(&thumb).into(),
        source: "leaf.png".into(),
    }
}

fn base_body(n: usize) -> Value {
    let rec = |i: usize| {
        json!({"handle": format!("{i:X}"), "scope": "", "kind": "LINE", "points": [[0.0, 0.0], [1.0, i as f64]],
               "layer": "walls and fences", "color": "7", "hash": format!("{i:016x}")})
    };
    json!({"type": "import_base", "drawing": "d", "revision": "r", "unit_scale": 1.0,
           "recs": (0..n).map(rec).collect::<Vec<_>>()})
}

/// A document with a picture, an image resource, an import base and a plain object.
fn sample_doc() -> Document {
    let mut doc = Document::with_sequential_ids();
    let add = |doc: &mut Document, n: u64, shape: Shape| {
        let o = Object::new(ObjectId::from_num(n), FIRST_LAYER, NONE_CLASS, shape);
        doc.objects.insert(o.id, o);
    };
    add(
        &mut doc,
        10,
        Shape::Circle {
            center: DVec2::new(5.0, 5.0),
            radius: 2.0,
        },
    );
    add(&mut doc, 11, underlay(1));
    add(&mut doc, 12, underlay(2));
    doc.resources.push(Resource {
        id: ResourceId::from_num(30),
        name: "Leaf".into(),
        body: ResourceBody::Image(Box::new(image(7))),
    });
    doc.resources.push(Resource {
        id: ResourceId::from_num(31),
        name: "Base".into(),
        body: ResourceBody::ImportBase(Arc::new(base_body(200))),
    });
    doc
}

fn sample_history() -> HistoryData {
    let blob = "iVBORw0KGgo".repeat(400);
    let hash = text_hash(&blob);
    HistoryData::from_parts(
        json!({"format": 2, "next_id": 3, "branch": "main", "head": 2,
               "newest": {"id": 2, "anchor": "ab".repeat(32)}}),
        vec![
            Arc::new(json!({"id": 1, "name": "first", "delta": [{"op": "set"}]})),
            Arc::new(json!({"id": 2, "name": "second", "delta": []})),
        ],
        vec![Arc::new(json!({"op": "create", "n": 1}))],
        vec![(hash, Arc::from(blob.as_str()))],
    )
}

fn container_bytes(doc: &Document) -> Vec<u8> {
    let opts = WriteOptions::default();
    write_container(Vec::new(), doc, &opts).unwrap().0
}

fn open(bytes: Vec<u8>) -> Opened {
    read_bytes(bytes, Limits::default(), ReadMode::Lazy).unwrap()
}

/// The document with the history that waited in the file read into it.
fn resolved(mut o: Opened) -> Document {
    resolve_history(&mut o.doc).unwrap();
    o.doc
}

#[test]
fn a_container_round_trips_and_keeps_the_pictures_in_the_file() {
    let doc = sample_doc();
    let bytes = container_bytes(&doc);
    let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
    let names: Vec<_> = c.entries().iter().map(|e| e.name.clone()).collect();
    assert_eq!(names[0], DOCUMENT);
    // Two underlays, one image and one import base are blobs.
    assert_eq!(
        names.iter().filter(|n| n.starts_with(BLOB_DIR)).count(),
        4,
        "{names:?}"
    );
    let opened = open(bytes);
    assert_eq!(opened.form, Form::Container);
    assert!(opened.source.is_some());
    assert_eq!(opened.doc, doc, "the same model");
    let mut lazy = 0;
    opened.doc.for_each_blob(&mut |b| match b {
        kadakas_core::model::DocBlob::Picture(p) => lazy += usize::from(p.is_lazy()),
        kadakas_core::model::DocBlob::Image(t) => lazy += usize::from(t.is_lazy()),
        kadakas_core::model::DocBlob::ImportBase(_) => {}
    });
    assert_eq!(lazy, 3, "the pictures wait in the file");
    // The picture is read when it is used.
    let Shape::Underlay(u) = &opened.doc.objects[&ObjectId::from_num(11)].shape else {
        panic!()
    };
    assert_eq!(*u.png.bytes().unwrap(), png_bytes(1, 20_000));
}

#[test]
fn the_document_json_has_references_and_no_pictures() {
    let bytes = container_bytes(&sample_doc());
    let c = Container::from_bytes(bytes, Limits::default()).unwrap();
    let text = String::from_utf8(c.read(DOCUMENT).unwrap()).unwrap();
    assert!(text.contains("\"$blob\""));
    assert_eq!(
        text.matches("$blob").count(),
        4,
        "three pictures and one base"
    );
    assert!(text.len() < 5_000, "{}", text.len());
    // Each blob is named by the hash of its text.
    for e in c.entries().iter().filter(|e| e.name.starts_with(BLOB_DIR)) {
        assert_eq!(e.name, blob_name(&sha256_hex(&c.read(&e.name).unwrap())));
        assert_eq!(e.method, kadakas_container::Method::Stored);
    }
}

#[test]
fn same_picture_twice_is_one_blob() {
    let mut doc = sample_doc();
    let o = Object::new(ObjectId::from_num(40), FIRST_LAYER, NONE_CLASS, underlay(1));
    doc.objects.insert(o.id, o);
    let c = Container::from_bytes(container_bytes(&doc), Limits::default()).unwrap();
    assert_eq!(
        c.entries()
            .iter()
            .filter(|e| e.name.starts_with(BLOB_DIR))
            .count(),
        4
    );
}

#[test]
fn history_round_trips_in_parts() {
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let bytes = container_bytes(&doc);
    let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
    for name in [HISTORY_TOP, HISTORY_VERSIONS, HISTORY_NEWEST, HISTORY_BLOBS] {
        assert!(c.contains(name), "{name}");
    }
    assert!(!c.contains(HISTORY_RAW));
    assert!(
        !String::from_utf8(c.read(DOCUMENT).unwrap())
            .unwrap()
            .contains("\"history\"")
    );
    let opened = open(bytes.clone());
    // The history waits in the file: it is not in the document and was not read.
    assert!(opened.doc.history.is_none() && opened.doc.lazy_history.is_some());
    let back = resolved(opened);
    assert_eq!(back.history, doc.history);
    assert_eq!(back, doc);
    // An eager read has it at once.
    let eager = read_bytes(bytes, Limits::default(), ReadMode::Eager)
        .unwrap()
        .doc;
    assert!(eager.lazy_history.is_none());
    assert_eq!(eager, doc);
}

/// An eager read of a container whose history is damaged: the drawing loads, there is no history,
/// and the note of the load says what is wrong (REV-01: the drawing never needs the history).
fn eager_note(bytes: Vec<u8>) -> String {
    let opened = read_bytes(bytes, Limits::default(), ReadMode::Eager).unwrap();
    assert!(opened.doc.history.is_none() && opened.doc.lazy_history.is_none());
    let notes = opened.doc.load_notes().join(" ");
    assert!(!notes.is_empty(), "a note says why there are no versions");
    notes
}

#[test]
fn a_damaged_history_is_not_found_at_open_but_when_it_is_used() {
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let good = Container::from_bytes(container_bytes(&doc), Limits::default()).unwrap();
    // The same container with a damaged history entry (a wrong hash in the manifest).
    let mut w = Writer::new(Vec::new());
    for e in good.entries() {
        if e.name == HISTORY_VERSIONS {
            w.add_bytes(&e.name, Compression::Stored, b"[1, 2").unwrap();
        } else {
            w.copy_raw(good.raw(&e.name).unwrap()).unwrap();
        }
    }
    let bytes = w.finish().unwrap();
    let opened = open(bytes.clone());
    let e = opened
        .doc
        .lazy_history
        .as_ref()
        .unwrap()
        .load()
        .unwrap_err();
    assert!(e.contains(HISTORY_VERSIONS), "{e}");
    let mut doc2 = opened.doc;
    let e = resolve_history(&mut doc2).unwrap_err();
    assert!(e.to_string().contains(HISTORY_VERSIONS), "{e}");
    assert!(
        doc2.lazy_history.is_some() && doc2.history.is_none(),
        "still waiting"
    );
    // An eager read loads the drawing and says in a note that the versions are not part of the copy.
    let note = eager_note(bytes);
    assert!(note.contains(HISTORY_VERSIONS), "{note}");
}

#[test]
fn a_history_that_waits_is_copied_without_being_read_and_the_plain_form_needs_it_resolved() {
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let first = open(container_bytes(&doc));
    let source = first.source.clone().unwrap();
    let opts = WriteOptions {
        generator: "t".into(),
        keep: vec![source.clone()],
        anchor: false,
        skip_missing: false,
        allow_overwrite: false,
    };
    let (bytes, report) = write_container(Vec::new(), &first.doc, &opts).unwrap();
    assert_eq!(report.parts_copied, 4, "{report:?}");
    // The history blob came along (the picture of the history is an entry of blobs/).
    let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
    assert_eq!(
        c.entries()
            .iter()
            .filter(|e| e.name.starts_with(BLOB_DIR))
            .count(),
        5
    );
    assert_eq!(resolved(open(bytes)).history, doc.history);
    // The plain form cannot hold what was not read.
    let e = write_json(Vec::new(), &first.doc).unwrap_err();
    assert!(e.to_string().contains("resolve the history"), "{e}");
}

#[test]
fn a_history_of_any_other_shape_is_kept_whole() {
    for v in [
        json!("not an object"),
        json!({"format": 2, "versions": "x"}),
        json!({"format": 2, "newest": {"id": 1}, "versions": [], "blobs": {"not-a-hash": "text"}}),
        json!({"format": 2, "newest": {"id": 1, "delta": []}, "versions": []}),
    ] {
        let mut doc = Document::with_sequential_ids();
        doc.history = Some(HistoryData::new(v.clone()));
        let bytes = container_bytes(&doc);
        let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
        assert!(c.contains(HISTORY_RAW), "{v}");
        let back = resolved(open(bytes));
        assert_eq!(back.history.as_ref().unwrap().to_value(), v);
    }
}

#[test]
fn a_picture_of_the_history_is_a_blob_shared_with_the_drawing() {
    let mut doc = Document::with_sequential_ids();
    let png = png_bytes(9, 20_000);
    let text = encode_base64(&png);
    let o = Object::new(
        ObjectId::from_num(10),
        FIRST_LAYER,
        NONE_CLASS,
        Shape::Underlay(Box::new(Underlay {
            name: "u".into(),
            min: DVec2::ZERO,
            max: DVec2::new(1.0, 1.0),
            width: 1,
            height: 1,
            png: PngData::new(png),
            opacity: 1.0,
            source: String::new(),
        })),
    );
    doc.objects.insert(o.id, o);
    doc.history = Some(HistoryData::from_parts(
        json!({"format": 2, "newest": {"id": 1, "anchor": "00"}}),
        vec![],
        vec![],
        vec![(text_hash(&text), Arc::from(text.as_str()))],
    ));
    let c = Container::from_bytes(container_bytes(&doc), Limits::default()).unwrap();
    assert_eq!(
        c.entries()
            .iter()
            .filter(|e| e.name.starts_with(BLOB_DIR))
            .count(),
        1
    );
}

#[test]
fn json_form_has_pictures_inline_and_history_inside_and_loads_the_same_model() {
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let mut json_bytes = Vec::new();
    write_json(&mut json_bytes, &doc).unwrap();
    let text = String::from_utf8(json_bytes.clone()).unwrap();
    assert!(text.contains("iVBORw0KGgo"), "base64 pictures");
    assert!(text.contains("\"history\""));
    assert!(!text.contains("$blob"));
    let from_json = read_bytes(json_bytes, Limits::default(), ReadMode::Lazy).unwrap();
    assert_eq!(from_json.form, Form::Json);
    assert!(from_json.source.is_none());
    assert_eq!(from_json.doc, doc);
    // The text is exactly what the old save wrote.
    assert_eq!(text, serde_json::to_string_pretty(&doc).unwrap());
}

#[test]
fn container_to_json_to_container_gives_the_same_document_and_history() {
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let a = resolved(open(container_bytes(&doc)));
    let mut json_bytes = Vec::new();
    write_json(&mut json_bytes, &a).unwrap();
    let b = read_bytes(json_bytes, Limits::default(), ReadMode::Lazy).unwrap();
    let c = resolved(open(container_bytes(&b.doc)));
    assert_eq!(c, doc);
    assert_eq!(c.history, doc.history);
    assert_eq!(a, b.doc);
}

#[test]
fn raw_copy_keeps_the_compressed_bytes_of_unchanged_entries() {
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let first = open(container_bytes(&doc));
    let source = first.source.clone().unwrap();
    let opts = WriteOptions {
        generator: "t".into(),
        keep: vec![source.clone()],
        anchor: false,
        skip_missing: false,
        allow_overwrite: false,
    };
    let (bytes, report) = write_container(Vec::new(), &first.doc, &opts).unwrap();
    // Every blob is in the old file: the pictures and the import base are copied, and the picture
    // of the history waits there with the history.
    // The summaries of the import base are in memory (they are read with the drawing): they are
    // written from there, not copied from a file that may have changed.
    assert_eq!(report.blobs_copied, 3, "{report:?}");
    assert_eq!(report.blobs_kept, 1, "{report:?}");
    assert_eq!(report.blobs_written, 1, "{report:?}");
    assert_eq!(report.parts_copied, 4);
    assert_eq!(
        resolved(open(b_bytes(
            &Container::from_bytes(
                write_container(Vec::new(), &first.doc, &opts).unwrap().0,
                Limits::default()
            )
            .unwrap()
        ))),
        doc
    );
    let a = source.container();
    let b = Container::from_bytes(bytes, Limits::default()).unwrap();
    for e in a.entries().iter().filter(|e| e.name != DOCUMENT) {
        let f = b.entry(&e.name).unwrap();
        assert_eq!(
            (f.sha256, f.crc32, f.compressed),
            (e.sha256, e.crc32, e.compressed),
            "{}",
            e.name
        );
    }
    // The second file reads like the first.
    assert_eq!(resolved(open(b_bytes(&b))), doc);
}

fn b_bytes(c: &Container) -> Vec<u8> {
    let m = c.medium();
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut m.open(0, m.len()).unwrap(), &mut out).unwrap();
    out
}

#[test]
fn a_picture_that_is_not_in_the_file_is_an_error_naming_it() {
    let bytes = container_bytes(&sample_doc());
    let c = Container::from_bytes(bytes, Limits::default()).unwrap();
    let blob = c
        .entries()
        .iter()
        .find(|e| e.name.starts_with(BLOB_DIR))
        .unwrap()
        .name
        .clone();
    // The same container without that blob.
    let mut w = Writer::new(Vec::new());
    for e in c.entries().iter().filter(|e| e.name != blob) {
        w.copy_raw(c.raw(&e.name).unwrap()).unwrap();
    }
    let e = read_bytes(w.finish().unwrap(), Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(e.to_string().contains("not in the"), "{e}");
}

#[test]
fn a_blob_with_the_wrong_content_for_its_name_is_not_used() {
    let text = "AAAA".repeat(2000);
    let mut doc = Document::with_sequential_ids();
    let o = Object::new(ObjectId::from_num(10), FIRST_LAYER, NONE_CLASS, underlay(3));
    doc.objects.insert(o.id, o);
    let good = container_bytes(&doc);
    let c = Container::from_bytes(good, Limits::default()).unwrap();
    let blob = c
        .entries()
        .iter()
        .find(|e| e.name.starts_with(BLOB_DIR))
        .unwrap()
        .name
        .clone();
    let mut w = Writer::new(Vec::new());
    w.copy_raw(c.raw(DOCUMENT).unwrap()).unwrap();
    // Another content under the name of the first.
    w.add_bytes(&blob, Compression::Stored, text.as_bytes())
        .unwrap();
    let e = read_bytes(w.finish().unwrap(), Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(e.to_string().contains("not in the"), "{e}");
}

#[test]
fn a_reference_in_a_plain_json_file_is_an_error() {
    let json = format!(
        r#"{{"format_version":3,"layers":[],"classes":[],"objects":[],"active_layer":1,"active_class":0,
            "resources":[{{"id":"{}","name":"x","body":{{"type":"image","data":{{"$blob":"{}"}}}}}}]}}"#,
        ResourceId::from_num(5),
        text_hash("x")
    );
    assert!(read_bytes(json.into_bytes(), Limits::default(), ReadMode::Lazy).is_err());
}

#[test]
fn damaged_documents_name_the_entry() {
    let bytes = container_bytes(&sample_doc());
    let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
    // A container without a drawing.
    let mut w = Writer::new(Vec::new());
    w.add_bytes("blobs/x", Compression::Stored, b"1").unwrap();
    let e = read_bytes(w.finish().unwrap(), Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(e.to_string().contains("document.json"), "{e}");
    // A drawing that is not JSON.
    let mut w = Writer::new(Vec::new());
    w.add_bytes(DOCUMENT, Compression::Deflate(1), b"{nope")
        .unwrap();
    let e = read_bytes(w.finish().unwrap(), Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(e.to_string().contains("not a Kadakas file"), "{e}");
    // A newer drawing says so.
    let mut w = Writer::new(Vec::new());
    w.add_bytes(
        DOCUMENT,
        Compression::Deflate(1),
        br#"{"format_version":99,"layers":"?"}"#,
    )
    .unwrap();
    let e = read_bytes(w.finish().unwrap(), Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(e.to_string().contains("newer Kadakas (format 99)"), "{e}");
    // A drawing and a history in two places.
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let mut json_bytes = Vec::new();
    write_json(&mut json_bytes, &doc).unwrap();
    let mut w = Writer::new(Vec::new());
    w.add_bytes(DOCUMENT, Compression::Deflate(1), &json_bytes)
        .unwrap();
    let both = container_bytes(&doc);
    let cb = Container::from_bytes(both, Limits::default()).unwrap();
    for e in cb.entries().iter().filter(|e| e.name != DOCUMENT) {
        w.copy_raw(cb.raw(&e.name).unwrap()).unwrap();
    }
    let e = read_bytes(w.finish().unwrap(), Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(e.to_string().contains("both hold a history"), "{e}");
    let _ = c;
}

#[test]
fn bad_history_entries_name_the_entry() {
    let mut doc = Document::with_sequential_ids();
    doc.history = Some(sample_history());
    let good = Container::from_bytes(container_bytes(&doc), Limits::default()).unwrap();
    let build = |replace: &str, with: &[u8]| {
        let mut w = Writer::new(Vec::new());
        for e in good.entries() {
            if e.name == replace {
                w.add_bytes(&e.name, Compression::Stored, with).unwrap();
            } else {
                w.copy_raw(good.raw(&e.name).unwrap()).unwrap();
            }
        }
        // The history is read when it is used: an eager read finds the damage at once, loads the
        // drawing, and says it in a note.
        eager_note(w.finish().unwrap())
    };
    assert!(build(HISTORY_TOP, b"{").contains(HISTORY_TOP));
    assert!(build(HISTORY_VERSIONS, b"{}").contains("not a list"));
    assert!(build(HISTORY_NEWEST, b"nope").contains(HISTORY_NEWEST));
    assert!(build(HISTORY_BLOBS, b"[5]").contains("not a hash"));
    assert!(build(HISTORY_BLOBS, br#"["aa"]"#).contains("not a hash"));
    let missing = format!("[\"{}\"]", text_hash("nothing"));
    assert!(build(HISTORY_BLOBS, missing.as_bytes()).contains("not in the container"));
}

#[test]
fn the_form_is_told_from_the_bytes_and_unknown_bytes_are_an_error() {
    for bad in [&b""[..], b"hello", b"[]", b"\x89PNG\r\n\x1a\n"] {
        assert!(matches!(
            read_bytes(bad.to_vec(), Limits::default(), ReadMode::Lazy),
            Err(FileError::Unknown(_))
        ));
    }
    let e = read_bytes(b"{\"a\":1}".to_vec(), Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(e.to_string().contains("format_version"), "{e}");
}

#[test]
fn limits_apply_to_the_uncompressed_size_of_a_container() {
    let bytes = container_bytes(&sample_doc());
    let file_size = bytes.len() as u64;
    let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
    let total = c.uncompressed_bytes();
    assert!(total > file_size / 2);
    let e = read_bytes(
        bytes.clone(),
        Limits::default().with_total(total - 1),
        ReadMode::Lazy,
    )
    .unwrap_err();
    assert!(
        matches!(
            e,
            FileError::Container(kadakas_container::ContainerError::Limit(_))
        ),
        "{e}"
    );
    assert!(read_bytes(bytes, Limits::default().with_total(total), ReadMode::Lazy).is_ok());
}

#[test]
fn lazy_pictures_serialize_inline_without_a_scope() {
    let doc = sample_doc();
    let opened = open(container_bytes(&doc));
    let inline = serde_json::to_string(&opened.doc).unwrap();
    assert_eq!(inline, serde_json::to_string(&doc).unwrap());
    // And a lazy text is equal to the text in memory.
    let ImageOf(a) = ImageOf::of(&opened.doc);
    let ImageOf(b) = ImageOf::of(&doc);
    assert_eq!(a, b);
}

struct ImageOf(BlobText);

impl ImageOf {
    fn of(doc: &Document) -> ImageOf {
        match &doc.resources.get(ResourceId::from_num(30)).unwrap().body {
            ResourceBody::Image(i) => ImageOf(i.data.clone()),
            _ => panic!(),
        }
    }
}

#[test]
fn the_generator_names_the_program() {
    assert!(generator().starts_with("kadakas "));
    let _ = std::io::sink().flush();
}

#[test]
fn errors_know_where_they_come_from() {
    let inner = kadakas_container::ContainerError::Entry {
        name: "blobs/x".into(),
        reason: "bad".into(),
    };
    let io: std::io::Error = inner.clone().into();
    assert!(matches!(FileError::from(io), FileError::Container(c) if c == inner));
    assert!(matches!(
        FileError::from(std::io::Error::other("plain")),
        FileError::Io(m) if m == "plain"
    ));
    assert!(
        FileError::Io("the file x changed on disk after it was opened (y)".into()).is_changed()
    );
    assert!(!FileError::Io("disk full".into()).is_changed());
    assert!(!FileError::Limit("x".into()).is_changed());
    assert!(
        FileError::Container(kadakas_container::ContainerError::Changed("c".into())).is_changed()
    );
}

#[test]
fn opened_has_a_short_debug_text_and_a_source_has_a_name() {
    let o = open(container_bytes(&sample_doc()));
    let text = format!("{o:?}");
    assert!(
        text.contains("Container") && text.contains("objects"),
        "{text}"
    );
    let s = o.source.unwrap();
    assert!(!s.label().is_empty());
    assert!(s.check_unchanged().is_ok());
    assert!(s.refresh().is_ok());
    assert!(s.live_hashes().len() <= 4);
    assert!(kadakas_core::blob::BlobSource::as_any(&*s).is::<FileSource>());
    let any = kadakas_core::blob::BlobSource::into_any(s.clone());
    assert!(any.downcast::<FileSource>().is_ok());
}

#[test]
fn the_list_of_live_pictures_is_pruned() {
    use kadakas_core::blob::{BlobSource, LazyBlob};
    // A container with more pictures than the list keeps before it prunes.
    let mut w = Writer::new(Vec::new());
    let mut hashes = Vec::new();
    for i in 0..4100 {
        let text = format!("picture number {i}");
        let h = text_hash(&text);
        w.add_bytes(&blob_name(&h), Compression::Stored, text.as_bytes())
            .unwrap();
        hashes.push(h);
    }
    let c = Container::from_bytes(w.finish().unwrap(), Limits::default()).unwrap();
    let source = FileSource::from_container(c);
    let mut keep = Vec::new();
    for (i, h) in hashes.iter().enumerate() {
        let blob = Arc::new(LazyBlob::new(source.clone(), h).unwrap());
        source.note_handle(&blob);
        // Every tenth handle stays alive.
        if i % 10 == 0 {
            keep.push(blob);
        }
    }
    let live = source.live_hashes();
    assert!(
        live.len() >= keep.len() && live.len() < 4100,
        "{}",
        live.len()
    );
    for b in &keep {
        assert!(live.iter().any(|h| h == b.hash()));
    }
}

/// A container with a hand-made history.
fn container_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = Writer::new(Vec::new());
    w.add_bytes(
        DOCUMENT,
        Compression::Deflate(1),
        serde_json::to_string(&Document::with_sequential_ids())
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    for (name, bytes) in entries {
        w.add_bytes(name, Compression::Stored, bytes).unwrap();
    }
    w.finish().unwrap()
}

#[test]
fn damaged_raw_histories_and_pictures_of_the_history_are_errors_that_name_the_entry() {
    let read = eager_note;
    assert!(read(container_with(&[(HISTORY_RAW, b"{nope")])).contains(HISTORY_RAW));
    // A picture of the history that is not text.
    let binary = [0xFFu8, 0xFE, 0x00, 0x01];
    let hash = sha256_hex(&binary);
    let listed = format!("[\"{hash}\"]");
    let name = blob_name(&hash);
    let bytes = container_with(&[
        (HISTORY_TOP, br#"{"newest": {}}"#),
        (HISTORY_VERSIONS, b"[]"),
        (HISTORY_NEWEST, b"[]"),
        (HISTORY_BLOBS, listed.as_bytes()),
        (&name, &binary),
    ]);
    let e = read(bytes.clone());
    assert!(e.contains(&name) && e.contains("not text"), "{e}");
    // The history waits: a lazy open does not look at it.
    let o = open(bytes);
    let loader = o.doc.lazy_history.clone().unwrap();
    assert!(loader.load().unwrap_err().contains("not text"));
    assert!(loader.bytes() > 10);
    assert!(loader.anchor().is_ok());
    // A history that lists a picture whose entry is another picture: the picture is not found, and
    // a save does not copy the wrong one.
    let other = b"some other text".to_vec();
    let name2 = blob_name(&text_hash("expected text"));
    let listed2 = format!("[\"{}\"]", text_hash("expected text"));
    let bytes = container_with(&[
        (HISTORY_TOP, br#"{"newest": {}}"#),
        (HISTORY_VERSIONS, b"[]"),
        (HISTORY_NEWEST, b"[]"),
        (HISTORY_BLOBS, listed2.as_bytes()),
        (&name2, &other),
    ]);
    let o = open(bytes);
    let e = write_container(Vec::new(), &o.doc, &WriteOptions::default()).unwrap_err();
    let FileError::Missing(m) = &e else {
        panic!("{e}")
    };
    assert_eq!(m.pictures, [text_hash("expected text")]);
    // Unreadable list of pictures: the save says so.
    let bytes = container_with(&[
        (HISTORY_TOP, br#"{"newest": {}}"#),
        (HISTORY_VERSIONS, b"[]"),
        (HISTORY_NEWEST, b"[]"),
        (HISTORY_BLOBS, b"{}"),
    ]);
    let o = open(bytes);
    let e = write_container(Vec::new(), &o.doc, &WriteOptions::default()).unwrap_err();
    assert!(e.to_string().contains(HISTORY_BLOBS), "{e}");
}

#[test]
fn a_history_with_no_list_of_pictures_has_none() {
    let o = open(container_with(&[(HISTORY_RAW, br#"{"format": 2}"#)]));
    let fh = o
        .doc
        .lazy_history
        .as_ref()
        .unwrap()
        .as_any()
        .downcast_ref::<FileHistory>()
        .unwrap();
    assert!(fh.blob_hashes().unwrap().is_empty());
    assert_eq!(fh.entry_names(), [HISTORY_RAW]);
    assert!(FileHistory::of(&open(container_bytes(&sample_doc())).source.unwrap()).is_none());
}

#[test]
fn a_history_loader_of_another_kind_cannot_be_copied() {
    #[derive(Debug)]
    struct Foreign;
    impl kadakas_core::history::HistoryLoader for Foreign {
        fn load(&self) -> Result<HistoryData, String> {
            Ok(sample_history())
        }
        fn anchor(&self) -> Result<Document, String> {
            Ok(Document::with_sequential_ids())
        }
        fn bytes(&self) -> usize {
            0
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    let mut doc = Document::with_sequential_ids();
    doc.lazy_history = Some(Arc::new(Foreign));
    let e = write_container(Vec::new(), &doc, &WriteOptions::default()).unwrap_err();
    assert!(e.to_string().contains("cannot be copied from"), "{e}");
    // It can be resolved, as it is.
    resolve_history(&mut doc).unwrap();
    assert!(doc.history.is_some() && doc.lazy_history.is_none());
    resolve_history(&mut doc).unwrap();
}

#[test]
fn a_picture_of_another_source_is_read_and_written_like_a_new_one() {
    use kadakas_core::blob::{BlobError, BlobSource, HEAD_CHARS, with_load_source};
    #[derive(Debug)]
    struct Memory(std::collections::HashMap<String, String>, bool);
    impl BlobSource for Memory {
        fn blob_len(&self, hash: &str) -> Option<u64> {
            self.0.get(hash).map(|t| t.len() as u64)
        }
        fn blob_head(&self, hash: &str) -> Result<String, BlobError> {
            Ok(self.0[hash].chars().take(HEAD_CHARS).collect())
        }
        fn blob_text(&self, hash: &str) -> Result<Arc<str>, BlobError> {
            if self.1 {
                return Err(BlobError("the memory failed".into()));
            }
            Ok(Arc::from(self.0[hash].as_str()))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn into_any(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
            self
        }
    }
    let png = encode_base64(&png_bytes(3, 9000));
    let image = {
        let mut doc = sample_doc();
        doc.resources = Default::default();
        doc.resources.push(Resource {
            id: ResourceId::from_num(30),
            name: "Leaf".into(),
            body: ResourceBody::Image(Box::new(image(7))),
        });
        doc
    };
    let image_text = match &image.resources.iter().next().unwrap().body {
        ResourceBody::Image(i) => i.data.text().unwrap().to_string(),
        _ => unreachable!(),
    };
    for failing in [false, true] {
        let mut table = std::collections::HashMap::new();
        table.insert(text_hash(&png), png.clone());
        table.insert(text_hash(&image_text), image_text.clone());
        let source = Arc::new(Memory(table, failing));
        let json = serde_json::to_string(&with_refs(&image, &png)).unwrap();
        let doc: Document = with_load_source(source, kadakas_core::blob::LoadKind::Lazy, || {
            serde_json::from_str(&json)
        })
        .unwrap();
        let out = write_container(Vec::new(), &doc, &WriteOptions::default());
        if failing {
            let e = out.unwrap_err();
            assert!(e.to_string().contains("the memory failed"), "{e}");
        } else {
            let back = resolved(open(out.unwrap().0));
            let Shape::Underlay(u) = &back.objects[&ObjectId::from_num(12)].shape else {
                panic!()
            };
            assert_eq!(u.png.base64().unwrap().as_ref(), png.as_str());
        }
    }
}

/// The JSON of a document with an underlay and an image, the pictures as references.
fn with_refs(image_doc: &Document, png: &str) -> Value {
    let mut v = serde_json::to_value(image_doc).unwrap();
    let image_ref = match &image_doc.resources.iter().next().unwrap().body {
        ResourceBody::Image(i) => i.data.hash(),
        _ => unreachable!(),
    };
    v["resources"][0]["data"] = json!({"$blob": image_ref});
    v["objects"] = json!([{
        "id": ObjectId::from_num(12), "layer": FIRST_LAYER, "class": NONE_CLASS,
        "shape": {"kind": "underlay", "name": "u", "min": [0, 0], "max": [1, 1], "width": 1,
                  "height": 1, "png": {"$blob": text_hash(png)}, "opacity": 1.0}
    }]);
    v
}

#[test]
fn a_damaged_document_entry_is_reported_as_the_entry() {
    // A stored drawing with a changed byte: the CRC-32 fails at the end of the entry.
    let json = serde_json::to_string(&Document::with_sequential_ids()).unwrap();
    let mut w = Writer::new(Vec::new());
    w.add_bytes(DOCUMENT, Compression::Stored, json.as_bytes())
        .unwrap();
    let mut bytes = w.finish().unwrap();
    let at = bytes
        .windows(10)
        .position(|w| w == b"\"layers\":[")
        .unwrap();
    bytes[at + 2] = b'L';
    let e = read_bytes(bytes, Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(
        matches!(&e, FileError::Container(c) if c.entry_name() == Some(DOCUMENT)),
        "{e}"
    );
    // A container with no drawing.
    let e = read_bytes(
        container_with(&[]).clone(),
        Limits::default(),
        ReadMode::Lazy,
    )
    .map(|_| ());
    assert!(e.is_ok());
    let mut w = Writer::new(Vec::new());
    w.add_bytes("blobs/x", Compression::Stored, b"1").unwrap();
    let e = read_bytes(w.finish().unwrap(), Limits::default(), ReadMode::Lazy).unwrap_err();
    assert!(e.to_string().contains(DOCUMENT));
}

#[test]
fn blobs_that_are_too_small_to_be_blobs_stay_inline() {
    let mut doc = Document::with_sequential_ids();
    let small = kadakas_core::picture::header_only_sample(1);
    doc.resources.push(Resource {
        id: ResourceId::from_num(40),
        name: "tiny".into(),
        body: ResourceBody::ImportBase(Arc::new(json!({
            "type": "import_base", "drawing": "d", "revision": "r", "unit_scale": 1.0, "recs": []
        }))),
    });
    doc.resources.push(Resource {
        id: ResourceId::from_num(41),
        name: "small".into(),
        body: ResourceBody::Image(Box::new(small)),
    });
    let bytes = container_bytes(&doc);
    let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
    assert!(!c.entries().iter().any(|e| e.name.starts_with(BLOB_DIR)));
    let back = open(bytes).doc;
    assert_eq!(back.resources.len(), 2);
}

#[test]
fn a_history_that_was_read_copies_its_unchanged_parts_on_the_next_save() {
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let first = open(container_bytes(&doc));
    let source = first.source.clone().unwrap();
    // The history is read (as a session does when a call uses it), nothing is changed.
    let mut again = first.doc;
    resolve_history(&mut again).unwrap();
    let opts = WriteOptions {
        generator: "t".into(),
        keep: vec![source],
        anchor: false,
        skip_missing: false,
        allow_overwrite: false,
    };
    let (bytes, report) = write_container(Vec::new(), &again, &opts).unwrap();
    assert_eq!(report.parts_copied, 4, "{report:?}");
    // The picture of the history and the import base are in memory now: they are written from
    // there.
    assert_eq!(report.blobs_written, 2, "{report:?}");
    assert_eq!(resolved(open(bytes)).history, doc.history);
    // A part that changed is written, and the others are still copied.
    let mut changed = again.clone();
    let h = changed.history.take().unwrap();
    let mut value = h.to_value();
    value["next_id"] = json!(99);
    changed.history = Some(HistoryData::new(value));
    let (_, report) = write_container(Vec::new(), &changed, &opts).unwrap();
    assert_eq!(report.parts_copied, 3, "{report:?}");
}

#[test]
fn a_picture_of_the_history_that_is_too_big_is_refused() {
    // The entry is declared as bigger than a picture may be. Its bytes are never read: the size
    // in the directory is enough, so the test needs no hash of 32 MiB.
    let size = kadakas_core::blob::MAX_BLOB_TEXT as u64 + 1;
    let hash = text_hash("a picture that is too big");
    let name = blob_name(&hash);
    let listed = format!("[\"{hash}\"]");
    let mut w = Writer::new(Vec::new());
    w.add_bytes(
        DOCUMENT,
        Compression::Deflate(1),
        serde_json::to_string(&Document::with_sequential_ids())
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    for (entry, bytes) in [
        (HISTORY_TOP, &br#"{"newest": {}}"#[..]),
        (HISTORY_VERSIONS, b"[]"),
        (HISTORY_NEWEST, b"[]"),
        (HISTORY_BLOBS, listed.as_bytes()),
    ] {
        w.add_bytes(entry, Compression::Stored, bytes).unwrap();
    }
    w.copy_raw(kadakas_container::RawEntry {
        name: name.clone(),
        method: kadakas_container::Method::Stored,
        crc32: 0,
        size,
        compressed: size,
        sha256: [0; 32],
        role: None,
        data: Box::new(std::io::Read::take(std::io::repeat(0), size)),
    })
    .unwrap();
    let e = eager_note(w.finish().unwrap());
    assert!(e.contains(&name) && e.contains("bigger than"), "{e}");
}

/// A document with `n` underlays that all have the same picture.
fn same_picture_doc(n: u64) -> Document {
    let mut doc = Document::with_sequential_ids();
    for i in 0..n {
        let o = Object::new(
            ObjectId::from_num(100 + i),
            FIRST_LAYER,
            NONE_CLASS,
            underlay(1),
        );
        doc.objects.insert(o.id, o);
    }
    doc
}

fn bytes_of(doc: &Document) -> Vec<Arc<Vec<u8>>> {
    doc.objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::Underlay(u) => u.png.mem_bytes().cloned(),
            _ => None,
        })
        .collect()
}

#[test]
fn fifty_objects_with_one_picture_are_one_copy_in_memory_on_every_path() {
    let doc = same_picture_doc(50);
    // The container holds the picture once.
    let bytes = container_bytes(&doc);
    let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
    assert_eq!(
        c.entries()
            .iter()
            .filter(|e| e.name.starts_with("blobs/"))
            .count(),
        1
    );
    // Eager container read: one copy of the bytes.
    let eager = read_bytes(bytes.clone(), Limits::default(), ReadMode::Eager).unwrap();
    let copies = bytes_of(&eager.doc);
    assert_eq!(copies.len(), 50);
    assert!(copies.iter().all(|b| Arc::ptr_eq(b, &copies[0])));
    // Lazy container read: one handle.
    let lazy = read_bytes(bytes, Limits::default(), ReadMode::Lazy).unwrap();
    let handles: Vec<_> = lazy
        .doc
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::Underlay(u) => u.png.lazy().cloned(),
            _ => None,
        })
        .collect();
    assert_eq!(handles.len(), 50);
    assert!(handles.iter().all(|h| Arc::ptr_eq(h, &handles[0])));
    assert_eq!(lazy.source.unwrap().live_hashes().len(), 1);
    // The plain JSON form has 50 copies of the text; the load merges them by SHA-256.
    let mut json = Vec::new();
    write_json(&mut json, &doc).unwrap();
    assert!(json.len() > 50 * 20_000);
    let plain = read_bytes(json, Limits::default(), ReadMode::Lazy).unwrap();
    let copies = bytes_of(&plain.doc);
    assert_eq!(copies.len(), 50);
    assert!(copies.iter().all(|b| Arc::ptr_eq(b, &copies[0])));
}

#[test]
fn an_image_resource_text_and_an_import_base_are_shared_too() {
    let mut doc = Document::with_sequential_ids();
    for n in 0..20u64 {
        doc.resources.push(Resource {
            id: ResourceId::from_num(200 + n),
            name: format!("Leaf {n}"),
            body: ResourceBody::Image(Box::new(image(7))),
        });
        doc.resources.push(Resource {
            id: ResourceId::from_num(300 + n),
            name: format!("Base {n}"),
            body: ResourceBody::ImportBase(Arc::new(base_body(200))),
        });
    }
    let bytes = container_bytes(&doc);
    let check = |doc: &Document| {
        let texts: Vec<_> = doc
            .resources
            .iter()
            .filter_map(|r| match &r.body {
                ResourceBody::Image(i) => i.data.mem_text().cloned(),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 20);
        assert!(texts.iter().all(|t| Arc::ptr_eq(t, &texts[0])));
        let bases: Vec<_> = doc
            .resources
            .iter()
            .filter_map(|r| match &r.body {
                ResourceBody::ImportBase(b) => Some(b.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(bases.len(), 20);
        assert!(bases.iter().all(|b| Arc::ptr_eq(b, &bases[0])));
    };
    // The names differ, so the images differ in their fields but share the text.
    for mode in [ReadMode::Eager, ReadMode::Lazy] {
        let o = read_bytes(bytes.clone(), Limits::default(), mode).unwrap();
        if mode == ReadMode::Eager {
            check(&o.doc);
        } else {
            // Lazy: the base is parsed at open (one copy); the image text stays in the file
            // behind one handle.
            let handles: Vec<_> = o
                .doc
                .resources
                .iter()
                .filter_map(|r| match &r.body {
                    ResourceBody::Image(i) => i.data.lazy().cloned(),
                    _ => None,
                })
                .collect();
            assert!(handles.iter().all(|h| Arc::ptr_eq(h, &handles[0])));
            let bases: Vec<_> = o
                .doc
                .resources
                .iter()
                .filter_map(|r| match &r.body {
                    ResourceBody::ImportBase(b) => Some(b.clone()),
                    _ => None,
                })
                .collect();
            assert!(bases.iter().all(|b| Arc::ptr_eq(b, &bases[0])));
        }
    }
    let mut json = Vec::new();
    write_json(&mut json, &doc).unwrap();
    let plain = read_bytes(json, Limits::default(), ReadMode::Lazy).unwrap();
    check(&plain.doc);
}

#[test]
fn live_handles_of_one_hash_are_all_kept() {
    use kadakas_core::blob::{BlobSource, LazyBlob};
    let doc = same_picture_doc(1);
    let o = open(container_bytes(&doc));
    let source = o.source.clone().unwrap();
    assert_eq!(source.live_hashes().len(), 1);
    let hash = source.live_hashes()[0].clone();
    // A second handle for the same hash (the read of the anchor makes one) does not hide the
    // first, and when the first is gone the second still stands for the picture.
    let src: Arc<dyn BlobSource> = source.clone();
    let second = Arc::new(LazyBlob::new(src, &hash).unwrap());
    source.note_handle(&second);
    drop(o);
    assert_eq!(source.live_hashes(), std::slice::from_ref(&hash));
    drop(second);
    assert!(source.live_hashes().is_empty());
}

#[test]
fn the_anchor_of_the_history_does_not_stand_for_a_picture_in_use() {
    let mut doc = same_picture_doc(1);
    doc.history = Some(sample_history());
    let o = open(container_bytes(&doc));
    let source = o.source.clone().unwrap();
    let loader = o.doc.lazy_history.clone().unwrap();
    let anchor = loader.anchor().unwrap();
    // The document that was opened holds the picture; the anchor read made a handle too, but it
    // is not noted.
    drop(o);
    assert!(
        source.live_hashes().is_empty(),
        "an anchor is read to be looked at"
    );
    drop(anchor);
}

#[test]
fn a_history_that_lists_a_picture_twice_is_refused() {
    let blob = "iVBORw0KGgo".repeat(400);
    let hash = text_hash(&blob);
    let listed = format!("[\"{hash}\",\"{hash}\"]");
    let name = blob_name(&hash);
    let bytes = container_with(&[
        (HISTORY_TOP, br#"{"newest": {}}"#),
        (HISTORY_VERSIONS, b"[]"),
        (HISTORY_NEWEST, b"[]"),
        (HISTORY_BLOBS, listed.as_bytes()),
        (&name, blob.as_bytes()),
    ]);
    let note = eager_note(bytes.clone());
    assert!(
        note.contains(HISTORY_BLOBS) && note.contains("twice"),
        "{note}"
    );
    let o = open(bytes);
    let e = o.doc.lazy_history.unwrap().load().unwrap_err();
    assert!(e.contains("twice"), "{e}");
}

#[test]
fn the_history_is_bounded_by_the_directory_before_it_is_read() {
    // The loader reports the size of its entries and of the pictures they list, without reading.
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let o = open(container_bytes(&doc));
    let loader = o.doc.lazy_history.unwrap();
    let c = o.source.unwrap().container();
    let history: u64 = c
        .entries()
        .iter()
        .filter(|e| e.name.starts_with("history/"))
        .map(|e| e.size)
        .sum();
    let blob = "iVBORw0KGgo".repeat(400).len() as u64;
    assert_eq!(loader.bytes() as u64, history + blob);
}

#[test]
fn a_picture_over_the_bound_is_refused_before_anything_is_written() {
    // 97 MiB of bytes make more than 128 MiB of base64 text. The size is known from the length, so
    // the refusal comes before the text is made.
    let mut doc = Document::with_sequential_ids();
    let mut shape = underlay(1);
    if let Shape::Underlay(u) = &mut shape {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        bytes.resize(100 * 1024 * 1024, 0);
        u.png = PngData::new(bytes);
    }
    let o = Object::new(ObjectId::from_num(5), FIRST_LAYER, NONE_CLASS, shape);
    doc.objects.insert(o.id, o);
    let mut out = Vec::new();
    let e = write_container(&mut out, &doc, &WriteOptions::default()).unwrap_err();
    assert!(matches!(e, FileError::Limit(_)), "{e}");
    assert!(
        e.to_string().contains("128") || e.to_string().contains("134217728"),
        "{e}"
    );
    assert!(out.is_empty(), "nothing was written");
}

#[test]
fn a_reference_amplification_file_cannot_make_a_load_read_more_than_the_file_has() {
    // 100 import bases that all refer to one blob: the blob is read and parsed once.
    let mut doc = Document::with_sequential_ids();
    for n in 0..100u64 {
        doc.resources.push(Resource {
            id: ResourceId::from_num(400 + n),
            name: format!("Base {n}"),
            body: ResourceBody::ImportBase(Arc::new(base_body(300))),
        });
    }
    let bytes = container_bytes(&doc);
    let c = Container::from_bytes(bytes.clone(), Limits::default()).unwrap();
    let blob = c
        .entries()
        .iter()
        .find(|e| e.name.starts_with("blobs/"))
        .unwrap()
        .size;
    // The document text has 100 references; the file is smaller than 100 times the blob.
    assert!(
        c.uncompressed_bytes() < 3 * blob + 100_000,
        "one blob in the file"
    );
    for mode in [ReadMode::Lazy, ReadMode::Eager] {
        let o = read_bytes(bytes.clone(), Limits::default(), mode).unwrap();
        let bases: Vec<_> = o
            .doc
            .resources
            .iter()
            .filter_map(|r| match &r.body {
                ResourceBody::ImportBase(b) => Some(b.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(bases.len(), 100);
        assert!(bases.iter().all(|b| Arc::ptr_eq(b, &bases[0])));
    }
}

#[test]
fn entries_of_a_newer_program_are_kept_not_dropped() {
    // A newer program adds an entry. A save of this program copies it as it is.
    let doc = sample_doc();
    let good = Container::from_bytes(container_bytes(&doc), Limits::default()).unwrap();
    let mut w = Writer::new(Vec::new());
    for e in good.entries() {
        w.copy_raw(good.raw(&e.name).unwrap()).unwrap();
    }
    w.add_bytes(
        "extra/thumbnail.png",
        Compression::Stored,
        b"\x89PNG not really",
    )
    .unwrap();
    let bytes = w.finish().unwrap();
    let o = open(bytes);
    let source = o.source.clone().unwrap();
    let opts = WriteOptions {
        keep: vec![source.clone()],
        ..WriteOptions::default()
    };
    let (out, report) = write_container(Vec::new(), &o.doc, &opts).unwrap();
    assert_eq!(report.unknown_copied, 1, "{report:?}");
    let c = Container::from_bytes(out, Limits::default()).unwrap();
    assert_eq!(
        c.read("extra/thumbnail.png").unwrap(),
        b"\x89PNG not really"
    );
    // A caller that does not name the old file has nothing to copy from.
    let lone = WriteOptions::default();
    let (out, report) = write_container(Vec::new(), &o.doc, &lone).unwrap();
    assert_eq!(report.unknown_copied, 0);
    assert!(
        !Container::from_bytes(out, Limits::default())
            .unwrap()
            .contains("extra/thumbnail.png")
    );
}

#[test]
fn a_picture_in_memory_is_written_from_memory_not_copied_from_a_file_that_may_have_changed() {
    // S6: the picture is in memory and in the file the document was opened from. The file is gone.
    // The save writes the picture from memory.
    let dir = std::env::temp_dir().join(format!("kf-s6-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.kad");
    crate::native::save_path(
        &p,
        &same_picture_doc(1),
        Form::Container,
        &WriteOptions::default(),
    )
    .unwrap();
    let mut o = crate::native::open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
    for obj in o.doc.objects.values_mut() {
        if let Shape::Underlay(u) = &mut obj.shape {
            u.png = PngData::new(png_bytes(1, 20_000));
        }
    }
    std::fs::remove_file(&p).unwrap();
    let opts = WriteOptions {
        keep: o.source.iter().cloned().collect(),
        ..WriteOptions::default()
    };
    let (out, report) = write_container(Vec::new(), &o.doc, &opts).unwrap();
    assert_eq!(report.blobs_written, 1, "{report:?}");
    assert_eq!(report.blobs_copied, 0, "{report:?}");
    assert_eq!(resolved(open(out)), o.doc);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_error_inside_the_encoder_is_not_lost() {
    // S7: the error of a failing source reaches the caller with its text.
    let doc = sample_doc();
    #[derive(Debug)]
    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("the disk is full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let e = write_container(Failing, &doc, &WriteOptions::default()).unwrap_err();
    assert!(e.to_string().contains("the disk is full"), "{e}");
}
#[test]
fn the_scan_for_references_finds_every_one_also_at_the_edge_of_a_chunk() {
    // The references are found in a stream of 1.5 chunks, wherever the cut falls.
    let chunk = 256 * 1024;
    let mut want = std::collections::BTreeSet::new();
    for shift in 0..120usize {
        let hash = text_hash(&format!("picture {shift}"));
        want.insert(hash.clone());
        let mut text = String::from("{\"objects\": [\n");
        // Put the reference so that it starts `shift` bytes before the first cut.
        let reference = format!("{{\"$blob\":  \"{hash}\"}}");
        let pad = chunk - shift - 20;
        text.push_str(&" ".repeat(pad));
        text.push_str(&reference);
        text.push_str(&" ".repeat(chunk / 2));
        // A text that only looks like a reference, and a reference with the white space of a
        // pretty printer.
        text.push_str("\"$blob\" : \"not a hash\"");
        let second = text_hash("second");
        text.push_str(&format!("{{\n  \"$blob\": \"{second}\"\n}}"));
        want.insert(second);
        text.push_str("]}");
        let mut w = Writer::new(Vec::new());
        w.add_bytes(DOCUMENT, Compression::Deflate(1), text.as_bytes())
            .unwrap();
        let c = Container::from_bytes(w.finish().unwrap(), Limits::default()).unwrap();
        let source = FileSource::from_container(c);
        let found = crate::write::scan_refs(&source, DOCUMENT).unwrap();
        let expected: std::collections::BTreeSet<String> =
            [text_hash(&format!("picture {shift}")), text_hash("second")]
                .into_iter()
                .collect();
        assert_eq!(found, expected, "shift {shift}");
    }
    assert_eq!(want.len(), 121);
}

#[test]
fn a_picture_that_the_drawing_and_the_history_share_is_one_copy_in_an_eager_read() {
    let mut doc = Document::with_sequential_ids();
    let img = image(7);
    let text = img.data.text().unwrap();
    let hash = text_hash(&text);
    doc.resources.push(Resource {
        id: ResourceId::from_num(30),
        name: "Leaf".into(),
        body: ResourceBody::Image(Box::new(img)),
    });
    doc.history = Some(HistoryData::from_parts(
        json!({"format": 2, "next_id": 3, "branch": "main", "head": 1,
               "newest": {"id": 1, "anchor": "ab".repeat(32)}}),
        vec![Arc::new(json!({"id": 1, "name": "first", "delta": []}))],
        vec![],
        vec![(hash, text.clone())],
    ));
    let o = read_bytes(container_bytes(&doc), Limits::default(), ReadMode::Eager).unwrap();
    let ResourceBody::Image(i) = &o.doc.resources.iter().next().unwrap().body else {
        panic!()
    };
    let in_drawing = i.data.mem_text().unwrap().clone();
    let history = o.doc.history.as_ref().unwrap();
    assert!(Arc::ptr_eq(&in_drawing, &history.blob_list()[0].1));
}

/// A container of `doc` plus an entry of a newer program.
fn with_unknown_entry(doc: &Document) -> Vec<u8> {
    let good = Container::from_bytes(container_bytes(doc), Limits::default()).unwrap();
    let mut w = Writer::new(Vec::new());
    for e in good.entries() {
        w.copy_raw(good.raw(&e.name).unwrap()).unwrap();
    }
    w.add_bytes("extra/x", Compression::Stored, b"from a newer program")
        .unwrap();
    w.finish().unwrap()
}

#[test]
fn what_only_an_undo_step_or_a_newer_program_needs_is_skipped_only_when_asked() {
    let dir = std::env::temp_dir().join(format!("kf-skip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.kad");
    std::fs::write(&p, with_unknown_entry(&sample_doc())).unwrap();
    let mut o = crate::native::open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
    // The picture of the document is in memory; the other underlay is only held by a handle (an
    // undo step), and the file is gone.
    let held = o.doc.objects.shift_remove(&ObjectId::from_num(12)).unwrap();
    for obj in o.doc.objects.values_mut() {
        if let Shape::Underlay(u) = &mut obj.shape {
            u.png = PngData::new(png_bytes(1, 20_000));
        }
    }
    for r in o.doc.resources.iter_mut() {
        if let ResourceBody::Image(i) = &mut r.body {
            i.data = BlobText::from(i.data.text().unwrap());
        }
    }
    std::fs::remove_file(&p).unwrap();
    let source = o.source.clone().unwrap();
    let opts = WriteOptions {
        keep: vec![source.clone()],
        ..WriteOptions::default()
    };
    let FileError::Missing(m) = write_container(Vec::new(), &o.doc, &opts).unwrap_err() else {
        panic!("not Missing")
    };
    // The picture that only an undo step holds does not stop the save: the drawing is saved, and
    // a note says what the undo step lost. An entry of a newer program needs the person.
    assert!(m.pictures.is_empty(), "{m:?}");
    assert_eq!(m.other, ["extra/x"]);
    assert!(!m.history && !m.is_empty());
    assert!(m.message().contains("1 other part"));
    let ask = WriteOptions {
        skip_missing: true,
        ..opts
    };
    let (out, report) = write_container(Vec::new(), &o.doc, &ask).unwrap();
    assert_eq!(report.skipped, 1, "{report:?}");
    assert_eq!(report.undo_pictures_lost, 1, "{report:?}");
    let c = Container::from_bytes(out, Limits::default()).unwrap();
    assert!(!c.contains("extra/x"));
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn versions_that_wait_in_a_file_that_is_gone_cannot_be_skipped_by_the_writer() {
    let dir = std::env::temp_dir().join(format!("kf-gone-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.kad");
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    std::fs::write(&p, container_bytes(&doc)).unwrap();
    let o = crate::native::open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
    std::fs::remove_file(&p).unwrap();
    for skip in [false, true] {
        let opts = WriteOptions {
            keep: o.source.iter().cloned().collect(),
            anchor: true,
            skip_missing: skip,
            ..WriteOptions::default()
        };
        let e = write_container(Vec::new(), &o.doc, &opts).unwrap_err();
        let FileError::Missing(m) = e else {
            panic!("not Missing")
        };
        assert!(m.history, "skip {skip}: {m:?}");
        assert!(m.message().contains("the versions"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_container_with_too_many_entries_is_refused_before_it_is_written() {
    use crate::write::{Extras, Planned, Plans, preflight_for_test};
    let mut plans = Plans {
        blobs: Default::default(),
        bases: Default::default(),
        sources: Vec::new(),
    };
    for i in 0..3 {
        plans
            .blobs
            .insert(format!("{i:064}"), Planned::History(Arc::from("text")));
    }
    let mut extras = Extras {
        waiting: None,
        anchor: None,
        unknown: Vec::new(),
    };
    let e = preflight_for_test(&mut plans, &mut extras, 4).unwrap_err();
    assert!(
        matches!(e, FileError::Limit(_)) && e.to_string().contains("JSON"),
        "{e}"
    );
    assert_eq!(preflight_for_test(&mut plans, &mut extras, 5).unwrap(), 0);
}

#[test]
fn small_edges_of_the_history_the_source_and_the_scan() {
    // A source without a history reads none.
    let o = open(container_bytes(&sample_doc()));
    let source = o.source.clone().unwrap();
    assert!(crate::history::read(&source).unwrap().is_none());
    assert!(source.container_with_entry("none").is_err());
    assert!(!source.has_blob("zz"));
    // A memory source has no state key; a file source has one that changes with the file.
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let o = open(container_bytes(&doc));
    let loader = o.doc.lazy_history.clone().unwrap();
    assert_eq!(loader.state_key(), "");
    // Many handles of one hash are pruned.
    let hash = o.source.as_ref().unwrap().live_hashes()[0].clone();
    let src: Arc<dyn kadakas_core::blob::BlobSource> = o.source.clone().unwrap();
    let keep: Vec<_> = (0..20)
        .map(|_| {
            let b = Arc::new(kadakas_core::blob::LazyBlob::new(src.clone(), &hash).unwrap());
            src.note_handle(&b);
            drop(Arc::clone(&b));
            b
        })
        .collect();
    assert!(keep.len() == 20);
    // The scan finds nothing at the very end of a stream: a key with nothing after it, a key with
    // something else after it, and a reference that is cut short.
    for tail in [
        "\"$blob\"",
        "\"$blob\"   ",
        "\"$blob\" , 1",
        "\"$blob\": \"abc\"",
        "\"$blob\":",
    ] {
        let mut w = Writer::new(Vec::new());
        w.add_bytes(DOCUMENT, Compression::Stored, tail.as_bytes())
            .unwrap();
        let c = Container::from_bytes(w.finish().unwrap(), Limits::default()).unwrap();
        let s = FileSource::from_container(c);
        assert!(
            crate::write::scan_refs(&s, DOCUMENT).unwrap().is_empty(),
            "{tail}"
        );
    }
    // A copy of an entry that the file does not have.
    let mut w = Writer::new(Vec::new());
    let e = crate::write::copy_entry(&mut w, &source, "nothing", None).unwrap_err();
    assert!(e.to_string().contains("nothing"), "{e}");
}

#[test]
fn the_state_key_of_a_file_changes_when_the_file_does() {
    let dir = std::env::temp_dir().join(format!("kf-key-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.kad");
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    crate::native::save_path(&p, &doc, Form::Container, &WriteOptions::default()).unwrap();
    let o = crate::native::open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
    let loader = o.doc.lazy_history.clone().unwrap();
    let before = loader.state_key();
    assert!(before.contains("a.kad"));
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&p, b"changed").unwrap();
    assert_ne!(loader.state_key(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_history_whose_picture_entry_is_another_picture_is_found_at_the_read() {
    let blob = "iVBORw0KGgo".repeat(400);
    let hash = text_hash(&blob);
    let listed = format!("[\"{hash}\"]");
    let name = blob_name(&hash);
    let bytes = container_with(&[
        (HISTORY_TOP, br#"{"newest": {}}"#),
        (HISTORY_VERSIONS, b"[]"),
        (HISTORY_NEWEST, b"[]"),
        (HISTORY_BLOBS, listed.as_bytes()),
        (&name, b"not the text of that hash"),
    ]);
    let note = eager_note(bytes);
    assert!(note.contains("not the picture of its name"), "{note}");
}

// ---- round 2 ----

#[test]
fn entries_of_a_newer_program_are_kept_with_a_note_also_under_history() {
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    let good = Container::from_bytes(container_bytes(&doc), Limits::default()).unwrap();
    let mut w = Writer::new(Vec::new());
    for e in good.entries() {
        w.copy_raw(good.raw(&e.name).unwrap()).unwrap();
    }
    w.add_bytes("history/graph.json", Compression::Stored, b"{}")
        .unwrap();
    w.add_bytes("extra/x", Compression::Stored, b"from a newer program")
        .unwrap();
    let o = open(w.finish().unwrap());
    let source = o.source.clone().unwrap();
    // The history is read into memory: it is written from there, and the unknown entry under
    // history/ is not dropped either.
    let mut again = o.doc;
    resolve_history(&mut again).unwrap();
    let opts = WriteOptions {
        keep: vec![source.clone()],
        ..WriteOptions::default()
    };
    let (out, report) = write_container(Vec::new(), &again, &opts).unwrap();
    assert_eq!(report.unknown_copied, 2, "{report:?}");
    assert!(
        report.notes.iter().any(|n| n.contains("newer program")),
        "{report:?}"
    );
    let c = Container::from_bytes(out, Limits::default()).unwrap();
    assert!(c.contains("history/graph.json") && c.contains("extra/x"));
    // While the history waits, all its entries are copied, and none is counted twice.
    let o = open(container_bytes(&doc));
    let waiting = WriteOptions {
        keep: o.source.iter().cloned().collect(),
        ..WriteOptions::default()
    };
    let (_, report) = write_container(Vec::new(), &o.doc, &waiting).unwrap();
    assert_eq!(report.unknown_copied, 0);
}

#[test]
fn an_unmarked_import_base_is_left_out_with_a_note_and_the_drawing_opens() {
    let mut doc = Document::with_sequential_ids();
    doc.resources.push(Resource {
        id: ResourceId::from_num(31),
        name: "Base".into(),
        body: ResourceBody::ImportBase(Arc::new(base_body(200))),
    });
    let good = Container::from_bytes(container_bytes(&doc), Limits::default()).unwrap();
    let base_blob = good
        .entries()
        .iter()
        .find(|e| e.role.is_some())
        .unwrap()
        .name
        .clone();
    let build = |keep_role: bool, other_role: bool| {
        let mut w = Writer::new(Vec::new());
        for e in good.entries() {
            let mut raw = good.raw(&e.name).unwrap();
            if e.name == base_blob && !keep_role {
                raw.role = None;
            }
            w.copy_raw(raw).unwrap();
        }
        if other_role {
            w.add_bytes("extra/y", Compression::Stored, b"1").unwrap();
            w.set_role("extra/y", "something");
        }
        w.finish().unwrap()
    };
    // Marked: it opens whole. Not marked, also in a file that marks no blob at all (the open
    // estimate counts only the marked blobs, so nothing else may be read as an import base): the
    // drawing opens, the base is left out, and a note names the drawing that loses its data.
    let whole = read_bytes(build(true, true), Limits::default(), ReadMode::Lazy).unwrap();
    assert!(whole.doc.load_notes().is_empty());
    assert!(kadakas_core::provenance::base_recs(&whole.doc, "d", "r").is_some());
    for (keep_role, other_role) in [(false, true), (false, false)] {
        let opened = read_bytes(
            build(keep_role, other_role),
            Limits::default(),
            ReadMode::Lazy,
        )
        .unwrap();
        let notes = opened.doc.load_notes().join(" ");
        assert!(
            notes.contains("\"d\"") && notes.contains("left out") && notes.contains("old file"),
            "{notes}"
        );
        assert!(kadakas_core::provenance::base_recs(&opened.doc, "d", "r").is_none());
        // The rest of the drawing is there, and it saves and opens again.
        assert_eq!(opened.doc.objects.len(), whole.doc.objects.len());
        let again = open(container_bytes(&opened.doc));
        assert_eq!(again.doc.objects.len(), whole.doc.objects.len());
    }
}

#[test]
fn a_placeholder_keeps_the_hash_of_the_picture_and_the_real_picture_comes_back() {
    let mut doc = sample_doc();
    doc.resources.iter_mut().for_each(|_| ());
    let original = open(container_bytes(&doc));
    let source_a = original.source.clone().unwrap();
    // The two pictures and the image are in file A. A copy of the document with placeholders.
    let hashes: std::collections::HashSet<String> = {
        let mut set = std::collections::HashSet::new();
        for o in original.doc.objects.values() {
            if let Shape::Underlay(u) = &o.shape {
                set.insert(u.png.lazy().unwrap().hash().to_string());
            }
        }
        for r in original.doc.resources.iter() {
            if let ResourceBody::Image(i) = &r.body {
                set.insert(i.data.lazy().unwrap().hash().to_string());
            }
        }
        set
    };
    assert_eq!(hashes.len(), 3);
    let mut with = original.doc.clone();
    assert_eq!(with.replace_missing_pictures(&hashes), 3);
    assert_eq!(with.placeholder_hashes().len(), 3);
    assert!(with.placeholder_note().unwrap().contains("3 picture"));
    // The placeholders are in the file of the person: the gray pixel is not written as the picture.
    let saved = open(container_bytes(&with));
    assert_eq!(saved.doc.placeholder_hashes().len(), 3);
    let mut back = saved.doc;
    let Shape::Underlay(u) = &back.objects[&ObjectId::from_num(11)].shape else {
        panic!()
    };
    assert!(u.source.starts_with("missing:"), "{}", u.source);
    assert_eq!((u.width, u.height), (1, 1));
    // A source that has the pictures (file A, or its .bak) puts them back, with their facts.
    let blobs: Arc<dyn kadakas_core::blob::BlobSource> = source_a;
    assert_eq!(back.relink_placeholders(&blobs), 3);
    assert!(back.placeholder_hashes().is_empty());
    let Shape::Underlay(u) = &back.objects[&ObjectId::from_num(11)].shape else {
        panic!()
    };
    assert_eq!(*u.png.bytes().unwrap(), png_bytes(1, 20_000));
    assert_eq!(u.source, "");
    let image = back.resources.image(ResourceId::from_num(30)).unwrap();
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(image.source, "leaf.png");
    // Nothing to put back from a source that lacks the pictures.
    let mut again = saved_again(&with);
    assert_eq!(again.relink_placeholders(&nothing_source()), 0);
}

fn saved_again(doc: &Document) -> Document {
    open(container_bytes(doc)).doc
}

fn nothing_source() -> Arc<dyn kadakas_core::blob::BlobSource> {
    open(container_bytes(&Document::with_sequential_ids()))
        .source
        .unwrap()
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_state_key_of_the_loader_includes_the_bak() {
    let dir = std::env::temp_dir().join(format!("kf-key-bak-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.kad");
    let mut doc = sample_doc();
    doc.history = Some(sample_history());
    crate::native::save_path(&p, &doc, Form::Container, &WriteOptions::default()).unwrap();
    let o = crate::native::open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
    let loader = o.doc.lazy_history.clone().unwrap();
    let before = loader.state_key();
    std::fs::write(dir.join("a.kad.bak"), b"a backup appears").unwrap();
    assert_ne!(loader.state_key(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(target_arch = "wasm32"))]
mod disk {
    use super::*;
    use crate::native::{convert, inspect, open_path, save_path};

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(d).unwrap()
    }

    fn opts(o: &Opened) -> WriteOptions {
        WriteOptions {
            generator: generator(),
            keep: o.source.iter().cloned().collect(),
            anchor: false,
            skip_missing: false,
            allow_overwrite: false,
        }
    }

    fn names(d: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn save_and_open_both_forms() {
        let d = dir("both");
        let doc = sample_doc();
        for (name, form) in [("a.kad", Form::Container), ("a.json", Form::Json)] {
            let p = d.join(name);
            let saved = save_path(&p, &doc, form, &WriteOptions::default()).unwrap();
            assert_eq!(saved.form, form);
            let o = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
            assert_eq!(o.form, form);
            assert_eq!(o.doc, doc);
            assert_eq!(o.source.is_some(), form == Form::Container);
        }
        assert!(
            std::fs::read(d.join("a.kad"))
                .unwrap()
                .starts_with(b"PK\x03\x04")
        );
        assert!(std::fs::read(d.join("a.json")).unwrap().starts_with(b"{"));
        assert_eq!(names(&d), ["a.json", "a.kad"]);
    }

    #[test]
    fn saving_over_the_lazy_source_works_and_keeps_the_pictures() {
        let d = dir("over");
        let p = d.join("a.kad");
        let doc = sample_doc();
        save_path(&p, &doc, Form::Container, &WriteOptions::default()).unwrap();
        let mut o = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
        // Edit: move the circle.
        let obj = o.doc.objects.get_mut(&ObjectId::from_num(10)).unwrap();
        obj.shape = Shape::Circle {
            center: DVec2::new(9.0, 9.0),
            radius: 2.0,
        };
        let before = std::fs::read(&p).unwrap();
        let saved = save_path(&p, &o.doc, Form::Container, &opts(&o)).unwrap();
        let rep = saved.written.unwrap();
        assert_eq!(rep.blobs_copied, 3, "{rep:?}");
        assert_eq!(
            rep.blobs_written, 1,
            "the import base is in memory: {rep:?}"
        );
        assert!(saved.notes.is_empty());
        assert_eq!(
            std::fs::read(d.join("a.kad.bak")).unwrap(),
            before,
            "the old file is the backup"
        );
        // The document still reads its pictures from the new file.
        let Shape::Underlay(u) = &o.doc.objects[&ObjectId::from_num(11)].shape else {
            panic!()
        };
        assert!(u.png.is_lazy());
        assert_eq!(*u.png.bytes().unwrap(), png_bytes(1, 20_000));
        // And a third save works from the refreshed source.
        let again = save_path(&p, &o.doc, Form::Container, &opts(&o)).unwrap();
        assert_eq!(again.written.unwrap().blobs_copied, 3);
        let back = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
        assert_eq!(back.doc, o.doc);
        assert_eq!(
            names(&d),
            ["a.kad", "a.kad.bak"],
            "no temporary file is left"
        );
    }

    #[test]
    fn a_picture_removed_from_the_drawing_stays_for_the_undo_steps() {
        let d = dir("undo");
        let p = d.join("a.kad");
        save_path(&p, &sample_doc(), Form::Container, &WriteOptions::default()).unwrap();
        let mut o = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
        // The undo step holds the object; the drawing no longer has it.
        let held = o.doc.objects.shift_remove(&ObjectId::from_num(11)).unwrap();
        let saved = save_path(&p, &o.doc, Form::Container, &opts(&o)).unwrap();
        assert_eq!(saved.written.unwrap().blobs_kept, 1);
        let Shape::Underlay(u) = &held.shape else {
            panic!()
        };
        assert_eq!(
            *u.png.bytes().unwrap(),
            png_bytes(1, 20_000),
            "undo finds the picture"
        );
        // Without a live handle the picture is dropped by the next save.
        drop(held);
        let saved = save_path(&p, &o.doc, Form::Container, &opts(&o)).unwrap();
        assert_eq!(saved.written.unwrap().blobs_kept, 0);
        let c = Container::from_bytes(std::fs::read(&p).unwrap(), Limits::default()).unwrap();
        assert_eq!(
            c.entries()
                .iter()
                .filter(|e| e.name.starts_with(BLOB_DIR))
                .count(),
            3
        );
    }

    #[test]
    fn a_file_changed_on_disk_is_found_and_nothing_wrong_is_read() {
        let d = dir("changed");
        let p = d.join("a.kad");
        save_path(&p, &sample_doc(), Form::Container, &WriteOptions::default()).unwrap();
        let o = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        // Another program writes another document over the file.
        let other = {
            let mut doc = Document::with_sequential_ids();
            let obj = Object::new(ObjectId::from_num(11), FIRST_LAYER, NONE_CLASS, underlay(5));
            doc.objects.insert(obj.id, obj);
            container_bytes(&doc)
        };
        std::fs::write(&p, &other).unwrap();
        let Shape::Underlay(u) = &o.doc.objects[&ObjectId::from_num(11)].shape else {
            panic!()
        };
        let e = u.png.bytes().unwrap_err();
        assert!(e.to_string().contains("changed on disk"), "{e}");
        // A save over the file that another program wrote is refused, and leaves that file alone.
        let e = save_path(&p, &o.doc, Form::Container, &opts(&o)).unwrap_err();
        assert!(matches!(e, FileError::FileChanged(_)), "{e}");
        assert_eq!(std::fs::read(&p).unwrap(), other);
        // The person agrees to replace it. The pictures are not in any file: the save says which
        // are missing, and still leaves the file alone.
        let mut agreed = opts(&o);
        agreed.allow_overwrite = true;
        let e = save_path(&p, &o.doc, Form::Container, &agreed).unwrap_err();
        let FileError::Missing(m) = &e else {
            panic!("{e}")
        };
        assert!(m.pictures.len() >= 2 && !m.history, "{m:?}");
        assert!(e.to_string().contains("changed on disk"), "{e}");
        assert_eq!(std::fs::read(&p).unwrap(), other);
        assert_eq!(names(&d), ["a.kad"]);
        // The same save to another path fails too (the pictures cannot be read), and leaves no file.
        let e = save_path(&d.join("b.kad"), &o.doc, Form::Container, &opts(&o)).unwrap_err();
        assert!(matches!(e, FileError::Missing(_)), "{e}");
        assert!(!d.join("b.kad").exists());
        // The JSON form needs the pictures as well.
        assert!(save_path(&d.join("c.json"), &o.doc, Form::Json, &opts(&o)).is_err());
        assert_eq!(names(&d), ["a.kad"]);
    }

    /// What happens to the file under an open document, for the matrix below.
    #[derive(Clone, Copy, Debug)]
    enum Change {
        /// The same bytes written again: a new time of change.
        Touch,
        /// The file is renamed.
        Rename,
        /// Another file with the same pictures but other parts.
        ReplaceWithTheSamePictures,
        /// Another file with other pictures.
        ReplaceWithOther,
        /// The file is deleted.
        Delete,
        /// The file is deleted, and a `.bak` of the last save is next to it.
        DeleteWithBak,
    }

    /// Opens a document, edits it, changes the file under it, and returns the open document.
    fn changed_file(d: &std::path::Path, change: Change) -> Opened {
        let p = d.join("a.kad");
        let mut doc = sample_doc();
        doc.history = Some(sample_history());
        save_path(&p, &doc, Form::Container, &WriteOptions::default()).unwrap();
        let mut o = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
        let obj = o.doc.objects.get_mut(&ObjectId::from_num(10)).unwrap();
        obj.shape = Shape::Circle {
            center: DVec2::new(9.0, 9.0),
            radius: 2.0,
        };
        std::thread::sleep(std::time::Duration::from_millis(30));
        let original = std::fs::read(&p).unwrap();
        match change {
            Change::Touch => std::fs::write(&p, &original).unwrap(),
            Change::Rename => std::fs::rename(&p, d.join("moved.kad")).unwrap(),
            Change::ReplaceWithTheSamePictures => {
                let c = Container::from_bytes(original, Limits::default()).unwrap();
                let mut w = Writer::new(Vec::new());
                for e in c.entries().iter().filter(|e| e.name != DOCUMENT) {
                    w.copy_raw(c.raw(&e.name).unwrap()).unwrap();
                }
                w.add_bytes(DOCUMENT, Compression::Deflate(1), b"{\"other\": true}")
                    .unwrap();
                std::fs::write(&p, w.finish().unwrap()).unwrap();
            }
            Change::ReplaceWithOther => {
                let mut other = Document::with_sequential_ids();
                let obj = Object::new(ObjectId::from_num(11), FIRST_LAYER, NONE_CLASS, underlay(5));
                other.objects.insert(obj.id, obj);
                std::fs::write(&p, container_bytes(&other)).unwrap();
            }
            Change::Delete => std::fs::remove_file(&p).unwrap(),
            Change::DeleteWithBak => {
                std::fs::write(d.join("a.kad.bak"), &original).unwrap();
                std::fs::remove_file(&p).unwrap();
            }
        }
        o
    }

    /// The pictures of the changed file are found by content, or the save says which are missing;
    /// it never writes a wrong picture and never leaves a partial file.
    #[test]
    fn the_file_changing_under_an_open_document_is_handled_by_content() {
        use Change::*;
        for (change, finds) in [
            (Touch, true),
            (ReplaceWithTheSamePictures, true),
            (DeleteWithBak, true),
            (Rename, false),
            (ReplaceWithOther, false),
            (Delete, false),
        ] {
            let d = dir(&format!("matrix-{change:?}"));
            let o = changed_file(&d, change);
            for (to, form) in [
                ("a.kad", Form::Container),
                ("b.kad", Form::Container),
                ("c.json", Form::Json),
            ] {
                let target = d.join(to);
                let before = std::fs::read(&target).ok();
                // Another program wrote the file at the path: a save over it asks the person
                // first (the other targets are new files).
                let mut options = opts(&o);
                if to == "a.kad" && matches!(change, ReplaceWithTheSamePictures | ReplaceWithOther)
                {
                    let e = save_path(&target, &o.doc, form, &options).unwrap_err();
                    assert!(matches!(e, FileError::FileChanged(_)), "{change:?}: {e}");
                    assert_eq!(std::fs::read(&target).ok(), before);
                    options.allow_overwrite = true;
                }
                let r = if form == Form::Json {
                    // The plain form needs the versions resolved, as the session does.
                    let mut doc = o.doc.clone();
                    resolve_history(&mut doc)
                        .and_then(|()| save_path(&target, &doc, form, &options).map(|_| ()))
                } else {
                    save_path(&target, &o.doc, form, &options).map(|_| ())
                };
                if finds {
                    r.unwrap_or_else(|e| panic!("{change:?} -> {to}: {e}"));
                    let back = open_path(&target, u64::MAX, ReadMode::Eager).unwrap();
                    let Shape::Circle { center, .. } =
                        &back.doc.objects[&ObjectId::from_num(10)].shape
                    else {
                        panic!()
                    };
                    assert_eq!(*center, DVec2::new(9.0, 9.0), "{change:?} -> {to}");
                    let Shape::Underlay(u) = &back.doc.objects[&ObjectId::from_num(11)].shape
                    else {
                        panic!()
                    };
                    assert_eq!(*u.png.bytes().unwrap(), png_bytes(1, 20_000));
                } else {
                    let e = r.expect_err(&format!("{change:?} -> {to}"));
                    if form == Form::Container {
                        assert!(
                            matches!(e, FileError::Missing(_)),
                            "{change:?} -> {to}: {e}"
                        );
                    }
                    // Nothing was written: the target is what it was.
                    assert_eq!(std::fs::read(&target).ok(), before, "{change:?} -> {to}");
                }
            }
            // No temporary file is left in any case.
            assert!(
                names(&d).iter().all(|n| !n.contains(".tmp")),
                "{change:?}: {:?}",
                names(&d)
            );
            std::fs::remove_dir_all(&d).unwrap();
        }
    }

    /// An edited document whose versions wait in the file: after a save over the file the versions
    /// are still there, written against the drawing of the old file (the anchor entry).
    #[test]
    fn an_edited_document_keeps_waiting_versions_with_the_anchor_drawing() {
        let d = dir("anchor");
        let p = d.join("a.kad");
        let mut doc = sample_doc();
        doc.history = Some(sample_history());
        save_path(&p, &doc, Form::Container, &WriteOptions::default()).unwrap();
        let mut o = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
        let obj = o.doc.objects.get_mut(&ObjectId::from_num(10)).unwrap();
        obj.shape = Shape::Circle {
            center: DVec2::new(9.0, 9.0),
            radius: 2.0,
        };
        let mut options = opts(&o);
        options.anchor = true;
        let saved = save_path(&p, &o.doc, Form::Container, &options).unwrap();
        let rep = saved.written.unwrap();
        assert!(rep.anchor_written && rep.parts_copied >= 4, "{rep:?}");
        let c = Container::from_bytes(std::fs::read(&p).unwrap(), Limits::default()).unwrap();
        assert!(c.contains(crate::HISTORY_ANCHOR) && c.contains(crate::HISTORY_ANCHOR_BLOBS));
        // The anchor is the drawing of the old file: the circle is where it was.
        let back = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
        let loader = back.doc.lazy_history.clone().unwrap();
        assert!(loader.separate_anchor());
        let anchor = loader.anchor().unwrap();
        assert_eq!(anchor.objects, sample_doc().objects);
        // The history itself is the same history.
        let h = loader.load().unwrap();
        assert_eq!(Some(h), doc.history);
        // A second save (no edit) copies the anchor along, and the file stays readable.
        let again = save_path(&p, &back.doc, Form::Container, &opts(&back)).unwrap();
        assert!(!again.written.unwrap().anchor_written);
        let back2 = open_path(&p, u64::MAX, ReadMode::Lazy).unwrap();
        assert_eq!(
            back2.doc.lazy_history.unwrap().anchor().unwrap().objects,
            sample_doc().objects
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_save_into_a_missing_folder_fails_cleanly() {
        let d = dir("fail");
        let p = d.join("a.kad");
        let doc = sample_doc();
        save_path(&p, &doc, Form::Container, &WriteOptions::default()).unwrap();
        let old = std::fs::read(&p).unwrap();
        let e = save_path(
            &d.join("no").join("a.kad"),
            &doc,
            Form::Container,
            &WriteOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(e, FileError::Io(_)), "{e}");
        assert_eq!(std::fs::read(&p).unwrap(), old);
        assert_eq!(names(&d), ["a.kad"]);
    }

    #[test]
    fn convert_in_both_directions_keeps_the_document_and_the_history() {
        let d = dir("convert");
        let mut doc = sample_doc();
        doc.history = Some(sample_history());
        let k = d.join("a.kad");
        let j = d.join("a.json");
        let k2 = d.join("b.kad");
        save_path(&k, &doc, Form::Container, &WriteOptions::default()).unwrap();
        convert(&k, &j, Form::Json, u64::MAX).unwrap();
        convert(&j, &k2, Form::Container, u64::MAX).unwrap();
        assert!(std::fs::read(&k2).unwrap().starts_with(b"PK"));
        let a = resolved(open_path(&k, u64::MAX, ReadMode::Lazy).unwrap());
        let b = resolved(open_path(&j, u64::MAX, ReadMode::Lazy).unwrap());
        let c = resolved(open_path(&k2, u64::MAX, ReadMode::Lazy).unwrap());
        assert_eq!(a, doc);
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a.history, c.history);
        // Convert over the same file.
        convert(&k2, &k2, Form::Json, u64::MAX).unwrap();
        assert!(std::fs::read(&k2).unwrap().starts_with(b"{"));
        assert_eq!(open_path(&k2, u64::MAX, ReadMode::Lazy).unwrap().doc, doc);
    }

    #[test]
    fn errors_for_missing_and_big_files() {
        let d = dir("errors");
        assert!(matches!(
            open_path(&d.join("none.kad"), u64::MAX, ReadMode::Lazy),
            Err(FileError::NotFound(_))
        ));
        assert!(matches!(
            open_path(&d, u64::MAX, ReadMode::Lazy),
            Err(FileError::NotRegular(_))
        ));
        let p = d.join("a.kad");
        save_path(&p, &sample_doc(), Form::Container, &WriteOptions::default()).unwrap();
        let len = std::fs::metadata(&p).unwrap().len();
        assert!(matches!(
            open_path(&p, len - 1, ReadMode::Lazy),
            Err(FileError::TooLarge { .. })
        ));
        // The limit counts the bytes of the entries too.
        let e = open_path(&p, len + 100, ReadMode::Lazy).unwrap_err();
        assert!(
            matches!(
                e,
                FileError::Container(kadakas_container::ContainerError::Limit(_))
            ),
            "{e}"
        );
        std::fs::write(&p, b"garbage").unwrap();
        assert!(matches!(
            open_path(&p, u64::MAX, ReadMode::Lazy),
            Err(FileError::Unknown(_))
        ));
        std::fs::write(&p, b"").unwrap();
        assert!(matches!(
            open_path(&p, u64::MAX, ReadMode::Lazy),
            Err(FileError::Unknown(_))
        ));
    }

    #[test]
    fn inspect_reports_sizes_without_loading() {
        let d = dir("inspect");
        let mut doc = sample_doc();
        doc.history = Some(sample_history());
        let k = d.join("a.kad");
        save_path(&k, &doc, Form::Container, &WriteOptions::default()).unwrap();
        let i = inspect(&k).unwrap();
        assert_eq!(i.form, Form::Container);
        assert!(i.blob_bytes > 60_000 && i.document_bytes > 1000 && i.history_bytes > 100);
        assert_eq!(i.entries, 10);
        assert!(i.uncompressed_bytes >= i.document_bytes + i.blob_bytes);
        assert!(i.resident_bytes() < i.uncompressed_bytes);
        let j = d.join("a.json");
        save_path(&j, &doc, Form::Json, &WriteOptions::default()).unwrap();
        let ij = inspect(&j).unwrap();
        assert_eq!(ij.form, Form::Json);
        assert_eq!(ij.resident_bytes(), ij.file_bytes);
        assert!(inspect(&d.join("none")).is_err());
    }

    #[test]
    fn the_size_estimate_of_the_drawing_counts_the_vertices_of_every_shape() {
        use kadakas_core::model::Vertex;
        let mut doc = Document::with_sequential_ids();
        let base = crate::write::estimate_document_bytes(&doc);
        let id = ObjectId::from_num(9);
        let points: Vec<Vertex> = (0..100_000)
            .map(|i| Vertex {
                p: DVec2::new(i as f64, 0.0),
                ..Vertex::default()
            })
            .collect();
        doc.objects.insert(
            id,
            Object::new(
                id,
                FIRST_LAYER,
                NONE_CLASS,
                Shape::Polyline {
                    points,
                    closed: true,
                },
            ),
        );
        // 100 000 vertices at about 70 bytes each, and the object.
        assert!(crate::write::estimate_document_bytes(&doc) >= base + 7_000_000);
    }

    #[test]
    fn a_content_stamp_tells_a_change_from_a_touch_in_both_forms() {
        let d = dir("seen");
        let doc = sample_doc();
        let seen = SeenFiles::default();
        for name in ["a.kad", "a.json"] {
            let p = d.join(name);
            assert!(!seen.changed(&p) && !seen.knows(&p));
            let form = Form::from_extension(&p);
            save_path(&p, &doc, form, &WriteOptions::default()).unwrap();
            // Never seen: no guard. Seen: the same file is not changed, also after a touch.
            assert!(!seen.changed(&p));
            seen.note(&p);
            assert!(seen.knows(&p) && !seen.changed(&p));
            std::thread::sleep(std::time::Duration::from_millis(20));
            let bytes = std::fs::read(&p).unwrap();
            std::fs::write(&p, &bytes).unwrap();
            assert!(!seen.changed(&p), "{name}: a touch");
            // Another content is a change; a deleted file is not.
            let mut other = sample_doc();
            other.drawing.project.name = "other".into();
            save_path(&p, &other, form, &WriteOptions::default()).unwrap();
            assert!(seen.changed(&p), "{name}: another content");
            seen.note(&p);
            assert!(!seen.changed(&p));
            std::fs::remove_file(&p).unwrap();
            assert!(!seen.changed(&p), "{name}: gone");
        }
    }

    #[test]
    fn a_save_records_the_stamp_of_the_bytes_it_wrote_and_a_file_is_hashed_only_when_it_differs() {
        let d = dir("seen-written");
        let doc = sample_doc();
        for name in ["w.kad", "w.json"] {
            let p = d.join(name);
            let form = Form::from_extension(&p);
            save_path(&p, &doc, form, &WriteOptions::default()).unwrap();
            let want = stamp_of(&p).unwrap();
            let seen = SeenFiles::default();
            seen.note(&p);
            assert!(seen.knows(&p) && !seen.changed(&p));
            // The recorded stamp is the one of the bytes that were written: a note takes it
            // without reading the file. Same size and time, other bytes: the note has the stamp
            // of the write (so it did not read), and the check does not hash the file either,
            // because neither the size nor the time differs.
            let bytes = std::fs::read(&p).unwrap();
            let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
            let mut other = bytes.clone();
            let last = other.len() - 1;
            other[last] ^= 0xFF;
            std::fs::write(&p, &other).unwrap();
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(mtime).unwrap();
            drop(f);
            assert_ne!(stamp_of(&p).unwrap(), want, "{name}: the bytes differ");
            let seen = SeenFiles::default();
            seen.note(&p);
            assert!(
                !seen.changed(&p),
                "{name}: not hashed again, size and time are the same"
            );
            // Another size or time is hashed, and found different.
            std::thread::sleep(std::time::Duration::from_millis(20));
            std::fs::write(&p, &other).unwrap();
            assert!(
                seen.changed(&p),
                "{name}: the time differs and the content is another"
            );
        }
    }

    #[test]
    fn only_the_pictures_that_the_drawing_does_not_use_are_read_when_the_file_changes() {
        use std::collections::HashSet;
        let d = dir("rescue");
        let k = d.join("a.kad");
        let doc = sample_doc();
        save_path(&k, &doc, Form::Container, &WriteOptions::default()).unwrap();
        let o = open_path(&k, u64::MAX, ReadMode::Lazy).unwrap();
        let source = o.source.clone().unwrap();
        let handles: Vec<_> = o
            .doc
            .objects
            .values()
            .filter_map(|ob| match &ob.shape {
                Shape::Underlay(u) => u.png.lazy().cloned(),
                _ => None,
            })
            .collect();
        assert!(handles.len() >= 2, "the sample has two pictures");
        // Every picture and image that a handle holds (the underlays and the image resource).
        let all: HashSet<String> = source.live_hashes().into_iter().collect();
        // Nothing changed: nothing is read.
        assert_eq!(source.rescue_if_changed(HashSet::new), 0);
        assert!(handles.iter().all(|h| !h.is_pinned()));
        // Another program saves over the file (our file is its .bak).
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut other = Document::with_sequential_ids();
        other.drawing.project.name = "other".into();
        save_path(&k, &other, Form::Container, &WriteOptions::default()).unwrap();
        // The pictures that the drawing uses stay as they are (the refusal and the consent cover
        // them): nothing is read.
        assert_eq!(source.rescue_if_changed(|| all.clone()), 0);
        assert!(handles.iter().all(|h| !h.is_pinned()));
        // The limit of the session: nothing fits, and the words say so.
        source.set_rescue_limit(0);
        assert_eq!(source.rescue_if_changed(HashSet::new), 0);
        let told = source.tell_undo_lost(handles.len()).unwrap();
        assert!(
            told.contains("could not be kept in memory") && told.contains("0 MiB"),
            "{told}"
        );
        // Only the undo-only pictures are read, as many as the limit allows.
        let first = handles[0].hash().to_string();
        let only_first_in_drawing: HashSet<String> = std::iter::once(first.clone()).collect();
        source.set_rescue_limit(u64::MAX);
        let n = source.rescue_if_changed(|| only_first_in_drawing.clone());
        assert_eq!(n, all.len() - 1);
        assert!(!handles[0].is_pinned() && handles[1..].iter().all(|h| h.is_pinned()));
        // The .bak goes too: those pictures are still there, from memory.
        std::fs::remove_file(d.join("a.kad.bak")).unwrap();
        for h in &handles[1..] {
            let text = h.text().unwrap();
            assert_eq!(kadakas_core::blob::text_hash(&text), h.hash());
            assert!(source.pinned_text(h.hash()).is_some());
        }
    }

    #[test]
    fn a_path_below_a_file_is_an_io_error_and_a_lost_source_is_a_note() {
        let d = dir("io");
        let f = d.join("a.kad");
        save_path(&f, &sample_doc(), Form::Container, &WriteOptions::default()).unwrap();
        // A path that goes through a file is not "not found".
        let e = open_path(&f.join("inside.kad"), u64::MAX, ReadMode::Lazy).unwrap_err();
        assert!(
            matches!(e, FileError::Io(_) | FileError::NotFound(_)),
            "{e}"
        );
        // A source that cannot move to the file it is given says so.
        let rep = write_container(
            Vec::new(),
            &open_path(&f, u64::MAX, ReadMode::Lazy).unwrap().doc,
            &WriteOptions::default(),
        )
        .unwrap()
        .1;
        assert_eq!(rep.sources.len(), 1);
        let notes = crate::native::rebind_sources(&rep, &d.join("none.kad"));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("none.kad"));
        assert_eq!(
            rep.sources[0].path(),
            Some(f.clone()),
            "the source stays where it was"
        );
    }

    #[test]
    fn the_pictures_of_a_document_can_be_checked_against_their_file() {
        let d = dir("check");
        let f = d.join("a.kad");
        save_path(&f, &sample_doc(), Form::Container, &WriteOptions::default()).unwrap();
        let o = open_path(&f, u64::MAX, ReadMode::Lazy).unwrap();
        assert!(o.doc.check_blobs().is_ok());
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(&f, b"another file").unwrap();
        let e = o.doc.check_blobs().unwrap_err();
        assert!(e.to_string().contains("changed on disk"), "{e}");
        // A document with no picture in a file has nothing to check.
        assert!(sample_doc().check_blobs().is_ok());
    }

    #[test]
    fn open_with_source_gives_the_source_of_a_container() {
        let d = dir("src");
        let k = d.join("a.kad");
        save_path(&k, &sample_doc(), Form::Container, &WriteOptions::default()).unwrap();
        let (_, s) = crate::native::open_with_source(&k, u64::MAX).unwrap();
        let s = s.unwrap();
        assert_eq!(s.path(), Some(k.clone()));
        assert!(s.check_unchanged().is_ok());
        assert!(format!("{s:?}").contains("a.kad"));
    }
}

/// Opening and saving a document with thousands of pictures: the open reads the first characters
/// of each picture, the save copies each one from the old file. The numbers are in
/// `docs/doc-load-benchmark.md`: `cargo test --release -p kadakas-file perf_ -- --ignored --nocapture`.
#[test]
#[ignore = "timing test: run in release mode"]
fn perf_many_pictures() {
    let n = 20_000;
    let mut doc = Document::with_sequential_ids();
    for i in 0..n {
        let mut r = image(7);
        // A different text for each picture, the same shape: a PNG header and some bytes.
        let mut file = r.data.text().map(|t| t.to_string()).unwrap();
        file.push_str(&format!("{i:08}"));
        r.data = BlobText::from(file);
        doc.resources.push(Resource {
            id: ResourceId::from_num(1000 + i),
            name: format!("r{i}"),
            body: ResourceBody::Image(Box::new(r)),
        });
    }
    let started = std::time::Instant::now();
    let dir = std::env::temp_dir().join(format!("kf-perf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("many.kad");
    crate::native::save_path(&f, &doc, Form::Container, &WriteOptions::default()).unwrap();
    let written = started.elapsed();
    let started = std::time::Instant::now();
    let o = crate::native::open_path(&f, u64::MAX, ReadMode::Lazy);
    // The pictures were made without a valid hash of the file, so the check at load may refuse
    // the document; the time of the reads is what counts here.
    let opened = started.elapsed();
    let kept = o.is_ok();
    println!(
        "{n} pictures: file {} bytes, written in {written:?}, opened in {opened:?} (loaded: {kept})",
        std::fs::metadata(&f).unwrap().len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
