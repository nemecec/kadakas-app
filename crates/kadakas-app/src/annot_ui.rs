//! Annotation in the app: painting, picking, the Tool bar options, and the fields in Object Info.

use egui::{Align2, Color32, DragValue, FontId, Painter, Pos2, Ui};
use glam::DVec2;
use kadakas_core::annot::draw::{Fill, Prim, prims_bounds};
use kadakas_core::annot::{Annot, Legend};
use kadakas_core::command::Command;
use kadakas_core::geom::dist_to_segment;
use kadakas_core::model::{Attributes, Document, Object, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};

use crate::canvas::{Style, color32};
use crate::tools::{DimToolMode, Tool, ToolSettings};
use crate::view::View;

thread_local! {
    /// Primitives and legend rows by document revision, so a frame or a click does not read the
    /// whole document again for every tag and legend.
    static CACHE: (std::cell::Cell<u64>, kadakas_annot::AnnotCache) = Default::default();
}

/// Tells the cache which document revision is current. The canvas calls it before it draws or picks.
pub fn set_revision(revision: u64) {
    CACHE.with(|c| c.0.set(revision));
}

/// The primitives of an annotation object (from the cache).
fn object_prims(doc: &Document, o: &Object) -> std::sync::Arc<Vec<Prim>> {
    CACHE.with(|c| c.1.prims(doc, o, c.0.get()))
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

/// True if `p` is within `tol` of a line of the annotation or inside the box of its text.
pub fn hit(doc: &Document, o: &Object, p: DVec2, tol: f64) -> bool {
    object_prims(doc, o).iter().any(|prim| match prim {
        Prim::Poly { pts, closed, .. } => {
            let n = pts.len();
            let segs = if *closed { n } else { n.saturating_sub(1) };
            (0..segs).any(|i| dist_to_segment(p, pts[i], pts[(i + 1) % n]) <= tol)
        }
        text => {
            let (lo, hi) = text.bounds();
            p.cmpge(lo - tol).all() && p.cmple(hi + tol).all()
        }
    })
}

/// The box around what an annotation draws.
pub fn bounds(doc: &Document, o: &Object) -> (DVec2, DVec2) {
    prims_bounds(&object_prims(doc, o)).unwrap_or_else(|| o.shape.bounds())
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

/// The options of the annotation tools in the Tool bar.
pub fn tool_options(ui: &mut Ui, tool: Tool, settings: &mut ToolSettings, doc: &Document) {
    let s = &mut settings.annot;
    ui.add_space(6.0);
    match tool {
        Tool::Dimension => {
            ui.label("Dimension mode");
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
            ui.label("Dimension style");
            if let Some(c) = combo(
                ui,
                "dim_style",
                s.dim_style,
                &styles(doc, "dimension_style"),
                "(default)",
            ) {
                s.dim_style = c;
            }
            ui.horizontal(|ui| {
                ui.label("Line offset");
                ui.add(DragValue::new(&mut s.offset_mm).speed(0.5).range(-200.0..=200.0).suffix(" mm"));
            })
            .response
            .on_hover_text("Distance of the dimension line for chain, baseline and ordinate dimensions, in paper mm");
        }
        Tool::Callout => {
            ui.label("Text style");
            if let Some(c) = combo(
                ui,
                "callout_text_style",
                s.text_style,
                &styles(doc, "text_style"),
                "(default)",
            ) {
                s.text_style = c;
            }
            ui.label("Text");
            ui.text_edit_singleline(&mut s.callout_text);
        }
        Tool::DataTag => {
            ui.label("Tag style");
            if let Some(c) = combo(
                ui,
                "tag_style",
                s.tag_style,
                &styles(doc, "tag_style"),
                "(first)",
            ) {
                s.tag_style = c;
            }
        }
        Tool::NorthArrow => {
            ui.horizontal(|ui| {
                ui.label("Size");
                ui.add(
                    DragValue::new(&mut s.north_size_mm)
                        .speed(0.5)
                        .range(2.0..=300.0)
                        .suffix(" mm"),
                );
            });
        }
        Tool::ScaleBar => {
            ui.horizontal(|ui| {
                ui.label("Segment");
                ui.add(
                    DragValue::new(&mut s.bar_segment_m)
                        .speed(0.1)
                        .range(0.001..=100000.0)
                        .suffix(" m"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Segments");
                ui.add(DragValue::new(&mut s.bar_segments).range(1..=20));
            });
        }
        Tool::RevisionCloud => {
            ui.label("Revision");
            ui.text_edit_singleline(&mut s.revision);
        }
        _ => {}
    }
}

/// The commands of the Annotation part of the tool panel: the Graphic Legend command and default
/// styles. The app runs what is left in `pending` and places the legend when `legend_requested` is set.
pub fn panel_buttons(ui: &mut Ui, doc: &Document, s: &mut crate::tools::AnnotSettings) {
    let cmds = &mut s.pending;
    if ui
        .button("Graphic Legend")
        .on_hover_text("Add a legend that lists the objects by style and updates itself")
        .clicked()
    {
        s.legend_requested = true;
    }
    let has = |ty: &str| doc.resources.iter().any(|r| r.body.type_name() == ty);
    if !has("dimension_style") || !has("tag_style") || !has("text_style") {
        let r = ui.button("Add default styles").on_hover_text(
            "Adds a text style, a dimension style and a tag style, if the document has none",
        );
        if r.clicked() {
            use kadakas_core::annot::style::{AnnotResource, DimensionStyle, TagStyle, TextStyle};
            let add = |name: &str, r: AnnotResource| Command::AddResource {
                name: name.into(),
                body: ResourceBody::annot(r),
            };
            if !has("text_style") {
                cmds.push(add(
                    "Text 3 mm",
                    AnnotResource::TextStyle(TextStyle::default()),
                ));
            }
            if !has("dimension_style") {
                cmds.push(add(
                    "Dimension 2.5 mm",
                    AnnotResource::DimensionStyle(DimensionStyle::default()),
                ));
            }
            if !has("tag_style") {
                cmds.push(add(
                    "Plant tag",
                    AnnotResource::TagStyle(TagStyle {
                        lines: vec!["{IdCode}".into(), "{Botanical}".into()],
                        ..TagStyle::default()
                    }),
                ));
            }
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
                "Line offset (paper mm)",
                &mut d.offset,
                0.5,
            );
            ui.label("Text");
            let mut t = d.text.clone().unwrap_or_default();
            let r = ui.text_edit_singleline(&mut t);
            d.text = (!t.is_empty()).then_some(t);
            commit |= r.lost_focus();
            ui.end_row();
            ui.label("Associative");
            ui.label(format!(
                "{} of {} points",
                d.anchors.iter().flatten().count(),
                d.points.len()
            ));
            ui.end_row();
            if d.detached {
                ui.label("Detached");
                ui.label("An anchored vertex or edge is gone. The dimension keeps its last place.");
                ui.end_row();
            }
        }
        Annot::Text(t) => {
            ui.label("Text");
            let r = ui.text_edit_multiline(&mut t.text);
            commit |= r.lost_focus();
            ui.end_row();
        }
        Annot::Callout(c) => {
            ui.label("Text");
            let r = ui.text_edit_multiline(&mut c.text);
            commit |= r.lost_focus();
            ui.end_row();
            num(
                ui,
                &mut commit,
                "Shoulder (paper mm)",
                &mut c.shoulder_mm,
                0.5,
            );
            if c.detached {
                ui.label("Detached");
                ui.label("A leader lost its vertex or edge.");
                ui.end_row();
            }
        }
        Annot::DataTag(t) => {
            let mut x = t.offset.x;
            let mut y = t.offset.y;
            num(ui, &mut commit, "Offset X (paper mm)", &mut x, 0.5);
            num(ui, &mut commit, "Offset Y (paper mm)", &mut y, 0.5);
            t.offset = DVec2::new(x, y);
        }
        Annot::Legend(l) => {
            ui.label("Title");
            commit |= ui.text_edit_singleline(&mut l.title).lost_focus();
            ui.end_row();
            ui.label("Criteria");
            commit |= ui.text_edit_singleline(&mut l.criteria).lost_focus();
            ui.end_row();
            ui.label("Group by");
            commit |= ui.text_edit_singleline(&mut l.group_by).lost_focus();
            ui.end_row();
            ui.label("Show count");
            commit |= ui.checkbox(&mut l.show_count, "").changed();
            ui.end_row();
        }
        Annot::NorthArrow(n) => {
            num(ui, &mut commit, "Size (paper mm)", &mut n.size_mm, 0.5);
            num(ui, &mut commit, "Rotation", &mut n.rotation_deg, 1.0);
            ui.label("True north");
            commit |= ui.checkbox(&mut n.use_georef, "Use georeference").changed();
            ui.end_row();
        }
        Annot::ScaleBar(b) => {
            num(ui, &mut commit, "Segment", &mut b.segment, 0.1);
            let mut n = f64::from(b.segments);
            num(ui, &mut commit, "Segments", &mut n, 0.1);
            b.segments = n.round().clamp(1.0, 20.0) as u32;
        }
        Annot::RevisionCloud(r) => {
            ui.label("Revision");
            commit |= ui.text_edit_singleline(&mut r.revision).lost_focus();
            ui.end_row();
            ui.label("Date");
            commit |= ui.text_edit_singleline(&mut r.date).lost_focus();
            ui.end_row();
            ui.label("Description");
            commit |= ui.text_edit_singleline(&mut r.description).lost_focus();
            ui.end_row();
            ui.label("Author");
            commit |= ui.text_edit_singleline(&mut r.author).lost_focus();
            ui.end_row();
        }
    }
    commit
}
