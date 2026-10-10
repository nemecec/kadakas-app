//! Keyboard shortcuts. Every key a UI program reacts to goes through this table.
//!
//! The defaults follow the published Vectorworks 2026 defaults (see
//! `docs/keyboard-shortcuts.md`). A JSON file in the per-user config
//! directory can override them: `{"tool.line": "Alt+2", "edit.redo": ["Cmd+Y"]}`.
//! Keys are action ids; values are one chord or a list of chords. An empty
//! list unbinds the action.

use std::collections::{BTreeMap, HashMap};

use crate::keys::{Key, Modifiers};
use kadakas_render::StandardView;

use crate::tools::Tool;

/// One key plus modifiers. `command` is Cmd on macOS and Ctrl elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub key: Key,
    /// Cmd on macOS, Ctrl elsewhere.
    pub command: bool,
    /// The real Control key on macOS. Always false elsewhere.
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    /// Parses text like `Cmd+Shift+Z`, `Alt+2`, `Tab`, `Backtick`, `[`.
    /// `Cmd` and `Cmd/Ctrl` mean the command key on every system. `Ctrl` means the command key
    /// on Windows and Linux, and the real Control key on macOS.
    pub fn parse(text: &str) -> Result<Chord, String> {
        Chord::parse_for(text, cfg!(target_os = "macos"))
    }

    pub fn parse_for(text: &str, mac: bool) -> Result<Chord, String> {
        let text = text.trim();
        let (mut command, mut ctrl, mut shift, mut alt) = (false, false, false, false);
        // The key is the last part; a trailing "+" is the Plus key.
        let (mods, key) = match text.rfind('+') {
            Some(i) if i + 1 < text.len() => (&text[..i], &text[i + 1..]),
            Some(i) if i > 0 && text.as_bytes()[i - 1] == b'+' => (&text[..i - 1], "+"),
            _ => ("", text),
        };
        if !mods.is_empty() {
            for m in mods.split('+') {
                match m.trim().to_ascii_lowercase().as_str() {
                    "cmd" | "cmd/ctrl" | "command" => command = true,
                    "ctrl" | "control" if mac => ctrl = true,
                    "ctrl" | "control" => command = true,
                    "shift" => shift = true,
                    "alt" | "option" | "opt" => alt = true,
                    other => return Err(format!("unknown modifier '{other}' in '{text}'")),
                }
            }
        }
        let key = match key {
            "Backtick" | "Backquote" => Some(Key::Backtick),
            "Plus" => Some(Key::Plus),
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphabetic() => {
                Key::from_name(&k.to_ascii_uppercase())
            }
            k => Key::from_name(k),
        };
        key.map(|key| Chord {
            key,
            command,
            ctrl,
            shift,
            alt,
        })
        .ok_or_else(|| format!("unknown key in '{text}'"))
    }

    /// The chord of a key event.
    pub fn from_event(key: Key, m: Modifiers) -> Chord {
        Chord::from_event_for(key, m, cfg!(target_os = "macos"))
    }

    pub fn from_event_for(key: Key, m: Modifiers, mac: bool) -> Chord {
        // Off macOS, `command` is the Ctrl key.
        Chord {
            key,
            command: m.command,
            ctrl: mac && m.ctrl,
            shift: m.shift,
            alt: m.alt,
        }
    }

    /// Text that `Chord::parse` reads back to the same chord, on every system.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        if self.command {
            s += "Cmd/Ctrl+";
        }
        if self.ctrl {
            s += "Ctrl+";
        }
        if self.alt {
            s += "Alt+";
        }
        if self.shift {
            s += "Shift+";
        }
        s + self.key.name()
    }

    /// Text for tooltips and menus.
    pub fn display(&self, mac: bool) -> String {
        let mut s = String::new();
        if self.command {
            s += if mac { "Cmd+" } else { "Ctrl+" };
        }
        if self.ctrl {
            s += "Ctrl+";
        }
        if self.alt {
            s += if mac { "Option+" } else { "Alt+" };
        }
        if self.shift {
            s += "Shift+";
        }
        s += match self.key {
            Key::Backtick => "`",
            Key::Escape => "Esc",
            k => {
                let n = k.name();
                n.strip_prefix("Num").filter(|d| d.len() == 1).unwrap_or(n)
            }
        };
        s
    }
}

macro_rules! actions {
    ($($variant:ident => $id:literal, $label:literal, [$($chord:literal),*], $impl_:literal;)*) => {
        /// Everything a key can do.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action { $($variant),* }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$variant),*];
            pub fn id(self) -> &'static str { match self { $(Action::$variant => $id),* } }
            pub fn label(self) -> &'static str { match self { $(Action::$variant => $label),* } }
            /// Default chords, as text.
            pub fn defaults(self) -> &'static [&'static str] { match self { $(Action::$variant => &[$($chord),*]),* } }
            /// False for actions that are bound but not built yet.
            pub fn implemented(self) -> bool { match self { $(Action::$variant => $impl_),* } }
        }
    };
}

actions! {
    ToolSelect => "tool.select", "Selection tool", ["X"], true;
    ToolText => "tool.text", "Text tool", ["1"], true;
    ToolLine => "tool.line", "Line tool", ["2"], true;
    ToolRect => "tool.rect", "Rectangle tool", ["4"], true;
    ToolCircle => "tool.circle", "Circle tool", ["6"], true;
    ToolPolyline => "tool.polyline", "Polyline tool", ["5"], true;
    ToolPlant => "tool.plant", "Plant tool", ["Alt+Shift+P"], true;
    ToolLandscapeArea => "tool.landscape_area", "Landscape Area tool", ["Alt+Shift+A"], true;
    ToolHardscape => "tool.hardscape", "Hardscape tool", ["Alt+Shift+H"], true;
    ToolSiteModifier => "tool.site_modifier", "Site Modifiers tool", ["Alt+Shift+G"], true;
    ToolMassing => "tool.massing", "Massing Model tool", ["Alt+Shift+M"], true;
    ToolIrrigation => "tool.irrigation", "Irrigation tool", ["Alt+Shift+I"], true;
    ToolGrading => "tool.grading", "Grading tool", [], true;
    ToolDimension => "tool.dimension", "Dimension tool (constrained linear)", ["N"], true;
    ToolAngularDim => "tool.angular_dimension", "Angular Dimension tool", ["."], true;
    ToolRadialDim => "tool.radial_dimension", "Radial Dimension tool", [","], true;
    ToolCallout => "tool.callout", "Callout tool", ["Alt+1"], true;
    ToolDataTag => "tool.data_tag", "Data Tag tool", ["Alt+Shift+T"], true;
    ToolNorthArrow => "tool.north_arrow", "North Arrow tool", ["Alt+Shift+N"], true;
    ToolScaleBar => "tool.scale_bar", "Scale Bar tool", ["Alt+Shift+B"], true;
    ToolRevisionCloud => "tool.revision_cloud", "Revision Cloud tool", ["Alt+Shift+V"], true;
    CmdGraphicLegend => "cmd.graphic_legend", "Add a graphic legend", ["Alt+Shift+E"], true;
    ToolRotate => "tool.rotate", "Rotate tool", ["Alt+="], true;
    ToolMirror => "tool.mirror", "Mirror tool", ["="], true;
    ToolScale => "tool.scale", "Scale tool", [], true;
    ToolOffset => "tool.offset", "Offset tool", ["Shift+-"], true;
    ToolFillet => "tool.fillet", "Fillet tool", ["7"], true;
    ToolChamfer => "tool.chamfer", "Chamfer tool", ["Alt+7"], true;
    ToolTrim => "tool.trim", "Trim tool", ["Alt+Shift+L"], true;
    ToolExtend => "tool.extend", "Extend tool", [], true;
    ToolSplit => "tool.split", "Split tool", ["L"], true;
    CmdDuplicateArray => "cmd.duplicate_array", "Duplicate Array", ["Cmd/Ctrl+Shift+Alt+D"], true;
    CmdAddSurface => "cmd.add_surface", "Add Surface", ["Cmd/Ctrl+Shift+Alt+A"], true;
    CmdSubtractSurface => "cmd.subtract_surface", "Subtract Surface", [], true;
    CmdClipSurface => "cmd.clip_surface", "Clip Surface", ["Cmd/Ctrl+Shift+Alt+C"], true;
    CmdIntersectSurface => "cmd.intersect_surface", "Intersect Surface", ["Cmd/Ctrl+Shift+Alt+I"], true;
    CmdCompose => "cmd.compose", "Compose", ["Cmd/Ctrl+Shift+Alt+P"], true;
    CmdRotateLeft90 => "cmd.rotate_left_90", "Rotate left 90 degrees", ["Cmd/Ctrl+L"], true;
    CmdRotateRight90 => "cmd.rotate_right_90", "Rotate right 90 degrees", ["Cmd/Ctrl+Shift+R"], true;
    CmdFlipHorizontal => "cmd.flip_horizontal", "Flip horizontal", ["Cmd/Ctrl+Shift+H"], true;
    CmdFlipVertical => "cmd.flip_vertical", "Flip vertical", ["Cmd/Ctrl+Shift+V"], true;
    QuickSearch => "ui.quick_search", "Quick search for commands", ["F"], true;
    ViewHistoryPrevious => "view.history_previous", "Previous view in history", ["Cmd/Ctrl+Shift+,"], true;
    ViewHistoryNext => "view.history_next", "Next view in history", ["Cmd/Ctrl+Shift+."], true;
    SnapCues => "snap.cues", "Toggle SmartCursor cues", ["Y"], true;
    SnapRelativeAngles => "snap.relative_angles", "Toggle angles relative to the prior segment", ["/"], true;
    SnapAcquire => "snap.acquire", "Set a smart point or smart edge at the pointer", ["T"], true;
    ToolMode1 => "mode.group1", "Tool mode group 1", ["U"], true;
    ToolMode2 => "mode.group2", "Tool mode group 2", ["I"], true;
    ToolMode3 => "mode.group3", "Tool mode group 3", ["O"], true;
    ToolMode4 => "mode.group4", "Tool mode group 4", ["P"], true;
    ToolMode5 => "mode.group5", "Tool mode group 5", ["["], true;
    ToolMode6 => "mode.group6", "Tool mode group 6", ["]"], true;
    FileNew => "file.new", "New", ["Cmd/Ctrl+N"], true;
    FileOpen => "file.open", "Open", ["Cmd/Ctrl+O"], true;
    FileSave => "file.save", "Save", ["Cmd/Ctrl+S"], true;
    FileSaveAs => "file.save_as", "Save As", ["Cmd/Ctrl+Shift+S"], true;
    FileExportPdf => "file.export_pdf", "Export PDF (print)", ["Cmd/Ctrl+P"], true;
    Undo => "edit.undo", "Undo", ["Cmd/Ctrl+Z"], true;
    Redo => "edit.redo", "Redo", ["Cmd/Ctrl+Y", "Cmd/Ctrl+Shift+Z"], true;
    Delete => "edit.delete", "Delete", ["Delete", "Backspace"], true;
    Group => "edit.group", "Group", ["Cmd/Ctrl+G"], true;
    Ungroup => "edit.ungroup", "Ungroup", ["Cmd/Ctrl+U"], true;
    EnterGroup => "edit.enter_group", "Edit group (enter)", ["Cmd/Ctrl+["], true;
    ExitGroup => "edit.exit_group", "Exit group", ["Cmd/Ctrl+]"], true;
    SelectAll => "edit.select_all", "Select All", ["Cmd/Ctrl+A"], true;
    Cancel => "edit.cancel", "Cancel the operation", ["Escape"], true;
    Finish => "edit.finish", "Finish the object", ["Enter"], true;
    NudgeLeft => "edit.nudge_left", "Nudge left", ["Shift+Left"], true;
    NudgeRight => "edit.nudge_right", "Nudge right", ["Shift+Right"], true;
    NudgeUp => "edit.nudge_up", "Nudge up", ["Shift+Up"], true;
    NudgeDown => "edit.nudge_down", "Nudge down", ["Shift+Down"], true;
    ZoomFit => "view.zoom_fit", "Zoom to Fit", ["Cmd/Ctrl+6", "Home"], true;
    ZoomIn => "view.zoom_in", "Zoom in (x2)", ["Cmd/Ctrl+1"], true;
    ZoomOut => "view.zoom_out", "Zoom out (x2)", ["Cmd/Ctrl+2"], true;
    PanLeft => "view.pan_left", "Pan left", ["Left"], true;
    PanRight => "view.pan_right", "Pan right", ["Right"], true;
    PanUp => "view.pan_up", "Pan up", ["Up"], true;
    PanDown => "view.pan_down", "Pan down", ["Down"], true;
    PanHold => "view.pan_hold", "Pan while held", ["Space"], true;
    SnapSuspend => "snap.suspend", "Suspend snapping while held", ["Backtick"], true;
    DataBarFocus => "databar.focus", "Data bar: next field", ["Tab"], true;
    DataBarPrev => "databar.prev", "Data bar: previous field", ["Shift+Tab"], true;
    ViewPlan => "view.top_plan", "Top/Plan view", ["Alt+Shift+0", "Cmd/Ctrl+5"], true;
    ViewTop => "view.top", "Top view", ["Alt+Shift+5"], true;
    ViewFront => "view.front", "Front view", ["Alt+Shift+2"], true;
    ViewBack => "view.back", "Back view", ["Alt+Shift+8"], true;
    ViewLeft => "view.left", "Left view", ["Alt+Shift+4"], true;
    ViewRight => "view.right", "Right view", ["Alt+Shift+6"], true;
    ViewLeftIso => "view.left_isometric", "Left Isometric view", ["Alt+Shift+1"], true;
    ViewRightIso => "view.right_isometric", "Right Isometric view", ["Alt+Shift+3"], true;
    ViewLeftRearIso => "view.left_rear_isometric", "Left Rear Isometric view", ["Alt+Shift+7"], true;
    ViewRightRearIso => "view.right_rear_isometric", "Right Rear Isometric view", ["Alt+Shift+9"], true;
    ViewFlyover => "view.flyover", "Flyover (orbit in 3D)", ["Shift+C"], true;
    ViewProjection => "view.projection", "Toggle parallel and perspective projection", ["Alt+Shift+J"], true;
    ViewWireframe => "view.wireframe", "Wireframe rendering", ["Cmd/Ctrl+Shift+W"], true;
    ViewShaded => "view.shaded", "Shaded rendering", ["Cmd/Ctrl+Shift+G"], true;
    ViewHiddenLine => "view.hidden_line", "Hidden Line rendering", ["Cmd/Ctrl+Shift+E"], true;
    ViewDashedHiddenLine => "view.dashed_hidden_line", "Dashed Hidden Line rendering", ["Cmd/Ctrl+Shift+D"], true;
    ViewRealistic => "view.realistic", "Realistic rendering (textures, sun and shadows)", ["Cmd/Ctrl+Shift+F"], true;
}

impl Action {
    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.id() == id)
    }

    /// The tool that this action selects.
    pub fn tool(self) -> Option<Tool> {
        Some(match self {
            Action::ToolSelect => Tool::Select,
            Action::ToolText => Tool::Text,
            Action::ToolLine => Tool::Line,
            Action::ToolRect => Tool::Rect,
            Action::ToolCircle => Tool::Circle,
            Action::ToolPolyline => Tool::Polyline,
            Action::ToolPlant => Tool::Plant,
            Action::ToolLandscapeArea => Tool::LandscapeArea,
            Action::ToolHardscape => Tool::Hardscape,
            Action::ToolSiteModifier => Tool::SiteModifier,
            Action::ToolMassing => Tool::Massing,
            Action::ToolIrrigation => Tool::Irrigation,
            Action::ToolGrading => Tool::Grading,
            Action::ToolDimension | Action::ToolAngularDim | Action::ToolRadialDim => {
                Tool::Dimension
            }
            Action::ToolCallout => Tool::Callout,
            Action::ToolDataTag => Tool::DataTag,
            Action::ToolNorthArrow => Tool::NorthArrow,
            Action::ToolScaleBar => Tool::ScaleBar,
            Action::ToolRevisionCloud => Tool::RevisionCloud,
            Action::ToolRotate => Tool::Rotate,
            Action::ToolMirror => Tool::Mirror,
            Action::ToolScale => Tool::Scale,
            Action::ToolOffset => Tool::Offset,
            Action::ToolFillet => Tool::Fillet,
            Action::ToolChamfer => Tool::Chamfer,
            Action::ToolTrim => Tool::Trim,
            Action::ToolExtend => Tool::Extend,
            Action::ToolSplit => Tool::Split,
            _ => return None,
        })
    }

    pub fn for_tool(tool: Tool) -> Action {
        match tool {
            Tool::Select => Action::ToolSelect,
            Tool::Text => Action::ToolText,
            Tool::Line => Action::ToolLine,
            Tool::Rect => Action::ToolRect,
            Tool::Circle => Action::ToolCircle,
            Tool::Polyline => Action::ToolPolyline,
            Tool::Plant => Action::ToolPlant,
            Tool::LandscapeArea => Action::ToolLandscapeArea,
            Tool::Hardscape => Action::ToolHardscape,
            Tool::SiteModifier => Action::ToolSiteModifier,
            Tool::Massing => Action::ToolMassing,
            Tool::Irrigation => Action::ToolIrrigation,
            Tool::Grading => Action::ToolGrading,
            Tool::Dimension => Action::ToolDimension,
            Tool::Callout => Action::ToolCallout,
            Tool::DataTag => Action::ToolDataTag,
            Tool::NorthArrow => Action::ToolNorthArrow,
            Tool::ScaleBar => Action::ToolScaleBar,
            Tool::RevisionCloud => Action::ToolRevisionCloud,
            Tool::Rotate => Action::ToolRotate,
            Tool::Mirror => Action::ToolMirror,
            Tool::Scale => Action::ToolScale,
            Tool::Offset => Action::ToolOffset,
            Tool::Fillet => Action::ToolFillet,
            Tool::Chamfer => Action::ToolChamfer,
            Tool::Trim => Action::ToolTrim,
            Tool::Extend => Action::ToolExtend,
            Tool::Split => Action::ToolSplit,
        }
    }

    /// The standard view that this action shows.
    pub fn standard_view(self) -> Option<StandardView> {
        Some(match self {
            Action::ViewPlan => StandardView::Plan,
            Action::ViewTop => StandardView::Top,
            Action::ViewFront => StandardView::Front,
            Action::ViewBack => StandardView::Back,
            Action::ViewLeft => StandardView::Left,
            Action::ViewRight => StandardView::Right,
            Action::ViewLeftIso => StandardView::LeftIsometric,
            Action::ViewRightIso => StandardView::RightIsometric,
            Action::ViewLeftRearIso => StandardView::LeftRearIsometric,
            Action::ViewRightRearIso => StandardView::RightRearIsometric,
            _ => return None,
        })
    }

    /// The action that shows `view`. Bottom has no action: Vectorworks gives it no key.
    pub fn for_view(view: StandardView) -> Option<Action> {
        Action::ALL
            .iter()
            .copied()
            .find(|a| a.standard_view() == Some(view))
    }

    /// Keys that repeat while held.
    pub fn repeats(self) -> bool {
        matches!(
            self,
            Action::PanLeft
                | Action::PanRight
                | Action::PanUp
                | Action::PanDown
                | Action::NudgeLeft
                | Action::NudgeRight
                | Action::NudgeUp
                | Action::NudgeDown
        )
    }
}

fn is_digit(k: Key) -> bool {
    matches!(
        k,
        Key::Minus
            | Key::Equals
            | Key::Num0
            | Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9
    )
}

/// While a text field has focus, only chords with Cmd/Ctrl may fire, and not the ones
/// that edit text. Single keys always go to the text field.
pub fn allowed_in_text_field(action: Action, chord: Chord) -> bool {
    chord.command && !matches!(action, Action::Undo | Action::Redo | Action::SelectAll)
}

#[derive(Clone, Debug)]
pub struct Keymap {
    by_action: BTreeMap<Action, Vec<Chord>>,
    lookup: HashMap<Chord, Action>,
}

fn parse_defaults() -> BTreeMap<Action, Vec<Chord>> {
    Action::ALL
        .iter()
        .map(|a| {
            (
                *a,
                a.defaults()
                    .iter()
                    .map(|c| Chord::parse(c).expect("valid default chord"))
                    .collect(),
            )
        })
        .collect()
}

impl Keymap {
    fn from_bindings(by_action: BTreeMap<Action, Vec<Chord>>) -> Keymap {
        let mut lookup = HashMap::new();
        for (a, chords) in &by_action {
            for c in chords {
                lookup.insert(*c, *a);
            }
        }
        Keymap { by_action, lookup }
    }

    pub fn defaults() -> Keymap {
        Keymap::from_bindings(parse_defaults())
    }

    /// The defaults with the overrides from `json` applied. The second value lists problems.
    /// When a chord is claimed twice, the user file wins and the loser is unbound.
    pub fn with_overrides(json: &str) -> (Keymap, Vec<String>) {
        let mut bindings = parse_defaults();
        let mut warnings = Vec::new();
        let parsed: serde_json::Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => {
                return (
                    Keymap::defaults(),
                    vec![format!("keymap file ignored: {e}")],
                );
            }
        };
        let Some(obj) = parsed.as_object() else {
            return (
                Keymap::defaults(),
                vec!["keymap file ignored: expected a JSON object".into()],
            );
        };
        let mut user: BTreeMap<&str, Vec<Chord>> = BTreeMap::new();
        for (id, val) in obj {
            let items: Vec<&serde_json::Value> = match val {
                serde_json::Value::Array(a) => a.iter().collect(),
                v => vec![v],
            };
            let mut chords = Vec::new();
            let mut bad = false;
            for item in items {
                match item.as_str().map(Chord::parse) {
                    Some(Ok(c)) => chords.push(c),
                    Some(Err(e)) => {
                        warnings.push(format!("keymap: {e} (for '{id}'; default kept)"));
                        bad = true;
                    }
                    None => {
                        warnings.push(format!(
                            "keymap: '{id}' needs a chord string or a list of them (default kept)"
                        ));
                        bad = true;
                    }
                }
            }
            if bad {
                continue;
            }
            user.insert(id.as_str(), chords);
        }
        for (id, chords) in user {
            let Some(action) = Action::from_id(id) else {
                warnings.push(format!("keymap: unknown action '{id}'"));
                continue;
            };
            for c in &chords {
                for (other, list) in bindings.iter_mut() {
                    if *other != action && list.contains(c) {
                        list.retain(|x| x != c);
                        warnings.push(format!(
                            "keymap conflict: {} moved from {} to {}",
                            c.display(cfg!(target_os = "macos")),
                            other.id(),
                            id
                        ));
                    }
                }
            }
            bindings.insert(action, chords);
        }
        (Keymap::from_bindings(bindings), warnings)
    }

    /// All bindings as JSON in the format of `with_overrides`: `{"tool.line": ["2"], ...}`.
    /// A workspace profile (PLAT-04) stores a keymap in this form. Reading it back with
    /// `with_overrides` gives the same keymap.
    pub fn to_json(&self) -> String {
        let map: BTreeMap<&str, Vec<String>> = self
            .by_action
            .iter()
            .map(|(a, cs)| (a.id(), cs.iter().map(Chord::to_text).collect()))
            .collect();
        serde_json::to_string_pretty(&map).expect("strings serialize")
    }

    /// The action of a key event and the chord that matched. Digit chords such as `Alt+Shift+1`
    /// are also matched by the physical key: with Shift held, many layouts report another logical key
    /// (`!` on a US layout), and with Alt held, macOS reports a symbol.
    pub fn action_for_event(
        &self,
        key: Key,
        physical: Option<Key>,
        m: Modifiers,
    ) -> Option<(Action, Chord)> {
        let chord = Chord::from_event(key, m);
        if let Some(a) = self.action_for(chord) {
            return Some((a, chord));
        }
        let physical = physical.filter(|p| *p != key && is_digit(*p))?;
        let chord = Chord::from_event(physical, m);
        self.action_for(chord).map(|a| (a, chord))
    }

    pub fn action_for(&self, chord: Chord) -> Option<Action> {
        self.lookup.get(&chord).copied()
    }

    pub fn chords(&self, action: Action) -> &[Chord] {
        self.by_action.get(&action).map_or(&[], |v| v.as_slice())
    }

    /// First bound chord as text, for tooltips and menus. Empty when unbound.
    pub fn hint(&self, action: Action) -> String {
        self.chords(action)
            .first()
            .map(|c| c.display(cfg!(target_os = "macos")))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        Chord::parse(s).unwrap()
    }

    #[test]
    fn every_standard_view_has_a_key() {
        for v in StandardView::ALL {
            match Action::for_view(v) {
                Some(a) => {
                    assert_eq!(a.standard_view(), Some(v));
                    assert!(!Keymap::defaults().chords(a).is_empty());
                }
                None => assert_eq!(v, StandardView::Bottom),
            }
        }
        // The numbering follows the Vectorworks numeric keypad: 0 Top/Plan, 5 Top, 2 Front, 8 Back, 4 Left, 6 Right, 1 and 3 front isometrics, 7 and 9 rear.
        let km = Keymap::defaults();
        let m = |k: &str| km.action_for(chord(&format!("Alt+Shift+{k}")));
        assert_eq!(m("0"), Some(Action::ViewPlan));
        assert_eq!(m("5"), Some(Action::ViewTop));
        assert_eq!(m("2"), Some(Action::ViewFront));
        assert_eq!(m("9"), Some(Action::ViewRightRearIso));
        assert_eq!(km.action_for(chord("Cmd/Ctrl+5")), Some(Action::ViewPlan));
        assert_eq!(km.action_for(chord("Shift+C")), Some(Action::ViewFlyover));
        assert_eq!(
            km.action_for(chord("Cmd/Ctrl+Shift+W")),
            Some(Action::ViewWireframe)
        );
        assert_eq!(
            km.action_for(chord("Cmd/Ctrl+Shift+G")),
            Some(Action::ViewShaded)
        );
        assert_eq!(
            km.action_for(chord("Cmd/Ctrl+Shift+E")),
            Some(Action::ViewHiddenLine)
        );
        assert_eq!(
            km.action_for(chord("Cmd/Ctrl+Shift+D")),
            Some(Action::ViewDashedHiddenLine)
        );
        assert_eq!(
            km.action_for(chord("Cmd/Ctrl+Shift+F")),
            Some(Action::ViewRealistic)
        );
        assert_eq!(Action::ViewFlyover.standard_view(), None);
    }

    #[test]
    fn digit_chords_match_the_physical_key_when_the_character_differs() {
        let km = Keymap::defaults();
        let alt_shift = Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        };
        // US layout, Shift+1 types "!": logical key Exclamationmark, physical key Digit1.
        assert_eq!(
            km.action_for_event(Key::Exclamationmark, Some(Key::Num1), alt_shift)
                .map(|x| x.0),
            Some(Action::ViewLeftIso)
        );
        // The logical key matches when it is the digit already.
        assert_eq!(
            km.action_for_event(Key::Num3, Some(Key::Num3), alt_shift)
                .map(|x| x.0),
            Some(Action::ViewRightIso)
        );
        assert_eq!(
            km.action_for_event(Key::Num3, None, alt_shift).map(|x| x.0),
            Some(Action::ViewRightIso)
        );
        // Other keys are not remapped: a logical key that is bound wins, and a physical letter never counts.
        assert_eq!(
            km.action_for_event(Key::J, Some(Key::Semicolon), alt_shift)
                .map(|x| x.0),
            Some(Action::ViewProjection)
        );
        assert_eq!(
            km.action_for_event(Key::Exclamationmark, Some(Key::A), alt_shift),
            None
        );
        // Shift+1 alone is not bound.
        assert_eq!(
            km.action_for_event(Key::Exclamationmark, Some(Key::Num1), Modifiers::SHIFT),
            None
        );
        // A plain digit is still a tool key.
        assert_eq!(
            km.action_for_event(Key::Num2, Some(Key::Num2), Modifiers::NONE)
                .map(|x| x.0),
            Some(Action::ToolLine)
        );
    }

    #[test]
    fn the_json_form_gives_the_same_keymap_back() {
        let (custom, w) = Keymap::with_overrides(
            r#"{"tool.line": ["G", "Alt+Shift+Backtick"], "edit.redo": []}"#,
        );
        assert!(w.is_empty(), "{w:?}");
        for km in [Keymap::defaults(), custom] {
            let (back, w) = Keymap::with_overrides(&km.to_json());
            assert!(w.is_empty(), "{w:?}");
            for a in Action::ALL {
                assert_eq!(back.chords(*a), km.chords(*a), "{}", a.id());
            }
        }
    }

    #[test]
    fn tools_and_actions_map_both_ways() {
        let all = [
            &Tool::BASIC[..],
            &Tool::LANDSCAPE[..],
            &Tool::SITE[..],
            &Tool::ANNOTATION[..],
            &Tool::IRRIGATION[..],
            &Tool::MODIFY[..],
        ]
        .concat();
        for t in all {
            assert_eq!(Action::for_tool(t).tool(), Some(t), "{t:?}");
        }
        // The three dimension keys all select the Dimension tool.
        for a in [
            Action::ToolDimension,
            Action::ToolAngularDim,
            Action::ToolRadialDim,
        ] {
            assert_eq!(a.tool(), Some(Tool::Dimension));
        }
        assert_eq!(Action::Undo.tool(), None);
        for a in Action::ALL {
            assert_eq!(Action::from_id(a.id()), Some(*a));
            assert!(!a.label().is_empty());
            assert!(a.implemented());
        }
        assert_eq!(Action::from_id("nope"), None);
        assert!(Action::PanLeft.repeats() && Action::NudgeDown.repeats());
        assert!(!Action::Undo.repeats());
    }

    #[test]
    fn chords_have_text_for_menus_and_for_files() {
        let c = chord("Cmd/Ctrl+Shift+Z");
        assert_eq!(c.display(true), "Cmd+Shift+Z");
        assert_eq!(c.display(false), "Ctrl+Shift+Z");
        assert_eq!(chord("Alt+2").display(true), "Option+2");
        assert_eq!(chord("Escape").display(false), "Esc");
        assert_eq!(chord("Backtick").display(false), "`");
        assert_eq!(c.to_text(), "Cmd/Ctrl+Shift+Z");
        let mac_ctrl = Chord::parse_for("Ctrl+Alt+Q", true).unwrap();
        assert_eq!(mac_ctrl.to_text(), "Ctrl+Alt+Q");
        assert_eq!(Chord::parse_for(&mac_ctrl.to_text(), true), Ok(mac_ctrl));
        // A trailing plus is the Plus key.
        assert_eq!(chord("Cmd/Ctrl++").key, Key::Plus);
        let km = Keymap::defaults();
        assert!(!km.hint(Action::Undo).is_empty());
        assert_eq!(km.hint(Action::ToolGrading), "");
    }

    #[test]
    fn parses_chords() {
        assert_eq!(
            chord("Cmd/Ctrl+Shift+Z"),
            Chord {
                key: Key::Z,
                command: true,
                ctrl: false,
                shift: true,
                alt: false
            }
        );
        assert_eq!(chord("Option+2").key, Key::Num2);
        assert!(chord("Option+2").alt);
        assert_eq!(chord("Backtick").key, Key::Backtick);
        assert_eq!(chord("[").key, Key::OpenBracket);
        assert_eq!(chord("]").key, Key::CloseBracket);
        assert_eq!(chord("-").key, Key::Minus);
        assert_eq!(chord("Tab").key, Key::Tab);
        assert!(Chord::parse("Hyper+Q").is_err());
        assert!(Chord::parse("Cmd+").is_err());
    }

    #[test]
    fn defaults_have_no_conflicts() {
        let mut seen: HashMap<Chord, Action> = HashMap::new();
        for a in Action::ALL {
            for c in a.defaults() {
                let c = chord(c);
                assert_eq!(seen.insert(c, *a), None, "{c:?} is bound twice");
            }
        }
        let (km, warnings) = Keymap::with_overrides("{}");
        assert!(warnings.is_empty());
        assert_eq!(km.action_for(chord("2")), Some(Action::ToolLine));
        assert_eq!(km.action_for(chord("Cmd/Ctrl+Shift+Z")), Some(Action::Redo));
    }

    #[test]
    fn override_replaces_defaults_of_the_action() {
        let (km, w) = Keymap::with_overrides(r#"{"tool.line": "G", "edit.redo": []}"#);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(km.action_for(chord("G")), Some(Action::ToolLine));
        assert_eq!(km.action_for(chord("2")), None);
        assert!(km.chords(Action::Redo).is_empty());
        assert_eq!(km.action_for(chord("Cmd/Ctrl+Y")), None);
    }

    #[test]
    fn conflict_user_file_wins_and_warns() {
        let (km, w) = Keymap::with_overrides(r#"{"tool.circle": "2"}"#);
        assert_eq!(km.action_for(chord("2")), Some(Action::ToolCircle));
        assert!(km.chords(Action::ToolLine).is_empty());
        assert_eq!(w.len(), 1);
        assert!(
            w[0].contains("tool.line") && w[0].contains("tool.circle"),
            "{w:?}"
        );
    }

    #[test]
    fn conflict_between_two_user_entries() {
        let (km, w) = Keymap::with_overrides(r#"{"tool.line": "Q", "tool.rect": "Q"}"#);
        assert_eq!(w.len(), 1);
        // Later action id wins; the other loses the chord.
        assert_eq!(km.action_for(chord("Q")), Some(Action::ToolRect));
        assert!(km.chords(Action::ToolLine).is_empty());
    }

    #[test]
    fn bad_input_warns_and_keeps_defaults() {
        let (km, w) =
            Keymap::with_overrides(r#"{"nope": "A", "tool.line": "Hyper+Q", "tool.rect": 5}"#);
        assert_eq!(w.len(), 3, "{w:?}");
        assert_eq!(km.action_for(chord("4")), Some(Action::ToolRect));
        assert_eq!(
            km.action_for(chord("2")),
            Some(Action::ToolLine),
            "a bad chord keeps the default"
        );
        let (km, w) = Keymap::with_overrides(r#"{"tool.line": ["Q", 7]}"#);
        assert_eq!(w.len(), 1);
        assert_eq!(km.action_for(chord("2")), Some(Action::ToolLine));
        assert_eq!(km.action_for(chord("Q")), None);
        let (km, w) = Keymap::with_overrides("not json");
        assert_eq!(w.len(), 1);
        assert_eq!(km.action_for(chord("2")), Some(Action::ToolLine));
    }

    #[test]
    fn event_chords_ignore_mac_ctrl() {
        let cmd = Modifiers {
            command: true,
            ..Default::default()
        };
        assert_eq!(
            Chord::from_event_for(Key::Z, cmd, true),
            chord("Cmd/Ctrl+Z")
        );
        // On macOS the Control key is its own modifier and does not trigger Cmd bindings.
        let mac_ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let c = Chord::from_event_for(Key::Z, mac_ctrl, true);
        assert!(c.ctrl && !c.command);
        assert_ne!(c, chord("Cmd/Ctrl+Z"));
        assert_eq!(Chord::parse_for("Ctrl+Z", true).unwrap(), c);
        // Elsewhere "Ctrl" is the command key.
        assert_eq!(
            Chord::parse_for("Ctrl+Z", false).unwrap(),
            Chord::parse_for("Cmd/Ctrl+Z", false).unwrap()
        );
        assert_eq!(c.display(true), "Ctrl+Z");
    }

    #[test]
    fn text_fields_block_single_keys() {
        let km = Keymap::defaults();
        // Every plain or Shift-only chord is blocked while typing.
        for a in Action::ALL {
            for c in km.chords(*a) {
                if !c.command {
                    assert!(
                        !allowed_in_text_field(*a, *c),
                        "{} must not fire while typing",
                        a.id()
                    );
                }
            }
        }
        assert!(!allowed_in_text_field(Action::Undo, chord("Cmd/Ctrl+Z")));
        assert!(!allowed_in_text_field(
            Action::SelectAll,
            chord("Cmd/Ctrl+A")
        ));
        assert!(allowed_in_text_field(Action::FileSave, chord("Cmd/Ctrl+S")));
    }

    /// The doc table and the default keymap must not drift apart.
    #[test]
    fn doc_matches_default_keymap() {
        let doc = include_str!("../../../docs/keyboard-shortcuts.md");
        let km = Keymap::defaults();
        let mut documented = std::collections::HashSet::new();
        let (mut checked, mut done_rows) = (0, 0);
        for line in doc.lines().filter(|l| l.starts_with("| ")) {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            if cells.len() != 4 {
                continue;
            }
            // Every backticked action id in the row counts as documented.
            let Some(id) = cells[0].split('`').nth(1) else {
                continue;
            };
            documented.insert(id.to_string());
            let action = Action::from_id(id)
                .unwrap_or_else(|| panic!("unknown action id '{id}' in the doc"));
            let status = cells[3];
            assert_eq!(
                status == "done",
                action.implemented(),
                "status of {id} disagrees with the code"
            );
            let binding = cells[2].split(" (").next().unwrap();
            // A tool without a default key says "none".
            let mut want: Vec<Chord> = if binding == "none" {
                Vec::new()
            } else {
                binding.split(" or ").map(|c| chord(c.trim())).collect()
            };
            let mut have = km.chords(action).to_vec();
            want.sort();
            have.sort();
            assert_eq!(want, have, "doc and keymap differ for {id}");
            checked += 1;
            done_rows += usize::from(status == "done");
        }
        assert!(
            done_rows >= 25 && checked >= 40,
            "table not parsed: {done_rows} done rows, {checked} checked"
        );
        for a in Action::ALL {
            assert!(
                documented.contains(a.id()),
                "{} is missing in docs/keyboard-shortcuts.md",
                a.id()
            );
        }
    }
}
