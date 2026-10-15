//! The joint pattern editor of a hardscape style (HRD-08, PAT-05): a bond with its sizes, or a
//! module (a preset with numbers, or the closed shapes of the selection), with a small preview. The
//! form logic is in `kadakas_ui_core::joint_edit`; this file only draws the fields. A save is one
//! `ModifyResource` command, so it is one undo step.

use crate::units_ui::DragValueExt as _;
use egui::{Color32, Context, DragValue, Pos2, Sense, Stroke, Ui, Vec2};
use kadakas_api::Session;
use kadakas_core::hardscape::style::JointKind;
use kadakas_core::resource::ResourceId;
use kadakas_l10n::tr;
use kadakas_ui_core::joint_edit::{JointEditor, ModuleSource, preset_names};

use crate::panels;

/// One open window.
struct Open {
    editor: JointEditor,
    /// The text under the form: a problem, or the note about the lattice.
    message: String,
}

/// The open joint pattern editors.
#[derive(Default)]
pub struct JointWindows {
    open: Vec<Open>,
}

impl JointWindows {
    /// Opens an editor on a hardscape style. A style that has a window already keeps it.
    pub fn open(&mut self, session: &Session, id: ResourceId) -> bool {
        if self.open.iter().any(|o| o.editor.id() == id) {
            return true;
        }
        match JointEditor::open(session.editor.doc(), id) {
            Some(editor) => {
                self.open.push(Open {
                    editor,
                    message: String::new(),
                });
                true
            }
            None => false,
        }
    }

    pub fn is_open(&self, id: ResourceId) -> bool {
        self.open.iter().any(|o| o.editor.id() == id)
    }

    pub fn count(&self) -> usize {
        self.open.len()
    }

    pub fn show(&mut self, ctx: &Context, session: &mut Session, status: &mut String) {
        let mut done: Vec<usize> = Vec::new();
        for (i, w) in self.open.iter_mut().enumerate() {
            if session
                .editor
                .doc()
                .resources
                .hardscape_style(w.editor.id())
                .is_none()
            {
                done.push(i);
                continue;
            }
            let name = session
                .editor
                .doc()
                .resources
                .get(w.editor.id())
                .map(|r| r.name.clone())
                .unwrap_or_default();
            let mut open = true;
            let mut close = false;
            egui::Window::new(tr!("joint-ui-title", name = name))
                .id(egui::Id::new(("win-joints", w.editor.id().0)))
                .open(&mut open)
                .default_width(360.0)
                .show(ctx, |ui| {
                    close = body(ui, w, session, status);
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

fn bond_label(k: JointKind) -> String {
    match k {
        JointKind::RunningBond => tr!("joint-ui-bond-running-bond"),
        JointKind::Stack => tr!("joint-ui-bond-stack"),
        JointKind::Herringbone => tr!("joint-ui-bond-herringbone"),
        JointKind::BasketWeave => tr!("joint-ui-bond-basket-weave"),
        JointKind::Module => tr!("joint-ui-bond-module"),
    }
}

fn preset_label(name: &str) -> String {
    match name {
        "hexagon" => tr!("joint-ui-preset-hexagon"),
        "octagon_square" => tr!("joint-ui-preset-octagon-square"),
        _ => tr!("joint-ui-preset-i-shape"),
    }
}

/// Draws the form. True when the window should close.
fn body(ui: &mut Ui, w: &mut Open, session: &mut Session, status: &mut String) -> bool {
    let mut close = false;
    let e = &mut w.editor;
    let kinds = [
        JointKind::RunningBond,
        JointKind::Stack,
        JointKind::Herringbone,
        JointKind::BasketWeave,
        JointKind::Module,
    ];
    let mut kind = e.draft.kind;
    egui::ComboBox::from_id_salt("joint-kind")
        .selected_text(bond_label(kind))
        .show_ui(ui, |ui| {
            for k in kinds {
                ui.selectable_value(&mut kind, k, bond_label(k));
            }
        });
    if kind != e.draft.kind {
        e.set_bond(kind);
        w.message.clear();
    }
    let e = &mut w.editor;
    egui::Grid::new("joint-numbers")
        .num_columns(2)
        .show(ui, |ui| {
            if e.draft.kind != JointKind::Module {
                ui.label(tr!("joint-ui-unit-length"));
                ui.add(
                    DragValue::new(&mut e.draft.unit_length)
                        .range(10.0..=5000.0)
                        .length(),
                );
                ui.end_row();
                ui.label(tr!("joint-ui-unit-width"));
                ui.add(
                    DragValue::new(&mut e.draft.unit_width)
                        .range(10.0..=5000.0)
                        .length(),
                );
                ui.end_row();
            }
            ui.label(tr!("joint-ui-gap"));
            ui.add(
                DragValue::new(&mut e.draft.spacing)
                    .range(0.0..=100.0)
                    .speed(0.1)
                    .length(),
            );
            ui.end_row();
            ui.label(tr!("joint-ui-angle"));
            ui.add(DragValue::new(&mut e.draft.angle).speed(1.0).degrees());
            ui.end_row();
        });
    if e.draft.kind == JointKind::Module {
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for name in preset_names() {
                let on = e.source == Some(ModuleSource::Preset(name));
                if ui.selectable_label(on, preset_label(name)).clicked() {
                    w.message = e.choose_preset(name).err().map(|p| p.0).unwrap_or_default();
                }
            }
            if ui.button(tr!("joint-ui-from-selection")).clicked() {
                let ids = session.selection.clone();
                w.message = match e.from_shapes(session.editor.doc(), &ids, None, None) {
                    Ok(note) => note.unwrap_or_default(),
                    Err(p) => p.0,
                };
            }
        });
        if let Some(ModuleSource::Preset(name)) = e.source.clone() {
            let mut changed = false;
            egui::Grid::new("joint-preset")
                .num_columns(2)
                .show(ui, |ui| match name {
                    "i_shape" => {
                        ui.label(tr!("joint-ui-unit-length"));
                        changed |= ui
                            .add(
                                DragValue::new(&mut e.numbers[1])
                                    .range(20.0..=2000.0)
                                    .length(),
                            )
                            .changed();
                        ui.end_row();
                        ui.label(tr!("joint-ui-unit-width"));
                        changed |= ui
                            .add(
                                DragValue::new(&mut e.numbers[2])
                                    .range(20.0..=2000.0)
                                    .length(),
                            )
                            .changed();
                        ui.end_row();
                        ui.label(tr!("joint-ui-notch"));
                        changed |= ui
                            .add(
                                DragValue::new(&mut e.numbers[3])
                                    .range(1.0..=1000.0)
                                    .length(),
                            )
                            .changed();
                        ui.end_row();
                    }
                    _ => {
                        if name == "hexagon" {
                            changed |= ui
                                .checkbox(&mut e.across_flats, tr!("joint-ui-across-flats"))
                                .changed();
                            ui.end_row();
                        }
                        ui.label(if e.across_flats {
                            tr!("joint-ui-across-flats")
                        } else {
                            tr!("joint-ui-side")
                        });
                        changed |= ui
                            .add(
                                DragValue::new(&mut e.numbers[0])
                                    .range(5.0..=2000.0)
                                    .length(),
                            )
                            .changed();
                        ui.end_row();
                    }
                });
            if changed {
                w.message = e.rebuild_preset().err().map(|p| p.0).unwrap_or_default();
            }
        } else if !e.draft.units.is_empty() {
            ui.small(tr!("joint-ui-shapes", count = e.draft.units.len()));
        }
    }
    // The preview.
    match e.preview() {
        Ok((size, units)) => {
            let side = 200.0f32;
            let k = (side as f64 / size.x.max(size.y)) as f32;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_gray(245));
            let colors = [
                Color32::from_rgb(190, 170, 140),
                Color32::from_rgb(150, 170, 190),
                Color32::from_rgb(170, 190, 150),
                Color32::from_rgb(190, 150, 170),
            ];
            let map = |p: glam::DVec2| {
                Pos2::new(rect.min.x + p.x as f32 * k, rect.max.y - p.y as f32 * k)
            };
            for u in &units {
                let pts: Vec<Pos2> = u.points.iter().map(|p| map(*p)).collect();
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    colors[u.kind % colors.len()],
                    Stroke::new(0.5, Color32::from_gray(90)),
                ));
            }
        }
        Err(p) => {
            if w.message.is_empty() {
                w.message = p.0;
            }
        }
    }
    if !w.message.is_empty() {
        ui.colored_label(ui.visuals().warn_fg_color, &w.message);
    }
    ui.horizontal(|ui| {
        let dirty = w.editor.is_dirty();
        if ui
            .add_enabled(dirty, egui::Button::new(tr!("joint-ui-save")))
            .clicked()
        {
            match w.editor.save_command(session.editor.doc()) {
                Ok(Some(cmd)) => {
                    if panels::run_command(session, &cmd, status).is_some() {
                        w.editor.mark_saved();
                        w.message.clear();
                    }
                }
                Ok(None) => {}
                Err(p) => w.message = p.0,
            }
        }
        if ui.button(tr!("joint-ui-close")).clicked() {
            close = true;
        }
    });
    close
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session() -> (Session, ResourceId) {
        let mut s = Session::with_sequential_ids();
        let id = s
            .handle("hardscape_style.create", json!({"name": "P"}))
            .unwrap()["id"]
            .clone();
        (s, serde_json::from_value(id).unwrap())
    }

    #[test]
    fn the_window_draws_every_kind_and_saves_a_module_in_one_step() {
        let (mut s, id) = session();
        let mut w = JointWindows::default();
        assert!(w.open(&s, id) && w.is_open(id) && w.open(&s, id));
        assert_eq!(w.count(), 1);
        assert!(!w.open(&s, ResourceId::from_num(999)));
        let ctx = Context::default();
        let mut status = String::new();
        let mut frame = |w: &mut JointWindows, s: &mut Session| {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                w.show(ui.ctx(), s, &mut String::new());
            });
        };
        frame(&mut w, &mut s);
        for k in [
            JointKind::Herringbone,
            JointKind::BasketWeave,
            JointKind::Module,
        ] {
            w.open[0].editor.set_bond(k);
            frame(&mut w, &mut s);
        }
        for name in preset_names() {
            w.open[0].editor.choose_preset(name).unwrap();
            frame(&mut w, &mut s);
        }
        // The body saves through the same command as the tests of the form.
        let cmd = w.open[0]
            .editor
            .save_command(s.editor.doc())
            .unwrap()
            .unwrap();
        panels::run_command(&mut s, &cmd, &mut status).unwrap();
        let joints = s
            .editor
            .doc()
            .resources
            .hardscape_style(id)
            .unwrap()
            .joints
            .clone()
            .unwrap();
        assert_eq!(joints.kind, JointKind::Module);
        // A style that is deleted closes its window.
        s.handle("hardscape_style.delete", json!({"id": id}))
            .unwrap();
        frame(&mut w, &mut s);
        assert_eq!(w.count(), 0);
    }

    #[test]
    fn the_strings_exist_in_both_languages() {
        let en = include_str!("../../kadakas-l10n/locales/en-US/area-ui.ftl");
        let et = include_str!("../../kadakas-l10n/locales/et/area-ui.ftl");
        for key in [
            "joint-ui-title",
            "joint-ui-bond-module",
            "joint-ui-preset-hexagon",
            "joint-ui-preset-octagon-square",
            "joint-ui-preset-i-shape",
            "joint-ui-from-selection",
            "joint-ui-save",
        ] {
            assert!(en.contains(&format!("{key} =")), "{key} in en-US");
            assert!(et.contains(&format!("{key} =")), "{key} in et");
        }
    }
}
