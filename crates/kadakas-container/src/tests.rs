//! Tests of the container: round trips, raw copy, and hostile files.

use std::io::{Read, Write};
use std::sync::Arc;

use proptest::prelude::*;

use crate::*;

fn write(entries: &[(&str, Compression, &[u8])]) -> Vec<u8> {
    let mut w = Writer::new(Vec::new()).generator("test");
    for (name, how, bytes) in entries {
        w.add_bytes(name, *how, bytes).unwrap();
    }
    w.finish().unwrap()
}

fn sample() -> Vec<u8> {
    write(&[
        (
            "document.json",
            Compression::Deflate(6),
            br#"{"a":1,"b":[1,2,3]}"#,
        ),
        ("blobs/aa", Compression::Stored, b"picture bytes"),
        ("history/top.json", Compression::Deflate(1), &[b'x'; 5000]),
    ])
}

fn open(bytes: &[u8]) -> Result<Container, ContainerError> {
    Container::from_bytes(bytes.to_vec(), Limits::default())
}

/// A hand-made ZIP, so that a test can put any value in any field.
#[derive(Clone)]
struct Forge {
    name: String,
    method: u16,
    flags: u16,
    data: Vec<u8>,
    crc: u32,
    size: u32,
    compressed: u32,
}

impl Forge {
    fn stored(name: &str, data: &[u8]) -> Forge {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        Forge {
            name: name.into(),
            method: 0,
            flags: 0,
            data: data.to_vec(),
            crc: crc.sum(),
            size: data.len() as u32,
            compressed: data.len() as u32,
        }
    }

    fn deflated(name: &str, plain: &[u8]) -> Forge {
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(plain).unwrap();
        let data = enc.finish().unwrap();
        let mut crc = flate2::Crc::new();
        crc.update(plain);
        Forge {
            name: name.into(),
            method: 8,
            flags: 0,
            compressed: data.len() as u32,
            data,
            crc: crc.sum(),
            size: plain.len() as u32,
        }
    }
}

fn manifest_for(entries: &[Forge], plain: &[&[u8]]) -> Vec<u8> {
    let list: Vec<_> = entries
        .iter()
        .zip(plain)
        .map(|(e, p)| serde_json::json!({"name": e.name, "size": p.len(), "sha256": sha256_hex(p)}))
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "format": FORMAT, "format_version": VERSION, "entries": list
    }))
    .unwrap()
}

fn build(entries: &[Forge]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut dir = Vec::new();
    for e in entries {
        let offset = out.len() as u32;
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(20u16.to_le_bytes());
        out.extend(e.flags.to_le_bytes());
        out.extend(e.method.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(e.crc.to_le_bytes());
        out.extend(e.compressed.to_le_bytes());
        out.extend(e.size.to_le_bytes());
        out.extend((e.name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(e.name.as_bytes());
        out.extend(&e.data);
        dir.extend(0x0201_4b50u32.to_le_bytes());
        dir.extend(20u16.to_le_bytes());
        dir.extend(20u16.to_le_bytes());
        dir.extend(e.flags.to_le_bytes());
        dir.extend(e.method.to_le_bytes());
        dir.extend([0u8; 4]);
        dir.extend(e.crc.to_le_bytes());
        dir.extend(e.compressed.to_le_bytes());
        dir.extend(e.size.to_le_bytes());
        dir.extend((e.name.len() as u16).to_le_bytes());
        dir.extend([0u8; 8]);
        dir.extend(0u32.to_le_bytes());
        dir.extend(offset.to_le_bytes());
        dir.extend(e.name.as_bytes());
    }
    let dir_offset = out.len() as u32;
    out.extend(&dir);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((dir.len() as u32).to_le_bytes());
    out.extend(dir_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// Entries with a manifest that is right for them.
fn forged(entries: Vec<Forge>, plain: &[&[u8]]) -> Vec<u8> {
    let manifest = manifest_for(&entries, plain);
    let mut all = entries;
    all.push(Forge::stored(MANIFEST_NAME, &manifest));
    build(&all)
}

fn err(bytes: &[u8]) -> String {
    open(bytes).unwrap_err().to_string()
}

#[test]
fn entries_round_trip_with_both_methods() {
    let bytes = sample();
    assert!(has_zip_magic(&bytes));
    let c = open(&bytes).unwrap();
    assert_eq!(c.entries().len(), 3);
    assert_eq!(c.read("document.json").unwrap(), br#"{"a":1,"b":[1,2,3]}"#);
    assert_eq!(c.read("blobs/aa").unwrap(), b"picture bytes");
    assert_eq!(c.read("history/top.json").unwrap(), vec![b'x'; 5000]);
    let e = c.entry("history/top.json").unwrap();
    assert_eq!((e.size, e.method), (5000, Method::Deflate));
    assert!(e.compressed < 100);
    assert_eq!(c.entry("blobs/aa").unwrap().method, Method::Stored);
    assert!(c.contains("blobs/aa") && !c.contains("blobs/bb"));
    assert_eq!(c.manifest().generator, "test");
    assert_eq!(c.manifest().entries.len(), 3);
    assert_eq!(c.manifest().entries[1].sha256, sha256_hex(b"picture bytes"));
    assert_eq!(c.manifest_sha256().len(), 64);
    assert!(c.uncompressed_bytes() > 5000);
    assert_eq!(c.file_bytes(), bytes.len() as u64);
    assert!(c.manifest_size() > 50);
    assert!(c.read("missing").is_err());
    assert!(c.reader("missing").is_err());
    assert!(c.raw("missing").is_err());
    assert!(c.read_prefix("missing", 1).is_err());
    assert!(format!("{c:?}").contains("entries"));
    assert!(c.verify().is_ok());
    assert!(!c.medium().is_empty());
}

#[test]
fn the_same_content_gives_the_same_bytes() {
    assert_eq!(sample(), sample());
}

#[test]
fn prefix_of_stored_and_deflated_entries() {
    let c = open(&sample()).unwrap();
    assert_eq!(c.read_prefix("blobs/aa", 7).unwrap(), b"picture");
    assert_eq!(c.read_prefix("blobs/aa", 99).unwrap(), b"picture bytes");
    assert_eq!(c.read_prefix("history/top.json", 3).unwrap(), b"xxx");
}

#[test]
fn a_reader_checks_at_the_end_and_finish_drains() {
    let c = open(&sample()).unwrap();
    let mut r = c.reader("document.json").unwrap();
    assert_eq!(r.name(), "document.json");
    assert_eq!(r.size(), 19);
    let mut first = [0u8; 5];
    r.read_exact(&mut first).unwrap();
    r.finish().unwrap();
    // After the end, more reads give nothing.
    assert_eq!(r.read(&mut first).unwrap(), 0);
    assert_eq!(r.read(&mut []).unwrap(), 0);
}

#[test]
fn raw_copy_keeps_the_compressed_bytes() {
    let src = sample();
    let c = open(&src).unwrap();
    let mut w = Writer::new(Vec::new());
    for name in ["history/top.json", "document.json"] {
        w.copy_raw(c.raw(name).unwrap()).unwrap();
    }
    w.add_bytes("new", Compression::Stored, b"n").unwrap();
    assert!(w.has("new") && !w.has("blobs/aa"));
    assert_eq!(w.len(), 3);
    assert!(!w.is_empty());
    let out = w.finish().unwrap();
    let d = open(&out).unwrap();
    assert_eq!(
        d.read("document.json").unwrap(),
        c.read("document.json").unwrap()
    );
    assert_eq!(d.read("history/top.json").unwrap(), vec![b'x'; 5000]);
    assert_eq!(d.read("new").unwrap(), b"n");
    // The compressed size is the same: the data was not encoded again.
    assert_eq!(
        d.entry("history/top.json").unwrap().compressed,
        c.entry("history/top.json").unwrap().compressed
    );
    assert_eq!(
        d.entry("history/top.json").unwrap().crc32,
        c.entry("history/top.json").unwrap().crc32
    );
}

#[test]
fn a_raw_copy_of_a_cut_file_fails() {
    let c = open(&sample()).unwrap();
    let mut raw = c.raw("blobs/aa").unwrap();
    raw.data = Box::new(std::io::Cursor::new(vec![1u8, 2]));
    let mut w = Writer::new(Vec::new());
    let e = w.copy_raw(raw).unwrap_err();
    assert_eq!(e.entry_name(), Some("blobs/aa"));
    assert!(format!("{:?}", c.raw("blobs/aa").unwrap()).contains("blobs/aa"));
}

#[test]
fn writer_refuses_bad_and_repeated_names() {
    let mut w = Writer::new(Vec::new());
    w.add_bytes("a", Compression::Stored, b"1").unwrap();
    for bad in ["a", "../b", "/c", MANIFEST_NAME, "", "d\\e"] {
        let e = w.add_bytes(bad, Compression::Stored, b"2").unwrap_err();
        assert!(matches!(e, ContainerError::Entry { .. }), "{bad}");
    }
    assert!(w.add_bytes("ok", Compression::Stored, b"3").is_ok());
}

#[test]
fn writer_fill_error_is_returned() {
    let mut w = Writer::new(Vec::new());
    let e = w
        .add_with("a", Compression::Deflate(1), |_| {
            Err(std::io::Error::other("disk full"))
        })
        .unwrap_err();
    assert!(e.to_string().contains("disk full"));
    let e = w
        .add_with("b", Compression::Stored, |_| {
            Err(std::io::Error::other("again"))
        })
        .unwrap_err();
    assert!(e.to_string().contains("again"));
}

#[test]
fn writer_stops_at_the_entry_limit() {
    let mut w = Writer::new(std::io::sink());
    let mut last = Ok(());
    for i in 0..=MAX_ENTRIES {
        last = w.add_bytes(&format!("e{i}"), Compression::Stored, b"");
        if last.is_err() {
            break;
        }
    }
    let e = last.unwrap_err();
    assert!(matches!(e, ContainerError::Limit(_)));
    assert!(e.to_string().contains("JSON"));
}

#[test]
fn a_full_container_of_many_entries_reads_back() {
    let mut w = Writer::new(Vec::new());
    for i in 0..3000 {
        w.add_bytes(
            &format!("blobs/{i:064}"),
            Compression::Stored,
            format!("{i}").as_bytes(),
        )
        .unwrap();
    }
    let out = w.finish().unwrap();
    let c = open(&out).unwrap();
    assert_eq!(c.entries().len(), 3000);
    assert_eq!(c.read(&format!("blobs/{:064}", 2999)).unwrap(), b"2999");
}

#[test]
fn not_a_container() {
    assert!(err(b"").contains("too short"));
    assert!(err(b"{\"format_version\": 3}").contains("not a Kadakas container"));
    assert!(err(&[0u8; 100]).contains("end of the central directory"));
    assert!(!has_zip_magic(b"{"));
}

#[test]
fn every_cut_of_a_container_is_an_error_not_a_panic() {
    let bytes = sample();
    for n in 0..bytes.len() {
        assert!(open(&bytes[..n]).is_err(), "cut at {n}");
    }
}

#[test]
fn a_changed_byte_gives_the_same_data_or_an_error() {
    let bytes = sample();
    let good = open(&bytes).unwrap();
    let names: Vec<String> = good.entries().iter().map(|e| e.name.clone()).collect();
    for i in 0..bytes.len() {
        let mut b = bytes.clone();
        b[i] ^= 0x5a;
        let Ok(c) = open(&b) else { continue };
        for name in &names {
            if let Ok(data) = c.read(name) {
                assert_eq!(data, good.read(name).unwrap(), "byte {i} entry {name}");
            }
        }
    }
}

#[test]
fn a_damaged_stored_entry_names_the_entry() {
    let mut bytes = sample();
    let at = bytes.windows(7).position(|w| w == b"picture").unwrap();
    bytes[at] = b'P';
    let c = open(&bytes).unwrap();
    let e = c.read("blobs/aa").unwrap_err();
    assert_eq!(e.entry_name(), Some("blobs/aa"));
    assert!(e.to_string().contains("CRC-32"));
    assert!(c.read("document.json").is_ok());
}

#[test]
fn a_hash_that_is_not_the_manifest_is_found() {
    // The CRC-32 is right, the manifest has another hash.
    let e = Forge::stored("a", b"hello");
    let manifest = serde_json::to_vec(&serde_json::json!({
        "format": FORMAT, "format_version": VERSION,
        "entries": [{"name": "a", "size": 5, "sha256": sha256_hex(b"HELLO")}]
    }))
    .unwrap();
    let bytes = build(&[e, Forge::stored(MANIFEST_NAME, &manifest)]);
    let c = open(&bytes).unwrap();
    let e = c.read("a").unwrap_err();
    assert_eq!(e.entry_name(), Some("a"));
    assert!(e.to_string().contains("SHA-256"));
}

#[test]
fn a_forged_container_that_is_right_opens() {
    let bytes = forged(
        vec![
            Forge::stored("a", b"hello"),
            Forge::deflated("b", b"world world"),
        ],
        &[b"hello", b"world world"],
    );
    let c = open(&bytes).unwrap();
    assert_eq!(c.read("b").unwrap(), b"world world");
}

#[test]
fn names_are_checked() {
    for bad in ["../x", "/abs", "a/../b", "a\\b", "a//b", "C:/x", "dir/"] {
        let bytes = forged(vec![Forge::stored(bad, b"1")], &[b"1"]);
        let e = open(&bytes).unwrap_err();
        assert!(matches!(e, ContainerError::Entry { .. }), "{bad}: {e}");
    }
}

#[test]
fn a_name_that_is_not_utf8_is_refused() {
    let mut f = Forge::stored("ab", b"1");
    f.name = "ab".into();
    let mut bytes = forged(vec![f], &[b"1"]);
    // Both the local header and the directory hold the name; break the directory.
    let at = bytes.windows(2).rposition(|w| w == b"ab").unwrap();
    bytes[at] = 0xFF;
    assert!(err(&bytes).contains("UTF-8"));
}

#[test]
fn a_repeated_name_is_refused() {
    let bytes = forged(
        vec![Forge::stored("a", b"1"), Forge::stored("a", b"2")],
        &[b"1", b"2"],
    );
    let e = open(&bytes).unwrap_err();
    assert_eq!(e.entry_name(), Some("a"));
    assert!(e.to_string().contains("twice"));
}

#[test]
fn too_many_entries() {
    let bytes = sample();
    let l = Limits {
        max_entries: 2,
        ..Limits::default()
    };
    let e = Container::from_bytes(bytes, l).unwrap_err();
    assert!(matches!(e, ContainerError::Limit(_)), "{e}");
}

#[test]
fn entry_and_total_limits() {
    let bytes = sample();
    let l = Limits::default().with_total(4000);
    let e = Container::from_bytes(bytes.clone(), l).unwrap_err();
    assert!(e.to_string().contains("history/top.json"), "{e}");
    let l = Limits {
        max_total_bytes: 5000,
        ..Limits::default()
    };
    let e = Container::from_bytes(bytes, l).unwrap_err();
    assert!(e.to_string().contains("together"), "{e}");
}

#[test]
fn a_compression_bomb_is_refused_before_it_is_decoded() {
    // 8 MiB of zeros deflate to a few KiB: a ratio above 200.
    let plain = vec![0u8; 8 << 20];
    let f = Forge::deflated("bomb", &plain);
    assert!(f.compressed < 20_000);
    let bytes = forged(vec![f], &[&plain]);
    let e = open(&bytes).unwrap_err();
    assert!(matches!(e, ContainerError::Limit(_)), "{e}");
    assert!(e.to_string().contains("bomb"));
    // A host that allows it can read it.
    let l = Limits {
        max_ratio: 100_000,
        ..Limits::default()
    };
    let c = Container::from_bytes(bytes, l).unwrap();
    assert_eq!(c.read("bomb").unwrap().len(), 8 << 20);
}

#[test]
fn a_short_text_with_a_high_ratio_is_fine() {
    let plain = vec![b'a'; 100_000];
    let bytes = forged(vec![Forge::deflated("t", &plain)], &[&plain]);
    assert!(open(&bytes).is_ok());
}

#[test]
fn a_directory_that_lies_about_the_size_stops_the_read() {
    // The directory says 10 bytes; the stream has 5000.
    let plain = vec![b'q'; 5000];
    let mut f = Forge::deflated("lie", &plain);
    f.size = 10;
    let bytes = forged_with_manifest(vec![f], br#"[{"name":"lie","size":10,"sha256":"HASH"}]"#);
    let c = open(&bytes).unwrap();
    let e = c.read("lie").unwrap_err();
    assert!(e.to_string().contains("more than"), "{e}");
    // The other way: the stream is shorter than declared.
    let mut f = Forge::deflated("short", b"abc");
    f.size = 50;
    let bytes = forged_with_manifest(vec![f], br#"[{"name":"short","size":50,"sha256":"HASH"}]"#);
    let e = open(&bytes).unwrap().read("short").unwrap_err();
    assert!(e.to_string().contains("directory declares"), "{e}");
}

fn forged_with_manifest(entries: Vec<Forge>, list: &[u8]) -> Vec<u8> {
    let list = String::from_utf8_lossy(list).replace("HASH", &sha256_hex(b"x"));
    let manifest =
        format!(r#"{{"format":"{FORMAT}","format_version":{VERSION},"entries":{list}}}"#);
    let mut all = entries;
    all.push(Forge::stored(MANIFEST_NAME, manifest.as_bytes()));
    build(&all)
}

#[test]
fn a_stored_entry_with_two_sizes_is_refused() {
    let mut f = Forge::stored("a", b"1234");
    f.compressed = 3;
    let bytes = forged(vec![f], &[b"1234"]);
    assert!(err(&bytes).contains("two sizes"));
}

#[test]
fn encryption_methods_and_zip64_are_refused() {
    let mut f = Forge::stored("enc", b"1");
    f.flags = 1;
    assert!(err(&forged(vec![f], &[b"1"])).contains("encrypted"));
    let mut f = Forge::stored("m", b"1");
    f.method = 9;
    assert!(err(&forged(vec![f], &[b"1"])).contains("method 9"));
    let mut f = Forge::stored("z", b"1");
    f.size = 0xFFFF_FFFF;
    assert!(err(&forged(vec![f], &[b"1"])).contains("ZIP64"));
}

#[test]
fn zip64_end_record_and_split_archives_are_refused() {
    let mut bytes = sample();
    let n = bytes.len();
    bytes[n - 6..n - 2].copy_from_slice(&[0xFF; 4]);
    assert!(err(&bytes).contains("ZIP64"));
    let mut bytes = sample();
    let n = bytes.len();
    bytes[n - 18] = 1;
    assert!(err(&bytes).contains("several parts"));
}

#[test]
fn the_manifest_is_required_and_checked() {
    let no_manifest = build(&[Forge::stored("a", b"1")]);
    assert!(err(&no_manifest).contains("manifest is missing"));

    let cases: [(&str, &str); 7] = [
        (
            r#"{"format":"kadakas-container","format_version":1,"entries":[]}"#,
            "does not list it",
        ),
        (
            r#"{"format":"other","format_version":1,"entries":[]}"#,
            "format is",
        ),
        (
            r#"{"format":"kadakas-container","format_version":9,"entries":[]}"#,
            "update Kadakas",
        ),
        ("not json", "cannot be read"),
        (
            r#"{"format":"kadakas-container","format_version":1,"entries":[{"name":"a","size":1,"sha256":"zz"},{"name":"b","size":1,"sha256":"zz"}]}"#,
            "64 lowercase",
        ),
        (
            r#"{"format":"kadakas-container","format_version":1,"entries":[{"name":"manifest.json","size":1,"sha256":"00"}]}"#,
            "lists itself",
        ),
        (
            r#"{"format":"kadakas-container","format_version":1,"entries":[{"name":"a","size":9,"sha256":"HASH"}]}"#,
            "manifest says 9",
        ),
    ];
    for (manifest, want) in cases {
        let manifest = manifest.replace("HASH", &sha256_hex(b"x"));
        let bytes = build(&[
            Forge::stored("a", b"1"),
            Forge::stored(MANIFEST_NAME, manifest.as_bytes()),
        ]);
        let e = err(&bytes);
        assert!(e.contains(want), "{want}: {e}");
    }
    // Listed twice, and listed but missing.
    let h = sha256_hex(b"1");
    let twice = format!(
        r#"{{"format":"kadakas-container","format_version":1,"entries":[{{"name":"a","size":1,"sha256":"{h}"}},{{"name":"a","size":1,"sha256":"{h}"}}]}}"#
    );
    let bytes = build(&[
        Forge::stored("a", b"1"),
        Forge::stored(MANIFEST_NAME, twice.as_bytes()),
    ]);
    assert!(err(&bytes).contains("twice"));
    let ghost = format!(
        r#"{{"format":"kadakas-container","format_version":1,"entries":[{{"name":"a","size":1,"sha256":"{h}"}},{{"name":"ghost","size":1,"sha256":"{h}"}}]}}"#
    );
    let bytes = build(&[
        Forge::stored("a", b"1"),
        Forge::stored(MANIFEST_NAME, ghost.as_bytes()),
    ]);
    let e = open(&bytes).unwrap_err();
    assert_eq!(e.entry_name(), Some("ghost"));
}

#[test]
fn a_big_manifest_or_directory_is_refused() {
    let l = Limits {
        max_manifest_bytes: 10,
        ..Limits::default()
    };
    assert!(Container::from_bytes(sample(), l).is_err());
    let l = Limits {
        max_directory_bytes: 10,
        ..Limits::default()
    };
    assert!(Container::from_bytes(sample(), l).is_err());
}

#[test]
fn a_local_header_that_disagrees_is_refused_at_read() {
    let bytes = forged(vec![Forge::stored("abc", b"1")], &[b"1"]);
    let good = open(&bytes).unwrap();
    assert!(good.read("abc").is_ok());
    // Another name in the local header.
    let mut b = bytes.clone();
    b[30] = b'x';
    let e = open(&b).unwrap().read("abc").unwrap_err();
    assert!(e.to_string().contains("another name"), "{e}");
    // Another method.
    let mut b = bytes.clone();
    b[8] = 8;
    let e = open(&b).unwrap().read("abc").unwrap_err();
    assert!(e.to_string().contains("method"), "{e}");
    // No signature.
    let mut b = bytes.clone();
    b[0] = 0;
    let e = open(&b).unwrap().read("abc").unwrap_err();
    assert!(e.to_string().contains("local header"), "{e}");
}

#[test]
fn a_garbage_deflate_stream_is_an_entry_error() {
    let mut f = Forge::deflated("g", b"hello hello hello hello");
    f.data = vec![0xFF; f.data.len()];
    let bytes = forged_with_manifest(vec![f], br#"[{"name":"g","size":23,"sha256":"HASH"}]"#);
    let e = open(&bytes).unwrap().read("g").unwrap_err();
    assert_eq!(e.entry_name(), Some("g"));
}

#[test]
fn a_directory_with_extra_bytes_or_a_wrong_header_is_refused() {
    let bytes = sample();
    // Wrong signature of the first directory entry.
    let c = open(&bytes).unwrap();
    let dir_at = bytes
        .windows(4)
        .position(|w| w == 0x0201_4b50u32.to_le_bytes())
        .unwrap();
    let mut b = bytes.clone();
    b[dir_at] = 0;
    assert!(err(&b).contains("wrong entry header"));
    // The directory size in the end record is one byte too big.
    let mut b = bytes.clone();
    let n = b.len();
    let size = u32::from_le_bytes(b[n - 10..n - 6].try_into().unwrap());
    b[n - 10..n - 6].copy_from_slice(&(size + 1).to_le_bytes());
    assert!(open(&b).is_err());
    let _ = c;
}

#[test]
fn a_comment_after_the_end_record_is_found() {
    let mut bytes = sample();
    let n = bytes.len();
    bytes[n - 2..].copy_from_slice(&5u16.to_le_bytes());
    bytes.extend(b"hello");
    assert!(open(&bytes).is_ok());
}

#[test]
fn a_part_source_over_memory() {
    let c = open(&sample()).unwrap();
    let s = ContainerSource::new(c.clone());
    assert_eq!(s.part_size("blobs/aa"), Some(13));
    assert_eq!(s.part_size("none"), None);
    assert_eq!(s.read_part("blobs/aa").unwrap(), b"picture bytes");
    assert_eq!(s.read_prefix("blobs/aa", 3).unwrap(), b"pic");
    assert_eq!(s.raw_part("blobs/aa").unwrap().size, 13);
    assert!(s.check_unchanged().is_ok());
    assert_eq!(s.manifest_sha256(), c.manifest_sha256());
    assert!(!s.can_refresh());
    assert!(s.refresh().is_ok());
    assert!(format!("{s:?}").contains("ContainerSource"));
    assert!(!s.label().is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
mod files {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kc-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_file_source_reads_by_name_and_follows_a_save_over_the_file() {
        let d = dir("src");
        let p = d.join("f.kad");
        std::fs::write(&p, sample()).unwrap();
        let s = ContainerSource::from_path(&p, Limits::default()).unwrap();
        assert!(s.can_refresh());
        assert_eq!(s.path(), Some(p.clone()));
        assert_eq!(s.read_part("blobs/aa").unwrap(), b"picture bytes");
        // Another file replaces it: the old source says so and reads nothing.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let other = write(&[("blobs/aa", Compression::Stored, b"other bytes")]);
        std::fs::write(&p, other).unwrap();
        let e = s.read_part("blobs/aa").unwrap_err();
        assert!(matches!(e, ContainerError::Changed(_)), "{e}");
        assert!(e.to_string().contains("changed on disk"));
        assert!(s.check_unchanged().is_err());
        // After a refresh it reads the new file.
        s.refresh().unwrap();
        assert_eq!(s.read_part("blobs/aa").unwrap(), b"other bytes");
        assert!(s.read_part("document.json").is_err());
        assert!(s.check_unchanged().is_ok());
        // A refresh that cannot open keeps the old container.
        std::fs::write(&p, b"junk").unwrap();
        assert!(s.refresh().is_err());
        // A source can move to another file with the same parts.
        let q = d.join("g.kad");
        std::fs::write(
            &q,
            write(&[("blobs/aa", Compression::Stored, b"moved bytes")]),
        )
        .unwrap();
        s.rebind(&q).unwrap();
        assert_eq!(s.path(), Some(q.clone()));
        assert_eq!(s.read_part("blobs/aa").unwrap(), b"moved bytes");
        assert!(s.rebind(&d.join("none.kad")).is_err());
        assert_eq!(s.path(), Some(q));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn same_size_and_time_but_other_content_is_still_found_by_the_hash() {
        let d = dir("same");
        let p = d.join("f.kad");
        let bytes = write(&[("blobs/aa", Compression::Stored, b"picture bytes")]);
        std::fs::write(&p, &bytes).unwrap();
        let s = ContainerSource::from_path(&p, Limits::default()).unwrap();
        let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
        let mut b = bytes.clone();
        let at = b.windows(7).position(|w| w == b"picture").unwrap();
        b[at] = b'P';
        std::fs::write(&p, &b).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let e = s.read_part("blobs/aa").unwrap_err();
        assert_eq!(e.entry_name(), Some("blobs/aa"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_file_with_the_same_size_and_time_but_another_manifest_is_found() {
        let d = dir("manifest");
        let p = d.join("f.kad");
        let a = write(&[("blobs/aa", Compression::Stored, b"picture bytes")]);
        // Another content of the same size, with a generator text that makes the file as long.
        let b = (0..200)
            .map(|pad| {
                let mut w = Writer::new(Vec::new()).generator(&"x".repeat(pad));
                w.add_bytes("blobs/aa", Compression::Stored, b"PICTURE BYTES")
                    .unwrap();
                w.finish().unwrap()
            })
            .find(|b| b.len() == a.len())
            .expect("a padding that makes the same size");
        std::fs::write(&p, &a).unwrap();
        let s = ContainerSource::from_path(&p, Limits::default()).unwrap();
        assert!(s.check_unchanged().is_ok());
        assert!(s.verify_manifest().is_ok());
        let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::fs::write(&p, &b).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        // The size and the time are the same; the manifest is not.
        assert!(s.container().verify().is_ok());
        let e = s.check_unchanged().unwrap_err();
        assert!(matches!(e, ContainerError::Changed(_)), "{e}");
        assert!(e.to_string().contains("manifest"), "{e}");
        // A file that is gone or damaged says that it changed.
        std::fs::write(&p, b"junk of the same size").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let e = s.verify_manifest().unwrap_err();
        assert!(e.to_string().contains("changed on disk"), "{e}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn open_path_of_a_missing_file_is_an_error() {
        let d = dir("missing");
        assert!(ContainerSource::from_path(&d.join("none"), Limits::default()).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}

#[test]
fn an_arc_medium_can_be_shared() {
    let m: Arc<dyn Medium> = Arc::new(MemMedium::new(sample()));
    let a = Container::open(m.clone(), Limits::default()).unwrap();
    let b = Container::open(m, Limits::default()).unwrap();
    assert_eq!(a.manifest_sha256(), b.manifest_sha256());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = Container::from_bytes(bytes, Limits::default());
    }

    #[test]
    fn random_entries_round_trip(
        parts in proptest::collection::vec(
            (proptest::collection::vec(any::<u8>(), 0..300), any::<bool>()), 0..8)
    ) {
        let mut w = Writer::new(Vec::new());
        for (i, (data, deflate)) in parts.iter().enumerate() {
            let how = if *deflate { Compression::Deflate(1) } else { Compression::Stored };
            w.add_bytes(&format!("p/{i}"), how, data).unwrap();
        }
        let bytes = w.finish().unwrap();
        let c = Container::from_bytes(bytes, Limits::default()).unwrap();
        prop_assert_eq!(c.entries().len(), parts.len());
        for (i, (data, _)) in parts.iter().enumerate() {
            prop_assert_eq!(&c.read(&format!("p/{i}")).unwrap(), data);
        }
    }

    #[test]
    fn a_flipped_byte_never_gives_other_data(at in 0usize..1000, mask in 1u8..=255) {
        let bytes = sample();
        let good = open(&bytes).unwrap();
        let mut b = bytes.clone();
        let i = at % b.len();
        b[i] ^= mask;
        if let Ok(c) = open(&b) {
            for e in good.entries() {
                if let Ok(data) = c.read(&e.name) {
                    prop_assert_eq!(data, good.read(&e.name).unwrap());
                }
            }
        }
    }
}
