//! Reading the original file of a linked import (IOP-22, for IOP-23).
//!
//! A drawing with more entities than an import keeps for updates can be imported with
//! `base: "link"`. The layer source then holds a [`FileLink`] (the absolute path, the size and the
//! SHA-256 of the file) instead of the summaries of the entities. An update calls [`read_records`]
//! to get the old summaries back: it reads the old file, checks that it is the file that was
//! imported, parses it and rebuilds the records (`revision::records`, compacted like a stored
//! base). It then diffs them against the records of the new file.
//!
//! The path is a hint from a file or from another user. This module reads whatever path it is
//! given: the caller applies the path policy of its session to `Path::new(&link.path)` first. All
//! work is bounded: the file by `max_bytes` (checked before the read), the parse by the size, and
//! the records by `max_recs`.

use std::path::Path;

use kadakas_core::provenance::FileLink;

use super::parse;
use super::revision::{self, Rec};

/// Why the old file could not be used. The update says so to the person.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LinkError {
    /// The link itself is not valid (see `FileLink::problem`).
    #[error("the link to the original file is not valid: {0}")]
    BadLink(&'static str),
    /// There is no file at the path any more.
    #[error("the original file {0} is gone: give the old file to compare with")]
    Gone(String),
    /// The file cannot be read (a folder, no access, an error of the disk).
    #[error("cannot read the original file {path}: {why}")]
    Unreadable { path: String, why: String },
    /// The file is bigger than the limit for a read.
    #[error("the original file {path} is {bytes} bytes, more than the limit of {max}")]
    TooLarge { path: String, bytes: u64, max: u64 },
    /// The file has another size or another hash than the file that was imported.
    #[error(
        "the original file {0} has changed since the import: give the old file to compare with"
    )]
    Changed(String),
    /// The file is the right one but is not a DXF file that can be read.
    #[error("the original file {path} could not be read as DXF: {why}")]
    Format { path: String, why: String },
    /// The file has more entities than the caller allows.
    #[error("the original file {path} has more than {max} entities")]
    TooMany { path: String, max: usize },
}

/// Checks the bytes of the old file against the link, parses them and rebuilds the records. `name`
/// is the path for the messages.
pub fn records_of_bytes(
    data: &[u8],
    link: &FileLink,
    name: &str,
    max_recs: usize,
) -> Result<Vec<Rec>, LinkError> {
    if data.len() as u64 != link.bytes || kadakas_core::picture::sha256_hex(data) != link.sha256 {
        return Err(LinkError::Changed(name.into()));
    }
    records_of_dxf(data, name, max_recs)
}

/// Parses DXF bytes and rebuilds the records, compacted like a stored base. `name` is the path for
/// the messages. The caller has checked that the bytes are the file of the link (for a DWG file the
/// link is the DWG, and these bytes are the converted DXF).
pub fn records_of_dxf(data: &[u8], name: &str, max_recs: usize) -> Result<Vec<Rec>, LinkError> {
    let dxf = parse::parse(data).map_err(|e| LinkError::Format {
        path: name.into(),
        why: e.to_string(),
    })?;
    let (mut recs, cut) = revision::records(&dxf, max_recs);
    if cut {
        return Err(LinkError::TooMany {
            path: name.into(),
            max: max_recs,
        });
    }
    recs.iter_mut().for_each(Rec::compact);
    Ok(recs)
}

/// Opens `path` as a regular file of at most `max_bytes`, with the errors of an update.
pub(crate) fn open(
    path: &Path,
    max_bytes: u64,
) -> Result<std::io::BufReader<kadakas_sys::fsio::Capped<std::fs::File>>, LinkError> {
    use kadakas_sys::fsio::{OpenError, open_regular};
    let name = path.display().to_string();
    open_regular(path, max_bytes).map_err(|e| match e {
        OpenError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => LinkError::Gone(name),
        OpenError::TooLarge { len, max } => LinkError::TooLarge {
            path: name,
            bytes: len,
            max,
        },
        e => LinkError::Unreadable {
            path: name,
            why: e.to_string(),
        },
    })
}

/// Reads the old file at `path` (the path of the link, after the caller's path policy) and
/// rebuilds its records. `max_bytes` bounds the read and `max_recs` the records.
///
/// The size of the file is compared with the link before the file is read, so a file that has
/// changed in size is not read at all.
pub fn read_records(
    path: &Path,
    link: &FileLink,
    max_bytes: u64,
    max_recs: usize,
) -> Result<Vec<Rec>, LinkError> {
    use std::io::Read;
    if let Some(why) = link.problem() {
        return Err(LinkError::BadLink(why));
    }
    let name = path.display().to_string();
    let mut reader = open(path, max_bytes)?;
    // A file of another size is not the imported one: it is not read at all.
    if std::fs::metadata(path).is_ok_and(|m| m.len() != link.bytes) {
        return Err(LinkError::Changed(name));
    }
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| LinkError::Unreadable {
            path: name.clone(),
            why: e.to_string(),
        })?;
    records_of_bytes(&data, link, &name, max_recs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(lines: usize, tail: &str) -> String {
        let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
        for k in 0..lines {
            s.push_str(&format!(
                "0\nLINE\n5\n{:X}\n8\nL\n10\n{k}\n20\n0\n11\n{k}\n21\n5\n",
                k + 10
            ));
        }
        s.push_str("0\nENDSEC\n0\nEOF\n");
        s.push_str(tail);
        s
    }

    fn folder(name: &str) -> kadakas_sys::tmp::PrivateTempDir {
        kadakas_sys::tmp::PrivateTempDir::new(name).unwrap()
    }

    #[test]
    fn the_records_come_back_from_the_linked_file() {
        let dir = folder("kadakas-link-ok");
        let path = dir.path().join("old.dxf");
        let text = dxf(5, "");
        std::fs::write(&path, &text).unwrap();
        let link = FileLink::of_bytes(path.to_str().unwrap(), text.as_bytes());
        let recs = read_records(&path, &link, 1 << 20, 100).unwrap();
        assert_eq!(recs.len(), 5);
        // The same records as the stored base would hold.
        let parsed = parse::parse(text.as_bytes()).unwrap();
        let (mut want, _) = revision::records(&parsed, 100);
        want.iter_mut().for_each(Rec::compact);
        assert_eq!(recs, want);
    }

    #[test]
    fn a_file_that_is_gone_or_changed_is_told() {
        let dir = folder("kadakas-link-changed");
        let path = dir.path().join("old.dxf");
        let text = dxf(3, "");
        std::fs::write(&path, &text).unwrap();
        let link = FileLink::of_bytes(path.to_str().unwrap(), text.as_bytes());
        // Same size, other content: the hash tells.
        let mut other = text.clone().into_bytes();
        let at = other.iter().position(|&b| b == b'L').unwrap();
        other[at] = b'M';
        std::fs::write(&path, &other).unwrap();
        let e = read_records(&path, &link, 1 << 20, 100).unwrap_err();
        assert!(matches!(e, LinkError::Changed(_)), "{e}");
        assert!(e.to_string().contains("changed"), "{e}");
        // Another size: refused without parsing.
        std::fs::write(&path, dxf(4, "")).unwrap();
        assert!(matches!(
            read_records(&path, &link, 1 << 20, 100),
            Err(LinkError::Changed(_))
        ));
        // Gone.
        std::fs::remove_file(&path).unwrap();
        let e = read_records(&path, &link, 1 << 20, 100).unwrap_err();
        assert!(matches!(e, LinkError::Gone(_)), "{e}");
        assert!(e.to_string().contains("gone"), "{e}");
    }

    #[test]
    fn the_work_is_bounded() {
        let dir = folder("kadakas-link-bounds");
        let path = dir.path().join("old.dxf");
        let text = dxf(10, "");
        std::fs::write(&path, &text).unwrap();
        let link = FileLink::of_bytes(path.to_str().unwrap(), text.as_bytes());
        // Too big for the read: the size is checked before the read.
        let e = read_records(&path, &link, text.len() as u64 - 1, 100).unwrap_err();
        assert!(matches!(e, LinkError::TooLarge { .. }), "{e}");
        // Too many entities.
        let e = read_records(&path, &link, 1 << 20, 9).unwrap_err();
        assert_eq!(
            e,
            LinkError::TooMany {
                path: path.display().to_string(),
                max: 9
            }
        );
        assert!(read_records(&path, &link, 1 << 20, 10).is_ok());
    }

    #[test]
    fn a_bad_link_a_folder_and_a_file_that_is_not_dxf_are_told() {
        let dir = folder("kadakas-link-bad");
        let path = dir.path().join("old.dxf");
        // The link itself is not valid (a relative path).
        let mut link = FileLink::of_bytes("/x/old.dxf", b"abc");
        link.path = "old.dxf".into();
        assert!(matches!(
            read_records(&path, &link, 1 << 20, 10),
            Err(LinkError::BadLink(_))
        ));
        // A folder cannot be read.
        let link = FileLink::of_bytes("/x/old.dxf", b"abc");
        let e = read_records(dir.path(), &link, 1 << 20, 10).unwrap_err();
        assert!(matches!(e, LinkError::Unreadable { .. }), "{e}");
        // The right file by hash, but not a DXF file.
        let junk = vec![0xffu8; 64];
        std::fs::write(&path, &junk).unwrap();
        let link = FileLink::of_bytes("/x/old.dxf", &junk);
        let e = read_records(&path, &link, 1 << 20, 10).unwrap_err();
        let text = e.to_string();
        assert!(
            matches!(e, LinkError::Format { .. }) && text.contains("DXF"),
            "{text}"
        );
    }
}
