//! Matches the entities of two revisions of a DXF drawing (IOP-22 to IOP-24).
//!
//! The module is pure: it reads no file and knows no document. [`records`] turns a parsed file
//! into [`Rec`] values, and [`diff`] matches two lists of them. Each entity ends up unchanged,
//! changed (with the list of changed properties), added or removed. The files are untrusted, so
//! all work is bounded.
//!
//! The order of the rules:
//!
//! 1. **Handle.** Entities with the same DXF handle (group code 5), scope and type are a pair.
//!    A handle that two entities of one file share is ignored. A pair stays a pair, whatever the
//!    distance, unless the scope is suspected of renumbered handles (see `by_handle`).
//! 2. **Renumbered handles.** Some programs write new handles when they export a drawing. Then
//!    most handle pairs differ in content, while the content of those entities is found
//!    elsewhere in the new file. The handle pairs are dropped and `renumbered` is set, so the
//!    rest of the rules decide.
//! 3. **Content hash.** Entities with the same hash of all content are a pair (unchanged).
//! 4. **Geometry hash.** Entities with the same type and geometry on the same layer, then on any
//!    layer, are a pair. They differ in a property such as the layer or the color.
//! 5. **Nearest.** A grid finds, for each entity that is left, the entities of the same type and
//!    scope within the tolerance. The nearest pairs win, first on the same layer, then on any
//!    layer. This finds entities that were moved or edited.
//! 6. What is left in the base file is removed, and what is left in the new file is added.
//!
//! Each pair says how it was found ([`Via`]). The result does not depend on the order of the
//! input: ties break on the content of the records, never on the position. The limits are in
//! [`Options`]; an input over a limit gives a result with `truncated` set, never an error.

mod matching;
#[cfg(test)]
mod more;
mod objects;
mod rec;
#[cfg(test)]
mod tests;

use serde::Serialize;

pub use matching::{changes, diff};
pub use objects::{TagIndex, objects_of};
pub use rec::{MAX_POINTS, MAX_STORED_POINTS, MODEL, PAPER, Rec, records};

/// What happened to an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unchanged,
    Changed,
    Added,
    Removed,
}

/// How a pair was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    Handle,
    Hash,
    Proximity,
}

/// A property that differs between the two entities of a changed pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Geometry,
    Layer,
    Color,
    Linetype,
    Lineweight,
    Text,
    /// The content differs, but none of the named properties does (for example a flag).
    Other,
}

/// One line of the result.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    /// Index in the base list. None for an added entity.
    pub base: Option<usize>,
    /// Index in the new list. None for a removed entity.
    pub new: Option<usize>,
    pub status: Status,
    /// How the pair was found. None for an added or removed entity.
    pub via: Option<Via>,
    /// The changed properties. Empty unless the status is `changed`.
    pub changes: Vec<Change>,
    /// The distance of a pair that the nearest match found, in drawing units.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
}

/// How many entries of each kind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub unchanged: usize,
    pub changed: usize,
    pub added: usize,
    pub removed: usize,
    pub by_handle: usize,
    pub by_hash: usize,
    pub by_proximity: usize,
}

/// The result of [`diff`]. The entries are in canonical order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diff {
    pub entries: Vec<Entry>,
    pub counts: Counts,
    /// A limit was reached: some entities were left out, or the nearest match stopped early.
    pub truncated: bool,
    /// The handles of the new file do not belong to the entities of the base file.
    pub renumbered: bool,
    /// The tolerance of the nearest match that was used (0 if it did not run).
    pub tolerance: f64,
    /// The work of the nearest match, in the units of the budget. It is never above the budget.
    pub work: u64,
}

/// The limits and settings of [`diff`].
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The largest distance (drawing units) for the nearest match. 0 means a part of the size of
    /// the drawing (0.5 percent of the diagonal of the box around the centers).
    pub tolerance: f64,
    /// The largest tolerance of the nearest match, in drawing units. 0 means no limit. The default
    /// is 1000, which is one meter for a drawing in millimeters. `Options::for_unit` gives one
    /// meter for any unit.
    pub max_tolerance: f64,
    /// Most entities of each side. More are left out, in the order of their handles and hashes.
    pub max_entities: usize,
    /// Most work units of the nearest match: one for each candidate and one for each point pair
    /// that it looks at. The nearest match stops before it goes over. The handle and hash rules
    /// are linear in `max_entities` and always run, so they are not in the budget.
    pub work_budget: u64,
    /// False: use only the handle and hash rules.
    pub proximity: bool,
}

impl Options {
    /// The default options for a drawing whose unit is `mm_per_unit` millimeters: the tolerance is
    /// at most one meter.
    pub fn for_unit(mm_per_unit: f64) -> Options {
        Options {
            max_tolerance: if mm_per_unit.is_finite() && mm_per_unit > 0.0 {
                1000.0 / mm_per_unit
            } else {
                1000.0
            },
            ..Options::default()
        }
    }
}

impl Options {
    /// The options for a parsed file: the tolerance is at most one meter in the unit of the file
    /// (`$INSUNITS`, else millimeters).
    pub fn for_dxf(dxf: &crate::dxf::parse::Dxf) -> Options {
        let unit = dxf
            .header_value("$INSUNITS")
            .and_then(crate::dxf::parse::parse_int)
            .and_then(crate::options::Unit::from_insunits);
        Options::for_unit(unit.map_or(1.0, crate::options::Unit::mm_per_unit))
    }
}

/// **A trap:** the default `max_tolerance` is 1000 drawing units. That is one meter only for a
/// drawing in millimeters; for a drawing in meters it is a kilometer. Use [`Options::for_unit`] or
/// [`Options::for_dxf`] unless the unit is known to be the millimeter.
impl Default for Options {
    fn default() -> Self {
        Options {
            tolerance: 0.0,
            max_tolerance: 1000.0,
            max_entities: 1_000_000,
            work_budget: 50_000_000,
            proximity: true,
        }
    }
}
