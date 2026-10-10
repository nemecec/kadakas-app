//! Unpacking of `.tar.gz` update archives, with path checks.
//!
//! The release workflow makes one `.tar.gz` per system. An archive is not trusted, even after the
//! hash check, so every entry is checked:
//!
//! - Only plain files and folders are written. Links, devices and pipes are refused.
//! - A path must be relative, with normal names only: no `..`, no root, no drive letter, no
//!   backslash and no `:` (Windows streams). This stops "zip slip" archives.
//! - A file that exists already is never overwritten, so a duplicate entry is an error.
//! - The number of entries and the total size are limited.

use crate::error::{Result, UpdateError};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Limits for one unpack.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_entries: 10_000,
            max_total_bytes: 4 * 1024 * 1024 * 1024,
        }
    }
}

/// Checks an archive path and returns it as a clean relative path.
pub fn safe_relative(path: &Path) -> Result<PathBuf> {
    let text = path.to_string_lossy();
    let unsafe_path = || UpdateError::UnsafePath(text.to_string());
    if text.contains('\\') || text.contains(':') || text.contains('\0') {
        return Err(unsafe_path());
    }
    let mut clean = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(n) => clean.push(n),
            Component::CurDir => {}
            _ => return Err(unsafe_path()),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(unsafe_path());
    }
    Ok(clean)
}

/// Unpacks a `.tar.gz` stream into `dest`. The folder `dest` must exist.
/// Returns the number of files written.
pub fn extract_tar_gz(reader: impl Read, dest: &Path, limits: Limits) -> Result<usize> {
    let gz = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(gz);
    let mut entries_seen = 0usize;
    let mut total = 0u64;
    let mut files = 0usize;
    let entries = archive
        .entries()
        .map_err(|e| UpdateError::Archive(e.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| UpdateError::Archive(e.to_string()))?;
        entries_seen += 1;
        if entries_seen > limits.max_entries {
            return Err(UpdateError::Archive("too many entries".into()));
        }
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() {
            continue;
        }
        let raw = entry
            .path()
            .map_err(|e| UpdateError::Archive(e.to_string()))?
            .into_owned();
        // Check the path first: a bad path is reported as such, whatever the entry type is.
        let rel = safe_relative(&raw)?;
        let target = dest.join(&rel);
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if !kind.is_file() {
            return Err(UpdateError::UnsafePath(format!(
                "{} is not a plain file or folder",
                raw.display()
            )));
        }
        total = total.saturating_add(entry.size());
        if total > limits.max_total_bytes {
            return Err(UpdateError::Archive("the archive is too large".into()));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mode = entry.header().mode().unwrap_or(0o644);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        std::io::copy(&mut entry, &mut file)?;
        drop(file);
        set_mode(&target, mode)?;
        files += 1;
    }
    Ok(files)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // Programs stay programs. Nothing else gets write access for others, and no set-id bits.
    let safe = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(safe))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    /// One entry for a test archive. `name` is written as it is, even `../x`.
    pub struct Item<'a> {
        pub name: &'a str,
        pub kind: tar::EntryType,
        pub data: &'a [u8],
        pub link: Option<&'a str>,
        pub mode: u32,
    }

    pub fn file<'a>(name: &'a str, data: &'a [u8]) -> Item<'a> {
        Item {
            name,
            kind: tar::EntryType::Regular,
            data,
            link: None,
            mode: 0o644,
        }
    }

    /// Builds a `.tar.gz` by writing the header bytes ourselves, so that bad paths are possible.
    pub fn build(items: &[Item]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        for it in items {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(it.kind);
            h.set_size(it.data.len() as u64);
            h.set_mode(it.mode);
            {
                let old = h.as_old_mut();
                old.name = [0; 100];
                old.name[..it.name.len()].copy_from_slice(it.name.as_bytes());
            }
            if let Some(l) = it.link {
                let old = h.as_old_mut();
                old.linkname[..l.len()].copy_from_slice(l.as_bytes());
            }
            h.set_cksum();
            tar_bytes.extend_from_slice(h.as_bytes());
            tar_bytes.extend_from_slice(it.data);
            let pad = (512 - it.data.len() % 512) % 512;
            tar_bytes.extend(std::iter::repeat_n(0u8, pad));
        }
        tar_bytes.extend(std::iter::repeat_n(0u8, 1024));
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        gz.finish().unwrap()
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-arch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn unpacks_files_and_folders() {
        let d = tmp("ok");
        let a = build(&[
            Item {
                name: "bin/",
                kind: tar::EntryType::Directory,
                data: b"",
                link: None,
                mode: 0o755,
            },
            file("kadakas-app", b"app"),
            Item {
                mode: 0o4755,
                ..file("bin/tool", b"tool")
            },
            file("./docs/readme.txt", b"hi"),
        ]);
        let n = extract_tar_gz(&a[..], &d, Limits::default()).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read(d.join("bin/tool")).unwrap(), b"tool");
        assert_eq!(std::fs::read(d.join("docs/readme.txt")).unwrap(), b"hi");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // The set-uid bit is dropped. The program stays a program.
            let m = std::fs::metadata(d.join("bin/tool"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(m & 0o7777, 0o755);
            let m = std::fs::metadata(d.join("kadakas-app"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(m & 0o7777, 0o644);
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn zip_slip_is_refused() {
        for name in [
            "../evil.txt",
            "a/../../evil.txt",
            "/etc/evil.txt",
            "a\\..\\evil.txt",
            "C:evil.txt",
            "a/b:stream",
        ] {
            let d = tmp("slip");
            let outside = d.parent().unwrap().join("evil.txt");
            let _ = std::fs::remove_file(&outside);
            let a = build(&[file("fine.txt", b"x"), file(name, b"pwned")]);
            let e = extract_tar_gz(&a[..], &d, Limits::default()).unwrap_err();
            assert!(matches!(e, UpdateError::UnsafePath(_)), "{name}: {e}");
            assert!(!outside.exists(), "{name} escaped");
            std::fs::remove_dir_all(&d).unwrap();
        }
    }

    #[test]
    fn links_and_devices_are_refused() {
        for kind in [
            tar::EntryType::Symlink,
            tar::EntryType::Link,
            tar::EntryType::Char,
            tar::EntryType::Block,
            tar::EntryType::Fifo,
        ] {
            let d = tmp("link");
            let a = build(&[Item {
                name: "x",
                kind,
                data: b"",
                link: Some("/etc/passwd"),
                mode: 0o777,
            }]);
            let e = extract_tar_gz(&a[..], &d, Limits::default()).unwrap_err();
            assert!(matches!(e, UpdateError::UnsafePath(_)), "{kind:?}");
            std::fs::remove_dir_all(&d).unwrap();
        }
    }

    #[test]
    fn a_file_cannot_be_written_twice_or_through_a_file() {
        let d = tmp("dup");
        let a = build(&[file("a", b"1"), file("a", b"2")]);
        assert!(extract_tar_gz(&a[..], &d, Limits::default()).is_err());
        assert_eq!(std::fs::read(d.join("a")).unwrap(), b"1");
        let a = build(&[file("b", b"1"), file("b/c", b"2")]);
        assert!(extract_tar_gz(&a[..], &d, Limits::default()).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn limits_apply() {
        let d = tmp("limits");
        let a = build(&[file("a", b"12345"), file("b", b"12345")]);
        let small = Limits {
            max_entries: 1,
            max_total_bytes: 100,
        };
        assert!(matches!(
            extract_tar_gz(&a[..], &d, small),
            Err(UpdateError::Archive(_))
        ));
        let d2 = tmp("limits2");
        let tiny = Limits {
            max_entries: 10,
            max_total_bytes: 8,
        };
        assert!(matches!(
            extract_tar_gz(&a[..], &d2, tiny),
            Err(UpdateError::Archive(_))
        ));
        std::fs::remove_dir_all(&d).unwrap();
        std::fs::remove_dir_all(&d2).unwrap();
    }

    #[test]
    fn broken_input_is_an_error() {
        let d = tmp("broken");
        assert!(extract_tar_gz(&b"not gzip at all"[..], &d, Limits::default()).is_err());
        let mut a = build(&[file("a", b"1")]);
        a.truncate(a.len() / 2);
        assert!(extract_tar_gz(&a[..], &d, Limits::default()).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn global_pax_headers_are_skipped() {
        let d = tmp("pax");
        let a = build(&[
            Item {
                name: "pax_global_header",
                kind: tar::EntryType::XGlobalHeader,
                data: b"",
                link: None,
                mode: 0o644,
            },
            file("a", b"1"),
        ]);
        assert_eq!(extract_tar_gz(&a[..], &d, Limits::default()).unwrap(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn path_rules() {
        assert_eq!(
            safe_relative(Path::new("./a//b/c")).unwrap(),
            PathBuf::from("a/b/c")
        );
        for bad in ["", ".", "..", "a/..", "/a", "a\\b"] {
            assert!(safe_relative(Path::new(bad)).is_err(), "{bad}");
        }
    }

    proptest::proptest! {
        #[test]
        fn a_clean_path_stays_inside(parts in proptest::collection::vec("[a-zA-Z0-9_.\\-/\\\\:]{0,6}", 0..6)) {
            let p = parts.join("/");
            if let Ok(clean) = safe_relative(Path::new(&p)) {
                let joined = Path::new("/base").join(&clean);
                proptest::prop_assert!(joined.starts_with("/base"));
                proptest::prop_assert!(clean.components().all(|c| matches!(c, Component::Normal(_))));
            }
        }
    }
}
