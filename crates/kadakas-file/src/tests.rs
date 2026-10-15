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
    let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
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
    // An eager read says so at open.
    let e = read_bytes(bytes, Limits::default(), ReadMode::Eager).unwrap_err();
    assert!(e.to_string().contains(HISTORY_VERSIONS), "{e}");
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
    };
    let (bytes, report) = write_container(Vec::new(), &first.doc, &opts).unwrap();
    // Every blob is in the old file: the pictures and the import base are copied, and the picture
    // of the history waits there with the history.
    assert_eq!(report.blobs_copied, 4, "{report:?}");
    assert_eq!(report.blobs_kept, 1, "{report:?}");
    assert_eq!(report.blobs_written, 0, "{report:?}");
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
        // The history is read when it is used: an eager read finds the damage at once.
        read_bytes(w.finish().unwrap(), Limits::default(), ReadMode::Eager)
            .unwrap_err()
            .to_string()
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
    let read = |bytes: Vec<u8>| {
        read_bytes(bytes, Limits::default(), ReadMode::Eager)
            .unwrap_err()
            .to_string()
    };
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
    // A history that lists a picture whose entry is another picture: a save refuses to copy it.
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
    assert!(
        e.to_string().contains("not the picture of this hash"),
        "{e}"
    );
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
        let doc: Document =
            with_load_source(source, false, || serde_json::from_str(&json)).unwrap();
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
        assert_eq!(rep.blobs_copied, 4, "{rep:?}");
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
        assert_eq!(again.written.unwrap().blobs_copied, 4);
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
        // A save that needs the old pictures fails and leaves the other program's file alone.
        let e = save_path(&p, &o.doc, Form::Container, &opts(&o)).unwrap_err();
        assert!(e.is_changed(), "{e}");
        assert_eq!(std::fs::read(&p).unwrap(), other);
        assert_eq!(names(&d), ["a.kad"]);
        // The same save to another path fails too (the pictures cannot be read), and leaves no file.
        let e = save_path(&d.join("b.kad"), &o.doc, Form::Container, &opts(&o)).unwrap_err();
        assert!(e.is_changed(), "{e}");
        assert!(!d.join("b.kad").exists());
        // The JSON form needs the pictures as well.
        assert!(save_path(&d.join("c.json"), &o.doc, Form::Json, &opts(&o)).is_err());
        assert_eq!(names(&d), ["a.kad"]);
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
