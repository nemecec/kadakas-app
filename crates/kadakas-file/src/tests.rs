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
