//! Unpacking of `.tar.gz` update archives, with path checks.
//!
//! The release workflow makes one `.tar.gz` per system. An archive is not trusted, even after the
//! hash check, so every entry is checked:
//!
//! - Only plain files and folders are written, and on Unix symbolic links that stay inside the
//!   unpacked folder (a macOS `.app` bundle has some). Hard links, devices and pipes are
//!   refused. A link target must be relative and look like `../..` followed by names, and the
//!   `..` parts must not climb out of the folder. Nothing is ever written through a link: an
//!   entry whose path passes a link is refused. So a link can never lead out of the folder.
//! - A path must be relative, with normal names only: no `..`, no root, no drive letter, no
//!   backslash and no `:` (Windows streams). This stops "zip slip" archives.
//! - Names that Windows cannot store are refused on every system, so an archive means the same
//!   everywhere: reserved device names (`CON`, `NUL`, `COM1`, ...), and names that end in a dot or
//!   a space.
//! - Two names that differ only in case are refused (they are one file on Windows and macOS).
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

/// A name that every supported file system can store.
fn portable_name(name: &str) -> bool {
    if name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    // Windows reserves the device name with any extension: `nul.txt` is `nul`.
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let numbered = |p: &str| {
        stem.strip_prefix(p)
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
    };
    !(matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT"))
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
            Component::Normal(n) => {
                if !portable_name(&n.to_string_lossy()) {
                    return Err(unsafe_path());
                }
                clean.push(n)
            }
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
    // The paths of the links written so far.
    let mut links: Vec<PathBuf> = Vec::new();
    // Lower-case path -> the path as written. Finds names that differ only in case.
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
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
        // Every prefix of the path is checked too: `A/x` and `a/y` would share one folder.
        let mut prefix = PathBuf::new();
        for c in rel.components() {
            prefix.push(c);
            let written = prefix.to_string_lossy().into_owned();
            let folded = written.to_lowercase();
            if let Some(first) = seen.get(&folded) {
                if *first != written {
                    return Err(UpdateError::UnsafePath(format!(
                        "{written} and {first} differ only in case"
                    )));
                }
            } else {
                seen.insert(folded, written);
            }
        }
        // Nothing is written through a link, and a link is not written twice.
        if links.iter().any(|l| rel.starts_with(l)) {
            return Err(UpdateError::UnsafePath(format!(
                "{} passes a link",
                raw.display()
            )));
        }
        if kind.is_symlink() {
            let to = entry
                .link_name()
                .map_err(|e| UpdateError::Archive(e.to_string()))?
                .map(std::borrow::Cow::into_owned)
                .unwrap_or_default();
            check_link_target(&rel, &to)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            make_symlink(&to, &target)?;
            links.push(rel);
            continue;
        }
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

/// Checks the target of a link at `rel` (relative to the unpacked folder). The target must be
/// relative: some `..`, then normal names only. The `..` must not climb above the folder.
fn check_link_target(rel: &Path, to: &Path) -> Result<()> {
    let bad = || UpdateError::UnsafePath(format!("link {} to {}", rel.display(), to.display()));
    let text = to.to_string_lossy();
    if text.contains('\\') || text.contains(':') || text.contains('\0') {
        return Err(bad());
    }
    let mut up = 0usize;
    let mut names = 0usize;
    for c in to.components() {
        match c {
            Component::ParentDir if names == 0 => up += 1,
            Component::Normal(n) if portable_name(&n.to_string_lossy()) => names += 1,
            Component::CurDir => {}
            _ => return Err(bad()),
        }
    }
    // The folder of the link is `depth` names below the root.
    let depth = rel.components().count().saturating_sub(1);
    if names == 0 || up > depth {
        return Err(bad());
    }
    Ok(())
}

#[cfg(unix)]
fn make_symlink(to: &Path, at: &Path) -> Result<()> {
    std::os::unix::fs::symlink(to, at)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_symlink(to: &Path, _at: &Path) -> Result<()> {
    Err(UpdateError::UnsafePath(format!(
        "links are not supported here: {}",
        to.display()
    )))
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

    fn link<'a>(name: &'a str, to: &'a str) -> Item<'a> {
        Item {
            name,
            kind: tar::EntryType::Symlink,
            data: b"",
            link: Some(to),
            mode: 0o755,
        }
    }

    /// A macOS bundle: programs keep their bit, a link inside the bundle is made.
    #[cfg(unix)]
    #[test]
    fn a_bundle_keeps_bits_and_links_inside() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("bundle");
        let a = build(&[
            Item {
                mode: 0o755,
                ..file("Kadakas.app/Contents/MacOS/kadakas-app", b"app")
            },
            file("Kadakas.app/Contents/Info.plist", b"plist"),
            link("Kadakas.app/Contents/Helpers", "MacOS"),
            link("Kadakas.app/Contents/MacOS/alias", "kadakas-app"),
            link("Kadakas.app/Current", "Contents/MacOS"),
            link("Kadakas.app/Contents/Frameworks/F", "../../Contents/MacOS"),
        ]);
        assert_eq!(extract_tar_gz(&a[..], &d, Limits::default()).unwrap(), 2);
        let app = d.join("Kadakas.app/Contents");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode(&app.join("MacOS/kadakas-app")), 0o755);
        assert_eq!(mode(&app.join("Info.plist")), 0o644);
        assert_eq!(
            std::fs::read(app.join("Helpers/kadakas-app")).unwrap(),
            b"app"
        );
        assert_eq!(std::fs::read(app.join("MacOS/alias")).unwrap(), b"app");
        assert_eq!(
            std::fs::read_link(d.join("Kadakas.app/Current")).unwrap(),
            Path::new("Contents/MacOS")
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_leaves_the_folder_is_refused() {
        for (name, to) in [
            ("x", "../outside"),
            ("a/x", "../../outside"),
            ("a/x", "/etc"),
            ("x", ".."),
            ("x", "."),
            ("a/x", "b/../../.."),
            ("x", "a:b"),
            ("x", "C:/w"),
            ("x", ""),
        ] {
            let d = tmp("linkout");
            let a = build(&[file("fine", b"1"), link(name, to)]);
            let e = extract_tar_gz(&a[..], &d, Limits::default()).unwrap_err();
            assert!(
                matches!(e, UpdateError::UnsafePath(_)),
                "{name} -> {to}: {e}"
            );
            assert!(
                std::fs::symlink_metadata(d.join(name)).is_err(),
                "{name} -> {to}"
            );
            std::fs::remove_dir_all(&d).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn nothing_is_written_through_a_link() {
        // A folder link to the inside, then a file through it: refused. The folder stays as it was.
        let d = tmp("through");
        std::fs::create_dir_all(d.join("real")).unwrap();
        let a = build(&[link("l", "real"), file("l/evil", b"x")]);
        let e = extract_tar_gz(&a[..], &d, Limits::default()).unwrap_err();
        assert!(matches!(e, UpdateError::UnsafePath(_)), "{e}");
        assert!(!d.join("real/evil").exists());
        // The same link twice is refused too.
        let d2 = tmp("twice");
        let a = build(&[link("l", "a"), link("l", "b")]);
        assert!(extract_tar_gz(&a[..], &d2, Limits::default()).is_err());
        std::fs::remove_dir_all(&d).unwrap();
        std::fs::remove_dir_all(&d2).unwrap();
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
        for bad in [
            "con", "NUL", "nul.txt", "Aux.tar", "COM1", "lpt9.x", "a/PRN", "x.", "x ", "dir /f",
        ] {
            assert!(safe_relative(Path::new(bad)).is_err(), "{bad}");
        }
        for ok in ["com0", "com10", "lpt", "console", "nullable", "a.b", "COM"] {
            assert!(safe_relative(Path::new(ok)).is_ok(), "{ok}");
        }
    }

    #[test]
    fn names_that_differ_only_in_case_are_refused() {
        for names in [["a.txt", "A.txt"], ["Dir/x", "dir/y"]] {
            let d = tmp("case");
            let a = build(&[file(names[0], b"1"), file(names[1], b"2")]);
            let e = extract_tar_gz(&a[..], &d, Limits::default()).unwrap_err();
            assert!(matches!(e, UpdateError::UnsafePath(_)), "{names:?}: {e}");
            std::fs::remove_dir_all(&d).unwrap();
        }
        // The same folder written twice in the same way is fine.
        let d = tmp("case-ok");
        let a = build(&[file("Dir/x", b"1"), file("Dir/y", b"2")]);
        assert_eq!(extract_tar_gz(&a[..], &d, Limits::default()).unwrap(), 2);
        std::fs::remove_dir_all(&d).unwrap();
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
