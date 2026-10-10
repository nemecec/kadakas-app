//! Plants: plant styles, plant objects, placement, tags, existing trees and the schedule.
//!
//! The plant style is a resource (see `crate::resource`). A plant object refers to a style
//! and can override single fields (see `link`). Everything that does not need the document
//! model is a pure module with its own tests.

pub mod care;
pub mod catalog;
pub mod form3d;
pub mod graphic;
pub mod link;
pub mod placement;
pub mod schedule;
pub mod style;
pub mod tree;
