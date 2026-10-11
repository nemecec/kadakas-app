//! Workspace profiles in the app (PLAT-04). The model is in `kadakas_ui_core::profile`; this
//! module keeps the state, reads and writes the user files, and draws the Preferences section,
//! the Shortcuts and tools window and the command alias field.

use egui::Ui;
use kadakas_l10n::tr;
use kadakas_ui_core::keymap::{Action, Chord};
use kadakas_ui_core::profile::{
    CommandLine, Outcome, Profile, ProfileSet, RenameError, VECTORWORKS_ID,
};

use crate::keymap;

/// Profiles, the active one, and the state of their windows.
pub struct Profiles {
    pub set: ProfileSet,
    pub active: String,
    pub command_line: CommandLine,
    /// Set when the command alias field should take the keyboard focus.
    pub focus_command: bool,
    /// The last message for the person (a result or an error), shown in the window.
    pub message: Option<String>,
    pub customize_open: bool,
    /// The config folder that holds `profiles/` and `profile.txt`. `None` writes nothing (the
    /// web build, and tests).
    config: Option<std::path::PathBuf>,
    changed: bool,
    switched: bool,
    dirty: Vec<String>,
    removed: Vec<String>,
    rename_text: String,
    rename_for: String,
    capture: Option<Action>,
    key_grabbed: bool,
    conflict: Option<(Action, Chord, Action)>,
    filter: String,
}

impl Default for Profiles {
    fn default() -> Self {
        Profiles::with_set(ProfileSet::new(Vec::new()))
    }
}

/// A change that the Shortcuts and tools window asks for. The window collects them while it
/// draws, and applies them afterwards.
enum Edit {
    RemoveChord(Action, Chord),
    AddChord(Action),
    Reset(Action),
    MoveTool(usize, usize, usize),
}

impl Profiles {
    fn with_set(set: ProfileSet) -> Profiles {
        Profiles {
            set,
            active: VECTORWORKS_ID.into(),
            command_line: CommandLine::new(),
            focus_command: false,
            message: None,
            customize_open: false,
            config: None,
            changed: false,
            switched: false,
            dirty: Vec::new(),
            removed: Vec::new(),
            rename_text: String::new(),
            rename_for: String::new(),
            capture: None,
            key_grabbed: false,
            conflict: None,
            filter: String::new(),
        }
    }

    /// The user profiles from the config folder, and warnings about files that did not load.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load() -> (Profiles, Vec<String>) {
        match crate::prefs::dir() {
            Some(dir) => Profiles::load_in(&dir),
            None => (Profiles::default(), Vec::new()),
        }
    }

    /// The user profiles in `config/profiles` (one `<id>.json` file each). Changes are written
    /// back there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_in(config: &std::path::Path) -> (Profiles, Vec<String>) {
        let mut user = Vec::new();
        let mut warnings = Vec::new();
        if let Ok(entries) = std::fs::read_dir(config.join("profiles")) {
            let mut files: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            files.sort();
            for path in files {
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                match std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| Profile::parse(&t, stem).map_err(|e| e.text()))
                {
                    Ok(parsed) => {
                        if let Some(first) = parsed.problems.first() {
                            warnings.push(tr!(
                                "profiles-ui-file-warning",
                                path = path.display(),
                                problem = first.text()
                            ));
                        }
                        user.push(parsed.profile);
                    }
                    Err(e) => warnings.push(tr!(
                        "profiles-ui-file-ignored",
                        path = path.display(),
                        e = e
                    )),
                }
            }
        }
        let mut p = Profiles::with_set(ProfileSet::new(user));
        p.config = Some(config.to_path_buf());
        (p, warnings)
    }

    /// The profile that is active. An unknown id gives the Vectorworks-like profile.
    pub fn active_profile(&self) -> &Profile {
        self.set
            .get(&self.active)
            .unwrap_or_else(|| &self.set.all()[0])
    }

    /// Makes `id` the active profile. False for an unknown id.
    pub fn select(&mut self, id: &str) -> bool {
        if self.set.get(id).is_none() {
            return false;
        }
        if self.active != id {
            self.active = id.to_string();
            self.switched = true;
            self.changed = true;
        }
        true
    }

    /// Makes the profile that `name` names active (id or name). False if there is none.
    pub fn select_named(&mut self, name: &str) -> bool {
        match self.set.find(name).map(|p| p.id.clone()) {
            Some(id) => self.select(&id),
            None => false,
        }
    }

    /// Returns `(changed, switched)` once: the active profile or its data changed, and whether
    /// the person picked another profile.
    pub fn take_changed(&mut self) -> Option<bool> {
        let switched = std::mem::take(&mut self.switched);
        std::mem::take(&mut self.changed).then_some(switched)
    }

    /// True while a key press belongs to the Shortcuts window, so the keymap must not run it.
    pub fn grabs_keys(&self) -> bool {
        self.capture.is_some() || self.key_grabbed
    }

    /// Call at the end of a frame.
    pub fn end_frame(&mut self) {
        self.key_grabbed = false;
    }

    /// Writes the user files that changed and deletes the removed ones.
    pub fn flush(&mut self) {
        let dirty = std::mem::take(&mut self.dirty);
        let removed = std::mem::take(&mut self.removed);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(config) = self.config.clone() else {
                return;
            };
            let dir = config.join("profiles");
            for id in dirty {
                if let Some(p) = self.set.get(&id).filter(|p| !p.builtin) {
                    let file = format!("{id}.json");
                    if let Err(e) = crate::prefs::write_file(&dir, &file, &p.to_json()) {
                        self.message = Some(e);
                    }
                }
            }
            for id in removed {
                let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
            }
            self.save_active();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (dirty, removed);
    }

    /// Remembers the active profile for the next start.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_active(&self) {
        if let Some(dir) = &self.config {
            let _ = crate::prefs::write_file(dir, "profile.txt", &self.active);
        }
    }

    /// The profile of the last session.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn saved_active(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.config.as_ref()?.join("profile.txt")).ok()?;
        Some(text.trim().to_string()).filter(|t| !t.is_empty())
    }

    /// The id of a profile that can be edited. A built-in profile gets a user copy, which
    /// becomes the active profile.
    fn edit_target(&mut self) -> Option<String> {
        let was_builtin = self.set.get(&self.active)?.builtin;
        let id = self.set.editable(&self.active)?;
        if was_builtin {
            let name = self.set.get(&id)?.name.clone();
            self.message = Some(tr!("profiles-ui-copy-made", name = name));
            self.active = id.clone();
            self.switched = true;
        }
        Some(id)
    }

    /// Changes the active profile, or its user copy. The change applies at once.
    fn edit(&mut self, f: impl FnOnce(&mut Profile)) {
        let Some(id) = self.edit_target() else {
            return;
        };
        if let Some(p) = self.set.user_mut(&id) {
            f(p);
        }
        if !self.dirty.contains(&id) {
            self.dirty.push(id);
        }
        self.changed = true;
    }

    fn apply(&mut self, edit: Edit) {
        match edit {
            Edit::RemoveChord(a, c) => self.edit(|p| {
                let rest: Vec<Chord> = p
                    .keymap
                    .chords(a)
                    .iter()
                    .copied()
                    .filter(|x| *x != c)
                    .collect();
                p.set_chords(a, &rest);
            }),
            Edit::AddChord(a) => {
                self.capture = Some(a);
                self.conflict = None;
            }
            Edit::Reset(a) => self.edit(|p| {
                p.reset_chords(a);
            }),
            Edit::MoveTool(set, from, to) => self.edit(|p| {
                p.move_tool(set, from, to);
            }),
        }
    }

    /// Gives `chord` to `action` (in addition to its other chords).
    fn add_chord(&mut self, action: Action, chord: Chord) {
        self.edit(|p| {
            let mut all = p.keymap.chords(action).to_vec();
            all.push(chord);
            p.set_chords(action, &all);
        });
    }

    /// Takes the key press that the Shortcuts window waits for.
    fn capture_key(&mut self, ctx: &egui::Context) {
        let Some(action) = self.capture else {
            return;
        };
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *physical_key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, physical, mods)) = pressed else {
            return;
        };
        self.key_grabbed = true;
        self.capture = None;
        if key == egui::Key::Escape {
            return;
        }
        let (Some(k), m) = (keymap::key(key), keymap::modifiers(mods)) else {
            return;
        };
        // A digit with Shift or Alt held: the physical key tells which digit it is.
        let digit = physical
            .and_then(keymap::key)
            .filter(|p| *p != k && p.name().starts_with("Num"));
        let chord = Chord::from_event(digit.unwrap_or(k), m);
        let current = self.active_profile().keymap.conflict_for(action, chord);
        match current {
            Some(other) => self.conflict = Some((action, chord, other)),
            None => self.add_chord(action, chord),
        }
    }
}

fn chord_text(c: &Chord) -> String {
    c.display(cfg!(target_os = "macos"))
}

/// The Preferences section: choose, duplicate, rename, export, import and delete a profile.
pub fn section(ui: &mut Ui, p: &mut Profiles) {
    ui.heading(tr!("profiles-ui-workspace-profile"));
    let (id, builtin, about) = {
        let a = p.active_profile();
        (a.id.clone(), a.builtin, a.display_description())
    };
    let mut pick = None;
    ui.horizontal(|ui| {
        ui.label(tr!("profiles-ui-profile"));
        egui::ComboBox::from_id_salt("pref_profile")
            .selected_text(p.active_profile().display_name())
            .show_ui(ui, |ui| {
                for q in p.set.all() {
                    if ui.selectable_label(q.id == id, q.display_name()).clicked() {
                        pick = Some(q.id.clone());
                    }
                }
            });
    });
    if let Some(new) = pick {
        p.select(&new);
    }
    if !about.is_empty() {
        ui.label(about);
    }
    if builtin {
        ui.label(tr!("profiles-ui-built-in-note"));
    } else {
        if p.rename_for != id {
            p.rename_for = id.clone();
            p.rename_text = p.active_profile().name.clone();
        }
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut p.rename_text);
            if ui.button(tr!("profiles-ui-rename")).clicked() {
                let text = p.rename_text.clone();
                match p.set.rename(&id, &text) {
                    Ok(()) => {
                        if !p.dirty.contains(&id) {
                            p.dirty.push(id.clone());
                        }
                        p.flush();
                        p.message = None;
                    }
                    Err(e) => {
                        p.message = Some(match e {
                            RenameError::NameTaken => tr!("profiles-ui-name-taken"),
                            RenameError::ReadOnly => tr!("profiles-ui-built-in-note"),
                            RenameError::BadName | RenameError::Unknown => {
                                tr!("profiles-ui-bad-name")
                            }
                        })
                    }
                }
            }
        });
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button(tr!("profiles-ui-duplicate")).clicked()
            && let Some(copy) = p.set.duplicate(&id)
        {
            p.dirty.push(copy.clone());
            p.select(&copy);
            p.flush();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if ui.button(tr!("profiles-ui-export")).clicked() {
                export(p, &id);
            }
            if ui.button(tr!("profiles-ui-import")).clicked() {
                import(p);
            }
        }
        if !builtin && ui.button(tr!("profiles-ui-delete")).clicked() {
            p.set.remove(&id);
            p.removed.push(id.clone());
            p.select(VECTORWORKS_ID);
            p.flush();
        }
        if ui.button(tr!("profiles-ui-shortcuts-and-tools")).clicked() {
            p.customize_open = true;
        }
    });
    if let Some(m) = &p.message {
        ui.label(m);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn export(p: &mut Profiles, id: &str) {
    let Some(profile) = p.set.get(id) else {
        return;
    };
    let text = profile.to_json();
    let Some(path) = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .set_file_name(format!("{id}.json"))
        .save_file()
    else {
        return;
    };
    p.message = Some(match std::fs::write(&path, text) {
        Ok(()) => tr!("profiles-ui-exported", path = path.display()),
        Err(e) => tr!("profiles-ui-cannot-write", path = path.display(), e = e),
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn import(p: &mut Profiles) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .pick_file()
    else {
        return;
    };
    let result = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| p.set.import(&text).map_err(|e| e.text()));
    match result {
        Ok((id, problems)) => {
            let name = p.set.get(&id).map(|q| q.name.clone()).unwrap_or_default();
            p.message = Some(match problems.first() {
                None => tr!("profiles-ui-imported", name = name),
                Some(first) => tr!(
                    "profiles-ui-imported-with-problems",
                    name = name,
                    count = problems.len(),
                    first = first.text()
                ),
            });
            p.dirty.push(id.clone());
            p.select(&id);
            p.flush();
        }
        Err(e) => {
            p.message = Some(tr!(
                "profiles-ui-cannot-import",
                path = path.display(),
                e = e
            ));
        }
    }
}

/// The Shortcuts and tools window: edit the keys of the active profile, and the order of the
/// tools in each tool set. A built-in profile gets a user copy at the first change.
pub fn customize_window(ctx: &egui::Context, p: &mut Profiles) {
    p.capture_key(ctx);
    if !p.customize_open {
        return;
    }
    let mut open = true;
    let mut edits: Vec<Edit> = Vec::new();
    egui::Window::new(tr!("profiles-ui-shortcuts-and-tools"))
        .id(egui::Id::new("win-profile-customize"))
        .open(&mut open)
        .default_width(520.0)
        .show(ctx, |ui| {
            if p.active_profile().builtin {
                ui.label(tr!("profiles-ui-built-in-note"));
            } else {
                ui.label(p.active_profile().name.clone());
            }
            if let Some((action, chord, other)) = p.conflict {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr!(
                        "profiles-ui-conflict",
                        chord = chord_text(&chord),
                        other = other.label(),
                        action = action.label()
                    ));
                    if ui.button(tr!("profiles-ui-move-here")).clicked() {
                        p.add_chord(action, chord);
                        p.conflict = None;
                    }
                    if ui.button(tr!("common-cancel")).clicked() {
                        p.conflict = None;
                    }
                });
            }
            if p.capture.is_some() {
                ui.label(tr!("profiles-ui-press-a-key"));
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::CollapsingHeader::new(tr!("profiles-ui-shortcuts"))
                    .id_salt("profile-shortcuts")
                    .default_open(true)
                    .show(ui, |ui| shortcuts(ui, p, &mut edits));
                egui::CollapsingHeader::new(tr!("profiles-ui-tool-order"))
                    .id_salt("profile-tool-order")
                    .show(ui, |ui| tool_order(ui, p, &mut edits));
            });
        });
    if !open {
        p.customize_open = false;
        p.capture = None;
        p.conflict = None;
    }
    for e in edits {
        p.apply(e);
    }
    if p.changed {
        p.flush();
    }
}

fn shortcuts(ui: &mut Ui, p: &mut Profiles, edits: &mut Vec<Edit>) {
    ui.horizontal(|ui| {
        ui.label(tr!("profiles-ui-filter"));
        ui.text_edit_singleline(&mut p.filter);
    });
    let filter = p.filter.to_lowercase();
    egui::Grid::new("profile-shortcut-grid")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for a in Action::ALL.iter().copied() {
                let label = a.label();
                if !filter.is_empty()
                    && !label.to_lowercase().contains(&filter)
                    && !a.id().contains(&filter)
                {
                    continue;
                }
                ui.label(label);
                ui.horizontal_wrapped(|ui| {
                    for c in p.active_profile().keymap.chords(a) {
                        if ui
                            .small_button(format!("{} \u{d7}", chord_text(c)))
                            .on_hover_text(tr!("profiles-ui-remove-key"))
                            .clicked()
                        {
                            edits.push(Edit::RemoveChord(a, *c));
                        }
                    }
                    if ui.small_button(tr!("profiles-ui-add-key")).clicked() {
                        edits.push(Edit::AddChord(a));
                    }
                    if ui
                        .small_button(tr!("profiles-ui-reset"))
                        .on_hover_text(tr!("profiles-ui-reset-tip"))
                        .clicked()
                    {
                        edits.push(Edit::Reset(a));
                    }
                });
                ui.end_row();
            }
        });
}

fn tool_order(ui: &mut Ui, p: &Profiles, edits: &mut Vec<Edit>) {
    for (si, set) in p.active_profile().tool_sets.iter().enumerate() {
        ui.label(set.title());
        for (i, tool) in set.tools.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(i > 0, |ui| {
                    if ui
                        .small_button("\u{25b2}")
                        .on_hover_text(tr!("profiles-ui-move-up"))
                        .clicked()
                    {
                        edits.push(Edit::MoveTool(si, i, i - 1));
                    }
                });
                ui.add_enabled_ui(i + 1 < set.tools.len(), |ui| {
                    if ui
                        .small_button("\u{25bc}")
                        .on_hover_text(tr!("profiles-ui-move-down"))
                        .clicked()
                    {
                        edits.push(Edit::MoveTool(si, i, i + 1));
                    }
                });
                ui.label(tool.name());
            });
        }
        ui.add_space(6.0);
    }
}

/// The compact command alias field. Returns what Enter did, if anything. Profiles without
/// aliases show nothing.
pub fn command_line_ui(ui: &mut Ui, p: &mut Profiles) -> Option<Outcome> {
    let profile = p.set.get(&p.active)?;
    if profile.aliases.is_empty() {
        return None;
    }
    let aliases = &profile.aliases;
    let id = egui::Id::new("command-alias-field");
    if std::mem::take(&mut p.focus_command) {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let mut text = p.command_line.text().to_string();
    let r = ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .desired_width(110.0)
                .hint_text(tr!("profiles-ui-command")),
        )
        .on_hover_text(tr!("profiles-ui-command-tip"));
    if text != p.command_line.text() {
        p.command_line.set_text(&text);
    }
    if r.has_focus() {
        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        if up {
            p.command_line.history_previous();
        }
        if down {
            p.command_line.history_next();
        }
    }
    if !p.command_line.is_empty() {
        match aliases.get(p.command_line.text()) {
            Some(a) => {
                ui.label(tr!("profiles-ui-alias-runs", name = a.label()));
            }
            None => {
                let names: Vec<String> = p
                    .command_line
                    .suggestions(aliases, 4)
                    .into_iter()
                    .map(|(alias, _)| alias)
                    .collect();
                ui.label(names.join(" "));
            }
        }
    }
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if r.lost_focus() && enter {
        return Some(p.command_line.enter(aliases));
    }
    if (r.lost_focus() || r.has_focus()) && escape {
        p.command_line.clear();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        Chord::parse(s).unwrap()
    }

    #[test]
    fn the_first_edit_of_a_builtin_profile_makes_a_copy() {
        let mut p = Profiles::default();
        assert_eq!(p.active, VECTORWORKS_ID);
        assert!(p.active_profile().builtin);
        p.add_chord(Action::ToolLine, chord("Q"));
        assert_ne!(p.active, VECTORWORKS_ID);
        assert!(!p.active_profile().builtin);
        assert!(p.message.is_some());
        assert_eq!(p.take_changed(), Some(true));
        assert_eq!(p.take_changed(), None);
        // The built-in profile is unchanged.
        assert!(
            p.set
                .get(VECTORWORKS_ID)
                .unwrap()
                .keymap
                .chords(Action::ToolLine)
                == [chord("2")]
        );
        assert_eq!(
            p.active_profile().keymap.chords(Action::ToolLine),
            [chord("2"), chord("Q")]
        );
        // A second edit stays in the copy.
        let copy = p.active.clone();
        p.apply(Edit::RemoveChord(Action::ToolLine, chord("2")));
        assert_eq!(p.active, copy);
        assert_eq!(p.take_changed(), Some(false));
        assert_eq!(
            p.active_profile().keymap.chords(Action::ToolLine),
            [chord("Q")]
        );
        p.apply(Edit::Reset(Action::ToolLine));
        assert_eq!(
            p.active_profile().keymap.chords(Action::ToolLine),
            [chord("2")]
        );
        p.apply(Edit::MoveTool(0, 0, 1));
        assert_eq!(p.active_profile().tool_sets[0].tools[1].name(), "Selection");
        p.apply(Edit::AddChord(Action::ToolRect));
        assert_eq!(p.capture, Some(Action::ToolRect));
        assert!(p.grabs_keys());
    }

    #[test]
    fn the_windows_draw_and_edit_the_profile() {
        let ctx = egui::Context::default();
        let mut p = Profiles {
            customize_open: true,
            ..Profiles::default()
        };
        for name in ["vectorworks", "autocad"] {
            p.select(name);
            let _ = ctx.run_ui(Default::default(), |ui| {
                section(ui, &mut p);
                customize_window(ui.ctx(), &mut p);
                let _ = command_line_ui(ui, &mut p);
            });
        }
        // A key press while the window waits for a key goes to the action. A free key is added.
        p.select("vectorworks");
        p.capture = Some(Action::ToolLine);
        let press = |k, m| egui::RawInput {
            events: vec![egui::Event::Key {
                key: k,
                physical_key: Some(k),
                pressed: true,
                repeat: false,
                modifiers: m,
            }],
            ..Default::default()
        };
        let _ = ctx.run_ui(press(egui::Key::Q, egui::Modifiers::NONE), |ui| {
            customize_window(ui.ctx(), &mut p)
        });
        assert!(p.grabs_keys());
        assert!(!p.active_profile().builtin);
        assert!(
            p.active_profile()
                .keymap
                .chords(Action::ToolLine)
                .contains(&chord("Q"))
        );
        p.end_frame();
        // A key of another action is a conflict that the window shows.
        p.capture = Some(Action::ToolLine);
        let _ = ctx.run_ui(press(egui::Key::Num4, egui::Modifiers::NONE), |ui| {
            customize_window(ui.ctx(), &mut p)
        });
        assert_eq!(
            p.conflict,
            Some((Action::ToolLine, chord("4"), Action::ToolRect))
        );
        let _ = ctx.run_ui(Default::default(), |ui| customize_window(ui.ctx(), &mut p));
        p.add_chord(Action::ToolLine, chord("4"));
        assert!(
            p.active_profile()
                .keymap
                .chords(Action::ToolRect)
                .is_empty()
        );
        // Escape cancels the capture. Closing the window ends everything.
        p.capture = Some(Action::ToolLine);
        let _ = ctx.run_ui(press(egui::Key::Escape, egui::Modifiers::NONE), |ui| {
            customize_window(ui.ctx(), &mut p)
        });
        assert!(p.capture.is_none() && p.key_grabbed);
        assert!(p.message.is_some());
    }

    #[test]
    fn profiles_are_selected_by_id_or_name() {
        let mut p = Profiles::default();
        assert!(!p.select("nope") && !p.select_named("nope"));
        assert!(p.select_named("AutoCAD-like"));
        assert_eq!(p.active, "autocad");
        assert_eq!(p.take_changed(), Some(true));
        assert!(p.select("autocad"));
        assert_eq!(p.take_changed(), None);
        p.active = "gone".into();
        assert_eq!(p.active_profile().id, VECTORWORKS_ID);
        p.end_frame();
        assert!(!p.grabs_keys());
    }

    #[test]
    fn user_profiles_are_written_read_and_removed() {
        let dir = std::env::temp_dir().join(format!("kadakas-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // The prefs folder is the parent of `profiles`.
        let mut p = Profiles::default();
        let copy = p.set.duplicate("autocad").unwrap();
        let text = p.set.get(&copy).unwrap().to_json();
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        std::fs::write(dir.join("profiles").join(format!("{copy}.json")), &text).unwrap();
        std::fs::write(dir.join("profiles").join("bad.json"), "nope").unwrap();
        std::fs::write(dir.join("profiles").join("note.txt"), "x").unwrap();
        let mut loaded = Profiles::load_in(&dir);
        assert_eq!(loaded.0.set.all().len(), 3);
        assert_eq!(loaded.1.len(), 1);
        assert!(loaded.0.set.get(&copy).is_some());
        assert_eq!(Profiles::load_in(&dir.join("none")).0.set.all().len(), 2);
        // Changes are written back, and the active profile is remembered.
        assert_eq!(loaded.0.saved_active(), None);
        loaded.0.select(&copy);
        loaded.0.add_chord(Action::ToolLine, chord("Q"));
        loaded.0.flush();
        assert_eq!(loaded.0.saved_active(), Some(copy.clone()));
        let again = Profiles::load_in(&dir).0;
        assert!(
            again
                .set
                .get(&copy)
                .unwrap()
                .keymap
                .chords(Action::ToolLine)
                .contains(&chord("Q"))
        );
        loaded.0.set.remove(&copy);
        loaded.0.removed.push(copy.clone());
        loaded.0.flush();
        assert!(!dir.join("profiles").join(format!("{copy}.json")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
