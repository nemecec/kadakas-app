//! BCF import and export (REV-06): `.bcfzip` files of BCF 3.0 and 2.1, the open issue format of
//! buildingSMART International.
//!
//! A BCF file is a ZIP archive: `bcf.version`, and for each issue a folder named by its GUID with
//! `markup.bcf` (title, status, comments, the list of viewpoints), one `.bcfv` file for each
//! viewpoint (camera and components) and the snapshots. The code is pure: it works on bytes and
//! has no clock and no file access.
//!
//! # What goes in and out
//!
//! | Kadakas | BCF |
//! |---|---|
//! | issue ID, viewpoint ID, comment ID | the GUIDs of the topic, the viewpoint, the comment |
//! | title, description, type, status, priority, assignee, labels, due date | the same topic fields |
//! | author, times | `CreationAuthor`, `CreationDate`, `ModifiedDate`, comment `Author` and `Date` |
//! | the camera of a viewpoint | `OrthogonalCamera` or `PerspectiveCamera`, in meters |
//! | selected objects | `Selection/Component` with `IfcGuid` (the GlobalId of the IFC export) |
//! | snapshot picture | the PNG or JPEG file next to the viewpoint |
//!
//! Not in BCF, so not in the file: the layer and class visibility of a viewpoint, the origin of
//! an issue and the pin point. A topic has no component list of its own in BCF, so the objects of
//! an issue go into the selection of its viewpoints (an issue with objects and no viewpoint gets
//! one made for the file). Not read from BCF: clipping planes, lines, bitmaps, colors, the
//! visibility of components, documents, related topics, BIM snippets. The import counts them in
//! its warnings.
//!
//! # Reading untrusted archives
//!
//! [`read::read`] checks everything before it trusts it: the number of entries, the declared and
//! the real sizes, the compression ratio, the length and form of the names (no `..`, no absolute
//! path, no drive letter), duplicate names, encrypted entries, the nodes of the XML (a DTD is
//! refused, so no entity can grow), the number of topics, comments and viewpoints, and the size
//! of the pictures. A violation is an error, and nothing is imported.

use kadakas_core::issue::{Comment, Issue};

pub mod camera;
pub mod read;
pub mod write;
mod xml;

#[cfg(test)]
mod tests;

/// A BCF version that we read and write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    V21,
    V30,
}

impl Version {
    /// `2.1` or `3.0`.
    pub fn id(self) -> &'static str {
        match self {
            Version::V21 => "2.1",
            Version::V30 => "3.0",
        }
    }

    /// Reads `2.1` or `3.0`; `2.0` is read as `2.1` (the files are the same for what we use).
    pub fn parse(text: &str) -> Option<Version> {
        match text.trim() {
            "2.1" | "2.0" => Some(Version::V21),
            "3.0" => Some(Version::V30),
            _ => None,
        }
    }
}

/// Why a BCF file was not read or written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BcfError {
    /// The bytes are not a ZIP archive, or an entry is damaged.
    #[error("not a BCF archive: {0}")]
    Archive(String),
    /// The archive breaks a limit of the reader.
    #[error("the BCF archive is over a limit: {0}")]
    Limit(String),
    /// The archive is a ZIP file, but it is not a BCF file or it holds an XML file that is wrong.
    #[error("not a valid BCF file: {0}")]
    Format(String),
    /// A version that we do not read.
    #[error("BCF version {0} is not supported (2.1 and 3.0 are)")]
    Version(String),
}

/// A picture file of a snapshot: PNG or JPEG bytes, named by their SHA-256.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// SHA-256 of the file, 64 lowercase hex digits (the hash that the viewpoint holds).
    pub hash: String,
    pub bytes: Vec<u8>,
}

/// One issue of a BCF file, ready to become commands.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedTopic {
    /// The issue with the GUID of the topic as its ID. Viewpoints and comments whose GUID is
    /// missing or not valid have the nil ID: a command makes a new one.
    pub issue: Issue,
    pub comments: Vec<Comment>,
    /// The snapshot files of the viewpoints.
    pub pictures: Vec<Picture>,
}

/// A topic that was not imported, and why. Nothing is skipped without a line here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// The GUID or the folder of the topic.
    pub topic: String,
    pub reason: String,
}

/// What a BCF file holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub version: Version,
    pub topics: Vec<ImportedTopic>,
    pub skipped: Vec<Skipped>,
    /// Things that were not read or were changed on the way in, in plain text.
    pub warnings: Vec<String>,
}

/// What an export wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportReport {
    pub topics: usize,
    pub viewpoints: usize,
    pub snapshots: usize,
    pub comments: usize,
    /// Things that BCF cannot hold, or that were changed on the way out.
    pub notes: Vec<String>,
}
