//! Annotation in the app: painting, picking, the Tool bar options, and the fields in Object Info.

use crate::units_ui::DragValueExt as _;
use egui::{Align2, Color32, DragValue, FontId, Painter, Pos2, Ui};
use glam::DVec2;
use kadakas_core::annot::draw::{Fill, Prim};
use kadakas_core::annot::{Annot, Legend};
use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, Document, Object, Shape};
use kadakas_core::resource::ResourceId;
use kadakas_l10n::tr;

use crate::canvas::{Style, color32};
use crate::view::{View, ViewExt as _};
use kadakas_ui_core::tools::{DimToolMode, Tool, ToolSettings};

thread_local! {
    /// Primitives and legend rows by document revision for drawing, so a frame does not read the
    /// whole document again for every tag and legend. Picking uses a cache of the app (see `Ctx`).
    static CACHE: (std::cell::Cell<u64>, kadakas_annot::AnnotCache) = Default::default();
    /// The document that the cache belongs to (its address, 0: not set) and whether ghosts of a
    /// compare are being drawn. A ghost has the ID of the live object but another shape, and a
    /// ghost document is not the live one: neither may read or fill the cache of the live drawing.
    static LIVE_DOC: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static GHOST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tells the drawing cache which document it is for. The canvas calls it before it draws.
pub fn set_document(doc: &Document) {
    LIVE_DOC.with(|d| d.set(std::ptr::from_ref(doc) as usize));
}

/// While true, annotations are drawn without the cache (the ghosts of a compare).
pub fn set_ghost(ghost: bool) {
    GHOST.with(|g| g.set(ghost));
}

/// Tells the drawing cache which document revision is current. The canvas calls it before it draws.
pub fn set_revision(revision: u64) {
    CACHE.with(|c| c.0.set(revision));
}

/// The primitives of an annotation object (from the drawing cache).
fn object_prims(doc: &Document, o: &Object) -> std::sync::Arc<Vec<Prim>> {
    let live = LIVE_DOC.with(std::cell::Cell::get);
    let same_doc = live == 0 || live == std::ptr::from_ref(doc) as usize;
    let revision = if same_doc && !GHOST.with(std::cell::Cell::get) {
        CACHE.with(|c| c.0.get())
    } else {
        0 // revision 0: nothing is read from the cache or kept in it
    };
    CACHE.with(|c| c.1.prims(doc, o, revision))
}

/// Draws an annotation object. `offset` is added to every point (drag ghosts).
pub fn draw_object(
    painter: &Painter,
    view: &View,
    doc: &Document,
    o: &Object,
    st: Style,
    offset: DVec2,
) {
    if !matches!(o.shape, Shape::Annotation(_)) {
        return;
    }
    draw_prims(painter, view, &object_prims(doc, o), st, offset);
}

/// Draws the preview of an annotation that a tool is making (on the active layer).
pub fn draw_preview(painter: &Painter, view: &View, doc: &Document, a: &Annot, st: Style) {
    let ctx = kadakas_core::annot::draw::Ctx::layer(doc, doc.active_layer);
    draw_prims(
        painter,
        view,
        &kadakas_annot::prims_in(doc, a, &ctx),
        st,
        DVec2::ZERO,
    );
}

fn draw_prims(painter: &Painter, view: &View, prims: &[Prim], st: Style, offset: DVec2) {
    let s = |p: DVec2| view.to_screen(p + offset);
    for p in prims {
        match p {
            Prim::Poly {
                pts,
                closed,
                fill,
                color,
            } => {
                let pen = color.map_or(st.pen, color32);
                let screen: Vec<Pos2> = pts.iter().map(|q| s(*q)).collect();
                let fill_color = match fill {
                    Fill::None => Color32::TRANSPARENT,
                    Fill::Ink => pen,
                    Fill::Color(c) => color32(*c),
                };
                if *closed && pts.len() >= 3 {
                    let moved: Vec<DVec2> = pts.iter().map(|q| *q + offset).collect();
                    crate::canvas::fill_polygon(painter, &moved, &screen, fill_color);
                }
                painter.add(egui::epaint::PathShape {
                    points: screen,
                    closed: *closed,
                    fill: Color32::TRANSPARENT,
                    stroke: egui::epaint::PathStroke::new(st.width.min(3.0), pen),
                });
            }
            Prim::Text {
                at,
                text,
                height,
                center,
                color,
                ..
            } => {
                let size = (*height * view.zoom) as f32;
                if size >= 3.0 {
                    let anchor = if *center {
                        Align2::CENTER_BOTTOM
                    } else {
                        Align2::LEFT_BOTTOM
                    };
                    painter.text(
                        s(*at),
                        anchor,
                        text,
                        FontId::proportional(size.min(400.0)),
                        color.map_or(st.pen, color32),
                    );
                }
            }
        }
    }
}

/// The command that adds a graphic legend at `at` (the top left corner), on the active layer.
pub fn legend_command(at: DVec2) -> Command {
    let legend = Legend {
        at,
        title: "Legend".into(),
        criteria: String::new(),
        group_by: "Style".into(),
        show_count: true,
        swatch_mm: 6.0,
        style: None,
    };
    Command::AddObject {
        shape: Shape::Annotation(Box::new(Annot::Legend(legend))),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    }
}

fn combo(
    ui: &mut Ui,
    salt: &str,
    current: Option<ResourceId>,
    items: &[(ResourceId, String)],
    none: &str,
) -> Option<Option<ResourceId>> {
    let mut chosen = current;
    let text = current
        .and_then(|c| items.iter().find(|(i, _)| *i == c))
        .map_or(none.to_string(), |(_, n)| n.clone());
    egui::ComboBox::from_id_salt(salt)
        .selected_text(text)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, none);
            for (id, n) in items {
                ui.selectable_value(&mut chosen, Some(*id), n);
            }
        });
    (chosen != current).then_some(chosen)
}

/// The resources of one annotation type, with their names.
fn styles(doc: &Document, ty: &str) -> Vec<(ResourceId, String)> {
    doc.resources
        .iter()
        .filter(|r| r.body.type_name() == ty)
        .map(|r| (r.id, r.name.clone()))
        .collect()
}

/// The "Edit style..." button under a style list. It asks the app to open the style editor.
fn edit_button(ui: &mut Ui, style: Option<ResourceId>, requests: &mut PanelRequests) {
    let r = ui.add_enabled(
        style.is_some(),
        egui::Button::new(tr!("style-ui-edit-style")),
    );
    if r.clicked() {
        requests.edit_style = style;
    }
}

/// The options of the annotation tools in the Tool bar.
pub fn tool_options(
    ui: &mut Ui,
    tool: Tool,
    settings: &mut ToolSettings,
    doc: &Document,
    requests: &mut PanelRequests,
) {
    let s = &mut settings.annot;
    ui.add_space(6.0);
    match tool {
        Tool::Dimension => {
            ui.label(tr!("annot-ui-dimension-mode"));
            ui.horizontal_wrapped(|ui| {
                for m in DimToolMode::ALL {
                    if ui
                        .selectable_label(s.dim_mode == m, m.label())
                        .on_hover_text(m.help())
                        .clicked()
                    {
                        s.dim_mode = m;
                    }
                }
            });
            ui.label(tr!("annot-ui-dimension-style"));
            if let Some(c) = combo(
                ui,
                "dim_style",
                s.dim_style,
                &styles(doc, "dimension_style"),
                "(default)",
            ) {
                s.dim_style = c;
            }
            edit_button(ui, s.dim_style, requests);
            ui.horizontal(|ui| {
                ui.label(tr!("annot-ui-line-offset"));
                ui.add(
                    DragValue::new(&mut s.offset_mm)
                        .speed(0.5)
                        .range(-200.0..=200.0)
                        .paper_mm(),
                );
            })
            .response
            .on_hover_text(tr!("annot-ui-distance-of-the-dimension-line-for-chain"));
        }
        Tool::Callout => {
            ui.label(tr!("annot-ui-text-style"));
            if let Some(c) = combo(
                ui,
                "callout_text_style",
                s.text_style,
                &styles(doc, "text_style"),
                "(default)",
            ) {
                s.text_style = c;
            }
            edit_button(ui, s.text_style, requests);
            ui.label(tr!("annot-ui-text"));
            ui.text_edit_singleline(&mut s.callout_text);
        }
        Tool::DataTag => {
            ui.label(tr!("annot-ui-tag-style"));
            if let Some(c) = combo(
                ui,
                "tag_style",
                s.tag_style,
                &styles(doc, "tag_style"),
                "(first)",
            ) {
                s.tag_style = c;
            }
            edit_button(ui, s.tag_style, requests);
        }
        Tool::NorthArrow => {
            ui.horizontal(|ui| {
                ui.label(tr!("annot-ui-size"));
                ui.add(
                    DragValue::new(&mut s.north_size_mm)
                        .speed(0.5)
                        .range(2.0..=300.0)
                        .paper_mm(),
                );
            });
        }
        Tool::ScaleBar => {
            ui.horizontal(|ui| {
                ui.label(tr!("annot-ui-segment"));
                ui.add(
                    DragValue::new(&mut s.bar_segment_m)
                        .speed(0.1)
                        .range(0.001..=100000.0)
                        .length_m(),
                );
            });
            ui.horizontal(|ui| {
                ui.label(tr!("annot-ui-segments"));
                ui.add(DragValue::new(&mut s.bar_segments).range(1..=20));
            });
        }
        Tool::RevisionCloud => {
            ui.label(tr!("annot-ui-revision"));
            ui.text_edit_singleline(&mut s.revision);
        }
        _ => {}
    }
}

/// What the buttons of the Annotation part of the tool panel asked for. The app runs it after the
/// frame.
#[derive(Default)]
pub struct PanelRequests {
    /// Commands to run next.
    pub commands: Vec<Command>,
    /// The Graphic Legend button was pressed.
    pub legend: bool,
    /// An "Edit style..." button was pressed: open the style editor on this resource.
    pub edit_style: Option<ResourceId>,
}

/// The buttons of the Annotation part of the tool panel: the Graphic Legend command and default
/// styles. They fill `requests`.
pub fn panel_buttons(ui: &mut Ui, doc: &Document, requests: &mut PanelRequests) {
    let cmds = &mut requests.commands;
    if ui
        .button(tr!("annot-ui-graphic-legend"))
        .on_hover_text(tr!("annot-ui-add-a-legend-that-lists-the-objects"))
        .clicked()
    {
        requests.legend = true;
    }
    let missing = kadakas_ui_core::style_edit::missing_default_styles(doc);
    if !missing.is_empty() {
        let r = ui
            .button(tr!("annot-ui-add-default-styles"))
            .on_hover_text(tr!("annot-ui-adds-a-text-style-a-dimension-style"));
        if r.clicked() {
            cmds.extend(missing);
        }
    }
}

fn num(ui: &mut Ui, commit: &mut bool, label: &str, v: &mut f64, speed: f64) {
    ui.label(label);
    let r = ui.add(DragValue::new(v).speed(speed));
    *commit |= r.drag_stopped() || r.lost_focus();
    ui.end_row();
}

/// Fields of an annotation in Object Info. Returns true when an edit is finished.
pub fn fields(ui: &mut Ui, a: &mut Annot) -> bool {
    let mut commit = false;
    match a {
        Annot::Dimension(d) => {
            num(
                ui,
                &mut commit,
                &tr!("annot-ui-line-offset-paper-mm"),
                &mut d.offset,
                0.5,
            );
            ui.label(tr!("annot-ui-text"));
            let mut t = d.text.clone().unwrap_or_default();
            let r = ui.text_edit_singleline(&mut t);
            d.text = (!t.is_empty()).then_some(t);
            commit |= r.lost_focus();
            ui.end_row();
            ui.label(tr!("annot-ui-associative"));
            ui.label(tr!(
                "annot-ui-of-points",
                used = d.anchors.iter().flatten().count(),
                total = d.points.len()
            ));
            ui.end_row();
            if d.detached {
                ui.label(tr!("annot-ui-detached"));
                ui.label(tr!("annot-ui-an-anchored-vertex-or-edge-is-gone"));
                ui.end_row();
            }
        }
        Annot::Text(t) => {
            ui.label(tr!("annot-ui-text"));
            let r = ui.text_edit_multiline(&mut t.text);
            commit |= r.lost_focus();
            ui.end_row();
        }
        Annot::Callout(c) => {
            ui.label(tr!("annot-ui-text"));
            let r = ui.text_edit_multiline(&mut c.text);
            commit |= r.lost_focus();
            ui.end_row();
            num(
                ui,
                &mut commit,
                &tr!("annot-ui-shoulder-paper-mm"),
                &mut c.shoulder_mm,
                0.5,
            );
            if c.detached {
                ui.label(tr!("annot-ui-detached"));
                ui.label(tr!("annot-ui-a-leader-lost-its-vertex-or-edge"));
                ui.end_row();
            }
        }
        Annot::DataTag(t) => {
            let mut x = t.offset.x;
            let mut y = t.offset.y;
            num(
                ui,
                &mut commit,
                &tr!("annot-ui-offset-x-paper-mm"),
                &mut x,
                0.5,
            );
            num(
                ui,
                &mut commit,
                &tr!("annot-ui-offset-y-paper-mm"),
                &mut y,
                0.5,
            );
            t.offset = DVec2::new(x, y);
        }
        Annot::Legend(l) => {
            ui.label(tr!("annot-ui-title"));
            commit |= ui.text_edit_singleline(&mut l.title).lost_focus();
            ui.end_row();
            ui.label(tr!("annot-ui-criteria"));
            commit |= ui.text_edit_singleline(&mut l.criteria).lost_focus();
            ui.end_row();
            ui.label(tr!("annot-ui-group-by"));
            commit |= ui.text_edit_singleline(&mut l.group_by).lost_focus();
            ui.end_row();
            ui.label(tr!("annot-ui-show-count"));
            commit |= ui.checkbox(&mut l.show_count, "").changed();
            ui.end_row();
        }
        Annot::NorthArrow(n) => {
            num(
                ui,
                &mut commit,
                &tr!("annot-ui-size-paper-mm"),
                &mut n.size_mm,
                0.5,
            );
            num(
                ui,
                &mut commit,
                &tr!("annot-ui-rotation"),
                &mut n.rotation_deg,
                1.0,
            );
            ui.label(tr!("annot-ui-true-north"));
            commit |= ui
                .checkbox(&mut n.use_georef, tr!("annot-ui-use-georeference"))
                .changed();
            ui.end_row();
        }
        Annot::ScaleBar(b) => {
            num(
                ui,
                &mut commit,
                &tr!("annot-ui-segment"),
                &mut b.segment,
                0.1,
            );
            let mut n = f64::from(b.segments);
            num(ui, &mut commit, &tr!("annot-ui-segments"), &mut n, 0.1);
            b.segments = n.round().clamp(1.0, 20.0) as u32;
        }
        Annot::RevisionCloud(r) => {
            ui.label(tr!("annot-ui-revision"));
            commit |= ui.text_edit_singleline(&mut r.revision).lost_focus();
            ui.end_row();
            ui.label(tr!("annot-ui-date"));
            commit |= ui.text_edit_singleline(&mut r.date).lost_focus();
            ui.end_row();
            ui.label(tr!("annot-ui-description"));
            commit |= ui.text_edit_singleline(&mut r.description).lost_focus();
            ui.end_row();
            ui.label(tr!("annot-ui-author"));
            commit |= ui.text_edit_singleline(&mut r.author).lost_focus();
            ui.end_row();
        }
    }
    commit
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_api::Session;
    use serde_json::json;

    /// The live document with a moved north arrow, the old document, and the arrow in both. A
    /// "previous" ghost has the ID of the live object but the old shape.
    fn moved_arrow() -> (Document, Document, Object, Object) {
        let mut s = Session::with_sequential_ids();
        s.handle("north_arrow.add", json!({"at": [0, 0]})).unwrap();
        let old = s.editor.doc().clone();
        let id = *old.objects.keys().next().unwrap();
        s.handle(
            "execute",
            json!({"command": {"op": "move_objects", "ids": [id.to_string()], "delta": [50000, 0]}}),
        )
        .unwrap();
        let live = s.editor.doc().clone();
        let (a, b) = (old.objects[&id].clone(), live.objects[&id].clone());
        (live, old, a, b)
    }

    #[test]
    fn a_compare_ghost_and_the_live_annotation_each_show_their_own_shape() {
        let (live, old, old_obj, live_obj) = moved_arrow();
        let fresh = |d: &Document, o: &Object| {
            kadakas_annot::AnnotCache::default()
                .prims(d, o, 0)
                .as_ref()
                .clone()
        };
        let (want_live, want_old) = (fresh(&live, &live_obj), fresh(&old, &old_obj));
        assert_ne!(want_live, want_old, "the arrow moved");

        set_revision(7);
        set_document(&live);
        // The ghost is drawn first, against the old document, and against the live one (no
        // ghost document): neither may fill the cache of the live drawing.
        set_ghost(true);
        assert_eq!(*object_prims(&old, &old_obj), want_old);
        assert_eq!(*object_prims(&live, &old_obj), fresh(&live, &old_obj));
        set_ghost(false);
        assert_eq!(
            *object_prims(&live, &live_obj),
            want_live,
            "the live annotation is not the ghost"
        );
        // A document that is not the live one does not use the cache either.
        assert_eq!(*object_prims(&old, &old_obj), want_old);
        // The other way round: the live annotation is cached first, a ghost does not read it.
        set_ghost(true);
        assert_eq!(*object_prims(&live, &old_obj), fresh(&live, &old_obj));
        set_ghost(false);
        // After the compare closes the live annotation is still right.
        assert_eq!(*object_prims(&live, &live_obj), want_live);
        set_revision(0);
    }
}
