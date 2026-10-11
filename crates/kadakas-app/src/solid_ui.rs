//! Extrude, sweep and 3D polygon in the app (3D-03): the Model > Extrude and Model > Sweep
//! dialogs, the Object Info fields and the options of the 3D Polygon tool.
//!
//! The dialogs only collect values. The batch that makes the solids is in
//! `kadakas_core::solid_batch`, which the API uses too.

use egui::{DragValue, Ui};
use kadakas_core::model::ObjectId;
use kadakas_core::solid_batch::{ExtrudeOptions, SweepOptions};
use kadakas_core::solid3d::{self, Extrude, Sweep};
use kadakas_l10n::tr;
use kadakas_ui_core::polygon3d::Polygon3dSettings;

use crate::panels::Fields;
use crate::units_ui::DragValueExt as _;

/// What the dialog asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Extrude(ExtrudeOptions),
    /// Sweep options, and whether the selected line is the axis.
    Sweep(SweepOptions, bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Extrude,
    Sweep,
}

/// The values of the Extrude and Sweep dialogs.
#[derive(Clone, Debug, Default)]
pub struct SolidDialog {
    open: Option<Kind>,
    extrude: ExtrudeOptions,
    sweep: SweepOptions,
    /// True: the selected line is the axis. False: a vertical axis at the profile.
    axis_is_line: bool,
}

impl SolidDialog {
    pub fn open_extrude(&mut self) {
        self.open = Some(Kind::Extrude);
    }

    pub fn open_sweep(&mut self) {
        self.open = Some(Kind::Sweep);
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The request for the current values.
    fn request(&self, kind: Kind) -> Request {
        match kind {
            Kind::Extrude => Request::Extrude(self.extrude.clone()),
            Kind::Sweep => Request::Sweep(self.sweep.clone(), self.axis_is_line),
        }
    }

    /// Shows the dialog. Returns the request when the person presses OK.
    pub fn show(&mut self, ctx: &egui::Context, selection: &[ObjectId]) -> Option<Request> {
        let kind = self.open?;
        let mut run = None;
        let mut open = true;
        let (title, id) = match kind {
            Kind::Extrude => (tr!("solid-ui-extrude"), "win-extrude"),
            Kind::Sweep => (tr!("solid-ui-sweep"), "win-sweep"),
        };
        egui::Window::new(title)
            .id(egui::Id::new(id))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("solid_dialog")
                    .num_columns(2)
                    .show(ui, |ui| match kind {
                        Kind::Extrude => {
                            ui.label(tr!("solid-ui-extrusion"));
                            ui.add(
                                DragValue::new(&mut self.extrude.height)
                                    .speed(10.0)
                                    .range(-1.0e7..=1.0e7)
                                    .length(),
                            );
                            ui.end_row();
                            ui.label(tr!("solid-ui-base"));
                            ui.add(
                                DragValue::new(&mut self.extrude.base)
                                    .speed(10.0)
                                    .range(-1.0e7..=1.0e7)
                                    .length(),
                            );
                            ui.end_row();
                            let mut keep = !self.extrude.delete_source;
                            ui.label("");
                            ui.checkbox(&mut keep, tr!("solid-ui-keep-the-shapes"));
                            self.extrude.delete_source = !keep;
                            ui.end_row();
                        }
                        Kind::Sweep => {
                            ui.label(tr!("solid-ui-sweep-angle"));
                            ui.add(
                                DragValue::new(&mut self.sweep.sweep_angle_deg)
                                    .speed(1.0)
                                    .range(1.0..=solid3d::MAX_SWEEP_ANGLE)
                                    .degrees(),
                            );
                            ui.end_row();
                            ui.label(tr!("solid-ui-segment-angle"));
                            ui.add(
                                DragValue::new(&mut self.sweep.segment_angle_deg)
                                    .speed(0.5)
                                    .range(solid3d::MIN_SEGMENT_ANGLE..=180.0)
                                    .degrees(),
                            );
                            ui.end_row();
                            ui.label(tr!("solid-ui-pitch"));
                            ui.add(
                                DragValue::new(&mut self.sweep.pitch)
                                    .speed(10.0)
                                    .range(-1.0e7..=1.0e7)
                                    .length(),
                            );
                            ui.end_row();
                            ui.label(tr!("solid-ui-axis"));
                            ui.vertical(|ui| {
                                ui.radio_value(
                                    &mut self.axis_is_line,
                                    false,
                                    tr!("solid-ui-vertical-axis-at-the-profile"),
                                );
                                ui.radio_value(
                                    &mut self.axis_is_line,
                                    true,
                                    tr!("solid-ui-the-selected-line"),
                                );
                            });
                            ui.end_row();
                            let mut keep = !self.sweep.delete_source;
                            ui.label("");
                            ui.checkbox(&mut keep, tr!("solid-ui-keep-the-shapes"));
                            self.sweep.delete_source = !keep;
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let ok = ui
                        .add_enabled(!selection.is_empty(), egui::Button::new(tr!("solid-ui-ok")));
                    if ok.clicked() {
                        run = Some(self.request(kind));
                    }
                    if selection.is_empty() {
                        ui.weak(tr!("solid-ui-select-2d-shapes-first"));
                    }
                });
            });
        if run.is_some() || !open {
            self.open = None;
        }
        run
    }
}

/// A number of degrees (not a direction) as a field.
fn degrees(f: &mut Fields, ui: &mut Ui, label: &str, deg: &mut f64, min: f64, max: f64) {
    ui.label(label);
    let r = ui.add(DragValue::new(deg).speed(1.0).range(min..=max).degrees());
    f.commit |= r.drag_stopped() || r.lost_focus();
    ui.end_row();
}

/// Object Info of an extrude.
pub fn extrude_fields(ui: &mut Ui, f: &mut Fields, e: &mut Extrude) {
    f.text(ui, &tr!("solid-ui-name"), &mut e.name);
    f.length_scalar(ui, &tr!("solid-ui-extrusion"), &mut e.height, 10.0);
    f.length_scalar(ui, &tr!("solid-ui-base"), &mut e.base, 10.0);
    f.count(ui, e.profile.len());
    if !e.holes.is_empty() {
        ui.label(tr!("solid-ui-holes"));
        ui.label(format!("{}", e.holes.len()));
        ui.end_row();
    }
    // The figures are made again only when a value changed, not in every frame.
    let (problem, volume) = cached(ui, e.fingerprint(), || {
        let problem = e.problem();
        let volume = problem.is_none().then(|| e.volume());
        (problem, volume)
    });
    ui.label(tr!("solid-ui-volume"));
    ui.label(match volume {
        Some(v) => kadakas_units::current().volume_text(v),
        None => "-".to_string(),
    });
    ui.end_row();
    warning(ui, problem);
}

/// Figures for Object Info, kept by the fingerprint of the solid.
fn cached<T: Clone + Send + Sync + 'static>(
    ui: &Ui,
    fingerprint: u64,
    make: impl FnOnce() -> T,
) -> T {
    let id = ui.id().with(("solid-info", std::any::type_name::<T>()));
    if let Some((f, v)) = ui.data(|d| d.get_temp::<(u64, T)>(id))
        && f == fingerprint
    {
        return v;
    }
    let v = make();
    ui.data_mut(|d| d.insert_temp(id, (fingerprint, v.clone())));
    v
}

fn warning(ui: &mut Ui, problem: Option<String>) {
    if let Some(why) = problem {
        ui.label(
            egui::RichText::new(why)
                .small()
                .color(egui::Color32::from_rgb(190, 110, 0)),
        );
        ui.end_row();
    }
}

/// Object Info of a sweep.
pub fn sweep_fields(ui: &mut Ui, f: &mut Fields, s: &mut Sweep) {
    f.text(ui, &tr!("solid-ui-name"), &mut s.name);
    ui.label(tr!("solid-ui-closed-profile"));
    f.commit |= ui.checkbox(&mut s.closed, "").changed();
    ui.end_row();
    degrees(
        f,
        ui,
        &tr!("solid-ui-sweep-angle"),
        &mut s.sweep_angle_deg,
        1.0,
        solid3d::MAX_SWEEP_ANGLE,
    );
    degrees(
        f,
        ui,
        &tr!("solid-ui-segment-angle"),
        &mut s.segment_angle_deg,
        solid3d::MIN_SEGMENT_ANGLE,
        180.0,
    );
    f.length_scalar(ui, &tr!("solid-ui-pitch"), &mut s.pitch, 10.0);
    degrees(
        f,
        ui,
        &tr!("solid-ui-profile-angle"),
        &mut s.rotation_deg,
        -360.0,
        360.0,
    );
    ui.label(tr!("solid-ui-axis-point"));
    ui.horizontal(|ui| {
        f.num(ui, &mut s.origin.x, 10.0);
        f.num(ui, &mut s.origin.y, 10.0);
        f.num(ui, &mut s.origin.z, 10.0);
    });
    ui.end_row();
    ui.label(tr!("solid-ui-axis-direction"));
    ui.horizontal(|ui| {
        f.plain(ui, &mut s.axis.x, 0.1);
        f.plain(ui, &mut s.axis.y, 0.1);
        f.plain(ui, &mut s.axis.z, 0.1);
    });
    ui.end_row();
    f.count(ui, s.profile.len());
    ui.label(tr!("solid-ui-segments"));
    ui.label(if s.segments_capped() {
        tr!("solid-ui-segments-capped", count = s.segments())
    } else {
        format!("{}", s.segments())
    });
    ui.end_row();
    let (problem, volume) = cached(ui, s.fingerprint(), || {
        let problem = s.problem();
        let volume = if problem.is_none() && s.closed {
            s.mesh().map(|m| m.volume())
        } else {
            None
        };
        (problem, volume)
    });
    if let Some(v) = volume {
        ui.label(tr!("solid-ui-volume"));
        ui.label(kadakas_units::current().volume_text(v));
        ui.end_row();
    }
    warning(ui, problem);
}

/// Object Info of a 3D polygon: the heights of the vertices move together.
pub fn polygon3d_fields(
    ui: &mut Ui,
    f: &mut Fields,
    points: &mut [glam::DVec3],
    closed: &mut bool,
) {
    ui.label(tr!("panels-closed"));
    f.commit |= ui.checkbox(closed, "").changed();
    ui.end_row();
    ui.label(tr!("panels-vertices"));
    ui.label(format!("{}", points.len()));
    ui.end_row();
    let (lo, hi) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.z), hi.max(p.z))
        });
    if points.is_empty() {
        return;
    }
    // The lowest height: a change moves all vertices by the same amount.
    let mut low = lo;
    f.length_scalar(ui, &tr!("solid-ui-lowest-point"), &mut low, 10.0);
    if low != lo && low.is_finite() {
        for p in points.iter_mut() {
            p.z += low - lo;
        }
    }
    ui.label(tr!("panels-z-range"));
    ui.label(tr!(
        "panels-range-to",
        low = kadakas_units::current().length_text_d(lo, 1),
        high = kadakas_units::current().length_text_d(hi, 1)
    ));
    ui.end_row();
}

/// The options of the 3D Polygon tool.
pub fn polygon3d_bar(s: &mut Polygon3dSettings, ui: &mut Ui) {
    ui.label(tr!("solid-ui-height-of-new-points"));
    ui.add(
        DragValue::new(&mut s.z)
            .speed(10.0)
            .range(-1.0e7..=1.0e7)
            .length(),
    );
    ui.checkbox(&mut s.closed, tr!("panels-closed"));
}
