//! The settings and the command of the Grading tool (modes landscape wall, grade, spot elevation,
//! stake and section line). The math is in `kadakas_site::grading`. The tool makes the same
//! objects as the API methods.

use std::collections::BTreeMap;

use glam::DVec2;
use serde_json::Value;

use kadakas_core::command::Command;
use kadakas_core::curve;
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_site::grading::{self, grade, section, spot, wall};

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

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::Editor;

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
