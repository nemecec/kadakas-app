//! The containers an IFC file comes in: STEP (`.ifc`), ifcZIP (`.ifczip`, a zip with one `.ifc`
//! inside), and the two encodings that Kadakas does not read: IFC XML and IFC JSON.
//!
//! The zip reader is built for files that nobody trusts:
//!
//! - It never writes anything to disk, so an entry name with `..` or a drive letter cannot leave a
//!   folder. An archive with such a name is refused all the same: it is not an ifcZIP of an exporter.
//! - It limits the number of entries ([`MAX_ENTRIES`]), the expanded size of the IFC file (the size
//!   limit of the import), and the ratio of expanded size to zip size above [`RATIO_FROM`] bytes
//!   ([`MAX_RATIO`]). The size is counted while reading, not taken from the header.
//! - It reads exactly one IFC entry. Other entries (a readme, a picture) are ignored and counted.

use std::io::{Read, Seek, Write};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::import::IfcImportError;

/// Most entries in an ifcZIP.
pub const MAX_ENTRIES: usize = 1000;
/// Most expanded bytes for one byte of zip (the ratio is checked only for big files).
pub const MAX_RATIO: u64 = 200;
/// The ratio is not checked below this expanded size: a small file cannot hurt.
pub const RATIO_FROM: u64 = 64 << 20;

/// What the first bytes of a file say it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    /// A STEP physical file (or something that the STEP reader will refuse).
    Step,
    /// A zip file.
    Zip,
    /// IFC XML (`<?xml` or `<ifcXML`).
    Xml,
    /// IFC JSON (starts with `{` or `[`).
    Json,
}

/// Looks at the first bytes of a file.
pub fn sniff(head: &[u8]) -> Container {
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Container::Zip;
    }
    let h = head.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(head);
    let h = &h[h
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(h.len())..];
    match h.first() {
        Some(b'<') => Container::Xml,
        Some(b'{' | b'[') => Container::Json,
        _ => Container::Step,
    }
}

/// The error for an encoding that is not read.
pub fn refusal(kind: Container) -> IfcImportError {
    IfcImportError::Format(
        match kind {
            Container::Xml => "IFC XML is not supported; export STEP (.ifc) or ifcZIP",
            _ => "IFC JSON is not supported; export STEP (.ifc) or ifcZIP",
        }
        .into(),
    )
}

/// The IFC file inside an ifcZIP.
pub struct Unpacked {
    pub data: Vec<u8>,
    /// Remarks: entries that were ignored.
    pub notes: Vec<String>,
}

fn bad(msg: impl Into<String>) -> IfcImportError {
    IfcImportError::Format(msg.into())
}

fn has_ext(name: &str, ext: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext))
}

/// True when the name could leave the folder it is unpacked into.
fn unsafe_name(name: &str) -> bool {
    let n = name.replace('\\', "/");
    n.starts_with('/')
        || n.split('/').any(|p| p == "..")
        || n.as_bytes().get(1) == Some(&b':')
        || n.contains('\0')
}

/// Reads the one IFC file of an ifcZIP. `max_bytes` caps the expanded size.
pub fn unpack<R: Read + Seek>(r: R, max_bytes: usize) -> Result<Unpacked, IfcImportError> {
    let mut zip = zip::ZipArchive::new(r).map_err(|e| bad(format!("not a valid zip file: {e}")))?;
    if zip.len() > MAX_ENTRIES {
        return Err(bad(format!(
            "the zip file has {} entries; the limit is {MAX_ENTRIES}",
            zip.len()
        )));
    }
    let mut ifc: Option<usize> = None;
    let mut others = 0usize;
    let mut other_kinds = Vec::new();
    for i in 0..zip.len() {
        let f = zip
            .by_index_raw(i)
            .map_err(|e| bad(format!("a zip entry cannot be read: {e}")))?;
        let Ok(name) = f.name().map(|n| n.into_owned()) else {
            return Err(bad(
                "the zip file has an entry with a name that is not valid text: it is refused",
            ));
        };
        if unsafe_name(&name) {
            return Err(bad(format!(
                "the zip file has an entry with an unsafe name ({}): it is refused",
                name.chars()
                    .filter(|c| !c.is_control())
                    .take(80)
                    .collect::<String>()
            )));
        }
        if f.is_dir() {
            continue;
        }
        if has_ext(&name, "ifc") {
            if ifc.is_some() {
                return Err(bad(
                    "the zip file holds more than one .ifc file: an ifcZIP has one",
                ));
            }
            ifc = Some(i);
        } else {
            others += 1;
            if has_ext(&name, "ifcxml") || has_ext(&name, "xml") {
                other_kinds.push("IFC XML");
            } else if has_ext(&name, "ifcjson") || has_ext(&name, "json") {
                other_kinds.push("IFC JSON");
            }
        }
    }
    let Some(index) = ifc else {
        return Err(match other_kinds.first() {
            Some(k) => bad(format!(
                "the zip file holds {k}, which is not supported; export STEP (.ifc) or ifcZIP with an .ifc file"
            )),
            None => bad("the zip file has no .ifc file"),
        });
    };
    let f = zip
        .by_index(index)
        .map_err(|e| bad(format!("the IFC file in the zip cannot be read: {e}")))?;
    let (declared, packed) = (f.size(), f.compressed_size());
    let too_big = |what: &str| bad(format!("the IFC file in the zip is too big ({what})"));
    if declared > max_bytes as u64 {
        return Err(too_big(&format!("more than {max_bytes} bytes")));
    }
    // The header may lie: count the bytes that come out.
    let mut data = Vec::with_capacity(declared.min(64 << 20) as usize);
    let mut reader = f.take(max_bytes as u64 + 1);
    reader
        .read_to_end(&mut data)
        .map_err(|e| bad(format!("the IFC file in the zip cannot be unpacked: {e}")))?;
    if data.len() > max_bytes {
        return Err(too_big(&format!("more than {max_bytes} bytes")));
    }
    let len = data.len() as u64;
    if len > RATIO_FROM && len / packed.max(1) > MAX_RATIO {
        return Err(too_big(&format!(
            "it grows by more than {MAX_RATIO} times when unpacked"
        )));
    }
    let mut notes = Vec::new();
    if others > 0 {
        notes.push(format!(
            "{others} other entries in the zip file were ignored (only the .ifc file is read)"
        ));
    }
    Ok(Unpacked { data, notes })
}

/// Starts an ifcZIP on `out` with the entry `name` (an `.ifc` name). Write the IFC file into the
/// returned writer, then call [`finish`]. The time stamp of the entry is fixed, so the same
/// document gives the same bytes.
pub fn start<W: Write + Seek>(out: W, name: &str) -> Result<zip::ZipWriter<W>, std::io::Error> {
    let mut z = zip::ZipWriter::new(out);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    z.start_file(name, opts).map_err(std::io::Error::other)?;
    Ok(z)
}

/// Ends an ifcZIP.
pub fn finish<W: Write + Seek>(z: zip::ZipWriter<W>) -> Result<W, std::io::Error> {
    z.finish().map_err(std::io::Error::other)
}

/// The name of the IFC entry for an ifcZIP file name: `site.ifczip` gives `site.ifc`.
pub fn inner_name(zip_name: &str) -> String {
    let base = zip_name.rsplit(['/', '\\']).next().unwrap_or(zip_name);
    let stem = match base.rsplit_once('.') {
        Some((s, e)) if e.eq_ignore_ascii_case("ifczip") || e.eq_ignore_ascii_case("zip") => s,
        _ => base,
    };
    let stem: String = stem.chars().filter(|c| !c.is_control()).collect();
    format!("{}.ifc", if stem.is_empty() { "model" } else { &stem })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (n, d) in entries {
            z.start_file(*n, o).unwrap();
            z.write_all(d).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    fn err(r: Result<Unpacked, IfcImportError>) -> String {
        match r {
            Err(e) => e.to_string(),
            Ok(_) => panic!("accepted"),
        }
    }

    #[test]
    fn the_first_bytes_tell_the_container() {
        assert_eq!(sniff(b"PK\x03\x04abc"), Container::Zip);
        assert_eq!(sniff(b"ISO-10303-21;"), Container::Step);
        assert_eq!(sniff(b"\xEF\xBB\xBF  <?xml version"), Container::Xml);
        assert_eq!(sniff(b"\n{\"type\""), Container::Json);
        assert_eq!(sniff(b""), Container::Step);
        assert!(
            refusal(Container::Xml)
                .to_string()
                .starts_with("IFC XML is not supported")
        );
        assert!(
            refusal(Container::Json)
                .to_string()
                .starts_with("IFC JSON is not supported")
        );
    }

    #[test]
    fn one_ifc_entry_is_read_and_the_others_are_counted() {
        let z = make(&[
            ("a/readme.txt", b"hi"),
            ("site.IFC", b"ISO-10303-21;"),
            ("d/", b""),
        ]);
        let u = unpack(Cursor::new(z), 1 << 20).unwrap();
        assert_eq!(u.data, b"ISO-10303-21;");
        assert_eq!(u.notes.len(), 1);
        assert!(u.notes[0].starts_with("1 other entries"));
    }

    #[test]
    fn wrong_archives_are_refused_with_a_reason() {
        let two = make(&[("a.ifc", b"x"), ("b.ifc", b"y")]);
        assert!(err(unpack(Cursor::new(two), 1 << 20)).contains("more than one .ifc"));
        let none = make(&[("a.txt", b"x")]);
        assert!(err(unpack(Cursor::new(none), 1 << 20)).contains("no .ifc file"));
        let xml = make(&[("a.ifcXML", b"<ifcXML/>")]);
        assert!(err(unpack(Cursor::new(xml), 1 << 20)).contains("IFC XML"));
        let json = make(&[("a.ifcJSON", b"{}")]);
        assert!(err(unpack(Cursor::new(json), 1 << 20)).contains("IFC JSON"));
        assert!(err(unpack(Cursor::new(b"PK\x03\x04junk".to_vec()), 1 << 20)).contains("zip"));
        assert!(err(unpack(Cursor::new(Vec::new()), 1 << 20)).contains("zip"));
    }

    #[test]
    fn unsafe_names_are_refused() {
        for n in [
            "../evil.ifc",
            "a/../../evil.ifc",
            "/etc/x.ifc",
            "C:\\x.ifc",
            "..\\x.ifc",
        ] {
            let z = make(&[(n, b"ISO-10303-21;")]);
            assert!(
                err(unpack(Cursor::new(z), 1 << 20)).contains("unsafe name"),
                "{n}"
            );
        }
    }

    #[test]
    fn zip_bombs_are_stopped() {
        // Too many entries.
        let names: Vec<String> = (0..MAX_ENTRIES + 1).map(|i| format!("f{i}.txt")).collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
        assert!(err(unpack(Cursor::new(make(&entries)), 1 << 20)).contains("entries"));
        // More bytes than the limit.
        let big = vec![b' '; 3 << 20];
        let z = make(&[("a.ifc", &big)]);
        assert!(err(unpack(Cursor::new(z.clone()), 1 << 20)).contains("too big"));
        assert!(unpack(Cursor::new(z), 4 << 20).is_ok());
        // A huge ratio: 80 MiB of one byte is about 80 KiB of zip.
        let huge = vec![0u8; 80 << 20];
        let z = make(&[("a.ifc", &huge)]);
        assert!(z.len() < (1 << 20));
        assert!(err(unpack(Cursor::new(z), 1 << 30)).contains("grows by more than"));
    }

    #[test]
    fn written_zips_read_back() {
        let mut z = start(Cursor::new(Vec::new()), "x.ifc").unwrap();
        z.write_all(b"ISO-10303-21;\nEND-ISO-10303-21;\n").unwrap();
        let bytes = finish(z).unwrap().into_inner();
        assert_eq!(sniff(&bytes), Container::Zip);
        let u = unpack(Cursor::new(bytes.clone()), 1 << 20).unwrap();
        assert!(u.data.starts_with(b"ISO-10303-21;"));
        // The same input gives the same bytes.
        let mut z2 = start(Cursor::new(Vec::new()), "x.ifc").unwrap();
        z2.write_all(b"ISO-10303-21;\nEND-ISO-10303-21;\n").unwrap();
        assert_eq!(finish(z2).unwrap().into_inner(), bytes);
    }

    #[test]
    fn inner_names() {
        assert_eq!(inner_name("/a/b/site.ifczip"), "site.ifc");
        assert_eq!(inner_name("C:\\a\\Site.IFCZIP"), "Site.ifc");
        assert_eq!(inner_name("x"), "x.ifc");
        assert_eq!(inner_name(".ifczip"), "model.ifc");
    }
}
