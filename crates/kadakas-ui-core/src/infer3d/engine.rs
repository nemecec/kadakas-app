//! The inference engine: candidates, priorities, linear inferences and locks.

use std::cmp::Ordering;

use glam::{DVec2, DVec3};

use super::bvh::Aabb;
use super::camera::{Camera, Ray};
use super::math::{inside_loops, line_line, nearest_on_screen_seg, ray_plane};
use super::scene::{EdgeItem, FaceItem, PointKind, Scene, Tag, TagKind};
use super::{Axis, Constraint, ConstraintLine, Inference, Kind, Options};

/// The keys that the inference reacts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Locks the red axis (X). Pressed again, it unlocks.
    Right,
    /// Locks the green axis (Y).
    Left,
    /// Locks the blue axis (Z).
    Up,
    /// Locks the edge that you hovered: first parallel to it, then perpendicular, then unlocked.
    Down,
    /// Locks the current inference for as long as Shift is down.
    ShiftDown,
    ShiftUp,
}

#[derive(Clone, Copy, Debug)]
struct Hover {
    a: DVec3,
    b: DVec3,
    tag: Tag,
}

#[derive(Clone, Copy, Debug)]
enum Spec {
    /// A line through the current reference point in this direction.
    Dir(DVec3),
    /// A fixed point, line or plane.
    Fixed(Constraint),
}

#[derive(Clone, Copy, Debug)]
struct Lock {
    spec: Spec,
    kind: Kind,
    source: Tag,
    other: Tag,
    /// The edge that a parallel or perpendicular lock refers to, for the helper line.
    helper: Option<(DVec3, DVec3)>,
    by_shift: bool,
}

/// One candidate point.
#[derive(Clone, Copy, Debug)]
struct Cand {
    kind: Kind,
    point: DVec3,
    /// The distance to the pointer in pixels.
    dist: f64,
    depth: f64,
    source: Tag,
    other: Tag,
    /// The scene edge the candidate belongs to.
    edge: Option<EdgeItem>,
    /// The normal of the face (for an on-face candidate).
    normal: Option<DVec3>,
}

fn cmp_cand(a: &Cand, b: &Cand) -> Ordering {
    a.kind
        .priority()
        .cmp(&b.kind.priority())
        .then(a.dist.total_cmp(&b.dist))
        .then(a.depth.total_cmp(&b.depth))
        .then(a.point.x.total_cmp(&b.point.x))
        .then(a.point.y.total_cmp(&b.point.y))
        .then(a.point.z.total_cmp(&b.point.z))
        .then(a.source.cmp(&b.source))
        .then(a.other.cmp(&b.other))
}

/// A line inference that is within the angle tolerance.
#[derive(Clone, Debug)]
struct LineCand {
    kind: Kind,
    origin: DVec3,
    dir: DVec3,
    angle: f64,
    point: DVec3,
    order: u8,
    from_point: bool,
    helper: Option<(DVec3, DVec3)>,
}

/// The result of the search for lines.
#[derive(Clone, Debug)]
struct Linear {
    kind: Kind,
    point: DVec3,
    constraint: Constraint,
    lines: Vec<ConstraintLine>,
    /// The single line (for the crossing with edges); `None` for the crossing of two lines.
    line: Option<(DVec3, DVec3, super::Rgb)>,
}

/// The engine. Keep one for the active tool.
#[derive(Clone, Debug)]
pub struct Inferencer {
    pub options: Options,
    hover: Option<Hover>,
    anchor: Option<DVec3>,
    lock: Option<Lock>,
    last: Option<Inference>,
    last_ref: Option<DVec3>,
}

impl Default for Inferencer {
    fn default() -> Self {
        Inferencer::new(Options::default())
    }
}

/// True when the part of the box in front of the near plane may be inside the pad around the
/// pixel. The box is cut at the near plane; a box fully behind the camera is never inside.
pub(super) fn overlaps_rect(cam: &Camera, b: &Aabb, c: DVec2, pad: f64) -> bool {
    let corners = b.corners();
    let depth: [f64; 8] = corners.map(|p| cam.depth(p));
    let near = cam.near;
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    let mut any = false;
    let mut add = |p: DVec3| {
        let px = cam.pixel(cam.hc(p));
        lo = lo.min(px);
        hi = hi.max(px);
        any = true;
    };
    for k in 0..8 {
        if depth[k] > near {
            add(corners[k]);
        }
        // The twelve edges of the box join the corners that differ in one bit of the index.
        for bit in [1usize, 2, 4] {
            let m = k | bit;
            if m != k && (depth[k] > near) != (depth[m] > near) {
                let t = ((near - depth[k]) / (depth[m] - depth[k])).clamp(0.0, 1.0);
                add(corners[k].lerp(corners[m], t));
            }
        }
    }
    any && hi.x >= c.x - pad && lo.x <= c.x + pad && hi.y >= c.y - pad && lo.y <= c.y + pad
}

/// What the engine looks at: the scenes, what to leave out and the tolerance.
struct Ctx<'a> {
    scenes: &'a [&'a Scene],
    skip: &'a dyn Fn(Tag) -> bool,
    eps: f64,
}

/// The nearest face along a ray.
struct Hit {
    t: f64,
    normal: DVec3,
    tag: Tag,
}

impl<'a> Ctx<'a> {
    fn new(scenes: &'a [&'a Scene], skip: &'a dyn Fn(Tag) -> bool) -> Ctx<'a> {
        let eps = scenes.iter().map(|s| s.eps()).fold(0.0, f64::max);
        Ctx { scenes, skip, eps }
    }

    /// The nearest face along the ray, over all scenes (an equal distance goes to the smaller
    /// tag).
    fn first_hit(&self, ray: &Ray) -> Option<Hit> {
        let mut best_hit: Option<Hit> = None;
        for scene in self.scenes {
            scene.bvh_faces.query(
                &|b| b.ray_enter(ray.origin, ray.dir, self.eps).is_some(),
                &mut |i| {
                    let f = &scene.faces[i as usize];
                    if (self.skip)(f.tag) {
                        return;
                    }
                    let Some(t) = ray_plane(ray.origin, ray.dir, f.origin(), f.normal) else {
                        return;
                    };
                    let p = ray.origin + ray.dir * t;
                    if !inside_loops(p, &f.loops, f.drop) {
                        return;
                    }
                    let better = match &best_hit {
                        None => true,
                        Some(h) => t < h.t || (t == h.t && f.tag < h.tag),
                    };
                    if better {
                        best_hit = Some(Hit {
                            t,
                            normal: f.normal,
                            tag: f.tag,
                        });
                    }
                },
            );
        }
        best_hit
    }

    /// False when a face hides the candidate: the first face along the ray through the pixel of
    /// the candidate is nearer than the candidate.
    fn visible(&self, cam: &Camera, c: &Cand) -> bool {
        let Some(px) = cam.project(c.point) else {
            return false;
        };
        let ray = cam.ray(px);
        let t = (c.point - ray.origin).dot(ray.dir);
        match self.first_hit(&ray) {
            None => true,
            Some(h) => h.t >= t - self.eps.max(1e-6 * t.abs()),
        }
    }

    /// The best candidate, in the order of `cmp_cand`, that passes `pred` and is not hidden.
    fn pick(&self, cam: &Camera, cands: &[Cand], pred: impl Fn(&Cand) -> bool) -> Option<Cand> {
        let mut v: Vec<Cand> = cands.iter().copied().filter(|c| pred(c)).collect();
        v.sort_by(cmp_cand);
        v.into_iter().find(|c| self.visible(cam, c))
    }
}

/// The point of the line nearest to the pixel on the screen (perspective correct).
fn line_point(cam: &Camera, cursor: DVec2, origin: DVec3, dir: DVec3) -> Option<DVec3> {
    let o = cam.hc(origin);
    if o.depth <= cam.near {
        return None;
    }
    let o2 = cam.pixel(o);
    let sample = cam.pixel_size_at(o.depth) * 200.0;
    let mut q = origin + dir * sample;
    if cam.depth(q) <= cam.near {
        q = origin - dir * sample;
        if cam.depth(q) <= cam.near {
            return None;
        }
    }
    let hq = cam.hc(q);
    let v = cam.pixel(hq) - o2;
    let l2 = v.length_squared();
    if l2 < 0.25 {
        return None;
    }
    let s = (cursor - o2).dot(v) / l2;
    let den = s * o.w + (1.0 - s) * hq.w;
    if den.abs() < 1e-300 {
        return None;
    }
    let t = s * o.w / den;
    let p = origin + (q - origin) * t;
    (p.is_finite() && cam.depth(p) > cam.near).then_some(p)
}

/// The direction of the perpendicular from `r` to the line of the edge.
fn perp_dir(r: DVec3, h: &Hover) -> Option<DVec3> {
    let e = (h.b - h.a).try_normalize()?;
    let foot = h.a + e * (r - h.a).dot(e);
    let d = foot - r;
    (d.length() > 1e-9 * (1.0 + r.abs().max_element())).then(|| d.normalize())
}

impl Inferencer {
    pub fn new(options: Options) -> Inferencer {
        Inferencer {
            options,
            hover: None,
            anchor: None,
            lock: None,
            last: None,
            last_ref: None,
        }
    }

    /// Forgets everything: locks, the hovered edge, the paused point. Call it when a tool starts
    /// or ends.
    pub fn reset(&mut self) {
        self.hover = None;
        self.anchor = None;
        self.lock = None;
        self.last = None;
        self.last_ref = None;
    }

    /// Drops the locks and the paused point but keeps the hovered edge. A tool calls this after a
    /// click when the next segment starts fresh.
    pub fn release_locks(&mut self) {
        self.lock = None;
        self.anchor = None;
    }

    /// The kind of the lock now in force.
    pub fn locked_kind(&self) -> Option<Kind> {
        self.lock.map(|l| l.kind)
    }

    /// The edge that parallel and perpendicular refer to: its ends and its tag.
    pub fn hovered_edge(&self) -> Option<(DVec3, DVec3, Tag)> {
        self.hover.map(|h| (h.a, h.b, h.tag))
    }

    /// The point that "from point" lines start from.
    pub fn anchor(&self) -> Option<DVec3> {
        self.anchor
    }

    /// Sets or clears the point that "from point" lines start from.
    pub fn set_anchor(&mut self, p: Option<DVec3>) {
        self.anchor = p.filter(|p| p.is_finite());
    }

    /// The last result.
    pub fn last(&self) -> Option<&Inference> {
        self.last.as_ref()
    }

    /// The pointer has rested: the point of the last inference becomes the start of the dotted
    /// lines along the axes ("from point"). Nothing happens when the last inference was `Free`.
    pub fn pause(&mut self) {
        if let Some(l) = &self.last
            && l.kind != Kind::Free
        {
            self.anchor = Some(l.point);
        }
    }

    /// Reacts to a key. Returns true when the lock changed (the caller should redraw).
    pub fn key(&mut self, key: Key) -> bool {
        match key {
            Key::Right => self.axis_lock(Axis::X),
            Key::Left => self.axis_lock(Axis::Y),
            Key::Up => self.axis_lock(Axis::Z),
            Key::Down => self.edge_lock(),
            Key::ShiftDown => self.shift_lock(),
            Key::ShiftUp => {
                if self.lock.is_some_and(|l| l.by_shift) {
                    self.lock = None;
                    true
                } else {
                    false
                }
            }
        }
    }

    fn axis_lock(&mut self, a: Axis) -> bool {
        if self.last_ref.is_none() {
            return false;
        }
        if self.lock.is_some_and(|l| l.kind == Kind::OnAxis(a)) {
            self.lock = None;
        } else {
            self.lock = Some(Lock {
                spec: Spec::Dir(a.dir()),
                kind: Kind::OnAxis(a),
                source: Tag::NONE,
                other: Tag::NONE,
                helper: None,
                by_shift: false,
            });
        }
        true
    }

    fn edge_lock(&mut self) -> bool {
        let Some(r) = self.last_ref else {
            return false;
        };
        let Some(h) = self.hover else {
            return false;
        };
        let Some(e) = (h.b - h.a).try_normalize() else {
            return false;
        };
        let parallel = Lock {
            spec: Spec::Dir(e),
            kind: Kind::Parallel,
            source: h.tag,
            other: Tag::NONE,
            helper: Some((h.a, h.b)),
            by_shift: false,
        };
        let perpendicular = perp_dir(r, &h).map(|d| Lock {
            spec: Spec::Dir(d),
            kind: Kind::Perpendicular,
            ..parallel
        });
        let current = self.lock.map(|l| l.kind);
        self.lock = match current {
            None => {
                if self
                    .last
                    .as_ref()
                    .is_some_and(|l| l.kind == Kind::Perpendicular)
                {
                    perpendicular.or(Some(parallel))
                } else {
                    Some(parallel)
                }
            }
            Some(Kind::Parallel) => perpendicular,
            Some(Kind::Perpendicular) => None,
            Some(_) => Some(parallel),
        };
        true
    }

    fn shift_lock(&mut self) -> bool {
        if self.lock.is_some() {
            return false;
        }
        let Some(l) = &self.last else {
            return false;
        };
        let Some(c) = l.constraint else {
            return false;
        };
        // With a reference point, a point (an endpoint, a midpoint or a center) locks the
        // direction from the reference point through it; without one, the point itself.
        let through = match (l.kind, self.last_ref) {
            (Kind::Endpoint | Kind::GuidePoint | Kind::Midpoint | Kind::Center, Some(r)) => {
                (l.point - r).try_normalize()
            }
            _ => None,
        };
        let (spec, kind) = match (l.kind, c, through) {
            (_, _, Some(dir)) => (Spec::Dir(dir), Kind::ThroughPoint),
            (
                Kind::OnAxis(_) | Kind::Parallel | Kind::Perpendicular,
                Constraint::Line { dir, .. },
                _,
            ) => (Spec::Dir(dir), l.kind),
            _ => (Spec::Fixed(c), l.kind),
        };
        let helper = matches!(l.kind, Kind::Parallel | Kind::Perpendicular)
            .then(|| self.hover.map(|h| (h.a, h.b)))
            .flatten();
        self.lock = Some(Lock {
            spec,
            kind,
            source: l.source,
            other: l.other,
            helper,
            by_shift: true,
        });
        true
    }

    /// The inference for the pointer at `cursor` (pixels). `reference` is the last point of the
    /// tool (`None` before the first click).
    pub fn infer(
        &mut self,
        scene: &Scene,
        cam: &Camera,
        cursor: DVec2,
        reference: Option<DVec3>,
    ) -> Inference {
        self.infer_scenes(&[scene], cam, cursor, reference, &|_| false)
    }

    /// Like [`Inferencer::infer`] over several scenes (one per model, so an edit rebuilds only
    /// its own scene) and with geometry left out: `skip` gets the tag of every point, edge and
    /// face and returns true for the ones that must not be offered or hide anything (the
    /// geometry that a move or a push/pull is working on).
    pub fn infer_scenes(
        &mut self,
        scenes: &[&Scene],
        cam: &Camera,
        cursor: DVec2,
        reference: Option<DVec3>,
        skip: &dyn Fn(Tag) -> bool,
    ) -> Inference {
        let ctx = Ctx::new(scenes, skip);
        let scene = &ctx;
        self.last_ref = reference;
        let active = self.lock.filter(|l| match l.spec {
            Spec::Dir(_) => reference.is_some(),
            Spec::Fixed(_) => true,
        });
        let res = match active {
            Some(l) => self.locked(&l, scene, cam, cursor, reference),
            None => self.natural(scene, cam, cursor, reference),
        };
        self.last = Some(res.clone());
        res
    }

    fn make(
        &self,
        cam: &Camera,
        cursor: DVec2,
        kind: Kind,
        point: DVec3,
        constraint: Option<Constraint>,
        lines: Vec<ConstraintLine>,
    ) -> Inference {
        Inference {
            point,
            kind,
            screen: cam.project(point).unwrap_or(cursor),
            color: kind.color(),
            label: kind.label(),
            lines,
            locked: false,
            source: Tag::NONE,
            other: Tag::NONE,
            constraint,
        }
    }

    fn cand_result(
        &self,
        cam: &Camera,
        cursor: DVec2,
        c: &Cand,
        lines: Vec<ConstraintLine>,
    ) -> Inference {
        let mut i = self.make(
            cam,
            cursor,
            c.kind,
            c.point,
            Some(Constraint::Point(c.point)),
            lines,
        );
        i.source = c.source;
        i.other = c.other;
        i
    }

    // ----- candidates -------------------------------------------------------------------------

    /// The edges near the pointer: (pixel distance, edge), nearest first. Also pushes the point
    /// candidates that edges give (when `cands` is given).
    fn edges_near(
        &self,
        ctx: &Ctx,
        cam: &Camera,
        cursor: DVec2,
        mut cands: Option<&mut Vec<Cand>>,
    ) -> Vec<(f64, EdgeItem)> {
        let o = &self.options;
        let pad = o.pad();
        let reach = o.px(o.edge_px.max(o.intersection_px));
        let (ep, mp, edp) = (o.px(o.endpoint_px), o.px(o.midpoint_px), o.px(o.edge_px));
        let mut near: Vec<(f64, EdgeItem)> = Vec::new();
        for scene in ctx.scenes {
            scene
                .bvh_edges
                .query(&|b| overlaps_rect(cam, b, cursor, pad), &mut |i| {
                    let e = scene.edges[i as usize];
                    if (ctx.skip)(e.tag) {
                        return;
                    }
                    let Some(seg) = cam.screen_seg(e.a, e.b) else {
                        return;
                    };
                    let (s, d) = nearest_on_screen_seg(cursor, seg.p[0], seg.p[1]);
                    if d <= reach {
                        near.push((d, e));
                    }
                    let Some(cands) = cands.as_deref_mut() else {
                        return;
                    };
                    // A guide line gives "on guide" only: its ends are no points to snap to.
                    if e.tag.kind() == TagKind::Guide {
                        if d <= edp {
                            cands.push(Cand {
                                kind: Kind::OnGuide,
                                point: e.a.lerp(e.b, seg.world_param(s)),
                                dist: d,
                                depth: cam.depth(e.a.lerp(e.b, seg.world_param(s))),
                                source: e.tag,
                                other: Tag::NONE,
                                edge: Some(e),
                                normal: None,
                            });
                        }
                        return;
                    }
                    let mut push = |kind: Kind, point: DVec3, dist: f64| {
                        cands.push(Cand {
                            kind,
                            point,
                            dist,
                            depth: cam.depth(point),
                            source: e.tag,
                            other: Tag::NONE,
                            edge: Some(e),
                            normal: None,
                        });
                    };
                    if seg.t[0] == 0.0 {
                        let dd = (seg.p[0] - cursor).length();
                        if dd <= ep {
                            push(Kind::Endpoint, e.a, dd);
                        }
                    }
                    if seg.t[1] == 1.0 {
                        let dd = (seg.p[1] - cursor).length();
                        if dd <= ep {
                            push(Kind::Endpoint, e.b, dd);
                        }
                    }
                    if seg.t[0] <= 0.5 && 0.5 <= seg.t[1] {
                        let m = e.a.lerp(e.b, 0.5);
                        if let Some(px) = cam.project(m) {
                            let dd = (px - cursor).length();
                            if dd <= mp {
                                push(Kind::Midpoint, m, dd);
                            }
                        }
                    }
                    if d <= edp {
                        push(Kind::OnEdge, e.a.lerp(e.b, seg.world_param(s)), d);
                    }
                });
        }
        // The same order for any order of the geometry.
        near.sort_by(|x, y| {
            x.0.total_cmp(&y.0)
                .then(x.1.tag.cmp(&y.1.tag))
                .then(x.1.a.x.total_cmp(&y.1.a.x))
                .then(x.1.a.y.total_cmp(&y.1.a.y))
                .then(x.1.a.z.total_cmp(&y.1.a.z))
                .then(x.1.b.x.total_cmp(&y.1.b.x))
                .then(x.1.b.y.total_cmp(&y.1.b.y))
                .then(x.1.b.z.total_cmp(&y.1.b.z))
        });
        near.truncate(o.max_near_edges);
        near
    }

    fn explicit_points(&self, ctx: &Ctx, cam: &Camera, cursor: DVec2, cands: &mut Vec<Cand>) {
        let o = &self.options;
        for scene in ctx.scenes {
            scene
                .bvh_points
                .query(&|b| overlaps_rect(cam, b, cursor, o.pad()), &mut |i| {
                    let it = &scene.points[i as usize];
                    if (ctx.skip)(it.tag) {
                        return;
                    }
                    let Some(px) = cam.project(it.p) else {
                        return;
                    };
                    let d = (px - cursor).length();
                    let (kind, r) = match it.kind {
                        PointKind::Endpoint => (Kind::Endpoint, o.endpoint_px),
                        PointKind::Guide => (Kind::GuidePoint, o.endpoint_px),
                        PointKind::Center => (Kind::Center, o.center_px),
                    };
                    if d <= o.px(r) {
                        cands.push(Cand {
                            kind,
                            point: it.p,
                            dist: d,
                            depth: cam.depth(it.p),
                            source: it.tag,
                            other: Tag::NONE,
                            edge: None,
                            normal: None,
                        });
                    }
                });
        }
    }

    /// The faces near the pointer: the nearest ones on the screen, at most `max_near_faces`.
    /// (The distance is from the pixel to the box of the projected outer loop; 0 when it is
    /// inside or the face reaches behind the near plane.)
    fn faces_near<'s>(&self, ctx: &Ctx<'s>, cam: &Camera, cursor: DVec2) -> Vec<&'s FaceItem> {
        let o = &self.options;
        let mut faces: Vec<(f64, &FaceItem)> = Vec::new();
        for scene in ctx.scenes {
            scene
                .bvh_faces
                .query(&|b| overlaps_rect(cam, b, cursor, o.pad()), &mut |i| {
                    let f = &scene.faces[i as usize];
                    if (ctx.skip)(f.tag) {
                        return;
                    }
                    faces.push((screen_distance(cam, f, cursor), f));
                });
        }
        faces.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then(a.1.tag.cmp(&b.1.tag))
                .then(a.1.loops[0][0].x.total_cmp(&b.1.loops[0][0].x))
                .then(a.1.loops[0][0].y.total_cmp(&b.1.loops[0][0].y))
                .then(a.1.loops[0][0].z.total_cmp(&b.1.loops[0][0].z))
        });
        faces.truncate(o.max_near_faces);
        faces.into_iter().map(|(_, f)| f).collect()
    }

    /// The nearest face along the pointer ray.
    fn face_hit(&self, ctx: &Ctx, cam: &Camera, cursor: DVec2) -> Option<Cand> {
        let ray = cam.ray(cursor);
        ctx.first_hit(&ray).map(|h| {
            let point = ray.origin + ray.dir * h.t;
            Cand {
                kind: Kind::OnFace,
                point,
                dist: 0.0,
                depth: cam.depth(point),
                source: h.tag,
                other: Tag::NONE,
                edge: None,
                normal: Some(h.normal),
            }
        })
    }

    /// Crossings of the edges near the pointer with each other and with the faces near them.
    fn crossings(
        &self,
        ctx: &Ctx,
        cam: &Camera,
        cursor: DVec2,
        near: &[(f64, EdgeItem)],
        cands: &mut Vec<Cand>,
    ) {
        let reach = self.options.px(self.options.intersection_px);
        let eps = ctx.eps;
        let mut add = |kind: Kind, p: DVec3, a: Tag, b: Tag| {
            if let Some(px) = cam.project(p) {
                let d = (px - cursor).length();
                if d <= reach {
                    cands.push(Cand {
                        kind,
                        point: p,
                        dist: d,
                        depth: cam.depth(p),
                        source: a,
                        other: b,
                        edge: None,
                        normal: None,
                    });
                }
            }
        };
        let inner = |t: f64| t > 1e-6 && t < 1.0 - 1e-6;
        for (k, (_, e1)) in near.iter().enumerate() {
            let d1 = e1.b - e1.a;
            for (_, e2) in near.iter().skip(k + 1) {
                let d2 = e2.b - e2.a;
                if let Some((s, t)) = line_line(e1.a, d1, e2.a, d2)
                    && inner(s)
                    && inner(t)
                {
                    let (p1, p2) = (e1.a + d1 * s, e2.a + d2 * t);
                    if p1.distance(p2) <= eps {
                        add(
                            Kind::Intersection,
                            p1,
                            e1.tag.min(e2.tag),
                            e1.tag.max(e2.tag),
                        );
                    }
                }
            }
        }
        if near.is_empty() {
            return;
        }
        for f in self.faces_near(ctx, cam, cursor) {
            for (_, e) in near {
                let (sa, sb) = (
                    (e.a - f.origin()).dot(f.normal),
                    (e.b - f.origin()).dot(f.normal),
                );
                if (sa > eps && sb < -eps) || (sa < -eps && sb > eps) {
                    let t = sa / (sa - sb);
                    if inner(t) {
                        let q = e.a.lerp(e.b, t);
                        if inside_loops(q, &f.loops, f.drop) {
                            add(Kind::Intersection, q, e.tag, f.tag);
                        }
                    }
                }
            }
        }
    }

    /// Where a line crosses an edge near the pointer (not hidden by a face).
    fn line_crossing(
        &self,
        ctx: &Ctx,
        cam: &Camera,
        cursor: DVec2,
        near: &[(f64, EdgeItem)],
        origin: DVec3,
        dir: DVec3,
    ) -> Option<Cand> {
        let reach = self.options.px(self.options.intersection_px);
        let eps = ctx.eps;
        let mut found = Vec::new();
        for (_, e) in near {
            let d = e.b - e.a;
            let Some((s, t)) = line_line(origin, dir, e.a, d) else {
                continue;
            };
            if !(-1e-9..=1.0 + 1e-9).contains(&t) {
                continue;
            }
            let (p1, p2) = (origin + dir * s, e.a + d * t);
            if p1.distance(p2) > eps {
                continue;
            }
            let Some(px) = cam.project(p2) else {
                continue;
            };
            let dist = (px - cursor).length();
            if dist <= reach {
                found.push(Cand {
                    kind: Kind::Intersection,
                    point: p2,
                    dist,
                    depth: cam.depth(p2),
                    source: e.tag,
                    other: Tag::NONE,
                    edge: Some(*e),
                    normal: None,
                });
            }
        }
        ctx.pick(cam, &found, |_| true)
    }

    // ----- linear inferences ------------------------------------------------------------------

    fn eval_line(
        &self,
        cam: &Camera,
        cursor: DVec2,
        origin: DVec3,
        dir: DVec3,
    ) -> Option<(f64, DVec3)> {
        let o = cam.hc(origin);
        if o.depth <= cam.near {
            return None;
        }
        let o2 = cam.pixel(o);
        let sample = cam.pixel_size_at(o.depth) * 200.0;
        let mut q = origin + dir * sample;
        if cam.depth(q) <= cam.near {
            q = origin - dir * sample;
        }
        let q2 = cam.project(q)?;
        let (v, u) = (q2 - o2, cursor - o2);
        if v.length() < 0.5 || u.length() < self.options.px(self.options.axis_min_px) {
            return None;
        }
        let angle = v.perp_dot(u).abs().atan2(v.dot(u).abs());
        if angle > self.options.axis_angle_deg.to_radians() {
            return None;
        }
        Some((angle, line_point(cam, cursor, origin, dir)?))
    }

    fn linear(&self, ctx: &Ctx, cam: &Camera, cursor: DVec2, r: DVec3) -> Option<Linear> {
        let mut lines: Vec<LineCand> = Vec::new();
        let mut try_line =
            |kind: Kind, origin: DVec3, dir: DVec3, order: u8, from_point: bool, helper| {
                if let Some((angle, point)) = self.eval_line(cam, cursor, origin, dir) {
                    lines.push(LineCand {
                        kind,
                        origin,
                        dir,
                        angle,
                        point,
                        order,
                        from_point,
                        helper,
                    });
                }
            };
        for (n, a) in Axis::ALL.iter().enumerate() {
            try_line(Kind::OnAxis(*a), r, a.dir(), n as u8, false, None);
        }
        let off_axes = |d: DVec3| Axis::ALL.iter().all(|a| a.dir().cross(d).length() > 1e-6);
        if let Some(h) = &self.hover
            && let Some(e) = (h.b - h.a).try_normalize()
        {
            if off_axes(e) {
                try_line(Kind::Parallel, r, e, 3, false, Some((h.a, h.b)));
            }
            if let Some(d) = perp_dir(r, h)
                && off_axes(d)
            {
                try_line(Kind::Perpendicular, r, d, 4, false, Some((h.a, h.b)));
            }
        }
        if let Some(anchor) = self.anchor {
            for (n, a) in Axis::ALL.iter().enumerate() {
                try_line(
                    Kind::FromPoint(*a),
                    anchor,
                    a.dir(),
                    5 + n as u8,
                    true,
                    None,
                );
            }
        }
        if lines.is_empty() {
            return None;
        }
        // Two lines, one from the reference point and one from the paused point, that cross.
        let reach = self.options.px(self.options.intersection_px) * 2.0;
        let eps = ctx.eps;
        let mut pair: Option<(f64, u8, u8, usize, usize, DVec3)> = None;
        for (i, a) in lines.iter().enumerate().filter(|(_, l)| !l.from_point) {
            for (j, b) in lines.iter().enumerate().filter(|(_, l)| l.from_point) {
                let Some((s, t)) = line_line(a.origin, a.dir, b.origin, b.dir) else {
                    continue;
                };
                let (p1, p2) = (a.origin + a.dir * s, b.origin + b.dir * t);
                if p1.distance(p2) > eps {
                    continue;
                }
                let Some(px) = cam.project(p1) else {
                    continue;
                };
                if (px - cursor).length() > reach {
                    continue;
                }
                let key = (a.angle + b.angle, a.order, b.order, i, j, p1);
                let better = pair.is_none_or(|p| {
                    key.0
                        .total_cmp(&p.0)
                        .then(key.1.cmp(&p.1))
                        .then(key.2.cmp(&p.2))
                        == Ordering::Less
                });
                if better {
                    pair = Some(key);
                }
            }
        }
        if let Some((_, _, _, i, j, p)) = pair {
            let (a, b) = (&lines[i], &lines[j]);
            return Some(Linear {
                kind: b.kind,
                point: p,
                constraint: Constraint::Point(p),
                lines: vec![
                    ConstraintLine {
                        from: a.origin,
                        to: p,
                        color: a.kind.color(),
                        dotted: false,
                    },
                    ConstraintLine {
                        from: b.origin,
                        to: p,
                        color: b.kind.color(),
                        dotted: true,
                    },
                ],
                line: None,
            });
        }
        let l = lines
            .iter()
            .min_by(|a, b| a.angle.total_cmp(&b.angle).then(a.order.cmp(&b.order)))?;
        let mut out = vec![ConstraintLine {
            from: l.origin,
            to: l.point,
            color: l.kind.color(),
            dotted: l.from_point,
        }];
        if let Some((a, b)) = l.helper {
            out.push(ConstraintLine {
                from: a,
                to: b,
                color: l.kind.color(),
                dotted: true,
            });
        }
        Some(Linear {
            kind: l.kind,
            point: l.point,
            constraint: Constraint::Line {
                origin: l.origin,
                dir: l.dir,
            },
            lines: out,
            line: Some((l.origin, l.dir, l.kind.color())),
        })
    }

    // ----- the two flows ----------------------------------------------------------------------

    fn natural(
        &mut self,
        ctx: &Ctx,
        cam: &Camera,
        cursor: DVec2,
        reference: Option<DVec3>,
    ) -> Inference {
        let mut cands: Vec<Cand> = Vec::new();
        self.explicit_points(ctx, cam, cursor, &mut cands);
        let near = self.edges_near(ctx, cam, cursor, Some(&mut cands));
        self.crossings(ctx, cam, cursor, &near, &mut cands);

        // Points first: endpoint, center, midpoint, intersection. A point behind a face is not
        // offered.
        let mut result: Option<Inference> = None;
        if let Some(c) = ctx.pick(cam, &cands, |c| c.kind.is_point()) {
            result = Some(self.cand_result(cam, cursor, &c, Vec::new()));
            self.remember(&c);
        }
        let linear = if result.is_none() {
            reference.and_then(|r| self.linear(ctx, cam, cursor, r))
        } else {
            None
        };
        // A line that crosses an edge near the pointer gives an intersection.
        if result.is_none()
            && let Some(l) = &linear
            && let Some((origin, dir, color)) = l.line
            && let Some(c) = self.line_crossing(ctx, cam, cursor, &near, origin, dir)
        {
            let lines = vec![ConstraintLine {
                from: origin,
                to: c.point,
                color,
                dotted: false,
            }];
            let mut i = self.cand_result(cam, cursor, &c, lines);
            i.constraint = Some(Constraint::Point(c.point));
            result = Some(i);
        }
        if result.is_none()
            && let Some(c) = ctx.pick(cam, &cands, |c| {
                matches!(c.kind, Kind::OnEdge | Kind::OnGuide)
            })
        {
            let mut i = self.cand_result(cam, cursor, &c, Vec::new());
            if let Some(e) = c.edge {
                i.constraint = Some(Constraint::Line {
                    origin: e.a,
                    dir: (e.b - e.a).normalize_or_zero(),
                });
            }
            self.remember(&c);
            result = Some(i);
        }
        if result.is_none()
            && let Some(l) = linear
        {
            let mut i = self.make(cam, cursor, l.kind, l.point, Some(l.constraint), l.lines);
            if let Some(h) = self.hover
                && matches!(l.kind, Kind::Parallel | Kind::Perpendicular)
            {
                i.source = h.tag;
            }
            result = Some(i);
        }
        if result.is_none()
            && let Some(c) = self.face_hit(ctx, cam, cursor)
        {
            let mut i = self.cand_result(cam, cursor, &c, Vec::new());
            if let Some(normal) = c.normal {
                i.constraint = Some(Constraint::Plane {
                    origin: c.point,
                    normal,
                });
            }
            result = Some(i);
        }
        result.unwrap_or_else(|| self.free(cam, cursor))
    }

    fn free(&self, cam: &Camera, cursor: DVec2) -> Inference {
        let ray = cam.ray(cursor);
        let ground = self
            .options
            .ground_z
            .and_then(|z| ray_plane(ray.origin, ray.dir, DVec3::new(0.0, 0.0, z), DVec3::Z));
        let point = match ground {
            Some(t) => ray.origin + ray.dir * t,
            None => ray.origin + ray.dir * self.options.free_distance,
        };
        self.make(cam, cursor, Kind::Free, point, None, Vec::new())
    }

    fn remember(&mut self, c: &Cand) {
        if matches!(c.kind, Kind::OnEdge | Kind::Endpoint | Kind::Midpoint)
            && let Some(e) = c.edge
        {
            self.hover = Some(Hover {
                a: e.a,
                b: e.b,
                tag: e.tag,
            });
        }
    }

    fn locked(
        &self,
        lock: &Lock,
        ctx: &Ctx,
        cam: &Camera,
        cursor: DVec2,
        reference: Option<DVec3>,
    ) -> Inference {
        let constraint = match lock.spec {
            Spec::Dir(d) => Constraint::Line {
                origin: reference.unwrap_or(DVec3::ZERO),
                dir: d,
            },
            Spec::Fixed(c) => c,
        };
        let ray = cam.ray(cursor);
        let mut lines = Vec::new();
        let (kind, point, constraint, source, other) = match constraint {
            Constraint::Point(p) => (lock.kind, p, constraint, lock.source, lock.other),
            Constraint::Plane { origin, normal } => {
                let p = ray_plane(ray.origin, ray.dir, origin, normal)
                    .map(|t| ray.origin + ray.dir * t)
                    .unwrap_or(origin);
                (
                    lock.kind,
                    p,
                    Constraint::Plane { origin: p, normal },
                    lock.source,
                    lock.other,
                )
            }
            Constraint::Line { origin, dir } => {
                let mut cands: Vec<Cand> = Vec::new();
                self.explicit_points(ctx, cam, cursor, &mut cands);
                let near = self.edges_near(ctx, cam, cursor, Some(&mut cands));
                let color = lock.kind.color();
                let push_helper = |lines: &mut Vec<ConstraintLine>| {
                    if let Some((a, b)) = lock.helper {
                        lines.push(ConstraintLine {
                            from: a,
                            to: b,
                            color,
                            dotted: true,
                        });
                    }
                };
                let solid = |lines: &mut Vec<ConstraintLine>, p: DVec3| {
                    if p.distance(origin) > 0.0 && matches!(lock.spec, Spec::Dir(_)) {
                        lines.push(ConstraintLine {
                            from: origin,
                            to: p,
                            color,
                            dotted: false,
                        });
                    }
                };
                // A line that crosses an edge near the pointer.
                if let Some(c) = self.line_crossing(ctx, cam, cursor, &near, origin, dir) {
                    solid(&mut lines, c.point);
                    push_helper(&mut lines);
                    let mut i = self.cand_result(cam, cursor, &c, lines);
                    i.constraint = Some(Constraint::Point(c.point));
                    i.locked = true;
                    return i;
                }
                // An endpoint, midpoint or center under the pointer: its place along the line
                // (the end of the perpendicular from the point to the line), with a dotted
                // helper from the point to it.
                let snap = ctx.pick(cam, &cands, |c| {
                    matches!(
                        c.kind,
                        Kind::Endpoint | Kind::GuidePoint | Kind::Midpoint | Kind::Center
                    )
                });
                if let Some(c) = snap
                    && let Some(d) = dir.try_normalize()
                {
                    let p = origin + d * (c.point - origin).dot(d);
                    solid(&mut lines, p);
                    if p.distance(c.point) > ctx.eps {
                        lines.push(ConstraintLine {
                            from: c.point,
                            to: p,
                            color,
                            dotted: true,
                        });
                    }
                    push_helper(&mut lines);
                    let mut i = self.make(
                        cam,
                        cursor,
                        lock.kind,
                        p,
                        Some(Constraint::Line { origin, dir }),
                        lines,
                    );
                    i.locked = true;
                    i.source = c.source;
                    return i;
                }
                let p = line_point(cam, cursor, origin, dir)
                    .or_else(|| {
                        line_line(ray.origin, ray.dir, origin, dir).map(|(_, t)| origin + dir * t)
                    })
                    .unwrap_or(origin);
                solid(&mut lines, p);
                push_helper(&mut lines);
                (lock.kind, p, constraint, lock.source, lock.other)
            }
        };
        let mut i = self.make(cam, cursor, kind, point, Some(constraint), lines);
        i.locked = true;
        i.source = source;
        i.other = other;
        i
    }
}

/// The distance in pixels from the pixel to the box of the projected outer loop of a face (0
/// inside the box, and when a corner is not in front of the near plane).
fn screen_distance(cam: &Camera, f: &FaceItem, cursor: DVec2) -> f64 {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in &f.loops[0] {
        let h = cam.hc(*p);
        if h.depth <= cam.near {
            return 0.0;
        }
        let px = cam.pixel(h);
        lo = lo.min(px);
        hi = hi.max(px);
    }
    let d = (lo - cursor).max(cursor - hi).max(DVec2::ZERO);
    d.length()
}
