//! Grading in the app: the Grading tool (modes landscape wall, grade, spot elevation, stake and
//! section line), the plan drawing of these objects and of the slope and elevation analysis.
//! The math is in `cadektor_site::grading`. The tool makes the same objects as the API methods.

use std::collections::BTreeMap;

use egui::epaint::{Mesh, PathShape, PathStroke, Vertex as EguiVertex};
use egui::{Align2, Color32, FontId, Painter, Pos2, Stroke, Ui};
use glam::DVec2;
use serde_json::Value;

use cadektor_core::command::Command;
use cadektor_core::curve;
use cadektor_core::model::{Attributes, Document, Object, ObjectId, Shape};
use cadektor_site::SiteCache;
use cadektor_site::grading::{self, analysis, grade, section, spot, wall};

use crate::view::View;

/// A macro keeps `serde` out of the dependencies of the app.
macro_rules! json {
    ($v:expr) => {
        serde_json::to_value($v).expect("settings are JSON")
    };
}

/// What the Grading tool makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradingMode {
    /// A landscape wall along a path.
    #[default]
    Wall,
    /// A grade object between two points.
    Grade,
    /// A spot elevation that reads the site model.
    Spot,
    /// A stake for setting out, with northing and easting.
    Stake,
    /// A section line.
    Section,
}

impl GradingMode {
    pub const ALL: [GradingMode; 5] = [
        GradingMode::Wall,
        GradingMode::Grade,
        GradingMode::Spot,
        GradingMode::Stake,
        GradingMode::Section,
    ];

    pub fn name(self) -> &'static str {
        match self {
            GradingMode::Wall => "Landscape Wall",
            GradingMode::Grade => "Grade",
            GradingMode::Spot => "Spot Elevation",
            GradingMode::Stake => "Stake",
            GradingMode::Section => "Section Line",
        }
    }

    /// How many clicks make the object. `None`: any number, Enter finishes.
    pub fn clicks(self) -> Option<usize> {
        match self {
            GradingMode::Wall => None,
            GradingMode::Grade | GradingMode::Section => Some(2),
            GradingMode::Spot | GradingMode::Stake => Some(1),
        }
    }
}

/// The settings of the Grading tool.
#[derive(Clone, Debug, PartialEq)]
pub struct GradingSettings {
    pub mode: GradingMode,
    /// Thickness of new walls in mm.
    pub wall_thickness: f64,
    /// Height of the top of new walls above the ground in mm.
    pub wall_height: f64,
    /// Start elevation of new grades without a site model, in mm.
    pub grade_elevation: f64,
    /// Slope of new grades without a site model, in percent.
    pub grade_percent: f64,
    pub stake_name: String,
    pub section_name: String,
}

impl Default for GradingSettings {
    fn default() -> Self {
        GradingSettings {
            mode: GradingMode::Wall,
            wall_thickness: wall::DEFAULT_THICKNESS,
            wall_height: 900.0,
            grade_elevation: 0.0,
            grade_percent: 2.0,
            stake_name: "S1".into(),
            section_name: "A".into(),
        }
    }
}

fn has_site(doc: &Document) -> bool {
    let layer = doc.active_layer;
    doc.objects
        .values()
        .any(|o| o.layer == layer && matches!(o.shape, Shape::SiteModel { .. }))
}

/// The command that adds an object and its settings: one undo step.
fn add_with(doc: &Document, shape: Shape, key: &str, data: Value) -> Command {
    let id = ObjectId(doc.next_id());
    Command::Batch {
        commands: vec![
            Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            },
            Command::SetData {
                id,
                set: BTreeMap::from([(key.to_string(), data)]),
                remove: vec![],
            },
        ],
    }
}

/// The command that the Grading tool runs for the clicked points, or `None` when they do not make an object.
pub fn command(doc: &Document, s: &GradingSettings, pts: &[DVec2]) -> Option<Command> {
    match s.mode {
        GradingMode::Wall if pts.len() >= 2 => {
            let d = wall::WallData {
                thickness: s.wall_thickness,
                top: wall::Top::Terrain {
                    offset: s.wall_height,
                },
                ..Default::default()
            };
            d.validate().ok()?;
            Some(add_with(
                doc,
                Shape::Polyline {
                    points: curve::corners(pts),
                    closed: false,
                },
                wall::KEY,
                json!(&d),
            ))
        }
        GradingMode::Grade if pts.len() == 2 && pts[0] != pts[1] => {
            let d = if has_site(doc) {
                grade::GradeData {
                    snap: Some(grading::Follow::Proposed),
                    ..Default::default()
                }
            } else {
                grade::GradeData {
                    elevation_a: s.grade_elevation,
                    slope: Some(grade::SlopeSpec::Percent(s.grade_percent)),
                    ..Default::default()
                }
            };
            d.validate().ok()?;
            Some(add_with(
                doc,
                Shape::Line {
                    a: pts[0],
                    b: pts[1],
                },
                grade::KEY,
                json!(&d),
            ))
        }
        GradingMode::Spot | GradingMode::Stake if pts.len() == 1 => {
            let kind = if s.mode == GradingMode::Stake {
                spot::Kind::Stake
            } else {
                spot::Kind::Spot
            };
            let name = if kind == spot::Kind::Stake {
                s.stake_name.clone()
            } else {
                String::new()
            };
            let d = spot::SpotData {
                kind,
                name,
                ..Default::default()
            };
            let scale = doc.layer(doc.active_layer).map_or(100.0, |l| l.scale);
            Some(add_with(
                doc,
                Shape::Circle {
                    center: pts[0],
                    radius: spot::MARKER_PAPER_RADIUS * scale,
                },
                spot::KEY,
                json!(&d),
            ))
        }
        GradingMode::Section if pts.len() == 2 && pts[0] != pts[1] => {
            let d = section::SectionData {
                name: s.section_name.clone(),
                ..Default::default()
            };
            Some(add_with(
                doc,
                Shape::Line {
                    a: pts[0],
                    b: pts[1],
                },
                section::KEY,
                json!(&d),
            ))
        }
        _ => None,
    }
}

/// The mode row and the fields of the Grading tool (Data bar).
pub fn options(ui: &mut Ui, s: &mut GradingSettings) {
    for mode in GradingMode::ALL {
        ui.selectable_value(&mut s.mode, mode, mode.name());
    }
    match s.mode {
        GradingMode::Wall => {
            ui.label("Thickness");
            ui.add(
                egui::DragValue::new(&mut s.wall_thickness)
                    .speed(10.0)
                    .range(10.0..=5000.0)
                    .suffix(" mm"),
            );
            ui.label("Height above ground");
            ui.add(
                egui::DragValue::new(&mut s.wall_height)
                    .speed(10.0)
                    .range(-5000.0..=20000.0)
                    .suffix(" mm"),
            );
        }
        GradingMode::Grade => {
            ui.label("Without a site model:");
            ui.label("Start elevation");
            ui.add(
                egui::DragValue::new(&mut s.grade_elevation)
                    .speed(10.0)
                    .range(-1.0e8..=1.0e8)
                    .suffix(" mm"),
            );
            ui.label("Slope");
            ui.add(
                egui::DragValue::new(&mut s.grade_percent)
                    .speed(0.1)
                    .range(-100.0..=100.0)
                    .suffix(" %"),
            );
        }
        GradingMode::Stake => {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut s.stake_name).desired_width(60.0));
        }
        GradingMode::Section => {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut s.section_name).desired_width(60.0));
        }
        GradingMode::Spot => {}
    }
}

fn color32(c: [u8; 4], opacity: f64) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (c[3] as f64 * opacity).round() as u8)
}

/// Text size on screen for a height in model units, kept readable.
fn text_px(view: &View, height_model: f64) -> f32 {
    ((height_model * view.zoom) as f32).clamp(8.0, 30.0)
}

fn label(painter: &Painter, at: Pos2, anchor: Align2, text: String, px: f32) {
    painter.text(at, anchor, text, FontId::proportional(px), Color32::BLACK);
}

/// Paints the colors of the slope or elevation analysis of a site model, under its contours.
pub fn draw_analysis(
    painter: &Painter,
    view: &View,
    doc: &Document,
    obj: &Object,
    cache: &SiteCache,
    revision: u64,
) {
    let Some(spec) = analysis::spec_of(obj) else {
        return;
    };
    let Ok(site) = cache.get(revision, doc, obj.id) else {
        return;
    };
    let Some(a) = analysis::for_site(&site, &spec) else {
        return;
    };
    let tin = site.surface(a.surface.surface());
    let mut mesh = Mesh::default();
    for (t, band) in tin.triangles.iter().zip(&a.tri_band) {
        let color = color32(a.bands[*band as usize].color, a.opacity);
        let base = mesh.vertices.len() as u32;
        for i in t {
            let p = tin.vertices[*i as usize];
            mesh.vertices.push(EguiVertex {
                pos: view.to_screen(p.truncate()),
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
        mesh.indices.extend([base, base + 1, base + 2]);
    }
    painter.add(mesh);
}

/// Paints the extras of a wall, grade, spot elevation, section line or legend: what core does not draw.
pub fn draw_extras(
    painter: &Painter,
    view: &View,
    doc: &Document,
    obj: &Object,
    cache: &SiteCache,
    revision: u64,
) {
    let scale = doc.layer(obj.layer).map_or(100.0, |l| l.scale);
    if let Some(d) = wall::data(obj) {
        let site = grading::host(doc, cache, revision, obj, d.site);
        let ground = site.as_ref().map(|s| grading::ground_of(s));
        let Some(w) = wall::evaluate(doc, obj, ground.as_ref().map(|g| g as grading::Ground<'_>))
        else {
            return;
        };
        let pts: Vec<DVec2> = w.stations.iter().map(|s| s.p).collect();
        let fill = color32(w.color, 0.55);
        for ring in wall::footprint(&pts, w.thickness) {
            let screen: Vec<Pos2> = ring.iter().map(|p| view.to_screen(*p)).collect();
            if !w.closed {
                let mut mesh = Mesh::default();
                for p in &screen {
                    mesh.vertices.push(EguiVertex {
                        pos: *p,
                        uv: egui::epaint::WHITE_UV,
                        color: fill,
                    });
                }
                for t in crate::tess::triangulate(&ring) {
                    mesh.indices.extend(t.map(|i| i as u32));
                }
                painter.add(mesh);
            }
            painter.add(PathShape {
                points: screen,
                closed: true,
                fill: Color32::TRANSPARENT,
                stroke: PathStroke::new(1.0, Color32::from_gray(80)),
            });
        }
        let px = text_px(view, 2.5 * scale);
        let ends = [
            w.stations.first(),
            if w.closed { None } else { w.stations.last() },
        ];
        for s in ends.into_iter().flatten() {
            let text = format!(
                "TW {} BW {}",
                grading::meters(s.top, 2),
                grading::meters(s.bottom, 2)
            );
            label(
                painter,
                view.to_screen(s.p) + egui::vec2(0.0, -px),
                Align2::LEFT_BOTTOM,
                text,
                px,
            );
        }
    } else if let Some(d) = grade::data(obj) {
        let site = d
            .snap
            .and_then(|_| grading::host(doc, cache, revision, obj, d.site));
        let ground = site.as_ref().map(|s| grading::ground_of(s));
        let Some((d, g)) = grade::evaluate(obj, ground.as_ref().map(|g| g as grading::Ground<'_>))
        else {
            return;
        };
        let px = text_px(view, 2.5 * scale);
        let (a, b) = (view.to_screen(g.a), view.to_screen(g.b));
        let mid = a.lerp(b, 0.5);
        let dir = (b - a).normalized();
        let n = egui::vec2(-dir.y, dir.x);
        if d.label {
            // The arrow points downhill.
            let down = if g.falls() { dir } else { -dir };
            let size = 9.0;
            let tip = mid + down * size * 0.5;
            let base = mid - down * size * 0.5;
            painter.add(PathShape::convex_polygon(
                vec![tip, base + n * size * 0.4, base - n * size * 0.4],
                Color32::BLACK,
                Stroke::NONE,
            ));
            label(
                painter,
                mid + n * (px * 1.1),
                Align2::CENTER_CENTER,
                g.label(),
                px,
            );
        }
        if d.end_tags {
            label(
                painter,
                a - dir * (px * 0.5) - n * (px * 1.2),
                Align2::CENTER_CENTER,
                grading::meters(g.elevation_a, 2),
                px,
            );
            label(
                painter,
                b + dir * (px * 0.5) - n * (px * 1.2),
                Align2::CENTER_CENTER,
                grading::meters(g.elevation_b, 2),
                px,
            );
        }
    } else if let (Some(d), Shape::Circle { center, radius }) = (spot::data(obj), &obj.shape) {
        let text = match grading::host(doc, cache, revision, obj, d.site) {
            Some(site) => spot::label(
                &d,
                &spot::read_at(&site, *center),
                *center,
                doc.georef.as_ref(),
            ),
            None => "no site model".to_string(),
        };
        let c = view.to_screen(*center);
        let r = (*radius * view.zoom) as f32;
        let stroke = Stroke::new(1.0, Color32::BLACK);
        painter.line_segment([c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0)], stroke);
        painter.line_segment([c - egui::vec2(0.0, r), c + egui::vec2(0.0, r)], stroke);
        let px = text_px(view, d.label_height * scale);
        label(
            painter,
            c + egui::vec2(r * 1.5, -r * 1.2),
            Align2::LEFT_BOTTOM,
            text,
            px,
        );
    } else if let (Some(d), Shape::Line { a, b }) = (section::data(obj), &obj.shape) {
        let (sa, sb) = (view.to_screen(*a), view.to_screen(*b));
        let dir = (sb - sa).normalized();
        let n = egui::vec2(-dir.y, dir.x) * 8.0;
        let px = text_px(view, 3.5 * scale);
        for p in [sa, sb] {
            painter.line_segment([p - n, p + n], Stroke::new(1.5, Color32::BLACK));
            label(
                painter,
                p + n * 1.5,
                Align2::CENTER_CENTER,
                d.name.clone(),
                px,
            );
        }
    } else if let (Some(d), Shape::Rect { min, max }) = (analysis::legend_of(obj), &obj.shape) {
        let site_obj = d.site.and_then(|id| doc.object(id)).or_else(|| {
            doc.objects.values().find(|o| {
                o.layer == obj.layer
                    && analysis::spec_of(o).is_some_and(|s| s.mode != analysis::Mode::Off)
            })
        });
        let Some(a) = site_obj.and_then(|so| {
            analysis::for_site(
                cache.get(revision, doc, so.id).ok()?.as_ref(),
                &analysis::spec_of(so)?,
            )
        }) else {
            return;
        };
        let (lo, hi) = (min.min(*max), min.max(*max));
        let rows = a.bands.len() + 1;
        let row = (hi.y - lo.y) / rows as f64;
        let px = text_px(view, row * 0.6);
        let title = if d.title.is_empty() {
            if a.mode == analysis::Mode::Slope {
                "Slope"
            } else {
                "Elevation"
            }
            .to_string()
        } else {
            d.title.clone()
        };
        label(
            painter,
            view.to_screen(DVec2::new(lo.x, hi.y)),
            Align2::LEFT_TOP,
            title,
            px,
        );
        for (k, band) in a.bands.iter().rev().enumerate() {
            let top = hi.y - row * (k + 1) as f64;
            let r = egui::Rect::from_two_pos(
                view.to_screen(DVec2::new(lo.x, top + row * 0.85)),
                view.to_screen(DVec2::new(lo.x + row * 1.2, top + row * 0.15)),
            );
            painter.rect_filled(r, 0.0, color32(band.color, 1.0));
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(0.5, Color32::BLACK),
                egui::StrokeKind::Middle,
            );
            let mut text = band.label.clone();
            if d.show_areas {
                text.push_str(&format!("  ({:.1} m2)", band.area_xy / 1e6));
            }
            label(
                painter,
                view.to_screen(DVec2::new(lo.x + row * 1.5, top + row * 0.5)),
                Align2::LEFT_CENTER,
                text,
                px,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::command::Editor;

    fn run(ed: &mut Editor, s: &GradingSettings, pts: &[DVec2]) -> Option<ObjectId> {
        let cmd = command(ed.doc(), s, pts)?;
        let out = ed.execute(&cmd).unwrap();
        Some(ObjectId(out.created[0]))
    }

    fn at(mode: GradingMode) -> GradingSettings {
        GradingSettings {
            mode,
            ..Default::default()
        }
    }

    #[test]
    fn the_tool_makes_each_object_in_one_step() {
        let mut ed = Editor::default();
        let p = |x: f64, y: f64| DVec2::new(x, y);
        let w = run(
            &mut ed,
            &at(GradingMode::Wall),
            &[p(0.0, 0.0), p(4000.0, 0.0), p(4000.0, 3000.0)],
        )
        .unwrap();
        assert!(wall::data(ed.doc().object(w).unwrap()).is_some());
        let g = run(
            &mut ed,
            &at(GradingMode::Grade),
            &[p(0.0, 0.0), p(10_000.0, 0.0)],
        )
        .unwrap();
        let (_, solved) = grade::evaluate(ed.doc().object(g).unwrap(), None).unwrap();
        assert_eq!(solved.elevation_b, 200.0);
        let s = run(&mut ed, &at(GradingMode::Spot), &[p(1000.0, 1000.0)]).unwrap();
        assert_eq!(
            spot::data(ed.doc().object(s).unwrap()).unwrap().kind,
            spot::Kind::Spot
        );
        let k = run(&mut ed, &at(GradingMode::Stake), &[p(2000.0, 1000.0)]).unwrap();
        let sd = spot::data(ed.doc().object(k).unwrap()).unwrap();
        assert_eq!((sd.kind, sd.name.as_str()), (spot::Kind::Stake, "S1"));
        let c = run(
            &mut ed,
            &at(GradingMode::Section),
            &[p(0.0, 5000.0), p(9000.0, 5000.0)],
        )
        .unwrap();
        assert!(section::data(ed.doc().object(c).unwrap()).is_some());
        // One undo removes the whole object.
        assert!(ed.undo());
        assert!(ed.doc().object(c).is_none());
        // Too few points, or a zero length, make nothing.
        assert!(command(ed.doc(), &at(GradingMode::Wall), &[p(0.0, 0.0)]).is_none());
        assert!(
            command(
                ed.doc(),
                &at(GradingMode::Grade),
                &[p(0.0, 0.0), p(0.0, 0.0)]
            )
            .is_none()
        );
        assert!(
            command(
                ed.doc(),
                &at(GradingMode::Section),
                &[p(0.0, 0.0), p(0.0, 0.0)]
            )
            .is_none()
        );
        assert!(command(ed.doc(), &at(GradingMode::Spot), &[]).is_none());
        let bad = GradingSettings {
            wall_thickness: 0.0,
            ..at(GradingMode::Wall)
        };
        assert!(command(ed.doc(), &bad, &[p(0.0, 0.0), p(1.0, 0.0)]).is_none());
        assert_eq!(
            GradingMode::ALL.map(|m| m.clicks()),
            [None, Some(2), Some(1), Some(1), Some(2)]
        );
        assert!(GradingMode::ALL.iter().all(|m| !m.name().is_empty()));
    }

    #[test]
    fn a_grade_with_a_site_model_snaps_to_it() {
        let mut ed = Editor::default();
        let pts: Vec<_> = (0..4)
            .flat_map(|i| {
                (0..4).map(move |j| {
                    glam::DVec3::new(i as f64 * 1000.0, j as f64 * 1000.0, i as f64 * 100.0)
                })
            })
            .collect();
        ed.execute(&Command::AddObject {
            shape: Shape::SiteModel {
                points: pts,
                lines: vec![],
                settings: Default::default(),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        let g = run(
            &mut ed,
            &at(GradingMode::Grade),
            &[DVec2::new(0.0, 500.0), DVec2::new(3000.0, 500.0)],
        )
        .unwrap();
        assert_eq!(
            grade::data(ed.doc().object(g).unwrap()).unwrap().snap,
            Some(grading::Follow::Proposed)
        );
    }
}
