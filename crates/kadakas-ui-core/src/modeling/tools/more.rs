//! Polygon, Offset and Follow Me (MOD-08): the state machines of the tools that came with the
//! second part of MOD-08. They live beside the other drawing tools (`tools`) and use the same
//! pointer, typed values and actions.
//!
//! - **Polygon**: click the center, then a corner (or type the radius). The polygon has
//!   [`DEFAULT_POLYGON_SIDES`] sides; `8s` (before, during or just after) sets the number, as for a
//!   circle. The radius goes to a corner (inscribed), as in other modelers of this kind; Ctrl
//!   (Option on a Mac) switches to a polygon around the circle (circumscribed: the radius goes to
//!   the middle of a side). The polygon is one curve, like a circle.
//! - **Offset** (`F`): click a face (or anywhere with one face selected), move the pointer and
//!   click again, or type the distance. Inside the face the loops move inward, outside it outward;
//!   a negative typed distance goes the other way. With edges selected (a chain in one plane), the
//!   chain is offset to the side of the pointer. Just after an offset a new value redoes it.
//! - **Follow Me**: with the path selected (edges), click the face to sweep. With nothing
//!   selected, click the face, then the path: an edge (its curve, or the chain of edges it is in)
//!   or a face (its outline).

use glam::DVec3;
use kadakas_core::model::ObjectId;
use kadakas_core::surface::{Analytic, EdgeId, Edit, FaceId, SurfaceModel};
use kadakas_l10n::tr;
use kadakas_units::Units;

use super::super::exec::Action;
use super::super::plane::Plane3;
use super::super::scenes::{World, frame_of, model_of};
use super::super::select::SubSelection;
use super::{LiveLabel, MIN_LEN, Modeling, Out, Pointer, Preview, circle_points};
use crate::infer3d::TagKind;
use crate::measure_box::{Phase, Value};
use crate::modeler::ModelerTool;

/// A modifier key that toggles something when it is pressed and let go alone: Ctrl+Z and the like
/// (another key, a button, another modifier while it is down) do not toggle (the Polygon kind).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyToggle {
    down: bool,
    armed: bool,
}

impl KeyToggle {
    /// One frame: `key` is down now, `other` is true when anything else was pressed or held in
    /// this frame. True when the key was let go alone (toggle now).
    pub fn step(&mut self, key: bool, other: bool) -> bool {
        if key && other {
            self.armed = false;
        } else if key && !self.down {
            self.armed = true;
        }
        let fire = !key && self.down && self.armed;
        if !key {
            self.armed = false;
        }
        self.down = key;
        fire
    }
}

/// The default number of sides of a polygon.
pub const DEFAULT_POLYGON_SIDES: u32 = 6;

/// True for the tools of this module.
pub fn is_more_tool(t: ModelerTool) -> bool {
    matches!(
        t,
        ModelerTool::Polygon | ModelerTool::Offset | ModelerTool::FollowMe
    )
}

/// What Offset works on.
#[derive(Clone, Debug, PartialEq)]
enum On {
    Face(FaceId),
    /// A chain of edges in one plane.
    Edges(Vec<EdgeId>),
}

#[derive(Clone, Debug)]
enum Run {
    Idle,
    Polygon {
        c: DVec3,
        plane: Plane3,
        hit: Option<ObjectId>,
    },
    Offset {
        model: ObjectId,
        on: On,
        /// The plane of the face, in the world (the pointer goes there).
        plane: Option<Plane3>,
        /// The distance in the local units of the model, and the side.
        distance: f64,
        outward: bool,
        /// The point of the pointer in local coordinates (the side of an edge offset).
        toward: DVec3,
        /// World mm per local unit along the plane.
        scale: f64,
        preview: Vec<[DVec3; 2]>,
    },
    /// Follow Me: the profile is picked, the path comes next.
    Follow {
        model: ObjectId,
        face: FaceId,
    },
}

/// What can be done again with a new typed value.
#[derive(Clone, Debug, PartialEq)]
enum Last {
    Polygon {
        c: DVec3,
        plane: Plane3,
        r: f64,
        theta: f64,
        hit: Option<ObjectId>,
    },
    Offset {
        model: ObjectId,
        on: On,
        outward: bool,
        toward: DVec3,
        scale: f64,
    },
}

/// The state of Polygon, Offset and Follow Me.
#[derive(Debug)]
pub(super) struct State {
    run: Run,
    cursor: Option<DVec3>,
    /// Sides of a polygon.
    pub sides: u32,
    /// The polygon goes around the circle of the radius (the radius to the middle of a side).
    pub circumscribed: bool,
    /// The selected faces and edges (Offset and Follow Me work on them).
    parts: SubSelection,
    pending: Option<Last>,
    last: Option<(Last, u64)>,
    /// The path under the pointer while Follow Me waits for it, for the preview.
    path_hover: Vec<[DVec3; 2]>,
}

impl Default for State {
    fn default() -> Self {
        State {
            run: Run::Idle,
            cursor: None,
            sides: DEFAULT_POLYGON_SIDES,
            circumscribed: false,
            parts: SubSelection::default(),
            pending: None,
            last: None,
            path_hover: Vec::new(),
        }
    }
}

impl State {
    /// The tool changed: what was in progress goes.
    pub fn choose(&mut self) {
        self.run = Run::Idle;
        self.cursor = None;
        self.pending = None;
        self.last = None;
        self.path_hover.clear();
    }

    pub fn reset(&mut self) {
        self.run = Run::Idle;
        self.cursor = None;
        self.path_hover.clear();
    }

    pub fn in_progress(&self) -> bool {
        !matches!(self.run, Run::Idle)
    }

    pub fn reference(&self) -> Option<DVec3> {
        match &self.run {
            Run::Polygon { c, .. } => Some(*c),
            _ => None,
        }
    }

    pub fn committed(&mut self, revision: u64) {
        if let Some(l) = self.pending.take() {
            self.last = Some((l, revision));
        }
    }

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

    pub fn set_parts(&mut self, parts: SubSelection) {
        self.parts = parts;
    }
}

/// The points of a regular polygon with `sides` corners around `c` in `plane`: with `outside`
/// the radius `r` goes to the middle of a side (at the angle `theta`), else to a corner.
pub fn polygon_points(
    c: DVec3,
    plane: &Plane3,
    r: f64,
    theta: f64,
    sides: u32,
    outside: bool,
) -> Vec<DVec3> {
    let n = sides.max(3);
    if outside {
        let half = std::f64::consts::PI / f64::from(n);
        circle_points(c, plane, r / half.cos(), theta + half, n)
    } else {
        circle_points(c, plane, r, theta, n)
    }
}

/// The world plane of a face of a model, with its front normal.
fn face_plane(w: &World, model: ObjectId, face: FaceId) -> Option<Plane3> {
    let m = model_of(w.doc, model)?;
    let f = m.face(face)?;
    let fr = frame_of(w.doc, model);
    let p0 = fr.transform_point3(m.vertex(*f.outer.first()?)?);
    let n = fr.matrix3.inverse().transpose() * f.plane.normal;
    Plane3::through(p0, n)
}

/// The world plane of an Offset of a chain of edges (the side of `toward`, local), if it has one.
fn chain_plane(w: &World, model: ObjectId, edges: &[EdgeId], toward: DVec3) -> Option<Plane3> {
    let m = model_of(w.doc, model)?;
    let pl = m.offset_chain_plane(edges, toward).ok()?;
    let fr = frame_of(w.doc, model);
    let p0 = fr.transform_point3(pl.normal * pl.offset);
    let n = fr.matrix3.inverse().transpose() * pl.normal;
    Plane3::through(p0, n)
}

/// The world length of one local unit along the plane of a face (1 for a model that is not
/// scaled).
fn plane_scale(w: &World, model: ObjectId, plane: &Plane3) -> f64 {
    let fr = frame_of(w.doc, model);
    let inv = fr.matrix3.inverse();
    let local = (inv * plane.u).length();
    if local > 1e-12 { 1.0 / local } else { 1.0 }
}

/// True if the local point lies inside the face (in its plane; even-odd over its loops).
fn inside_face(m: &SurfaceModel, face: FaceId, p: DVec3) -> bool {
    let Some(f) = m.face(face) else {
        return false;
    };
    let q = f.plane.project(p);
    let mut odd = false;
    for l in f.loops() {
        let pts: Vec<glam::DVec2> = l
            .iter()
            .filter_map(|v| m.vertex(*v))
            .map(|x| f.plane.project(x))
            .collect();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if (a.y > q.y) != (b.y > q.y) {
                let x = a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if q.x < x {
                    odd = !odd;
                }
            }
        }
    }
    odd
}

/// The distance from a local point to the nearest side of the loops of a face, or of a chain.
fn nearest_side(m: &SurfaceModel, on: &On, p: DVec3) -> f64 {
    let seg = |a: DVec3, b: DVec3| {
        let d = b - a;
        let t = if d.length_squared() > 0.0 {
            ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (a + d * t).distance(p)
    };
    let mut best = f64::INFINITY;
    match on {
        On::Face(face) => {
            if let Some(f) = m.face(*face) {
                for (a, b) in f.uses() {
                    if let (Some(x), Some(y)) = (m.vertex(a), m.vertex(b)) {
                        best = best.min(seg(x, y));
                    }
                }
            }
        }
        On::Edges(es) => {
            for e in es {
                if let Some(ed) = m.edge(*e)
                    && let (Some(x), Some(y)) = (m.vertex(ed.v[0]), m.vertex(ed.v[1]))
                {
                    best = best.min(seg(x, y));
                }
            }
        }
    }
    best
}

/// The lines that an offset would draw, in world coordinates, or nothing when it does not fit.
fn offset_preview(
    w: &World,
    model: ObjectId,
    on: &On,
    distance: f64,
    outward: bool,
    toward: DVec3,
) -> Vec<[DVec3; 2]> {
    let Some(m) = model_of(w.doc, model) else {
        return Vec::new();
    };
    let fr = frame_of(w.doc, model);
    let lines: Vec<(Vec<DVec3>, bool)> = match on {
        On::Face(face) => match m.offset_outline(*face, distance, outward) {
            Ok(ls) => ls.into_iter().map(|l| (l, true)).collect(),
            Err(_) => return Vec::new(),
        },
        On::Edges(es) => match m.offset_chain(es, distance, toward) {
            Ok((l, closed)) => vec![(l, closed)],
            Err(_) => return Vec::new(),
        },
    };
    let mut out = Vec::new();
    for (l, closed) in lines {
        let k = l.len();
        let count = if closed { k } else { k.saturating_sub(1) };
        for i in 0..count {
            out.push([
                fr.transform_point3(l[i]),
                fr.transform_point3(l[(i + 1) % k]),
            ]);
        }
    }
    out
}

/// The path of Follow Me that a click on `tag` names: the curve of an edge, else the chain of
/// edges through it up to a branch; the outline of a face.
fn path_of(m: &SurfaceModel, kind: TagKind, id: u32, profile: FaceId) -> Vec<EdgeId> {
    match kind {
        TagKind::Edge => {
            let e = EdgeId(id);
            if let Some(c) = m.curve_of(e)
                && let Some((_, es)) = m.curves().find(|(k, _)| *k == c)
            {
                return es.clone();
            }
            chain_through(m, e)
        }
        TagKind::Face if FaceId(id) != profile => {
            let Some(f) = m.face(FaceId(id)) else {
                return Vec::new();
            };
            let n = f.outer.len();
            (0..n)
                .filter_map(|i| m.edge_between(f.outer[i], f.outer[(i + 1) % n]))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// The edges through `e` along vertices that have two edges (a polyline), in order.
fn chain_through(m: &SurfaceModel, e: EdgeId) -> Vec<EdgeId> {
    let mut at: std::collections::BTreeMap<_, Vec<EdgeId>> = std::collections::BTreeMap::new();
    for (id, ed) in m.edges() {
        for v in ed.v {
            at.entry(v).or_default().push(id);
        }
    }
    let Some(start) = m.edge(e) else {
        return Vec::new();
    };
    let mut out = vec![e];
    let mut seen = std::collections::BTreeSet::from([e]);
    for end in start.v {
        let mut v = end;
        let mut cur = e;
        loop {
            let es = at.get(&v).map_or(&[][..], Vec::as_slice);
            if es.len() != 2 {
                break;
            }
            let next = if es[0] == cur { es[1] } else { es[0] };
            if !seen.insert(next) {
                break;
            }
            out.push(next);
            cur = next;
            let Some(o) = m.edge(next).and_then(|x| x.other(v)) else {
                break;
            };
            v = o;
        }
    }
    out
}

impl Modeling {
    /// The selected faces and edges, for Offset and Follow Me. Call it before each event.
    pub fn set_parts(&mut self, parts: &SubSelection) {
        if self.more.parts != *parts {
            self.more.set_parts(parts.clone());
        }
    }

    /// Ctrl (Option on a Mac) while drawing a polygon: inscribed or circumscribed.
    pub fn toggle_polygon_kind(&mut self) {
        self.more.circumscribed = !self.more.circumscribed;
    }

    /// True if the polygon goes around the circle of the radius.
    pub fn polygon_circumscribed(&self) -> bool {
        self.more.circumscribed
    }

    /// The sides of the next polygon.
    pub fn polygon_sides(&self) -> u32 {
        self.more.sides
    }

    pub(super) fn more_phase(&self) -> Phase {
        match &self.more.run {
            Run::Idle if self.more.last.is_some() => Phase::Done,
            Run::Idle => Phase::Step(0),
            _ => Phase::Step(1),
        }
    }

    pub(super) fn more_live(&self, units: &Units) -> Option<(LiveLabel, String)> {
        let len = |v: f64| units.length_text(v);
        match &self.more.run {
            Run::Polygon { c, plane, .. } => {
                let q = self.more.cursor?;
                let d = plane.coords(q) - plane.coords(*c);
                Some((LiveLabel::Radius, len(d.length())))
            }
            Run::Offset {
                distance, scale, ..
            } => Some((LiveLabel::Distance, len(*distance * *scale))),
            _ => None,
        }
    }

    pub(super) fn more_moved(&mut self, w: &World, p: &Pointer) {
        match &mut self.more.run {
            Run::Polygon { plane, .. } => {
                let pl = *plane;
                self.more.cursor = Some(Modeling::resolve(p, Some(&pl)));
            }
            Run::Offset {
                model,
                on,
                plane,
                distance,
                outward,
                toward,
                preview,
                ..
            } => {
                let model = *model;
                let Some(m) = model_of(w.doc, model) else {
                    return;
                };
                // In the plane of the face or the chain; a snapped point counts as it is.
                let q = match plane {
                    Some(pl) => Modeling::resolve(p, Some(pl)),
                    None => p.inf.point,
                };
                let lq = w.to_local(model, q);
                let d = nearest_side(m, on, lq);
                if !d.is_finite() {
                    return;
                }
                *distance = d;
                *toward = lq;
                *outward = match on {
                    On::Face(f) => !inside_face(m, *f, lq),
                    On::Edges(_) => false,
                };
                *preview = offset_preview(w, model, on, d, *outward, lq);
                self.more.cursor = Some(q);
            }
            Run::Follow { model, face } => {
                let (model, face) = (*model, *face);
                self.more.path_hover.clear();
                if let Some((id, m)) = w.model(p.inf.source)
                    && id == model
                {
                    let fr = frame_of(w.doc, model);
                    for e in path_of(m, p.inf.source.kind(), p.inf.source.id(), face) {
                        if let Some(ed) = m.edge(e)
                            && let (Some(a), Some(b)) = (m.vertex(ed.v[0]), m.vertex(ed.v[1]))
                        {
                            self.more
                                .path_hover
                                .push([fr.transform_point3(a), fr.transform_point3(b)]);
                        }
                    }
                }
                self.more.cursor = Some(p.inf.point);
            }
            Run::Idle => {
                self.more.cursor = Some(p.inf.point);
            }
        }
    }

    pub(super) fn more_click(&mut self, w: &World, p: &Pointer) -> Out {
        match self.tool {
            ModelerTool::Polygon => self.click_polygon(w, p),
            ModelerTool::Offset => self.click_offset(w, p),
            ModelerTool::FollowMe => self.click_follow(w, p),
            _ => Out::default(),
        }
    }

    fn click_polygon(&mut self, w: &World, p: &Pointer) -> Out {
        match self.more.run.clone() {
            Run::Polygon { c, plane, hit } => {
                let b = Modeling::resolve(p, Some(&plane));
                let d = plane.coords(b) - plane.coords(c);
                self.more.run = Run::Idle;
                self.finish_polygon(
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
                self.more.last = None;
                let at = p.inf.point;
                let plane = Modeling::plane_for(w, p, at);
                self.more.run = Run::Polygon {
                    c: at,
                    plane,
                    hit: Modeling::hit_model(w, p),
                };
                self.more.cursor = Some(at);
                Out::default()
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_polygon(
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
        self.more.pending = Some(Last::Polygon {
            c,
            plane,
            r,
            theta,
            hit,
        });
        Out {
            actions: vec![Action::AddCurve {
                target: self.target(w, hit, selected),
                points: polygon_points(
                    c,
                    &plane,
                    r,
                    theta,
                    self.more.sides,
                    self.more.circumscribed,
                ),
                closed: true,
                faceted: true,
                analytic: Some(Analytic::polygon(
                    c,
                    plane.n,
                    plane.u,
                    if self.more.circumscribed {
                        r / (std::f64::consts::PI / f64::from(self.more.sides.max(3))).cos()
                    } else {
                        r
                    },
                )),
            }],
            redo,
            message: None,
            ..Out::default()
        }
    }

    fn click_offset(&mut self, w: &World, p: &Pointer) -> Out {
        if let Run::Offset {
            model,
            on,
            distance,
            outward,
            toward,
            scale,
            ..
        } = self.more.run.clone()
        {
            self.more.run = Run::Idle;
            return self.finish_offset(model, on, distance, outward, toward, scale, false);
        }
        self.more.last = None;
        // A selected face, or selected edges of one model, else the face under the pointer. With
        // several faces selected, the one under the pointer if it is one of them.
        let faces = self.more.parts.faces();
        let edges = self.more.parts.edges();
        let under = (p.inf.source.kind() == TagKind::Face)
            .then(|| {
                w.model(p.inf.source)
                    .map(|(m, _)| (m, FaceId(p.inf.source.id())))
            })
            .flatten();
        let (model, on) = if let [(m, f)] = faces.as_slice() {
            (*m, On::Face(*f))
        } else if faces.len() > 1 {
            match under.filter(|u| faces.contains(u)) {
                Some((m, f)) => (m, On::Face(f)),
                None => return Out::msg(tr!("uic-modeling-offset-one-face")),
            }
        } else if faces.is_empty()
            && !edges.is_empty()
            && edges.iter().all(|(m, _)| *m == edges[0].0)
        {
            (
                edges[0].0,
                On::Edges(edges.iter().map(|(_, e)| *e).collect()),
            )
        } else if p.inf.source.kind() == TagKind::Face
            && let Some((m, _)) = w.model(p.inf.source)
        {
            (m, On::Face(FaceId(p.inf.source.id())))
        } else {
            return Out::msg(tr!("uic-modeling-offset-pick"));
        };
        let toward = w.to_local(model, p.inf.point);
        let plane = match &on {
            On::Face(f) => face_plane(w, model, *f),
            On::Edges(es) => chain_plane(w, model, es, toward),
        };
        let scale = plane.map_or(1.0, |pl| plane_scale(w, model, &pl));
        self.more.run = Run::Offset {
            model,
            on,
            plane,
            distance: 0.0,
            outward: false,
            toward,
            scale,
            preview: Vec::new(),
        };
        self.more_moved(w, p);
        Out::default()
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_offset(
        &mut self,
        model: ObjectId,
        on: On,
        distance: f64,
        outward: bool,
        toward: DVec3,
        scale: f64,
        redo: bool,
    ) -> Out {
        if distance * scale < MIN_LEN {
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        self.more.pending = Some(Last::Offset {
            model,
            on: on.clone(),
            outward,
            toward,
            scale,
        });
        let edit = match on {
            On::Face(face) => Edit::OffsetFace {
                face,
                distance,
                outward,
            },
            On::Edges(edges) => Edit::OffsetEdges {
                edges,
                distance,
                toward: toward.to_array(),
            },
        };
        Out {
            actions: vec![Action::Edit { model, edit }],
            redo,
            message: None,
            ..Out::default()
        }
    }

    fn click_follow(&mut self, w: &World, p: &Pointer) -> Out {
        let source = p.inf.source;
        if let Run::Follow { model, face } = self.more.run.clone() {
            let Some((m_id, m)) = w.model(source) else {
                return Out::msg(tr!("uic-modeling-follow-pick-path"));
            };
            if m_id != model {
                return Out::msg(tr!("uic-modeling-follow-other-model"));
            }
            let path = path_of(m, source.kind(), source.id(), face);
            if path.is_empty() {
                return Out::msg(tr!("uic-modeling-follow-pick-path"));
            }
            self.more.run = Run::Idle;
            self.more.path_hover.clear();
            return Out::act(Action::Edit {
                model,
                edit: Edit::FollowMe { face, path },
            });
        }
        if source.kind() != TagKind::Face {
            return Out::msg(tr!("uic-modeling-follow-pick-profile"));
        }
        let Some((model, _)) = w.model(source) else {
            return Out::default();
        };
        let face = FaceId(source.id());
        // The path selected first.
        let edges = self.more.parts.edges();
        if !edges.is_empty() {
            if edges.iter().any(|(m, _)| *m != model) {
                return Out::msg(tr!("uic-modeling-follow-other-model"));
            }
            return Out::act(Action::Edit {
                model,
                edit: Edit::FollowMe {
                    face,
                    path: edges.iter().map(|(_, e)| *e).collect(),
                },
            });
        }
        self.more.run = Run::Follow { model, face };
        Out::msg(tr!("uic-modeling-follow-pick-path"))
    }

    pub(super) fn more_apply(
        &mut self,
        w: &World,
        selected: Option<ObjectId>,
        revision: u64,
        v: Value,
    ) -> Out {
        if let Value::Sides(n) = v {
            if self.tool != ModelerTool::Polygon {
                return Out::default();
            }
            self.more.sides = n;
            if matches!(self.more.run, Run::Idle)
                && let Some(Last::Polygon {
                    c,
                    plane,
                    r,
                    theta,
                    hit,
                }) = self.more.can_redo(revision)
            {
                return self.finish_polygon(w, selected, c, plane, r, theta, hit, true);
            }
            return Out::default();
        }
        match (self.tool, self.more.run.clone(), v) {
            (ModelerTool::Polygon, Run::Polygon { c, plane, hit }, Value::Length(r)) => {
                let theta = self.more.cursor.map_or(0.0, |q| {
                    let d = plane.coords(q) - plane.coords(c);
                    d.y.atan2(d.x)
                });
                self.more.run = Run::Idle;
                self.finish_polygon(w, selected, c, plane, r, theta, hit, false)
            }
            (ModelerTool::Polygon, Run::Idle, Value::Length(r)) => {
                match self.more.can_redo(revision) {
                    Some(Last::Polygon {
                        c,
                        plane,
                        theta,
                        hit,
                        ..
                    }) => self.finish_polygon(w, selected, c, plane, r, theta, hit, true),
                    _ => Modeling::need_start(),
                }
            }
            (
                ModelerTool::Offset,
                Run::Offset {
                    model,
                    on,
                    outward,
                    toward,
                    scale,
                    ..
                },
                Value::Length(l),
            ) => {
                self.more.run = Run::Idle;
                let side = outward != (l < 0.0);
                // An edge offset to the other side: the point goes across the chain.
                let toward = if l < 0.0 {
                    mirror_toward(w, model, &on, toward)
                } else {
                    toward
                };
                self.finish_offset(model, on, l.abs() / scale, side, toward, scale, false)
            }
            (ModelerTool::Offset, Run::Idle, Value::Length(l)) => {
                match self.more.can_redo(revision) {
                    Some(Last::Offset {
                        model,
                        on,
                        outward,
                        toward,
                        scale,
                    }) => {
                        let side = outward != (l < 0.0);
                        // An edge offset to the other side: the point goes across the chain.
                        let toward = if l < 0.0 {
                            mirror_toward(w, model, &on, toward)
                        } else {
                            toward
                        };
                        self.finish_offset(model, on, l.abs() / scale, side, toward, scale, true)
                    }
                    _ => Modeling::need_start(),
                }
            }
            _ => Modeling::need_start(),
        }
    }

    pub(super) fn more_preview(&self, pv: &mut Preview) {
        match &self.more.run {
            Run::Polygon { c, plane, .. } => {
                if let Some(q) = self.more.cursor {
                    let d = plane.coords(q) - plane.coords(*c);
                    let pts = polygon_points(
                        *c,
                        plane,
                        d.length(),
                        d.y.atan2(d.x),
                        self.more.sides,
                        self.more.circumscribed,
                    );
                    let n = pts.len();
                    pv.shape = (0..n).map(|i| [pts[i], pts[(i + 1) % n]]).collect();
                }
            }
            Run::Offset { preview, .. } => pv.shape.clone_from(preview),
            Run::Follow { .. } => pv.pull.clone_from(&self.more.path_hover),
            Run::Idle => {}
        }
    }
}

/// The point across the chain from `toward` (for a negative typed offset of edges).
fn mirror_toward(w: &World, model: ObjectId, on: &On, toward: DVec3) -> DVec3 {
    let (Some(m), On::Edges(es)) = (model_of(w.doc, model), on) else {
        return toward;
    };
    let mut best: Option<(f64, DVec3)> = None;
    for e in es {
        if let Some(ed) = m.edge(*e)
            && let (Some(a), Some(b)) = (m.vertex(ed.v[0]), m.vertex(ed.v[1]))
        {
            let d = b - a;
            let t = if d.length_squared() > 0.0 {
                ((toward - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let foot = a + d * t;
            let dist = foot.distance(toward);
            if best.is_none_or(|(bd, _)| dist < bd) {
                best = Some((dist, foot));
            }
        }
    }
    best.map_or(toward, |(_, foot)| foot * 2.0 - toward)
}
