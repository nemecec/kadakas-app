//! The errors of reading and writing a file.

use kadakas_container::ContainerError;
use kadakas_core::model::LoadError;

/// Why a file could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// The container is damaged, over a limit, or changed on disk. The error names the entry.
    #[error("{0}")]
    Container(ContainerError),
    /// The document does not load (not JSON, a newer format, a broken rule).
    #[error("{0}")]
    Load(LoadError),
    /// A file could not be read or written.
    #[error("{0}")]
    Io(String),
    /// The file is neither a container nor JSON.
    #[error("not a Kadakas file: it is neither a container (a ZIP file) nor JSON text. {0}")]
    Unknown(String),
    /// The path does not exist.
    #[error("{0}")]
    NotFound(String),
    /// The path is not a regular file.
    #[error("{0}")]
    NotRegular(String),
    /// The file has more bytes than the limit of the caller.
    #[error("the file is {len} bytes; the limit is {max} bytes")]
    TooLarge { len: u64, max: u64 },
    /// A part of the document is too big for the form (the format limits of the container).
    #[error("{0}")]
    Limit(String),
}

impl From<ContainerError> for FileError {
    fn from(e: ContainerError) -> Self {
        FileError::Container(e)
    }
}

impl From<LoadError> for FileError {
    fn from(e: LoadError) -> Self {
        FileError::Load(e)
    }
}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        // An error of the container inside an I/O error is the container error.
        match e.get_ref().and_then(|i| i.downcast_ref::<ContainerError>()) {
            Some(c) => FileError::Container(c.clone()),
            None => FileError::Io(e.to_string()),
        }
    }
}

impl FileError {
    /// True if the error means that the file changed on disk after it was opened. A picture that
    /// is read while the document is written reports it as text, so the text counts too.
    pub fn is_changed(&self) -> bool {
        match self {
            FileError::Container(ContainerError::Changed(_)) => true,
            FileError::Io(m) => m.contains("changed on disk after it was opened"),
            _ => false,
        }
    }
}
