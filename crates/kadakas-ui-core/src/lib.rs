//! What every Kadakas UI program needs, without egui (PLAT-03).
//!
//! The crate is pure: it has no window, no GPU and no file access, and it builds for
//! `wasm32-unknown-unknown`. A UI program turns its own input (pointer, keys, modifiers) into the
//! events of this crate, and draws the previews that the crate gives back. Another UI flavor
//! (PLAT-11) can reuse all of it.
//!
//! - [`hit`] and [`annot_pick`]: picking and box selection.
//! - [`snap`]: the geometry that the pointer snaps to, and the snap result.

pub mod annot_pick;
pub mod area;
pub mod grading;
pub mod hit;
pub mod keymap;
pub mod keys;
pub mod massing;
pub mod snap;
pub mod tools;
