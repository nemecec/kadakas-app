//! Why a mesh file is refused.

/// A mesh file that cannot be read. Every message names the counts, so a person or an agent can
/// decide what to do.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum MeshError {
    #[error("the file is {bytes} bytes, more than the limit of {limit} bytes")]
    FileTooLarge { bytes: u64, limit: u64 },
    #[error("the file has {count} triangles, more than the limit of {limit}")]
    TooManyTriangles { count: u64, limit: u64 },
    #[error("the file has {count} vertices, more than the limit of {limit}")]
    TooManyVertices { count: u64, limit: u64 },
    #[error("the file would need about {bytes} bytes of memory, more than the limit of {limit}")]
    TooMuchMemory { bytes: u64, limit: u64 },
    #[error("the file has {count} parts, more than the limit of {limit}")]
    TooManyParts { count: u64, limit: u64 },
    #[error("{what} ends too soon: found {have}, needed {need}")]
    Truncated { what: String, have: u64, need: u64 },
    #[error("{0}")]
    Malformed(String),
    #[error("{what} is not a finite number")]
    NotFinite { what: String },
    #[error("{what} is out of range: {value} (the limit is {limit} mm)")]
    OutOfRange {
        what: String,
        value: f64,
        limit: f64,
    },
    #[error("{what}: index {index} is not valid, there are {count} vertices")]
    BadIndex {
        what: String,
        index: i64,
        count: u64,
    },
    #[error("{0}")]
    Unsupported(String),
    #[error("the file has no triangles")]
    Empty,
}

impl MeshError {
    /// A stable short name of the kind, for the `reason` of an API error.
    pub fn kind(&self) -> &'static str {
        match self {
            MeshError::FileTooLarge { .. } => "mesh_file_too_large",
            MeshError::TooManyTriangles { .. } => "mesh_too_many_triangles",
            MeshError::TooManyVertices { .. } => "mesh_too_many_vertices",
            MeshError::TooMuchMemory { .. } => "mesh_too_much_memory",
            MeshError::TooManyParts { .. } => "mesh_too_many_parts",
            MeshError::Truncated { .. } => "mesh_truncated",
            MeshError::Malformed(_) => "mesh_malformed",
            MeshError::NotFinite { .. } => "mesh_not_finite",
            MeshError::OutOfRange { .. } => "mesh_out_of_range",
            MeshError::BadIndex { .. } => "mesh_bad_index",
            MeshError::Unsupported(_) => "mesh_unsupported",
            MeshError::Empty => "mesh_empty",
        }
    }

    /// True for the refusals of a size limit (the file may be fine, but too big for this session).
    pub fn is_limit(&self) -> bool {
        matches!(
            self,
            MeshError::FileTooLarge { .. }
                | MeshError::TooManyTriangles { .. }
                | MeshError::TooManyVertices { .. }
                | MeshError::TooMuchMemory { .. }
                | MeshError::TooManyParts { .. }
        )
    }
}

impl MeshError {
    /// The counts of the error as JSON, for the data of an API error.
    pub fn details(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            MeshError::FileTooLarge { bytes, limit } => json!({"bytes": bytes, "limit": limit}),
            MeshError::TooManyTriangles { count, limit }
            | MeshError::TooManyVertices { count, limit }
            | MeshError::TooManyParts { count, limit } => json!({"count": count, "limit": limit}),
            MeshError::TooMuchMemory { bytes, limit } => json!({"bytes": bytes, "limit": limit}),
            MeshError::Truncated { have, need, .. } => json!({"have": have, "need": need}),
            MeshError::BadIndex { index, count, .. } => json!({"index": index, "count": count}),
            MeshError::OutOfRange { value, limit, .. } => json!({"value": value, "limit": limit}),
            _ => json!({}),
        }
    }
}
