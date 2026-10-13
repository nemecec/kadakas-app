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
pub mod collab;
pub mod databar;
pub mod effects;
pub mod feedback;
pub mod grading;
pub mod hit;
pub mod keymap;
pub mod keys;
pub mod massing;
pub mod plugin_form;
pub mod plugin_tool;
pub mod polygon3d;
pub mod profile;
pub mod snap;
pub mod style_edit;
pub mod tools;
pub mod view;
pub mod view_history;

/// Helpers for the tests: a small stand-in for an API session (an editor, a selection and the
/// group you are in), so the tests need no API crate.
#[cfg(test)]
pub(crate) mod testkit {
    use kadakas_core::command::Editor;
    use kadakas_core::model::ObjectId;

    #[derive(Default)]
    pub struct Session {
        pub editor: Editor,
        pub selection: Vec<ObjectId>,
        pub context: Vec<ObjectId>,
    }

    impl Session {
        pub fn edit_context(&self) -> Option<ObjectId> {
            self.context.last().copied()
        }
    }
}
