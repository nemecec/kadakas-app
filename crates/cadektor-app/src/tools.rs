//! Tool state machines. They turn pointer input (already in model
//! coordinates, already snapped) into commands and selection changes.
//! They never touch the document.

use cadektor_core::command::Command;
use cadektor_core::curve;
use cadektor_core::model::{Attributes, Document, ModifierKind, ObjectId, Shape, Vertex, VertexKind};
use glam::DVec2;

use crate::hit;

mod plant_tool;
pub use plant_tool::{By, PlantMode, PlantSettings};

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
    /// Site modifiers: one tool with modes (pad, grade limits).
    SiteModifier,
}

/// What the Site Modifiers tool makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModifierMode {
    /// A flat or sloped area of the proposed surface.
    #[default]
    Pad,
    /// The edge of the modified area.
    GradeLimits,
}

impl ModifierMode {
    pub const ALL: [ModifierMode; 2] = [ModifierMode::Pad, ModifierMode::GradeLimits];

    pub fn name(self) -> &'static str {
        match self {
            ModifierMode::Pad => "Pad",
            ModifierMode::GradeLimits => "Grade Limits",
        }
    }
}

impl Tool {
    pub const BASIC: [Tool; 6] = [Tool::Select, Tool::Line, Tool::Rect, Tool::Circle, Tool::Polyline, Tool::Text];
    pub const LANDSCAPE: [Tool; 3] = [Tool::Plant, Tool::LandscapeArea, Tool::Hardscape];
    pub const SITE: [Tool; 1] = [Tool::SiteModifier];

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
            Tool::SiteModifier => "Site Modifiers",
        }
    }
}

/// Radius of new arc vertices until the user changes it, in mm.
pub const DEFAULT_ARC_RADIUS: f64 = 500.0;

/// True for the tools that draw a path of vertices and have vertex modes.
pub fn has_vertex_modes(tool: Tool) -> bool {
    matches!(tool, Tool::Polyline | Tool::LandscapeArea | Tool::Hardscape | Tool::SiteModifier)
}

/// What the next click adds to a polyline, landscape area or hardscape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VertexMode {
    #[default]
    Corner,
    /// A quadratic Bezier control point.
    Bezier,
    /// A smooth point that the curve passes through (cubic spline).
    Cubic,
    /// A fillet arc with the default radius.
    Arc,
    /// An arc that continues the last segment and ends at the click.
    TangentArc,
    /// The click is a point on an arc through the neighbors.
    PointOnArc,
}

impl VertexMode {
    pub const ALL: [VertexMode; 6] = [
        VertexMode::Corner,
        VertexMode::Bezier,
        VertexMode::Cubic,
        VertexMode::TangentArc,
        VertexMode::PointOnArc,
        VertexMode::Arc,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VertexMode::Corner => "Corner",
            VertexMode::Bezier => "B\u{e9}zier",
            VertexMode::Cubic => "Cubic",
            VertexMode::Arc => "Arc",
            VertexMode::TangentArc => "Tangent arc",
            VertexMode::PointOnArc => "Point on arc",
        }
    }

    /// The next mode in the list, wrapping around.
    pub fn next(self) -> VertexMode {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn kind(self, radius: f64) -> VertexKind {
        match self {
            VertexMode::Corner | VertexMode::TangentArc => VertexKind::Corner,
            VertexMode::Bezier => VertexKind::Bezier,
            VertexMode::Cubic => VertexKind::Cubic,
            VertexMode::Arc => VertexKind::Arc { radius },
            VertexMode::PointOnArc => VertexKind::PointOnArc,
        }
    }
}

/// The vertices of a path after a click at `p` in `mode`. A tangent arc adds
/// two vertices (a point on the arc, and `p`). It is a corner when there is
/// no segment to continue or `p` is on the tangent line.
pub fn extend_path(existing: &[Vertex], p: DVec2, mode: VertexMode, radius: f64) -> Vec<Vertex> {
    let mut v = existing.to_vec();
    if mode == VertexMode::TangentArc
        && let Some(pair) = curve::tangent_arc_vertices(&v, p)
    {
        // The start of the arc must be a corner for the arc to be tangent.
        if let Some(last) = v.last_mut() {
            last.kind = VertexKind::Corner;
        }
        v.extend(pair);
    } else {
        v.push(Vertex::new(p, mode.kind(radius)));
    }
    v
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
    /// Vertex mode of the Polyline, Landscape Area and Hardscape tools.
    pub vertex_mode: VertexMode,
    /// Radius for new arc vertices in mm. It stays until the user changes it.
    pub arc_radius: f64,
    /// Placement mode and style of the Plant tool.
    pub plant: PlantSettings,
    /// Elevation of new pads in mm.
    pub pad_elevation: f64,
    /// What the Site Modifiers tool makes.
    pub modifier_mode: ModifierMode,
    /// Elevation at the end of a sloped pad in mm. `None`: a level pad.
    pub pad_end_elevation: Option<f64>,
    /// The direction in which a sloped pad falls, in degrees counterclockwise from +X.
    pub pad_direction_deg: f64,
    /// Style, path and width options of the Hardscape and Landscape Area tools.
    pub areas: crate::area_ui::AreaToolSettings,
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
            vertex_mode: VertexMode::Corner,
            arc_radius: DEFAULT_ARC_RADIUS,
            plant: PlantSettings::default(),
            pad_elevation: 0.0,
            modifier_mode: ModifierMode::Pad,
            pad_end_elevation: None,
            pad_direction_deg: 0.0,
            areas: Default::default(),
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
    /// The group you are in (selection picks its members), if any.
    pub context: Option<ObjectId>,
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
    /// A message for the status line.
    Notice(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    None,
    Shape(Shape),
    Path { vertices: Vec<Vertex>, closed: bool },
    Box { min: DVec2, max: DVec2, crossing: bool },
    /// Move the listed objects by this delta.
    Move { ids: Vec<ObjectId>, delta: DVec2 },
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    Idle,
    Points(Vec<DVec2>),
    /// The vertices of a path after each click. The last entry is the current path.
    /// Taking back a click removes the last entry.
    Path(Vec<Vec<Vertex>>),
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

/// Removes a vertex that sits on the same point as the one before it.
fn dedup(vertices: &[Vertex]) -> Vec<Vertex> {
    let mut out: Vec<Vertex> = Vec::new();
    for v in vertices {
        if out.last().map(|l| l.p) != Some(v.p) {
            out.push(*v);
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
            State::Path(h) => h.last().and_then(|v| v.last()).map(|v| v.p),
            _ => None,
        }
    }

    /// Takes back the last clicked point. The operation ends when none is left.
    pub fn pop_point(&mut self) {
        match &mut self.state {
            State::Points(p) => {
                p.pop();
                if p.is_empty() {
                    self.state = State::Idle;
                }
            }
            State::Path(h) => {
                h.pop();
                if h.is_empty() {
                    self.state = State::Idle;
                }
            }
            _ => {}
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
        let State::Path(mut history) = std::mem::replace(&mut self.state, State::Idle) else {
            return vec![];
        };
        let mut pts = dedup(&history.pop().unwrap_or_default());
        let s = cx.settings;
        if self.tool == Tool::Polyline {
            // The ends of an open path are on the curve, so their kind has no meaning.
            if let Some(first) = pts.first_mut() {
                first.kind = VertexKind::Corner;
            }
            if let Some(last) = pts.last_mut() {
                last.kind = VertexKind::Corner;
            }
        }
        // A path or a style makes a hardscape with settings, a style makes a landscape area with a mix.
        if self.tool == Tool::Hardscape && s.areas.path {
            if pts.len() < 2 {
                return vec![];
            }
            // The ends of a center line are on the curve, so their kind has no meaning.
            let last = pts.len() - 1;
            pts[0].kind = VertexKind::Corner;
            pts[last].kind = VertexKind::Corner;
            return s.areas.hardscape_command(&pts).map(Effect::Exec).into_iter().collect();
        }
        let styled = match self.tool {
            Tool::Hardscape if pts.len() >= 3 => s.areas.hardscape_command(&pts),
            Tool::LandscapeArea if pts.len() >= 3 => s.areas.landscape_command(&pts, &s.area_name),
            _ => None,
        };
        if let Some(cmd) = styled {
            return vec![Effect::Exec(cmd)];
        }
        let shape = match self.tool {
            Tool::Polyline if pts.len() >= 2 => Shape::Polyline { points: pts, closed: false },
            Tool::LandscapeArea if pts.len() >= 3 => Shape::LandscapeArea { boundary: pts, name: s.area_name.clone() },
            Tool::Hardscape if pts.len() >= 3 => Shape::Hardscape { boundary: pts, material: s.material.clone(), thickness: None, elevation: None },
            Tool::SiteModifier if pts.len() >= 3 && s.modifier_mode == ModifierMode::Pad => {
                let flat = cadektor_core::site::boundary_points(&pts);
                let slope = s.pad_end_elevation.and_then(|end| {
                    cadektor_core::site::slope_from_end_elevation(&flat, s.pad_elevation, s.pad_direction_deg.to_radians(), end)
                });
                Shape::SiteModifier { modifier: ModifierKind::Pad { elevation: s.pad_elevation, slope }, boundary: pts }
            }
            Tool::SiteModifier if pts.len() >= 3 => Shape::SiteModifier { modifier: ModifierKind::GradeLimit, boundary: pts },
            _ => return vec![],
        };
        vec![add(shape)]
    }

    pub fn handle(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        match self.tool {
            Tool::Select => self.handle_select(input, cx),
            Tool::Line | Tool::Rect | Tool::Circle => self.handle_two_click(input),
            Tool::Text => match input {
                Input::Press { p, .. } => self.place(p, cx),
                _ => vec![],
            },
            Tool::Plant => self.handle_plant(input, cx),
            Tool::Polyline | Tool::LandscapeArea | Tool::Hardscape | Tool::SiteModifier => match input {
                Input::Press { p, .. } => {
                    let s = cx.settings;
                    if !matches!(self.state, State::Path(_)) {
                        self.state = State::Path(vec![]);
                    }
                    if let State::Path(history) = &mut self.state {
                        let next = extend_path(history.last().map_or(&[], |v| v.as_slice()), p, s.vertex_mode, s.arc_radius);
                        history.push(next);
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
            Input::Press { p, shift } => match hit::hit_test_in(cx.doc, p, cx.tol, cx.context) {
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
                    for id in hit::box_select_in(cx.doc, start, p, p.x < start.x, cx.context) {
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
            (_, Tool::Plant) => self.plant_preview(cursor, cx),
            (State::Points(_), _) => Preview::None,
            (State::Path(h), _) => {
                let s = cx.settings;
                let vertices = extend_path(h.last().map_or(&[], |v| v.as_slice()), cursor, s.vertex_mode, s.arc_radius);
                Preview::Path { vertices, closed: self.tool != Tool::Polyline }
            }
            (State::Drag { start, ids, .. }, _) => Preview::Move { ids: ids.clone(), delta: cursor - *start },
            (State::Box { start, .. }, _) => {
                Preview::Box { min: start.min(cursor), max: start.max(cursor), crossing: cursor.x < start.x }
            }
            (State::Idle, Tool::Text) if !cx.settings.text.is_empty() => Preview::Shape(Shape::Text {
                at: cursor,
                text: cx.settings.text.clone(),
                height: cx.settings.text_height.max(1.0),
            }),
            _ => Preview::None,
        }
    }

    /// Text for the data bar: the live measurement of the current operation.
    pub fn readout(&self, cursor: DVec2, settings: &ToolSettings) -> String {
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
            (State::Path(h), _) => {
                let verts = h.last().map_or(&[][..], |v| v.as_slice());
                let last = verts.last().map_or(cursor, |v| v.p);
                let mode = settings.vertex_mode;
                let radius = if mode == VertexMode::Arc { format!("  Radius {:.1} mm", settings.arc_radius) } else { String::new() };
                format!("{}  Points {}  Mode {}{radius}", seg(last, cursor), verts.len(), mode.label())
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
            (Tool::Polyline | Tool::LandscapeArea | Tool::Hardscape | Tool::SiteModifier, _) => {
                "Click points. Pick a vertex mode in the Tool bar (U cycles). Double-click or Enter finishes. Esc cancels."
            }
            (Tool::Text, _) => "Click to place. Set values in the Data bar.",
            (Tool::Plant, _) => "Pick a placement mode and a plant style in the Tool bar. Click to place. Enter finishes path, fill and poly. Esc cancels.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_api::Session;

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
                let cx = Ctx { doc: self.session.editor.doc(), selection: &self.session.selection, settings: &self.settings, tol: 5.0, context: self.session.edit_context() };
                self.m.handle(input, &cx)
            };
            for e in &fx {
                match e {
                    Effect::Exec(c) => {
                        self.session.editor.execute(c).unwrap();
                    }
                    Effect::SetSelection(s) => self.session.selection = s.clone(),
                    Effect::Notice(_) => {}
                }
            }
            fx
        }

        fn click(&mut self, x: f64, y: f64) -> Vec<Effect> {
            self.send(Input::Press { p: DVec2::new(x, y), shift: false })
        }
        fn shapes(&self) -> Vec<Shape> {
            self.session.editor.doc().objects.values().map(|o| o.shape.clone()).collect()
        }
    }

    #[test]
    fn site_modifier_tool_makes_pads_and_grade_limits_by_mode() {
        let mut f = Fixture::new(Tool::SiteModifier);
        f.settings.pad_elevation = 1250.0;
        for (x, y) in [(0.0, 0.0), (1000.0, 0.0), (1000.0, 800.0)] {
            f.click(x, y);
        }
        f.send(Input::Enter);
        let mut g = Fixture::new(Tool::SiteModifier);
        g.settings.modifier_mode = ModifierMode::GradeLimits;
        for (x, y) in [(-100.0, -100.0), (2000.0, 0.0), (2000.0, 2000.0)] {
            g.click(x, y);
        }
        g.send(Input::DoubleClick);
        let Shape::SiteModifier { modifier, boundary } = &f.shapes()[0] else { panic!("not a modifier") };
        assert_eq!(*modifier, ModifierKind::Pad { elevation: 1250.0, slope: None });
        assert_eq!(boundary.len(), 3);
        assert!(matches!(&g.shapes()[0], Shape::SiteModifier { modifier: ModifierKind::GradeLimit, .. }));
        // A sloped pad: the elevation at its end sets the percent.
        let mut k = Fixture::new(Tool::SiteModifier);
        k.settings.pad_elevation = 1000.0;
        k.settings.pad_end_elevation = Some(900.0);
        for (x, y) in [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)] {
            k.click(x, y);
        }
        k.send(Input::Enter);
        let Shape::SiteModifier { modifier: ModifierKind::Pad { slope: Some(s), .. }, .. } = &k.shapes()[0] else { panic!("no slope") };
        assert!((s.percent - 20.0).abs() < 1e-9 && s.direction == 0.0, "{s:?}");
        // Two points are not enough.
        let mut h = Fixture::new(Tool::SiteModifier);
        h.click(0.0, 0.0);
        h.click(10.0, 0.0);
        h.send(Input::Enter);
        assert!(h.shapes().is_empty());
        assert!(has_vertex_modes(Tool::SiteModifier) && Tool::SITE.contains(&Tool::SiteModifier));
        assert_eq!((Tool::SiteModifier.name(), ModifierMode::GradeLimits.name(), ModifierMode::Pad.name()), ("Site Modifiers", "Grade Limits", "Pad"));
        assert!(h.m.hint().contains("Click points"));
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
            vec![Shape::Polyline { points: curve::corners(&[DVec2::ZERO, DVec2::new(100.0, 0.0), DVec2::new(100.0, 100.0)]), closed: false }]
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
        assert!(matches!(&f.shapes()[0], Shape::Hardscape { boundary, material, .. } if boundary.len() == 3 && material == "Paving"));
    }

    #[test]
    fn hardscape_tool_makes_styled_slabs_and_paths() {
        use cadektor_core::hardscape;
        use cadektor_core::resource::ResourceId;
        let mut f = Fixture::new(Tool::Hardscape);
        let style = ResourceId(
            f.session
                .editor
                .execute(&Command::AddResource { name: "S".into(), body: cadektor_core::resource::ResourceBody::HardscapeStyle(Default::default()) })
                .unwrap()
                .created[0],
        );
        // A style makes a hardscape with settings.
        f.settings.areas.hardscape_style = Some(style);
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)] {
            f.click(x, y);
        }
        f.send(Input::Enter);
        let o = f.session.editor.doc().objects.values().next().unwrap().clone();
        assert_eq!(hardscape::data(&o).unwrap().style, Some(style));
        // Path mode needs two points and makes a path with the width.
        f.settings.areas.path = true;
        f.settings.areas.width = 800.0;
        f.click(0.0, 0.0);
        f.send(Input::Enter);
        assert_eq!(f.session.editor.doc().objects.len(), 1, "one point is not a path");
        f.click(0.0, 0.0);
        f.click(5000.0, 0.0);
        f.send(Input::Enter);
        let path = f.session.editor.doc().objects.values().nth(1).unwrap().clone();
        let d = hardscape::data(&path).unwrap();
        assert_eq!((d.mode, d.width), (hardscape::Mode::Path, 800.0));
        assert!((path.shape.area().unwrap() - 4.0e6).abs() < 1.0);
        // A landscape area with a style.
        let mut g = Fixture::new(Tool::LandscapeArea);
        let ls = ResourceId(
            g.session
                .editor
                .execute(&Command::AddResource { name: "L".into(), body: cadektor_core::resource::ResourceBody::LandscapeAreaStyle(Default::default()) })
                .unwrap()
                .created[0],
        );
        g.settings.areas.landscape_style = Some(ls);
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)] {
            g.click(x, y);
        }
        g.send(Input::Enter);
        let a = g.session.editor.doc().objects.values().next().unwrap().clone();
        assert_eq!(cadektor_core::landscape::data(&a).unwrap().style, Some(ls));
        assert!(matches!(a.shape, Shape::LandscapeArea { ref name, .. } if name == "Planting bed"));
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
        let ids: Vec<_> = f.session.editor.doc().objects.values().map(|o| o.id).collect();
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
        let doc = &f.session.editor.doc();
        assert!(matches!(doc.objects[0].shape, Shape::Circle { center, .. } if center == DVec2::new(100.0, 40.0)));
        f.session.editor.undo();
        assert!(matches!(f.session.editor.doc().objects[0].shape, Shape::Circle { center, .. } if center == DVec2::ZERO));
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
        let cx = Ctx { doc: f.session.editor.doc(), selection: &[], settings: &f.settings, tol: 1.0, context: None };
        assert!(matches!(f.m.preview(DVec2::X, &cx), Preview::Shape(Shape::Line { .. })));
        assert!(f.m.readout(DVec2::new(3.0, 4.0), &f.settings).contains("Length 5.0"));
        assert_eq!(f.m.anchor(), Some(DVec2::ZERO));
        f.m.pop_point();
        assert!(!f.m.is_busy() && f.m.anchor().is_none());
    }

    fn verts(shape: &Shape) -> Vec<Vertex> {
        shape.vertices().expect("a path shape").clone()
    }

    #[test]
    fn mode_list_cycles_through_all_six_and_wraps() {
        let mut m = VertexMode::Corner;
        let mut seen = vec![m];
        for _ in 0..6 {
            m = m.next();
            seen.push(m);
        }
        assert_eq!(seen[..6], VertexMode::ALL);
        assert_eq!(seen[6], VertexMode::Corner);
        let labels: Vec<_> = VertexMode::ALL.iter().map(|m| m.label()).collect();
        assert_eq!(labels, ["Corner", "B\u{e9}zier", "Cubic", "Tangent arc", "Point on arc", "Arc"]);
        assert!(has_vertex_modes(Tool::Polyline) && has_vertex_modes(Tool::Hardscape) && has_vertex_modes(Tool::LandscapeArea));
        assert!(!has_vertex_modes(Tool::Line) && !has_vertex_modes(Tool::Select));
        assert_eq!(ToolSettings::default().vertex_mode, VertexMode::Corner);
    }

    #[test]
    fn each_mode_adds_its_vertex_kind() {
        let cases = [
            (VertexMode::Corner, VertexKind::Corner),
            (VertexMode::Bezier, VertexKind::Bezier),
            (VertexMode::Cubic, VertexKind::Cubic),
            (VertexMode::Arc, VertexKind::Arc { radius: 123.0 }),
            (VertexMode::PointOnArc, VertexKind::PointOnArc),
            // A tangent arc with nothing to continue is a corner.
            (VertexMode::TangentArc, VertexKind::Corner),
        ];
        for (mode, kind) in cases {
            let mut f = Fixture::new(Tool::Hardscape);
            f.settings.vertex_mode = mode;
            f.settings.arc_radius = 123.0;
            for (x, y) in [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)] {
                f.click(x, y);
            }
            // The first vertex (nothing to continue) is a corner for a tangent arc.
            f.send(Input::Enter);
            let vs = verts(&f.shapes()[0]);
            assert_eq!(vs[0].kind, kind, "{mode:?}");
            if mode != VertexMode::TangentArc {
                assert!(vs.iter().all(|v| v.kind == kind), "{mode:?}");
            }
            assert!(f.session.editor.doc().objects.len() == 1);
        }
    }

    #[test]
    fn polyline_ends_are_corners_but_inner_vertices_keep_their_kind() {
        let mut f = Fixture::new(Tool::Polyline);
        f.settings.vertex_mode = VertexMode::Arc;
        for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)] {
            f.click(x, y);
        }
        f.send(Input::Enter);
        let vs = verts(&f.shapes()[0]);
        assert_eq!(vs[0].kind, VertexKind::Corner);
        assert_eq!(vs[1].kind, VertexKind::Arc { radius: DEFAULT_ARC_RADIUS });
        assert_eq!(vs[2].kind, VertexKind::Corner);
    }

    #[test]
    fn the_mode_can_change_between_clicks_and_the_radius_follows_the_setting() {
        let mut f = Fixture::new(Tool::Polyline);
        f.click(0.0, 0.0);
        f.settings.vertex_mode = VertexMode::Arc;
        f.settings.arc_radius = 40.0;
        f.click(100.0, 0.0);
        f.settings.arc_radius = 60.0;
        f.click(100.0, 100.0);
        f.settings.vertex_mode = VertexMode::Corner;
        f.click(0.0, 100.0);
        f.send(Input::DoubleClick);
        let vs = verts(&f.shapes()[0]);
        assert_eq!(vs.len(), 4);
        assert_eq!(vs[1].kind, VertexKind::Arc { radius: 40.0 });
        assert_eq!(vs[2].kind, VertexKind::Arc { radius: 60.0 });
        assert_eq!(vs[3].kind, VertexKind::Corner);
    }

    #[test]
    fn tangent_arc_continues_the_last_segment_and_ends_at_the_click() {
        let mut f = Fixture::new(Tool::Polyline);
        f.click(0.0, 0.0);
        f.settings.vertex_mode = VertexMode::TangentArc;
        f.click(100.0, 0.0); // No segment yet before this one: a line.
        f.click(200.0, 100.0);
        f.send(Input::Enter);
        let Shape::Polyline { points, .. } = &f.shapes()[0] else { panic!() };
        // Corner, corner, point on arc, corner.
        let kinds: Vec<_> = points.iter().map(|v| v.kind).collect();
        assert_eq!(kinds, [VertexKind::Corner, VertexKind::Corner, VertexKind::PointOnArc, VertexKind::Corner]);
        assert_eq!(points[3].p, DVec2::new(200.0, 100.0));
        // The arc is a circle of radius 100 around (100, 100), tangent to the line at (100, 0).
        let path = curve::path(points, false);
        let arcs: Vec<_> = path.segs.iter().filter(|s| matches!(s, curve::Seg::Arc { .. })).collect();
        assert_eq!(arcs.len(), 2);
        for s in arcs {
            let curve::Seg::Arc { center, radius, .. } = s else { unreachable!() };
            assert!(center.distance(DVec2::new(100.0, 100.0)) < 1e-9 && (radius - 100.0).abs() < 1e-9);
        }
        assert_eq!(path.segs.last().unwrap().to(), DVec2::new(200.0, 100.0));
    }

    #[test]
    fn tangent_arc_chains_and_a_point_on_the_tangent_line_is_a_line() {
        let mut f = Fixture::new(Tool::Polyline);
        f.settings.vertex_mode = VertexMode::TangentArc;
        f.click(0.0, 0.0);
        f.click(100.0, 0.0);
        f.click(200.0, 0.0); // On the tangent line: a plain corner vertex.
        f.click(300.0, 100.0);
        f.click(400.0, 100.0);
        f.send(Input::Enter);
        let vs = verts(&f.shapes()[0]);
        let kinds: Vec<_> = vs.iter().map(|v| v.kind).collect();
        use VertexKind::{Corner, PointOnArc};
        assert_eq!(kinds, [Corner, Corner, Corner, PointOnArc, Corner, PointOnArc, Corner]);
        // Everything is connected and the path ends exactly at the last click.
        let path = curve::path(&vs, false);
        let mut at = path.start;
        for s in &path.segs {
            assert!(s.from().distance(at) < 1e-9);
            at = s.to();
        }
        assert_eq!(at, DVec2::new(400.0, 100.0));
    }

    #[test]
    fn taking_back_a_tangent_arc_click_restores_the_previous_vertices() {
        let mut f = Fixture::new(Tool::Polyline);
        f.click(0.0, 0.0);
        f.settings.vertex_mode = VertexMode::Arc;
        f.click(100.0, 0.0);
        f.settings.vertex_mode = VertexMode::TangentArc;
        f.click(200.0, 100.0);
        assert_eq!(f.m.anchor(), Some(DVec2::new(200.0, 100.0)));
        f.m.pop_point();
        // Back to two vertices; the Arc kind of the second vertex is back too.
        assert_eq!(f.m.anchor(), Some(DVec2::new(100.0, 0.0)));
        f.settings.vertex_mode = VertexMode::Corner;
        f.click(100.0, 50.0);
        f.send(Input::Enter);
        let vs = verts(&f.shapes()[0]);
        assert_eq!(vs.len(), 3);
        assert_eq!(vs[1].kind, VertexKind::Arc { radius: DEFAULT_ARC_RADIUS });
        f.m.pop_point();
        assert!(!f.m.is_busy());
    }

    #[test]
    fn the_preview_shows_what_the_next_click_would_add() {
        let mut f = Fixture::new(Tool::Hardscape);
        f.settings.vertex_mode = VertexMode::Arc;
        f.settings.arc_radius = 80.0;
        f.click(0.0, 0.0);
        f.click(100.0, 0.0);
        let cx = Ctx { doc: f.session.editor.doc(), selection: &[], settings: &f.settings, tol: 1.0, context: None };
        let Preview::Path { vertices, closed } = f.m.preview(DVec2::new(100.0, 100.0), &cx) else { panic!() };
        assert!(closed);
        assert_eq!(vertices.len(), 3);
        assert_eq!(vertices[2], Vertex::new(DVec2::new(100.0, 100.0), VertexKind::Arc { radius: 80.0 }));
        let readout = f.m.readout(DVec2::new(100.0, 100.0), &f.settings);
        assert!(readout.contains("Mode Arc") && readout.contains("Radius 80.0"), "{readout}");
        f.settings.vertex_mode = VertexMode::Corner;
        let readout = f.m.readout(DVec2::new(100.0, 100.0), &f.settings);
        assert!(readout.contains("Mode Corner") && !readout.contains("Radius"), "{readout}");
        // A polyline preview is open.
        let mut f = Fixture::new(Tool::Polyline);
        f.click(0.0, 0.0);
        let cx = Ctx { doc: f.session.editor.doc(), selection: &[], settings: &f.settings, tol: 1.0, context: None };
        assert!(matches!(f.m.preview(DVec2::X, &cx), Preview::Path { closed: false, .. }));
    }

    #[test]
    fn a_rounded_hardscape_gets_the_exact_area() {
        let mut f = Fixture::new(Tool::Hardscape);
        f.settings.vertex_mode = VertexMode::Arc;
        f.settings.arc_radius = 100.0;
        for (x, y) in [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)] {
            f.click(x, y);
        }
        f.send(Input::Enter);
        let expected = 1000.0 * 1000.0 - (4.0 - std::f64::consts::PI) * 100.0 * 100.0;
        let area = f.shapes()[0].area().unwrap();
        assert!((area - expected).abs() < 1e-6 * expected, "{area} vs {expected}");
    }

    #[test]
    fn mode_buttons_and_hint_text() {
        let f = Fixture::new(Tool::Polyline);
        assert!(f.m.hint().contains("vertex mode"));
        assert!(f.m.hint().contains("U cycles"));
    }
}
