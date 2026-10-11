//! The Resource Manager for all resource types: thumbnails, search, filter, duplicate, rename, delete,
//! delete unused, and import from another file. It calls the same API methods as agents do.

use kadakas_l10n::tr;
use std::collections::HashMap;

use egui::epaint::PathStroke;
use egui::{Color32, Pos2, Sense, Ui, Vec2};
use kadakas_api::Session;
use kadakas_core::resource::ResourceId;
use kadakas_render::thumbnail::{SIZE, Thumbnail, thumbnail};
use serde_json::{Value, json};

const TYPES: [&str; 11] = [
    "",
    "plant_style",
    "hardscape_style",
    "landscape_area_style",
    "record_format",
    "data_map",
    "worksheet",
    "symbol",
    "hatch",
    "tile",
    "line_type",
];

#[derive(Default)]
pub struct ResourceUi {
    search: String,
    ty: usize,
    rename: Option<(ResourceId, String)>,
    import_path: String,
    /// Thumbnails by resource, for one revision of the document.
    thumbs: HashMap<ResourceId, Thumbnail>,
    revision: u64,
}

/// Paints a thumbnail in a square of `side` points.
pub fn paint_thumbnail(ui: &mut Ui, t: &Thumbnail, side: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, Color32::WHITE);
    p.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, Color32::from_gray(200)),
        egui::StrokeKind::Inside,
    );
    let k = side / SIZE as f32;
    let map = |q: &[f64; 2]| Pos2::new(rect.min.x + q[0] as f32 * k, rect.min.y + q[1] as f32 * k);
    let col = |c: [u8; 4]| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    for path in &t.paths {
        let pts: Vec<Pos2> = path.points.iter().map(map).collect();
        if pts.len() < 2 {
            continue;
        }
        let stroke = path
            .stroke
            .map_or(PathStroke::NONE, |c| PathStroke::new(1.0, col(c)));
        p.add(egui::epaint::PathShape {
            points: pts,
            closed: path.closed,
            fill: path.fill.map_or(Color32::TRANSPARENT, col),
            stroke,
        });
    }
}

fn call(s: &mut Session, method: &str, params: Value, status: &mut String) -> Option<Value> {
    match s.handle(method, params) {
        Ok(v) => Some(v),
        Err(e) => {
            *status = tr!("resource-ui-error", message = e.message);
            None
        }
    }
}

/// The section of the palette. It lists every resource of the document.
pub fn section(ui: &mut Ui, session: &mut Session, st: &mut ResourceUi, status: &mut String) {
    egui::CollapsingHeader::new(tr!("resource-ui-resources-all-types"))
        .id_salt("resource-ui-resources-all-types")
        .default_open(false)
        .show(ui, |ui| {
            let revision = session.editor.revision();
            if st.revision != revision {
                st.thumbs.clear();
                st.revision = revision;
            }
            ui.horizontal(|ui| {
                ui.label(tr!("resource-ui-search"));
                ui.text_edit_singleline(&mut st.search);
            });
            egui::ComboBox::from_label(tr!("resource-ui-type"))
                .selected_text(if st.ty == 0 {
                    tr!("resource-ui-all-types")
                } else {
                    TYPES[st.ty].to_string()
                })
                .show_ui(ui, |ui| {
                    for (i, t) in TYPES.iter().enumerate() {
                        let label = if i == 0 {
                            tr!("resource-ui-all-types")
                        } else {
                            t.to_string()
                        };
                        let icon = crate::icons::resource_icon(t);
                        if crate::icons::select_button(ui, icon, &label, st.ty == i).clicked() {
                            st.ty = i;
                        }
                    }
                });
            let ty = (st.ty > 0).then(|| TYPES[st.ty]);
            let rows = kadakas_core::library::list(session.editor.doc(), ty, Some(&st.search));
            let mut action: Option<(ResourceId, &str)> = None;
            for r in rows.iter().take(200) {
                ui.horizontal(|ui| {
                    let doc = session.editor.doc();
                    let t = st
                        .thumbs
                        .entry(r.id)
                        .or_insert_with(|| thumbnail(doc, r.id));
                    paint_thumbnail(ui, t, 36.0);
                    ui.vertical(|ui| {
                        match &mut st.rename {
                            Some((id, name)) if *id == r.id => {
                                let resp = ui.text_edit_singleline(name);
                                if resp.lost_focus() {
                                    action = Some((r.id, "rename"));
                                }
                            }
                            _ => {
                                ui.label(&r.name);
                            }
                        }
                        ui.horizontal(|ui| {
                            if let Some(img) = crate::icons::image(
                                ui,
                                crate::icons::resource_icon(r.type_name),
                                12.0,
                            ) {
                                ui.add(img);
                            }
                            ui.small(tr!(
                                "resource-ui-used-by",
                                type_name = r.type_name,
                                used_by = r.used_by
                            ));
                        });
                    });
                    ui.menu_button("...", |ui| {
                        if ui.button(tr!("resource-ui-duplicate")).clicked() {
                            action = Some((r.id, "duplicate"));
                            ui.close();
                        }
                        if ui.button(tr!("resource-ui-rename")).clicked() {
                            st.rename = Some((r.id, r.name.clone()));
                            ui.close();
                        }
                        if ui.button(tr!("resource-ui-delete")).clicked() {
                            action = Some((r.id, "delete"));
                            ui.close();
                        }
                    });
                });
            }
            if rows.len() > 200 {
                ui.small(tr!(
                    "resource-ui-more-search-to-narrow-the-list",
                    count = rows.len() - 200
                ));
            }
            match action {
                Some((id, "duplicate")) => {
                    call(session, "resource.duplicate", json!({"id": id}), status);
                }
                Some((id, "delete")) => {
                    if call(session, "resource.delete", json!({"id": id}), status).is_some() {
                        *status = tr!("resource-ui-deleted");
                    }
                }
                Some((id, "rename")) => {
                    if let Some((_, name)) = st.rename.take() {
                        call(
                            session,
                            "resource.rename",
                            json!({"id": id, "name": name}),
                            status,
                        );
                    }
                }
                _ => {}
            }
            if ui.button(tr!("resource-ui-delete-unused")).clicked()
                && let Some(v) = call(session, "resource.purge_unused", json!({}), status)
            {
                *status = tr!(
                    "resource-ui-deleted-unused-resources",
                    count = v["removed"].as_array().map_or(0, Vec::len)
                );
            }
            ui.separator();
            ui.label(tr!("resource-ui-import-from-a-file"));
            ui.text_edit_singleline(&mut st.import_path);
            if ui.button(tr!("resource-ui-import-all-resources")).clicked()
                && !st.import_path.trim().is_empty()
            {
                let params =
                    json!({"path": st.import_path.trim(), "all": true, "on_conflict": "rename"});
                if let Some(v) = call(session, "resource.import_from_file", params, status) {
                    *status = tr!(
                        "resource-ui-imported-resources",
                        count = v["imported"].as_array().map_or(0, Vec::len)
                    );
                }
            }
        });
}
