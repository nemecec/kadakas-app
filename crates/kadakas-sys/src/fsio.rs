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

/// The content of a streamed write: it writes everything into a buffered writer on the
/// temporary file. The caller flushes, syncs and renames.
type Fill<'a> = Box<dyn FnOnce(&mut dyn Write) -> std::io::Result<()> + 'a>;

/// Writes the content that `fill` makes to a temporary file in the same folder, flushes it, and
/// renames it over `path`. A crash never leaves a half-written file, and a failure of `fill`
/// leaves the old file as it was and removes the temporary file.
/// With `keep_backup`, the previous file is first copied to `<path>.bak`.
/// With `private`, the file is readable by the owner only (Unix).
fn write_replacing(
    path: &Path,
    fill: Fill<'_>,
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
        let mut w = std::io::BufWriter::new(opts.open(&tmp)?);
        fill(&mut w)?;
        let f = w.into_inner().map_err(|e| e.into_error())?;
        f.sync_all()?;
        if let Some(p) = &old_perms {
            f.set_permissions(p.clone())?;
        }
        // On wasm `File` has no drop code.
        #[allow(clippy::drop_non_drop)]
        drop(f);
        if keep_backup && path.is_file() {
            std::fs::copy(path, sibling(path, "", ".bak"))?;
        }
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
    write_atomic_with(path, |w| w.write_all(bytes))
}

/// Like [`write_atomic`], for content that `fill` streams into a buffered writer on the
/// temporary file: write, flush, fsync, rename. No copy of the whole content is held. If `fill`
/// fails, the old file stays and the temporary file is deleted.
pub fn write_atomic_with(
    path: &Path,
    fill: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
) -> std::io::Result<()> {
    write_replacing(path, Box::new(fill), true, false)
}

/// Like [`write_atomic`], with no backup and owner-only permissions (Unix).
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_replacing(path, Box::new(|w| w.write_all(bytes)), false, true)
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
    write_durable_with(path, access, tmp_suffix, |w| w.write_all(bytes))
}

/// Like [`write_durable`], for content that `fill` streams into a buffered writer.
pub fn write_durable_with(
    path: &Path,
    access: Access,
    tmp_suffix: &str,
    fill: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
) -> std::io::Result<()> {
    stage_durable(path, access, tmp_suffix, fill)?.commit()
}

/// A finished temporary file that waits for its rename. Dropping it deletes the temporary file.
/// The caller may take a lock between `stage_durable` (the slow part, the write) and
/// [`Staged::commit`] (the rename), so the lock is short.
#[must_use = "a staged file is deleted when it is dropped"]
pub struct Staged {
    tmp: PathBuf,
    target: PathBuf,
    done: bool,
}

impl Staged {
    /// Renames the temporary file over the target and flushes the folder.
    pub fn commit(mut self) -> std::io::Result<()> {
        std::fs::rename(&self.tmp, &self.target)?;
        self.done = true;
        if let Some(dir) = self.target.parent() {
            sync_dir(dir);
        }
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.done {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

/// The first half of [`write_durable_with`]: writes and flushes the temporary file
/// `<path><tmp_suffix>`. It is deleted again if `fill` or the flush fails.
pub fn stage_durable(
    path: &Path,
    access: Access,
    tmp_suffix: &str,
    fill: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
) -> std::io::Result<Staged> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(tmp_suffix);
    let staged = Staged {
        tmp: PathBuf::from(tmp),
        target: path.to_path_buf(),
        done: false,
    };
    let mut opts = std::fs::File::options();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if access == Access::Private {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let f = opts.open(&staged.tmp)?;
    if access == Access::KeepExisting
        && let Ok(m) = std::fs::metadata(path)
    {
        f.set_permissions(m.permissions())?;
    }
    let mut w = std::io::BufWriter::new(f);
    fill(&mut w)?;
    let f = w.into_inner().map_err(|e| e.into_error())?;
    f.sync_all()?;
    // Close the file before the rename (Windows needs it). On wasm `File` has no drop code.
    #[allow(clippy::drop_non_drop)]
    drop(f);
    Ok(staged)
}

/// Why [`open_regular`] refused a file.
#[derive(Debug)]
pub enum OpenError {
    /// The file cannot be opened or read (the kind says if it is missing).
    Io(std::io::Error),
    /// A folder, a pipe, a device: only a regular file is read.
    NotRegular,
    /// The file is larger than the limit.
    TooLarge { len: u64, max: u64 },
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Io(e) => e.fmt(f),
            OpenError::NotRegular => f.write_str("it is not a regular file"),
            OpenError::TooLarge { len, max } => {
                write!(f, "the file is {len} bytes; the limit is {max} bytes")
            }
        }
    }
}

/// A reader that never gives more than `max` bytes of the file. A file that grows after the size
/// check ends with a read error and not with more data.
pub struct Capped<R> {
    inner: R,
    max: u64,
    pos: u64,
}

impl<R: std::io::Read> std::io::Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // One byte more than the limit, so that the end of a file of exactly `max` bytes is a
        // normal end and a longer file is an error.
        let room = self.max.saturating_add(1).saturating_sub(self.pos);
        if room == 0 {
            return Err(std::io::Error::other(format!(
                "the file grew beyond the limit of {} bytes",
                self.max
            )));
        }
        let n = buf.len().min(usize::try_from(room).unwrap_or(usize::MAX));
        let got = self.inner.read(&mut buf[..n])?;
        self.pos += got as u64;
        Ok(got)
    }
}

impl<R: std::io::Seek> std::io::Seek for Capped<R> {
    fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
        self.pos = self.inner.seek(from)?;
        Ok(self.pos)
    }
}

/// Opens a regular file of at most `max` bytes for a streamed read. The kind and the size are
/// checked before anything is read, and the reader is capped as well.
pub fn open_regular(
    path: &Path,
    max: u64,
) -> Result<std::io::BufReader<Capped<std::fs::File>>, OpenError> {
    // A pipe or a device must not be opened at all: opening a FIFO blocks until a writer comes. The
    // kind is read first, and the open itself does not block (Unix) in case the file is swapped
    // between the two steps.
    if !std::fs::metadata(path).map_err(OpenError::Io)?.is_file() {
        return Err(OpenError::NotRegular);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(OpenError::Io)?;
    let meta = file.metadata().map_err(OpenError::Io)?;
    if !meta.is_file() {
        return Err(OpenError::NotRegular);
    }
    if meta.len() > max {
        return Err(OpenError::TooLarge {
            len: meta.len(),
            max,
        });
    }
    Ok(std::io::BufReader::new(Capped {
        inner: file,
        max,
        pos: 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-fsio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn atomic_write_replaces_file_keeps_backup_and_leaves_no_temp_file() {
        let dir = temp_dir("atomic");
        let f = dir.join("a.kad");
        write_atomic(&f, b"one").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"one");
        assert!(!dir.join("a.kad.bak").exists(), "no backup for a new file");
        write_atomic(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert_eq!(std::fs::read(dir.join("a.kad.bak")).unwrap(), b"one");
        write_atomic(&f, b"three").unwrap();
        assert_eq!(std::fs::read(dir.join("a.kad.bak")).unwrap(), b"two");
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
    fn a_streamed_write_flushes_replaces_and_keeps_a_backup() {
        let dir = temp_dir("stream");
        let f = dir.join("s.kad");
        write_atomic_with(&f, |w| {
            // Many small writes, more than one buffer.
            for _ in 0..10_000 {
                w.write_all(b"0123456789")?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().len(), 100_000);
        write_atomic_with(&f, |w| w.write_all(b"two")).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert_eq!(
            std::fs::metadata(dir.join("s.kad.bak")).unwrap().len(),
            100_000
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failing_stream_keeps_the_old_file_and_removes_the_temporary_file() {
        let dir = temp_dir("stream-fail");
        let f = dir.join("s.kad");
        std::fs::write(&f, "old").unwrap();
        let e = write_atomic_with(&f, |w| {
            w.write_all(&[7u8; 50_000])?;
            Err(std::io::Error::other("disk full"))
        })
        .unwrap_err();
        assert!(e.to_string().contains("disk full"));
        assert_eq!(std::fs::read(&f).unwrap(), b"old");
        let e = write_durable_with(&f, Access::KeepExisting, ".tmp", |w| {
            w.write_all(b"half")?;
            Err(std::io::Error::other("disk full"))
        })
        .unwrap_err();
        assert!(e.to_string().contains("disk full"));
        assert_eq!(std::fs::read(&f).unwrap(), b"old");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_regular_checks_kind_size_and_growth() {
        use std::io::{Read, Seek, SeekFrom};
        let dir = temp_dir("open");
        let f = dir.join("a.kad");
        std::fs::write(&f, b"0123456789").unwrap();
        let mut r = open_regular(&f, 10).unwrap();
        let mut s = String::new();
        r.read_to_string(&mut s).unwrap();
        assert_eq!(s, "0123456789");
        r.seek(SeekFrom::Start(2)).unwrap();
        let mut two = [0u8; 2];
        r.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"23");
        let e = open_regular(&f, 9).err().unwrap();
        assert!(matches!(e, OpenError::TooLarge { len: 10, max: 9 }));
        assert!(e.to_string().contains("10 bytes") && e.to_string().contains("9 bytes"));
        assert!(matches!(open_regular(&dir, 10), Err(OpenError::NotRegular)));
        let e = open_regular(&dir.join("missing"), 10).err().unwrap();
        assert!(matches!(&e, OpenError::Io(x) if x.kind() == std::io::ErrorKind::NotFound));
        assert!(!e.to_string().is_empty());
        assert!(!OpenError::NotRegular.to_string().is_empty());
        // A file that grew after the check: the read stops with an error.
        let mut r = open_regular(&f, 10).unwrap();
        std::fs::write(&f, b"0123456789ABCDEF").unwrap();
        let e = r.read_to_end(&mut Vec::new()).unwrap_err();
        assert!(e.to_string().contains("grew"), "{e}");
        // No limit at all (the app asks the person instead): the largest value must not overflow.
        let mut r = open_regular(&f, u64::MAX).unwrap();
        let mut all = Vec::new();
        r.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 16);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_is_refused_without_blocking() {
        let dir = temp_dir("fifo");
        let fifo = dir.join("pipe.kad");
        let name = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: a valid C string and a mode.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        // No writer exists, so a plain open would wait for ever.
        assert!(matches!(
            open_regular(&fifo, 10),
            Err(OpenError::NotRegular)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_staged_file_commits_or_vanishes() {
        let dir = temp_dir("staged");
        let f = dir.join("s.kad");
        std::fs::write(&f, "old").unwrap();
        let staged =
            stage_durable(&f, Access::KeepExisting, ".st", |w| w.write_all(b"new")).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"old");
        assert!(dir.join("s.kad.st").exists());
        drop(staged);
        assert!(!dir.join("s.kad.st").exists(), "a dropped file is deleted");
        assert_eq!(std::fs::read(&f).unwrap(), b"old");
        stage_durable(&f, Access::KeepExisting, ".st", |w| w.write_all(b"new"))
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        // A commit that fails (the target is a folder) deletes the temporary file too.
        let folder = dir.join("d");
        std::fs::create_dir(&folder).unwrap();
        let staged = stage_durable(&folder, Access::Private, ".st", |w| w.write_all(b"x")).unwrap();
        assert!(staged.commit().is_err());
        assert!(!dir.join("d.st").exists());
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
        #[allow(clippy::drop_non_drop)]
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
