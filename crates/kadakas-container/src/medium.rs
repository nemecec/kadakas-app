//! Where the bytes of a container are: in memory, or in a file that is opened again for each read.

use std::io::Read;
use std::sync::Arc;

use crate::error::ContainerError;

/// The bytes of a container, read by position. A container keeps no open file: a lazy read opens
/// the medium again, so the file can be replaced by a save at any time (and on Windows the rename
/// of a save is never blocked by a handle of this program).
pub trait Medium: Send + Sync + std::fmt::Debug {
    /// The size of the container in bytes, as it was when the medium was made.
    fn len(&self) -> u64;

    /// True if the container has no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A reader for the bytes `offset..offset + len`. It fails if the bytes are not there, or if
    /// the medium is not the one that was opened (see [`ContainerError::Changed`]).
    fn open(&self, offset: u64, len: u64) -> Result<Box<dyn Read + Send>, ContainerError>;

    /// Checks that the medium is still the one that was opened. A file medium compares the size
    /// and the time of change. The default has nothing to check (memory does not change).
    fn verify(&self) -> Result<(), ContainerError> {
        Ok(())
    }

    /// A short text for messages, such as the path of the file.
    fn label(&self) -> String;
}

/// A container in memory (the bytes of the browser's file picker, or a test).
#[derive(Clone)]
pub struct MemMedium(Arc<[u8]>);

impl std::fmt::Debug for MemMedium {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemMedium({} bytes)", self.0.len())
    }
}

impl MemMedium {
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> MemMedium {
        MemMedium(bytes.into())
    }

    /// The bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

struct ArcSlice {
    data: Arc<[u8]>,
    pos: usize,
    end: usize,
}

impl Read for ArcSlice {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.end - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Medium for MemMedium {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn open(&self, offset: u64, len: u64) -> Result<Box<dyn Read + Send>, ContainerError> {
        let end = offset
            .checked_add(len)
            .filter(|e| *e <= self.0.len() as u64)
            .ok_or_else(|| ContainerError::format("the file is cut short"))?;
        Ok(Box::new(ArcSlice {
            data: self.0.clone(),
            pos: offset as usize,
            end: end as usize,
        }))
    }

    fn label(&self) -> String {
        "the file in memory".into()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use file::FileMedium;

#[cfg(not(target_arch = "wasm32"))]
mod file {
    use super::*;
    use std::io::{BufReader, Seek, SeekFrom};
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    /// A container in a file. Each [`Medium::open`] opens the path again and compares the size and
    /// the time of change of the opened file with those of the first open.
    #[derive(Debug)]
    pub struct FileMedium {
        path: PathBuf,
        len: u64,
        modified: Option<SystemTime>,
    }

    impl FileMedium {
        /// Takes the stamp (size, time of change) from a file that the caller opened and checked
        /// (kind and size, see `kadakas_sys::fsio::open_regular`). `path` is where it is opened
        /// again.
        pub fn new(path: &Path, file: &std::fs::File) -> std::io::Result<FileMedium> {
            let meta = file.metadata()?;
            Ok(FileMedium {
                path: path.to_path_buf(),
                len: meta.len(),
                modified: meta.modified().ok(),
            })
        }

        /// Opens the file at `path`.
        pub fn open_path(path: &Path) -> std::io::Result<FileMedium> {
            // A named pipe or a device would block or never end: only a regular file.
            if !std::fs::metadata(path)?.is_file() {
                return Err(std::io::Error::other(format!(
                    "{} is not a regular file",
                    path.display()
                )));
            }
            let file = std::fs::File::open(path)?;
            FileMedium::new(path, &file)
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        fn changed(&self, why: &str) -> ContainerError {
            ContainerError::Changed(format!(
                "the file {} changed on disk after it was opened ({why}). Open it again; nothing from the changed file is used",
                self.path.display()
            ))
        }

        fn check(&self, file: &std::fs::File) -> Result<(), ContainerError> {
            let meta = file
                .metadata()
                .map_err(|e| ContainerError::Io(format!("{}: {e}", self.path.display())))?;
            if meta.len() != self.len {
                return Err(self.changed(&format!(
                    "the size is {} bytes, it was {}",
                    meta.len(),
                    self.len
                )));
            }
            if meta.modified().ok() != self.modified {
                return Err(self.changed("the time of change is different"));
            }
            Ok(())
        }
    }

    impl Medium for FileMedium {
        fn len(&self) -> u64 {
            self.len
        }

        fn open(&self, offset: u64, len: u64) -> Result<Box<dyn Read + Send>, ContainerError> {
            let mut file = std::fs::File::open(&self.path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    self.changed("the file is gone")
                } else {
                    ContainerError::Io(format!("{}: {e}", self.path.display()))
                }
            })?;
            self.check(&file)?;
            if offset.checked_add(len).is_none_or(|end| end > self.len) {
                return Err(ContainerError::format("the file is cut short"));
            }
            file.seek(SeekFrom::Start(offset))?;
            Ok(Box::new(BufReader::with_capacity(
                64 * 1024,
                file.take(len),
            )))
        }

        fn verify(&self) -> Result<(), ContainerError> {
            let file = std::fs::File::open(&self.path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    self.changed("the file is gone")
                } else {
                    ContainerError::Io(format!("{}: {e}", self.path.display()))
                }
            })?;
            self.check(&file)
        }

        fn label(&self) -> String {
            self.path.display().to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_medium_reads_a_range_and_refuses_beyond_the_end() {
        let m = MemMedium::new(vec![1u8, 2, 3, 4, 5]);
        assert_eq!(m.len(), 5);
        assert!(!m.is_empty());
        assert_eq!(m.bytes(), &[1, 2, 3, 4, 5]);
        let mut out = Vec::new();
        m.open(1, 3).unwrap().read_to_end(&mut out).unwrap();
        assert_eq!(out, [2, 3, 4]);
        assert!(m.open(3, 3).is_err());
        assert!(m.open(u64::MAX, 2).is_err());
        assert!(m.verify().is_ok());
        assert!(format!("{m:?}").contains('5'));
        assert!(!m.label().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn file_medium_notices_a_change() {
        let dir = std::env::temp_dir().join(format!("kc-medium-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.bin");
        std::fs::write(&p, b"hello world").unwrap();
        let m = FileMedium::open_path(&p).unwrap();
        assert_eq!(m.len(), 11);
        assert_eq!(m.path(), p);
        let mut out = Vec::new();
        m.open(6, 5).unwrap().read_to_end(&mut out).unwrap();
        assert_eq!(out, b"world");
        assert!(m.verify().is_ok());
        assert!(m.open(8, 9).is_err());
        // The same size and a new time of change.
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(&p, b"HELLO WORLD").unwrap();
        let e = m.open(0, 1).err().unwrap();
        assert!(matches!(e, ContainerError::Changed(_)), "{e}");
        assert!(m.verify().is_err());
        // A different size.
        std::fs::write(&p, b"short").unwrap();
        let e = m.verify().unwrap_err();
        assert!(e.to_string().contains("size"), "{e}");
        std::fs::remove_file(&p).unwrap();
        assert!(m.verify().unwrap_err().to_string().contains("gone"));
        assert!(m.open(0, 1).is_err());
        assert!(FileMedium::open_path(&p).is_err());
        // A folder is not a file.
        let e = FileMedium::open_path(&dir).unwrap_err().to_string();
        assert!(e.contains("not a regular file"), "{e}");
        let _ = std::fs::remove_dir(&dir);
    }
}
