//! The inference engine: candidates, priorities, linear inferences and locks.

use std::cmp::Ordering;

use glam::{DVec2, DVec3};

use super::bvh::Aabb;
use super::camera::Camera;
use super::math::{inside_loops, line_line, nearest_on_screen_seg, ray_plane};
use super::scene::{PointKind, Scene, Tag};
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
    edge: Option<usize>,
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

fn best(cands: impl Iterator<Item = Cand>) -> Option<Cand> {
    cands.min_by(cmp_cand)
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

/// True when the box may be inside the pad around the pixel.
fn overlaps_rect(cam: &Camera, b: &Aabb, c: DVec2, pad: f64) -> bool {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for corner in b.corners() {
        let h = cam.hc(corner);
        if h.depth <= cam.near {
            return true;
        }
        let p = cam.pixel(h);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    hi.x >= c.x - pad && lo.x <= c.x + pad && hi.y >= c.y - pad && lo.y <= c.y + pad
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
        let spec = match (l.kind, c) {
            (
                Kind::OnAxis(_) | Kind::Parallel | Kind::Perpendicular,
                Constraint::Line { dir, .. },
            ) => Spec::Dir(dir),
            _ => Spec::Fixed(c),
        };
        let helper = matches!(l.kind, Kind::Parallel | Kind::Perpendicular)
            .then(|| self.hover.map(|h| (h.a, h.b)))
            .flatten();
        self.lock = Some(Lock {
            spec,
            kind: l.kind,
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

    /// The edges near the pointer: (pixel distance, index), nearest first. Also pushes the point
    /// candidates that edges give (when `cands` is given).
    fn edges_near(
        &self,
        scene: &Scene,
        cam: &Camera,
        cursor: DVec2,
        mut cands: Option<&mut Vec<Cand>>,
    ) -> Vec<(f64, usize)> {
        let o = &self.options;
        let pad = o.pad();
        let reach = o.px(o.edge_px.max(o.intersection_px));
        let (ep, mp, edp) = (o.px(o.endpoint_px), o.px(o.midpoint_px), o.px(o.edge_px));
        let mut near: Vec<(f64, usize)> = Vec::new();
        scene
            .bvh_edges
            .query(&|b| overlaps_rect(cam, b, cursor, pad), &mut |i| {
                let e = &scene.edges[i as usize];
                let Some(seg) = cam.screen_seg(e.a, e.b) else {
                    return;
                };
                let (s, d) = nearest_on_screen_seg(cursor, seg.p[0], seg.p[1]);
                if d <= reach {
                    near.push((d, i as usize));
                }
                let Some(cands) = cands.as_deref_mut() else {
                    return;
                };
                let mut push = |kind: Kind, point: DVec3, dist: f64| {
                    cands.push(Cand {
                        kind,
                        point,
                        dist,
                        depth: cam.depth(point),
                        source: e.tag,
                        other: Tag::NONE,
                        edge: Some(i as usize),
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
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        near.truncate(o.max_near_edges);
        near
    }

    fn explicit_points(&self, scene: &Scene, cam: &Camera, cursor: DVec2, cands: &mut Vec<Cand>) {
        let o = &self.options;
        scene
            .bvh_points
            .query(&|b| overlaps_rect(cam, b, cursor, o.pad()), &mut |i| {
                let it = &scene.points[i as usize];
                let Some(px) = cam.project(it.p) else {
                    return;
                };
                let d = (px - cursor).length();
                let (kind, r) = match it.kind {
                    PointKind::Endpoint => (Kind::Endpoint, o.endpoint_px),
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
                    });
                }
            });
    }

    /// The faces near the pointer, in index order.
    fn faces_near(&self, scene: &Scene, cam: &Camera, cursor: DVec2) -> Vec<usize> {
        let o = &self.options;
        let mut faces = Vec::new();
        scene
            .bvh_faces
            .query(&|b| overlaps_rect(cam, b, cursor, o.pad()), &mut |i| {
                faces.push(i as usize)
            });
        faces.sort_unstable();
        faces.truncate(o.max_near_faces);
        faces
    }

    /// The nearest face along the ray.
    fn face_hit(&self, scene: &Scene, cam: &Camera, cursor: DVec2) -> Option<Cand> {
        let ray = cam.ray(cursor);
        let mut best_hit: Option<(f64, usize)> = None;
        scene.bvh_faces.query(
            &|b| b.ray_enter(ray.origin, ray.dir, scene.eps()).is_some(),
            &mut |i| {
                let f = &scene.faces[i as usize];
                let Some(t) = ray_plane(ray.origin, ray.dir, f.origin(), f.normal) else {
                    return;
                };
                let p = ray.origin + ray.dir * t;
                if !inside_loops(p, &f.loops, f.drop) {
                    return;
                }
                let better = match best_hit {
                    None => true,
                    Some((bt, bi)) => t < bt || (t == bt && f.tag < scene.faces[bi].tag),
                };
                if better {
                    best_hit = Some((t, i as usize));
                }
            },
        );
        best_hit.map(|(t, i)| Cand {
            kind: Kind::OnFace,
            point: ray.origin + ray.dir * t,
            dist: 0.0,
            depth: cam.depth(ray.origin + ray.dir * t),
            source: scene.faces[i].tag,
            other: Tag::NONE,
            edge: None,
        })
    }

    /// Crossings of the edges near the pointer with each other and with the faces near them.
    fn crossings(
        &self,
        scene: &Scene,
        cam: &Camera,
        cursor: DVec2,
        near: &[(f64, usize)],
        cands: &mut Vec<Cand>,
    ) {
        let reach = self.options.px(self.options.intersection_px);
        let eps = scene.eps();
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
                    });
                }
            }
        };
        let inner = |t: f64| t > 1e-6 && t < 1.0 - 1e-6;
        for (k, (_, i)) in near.iter().enumerate() {
            let e1 = &scene.edges[*i];
            let d1 = e1.b - e1.a;
            for (_, j) in near.iter().skip(k + 1) {
                let e2 = &scene.edges[*j];
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
        for fi in self.faces_near(scene, cam, cursor) {
            let f = &scene.faces[fi];
            for (_, i) in near {
                let e = &scene.edges[*i];
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

    /// Where a line crosses an edge near the pointer.
    fn line_crossing(
        &self,
        scene: &Scene,
        cam: &Camera,
        cursor: DVec2,
        near: &[(f64, usize)],
        origin: DVec3,
        dir: DVec3,
    ) -> Option<Cand> {
        let reach = self.options.px(self.options.intersection_px);
        let eps = scene.eps();
        let mut found = Vec::new();
        for (_, i) in near {
            let e = &scene.edges[*i];
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
                    edge: Some(*i),
                });
            }
        }
        best(found.into_iter())
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

    fn linear(&self, scene: &Scene, cam: &Camera, cursor: DVec2, r: DVec3) -> Option<Linear> {
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
        let eps = scene.eps();
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
        scene: &Scene,
        cam: &Camera,
        cursor: DVec2,
        reference: Option<DVec3>,
    ) -> Inference {
        let mut cands: Vec<Cand> = Vec::new();
        self.explicit_points(scene, cam, cursor, &mut cands);
        let near = self.edges_near(scene, cam, cursor, Some(&mut cands));
        self.crossings(scene, cam, cursor, &near, &mut cands);

        // Points first: endpoint, center, midpoint, intersection.
        let mut result: Option<Inference> = None;
        if let Some(c) = best(cands.iter().copied().filter(|c| c.kind.is_point())) {
            result = Some(self.cand_result(cam, cursor, &c, Vec::new()));
            self.remember(scene, &c);
        }
        let linear = if result.is_none() {
            reference.and_then(|r| self.linear(scene, cam, cursor, r))
        } else {
            None
        };
        // A line that crosses an edge near the pointer gives an intersection.
        if result.is_none()
            && let Some(l) = &linear
            && let Some((origin, dir, color)) = l.line
            && let Some(c) = self.line_crossing(scene, cam, cursor, &near, origin, dir)
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
            && let Some(c) = best(cands.iter().copied().filter(|c| c.kind == Kind::OnEdge))
        {
            let mut i = self.cand_result(cam, cursor, &c, Vec::new());
            if let Some(ei) = c.edge {
                let e = &scene.edges[ei];
                i.constraint = Some(Constraint::Line {
                    origin: e.a,
                    dir: (e.b - e.a).normalize_or_zero(),
                });
            }
            self.remember(scene, &c);
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
            && let Some(c) = self.face_hit(scene, cam, cursor)
        {
            let f = scene.faces.iter().find(|f| f.tag == c.source);
            let mut i = self.cand_result(cam, cursor, &c, Vec::new());
            if let Some(f) = f {
                i.constraint = Some(Constraint::Plane {
                    origin: c.point,
                    normal: f.normal,
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

    fn remember(&mut self, scene: &Scene, c: &Cand) {
        if matches!(c.kind, Kind::OnEdge | Kind::Endpoint | Kind::Midpoint)
            && let Some(i) = c.edge
        {
            let e = &scene.edges[i];
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
        scene: &Scene,
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
                let p = line_point(cam, cursor, origin, dir)
                    .or_else(|| {
                        line_line(ray.origin, ray.dir, origin, dir).map(|(_, t)| origin + dir * t)
                    })
                    .unwrap_or(origin);
                let color = lock.kind.color();
                if p.distance(origin) > 0.0 && matches!(lock.spec, Spec::Dir(_)) {
                    lines.push(ConstraintLine {
                        from: origin,
                        to: p,
                        color,
                        dotted: false,
                    });
                }
                if let Some((a, b)) = lock.helper {
                    lines.push(ConstraintLine {
                        from: a,
                        to: b,
                        color,
                        dotted: true,
                    });
                }
                let near = self.edges_near(scene, cam, cursor, None);
                if let Some(c) = self.line_crossing(scene, cam, cursor, &near, origin, dir) {
                    let mut i = self.cand_result(cam, cursor, &c, lines);
                    i.constraint = Some(Constraint::Point(c.point));
                    i.locked = true;
                    return i;
                }
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
