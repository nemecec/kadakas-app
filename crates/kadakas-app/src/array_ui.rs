//! The Duplicate Array dialog (Edit > Duplicate Array). It builds an `ArrayObjects` command.
//! The fields follow the Duplicate Array command of Vectorworks: linear, rectangular and circular
//! arrays, resize and rotate duplicates, and keeping or removing the original.

use crate::units_ui::DragValueExt as _;
use egui::DragValue;
use glam::DVec2;
use kadakas_core::command::Command;
use kadakas_core::edit2d::spec::{ArrayLayout, ArrayParams};
use kadakas_core::model::ObjectId;
use kadakas_l10n::tr;
use kadakas_view3d::numfmt::UiNumber;

fn len(v: &mut f64) -> DragValue<'_> {
    DragValue::new(v).speed(10.0).range(-1.0e7..=1.0e7).length()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    #[default]
    Linear,
    Rectangular,
    Circular,
}

impl Shape {
    const ALL: [Shape; 3] = [Shape::Linear, Shape::Rectangular, Shape::Circular];

    fn label(self) -> String {
        match self {
            Shape::Linear => tr!("array-ui-linear"),
            Shape::Rectangular => tr!("array-ui-rectangular"),
            Shape::Circular => tr!("array-ui-circular"),
        }
    }
}

/// The values of the dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrayDialog {
    pub open: bool,
    pub shape: Shape,
    // Linear
    pub count: usize,
    pub polar: bool,
    pub dx: f64,
    pub dy: f64,
    pub distance: f64,
    pub angle_deg: f64,
    // Rectangular
    pub columns: usize,
    pub rows: usize,
    pub column_spacing: f64,
    pub row_spacing: f64,
    pub grid_angle_deg: f64,
    // Circular
    pub center: DVec2,
    pub step_deg: f64,
    pub rotate_objects: bool,
    // All
    pub resize: bool,
    pub resize_factor: f64,
    pub rotate: bool,
    pub rotate_deg: f64,
    pub retain: bool,
}

impl Default for ArrayDialog {
    fn default() -> Self {
        ArrayDialog {
            open: false,
            shape: Shape::Linear,
            count: 3,
            polar: false,
            dx: 1000.0,
            dy: 0.0,
            distance: 1000.0,
            angle_deg: 0.0,
            columns: 3,
            rows: 2,
            column_spacing: 1000.0,
            row_spacing: 1000.0,
            grid_angle_deg: 0.0,
            center: DVec2::ZERO,
            step_deg: 60.0,
            rotate_objects: true,
            resize: false,
            resize_factor: 1.1,
            rotate: false,
            rotate_deg: 10.0,
            retain: true,
        }
    }
}

impl ArrayDialog {
    pub fn open(&mut self) {
        self.open = true;
    }

    /// The array that the values describe.
    pub fn params(&self) -> ArrayParams {
        let layout = match self.shape {
            Shape::Linear if self.polar => ArrayLayout::Linear {
                count: self.count,
                offset: None,
                distance: Some(self.distance),
                angle: Some(self.angle_deg.to_radians()),
            },
            Shape::Linear => ArrayLayout::Linear {
                count: self.count,
                offset: Some(DVec2::new(self.dx, self.dy)),
                distance: None,
                angle: None,
            },
            Shape::Rectangular => ArrayLayout::Rectangular {
                columns: self.columns,
                rows: self.rows,
                column_spacing: self.column_spacing,
                row_spacing: self.row_spacing,
                angle: self.grid_angle_deg.to_radians(),
            },
            Shape::Circular => ArrayLayout::Circular {
                count: self.count,
                center: self.center,
                angle: Some(self.step_deg.to_radians()),
                rotate_objects: self.rotate_objects,
            },
        };
        ArrayParams {
            layout,
            resize: self.resize.then_some(self.resize_factor),
            rotate: self.rotate.then_some(self.rotate_deg.to_radians()),
            retain: self.retain,
        }
    }

    /// The command for the objects.
    pub fn command(&self, ids: Vec<ObjectId>) -> Command {
        Command::ArrayObjects {
            ids,
            array: self.params(),
        }
    }

    /// Shows the dialog. Returns the command when the user presses OK.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        selection: &[ObjectId],
        center_hint: Option<DVec2>,
    ) -> Option<Command> {
        if !self.open {
            return None;
        }
        let mut run = None;
        let mut open = true;
        egui::Window::new(tr!("array-ui-duplicate-array"))
            .id(egui::Id::new("win-duplicate-array"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for s in Shape::ALL {
                        ui.selectable_value(&mut self.shape, s, s.label());
                    }
                });
                ui.separator();
                match self.shape {
                    Shape::Linear => {
                        ui.horizontal(|ui| {
                            ui.label(tr!("array-ui-number-of-duplicates"));
                            ui.add(DragValue::new(&mut self.count).range(1..=1000).ui_number());
                        });
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.polar,
                                false,
                                tr!("array-ui-cartesian-offset"),
                            );
                            ui.selectable_value(
                                &mut self.polar,
                                true,
                                tr!("array-ui-polar-offset"),
                            );
                        });
                        if self.polar {
                            ui.horizontal(|ui| {
                                ui.label(tr!("array-ui-r"));
                                ui.add(len(&mut self.distance));
                                ui.label(tr!("array-ui-angle"));
                                ui.add(DragValue::new(&mut self.angle_deg).speed(1.0).direction());
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.label(tr!("array-ui-x"));
                                ui.add(len(&mut self.dx));
                                ui.label(tr!("array-ui-y"));
                                ui.add(len(&mut self.dy));
                            });
                        }
                    }
                    Shape::Rectangular => {
                        ui.horizontal(|ui| {
                            ui.label(tr!("array-ui-columns"));
                            ui.add(
                                DragValue::new(&mut self.columns)
                                    .range(1..=1000)
                                    .ui_number(),
                            );
                            ui.label(tr!("array-ui-rows"));
                            ui.add(DragValue::new(&mut self.rows).range(1..=1000).ui_number());
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr!("array-ui-distance-between-columns"));
                            ui.add(len(&mut self.column_spacing));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr!("array-ui-distance-between-rows"));
                            ui.add(len(&mut self.row_spacing));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr!("array-ui-turn-the-grid"));
                            ui.add(
                                DragValue::new(&mut self.grid_angle_deg)
                                    .speed(1.0)
                                    .direction(),
                            );
                        });
                    }
                    Shape::Circular => {
                        ui.horizontal(|ui| {
                            ui.label(tr!("array-ui-number-of-duplicates"));
                            ui.add(DragValue::new(&mut self.count).range(1..=1000).ui_number());
                            ui.label(tr!("array-ui-angle-between"));
                            ui.add(
                                DragValue::new(&mut self.step_deg)
                                    .speed(1.0)
                                    .range(-360.0..=360.0)
                                    .degrees(),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr!("array-ui-center-x"));
                            ui.add(len(&mut self.center.x));
                            ui.label(tr!("array-ui-y"));
                            ui.add(len(&mut self.center.y));
                        });
                        if let Some(c) = center_hint
                            && ui
                                .button(tr!("array-ui-use-the-center-of-the-selection"))
                                .clicked()
                        {
                            self.center = c;
                        }
                        ui.checkbox(
                            &mut self.rotate_objects,
                            tr!("array-ui-rotate-duplicates-with-the-circle"),
                        );
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.resize, tr!("array-ui-resize-duplicates"));
                    ui.add_enabled(
                        self.resize,
                        DragValue::new(&mut self.resize_factor)
                            .speed(0.01)
                            .range(0.01..=100.0)
                            .prefix("x ")
                            .ui_number(),
                    );
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.rotate, tr!("array-ui-rotate-duplicates"));
                    ui.add_enabled(
                        self.rotate,
                        DragValue::new(&mut self.rotate_deg).speed(1.0).direction(),
                    );
                });
                ui.checkbox(&mut self.retain, tr!("array-ui-retain-the-original-object"));
                ui.separator();
                ui.horizontal(|ui| {
                    let ok = ui
                        .add_enabled(!selection.is_empty(), egui::Button::new(tr!("array-ui-ok")));
                    if ok.clicked() {
                        run = Some(self.command(selection.to_vec()));
                    }
                    if selection.is_empty() {
                        ui.weak(tr!("array-ui-select-objects-first"));
                    }
                });
            });
        if run.is_some() || !open {
            self.open = false;
        }
        run
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::Editor;
    use kadakas_core::model::{Attributes, Shape as ModelShape};

    #[test]
    fn every_shape_makes_a_command_that_runs() {
        let mut ed = Editor::default();
        let id = ObjectId(
            ed.execute(&Command::AddObject {
                shape: ModelShape::Circle {
                    center: DVec2::new(1000.0, 0.0),
                    radius: 100.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let mut d = ArrayDialog::default();
        let mut total = 1;
        for (shape, polar) in [
            (Shape::Linear, false),
            (Shape::Linear, true),
            (Shape::Rectangular, false),
            (Shape::Circular, false),
        ] {
            d.shape = shape;
            d.polar = polar;
            let out = ed.execute(&d.command(vec![id])).unwrap();
            let want = match shape {
                Shape::Rectangular => d.columns * d.rows - 1,
                _ => d.count,
            };
            assert_eq!(out.created.len(), want, "{shape:?}");
            total += want;
        }
        assert_eq!(ed.doc().objects.len(), total);
        // Resize, rotate and not keeping the original.
        d.shape = Shape::Linear;
        d.resize = true;
        d.rotate = true;
        d.retain = false;
        let p = d.params();
        assert_eq!(p.resize, Some(1.1));
        assert!(p.rotate.is_some() && !p.retain);
        ed.execute(&d.command(vec![id])).unwrap();
        assert!(ed.doc().object(id).is_none());
    }

    #[test]
    fn the_dialog_shows_and_stays_closed_until_opened() {
        let ctx = egui::Context::default();
        let mut d = ArrayDialog::default();
        assert!(d.show(&ctx, &[], None).is_none());
        d.open();
        for shape in Shape::ALL {
            d.shape = shape;
            d.polar = shape == Shape::Linear;
            let _ = ctx.run_ui(Default::default(), |_| {
                assert!(
                    d.show(&ctx, &[ObjectId::from_num(2)], Some(DVec2::new(1.0, 2.0)))
                        .is_none()
                );
            });
        }
        assert!(d.open);
    }
}
