//! The menu bar: File, Edit, Camera, Tools, View, Help. The Camera menu has the navigation of
//! MOD-02: Previous and Next view, Standard Views, Perspective and Parallel Projection, Field of
//! View, Zoom Extents and Zoom Window. Items give a `Cmd`; the program runs them after the frame.

use kadakas_core::surface::{IntersectScope, SolidOp};
use kadakas_l10n::tr;
use kadakas_render::StandardView;
use kadakas_ui_core::keymap::Action;
use kadakas_ui_core::modeler::ModelerTool;
use kadakas_ui_core::nav3d::{MAX_FOV_DEG, MIN_FOV_DEG, NavAction};

use crate::app::{Cmd, MENU_VIEWS, ModelerApp};
use crate::prefs::ThemeChoice;

/// The name of a standard view in the Camera menu.
pub fn view_name(view: StandardView) -> String {
    match view {
        StandardView::Top | StandardView::Plan => tr!("modeler-view-top"),
        StandardView::Bottom => tr!("modeler-view-bottom"),
        StandardView::Front => tr!("modeler-view-front"),
        StandardView::Back => tr!("modeler-view-back"),
        StandardView::Left => tr!("modeler-view-left"),
        StandardView::Right => tr!("modeler-view-right"),
        _ => tr!("modeler-view-iso"),
    }
}

/// The action whose key the menu shows for a standard view.
fn view_action(view: StandardView) -> Action {
    match view {
        StandardView::Top | StandardView::Plan => Action::ViewTop,
        StandardView::Bottom => Action::ViewBottom,
        StandardView::Front => Action::ViewFront,
        StandardView::Back => Action::ViewBack,
        StandardView::Left => Action::ViewLeft,
        StandardView::Right => Action::ViewRight,
        _ => Action::ViewRightIso,
    }
}

/// One menu item. A click closes the menu and queues the command.
fn entry(
    ui: &mut egui::Ui,
    cmds: &mut Vec<Cmd>,
    label: String,
    hint: String,
    enabled: bool,
    cmd: Cmd,
) {
    let mut button = egui::Button::new(label);
    if !hint.is_empty() {
        button = button.shortcut_text(hint);
    }
    if ui.add_enabled(enabled, button).clicked() {
        cmds.push(cmd);
        ui.close();
    }
}

/// The submenu Intersect Faces (MOD-08) of the Edit menu and of the context menu of the 3D view:
/// With Model and With Selection, each enabled by its flag.
pub fn intersect_menu(ui: &mut egui::Ui, enabled: [bool; 2], cmds: &mut Vec<Cmd>) {
    ui.menu_button(tr!("modeler-edit-intersect"), |ui| {
        for (i, (label, scope)) in [
            (tr!("modeler-edit-intersect-model"), IntersectScope::Model),
            (
                tr!("modeler-edit-intersect-selection"),
                IntersectScope::Selection,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            entry(
                ui,
                cmds,
                label,
                String::new(),
                enabled[i],
                Cmd::IntersectFaces(scope),
            );
        }
    });
}

/// The submenu Solid Tools (MOD-09) of the Edit menu: Check Solid, the five operations and the
/// order of the two solids. An operation is enabled when the selected faces name two solids; the
/// text of the item says what it would do, or why it is refused.
pub fn solid_menu(ui: &mut egui::Ui, app: &ModelerApp, cmds: &mut Vec<Cmd>) {
    ui.menu_button(tr!("modeler-edit-solid"), |ui| {
        entry(
            ui,
            cmds,
            tr!("modeler-solid-check"),
            String::new(),
            true,
            Cmd::CheckSolid,
        );
        ui.separator();
        let operands = app.solid_operands();
        for op in SolidOp::ALL {
            let label = kadakas_ui_core::modeling::solid::op_name(op);
            let text = match &operands {
                Ok(_) => app.solid_hint(op),
                Err(p) => p.text(),
            };
            let button = ui.add_enabled(operands.is_ok(), egui::Button::new(label));
            let button = if operands.is_ok() {
                button.on_hover_text(text)
            } else {
                button.on_disabled_hover_text(text)
            };
            if button.clicked() {
                cmds.push(Cmd::Solid(op));
                ui.close();
            }
        }
        ui.separator();
        choice(
            ui,
            cmds,
            tr!("modeler-solid-swap"),
            app.solid_swap,
            Cmd::SolidSwap,
        );
    });
}

/// Hide and Unhide (MOD-05) of the Edit menu and of the context menu of the 3D view, each enabled
/// by its flag.
pub fn hide_entries(ui: &mut egui::Ui, enabled: [bool; 2], cmds: &mut Vec<Cmd>) {
    entry(
        ui,
        cmds,
        tr!("modeler-edit-hide"),
        String::new(),
        enabled[0],
        Cmd::Hide,
    );
    entry(
        ui,
        cmds,
        tr!("modeler-edit-unhide"),
        String::new(),
        enabled[1],
        Cmd::Unhide,
    );
}

/// An item that shows whether it is on.
fn choice(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>, label: String, selected: bool, cmd: Cmd) {
    if ui.selectable_label(selected, label).clicked() {
        cmds.push(cmd);
        ui.close();
    }
}

/// Draws the menu bar.
pub fn show(ui: &mut egui::Ui, app: &ModelerApp, cmds: &mut Vec<Cmd>) {
    let keys = &app.keys;
    let hint = |a: Action| keys.hint(a);
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(tr!("modeler-menu-file"), |ui| {
            entry(
                ui,
                cmds,
                tr!("modeler-file-new"),
                hint(Action::FileNew),
                true,
                Cmd::New,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-file-open"),
                hint(Action::FileOpen),
                true,
                Cmd::Open,
            );
            ui.separator();
            entry(
                ui,
                cmds,
                tr!("modeler-file-save"),
                hint(Action::FileSave),
                true,
                Cmd::Save,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-file-save-as"),
                hint(Action::FileSaveAs),
                true,
                Cmd::SaveAs,
            );
            ui.separator();
            entry(
                ui,
                cmds,
                tr!("modeler-file-open-in-app"),
                String::new(),
                true,
                Cmd::OpenInApp,
            );
            ui.separator();
            entry(
                ui,
                cmds,
                tr!("modeler-file-quit"),
                String::new(),
                true,
                Cmd::Quit,
            );
        });
        ui.menu_button(tr!("modeler-menu-edit"), |ui| {
            let ed = &app.session.editor;
            entry(
                ui,
                cmds,
                tr!("modeler-edit-undo"),
                hint(Action::Undo),
                ed.can_undo(),
                Cmd::Undo,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-redo"),
                hint(Action::Redo),
                ed.can_redo(),
                Cmd::Redo,
            );
            ui.separator();
            entry(
                ui,
                cmds,
                tr!("modeler-edit-select-all"),
                hint(Action::SelectAll),
                true,
                Cmd::SelectAll,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-select-none"),
                hint(Action::SelectNone),
                !app.session.selection.is_empty(),
                Cmd::SelectNone,
            );
            ui.separator();
            intersect_menu(ui, app.intersect_flags(), cmds);
            solid_menu(ui, app, cmds);
            ui.separator();
            // Hide, guides (MOD-05).
            hide_entries(ui, [app.can_hide(), app.can_unhide()], cmds);
            entry(
                ui,
                cmds,
                tr!("modeler-edit-unhide-all"),
                String::new(),
                true,
                Cmd::UnhideAll,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-delete-guides"),
                String::new(),
                app.session.editor.doc().guide_count() > 0,
                Cmd::DeleteGuides,
            );
            ui.separator();
            // Groups and components (MOD-07).
            entry(
                ui,
                cmds,
                tr!("modeler-edit-make-group"),
                String::new(),
                app.can_structure(&Cmd::MakeGroup),
                Cmd::MakeGroup,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-make-component"),
                String::new(),
                app.can_structure(&Cmd::MakeComponent),
                Cmd::MakeComponent,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-explode"),
                String::new(),
                app.can_structure(&Cmd::Explode),
                Cmd::Explode,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-make-unique"),
                String::new(),
                app.can_structure(&Cmd::MakeUnique),
                Cmd::MakeUnique,
            );
            ui.separator();
            entry(
                ui,
                cmds,
                tr!("modeler-edit-edit-selected"),
                String::new(),
                app.can_structure(&Cmd::EditSelected),
                Cmd::EditSelected,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-close-context"),
                String::new(),
                app.can_structure(&Cmd::CloseContext),
                Cmd::CloseContext,
            );
            // After a conflict (somebody else changed the component while it was open).
            entry(
                ui,
                cmds,
                tr!("modeler-edit-close-keep-mine"),
                String::new(),
                app.can_structure(&Cmd::CloseKeepMine),
                Cmd::CloseKeepMine,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-close-discard-mine"),
                String::new(),
                app.can_structure(&Cmd::CloseDiscardMine),
                Cmd::CloseDiscardMine,
            );
            entry(
                ui,
                cmds,
                tr!("modeler-edit-close-overwrite"),
                String::new(),
                app.can_structure(&Cmd::CloseOverwrite),
                Cmd::CloseOverwrite,
            );
        });
        ui.menu_button(tr!("modeler-menu-camera"), |ui| camera_menu(ui, app, cmds));
        ui.menu_button(tr!("modeler-menu-tools"), |ui| {
            for tool in ModelerTool::ALL {
                let label = if tool.works() {
                    tool.name()
                } else {
                    tr!(
                        "uic-modeler-tool-later",
                        name = tool.name(),
                        note = tr!("uic-modeler-coming-later")
                    )
                };
                entry(
                    ui,
                    cmds,
                    label,
                    hint(tool.action()),
                    tool.works(),
                    Cmd::Tool(tool),
                );
            }
        });
        ui.menu_button(tr!("modeler-menu-view"), |ui| {
            choice(
                ui,
                cmds,
                tr!("modeler-view-tray"),
                app.prefs.tray,
                Cmd::ToggleTray,
            );
            ui.separator();
            let show = app.edit.cache.show;
            choice(
                ui,
                cmds,
                tr!("modeler-view-guides"),
                show.guides,
                Cmd::ToggleGuides,
            );
            choice(
                ui,
                cmds,
                tr!("modeler-view-hidden-geometry"),
                show.hidden_geometry,
                Cmd::ToggleHiddenGeometry,
            );
            choice(
                ui,
                cmds,
                tr!("modeler-view-hidden-objects"),
                show.hidden_objects,
                Cmd::ToggleHiddenObjects,
            );
            ui.separator();
            ui.menu_button(tr!("modeler-view-theme"), |ui| {
                for t in ThemeChoice::ALL {
                    choice(ui, cmds, t.label(), app.prefs.theme == t, Cmd::Theme(t));
                }
            });
            ui.menu_button(tr!("modeler-view-language"), |ui| {
                choice(
                    ui,
                    cmds,
                    tr!("modeler-language-system"),
                    app.prefs.language.is_none(),
                    Cmd::Language(None),
                );
                for lang in kadakas_l10n::languages() {
                    let on = app
                        .prefs
                        .language
                        .as_deref()
                        .is_some_and(|t| t.eq_ignore_ascii_case(lang.tag));
                    choice(
                        ui,
                        cmds,
                        lang.native_name.to_string(),
                        on,
                        Cmd::Language(Some(lang.tag.to_string())),
                    );
                }
            });
        });
        ui.menu_button(tr!("modeler-menu-help"), |ui| {
            entry(
                ui,
                cmds,
                tr!("modeler-help-about"),
                String::new(),
                true,
                Cmd::About,
            );
        });
    });
}

fn camera_menu(ui: &mut egui::Ui, app: &ModelerApp, cmds: &mut Vec<Cmd>) {
    let hint = |a: Action| app.keys.hint(a);
    entry(
        ui,
        cmds,
        tr!("modeler-camera-previous"),
        hint(Action::ViewHistoryPrevious),
        app.nav.can_go_back(),
        Cmd::Nav(NavAction::Previous),
    );
    entry(
        ui,
        cmds,
        tr!("modeler-camera-next"),
        hint(Action::ViewHistoryNext),
        app.nav.can_go_forward(),
        Cmd::Nav(NavAction::Next),
    );
    ui.separator();
    ui.menu_button(tr!("modeler-camera-standard-views"), |ui| {
        for view in MENU_VIEWS {
            entry(
                ui,
                cmds,
                view_name(view),
                hint(view_action(view)),
                true,
                Cmd::Nav(NavAction::View(view)),
            );
        }
    });
    ui.separator();
    let perspective = app.nav.is_perspective();
    choice(
        ui,
        cmds,
        tr!("modeler-camera-perspective"),
        perspective,
        Cmd::Projection(true),
    );
    choice(
        ui,
        cmds,
        tr!("modeler-camera-parallel"),
        !perspective,
        Cmd::Projection(false),
    );
    ui.menu_button(tr!("modeler-camera-fov"), |ui| {
        let mut deg = app.nav.fov_deg();
        let slider = egui::Slider::new(&mut deg, MIN_FOV_DEG..=MAX_FOV_DEG)
            .suffix("°")
            .fixed_decimals(1);
        if ui.add_enabled(perspective, slider).changed() {
            cmds.push(Cmd::SetFov(deg));
        }
        if !perspective {
            ui.label(egui::RichText::new(tr!("modeler-camera-fov-parallel")).weak());
        }
    });
    ui.separator();
    entry(
        ui,
        cmds,
        tr!("modeler-camera-zoom-extents"),
        hint(Action::ZoomFit),
        true,
        Cmd::Tool(ModelerTool::ZoomExtents),
    );
    entry(
        ui,
        cmds,
        tr!("modeler-camera-zoom-window"),
        hint(Action::NavZoomWindow),
        true,
        Cmd::ZoomWindow,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_menu_lists_the_seven_standard_views() {
        let names: Vec<String> = MENU_VIEWS.iter().map(|v| view_name(*v)).collect();
        assert_eq!(names.len(), 7);
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 7, "each view has its own name: {names:?}");
        assert_eq!(names[0], tr!("modeler-view-top"));
        assert_eq!(names[6], tr!("modeler-view-iso"));
    }
}
