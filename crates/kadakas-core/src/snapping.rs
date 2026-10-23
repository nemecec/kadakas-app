//! Snapping math: where the pointer lands when it is near something.
//!
//! The words follow the Snapping palette of Vectorworks, in our own implementation:
//! - Object points: endpoints and corners, centers, circle quadrants (`SnapSet::object_points`) and
//!   midpoints (`midpoints`).
//! - Intersections of edges (`intersections`) and the nearest point on an edge (`edges`).
//! - Perpendicular and tangent points, seen from the last point you clicked (`perpendicular`, `tangent`).
//! - Angle and distance from the last clicked point (`angle`, `distance`).
//! - Smart points and smart edges: points and edges that you acquired (up to 3 and 2) give
//!   alignment lines. The pointer snaps to a line, or to the crossing of two lines (`smart_points`,
//!   `smart_edges`).
//! - The grid (`grid`).
//!
//! `snap` tries the kinds in that order and returns the first one that is within `Context::tol` of
//! the pointer (the grid always applies). The caller builds a [`Geometry`] of the objects near the
//! pointer, so the work stays small in big drawings. `Snap::cue` is the text for the label at the
//! cursor, and `Snap::guides` are the alignment lines to draw.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::edit2d::piece::{Chain, Piece, piece_intersections};
use crate::model::{Document, Object, ObjectId, Shape};

/// What a snap found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Grid,
    Endpoint,
    Midpoint,
    Center,
    Quadrant,
    Intersection,
    Edge,
    Perpendicular,
    Tangent,
    Angle,
    Distance,
    SmartPoint,
    SmartEdge,
}

impl Kind {
    /// The name in the cue label at the cursor.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Grid => "Grid",
            Kind::Endpoint => "Endpoint",
            Kind::Midpoint => "Midpoint",
            Kind::Center => "Center",
            Kind::Quadrant => "Quadrant",
            Kind::Intersection => "Intersection",
            Kind::Edge => "On edge",
            Kind::Perpendicular => "Perpendicular",
            Kind::Tangent => "Tangent",
            Kind::Angle => "Angle",
            Kind::Distance => "Distance",
            Kind::SmartPoint => "Smart point",
            Kind::SmartEdge => "Smart edge",
        }
    }
}

/// The categories of the Snapping palette. Each one can be on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapSet {
    pub grid: bool,
    /// Endpoints, corners, centers and quadrant points.
    pub object_points: bool,
    pub midpoints: bool,
    pub intersections: bool,
    /// The nearest point on an edge.
    pub edges: bool,
    pub perpendicular: bool,
    pub tangent: bool,
    pub angle: bool,
    pub distance: bool,
    pub smart_points: bool,
    pub smart_edges: bool,
}

impl Default for SnapSet {
    fn default() -> Self {
        SnapSet {
            grid: true,
            object_points: true,
            midpoints: true,
            intersections: true,
            edges: false,
            perpendicular: false,
            tangent: false,
            angle: false,
            distance: false,
            smart_points: true,
            smart_edges: false,
        }
    }
}

impl SnapSet {
    pub const NAMES: [&'static str; 11] = [
        "grid",
        "object_points",
        "midpoints",
        "intersections",
        "edges",
        "perpendicular",
        "tangent",
        "angle",
        "distance",
        "smart_points",
        "smart_edges",
    ];

    /// Nothing on.
    pub const OFF: SnapSet = SnapSet {
        grid: false,
        object_points: false,
        midpoints: false,
        intersections: false,
        edges: false,
        perpendicular: false,
        tangent: false,
        angle: false,
        distance: false,
        smart_points: false,
        smart_edges: false,
    };

    pub fn get(&self, name: &str) -> Option<bool> {
        Some(match name {
            "grid" => self.grid,
            "object_points" => self.object_points,
            "midpoints" => self.midpoints,
            "intersections" => self.intersections,
            "edges" => self.edges,
            "perpendicular" => self.perpendicular,
            "tangent" => self.tangent,
            "angle" => self.angle,
            "distance" => self.distance,
            "smart_points" => self.smart_points,
            "smart_edges" => self.smart_edges,
            _ => return None,
        })
    }

    /// Turns one category on or off. False if the name is not one.
    pub fn set(&mut self, name: &str, on: bool) -> bool {
        let slot = match name {
            "grid" => &mut self.grid,
            "object_points" => &mut self.object_points,
            "midpoints" => &mut self.midpoints,
            "intersections" => &mut self.intersections,
            "edges" => &mut self.edges,
            "perpendicular" => &mut self.perpendicular,
            "tangent" => &mut self.tangent,
            "angle" => &mut self.angle,
            "distance" => &mut self.distance,
            "smart_points" => &mut self.smart_points,
            "smart_edges" => &mut self.smart_edges,
            _ => return false,
        };
        *slot = on;
        true
    }

    /// True if at least one category is on.
    pub fn any(&self) -> bool {
        Self::NAMES.iter().any(|n| self.get(n) == Some(true))
    }
}

/// The settings of the snaps (the Snapping set and its preferences). Saved per user by the app.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub set: SnapSet,
    /// The step of the angle snap, in radians (default 15 degrees).
    pub angle_step: f64,
    /// Count angles from the direction of the segment before (the `/` key), not from the x axis.
    pub relative_angles: bool,
    /// The step of the distance snap in mm. 0 uses the step of the grid.
    pub distance_step: f64,
    /// Snap to objects on other layers than the active one.
    pub other_layers: bool,
    /// Snap to objects of grey layers and classes.
    pub grey: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            set: SnapSet::default(),
            angle_step: PI / 12.0,
            relative_angles: false,
            distance_step: 0.0,
            other_layers: true,
            grey: false,
        }
    }
}

impl Settings {
    /// True if the numbers are usable.
    pub fn problem(&self) -> Option<&'static str> {
        if !(self.angle_step.is_finite() && self.angle_step > 1e-6 && self.angle_step <= PI) {
            return Some("angle_step must be above 0 and at most 180 degrees (in radians)");
        }
        if !(self.distance_step.is_finite() && self.distance_step >= 0.0) {
            return Some("distance_step must be 0 or more");
        }
        None
    }
}

/// A point to snap to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub p: DVec2,
    pub kind: Kind,
}

/// Points and edges near the pointer.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub points: Vec<Pt>,
    pub edges: Vec<Piece>,
}

fn polygon_edges(v: &[crate::curve::Vertex], closed: bool, g: &mut Geometry) {
    if let Some(chain) = Chain::from_vertices(v, closed) {
        g.add_chain(&chain);
    }
}

impl Geometry {
    pub fn add_point(&mut self, p: DVec2, kind: Kind) {
        if p.is_finite() {
            self.points.push(Pt { p, kind });
        }
    }

    /// The ends and the middles of the pieces, and their edges.
    pub fn add_chain(&mut self, chain: &Chain) {
        for piece in &chain.pieces {
            self.add_point(piece.from(), Kind::Endpoint);
            self.add_point(piece.point(0.5), Kind::Midpoint);
            if let Piece::Arc { c, .. } = piece {
                self.add_point(*c, Kind::Center);
            }
            self.edges.push(*piece);
        }
        if !chain.closed {
            self.add_point(chain.end(), Kind::Endpoint);
        }
    }

    /// The snap points and edges of one shape.
    pub fn add_shape(&mut self, shape: &Shape) {
        use Kind::*;
        match shape {
            Shape::Line { .. } | Shape::Arc { .. } | Shape::Polyline { .. } => {
                if let Some(c) = Chain::from_shape(shape) {
                    self.add_chain(&c);
                }
            }
            Shape::Rect { min, max } => {
                if let Some(c) = Chain::from_shape(shape) {
                    self.add_chain(&c);
                }
                self.add_point((*min + *max) / 2.0, Center);
            }
            Shape::Circle { center, radius } => {
                self.add_point(*center, Center);
                for d in [DVec2::X, DVec2::Y, -DVec2::X, -DVec2::Y] {
                    self.add_point(*center + d * *radius, Quadrant);
                }
                if let Some(c) = Chain::circle(*center, *radius) {
                    self.edges.extend(c.pieces);
                }
            }
            Shape::LandscapeArea { boundary, .. }
            | Shape::Hardscape { boundary, .. }
            | Shape::SiteModifier { boundary, .. } => polygon_edges(boundary, true, self),
            Shape::Massing(m) => polygon_edges(&m.footprint, true, self),
            Shape::Extrude(e) => {
                polygon_edges(&e.profile, true, self);
                for h in &e.holes {
                    polygon_edges(h, true, self);
                }
            }
            Shape::Sweep(_) => {
                let (lo, hi) = shape.bounds();
                self.add_box(lo, hi);
            }
            Shape::Mesh3d(m) => {
                let (lo, hi) = m.bounds3();
                self.add_box(lo.truncate(), hi.truncate());
            }
            Shape::SurfaceModel(m) => {
                for (_, p) in m.vertices().take(5000) {
                    self.add_point(m.transform.transform_point3(p).truncate(), Endpoint);
                }
            }
            // The shape alone does not know its definition: the origin of the instance.
            Shape::Component { transform, .. } => {
                self.add_point(DVec2::new(transform[9], transform[10]), Endpoint)
            }
            Shape::Underlay(u) => self.add_box(u.min, u.max),
            Shape::Viewport(vp) => self.add_box(vp.min, vp.max),
            Shape::SiteModel { points, lines, .. } => {
                for p in points
                    .iter()
                    .chain(lines.iter().flat_map(|l| &l.points))
                    .take(5000)
                {
                    self.add_point(p.truncate(), Endpoint);
                }
            }
            Shape::Locus3d { p } => self.add_point(p.truncate(), Endpoint),
            Shape::Polyline3d { points, .. } => {
                for p in points {
                    self.add_point(p.truncate(), Endpoint);
                }
            }
            Shape::Text { at, .. } => self.add_point(*at, Endpoint),
            Shape::Plant { at, .. } | Shape::ExistingTree { at, .. } => self.add_point(*at, Center),
            Shape::PlantTag { at, anchor, .. } => {
                self.add_point(*at, Endpoint);
                self.add_point(*anchor, Endpoint);
            }
            Shape::Symbol { at, .. } => self.add_point(*at, Center),
            Shape::TitleBlock(_)
            | Shape::Group { .. }
            | Shape::Unknown { .. }
            | Shape::Guide(_)
            | Shape::Annotation(_)
            | Shape::Irrigation(_) => {}
        }
    }

    fn add_box(&mut self, lo: DVec2, hi: DVec2) {
        let c = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
        for i in 0..4 {
            self.add_point(c[i], Kind::Endpoint);
            self.edges.push(Piece::line(c[i], c[(i + 1) % 4]));
        }
    }

    /// The geometry of the objects of a document that the pointer can snap to, near `near`
    /// (center and radius) if given. Objects in `exclude` are left out (the ones being moved).
    pub fn from_doc(
        doc: &Document,
        exclude: &[ObjectId],
        reach: Reach,
        near: Option<(DVec2, f64)>,
    ) -> Geometry {
        let mut g = Geometry::default();
        let exclude: std::collections::HashSet<ObjectId> = exclude.iter().copied().collect();
        for o in doc.objects.values() {
            if exclude.contains(&o.id) || !reach.allows(doc, o) {
                continue;
            }
            if let Some((c, r)) = near {
                let (lo, hi) = doc.object_bounds(o);
                if lo.is_finite() && hi.is_finite() {
                    let d = (c.clamp(lo, hi) - c).length();
                    if d > r {
                        continue;
                    }
                }
            }
            if matches!(o.shape, Shape::SurfaceModel(_) | Shape::Component { .. }) {
                g.add_surface(doc, o, near);
            } else {
                g.add_shape(&o.shape);
            }
        }
        g
    }

    /// The snap points of a surface model or an instance (MOD-13): its vertices, the middles of
    /// its edges that are shown, the centres of its faces that are seen from above, and the edges.
    /// Hidden faces and edges and soft edges give nothing. `near` keeps what is close to the pointer.
    pub fn add_surface(&mut self, doc: &Document, o: &Object, near: Option<(DVec2, f64)>) {
        let Some(Ok(parts)) = crate::surface::plan::parts_of(doc, o) else {
            return;
        };
        let s = crate::surface::plan::snap_geometry(&parts, near, SURFACE_EDGE_CAP);
        for p in s.vertices {
            self.add_point(p, Kind::Endpoint);
        }
        for p in s.midpoints {
            self.add_point(p, Kind::Midpoint);
        }
        for p in s.centers {
            self.add_point(p, Kind::Center);
        }
        for [a, b] in s.edges {
            self.edges.push(Piece::line(a, b));
        }
    }
}

/// Most edges of one surface model that the snap looks at.
const SURFACE_EDGE_CAP: usize = 20_000;

/// Which objects the pointer snaps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reach {
    /// Objects on other layers than the active one.
    pub other_layers: bool,
    /// Objects of grey layers and classes.
    pub grey: bool,
}

impl Default for Reach {
    fn default() -> Self {
        Reach {
            other_layers: true,
            grey: false,
        }
    }
}

impl From<&Settings> for Reach {
    fn from(s: &Settings) -> Reach {
        Reach {
            other_layers: s.other_layers,
            grey: s.grey,
        }
    }
}

impl Reach {
    /// True if the pointer may snap to the object. Objects of invisible layers and classes never
    /// count. An object on a sheet layer counts only when its layer is the active one.
    pub fn allows(&self, doc: &Document, o: &Object) -> bool {
        let shown = doc.is_visible(o) || (doc.is_shown(o) && o.layer == doc.active_layer);
        if !shown {
            return false;
        }
        if !self.other_layers && o.layer != doc.active_layer {
            return false;
        }
        if !self.grey && doc.is_grey(o) {
            return false;
        }
        true
    }
}

/// What the pointer is doing right now.
#[derive(Clone, Copy, Debug)]
pub struct Context {
    /// How close a point must be, in mm.
    pub tol: f64,
    /// The step of the grid, in mm.
    pub grid_step: f64,
    /// The last point you clicked, while you draw.
    pub anchor: Option<DVec2>,
    /// The direction of the segment before (radians), for relative angles.
    pub prior_direction: Option<f64>,
}

/// Points and edges that you took as guides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smart {
    pub points: Vec<DVec2>,
    /// Edges as two points on the line of the edge.
    pub edges: Vec<(DVec2, DVec2)>,
}

/// What `Smart::acquire_at` took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Acquired {
    Point(DVec2),
    Edge(DVec2, DVec2),
}

impl Smart {
    pub const MAX_POINTS: usize = 3;
    pub const MAX_EDGES: usize = 2;

    pub fn is_empty(&self) -> bool {
        self.points.is_empty() && self.edges.is_empty()
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.edges.clear();
    }

    /// Takes a point. The oldest one goes when there are too many. The same point twice counts once.
    pub fn acquire_point(&mut self, p: DVec2) {
        self.points.retain(|q| q.distance(p) > 1e-9);
        self.points.push(p);
        if self.points.len() > Self::MAX_POINTS {
            self.points.remove(0);
        }
    }

    pub fn acquire_edge(&mut self, a: DVec2, b: DVec2) {
        if a.distance(b) <= 1e-12 {
            return;
        }
        self.edges.push((a, b));
        if self.edges.len() > Self::MAX_EDGES {
            self.edges.remove(0);
        }
    }

    /// Takes the object point under the pointer, else (if `edges`) the edge under it.
    pub fn acquire_at(
        &mut self,
        cursor: DVec2,
        geom: &Geometry,
        tol: f64,
        edges: bool,
    ) -> Option<Acquired> {
        if let Some(pt) = geom
            .points
            .iter()
            .filter(|p| p.p.distance(cursor) <= tol)
            .min_by(|a, b| a.p.distance(cursor).total_cmp(&b.p.distance(cursor)))
        {
            self.acquire_point(pt.p);
            return Some(Acquired::Point(pt.p));
        }
        if edges && let Some((piece, t, q)) = nearest_edge(geom, cursor, tol) {
            let tan = piece.tangent(t);
            self.acquire_edge(q, q + tan);
            return Some(Acquired::Edge(q, q + tan));
        }
        None
    }
}

/// An alignment line: a point on it, a direction and where it comes from.
#[derive(Clone, Copy, Debug)]
struct Line {
    o: DVec2,
    d: DVec2,
    kind: Kind,
    /// A ray goes in one direction only.
    ray: bool,
}

/// A line to draw at the cursor (a guide), as two points far apart on the line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub a: DVec2,
    pub b: DVec2,
    pub kind: Kind,
}

/// The result of a snap.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub point: DVec2,
    pub kind: Kind,
    /// The text for the cue label at the cursor, in English. A UI that speaks another language
    /// builds its own text from `kind`, `angle_deg` and `length`.
    pub cue: String,
    /// The angle in degrees (0 up to 360) when the cue names one: an angle snap, or a smart point
    /// or edge that also snaps to an angle.
    pub angle_deg: Option<f64>,
    /// The length in mm when the cue names one: a distance snap, or an angle snap with a length.
    pub length: Option<f64>,
    pub guides: Vec<Guide>,
}

fn guide(l: &Line, around: DVec2, len: f64) -> Guide {
    let p = l.o + l.d * (around - l.o).dot(l.d);
    Guide {
        a: if l.ray { l.o } else { p - l.d * len },
        b: p + l.d * len,
        kind: l.kind,
    }
}

fn dist_to_line(l: &Line, p: DVec2) -> Option<(f64, DVec2)> {
    let t = (p - l.o).dot(l.d);
    if l.ray && t < 0.0 {
        return None;
    }
    let q = l.o + l.d * t;
    Some((q.distance(p), q))
}

fn nearest_edge(geom: &Geometry, cursor: DVec2, tol: f64) -> Option<(Piece, f64, DVec2)> {
    geom.edges
        .iter()
        .map(|e| {
            let (t, q, d) = e.closest(cursor);
            (d, *e, t, q)
        })
        .filter(|x| x.0 <= tol)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, e, t, q)| (e, t, q))
}

fn nearest<T: Copy>(
    items: impl Iterator<Item = (DVec2, T)>,
    cursor: DVec2,
    tol: f64,
) -> Option<(DVec2, T)> {
    items
        .filter(|(p, _)| p.distance(cursor) <= tol)
        .min_by(|a, b| a.0.distance(cursor).total_cmp(&b.0.distance(cursor)))
}

fn priority(k: Kind) -> u8 {
    match k {
        Kind::Endpoint => 0,
        Kind::Center => 1,
        Kind::Quadrant => 2,
        Kind::Midpoint => 3,
        _ => 9,
    }
}

/// An angle in radians as degrees from 0 up to 360.
fn degrees_0_360(rad: f64) -> f64 {
    ((rad.to_degrees() % 360.0) + 360.0) % 360.0
}

fn angle_text(rad: f64) -> String {
    let deg = degrees_0_360(rad);
    if (deg - deg.round()).abs() < 0.005 {
        format!("{}°", deg.round() as i64)
    } else {
        format!("{deg:.2}°")
    }
}

fn length_text(d: f64) -> String {
    if (d - d.round()).abs() < 0.005 {
        format!("{}", d.round() as i64)
    } else {
        format!("{d:.2}")
    }
}

/// Snaps the pointer. `None` if no kind is on and near, and the grid is off.
pub fn snap(
    cursor: DVec2,
    ctx: &Context,
    settings: &Settings,
    geom: &Geometry,
    smart: &Smart,
) -> Option<Snap> {
    if !cursor.is_finite() {
        return None;
    }
    let set = &settings.set;
    let tol = ctx.tol.max(0.0);
    let plain = |point: DVec2, kind: Kind| Snap {
        point,
        kind,
        cue: kind.label().to_string(),
        angle_deg: None,
        length: None,
        guides: Vec::new(),
    };

    // 1. Object points.
    let allowed = |k: Kind| match k {
        Kind::Midpoint => set.midpoints,
        _ => set.object_points,
    };
    let best = geom
        .points
        .iter()
        .filter(|p| allowed(p.kind) && p.p.distance(cursor) <= tol)
        .min_by(|a, b| {
            let (da, db) = (a.p.distance(cursor), b.p.distance(cursor));
            // Points at nearly the same place: the one that matters more wins.
            if (da - db).abs() < tol * 0.05 {
                priority(a.kind).cmp(&priority(b.kind))
            } else {
                da.total_cmp(&db)
            }
        });
    if let Some(b) = best {
        return Some(plain(b.p, b.kind));
    }

    // Edges that come near the pointer.
    let near: Vec<&Piece> = geom
        .edges
        .iter()
        .filter(|e| e.closest(cursor).2 <= tol * 3.0)
        .collect();

    // 2. Intersections.
    if set.intersections {
        let pts = near.iter().enumerate().flat_map(|(i, a)| {
            near[i + 1..]
                .iter()
                .flat_map(move |b| piece_intersections(a, b).into_iter().map(|x| (x.2, ())))
        });
        if let Some((p, ())) = nearest(pts, cursor, tol) {
            return Some(plain(p, Kind::Intersection));
        }
    }

    // 3. Perpendicular and tangent points, seen from the anchor.
    if let Some(a) = ctx.anchor {
        if set.perpendicular {
            let pts = near.iter().filter_map(|e| {
                let t = e.param_of(match e {
                    Piece::Line { .. } => a,
                    Piece::Arc { c, .. } => {
                        // The foot on a circle is on the line from the center through the anchor.
                        *c + (a - *c).try_normalize().unwrap_or(DVec2::X) * 1.0
                    }
                });
                (Piece::has_param(t)).then(|| (e.point(t), ()))
            });
            if let Some((p, ())) = nearest(pts, cursor, tol) {
                return Some(Snap {
                    guides: vec![Guide {
                        a,
                        b: p,
                        kind: Kind::Perpendicular,
                    }],
                    ..plain(p, Kind::Perpendicular)
                });
            }
        }
        if set.tangent {
            let pts = near
                .iter()
                .filter_map(|e| e.circle().map(|c| (*e, c)))
                .flat_map(|(e, (c, r))| {
                    let d = a.distance(c);
                    let mut out = Vec::new();
                    if d > r {
                        let base = (a - c).to_angle();
                        let off = (r / d).acos();
                        for s in [-1.0, 1.0] {
                            let p = c + DVec2::from_angle(base + s * off) * r;
                            if Piece::has_param(e.param_of(p)) {
                                out.push((p, ()));
                            }
                        }
                    }
                    out
                });
            if let Some((p, ())) = nearest(pts, cursor, tol) {
                return Some(Snap {
                    guides: vec![Guide {
                        a,
                        b: p,
                        kind: Kind::Tangent,
                    }],
                    ..plain(p, Kind::Tangent)
                });
            }
        }
    }

    // 4. Alignment lines: smart points, smart edges and angle rays.
    let mut lines: Vec<Line> = Vec::new();
    let mut step_dirs: Vec<f64> = vec![0.0, FRAC_PI_2];
    if set.angle {
        step_dirs = (0..((TAU / settings.angle_step).ceil() as usize).min(720))
            .map(|i| i as f64 * settings.angle_step)
            .collect();
    }
    if set.smart_points {
        for p in &smart.points {
            for a in step_dirs.iter().filter(|a| **a < PI - 1e-9) {
                lines.push(Line {
                    o: *p,
                    d: DVec2::from_angle(*a),
                    kind: Kind::SmartPoint,
                    ray: false,
                });
            }
        }
    }
    if set.smart_edges {
        for (a, b) in &smart.edges {
            let d = (*b - *a).normalize_or_zero();
            if d == DVec2::ZERO {
                continue;
            }
            let n = DVec2::new(-d.y, d.x);
            for (o, dir) in [(*a, d), (*a, n), (*b, n)] {
                lines.push(Line {
                    o,
                    d: dir,
                    kind: Kind::SmartEdge,
                    ray: false,
                });
            }
        }
    }
    if set.angle
        && let Some(a) = ctx.anchor
    {
        let base = if settings.relative_angles {
            ctx.prior_direction.unwrap_or(0.0)
        } else {
            0.0
        };
        let n = (TAU / settings.angle_step).ceil() as usize;
        for i in 0..n.min(720) {
            lines.push(Line {
                o: a,
                d: DVec2::from_angle(base + i as f64 * settings.angle_step),
                kind: Kind::Angle,
                ray: true,
            });
        }
    }
    let reach = cursor.distance(ctx.anchor.unwrap_or(cursor)).max(1000.0) * 2.0;
    // Crossings of two lines.
    let mut best: Option<(f64, DVec2, usize, usize)> = None;
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            let (l1, l2) = (&lines[i], &lines[j]);
            if l1.kind == Kind::Angle && l2.kind == Kind::Angle {
                continue;
            }
            let den = l1.d.perp_dot(l2.d);
            if den.abs() < 1e-9 {
                continue;
            }
            let t = (l2.o - l1.o).perp_dot(l2.d) / den;
            let x = l1.o + l1.d * t;
            let ok = |l: &Line| !l.ray || (x - l.o).dot(l.d) >= -1e-9;
            if !(ok(l1) && ok(l2)) {
                continue;
            }
            let d = x.distance(cursor);
            if d <= tol && best.is_none_or(|b| d < b.0) {
                best = Some((d, x, i, j));
            }
        }
    }
    if let Some((_, x, i, j)) = best {
        let (l1, l2) = (lines[i], lines[j]);
        let kind = if l1.kind == Kind::SmartEdge || l2.kind == Kind::SmartEdge {
            Kind::SmartEdge
        } else {
            Kind::SmartPoint
        };
        let mut s = Snap {
            guides: vec![guide(&l1, x, reach), guide(&l2, x, reach)],
            ..plain(x, kind)
        };
        if let Some(a) = ctx
            .anchor
            .filter(|_| l1.kind == Kind::Angle || l2.kind == Kind::Angle)
        {
            s.cue = format!(
                "{} + {} {}",
                kind.label(),
                Kind::Angle.label(),
                angle_text((x - a).to_angle())
            );
            s.angle_deg = Some(degrees_0_360((x - a).to_angle()));
        }
        return Some(s);
    }
    // One line: the nearest.
    let mut best: Option<(f64, DVec2, usize)> = None;
    for (i, l) in lines.iter().enumerate() {
        if let Some((d, q)) = dist_to_line(l, cursor)
            && d <= tol
            && best.is_none_or(|b| d < b.0 - 1e-9)
        {
            best = Some((d, q, i));
        }
    }
    if let Some((_, q, i)) = best {
        let l = lines[i];
        let mut point = q;
        let mut s = plain(q, l.kind);
        if l.kind == Kind::Angle
            && let Some(a) = ctx.anchor
        {
            let step = if settings.distance_step > 0.0 {
                settings.distance_step
            } else {
                ctx.grid_step
            };
            let len = point.distance(a);
            let mut text = format!("Angle {}", angle_text(l.d.to_angle()));
            let mut length = None;
            if set.distance && step > 0.0 {
                let k = (len / step).round();
                let target = k * step;
                if (len - target).abs() <= tol && target > 0.0 {
                    point = a + l.d * target;
                    text = format!("{text}, length {}", length_text(target));
                    length = Some(target);
                }
            }
            s.point = point;
            s.cue = text;
            s.angle_deg = Some(degrees_0_360(l.d.to_angle()));
            s.length = length;
            s.kind = Kind::Angle;
            s.guides = vec![Guide {
                a,
                b: a + l.d * reach,
                kind: Kind::Angle,
            }];
        } else {
            s.guides = vec![guide(&l, q, reach)];
        }
        return Some(s);
    }

    // 5. The nearest point on an edge.
    if set.edges
        && let Some((_, _, q)) = nearest_edge(geom, cursor, tol)
    {
        return Some(plain(q, Kind::Edge));
    }

    // 6. Distance from the anchor, in any direction.
    if set.distance
        && let Some(a) = ctx.anchor
    {
        let step = if settings.distance_step > 0.0 {
            settings.distance_step
        } else {
            ctx.grid_step
        };
        let len = cursor.distance(a);
        if step > 0.0 && len > 0.0 {
            let target = (len / step).round() * step;
            if target > 0.0 && (len - target).abs() <= tol {
                let p = a + (cursor - a) / len * target;
                return Some(Snap {
                    cue: format!("Distance {}", length_text(target)),
                    length: Some(target),
                    ..plain(p, Kind::Distance)
                });
            }
        }
    }

    // 7. The grid.
    if set.grid && ctx.grid_step > 0.0 {
        let p = (cursor / ctx.grid_step).round() * ctx.grid_step;
        return Some(plain(p, Kind::Grid));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn ctx(anchor: Option<DVec2>) -> Context {
        Context {
            tol: 5.0,
            grid_step: 50.0,
            anchor,
            prior_direction: None,
        }
    }

    fn geom(shapes: &[Shape]) -> Geometry {
        let mut g = Geometry::default();
        shapes.iter().for_each(|s| g.add_shape(s));
        g
    }

    fn line(a: DVec2, b: DVec2) -> Shape {
        Shape::Line { a, b }
    }

    fn only(set: SnapSet) -> Settings {
        Settings {
            set,
            ..Settings::default()
        }
    }

    #[test]
    fn endpoints_midpoints_and_centers() {
        let g = geom(&[
            line(v(0.0, 0.0), v(100.0, 0.0)),
            Shape::Circle {
                center: v(300.0, 0.0),
                radius: 40.0,
            },
        ]);
        let s = Settings::default();
        let k = |c: DVec2| snap(c, &ctx(None), &s, &g, &Smart::default()).unwrap();
        assert_eq!(
            (k(v(2.0, 1.0)).kind, k(v(2.0, 1.0)).point),
            (Kind::Endpoint, v(0.0, 0.0))
        );
        assert_eq!(k(v(51.0, 2.0)).kind, Kind::Midpoint);
        assert_eq!(k(v(301.0, 2.0)).kind, Kind::Center);
        assert_eq!(k(v(340.0, 3.0)).kind, Kind::Quadrant);
        // Far from everything the grid applies.
        assert_eq!(k(v(171.0, 90.0)).kind, Kind::Grid);
        assert_eq!(k(v(171.0, 90.0)).point, v(150.0, 100.0));
        // Midpoints off.
        let mut s2 = s;
        s2.set.midpoints = false;
        assert_eq!(
            snap(v(51.0, 2.0), &ctx(None), &s2, &g, &Smart::default())
                .unwrap()
                .kind,
            Kind::Grid
        );
        // Everything off.
        assert!(
            snap(
                v(51.0, 2.0),
                &ctx(None),
                &only(SnapSet::OFF),
                &g,
                &Smart::default()
            )
            .is_none()
        );
        assert!(snap(v(f64::NAN, 2.0), &ctx(None), &s, &g, &Smart::default()).is_none());
    }

    #[test]
    fn grid_only_rounds_to_the_step() {
        let s = only(SnapSet {
            grid: true,
            ..SnapSet::OFF
        });
        let r = snap(
            v(124.0, -76.0),
            &ctx(None),
            &s,
            &Geometry::default(),
            &Smart::default(),
        )
        .unwrap();
        assert_eq!(
            (r.point, r.kind, r.cue.as_str()),
            (v(100.0, -100.0), Kind::Grid, "Grid")
        );
    }

    #[test]
    fn intersections_of_lines_and_circles() {
        let g = geom(&[
            line(v(0.0, 0.0), v(100.0, 100.0)),
            line(v(0.0, 100.0), v(100.0, 0.0)),
        ]);
        let s = only(SnapSet {
            intersections: true,
            ..SnapSet::OFF
        });
        let r = snap(v(52.0, 48.0), &ctx(None), &s, &g, &Smart::default()).unwrap();
        assert_eq!(r.kind, Kind::Intersection);
        assert!(r.point.distance(v(50.0, 50.0)) < 1e-9);
        let c = geom(&[
            Shape::Circle {
                center: v(0.0, 0.0),
                radius: 50.0,
            },
            line(v(-100.0, 0.0), v(100.0, 0.0)),
        ]);
        let r = snap(v(48.0, 2.0), &ctx(None), &s, &c, &Smart::default()).unwrap();
        assert!(r.point.distance(v(50.0, 0.0)) < 1e-9);
    }

    #[test]
    fn nearest_point_on_an_edge() {
        let g = geom(&[line(v(0.0, 0.0), v(100.0, 0.0))]);
        let s = only(SnapSet {
            edges: true,
            ..SnapSet::OFF
        });
        let r = snap(v(33.0, 3.0), &ctx(None), &s, &g, &Smart::default()).unwrap();
        assert_eq!((r.kind, r.point), (Kind::Edge, v(33.0, 0.0)));
        assert!(snap(v(33.0, 30.0), &ctx(None), &s, &g, &Smart::default()).is_none());
    }

    #[test]
    fn perpendicular_and_tangent_from_the_anchor() {
        let g = geom(&[line(v(0.0, 0.0), v(100.0, 0.0))]);
        let s = only(SnapSet {
            perpendicular: true,
            ..SnapSet::OFF
        });
        let r = snap(
            v(41.0, 2.0),
            &ctx(Some(v(40.0, 80.0))),
            &s,
            &g,
            &Smart::default(),
        )
        .unwrap();
        assert_eq!((r.kind, r.point), (Kind::Perpendicular, v(40.0, 0.0)));
        assert_eq!(r.guides.len(), 1);
        // Tangent to a circle from a point outside.
        let c = geom(&[Shape::Circle {
            center: v(0.0, 0.0),
            radius: 30.0,
        }]);
        let s = only(SnapSet {
            tangent: true,
            ..SnapSet::OFF
        });
        let a = v(50.0, 0.0);
        let t = (30.0f64 / 50.0).acos();
        let p = DVec2::from_angle(t) * 30.0;
        let r = snap(p + v(1.0, 1.0), &ctx(Some(a)), &s, &c, &Smart::default()).unwrap();
        assert_eq!(r.kind, Kind::Tangent);
        assert!(r.point.distance(p) < 1e-9);
        // The tangent line is perpendicular to the radius.
        assert!((r.point.dot(r.point - a)).abs() < 1e-6 * 1000.0);
    }

    #[test]
    fn angle_and_distance_snap() {
        let a = v(0.0, 0.0);
        let mut s = only(SnapSet {
            angle: true,
            distance: true,
            ..SnapSet::OFF
        });
        s.angle_step = std::f64::consts::FRAC_PI_4;
        s.distance_step = 100.0;
        // Close to 45 degrees at a length near 300.
        let cursor = DVec2::from_angle(0.78) * 301.0;
        let r = snap(
            cursor,
            &ctx(Some(a)),
            &s,
            &Geometry::default(),
            &Smart::default(),
        )
        .unwrap();
        assert_eq!(r.kind, Kind::Angle);
        assert!(
            r.point
                .distance(DVec2::from_angle(std::f64::consts::FRAC_PI_4) * 300.0)
                < 1e-9
        );
        assert!(r.cue.contains("45°") && r.cue.contains("300"), "{}", r.cue);
        // Far from any angle line: nothing.
        assert!(
            snap(
                DVec2::from_angle(0.39) * 250.0,
                &ctx(Some(a)),
                &s,
                &Geometry::default(),
                &Smart::default()
            )
            .is_none()
        );
        // Distance only: any direction, snapped length.
        let d = only(SnapSet {
            distance: true,
            ..SnapSet::OFF
        });
        let mut d = d;
        d.distance_step = 100.0;
        let r = snap(
            DVec2::from_angle(0.39) * 202.0,
            &ctx(Some(a)),
            &d,
            &Geometry::default(),
            &Smart::default(),
        )
        .unwrap();
        assert_eq!(r.kind, Kind::Distance);
        assert!((r.point.length() - 200.0).abs() < 1e-9);
        // Without an anchor there is nothing to measure from.
        assert!(
            snap(
                v(5.0, 5.0),
                &ctx(None),
                &d,
                &Geometry::default(),
                &Smart::default()
            )
            .is_none()
        );
    }

    #[test]
    fn relative_angles_follow_the_prior_segment() {
        let mut s = only(SnapSet {
            angle: true,
            ..SnapSet::OFF
        });
        s.angle_step = std::f64::consts::FRAC_PI_2;
        s.relative_angles = true;
        let mut c = ctx(Some(v(0.0, 0.0)));
        c.prior_direction = Some(std::f64::consts::FRAC_PI_4);
        let cursor = DVec2::from_angle(std::f64::consts::FRAC_PI_4 + 0.01) * 200.0;
        let r = snap(cursor, &c, &s, &Geometry::default(), &Smart::default()).unwrap();
        assert!(r.point.to_angle() - std::f64::consts::FRAC_PI_4 < 1e-9);
        assert!(r.cue.contains("45°"));
    }

    #[test]
    fn smart_points_give_alignment_lines() {
        let mut smart = Smart::default();
        smart.acquire_point(v(100.0, 200.0));
        smart.acquire_point(v(400.0, 50.0));
        let s = only(SnapSet {
            smart_points: true,
            ..SnapSet::OFF
        });
        // The crossing of the horizontal line of the first point with the vertical of the second.
        let r = snap(
            v(402.0, 198.0),
            &ctx(None),
            &s,
            &Geometry::default(),
            &smart,
        )
        .unwrap();
        assert_eq!((r.kind, r.point), (Kind::SmartPoint, v(400.0, 200.0)));
        assert_eq!(r.guides.len(), 2);
        // One line only.
        let r = snap(
            v(250.0, 203.0),
            &ctx(None),
            &s,
            &Geometry::default(),
            &smart,
        )
        .unwrap();
        assert_eq!(r.point, v(250.0, 200.0));
        assert_eq!(r.guides.len(), 1);
        assert!(
            snap(
                v(250.0, 130.0),
                &ctx(None),
                &s,
                &Geometry::default(),
                &smart
            )
            .is_none()
        );
        // Off: no snap.
        let off = only(SnapSet::OFF);
        assert!(
            snap(
                v(250.0, 203.0),
                &ctx(None),
                &off,
                &Geometry::default(),
                &smart
            )
            .is_none()
        );
    }

    #[test]
    fn smart_points_remember_three_and_edges_two() {
        let mut s = Smart::default();
        for i in 0..5 {
            s.acquire_point(v(i as f64, 0.0));
        }
        assert_eq!(s.points, vec![v(2.0, 0.0), v(3.0, 0.0), v(4.0, 0.0)]);
        s.acquire_point(v(3.0, 0.0));
        assert_eq!(s.points.last(), Some(&v(3.0, 0.0)));
        assert_eq!(s.points.len(), 3);
        for i in 0..3 {
            s.acquire_edge(v(0.0, i as f64), v(1.0, i as f64));
        }
        assert_eq!(s.edges.len(), 2);
        s.acquire_edge(v(1.0, 1.0), v(1.0, 1.0));
        assert_eq!(s.edges.len(), 2);
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn acquiring_at_the_pointer() {
        let g = geom(&[line(v(0.0, 0.0), v(100.0, 0.0))]);
        let mut s = Smart::default();
        assert_eq!(
            s.acquire_at(v(1.0, 1.0), &g, 5.0, true),
            Some(Acquired::Point(v(0.0, 0.0)))
        );
        // On the edge, away from points: an edge, when edges are allowed.
        assert_eq!(s.acquire_at(v(30.0, 2.0), &g, 5.0, false), None);
        let got = s.acquire_at(v(30.0, 2.0), &g, 5.0, true).unwrap();
        assert!(
            matches!(got, Acquired::Edge(a, b) if a == v(30.0, 0.0) && (b - a).y.abs() < 1e-12)
        );
        assert_eq!(s.edges.len(), 1);
    }

    #[test]
    fn smart_edges_extend_and_cross() {
        let mut smart = Smart::default();
        smart.acquire_edge(v(0.0, 0.0), v(100.0, 100.0));
        let s = only(SnapSet {
            smart_edges: true,
            ..SnapSet::OFF
        });
        // On the extension of the edge, far beyond its end.
        let r = snap(
            v(300.0, 302.0),
            &ctx(None),
            &s,
            &Geometry::default(),
            &smart,
        )
        .unwrap();
        assert_eq!(r.kind, Kind::SmartEdge);
        assert!(r.point.distance(v(301.0, 301.0)) < 1e-9);
    }

    #[test]
    fn smart_lines_meet_the_angle_ray() {
        let mut smart = Smart::default();
        smart.acquire_point(v(200.0, 0.0));
        let mut s = only(SnapSet {
            smart_points: true,
            angle: true,
            ..SnapSet::OFF
        });
        s.angle_step = std::f64::consts::FRAC_PI_4;
        // From the anchor at the origin, the 45 degree ray meets the vertical line through (200, 0).
        let r = snap(
            v(201.0, 199.0),
            &ctx(Some(v(0.0, 0.0))),
            &s,
            &Geometry::default(),
            &smart,
        )
        .unwrap();
        assert_eq!(r.kind, Kind::SmartPoint);
        assert!(r.point.distance(v(200.0, 200.0)) < 1e-9);
        assert!(r.cue.contains("Angle"), "{}", r.cue);
    }

    #[test]
    fn reach_filters_layers_and_grey_objects() {
        use crate::command::{Command, Editor};
        use crate::model::{Attributes, LayerId};
        let mut ed = Editor::default();
        let l2 = LayerId(
            ed.execute(&Command::AddLayer {
                name: "B".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0],
        );
        let add = |ed: &mut Editor, layer: LayerId| {
            ed.execute(&Command::AddObject {
                shape: line(v(0.0, 0.0), v(10.0, 0.0)),
                layer: Some(layer),
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0]
        };
        let (a, b) = (add(&mut ed, LayerId::from_num(1)), add(&mut ed, l2));
        let points =
            |ed: &Editor, reach: Reach| Geometry::from_doc(ed.doc(), &[], reach, None).points.len();
        assert_eq!(points(&ed, Reach::default()), 6);
        assert_eq!(
            points(
                &ed,
                Reach {
                    other_layers: false,
                    grey: false
                }
            ),
            3
        );
        assert_eq!(
            Geometry::from_doc(ed.doc(), &[ObjectId(a)], Reach::default(), None)
                .points
                .len(),
            3
        );
        let _ = b;
        // A grey layer: no snap unless the option says so.
        ed.execute(&Command::SetVisibility {
            layers: vec![l2],
            classes: vec![],
            mode: crate::visibility::VisMode::Grey,
        })
        .unwrap();
        assert_eq!(points(&ed, Reach::default()), 3);
        assert_eq!(
            points(
                &ed,
                Reach {
                    other_layers: true,
                    grey: true
                }
            ),
            6
        );
        // Near filter: nothing near a far point.
        assert!(
            Geometry::from_doc(
                ed.doc(),
                &[],
                Reach {
                    other_layers: true,
                    grey: true
                },
                Some((v(1000.0, 1000.0), 10.0))
            )
            .points
            .is_empty()
        );
        assert_eq!(
            Geometry::from_doc(
                ed.doc(),
                &[],
                Reach {
                    other_layers: true,
                    grey: true
                },
                Some((v(5.0, 3.0), 10.0))
            )
            .points
            .len(),
            6
        );
    }

    #[test]
    fn shapes_give_points_and_edges() {
        let g = geom(&[
            Shape::Rect {
                min: v(0.0, 0.0),
                max: v(10.0, 10.0),
            },
            Shape::Text {
                at: v(50.0, 50.0),
                text: "x".into(),
                height: 3.0,
            },
            Shape::Plant {
                at: v(60.0, 60.0),
                species: "x".into(),
                spread: 2.0,
                height: 1.0,
            },
        ]);
        assert!(
            g.points
                .iter()
                .any(|p| p.p == v(5.0, 5.0) && p.kind == Kind::Center)
        );
        assert!(
            g.points
                .iter()
                .any(|p| p.p == v(5.0, 0.0) && p.kind == Kind::Midpoint)
        );
        assert!(
            g.points
                .iter()
                .any(|p| p.p == v(60.0, 60.0) && p.kind == Kind::Center)
        );
        assert_eq!(g.edges.len(), 4);
    }

    #[test]
    fn settings_names_and_validation() {
        let mut set = SnapSet::default();
        assert_eq!(set.get("grid"), Some(true));
        assert_eq!(set.get("nope"), None);
        assert!(set.set("angle", true));
        assert!(!set.set("nope", true));
        assert_eq!(set.get("angle"), Some(true));
        for n in SnapSet::NAMES {
            assert!(set.get(n).is_some());
        }
        assert!(set.any());
        assert!(!SnapSet::OFF.any());
        assert!(Settings::default().problem().is_none());
        let s = Settings {
            angle_step: 0.0,
            ..Settings::default()
        };
        assert!(s.problem().is_some());
        let s = Settings {
            angle_step: 0.5,
            distance_step: -1.0,
            ..Settings::default()
        };
        assert!(s.problem().is_some());
        let j = serde_json::to_value(Settings::default()).unwrap();
        let back: Settings = serde_json::from_value(j).unwrap();
        assert_eq!(back, Settings::default());
        // Missing fields get their defaults.
        let part: Settings = serde_json::from_str(r#"{"set": {"angle": true}}"#).unwrap();
        assert!(part.set.angle && part.set.grid);
        for k in [
            Kind::Grid,
            Kind::Endpoint,
            Kind::Midpoint,
            Kind::Center,
            Kind::Quadrant,
            Kind::Intersection,
            Kind::Edge,
            Kind::Perpendicular,
            Kind::Tangent,
            Kind::Angle,
            Kind::Distance,
            Kind::SmartPoint,
            Kind::SmartEdge,
        ] {
            assert!(!k.label().is_empty());
        }
    }
}
