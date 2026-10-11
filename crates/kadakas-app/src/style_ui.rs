//! The style editor windows (ANN-12): dimension standards, text styles, data tag styles and notes
//! databases. They open from the Resource Manager ("Edit...") and from the style lists of the
//! annotation tools. The form logic is in `kadakas_ui_core::style_edit`; this file only draws the
//! fields. A save is one `ModifyResource` command, so it is one undo step.

use egui::{Context, DragValue, Ui};
use kadakas_api::Session;
use kadakas_core::annot::style::TagStyle;
use kadakas_core::model::Color;
use kadakas_core::resource::ResourceId;
use kadakas_l10n::tr;
use kadakas_ui_core::style_edit::{
    self, Choice, Follow, StyleEditor, StyleKind, area_units, default_tolerance, dimension_samples,
    length_units, set_enabled,
};

use crate::enum_names;
use crate::panels;
use crate::units_ui::DragValueExt as _;

/// One open window.
struct Open {
    editor: StyleEditor,
    /// The style changed elsewhere while this window had edits.
    conflict: bool,
}

/// The open style editors.
#[derive(Default)]
pub struct StyleWindows {
    open: Vec<Open>,
}

impl StyleWindows {
    /// Opens an editor on the style. A style that has a window already keeps it.
    pub fn open(&mut self, session: &Session, id: ResourceId) -> bool {
        if self.open.iter().any(|o| o.editor.id() == id) {
            return true;
        }
        match StyleEditor::open(session.editor.doc(), id) {
            Some(editor) => {
                self.open.push(Open {
                    editor,
                    conflict: false,
                });
                true
            }
            None => false,
        }
    }

    /// Makes a new style of the kind and opens its editor.
    pub fn create(&mut self, session: &mut Session, kind: StyleKind, status: &mut String) {
        let cmd = style_edit::create_command(session.editor.doc(), kind);
        if let Some(out) = panels::run_command(session, &cmd, status)
            && let Some(id) = out.created.first()
        {
            self.open(session, ResourceId(*id));
        }
    }

    #[cfg(test)]
    pub fn is_open(&self, id: ResourceId) -> bool {
        self.open.iter().any(|o| o.editor.id() == id)
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.open.len()
    }

    /// Draws the windows. Call it once per frame.
    pub fn show(&mut self, ctx: &Context, session: &mut Session, status: &mut String) {
        let mut done: Vec<usize> = Vec::new();
        for (i, w) in self.open.iter_mut().enumerate() {
            match w.editor.sync(session.editor.doc()) {
                Follow::Gone => {
                    done.push(i);
                    continue;
                }
                Follow::Conflict => w.conflict = true,
                Follow::Reloaded => w.conflict = false,
                Follow::Same => {}
            }
            let title = title(&w.editor);
            let mut open = true;
            let mut close = false;
            egui::Window::new(title)
                .id(egui::Id::new(("win-style", w.editor.id().0)))
                .open(&mut open)
                .default_width(380.0)
                .show(ctx, |ui| {
                    close = window_body(ui, w, session, status);
                });
            if !open || close {
                done.push(i);
            }
        }
        for i in done.into_iter().rev() {
            self.open.remove(i);
        }
    }
}

fn kind_name(kind: StyleKind) -> String {
    match kind {
        StyleKind::Dimension => tr!("style-ui-kind-dimension"),
        StyleKind::Text => tr!("style-ui-kind-text"),
        StyleKind::Tag => tr!("style-ui-kind-tag"),
        StyleKind::Notes => tr!("style-ui-kind-notes"),
    }
}

/// The name of a kind, for the "New style" menu.
pub fn kind_label(kind: StyleKind) -> String {
    kind_name(kind)
}

fn title(e: &StyleEditor) -> String {
    let name = e.name.clone();
    match e.kind() {
        StyleKind::Dimension => tr!("style-ui-title-dimension", name = name),
        StyleKind::Text => tr!("style-ui-title-text", name = name),
        StyleKind::Tag => tr!("style-ui-title-tag", name = name),
        StyleKind::Notes => tr!("style-ui-title-notes", name = name),
    }
}

/// Saves the draft. True when the document has the draft afterwards.
fn save(e: &mut StyleEditor, session: &mut Session, status: &mut String) -> bool {
    match e.save_command(session.editor.doc()) {
        Ok(None) => true,
        Ok(Some(cmd)) => {
            if panels::run_command(session, &cmd, status).is_some() {
                e.mark_saved();
                true
            } else {
                false
            }
        }
        Err(message) => {
            *status = tr!("style-ui-error", message = message);
            false
        }
    }
}

/// The fields and the buttons. True when the window should close.
fn window_body(ui: &mut Ui, w: &mut Open, session: &mut Session, status: &mut String) -> bool {
    let e = &mut w.editor;
    egui::Grid::new("style-ui-head")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr!("style-ui-name"));
            ui.text_edit_singleline(&mut e.name);
            ui.end_row();
        });
    ui.separator();
    match e.kind() {
        StyleKind::Dimension => dimension_fields(ui, e),
        StyleKind::Text => text_fields(ui, e),
        StyleKind::Tag => tag_fields(ui, e),
        StyleKind::Notes => notes_fields(ui, e),
    }
    ui.separator();
    if w.conflict {
        ui.colored_label(ui.visuals().warn_fg_color, tr!("style-ui-conflict"));
    }
    let problem = e.problem(session.editor.doc());
    if let Some(p) = &problem {
        ui.colored_label(
            ui.visuals().error_fg_color,
            tr!("style-ui-problem", message = p),
        );
    }
    let mut close = false;
    ui.horizontal(|ui| {
        let can = problem.is_none();
        if ui
            .add_enabled(can, egui::Button::new(tr!("common-ok")))
            .clicked()
            && save(e, session, status)
        {
            close = true;
        }
        if ui
            .add_enabled(
                can && e.is_dirty(),
                egui::Button::new(tr!("style-ui-apply")),
            )
            .clicked()
        {
            save(e, session, status);
            w.conflict = false;
        }
        if ui
            .add_enabled(e.is_dirty(), egui::Button::new(tr!("style-ui-revert")))
            .clicked()
        {
            e.revert();
        }
        if ui.button(tr!("common-cancel")).clicked() {
            close = true;
        }
    });
    close
}

/// A drop-down for a choice list.
fn choice<T: Copy + PartialEq + 'static>(
    ui: &mut Ui,
    salt: &str,
    current: &mut T,
    list: &'static [Choice<T>],
    domain: &str,
) {
    let name = |id: &str| enum_names::name(domain, id);
    egui::ComboBox::from_id_salt(salt)
        .selected_text(name(style_edit::id_of(list, current)))
        .show_ui(ui, |ui| {
            for c in list {
                ui.selectable_value(current, c.value, name(c.id));
            }
        });
}

fn paper_field(ui: &mut Ui, label: String, v: &mut f64, lo: f64, hi: f64) {
    ui.label(label);
    ui.add(DragValue::new(v).speed(0.1).range(lo..=hi).paper_mm());
    ui.end_row();
}

fn decimals_field(ui: &mut Ui, label: String, v: &mut u8) {
    ui.label(label);
    ui.add(DragValue::new(v).range(0..=8));
    ui.end_row();
}

/// An optional color: a checkbox and a color button.
fn color_field(ui: &mut Ui, label: String, color: &mut Option<Color>) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut on = color.is_some();
        if ui.checkbox(&mut on, tr!("style-ui-use-color")).changed() {
            set_enabled(color, on, Color([0, 0, 0, 255]));
        }
        if let Some(c) = color {
            let mut c32 = crate::canvas::color32(*c);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                *c = Color(c32.to_srgba_unmultiplied());
            }
        }
    });
    ui.end_row();
}

fn dimension_fields(ui: &mut Ui, e: &mut StyleEditor) {
    let Some(s) = e.dimension() else { return };
    egui::Grid::new("style-ui-dimension")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr!("style-ui-font"));
            ui.text_edit_singleline(&mut s.font);
            ui.end_row();
            paper_field(
                ui,
                tr!("style-ui-text-size"),
                &mut s.text_size_mm,
                0.1,
                200.0,
            );
            ui.label(tr!("style-ui-length-unit"));
            egui::ComboBox::from_id_salt("style-ui-length-unit")
                .selected_text(s.unit.short_name())
                .show_ui(ui, |ui| {
                    for (u, name) in length_units() {
                        ui.selectable_value(&mut s.unit, u, name);
                    }
                });
            ui.end_row();
            decimals_field(ui, tr!("style-ui-precision"), &mut s.precision);
            ui.label(tr!("style-ui-show-unit"));
            ui.checkbox(&mut s.show_unit, "");
            ui.end_row();
            decimals_field(ui, tr!("style-ui-angle-precision"), &mut s.angle_precision);
            ui.label(tr!("style-ui-round-to"));
            ui.horizontal(|ui| {
                let mut on = s.round_to.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    set_enabled(&mut s.round_to, on, 10.0);
                }
                if let Some(r) = &mut s.round_to {
                    ui.add(DragValue::new(r).speed(1.0).range(0.001..=1.0e6).length());
                }
            });
            ui.end_row();
            ui.label(tr!("style-ui-terminator"));
            choice(
                ui,
                "style-ui-terminator",
                &mut s.terminator,
                &style_edit::TERMINATORS,
                style_edit::TERMINATOR_DOMAIN,
            );
            ui.end_row();
            paper_field(
                ui,
                tr!("style-ui-terminator-size"),
                &mut s.terminator_size_mm,
                0.0,
                50.0,
            );
            paper_field(ui, tr!("style-ui-ext-gap"), &mut s.ext_gap_mm, 0.0, 50.0);
            paper_field(
                ui,
                tr!("style-ui-ext-overshoot"),
                &mut s.ext_overshoot_mm,
                0.0,
                50.0,
            );
            paper_field(
                ui,
                tr!("style-ui-baseline-spacing"),
                &mut s.baseline_spacing_mm,
                1.0,
                100.0,
            );
            paper_field(ui, tr!("style-ui-text-gap"), &mut s.text_gap_mm, 0.0, 50.0);
            ui.label(tr!("style-ui-prefix"));
            ui.text_edit_singleline(&mut s.prefix);
            ui.end_row();
            ui.label(tr!("style-ui-suffix"));
            ui.text_edit_singleline(&mut s.suffix);
            ui.end_row();
            ui.label(tr!("style-ui-tolerance"));
            ui.horizontal(|ui| {
                let mut on = s.tolerance.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    set_enabled(&mut s.tolerance, on, default_tolerance());
                }
                if let Some(t) = &mut s.tolerance {
                    ui.label(tr!("style-ui-plus"));
                    ui.add(DragValue::new(&mut t.plus).speed(0.1).range(0.0..=1.0e6));
                    ui.label(tr!("style-ui-minus"));
                    ui.add(DragValue::new(&mut t.minus).speed(0.1).range(0.0..=1.0e6));
                }
            });
            ui.end_row();
            color_field(ui, tr!("style-ui-color"), &mut s.color);
        });
    ui.add_space(6.0);
    ui.strong(tr!("style-ui-area-section"));
    egui::Grid::new("style-ui-dimension-area")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr!("style-ui-area-unit"));
            egui::ComboBox::from_id_salt("style-ui-area-unit")
                .selected_text(s.area_unit.short_name())
                .show_ui(ui, |ui| {
                    for (u, name) in area_units() {
                        ui.selectable_value(&mut s.area_unit, u, name);
                    }
                });
            ui.end_row();
            decimals_field(ui, tr!("style-ui-area-precision"), &mut s.area_precision);
            ui.label(tr!("style-ui-area-show-unit"));
            ui.checkbox(&mut s.area_show_unit, "");
            ui.end_row();
            ui.label(tr!("style-ui-area-round-to"));
            ui.horizontal(|ui| {
                let mut on = s.area_round_to.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    set_enabled(&mut s.area_round_to, on, 1.0);
                }
                if let Some(r) = &mut s.area_round_to {
                    ui.add(DragValue::new(r).speed(0.1).range(0.0001..=1.0e9));
                    ui.label(s.area_unit.short_name());
                }
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    let [length, area, angle] = dimension_samples(s);
    ui.label(tr!(
        "style-ui-samples",
        length = length,
        area = area,
        angle = angle
    ));
}

fn text_fields(ui: &mut Ui, e: &mut StyleEditor) {
    let Some(s) = e.text() else { return };
    egui::Grid::new("style-ui-text")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr!("style-ui-font"));
            ui.text_edit_singleline(&mut s.font);
            ui.end_row();
            paper_field(ui, tr!("style-ui-size"), &mut s.size_mm, 0.1, 200.0);
            ui.label(tr!("style-ui-bold"));
            ui.checkbox(&mut s.bold, "");
            ui.end_row();
            ui.label(tr!("style-ui-italic"));
            ui.checkbox(&mut s.italic, "");
            ui.end_row();
            ui.label(tr!("style-ui-justify"));
            choice(
                ui,
                "style-ui-justify",
                &mut s.justify,
                &style_edit::JUSTIFICATIONS,
                style_edit::JUSTIFY_DOMAIN,
            );
            ui.end_row();
            ui.label(tr!("style-ui-line-spacing"));
            ui.add(
                DragValue::new(&mut s.line_spacing)
                    .speed(0.05)
                    .range(0.5..=5.0),
            );
            ui.end_row();
            color_field(ui, tr!("style-ui-color"), &mut s.color);
        });
}

fn tag_fields(ui: &mut Ui, e: &mut StyleEditor) {
    {
        let Some(s) = e.tag() else { return };
        tag_options(ui, s);
    }
    ui.add_space(6.0);
    ui.label(tr!("style-ui-lines"));
    list_buttons(ui, e);
    let selected = e.selected;
    let mut pick = None;
    if let Some(s) = e.tag() {
        for (i, line) in s.lines.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(i == selected, format!("{}", i + 1))
                    .clicked()
                {
                    pick = Some(i);
                }
                if ui.text_edit_singleline(line).gained_focus() {
                    pick = Some(i);
                }
            });
        }
    }
    if let Some(i) = pick {
        e.selected = i;
    }
}

fn tag_options(ui: &mut Ui, s: &mut TagStyle) {
    egui::Grid::new("style-ui-tag")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr!("style-ui-font"));
            ui.text_edit_singleline(&mut s.font);
            ui.end_row();
            paper_field(ui, tr!("style-ui-size"), &mut s.size_mm, 0.1, 200.0);
            ui.label(tr!("style-ui-frame"));
            choice(
                ui,
                "style-ui-frame",
                &mut s.frame,
                &style_edit::FRAMES,
                style_edit::FRAME_DOMAIN,
            );
            ui.end_row();
            paper_field(ui, tr!("style-ui-pad"), &mut s.pad_mm, 0.0, 50.0);
            ui.label(tr!("style-ui-leader"));
            ui.checkbox(&mut s.leader, "");
            ui.end_row();
            ui.label(tr!("style-ui-terminator"));
            choice(
                ui,
                "style-ui-tag-terminator",
                &mut s.terminator,
                &style_edit::TERMINATORS,
                style_edit::TERMINATOR_DOMAIN,
            );
            ui.end_row();
            color_field(ui, tr!("style-ui-color"), &mut s.color);
        });
}

fn notes_fields(ui: &mut Ui, e: &mut StyleEditor) {
    list_buttons(ui, e);
    let selected = e.selected;
    let mut pick = None;
    if let Some(n) = e.notes() {
        egui::Grid::new("style-ui-notes")
            .num_columns(3)
            .show(ui, |ui| {
                ui.strong("#");
                ui.strong(tr!("style-ui-key"));
                ui.strong(tr!("style-ui-note-text"));
                ui.end_row();
                for (i, note) in n.notes.iter_mut().enumerate() {
                    if ui
                        .selectable_label(i == selected, format!("{}", i + 1))
                        .clicked()
                    {
                        pick = Some(i);
                    }
                    if ui.text_edit_singleline(&mut note.key).gained_focus() {
                        pick = Some(i);
                    }
                    if ui.text_edit_singleline(&mut note.text).gained_focus() {
                        pick = Some(i);
                    }
                    ui.end_row();
                }
            });
    }
    if let Some(i) = pick {
        e.selected = i;
    }
}

/// Add, remove, up and down for the list of a tag style or a notes database.
fn list_buttons(ui: &mut Ui, e: &mut StyleEditor) {
    ui.horizontal(|ui| {
        if ui.button(tr!("style-ui-add-row")).clicked() {
            e.add_row();
        }
        let sel = e.selected;
        let has = e.rows() > 0;
        if ui
            .add_enabled(has, egui::Button::new(tr!("style-ui-remove-row")))
            .clicked()
        {
            e.remove_row(sel);
        }
        if ui
            .add_enabled(has && sel > 0, egui::Button::new(tr!("style-ui-move-up")))
            .clicked()
        {
            e.move_row(sel, -1);
        }
        if ui
            .add_enabled(
                has && sel + 1 < e.rows(),
                egui::Button::new(tr!("style-ui-move-down")),
            )
            .clicked()
        {
            e.move_row(sel, 1);
        }
    });
}

/// True for the resource types that have a style editor.
pub fn has_editor(type_name: &str) -> bool {
    StyleKind::from_type_name(type_name).is_some()
}

/// The content of the resource as an annotation resource, for the tests.
#[cfg(test)]
fn annot_of(
    session: &Session,
    id: ResourceId,
) -> Option<kadakas_core::annot::style::AnnotResource> {
    StyleEditor::open(session.editor.doc(), id).map(|e| e.draft)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::annot::style::AnnotResource;
    use kadakas_core::command::Command;

    fn frame(w: &mut StyleWindows, ctx: &Context, s: &mut Session, status: &mut String) {
        let _ = ctx.run_ui(Default::default(), |ui| {
            w.show(ui.ctx(), s, status);
        });
    }

    fn new_style(
        w: &mut StyleWindows,
        s: &mut Session,
        kind: StyleKind,
        status: &mut String,
    ) -> ResourceId {
        w.create(s, kind, status);
        w.open.last().expect("the editor opened").editor.id()
    }

    #[test]
    fn every_kind_draws_in_every_language() {
        for lang in kadakas_l10n::languages() {
            kadakas_l10n::with_language(lang.tag, || {
                let ctx = Context::default();
                let mut s = Session::with_sequential_ids();
                let mut w = StyleWindows::default();
                let mut status = String::new();
                for kind in StyleKind::ALL {
                    new_style(&mut w, &mut s, kind, &mut status);
                }
                assert_eq!(w.count(), 4, "{status}");
                // Rows in the lists, a tolerance and a color, so every field draws.
                for o in &mut w.open {
                    o.editor.add_row();
                    if let Some(d) = o.editor.dimension() {
                        d.tolerance = Some(default_tolerance());
                        d.round_to = Some(5.0);
                        d.area_round_to = Some(0.5);
                        d.color = Some(Color([1, 2, 3, 255]));
                    }
                    if let Some(t) = o.editor.text() {
                        t.color = Some(Color([9, 9, 9, 255]));
                    }
                    o.conflict = true;
                }
                frame(&mut w, &ctx, &mut s, &mut status);
                frame(&mut w, &ctx, &mut s, &mut status);
                assert_eq!(w.count(), 4);
                for o in &w.open {
                    assert!(!title(&o.editor).is_empty());
                }
            });
        }
    }

    #[test]
    fn save_is_one_undo_step_and_the_buttons_work() {
        let mut s = Session::with_sequential_ids();
        let mut w = StyleWindows::default();
        let mut status = String::new();
        let id = new_style(&mut w, &mut s, StyleKind::Dimension, &mut status);
        assert!(w.is_open(id));
        assert!(w.open(&s, id), "a second open keeps the window");
        assert_eq!(w.count(), 1);
        let before = s.editor.revision();
        let e = &mut w.open[0].editor;
        e.dimension().unwrap().area_precision = 5;
        assert!(save(e, &mut s, &mut status));
        assert_ne!(s.editor.revision(), before);
        assert!(!e.is_dirty());
        assert!(matches!(
            annot_of(&s, id),
            Some(AnnotResource::DimensionStyle(d)) if d.area_precision == 5
        ));
        // Nothing changed: nothing runs.
        let rev = s.editor.revision();
        assert!(save(e, &mut s, &mut status));
        assert_eq!(s.editor.revision(), rev);
        // A bad draft is refused with a message.
        e.dimension().unwrap().area_precision = 9;
        assert!(!save(e, &mut s, &mut status));
        assert!(
            status.contains('9') || status.contains("precision"),
            "{status}"
        );
        // A refused command (the style was deleted) keeps the window dirty.
        e.revert();
        e.name = "Renamed".into();
        s.editor
            .execute(&Command::DeleteResource { id, reassign: None })
            .unwrap();
        assert!(!save(e, &mut s, &mut status));
        // The window closes when its style is gone.
        let ctx = Context::default();
        frame(&mut w, &ctx, &mut s, &mut status);
        assert_eq!(w.count(), 0);
        assert!(!w.open(&s, id));
    }

    #[test]
    fn helper_functions() {
        assert!(has_editor("tag_style") && !has_editor("hatch"));
        for k in StyleKind::ALL {
            assert!(!kind_label(k).is_empty());
        }
        let _ = TagStyle::default();
    }
}
