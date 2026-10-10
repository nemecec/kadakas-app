//! Grading in the app: the Grading tool (modes landscape wall, grade, spot elevation, stake and
//! section line), the plan drawing of these objects and of the slope and elevation analysis.
//! The math is in `cadektor_site::grading`. The tool makes the same objects as the API methods.

use std::collections::BTreeMap;

use egui::epaint::{Mesh, PathShape, PathStroke, Vertex as EguiVertex};
use std::sync::Arc;

use egui::{Color32, FontId, Painter, Pos2, Stroke, Ui};
use glam::DVec2;
use serde_json::Value;

use cadektor_core::command::Command;
use cadektor_core::curve;
use cadektor_core::model::{Attributes, Document, Object, ObjectId, Shape};
use cadektor_site::SiteCache;
use cadektor_site::grading::{self, analysis, grade, section, spot, wall};

use crate::view::View;

/// A macro keeps `serde` out of the dependencies of the app.
macro_rules! to_json {
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
    let id = ObjectId(doc.next_ids(1)[0]);
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
                to_json!(&d),
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
                to_json!(&d),
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
                to_json!(&d),
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
                to_json!(&d),
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

/// What the analysis mesh was made for, so a view that does not change does not make it again.
struct MeshKey {
    analysis: usize,
    view: View,
    clip: egui::Rect,
}

thread_local! {
    static ANALYSIS_MESH: std::cell::RefCell<Option<(MeshKey, Arc<Mesh>)>> = const { std::cell::RefCell::new(None) };
}

/// Paints the colors of the slope or elevation analysis of a site model, under its contours.
/// Only the triangles in view are made, and the mesh stays while the view and the analysis stay.
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
    let Some(a) = analysis::for_site(cache, &site, &spec) else {
        return;
    };
    let key = MeshKey {
        analysis: Arc::as_ptr(&a) as usize,
        view: *view,
        clip: painter.clip_rect(),
    };
    let cached = ANALYSIS_MESH.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(k, _)| k.analysis == key.analysis && k.view == key.view && k.clip == key.clip)
            .map(|(_, m)| m.clone())
    });
    let mesh = cached.unwrap_or_else(|| {
        let mesh = Arc::new(analysis_mesh(view, painter.clip_rect(), &site, &a));
        ANALYSIS_MESH.with(|c| *c.borrow_mut() = Some((key, mesh.clone())));
        mesh
    });
    painter.add(egui::Shape::mesh(mesh));
}

/// The triangles of the analysis that touch the clip rectangle, in band colors.
fn analysis_mesh(
    view: &View,
    clip: egui::Rect,
    site: &cadektor_site::SiteResult,
    a: &analysis::AnalysisResult,
) -> Mesh {
    let tin = site.surface(a.surface);
    let mut mesh = Mesh::default();
    let colors: Vec<Color32> = a
        .bands
        .iter()
        .map(|b| color32(b.color, a.opacity))
        .collect();
    for (t, band) in tin.triangles.iter().zip(&a.tri_band) {
        let p = t.map(|i| view.to_screen(tin.vertices[i as usize].truncate()));
        let rect = egui::Rect::from_min_max(p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        if !rect.intersects(clip) {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        for pos in p {
            mesh.vertices.push(EguiVertex {
                pos,
                uv: egui::epaint::WHITE_UV,
                color: colors[*band as usize],
            });
        }
        mesh.indices.extend([base, base + 1, base + 2]);
    }
    mesh
}

/// Paints what a `Drawn` holds: the shapes of the grading objects, as the SVG and the sheet show them.
pub fn paint_drawn(painter: &Painter, view: &View, d: &grading::draw::Drawn) {
    for poly in &d.polys {
        let rings: Vec<Vec<Pos2>> = poly
            .rings
            .iter()
            .map(|r| r.iter().map(|p| view.to_screen(*p)).collect())
            .collect();
        if let (Some(fill), true, 1) = (poly.fill, poly.closed, rings.len()) {
            let mut mesh = Mesh::default();
            let color = color32(fill, 1.0);
            for p in &rings[0] {
                mesh.vertices.push(EguiVertex {
                    pos: *p,
                    uv: egui::epaint::WHITE_UV,
                    color,
                });
            }
            for t in crate::tess::triangulate(&poly.rings[0]) {
                mesh.indices.extend(t.map(|i| i as u32));
            }
            painter.add(mesh);
        }
        let stroke = match (poly.stroke, poly.fill) {
            (Some(c), _) => Some(Stroke::new(
                ((poly.weight * view.zoom) as f32).max(1.0),
                color32(c, 1.0),
            )),
            // A filled shape without a pen still shows its edge, thin, when it has holes.
            (None, Some(_)) if rings.len() > 1 => Some(Stroke::new(1.0, Color32::from_gray(80))),
            _ => None,
        };
        if let Some(stroke) = stroke {
            for r in rings {
                painter.add(PathShape {
                    points: r,
                    closed: poly.closed,
                    fill: Color32::TRANSPARENT,
                    stroke: PathStroke::new(stroke.width, stroke.color),
                });
            }
        }
    }
    for l in &d.labels {
        let px = ((l.height * view.zoom) as f32).clamp(8.0, 30.0);
        let galley =
            painter.layout_no_wrap(l.text.clone(), FontId::proportional(px), Color32::BLACK);
        let at = view.to_screen(l.at);
        let mut pos = at - egui::vec2(if l.center { galley.size().x / 2.0 } else { 0.0 }, px);
        if l.angle_deg != 0.0 {
            // The text turns around its start; the model angle counts counterclockwise.
            let (sin, cos) = (-l.angle_deg.to_radians()).sin_cos();
            let (sin, cos) = (sin as f32, cos as f32);
            let off = at - pos;
            let turned = egui::vec2(off.x * cos - off.y * sin, off.x * sin + off.y * cos);
            pos = at - turned;
            painter.add(
                egui::epaint::TextShape::new(pos, galley, Color32::BLACK)
                    .with_angle((-l.angle_deg).to_radians() as f32),
            );
        } else {
            painter.galley(pos, galley, Color32::BLACK);
        }
    }
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
    paint_drawn(
        painter,
        view,
        &grading::hook::extras(doc, cache, revision, obj),
    );
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

    fn doc_with_everything() -> (cadektor_core::model::Document, ObjectId) {
        use cadektor_core::command::Command;
        let mut ed = Editor::default();
        let pts: Vec<_> = (0..6)
            .flat_map(|i| {
                (0..6).map(move |j| {
                    glam::DVec3::new(i as f64 * 1000.0, j as f64 * 1000.0, i as f64 * 100.0)
                })
            })
            .collect();
        let out = ed
            .execute(&Command::AddObject {
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
        let site = ObjectId(out.created[0]);
        let spec = cadektor_site::grading::analysis::AnalysisSpec {
            mode: analysis::Mode::Slope,
            ..Default::default()
        };
        let set = |ed: &mut Editor, id: ObjectId, key: &str, v: serde_json::Value| {
            ed.execute(&Command::SetData {
                id,
                set: [(key.to_string(), v)].into(),
                remove: vec![],
            })
            .unwrap();
        };
        set(&mut ed, site, analysis::KEY, to_json!(&spec));
        let p = |x: f64, y: f64| DVec2::new(x, y);
        for (mode, pts) in [
            (
                GradingMode::Wall,
                vec![p(500.0, 500.0), p(4000.0, 500.0), p(4000.0, 2500.0)],
            ),
            (
                GradingMode::Grade,
                vec![p(500.0, 1500.0), p(4500.0, 1500.0)],
            ),
            (GradingMode::Spot, vec![p(2000.0, 3000.0)]),
            (GradingMode::Stake, vec![p(2500.0, 3000.0)]),
            (
                GradingMode::Section,
                vec![p(0.0, 4000.0), p(5000.0, 4000.0)],
            ),
        ] {
            let cmd = command(ed.doc(), &at(mode), &pts).unwrap();
            ed.execute(&cmd).unwrap();
        }
        let legend = Shape::Rect {
            min: p(6000.0, 0.0),
            max: p(9000.0, 4000.0),
        };
        let out = ed
            .execute(&Command::AddObject {
                shape: legend,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        set(
            &mut ed,
            ObjectId(out.created[0]),
            analysis::LEGEND_KEY,
            serde_json::json!({"show_areas": true}),
        );
        (ed.doc().clone(), site)
    }

    #[test]
    fn the_canvas_paints_the_grading_graphics_and_keeps_the_analysis_mesh() {
        let (doc, site) = doc_with_everything();
        let cache = SiteCache::default();
        let ctx = egui::Context::default();
        let view = View {
            origin: DVec2::new(40.0, 400.0),
            zoom: 0.1,
        };
        let paint = |view: &View| {
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let painter = ui.painter().clone();
                for o in doc.objects.values() {
                    if matches!(o.shape, Shape::SiteModel { .. }) {
                        draw_analysis(&painter, view, &doc, o, &cache, 1);
                    }
                    if cadektor_core::site::carries_extra(o) {
                        draw_extras(&painter, view, &doc, o, &cache, 1);
                    }
                }
            });
            out.shapes.len()
        };
        let first = paint(&view);
        assert!(first > 10, "{first} shapes");
        let key_of = || {
            ANALYSIS_MESH.with(|c| {
                c.borrow()
                    .as_ref()
                    .map(|(k, m)| (k.analysis, Arc::as_ptr(m) as usize))
            })
        };
        let k1 = key_of().unwrap();
        // The same view gives the same mesh. Another view makes a new one.
        paint(&view);
        assert_eq!(key_of().unwrap(), k1);
        paint(&View { zoom: 0.2, ..view });
        assert_ne!(key_of().unwrap().1, k1.1);
        // Only the triangles in view are made: a view far from the site has none.
        let tin_site = cache.get(1, &doc, site).unwrap();
        let spec = analysis::spec_of(doc.object(site).unwrap()).unwrap();
        let a = analysis::for_site(&cache, &tin_site, &spec).unwrap();
        let far = analysis_mesh(
            &View {
                origin: DVec2::new(1.0e6, 1.0e6),
                zoom: 0.1,
            },
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0)),
            &tin_site,
            &a,
        );
        assert!(far.indices.is_empty());
        let near = analysis_mesh(
            &view,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0)),
            &tin_site,
            &a,
        );
        assert_eq!(near.indices.len(), a.tri_band.len() * 3);
        assert!(color32([10, 20, 30, 200], 0.5).a() == 100);
    }

    #[test]
    fn labels_and_polygons_paint_with_holes_angles_and_centers() {
        use cadektor_site::grading::draw::{Drawn, Label, Poly};
        let ctx = egui::Context::default();
        let view = View {
            origin: DVec2::new(10.0, 300.0),
            zoom: 0.1,
        };
        let ring = |o: f64| {
            vec![
                DVec2::new(o, o),
                DVec2::new(1000.0 - o, o),
                DVec2::new(1000.0 - o, 1000.0 - o),
                DVec2::new(o, 1000.0 - o),
            ]
        };
        let mut d = Drawn::default();
        d.polys.push(
            Poly::new("a", ObjectId::from_num(1), vec![ring(0.0)], true).filled([200, 0, 0, 255]),
        );
        d.polys.push(
            Poly::new(
                "b",
                ObjectId::from_num(1),
                vec![ring(0.0), ring(200.0)],
                true,
            )
            .filled([0, 200, 0, 255]),
        );
        d.polys.push(
            Poly::new("c", ObjectId::from_num(1), vec![ring(100.0)], false)
                .stroked([0, 0, 0, 255], 5.0),
        );
        let mut turned = Label::new(
            "t",
            ObjectId::from_num(1),
            DVec2::new(500.0, 500.0),
            "up".into(),
            100.0,
        );
        turned.angle_deg = 30.0;
        turned.center = true;
        d.labels.push(turned);
        d.labels.push(Label::new(
            "t",
            ObjectId::from_num(1),
            DVec2::new(500.0, 100.0),
            "flat".into(),
            100.0,
        ));
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_drawn(ui.painter(), &view, &d)
        });
        // A filled ring, a ring with a hole (two outlines), a stroked line and two labels.
        assert_eq!(out.shapes.len(), 6);
    }
}
