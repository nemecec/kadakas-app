//! The Data bar in the app. The fields, the parsing and the edit state are in
//! `kadakas_ui_core::databar`. This module adds the egui focus handling.

pub use kadakas_ui_core::databar::*;

pub fn field_id(i: usize) -> egui::Id {
    egui::Id::new(("databar_field", i))
}

/// Leaves the entry mode and takes keyboard focus away from the fields.
pub fn release(entry: &mut DataEntry, ctx: &egui::Context) {
    entry.deactivate();
    ctx.memory_mut(|m| (0..4).for_each(|i| m.surrender_focus(field_id(i))));
}
