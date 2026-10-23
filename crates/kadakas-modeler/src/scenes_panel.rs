//! The Scenes panel and the Styles panel of the tray (MOD-10). A scene is a saved view of the
//! document: the camera, the visible tags and layers and the render style. The list of scenes is
//! the list of saved views, so the Kadakas app shows the same scenes. The rows and the commands
//! come from `kadakas_ui_core::scene_panel` (headless); this file draws them. The panel never
//! changes the document by itself: it returns [`Action`]s, and the program runs them after the
//! frame, because a scene needs the camera and the size of the 3D view.

use kadakas_l10n::tr;
use kadakas_ui_core::scene_panel::{Problem, Row, StyleRow};

/// The marks of the move buttons: a triangle up and a triangle down.
const UP: &str = "\u{25B2}";
const DOWN: &str = "\u{25BC}";

/// What the person asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Store the present state as a new scene.
    Add,
    /// Store the present camera, visibility and style in the scene.
    Update(String),
    Rename(String, String),
    Delete(String),
    /// Move a scene up (`-1`) or down (`1`) in the list.
    Move(String, isize),
    /// Show a scene.
    Activate(String),
    /// Choose a render style by name.
    Style(String),
    /// Switch the camera transition on or off.
    Animate(bool),
}

/// The state of the panel: the chosen row, a rename in progress, and the scene that was shown last.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScenesState {
    /// The row that the buttons work on.
    pub chosen: Option<String>,
    /// A rename in progress: the scene and the text so far.
    pub renaming: Option<(String, String)>,
    /// The scene that was shown last. It stays marked while the document keeps a saved view of
    /// that name.
    pub active: Option<String>,
    focus_rename: bool,
}

impl ScenesState {
    /// Starts the rename of a scene: its name is chosen for editing at the next frame.
    pub fn begin_rename(&mut self, name: &str) {
        self.chosen = Some(name.to_string());
        self.renaming = Some((name.to_string(), name.to_string()));
        self.focus_rename = true;
    }

    /// Forget the names that the document does not hold any more (after a delete or an undo).
    pub fn forget_missing(&mut self, rows: &[Row]) {
        let has = |n: &str| rows.iter().any(|r| r.name.eq_ignore_ascii_case(n));
        if self.chosen.as_deref().is_some_and(|n| !has(n)) {
            self.chosen = None;
        }
        if self.active.as_deref().is_some_and(|n| !has(n)) {
            self.active = None;
        }
        if self.renaming.as_ref().is_some_and(|(n, _)| !has(n)) {
            self.renaming = None;
        }
    }
}

/// What the Scenes panel needs to know.
pub struct SceneView<'a> {
    pub rows: &'a [Row],
    /// The camera moves now (a transition runs): the panel shows no "changed" mark.
    pub animating: bool,
    /// The active scene differs from the present state.
    pub modified: bool,
    pub animate: bool,
}

/// The text for a refused command.
pub fn problem_text(p: Problem) -> String {
    match p {
        Problem::Empty => tr!("modeler-scenes-name-empty"),
        Problem::Taken => tr!("modeler-scenes-name-taken"),
        Problem::NoSuchScene => tr!("modeler-scenes-missing"),
        Problem::BadCamera => tr!("modeler-scenes-bad-camera"),
    }
}

/// The Scenes panel.
pub fn show(ui: &mut egui::Ui, state: &mut ScenesState, view: &SceneView) -> Vec<Action> {
    let mut out = Vec::new();
    state.forget_missing(view.rows);
    let chosen = state.chosen.clone();
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(tr!("modeler-scenes-add"))
            .on_hover_text(tr!("modeler-scenes-add-tip"))
            .clicked()
        {
            out.push(Action::Add);
        }
        ui.add_enabled_ui(chosen.is_some(), |ui| {
            let name = chosen.clone().unwrap_or_default();
            if ui
                .button(tr!("modeler-scenes-update"))
                .on_hover_text(tr!("modeler-scenes-update-tip"))
                .clicked()
            {
                out.push(Action::Update(name.clone()));
            }
            if ui.button(tr!("modeler-scenes-rename")).clicked() {
                state.begin_rename(&name);
            }
            if ui.button(tr!("modeler-scenes-delete")).clicked() {
                out.push(Action::Delete(name.clone()));
            }
            if ui
                .button(UP)
                .on_hover_text(tr!("modeler-scenes-up"))
                .clicked()
            {
                out.push(Action::Move(name.clone(), -1));
            }
            if ui
                .button(DOWN)
                .on_hover_text(tr!("modeler-scenes-down"))
                .clicked()
            {
                out.push(Action::Move(name.clone(), 1));
            }
        });
    });
    ui.add_space(2.0);
    if view.rows.is_empty() {
        ui.label(egui::RichText::new(tr!("modeler-scenes-none")).weak());
    }
    for row in view.rows {
        ui.horizontal(|ui| {
            if let Some((id, text)) = &mut state.renaming
                && id.eq_ignore_ascii_case(&row.name)
            {
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(130.0));
                if state.focus_rename {
                    r.request_focus();
                    state.focus_rename = false;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if enter {
                    out.push(Action::Rename(id.clone(), text.clone()));
                    state.renaming = None;
                } else if escape || r.lost_focus() {
                    state.renaming = None;
                }
                return;
            }
            let changed = row.active && view.modified && !view.animating;
            let mut label = egui::RichText::new(if changed {
                format!("{} *", row.name)
            } else {
                row.name.clone()
            });
            if row.active {
                label = label.strong();
            }
            let selected = state
                .chosen
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(&row.name));
            let r = ui.selectable_label(selected, label);
            if r.clicked() {
                state.chosen = Some(row.name.clone());
                state.active = Some(row.name.clone());
                out.push(Action::Activate(row.name.clone()));
            }
            if r.double_clicked() {
                state.begin_rename(&row.name);
            }
            if changed {
                r.on_hover_text(tr!("modeler-scenes-changed-tip"));
            }
            if row.hidden > 0 {
                ui.label(
                    egui::RichText::new(tr!("modeler-scenes-hidden", count = row.hidden)).weak(),
                );
            }
            if let Some(s) = &row.style {
                if row.style_missing {
                    ui.label(
                        egui::RichText::new(tr!("modeler-scenes-style-missing", name = s.clone()))
                            .weak()
                            .italics(),
                    );
                } else {
                    ui.label(egui::RichText::new(style_name_label(s)).weak());
                }
            }
        });
    }
    ui.add_space(4.0);
    let mut animate = view.animate;
    if ui
        .checkbox(&mut animate, tr!("modeler-scenes-animate"))
        .changed()
    {
        out.push(Action::Animate(animate));
    }
    out
}

/// The name of a built-in style in the language of the window. Other names are the names that the
/// person gave.
pub fn style_label(row: &StyleRow) -> String {
    if row.builtin {
        style_name_label(&row.name)
    } else {
        row.name.clone()
    }
}

/// The name of a style in the language of the window if it is a built-in one, else as it is.
pub fn style_name_label(name: &str) -> String {
    {
        match name {
            "Wireframe" => tr!("modeler-style-wireframe"),
            "Shaded" => tr!("modeler-style-shaded"),
            "Realistic" => tr!("modeler-style-realistic"),
            "Hidden Line" => tr!("modeler-style-hidden-line"),
            "Dashed Hidden Line" => tr!("modeler-style-dashed-hidden-line"),
            other => other.to_string(),
        }
    }
}

/// The style picker: a list of the built-in styles and the styles of the document. A pick is one
/// undo step.
pub fn styles(ui: &mut egui::Ui, rows: &[StyleRow]) -> Vec<Action> {
    let mut out = Vec::new();
    let shown = rows
        .iter()
        .find(|r| r.active)
        .map(style_label)
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("modeler-style-label")).weak());
        egui::ComboBox::from_id_salt("modeler-style-picker")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (i, r) in rows.iter().enumerate() {
                    if i > 0 && !r.builtin && rows[i - 1].builtin {
                        ui.separator();
                    }
                    if ui.selectable_label(r.active, style_label(r)).clicked() && !r.active {
                        out.push(Action::Style(r.name.clone()));
                    }
                }
            });
    });
    out
}
