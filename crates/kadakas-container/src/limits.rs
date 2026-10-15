//! The bounds for reading a container, which can be a hostile file.

use crate::error::ContainerError;

/// The most entries a container can hold. The ZIP end record has 16 bits for the count, and the
/// value 0xFFFF means ZIP64, which this code does not read or write.
pub const MAX_ENTRIES: usize = 65_534;
/// The largest size of an entry, of the offsets and of the whole file: the 32 bit fields of ZIP
/// (the value 0xFFFFFFFF means ZIP64).
pub const MAX_FIELD: u64 = 0xFFFF_FFFE;
/// The name of the manifest entry.
pub const MANIFEST_NAME: &str = "manifest.json";

/// Bounds for [`crate::Container::open`] and the reads of entries. The reader checks them before
/// it allocates or decodes, and a read stops at the declared size, so a small file cannot make a
/// big allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Most entries (at most [`MAX_ENTRIES`]).
    pub max_entries: usize,
    /// Most bytes of one entry, uncompressed.
    pub max_entry_bytes: u64,
    /// Most bytes of all entries together, uncompressed.
    pub max_total_bytes: u64,
    /// Most times that an entry may grow when it is decompressed. Only entries above
    /// `ratio_floor` bytes are checked, because a short text can compress very well.
    pub max_ratio: u64,
    /// Entries of this many bytes or fewer have no ratio check.
    pub ratio_floor: u64,
    /// Longest entry name in bytes.
    pub max_name_bytes: usize,
    /// Most bytes of the central directory.
    pub max_directory_bytes: u64,
    /// Most bytes of the manifest.
    pub max_manifest_bytes: u64,
}

impl Default for Limits {
    /// The bounds of a program that opens the files of a person: the limits of the format, and a
    /// ratio that real documents stay far below (see `docs/doc-load-benchmark.md`).
    fn default() -> Self {
        Limits {
            max_entries: MAX_ENTRIES,
            max_entry_bytes: MAX_FIELD,
            max_total_bytes: MAX_FIELD,
            max_ratio: 200,
            ratio_floor: 1 << 20,
            max_name_bytes: 255,
            max_directory_bytes: 32 * 1024 * 1024,
            max_manifest_bytes: 32 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// The same limits with a cap on the uncompressed bytes of the whole container (and of one
    /// entry).
    pub fn with_total(mut self, bytes: u64) -> Limits {
        self.max_total_bytes = self.max_total_bytes.min(bytes);
        self.max_entry_bytes = self.max_entry_bytes.min(bytes);
        self
    }

    pub(crate) fn clamped(mut self) -> Limits {
        self.max_entries = self.max_entries.min(MAX_ENTRIES);
        self.max_entry_bytes = self.max_entry_bytes.min(MAX_FIELD);
        self.max_total_bytes = self.max_total_bytes.min(u64::MAX / 2);
        self.max_name_bytes = self.max_name_bytes.clamp(1, 1024);
        self
    }
}

/// Checks the name of an entry: a relative path of plain parts with `/` between them. It is not
/// empty, has no `..` or `.` part, no empty part, no leading `/`, no `\`, no drive letter and no
/// control character, and is not longer than `max`. This keeps a name from pointing outside the
/// container if a program ever writes entries to a folder.
pub fn check_name(name: &str, max: usize) -> Result<(), ContainerError> {
    let bad = |why: &str| Err(ContainerError::entry(&shown(name), why));
    if name.is_empty() {
        return bad("the name is empty");
    }
    if name.len() > max {
        return bad("the name is too long");
    }
    if name.starts_with('/') {
        return bad("the name is an absolute path");
    }
    if name.contains('\\') {
        return bad("the name has a backslash");
    }
    if name.chars().any(|c| c.is_control()) {
        return bad("the name has a control character");
    }
    let mut parts = name.split('/').peekable();
    while let Some(part) = parts.next() {
        if part.is_empty() {
            // A trailing slash is a folder entry, which a container does not have.
            return bad(if parts.peek().is_none() {
                "the name is a folder"
            } else {
                "the name has an empty part"
            });
        }
        if part == ".." {
            return bad("the name has a .. part");
        }
        if part == "." {
            return bad("the name has a . part");
        }
    }
    let first = name.split('/').next().unwrap_or_default();
    if first.len() == 2 && first.ends_with(':') {
        return bad("the name starts with a drive letter");
    }
    Ok(())
}

/// A name for an error text: cut, so a hostile name cannot flood a message.
pub(crate) fn shown(name: &str) -> String {
    let mut s: String = name.chars().filter(|c| !c.is_control()).take(80).collect();
    if name.chars().count() > 80 {
        s.push_str("...");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_and_bad_names() {
        for good in [
            "document.json",
            "blobs/ab12",
            "history/top.json",
            "a/b/c.txt",
        ] {
            assert!(check_name(good, 255).is_ok(), "{good}");
        }
        for bad in [
            "", "/abs", "../x", "a/../b", "a/./b", "a//b", "a/", "a\\b", "C:/x", "a\u{0}b", "a\nb",
        ] {
            assert!(check_name(bad, 255).is_err(), "{bad:?}");
        }
        assert!(check_name(&"a".repeat(256), 255).is_err());
        assert!(check_name(&"a".repeat(255), 255).is_ok());
    }

    #[test]
    fn a_long_name_is_cut_in_the_message() {
        let e = check_name(&format!("{}/..", "x".repeat(300)), 255).unwrap_err();
        assert!(e.to_string().len() < 200);
    }

    #[test]
    fn limits_are_clamped() {
        let l = Limits {
            max_entries: usize::MAX,
            max_entry_bytes: u64::MAX,
            max_name_bytes: 0,
            ..Limits::default()
        }
        .clamped();
        assert_eq!(l.max_entries, MAX_ENTRIES);
        assert_eq!(l.max_entry_bytes, MAX_FIELD);
        assert_eq!(l.max_name_bytes, 1);
        let t = Limits::default().with_total(10);
        assert_eq!((t.max_total_bytes, t.max_entry_bytes), (10, 10));
    }
}
