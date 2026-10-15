//! The names of the entries of a container, and how the form of a file is told.

use kadakas_container::has_zip_magic;

use crate::error::FileError;

/// The drawing: the JSON of `Document` without the history, pictures as references.
pub const DOCUMENT: &str = "document.json";
/// The folder of the blobs: the pictures and the other big texts, once each, by hash.
pub const BLOB_DIR: &str = "blobs/";
/// The history of versions (REV-01) is stored in parts: the value without the lists...
pub const HISTORY_TOP: &str = "history/top.json";
/// ...the versions...
pub const HISTORY_VERSIONS: &str = "history/versions.json";
/// ...the operations of the newest change...
pub const HISTORY_NEWEST: &str = "history/newest.json";
/// ...and the list of the hashes of its pictures (the pictures are entries of `blobs/`).
pub const HISTORY_BLOBS: &str = "history/blobs.json";
/// The drawing that the history of the file is written against, when it is not the drawing of the
/// file: a raw copy of the `document.json` of the file the versions were saved with. A save of an
/// edited document whose history was not read writes it, so that the versions can be read against
/// the drawing they belong to (REV-01). Its pictures are entries of `blobs/`, listed in the
/// next entry.
pub const HISTORY_ANCHOR: &str = "history/anchor.json";
/// The hashes of the pictures that [`HISTORY_ANCHOR`] refers to.
pub const HISTORY_ANCHOR_BLOBS: &str = "history/anchor-blobs.json";
/// A history that does not have the usual shape is stored as one JSON value, pictures inline.
pub const HISTORY_RAW: &str = "history/raw.json";

/// The entry of the blob with this hash.
pub fn blob_name(hash: &str) -> String {
    format!("{BLOB_DIR}{hash}")
}

/// The two forms of a file (DOC-22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// A ZIP container (DOC-20).
    Container,
    /// One readable JSON file, pictures as base64 text.
    Json,
}

impl Form {
    /// The name in the API: `container` or `json`.
    pub fn name(self) -> &'static str {
        match self {
            Form::Container => "container",
            Form::Json => "json",
        }
    }

    /// Reads the name of a form.
    pub fn from_name(name: &str) -> Option<Form> {
        match name.to_ascii_lowercase().as_str() {
            "container" | "zip" | "kad" => Some(Form::Container),
            "json" => Some(Form::Json),
            _ => None,
        }
    }

    /// The form that a file name asks for: `.json` is the plain form, anything else (`.kad`) the
    /// container.
    pub fn from_extension(path: &std::path::Path) -> Form {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("json") => Form::Json,
            _ => Form::Container,
        }
    }
}

/// Tells the form of a file by its first bytes: the signature of a ZIP file, or the `{` of JSON
/// (after a byte order mark and white space).
pub fn sniff(head: &[u8]) -> Result<Form, FileError> {
    if has_zip_magic(head) {
        return Ok(Form::Container);
    }
    let text = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    match text.iter().find(|b| !b.is_ascii_whitespace()) {
        Some(b'{') => Ok(Form::Json),
        None => Err(FileError::Unknown("The file is empty.".into())),
        Some(_) => Err(FileError::Unknown(
            "It does not start with the ZIP signature or with {.".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_is_told_by_the_first_bytes() {
        assert_eq!(sniff(b"PK\x03\x04rest").unwrap(), Form::Container);
        assert_eq!(sniff(b"{\"a\":1}").unwrap(), Form::Json);
        assert_eq!(sniff(b"  \n\t{").unwrap(), Form::Json);
        assert_eq!(sniff(b"\xEF\xBB\xBF {").unwrap(), Form::Json);
        for bad in [&b""[..], b"   ", b"[1]", b"PK\x05\x06", b"\x89PNG"] {
            assert!(matches!(sniff(bad), Err(FileError::Unknown(_))), "{bad:?}");
        }
    }

    #[test]
    fn form_names_and_extensions() {
        assert_eq!(Form::from_name("JSON"), Some(Form::Json));
        assert_eq!(Form::from_name("container"), Some(Form::Container));
        assert_eq!(Form::from_name("zip"), Some(Form::Container));
        assert_eq!(Form::from_name("xml"), None);
        assert_eq!(Form::Json.name(), "json");
        assert_eq!(Form::Container.name(), "container");
        let p = std::path::Path::new;
        assert_eq!(Form::from_extension(p("a.json")), Form::Json);
        assert_eq!(Form::from_extension(p("a.JSON")), Form::Json);
        assert_eq!(Form::from_extension(p("a.kad")), Form::Container);
        assert_eq!(Form::from_extension(p("a")), Form::Container);
        assert_eq!(blob_name("ab"), "blobs/ab");
    }
}
