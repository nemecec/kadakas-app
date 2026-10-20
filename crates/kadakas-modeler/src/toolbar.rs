//! The tool bar: the Getting Started set, in one row under the menu. The tools that do not run yet
//! are shown, disabled, with a tooltip.

use kadakas_ui_core::keymap::Keymap;
use kadakas_ui_core::modeler::{ModelerTool, ToolState};

use crate::icons;

/// The side of a tool button, in points.
pub const BUTTON: f32 = 34.0;
/// The side of an icon, in points.
pub const ICON: f32 = 24.0;

/// Draws the tool bar. Returns the tool that was clicked, if any (a disabled tool cannot be).
pub fn show(ui: &mut egui::Ui, tools: &ToolState, keys: &Keymap) -> Option<ModelerTool> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        for (i, group) in ModelerTool::GROUPS.iter().enumerate() {
            if i > 0 {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
            }
            for tool in group.iter().copied() {
                if button(ui, tool, tools.active() == tool, keys).clicked() {
                    clicked = Some(tool);
                }
            }
        }
    });
    clicked
}

/// One button. The active tool is marked; a tool that does not run yet is disabled.
fn button(ui: &mut egui::Ui, tool: ModelerTool, active: bool, keys: &Keymap) -> egui::Response {
    let hint = keys.hint(tool.action());
    let tip = tool.tooltip(&hint);
    let size = egui::Vec2::splat(BUTTON);
    let widget = match icons::image(ui, icons::tool_icon(tool), ICON) {
        Some(img) => egui::Button::image(img).image_tint_follows_text_color(true),
        None => egui::Button::new(tool.name()),
    }
    .min_size(size)
    .selected(active);
    let response = ui.add_enabled(tool.works(), widget);
    if tool.works() {
        response.on_hover_text(tip)
    } else {
        response.on_disabled_hover_text(tip)
    }
}
