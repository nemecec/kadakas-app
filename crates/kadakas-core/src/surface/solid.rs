//! Solid tools (MOD-09): the closed-solid check and the boolean operations of two closed solids.
//!
//! The wording follows the public help of SketchUp (Solid Tools), not verified against the product.
//! Nothing here was taken from studying the product.
//!
//! # What a solid is
//!
//! A *solid* is a set of faces of one surface model that closes a volume: every edge of the set is
//! used by exactly two faces of the set, in opposite directions, and the front of every face looks
//! out of the volume. Nothing else is needed. A model may hold many solids. Two solids may share
//! faces (two boxes drawn through each other in one plane share the faces where the planes meet),
//! and they may touch with faces back to back. A line that is no part of a face, a face with a free
//! edge and an edge with three faces make the set open: [`SolidReport`] names these *open edges*.
//!
//! The faces of a solid are found by a walk: from a face over each of its edges to the face that
//! lies next to it in the material. At an edge with exactly two faces this is the other face. At an
//! edge with more faces (a fin, or a second solid that shares the edge) it is the face that comes
//! first when the first face turns toward its back, as in a radial-edge kernel (see [`shell`]).
//!
//! # The operations
//!
//! [`SolidOp`] names five operations on two solids `first` and `second` (the order matters for
//! Subtract and Trim). Each one is one edit ([`crate::surface::Edit::Solid`]) and so one undo step.
//!
//! - **Union**: one solid that holds both. Faces inside the other solid go.
//! - **Subtract**: `first` less `second`. `second` is used up: it is not in the model afterwards.
//! - **Intersect**: only the volume that both hold. Both solids are used up.
//! - **Trim**: like Subtract, and `second` stays in the model as it was.
//! - **Split**: three solids in place: `first` less `second`, the common part and `second` less
//!   `first`. They touch with faces back to back and share the vertices of the cut.
//!
//! The faces are cut with Intersect Faces (module `edit::intersect`) in a copy that holds only the
//! two solids. Each piece is then looked at by its own: a point of it is inside, outside or on the
//! surface of the other solid (a ray cast against its triangles, see [`locate`]), and the piece
//! stays, goes or turns over by the rules of the operation (see [`boolean`]). A piece on the
//! surface of the other solid is told apart by its direction: the same way (the solids lie on one
//! side of it) or the opposite way (they lie on the two sides). Pieces with the same plane and
//! direction are merged back into one face when they have the same paint.
//!
//! # Refusals
//!
//! An operation changes nothing and says why ([`SolidError`]) when an input is not a closed solid,
//! when a solid looks inward or has no volume, when it passes through itself, when it is stuck to
//! other geometry of the model (make a group first), when the cut leaves a crossing that is too
//! small to be cut, when the result would be empty or is not a closed solid, and when a limit of the
//! model would be passed. It is checked that the result is a valid model and has no problem that the
//! model did not have before.
//!
//! # Tolerances
//!
//! The same as in the module `surface`: points closer than the length tolerance are one point, and
//! a point is on a plane within the plane tolerance.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{EdgeId, FaceId, SurfaceModel};

mod boolean;
mod locate;
mod shell;

pub use shell::{OpenEdge, OpenKind, Shell, ShellState, SolidReport};

pub(super) use boolean::solid_edit;

/// The operation of the solid tools on two solids (`first`, `second`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SolidOp {
    /// One solid that holds both.
    Union,
    /// `first` less `second`; `second` is used up.
    Subtract,
    /// The volume that both hold; both are used up.
    Intersect,
    /// `first` less `second`; `second` stays as it was.
    Trim,
    /// Three solids: `first` less `second`, the common part, and `second` less `first`.
    Split,
}

impl SolidOp {
    /// All operations, in the order of the menu.
    pub const ALL: [SolidOp; 5] = [
        SolidOp::Union,
        SolidOp::Subtract,
        SolidOp::Intersect,
        SolidOp::Trim,
        SolidOp::Split,
    ];

    /// The name of the operation in the commands (`union`, `subtract` and so on).
    pub fn name(self) -> &'static str {
        match self {
            SolidOp::Union => "union",
            SolidOp::Subtract => "subtract",
            SolidOp::Intersect => "intersect",
            SolidOp::Trim => "trim",
            SolidOp::Split => "split",
        }
    }
}

impl std::fmt::Display for SolidOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Which of the two inputs of an operation a refusal is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operand {
    First,
    Second,
}

impl std::fmt::Display for Operand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Operand::First => "first",
            Operand::Second => "second",
        })
    }
}

/// Why a solid operation was refused. The model is unchanged after it. The texts are English, for
/// agents and logs. The window shows the same reasons in the language of the UI (it looks for the
/// phrases that are marked below).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SolidError {
    /// "has no faces"
    #[error("the {0} solid has no faces")]
    Empty(Operand),
    /// "is not a closed solid"
    #[error(
        "the {which} solid is not a closed solid: {count} edges are open (edge {first}: {kind})"
    )]
    Open {
        which: Operand,
        count: usize,
        first: EdgeId,
        kind: OpenKind,
    },
    /// "passes through itself"
    #[error("the {which} solid passes through itself (edge {edge} and face {face})")]
    SelfCrossing {
        which: Operand,
        edge: EdgeId,
        face: FaceId,
    },
    /// "looks inward"
    #[error("the {0} solid looks inward: its faces need Reverse Faces first")]
    Inverted(Operand),
    /// "has no volume"
    #[error("the {0} solid has no volume")]
    Flat(Operand),
    /// "is stuck to other geometry"
    #[error(
        "the {0} solid is stuck to other geometry of the model (shares an edge or a vertex): make it a group first"
    )]
    Attached(Operand),
    /// "would be empty"
    #[error("the result of {0} would be empty: the solids do not have a common volume")]
    NoResult(SolidOp),
    /// "could not be cut"
    #[error("the solids could not be cut cleanly: {0}")]
    Cut(String),
    /// "is not a closed solid" with "the result"
    #[error("the result of the operation is not a closed solid: {0}")]
    NotClosed(String),
}

impl SurfaceModel {
    /// The closed-solid check (MOD-09): the shells of the model, whether each closes a volume, the
    /// edges that make a shell open, and the volume of each. See [`SolidReport`]. It takes
    /// `O(n log n)` time (the full check of the model runs once).
    pub fn solid_report(&self) -> SolidReport {
        shell::report(self)
    }

    /// The faces of the solid that holds `face`: found by the walk that the module documentation
    /// describes, with the faces of the cavities inside it. An open set comes back as it is (look
    /// at [`SurfaceModel::solid_state`]). `None` for a face that does not exist.
    pub fn solid_of(&self, face: FaceId) -> Option<Vec<FaceId>> {
        shell::solid_of(self, face)
    }

    /// Whether the faces close a solid, and its volume in mm³ (positive when the fronts look
    /// out). The faces must exist.
    pub fn solid_state(&self, faces: &[FaceId]) -> Option<Shell> {
        let set: BTreeSet<FaceId> = faces.iter().copied().collect();
        if set.is_empty() || set.iter().any(|f| !self.faces.contains_key(f)) {
            return None;
        }
        Some(shell::state_of(self, &set))
    }

    /// The volume of the faces in mm³ (see [`SurfaceModel::solid_state`]), or `None` for a face
    /// that does not exist.
    pub fn solid_volume(&self, faces: &[FaceId]) -> Option<f64> {
        self.solid_state(faces).map(|s| s.volume)
    }

    /// What an operation would give, without changing the model: the volumes of the solids it
    /// makes (in mm³, one for most operations, three for Split in the order `first` less
    /// `second`, common, `second` less `first`; the one that stays for Trim comes last), for the
    /// status line. The error is the refusal that the edit would give.
    pub fn solid_preview(
        &self,
        op: SolidOp,
        first: &[FaceId],
        second: &[FaceId],
    ) -> Result<Vec<f64>, super::SurfaceError> {
        let mut work = self.clone();
        let mut notes = Vec::new();
        boolean::run(&mut work, op, first, second, &mut notes).map(|o| o.volumes)
    }
}
