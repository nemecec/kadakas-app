//! The Resource Manager for all resource types: thumbnails, search, filter, duplicate, rename, delete,
//! delete unused, and import from another file. It calls the same API methods as agents do.

use kadakas_l10n::tr;
use std::collections::HashMap;

use egui::epaint::PathStroke;
use egui::{Color32, Pos2, Sense, Ui, Vec2};
use kadakas_api::Session;
use kadakas_core::resource::ResourceId;
use kadakas_render::thumbnail::{SIZE, Thumbnail, thumbnail_with};
use serde_json::{Value, json};

const TYPES: [&str; 15] = [
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
    "dimension_style",
    "text_style",
    "tag_style",
    "notes_database",
];

#[derive(Default)]
pub struct ResourceUi {
    search: String,
    ty: usize,
    rename: Option<(ResourceId, String)>,
    /// The style editor windows (ANN-12).
    pub styles: crate::style_ui::StyleWindows,
    import_path: String,
    /// The question before a big library file is read (DOC-19). It holds the parameters of the call.
    #[cfg(not(target_arch = "wasm32"))]
    import_gate: crate::open_gate::OpenGate<Value>,
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
    if let Some(pic) = &t.picture
        && let Some((tex, _)) = crate::picture_ui::cached(ui.ctx(), &pic.sha256)
    {
        let ([x, y], [w, h]) = pic.placement();
        let r = egui::Rect::from_min_size(
            Pos2::new(rect.min.x + x as f32 * k, rect.min.y + y as f32 * k),
            Vec2::new(w as f32 * k, h as f32 * k),
        );
        p.image(
            tex.id(),
            r,
            egui::Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
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

/// The name of a resource type for the list: the annotation styles have readable names, the other
/// types show their API name.
fn type_label(ty: &str) -> String {
    kadakas_ui_core::style_edit::StyleKind::from_type_name(ty)
        .map_or_else(|| ty.to_string(), |k| k.label())
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

/// `resource.import_from_file`. With `agreed` the person said yes to a big file, so the size limit
/// of the session (a limit for agents) does not apply to this one call.
fn import_from_file(session: &mut Session, params: Value, status: &mut String, agreed: bool) {
    let limits = session.limits().clone();
    if agreed {
        let mut open = limits.clone();
        open.document_file_bytes = u64::MAX;
        session.set_limits(open);
    }
    let reply = call(session, "resource.import_from_file", params, status);
    if agreed {
        session.set_limits(limits);
    }
    if let Some(v) = reply {
        *status = tr!(
            "resource-ui-imported-resources",
            count = v["imported"].as_array().map_or(0, Vec::len)
        );
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
                    type_label(TYPES[st.ty])
                })
                .show_ui(ui, |ui| {
                    for (i, t) in TYPES.iter().enumerate() {
                        let label = if i == 0 {
                            tr!("resource-ui-all-types")
                        } else {
                            type_label(t)
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
                    let ctx = ui.ctx().clone();
                    let pictured = |doc: &kadakas_core::model::Document| match doc
                        .resources
                        .get(r.id)
                        .map(|x| &x.body)
                    {
                        Some(kadakas_core::resource::ResourceBody::Image(_)) => true,
                        Some(kadakas_core::resource::ResourceBody::PlantStyle(s)) => {
                            s.image.is_some()
                        }
                        _ => false,
                    };
                    let make = || {
                        thumbnail_with(doc, r.id, &|res| crate::picture_ui::thumb_for(&ctx, res))
                    };
                    // An image appears when its thumbnail is derived, so it is not kept for the
                    // revision until it is ready.
                    if pictured(doc) {
                        let t = make();
                        paint_thumbnail(ui, &t, 36.0);
                    } else {
                        let t = st.thumbs.entry(r.id).or_insert_with(make);
                        paint_thumbnail(ui, t, 36.0);
                    }
                    ui.vertical(|ui| {
                        match &mut st.rename {
                            Some((id, name)) if *id == r.id => {
                                let resp = ui.text_edit_singleline(name);
                                if resp.lost_focus() {
                                    action = Some((r.id, "rename"));
                                }
                            }
                            _ => {
                                // A double-click on the name opens the style editor.
                                let name = ui.add(egui::Label::new(&r.name).sense(Sense::click()));
                                if name.double_clicked() && crate::style_ui::has_editor(r.type_name)
                                {
                                    action = Some((r.id, "edit"));
                                }
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
                                type_name = type_label(r.type_name),
                                used_by = r.used_by
                            ));
                        });
                    });
                    ui.menu_button("...", |ui| {
                        if crate::style_ui::has_editor(r.type_name)
                            && ui.button(tr!("style-ui-edit")).clicked()
                        {
                            action = Some((r.id, "edit"));
                            ui.close();
                        }
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
                Some((id, "edit")) => {
                    st.styles.open(session, id);
                }
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
            ui.menu_button(tr!("style-ui-new-style"), |ui| {
                for kind in kadakas_ui_core::style_edit::StyleKind::ALL {
                    if ui.button(kind.label()).clicked() {
                        st.styles.create(session, kind, status);
                        ui.close();
                    }
                }
            });
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
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // The path goes through the path policy of the session before it is measured.
                    // A big file of the person is asked about first; the session limit is for
                    // agents.
                    let given = std::path::Path::new(st.import_path.trim());
                    match session.check_path(given, false, &["kad"]) {
                        Ok(path) => {
                            let params =
                                json!({"path": path, "all": true, "on_conflict": "rename"});
                            let decision = crate::file_io::open_decision(&path);
                            if let Some(params) = st.import_gate.check(params, decision) {
                                import_from_file(session, params, status, false);
                            }
                        }
                        Err(e) => *status = tr!("resource-ui-error", message = e),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                import_from_file(
                    session,
                    json!({"path": st.import_path.trim(), "all": true, "on_conflict": "rename"}),
                    status,
                    false,
                );
            }
        });
}

/// The question before a big library file is read. It is a modal window of the app, so the app
/// calls it every frame, outside the (maybe closed) section of the palette.
#[cfg(not(target_arch = "wasm32"))]
pub fn question(
    ctx: &egui::Context,
    session: &mut Session,
    st: &mut ResourceUi,
    status: &mut String,
) {
    if let Some((params, _)) = st.import_gate.show(ctx) {
        import_from_file(session, params, status, true);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn a_big_library_file_that_the_person_agreed_to_is_read_over_the_session_limit() {
        let dir = std::env::temp_dir().join(format!("kadakas-resui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("lib.kad");
        let mut source = Session::with_sequential_ids();
        source
            .handle(
                "resource.create",
                json!({"type": "texture", "name": "Bark", "color": [1, 2, 3, 255]}),
            )
            .unwrap();
        crate::file_io::write_document_raw(&file, source.editor.doc()).unwrap();
        let len = std::fs::metadata(&file).unwrap().len();

        let mut s = Session::with_sequential_ids();
        let limits = kadakas_api::SessionLimits {
            document_file_bytes: len - 1,
            ..Default::default()
        };
        s.set_limits(limits.clone());
        let params = json!({"path": file, "all": true, "on_conflict": "rename"});
        // Not agreed: the limit of the session (a limit for agents) holds.
        let mut status = String::new();
        import_from_file(&mut s, params.clone(), &mut status, false);
        assert!(status.contains("limit"), "{status}");
        // Agreed: the file is read, and the limit of the session is back afterwards.
        import_from_file(&mut s, params, &mut status, true);
        assert!(!status.contains("limit"), "{status}");
        assert_eq!(s.limits(), &limits);
        assert_eq!(s.editor.doc().resources.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
