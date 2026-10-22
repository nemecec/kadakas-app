//! Move, Rotate and Scale (MOD-05): the state machines of the three tools, headless like the
//! drawing tools. They work on the selection (whole objects, or faces and edges of surface
//! models) and answer with one [`Action::Xform`] that runs as one undo step.
//!
//! # Move (M)
//!
//! Click a base point (the inference snaps, arrow keys and Shift lock a direction), move, click the
//! end. Typed values go through the Measurements box: a length along the current direction, three
//! lengths `x;y;z` or `<x,y,z>` relative, `[x,y,z]` absolute. Ctrl (Option on macOS) toggles copy;
//! just after a copy `5x` makes five copies at the same step and `/5` makes five copies that
//! divide the distance. A new value after the move redoes it (the step before is taken back).
//!
//! # Rotate (Q)
//!
//! A protractor: click the center, click the start direction, move to the angle and click. The
//! plane of the protractor is the plane of the face under the pointer, else the horizontal plane;
//! the arrow keys lock it to an axis (Up blue, Left green, Right red, Down free), Shift keeps the
//! plane that the pointer showed. The angle snaps to 15 degrees when it is within 3 degrees of a
//! tick. A typed angle (`45`, `-30`, `8:12`) ends the rotation; positive turns counterclockwise
//! seen from the side the normal of the protractor points to. Copy, `5x` and `/5` work as in Move
//! (each copy turned one step further).
//!
//! # Scale (S)
//!
//! Grips on the box around the selection: the 8 corners (uniform), the 12 edges (two axes) and the
//! 6 faces (one axis). Click a grip, move, click. Ctrl toggles scaling about the center, Shift
//! toggles uniform scaling. A typed factor (`2`), three factors (`1;1;2`) or a length (`2m`: the
//! length that the box gets along the grip) ends the scale.
//!
//! # Selection
//!
//! The window passes the selection with [`Context`] before each event. With nothing selected the
//! window picks the entity under the pointer first (as Select does), then calls the tool.

use std::collections::BTreeMap;

use glam::{DAffine3, DQuat, DVec3};
use kadakas_core::model::ObjectId;
use kadakas_core::surface::{EdgeId, FaceId};
use kadakas_l10n::tr;
use kadakas_units::Units;

use super::exec::Action;
use super::plane::Plane3;
use super::scenes::{World, frame_of, model_of};
use super::select::{Item, SubSelection};
use super::tools::{LiveLabel, MIN_LEN, Modeling, Out, Pointer, Preview, closest_on_line};
use crate::infer3d::{Constraint, Key, Ray, TagKind};
use crate::measure_box::Value;
use crate::modeler::ModelerTool;

/// The most segments of an outline in a preview.
const MAX_OUTLINE: usize = 6000;
/// The protractor snaps to this angle (radians) ...
const TICK: f64 = std::f64::consts::PI / 12.0;
/// ... when the angle is within this of a tick (radians).
const SNAP: f64 = 3.0 * std::f64::consts::PI / 180.0;
/// How near a pointer ray must pass to a grip, as a share of the distance to the grip.
const GRIP_REACH: f64 = 0.012;
/// Smallest scale factor (it keeps the sign).
const MIN_FACTOR: f64 = 1e-3;

/// What the window tells the tools about the selection and the keys, before each event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    /// The selected objects.
    pub objects: Vec<ObjectId>,
    /// The selected faces and edges.
    pub parts: SubSelection,
    /// The box around the selected objects in the world; `None` for surface models, which the
    /// tool measures itself.
    pub object_bounds: Option<(DVec3, DVec3)>,
    /// Shift is down.
    pub shift: bool,
}

/// What the tool works on: whole objects, or faces and edges of surface models.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Subject {
    objects: Vec<ObjectId>,
    parts: BTreeMap<ObjectId, (Vec<FaceId>, Vec<EdgeId>)>,
    bounds: Option<(DVec3, DVec3)>,
}

impl Subject {
    /// The subject of a selection; `None` when nothing that exists is selected.
    pub fn resolve(w: &World, ctx: &Context) -> Option<Subject> {
        let objects: Vec<ObjectId> = ctx
            .objects
            .iter()
            .copied()
            .filter(|id| w.doc.object(*id).is_some())
            .collect();
        let mut parts: BTreeMap<ObjectId, (Vec<FaceId>, Vec<EdgeId>)> = BTreeMap::new();
        for item in ctx.parts.items() {
            match item {
                Item::Face(m, f) => {
                    if model_of(w.doc, m).is_some_and(|s| s.face(f).is_some()) {
                        parts.entry(m).or_default().0.push(f);
                    }
                }
                Item::Edge(m, e) => {
                    if model_of(w.doc, m).is_some_and(|s| s.edge(e).is_some()) {
                        parts.entry(m).or_default().1.push(e);
                    }
                }
            }
        }
        if objects.is_empty() && parts.is_empty() {
            return None;
        }
        let mut s = Subject {
            objects,
            parts,
            bounds: None,
        };
        s.bounds = s.measure(w, ctx.object_bounds);
        Some(s)
    }

    fn measure(&self, w: &World, objects: Option<(DVec3, DVec3)>) -> Option<(DVec3, DVec3)> {
        let mut b: Option<(DVec3, DVec3)> = None;
        let mut add = |p: DVec3| {
            b = Some(match b {
                None => (p, p),
                Some((lo, hi)) => (lo.min(p), hi.max(p)),
            });
        };
        for (m, (faces, edges)) in &self.parts {
            let Some(model) = model_of(w.doc, *m) else {
                continue;
            };
            let fr = frame_of(w.doc, *m);
            for f in faces {
                if let Some(face) = model.face(*f) {
                    for v in face.loops().flatten() {
                        if let Some(p) = model.vertex(*v) {
                            add(fr.transform_point3(p));
                        }
                    }
                }
            }
            for e in edges {
                if let Some(edge) = model.edge(*e) {
                    for v in edge.v {
                        if let Some(p) = model.vertex(v) {
                            add(fr.transform_point3(p));
                        }
                    }
                }
            }
        }
        // Surface models are measured here; the window gives the box of the other objects.
        for id in &self.objects {
            if let Some(model) = model_of(w.doc, *id) {
                let fr = frame_of(w.doc, *id);
                for (_, p) in model.vertices() {
                    add(fr.transform_point3(p));
                }
            }
        }
        if let Some((lo, hi)) = objects {
            add(lo);
            add(hi);
        }
        b
    }

    /// The segments to draw for the subject, in the world.
    fn outline(&self, w: &World) -> Vec<[DVec3; 2]> {
        let mut out: Vec<[DVec3; 2]> = Vec::new();
        for (m, (faces, edges)) in &self.parts {
            let Some(model) = model_of(w.doc, *m) else {
                continue;
            };
            let fr = frame_of(w.doc, *m);
            let at = |v| model.vertex(v).map(|p| fr.transform_point3(p));
            for f in faces {
                if let Some(face) = model.face(*f) {
                    for (a, b) in face.uses() {
                        if let (Some(p), Some(q)) = (at(a), at(b)) {
                            out.push([p, q]);
                        }
                    }
                }
            }
            for e in edges {
                if let Some(edge) = model.edge(*e)
                    && let (Some(p), Some(q)) = (at(edge.v[0]), at(edge.v[1]))
                {
                    out.push([p, q]);
                }
            }
        }
        for id in &self.objects {
            if out.len() >= MAX_OUTLINE {
                break;
            }
            if let Some(model) = model_of(w.doc, *id)
                && model.edge_count() + out.len() <= MAX_OUTLINE
            {
                let fr = frame_of(w.doc, *id);
                for (_, e) in model.edges() {
                    if let (Some(p), Some(q)) = (model.vertex(e.v[0]), model.vertex(e.v[1])) {
                        out.push([fr.transform_point3(p), fr.transform_point3(q)]);
                    }
                }
            } else if let Some((lo, hi)) = self.bounds {
                out.extend(box_edges(lo, hi));
            }
        }
        out.truncate(MAX_OUTLINE);
        out
    }

    /// The action that applies `matrix` (world) to the subject, in place or as `copies` copies.
    fn action(&self, matrix: DAffine3, copies: u32) -> Action {
        Action::Xform {
            objects: self.objects.clone(),
            parts: self
                .parts
                .iter()
                .map(|(m, (f, e))| (*m, f.clone(), e.clone()))
                .collect(),
            matrix,
            copies,
        }
    }

    /// The models whose geometry moves, for the inference to skip.
    pub fn models(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.objects.iter().chain(self.parts.keys()).copied()
    }

    /// The faces and edges that move (parts only).
    pub fn parts_moving(&self) -> impl Iterator<Item = (ObjectId, &[FaceId], &[EdgeId])> + '_ {
        self.parts
            .iter()
            .map(|(m, (f, e))| (*m, f.as_slice(), e.as_slice()))
    }

    /// The objects that move whole.
    pub fn objects(&self) -> &[ObjectId] {
        &self.objects
    }
}

fn box_edges(lo: DVec3, hi: DVec3) -> Vec<[DVec3; 2]> {
    let c = |i: usize| {
        DVec3::new(
            if i & 1 == 1 { hi.x } else { lo.x },
            if i & 2 == 2 { hi.y } else { lo.y },
            if i & 4 == 4 { hi.z } else { lo.z },
        )
    };
    let mut v = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                v.push([c(i), c(i | bit)]);
            }
        }
    }
    v
}

fn rotation(c: DVec3, n: DVec3, angle: f64) -> DAffine3 {
    let q = DQuat::from_axis_angle(n, angle);
    DAffine3::from_rotation_translation(q, c - q * c)
}

fn scaling(anchor: DVec3, f: DVec3) -> DAffine3 {
    DAffine3::from_translation(anchor)
        * DAffine3::from_scale(f)
        * DAffine3::from_translation(-anchor)
}

/// A grip on the box of the selection: for each axis -1 (the low side), 0 (the middle) or 1 (the
/// high side). Not all zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grip(pub [i8; 3]);

impl Grip {
    /// The 26 grips: the 8 corners, the 12 edges and the 6 faces.
    pub fn all() -> Vec<Grip> {
        let mut v = Vec::new();
        for x in -1..=1i8 {
            for y in -1..=1i8 {
                for z in -1..=1i8 {
                    if (x, y, z) != (0, 0, 0) {
                        v.push(Grip([x, y, z]));
                    }
                }
            }
        }
        v
    }

    /// How many axes the grip scales: 3 for a corner, 2 for an edge, 1 for a face.
    pub fn axes(&self) -> usize {
        self.0.iter().filter(|s| **s != 0).count()
    }

    fn point(&self, lo: DVec3, hi: DVec3) -> DVec3 {
        let pick = |s: i8, l: f64, h: f64| match s {
            -1 => l,
            1 => h,
            _ => (l + h) / 2.0,
        };
        DVec3::new(
            pick(self.0[0], lo.x, hi.x),
            pick(self.0[1], lo.y, hi.y),
            pick(self.0[2], lo.z, hi.z),
        )
    }

    /// The point that stays: the opposite side of the box, or the center.
    fn anchor(&self, lo: DVec3, hi: DVec3, about_center: bool) -> DVec3 {
        let center = (lo + hi) / 2.0;
        if about_center {
            return center;
        }
        let pick = |s: i8, l: f64, h: f64, c: f64| match s {
            -1 => h,
            1 => l,
            _ => c,
        };
        DVec3::new(
            pick(self.0[0], lo.x, hi.x, center.x),
            pick(self.0[1], lo.y, hi.y, center.y),
            pick(self.0[2], lo.z, hi.z, center.z),
        )
    }

    fn uses(&self, axis: usize) -> bool {
        self.0[axis] != 0
    }
}

/// Which grips can be used: those that scale only axes along which the box has a size.
pub fn usable_grips(lo: DVec3, hi: DVec3) -> Vec<Grip> {
    let size = hi - lo;
    let eps = 1e-6_f64.max(size.max_element() * 1e-9);
    Grip::all()
        .into_iter()
        .filter(|g| (0..3).all(|a| !g.uses(a) || size[a] > eps))
        .collect()
}

#[derive(Clone, Debug, Default)]
enum XRun {
    #[default]
    Idle,
    Move {
        subject: Subject,
        base: DVec3,
    },
    /// The center of the protractor is clicked.
    Center {
        subject: Subject,
        plane: Plane3,
    },
    /// The start direction is clicked; the angle follows the pointer.
    Angle {
        subject: Subject,
        plane: Plane3,
        start: f64,
        radius: f64,
        angle: f64,
    },
    Scale {
        subject: Subject,
        grip: Grip,
        bounds: (DVec3, DVec3),
        factors: DVec3,
        uniform: bool,
    },
}

/// What was done, to redo it with a new value.
#[derive(Clone, Debug)]
struct Last {
    subject: Subject,
    kind: LastKind,
    copy: bool,
    /// Copies made (at least 1 for a copy).
    count: u32,
    /// The value was divided: the step is the whole divided by `count`.
    divided: bool,
}

#[derive(Clone, Debug)]
enum LastKind {
    /// `d` is one step.
    Move { base: DVec3, d: DVec3 },
    Rotate { plane: Plane3, angle: f64 },
    Scale {
        grip: Grip,
        bounds: (DVec3, DVec3),
        about_center: bool,
        uniform: bool,
    },
}

/// The state of Move, Rotate and Scale.
#[derive(Debug, Default)]
pub struct Xform {
    tool: Option<ModelerTool>,
    run: XRun,
    ctx: Context,
    cursor: Option<DVec3>,
    /// Ctrl: copy (Move, Rotate) or about the center (Scale).
    pub ctrl: bool,
    /// The axis that the protractor is locked to by an arrow key.
    lock: Option<DVec3>,
    /// The normal that the pointer shows for the protractor.
    hover_normal: DVec3,
    hover_center: Option<DVec3>,
    hover_grip: Option<Grip>,
    pending: Option<Last>,
    last: Option<(Last, u64)>,
}

/// True for the tools of this module.
pub fn is_transform_tool(t: ModelerTool) -> bool {
    matches!(
        t,
        ModelerTool::Move | ModelerTool::Rotate | ModelerTool::Scale
    )
}

fn text_len(units: &Units, v: f64) -> String {
    units.length_text(v)
}

impl Xform {
    pub fn choose(&mut self, tool: ModelerTool) {
        *self = Xform {
            tool: Some(tool),
            ctx: std::mem::take(&mut self.ctx),
            ..Xform::default()
        };
    }

    pub fn abort(&mut self) {
        self.run = XRun::Idle;
        self.cursor = None;
    }

    pub fn in_progress(&self) -> bool {
        !matches!(self.run, XRun::Idle)
    }

    /// Ends what is in progress. True if something was.
    pub fn cancel(&mut self) -> bool {
        let was = self.in_progress();
        self.abort();
        was
    }

    pub fn set_context(&mut self, ctx: Context) {
        self.ctx = ctx;
    }

    /// Ctrl was pressed: copy (Move, Rotate) or about the center (Scale) is toggled.
    pub fn toggle_ctrl(&mut self) {
        self.ctrl = !self.ctrl;
    }

    /// True while Move or Rotate is in copy mode.
    pub fn copying(&self) -> bool {
        self.ctrl && matches!(self.tool, Some(ModelerTool::Move | ModelerTool::Rotate))
    }

    /// The point the inference works from.
    pub fn reference(&self) -> Option<DVec3> {
        match &self.run {
            XRun::Move { base, .. } => Some(*base),
            XRun::Center { plane, .. } | XRun::Angle { plane, .. } => Some(plane.origin),
            _ => None,
        }
    }

    /// The subject that moves now, for the inference to skip.
    pub fn moving(&self) -> Option<&Subject> {
        match &self.run {
            XRun::Move { subject, .. }
            | XRun::Center { subject, .. }
            | XRun::Angle { subject, .. }
            | XRun::Scale { subject, .. } => Some(subject),
            XRun::Idle => None,
        }
    }

    /// An arrow key before the center of the protractor is clicked: locks its plane to an axis
    /// (Up blue, Left green, Right red) or frees it (Down). True if used.
    pub fn arrow(&mut self, key: Key) -> bool {
        if self.tool != Some(ModelerTool::Rotate) || !matches!(self.run, XRun::Idle) {
            return false;
        }
        self.lock = match key {
            Key::Up => Some(DVec3::Z),
            Key::Left => Some(DVec3::Y),
            Key::Right => Some(DVec3::X),
            Key::Down => None,
            _ => return false,
        };
        if let Some(n) = self.lock {
            self.hover_normal = n;
        }
        true
    }

    pub fn phase(&self) -> crate::measure_box::Phase {
        use crate::measure_box::Phase;
        match &self.run {
            XRun::Idle => match &self.last {
                Some((l, _)) if l.copy => Phase::Copied,
                Some(_) => Phase::Done,
                None => Phase::Step(0),
            },
            XRun::Move { .. } | XRun::Center { .. } | XRun::Scale { .. } => Phase::Step(1),
            XRun::Angle { .. } => Phase::Step(2),
        }
    }

    /// What the Measurements box shows while the tool runs.
    pub fn live(&self, units: &Units) -> Option<(LiveLabel, String)> {
        match &self.run {
            XRun::Move { base, .. } => {
                let c = self.cursor?;
                Some((LiveLabel::Length, text_len(units, base.distance(c))))
            }
            XRun::Center { plane, .. } => {
                let c = self.cursor?;
                Some((LiveLabel::Length, text_len(units, plane.origin.distance(c))))
            }
            XRun::Angle { angle, .. } => Some((
                LiveLabel::Angle,
                format!("{:.1}°", angle.to_degrees()),
            )),
            XRun::Scale { factors, uniform, .. } => {
                let t = if *uniform || factors.x == factors.y && factors.y == factors.z {
                    format!("{:.2}", factors.x)
                } else {
                    format!("{:.2}; {:.2}; {:.2}", factors.x, factors.y, factors.z)
                };
                Some((LiveLabel::Factor, t))
            }
            XRun::Idle => None,
        }
    }

    // ---- the pointer ---------------------------------------------------------------------------

    pub fn moved(&mut self, w: &World, p: &Pointer) {
        match &self.run {
            XRun::Move { .. } => self.cursor = Some(p.inf.point),
            XRun::Center { plane, .. } => {
                let pl = *plane;
                self.cursor = Some(Modeling::resolve(p, Some(&pl)));
            }
            XRun::Angle { .. } => self.angle_to(p),
            XRun::Scale { .. } => self.scale_to(&p.ray),
            XRun::Idle => match self.tool {
                Some(ModelerTool::Rotate) => {
                    self.cursor = Some(p.inf.point);
                    self.hover_center = Some(p.inf.point);
                    // The pointer shows the plane; Shift keeps the plane it showed.
                    if !self.ctx.shift || self.hover_normal == DVec3::ZERO {
                        self.hover_normal = self.lock.unwrap_or_else(|| plane_normal_at(w, p));
                    }
                }
                Some(ModelerTool::Scale) => self.hover_grip = self.grip_at(w, &p.ray),
                _ => self.cursor = Some(p.inf.point),
            },
        }
    }

    pub fn click(&mut self, w: &World, p: &Pointer) -> Out {
        match self.tool {
            Some(ModelerTool::Move) => self.click_move(w, p),
            Some(ModelerTool::Rotate) => self.click_rotate(w, p),
            Some(ModelerTool::Scale) => self.click_scale(w, p),
            _ => Out::default(),
        }
    }

    // ---- Move ----------------------------------------------------------------------------------

    fn click_move(&mut self, w: &World, p: &Pointer) -> Out {
        match std::mem::take(&mut self.run) {
            XRun::Move { subject, base } => {
                let d = p.inf.point - base;
                if d.length() < MIN_LEN {
                    self.run = XRun::Move { subject, base };
                    return Out::msg(tr!("uic-modeling-too-small"));
                }
                self.finish_move(subject, base, d, 1, false, false)
            }
            _ => {
                self.last = None;
                let Some(subject) = Subject::resolve(w, &self.ctx) else {
                    return Out::msg(tr!("uic-modeling-select-first"));
                };
                self.run = XRun::Move {
                    subject,
                    base: p.inf.point,
                };
                self.cursor = Some(p.inf.point);
                Out::default()
            }
        }
    }

    /// Ends a move of `step` (one copy's distance). `count` copies when copying.
    fn finish_move(
        &mut self,
        subject: Subject,
        base: DVec3,
        step: DVec3,
        count: u32,
        divided: bool,
        redo: bool,
    ) -> Out {
        let copy = self.ctrl;
        let action = subject.action(
            DAffine3::from_translation(step),
            if copy { count } else { 0 },
        );
        self.pending = Some(Last {
            subject,
            kind: LastKind::Move { base, d: step },
            copy,
            count: if copy { count } else { 1 },
            divided: copy && divided,
        });
        self.cursor = None;
        Out {
            actions: vec![action],
            redo,
            message: None,
        }
    }

    // ---- Rotate --------------------------------------------------------------------------------

    fn click_rotate(&mut self, w: &World, p: &Pointer) -> Out {
        match std::mem::take(&mut self.run) {
            XRun::Idle => {
                self.last = None;
                let Some(subject) = Subject::resolve(w, &self.ctx) else {
                    return Out::msg(tr!("uic-modeling-select-first"));
                };
                let n = if self.hover_normal == DVec3::ZERO {
                    self.lock.unwrap_or_else(|| plane_normal_at(w, p))
                } else {
                    self.hover_normal
                };
                let Some(plane) = Plane3::through(p.inf.point, n) else {
                    return Out::default();
                };
                self.run = XRun::Center { subject, plane };
                self.cursor = Some(p.inf.point);
                Out::default()
            }
            XRun::Center { subject, plane } => {
                let q = Modeling::resolve(p, Some(&plane));
                let d = plane.coords(q);
                if d.length() < MIN_LEN {
                    self.run = XRun::Center { subject, plane };
                    return Out::msg(tr!("uic-modeling-too-small"));
                }
                self.run = XRun::Angle {
                    subject,
                    plane,
                    start: d.y.atan2(d.x),
                    radius: d.length(),
                    angle: 0.0,
                };
                self.cursor = Some(q);
                Out::default()
            }
            XRun::Angle {
                subject,
                plane,
                angle,
                ..
            } => {
                if angle.abs() < 1e-6 {
                    self.run = XRun::Idle;
                    return Out::msg(tr!("uic-modeling-too-small"));
                }
                self.finish_rotate(subject, plane, angle, 1, false, false)
            }
            other => {
                self.run = other;
                Out::default()
            }
        }
    }

    fn angle_to(&mut self, p: &Pointer) {
        let XRun::Angle {
            plane,
            start,
            angle,
            ..
        } = &mut self.run
        else {
            return;
        };
        let q = Modeling::resolve(p, Some(plane));
        let d = plane.coords(q);
        if d.length() < MIN_LEN {
            return;
        }
        let mut a = d.y.atan2(d.x) - *start;
        // Into (-pi, pi].
        while a > std::f64::consts::PI {
            a -= std::f64::consts::TAU;
        }
        while a <= -std::f64::consts::PI {
            a += std::f64::consts::TAU;
        }
        let tick = (a / TICK).round() * TICK;
        if (a - tick).abs() <= SNAP {
            a = tick;
        }
        *angle = a;
        self.cursor = Some(q);
    }

    fn finish_rotate(
        &mut self,
        subject: Subject,
        plane: Plane3,
        angle: f64,
        count: u32,
        divided: bool,
        redo: bool,
    ) -> Out {
        let copy = self.ctrl;
        let action = subject.action(
            rotation(plane.origin, plane.n, angle),
            if copy { count } else { 0 },
        );
        self.pending = Some(Last {
            subject,
            kind: LastKind::Rotate { plane, angle },
            copy,
            count: if copy { count } else { 1 },
            divided: copy && divided,
        });
        self.cursor = None;
        Out {
            actions: vec![action],
            redo,
            message: None,
        }
    }

    // ---- Scale ---------------------------------------------------------------------------------

    fn grips(&self, w: &World) -> Vec<(Grip, DVec3)> {
        let Some(subject) = Subject::resolve(w, &self.ctx) else {
            return Vec::new();
        };
        let Some((lo, hi)) = subject.bounds else {
            return Vec::new();
        };
        usable_grips(lo, hi)
            .into_iter()
            .map(|g| (g, g.point(lo, hi)))
            .collect()
    }

    /// The grip that the ray passes nearest to, within reach. Grips that are about equally near
    /// (the grips of a small box, or two grips on one line of sight): a corner before an edge
    /// before a face, then the one nearer the eye.
    fn grip_at(&self, w: &World, ray: &Ray) -> Option<Grip> {
        let mut best: Option<((i64, i64), f64, Grip)> = None;
        for (g, p) in self.grips(w) {
            let t = (p - ray.origin).dot(ray.dir);
            if t <= 0.0 {
                continue;
            }
            let off = (p - ray.origin - ray.dir * t).length() / t;
            if off > GRIP_REACH {
                continue;
            }
            let key = ((off / 5e-4).floor() as i64, -(g.axes() as i64));
            let better = best.is_none_or(|(k, bt, _)| (key, t.to_bits()) < (k, bt.to_bits()));
            if better {
                best = Some((key, t, g));
            }
        }
        best.map(|(_, _, g)| g)
    }

    fn click_scale(&mut self, w: &World, p: &Pointer) -> Out {
        match std::mem::take(&mut self.run) {
            XRun::Idle => {
                self.last = None;
                let Some(subject) = Subject::resolve(w, &self.ctx) else {
                    return Out::msg(tr!("uic-modeling-select-first"));
                };
                let Some(bounds) = subject.bounds else {
                    return Out::default();
                };
                let Some(grip) = self.grip_at(w, &p.ray) else {
                    return Out::msg(tr!("uic-modeling-pick-grip"));
                };
                self.run = XRun::Scale {
                    subject,
                    grip,
                    bounds,
                    factors: DVec3::ONE,
                    uniform: (grip.axes() == 3) != self.ctx.shift,
                };
                self.scale_to(&p.ray);
                Out::default()
            }
            XRun::Scale {
                subject,
                grip,
                bounds,
                factors,
                uniform,
            } => {
                if (factors - DVec3::ONE).abs().max_element() < 1e-6 {
                    self.run = XRun::Scale {
                        subject,
                        grip,
                        bounds,
                        factors,
                        uniform,
                    };
                    return Out::msg(tr!("uic-modeling-too-small"));
                }
                self.finish_scale(subject, grip, bounds, factors, uniform, false)
            }
            other => {
                self.run = other;
                Out::default()
            }
        }
    }

    /// The distance from the anchor to the grip along the axes that the grip scales (the length
    /// that a typed length is measured against).
    fn ref_length(
        &self,
        grip: Grip,
        bounds: (DVec3, DVec3),
        about_center: bool,
        uniform: bool,
    ) -> f64 {
        let anchor = grip.anchor(bounds.0, bounds.1, about_center);
        let g = grip.point(bounds.0, bounds.1);
        let mut r = g - anchor;
        if uniform && grip.axes() < 3 {
            // A uniform scale from a face or an edge grip: the diagonal of the box.
            r = bounds.1 - bounds.0;
        }
        r.length()
    }

    /// The factors that the pointer ray gives for the grip.
    fn scale_to(&mut self, ray: &Ray) {
        let ctrl = self.ctrl;
        let shift = self.ctx.shift;
        let XRun::Scale {
            grip,
            bounds,
            factors,
            uniform,
            ..
        } = &mut self.run
        else {
            return;
        };
        *uniform = (grip.axes() == 3) != shift;
        let anchor = grip.anchor(bounds.0, bounds.1, ctrl);
        let g = grip.point(bounds.0, bounds.1);
        let r = g - anchor;
        if r.length() < 1e-9 {
            return;
        }
        let f = if *uniform {
            let dir = r.normalize();
            closest_on_line(ray, anchor, dir).map(|t| DVec3::splat(t / r.length()))
        } else {
            let axes: Vec<usize> = (0..3).filter(|a| grip.uses(*a)).collect();
            let point = match axes.len() {
                1 => {
                    let mut d = DVec3::ZERO;
                    d[axes[0]] = 1.0;
                    closest_on_line(ray, anchor, d).map(|t| anchor + d * t)
                }
                2 => {
                    let mut n = DVec3::ZERO;
                    let third = (0..3).find(|a| !grip.uses(*a)).expect("a free axis");
                    n[third] = 1.0;
                    Plane3::through(g, n).and_then(|pl| pl.intersect(ray))
                }
                _ => Plane3::through(g, -ray.dir).and_then(|pl| pl.intersect(ray)),
            };
            point.map(|q| {
                let mut f = DVec3::ONE;
                for a in &axes {
                    f[*a] = (q - anchor)[*a] / r[*a];
                }
                f
            })
        };
        if let Some(f) = f.filter(|f| f.is_finite()) {
            *factors = f.map(clamp_factor);
        }
    }

    fn finish_scale(
        &mut self,
        subject: Subject,
        grip: Grip,
        bounds: (DVec3, DVec3),
        factors: DVec3,
        uniform: bool,
        redo: bool,
    ) -> Out {
        let about_center = self.ctrl;
        let anchor = grip.anchor(bounds.0, bounds.1, about_center);
        let action = subject.action(scaling(anchor, factors), 0);
        self.pending = Some(Last {
            subject,
            kind: LastKind::Scale {
                grip,
                bounds,
                about_center,
                uniform,
            },
            copy: false,
            count: 1,
            divided: false,
        });
        self.cursor = None;
        Out {
            actions: vec![action],
            redo,
            message: None,
        }
    }

    // ---- redo ----------------------------------------------------------------------------------

    /// Call after the actions ran: the step can be redone with a new value.
    pub fn committed(&mut self, revision: u64) {
        if let Some(l) = self.pending.take() {
            self.last = Some((l, revision));
        }
    }

    /// The actions were refused and the step that was taken back is back: it can still be redone.
    pub fn refused(&mut self, revision: u64) {
        if let Some((l, _)) = self.last.take() {
            self.last = Some((l, revision));
        }
    }

    fn can_redo(&self, revision: u64) -> Option<Last> {
        self.last
            .as_ref()
            .filter(|(_, r)| *r == revision)
            .map(|(l, _)| l.clone())
    }

    // ---- typed values --------------------------------------------------------------------------

    /// A typed value, read by the Measurements box.
    pub fn apply(&mut self, v: Value, revision: u64) -> Out {
        let run = std::mem::take(&mut self.run);
        match (self.tool, run) {
            (Some(ModelerTool::Move), XRun::Move { subject, base }) => {
                let cur = self.cursor.map_or(DVec3::X, |c| c - base);
                let Some(d) = move_vector(v, cur, base) else {
                    self.run = XRun::Move { subject, base };
                    return Out::default();
                };
                if d.length() < MIN_LEN {
                    self.run = XRun::Move { subject, base };
                    return Out::msg(tr!("uic-modeling-too-small"));
                }
                self.finish_move(subject, base, d, 1, false, false)
            }
            (Some(ModelerTool::Move), XRun::Idle) => self.redo_move(v, revision),
            (Some(ModelerTool::Rotate), XRun::Angle { subject, plane, .. }) => match v {
                Value::Angle(a) => self.finish_rotate_typed(subject, plane, a),
                _ => {
                    self.run = XRun::Angle {
                        subject,
                        plane,
                        start: 0.0,
                        radius: 0.0,
                        angle: 0.0,
                    };
                    Out::default()
                }
            },
            (Some(ModelerTool::Rotate), XRun::Idle) => self.redo_rotate(v, revision),
            (Some(ModelerTool::Scale), XRun::Scale { subject, grip, bounds, uniform, factors }) => {
                self.run = XRun::Scale {
                    subject: subject.clone(),
                    grip,
                    bounds,
                    uniform,
                    factors,
                };
                let about_center = self.ctrl;
                let Some(f) = self.typed_factors(v, grip, bounds, about_center, uniform) else {
                    return Out::default();
                };
                self.run = XRun::Idle;
                self.finish_scale(subject, grip, bounds, f, uniform, false)
            }
            (Some(ModelerTool::Scale), XRun::Idle) => self.redo_scale(v, revision),
            (_, run) => {
                self.run = run;
                Modeling::need_start()
            }
        }
    }

    fn finish_rotate_typed(&mut self, subject: Subject, plane: Plane3, deg: f64) -> Out {
        let a = deg.to_radians();
        if a.abs() < 1e-9 {
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        self.finish_rotate(subject, plane, a, 1, false, false)
    }

    fn redo_move(&mut self, v: Value, revision: u64) -> Out {
        let Some(l) = self.can_redo(revision) else {
            return Modeling::need_start();
        };
        let LastKind::Move { base, d } = l.kind.clone() else {
            return Modeling::need_start();
        };
        // The whole distance of a divided copy, else one step.
        let whole = if l.divided { d * f64::from(l.count) } else { d };
        let (step, count, divided) = match v {
            Value::Count(n) if l.copy => (d, n, false),
            Value::Divide(n) if l.copy => (whole / f64::from(n), n, true),
            other => {
                let Some(nd) = move_vector(other, d, base) else {
                    return Out::default();
                };
                if l.divided && l.copy {
                    (nd / f64::from(l.count), l.count, true)
                } else {
                    (nd, l.count, false)
                }
            }
        };
        if step.length() < MIN_LEN {
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        self.ctrl = l.copy;
        self.finish_move(l.subject, base, step, count, divided, true)
    }

    fn redo_rotate(&mut self, v: Value, revision: u64) -> Out {
        let Some(l) = self.can_redo(revision) else {
            return Modeling::need_start();
        };
        let LastKind::Rotate { plane, angle } = l.kind.clone() else {
            return Modeling::need_start();
        };
        let whole = if l.divided { angle * f64::from(l.count) } else { angle };
        let (a, count, divided) = match v {
            Value::Count(n) if l.copy => (angle, n, false),
            Value::Divide(n) if l.copy => (whole / f64::from(n), n, true),
            Value::Angle(deg) => {
                let a = deg.to_radians();
                if l.divided && l.copy {
                    (a / f64::from(l.count), l.count, true)
                } else {
                    (a, l.count, false)
                }
            }
            _ => return Out::default(),
        };
        if a.abs() < 1e-9 {
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        self.ctrl = l.copy;
        self.finish_rotate(l.subject, plane, a, count, divided, true)
    }

    fn redo_scale(&mut self, v: Value, revision: u64) -> Out {
        let Some(l) = self.can_redo(revision) else {
            return Modeling::need_start();
        };
        let LastKind::Scale {
            grip,
            bounds,
            about_center,
            uniform,
            ..
        } = l.kind.clone()
        else {
            return Modeling::need_start();
        };
        let Some(f) = self.typed_factors(v, grip, bounds, about_center, uniform) else {
            return Out::default();
        };
        self.ctrl = about_center;
        self.finish_scale(l.subject, grip, bounds, f, uniform, true)
    }

    /// The factors for a typed value.
    fn typed_factors(
        &self,
        v: Value,
        grip: Grip,
        bounds: (DVec3, DVec3),
        about_center: bool,
        uniform: bool,
    ) -> Option<DVec3> {
        let per_grip = |f: f64| {
            if uniform {
                DVec3::splat(f)
            } else {
                let mut r = DVec3::ONE;
                for a in 0..3 {
                    if grip.uses(a) {
                        r[a] = f;
                    }
                }
                r
            }
        };
        let f = match v {
            Value::Factor(f) => per_grip(f),
            Value::Triple([x, y, z]) => DVec3::new(x.unwrap_or(1.0), y.unwrap_or(1.0), z.unwrap_or(1.0)),
            Value::Length(l) => {
                let r = self.ref_length(grip, bounds, about_center, uniform);
                if r < 1e-9 {
                    return None;
                }
                per_grip(l / r)
            }
            _ => return None,
        };
        (f.is_finite() && f.abs().min_element() > 0.0).then(|| f.map(clamp_factor))
    }

    // ---- drawing -------------------------------------------------------------------------------

    /// Adds what the tool shows to the preview.
    pub fn preview(&self, w: &World, pv: &mut Preview) {
        pv.copy = self.copying();
        match &self.run {
            XRun::Move { subject, base } => {
                if let Some(c) = self.cursor {
                    let m = DAffine3::from_translation(c - *base);
                    pv.shape.extend(moved_outline(subject, w, &m));
                    pv.shape.push([*base, c]);
                }
            }
            XRun::Center { plane, .. } => {
                if let Some(c) = self.cursor {
                    let r = plane.origin.distance(c).max(MIN_LEN);
                    pv.shape.extend(protractor(plane, r, 0.0));
                    pv.shape.push([plane.origin, c]);
                }
            }
            XRun::Angle {
                subject,
                plane,
                start,
                radius,
                angle,
            } => {
                pv.shape.extend(protractor(plane, *radius, *start));
                let at = |a: f64| plane.at(plane.origin, radius * a.cos(), radius * a.sin());
                pv.shape.push([plane.origin, at(*start)]);
                pv.shape.push([plane.origin, at(*start + angle)]);
                // The arc of the angle.
                let n = ((angle.abs() / TICK).ceil() as usize * 3).clamp(2, 72);
                for i in 0..n {
                    let (a, b) = (
                        start + angle * i as f64 / n as f64,
                        start + angle * (i + 1) as f64 / n as f64,
                    );
                    pv.shape.push([
                        plane.origin + (at(a) - plane.origin) * 0.8,
                        plane.origin + (at(b) - plane.origin) * 0.8,
                    ]);
                }
                let m = rotation(plane.origin, plane.n, *angle);
                pv.shape.extend(moved_outline(subject, w, &m));
            }
            XRun::Scale {
                subject,
                grip,
                bounds,
                factors,
                ..
            } => {
                let anchor = grip.anchor(bounds.0, bounds.1, self.ctrl);
                pv.shape
                    .extend(moved_outline(subject, w, &scaling(anchor, *factors)));
                pv.shape.extend(box_edges(bounds.0, bounds.1));
            }
            XRun::Idle => match self.tool {
                Some(ModelerTool::Rotate) => {
                    if let (Some(c), n) = (self.hover_center, self.hover_normal)
                        && let Some(pl) = Plane3::through(c, n)
                    {
                        let r = self.hint_radius(w);
                        pv.shape.extend(protractor(&pl, r, 0.0));
                    }
                }
                Some(ModelerTool::Scale) => {
                    let grips = self.grips(w);
                    pv.grip_hover = self
                        .hover_grip
                        .and_then(|h| grips.iter().position(|(g, _)| *g == h));
                    pv.grips = grips.into_iter().map(|(_, p)| p).collect();
                    if let Some(s) = Subject::resolve(w, &self.ctx)
                        && let Some((lo, hi)) = s.bounds
                    {
                        pv.shape.extend(box_edges(lo, hi));
                    }
                }
                _ => {}
            },
        }
    }

    /// A radius for the protractor before the center is clicked: a share of the selection.
    fn hint_radius(&self, w: &World) -> f64 {
        Subject::resolve(w, &self.ctx)
            .and_then(|s| s.bounds)
            .map_or(500.0, |(lo, hi)| ((hi - lo).length() * 0.5).max(100.0))
    }
}

fn clamp_factor(f: f64) -> f64 {
    if f.abs() < MIN_FACTOR {
        MIN_FACTOR.copysign(if f == 0.0 { 1.0 } else { f })
    } else {
        f
    }
}

/// The vector of a typed value for a move, `cur` the vector that the pointer shows now (or the
/// last step) and `base` the start.
fn move_vector(v: Value, cur: DVec3, base: DVec3) -> Option<DVec3> {
    match v {
        Value::Length(l) => {
            let dir = cur.try_normalize().unwrap_or(DVec3::X);
            Some(dir * l)
        }
        Value::Triple([x, y, z]) => Some(DVec3::new(
            x.unwrap_or(cur.x),
            y.unwrap_or(cur.y),
            z.unwrap_or(cur.z),
        )),
        Value::PointRel(d) => Some(d),
        Value::PointAbs(q) => Some(q - base),
        _ => None,
    }
}

/// The subject placed by `m`, as segments.
fn moved_outline(subject: &Subject, w: &World, m: &DAffine3) -> Vec<[DVec3; 2]> {
    subject
        .outline(w)
        .into_iter()
        .map(|[a, b]| [m.transform_point3(a), m.transform_point3(b)])
        .collect()
}

/// The normal of the plane for the protractor under the pointer: the face, else the plane of the
/// inference, else horizontal.
fn plane_normal_at(w: &World, p: &Pointer) -> DVec3 {
    if p.inf.source.kind() == TagKind::Face
        && let Some(pl) = w.face_plane(p.inf.source)
    {
        return pl.n;
    }
    if let Some(Constraint::Plane { normal, .. }) = p.inf.constraint {
        return normal.try_normalize().unwrap_or(DVec3::Z);
    }
    DVec3::Z
}

/// The circle of the protractor with a tick every 15 degrees, the first tick at `from`.
fn protractor(plane: &Plane3, r: f64, from: f64) -> Vec<[DVec3; 2]> {
    let mut out = Vec::new();
    let at = |a: f64, k: f64| plane.at(plane.origin, k * r * a.cos(), k * r * a.sin());
    let n = 48;
    for i in 0..n {
        let (a, b) = (
            from + std::f64::consts::TAU * i as f64 / f64::from(n),
            from + std::f64::consts::TAU * (i + 1) as f64 / f64::from(n),
        );
        out.push([at(a, 1.0), at(b, 1.0)]);
    }
    for i in 0..24 {
        let a = from + TICK * f64::from(i);
        out.push([at(a, 0.92), at(a, 1.0)]);
    }
    out
}

/// The Move, Rotate or Scale action of a selection, for the menu or a test: the world matrix.
pub fn xform_action(w: &World, ctx: &Context, matrix: DAffine3, copies: u32) -> Option<Action> {
    Subject::resolve(w, ctx).map(|s| s.action(matrix, copies))
}
