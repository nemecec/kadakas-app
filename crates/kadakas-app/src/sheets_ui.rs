//! Sheet layers in the app: the sheet canvas, the sheet and saved view sections of the
//! Navigation palette, and PDF export. The sheet is drawn from the display list of
//! `kadakas-print`, so the screen, the PDF, the SVG and the PNG show the same thing.

use kadakas_l10n::tr;
use std::cell::RefCell;
use std::collections::HashMap;

use egui::epaint::PathStroke;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, TextureHandle, Ui};
use glam::DVec2;
use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::model::{LayerId, LayerKind};
use kadakas_core::sheets::{Orientation, PaperSize, SavedView};
use kadakas_print::Page;
use kadakas_print::display::{PSeg, Prim};

use crate::panels::run_command;
use crate::tess;
use crate::view::{View, ViewExt as _};

/// State of the sheet parts of the app.
#[derive(Default)]
pub struct SheetUi {
    /// Textures of 3D viewport images, by the address of their pixel data.
    textures: RefCell<HashMap<usize, TextureHandle>>,
    /// The sheet layer that the canvas showed last. A change fits the page to the window.
    pub last_sheet: Option<LayerId>,
    /// Name typed for a new saved view.
    new_view: String,
    /// The Navigation palette asks the app to save a view with this name. The app knows the zoom.
    pub save_request: Option<String>,
    renaming: Option<(LayerId, String)>,
}

/// The active layer if it is a sheet layer.
pub fn active_sheet(session: &Session) -> Option<LayerId> {
    let doc = session.editor.doc();
    doc.layer(doc.active_layer)
        .filter(|l| l.kind == LayerKind::Sheet)
        .map(|l| l.id)
}

fn col(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// The subpaths of a path, curves cut into short lines. A `Move` starts a subpath: a path with
/// many (the units of a pattern) is not one line.
fn flatten(segs: &[PSeg]) -> Vec<Vec<DVec2>> {
    kadakas_print::display::sample_subpaths(segs, 12)
}

fn ring_area(r: &[DVec2]) -> f64 {
    (0..r.len())
        .map(|i| r[i].perp_dot(r[(i + 1) % r.len()]))
        .sum::<f64>()
        .abs()
        / 2.0
}

fn inside_ring(poly: &[DVec2], p: DVec2) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

/// The rings of a filled path, grouped as outlines with their holes. Which ring is an outline and
/// which a hole is decided by nesting, not by the direction of the ring (a ring inside one ring is
/// a hole, inside two it is an outline again): the paths of the sheets promise only that a hole runs
/// the other way, and some shapes (the poche of a section) may not even do that. The closing point
/// that repeats the first one is dropped.
fn fill_groups(subs: &[Vec<DVec2>]) -> Vec<Vec<Vec<DVec2>>> {
    let rings: Vec<Vec<DVec2>> = subs
        .iter()
        .map(|s| {
            let mut r = s.clone();
            if r.len() > 1 && r.first() == r.last() {
                r.pop();
            }
            r
        })
        .filter(|r| r.len() >= 3)
        .collect();
    // The rings that hold each ring.
    let holders: Vec<Vec<usize>> = (0..rings.len())
        .map(|i| {
            (0..rings.len())
                .filter(|&j| j != i && inside_ring(&rings[j], rings[i][0]))
                .collect()
        })
        .collect();
    let mut groups: Vec<Vec<Vec<DVec2>>> = Vec::new();
    let mut group_of: Vec<Option<usize>> = vec![None; rings.len()];
    for i in 0..rings.len() {
        if holders[i].len().is_multiple_of(2) {
            group_of[i] = Some(groups.len());
            groups.push(vec![rings[i].clone()]);
        }
    }
    for i in 0..rings.len() {
        if !holders[i].len().is_multiple_of(2) {
            // The hole of the smallest ring that holds it and is an outline.
            let parent = holders[i]
                .iter()
                .copied()
                .filter(|&j| holders[j].len().is_multiple_of(2))
                .min_by(|&a, &b| ring_area(&rings[a]).total_cmp(&ring_area(&rings[b])));
            if let Some(g) = parent.and_then(|j| group_of[j]) {
                groups[g].push(rings[i].clone());
            }
        }
    }
    groups
}

impl SheetUi {
    /// Draws a page: the paper with a shadow, then everything on it. A polygon crop is drawn
    /// as its bounding box here. The PDF, SVG and PNG have the exact shape.
    pub fn paint(&self, painter: &Painter, view: &View, page: &Page) {
        let sheet = Rect::from_two_pos(view.to_screen(DVec2::ZERO), view.to_screen(page.size));
        painter.rect_filled(
            sheet.translate(egui::vec2(4.0, 4.0)),
            0.0,
            Color32::from_black_alpha(50),
        );
        painter.rect_filled(sheet, 0.0, Color32::WHITE);
        let mut stack: Vec<Painter> = Vec::new();
        for prim in &page.prims {
            let p = stack.last().unwrap_or(painter);
            match prim {
                Prim::PushClip(poly) => {
                    let (lo, hi) = poly.iter().fold(
                        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                        |(a, b), q| (a.min(*q), b.max(*q)),
                    );
                    let r = Rect::from_two_pos(view.to_screen(lo), view.to_screen(hi));
                    stack.push(p.with_clip_rect(p.clip_rect().intersect(r)));
                }
                Prim::PopClip => {
                    stack.pop();
                }
                Prim::Path(path) => {
                    let subs = flatten(&path.segs);
                    if let Some(f) = path.fill
                        && f[3] > 0
                    {
                        for group in fill_groups(&subs) {
                            let (pts, tris) = tess::triangulate_with_holes(&group);
                            let mut mesh = egui::epaint::Mesh::default();
                            for q in &pts {
                                mesh.colored_vertex(view.to_screen(*q), col(f));
                            }
                            for t in tris {
                                mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
                            }
                            p.add(mesh);
                        }
                    }
                    if let Some(s) = path.stroke {
                        let w = ((path.width * view.zoom) as f32).max(0.8);
                        for sub in &subs {
                            let mut sp: Vec<Pos2> =
                                sub.iter().map(|q| view.to_screen(*q)).collect();
                            if sp.len() < 2 {
                                continue;
                            }
                            // A ring that returns to its start is closed.
                            let mut closed = path.closed;
                            if sub.len() > 2 && sub.first() == sub.last() {
                                sp.pop();
                                closed = true;
                            }
                            match path.dash {
                                Some([d, g]) => {
                                    let mut pts = sp.clone();
                                    if closed {
                                        pts.push(sp[0]);
                                    }
                                    p.extend(egui::Shape::dashed_line(
                                        &pts,
                                        Stroke::new(w, col(s)),
                                        (d * view.zoom) as f32,
                                        (g * view.zoom) as f32,
                                    ));
                                }
                                None => {
                                    p.add(egui::epaint::PathShape {
                                        points: sp,
                                        closed,
                                        fill: Color32::TRANSPARENT,
                                        stroke: PathStroke::new(w, col(s)),
                                    });
                                }
                            }
                        }
                    }
                }
                Prim::Text(t) => {
                    let size = (t.height * view.zoom) as f32;
                    if size >= 3.0 {
                        let align = if t.align == kadakas_print::display::Align::Center {
                            Align2::CENTER_BOTTOM
                        } else {
                            Align2::LEFT_BOTTOM
                        };
                        p.text(
                            view.to_screen(t.at),
                            align,
                            &t.text,
                            FontId::proportional(size.min(400.0)),
                            col(t.color),
                        );
                    }
                }
                Prim::Image(img) => {
                    let key = std::sync::Arc::as_ptr(&img.rgb) as usize;
                    let mut textures = self.textures.borrow_mut();
                    if textures.len() > 24 && !textures.contains_key(&key) {
                        textures.clear();
                    }
                    let tex = textures.entry(key).or_insert_with(|| {
                        let image = egui::ColorImage::from_rgb(
                            [img.width as usize, img.height as usize],
                            &img.rgb,
                        );
                        painter
                            .ctx()
                            .load_texture("viewport", image, egui::TextureOptions::LINEAR)
                    });
                    let r = Rect::from_two_pos(view.to_screen(img.min), view.to_screen(img.max));
                    p.image(
                        tex.id(),
                        r,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            }
        }
    }
}

const PAPERS: [(&str, PaperSize); 11] = [
    ("A0", PaperSize::A0),
    ("A1", PaperSize::A1),
    ("A2", PaperSize::A2),
    ("A3", PaperSize::A3),
    ("A4", PaperSize::A4),
    ("Letter", PaperSize::Letter),
    ("Legal", PaperSize::Legal),
    ("Tabloid", PaperSize::Tabloid),
    ("Arch C", PaperSize::ArchC),
    ("Arch D", PaperSize::ArchD),
    ("Arch E", PaperSize::ArchE),
];

/// The Sheet Layers and Saved Views sections of the Navigation palette.
pub fn navigation_sections(
    ui: &mut Ui,
    session: &mut Session,
    sui: &mut SheetUi,
    status: &mut String,
) {
    let mut cmds: Vec<Command> = Vec::new();
    let doc = session.editor.doc();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong(tr!("sheets-ui-sheet-layers"));
        if ui
            .small_button("+")
            .on_hover_text(tr!("sheets-ui-add-sheet-layer"))
            .clicked()
        {
            let n = doc.sheet_layers().count() + 1;
            cmds.push(Command::AddSheet {
                name: format!("Sheet {n}"),
                paper: None,
                orientation: None,
                number: None,
                title: None,
            });
        }
    });
    for l in doc.sheet_layers() {
        let sheet = doc.sheet(l.id).expect("sheet layer");
        ui.horizontal(|ui| {
            let mut vis = l.visible;
            if ui
                .checkbox(&mut vis, "")
                .on_hover_text(tr!("sheets-ui-visible"))
                .changed()
            {
                cmds.push(Command::ModifyLayer {
                    id: l.id,
                    name: None,
                    visible: Some(vis),
                    locked: None,
                    elevation: None,
                    scale: None,
                });
            }
            if let Some((_, buf)) = sui.renaming.as_mut().filter(|r| r.0 == l.id) {
                let r = ui.text_edit_singleline(buf);
                r.request_focus();
                if r.lost_focus() {
                    if !buf.is_empty() {
                        cmds.push(Command::ModifyLayer {
                            id: l.id,
                            name: Some(buf.clone()),
                            visible: None,
                            locked: None,
                            elevation: None,
                            scale: None,
                        });
                    }
                    sui.renaming = None;
                }
            } else {
                let label = if sheet.number.is_empty() {
                    l.name.clone()
                } else {
                    format!("{}  {}", sheet.number, l.name)
                };
                let r = ui.selectable_label(l.id == doc.active_layer, label);
                if r.clicked() {
                    cmds.push(Command::SetActive {
                        layer: Some(l.id),
                        class: None,
                    });
                }
                if r.double_clicked() {
                    sui.renaming = Some((l.id, l.name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button(tr!("sheets-ui-rename")).clicked() {
                        sui.renaming = Some((l.id, l.name.clone()));
                        ui.close();
                    }
                    if ui.button(tr!("sheets-ui-delete-if-empty")).clicked() {
                        cmds.push(Command::DeleteLayer { id: l.id });
                        ui.close();
                    }
                });
            }
        });
        if l.id == doc.active_layer {
            // Page settings of the active sheet layer.
            let (mut number, mut title, mut paper, mut orientation) = (
                sheet.number.clone(),
                sheet.title.clone(),
                sheet.paper,
                sheet.orientation,
            );
            egui::Grid::new("sheet_page").num_columns(2).show(ui, |ui| {
                ui.label(tr!("sheets-ui-number"));
                let n = ui.text_edit_singleline(&mut number);
                ui.end_row();
                ui.label(tr!("sheets-ui-title"));
                let t = ui.text_edit_singleline(&mut title);
                ui.end_row();
                ui.label(tr!("sheets-ui-paper"));
                egui::ComboBox::from_id_salt("sheet_paper")
                    .selected_text(
                        PAPERS
                            .iter()
                            .find(|(_, p)| *p == paper)
                            .map_or(tr!("sheets-ui-custom"), |(n, _)| (*n).to_string()),
                    )
                    .show_ui(ui, |ui| {
                        for (name, p) in PAPERS {
                            ui.selectable_value(&mut paper, p, name);
                        }
                    });
                ui.end_row();
                ui.label(tr!("sheets-ui-orientation"));
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut orientation,
                        Orientation::Landscape,
                        tr!("sheets-ui-landscape"),
                    );
                    ui.selectable_value(
                        &mut orientation,
                        Orientation::Portrait,
                        tr!("sheets-ui-portrait"),
                    );
                });
                ui.end_row();
                if n.lost_focus() && number != sheet.number {
                    cmds.push(Command::SetSheet {
                        layer: l.id,
                        paper: None,
                        orientation: None,
                        number: Some(number.clone()),
                        title: None,
                    });
                }
                if t.lost_focus() && title != sheet.title {
                    cmds.push(Command::SetSheet {
                        layer: l.id,
                        paper: None,
                        orientation: None,
                        number: None,
                        title: Some(title.clone()),
                    });
                }
            });
            if paper != sheet.paper || orientation != sheet.orientation {
                cmds.push(Command::SetSheet {
                    layer: l.id,
                    paper: Some(paper),
                    orientation: Some(orientation),
                    number: None,
                    title: None,
                });
            }
        }
    }

    ui.add_space(8.0);
    ui.strong(tr!("sheets-ui-saved-views"));
    for v in &doc.drawing.saved_views {
        let r = ui
            .selectable_label(false, &v.name)
            .on_hover_text(tr!("sheets-ui-click-to-restore"));
        if r.clicked() {
            cmds.push(Command::RestoreView {
                name: v.name.clone(),
            });
        }
        r.context_menu(|ui| {
            if ui
                .button(tr!("sheets-ui-update-from-current-view"))
                .clicked()
            {
                sui.save_request = Some(v.name.clone());
                ui.close();
            }
            if ui.button(tr!("sheets-ui-delete")).clicked() {
                cmds.push(Command::DeleteView {
                    name: v.name.clone(),
                });
                ui.close();
            }
        });
    }
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut sui.new_view)
                .hint_text(tr!("sheets-ui-view-name"))
                .desired_width(120.0),
        );
        let add = ui.button(tr!("sheets-ui-save-view")).clicked()
            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if add && !sui.new_view.trim().is_empty() {
            sui.save_request = Some(sui.new_view.trim().to_string());
            sui.new_view.clear();
        }
    });

    for c in cmds {
        if let Command::RestoreView { name } = &c {
            if run_command(session, &c, status).is_some() {
                let view = session.editor.doc().drawing.saved_view(name).cloned();
                if let Some(v) = view {
                    session.apply_saved_view(&v);
                }
            }
        } else {
            run_command(session, &c, status);
        }
    }
}

/// Builds the saved view for the current state, with the zoom and pan of the 2D view.
pub fn capture(session: &Session, name: &str, zoom: f64, pan: DVec2) -> SavedView {
    let doc = session.editor.doc();
    SavedView {
        name: name.to_string(),
        layers: doc.layers.iter().map(|l| (l.id, l.visible)).collect(),
        classes: doc.classes.iter().map(|c| (c.id, c.visible)).collect(),
        active_layer: Some(doc.active_layer),
        active_class: Some(doc.active_class),
        zoom: Some(zoom),
        pan: Some(pan),
        view: Some(session.view.view.id().to_string()),
        projection: Some(session.view.projection.id().to_string()),
        mode: Some(session.view.mode.id().to_string()),
        // A scene of the modeler keeps its free camera when the app stores the view again under
        // the same name (MOD-10). The app has no such camera of its own.
        // It is dropped when the view now differs from that camera.
        camera: doc
            .drawing
            .saved_view(name)
            .and_then(|v| v.camera.clone())
            .filter(|c| {
                let near = kadakas_ui_core::scene_panel::nearest_standard(
                    &kadakas_ui_core::scene_panel::camera_from_saved(c),
                );
                near.id() == session.view.view.id()
            }),
        extra: doc
            .drawing
            .saved_view(name)
            .map(|v| v.extra.clone())
            .unwrap_or_default(),
        render_style: doc
            .drawing
            .presentation
            .active_style
            .and_then(|id| doc.resources.get(id))
            .map(|r| r.name.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_keeps_the_camera_of_a_scene_and_stores_the_active_style() {
        use kadakas_core::sheets::ViewCamera;
        let mut session = Session::with_sequential_ids();
        let cam = ViewCamera {
            target: [1.0, 2.0, 3.0],
            yaw: 0.5,
            pitch: 0.25,
            distance: 5000.0,
            fov: 35.0,
            projection: "perspective".into(),
        };
        session
            .editor
            .execute(&Command::SaveView {
                view: Box::new(SavedView {
                    name: "Scene 1".into(),
                    camera: Some(cam.clone()),
                    ..SavedView::default()
                }),
            })
            .unwrap();
        session
            .editor
            .execute(&Command::UseRenderStyle {
                name: "Realistic".into(),
            })
            .unwrap();
        // The app stores the view again under the name of a scene of the modeler.
        let near = kadakas_ui_core::scene_panel::nearest_standard(
            &kadakas_ui_core::scene_panel::camera_from_saved(&cam),
        );
        session.view.view = near;
        let v = capture(&session, "scene 1", 0.5, DVec2::ZERO);
        assert_eq!(v.camera, Some(cam));
        assert_eq!(v.render_style.as_deref(), Some("Realistic"));
        assert!(
            capture(&session, "other", 0.5, DVec2::ZERO)
                .camera
                .is_none()
        );
    }

    fn square(x0: f64, y0: f64, x1: f64, y1: f64, ccw: bool) -> Vec<DVec2> {
        let mut r = vec![
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ];
        if !ccw {
            r.reverse();
        }
        r
    }

    #[test]
    fn a_hole_is_a_ring_inside_a_ring_whatever_the_direction() {
        // A clockwise outline with a counterclockwise hole.
        let g = fill_groups(&[
            square(0.0, 0.0, 10.0, 10.0, false),
            square(3.0, 3.0, 6.0, 6.0, true),
        ]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].len(), 2);
        // Both the same direction: still a hole.
        let g = fill_groups(&[
            square(0.0, 0.0, 10.0, 10.0, true),
            square(3.0, 3.0, 6.0, 6.0, true),
        ]);
        assert_eq!((g.len(), g[0].len()), (1, 2));
        // An island in the hole is an outline again; two separate shapes are two groups; the closing
        // point that repeats the first is dropped.
        let mut closed = square(20.0, 0.0, 30.0, 10.0, true);
        closed.push(closed[0]);
        let g = fill_groups(&[
            square(0.0, 0.0, 10.0, 10.0, true),
            square(2.0, 2.0, 8.0, 8.0, false),
            square(4.0, 4.0, 5.0, 5.0, true),
            closed,
        ]);
        assert_eq!(g.len(), 3);
        let sizes: Vec<usize> = g.iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![2, 1, 1]);
        assert_eq!(g[2][0].len(), 4);
        // Rings that are too small are dropped.
        assert!(fill_groups(&[vec![DVec2::ZERO, DVec2::X]]).is_empty());
    }
}
