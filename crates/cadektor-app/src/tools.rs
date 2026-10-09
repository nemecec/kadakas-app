//! Tool state machines. They turn pointer input (already in model
//! coordinates, already snapped) into commands and selection changes.
//! They never touch the document.

use cadektor_core::command::Command;
use cadektor_core::model::{Attributes, Document, ObjectId, Shape};
use glam::DVec2;

use crate::hit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Line,
    Rect,
    Circle,
    Polyline,
    Text,
    Plant,
    LandscapeArea,
    Hardscape,
}

impl Tool {
    pub const BASIC: [Tool; 6] = [Tool::Select, Tool::Line, Tool::Rect, Tool::Circle, Tool::Polyline, Tool::Text];
    pub const LANDSCAPE: [Tool; 3] = [Tool::Plant, Tool::LandscapeArea, Tool::Hardscape];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Selection",
            Tool::Line => "Line",
            Tool::Rect => "Rectangle",
            Tool::Circle => "Circle",
            Tool::Polyline => "Polyline",
            Tool::Text => "Text",
            Tool::Plant => "Plant",
            Tool::LandscapeArea => "Landscape Area",
            Tool::Hardscape => "Hardscape",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolSettings {
    pub text: String,
    pub text_height: f64,
    pub species: String,
    pub spread: f64,
    pub plant_height: f64,
    pub area_name: String,
    pub material: String,
}

impl Default for ToolSettings {
    fn default() -> Self {
        ToolSettings {
            text: "Text".into(),
            text_height: 250.0,
            species: "Acer campestre".into(),
            spread: 4000.0,
            plant_height: 8000.0,
            area_name: "Planting bed".into(),
            material: "Paving".into(),
        }
    }
}

/// Read-only context for a tool.
pub struct Ctx<'a> {
    pub doc: &'a Document,
    pub selection: &'a [ObjectId],
    pub settings: &'a ToolSettings,
    /// Pick distance in mm.
    pub tol: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Press { p: DVec2, shift: bool },
    Move(DVec2),
    Release(DVec2),
    DoubleClick,
    Enter,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Exec(Command),
    SetSelection(Vec<ObjectId>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    None,
    Shape(Shape),
    Path { points: Vec<DVec2>, closed: bool },
    Box { min: DVec2, max: DVec2, crossing: bool },
    /// Move the listed objects by this delta.
    Move { ids: Vec<ObjectId>, delta: DVec2 },
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    Idle,
    Points(Vec<DVec2>),
    Drag { start: DVec2, cur: DVec2, ids: Vec<ObjectId> },
    Box { start: DVec2, cur: DVec2, add: bool },
}

pub struct ToolMachine {
    tool: Tool,
    state: State,
}

impl Default for ToolMachine {
    fn default() -> Self {
        ToolMachine { tool: Tool::Select, state: State::Idle }
    }
}

fn add(shape: Shape) -> Effect {
    Effect::Exec(Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() })
}

fn dedup(points: &[DVec2]) -> Vec<DVec2> {
    let mut out: Vec<DVec2> = Vec::new();
    for p in points {
        if out.last() != Some(p) {
            out.push(*p);
        }
    }
    out
}

impl ToolMachine {
    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.state = State::Idle;
    }

    /// The last clicked point of the object being drawn.
    pub fn anchor(&self) -> Option<DVec2> {
        match &self.state {
            State::Points(p) => p.last().copied(),
            _ => None,
        }
    }

    /// Takes back the last clicked point. The operation ends when none is left.
    pub fn pop_point(&mut self) {
        if let State::Points(p) = &mut self.state {
            p.pop();
            if p.is_empty() {
                self.state = State::Idle;
            }
        }
    }

    /// True while an operation is in progress (Esc cancels it).
    pub fn is_busy(&self) -> bool {
        self.state != State::Idle
    }

    pub fn cancel(&mut self) {
        self.state = State::Idle;
    }

    /// Objects that a drag is moving. Snapping ignores them.
    pub fn moving_ids(&self) -> &[ObjectId] {
        match &self.state {
            State::Drag { ids, .. } => ids,
            _ => &[],
        }
    }

    fn finish_polygon(&mut self, cx: &Ctx) -> Vec<Effect> {
        let State::Points(pts) = std::mem::replace(&mut self.state, State::Idle) else {
            return vec![];
        };
        let pts = dedup(&pts);
        let s = cx.settings;
        let shape = match self.tool {
            Tool::Polyline if pts.len() >= 2 => Shape::Polyline { points: pts, closed: false },
            Tool::LandscapeArea if pts.len() >= 3 => Shape::LandscapeArea { boundary: pts, name: s.area_name.clone() },
            Tool::Hardscape if pts.len() >= 3 => Shape::Hardscape { boundary: pts, material: s.material.clone() },
            _ => return vec![],
        };
        vec![add(shape)]
    }

    pub fn handle(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        match self.tool {
            Tool::Select => self.handle_select(input, cx),
            Tool::Line | Tool::Rect | Tool::Circle => self.handle_two_click(input),
            Tool::Text | Tool::Plant => match input {
                Input::Press { p, .. } => self.place(p, cx),
                _ => vec![],
            },
            Tool::Polyline | Tool::LandscapeArea | Tool::Hardscape => match input {
                Input::Press { p, .. } => {
                    match &mut self.state {
                        State::Points(pts) => pts.push(p),
                        s => *s = State::Points(vec![p]),
                    }
                    vec![]
                }
                Input::DoubleClick | Input::Enter => self.finish_polygon(cx),
                _ => vec![],
            },
        }
    }

    fn place(&self, p: DVec2, cx: &Ctx) -> Vec<Effect> {
        let s = cx.settings;
        match self.tool {
            Tool::Text if !s.text.is_empty() && s.text_height > 0.0 => {
                vec![add(Shape::Text { at: p, text: s.text.clone(), height: s.text_height })]
            }
            Tool::Plant if s.spread > 0.0 => vec![add(Shape::Plant {
                at: p,
                species: s.species.clone(),
                spread: s.spread,
                height: s.plant_height,
            })],
            _ => vec![],
        }
    }

    fn two_click_shape(tool: Tool, a: DVec2, b: DVec2) -> Option<Shape> {
        if a == b {
            return None;
        }
        Some(match tool {
            Tool::Line => Shape::Line { a, b },
            Tool::Rect => Shape::Rect { min: a.min(b), max: a.max(b) },
            Tool::Circle => Shape::Circle { center: a, radius: a.distance(b) },
            _ => return None,
        })
    }

    fn handle_two_click(&mut self, input: Input) -> Vec<Effect> {
        let Input::Press { p, .. } = input else { return vec![] };
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Points(pts) if !pts.is_empty() => {
                Self::two_click_shape(self.tool, pts[0], p).map(|s| vec![add(s)]).unwrap_or_default()
            }
            _ => {
                self.state = State::Points(vec![p]);
                vec![]
            }
        }
    }

    fn handle_select(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        match input {
            Input::Press { p, shift } => match hit::hit_test(cx.doc, p, cx.tol) {
                Some(id) if shift => {
                    let mut sel = cx.selection.to_vec();
                    match sel.iter().position(|s| *s == id) {
                        Some(i) => {
                            sel.remove(i);
                        }
                        None => sel.push(id),
                    }
                    vec![Effect::SetSelection(sel)]
                }
                Some(id) => {
                    let selected = cx.selection.contains(&id);
                    let ids = if selected { cx.selection.to_vec() } else { vec![id] };
                    self.state = State::Drag { start: p, cur: p, ids };
                    if selected { vec![] } else { vec![Effect::SetSelection(vec![id])] }
                }
                None => {
                    self.state = State::Box { start: p, cur: p, add: shift };
                    if shift { vec![] } else { vec![Effect::SetSelection(vec![])] }
                }
            },
            Input::Move(p) => {
                match &mut self.state {
                    State::Drag { cur, .. } | State::Box { cur, .. } => *cur = p,
                    _ => {}
                }
                vec![]
            }
            Input::Release(p) => match std::mem::replace(&mut self.state, State::Idle) {
                State::Drag { start, ids, .. } => {
                    let delta = p - start;
                    if delta.length_squared() > 0.0 {
                        vec![Effect::Exec(Command::MoveObjects { ids, delta })]
                    } else {
                        vec![]
                    }
                }
                State::Box { start, add, .. } => {
                    if (p - start).abs().max_element() <= cx.tol {
                        return vec![];
                    }
                    let mut sel = if add { cx.selection.to_vec() } else { vec![] };
                    for id in hit::box_select(cx.doc, start, p, p.x < start.x) {
                        if !sel.contains(&id) {
                            sel.push(id);
                        }
                    }
                    vec![Effect::SetSelection(sel)]
                }
                _ => vec![],
            },
            Input::DoubleClick | Input::Enter => vec![],
        }
    }

    /// What to draw as feedback while the tool is active.
    pub fn preview(&self, cursor: DVec2, cx: &Ctx) -> Preview {
        match (&self.state, self.tool) {
            (State::Points(pts), Tool::Line | Tool::Rect | Tool::Circle) => {
                Self::two_click_shape(self.tool, pts[0], cursor).map_or(Preview::None, Preview::Shape)
            }
            (State::Points(pts), _) => {
                let mut points = pts.clone();
                points.push(cursor);
                Preview::Path { points, closed: self.tool != Tool::Polyline }
            }
            (State::Drag { start, ids, .. }, _) => Preview::Move { ids: ids.clone(), delta: cursor - *start },
            (State::Box { start, .. }, _) => {
                Preview::Box { min: start.min(cursor), max: start.max(cursor), crossing: cursor.x < start.x }
            }
            (State::Idle, Tool::Plant) if cx.settings.spread > 0.0 => Preview::Shape(Shape::Plant {
                at: cursor,
                species: cx.settings.species.clone(),
                spread: cx.settings.spread,
                height: cx.settings.plant_height,
            }),
            (State::Idle, Tool::Text) if !cx.settings.text.is_empty() => Preview::Shape(Shape::Text {
                at: cursor,
                text: cx.settings.text.clone(),
                height: cx.settings.text_height.max(1.0),
            }),
            _ => Preview::None,
        }
    }

    /// Text for the data bar: the live measurement of the current operation.
    pub fn readout(&self, cursor: DVec2) -> String {
        let seg = |a: DVec2, b: DVec2| {
            let d = b - a;
            format!("Length {:.1} mm  Angle {:.1}\u{b0}", d.length(), d.y.atan2(d.x).to_degrees())
        };
        match (&self.state, self.tool) {
            (State::Points(p), Tool::Rect) => {
                let d = (cursor - p[0]).abs();
                format!("Width {:.1}  Height {:.1} mm", d.x, d.y)
            }
            (State::Points(p), Tool::Circle) => format!("Radius {:.1} mm", p[0].distance(cursor)),
            (State::Points(p), _) => {
                let last = *p.last().unwrap_or(&cursor);
                format!("{}  Points {}", seg(last, cursor), p.len())
            }
            (State::Drag { start, .. }, _) => format!("Move dX {:.1}  dY {:.1} mm", cursor.x - start.x, cursor.y - start.y),
            (State::Box { start, .. }, _) => {
                let d = (cursor - *start).abs();
                format!("Box {:.1} x {:.1} mm", d.x, d.y)
            }
            (State::Idle, _) => String::new(),
        }
    }

    /// Hint for the status line.
    pub fn hint(&self) -> &'static str {
        match (self.tool, self.is_busy()) {
            (Tool::Select, _) => "Click to select, Shift-click to toggle, drag to move or box select. Delete removes.",
            (Tool::Line | Tool::Rect | Tool::Circle, false) => "Click the first point. Esc returns to Selection.",
            (Tool::Line | Tool::Rect, true) => "Click the second point. Esc cancels.",
            (Tool::Circle, true) => "Click a point on the circle. Esc cancels.",
            (Tool::Polyline | Tool::LandscapeArea | Tool::Hardscape, _) => {
                "Click points. Double-click or Enter finishes. Esc cancels."
            }
            (Tool::Text | Tool::Plant, _) => "Click to place. Set values in the Data bar.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::api::Session;

    struct Fixture {
        session: Session,
        settings: ToolSettings,
        m: ToolMachine,
    }

    impl Fixture {
        fn new(tool: Tool) -> Self {
            let mut m = ToolMachine::default();
            m.set_tool(tool);
            Fixture { session: Session::default(), settings: ToolSettings::default(), m }
        }

        /// Sends an input and applies the effects like the app does.
        fn send(&mut self, input: Input) -> Vec<Effect> {
            let fx = {
                let cx = Ctx { doc: &self.session.editor.doc, selection: &self.session.selection, settings: &self.settings, tol: 5.0 };
                self.m.handle(input, &cx)
            };
            for e in &fx {
                match e {
                    Effect::Exec(c) => {
                        self.session.editor.execute(c).unwrap();
                    }
                    Effect::SetSelection(s) => self.session.selection = s.clone(),
                }
            }
            fx
        }

        fn click(&mut self, x: f64, y: f64) -> Vec<Effect> {
            self.send(Input::Press { p: DVec2::new(x, y), shift: false })
        }
        fn shapes(&self) -> Vec<Shape> {
            self.session.editor.doc.objects.values().map(|o| o.shape.clone()).collect()
        }
    }

    #[test]
    fn line_two_clicks() {
        let mut f = Fixture::new(Tool::Line);
        assert!(f.click(0.0, 0.0).is_empty());
        assert!(f.m.is_busy());
        f.click(100.0, 50.0);
        assert!(!f.m.is_busy());
        assert_eq!(f.shapes(), vec![Shape::Line { a: DVec2::ZERO, b: DVec2::new(100.0, 50.0) }]);
    }

    #[test]
    fn rect_normalizes_and_circle_radius() {
        let mut f = Fixture::new(Tool::Rect);
        f.click(100.0, 100.0);
        f.click(0.0, 20.0);
        assert_eq!(f.shapes(), vec![Shape::Rect { min: DVec2::new(0.0, 20.0), max: DVec2::new(100.0, 100.0) }]);
        let mut f = Fixture::new(Tool::Circle);
        f.click(0.0, 0.0);
        f.click(30.0, 40.0);
        assert_eq!(f.shapes(), vec![Shape::Circle { center: DVec2::ZERO, radius: 50.0 }]);
    }

    #[test]
    fn zero_size_click_makes_nothing() {
        let mut f = Fixture::new(Tool::Line);
        f.click(5.0, 5.0);
        f.click(5.0, 5.0);
        assert!(f.shapes().is_empty());
    }

    #[test]
    fn polyline_double_click_finishes_without_duplicates() {
        let mut f = Fixture::new(Tool::Polyline);
        f.click(0.0, 0.0);
        f.click(100.0, 0.0);
        f.click(100.0, 100.0);
        f.click(100.0, 100.0); // second press of the double click
        f.send(Input::DoubleClick);
        assert_eq!(
            f.shapes(),
            vec![Shape::Polyline { points: vec![DVec2::ZERO, DVec2::new(100.0, 0.0), DVec2::new(100.0, 100.0)], closed: false }]
        );
        assert!(!f.m.is_busy());
    }

    #[test]
    fn polygon_needs_three_points_and_esc_cancels() {
        let mut f = Fixture::new(Tool::Hardscape);
        f.click(0.0, 0.0);
        f.click(10.0, 0.0);
        f.send(Input::Enter);
        assert!(f.shapes().is_empty());
        f.click(0.0, 0.0);
        f.m.cancel();
        f.send(Input::Enter);
        assert!(f.shapes().is_empty());
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)] {
            f.click(x, y);
        }
        f.send(Input::Enter);
        assert!(matches!(&f.shapes()[0], Shape::Hardscape { boundary, material } if boundary.len() == 3 && material == "Paving"));
    }

    #[test]
    fn plant_uses_settings() {
        let mut f = Fixture::new(Tool::Plant);
        f.settings.species = "Quercus robur".into();
        f.settings.spread = 6000.0;
        f.click(10.0, 20.0);
        assert!(matches!(&f.shapes()[0], Shape::Plant { species, spread, .. } if species == "Quercus robur" && *spread == 6000.0));
    }

    fn with_two_circles(f: &mut Fixture) {
        for x in [0.0, 1000.0] {
            f.session
                .editor
                .execute(&Command::AddObject {
                    shape: Shape::Circle { center: DVec2::new(x, 0.0), radius: 100.0 },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap();
        }
    }

    #[test]
    fn select_click_shift_and_empty() {
        let mut f = Fixture::new(Tool::Select);
        with_two_circles(&mut f);
        let ids: Vec<_> = f.session.editor.doc.objects.values().map(|o| o.id).collect();
        f.click(100.0, 0.0);
        f.send(Input::Release(DVec2::new(100.0, 0.0)));
        assert_eq!(f.session.selection, vec![ids[0]]);
        f.send(Input::Press { p: DVec2::new(1100.0, 0.0), shift: true });
        f.send(Input::Release(DVec2::new(1100.0, 0.0)));
        assert_eq!(f.session.selection, vec![ids[0], ids[1]]);
        f.send(Input::Press { p: DVec2::new(1100.0, 0.0), shift: true });
        assert_eq!(f.session.selection, vec![ids[0]]);
        f.click(500.0, 500.0);
        f.send(Input::Release(DVec2::new(500.0, 500.0)));
        assert!(f.session.selection.is_empty());
    }

    #[test]
    fn drag_moves_with_one_command() {
        let mut f = Fixture::new(Tool::Select);
        with_two_circles(&mut f);
        f.click(100.0, 0.0);
        f.send(Input::Move(DVec2::new(150.0, 20.0)));
        assert_eq!(f.m.moving_ids().len(), 1);
        let fx = f.send(Input::Release(DVec2::new(200.0, 40.0)));
        assert_eq!(fx.len(), 1);
        let doc = &f.session.editor.doc;
        assert!(matches!(doc.objects[0].shape, Shape::Circle { center, .. } if center == DVec2::new(100.0, 40.0)));
        f.session.editor.undo();
        assert!(matches!(f.session.editor.doc.objects[0].shape, Shape::Circle { center, .. } if center == DVec2::ZERO));
    }

    #[test]
    fn click_without_motion_does_not_move() {
        let mut f = Fixture::new(Tool::Select);
        with_two_circles(&mut f);
        f.click(100.0, 0.0);
        assert!(f.send(Input::Release(DVec2::new(100.0, 0.0))).is_empty());
    }

    #[test]
    fn rubber_band_directions() {
        let mut f = Fixture::new(Tool::Select);
        with_two_circles(&mut f);
        // Left to right: fully enclosed only.
        f.click(-300.0, -300.0);
        f.send(Input::Release(DVec2::new(150.0, 300.0)));
        assert_eq!(f.session.selection.len(), 1);
        // Right to left: crossing selects both.
        f.click(1050.0, 300.0);
        f.send(Input::Release(DVec2::new(50.0, -300.0)));
        assert_eq!(f.session.selection.len(), 2);
    }

    #[test]
    fn previews_and_keys() {
        let mut f = Fixture::new(Tool::Line);
        f.click(0.0, 0.0);
        let cx = Ctx { doc: &f.session.editor.doc, selection: &[], settings: &f.settings, tol: 1.0 };
        assert!(matches!(f.m.preview(DVec2::X, &cx), Preview::Shape(Shape::Line { .. })));
        assert!(f.m.readout(DVec2::new(3.0, 4.0)).contains("Length 5.0"));
        assert_eq!(f.m.anchor(), Some(DVec2::ZERO));
        f.m.pop_point();
        assert!(!f.m.is_busy() && f.m.anchor().is_none());
    }
}
