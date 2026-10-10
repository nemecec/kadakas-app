//! Safe file writes: a temporary file in the same folder, a flush to disk, then a rename.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The path for a sibling of `path`: same folder, `prefix` + file name + `suffix`.
pub fn sibling(path: &Path, prefix: &str, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{prefix}{name}{suffix}"))
}

/// Who may read a file that [`write_durable`] makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    /// Mode 0600 on Unix.
    Private,
    /// The mode of the file that is replaced, if there is one.
    KeepExisting,
}

/// Writes `bytes` to a temporary file in the same folder, flushes it, and
/// renames it over `path`. A crash never leaves a half-written file.
/// With `keep_backup`, the previous file is first copied to `<path>.bak`.
/// With `private`, the file is readable by the owner only (Unix).
fn write_replacing(
    path: &Path,
    bytes: &[u8],
    keep_backup: bool,
    private: bool,
) -> std::io::Result<()> {
    // Write to the real target, so a symlink stays a symlink.
    let resolved = std::fs::canonicalize(path).ok();
    let path = resolved.as_deref().unwrap_or(path);
    let old_perms = std::fs::metadata(path).ok().map(|m| m.permissions());
    let tmp = sibling(path, ".", &format!(".{}.tmp", std::process::id()));
    let result = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let _ = private;
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if let Some(p) = &old_perms {
            f.set_permissions(p.clone())?;
        }
        drop(f);
        if keep_backup && path.is_file() {
            std::fs::copy(path, sibling(path, "", ".bak"))?;
        }
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Moves a finished temporary file over `target`, which is the last step of a streamed save.
/// The old `target` is first copied to `<target>.bak`. The temporary file must be in the same folder.
pub fn replace_with_backup(tmp: &Path, target: &Path) -> std::io::Result<()> {
    if target.is_file() {
        std::fs::copy(target, sibling(target, "", ".bak"))?;
    }
    std::fs::rename(tmp, target)
}

/// Saves a file atomically and keeps the previous version as `<path>.bak`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_replacing(path, bytes, true, false)
}

/// Like [`write_atomic`], with no backup and owner-only permissions (Unix).
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_replacing(path, bytes, false, true)
}

/// Creates the folder (and parents). The last folder gets mode 0700 on Unix.
pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
}

/// Flushes a folder, so a rename in it survives a power failure. Not needed on Windows.
pub fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Creates a new file that must not exist yet. On Unix the mode is 0600.
pub fn create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::File::options();
    opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

/// Writes a file atomically: temporary file, flush, rename, flush of the folder.
/// The temporary file is `<path><tmp_suffix>`. It is deleted again if anything fails.
pub fn write_durable(
    path: &Path,
    bytes: &[u8],
    access: Access,
    tmp_suffix: &str,
) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(tmp_suffix);
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut opts = std::fs::File::options();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if access == Access::Private {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        if access == Access::KeepExisting
            && let Ok(m) = std::fs::metadata(path)
        {
            f.set_permissions(m.permissions())?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        if let Some(dir) = path.parent() {
            sync_dir(dir);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadektor-fsio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn atomic_write_replaces_file_keeps_backup_and_leaves_no_temp_file() {
        let dir = temp_dir("atomic");
        let f = dir.join("a.cdk.json");
        write_atomic(&f, b"one").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"one");
        assert!(
            !dir.join("a.cdk.json.bak").exists(),
            "no backup for a new file"
        );
        write_atomic(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert_eq!(std::fs::read(dir.join("a.cdk.json.bak")).unwrap(), b"one");
        write_atomic(&f, b"three").unwrap();
        assert_eq!(std::fs::read(dir.join("a.cdk.json.bak")).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_keeps_permissions_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perm");
        let real = dir.join("real.json");
        std::fs::write(&real, "old").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&real, b"new").unwrap();
        assert_eq!(
            std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, b"via link").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&real).unwrap(), b"via link");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_write_cleans_up_and_keeps_the_old_file() {
        let dir = temp_dir("fail");
        // The target is a folder, so the final rename fails.
        let target = dir.join("t");
        std::fs::create_dir(&target).unwrap();
        assert!(write_atomic(&target, b"x").is_err());
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        // A missing folder is an error too.
        assert!(write_atomic(&dir.join("missing").join("f"), b"x").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn replace_with_backup_keeps_the_old_file() {
        let dir = temp_dir("rwb");
        let target = dir.join("t.ifc");
        std::fs::write(&target, "old").unwrap();
        let tmp = dir.join(".t.tmp");
        std::fs::write(&tmp, "new").unwrap();
        replace_with_backup(&tmp, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(dir.join("t.ifc.bak")).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn durable_write_and_private_files() {
        let dir = temp_dir("durable");
        let f = dir.join("v.json");
        write_durable(&f, b"1", Access::Private, ".tmp").unwrap();
        write_durable(&f, b"2", Access::KeepExisting, ".tmp").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"2");
        assert!(!dir.join("v.json.tmp").exists());
        write_private(&dir.join("p.json"), b"p").unwrap();
        let sub = dir.join("a").join("b");
        create_private_dir(&sub).unwrap();
        let lock = sub.join("l");
        drop(create_new_private(&lock).unwrap());
        assert!(create_new_private(&lock).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir.join("p.json")), 0o600);
            assert_eq!(mode(&sub), 0o700);
            assert_eq!(mode(&lock), 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
