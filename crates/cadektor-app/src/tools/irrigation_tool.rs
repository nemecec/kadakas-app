//! The Irrigation tool and its modes: Outlet, Line, POC, Valve and Controller.
//! Outlets, POCs, valves and controllers are placed with one click. A line takes a click for
//! each point, and Enter or a double-click finishes it. Snapping joins line ends to each other
//! and to valves and POCs (the connection comes from the geometry).

use cadektor_core::curve;
use cadektor_core::irrigation::{IrrigationShape, LineType};
use cadektor_core::model::{Attributes, Document, Shape};
use cadektor_irrigation::Catalog;
use glam::DVec2;

use super::{Ctx, Effect, Input, Preview, State, ToolMachine};
use cadektor_core::command::Command;

/// What the Irrigation tool makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IrrMode {
    #[default]
    Outlet,
    Line,
    Poc,
    Valve,
    Controller,
}

impl IrrMode {
    pub const ALL: [IrrMode; 5] = [
        IrrMode::Outlet,
        IrrMode::Line,
        IrrMode::Poc,
        IrrMode::Valve,
        IrrMode::Controller,
    ];

    pub fn label(self) -> &'static str {
        match self {
            IrrMode::Outlet => "Outlet",
            IrrMode::Line => "Line",
            IrrMode::Poc => "POC",
            IrrMode::Valve => "Valve",
            IrrMode::Controller => "Controller",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            IrrMode::Outlet => {
                "Click to place a head. Set the model, arc and direction in the Tool bar."
            }
            IrrMode::Line => {
                "Click the points of a pipe. Snap to line ends, valves and POCs to connect. Enter or double-click finishes."
            }
            IrrMode::Poc => "Click to place the point of connection (the water supply).",
            IrrMode::Valve => "Click to place a control valve on the end of a mainline.",
            IrrMode::Controller => "Click to place an irrigation controller.",
        }
    }

    pub fn next(self) -> IrrMode {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// Settings of the Irrigation tool.
#[derive(Clone, Debug, PartialEq)]
pub struct IrrSettings {
    pub mode: IrrMode,
    pub model: String,
    pub arc: f64,
    pub direction: f64,
    pub line_type: LineType,
    pub size: String,
    pub material: String,
    /// Zone of new valves. Zero picks the next free zone number.
    pub zone: u32,
    pub kv: f64,
    pub static_pressure: f64,
    pub available_flow: f64,
    pub name: String,
    pub stations: u32,
}

impl Default for IrrSettings {
    fn default() -> Self {
        IrrSettings {
            mode: IrrMode::Outlet,
            model: "Spray-4.0".into(),
            arc: 360.0,
            direction: 0.0,
            line_type: LineType::Lateral,
            size: "32".into(),
            material: "PE".into(),
            zone: 0,
            kv: 6.0,
            static_pressure: 400.0,
            available_flow: 100.0,
            name: String::new(),
            stations: 6,
        }
    }
}

/// The lowest zone number above the zones of all valves.
fn next_zone(doc: &Document) -> u32 {
    doc.objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::Irrigation(i) => match i.as_ref() {
                IrrigationShape::Valve { zone, .. } => Some(*zone),
                _ => None,
            },
            _ => None,
        })
        .max()
        .map_or(1, |z| z + 1)
}

/// The object that a click at `at` makes, if the settings allow one.
pub fn shape_at(s: &IrrSettings, doc: &Document, at: DVec2) -> Option<IrrigationShape> {
    let shape = match s.mode {
        IrrMode::Outlet => {
            // The radius comes from the catalog when the outlet is placed.
            let head = Catalog::of_document(doc).head(&s.model).cloned()?;
            IrrigationShape::Outlet {
                at,
                model: head.name,
                arc: s.arc,
                direction: s.direction,
                radius: Some(head.radius),
                zone: None,
            }
        }
        IrrMode::Poc => IrrigationShape::Poc {
            at,
            name: s.name.clone(),
            static_pressure: s.static_pressure,
            available_flow: s.available_flow,
        },
        IrrMode::Valve => IrrigationShape::Valve {
            at,
            zone: if s.zone == 0 { next_zone(doc) } else { s.zone },
            name: s.name.clone(),
            kv: s.kv,
            controller: None,
            station: None,
        },
        IrrMode::Controller => IrrigationShape::Controller {
            at,
            name: if s.name.is_empty() {
                "Controller".into()
            } else {
                s.name.clone()
            },
            stations: s.stations,
        },
        IrrMode::Line => return None,
    };
    shape.problem().is_none().then_some(shape)
}

/// A line through `points` with the settings of the tool.
pub fn line_from(s: &IrrSettings, points: &[DVec2]) -> Option<IrrigationShape> {
    let shape = IrrigationShape::Line {
        points: points.to_vec(),
        line_type: s.line_type,
        size: s.size.clone(),
        material: s.material.clone(),
        emitter_spacing: None,
        emitter_flow: None,
    };
    (points.len() >= 2 && shape.problem().is_none()).then_some(shape)
}

fn add(shape: IrrigationShape) -> Effect {
    Effect::Exec(Command::AddObject {
        shape: Shape::Irrigation(Box::new(shape)),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
}

impl ToolMachine {
    pub(super) fn handle_irrigation(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        let s = &cx.settings.irrigation;
        match (input, s.mode) {
            (Input::Press { p, .. }, IrrMode::Line) => {
                match &mut self.state {
                    State::Points(pts) => {
                        if pts.last() != Some(&p) {
                            pts.push(p);
                        }
                    }
                    state => *state = State::Points(vec![p]),
                }
                vec![]
            }
            (Input::Press { p, .. }, _) => match shape_at(s, cx.doc, p) {
                Some(shape) => vec![add(shape)],
                None => vec![Effect::Notice(
                    "Check the values in the Tool bar: the head model must be in the catalog, pressure and flow above zero".into(),
                )],
            },
            (Input::DoubleClick | Input::Enter, IrrMode::Line) => {
                match std::mem::replace(&mut self.state, State::Idle) {
                    State::Points(pts) => match line_from(s, &pts) {
                        Some(shape) => vec![add(shape)],
                        None => vec![Effect::Notice("A line needs 2 or more points".into())],
                    },
                    _ => vec![],
                }
            }
            _ => vec![],
        }
    }

    pub(super) fn irrigation_preview(&self, cursor: DVec2, cx: &Ctx) -> Preview {
        let s = &cx.settings.irrigation;
        match (&self.state, s.mode) {
            (State::Points(pts), IrrMode::Line) => {
                let mut v = pts.clone();
                v.push(cursor);
                Preview::Path {
                    vertices: curve::corners(&v),
                    closed: false,
                }
            }
            (_, IrrMode::Line) => Preview::None,
            _ => shape_at(s, cx.doc, cursor).map_or(Preview::None, |i| {
                Preview::Shape(Shape::Irrigation(Box::new(i)))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    struct Fx {
        session: cadektor_api::Session,
        settings: ToolSettings,
        m: ToolMachine,
    }

    impl Fx {
        fn new(mode: IrrMode) -> Fx {
            let mut m = ToolMachine::default();
            m.set_tool(Tool::Irrigation);
            let mut settings = ToolSettings::default();
            settings.irrigation.mode = mode;
            Fx {
                session: cadektor_api::Session::default(),
                settings,
                m,
            }
        }

        fn send(&mut self, input: Input) -> Vec<Effect> {
            let fx = {
                let cx = Ctx {
                    doc: self.session.editor.doc(),
                    selection: &self.session.selection,
                    settings: &self.settings,
                    tol: 5.0,
                    context: None,
                };
                self.m.handle(input, &cx)
            };
            for e in &fx {
                if let Effect::Exec(c) = e {
                    self.session.editor.execute(c).unwrap();
                }
            }
            fx
        }

        fn click(&mut self, x: f64, y: f64) -> Vec<Effect> {
            self.send(Input::Press {
                p: DVec2::new(x, y),
                shift: false,
            })
        }

        fn shapes(&self) -> Vec<IrrigationShape> {
            self.session
                .editor
                .doc()
                .objects
                .values()
                .filter_map(|o| match &o.shape {
                    Shape::Irrigation(i) => Some(i.as_ref().clone()),
                    _ => None,
                })
                .collect()
        }

        fn preview(&self, x: f64, y: f64) -> Preview {
            let cx = Ctx {
                doc: self.session.editor.doc(),
                selection: &[],
                settings: &self.settings,
                tol: 5.0,
                context: None,
            };
            self.m.preview(DVec2::new(x, y), &cx)
        }
    }

    #[test]
    fn outlet_click_places_a_head_with_the_catalog_radius() {
        let mut f = Fx::new(IrrMode::Outlet);
        f.settings.irrigation.arc = 90.0;
        f.settings.irrigation.direction = 45.0;
        assert!(matches!(
            f.preview(1.0, 2.0),
            Preview::Shape(Shape::Irrigation(_))
        ));
        f.click(100.0, 200.0);
        assert_eq!(
            f.shapes(),
            [IrrigationShape::Outlet {
                at: DVec2::new(100.0, 200.0),
                model: "Spray-4.0".into(),
                arc: 90.0,
                direction: 45.0,
                radius: Some(4000.0),
                zone: None,
            }]
        );
        // An unknown model gives a notice and no object.
        f.settings.irrigation.model = "Nope".into();
        let fx = f.click(0.0, 0.0);
        assert!(matches!(fx[0], Effect::Notice(_)));
        assert_eq!(f.shapes().len(), 1);
        assert!(matches!(f.preview(1.0, 2.0), Preview::None));
    }

    #[test]
    fn line_mode_collects_points_until_enter() {
        let mut f = Fx::new(IrrMode::Line);
        f.settings.irrigation.line_type = LineType::Mainline;
        f.settings.irrigation.size = "50".into();
        f.settings.irrigation.material = "PVC".into();
        f.click(0.0, 0.0);
        f.click(0.0, 0.0); // the same point twice counts once
        assert!(f.m.is_busy());
        assert!(matches!(f.preview(10.0, 0.0), Preview::Path { .. }));
        f.click(1000.0, 0.0);
        f.click(1000.0, 500.0);
        assert!(f.shapes().is_empty());
        f.send(Input::Enter);
        let IrrigationShape::Line {
            points,
            line_type,
            size,
            ..
        } = &f.shapes()[0]
        else {
            panic!()
        };
        assert_eq!(points.len(), 3);
        assert_eq!((*line_type, size.as_str()), (LineType::Mainline, "50"));
        assert!(!f.m.is_busy());
        // One point is not a line.
        f.click(0.0, 0.0);
        let fx = f.send(Input::DoubleClick);
        assert!(matches!(fx[0], Effect::Notice(_)));
        // Enter with nothing started does nothing, and so do other inputs.
        assert!(f.send(Input::Enter).is_empty());
        assert!(f.send(Input::Move(DVec2::ZERO)).is_empty());
        // Line with an empty size is refused.
        f.settings.irrigation.size.clear();
        f.click(0.0, 0.0);
        f.click(5.0, 5.0);
        assert!(matches!(f.send(Input::Enter)[0], Effect::Notice(_)));
    }

    #[test]
    fn poc_valve_and_controller_clicks() {
        let mut f = Fx::new(IrrMode::Poc);
        f.click(0.0, 0.0);
        f.settings.irrigation.mode = IrrMode::Valve;
        f.click(1000.0, 0.0);
        f.click(2000.0, 0.0);
        f.settings.irrigation.zone = 7;
        f.click(3000.0, 0.0);
        f.settings.irrigation.mode = IrrMode::Controller;
        f.click(0.0, 1000.0);
        let s = f.shapes();
        assert!(
            matches!(s[0], IrrigationShape::Poc { static_pressure, .. } if static_pressure == 400.0)
        );
        // Zones count up from 1, then the chosen zone is used.
        let zones: Vec<u32> = s
            .iter()
            .filter_map(|i| match i {
                IrrigationShape::Valve { zone, .. } => Some(*zone),
                _ => None,
            })
            .collect();
        assert_eq!(zones, [1, 2, 7]);
        assert!(
            matches!(&s[4], IrrigationShape::Controller { name, stations: 6, .. } if name == "Controller")
        );
        // A bad value makes no object: the preview is empty too.
        f.settings.irrigation.mode = IrrMode::Poc;
        f.settings.irrigation.static_pressure = 0.0;
        assert!(matches!(f.click(0.0, 0.0)[0], Effect::Notice(_)));
        assert!(matches!(f.preview(0.0, 0.0), Preview::None));
    }

    #[test]
    fn modes_cycle_and_have_text() {
        let mut m = IrrMode::Outlet;
        for _ in 0..IrrMode::ALL.len() {
            assert!(!m.label().is_empty() && !m.help().is_empty());
            m = m.next();
        }
        assert_eq!(m, IrrMode::Outlet);
    }
}
