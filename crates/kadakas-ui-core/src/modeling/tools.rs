//! The drawing tools of the modeler as headless state machines: Line, Rectangle, Circle, Arc,
//! Eraser and Push/Pull.
//!
//! The window feeds them the pointer (an [`Inference`] and the pointer ray), the clicks and the
//! text of the Measurements box. They answer with [`Action`]s in world coordinates, which
//! [`super::exec::run`] turns into one command each (one undo step), and with a [`Preview`] to
//! draw. Nothing here draws, reads a clock or touches the document.
//!
//! # Where the geometry goes
//!
//! At the commit of an action the model is the first of: the model under the first click, the
//! selected model, the model that was drawn in last (`current`), the first surface model of the
//! document. With none of them a new model is made by the same undo step. A shape is drawn on the
//! plane of the face under the first click, else on the horizontal plane through the first point.
//!
//! # Typing
//!
//! `type_char`, `backspace` and `enter` edit the text of the Measurements box. `enter` reads it
//! with [`crate::measure_box::parse`] for the tool and phase (see `accepts`), and applies it:
//! Line takes a length (along the direction of the pointer) or a point, Rectangle `w;h`, Circle a
//! radius, Arc a chord length, then a bulge or a radius, Push/Pull a distance. `24s` sets the
//! number of segments. Just after a Rectangle, Circle, Arc or Push/Pull ended, a new value redoes
//! it: the app takes back that step first (`Out::redo`) when nothing else happened since.
//!
//! # Exclusion
//!
//! [`Modeling::excluded`] names the face or the edges that the tool moves or erases now, so that
//! the inference can skip them (a pulled face must not snap to itself).

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;
use kadakas_core::model::ObjectId;
use kadakas_core::surface::{EdgeId, FaceId, IntersectScope};
use kadakas_l10n::tr;
use kadakas_units::Units;

use super::exec::{Action, Target};
use super::plane::Plane3;
use super::scenes::World;
use super::select::SubSelection;
use super::xform::{Context, Xform, is_transform_tool};
use crate::infer3d::{Constraint, Inference, Key, Kind, Ray, Rgb, TagKind};
use crate::measure_box::{self, Phase, Tool, Value};
use crate::modeler::ModelerTool;

/// Shortest segment or side that a tool draws (mm).
pub(super) const MIN_LEN: f64 = 0.01;
/// The default number of sides of a circle.
pub const DEFAULT_SIDES: u32 = 24;
/// The default number of segments of an arc.
pub const DEFAULT_ARC_SIDES: u32 = 12;

/// The pointer for one event.
pub struct Pointer<'a> {
    pub inf: &'a Inference,
    pub ray: Ray,
    /// The surface model that is selected as a whole, if one is.
    pub selected: Option<ObjectId>,
}

/// What a tool did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    pub actions: Vec<Action>,
    /// The step before is taken back before the actions run (a new value for what was just done).
    pub redo: bool,
    /// Something to tell the person (an input that was refused).
    pub message: Option<String>,
}

impl Out {
    pub(super) fn msg(m: String) -> Out {
        Out {
            message: Some(m),
            ..Out::default()
        }
    }

    pub(super) fn act(a: Action) -> Out {
        Out {
            actions: vec![a],
            ..Out::default()
        }
    }
}

/// What the window draws for the active tool, in world coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    /// The shape that is being drawn.
    pub shape: Vec<[DVec3; 2]>,
    /// Edges that will be erased (the Eraser), and the edge that the pointer is on.
    pub erase: Vec<[DVec3; 2]>,
    /// The face that Push/Pull will move and the place it moves to.
    pub pull: Vec<[DVec3; 2]>,
    /// The grips of Scale on the box around the selection, and the one under the pointer.
    pub grips: Vec<DVec3>,
    pub grip_hover: Option<usize>,
    /// Move or Rotate is in copy mode (the window shows a plus at the pointer).
    pub copy: bool,
}

/// The kind of the value in the Measurements box while a tool runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveLabel {
    Length,
    Dimensions,
    Radius,
    Bulge,
    Distance,
    /// The angle of Rotate.
    Angle,
    /// The factor of Scale.
    Factor,
}

#[derive(Clone, Debug)]
enum Run {
    Idle,
    Line {
        pts: Vec<DVec3>,
        hit: Option<ObjectId>,
    },
    Rect {
        a: DVec3,
        plane: Plane3,
        hit: Option<ObjectId>,
    },
    Circle {
        c: DVec3,
        plane: Plane3,
        hit: Option<ObjectId>,
    },
    Arc {
        pts: Vec<DVec3>,
        plane: Plane3,
        hit: Option<ObjectId>,
    },
    Erase {
        marked: BTreeSet<(ObjectId, EdgeId)>,
        down: bool,
    },
    Pull {
        model: ObjectId,
        face: FaceId,
        base: DVec3,
        normal: DVec3,
        loops: Vec<Vec<DVec3>>,
        /// The edges of the face: they move or stay with it, so the inference skips them too.
        edges: Vec<EdgeId>,
        distance: f64,
    },
}

/// What can be done again with a new typed value.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Last {
    Rect {
        a: DVec3,
        plane: Plane3,
        w: f64,
        h: f64,
        hit: Option<ObjectId>,
    },
    Circle {
        c: DVec3,
        plane: Plane3,
        r: f64,
        theta: f64,
        hit: Option<ObjectId>,
    },
    Arc {
        a: DVec3,
        b: DVec3,
        plane: Plane3,
        h: f64,
        hit: Option<ObjectId>,
    },
    Pull {
        model: ObjectId,
        face: FaceId,
        distance: f64,
    },
}

/// The state of the drawing tools.
#[derive(Debug)]
pub struct Modeling {
    tool: ModelerTool,
    run: Run,
    /// The point under the pointer, on the plane of the tool.
    cursor: Option<DVec3>,
    /// Sides of a circle.
    pub sides: u32,
    /// Segments of an arc.
    pub arc_sides: u32,
    typed: String,
    /// The model that was drawn in last.
    pub current: Option<ObjectId>,
    pending: Option<Last>,
    last: Option<(Last, u64)>,
    /// The distance of the last Push/Pull, for a double click on another face.
    pub last_pull: Option<f64>,
    hover_edge: Option<(ObjectId, EdgeId)>,
    /// Move, Rotate and Scale.
    xf: Xform,
    /// The Paint Bucket and its material. It stays when another tool is chosen.
    pub paint: super::paint::Paint,
}

impl Default for Modeling {
    fn default() -> Self {
        Modeling {
            tool: ModelerTool::Select,
            run: Run::Idle,
            cursor: None,
            sides: DEFAULT_SIDES,
            arc_sides: DEFAULT_ARC_SIDES,
            typed: String::new(),
            current: None,
            pending: None,
            last: None,
            last_pull: None,
            hover_edge: None,
            xf: Xform::default(),
            paint: super::paint::Paint::default(),
        }
    }
}

/// True for the tools that this module runs.
pub fn is_drawing_tool(t: ModelerTool) -> bool {
    matches!(
        t,
        ModelerTool::Line
            | ModelerTool::Rectangle
            | ModelerTool::Circle
            | ModelerTool::Arc
            | ModelerTool::Eraser
            | ModelerTool::PushPull
    )
}

/// True for the tools that take typed values and run the inference: the drawing tools (not the
/// Eraser) and Move, Rotate and Scale.
pub fn takes_values(t: ModelerTool) -> bool {
    (is_drawing_tool(t) && t != ModelerTool::Eraser) || is_transform_tool(t)
}

fn mb_tool(t: ModelerTool) -> Tool {
    match t {
        ModelerTool::Line => Tool::Line,
        ModelerTool::Rectangle => Tool::Rectangle,
        ModelerTool::Circle => Tool::Circle,
        ModelerTool::Arc => Tool::TwoPointArc,
        ModelerTool::PushPull => Tool::PushPull,
        ModelerTool::Eraser => Tool::Eraser,
        ModelerTool::Move => Tool::Move,
        ModelerTool::Rotate => Tool::Rotate,
        ModelerTool::Scale => Tool::Scale,
        _ => Tool::Select,
    }
}

/// What the face or edge that the tool moves or erases is, for the inference to skip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Excluded {
    pub faces: BTreeSet<(ObjectId, FaceId)>,
    pub edges: BTreeSet<(ObjectId, EdgeId)>,
    /// Whole models (an object that is moved, turned or scaled).
    pub models: BTreeSet<ObjectId>,
}

impl Modeling {
    pub fn tool(&self) -> ModelerTool {
        self.tool
    }

    /// Chooses the tool. What was in progress is dropped.
    pub fn choose(&mut self, tool: ModelerTool) {
        self.tool = tool;
        self.run = Run::Idle;
        self.cursor = None;
        self.typed.clear();
        self.hover_edge = None;
        self.pending = None;
        self.last = None;
        self.xf.choose(tool);
    }

    /// True while an operation is in progress (a click was made and the tool waits for more).
    pub fn in_progress(&self) -> bool {
        !matches!(self.run, Run::Idle) || self.xf.in_progress()
    }

    /// The last point of the tool: the inference works from it.
    pub fn reference(&self) -> Option<DVec3> {
        if let Some(r) = self.xf.reference() {
            return Some(r);
        }
        match &self.run {
            Run::Line { pts, .. } | Run::Arc { pts, .. } => pts.last().copied(),
            Run::Rect { a, .. } => Some(*a),
            Run::Circle { c, .. } => Some(*c),
            Run::Pull { base, .. } => Some(*base),
            Run::Idle | Run::Erase { .. } => None,
        }
    }

    /// The face or edges the tool works on now.
    pub fn excluded(&self) -> Excluded {
        let mut e = Excluded::default();
        match &self.run {
            Run::Pull {
                model, face, edges, ..
            } => {
                e.faces.insert((*model, *face));
                e.edges.extend(edges.iter().map(|x| (*model, *x)));
            }
            Run::Erase { marked, .. } => e.edges.clone_from(marked),
            _ => {}
        }
        if let Some(s) = self.xf.moving() {
            e.models.extend(s.objects().iter().copied());
            for (m, faces, edges) in s.parts_moving() {
                e.faces.extend(faces.iter().map(|f| (m, *f)));
                e.edges.extend(edges.iter().map(|x| (m, *x)));
            }
        }
        e
    }

    /// The selection and the keys for Move, Rotate and Scale. Call it before each event.
    pub fn set_context(&mut self, ctx: Context) {
        self.xf.set_context(ctx);
    }

    /// Ctrl (Option on macOS) was pressed: Move and Rotate toggle copy, Scale toggles scaling about
    /// the center.
    pub fn toggle_ctrl(&mut self) {
        self.xf.toggle_ctrl();
    }

    /// True while Move or Rotate is in copy mode.
    pub fn copying(&self) -> bool {
        self.xf.copying()
    }

    /// How many times Move, Rotate or Scale measured the selection (a test counts them).
    pub fn subject_builds(&self) -> u64 {
        self.xf.subject_builds()
    }

    /// An arrow key for the protractor of Rotate before its center is clicked. True if used.
    pub fn arrow_plane(&mut self, key: Key) -> bool {
        self.xf.arrow(key)
    }

    /// Drops what is in progress and the typed text at once, whatever it was (an undo or a redo
    /// can remove the face or the points that a tool holds). The tool stays chosen.
    pub fn abort(&mut self) {
        self.xf.abort();
        self.run = Run::Idle;
        self.cursor = None;
        self.typed.clear();
        self.hover_edge = None;
    }

    /// Ends what is in progress or clears the typed text. True if there was something.
    pub fn cancel(&mut self) -> bool {
        if !self.typed.is_empty() {
            self.typed.clear();
            return true;
        }
        let was = self.in_progress();
        self.xf.cancel();
        self.run = Run::Idle;
        self.cursor = None;
        was
    }

    // ---- the Measurements box ----------------------------------------------------------------

    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// True for the characters that start a typed value even when the box is empty: digits and
    /// the marks of lengths, angles and points. Letters (units, `s`, `r`) only count when the box
    /// has text already, so that the letters of the tool keys still choose tools.
    pub fn starts_value(c: char) -> bool {
        c.is_ascii_digit() || matches!(c, '.' | ',' | ';' | '-' | '[' | '<' | '\'' | '"' | '/')
    }

    /// A character typed while the tool runs. Returns true if it went into the box.
    pub fn type_char(&mut self, c: char) -> bool {
        if !takes_values(self.tool) {
            return false;
        }
        if Modeling::starts_value(c)
            || (!self.typed.is_empty() && (c.is_alphanumeric() || " ;,.-+*/:°'\"[]<>".contains(c)))
        {
            self.typed.push(c);
            true
        } else {
            false
        }
    }

    pub fn backspace(&mut self) -> bool {
        self.typed.pop().is_some()
    }

    /// The phase for the table of what the box accepts.
    pub fn phase(&self) -> Phase {
        if is_transform_tool(self.tool) {
            return self.xf.phase();
        }
        let done = matches!(self.run, Run::Idle) && self.last.is_some();
        if done {
            return Phase::Done;
        }
        // Step 0: nothing clicked yet (sides can be typed). Then the clicks so far; the
        // two-point arc is at step 1 for the chord and step 2 for the bulge.
        match &self.run {
            Run::Arc { pts, .. } => Phase::Step(pts.len() as u8),
            Run::Idle => Phase::Step(0),
            _ => Phase::Step(1),
        }
    }

    /// The value the box shows while the tool runs, with its label.
    pub fn live(&self, units: &Units) -> Option<(LiveLabel, String)> {
        if is_transform_tool(self.tool) {
            return self.xf.live(units);
        }
        let c = self.cursor?;
        let len = |v: f64| units.length_text(v);
        match &self.run {
            Run::Line { pts, .. } => Some((LiveLabel::Length, len(pts.last()?.distance(c)))),
            Run::Rect { a, plane, .. } => {
                let d = plane.coords(c) - plane.coords(*a);
                let sep = measure_box::value_separator(units);
                Some((
                    LiveLabel::Dimensions,
                    format!("{}{sep} {}", len(d.x.abs()), len(d.y.abs())),
                ))
            }
            Run::Circle { c: center, .. } => Some((LiveLabel::Radius, len(center.distance(c)))),
            Run::Arc { pts, plane, .. } => match pts.as_slice() {
                [a] => Some((LiveLabel::Length, len(a.distance(c)))),
                [a, b] => Some((LiveLabel::Bulge, len(bulge_of(*a, *b, c, plane).abs()))),
                _ => None,
            },
            Run::Pull { distance, .. } => Some((LiveLabel::Distance, len(*distance))),
            Run::Idle | Run::Erase { .. } => None,
        }
    }

    // ---- targets -----------------------------------------------------------------------------

    fn target(&self, w: &World, hit: Option<ObjectId>, selected: Option<ObjectId>) -> Target {
        let exists = |id: &ObjectId| w.scenes.objects().contains(id);
        hit.filter(exists)
            .or(selected.filter(exists))
            .or(self.current.filter(exists))
            .or_else(|| w.scenes.objects().first().copied())
            .map_or(Target::New, Target::Model)
    }

    /// Call after the actions ran: the model they worked in becomes the one to draw in, and a
    /// step that can be redone is kept with the revision it left.
    pub fn committed(&mut self, model: Option<ObjectId>, revision: u64) {
        if model.is_some() {
            self.current = model;
        }
        self.xf.committed(revision);
        if let Some(l) = self.pending.take() {
            self.last = Some((l, revision));
        }
    }

    // ---- the pointer -------------------------------------------------------------------------

    pub(super) fn resolve(p: &Pointer, plane: Option<&Plane3>) -> DVec3 {
        let snapped = !matches!(p.inf.kind, Kind::Free | Kind::OnFace);
        match plane {
            Some(pl) if snapped => pl.project(p.inf.point),
            Some(pl) => pl
                .intersect(&p.ray)
                .unwrap_or_else(|| pl.project(p.inf.point)),
            None => p.inf.point,
        }
    }

    /// The plane to draw on for a first click: the face under the pointer, else the horizontal
    /// plane through the point.
    fn plane_for(w: &World, p: &Pointer, at: DVec3) -> Plane3 {
        if p.inf.source.kind() == TagKind::Face
            && let Some(pl) = w.face_plane(p.inf.source)
        {
            return pl.with_origin(at);
        }
        if let Some(Constraint::Plane { normal, .. }) = p.inf.constraint
            && let Some(pl) = Plane3::through(at, normal)
        {
            return pl;
        }
        Plane3::through(at, DVec3::Z).expect("a unit normal")
    }

    fn hit_model(w: &World, p: &Pointer) -> Option<ObjectId> {
        w.scenes.object(p.inf.source)
    }

    fn edge_under(w: &World, p: &Pointer) -> Option<(ObjectId, EdgeId)> {
        if p.inf.source.kind() != TagKind::Edge {
            return None;
        }
        let id = w.scenes.object(p.inf.source)?;
        Some((id, EdgeId(p.inf.source.id())))
    }

    /// The pointer moved.
    pub fn moved(&mut self, w: &World, p: &Pointer) {
        if is_transform_tool(self.tool) {
            self.xf.moved(w, p);
            return;
        }
        match &mut self.run {
            Run::Line { .. } => self.cursor = Some(p.inf.point),
            Run::Rect { plane, .. } | Run::Circle { plane, .. } | Run::Arc { plane, .. } => {
                let pl = *plane;
                self.cursor = Some(Modeling::resolve(p, Some(&pl)));
            }
            Run::Pull {
                base,
                normal,
                distance,
                ..
            } => {
                let (b, n) = (*base, *normal);
                let snapped = !matches!(p.inf.kind, Kind::Free | Kind::OnFace);
                let t = if snapped {
                    Some((p.inf.point - b).dot(n))
                } else {
                    closest_on_line(&p.ray, b, n)
                };
                if let Some(t) = t.filter(|t| t.is_finite()) {
                    *distance = t;
                    self.cursor = Some(b + n * t);
                }
            }
            Run::Erase { marked, down } => {
                let edge = Modeling::edge_under(w, p);
                self.hover_edge = edge;
                if *down && let Some((m, e)) = edge {
                    for x in w.with_curve(m, e) {
                        marked.insert((m, x));
                    }
                }
            }
            Run::Idle => {
                self.cursor = Some(p.inf.point);
                if self.tool == ModelerTool::Eraser {
                    self.hover_edge = Modeling::edge_under(w, p);
                }
            }
        }
    }

    /// The primary button went down (the Eraser starts a drag; the other tools act on the click).
    pub fn press(&mut self, w: &World, p: &Pointer) {
        if self.tool == ModelerTool::Eraser {
            let mut marked = BTreeSet::new();
            if let Some((m, e)) = Modeling::edge_under(w, p) {
                for x in w.with_curve(m, e) {
                    marked.insert((m, x));
                }
            }
            self.run = Run::Erase { marked, down: true };
        }
    }

    /// The primary button went up (after a drag of the Eraser, this erases what it passed over).
    pub fn release(&mut self) -> Out {
        if let Run::Erase { marked, .. } = std::mem::replace(&mut self.run, Run::Idle) {
            let mut by_model: BTreeMap<ObjectId, Vec<EdgeId>> = BTreeMap::new();
            for (m, e) in marked {
                by_model.entry(m).or_default().push(e);
            }
            return Out {
                actions: by_model
                    .into_iter()
                    .map(|(model, edges)| Action::EraseEdges { model, edges })
                    .collect(),
                ..Out::default()
            };
        }
        Out::default()
    }

    /// A click of the primary button (not for the Eraser: it works with `press` and `release`).
    pub fn click(&mut self, w: &World, p: &Pointer) -> Out {
        self.typed.clear();
        if is_transform_tool(self.tool) {
            return self.xf.click(w, p);
        }
        match self.tool {
            ModelerTool::Line => self.click_line(w, p),
            ModelerTool::Rectangle => self.click_rect(w, p),
            ModelerTool::Circle => self.click_circle(w, p),
            ModelerTool::Arc => self.click_arc(w, p),
            ModelerTool::PushPull => self.click_pull(w, p),
            _ => Out::default(),
        }
    }

    fn click_line(&mut self, w: &World, p: &Pointer) -> Out {
        let pt = p.inf.point;
        let hit = Modeling::hit_model(w, p);
        let Run::Line { pts, hit: first } = &mut self.run else {
            self.last = None;
            self.run = Run::Line { pts: vec![pt], hit };
            self.cursor = Some(pt);
            return Out::default();
        };
        let last = *pts.last().expect("a line has a point");
        if pt.distance(last) < MIN_LEN {
            return Out::default();
        }
        let closes = pts.len() >= 2 && pt.distance(pts[0]) < MIN_LEN * 10.0;
        let hit = first.or(hit);
        pts.push(pt);
        let out = Out::act(Action::AddEdges {
            target: self.target(w, hit, p.selected),
            segments: vec![[last, pt]],
        });
        if closes {
            self.run = Run::Idle;
        }
        out
    }

    fn click_rect(&mut self, w: &World, p: &Pointer) -> Out {
        match self.run.clone() {
            Run::Rect { a, plane, hit } => {
                let b = Modeling::resolve(p, Some(&plane));
                let d = plane.coords(b) - plane.coords(a);
                self.run = Run::Idle;
                self.finish_rect(w, p.selected, a, plane, d.x, d.y, hit, false)
            }
            _ => {
                self.last = None;
                let at = p.inf.point;
                let plane = Modeling::plane_for(w, p, at);
                self.run = Run::Rect {
                    a: at,
                    plane,
                    hit: Modeling::hit_model(w, p),
                };
                self.cursor = Some(at);
                Out::default()
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_rect(
        &mut self,
        w: &World,
        selected: Option<ObjectId>,
        a: DVec3,
        plane: Plane3,
        wd: f64,
        ht: f64,
        hit: Option<ObjectId>,
        redo: bool,
    ) -> Out {
        if wd.abs() < MIN_LEN || ht.abs() < MIN_LEN {
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        let c = [
            a,
            plane.at(a, wd, 0.0),
            plane.at(a, wd, ht),
            plane.at(a, 0.0, ht),
        ];
        self.pending = Some(Last::Rect {
            a,
            plane,
            w: wd,
            h: ht,
            hit,
        });
        Out {
            actions: vec![Action::AddEdges {
                target: self.target(w, hit, selected),
                segments: (0..4).map(|i| [c[i], c[(i + 1) % 4]]).collect(),
            }],
            redo,
            message: None,
        }
    }

    fn click_circle(&mut self, w: &World, p: &Pointer) -> Out {
        match self.run.clone() {
            Run::Circle { c, plane, hit } => {
                let b = Modeling::resolve(p, Some(&plane));
                let d = plane.coords(b) - plane.coords(c);
                self.run = Run::Idle;
                self.finish_circle(
                    w,
                    p.selected,
                    c,
                    plane,
                    d.length(),
                    d.y.atan2(d.x),
                    hit,
                    false,
                )
            }
            _ => {
                self.last = None;
                let at = p.inf.point;
                let plane = Modeling::plane_for(w, p, at);
                self.run = Run::Circle {
                    c: at,
                    plane,
                    hit: Modeling::hit_model(w, p),
                };
                self.cursor = Some(at);
                Out::default()
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_circle(
        &mut self,
        w: &World,
        selected: Option<ObjectId>,
        c: DVec3,
        plane: Plane3,
        r: f64,
        theta: f64,
        hit: Option<ObjectId>,
        redo: bool,
    ) -> Out {
        if r < MIN_LEN {
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        self.pending = Some(Last::Circle {
            c,
            plane,
            r,
            theta,
            hit,
        });
        Out {
            actions: vec![Action::AddCurve {
                target: self.target(w, hit, selected),
                points: circle_points(c, &plane, r, theta, self.sides),
                closed: true,
            }],
            redo,
            message: None,
        }
    }

    fn click_arc(&mut self, w: &World, p: &Pointer) -> Out {
        match self.run.clone() {
            Run::Arc {
                mut pts,
                plane,
                hit,
            } => {
                let b = Modeling::resolve(p, Some(&plane));
                if pts.len() == 1 {
                    if b.distance(pts[0]) < MIN_LEN {
                        return Out::default();
                    }
                    pts.push(b);
                    self.run = Run::Arc { pts, plane, hit };
                    return Out::default();
                }
                let h = bulge_of(pts[0], pts[1], b, &plane);
                self.run = Run::Idle;
                self.finish_arc(w, p.selected, pts[0], pts[1], plane, h, hit, false)
            }
            _ => {
                self.last = None;
                let at = p.inf.point;
                let plane = Modeling::plane_for(w, p, at);
                self.run = Run::Arc {
                    pts: vec![at],
                    plane,
                    hit: Modeling::hit_model(w, p),
                };
                self.cursor = Some(at);
                Out::default()
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_arc(
        &mut self,
        w: &World,
        selected: Option<ObjectId>,
        a: DVec3,
        b: DVec3,
        plane: Plane3,
        h: f64,
        hit: Option<ObjectId>,
        redo: bool,
    ) -> Out {
        if h.abs() < MIN_LEN {
            // No bulge: a straight line.
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        self.pending = Some(Last::Arc {
            a,
            b,
            plane,
            h,
            hit,
        });
        Out {
            actions: vec![Action::AddCurve {
                target: self.target(w, hit, selected),
                points: arc_points(a, b, h, &plane, self.arc_sides),
                closed: false,
            }],
            redo,
            message: None,
        }
    }

    fn click_pull(&mut self, w: &World, p: &Pointer) -> Out {
        if let Run::Pull {
            model,
            face,
            distance,
            ..
        } = self.run.clone()
        {
            self.run = Run::Idle;
            return self.finish_pull(model, face, distance, false);
        }
        self.last = None;
        if p.inf.source.kind() != TagKind::Face {
            return Out::msg(tr!("uic-modeling-pick-face"));
        }
        let Some((model, _)) = w.model(p.inf.source) else {
            return Out::default();
        };
        let face = FaceId(p.inf.source.id());
        let Some(pl) = w.face_plane(p.inf.source) else {
            return Out::default();
        };
        self.run = Run::Pull {
            model,
            face,
            base: p.inf.point,
            normal: pl.n,
            loops: w.face_loops(model, face),
            edges: super::select::face_edges(w.doc, model, face),
            distance: 0.0,
        };
        self.cursor = Some(p.inf.point);
        Out::default()
    }

    fn finish_pull(&mut self, model: ObjectId, face: FaceId, d: f64, redo: bool) -> Out {
        if d.abs() < MIN_LEN {
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        self.last_pull = Some(d);
        self.pending = Some(Last::Pull {
            model,
            face,
            distance: d,
        });
        Out {
            actions: vec![Action::PushPull {
                model,
                face,
                distance: d,
            }],
            redo,
            message: None,
        }
    }

    /// A double click with Push/Pull on a face: the face moves by the distance of the last
    /// Push/Pull.
    pub fn double_click(&mut self, w: &World, p: &Pointer) -> Out {
        if self.tool != ModelerTool::PushPull || p.inf.source.kind() != TagKind::Face {
            return Out::default();
        }
        let (Some(d), Some((model, _))) = (self.last_pull, w.model(p.inf.source)) else {
            return Out::default();
        };
        self.run = Run::Idle;
        self.finish_pull(model, FaceId(p.inf.source.id()), d, false)
    }

    // ---- typed values ------------------------------------------------------------------------

    /// Enter: reads the text of the box and applies it. `revision` is the revision of the
    /// document now: a step is only redone when nothing happened since.
    pub fn enter(&mut self, w: &World, selected: Option<ObjectId>, revision: u64) -> Out {
        if self.typed.trim().is_empty() {
            return Out::default();
        }
        let units = kadakas_units::current();
        let text = std::mem::take(&mut self.typed);
        match measure_box::parse(&text, &units, mb_tool(self.tool), self.phase()) {
            Ok(v) => self.apply(w, selected, revision, v),
            Err(e) => {
                self.typed = text;
                Out::msg(e.message())
            }
        }
    }

    /// The actions were refused and the step that was taken back for them is back: the revision
    /// moved on, but the document is as it was, so the step can still be redone with a new value.
    pub fn refused(&mut self, revision: u64) {
        self.xf.refused(revision);
        if let Some((l, _)) = self.last {
            self.last = Some((l, revision));
        }
    }

    fn can_redo(&self, revision: u64) -> Option<Last> {
        self.last.filter(|(_, r)| *r == revision).map(|(l, _)| l)
    }

    pub(super) fn need_start() -> Out {
        Out::msg(tr!("uic-modeling-need-start"))
    }

    fn apply(&mut self, w: &World, selected: Option<ObjectId>, revision: u64, v: Value) -> Out {
        if is_transform_tool(self.tool) {
            return self.xf.apply(v, revision);
        }
        if let Value::Sides(n) = v {
            return self.set_sides(w, selected, revision, n);
        }
        match (self.tool, self.run.clone(), v) {
            (ModelerTool::Line, Run::Line { pts, hit }, v) => {
                let last = *pts.last().expect("a line has a point");
                let cur = self.cursor.unwrap_or(last + DVec3::X);
                let end = match v {
                    Value::Length(l) => {
                        let dir = (cur - last).try_normalize().unwrap_or(DVec3::X);
                        last + dir * l
                    }
                    Value::PointAbs(q) => q,
                    Value::PointRel(d) => last + d,
                    _ => return Out::default(),
                };
                if end.distance(last) < MIN_LEN {
                    return Out::msg(tr!("uic-modeling-too-small"));
                }
                let closes = pts.len() >= 2 && end.distance(pts[0]) < MIN_LEN * 10.0;
                let mut pts = pts;
                pts.push(end);
                self.run = if closes {
                    Run::Idle
                } else {
                    Run::Line { pts, hit }
                };
                Out::act(Action::AddEdges {
                    target: self.target(w, hit, selected),
                    segments: vec![[last, end]],
                })
            }
            (ModelerTool::Line, _, _) => Modeling::need_start(),
            (ModelerTool::Rectangle, Run::Rect { a, plane, hit }, Value::Pair(pw, ph)) => {
                let d = self.cursor.map_or(glam::DVec2::new(1.0, 1.0), |c| {
                    plane.coords(c) - plane.coords(a)
                });
                let sx = if d.x < 0.0 { -1.0 } else { 1.0 };
                let sy = if d.y < 0.0 { -1.0 } else { 1.0 };
                let wd = pw.map_or(d.x, |x| if x < 0.0 { x } else { x * sx });
                let ht = ph.map_or(d.y, |x| if x < 0.0 { x } else { x * sy });
                self.run = Run::Idle;
                self.finish_rect(w, selected, a, plane, wd, ht, hit, false)
            }
            (ModelerTool::Rectangle, Run::Idle, Value::Pair(pw, ph)) => {
                match self.can_redo(revision) {
                    Some(Last::Rect {
                        a,
                        plane,
                        w: ow,
                        h: oh,
                        hit,
                    }) => {
                        let wd = pw.map_or(ow, |x| if x < 0.0 { x } else { x * ow.signum() });
                        let ht = ph.map_or(oh, |x| if x < 0.0 { x } else { x * oh.signum() });
                        self.finish_rect(w, selected, a, plane, wd, ht, hit, true)
                    }
                    _ => Modeling::need_start(),
                }
            }
            (ModelerTool::Circle, Run::Circle { c, plane, hit }, Value::Length(r)) => {
                let theta = self.cursor.map_or(0.0, |q| {
                    let d = plane.coords(q) - plane.coords(c);
                    d.y.atan2(d.x)
                });
                self.run = Run::Idle;
                self.finish_circle(w, selected, c, plane, r, theta, hit, false)
            }
            (ModelerTool::Circle, Run::Idle, Value::Length(r) | Value::Radius(r)) => {
                match self.can_redo(revision) {
                    Some(Last::Circle {
                        c,
                        plane,
                        theta,
                        hit,
                        ..
                    }) => self.finish_circle(w, selected, c, plane, r, theta, hit, true),
                    _ => Modeling::need_start(),
                }
            }
            (
                ModelerTool::Arc,
                Run::Arc {
                    mut pts,
                    plane,
                    hit,
                },
                Value::Length(l),
            ) if pts.len() == 1 => {
                let cur = self.cursor.unwrap_or(pts[0] + plane.u);
                let dir = (cur - pts[0]).try_normalize().unwrap_or(plane.u);
                pts.push(pts[0] + dir * l);
                self.run = Run::Arc { pts, plane, hit };
                Out::default()
            }
            (ModelerTool::Arc, Run::Arc { pts, plane, hit }, v) if pts.len() == 2 => {
                let chord = pts[0].distance(pts[1]);
                let side = self
                    .cursor
                    .map_or(1.0, |c| bulge_of(pts[0], pts[1], c, &plane).signum());
                let h = match v {
                    Value::Length(l) => l.abs() * side,
                    Value::Radius(r) => match radius_to_bulge(chord, r) {
                        Some(h) => h * side,
                        None => return Out::msg(tr!("uic-modeling-radius-small")),
                    },
                    _ => return Out::default(),
                };
                self.run = Run::Idle;
                self.finish_arc(w, selected, pts[0], pts[1], plane, h, hit, false)
            }
            (ModelerTool::Arc, Run::Idle, v) => match (self.can_redo(revision), v) {
                (
                    Some(Last::Arc {
                        a,
                        b,
                        plane,
                        h,
                        hit,
                    }),
                    Value::Length(l),
                ) => self.finish_arc(w, selected, a, b, plane, l.abs() * h.signum(), hit, true),
                (
                    Some(Last::Arc {
                        a,
                        b,
                        plane,
                        h,
                        hit,
                    }),
                    Value::Radius(r),
                ) => match radius_to_bulge(a.distance(b), r) {
                    Some(nh) => {
                        self.finish_arc(w, selected, a, b, plane, nh * h.signum(), hit, true)
                    }
                    None => Out::msg(tr!("uic-modeling-radius-small")),
                },
                _ => Modeling::need_start(),
            },
            (ModelerTool::PushPull, Run::Pull { model, face, .. }, Value::Length(d)) => {
                self.run = Run::Idle;
                self.finish_pull(model, face, d, false)
            }
            (ModelerTool::PushPull, Run::Idle, Value::Length(d)) => match self.can_redo(revision) {
                Some(Last::Pull { model, face, .. }) => self.finish_pull(model, face, d, true),
                _ => Modeling::need_start(),
            },
            _ => Modeling::need_start(),
        }
    }

    fn set_sides(&mut self, w: &World, selected: Option<ObjectId>, revision: u64, n: u32) -> Out {
        match self.tool {
            ModelerTool::Circle => {
                self.sides = n;
                if matches!(self.run, Run::Idle)
                    && let Some(Last::Circle {
                        c,
                        plane,
                        r,
                        theta,
                        hit,
                    }) = self.can_redo(revision)
                {
                    return self.finish_circle(w, selected, c, plane, r, theta, hit, true);
                }
            }
            ModelerTool::Arc => {
                self.arc_sides = n;
                if matches!(self.run, Run::Idle)
                    && let Some(Last::Arc {
                        a,
                        b,
                        plane,
                        h,
                        hit,
                    }) = self.can_redo(revision)
                {
                    return self.finish_arc(w, selected, a, b, plane, h, hit, true);
                }
            }
            _ => {}
        }
        Out::default()
    }

    // ---- drawing -----------------------------------------------------------------------------

    /// What to draw now.
    pub fn preview(&self, w: &World) -> Preview {
        let mut pv = Preview::default();
        if is_transform_tool(self.tool) {
            self.xf.preview(w, &mut pv);
            return pv;
        }
        let cur = self.cursor;
        match &self.run {
            Run::Line { pts, .. } => {
                if let (Some(l), Some(c)) = (pts.last(), cur) {
                    pv.shape.push([*l, c]);
                }
            }
            Run::Rect { a, plane, .. } => {
                if let Some(c) = cur {
                    let d = plane.coords(c) - plane.coords(*a);
                    let q = [
                        *a,
                        plane.at(*a, d.x, 0.0),
                        plane.at(*a, d.x, d.y),
                        plane.at(*a, 0.0, d.y),
                    ];
                    pv.shape = (0..4).map(|i| [q[i], q[(i + 1) % 4]]).collect();
                }
            }
            Run::Circle { c, plane, .. } => {
                if let Some(q) = cur {
                    let d = plane.coords(q) - plane.coords(*c);
                    let pts = circle_points(*c, plane, d.length(), d.y.atan2(d.x), self.sides);
                    pv.shape = ring(&pts, true);
                }
            }
            Run::Arc { pts, plane, .. } => match (pts.as_slice(), cur) {
                ([a], Some(c)) => pv.shape.push([*a, c]),
                ([a, b], Some(c)) => {
                    let h = bulge_of(*a, *b, c, plane);
                    if h.abs() >= MIN_LEN {
                        pv.shape = ring(&arc_points(*a, *b, h, plane, self.arc_sides), false);
                    } else {
                        pv.shape.push([*a, *b]);
                    }
                }
                _ => {}
            },
            Run::Erase { marked, .. } => {
                for (m, e) in marked {
                    if let Some(s) = w.edge_ends(*m, *e) {
                        pv.erase.push(s);
                    }
                }
            }
            Run::Pull {
                normal,
                loops,
                distance,
                ..
            } => {
                let off = *normal * *distance;
                for l in loops {
                    for i in 0..l.len() {
                        let (a, b) = (l[i], l[(i + 1) % l.len()]);
                        pv.pull.push([a + off, b + off]);
                        pv.pull.push([a, a + off]);
                    }
                }
            }
            Run::Idle => {}
        }
        if self.tool == ModelerTool::Eraser
            && let Some((m, e)) = self.hover_edge
        {
            for x in w.with_curve(m, e) {
                if let Some(s) = w.edge_ends(m, x)
                    && !pv.erase.contains(&s)
                {
                    pv.erase.push(s);
                }
            }
        }
        pv
    }

    /// The color for the shape that is being drawn.
    pub const SHAPE_COLOR: Rgb = Rgb(0x20, 0x20, 0x20);
}

/// The Delete key on selected faces and edges: one action for each model.
pub fn delete_actions(sel: &SubSelection) -> Vec<Action> {
    let mut by_model: BTreeMap<ObjectId, (Vec<FaceId>, Vec<EdgeId>)> = BTreeMap::new();
    for (m, f) in sel.faces() {
        by_model.entry(m).or_default().0.push(f);
    }
    for (m, e) in sel.edges() {
        by_model.entry(m).or_default().1.push(e);
    }
    by_model
        .into_iter()
        .map(|(model, (faces, edges))| Action::Delete {
            model,
            faces,
            edges,
        })
        .collect()
}

/// Intersect Faces on the selection: with `Selection` one action for each model with selected
/// faces or edges (those against the rest of the model); with `Model` one action for each model
/// that has a selected part or is in `models` (the whole model).
pub fn intersect_actions(
    sel: &SubSelection,
    scope: IntersectScope,
    models: &[ObjectId],
) -> Vec<Action> {
    let mut by_model: BTreeMap<ObjectId, (Vec<FaceId>, Vec<EdgeId>)> = BTreeMap::new();
    for (m, f) in sel.faces() {
        by_model.entry(m).or_default().0.push(f);
    }
    for (m, e) in sel.edges() {
        by_model.entry(m).or_default().1.push(e);
    }
    if scope == IntersectScope::Model {
        for m in models {
            by_model.entry(*m).or_default();
        }
    }
    by_model
        .into_iter()
        .map(|(model, (faces, edges))| match scope {
            IntersectScope::Model => Action::Intersect {
                model,
                scope,
                faces: Vec::new(),
                edges: Vec::new(),
            },
            IntersectScope::Selection => Action::Intersect {
                model,
                scope,
                faces,
                edges,
            },
        })
        .collect()
}

fn ring(pts: &[DVec3], closed: bool) -> Vec<[DVec3; 2]> {
    let n = pts.len();
    let count = if closed { n } else { n.saturating_sub(1) };
    (0..count).map(|i| [pts[i], pts[(i + 1) % n]]).collect()
}

/// The points of a circle with `sides` corners; the first corner is at the angle `theta`.
pub fn circle_points(c: DVec3, plane: &Plane3, r: f64, theta: f64, sides: u32) -> Vec<DVec3> {
    let n = sides.max(3);
    (0..n)
        .map(|i| {
            let a = theta + std::f64::consts::TAU * f64::from(i) / f64::from(n);
            plane.at(c, r * a.cos(), r * a.sin())
        })
        .collect()
}

/// The height of the arc over the chord `a`–`b` that passes through `through`, with a sign: the
/// side of the chord in the plane (positive to the left of the chord seen from the front).
pub fn bulge_of(a: DVec3, b: DVec3, through: DVec3, plane: &Plane3) -> f64 {
    let Some(d) = (b - a).try_normalize() else {
        return 0.0;
    };
    let perp = plane.n.cross(d);
    let mid = (a + b) / 2.0;
    (through - mid).dot(perp)
}

/// The bulge of the arc with chord `chord` and radius `r`, the smaller arc. `None` when the
/// radius is smaller than half the chord.
pub fn radius_to_bulge(chord: f64, r: f64) -> Option<f64> {
    let r = r.abs();
    let half = chord / 2.0;
    (r >= half && half > 0.0).then(|| r - (r * r - half * half).sqrt())
}

/// The corners of the arc over the chord `a`–`b` with the bulge `h` (signed, see [`bulge_of`]),
/// in `segments` pieces. The first corner is `a` and the last is `b`.
pub fn arc_points(a: DVec3, b: DVec3, h: f64, plane: &Plane3, segments: u32) -> Vec<DVec3> {
    let segments = segments.max(1);
    let Some(d) = (b - a).try_normalize() else {
        return vec![a, b];
    };
    let perp = plane.n.cross(d);
    let mid = (a + b) / 2.0;
    let top = mid + perp * h;
    let (pa, pt, pb) = (plane.coords(a), plane.coords(top), plane.coords(b));
    // The center of the circle through the three points, in the axes of the plane.
    let den = 2.0 * (pa.x * (pt.y - pb.y) + pt.x * (pb.y - pa.y) + pb.x * (pa.y - pt.y));
    if den.abs() < 1e-12 {
        return vec![a, b];
    }
    let f = |p: glam::DVec2| p.length_squared();
    let cx = (f(pa) * (pt.y - pb.y) + f(pt) * (pb.y - pa.y) + f(pb) * (pa.y - pt.y)) / den;
    let cy = (f(pa) * (pb.x - pt.x) + f(pt) * (pa.x - pb.x) + f(pb) * (pt.x - pa.x)) / den;
    let c = glam::DVec2::new(cx, cy);
    let r = (pa - c).length();
    let ang = |p: glam::DVec2| (p.y - c.y).atan2(p.x - c.x);
    let tau = std::f64::consts::TAU;
    let norm = |x: f64| x.rem_euclid(tau);
    let (a0, a1, at) = (ang(pa), ang(pb), ang(pt));
    let ccw = norm(a1 - a0);
    let sweep = if norm(at - a0) < ccw { ccw } else { ccw - tau };
    let mut out = Vec::with_capacity(segments as usize + 1);
    out.push(a);
    for i in 1..segments {
        let t = a0 + sweep * f64::from(i) / f64::from(segments);
        out.push(plane.at(plane.origin, c.x + r * t.cos(), c.y + r * t.sin()));
    }
    out.push(b);
    out
}

/// The parameter `t` of the point `base + normal * t` of the line that is nearest to the ray.
pub(super) fn closest_on_line(ray: &Ray, base: DVec3, normal: DVec3) -> Option<f64> {
    let (u, v) = (ray.dir, normal);
    let w0 = ray.origin - base;
    let (a, b, c) = (u.dot(u), u.dot(v), v.dot(v));
    let (d, e) = (u.dot(w0), v.dot(w0));
    let den = a * c - b * b;
    if den.abs() < 1e-9 {
        return None;
    }
    Some((a * e - b * d) / den)
}
