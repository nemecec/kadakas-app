//! The errors of the container. Every error that comes from one entry names the entry.

/// What went wrong with a container. A bad entry is always an error value that names it; the code
/// never panics on the bytes of a file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContainerError {
    /// The bytes are not a container, or the structure is damaged (not one entry).
    #[error("this is not a Kadakas container: {0}")]
    Format(String),
    /// One entry is bad. `name` is the name of the entry.
    #[error("the entry \"{name}\" is not valid: {reason}")]
    Entry { name: String, reason: String },
    /// A bound was reached (entry count, sizes, ratio).
    #[error("the container is over a limit: {0}")]
    Limit(String),
    /// The file is not the file that was opened (size, time or manifest changed).
    #[error("{0}")]
    Changed(String),
    /// A read or a write failed.
    #[error("{0}")]
    Io(String),
}

impl ContainerError {
    pub(crate) fn entry(name: &str, reason: impl Into<String>) -> ContainerError {
        ContainerError::Entry {
            name: name.to_string(),
            reason: reason.into(),
        }
    }

    pub(crate) fn format(reason: impl Into<String>) -> ContainerError {
        ContainerError::Format(reason.into())
    }

    /// The name of the entry that the error is about, if it is about one.
    pub fn entry_name(&self) -> Option<&str> {
        match self {
            ContainerError::Entry { name, .. } => Some(name),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ContainerError {
    fn from(e: std::io::Error) -> Self {
        ContainerError::Io(e.to_string())
    }
}

impl From<ContainerError> for std::io::Error {
    fn from(e: ContainerError) -> Self {
        std::io::Error::other(e)
    }
}
