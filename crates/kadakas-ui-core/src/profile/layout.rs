//! The menu layout and the tool sets of a profile (PLAT-04).
//!
//! A menu item is a separator, a command of the window, or a tool. A tool set is an ordered list
//! of tools with a title. Both are plain data: the UI program draws them and turns a click into
//! an action.

use kadakas_l10n::{msg, translate};

use crate::keymap::Action;
use crate::tools::Tool;

macro_rules! commands {
    ($($variant:ident => $id:literal, $label:expr, $native:literal;)*) => {
        /// A command that a menu can hold. The id is the text in the profile file. Where the
        /// command has a key action, the id is the id of that action.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Cmd { $($variant),* }

        impl Cmd {
            pub const ALL: &'static [Cmd] = &[$(Cmd::$variant),*];
            pub fn id(self) -> &'static str { match self { $(Cmd::$variant => $id),* } }
            /// The message id of the label.
            pub fn label_id(self) -> &'static str { match self { $(Cmd::$variant => $label),* } }
            /// True for a command that the browser build lacks (files, scripts, converters).
            pub fn native_only(self) -> bool { match self { $(Cmd::$variant => $native),* } }
        }
    };
}

commands! {
    New => "file.new", msg!("panels-new"), false;
    Open => "file.open", msg!("panels-open"), false;
    Save => "file.save", msg!("panels-save"), false;
    SaveAs => "file.save_as", msg!("panels-save-as"), false;
    ExportSvg => "file.export_svg", msg!("panels-export-svg"), false;
    ExportPdf => "file.export_pdf", msg!("panels-export-pdf"), true;
    Import => "file.import", msg!("panels-import"), true;
    // A block: the submenu File > Import (Import..., Update from DXF/DWG...).
    ImportMenu => "file.import_menu", msg!("panels-import-menu"), true;
    // File > Import > Update from DXF/DWG...: a new revision of an imported drawing (IOP-23).
    UpdateFromDxf => "file.update_from_dxf", msg!("panels-update-from-dxf"), true;
    Export => "file.export", msg!("panels-export"), true;
    // File > BCF Issues...: read and write BCF files, the open issue format (REV-06).
    Bcf => "file.bcf", msg!("panels-bcf"), true;
    // Edit > New Issue from Selection (REV-05).
    NewIssue => "edit.new_issue", msg!("panels-new-issue"), false;
    // Edit > Delete Guides: all guides of the 3D modeler in one step; it asks above three (MOD-05).
    DeleteGuides => "edit.delete_guides", msg!("panels-delete-guides"), false;
    // Edit > Unhide All: everything that Hide hid in the 3D modeler (MOD-05).
    UnhideAll => "edit.unhide_all", msg!("panels-unhide-all"), false;
    MapSources => "file.map_sources", msg!("panels-map-sources"), true;
    ChangeCrs => "file.change_crs", msg!("panels-change-crs"), true;
    Scripts => "file.scripts", msg!("panels-scripts"), true;
    // The window that lists installed WebAssembly plug-ins: install, review, trust, remove.
    PlugIns => "file.plugins", msg!("panels-plug-ins"), false;
    // The window of named versions: save, restore, delete and compare (REV-01, REV-02).
    Versions => "file.versions", msg!("panels-versions"), false;
    // The window that compares two versions with a colored overlay in the plan and in 3D (REV-03).
    Compare => "file.compare", msg!("panels-compare"), false;
    // The window that compares two DXF or DWG files without importing them (IOP-24).
    // The Activity panel of the agent: its steps, what each changed, undo and revert (AGR-06).
    AgentActivity => "view.agent_activity", msg!("panels-agent-activity"), false;
    CompareDrawings => "file.compare_drawings", msg!("panels-compare-drawings"), true;
    // A block: the submenu File > Collaboration (Connect, Disconnect, People).
    Collaboration => "file.collaboration", msg!("panels-collaboration"), false;
    Preferences => "file.preferences", msg!("panels-preferences"), false;
    Undo => "edit.undo", msg!("panels-undo"), false;
    Redo => "edit.redo", msg!("panels-redo"), false;
    Delete => "edit.delete", msg!("panels-delete"), false;
    SelectAll => "edit.select_all", msg!("panels-select-all"), false;
    QuickSearch => "ui.quick_search", msg!("panels-quick-search"), false;
    DuplicateArray => "cmd.duplicate_array", msg!("panels-duplicate-array"), false;
    RotateLeft90 => "cmd.rotate_left_90", msg!("panels-rotate-left-90"), false;
    RotateRight90 => "cmd.rotate_right_90", msg!("panels-rotate-right-90"), false;
    FlipHorizontal => "cmd.flip_horizontal", msg!("panels-flip-horizontal"), false;
    FlipVertical => "cmd.flip_vertical", msg!("panels-flip-vertical"), false;
    AddSurface => "cmd.add_surface", msg!("panels-add-surface"), false;
    SubtractSurface => "cmd.subtract_surface", msg!("panels-subtract-surface"), false;
    ClipSurface => "cmd.clip_surface", msg!("panels-clip-surface"), false;
    IntersectSurface => "cmd.intersect_surface", msg!("panels-intersect-surface"), false;
    Compose => "cmd.compose", msg!("panels-compose"), false;
    Extrude => "cmd.extrude", msg!("panels-extrude"), false;
    Sweep => "cmd.sweep", msg!("panels-sweep"), false;
    // Landscape > Create Objects from Shapes: loose lines become hardscape areas (PAT-06).
    CreateFromShapes => "cmd.create_from_shapes", msg!("panels-create-objects-from-shapes"), false;
    // Landscape > Split Hardscape into Zones: split lines make zones with their own pattern (PAT-07).
    SplitHardscape => "cmd.split_hardscape", msg!("panels-split-hardscape-into-zones"), false;
    // Landscape > Check Unit Patterns: the findings of the pattern check (PAT-10).
    CheckUnitPatterns => "cmd.check_unit_patterns", msg!("panels-check-unit-patterns"), false;
    CreateSiteModel => "site.create_model", msg!("panels-create-site-model-from-selection"), false;
    UpdateSiteModel => "site.update_model", msg!("panels-update-site-model"), false;
    CreateTextureBed => "site.create_texture_bed", msg!("panels-create-texture-bed-from-selection"), false;
    ZoomFit => "view.zoom_fit", msg!("panels-zoom-to-fit"), false;
    PreviousView => "view.history_previous", msg!("panels-previous-view"), false;
    NextView => "view.history_next", msg!("panels-next-view"), false;
    ShowGrid => "view.show_grid", msg!("panels-show-grid"), false;
    Snapping => "view.snapping", msg!("panels-snapping"), false;
    Theme => "view.theme", msg!("panels-theme"), false;
    IconSize => "view.icon_size", msg!("panels-icon-size"), false;
    // The next two are blocks, not single items: the list of palettes to show, and the
    // commands that save, reset and load window layouts.
    WindowPanels => "window.panels", msg!("panels-window"), false;
    WindowLayouts => "window.layouts", msg!("panels-saved-layouts"), false;
    About => "help.about", msg!("panels-about-kadakas"), false;
    // Feedback (API-47, UI-14): one short form for a bug report or a feature request, and the
    // window with the outbox.
    ReportBug => "help.report_bug", msg!("panels-report-a-bug"), false;
    RequestFeature => "help.request_feature", msg!("panels-request-a-feature"), false;
    Feedback => "help.feedback", msg!("panels-feedback"), false;
    // Help > Show welcome: the welcome dialog of the launcher again (PLAT-26).
    ShowWelcome => "help.show_welcome", msg!("panels-show-welcome"), true;
}

impl Cmd {
    pub fn from_id(id: &str) -> Option<Cmd> {
        Cmd::ALL.iter().copied().find(|c| c.id() == id)
    }

    /// The label in the language of the UI.
    pub fn label(self) -> String {
        translate(self.label_id(), &[])
    }

    /// The key action that does the same, for the shortcut text of the menu item.
    pub fn action(self) -> Option<Action> {
        Some(match self {
            Cmd::New => Action::FileNew,
            Cmd::Open => Action::FileOpen,
            Cmd::Save => Action::FileSave,
            Cmd::SaveAs => Action::FileSaveAs,
            Cmd::ExportPdf => Action::FileExportPdf,
            Cmd::Undo => Action::Undo,
            Cmd::Redo => Action::Redo,
            Cmd::Delete => Action::Delete,
            Cmd::SelectAll => Action::SelectAll,
            Cmd::QuickSearch => Action::QuickSearch,
            Cmd::DuplicateArray => Action::CmdDuplicateArray,
            Cmd::RotateLeft90 => Action::CmdRotateLeft90,
            Cmd::RotateRight90 => Action::CmdRotateRight90,
            Cmd::FlipHorizontal => Action::CmdFlipHorizontal,
            Cmd::FlipVertical => Action::CmdFlipVertical,
            Cmd::AddSurface => Action::CmdAddSurface,
            Cmd::SubtractSurface => Action::CmdSubtractSurface,
            Cmd::ClipSurface => Action::CmdClipSurface,
            Cmd::IntersectSurface => Action::CmdIntersectSurface,
            Cmd::Compose => Action::CmdCompose,
            Cmd::Extrude => Action::CmdExtrude,
            Cmd::Sweep => Action::CmdSweep,
            Cmd::AgentActivity => Action::ViewAgentActivity,
            Cmd::CreateFromShapes => Action::CmdCreateFromShapes,
            Cmd::SplitHardscape => Action::CmdSplitHardscape,
            Cmd::CheckUnitPatterns => Action::CmdCheckUnitPatterns,
            Cmd::ZoomFit => Action::ZoomFit,
            Cmd::PreviousView => Action::ViewHistoryPrevious,
            Cmd::NextView => Action::ViewHistoryNext,
            _ => return None,
        })
    }
}

/// One entry of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Separator,
    Command(Cmd),
    /// A tool: choosing the item selects the tool.
    Tool(Tool),
}

/// The text of a separator in the profile file.
pub const SEPARATOR: &str = "-";

impl Item {
    pub fn parse(text: &str) -> Option<Item> {
        if text == SEPARATOR {
            return Some(Item::Separator);
        }
        if let Some(c) = Cmd::from_id(text) {
            return Some(Item::Command(c));
        }
        Action::from_id(text).and_then(Action::tool).map(Item::Tool)
    }

    pub fn id(self) -> &'static str {
        match self {
            Item::Separator => SEPARATOR,
            Item::Command(c) => c.id(),
            Item::Tool(t) => Action::for_tool(t).id(),
        }
    }
}

/// True for ids that are safe in a file name and in a message: lower case letters, digits,
/// `-` and `_`, one to 24 characters.
pub fn valid_id(id: &str) -> bool {
    (1..=24).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The title of a part of the layout that has a known id, as a message id.
fn known_title(id: &str) -> Option<&'static str> {
    Some(match id {
        "file" => msg!("panels-file"),
        "edit" => msg!("panels-edit"),
        "modify" => msg!("panels-modify"),
        "site" => msg!("panels-site"),
        "model" => msg!("panels-model"),
        "3d" => msg!("panels-3d-modeling"),
        "view" => msg!("panels-view"),
        "window" => msg!("panels-window"),
        "help" => msg!("panels-help"),
        "draw" => msg!("uic-profile-draw"),
        "annotate" => msg!("uic-profile-annotate"),
        "basic" => msg!("panels-basic"),
        "landscape" => msg!("panels-landscape"),
        "irrigation" => msg!("panels-irrigation"),
        "annotation" => msg!("panels-annotation"),
        _ => return None,
    })
}

/// True when `id` has a title in the program, so a layout part needs no name of its own.
pub fn has_known_title(id: &str) -> bool {
    known_title(id).is_some()
}

fn title_of(id: &str, name: &Option<String>) -> String {
    match (name, known_title(id)) {
        (Some(n), _) => n.clone(),
        (None, Some(m)) => translate(m, &[]),
        (None, None) => id.to_string(),
    }
}

/// A menu of the menu bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub id: String,
    /// A name chosen by the person. It is not translated. Without it, the title comes from the
    /// id.
    pub name: Option<String>,
    pub items: Vec<Item>,
}

impl Menu {
    pub fn title(&self) -> String {
        title_of(&self.id, &self.name)
    }
}

/// A group of tools in the Tool bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSet {
    pub id: String,
    pub name: Option<String>,
    pub tools: Vec<Tool>,
}

impl ToolSet {
    pub fn title(&self) -> String {
        title_of(&self.id, &self.name)
    }

    /// Moves the tool at `from` to the place `to` (both are indexes in the list now). False
    /// when an index is out of range.
    pub fn move_tool(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tools.len() || to >= self.tools.len() {
            return false;
        }
        let t = self.tools.remove(from);
        self.tools.insert(to, t);
        true
    }
}

fn set(id: &str, tools: &[Tool]) -> ToolSet {
    ToolSet {
        id: id.to_string(),
        name: None,
        tools: tools.to_vec(),
    }
}

/// The tool sets of the Vectorworks-like profile, in the order of the Tool bar.
pub fn default_tool_sets() -> Vec<ToolSet> {
    vec![
        set("basic", &Tool::BASIC),
        set("modify", &Tool::MODIFY),
        set("landscape", &Tool::LANDSCAPE),
        set("site", &Tool::SITE),
        set("3d", &Tool::MODEL),
        set("irrigation", &Tool::IRRIGATION),
        set("annotation", &Tool::ANNOTATION),
    ]
}

fn menu(id: &str, items: &[&str]) -> Menu {
    Menu {
        id: id.to_string(),
        name: None,
        items: items
            .iter()
            .map(|i| Item::parse(i).expect("a known item"))
            .collect(),
    }
}

/// The menus of the Vectorworks-like profile.
pub fn default_menus() -> Vec<Menu> {
    vec![
        menu(
            "file",
            &[
                "file.new",
                "file.open",
                "file.save",
                "file.save_as",
                "file.export_svg",
                "file.export_pdf",
                "-",
                "file.import_menu",
                "file.export",
                "file.bcf",
                "-",
                "file.map_sources",
                "file.change_crs",
                "-",
                "file.scripts",
                "file.plugins",
                "file.versions",
                "file.compare",
                "view.agent_activity",
                "file.compare_drawings",
                "-",
                "file.collaboration",
                "-",
                "file.preferences",
            ],
        ),
        menu(
            "edit",
            &[
                "edit.undo",
                "edit.redo",
                "-",
                "edit.delete",
                "edit.select_all",
                "-",
                "edit.new_issue",
                "-",
                "edit.unhide_all",
                "edit.delete_guides",
                "-",
                "ui.quick_search",
            ],
        ),
        menu(
            "modify",
            &[
                "cmd.create_from_shapes",
                "-",
                "cmd.duplicate_array",
                "-",
                "cmd.rotate_left_90",
                "cmd.rotate_right_90",
                "cmd.flip_horizontal",
                "cmd.flip_vertical",
                "-",
                "cmd.add_surface",
                "cmd.subtract_surface",
                "cmd.clip_surface",
                "cmd.intersect_surface",
                "cmd.compose",
            ],
        ),
        menu(
            "model",
            &["cmd.extrude", "cmd.sweep", "-", "tool.polygon3d"],
        ),
        menu(
            "landscape",
            &[
                "cmd.create_from_shapes",
                "cmd.split_hardscape",
                "-",
                "cmd.check_unit_patterns",
            ],
        ),
        menu(
            "site",
            &[
                "site.create_model",
                "site.update_model",
                "site.create_texture_bed",
            ],
        ),
        menu(
            "view",
            &[
                "view.zoom_fit",
                "view.history_previous",
                "view.history_next",
                "-",
                "view.show_grid",
                "view.snapping",
                "view.theme",
                "view.icon_size",
            ],
        ),
        menu("window", &["window.panels", "-", "window.layouts"]),
        menu(
            "help",
            &[
                "help.report_bug",
                "help.request_feature",
                "help.feedback",
                "help.show_welcome",
                "-",
                "help.about",
            ],
        ),
    ]
}

/// The items of a menu without the ones that this program cannot run (`native` false removes
/// the native-only commands), with separators cleaned: none at the start or at the end, and no
/// two in a row.
pub fn visible_items(items: &[Item], native: bool) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for item in items {
        match item {
            Item::Command(c) if c.native_only() && !native => continue,
            Item::Separator if matches!(out.last(), None | Some(Item::Separator)) => continue,
            _ => out.push(*item),
        }
    }
    while out.last() == Some(&Item::Separator) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_ids_are_unique_and_match_their_actions() {
        let mut seen = std::collections::HashSet::new();
        for c in Cmd::ALL {
            assert!(seen.insert(c.id()), "{} twice", c.id());
            assert_eq!(Cmd::from_id(c.id()), Some(*c));
            assert!(!c.label().is_empty() && c.label() != c.label_id());
            if let Some(a) = c.action() {
                assert_eq!(a.id(), c.id());
            }
        }
        assert_eq!(Cmd::from_id("nope"), None);
        assert!(Cmd::Import.native_only() && !Cmd::Save.native_only());
    }

    #[test]
    fn items_parse_and_print() {
        assert_eq!(Item::parse("-"), Some(Item::Separator));
        assert_eq!(Item::parse("file.new"), Some(Item::Command(Cmd::New)));
        assert_eq!(Item::parse("tool.line"), Some(Item::Tool(Tool::Line)));
        assert_eq!(Item::parse("edit.cancel"), None);
        assert_eq!(Item::parse("nope"), None);
        for item in [
            Item::Separator,
            Item::Command(Cmd::Undo),
            Item::Tool(Tool::Fillet),
        ] {
            assert_eq!(Item::parse(item.id()), Some(item));
        }
    }

    #[test]
    fn ids_are_checked() {
        assert!(valid_id("draw") && valid_id("my-set_2"));
        assert!(!valid_id("") && !valid_id("Draw") && !valid_id("a b"));
        assert!(!valid_id(&"x".repeat(25)));
        assert!(has_known_title("file") && !has_known_title("mine"));
    }

    #[test]
    fn titles_come_from_the_id_or_the_name() {
        let m = menu("file", &["file.new"]);
        assert_eq!(m.title(), "File");
        let m = Menu {
            name: Some("Mine".into()),
            ..m
        };
        assert_eq!(m.title(), "Mine");
        let s = ToolSet {
            id: "zzz".into(),
            name: None,
            tools: vec![],
        };
        assert_eq!(s.title(), "zzz");
        assert_eq!(default_tool_sets()[0].title(), "Basic");
        for id in [
            "file",
            "edit",
            "modify",
            "site",
            "view",
            "window",
            "help",
            "draw",
            "annotate",
            "basic",
            "landscape",
            "irrigation",
            "annotation",
        ] {
            assert_ne!(title_of(id, &None), id, "{id} has a title in the language");
        }
        kadakas_l10n::with_language("et", || {
            assert_eq!(menu("file", &[]).title(), "Fail");
        });
    }

    #[test]
    fn the_default_layout_has_every_tool_once() {
        let sets = default_tool_sets();
        let mut all: Vec<Tool> = sets.iter().flat_map(|s| s.tools.clone()).collect();
        assert_eq!(all.len(), 29);
        all.dedup();
        assert_eq!(all.len(), 29);
        assert_eq!(sets[0].tools, Tool::BASIC);
        assert_eq!(sets[6].tools, Tool::ANNOTATION);
    }

    #[test]
    fn tools_move_inside_a_set() {
        let mut s = set("x", &[Tool::Line, Tool::Rect, Tool::Circle]);
        assert!(s.move_tool(0, 2));
        assert_eq!(s.tools, [Tool::Rect, Tool::Circle, Tool::Line]);
        assert!(s.move_tool(2, 0));
        assert_eq!(s.tools, [Tool::Line, Tool::Rect, Tool::Circle]);
        assert!(!s.move_tool(3, 0) && !s.move_tool(0, 3));
        assert_eq!(s.tools, [Tool::Line, Tool::Rect, Tool::Circle]);
    }

    #[test]
    fn the_default_menus_hold_known_items_and_every_command_once() {
        let menus = default_menus();
        let mut cmds: Vec<Cmd> = menus
            .iter()
            .flat_map(|m| m.items.iter())
            .filter_map(|i| match i {
                Item::Command(c) => Some(*c),
                _ => None,
            })
            .collect();
        cmds.sort();
        // Create Objects from Shapes is in Modify (as in Vectorworks) and in Landscape.
        cmds.dedup();
        // Import... and Update from DXF/DWG... are in the block File > Import.
        let mut want: Vec<Cmd> = Cmd::ALL
            .iter()
            .copied()
            .filter(|c| !matches!(c, Cmd::Import | Cmd::UpdateFromDxf))
            .collect();
        want.sort();
        assert_eq!(cmds, want);
        assert_eq!(menus.len(), 9);
    }

    #[test]
    fn separators_are_cleaned_when_commands_are_hidden() {
        use Item::*;
        let items = [
            Separator,
            Command(Cmd::New),
            Separator,
            Command(Cmd::Import),
            Separator,
            Command(Cmd::Export),
            Separator,
            Command(Cmd::Preferences),
            Separator,
        ];
        assert_eq!(
            visible_items(&items, true),
            vec![
                Command(Cmd::New),
                Separator,
                Command(Cmd::Import),
                Separator,
                Command(Cmd::Export),
                Separator,
                Command(Cmd::Preferences)
            ]
        );
        assert_eq!(
            visible_items(&items, false),
            vec![Command(Cmd::New), Separator, Command(Cmd::Preferences)]
        );
        assert!(visible_items(&[Separator, Separator], true).is_empty());
    }
}
