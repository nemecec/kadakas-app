//! Site model parts of the app: contours on the plan, the Object Info rows and the
//! "Create Site Model" command. The terrain math is in `kadakas-site`.

use crate::units_ui::DragValueExt as _;
use egui::epaint::{PathShape, PathStroke};
use egui::{Color32, DragValue, Painter, Ui};
use kadakas_api::Session;
use kadakas_core::curve::Vertex;
use kadakas_core::model::{Document, ModifierKind, Object, ObjectId, Shape, SiteSettings};
use kadakas_core::resource::ResourceId;
use kadakas_core::site::{
    Display3d, MAX_ELEVATION_BANDS, MAX_RAMP_STOPS, PadSlope, SurfaceDisplay, SurfaceLook,
    boundary_points, end_elevation, slope_from_end_elevation,
};
use kadakas_l10n::tr;
use kadakas_site::draw::shown_surfaces;
use kadakas_site::{SiteCache, Surface};
use serde_json::json;

use crate::canvas::color32;
use crate::view::{View, ViewExt as _};

const EXISTING_LINE: Color32 = Color32::from_rgb(150, 104, 60);
const PROPOSED_LINE: Color32 = Color32::from_rgb(30, 120, 50);

/// The selected objects that can be site model source data (3D loci and 3D polylines).
pub fn source_ids(doc: &Document, selection: &[ObjectId]) -> Vec<ObjectId> {
    selection
        .iter()
        .copied()
        .filter(|id| {
            doc.object(*id).is_some_and(|o| {
                matches!(o.shape, Shape::Locus3d { .. } | Shape::Polyline3d { .. })
            })
        })
        .collect()
}

/// Makes a site model from the selected loci and 3D polylines, which move into it.
/// Returns the new object, or the message to show.
pub fn create_site_model(session: &mut Session) -> Result<ObjectId, String> {
    let ids = source_ids(session.editor.doc(), &session.selection);
    if ids.is_empty() {
        return Err(tr!("site-ui-select-3d-loci-or-3d-polylines-first"));
    }
    let r = session
        .handle("site.create", json!({ "source_ids": ids }))
        .map_err(|e| e.message)?;
    let id = ObjectId(kadakas_core::uid::AsUid::as_uid(&r["id"]).unwrap_or_default());
    session.selection = vec![id];
    Ok(id)
}

/// The selected site models.
pub fn selected_site_models(doc: &Document, selection: &[ObjectId]) -> Vec<ObjectId> {
    selection
        .iter()
        .copied()
        .filter(|id| {
            doc.object(*id)
                .is_some_and(|o| matches!(o.shape, Shape::SiteModel { .. }))
        })
        .collect()
}

/// "Update Site Model": rebuilds the surfaces of the selected site models now.
/// Returns the message to show.
pub fn update_site_models(session: &mut Session) -> String {
    let ids = selected_site_models(session.editor.doc(), &session.selection);
    if ids.is_empty() {
        return tr!("site-ui-select-a-site-model-first");
    }
    for id in &ids {
        if let Err(e) = session.handle("site.refresh", json!({ "id": id })) {
            return tr!("site-ui-error", message = e.message);
        }
    }
    tr!("site-ui-updated-site-model-s", count = ids.len())
}

/// Draws the contours of a site model over its outline, for the surfaces its settings choose.
pub fn draw_contours(
    painter: &Painter,
    view: &View,
    doc: &Document,
    obj: &Object,
    cache: &SiteCache,
    revision: u64,
    base_width: f32,
) {
    let Ok(site) = cache.get(revision, doc, obj.id) else {
        return;
    };
    for which in shown_surfaces(&site, site.settings.display_2d) {
        let color = match which {
            Surface::Existing => site.settings.existing_color.map_or(EXISTING_LINE, color32),
            Surface::Proposed => site.settings.proposed_color.map_or(PROPOSED_LINE, color32),
        };
        let Ok(list) = site.contours(which) else {
            continue;
        };
        for c in list {
            let width = if c.major {
                (base_width * 2.0).max(1.5)
            } else {
                base_width.min(1.0)
            };
            let points = c.points.iter().map(|p| view.to_screen(*p)).collect();
            painter.add(PathShape {
                points,
                closed: c.closed,
                fill: Color32::TRANSPARENT,
                stroke: PathStroke::new(width, color),
            });
            if site.settings.label_contours && c.major && !c.points.is_empty() {
                let mid = view.to_screen(c.points[c.points.len() / 2]);
                painter.text(
                    mid,
                    egui::Align2::CENTER_CENTER,
                    kadakas_site::draw::format_level(c.level),
                    egui::FontId::proportional(10.0),
                    color,
                );
            }
        }
    }
}

fn surface_combo(ui: &mut Ui, id: &str, value: &mut SurfaceDisplay) -> bool {
    let before = *value;
    egui::ComboBox::from_id_salt(id)
        .selected_text(surface_label(*value))
        .show_ui(ui, |ui| {
            for v in [
                SurfaceDisplay::Existing,
                SurfaceDisplay::Proposed,
                SurfaceDisplay::Both,
            ] {
                ui.selectable_value(value, v, surface_label(v));
            }
        });
    *value != before
}

fn surface_label(v: SurfaceDisplay) -> String {
    match v {
        SurfaceDisplay::Existing => tr!("site-ui-existing"),
        SurfaceDisplay::Proposed => tr!("site-ui-proposed"),
        SurfaceDisplay::Both => tr!("site-ui-both"),
    }
}

fn display3d_label(v: Display3d) -> String {
    match v {
        Display3d::Mesh => tr!("site-ui-mesh"),
        Display3d::Contours => tr!("site-ui-contours"),
        Display3d::Both => tr!("site-ui-mesh-and-contours"),
    }
}

fn length_row(
    ui: &mut Ui,
    label: &str,
    v: &mut f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
) -> bool {
    ui.label(label);
    let r = ui.add(DragValue::new(v).speed(speed).range(range).length());
    ui.end_row();
    r.drag_stopped() || r.lost_focus()
}

fn look_label(v: SurfaceLook) -> String {
    match v {
        SurfaceLook::Color => tr!("site-ui-look-color"),
        SurfaceLook::Texture => tr!("site-ui-look-texture"),
        SurfaceLook::Elevation => tr!("site-ui-look-elevation"),
    }
}

/// The rows of the 3D look of a site model: the surface color, a texture (a material with a
/// planar top projection) or colors by elevation. `materials` are the material resources of the
/// document.
fn look_fields(ui: &mut Ui, s: &mut SiteSettings, materials: &[(ResourceId, String)]) -> bool {
    let mut commit = false;
    ui.label(tr!("site-ui-3d-look"));
    let before = s.surface_look;
    egui::ComboBox::from_id_salt("site_3d_look")
        .selected_text(look_label(s.surface_look))
        .show_ui(ui, |ui| {
            for v in SurfaceLook::ALL {
                ui.selectable_value(&mut s.surface_look, v, look_label(v));
            }
        });
    commit |= s.surface_look != before;
    ui.end_row();
    match s.surface_look {
        SurfaceLook::Color => {}
        SurfaceLook::Texture => {
            ui.label(tr!("site-ui-texture-material"));
            let before = s.texture;
            let name = s
                .texture
                .and_then(|t| materials.iter().find(|(m, _)| *m == t))
                .map_or_else(|| tr!("site-ui-by-class"), |(_, n)| n.clone());
            egui::ComboBox::from_id_salt("site_texture_material")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.texture, None, tr!("site-ui-by-class"));
                    for (id, name) in materials {
                        ui.selectable_value(&mut s.texture, Some(*id), name);
                    }
                });
            commit |= s.texture != before;
            ui.end_row();
            commit |= length_row(
                ui,
                &tr!("site-ui-tile-size"),
                &mut s.texture_tile,
                10.0,
                kadakas_core::site::MIN_TEXTURE_TILE..=kadakas_core::site::MAX_TEXTURE_TILE,
            );
            let limit = kadakas_core::site::MAX_TEXTURE_OFFSET;
            commit |= length_row(
                ui,
                &tr!("site-ui-texture-offset-x"),
                &mut s.texture_offset[0],
                10.0,
                -limit..=limit,
            );
            commit |= length_row(
                ui,
                &tr!("site-ui-texture-offset-y"),
                &mut s.texture_offset[1],
                10.0,
                -limit..=limit,
            );
            ui.label(tr!("site-ui-texture-rotation"));
            let r = ui.add(
                DragValue::new(&mut s.texture_rotation)
                    .speed(1.0)
                    .direction(),
            );
            commit |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
        }
        SurfaceLook::Elevation => {
            ui.label(tr!("site-ui-elevation-bands"));
            let r = ui.add(DragValue::new(&mut s.elevation_bands).range(1..=MAX_ELEVATION_BANDS));
            commit |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
            ui.label(tr!("site-ui-fixed-range"));
            let mut fixed = s.elevation_range.is_some();
            if ui.checkbox(&mut fixed, "").changed() {
                s.elevation_range = fixed.then_some([0.0, 10_000.0]);
                commit = true;
            }
            ui.end_row();
            if let Some(range) = &mut s.elevation_range {
                let low_limit = range[0] + 1.0;
                commit |= length_row(
                    ui,
                    &tr!("site-ui-range-low"),
                    &mut range[0],
                    10.0,
                    -1.0e8..=1.0e8,
                );
                commit |= length_row(
                    ui,
                    &tr!("site-ui-range-high"),
                    &mut range[1],
                    10.0,
                    low_limit..=1.0e8,
                );
            }
            ui.label(tr!("site-ui-ramp"));
            ui.horizontal(|ui| {
                for c in &mut s.elevation_ramp {
                    let mut c32 = color32(*c);
                    if ui.color_edit_button_srgba(&mut c32).changed() {
                        *c = kadakas_core::model::Color(c32.to_srgba_unmultiplied());
                        commit = true;
                    }
                }
                if s.elevation_ramp.len() < MAX_RAMP_STOPS
                    && ui
                        .button("+")
                        .on_hover_text(tr!("site-ui-add-color"))
                        .clicked()
                {
                    let last = s
                        .elevation_ramp
                        .last()
                        .copied()
                        .unwrap_or(kadakas_core::model::Color([128, 128, 128, 255]));
                    s.elevation_ramp.push(last);
                    commit = true;
                }
                if s.elevation_ramp.len() > 1
                    && ui
                        .button("-")
                        .on_hover_text(tr!("site-ui-remove-color"))
                        .clicked()
                {
                    s.elevation_ramp.pop();
                    commit = true;
                }
            });
            ui.end_row();
        }
    }
    commit
}

/// The Object Info rows of a site model. Returns true when an edit is finished.
pub fn model_fields(
    ui: &mut Ui,
    points: usize,
    lines: usize,
    s: &mut SiteSettings,
    materials: &[(ResourceId, String)],
) -> bool {
    let mut commit = false;
    ui.label(tr!("site-ui-source-data"));
    ui.label(tr!(
        "site-ui-points-polygons",
        points = points,
        lines = lines
    ));
    ui.end_row();
    commit |= length_row(
        ui,
        &tr!("site-ui-minor-interval"),
        &mut s.minor_interval,
        10.0,
        1.0..=1.0e6,
    );
    commit |= length_row(
        ui,
        &tr!("site-ui-major-interval"),
        &mut s.major_interval,
        10.0,
        s.minor_interval..=1.0e7,
    );
    commit |= length_row(
        ui,
        &tr!("site-ui-datum"),
        &mut s.datum,
        10.0,
        -1.0e8..=1.0e8,
    );
    ui.label(tr!("site-ui-update"));
    commit |= ui
        .checkbox(&mut s.auto_update, tr!("site-ui-automatic"))
        .changed();
    ui.end_row();
    ui.label(tr!("site-ui-crop-boundary"));
    if s.crop.is_empty() {
        ui.label(tr!("site-ui-none-set-it-with-site-update"));
    } else {
        ui.label(tr!("site-ui-points", count = s.crop.len()));
        if ui.button(tr!("site-ui-clear")).clicked() {
            s.crop.clear();
            commit = true;
        }
    }
    ui.end_row();
    ui.label(tr!("site-ui-smooth-contours"));
    commit |= ui.checkbox(&mut s.smooth_contours, "").changed();
    ui.end_row();
    ui.label(tr!("site-ui-contour-labels"));
    commit |= ui.checkbox(&mut s.label_contours, "").changed();
    ui.end_row();
    ui.label(tr!("site-ui-2d-surface"));
    commit |= surface_combo(ui, "site_2d", &mut s.display_2d);
    ui.end_row();
    ui.label(tr!("site-ui-3d-surface"));
    commit |= surface_combo(ui, "site_3d_surface", &mut s.surface_3d);
    ui.end_row();
    ui.label(tr!("site-ui-3d-display"));
    let before = s.display_3d;
    egui::ComboBox::from_id_salt("site_3d_display")
        .selected_text(display3d_label(s.display_3d))
        .show_ui(ui, |ui| {
            for v in [Display3d::Mesh, Display3d::Contours, Display3d::Both] {
                ui.selectable_value(&mut s.display_3d, v, display3d_label(v));
            }
        });
    commit |= s.display_3d != before;
    ui.end_row();
    commit |= look_fields(ui, s, materials);
    commit
}

/// The Object Info rows of a site modifier. Returns true when an edit is finished.
pub fn modifier_fields(ui: &mut Ui, modifier: &mut ModifierKind, boundary: &[Vertex]) -> bool {
    let vertices = boundary.len();
    let mut commit = false;
    match modifier {
        ModifierKind::Pad { elevation, slope } => {
            ui.label(tr!("site-ui-type"));
            ui.label(tr!("site-ui-pad"));
            ui.end_row();
            commit |= length_row(
                ui,
                &tr!("site-ui-elevation"),
                elevation,
                10.0,
                -1.0e8..=1.0e8,
            );
            ui.label(tr!("site-ui-slope"));
            let mut sloped = slope.is_some();
            if ui.checkbox(&mut sloped, "").changed() {
                *slope = sloped.then_some(PadSlope {
                    direction: 0.0,
                    percent: 2.0,
                });
                commit = true;
            }
            ui.end_row();
            if let Some(s) = slope {
                // The slope is given as the elevation at the end of the pad, in the falling direction.
                let flat = boundary_points(boundary);
                let mut end = end_elevation(&flat, *elevation, *s);
                ui.label(tr!("site-ui-elevation-at-end"));
                let r = ui.add(
                    DragValue::new(&mut end)
                        .speed(10.0)
                        .range(-1.0e8..=1.0e8)
                        .length(),
                );
                if r.changed()
                    && let Some(n) = slope_from_end_elevation(&flat, *elevation, s.direction, end)
                {
                    *s = n;
                }
                commit |= r.drag_stopped() || r.lost_focus();
                ui.end_row();
                ui.label(tr!("site-ui-slope"));
                ui.label(format!(
                    "{} %",
                    kadakas_units::current().number_text(s.percent, 2)
                ));
                ui.end_row();
                let mut deg = s.direction.to_degrees();
                ui.label(tr!("site-ui-falls-toward"));
                let r = ui.add(DragValue::new(&mut deg).speed(1.0).direction());
                s.direction = deg.to_radians();
                commit |= r.drag_stopped() || r.lost_focus();
                ui.end_row();
            }
        }
        ModifierKind::GradeLimit => {
            ui.label(tr!("site-ui-type"));
            ui.label(tr!("site-ui-grade-limits"));
            ui.end_row();
        }
    }
    ui.label(tr!("site-ui-vertices"));
    ui.label(format!("{vertices}"));
    ui.end_row();
    commit
}

/// Cut and fill of a selected site model, in cubic meters.
pub fn cut_fill_readout(ui: &mut Ui, session: &Session, id: ObjectId) {
    let doc = session.editor.doc();
    if !matches!(
        doc.object(id).map(|o| &o.shape),
        Some(Shape::SiteModel { .. })
    ) {
        return;
    }
    match session.site_cache().get(session.editor.revision(), doc, id) {
        Ok(site) => {
            let c = site.cut_fill();
            let u = kadakas_units::current();
            ui.label(tr!(
                "site-ui-cut-fill-net",
                cut = u.volume_text(c.cut),
                fill = u.volume_text(c.fill),
                net = u.volume_text(c.net())
            ));
            ui.label(tr!(
                "site-ui-surface-area",
                area = u.area_text_d(site.existing.area_xy(), 1)
            ));
            if session.site_cache().is_stale(doc, id) {
                ui.colored_label(
                    Color32::from_rgb(200, 120, 20),
                    tr!("site-ui-proposed-surface-is-out-of-date-site"),
                );
            }
        }
        Err(e) => {
            ui.colored_label(Color32::from_rgb(200, 80, 40), e.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DVec2, DVec3};
    use kadakas_core::command::Command;
    use kadakas_core::model::Attributes;

    fn add(s: &mut Session, shape: Shape) -> ObjectId {
        let out = s
            .editor
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        ObjectId(out.created[0])
    }

    fn locus(s: &mut Session, x: f64, y: f64, z: f64) -> ObjectId {
        add(
            s,
            Shape::Locus3d {
                p: DVec3::new(x, y, z),
            },
        )
    }

    #[test]
    fn create_uses_only_the_selected_sources() {
        let mut s = Session::default();
        let circle = add(
            &mut s,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
        );
        assert_eq!(
            create_site_model(&mut s).unwrap_err(),
            "Select 3D loci or 3D polylines first"
        );
        let pts = [
            locus(&mut s, 0.0, 0.0, 0.0),
            locus(&mut s, 1000.0, 0.0, 100.0),
            locus(&mut s, 0.0, 1000.0, 200.0),
        ];
        s.selection = vec![circle, pts[0], pts[1], pts[2]];
        let id = create_site_model(&mut s).unwrap();
        assert_eq!(s.selection, vec![id]);
        // The circle stays, the loci moved into the site model.
        assert_eq!(s.editor.doc().objects.len(), 2);
        assert!(
            matches!(&s.editor.doc().object(id).unwrap().shape, Shape::SiteModel { points, .. } if points.len() == 3)
        );
        // One locus alone is accepted. The missing surface is reported when the model is used.
        let a = locus(&mut s, 5.0, 5.0, 0.0);
        s.selection = vec![a, ObjectId::from_num(9999)];
        assert!(create_site_model(&mut s).is_ok());
    }

    #[test]
    fn source_ids_keep_loci_and_3d_polylines() {
        let mut s = Session::default();
        let l = locus(&mut s, 0.0, 0.0, 0.0);
        let p = add(
            &mut s,
            Shape::Polyline3d {
                points: vec![DVec3::ZERO, DVec3::X],
                closed: false,
            },
        );
        let t = add(
            &mut s,
            Shape::Text {
                at: DVec2::ZERO,
                text: "a".into(),
                height: 5.0,
            },
        );
        assert_eq!(
            source_ids(s.editor.doc(), &[t, l, p, ObjectId::from_num(77)]),
            vec![l, p]
        );
    }

    #[test]
    fn labels_are_plain_words() {
        assert_eq!(surface_label(SurfaceDisplay::Both), "Both");
        assert_eq!(display3d_label(Display3d::Both), "Mesh and contours");
    }

    #[test]
    fn panels_run_without_a_window() {
        let ctx = egui::Context::default();
        let mut s = Session::default();
        let pts = [
            locus(&mut s, 0.0, 0.0, 0.0),
            locus(&mut s, 5000.0, 0.0, 100.0),
            locus(&mut s, 0.0, 5000.0, 200.0),
        ];
        s.selection = pts.to_vec();
        let id = create_site_model(&mut s).unwrap();
        let mut settings = SiteSettings::default();
        let mut kind = ModifierKind::Pad {
            elevation: 10.0,
            slope: Some(PadSlope {
                direction: 0.0,
                percent: 2.0,
            }),
        };
        let mut grade = ModifierKind::GradeLimit;
        let _ = ctx.run_ui(Default::default(), |ui| {
            egui::Grid::new("g").show(ui, |ui| {
                assert!(!model_fields(ui, 3, 0, &mut settings, &[]));
                // Every look draws its rows without an edit.
                let mats = [(ResourceId::from_num(3), "Grass".to_string())];
                for look in SurfaceLook::ALL {
                    settings.surface_look = look;
                    settings.texture = Some(ResourceId::from_num(3));
                    settings.elevation_range = Some([0.0, 100.0]);
                    assert!(!model_fields(ui, 3, 0, &mut settings, &mats));
                }
                settings = SiteSettings::default();
                let b = kadakas_core::curve::corners(&[
                    glam::DVec2::ZERO,
                    glam::DVec2::X * 1000.0,
                    glam::DVec2::ONE * 1000.0,
                ]);
                assert!(!modifier_fields(ui, &mut kind, &b));
                assert!(!modifier_fields(ui, &mut grade, &b));
            });
            cut_fill_readout(ui, &s, id);
            cut_fill_readout(ui, &s, ObjectId::from_num(4242));
        });
        // A site model without a surface shows its error.
        let mut bad = Session::default();
        let lone = locus(&mut bad, 0.0, 0.0, 0.0);
        bad.selection = vec![lone];
        let site = create_site_model(&mut bad).unwrap();
        let _ = ctx.run_ui(Default::default(), |ui| cut_fill_readout(ui, &bad, site));
    }
}
