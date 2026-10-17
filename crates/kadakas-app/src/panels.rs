//! Side panels, menu and data bar. Every document change goes through
//! `run_command`, which uses `Session::editor.execute`.

use crate::units_ui::DragValueExt as _;
use egui::{DragValue, Ui};
use glam::DVec2;
use kadakas_api::Session;
use kadakas_core::command::{Command, Outcome};
use kadakas_core::model::{Attributes, Color, ObjectId, Shape, Vertex, VertexKind};
use kadakas_l10n::tr;
use kadakas_render::{Projection, RenderMode, StandardView, ViewSettings};

use kadakas_core::plants::tree::{DEFAULT_RPZ_CAP, DEFAULT_RPZ_FACTOR, TreeAction, TreeCondition};

use crate::canvas::color32;
use crate::databar::{self, DataEntry};
use crate::icon_size::{IconRole, IconSize};
use crate::keymap::{Action, Keymap};
use kadakas_ui_core::profile::layout::visible_items;
use kadakas_ui_core::profile::{Cmd, Item, Profile};
use kadakas_ui_core::snap::SnapResult;
use kadakas_ui_core::tools::{
    DEFAULT_ARC_RADIUS, Tool, ToolMachine, ToolSettings, VertexMode, has_vertex_modes,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    New,
    Open,
    Save,
    SaveAs,
    ExportSvg,
    ExportPdf,
    Import,
    Export,
    /// File > Map Sources: WMS, WMTS, WFS and geoimages.
    MapSources,
    /// File > Change CRS: convert the document.
    ChangeCrs,
    /// File > Scripts: the Scripts palette with its console.
    Scripts,
    /// File > Plug-ins: install, review and trust WebAssembly plug-ins.
    PlugIns,
    /// File > Versions: the window of named versions and the comparison.
    Versions,
    /// File > Compare Versions: the colored overlay of two versions in the plan and in 3D.
    Compare,
    /// File > Import > Update from DXF/DWG...: a new revision of an imported drawing (IOP-23).
    UpdateFromDxf,
    /// File > BCF Issues...: read and write BCF files (REV-06).
    Bcf,
    /// Edit > New Issue from Selection (REV-05).
    NewIssue,
    /// File > Compare Drawings...: two DXF or DWG files without an import (IOP-24).
    CompareDrawings,
    Undo,
    Redo,
    Delete,
    SelectAll,
    ZoomFit,
    CreateSiteModel,
    UpdateSiteModel,
    /// Site > Create Texture Bed: closed shapes become texture beds.
    CreateTextureBed,
    Preferences,
    /// Modify > Duplicate Array.
    DuplicateArray,
    AddSurface,
    SubtractSurface,
    ClipSurface,
    IntersectSurface,
    Compose,
    /// Model > Extrude.
    Extrude,
    /// Model > Sweep.
    Sweep,
    /// Landscape > Create Objects from Shapes (PAT-06).
    CreateFromShapes,
    /// Landscape > Split Hardscape into Zones (PAT-07).
    SplitHardscape,
    /// Landscape > Check Unit Patterns (PAT-10).
    CheckUnitPatterns,
    RotateLeft90,
    RotateRight90,
    FlipHorizontal,
    FlipVertical,
    /// View > Previous View and Next View.
    PreviousView,
    NextView,
    QuickSearch,
    /// File > Collaboration > Connect.
    CollabConnect,
    /// File > Collaboration > Disconnect.
    CollabDisconnect,
    /// A tool in a menu of the profile: choosing it selects the tool.
    Tool(Tool),
}

/// What the menu needs besides the keys: the settings it changes and what is possible now.
pub struct MenuExtras<'a> {
    pub snap: &'a mut kadakas_core::snapping::Settings,
    pub cues: &'a mut bool,
    pub theme: &'a mut crate::theme::ThemeChoice,
    pub icon_size: &'a mut IconSize,
    pub layout_names: Vec<String>,
    pub can_back: bool,
    pub can_forward: bool,
    pub has_selection: bool,
    /// Connect works: the session is not joined or joining. It stays on during the wait between
    /// two tries, so a new token can be given.
    pub collab_can_connect: bool,
    /// Disconnect works: a shared document is joined, joining, waiting for the next try or failed.
    pub collab_can_disconnect: bool,
    /// Requests that `MenuAction` (which is `Copy`) cannot hold. The menu fills this list.
    pub requests: Vec<MenuRequest>,
}

/// A request of the Window menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuRequest {
    /// Help > About.
    About,
    /// Help > Report a Bug..., Request a Feature... and Feedback... (see `feedback_ui`).
    Feedback(crate::feedback_ui::Open),
    ShowTab(crate::layout::Tab),
    SaveLayoutAs,
    LoadLayout(String),
    DeleteLayout(String),
    ResetLayout,
}

/// Runs a command on the session. Errors go to the status line.
pub fn run_command(session: &mut Session, cmd: &Command, status: &mut String) -> Option<Outcome> {
    match session.editor.execute(cmd) {
        Ok(out) => {
            let doc = &session.editor.doc();
            session.selection.retain(|id| doc.object(*id).is_some());
            Some(out)
        }
        Err(e) => {
            *status = tr!("panels-error", e = e);
            None
        }
    }
}

/// The menu action of a command of a profile menu. The commands that are blocks of the menu
/// (View > Snapping, Window and so on) have none; the menu draws them itself.
pub fn menu_action(c: Cmd) -> Option<MenuAction> {
    use MenuAction as M;
    Some(match c {
        Cmd::New => M::New,
        Cmd::Open => M::Open,
        Cmd::Save => M::Save,
        Cmd::SaveAs => M::SaveAs,
        Cmd::ExportSvg => M::ExportSvg,
        Cmd::ExportPdf => M::ExportPdf,
        Cmd::Import => M::Import,
        Cmd::Export => M::Export,
        Cmd::MapSources => M::MapSources,
        Cmd::ChangeCrs => M::ChangeCrs,
        Cmd::Scripts => M::Scripts,
        Cmd::PlugIns => M::PlugIns,
        Cmd::Versions => M::Versions,
        Cmd::Compare => M::Compare,
        Cmd::UpdateFromDxf => M::UpdateFromDxf,
        Cmd::Bcf => M::Bcf,
        Cmd::NewIssue => M::NewIssue,
        Cmd::CompareDrawings => M::CompareDrawings,
        Cmd::Preferences => M::Preferences,
        Cmd::Undo => M::Undo,
        Cmd::Redo => M::Redo,
        Cmd::Delete => M::Delete,
        Cmd::SelectAll => M::SelectAll,
        Cmd::QuickSearch => M::QuickSearch,
        Cmd::DuplicateArray => M::DuplicateArray,
        Cmd::RotateLeft90 => M::RotateLeft90,
        Cmd::RotateRight90 => M::RotateRight90,
        Cmd::FlipHorizontal => M::FlipHorizontal,
        Cmd::FlipVertical => M::FlipVertical,
        Cmd::AddSurface => M::AddSurface,
        Cmd::SubtractSurface => M::SubtractSurface,
        Cmd::ClipSurface => M::ClipSurface,
        Cmd::IntersectSurface => M::IntersectSurface,
        Cmd::Compose => M::Compose,
        Cmd::Extrude => M::Extrude,
        Cmd::Sweep => M::Sweep,
        Cmd::CreateFromShapes => M::CreateFromShapes,
        Cmd::SplitHardscape => M::SplitHardscape,
        Cmd::CheckUnitPatterns => M::CheckUnitPatterns,
        Cmd::CreateSiteModel => M::CreateSiteModel,
        Cmd::UpdateSiteModel => M::UpdateSiteModel,
        Cmd::CreateTextureBed => M::CreateTextureBed,
        Cmd::ZoomFit => M::ZoomFit,
        Cmd::PreviousView => M::PreviousView,
        Cmd::NextView => M::NextView,
        Cmd::ShowGrid
        | Cmd::Snapping
        | Cmd::Theme
        | Cmd::IconSize
        | Cmd::WindowPanels
        | Cmd::WindowLayouts
        | Cmd::Collaboration
        | Cmd::ImportMenu
        | Cmd::ReportBug
        | Cmd::RequestFeature
        | Cmd::Feedback
        | Cmd::About => return None,
    })
}

/// What the menu knows about the state of the program, for the enabled state of items.
pub struct MenuState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub can_site: bool,
    /// Closed shapes are selected: Create Texture Bed works.
    pub can_texture_bed: bool,
    pub can_update_site: bool,
}

/// True when a command needs a selection.
fn needs_selection(c: Cmd) -> bool {
    matches!(
        c,
        Cmd::DuplicateArray
            | Cmd::RotateLeft90
            | Cmd::RotateRight90
            | Cmd::FlipHorizontal
            | Cmd::FlipVertical
            | Cmd::AddSurface
            | Cmd::SubtractSurface
            | Cmd::ClipSurface
            | Cmd::IntersectSurface
            | Cmd::Compose
            | Cmd::Extrude
            | Cmd::Sweep
            | Cmd::CreateFromShapes
            | Cmd::SplitHardscape
    )
}

/// The menu bar of a profile: its menus, in its order.
pub fn menu_bar(
    ui: &mut Ui,
    keys: &Keymap,
    profile: &Profile,
    state: &MenuState,
    extras: &mut MenuExtras,
    show_grid: &mut bool,
) -> Vec<MenuAction> {
    let mut out = Vec::new();
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in &profile.menus {
            ui.menu_button(menu.title(), |ui| {
                let items = visible_items(&menu.items, cfg!(not(target_arch = "wasm32")));
                for item in items {
                    match item {
                        Item::Separator => {
                            ui.separator();
                        }
                        Item::Tool(t) => {
                            let a = MenuAction::Tool(t);
                            menu_item(ui, keys, &t.name(), true, a, &mut out);
                        }
                        Item::Command(c) => {
                            command_item(ui, keys, c, state, extras, show_grid, &mut out)
                        }
                    }
                }
            });
        }
        // Preferences is how a person reaches the profiles. A profile without that command
        // still gets a way to it.
        if lacks_preferences(profile) {
            ui.menu_button(Cmd::Preferences.label(), |ui| {
                let label = Cmd::Preferences.label();
                menu_item(ui, keys, &label, true, MenuAction::Preferences, &mut out);
            });
        }
    });
    out
}

/// True when no menu of the profile has the Preferences command (menus that are empty, or
/// that a profile file left it out of).
pub fn lacks_preferences(profile: &Profile) -> bool {
    !profile
        .menus
        .iter()
        .any(|m| m.items.contains(&Item::Command(Cmd::Preferences)))
}

fn menu_item(
    ui: &mut Ui,
    keys: &Keymap,
    label: &str,
    enabled: bool,
    a: MenuAction,
    out: &mut Vec<MenuAction>,
) {
    let button = crate::icons::button(ui, crate::icons::menu_icon(a), label)
        .shortcut_text(menu_key(a).map(|k| keys.hint(k)).unwrap_or_default());
    if ui.add_enabled(enabled, button).clicked() {
        out.push(a);
        ui.close();
    }
}

fn command_item(
    ui: &mut Ui,
    keys: &Keymap,
    c: Cmd,
    state: &MenuState,
    extras: &mut MenuExtras,
    show_grid: &mut bool,
    out: &mut Vec<MenuAction>,
) {
    if let Some(a) = menu_action(c) {
        let enabled = match c {
            Cmd::Undo => state.can_undo,
            Cmd::Redo => state.can_redo,
            Cmd::CreateSiteModel => state.can_site,
            Cmd::UpdateSiteModel => state.can_update_site,
            Cmd::CreateTextureBed => state.can_texture_bed,
            Cmd::PreviousView => extras.can_back,
            Cmd::NextView => extras.can_forward,
            c if needs_selection(c) => extras.has_selection,
            _ => true,
        };
        menu_item(ui, keys, &c.label(), enabled, a, out);
        return;
    }
    match c {
        Cmd::ShowGrid => {
            ui.checkbox(show_grid, c.label());
        }
        Cmd::Snapping => {
            ui.menu_button(c.label(), |ui| {
                for (name, label, tip) in crate::snap_ui::CATEGORIES {
                    let mut on = extras.snap.set.get(name).unwrap_or(false);
                    if ui
                        .checkbox(&mut on, kadakas_l10n::translate(label, &[]))
                        .on_hover_text(kadakas_l10n::translate(tip, &[]))
                        .changed()
                    {
                        extras.snap.set.set(name, on);
                    }
                }
                ui.separator();
                ui.checkbox(extras.cues, tr!("panels-smartcursor-cues"));
            });
        }
        Cmd::Theme => {
            ui.menu_button(c.label(), |ui| {
                for t in crate::theme::ThemeChoice::ALL {
                    ui.selectable_value(extras.theme, t, t.label());
                }
            });
        }
        Cmd::IconSize => {
            ui.menu_button(c.label(), |ui| {
                for z in IconSize::ALL {
                    ui.selectable_value(extras.icon_size, z, z.label());
                }
            });
        }
        Cmd::WindowPanels => {
            for tab in crate::layout::Tab::ALL {
                if tab.closeable() && ui.button(tr!("panels-show", name = tab.title())).clicked() {
                    extras.requests.push(MenuRequest::ShowTab(tab));
                    ui.close();
                }
            }
        }
        Cmd::WindowLayouts => window_layouts(ui, extras),
        Cmd::Collaboration => {
            let (can_connect, can_disconnect) =
                (extras.collab_can_connect, extras.collab_can_disconnect);
            ui.menu_button(c.label(), |ui| {
                let label = tr!("collab-ui-connect");
                menu_item(
                    ui,
                    keys,
                    &label,
                    can_connect,
                    MenuAction::CollabConnect,
                    out,
                );
                let label = tr!("collab-ui-disconnect");
                menu_item(
                    ui,
                    keys,
                    &label,
                    can_disconnect,
                    MenuAction::CollabDisconnect,
                    out,
                );
                ui.separator();
                if ui
                    .button(tr!(
                        "panels-show",
                        name = crate::layout::Tab::People.title()
                    ))
                    .clicked()
                {
                    extras
                        .requests
                        .push(MenuRequest::ShowTab(crate::layout::Tab::People));
                    ui.close();
                }
            });
        }
        Cmd::ImportMenu => {
            ui.menu_button(c.label(), |ui| {
                menu_item(
                    ui,
                    keys,
                    &Cmd::Import.label(),
                    true,
                    MenuAction::Import,
                    out,
                );
                menu_item(
                    ui,
                    keys,
                    &Cmd::UpdateFromDxf.label(),
                    true,
                    MenuAction::UpdateFromDxf,
                    out,
                );
            });
        }
        Cmd::About if ui.button(c.label()).clicked() => {
            extras.requests.push(MenuRequest::About);
            ui.close();
        }
        Cmd::ReportBug | Cmd::RequestFeature | Cmd::Feedback if ui.button(c.label()).clicked() => {
            extras
                .requests
                .push(MenuRequest::Feedback(crate::feedback_ui::Open::of(c)));
            ui.close();
        }
        _ => {}
    }
}

/// The layout commands of the Window menu.
fn window_layouts(ui: &mut Ui, extras: &mut MenuExtras) {
    if ui.button(tr!("panels-save-layout-as")).clicked() {
        extras.requests.push(MenuRequest::SaveLayoutAs);
        ui.close();
    }
    if ui.button(tr!("panels-reset-layout")).clicked() {
        extras.requests.push(MenuRequest::ResetLayout);
        ui.close();
    }
    if !extras.layout_names.is_empty() {
        ui.separator();
        ui.label(tr!("panels-saved-layouts"));
        for name in extras.layout_names.clone() {
            ui.horizontal(|ui| {
                if ui.button(&name).clicked() {
                    extras.requests.push(MenuRequest::LoadLayout(name.clone()));
                    ui.close();
                }
                if ui
                    .small_button("\u{d7}")
                    .on_hover_text(tr!("panels-delete-this-layout"))
                    .clicked()
                {
                    extras
                        .requests
                        .push(MenuRequest::DeleteLayout(name.clone()));
                }
            });
        }
    }
}

/// The key of a view, for a tooltip. Empty for a view without a key.
fn view_hint(keys: &Keymap, view: StandardView) -> String {
    Action::for_view(view)
        .map(|a| keys.hint(a))
        .unwrap_or_default()
}

/// The view bar: standard view, projection and render mode. The projection and the render
/// mode apply to the 3D views only; Top/Plan is the 2D drawing.
pub fn view_bar(ui: &mut Ui, v: &mut ViewSettings, keys: &Keymap, gpu_ready: bool) {
    ui.horizontal(|ui| {
        ui.label(tr!("panels-view"));
        egui::ComboBox::from_id_salt("standard_view")
            .selected_text(crate::enum_names::standard_view(v.view))
            .show_ui(ui, |ui| {
                for s in StandardView::ALL {
                    ui.selectable_value(&mut v.view, s, crate::enum_names::standard_view(s))
                        .on_hover_text(view_hint(keys, s));
                }
            });
        for s in [
            StandardView::Plan,
            StandardView::Front,
            StandardView::RightIsometric,
        ] {
            if crate::icons::select_button_for(
                ui,
                IconRole::Bar,
                crate::icons::view_icon(s),
                s.label(),
                v.view == s,
            )
            .on_hover_text(view_hint(keys, s))
            .clicked()
            {
                v.view = s;
            }
        }
        ui.separator();
        ui.add_enabled_ui(!v.view.is_plan() && gpu_ready, |ui| {
            ui.selectable_value(
                &mut v.projection,
                Projection::Orthographic,
                tr!("panels-parallel"),
            );
            ui.selectable_value(
                &mut v.projection,
                Projection::Perspective,
                tr!("panels-perspective"),
            )
            .on_hover_text(keys.hint(Action::ViewProjection));
            ui.separator();
            if crate::icons::select_button_for(
                ui,
                IconRole::Bar,
                crate::icons::render_icon(RenderMode::Wireframe),
                &tr!("panels-wireframe"),
                v.mode == RenderMode::Wireframe,
            )
            .on_hover_text(keys.hint(Action::ViewWireframe))
            .clicked()
            {
                v.mode = RenderMode::Wireframe;
            }
            if crate::icons::select_button_for(
                ui,
                IconRole::Bar,
                crate::icons::render_icon(RenderMode::Shaded),
                &tr!("panels-shaded"),
                v.mode == RenderMode::Shaded,
            )
            .on_hover_text(keys.hint(Action::ViewShaded))
            .clicked()
            {
                v.mode = RenderMode::Shaded;
            }
            if crate::icons::select_button_for(
                ui,
                IconRole::Bar,
                crate::icons::render_icon(RenderMode::HiddenLine),
                &tr!("panels-hidden-line"),
                v.mode == RenderMode::HiddenLine,
            )
            .on_hover_text(keys.hint(Action::ViewHiddenLine))
            .clicked()
            {
                v.mode = RenderMode::HiddenLine;
            }
            if crate::icons::select_button_for(
                ui,
                IconRole::Bar,
                crate::icons::render_icon(RenderMode::DashedHiddenLine),
                &tr!("panels-dashed"),
                v.mode == RenderMode::DashedHiddenLine,
            )
            .on_hover_text(keys.hint(Action::ViewDashedHiddenLine))
            .clicked()
            {
                v.mode = RenderMode::DashedHiddenLine;
            }
        });
    });
}

/// The key action that does the same as a menu item.
pub fn menu_key(a: MenuAction) -> Option<Action> {
    Some(match a {
        MenuAction::New => Action::FileNew,
        MenuAction::Open => Action::FileOpen,
        MenuAction::Save => Action::FileSave,
        MenuAction::SaveAs => Action::FileSaveAs,
        MenuAction::ExportPdf => Action::FileExportPdf,
        MenuAction::Undo => Action::Undo,
        MenuAction::Redo => Action::Redo,
        MenuAction::Delete => Action::Delete,
        MenuAction::SelectAll => Action::SelectAll,
        MenuAction::ZoomFit => Action::ZoomFit,
        MenuAction::DuplicateArray => Action::CmdDuplicateArray,
        MenuAction::AddSurface => Action::CmdAddSurface,
        MenuAction::SubtractSurface => Action::CmdSubtractSurface,
        MenuAction::ClipSurface => Action::CmdClipSurface,
        MenuAction::IntersectSurface => Action::CmdIntersectSurface,
        MenuAction::Compose => Action::CmdCompose,
        MenuAction::Extrude => Action::CmdExtrude,
        MenuAction::Sweep => Action::CmdSweep,
        MenuAction::CreateFromShapes => Action::CmdCreateFromShapes,
        MenuAction::SplitHardscape => Action::CmdSplitHardscape,
        MenuAction::CheckUnitPatterns => Action::CmdCheckUnitPatterns,
        MenuAction::RotateLeft90 => Action::CmdRotateLeft90,
        MenuAction::RotateRight90 => Action::CmdRotateRight90,
        MenuAction::FlipHorizontal => Action::CmdFlipHorizontal,
        MenuAction::FlipVertical => Action::CmdFlipVertical,
        MenuAction::PreviousView => Action::ViewHistoryPrevious,
        MenuAction::NextView => Action::ViewHistoryNext,
        MenuAction::QuickSearch => Action::QuickSearch,
        MenuAction::Tool(t) => Action::for_tool(t),
        MenuAction::ExportSvg
        | MenuAction::Import
        | MenuAction::Export
        | MenuAction::MapSources
        | MenuAction::ChangeCrs
        | MenuAction::Scripts
        | MenuAction::PlugIns
        | MenuAction::Versions
        | MenuAction::Compare
        | MenuAction::UpdateFromDxf
        | MenuAction::Bcf
        | MenuAction::NewIssue
        | MenuAction::CompareDrawings
        | MenuAction::CreateSiteModel
        | MenuAction::Preferences
        | MenuAction::CollabConnect
        | MenuAction::CollabDisconnect
        | MenuAction::CreateTextureBed
        | MenuAction::UpdateSiteModel => return None,
    })
}

/// Button label and tooltip text of a tool, with its bound key.
pub fn tool_label(tool: Tool, keys: &Keymap) -> (String, String) {
    let key = keys.hint(Action::for_tool(tool));
    if key.is_empty() {
        (tool.name().to_string(), tool.name().to_string())
    } else {
        (
            tr!("panels-text", name = tool.name(), key = key),
            tr!("panels-text-2", name = tool.name(), key = key),
        )
    }
}

fn tool_button(ui: &mut Ui, m: &mut ToolMachine, keys: &Keymap, tool: Tool) {
    let (label, tip) = tool_label(tool, keys);
    let r = crate::icons::select_button_for(
        ui,
        IconRole::Palette,
        crate::icons::tool_icon(tool),
        &label,
        m.tool() == tool,
    )
    .on_hover_text(tip);
    if r.clicked() {
        m.set_tool(tool);
    }
}

/// The vertex mode buttons and the default arc radius, for the tools that draw paths.
pub fn vertex_mode_bar(ui: &mut Ui, settings: &mut ToolSettings, keys: &Keymap) {
    ui.add_space(8.0);
    ui.label(tr!("panels-vertex-mode"));
    let key = keys.hint(Action::ToolMode1);
    let tip = if key.is_empty() {
        String::new()
    } else {
        tr!("panels-cycles-the-modes", key = key)
    };
    ui.horizontal_wrapped(|ui| {
        for mode in VertexMode::ALL {
            let r = ui.selectable_label(settings.vertex_mode == mode, mode.label());
            if r.on_hover_text(tr!("panels-vertex", mode = mode.label(), tip = tip))
                .clicked()
            {
                settings.vertex_mode = mode;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr!("panels-arc-radius"));
        let r = ui.add(
            DragValue::new(&mut settings.arc_radius)
                .speed(10.0)
                .range(0.0..=1.0e7)
                .length(),
        );
        r.on_hover_text(tr!("panels-radius-of-new-arc-vertices-type-a"));
    });
}

pub fn tool_panel(
    ui: &mut Ui,
    m: &mut ToolMachine,
    keys: &Keymap,
    profile: &Profile,
    settings: &mut ToolSettings,
    doc: &kadakas_core::model::Document,
    requests: &mut crate::annot_ui::PanelRequests,
) {
    ui.heading(tr!("panels-tools"));
    ui.separator();
    let current = m.tool();
    let mut extras_shown = false;
    let mut buttons_shown = false;
    for (i, set) in profile.tool_sets.iter().enumerate() {
        if i > 0 {
            ui.add_space(8.0);
        }
        ui.label(set.title());
        for t in &set.tools {
            tool_button(ui, m, keys, *t);
        }
        // The Graphic Legend and style buttons sit with the dimension tools.
        if set.tools.contains(&Tool::Dimension) && !buttons_shown {
            crate::annot_ui::panel_buttons(ui, doc, requests);
            buttons_shown = true;
        }
        // The options of the current tool sit under the set that holds it.
        if set.tools.contains(&current) && !extras_shown {
            tool_extras(ui, m, settings, doc, requests);
            extras_shown = true;
        }
    }
    if !buttons_shown {
        crate::annot_ui::panel_buttons(ui, doc, requests);
    }
    if !extras_shown {
        tool_extras(ui, m, settings, doc, requests);
    }
    if has_vertex_modes(m.tool()) {
        vertex_mode_bar(ui, settings, keys);
    }
    if matches!(m.tool(), Tool::Hardscape | Tool::LandscapeArea) {
        crate::area_ui::tool_options(ui, m.tool(), settings, doc);
    }
    if m.tool() == Tool::Plant && crate::plant_ui::plant_mode_bar(ui, settings, doc) {
        // A new mode starts a new placement.
        m.cancel();
    }
}

/// The options that belong to the current tool, under its tool set.
fn tool_extras(
    ui: &mut Ui,
    m: &mut ToolMachine,
    settings: &mut ToolSettings,
    doc: &kadakas_core::model::Document,
    requests: &mut crate::annot_ui::PanelRequests,
) {
    if m.tool() == Tool::Select {
        select_mode_bar(ui, settings);
    }
    if kadakas_ui_core::tools::is_modify(m.tool()) {
        modify_options(ui, m, settings);
    }
    if m.tool() == Tool::SiteModifier
        && settings.modifier_mode == kadakas_ui_core::tools::ModifierMode::TextureBed
    {
        crate::site_ui::bed_tool_options(ui, settings, doc);
    }
    if m.tool() == Tool::Irrigation && crate::irrigation_ui::tool_options(ui, settings, doc) {
        // A new mode starts a new object.
        m.cancel();
    }
    if Tool::ANNOTATION.contains(&m.tool()) {
        crate::annot_ui::tool_options(ui, m.tool(), settings, doc, requests);
    }
}

/// The modes of the Selection tool: how a drag on empty space picks.
fn select_mode_bar(ui: &mut Ui, settings: &mut ToolSettings) {
    ui.add_space(4.0);
    ui.label(tr!("panels-selection-mode"));
    ui.horizontal_wrapped(|ui| {
        for mode in kadakas_ui_core::tools::SelectMode::ALL {
            let icon = match mode {
                kadakas_ui_core::tools::SelectMode::Rectangle => "rectangle",
                kadakas_ui_core::tools::SelectMode::Lasso => "lasso",
            };
            let r = crate::icons::select_button_for(
                ui,
                IconRole::Bar,
                icon,
                &mode.label(),
                settings.select_mode == mode,
            );
            let tip = match mode {
                kadakas_ui_core::tools::SelectMode::Rectangle => {
                    tr!("panels-drag-a-box-left-to-right-selects")
                }
                kadakas_ui_core::tools::SelectMode::Lasso => {
                    tr!("panels-drag-a-free-hand-shape-it-ends")
                }
            };
            if r.on_hover_text(tip).clicked() {
                settings.select_mode = mode;
            }
        }
    });
}

/// The options of the modify tools (the Tool bar fields).
fn modify_options(ui: &mut Ui, m: &mut ToolMachine, settings: &mut ToolSettings) {
    use kadakas_core::edit2d::modify::JoinMode;
    let s = &mut settings.modify;
    let tool = m.tool();
    ui.add_space(4.0);
    match tool {
        Tool::Rotate | Tool::Mirror | Tool::Scale => {
            ui.checkbox(&mut s.copy, tr!("panels-duplicate"))
                .on_hover_text(tr!("panels-work-on-copies-and-keep-the-originals"));
        }
        Tool::Offset => {
            ui.horizontal(|ui| {
                ui.label(tr!("panels-distance"));
                ui.add(
                    DragValue::new(&mut s.offset_distance)
                        .speed(10.0)
                        .range(0.0..=1.0e7)
                        .length(),
                )
                .on_hover_text(tr!("panels-0-the-copy-passes-through-the-second"));
            });
            ui.checkbox(&mut s.offset_copy, tr!("panels-make-a-copy"));
        }
        Tool::Fillet | Tool::Chamfer => {
            if tool == Tool::Fillet {
                ui.horizontal(|ui| {
                    ui.label(tr!("panels-radius"));
                    ui.add(
                        DragValue::new(&mut s.radius)
                            .speed(10.0)
                            .range(0.001..=1.0e7)
                            .length(),
                    );
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label(tr!("panels-first"));
                    ui.add(
                        DragValue::new(&mut s.chamfer_a)
                            .speed(10.0)
                            .range(0.001..=1.0e7)
                            .length(),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(tr!("panels-second"));
                    ui.add(
                        DragValue::new(&mut s.chamfer_b)
                            .speed(10.0)
                            .range(0.001..=1.0e7)
                            .length(),
                    );
                });
            }
            ui.horizontal_wrapped(|ui| {
                for (mode, label, tip) in [
                    (
                        JoinMode::Standard,
                        tr!("panels-standard"),
                        tr!("panels-only-add-the-new-piece-the-objects"),
                    ),
                    (
                        JoinMode::Split,
                        tr!("panels-split"),
                        tr!("panels-cut-the-objects-at-the-new-piece"),
                    ),
                    (
                        JoinMode::Trim,
                        tr!("panels-trim"),
                        tr!("panels-cut-the-objects-at-the-new-piece-2"),
                    ),
                ] {
                    if ui
                        .selectable_label(s.join_mode == mode, label)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        s.join_mode = mode;
                    }
                }
            });
        }
        Tool::Trim | Tool::Extend => {
            ui.horizontal_wrapped(|ui| {
                for (mode, label) in [
                    (
                        kadakas_ui_core::tools::CutTo::AllObjects,
                        tr!("panels-all-objects"),
                    ),
                    (
                        kadakas_ui_core::tools::CutTo::SelectedObjects,
                        tr!("panels-selected-objects"),
                    ),
                ] {
                    if ui.selectable_label(s.cut_to == mode, label).clicked() {
                        s.cut_to = mode;
                    }
                }
            });
        }
        _ => {}
    }
}

/// Draws the Data bar. Returns true when Enter was pressed in an entry field.
#[allow(clippy::too_many_arguments)]
pub fn data_bar(
    ui: &mut Ui,
    m: &ToolMachine,
    s: &mut ToolSettings,
    entry: &mut DataEntry,
    cursor: DVec2,
    snap: Option<&SnapResult>,
    snap_on: bool,
    readout: Option<(&str, &mut bool)>,
) -> bool {
    let mut commit = false;
    ui.horizontal_wrapped(|ui| {
        let u = kadakas_units::current();
        ui.monospace(tr!(
            "panels-cursor-position",
            x = format!("{:>10}", u.length_field(cursor.x, 1)),
            y = format!("{:>10}", u.length_field(cursor.y, 1)),
            unit = u.length_field_unit()
        ));
        if let Some((t, lonlat)) = readout {
            ui.separator();
            let label =
                egui::Label::new(egui::RichText::new(t).monospace()).sense(egui::Sense::click());
            if ui
                .add(label)
                .on_hover_text(tr!("panels-map-position-of-the-cursor-click-to"))
                .clicked()
            {
                *lonlat = !*lonlat;
            }
        }
        ui.separator();
        ui.label(tr!("panels-tool", tool = m.tool().name()));
        ui.separator();
        match m.tool() {
            Tool::Text => {
                ui.label(tr!("panels-text-3"));
                ui.add(egui::TextEdit::singleline(&mut s.text).desired_width(120.0));
                ui.label(tr!("panels-height"));
                ui.add(
                    DragValue::new(&mut s.text_height)
                        .speed(5.0)
                        .range(1.0..=100000.0),
                );
            }
            Tool::Plant => {
                ui.label(tr!("panels-species"));
                ui.add(egui::TextEdit::singleline(&mut s.species).desired_width(140.0));
                ui.label(tr!("panels-spread"));
                ui.add(
                    DragValue::new(&mut s.spread)
                        .speed(50.0)
                        .range(1.0..=100000.0),
                );
                ui.label(tr!("panels-height"));
                ui.add(
                    DragValue::new(&mut s.plant_height)
                        .speed(50.0)
                        .range(0.0..=100000.0),
                );
            }
            Tool::LandscapeArea => {
                ui.label(tr!("panels-name"));
                ui.add(egui::TextEdit::singleline(&mut s.area_name).desired_width(140.0));
            }
            Tool::Hardscape => {
                ui.label(tr!("panels-material"));
                ui.add(egui::TextEdit::singleline(&mut s.material).desired_width(140.0));
            }
            Tool::Massing => crate::gis_ui::massing_bar(&mut s.massing, ui),
            Tool::Polygon3d => crate::solid_ui::polygon3d_bar(&mut s.polygon3d, ui),
            Tool::SiteModifier => {
                for mode in kadakas_ui_core::tools::ModifierMode::ALL {
                    ui.selectable_value(&mut s.modifier_mode, mode, mode.name());
                }
                if s.modifier_mode == kadakas_ui_core::tools::ModifierMode::Pad {
                    ui.label(tr!("panels-elevation"));
                    ui.add(
                        DragValue::new(&mut s.pad_elevation)
                            .speed(10.0)
                            .range(-1.0e8..=1.0e8)
                            .length(),
                    );
                    let mut sloped = s.pad_end_elevation.is_some();
                    if ui.checkbox(&mut sloped, tr!("panels-slope")).changed() {
                        s.pad_end_elevation = sloped.then_some(s.pad_elevation);
                    }
                    if let Some(end) = &mut s.pad_end_elevation {
                        ui.label(tr!("panels-elevation-at-end"));
                        ui.add(
                            DragValue::new(end)
                                .speed(10.0)
                                .range(-1.0e8..=1.0e8)
                                .length(),
                        );
                        ui.label(tr!("panels-falls-toward"));
                        ui.add(
                            DragValue::new(&mut s.pad_direction_deg)
                                .speed(1.0)
                                .direction(),
                        );
                    }
                }
            }
            Tool::Grading => crate::grading_ui::options(ui, &mut s.grading),
            _ => {}
        }
        let anchor = m.anchor();
        let arc = has_vertex_modes(m.tool()) && s.vertex_mode == VertexMode::Arc;
        let fields = databar::fields(m.tool(), anchor.is_some(), arc);
        if !fields.is_empty() {
            let live = databar::live_values(m.tool(), anchor, cursor, arc, s.arc_radius);
            let mut focused = false;
            let target = entry.want_focus.then_some(entry.focus);
            for (i, f) in fields.iter().enumerate() {
                ui.label(f.label());
                let Some(text) = entry.text.get_mut(i) else {
                    continue;
                };
                let hint = databar::field_text(*f, live[i]);
                let r = ui.add(
                    egui::TextEdit::singleline(text)
                        .id(databar::field_id(i))
                        .desired_width(70.0)
                        .hint_text(hint),
                );
                ui.label(f.unit());
                if target == Some(i) {
                    r.request_focus();
                    entry.want_focus = false;
                }
                if r.has_focus() {
                    focused = true;
                    if target.is_none() {
                        entry.focus = i;
                    }
                    // Keep Tab for the entry: it moves between fields instead of egui's focus order.
                    ui.memory_mut(|mem| {
                        mem.set_focus_lock_filter(
                            r.id,
                            egui::EventFilter {
                                tab: true,
                                horizontal_arrows: true,
                                vertical_arrows: false,
                                escape: false,
                            },
                        )
                    });
                }
                if r.lost_focus() && ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
                    commit = true;
                }
            }
            if let Some(problem) = entry.problem() {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 40), problem);
            }
            if !entry.want_focus {
                entry.active = focused || commit;
            }
        } else {
            entry.active = false;
        }
        let readout = m.readout(cursor, s);
        if !readout.is_empty() {
            ui.monospace(readout);
        }
        ui.separator();
        ui.label(crate::snap_ui::status_text(snap, snap_on));
    });
    commit
}

/// Edit buffer of the Object Info palette. Changes are committed when the
/// user finishes editing a field, so a drag is one undo step.
#[derive(Default)]
pub struct InfoEdit {
    id: ObjectId,
    base: Option<Shape>,
    edited: Option<Shape>,
}

pub(crate) struct Fields {
    pub(crate) commit: bool,
}

impl Fields {
    /// A length in mm, in the units of the person.
    pub(crate) fn num(&mut self, ui: &mut Ui, v: &mut f64, speed: f64) {
        let r = ui.add(DragValue::new(v).speed(speed).length());
        self.commit |= r.drag_stopped() || r.lost_focus();
    }
    fn num_range(&mut self, ui: &mut Ui, v: &mut f64) {
        let r = ui.add(DragValue::new(v).speed(10.0).range(0.0..=1.0e9).length());
        self.commit |= r.drag_stopped() || r.lost_focus();
    }
    /// A plain number: a scale, a factor, a count, a rotation in radians.
    pub(crate) fn plain(&mut self, ui: &mut Ui, v: &mut f64, speed: f64) {
        let r = ui.add(DragValue::new(v).speed(speed));
        self.commit |= r.drag_stopped() || r.lost_focus();
    }
    /// A labeled length in mm, in the units of the person.
    pub(crate) fn length_scalar(&mut self, ui: &mut Ui, label: &str, v: &mut f64, speed: f64) {
        ui.label(label);
        self.num(ui, v, speed);
        ui.end_row();
    }
    /// A labeled length on paper, in mm.
    pub(crate) fn paper_scalar(&mut self, ui: &mut Ui, label: &str, v: &mut f64, speed: f64) {
        ui.label(label);
        let r = ui.add(DragValue::new(v).speed(speed).paper_mm());
        self.commit |= r.drag_stopped() || r.lost_focus();
        ui.end_row();
    }
    pub(crate) fn xy(&mut self, ui: &mut Ui, label: &str, p: &mut DVec2) {
        ui.label(label);
        ui.horizontal(|ui| {
            self.num(ui, &mut p.x, 10.0);
            self.num(ui, &mut p.y, 10.0);
        });
        ui.end_row();
    }
    pub(crate) fn scalar(&mut self, ui: &mut Ui, label: &str, v: &mut f64, speed: f64) {
        ui.label(label);
        self.plain(ui, v, speed);
        ui.end_row();
    }
    pub(crate) fn text(&mut self, ui: &mut Ui, label: &str, s: &mut String) {
        ui.label(label);
        let r = ui.text_edit_singleline(s);
        self.commit |= r.lost_focus();
        ui.end_row();
    }
    fn angle(&mut self, ui: &mut Ui, label: &str, rad: &mut f64) {
        let mut deg = rad.to_degrees();
        ui.label(label);
        let r = ui.add(DragValue::new(&mut deg).speed(1.0).direction());
        self.commit |= r.drag_stopped() || r.lost_focus();
        *rad = deg.to_radians();
        ui.end_row();
    }
    pub(crate) fn count(&mut self, ui: &mut Ui, n: usize) {
        ui.label(tr!("panels-vertices"));
        ui.label(format!("{n}"));
        ui.end_row();
    }
}

/// Names of the vertex kinds in the vertex table.
pub fn kind_label(kind: VertexKind) -> String {
    match kind {
        VertexKind::Corner => tr!("panels-corner"),
        VertexKind::Arc { .. } => tr!("panels-arc"),
        VertexKind::PointOnArc => tr!("panels-point-on-arc"),
        VertexKind::Bezier => tr!("panels-bezier"),
        VertexKind::Cubic => tr!("panels-cubic"),
    }
}

const KIND_CHOICES: [VertexKind; 5] = [
    VertexKind::Corner,
    VertexKind::Arc {
        radius: DEFAULT_ARC_RADIUS,
    },
    VertexKind::PointOnArc,
    VertexKind::Bezier,
    VertexKind::Cubic,
];

/// The most vertex rows that the palette shows.
const MAX_VERTEX_ROWS: usize = 500;

/// The vertex list: index, X, Y, kind and radius. Returns true when an edit is finished.
fn vertex_table(ui: &mut Ui, id: ObjectId, vertices: &mut [Vertex]) -> bool {
    let mut f = Fields { commit: false };
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .id_salt("vertex_scroll")
        .show(ui, |ui| {
            egui::Grid::new("vertex_table")
                .num_columns(5)
                .striped(true)
                .show(ui, |ui| {
                    for h in [
                        "#".to_string(),
                        tr!("panels-x"),
                        tr!("panels-y"),
                        tr!("panels-kind"),
                        tr!("panels-radius"),
                    ] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (i, v) in vertices.iter_mut().enumerate().take(MAX_VERTEX_ROWS) {
                        ui.label(format!("{i}"));
                        f.num(ui, &mut v.p.x, 10.0);
                        f.num(ui, &mut v.p.y, 10.0);
                        let before = kind_label(v.kind);
                        egui::ComboBox::from_id_salt(("vertex_kind", id, i))
                            .selected_text(&before)
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for choice in KIND_CHOICES {
                                    let same = kind_label(choice) == before;
                                    if ui.selectable_label(same, kind_label(choice)).clicked()
                                        && !same
                                    {
                                        v.kind = choice;
                                        f.commit = true;
                                    }
                                }
                            });
                        if let VertexKind::Arc { radius } = &mut v.kind {
                            f.num_range(ui, radius);
                        } else {
                            ui.label("-");
                        }
                        ui.end_row();
                    }
                });
        });
    f.commit
}

/// The commands that turn the vertices `base` into `edited`: one `SetVertex` per changed vertex.
pub fn vertex_edit_commands(id: ObjectId, base: &[Vertex], edited: &[Vertex]) -> Vec<Command> {
    base.iter()
        .zip(edited)
        .enumerate()
        .filter(|(_, (b, e))| b != e)
        .map(|(index, (b, e))| Command::SetVertex {
            id,
            index,
            kind: (b.kind != e.kind).then_some(e.kind),
            p: (b.p != e.p).then_some(e.p),
        })
        .collect()
}

fn edit_shape(
    ui: &mut Ui,
    shape: &mut Shape,
    materials: &[(kadakas_core::resource::ResourceId, String)],
    effective_tile: Option<f64>,
    site_mapping: kadakas_core::texture_bed::BedMapping,
) -> bool {
    let mut f = Fields { commit: false };
    egui::Grid::new("shape_fields")
        .num_columns(2)
        .show(ui, |ui| match shape {
            Shape::Viewport(vp) => {
                f.text(ui, &tr!("panels-name"), &mut vp.name);
                f.scalar(ui, &tr!("panels-scale-1"), &mut vp.scale, 1.0);
                f.xy(ui, &tr!("panels-frame-min"), &mut vp.min);
                f.xy(ui, &tr!("panels-frame-max"), &mut vp.max);
                f.xy(ui, &tr!("panels-model-center"), &mut vp.center);
                ui.label(tr!("panels-frame"));
                f.commit |= ui
                    .checkbox(&mut vp.show_frame, tr!("panels-show-2"))
                    .changed();
                ui.end_row();
            }
            Shape::TitleBlock(tb) => f.paper_scalar(ui, &tr!("panels-margin"), &mut tb.margin, 1.0),
            Shape::Annotation(a) => f.commit |= crate::annot_ui::fields(ui, a),
            Shape::Unknown { kind, .. } => {
                ui.label(tr!(
                    "panels-unknown-shape-kind-made-by-a-newer",
                    kind = kind
                ));
            }
            Shape::Group { name } => f.text(ui, &tr!("panels-name"), name),
            Shape::Irrigation(i) => crate::irrigation_ui::edit(ui, &mut f, i),
            Shape::Symbol {
                at,
                rotation,
                scale,
                scale_y,
                ..
            } => {
                f.xy(ui, &tr!("panels-position"), at);
                f.scalar(ui, &tr!("panels-rotation-rad"), rotation, 0.1);
                f.scalar(ui, &tr!("panels-scale"), scale, 0.1);
                let mut sy = scale_y.unwrap_or(*scale);
                f.scalar(ui, &tr!("panels-scale-y"), &mut sy, 0.1);
                *scale_y = (sy != *scale).then_some(sy);
            }
            Shape::Line { a, b } => {
                f.xy(ui, &tr!("panels-start"), a);
                f.xy(ui, &tr!("panels-end"), b);
            }
            Shape::Rect { min, max } => {
                f.xy(ui, &tr!("panels-corner-1"), min);
                f.xy(ui, &tr!("panels-corner-2"), max);
            }
            Shape::Circle { center, radius } => {
                f.xy(ui, &tr!("panels-center"), center);
                f.length_scalar(ui, &tr!("panels-radius"), radius, 10.0);
            }
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                f.xy(ui, &tr!("panels-center"), center);
                f.length_scalar(ui, &tr!("panels-radius"), radius, 10.0);
                f.angle(ui, &tr!("panels-start"), start);
                f.angle(ui, &tr!("panels-end"), end);
            }
            Shape::Polyline { points, closed } => {
                ui.label(tr!("panels-closed"));
                f.commit |= ui.checkbox(closed, "").changed();
                ui.end_row();
                f.count(ui, points.len());
            }
            Shape::Locus3d { p } => {
                ui.label(tr!("panels-position"));
                ui.horizontal(|ui| {
                    f.num(ui, &mut p.x, 10.0);
                    f.num(ui, &mut p.y, 10.0);
                });
                ui.end_row();
                f.length_scalar(ui, &tr!("panels-z"), &mut p.z, 10.0);
            }
            Shape::Polyline3d { points, closed } => {
                crate::solid_ui::polygon3d_fields(ui, &mut f, points, closed)
            }
            Shape::Text { at, text, height } => {
                f.xy(ui, &tr!("panels-position"), at);
                f.text(ui, &tr!("panels-text-3"), text);
                f.length_scalar(ui, &tr!("panels-height"), height, 5.0);
            }
            Shape::Plant {
                at,
                species,
                spread,
                height,
            } => {
                f.xy(ui, &tr!("panels-position"), at);
                f.text(ui, &tr!("panels-species"), species);
                f.length_scalar(ui, &tr!("panels-spread"), spread, 50.0);
                f.length_scalar(ui, &tr!("panels-height"), height, 50.0);
            }
            Shape::ExistingTree {
                at,
                species,
                dbh,
                spread,
                height,
                condition,
                action,
                rpz_factor,
                rpz_cap,
            } => {
                f.xy(ui, &tr!("panels-position"), at);
                f.text(ui, &tr!("panels-species"), species);
                f.length_scalar(ui, &tr!("panels-trunk-diameter"), dbh, 10.0);
                f.length_scalar(ui, &tr!("panels-canopy-spread"), spread, 50.0);
                f.length_scalar(ui, &tr!("panels-height"), height, 50.0);
                ui.label(tr!("panels-condition"));
                let before = *condition;
                egui::ComboBox::from_id_salt("tree_condition")
                    .selected_text(crate::enum_names::name("condition", condition.id()))
                    .show_ui(ui, |ui| {
                        for c in TreeCondition::ALL {
                            ui.selectable_value(
                                condition,
                                c,
                                crate::enum_names::name("condition", c.id()),
                            );
                        }
                    });
                f.commit |= *condition != before;
                ui.end_row();
                ui.label(tr!("panels-action"));
                let before = *action;
                egui::ComboBox::from_id_salt("tree_action")
                    .selected_text(crate::enum_names::name("action", action.id()))
                    .show_ui(ui, |ui| {
                        for a in TreeAction::ALL {
                            ui.selectable_value(
                                action,
                                a,
                                crate::enum_names::name("action", a.id()),
                            );
                        }
                    });
                f.commit |= *action != before;
                ui.end_row();
                ui.label(tr!("panels-root-zone-factor"));
                let mut factor = rpz_factor.unwrap_or(DEFAULT_RPZ_FACTOR);
                let r = ui.add(
                    DragValue::new(&mut factor)
                        .speed(0.1)
                        .range(0.0..=100.0)
                        .suffix(tr!("panels-x-dbh")),
                );
                if r.changed() {
                    *rpz_factor = Some(factor);
                }
                f.commit |= r.drag_stopped() || r.lost_focus();
                ui.end_row();
                ui.label(tr!("panels-root-zone-cap"));
                let mut cap = rpz_cap.unwrap_or(DEFAULT_RPZ_CAP);
                let r = ui.add(
                    DragValue::new(&mut cap)
                        .speed(100.0)
                        .range(0.0..=1.0e6)
                        .length(),
                );
                if r.changed() {
                    *rpz_cap = Some(cap);
                }
                f.commit |= r.drag_stopped() || r.lost_focus();
                ui.end_row();
            }
            Shape::PlantTag {
                at, anchor, height, ..
            } => {
                f.xy(ui, &tr!("panels-label-position"), at);
                f.xy(ui, &tr!("panels-leader-end"), anchor);
                f.length_scalar(ui, &tr!("panels-text-height"), height, 5.0);
            }
            Shape::LandscapeArea { boundary, name } => {
                f.text(ui, &tr!("panels-name"), name);
                f.count(ui, boundary.len());
            }
            Shape::Hardscape {
                boundary, material, ..
            } => {
                f.text(ui, &tr!("panels-material"), material);
                f.count(ui, boundary.len());
            }
            Shape::Massing(m) => {
                f.commit |= crate::gis_ui::massing_fields(ui, m);
                f.count(ui, m.footprint.len());
            }
            Shape::Mesh3d(m) => f.count(ui, m.vertices.len()),
            Shape::Extrude(e) => crate::solid_ui::extrude_fields(ui, &mut f, e),
            Shape::Sweep(sw) => crate::solid_ui::sweep_fields(ui, &mut f, sw),
            Shape::Underlay(u) => f.commit |= crate::gis_ui::underlay_fields(ui, u),
            Shape::SiteModel {
                points,
                lines,
                settings,
            } => {
                f.commit |= crate::site_ui::model_fields(
                    ui,
                    points.len(),
                    lines.len(),
                    settings,
                    materials,
                    effective_tile,
                )
            }
            Shape::SiteModifier { modifier, boundary } => {
                f.commit |=
                    crate::site_ui::modifier_fields(ui, modifier, boundary, materials, site_mapping)
            }
        });
    f.commit
}

fn attr_row(ui: &mut Ui, label: &str, value: &mut Option<Color>, resolved: Option<Color>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let mut over = value.is_some();
        if ui
            .checkbox(&mut over, label)
            .on_hover_text(tr!("panels-override-the-class-attribute"))
            .changed()
        {
            *value = over.then(|| resolved.unwrap_or(Color::BLACK));
            changed = true;
        }
        if let Some(c) = value {
            let mut c32 = color32(*c);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                *c = Color(c32.to_srgba_unmultiplied());
                changed = true;
            }
        }
    });
    changed
}

pub fn object_info(
    ui: &mut Ui,
    session: &mut Session,
    edit: &mut InfoEdit,
    paving: &mut crate::paving_ui::PavingUi,
    status: &mut String,
) {
    ui.heading(tr!("panels-object-info"));
    ui.separator();
    let sel = session.selection.clone();
    let Some(&id) = sel.first() else {
        let d = &session.editor.doc();
        let layer = d.layer(d.active_layer).map_or("?", |l| l.name.as_str());
        let class = d.class(d.active_class).map_or("?", |c| c.name.as_str());
        ui.label(tr!(
            "panels-nothing-selected-active-layer-active-class",
            layer = layer,
            class = class
        ));
        *edit = InfoEdit::default();
        return;
    };
    if crate::data_ui::info_tab(ui, session, &sel, status) {
        *edit = InfoEdit::default();
        return;
    }
    if sel.len() > 1 {
        ui.label(tr!("panels-objects-selected", count = sel.len()));
        *edit = InfoEdit::default();
        class_layer_combos(ui, session, &sel, status);
        return;
    }
    let Some(obj) = session.editor.doc().object(id).cloned() else {
        return;
    };
    // Reset the buffer when another object is selected or the shape changed elsewhere.
    if edit.id != id || edit.base.as_ref() != Some(&obj.shape) {
        *edit = InfoEdit {
            id,
            base: Some(obj.shape.clone()),
            edited: Some(obj.shape.clone()),
        };
    }
    ui.label(tr!("panels-id", name = obj.shape.kind(), id = id));
    let edited = edit.edited.as_mut().expect("edit buffer");
    let materials: Vec<(kadakas_core::resource::ResourceId, String)> = if matches!(
        edited,
        Shape::SiteModel { .. }
            | Shape::SiteModifier {
                modifier: kadakas_core::site::ModifierKind::TextureBed { .. },
                ..
            }
    ) {
        let doc = session.editor.doc();
        doc.resources
            .iter()
            .filter(|r| kadakas_core::present::material(doc, r.id).is_some())
            .map(|r| (r.id, r.name.clone()))
            .collect()
    } else {
        Vec::new()
    };
    // The tile that the renderers use for the terrain texture (larger on a very big terrain).
    let effective_tile = match edited {
        Shape::SiteModel { settings, .. }
            if settings.surface_look == kadakas_core::site::SurfaceLook::Texture =>
        {
            let tile = settings.texture_tile;
            let scene = session.scene();
            Some(kadakas_render::look::terrain_tile(&scene, id, tile))
        }
        _ => None,
    };
    let site_mapping = kadakas_core::texture_bed::site_mapping(session.editor.doc(), obj.layer);
    let commit = edit_shape(ui, edited, &materials, effective_tile, site_mapping);
    // Vertex edits are sent as `set_vertex` commands, one per changed vertex.
    if edited.vertices().is_some() {
        let base_vertices = edit
            .base
            .as_ref()
            .and_then(Shape::vertices)
            .cloned()
            .unwrap_or_default();
        let finished = vertex_table(ui, id, edited.vertices_mut().expect("has vertices"));
        let cmds =
            vertex_edit_commands(id, &base_vertices, edited.vertices().expect("has vertices"));
        if finished && !cmds.is_empty() {
            let cmd = if cmds.len() == 1 {
                cmds[0].clone()
            } else {
                Command::Batch { commands: cmds }
            };
            run_command(session, &cmd, status);
            *edit = InfoEdit::default();
            return;
        }
    }
    if commit && Some(&*edited) != edit.base.as_ref() {
        let cmd = Command::ModifyObject {
            id,
            shape: Some(edited.clone()),
            layer: None,
            class: None,
            attrs: None,
        };
        if run_command(session, &cmd, status).is_none() {
            *edit = InfoEdit::default();
        }
    }
    if let Some(area) = obj.shape.area() {
        ui.label(tr!(
            "panels-area-value",
            area = kadakas_units::current().area_text(area)
        ));
    }
    if matches!(obj.shape, Shape::Plant { .. } | Shape::ExistingTree { .. }) {
        crate::plant_ui::plant_object_info(ui, session, id, status);
        crate::plant_ui::terrain_buttons(ui, session, id, status);
    }
    if matches!(obj.shape, Shape::PlantTag { .. }) {
        crate::plant_ui::plant_tag_info(ui, session, id, status);
    }
    if matches!(
        obj.shape,
        Shape::Hardscape { .. } | Shape::LandscapeArea { .. }
    ) {
        crate::area_ui::object_info(ui, session, id, paving, status);
    }
    if matches!(obj.shape, Shape::Irrigation(_)) {
        crate::irrigation_ui::object_info(ui, session, id, status);
    }
    crate::data_ui::table_info(ui, session, id, status);
    crate::site_ui::cut_fill_readout(ui, session, id);
    ui.separator();
    class_layer_combos(ui, session, &sel, status);
    let doc = &session.editor.doc();
    let res = doc.resolved_attrs(&obj);
    let mut attrs: Attributes = obj.attrs.clone();
    let mut changed = attr_row(ui, &tr!("panels-pen"), &mut attrs.pen, res.pen);
    changed |= attr_row(ui, &tr!("panels-fill"), &mut attrs.fill, res.fill);
    ui.horizontal(|ui| {
        let mut over = attrs.line_weight.is_some();
        if ui.checkbox(&mut over, tr!("panels-line-weight")).changed() {
            attrs.line_weight = over.then(|| res.line_weight.unwrap_or(0.25));
            changed = true;
        }
        if let Some(w) = &mut attrs.line_weight {
            let r = ui.add(DragValue::new(w).speed(0.05).range(0.01..=10.0).paper_mm());
            changed |= r.drag_stopped() || r.lost_focus();
        }
    });
    if changed && attrs != obj.attrs {
        let cmd = Command::ModifyObject {
            id,
            shape: None,
            layer: None,
            class: None,
            attrs: Some(attrs),
        };
        run_command(session, &cmd, status);
    }
}

fn class_layer_combos(ui: &mut Ui, session: &mut Session, sel: &[ObjectId], status: &mut String) {
    let doc = &session.editor.doc();
    let first = doc.object(sel[0]).cloned();
    let Some(first) = first else { return };
    let layers: Vec<_> = doc.layers.iter().map(|l| (l.id, l.name.clone())).collect();
    let layer_names: Vec<_> = layers.iter().map(|(id, n)| (id.0, n.clone())).collect();
    let classes: Vec<_> = doc.classes.iter().map(|c| (c.id, c.name.clone())).collect();
    let class_names: Vec<_> = classes.iter().map(|(id, n)| (id.0, n.clone())).collect();
    let mut layer = first.layer;
    let mut class = first.class;
    let name_of = |v: &[(kadakas_core::uid::Uid, String)], id: kadakas_core::uid::Uid| {
        v.iter()
            .find(|x| x.0 == id)
            .map(|x| x.1.clone())
            .unwrap_or_default()
    };
    egui::ComboBox::from_label(tr!("panels-layer"))
        .selected_text(name_of(&layer_names, layer.0))
        .show_ui(ui, |ui| {
            for (id, n) in &layers {
                ui.selectable_value(&mut layer, *id, n);
            }
        });
    egui::ComboBox::from_label(tr!("panels-class"))
        .selected_text(name_of(&class_names, class.0))
        .show_ui(ui, |ui| {
            for (id, n) in &classes {
                ui.selectable_value(&mut class, *id, n);
            }
        });
    if layer != first.layer || class != first.class {
        let cmds = sel
            .iter()
            .map(|&id| Command::ModifyObject {
                id,
                shape: None,
                layer: (layer != first.layer).then_some(layer),
                class: (class != first.class).then_some(class),
                attrs: None,
            })
            .collect();
        run_command(session, &Command::Batch { commands: cmds }, status);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenameKind {
    Layer,
    Class,
}

#[derive(Default)]
pub struct NavState {
    pub sheets: crate::sheets_ui::SheetUi,
    renaming: Option<(RenameKind, kadakas_core::uid::Uid, String)>,
}

pub fn navigation(ui: &mut Ui, session: &mut Session, nav: &mut NavState, status: &mut String) {
    ui.heading(tr!("panels-navigation"));
    ui.separator();
    let mut cmds: Vec<Command> = Vec::new();
    let doc = &session.editor.doc();

    ui.horizontal(|ui| {
        ui.strong(tr!("panels-design-layers"));
        if ui
            .small_button("+")
            .on_hover_text(tr!("panels-add-design-layer"))
            .clicked()
        {
            cmds.push(Command::AddLayer {
                name: format!("Layer {}", doc.layers.len() + 1),
                elevation: 0.0,
                scale: None,
                kind: None,
            });
        }
    });
    for l in doc
        .layers
        .iter()
        .filter(|l| l.kind == kadakas_core::model::LayerKind::Design)
    {
        ui.horizontal(|ui| {
            let mut vis = l.visible;
            if ui
                .checkbox(&mut vis, "")
                .on_hover_text(tr!("panels-visible"))
                .changed()
            {
                cmds.push(Command::ModifyLayer {
                    id: l.id,
                    name: None,
                    visible: Some(vis),
                    locked: None,
                    elevation: None,
                    scale: None,
                });
            }
            let mut locked = l.locked;
            if ui
                .toggle_value(&mut locked, tr!("panels-lock"))
                .on_hover_text(tr!("panels-lock-layer"))
                .changed()
            {
                cmds.push(Command::ModifyLayer {
                    id: l.id,
                    name: None,
                    visible: None,
                    locked: Some(locked),
                    elevation: None,
                    scale: None,
                });
            }
            if let Some((_, _, buf)) = nav
                .renaming
                .as_mut()
                .filter(|r| r.0 == RenameKind::Layer && r.1 == l.id.0)
            {
                let r = ui.text_edit_singleline(buf);
                r.request_focus();
                if r.lost_focus() {
                    if !buf.is_empty() {
                        cmds.push(Command::ModifyLayer {
                            id: l.id,
                            name: Some(buf.clone()),
                            visible: None,
                            locked: None,
                            elevation: None,
                            scale: None,
                        });
                    }
                    nav.renaming = None;
                }
            } else {
                let r = ui.selectable_label(l.id == doc.active_layer, &l.name);
                if r.clicked() {
                    cmds.push(Command::SetActive {
                        layer: Some(l.id),
                        class: None,
                    });
                }
                if r.double_clicked() {
                    nav.renaming = Some((RenameKind::Layer, l.id.0, l.name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button(tr!("panels-rename")).clicked() {
                        nav.renaming = Some((RenameKind::Layer, l.id.0, l.name.clone()));
                        ui.close();
                    }
                    if ui.button(tr!("panels-delete-if-empty")).clicked() {
                        cmds.push(Command::DeleteLayer { id: l.id });
                        ui.close();
                    }
                });
            }
        });
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong(tr!("panels-classes"));
        if ui
            .small_button("+")
            .on_hover_text(tr!("panels-add-class"))
            .clicked()
        {
            cmds.push(Command::AddClass {
                name: format!("Class-{}", doc.classes.len()),
                attrs: Attributes::default(),
            });
        }
    });
    for c in &doc.classes {
        ui.horizontal(|ui| {
            let mut vis = c.visible;
            if ui
                .checkbox(&mut vis, "")
                .on_hover_text(tr!("panels-visible"))
                .changed()
            {
                cmds.push(Command::ModifyClass {
                    id: c.id,
                    name: None,
                    visible: Some(vis),
                    attrs: None,
                });
            }
            if let Some((_, _, buf)) = nav
                .renaming
                .as_mut()
                .filter(|r| r.0 == RenameKind::Class && r.1 == c.id.0)
            {
                let r = ui.text_edit_singleline(buf);
                r.request_focus();
                if r.lost_focus() {
                    if !buf.is_empty() {
                        cmds.push(Command::ModifyClass {
                            id: c.id,
                            name: Some(buf.clone()),
                            visible: None,
                            attrs: None,
                        });
                    }
                    nav.renaming = None;
                }
            } else {
                let r = ui.selectable_label(c.id == doc.active_class, &c.name);
                if r.clicked() {
                    cmds.push(Command::SetActive {
                        layer: None,
                        class: Some(c.id),
                    });
                }
                if r.double_clicked() {
                    nav.renaming = Some((RenameKind::Class, c.id.0, c.name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button(tr!("panels-rename")).clicked() {
                        nav.renaming = Some((RenameKind::Class, c.id.0, c.name.clone()));
                        ui.close();
                    }
                });
            }
        });
    }
    for c in cmds {
        run_command(session, &c, status);
    }
    crate::sheets_ui::navigation_sections(ui, session, &mut nav.sheets, status);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::curve;

    fn rounded() -> Shape {
        let arc = |x: f64, y: f64| Vertex::new(DVec2::new(x, y), VertexKind::Arc { radius: 100.0 });
        Shape::Hardscape {
            boundary: vec![
                arc(0.0, 0.0),
                arc(1000.0, 0.0),
                arc(1000.0, 1000.0),
                arc(0.0, 1000.0),
            ],
            material: "m".into(),
            thickness: None,
            elevation: None,
        }
    }

    fn add(session: &mut Session, shape: Shape) -> ObjectId {
        let cmd = Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        };
        ObjectId(session.editor.execute(&cmd).unwrap().created[0])
    }

    #[test]
    fn a_profile_without_a_preferences_command_still_gets_a_way_to_it() {
        let mut p = Profile::vectorworks();
        assert!(!lacks_preferences(&p));
        for m in &mut p.menus {
            m.items.clear();
        }
        assert!(lacks_preferences(&p));
        p.menus.clear();
        assert!(lacks_preferences(&p));
        // The bar draws without a menu of the profile.
        let ctx = egui::Context::default();
        let state = MenuState {
            can_undo: false,
            can_redo: false,
            can_site: false,
            can_texture_bed: false,
            can_update_site: false,
        };
        let mut snap = kadakas_core::snapping::Settings::default();
        let (mut cues, mut theme, mut size, mut grid) = (
            true,
            crate::theme::ThemeChoice::System,
            IconSize::Medium,
            true,
        );
        let mut extras = MenuExtras {
            snap: &mut snap,
            cues: &mut cues,
            theme: &mut theme,
            icon_size: &mut size,
            layout_names: Vec::new(),
            can_back: false,
            can_forward: false,
            has_selection: false,
            collab_can_connect: true,
            collab_can_disconnect: false,
            requests: Vec::new(),
        };
        let _ = ctx.run_ui(Default::default(), |ui| {
            let out = menu_bar(ui, &Keymap::defaults(), &p, &state, &mut extras, &mut grid);
            assert!(out.is_empty());
        });
    }

    #[test]
    fn vertex_edits_become_set_vertex_commands() {
        let mut s = Session::default();
        let id = add(&mut s, rounded());
        let base = s
            .editor
            .doc()
            .object(id)
            .unwrap()
            .shape
            .vertices()
            .unwrap()
            .clone();
        assert!(vertex_edit_commands(id, &base, &base).is_empty());
        let mut edited = base.clone();
        edited[1].kind = VertexKind::Arc { radius: 250.0 };
        edited[2].p = DVec2::new(1100.0, 1000.0);
        edited[3].kind = VertexKind::Bezier;
        edited[3].p = DVec2::new(-10.0, 1000.0);
        let cmds = vertex_edit_commands(id, &base, &edited);
        assert_eq!(
            cmds,
            vec![
                Command::SetVertex {
                    id,
                    index: 1,
                    kind: Some(VertexKind::Arc { radius: 250.0 }),
                    p: None
                },
                Command::SetVertex {
                    id,
                    index: 2,
                    kind: None,
                    p: Some(DVec2::new(1100.0, 1000.0))
                },
                Command::SetVertex {
                    id,
                    index: 3,
                    kind: Some(VertexKind::Bezier),
                    p: Some(DVec2::new(-10.0, 1000.0))
                },
            ]
        );
        let mut status = String::new();
        assert!(run_command(&mut s, &Command::Batch { commands: cmds }, &mut status).is_some());
        assert_eq!(
            s.editor.doc().object(id).unwrap().shape.vertices().unwrap(),
            &edited
        );
        // One undo step brings all three back.
        assert!(s.editor.undo());
        assert_eq!(
            s.editor.doc().object(id).unwrap().shape.vertices().unwrap(),
            &base
        );
    }

    #[test]
    fn kind_labels() {
        assert_eq!(kind_label(VertexKind::Corner), "Corner");
        assert_eq!(kind_label(VertexKind::Arc { radius: 1.0 }), "Arc");
        assert_eq!(kind_label(VertexKind::PointOnArc), "Point on arc");
        assert_eq!(kind_label(VertexKind::Bezier), "B\u{e9}zier");
        assert_eq!(kind_label(VertexKind::Cubic), "Cubic");
        assert_eq!(KIND_CHOICES.len(), 5);
    }

    #[test]
    fn palettes_draw_for_a_curved_selection() {
        let ctx = egui::Context::default();
        let mut s = Session::default();
        let id = add(&mut s, rounded());
        let poly = add(
            &mut s,
            Shape::Polyline {
                points: vec![
                    Vertex::corner(DVec2::ZERO),
                    Vertex::new(DVec2::new(5.0, 5.0), VertexKind::Bezier),
                    Vertex::corner(DVec2::new(10.0, 0.0)),
                ],
                closed: false,
            },
        );
        let (mut edit, mut status) = (InfoEdit::default(), String::new());
        let mut settings = ToolSettings::default();
        let mut machine = ToolMachine::default();
        let keys = Keymap::defaults();
        for profile in Profile::builtins() {
            for sel in [id, poly] {
                s.selection = vec![sel];
                for tool in [Tool::Select, Tool::Polyline, Tool::Rotate, Tool::Dimension] {
                    machine.set_tool(tool);
                    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                        object_info(
                            ui,
                            &mut s,
                            &mut edit,
                            &mut crate::paving_ui::PavingUi::default(),
                            &mut status,
                        );
                        tool_panel(
                            ui,
                            &mut machine,
                            &keys,
                            &profile,
                            &mut settings,
                            &Default::default(),
                            &mut Default::default(),
                        );
                    });
                }
            }
        }
        assert!(
            curve::path(
                s.editor
                    .doc()
                    .object(poly)
                    .unwrap()
                    .shape
                    .vertices()
                    .unwrap(),
                false
            )
            .segs
            .len()
                == 1
        );
    }
}
