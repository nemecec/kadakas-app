//! The Tags panel of the tray (MOD-05) and the tag selector of Entity Info. Tags are classes. The
//! panel lists them with an eye, a color swatch and a mark for the active tag, and below them the
//! layers with an eye. The rows and the commands come from `kadakas_ui_core::tags` (headless);
//! this file draws them. The panel never changes the document by itself: it returns the commands
//! and a message, and the program runs them after the frame.

use egui::{Color32, Pos2, Sense, Stroke, vec2};
use kadakas_core::command::Command;
use kadakas_core::model::{ClassId, Document, ObjectId};
use kadakas_l10n::tr;
use kadakas_ui_core::tags::{self, NameProblem, SelectionTag};

/// Where in the tray the program draws a part of the tag code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The tag selector under Entity Info.
    EntityInfo,
    /// The panel with the list of tags and layers.
    Panel,
}

/// The state of the panel: which row is chosen, a rename in progress, and a delete that waits for
/// the answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagsState {
    /// The row that Rename and Delete work on.
    pub chosen: Option<ClassId>,
    /// A rename in progress: the tag and the text so far.
    pub renaming: Option<(ClassId, String)>,
    /// A delete that waits for the answer: the tag, its name and every reference to it.
    pub deleting: Option<(ClassId, String, kadakas_core::classes::ClassUses)>,
    /// The tag that was made last: its name is chosen for editing at the next frame.
    focus_rename: bool,
    /// The rows of the panel for a revision. They walk all objects, so they are made once per
    /// revision and not once per frame.
    rows: Option<(u64, tags::Rows)>,
    /// How many times the rows were made (a test counts them).
    pub rows_built: u64,
}

/// What the panel wants done, after the frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Commands for the document, each one undo step.
    pub commands: Vec<Command>,
    /// A line for the status bar.
    pub message: Option<String>,
    /// The message to show after the commands ran well (a delete says what moved).
    pub after: Option<String>,
}

/// What the panel needs to know: the document and the objects that Entity Info changes.
pub struct View<'a> {
    pub doc: &'a Document,
    /// The objects whose tag the selector shows and changes: the selected objects.
    pub objects: &'a [ObjectId],
    /// The revision of the document: the rows are made again only when it changes.
    pub rev: u64,
    /// Faces or edges are selected. A tag belongs to an object, so the selector only explains this.
    pub parts: bool,
}

fn problem_text(p: NameProblem) -> String {
    match p {
        NameProblem::Empty => tr!("modeler-tags-name-empty"),
        NameProblem::Taken => tr!("modeler-tags-name-taken"),
        NameProblem::Untagged => tr!("modeler-tags-name-untagged"),
        NameProblem::NoSuchTag => tr!("modeler-tags-name-missing"),
    }
}

/// The name of a tag for the panel: Untagged is shown in the language of the window.
pub fn tag_name(row: &tags::TagRow) -> String {
    if row.untagged {
        tr!("modeler-tags-untagged")
    } else {
        row.name.clone()
    }
}

/// The eye: an almond with a pupil when the tag shows, and crossed out when it is hidden.
fn eye(ui: &mut egui::Ui, visible: bool, tip: String) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 16.0), Sense::click());
    let color = if visible {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let stroke = Stroke::new(1.3, color);
    let c = rect.center();
    let n = 14;
    let almond = |sign: f32| -> Vec<Pos2> {
        (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32 * 2.0 - 1.0;
                Pos2::new(c.x + 8.0 * t, c.y + sign * 4.5 * (1.0 - t * t))
            })
            .collect()
    };
    let painter = ui.painter();
    painter.add(egui::Shape::line(almond(-1.0), stroke));
    painter.add(egui::Shape::line(almond(1.0), stroke));
    if visible {
        painter.circle_filled(c, 2.6, color);
    } else {
        painter.line_segment([c + vec2(-8.0, 6.0), c + vec2(8.0, -6.0)], stroke);
    }
    resp.on_hover_text(tip)
}

/// The mark for the active tag or layer: a filled disc, or an empty ring.
fn radio(ui: &mut egui::Ui, active: bool, tip: String) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(14.0, 16.0), Sense::click());
    let color = ui.visuals().text_color();
    let c = rect.center();
    if active {
        ui.painter().circle_filled(c, 4.0, color);
    } else {
        ui.painter()
            .circle_stroke(c, 4.0, Stroke::new(1.0, ui.visuals().weak_text_color()));
    }
    resp.on_hover_text(tip)
}

fn swatch(ui: &mut egui::Ui, color: Option<[u8; 3]>) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    match color {
        Some([r, g, b]) => {
            ui.painter()
                .rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
        }
        None => {
            ui.painter().rect_stroke(
                rect,
                2.0,
                Stroke::new(1.0, ui.visuals().weak_text_color()),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// Draws the part of the tag code for the slot. Returns what to do after the frame.
pub fn show(ui: &mut egui::Ui, state: &mut TagsState, slot: Slot, view: &View) -> Outcome {
    if state.rows.as_ref().is_none_or(|(rev, _)| *rev != view.rev) {
        state.rows = Some((view.rev, tags::rows(view.doc)));
        state.rows_built += 1;
    }
    let rows = state
        .rows
        .take()
        .map(|r| r.1)
        .unwrap_or_else(|| tags::rows(view.doc));
    let out = match slot {
        Slot::EntityInfo => selector(ui, view, &rows),
        Slot::Panel => panel(ui, state, view, &rows),
    };
    state.rows = Some((view.rev, rows));
    out
}

/// The tag of the selection with a list to change it: the same tag for the object and for its
/// faces and edges, because the tag belongs to the object.
fn selector(ui: &mut egui::Ui, view: &View, rows: &tags::Rows) -> Outcome {
    let mut out = Outcome::default();
    if view.parts {
        ui.label(egui::RichText::new(tr!("modeler-tags-parts-hint")).weak());
        return out;
    }
    let current = tags::tag_of(view.doc, view.objects);
    if current == SelectionTag::None {
        return out;
    }
    let shown = match current {
        SelectionTag::One(id) => rows
            .tags
            .iter()
            .find(|t| t.id == id)
            .map_or_else(String::new, tag_name),
        _ => tr!("modeler-tags-mixed"),
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("modeler-tags-entity")).weak());
        egui::ComboBox::from_id_salt("entity-tag")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for t in &rows.tags {
                    let on = current == SelectionTag::One(t.id);
                    if ui.selectable_label(on, tag_name(t)).clicked() && !on {
                        out.commands
                            .extend(tags::set_tag(view.doc, view.objects, t.id));
                        out.after = Some(tr!(
                            "modeler-tags-changed",
                            count = view.objects.len(),
                            name = tag_name(t)
                        ));
                    }
                }
            });
    });
    out
}

fn panel(ui: &mut egui::Ui, state: &mut TagsState, view: &View, rows: &tags::Rows) -> Outcome {
    let mut out = Outcome::default();
    let doc = view.doc;
    // A chosen row that is gone (deleted, or undone away) is forgotten.
    if state
        .chosen
        .is_some_and(|c| !rows.tags.iter().any(|t| t.id == c))
    {
        state.chosen = None;
    }
    if state
        .renaming
        .as_ref()
        .is_some_and(|(c, _)| doc.class(*c).is_none())
    {
        state.renaming = None;
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button(tr!("modeler-tags-new")).clicked() {
            let name = tags::new_tag_name(doc);
            match tags::new_tag(doc, &name) {
                Ok(c) => out.commands.push(c),
                Err(p) => out.message = Some(problem_text(p)),
            }
        }
        if ui.button(tr!("modeler-tags-rename")).clicked() {
            match state.chosen {
                Some(id) => {
                    if let Some(row) = rows.tags.iter().find(|t| t.id == id) {
                        state.renaming = Some((id, row.name.clone()));
                        state.focus_rename = true;
                    }
                }
                None => out.message = Some(tr!("modeler-tags-pick-one")),
            }
        }
        if ui.button(tr!("modeler-tags-delete")).clicked() {
            match state.chosen {
                Some(id) => match tags::delete_tag(doc, id) {
                    Ok(plan) => {
                        let name = rows
                            .tags
                            .iter()
                            .find(|t| t.id == id)
                            .map_or_else(String::new, |t| t.name.clone());
                        if plan.uses.total() == 0 {
                            out.commands.push(plan.command);
                            out.after = Some(tr!("modeler-tags-deleted-empty", name = name));
                        } else {
                            // Never silently: the person answers first.
                            state.deleting = Some((id, name, plan.uses));
                        }
                    }
                    Err(p) => out.message = Some(problem_text(p)),
                },
                None => out.message = Some(tr!("modeler-tags-pick-one")),
            }
        }
        if ui.button(tr!("modeler-tags-show-all")).clicked() {
            out.commands.extend(tags::show_all(doc));
        }
    });
    ui.add_space(2.0);
    for row in &rows.tags {
        ui.horizontal(|ui| {
            let tip = if row.visible {
                tr!("modeler-tags-eye-hide")
            } else {
                tr!("modeler-tags-eye-show")
            };
            if eye(ui, row.visible, tip).clicked() {
                out.commands.extend(tags::toggle_tag(doc, row.id));
            }
            swatch(ui, row.color);
            if radio(ui, row.active, tr!("modeler-tags-active")).clicked() && !row.active {
                out.commands.push(tags::set_active_tag(row.id));
                if !row.visible {
                    out.after = Some(tr!("modeler-tags-hidden-active"));
                }
            }
            if let Some((id, text)) = &mut state.renaming
                && *id == row.id
            {
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(110.0));
                if state.focus_rename {
                    r.request_focus();
                    state.focus_rename = false;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if enter {
                    match tags::rename_tag(doc, *id, text) {
                        Ok(c) => out.commands.push(c),
                        Err(p) => out.message = Some(problem_text(p)),
                    }
                    state.renaming = None;
                } else if escape || r.lost_focus() {
                    state.renaming = None;
                }
            } else {
                let name = egui::RichText::new(tag_name(row));
                let name = if row.visible { name } else { name.weak() };
                let r = ui.selectable_label(state.chosen == Some(row.id), name);
                if r.clicked() {
                    state.chosen = Some(row.id);
                }
                if r.double_clicked() && !row.untagged {
                    state.renaming = Some((row.id, row.name.clone()));
                    state.focus_rename = true;
                }
                ui.label(
                    egui::RichText::new(tr!("modeler-tags-objects", count = row.objects)).weak(),
                );
            }
        });
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tr!("modeler-tags-layers")).strong());
    for layer in &rows.layers {
        ui.horizontal(|ui| {
            let tip = if layer.visible {
                tr!("modeler-layers-eye-hide")
            } else {
                tr!("modeler-layers-eye-show")
            };
            if eye(ui, layer.visible, tip).clicked() {
                out.commands.extend(tags::toggle_layer(doc, layer.id));
            }
            if radio(ui, layer.active, tr!("modeler-layers-active")).clicked() && !layer.active {
                out.commands.push(tags::set_active_layer(layer.id));
            }
            let name = egui::RichText::new(&layer.name);
            ui.label(if layer.visible { name } else { name.weak() });
        });
    }
    delete_dialog(ui.ctx(), state, doc, &mut out);
    out
}

/// The question before a tag with objects is deleted.
fn delete_dialog(ctx: &egui::Context, state: &mut TagsState, doc: &Document, out: &mut Outcome) {
    let Some((id, name, uses)) = state.deleting.clone() else {
        return;
    };
    if doc.class(id).is_none() {
        state.deleting = None;
        return;
    }
    let mut answer = None;
    egui::Window::new(tr!("modeler-tags-delete-title"))
        .id(egui::Id::new("win-modeler-tags-delete"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            if uses.objects > 0 {
                ui.label(tr!(
                    "modeler-tags-delete-objects",
                    count = uses.objects,
                    name = name.clone()
                ));
            }
            if uses.others() > 0 {
                ui.label(tr!(
                    "modeler-tags-delete-others",
                    guides = uses.guides,
                    parts = uses.definition_parts,
                    overrides = uses.viewport_overrides + uses.annotations,
                    views = uses.saved_views,
                    slots = uses.material_slots
                ));
            }
            ui.label(tr!("modeler-tags-delete-ask", name = name.clone()));
            ui.horizontal(|ui| {
                if ui.button(tr!("modeler-tags-delete-yes")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr!("modeler-tags-cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
    match answer {
        Some(true) => {
            if let Ok(plan) = tags::delete_tag(doc, id) {
                out.commands.push(plan.command);
                out.after = Some(tr!(
                    "modeler-tags-deleted",
                    count = uses.objects,
                    name = name
                ));
            }
            state.deleting = None;
        }
        Some(false) => state.deleting = None,
        None => {}
    }
}
