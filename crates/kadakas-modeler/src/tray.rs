//! The tray on the right: Entity Info, Materials, Soften Edges and Scenes, one under the other, each with a
//! title bar that collapses it. Entity Info shows the data of the selection, the materials of a
//! face and a summary of the plants, hardscapes and site models (MOD-05). Materials is the list
//! of the materials of the model with thumbnails and a small editor. Scenes (saved views,
//! MOD-10) and Styles are drawn by `scenes_panel`; the tray only places them.
//!
//! The panels draw and return [`TrayAction`]s; the program runs them after the frame. The
//! logic (what a request does, the questions) is in `kadakas_ui_core::materials`.

use std::collections::HashMap;

use kadakas_core::model::Document;
use kadakas_core::present::{self, MappingKind, Material, Procedural, Texture};
use kadakas_core::resource::ResourceId;
use kadakas_core::surface::{FaceId, PaintSide};
use kadakas_l10n::tr;
use kadakas_ui_core::materials::{MaterialEdit, MaterialRow, MaterialsPanel, Request};
use kadakas_ui_core::modeler::{EntityInfo, Summary};
use kadakas_ui_number::UiNumber;

/// Which panels of the tray are open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayState {
    pub entity_info: bool,
    pub materials: bool,
    /// Soften Edges (MOD-08).
    pub soften: bool,
    pub scenes: bool,
    pub tags: bool,
    /// The style picker (MOD-10).
    pub styles: bool,
}

impl Default for TrayState {
    fn default() -> Self {
        TrayState {
            entity_info: true,
            materials: true,
            soften: true,
            scenes: true,
            tags: true,
            styles: true,
        }
    }
}

/// What a tray panel asks the program to do.
#[derive(Clone, Debug, PartialEq)]
pub enum TrayAction {
    Material(Request),
    /// Open the model in the Kadakas app (File > Open in the Kadakas App).
    OpenInApp,
    /// A value of the Soften Edges panel changed: apply it to the selection (MOD-08).
    Soften,
    /// The slider of the Soften Edges panel is being dragged: show what it would soften.
    SoftenPreview,
    /// Entity Info set the number of segments of a circle or an arc (MOD-15).
    CurveSegments {
        model: kadakas_core::model::ObjectId,
        curve: u32,
        segments: u32,
    },
}

/// What the tray draws from: the data of the program for one frame.
pub struct TrayData<'a> {
    pub info: &'a EntityInfo,
    pub doc: &'a Document,
    pub rows: &'a [MaterialRow],
    /// Built-in materials the model does not have.
    pub missing: usize,
    pub panel: &'a MaterialsPanel,
    /// The material of the Paint Bucket.
    pub bucket: Option<ResourceId>,
    /// Edges or faces of surface models are selected (the Soften Edges panel works on them).
    pub edges_selected: bool,
}

/// The fields of the small editor while the person works on them. A field is applied (one undo
/// step) when the person lets go of the slider or leaves the box.
#[derive(Clone, Debug)]
struct EditBuffer {
    id: ResourceId,
    name: String,
    /// The material and the name as the document had them when the buffer was loaded.
    seen: Material,
    seen_name: String,
    value: Material,
}

/// The state of the tray that is not a flag: the thumbnails and the editor.
#[derive(Default)]
pub struct TrayUi {
    thumbs: HashMap<ResourceId, (u64, egui::TextureHandle)>,
    buffer: Option<EditBuffer>,
    /// The values of the Soften Edges panel.
    pub soften: kadakas_ui_core::modeling::SoftenPanel,
}

impl TrayUi {
    /// The thumbnail of a material, made again when its look changed.
    fn thumb(&mut self, ctx: &egui::Context, row: &MaterialRow, m: &Material) -> egui::TextureId {
        if let Some((k, h)) = self.thumbs.get(&row.id)
            && *k == row.thumb_key
        {
            return h.id();
        }
        const SIDE: usize = 32;
        let pixels: Vec<egui::Color32> = kadakas_ui_core::materials::swatch(m, SIDE as u32)
            .into_iter()
            .map(|p| egui::Color32::from_rgb(p[0], p[1], p[2]))
            .collect();
        let image = egui::ColorImage::new([SIDE, SIDE], pixels);
        let handle = ctx.load_texture(
            format!("material-{}", row.id),
            image,
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.thumbs.insert(row.id, (row.thumb_key, handle));
        id
    }

    /// Drops what belongs to materials that are gone.
    fn retain(&mut self, rows: &[MaterialRow]) {
        self.thumbs.retain(|id, _| rows.iter().any(|r| r.id == *id));
        if self
            .buffer
            .as_ref()
            .is_some_and(|b| !rows.iter().any(|r| r.id == b.id))
        {
            self.buffer = None;
        }
    }
}

/// The width of the tray in points.
pub const WIDTH: f32 = 240.0;

/// The name of a kind of object for the panel: the API id with spaces. (The ids stay English in
/// the files and the API; a newer program may know more kinds than this one has names for.)
pub fn kind_text(kind: &str) -> String {
    let s = kind.replace('_', " ");
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => s,
    }
}

/// Three angles in degrees.
fn degrees(a: [f64; 3]) -> String {
    let n = |v: f64| kadakas_units::num(v, 1);
    format!("{}°, {}°, {}°", n(a[0]), n(a[1]), n(a[2]))
}

/// The normal of a face: three numbers in the number format of the person. They are separated by
/// a semicolon where the decimal symbol is a comma.
fn normal_text(n: [f64; 3]) -> String {
    let sep = if kadakas_units::current().number.decimal == kadakas_units::Decimal::Comma {
        "; "
    } else {
        ", "
    };
    let f = |v: f64| kadakas_units::num(v, 2);
    [f(n[0]), f(n[1]), f(n[2])].join(sep)
}

/// Three scale factors.
fn factors(a: [f64; 3]) -> String {
    let n = |v: f64| kadakas_units::num(v, 3);
    format!("{} × {} × {}", n(a[0]), n(a[1]), n(a[2]))
}

fn lengths(size: [f64; 3]) -> String {
    let u = kadakas_units::current();
    tr!(
        "uic-modeler-dimensions-value",
        x = u.length_text(size[0]),
        y = u.length_text(size[1]),
        z = u.length_text(size[2])
    )
}

fn row(ui: &mut egui::Ui, label: String, value: String) {
    ui.label(egui::RichText::new(label).weak());
    ui.label(value);
    ui.end_row();
}

/// The name of the pattern of a generated texture.
pub fn pattern_name(p: Procedural) -> String {
    match p {
        Procedural::Grass => tr!("modeler-materials-pattern-grass"),
        Procedural::Gravel => tr!("modeler-materials-pattern-gravel"),
        Procedural::Pavers => tr!("modeler-materials-pattern-pavers"),
        Procedural::Mulch => tr!("modeler-materials-pattern-mulch"),
        Procedural::Water => tr!("modeler-materials-pattern-water"),
        Procedural::Brick => tr!("modeler-materials-pattern-brick"),
        Procedural::Timber => tr!("modeler-materials-pattern-timber"),
        Procedural::Noise => tr!("modeler-materials-pattern-noise"),
    }
}

fn mapping_name(k: MappingKind) -> String {
    match k {
        MappingKind::Planar => tr!("modeler-materials-mapping-planar"),
        MappingKind::Box => tr!("modeler-materials-mapping-box"),
        MappingKind::Cylindrical => tr!("modeler-materials-mapping-cylindrical"),
    }
}

/// The name of the material on one side, or "Default".
fn material_name(rows: &[MaterialRow], id: Option<ResourceId>) -> String {
    id.and_then(|i| rows.iter().find(|r| r.id == i))
        .map_or_else(|| tr!("modeler-entity-no-material"), |r| r.name.clone())
}

/// A box to choose a material or none. Returns the new choice when the person made one.
fn material_combo(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    current: Option<ResourceId>,
    rows: &[MaterialRow],
) -> Option<Option<ResourceId>> {
    let mut chosen = None;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(material_name(rows, current))
        .width(120.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(current.is_none(), tr!("modeler-entity-no-material"))
                .clicked()
            {
                chosen = Some(None);
            }
            for r in rows {
                if ui
                    .selectable_label(current == Some(r.id), r.name.clone())
                    .clicked()
                {
                    chosen = Some(Some(r.id));
                }
            }
        });
    chosen.filter(|c| *c != current)
}

/// The rows of the summary of a plant, a hardscape, a site model and the like.
fn summary_rows(ui: &mut egui::Ui, s: &Summary) {
    let u = kadakas_units::current();
    match s {
        Summary::Plant {
            species,
            style,
            height,
            spread,
        } => {
            row(ui, tr!("modeler-entity-species"), species.clone());
            if let Some(st) = style {
                row(ui, tr!("modeler-entity-style"), st.clone());
            }
            row(ui, tr!("modeler-entity-height"), u.length_text(*height));
            row(ui, tr!("modeler-entity-spread"), u.length_text(*spread));
        }
        Summary::ExistingTree {
            species,
            height,
            spread,
            dbh,
        } => {
            row(ui, tr!("modeler-entity-species"), species.clone());
            row(ui, tr!("modeler-entity-height"), u.length_text(*height));
            row(ui, tr!("modeler-entity-spread"), u.length_text(*spread));
            row(ui, tr!("modeler-entity-trunk"), u.length_text(*dbh));
        }
        Summary::Hardscape {
            area,
            style,
            material,
        } => {
            row(ui, tr!("modeler-entity-area-total"), u.area_text(*area));
            if let Some(st) = style {
                row(ui, tr!("modeler-entity-style"), st.clone());
            }
            row(ui, tr!("modeler-entity-surface-material"), material.clone());
        }
        Summary::LandscapeArea { name, area, style } => {
            row(ui, tr!("modeler-entity-name"), name.clone());
            row(ui, tr!("modeler-entity-area-total"), u.area_text(*area));
            if let Some(st) = style {
                row(ui, tr!("modeler-entity-style"), st.clone());
            }
        }
        Summary::SiteModel {
            points,
            lines,
            extent,
            tile,
        } => {
            row(ui, tr!("modeler-entity-survey-points"), points.to_string());
            row(ui, tr!("modeler-entity-breaklines"), lines.to_string());
            if let Some(e) = extent {
                row(ui, tr!("modeler-entity-extent"), lengths(*e));
            }
            if let Some(t) = tile {
                row(ui, tr!("modeler-entity-tile"), u.length_text(*t));
            }
        }
        Summary::Symbol { name } | Summary::Component { name } => {
            row(ui, tr!("modeler-entity-definition"), name.clone());
        }
        Summary::Group { members } => {
            row(ui, tr!("modeler-entity-members"), members.to_string());
        }
    }
}

/// The Entity Info panel: what the selection is.
pub fn entity_info(ui: &mut egui::Ui, data: &TrayData, out: &mut Vec<TrayAction>) {
    match data.info {
        EntityInfo::Nothing { layer, class } => {
            ui.label(egui::RichText::new(tr!("modeler-entity-nothing")).weak());
            egui::Grid::new("entity-info-nothing")
                .num_columns(2)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-active-layer"), layer.clone());
                    row(ui, tr!("modeler-entity-active-class"), class.clone());
                });
        }
        EntityInfo::One {
            id,
            kind,
            layer,
            class,
            size,
            min,
            surface,
            placement,
            summary,
            material,
        } => {
            egui::Grid::new("entity-info-one")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-type"), kind_text(kind));
                    row(ui, tr!("modeler-entity-id"), id.clone());
                    row(ui, tr!("modeler-entity-layer"), layer.clone());
                    row(ui, tr!("modeler-entity-class"), class.clone());
                    if let Some(s) = summary {
                        summary_rows(ui, s);
                    }
                    if let Some(m) = material {
                        row(ui, tr!("modeler-entity-material"), m.clone());
                    }
                    if let Some(s) = size {
                        row(ui, tr!("modeler-entity-size"), lengths(*s));
                    }
                    if let Some(m) = min {
                        row(ui, tr!("modeler-entity-corner"), lengths(*m));
                    }
                    if let Some(p) = placement {
                        row(ui, tr!("modeler-entity-position"), lengths(p.position));
                        let rotation = if p.skewed {
                            tr!("modeler-entity-skewed")
                        } else {
                            degrees(p.rotation_deg)
                        };
                        row(ui, tr!("modeler-entity-rotation"), rotation);
                        row(ui, tr!("modeler-entity-scale"), factors(p.scale));
                    }
                    if let Some(s) = surface {
                        row(ui, tr!("modeler-entity-vertices"), s.vertices.to_string());
                        row(ui, tr!("modeler-entity-edges"), s.edges.to_string());
                        row(ui, tr!("modeler-entity-faces"), s.faces.to_string());
                        row(
                            ui,
                            tr!("modeler-entity-solid"),
                            if s.closed {
                                tr!("modeler-entity-solid-closed")
                            } else {
                                tr!("modeler-entity-solid-open")
                            },
                        );
                        if s.open_edges > 0 {
                            row(
                                ui,
                                tr!("modeler-entity-open-edges"),
                                tr!("modeler-entity-open-edges-value", count = s.open_edges),
                            );
                        }
                        if s.not_intersected > 0 {
                            row(
                                ui,
                                tr!("modeler-entity-not-intersected"),
                                tr!(
                                    "modeler-entity-not-intersected-value",
                                    count = s.not_intersected
                                ),
                            );
                        }
                    }
                });
            if summary.as_ref().is_some_and(|s| s.needs_app()) {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tr!("modeler-entity-needs-app")).weak());
                if ui.button(tr!("modeler-entity-open-in-app")).clicked() {
                    out.push(TrayAction::OpenInApp);
                }
            }
        }
        EntityInfo::Face {
            model,
            id,
            area,
            normal,
            edges,
            holes,
            model_object,
            front,
            back,
        } => {
            let u = kadakas_units::current();
            egui::Grid::new("entity-info-face")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-type"), tr!("modeler-entity-face"));
                    row(ui, tr!("modeler-entity-id"), id.to_string());
                    row(ui, tr!("modeler-entity-model"), model.clone());
                    row(ui, tr!("modeler-entity-area"), u.area_text(*area));
                    row(ui, tr!("modeler-entity-normal"), normal_text(*normal));
                    row(ui, tr!("modeler-entity-loop-edges"), edges.to_string());
                    row(ui, tr!("modeler-entity-holes"), holes.to_string());
                    for (label, side, current) in [
                        (
                            tr!("modeler-entity-front-material"),
                            PaintSide::Front,
                            *front,
                        ),
                        (tr!("modeler-entity-back-material"), PaintSide::Back, *back),
                    ] {
                        ui.label(egui::RichText::new(label).weak());
                        if let Some(new) =
                            material_combo(ui, ("face-material", side as u8), current, data.rows)
                        {
                            out.push(TrayAction::Material(Request::SetFaces {
                                model: *model_object,
                                faces: vec![FaceId(*id)],
                                side,
                                material: new,
                            }));
                        }
                        ui.end_row();
                    }
                });
        }
        EntityInfo::Edge {
            model,
            id,
            length,
            start,
            end,
            curve,
            faces,
            style,
            shape,
        } => {
            let u = kadakas_units::current();
            egui::Grid::new("entity-info-edge")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-type"), tr!("modeler-entity-edge"));
                    row(ui, tr!("modeler-entity-id"), id.to_string());
                    row(ui, tr!("modeler-entity-model"), model.clone());
                    row(ui, tr!("modeler-entity-length"), u.length_text(*length));
                    row(ui, tr!("modeler-entity-start"), lengths(*start));
                    row(ui, tr!("modeler-entity-end"), lengths(*end));
                    if let Some((c, n)) = curve {
                        row(
                            ui,
                            tr!("modeler-entity-curve"),
                            tr!("modeler-entity-curve-value", id = *c, edges = *n),
                        );
                    }
                    row(ui, tr!("modeler-entity-face-uses"), faces.to_string());
                    let yes_no = |b: bool| {
                        if b {
                            tr!("modeler-entity-yes")
                        } else {
                            tr!("modeler-entity-no")
                        }
                    };
                    row(ui, tr!("modeler-entity-soft"), yes_no(style.soft));
                    row(ui, tr!("modeler-entity-smooth"), yes_no(style.smooth));
                    row(ui, tr!("modeler-entity-hidden"), yes_no(style.hidden));
                    if let Some(s) = shape {
                        crate::curve_panel::rows(ui, s, out);
                    }
                });
        }
        EntityInfo::Curve { shape } => {
            egui::Grid::new("entity-info-curve")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-type"), tr!("modeler-entity-curve"));
                    row(ui, tr!("modeler-entity-model"), shape.model.to_string());
                    crate::curve_panel::rows(ui, shape, out);
                });
        }
        EntityInfo::Parts {
            faces,
            edges,
            soft_surface,
        } => {
            if *soft_surface {
                ui.label(tr!("modeler-entity-soft-surface", faces = *faces));
            } else {
                ui.label(tr!("modeler-entity-parts", faces = *faces, edges = *edges));
            }
        }
        EntityInfo::Many { count, size } => {
            ui.label(tr!("modeler-entity-many", count = *count));
            if let Some(s) = size {
                egui::Grid::new("entity-info-many")
                    .num_columns(2)
                    .show(ui, |ui| {
                        row(ui, tr!("modeler-entity-size"), lengths(*s));
                    });
            }
        }
    }
}

/// True when the person let go of the widget or left it: the moment to apply the value.
fn commit(r: &egui::Response) -> bool {
    r.drag_stopped() || r.lost_focus()
}

/// The small editor of a material. The fields are kept in a buffer and applied one by one.
fn editor(
    ui: &mut egui::Ui,
    tray: &mut TrayUi,
    id: ResourceId,
    doc: &Document,
    out: &mut Vec<TrayAction>,
) {
    let (Some(m), Some(r)) = (present::material(doc, id), doc.resources.get(id)) else {
        return;
    };
    // Load the buffer, or load it again when the document changed under it (an undo).
    let stale = tray
        .buffer
        .as_ref()
        .is_none_or(|b| b.id != id || b.seen != *m || b.seen_name != r.name);
    if stale {
        tray.buffer = Some(EditBuffer {
            id,
            name: r.name.clone(),
            seen: m.clone(),
            seen_name: r.name.clone(),
            value: m.clone(),
        });
    }
    let b = tray.buffer.as_mut().expect("just loaded");
    let mut edits: Vec<MaterialEdit> = Vec::new();
    egui::Grid::new("material-editor")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(tr!("modeler-materials-name")).weak());
            let name = ui.add(egui::TextEdit::singleline(&mut b.name).desired_width(120.0));
            if name.lost_focus() {
                edits.push(MaterialEdit::Name(b.name.clone()));
            }
            ui.end_row();

            ui.label(egui::RichText::new(tr!("modeler-materials-color")).weak());
            ui.horizontal(|ui| {
                let mut rgb = [
                    b.value.base_color.0[0],
                    b.value.base_color.0[1],
                    b.value.base_color.0[2],
                ];
                let mut done = false;
                for c in &mut rgb {
                    let r = ui.add(egui::DragValue::new(c).range(0..=255).ui_number());
                    done |= commit(&r);
                }
                b.value.base_color.0[..3].copy_from_slice(&rgb);
                if done {
                    edits.push(MaterialEdit::Color(rgb));
                }
            });
            ui.end_row();

            for (label, which) in [
                (tr!("modeler-materials-opacity"), 0),
                (tr!("modeler-materials-roughness"), 1),
                (tr!("modeler-materials-metallic"), 2),
            ] {
                ui.label(egui::RichText::new(label).weak());
                let v = match which {
                    0 => &mut b.value.opacity,
                    1 => &mut b.value.roughness,
                    _ => &mut b.value.metallic,
                };
                let r = ui.add(egui::Slider::new(v, 0.0..=1.0)
                        .fixed_decimals(2)
                        .ui_number(),
                );
                if commit(&r) {
                    edits.push(match which {
                        0 => MaterialEdit::Opacity(*v),
                        1 => MaterialEdit::Roughness(*v),
                        _ => MaterialEdit::Metallic(*v),
                    });
                }
                ui.end_row();
            }

            ui.label(egui::RichText::new(tr!("modeler-materials-pattern")).weak());
            let current = match &b.value.texture {
                None => tr!("modeler-materials-pattern-none"),
                Some(Texture::Procedural { kind, .. }) => pattern_name(*kind),
                Some(Texture::Image { .. }) => tr!("modeler-materials-pattern-image"),
            };
            egui::ComboBox::from_id_salt("material-pattern")
                .selected_text(current)
                .width(120.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            b.value.texture.is_none(),
                            tr!("modeler-materials-pattern-none"),
                        )
                        .clicked()
                    {
                        edits.push(MaterialEdit::Pattern(None));
                    }
                    for p in Procedural::ALL {
                        let on = matches!(&b.value.texture, Some(Texture::Procedural { kind, .. }) if *kind == p);
                        if ui.selectable_label(on, pattern_name(p)).clicked() {
                            edits.push(MaterialEdit::Pattern(Some(p)));
                        }
                    }
                });
            ui.end_row();

            ui.label(egui::RichText::new(tr!("modeler-materials-mapping")).weak());
            egui::ComboBox::from_id_salt("material-mapping")
                .selected_text(mapping_name(b.value.mapping.kind))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for k in [
                        MappingKind::Planar,
                        MappingKind::Box,
                        MappingKind::Cylindrical,
                    ] {
                        if ui
                            .selectable_label(b.value.mapping.kind == k, mapping_name(k))
                            .clicked()
                        {
                            edits.push(MaterialEdit::MappingKind(k));
                        }
                    }
                });
            ui.end_row();

            let pair = |ui: &mut egui::Ui, v: &mut [f64; 2], min: f64| -> bool {
                let mut done = false;
                ui.horizontal(|ui| {
                    for c in v.iter_mut() {
                        let r = ui.add(egui::DragValue::new(c)
                                .speed(1.0)
                                .range(min..=1e9)
                                .ui_number(),
                        );
                        done |= commit(&r);
                    }
                });
                done
            };
            ui.label(egui::RichText::new(tr!("modeler-materials-repeat")).weak());
            let mut scale = b.value.mapping.scale;
            if pair(ui, &mut scale, 0.001) {
                edits.push(MaterialEdit::Scale(scale));
            }
            b.value.mapping.scale = scale;
            ui.end_row();

            ui.label(egui::RichText::new(tr!("modeler-materials-rotation")).weak());
            let r = ui.add(egui::DragValue::new(&mut b.value.mapping.rotation)
                    .speed(1.0)
                    .ui_number(),
            );
            if commit(&r) {
                edits.push(MaterialEdit::Rotation(b.value.mapping.rotation));
            }
            ui.end_row();

            ui.label(egui::RichText::new(tr!("modeler-materials-offset")).weak());
            let mut offset = b.value.mapping.offset;
            if pair(ui, &mut offset, -1e9) {
                edits.push(MaterialEdit::Offset(offset));
            }
            b.value.mapping.offset = offset;
            ui.end_row();
        });
    for e in edits {
        out.push(TrayAction::Material(Request::Edit(id, e)));
    }
}

/// The Materials panel: the buttons, the list with thumbnails, what to do with the chosen
/// material, and the small editor.
pub fn materials(ui: &mut egui::Ui, tray: &mut TrayUi, data: &TrayData, out: &mut Vec<TrayAction>) {
    tray.retain(data.rows);
    ui.horizontal_wrapped(|ui| {
        if ui.button(tr!("modeler-materials-new")).clicked() {
            out.push(TrayAction::Material(Request::New));
        }
        if ui
            .add_enabled(
                data.missing > 0,
                egui::Button::new(tr!("modeler-materials-add-defaults")),
            )
            .clicked()
        {
            out.push(TrayAction::Material(Request::AddDefaults));
        }
        if ui
            .add_enabled(
                data.rows.iter().any(|r| r.uses == 0),
                egui::Button::new(tr!("modeler-materials-purge")),
            )
            .clicked()
        {
            out.push(TrayAction::Material(Request::Purge));
        }
    });
    if data.rows.is_empty() {
        ui.label(egui::RichText::new(tr!("modeler-materials-empty")).weak());
    }
    // The default: an empty bucket clears the paint.
    let ctx = ui.ctx().clone();
    egui::ScrollArea::vertical()
        .id_salt("materials-list")
        .max_height(220.0)
        .show(ui, |ui| {
            if ui
                .selectable_label(data.bucket.is_none(), tr!("modeler-materials-default"))
                .clicked()
            {
                out.push(TrayAction::Material(Request::Select(None)));
            }
            for r in data.rows {
                let Some(m) = present::material(data.doc, r.id) else {
                    continue;
                };
                let tex = tray.thumb(&ctx, r, m);
                let resp = ui
                    .horizontal(|ui| {
                        ui.image((tex, egui::vec2(24.0, 24.0)));
                        ui.selectable_label(data.bucket == Some(r.id), r.name.clone())
                    })
                    .inner;
                if resp.clicked() {
                    out.push(TrayAction::Material(Request::Select(Some(r.id))));
                }
                if resp.double_clicked() {
                    out.push(TrayAction::Material(Request::Editor(Some(r.id))));
                }
                resp.on_hover_text(tr!("modeler-materials-uses", count = r.uses));
            }
        });
    // What to do with the chosen material.
    if let Some(id) = data
        .bucket
        .filter(|id| data.rows.iter().any(|r| r.id == *id))
    {
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("modeler-materials-edit")).clicked() {
                out.push(TrayAction::Material(Request::Editor(
                    (data.panel.editing != Some(id)).then_some(id),
                )));
            }
            if ui.button(tr!("modeler-materials-duplicate")).clicked() {
                out.push(TrayAction::Material(Request::Duplicate(id)));
            }
            if ui.button(tr!("modeler-materials-delete")).clicked() {
                out.push(TrayAction::Material(Request::Delete(id)));
            }
        });
    }
    // A used material asks first.
    if let Some(p) = &data.panel.pending_delete {
        ui.group(|ui| {
            ui.label(tr!(
                "modeler-materials-delete-ask",
                name = p.name.clone(),
                count = p.uses.total()
            ));
            ui.horizontal(|ui| {
                if ui.button(tr!("modeler-materials-delete-yes")).clicked() {
                    out.push(TrayAction::Material(Request::ConfirmDelete));
                }
                if ui.button(tr!("modeler-materials-delete-no")).clicked() {
                    out.push(TrayAction::Material(Request::CancelDelete));
                }
            });
        });
    }
    if let Some(id) = data.panel.editing {
        ui.separator();
        editor(ui, tray, id, data.doc, out);
    }
}

/// The parts of the tray that `scenes_panel` draws (MOD-10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenesSlot {
    /// The list of scenes.
    Scenes,
    /// The style picker.
    Styles,
}

/// Draws the whole tray inside `ui`. Returns what the person asked for. `tags` draws the part of
/// the Tags code for a slot (the tag selector under Entity Info, and the Tags panel; see
/// `tags_panel`).
pub fn show(
    ui: &mut egui::Ui,
    state: &mut TrayState,
    tray: &mut TrayUi,
    data: &TrayData,
    tags: &mut dyn FnMut(&mut egui::Ui, crate::tags_panel::Slot),
    scenes: &mut dyn FnMut(&mut egui::Ui, ScenesSlot),
) -> Vec<TrayAction> {
    let mut out = Vec::new();
    egui::ScrollArea::vertical().show(ui, |ui| {
        panel(
            ui,
            "tray-entity-info",
            tr!("modeler-tray-entity-info"),
            &mut state.entity_info,
            |ui| {
                entity_info(ui, data, &mut out);
                tags(ui, crate::tags_panel::Slot::EntityInfo);
            },
        );
        panel(
            ui,
            "tray-tags",
            tr!("modeler-tray-tags"),
            &mut state.tags,
            |ui| tags(ui, crate::tags_panel::Slot::Panel),
        );
        panel(
            ui,
            "tray-materials",
            tr!("modeler-tray-materials"),
            &mut state.materials,
            |ui| materials(ui, tray, data, &mut out),
        );
        panel(
            ui,
            "tray-soften",
            tr!("modeler-tray-soften"),
            &mut state.soften,
            |ui| match crate::soften::panel(ui, &mut tray.soften, data.edges_selected) {
                Some(crate::soften::Change::Apply) => out.push(TrayAction::Soften),
                Some(crate::soften::Change::Preview) => out.push(TrayAction::SoftenPreview),
                None => {}
            },
        );
        panel(
            ui,
            "tray-scenes",
            tr!("modeler-tray-scenes"),
            &mut state.scenes,
            |ui| scenes(ui, ScenesSlot::Scenes),
        );
        panel(
            ui,
            "tray-styles",
            tr!("modeler-tray-styles"),
            &mut state.styles,
            |ui| scenes(ui, ScenesSlot::Styles),
        );
    });
    out
}

/// One panel: a title bar that collapses and expands it.
fn panel(
    ui: &mut egui::Ui,
    id: &str,
    title: String,
    open: &mut bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    let header = egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .id_salt(id)
        .open(Some(*open))
        .show(ui, body);
    if header.header_response.clicked() {
        *open = !*open;
    }
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_reads_as_words() {
        assert_eq!(kind_text("hardscape"), "Hardscape");
        assert_eq!(kind_text("landscape_area"), "Landscape area");
        assert_eq!(kind_text(""), "");
        assert_eq!(kind_text("ünknown_thing"), "Ünknown thing");
    }
}
