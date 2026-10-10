//! 3D geometry for the document: triangle meshes and line lists. Pure CPU code
//! with no GPU types, so the app, the agent renderer and tests all share it.
//!
//! Units are millimeters, Z up. An object sits at the elevation of its layer:
//!
//! - Hardscape: a slab. The top surface is at `elevation` (default 0) above the
//!   layer, and the slab is `thickness` (default 150 mm) thick, below the top.
//! - Landscape area, rect, circle, closed polyline: a flat face at the layer elevation.
//! - Plant: a trunk and a canopy. The plant is `height` tall and `spread` wide. The canopy follows the
//!   form of the plant style: tree (ellipsoid), columnar, conical, spreading or shrub mound.
//! - Existing tree: like a tree plant, with a trunk as thick as its trunk diameter.
//! - Line, arc, open polyline: 3D line segments.
//!
//! Every face and slab also adds its outline to the line list.
//!
//! Positions are `f32` relative to the scene origin (a `f64` point near the middle of the
//! drawing), so survey coordinates in the millions of mm keep their precision.
//! Text is not drawn in 3D. Hidden layers and classes, and sheet layers, are skipped.

use std::ops::Range;
use std::sync::Arc;

use glam::{DVec2, DVec3, Vec3};
use indexmap::IndexMap;

use crate::curve;
use crate::model::{Color, Document, Layer, LayerKind, Object, ObjectId, Shape, arc_sweep};
use crate::plants::link;
use crate::plants::style::PlantForm;

/// Thickness of a hardscape slab when the shape does not say.
pub const DEFAULT_SLAB_THICKNESS: f64 = 150.0;

/// Largest distance between a curve and its flat segments, in mm.
const FLATTEN_TOLERANCE: f64 = 2.0;

/// Axis-aligned box as (min, max).
pub type Bounds3 = (Vec3, Vec3);

/// Triangles with per-vertex normal and color, and the object of every triangle.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub colors: Vec<[u8; 4]>,
    /// Three indices per triangle, counterclockwise seen from outside.
    pub indices: Vec<u32>,
    /// The object of each triangle.
    pub tri_object: Vec<ObjectId>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The three corners of triangle `t`.
    pub fn triangle(&self, t: usize) -> [Vec3; 3] {
        let i = &self.indices[t * 3..t * 3 + 3];
        [
            self.positions[i[0] as usize],
            self.positions[i[1] as usize],
            self.positions[i[2] as usize],
        ]
    }

    fn vertex(&mut self, p: Vec3, n: Vec3, c: [u8; 4]) -> u32 {
        self.positions.push(p);
        self.normals.push(n);
        self.colors.push(c);
        (self.positions.len() - 1) as u32
    }

    fn tri(&mut self, a: u32, b: u32, c: u32, object: ObjectId) {
        self.indices.extend_from_slice(&[a, b, c]);
        self.tri_object.push(object);
    }
}

/// Line segments. Segment `i` runs from `positions[2i]` to `positions[2i + 1]`.
#[derive(Clone, Debug, Default)]
pub struct Lines {
    pub positions: Vec<Vec3>,
    /// One color per segment.
    pub colors: Vec<[u8; 4]>,
    /// The object of each segment.
    pub objects: Vec<ObjectId>,
}

impl Lines {
    pub fn segment_count(&self) -> usize {
        self.colors.len()
    }

    pub fn segment(&self, i: usize) -> [Vec3; 2] {
        [self.positions[2 * i], self.positions[2 * i + 1]]
    }

    fn push(&mut self, a: Vec3, b: Vec3, c: [u8; 4], object: ObjectId) {
        self.positions.push(a);
        self.positions.push(b);
        self.colors.push(c);
        self.objects.push(object);
    }

    fn polyline(&mut self, pts: &[Vec3], closed: bool, c: [u8; 4], object: ObjectId) {
        let n = pts.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            self.push(pts[i], pts[(i + 1) % n], c, object);
        }
    }
}

/// Where the triangles and segments of one object are.
#[derive(Clone, Debug, Default)]
struct ObjectRange {
    tris: Range<usize>,
    segs: Range<usize>,
}

/// Everything that is drawn in 3D.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub mesh: Mesh,
    pub lines: Lines,
    /// Box around all triangles and segments, relative to `origin`. `None` for an empty scene.
    pub bounds: Option<Bounds3>,
    /// World position (mm) of the local point (0, 0, 0). Positions in the mesh and lines are relative to it.
    pub origin: DVec3,
    /// Geometry of each object, in document order.
    ranges: IndexMap<ObjectId, ObjectRange>,
}

impl Scene {
    pub fn is_empty(&self) -> bool {
        self.mesh.indices.is_empty() && self.lines.segment_count() == 0
    }

    /// Box around the geometry of one object.
    pub fn object_bounds(&self, id: ObjectId) -> Option<Bounds3> {
        let r = self.ranges.get(&id)?;
        let mut b = BoundsAcc::default();
        for t in r.tris.clone() {
            self.mesh.triangle(t).iter().for_each(|p| b.add(*p));
        }
        for s in r.segs.clone() {
            self.lines.segment(s).iter().for_each(|p| b.add(*p));
        }
        b.get()
    }

    /// Objects that have geometry, in document order.
    pub fn objects(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.ranges.keys().copied()
    }

    /// A scene point in world coordinates (mm).
    pub fn to_world(&self, p: Vec3) -> DVec3 {
        self.origin + p.as_dvec3()
    }
}

#[derive(Default)]
struct BoundsAcc(Option<Bounds3>);

impl BoundsAcc {
    fn add(&mut self, p: Vec3) {
        if !p.is_finite() {
            return;
        }
        self.0 = Some(match self.0 {
            None => (p, p),
            Some((lo, hi)) => (lo.min(p), hi.max(p)),
        });
    }
    fn get(&self) -> Option<Bounds3> {
        self.0
    }
}

/// Adds triangles and lines of one object to a scene. An extension gets one of these for
/// each object that core cannot draw itself (a site model), so core needs no other crate.
/// Positions are world coordinates in mm, relative to the layer elevation in Z.
pub struct SceneSink<'a> {
    scene: &'a mut Scene,
    object: ObjectId,
    z0: f64,
}

impl SceneSink<'_> {
    fn local(&self, p: DVec3) -> Vec3 {
        let o = self.scene.origin;
        Vec3::new(
            (p.x - o.x) as f32,
            (p.y - o.y) as f32,
            (self.z0 + p.z - o.z) as f32,
        )
    }

    /// Adds a triangle mesh with one color. Indices are three per triangle. Normals are smooth.
    /// Triangles that are not counterclockwise seen from above are flipped to face up.
    pub fn mesh(&mut self, positions: &[DVec3], indices: &[u32], color: [u8; 4]) {
        let pts: Vec<Vec3> = positions.iter().map(|p| self.local(*p)).collect();
        let mut normals = vec![Vec3::ZERO; pts.len()];
        let mut tris = Vec::with_capacity(indices.len() / 3);
        for t in indices.chunks_exact(3) {
            let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
            if a >= pts.len() || b >= pts.len() || c >= pts.len() {
                continue;
            }
            let n = (pts[b] - pts[a]).cross(pts[c] - pts[a]);
            let (b, c, n) = if n.z < 0.0 { (c, b, -n) } else { (b, c, n) };
            for i in [a, b, c] {
                normals[i] += n;
            }
            tris.push([a, b, c]);
        }
        let base = self.scene.mesh.positions.len() as u32;
        for (p, n) in pts.iter().zip(&normals) {
            self.scene.mesh.vertex(*p, n.normalize_or_zero(), color);
        }
        for [a, b, c] in tris {
            self.scene.mesh.tri(
                base + a as u32,
                base + b as u32,
                base + c as u32,
                self.object,
            );
        }
    }

    /// Adds a 3D line.
    pub fn polyline(&mut self, points: &[DVec3], closed: bool, color: [u8; 4]) {
        let pts: Vec<Vec3> = points.iter().map(|p| self.local(*p)).collect();
        self.scene.lines.polyline(&pts, closed, color, self.object);
    }
}

/// Draws objects that core does not know how to draw. Gets the document and the object, and
/// adds geometry to the sink. It is called for site models, site modifiers, and hardscapes and
/// landscape areas that drape on the site model.
pub type SceneExt<'a> = &'a dyn Fn(&Document, &Object, &mut SceneSink<'_>);

/// Keeps the scene of the latest document revision. `get` rebuilds it only when
/// the revision changed, so a viewer can call it every frame. The revision comes from
/// `Editor::revision`; this module does not know the editor.
#[derive(Default)]
pub struct MeshCache {
    revision: Option<u64>,
    scene: Option<Arc<Scene>>,
    /// How often the scene was built.
    pub builds: u64,
}

impl MeshCache {
    /// The scene of `doc`, which must be the document of `revision`.
    pub fn get(&mut self, revision: u64, doc: &Document) -> Arc<Scene> {
        self.get_with(revision, doc, None)
    }

    /// Like `get`, with an extension that draws site models.
    pub fn get_with(
        &mut self,
        revision: u64,
        doc: &Document,
        ext: Option<SceneExt<'_>>,
    ) -> Arc<Scene> {
        if self.revision != Some(revision) || self.scene.is_none() {
            self.scene = Some(Arc::new(build_with(doc, scene_origin(doc), ext)));
            self.revision = Some(revision);
            self.builds += 1;
        }
        self.scene.clone().expect("scene was just built")
    }

    /// The revision of the cached scene.
    pub fn revision(&self) -> Option<u64> {
        self.revision
    }

    /// Forces a rebuild at the next `get`.
    pub fn invalidate(&mut self) {
        self.revision = None;
    }
}

// ---------------------------------------------------------------------------
// Triangulation

/// Removes repeated points and points that lie on a straight run, and returns the ring
/// counterclockwise. Fewer than three points left means the ring has no area.
pub fn clean_ring(points: &[DVec2]) -> Vec<DVec2> {
    let mut pts: Vec<DVec2> = Vec::with_capacity(points.len());
    for &p in points {
        if pts.last().is_none_or(|q| q.distance_squared(p) > 1e-18) {
            pts.push(p);
        }
    }
    while pts.len() > 1 && pts[0].distance_squared(pts[pts.len() - 1]) <= 1e-18 {
        pts.pop();
    }
    // Drop points where the ring does not turn. Repeat, because a drop can expose another.
    loop {
        let n = pts.len();
        if n < 3 {
            return Vec::new();
        }
        let straight = (0..n).find(|&i| {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (u, v) = (b - a, c - b);
            u.perp_dot(v).abs() <= 1e-9 * u.length() * v.length() && u.dot(v) > 0.0
        });
        match straight {
            Some(i) => {
                pts.remove(i);
            }
            None => break,
        }
    }
    let o = pts[0];
    let area2: f64 = (0..pts.len())
        .map(|i| (pts[i] - o).perp_dot(pts[(i + 1) % pts.len()] - o))
        .sum();
    if area2 < 0.0 {
        pts.reverse();
    }
    pts
}

fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    (b - a).perp_dot(p - a) >= 0.0
        && (c - b).perp_dot(p - b) >= 0.0
        && (a - c).perp_dot(p - c) >= 0.0
}

/// Ear clipping. Returns triangles as index triples into `pts`, all counterclockwise
/// (normal +Z), whether the polygon is given clockwise or counterclockwise.
/// Concave polygons work. Repeated and collinear points are skipped. A polygon that
/// crosses itself gives some triangles, but no guarantee about which.
pub fn triangulate(pts: &[DVec2]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let o = pts[0];
    let area2: f64 = (0..n)
        .map(|i| (pts[i] - o).perp_dot(pts[(i + 1) % n] - o))
        .sum();
    let mut idx: Vec<usize> = if area2 >= 0.0 {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let mut out = Vec::with_capacity(n - 2);
    let (mut i, mut misses) = (0usize, 0usize);
    while idx.len() > 3 {
        let m = idx.len();
        i %= m;
        let (a, b, c) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
        let (pa, pb, pc) = (pts[a], pts[b], pts[c]);
        let (u, v) = (pb - pa, pc - pb);
        let cross = u.perp_dot(v);
        let flat = cross.abs() <= 1e-12 * u.length() * v.length()
            || u.length_squared() == 0.0
            || v.length_squared() == 0.0;
        if flat {
            // No area: the corner only repeats or continues an edge. Drop it.
            idx.remove(i);
            misses = 0;
            continue;
        }
        let is_ear = cross > 0.0
            && !idx.iter().any(|&k| {
                k != a
                    && k != b
                    && k != c
                    && pts[k] != pa
                    && pts[k] != pb
                    && pts[k] != pc
                    && in_triangle(pts[k], pa, pb, pc)
            });
        if is_ear {
            out.push([a, b, c]);
            idx.remove(i);
            misses = 0;
        } else {
            i += 1;
            misses += 1;
            if misses >= m {
                // No ear exists: the polygon crosses itself. Drop a corner to finish.
                idx.remove(i % m);
                misses = 0;
            }
        }
    }
    let (a, b, c) = (idx[0], idx[1], idx[2]);
    if (pts[b] - pts[a]).perp_dot(pts[c] - pts[b]) > 1e-12 {
        out.push([a, b, c]);
    }
    out
}

// ---------------------------------------------------------------------------
// Building

fn z_of(layer: &Layer) -> f64 {
    layer.elevation
}

/// A world point as a scene point: relative to `o`, in `f32`.
fn v3(o: DVec3, p: DVec2, z: f64) -> Vec3 {
    Vec3::new((p.x - o.x) as f32, (p.y - o.y) as f32, (z - o.z) as f32)
}

fn rgba(c: Color) -> [u8; 4] {
    c.0
}

fn shade(c: [u8; 4], f: f32) -> [u8; 4] {
    [
        (c[0] as f32 * f) as u8,
        (c[1] as f32 * f) as u8,
        (c[2] as f32 * f) as u8,
        c[3],
    ]
}

const EDGE_DEFAULT: [u8; 4] = [45, 48, 56, 255];
const FACE_DEFAULT: [u8; 4] = [196, 202, 212, 255];
const AREA_DEFAULT: [u8; 4] = [128, 176, 104, 255];
const HARDSCAPE_DEFAULT: [u8; 4] = [160, 156, 150, 255];
const TRUNK: [u8; 4] = [112, 82, 56, 255];

/// Number of segments for a full circle of `radius`, within the flatten tolerance.
fn circle_segments(radius: f64) -> usize {
    let r = radius.abs().max(1e-6);
    let step = 2.0 * (1.0 - (FLATTEN_TOLERANCE / r).min(1.0)).acos();
    if step <= 0.0 {
        12
    } else {
        ((std::f64::consts::TAU / step).ceil() as usize).clamp(12, 256)
    }
}

fn arc_points(center: DVec2, radius: f64, start: f64, end: f64) -> Vec<DVec2> {
    let sweep = arc_sweep(start, end);
    let full = sweep >= std::f64::consts::TAU;
    let per_circle = circle_segments(radius);
    let n = ((per_circle as f64 * sweep / std::f64::consts::TAU).ceil() as usize).max(2);
    (0..=n)
        .map(|k| {
            let a = if k == n && !full {
                start + sweep
            } else {
                start + sweep * k as f64 / n as f64
            };
            center + DVec2::from_angle(a) * radius
        })
        .collect()
}

fn circle_points(center: DVec2, radius: f64) -> Vec<DVec2> {
    let n = circle_segments(radius);
    (0..n)
        .map(|k| center + DVec2::from_angle(std::f64::consts::TAU * k as f64 / n as f64) * radius)
        .collect()
}

/// Unit sphere template: positions equal normals.
struct UnitSphere {
    verts: Vec<Vec3>,
    tris: Vec<[u32; 3]>,
    segments: usize,
    rings: usize,
}

/// Canopy detail of a normal scene, and of a scene with many plants.
const SPHERE_DETAIL: (usize, usize) = (12, 8);
const SPHERE_COARSE: (usize, usize) = (8, 4);
/// More plants than this get the coarse canopy.
pub const COARSE_PLANT_COUNT: usize = 1500;

impl UnitSphere {
    /// A sphere of `segments` around and `rings` from pole to pole. `segments` must divide by 4 or be even.
    fn new((segments, rings): (usize, usize)) -> Self {
        let mut verts = Vec::new();
        for r in 0..=rings {
            let phi = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * r as f32 / rings as f32;
            for s in 0..segments {
                let th = std::f32::consts::TAU * s as f32 / segments as f32;
                // Exact values on the axes keep the bounds exact.
                let (sin_t, cos_t) = if s * 4 % segments == 0 {
                    [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)][s * 4 / segments]
                } else {
                    th.sin_cos()
                };
                let (sin_p, cos_p) = if r == 0 {
                    (-1.0, 0.0)
                } else if r == rings {
                    (1.0, 0.0)
                } else if r * 2 == rings {
                    (0.0, 1.0)
                } else {
                    phi.sin_cos()
                };
                verts.push(Vec3::new(cos_p * cos_t, cos_p * sin_t, sin_p));
            }
        }
        let mut tris = Vec::new();
        for r in 0..rings {
            for s in 0..segments {
                let s2 = (s + 1) % segments;
                let at = |r: usize, s: usize| (r * segments + s) as u32;
                let (a, b, c, d) = (at(r, s), at(r, s2), at(r + 1, s2), at(r + 1, s));
                if r > 0 {
                    tris.push([a, b, c]);
                }
                if r + 1 < rings {
                    tris.push([a, c, d]);
                }
            }
        }
        UnitSphere {
            verts,
            tris,
            segments,
            rings,
        }
    }
}

const TRUNK_SIDES: usize = 8;

struct Builder<'a> {
    ext: Option<SceneExt<'a>>,
    doc: &'a Document,
    origin: DVec3,
    scene: Scene,
    sphere: UnitSphere,
}

impl Builder<'_> {
    fn add_object(&mut self, obj: &Object, layer: &Layer) {
        let o = self.origin;
        let attrs = crate::hardscape::styled_attrs(self.doc, obj, self.doc.resolved_attrs(obj));
        let fill = attrs.fill.filter(|c| c.0[3] > 0).map(rgba);
        let pen = attrs.pen.filter(|c| c.0[3] > 0).map(rgba);
        let edge = pen.unwrap_or(EDGE_DEFAULT);
        let z = z_of(layer);
        let id = obj.id;
        match &obj.shape {
            Shape::Line { a, b } => self.scene.lines.push(
                v3(o, *a, z),
                v3(o, *b, z),
                pen.unwrap_or([0, 0, 0, 255]),
                id,
            ),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let pts: Vec<Vec3> = arc_points(*center, *radius, *start, *end)
                    .into_iter()
                    .map(|p| v3(o, p, z))
                    .collect();
                self.scene
                    .lines
                    .polyline(&pts, false, pen.unwrap_or([0, 0, 0, 255]), id);
            }
            Shape::Polyline {
                points,
                closed: false,
            } => {
                let pts: Vec<Vec3> = curve::flatten(points, false, FLATTEN_TOLERANCE)
                    .into_iter()
                    .map(|p| v3(o, p, z))
                    .collect();
                self.scene
                    .lines
                    .polyline(&pts, false, pen.unwrap_or([0, 0, 0, 255]), id);
            }
            Shape::Polyline {
                points,
                closed: true,
            } => self.flat_face(
                &curve::flatten(points, true, FLATTEN_TOLERANCE),
                z,
                fill.unwrap_or(FACE_DEFAULT),
                edge,
                id,
            ),
            Shape::Rect { min, max } => {
                let (lo, hi) = (min.min(*max), min.max(*max));
                let ring = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
                self.flat_face(&ring, z, fill.unwrap_or(FACE_DEFAULT), edge, id);
            }
            Shape::Circle { center, radius } => self.flat_face(
                &circle_points(*center, *radius),
                z,
                fill.unwrap_or(FACE_DEFAULT),
                edge,
                id,
            ),
            Shape::LandscapeArea { .. } | Shape::Hardscape { .. } if self.styled_solid(obj, z) => {}
            Shape::LandscapeArea { boundary, .. } => self.flat_face(
                &curve::flatten(boundary, true, FLATTEN_TOLERANCE),
                z,
                fill.unwrap_or(AREA_DEFAULT),
                edge,
                id,
            ),
            Shape::Hardscape {
                boundary,
                thickness,
                elevation,
                ..
            } => {
                let thickness = thickness
                    .filter(|t| t.is_finite() && *t > 0.0)
                    .unwrap_or(DEFAULT_SLAB_THICKNESS);
                let top = z + elevation.filter(|e| e.is_finite()).unwrap_or(0.0);
                let ring = curve::flatten(boundary, true, FLATTEN_TOLERANCE);
                self.slab(
                    &ring,
                    top,
                    thickness,
                    fill.unwrap_or(HARDSCAPE_DEFAULT),
                    edge,
                    id,
                );
            }
            Shape::Plant {
                at,
                spread,
                height,
                species,
            } => {
                // A plant with a style takes its form and colors from the effective style.
                let look = link::look(self.doc, obj);
                let color = fill
                    .or(look.fill.map(rgba))
                    .unwrap_or_else(|| species_color(species));
                let edge = if pen.is_none() {
                    look.pen.map(rgba).unwrap_or(edge)
                } else {
                    edge
                };
                let build = PlantBuild {
                    at: *at,
                    spread: *spread,
                    height: *height,
                    form: look.form,
                    color,
                    trunk_radius: None,
                };
                self.plant(&build, z, edge, id);
            }
            Shape::ExistingTree {
                at,
                species,
                dbh,
                spread,
                height,
                ..
            } => {
                let build = PlantBuild {
                    at: *at,
                    spread: *spread,
                    height: *height,
                    form: PlantForm::Tree,
                    color: fill.unwrap_or_else(|| species_color(species)),
                    trunk_radius: Some(dbh / 2.0),
                };
                self.plant(&build, z, edge, id);
            }
            Shape::Text { .. } | Shape::PlantTag { .. } => {}
            Shape::Locus3d { p } => {
                let h = crate::model::LOCUS_MARKER_HALF;
                let c = pen.unwrap_or([0, 0, 0, 255]);
                let at = |d: DVec3| {
                    Vec3::new(
                        (p.x + d.x - o.x) as f32,
                        (p.y + d.y - o.y) as f32,
                        (z + p.z + d.z - o.z) as f32,
                    )
                };
                for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
                    self.scene.lines.push(at(-axis * h), at(axis * h), c, id);
                }
            }
            Shape::Polyline3d { points, closed } => {
                let pts: Vec<Vec3> = points
                    .iter()
                    .map(|p| v3(o, p.truncate(), z + p.z))
                    .collect();
                self.scene
                    .lines
                    .polyline(&pts, *closed, pen.unwrap_or([0, 0, 0, 255]), id);
            }
            // Sheet layer objects are paper space. They never reach the 3D scene.
            Shape::Viewport(_) | Shape::TitleBlock(_) | Shape::Annotation(_) => {}
            // A group draws through its members.
            Shape::Group { .. } => {}
            Shape::Symbol { .. } => {
                // The 3D components if there are some. Otherwise the 2D components, flat at the layer.
                let solids = crate::symbols::instance_solids(self.doc, obj);
                if solids.is_empty() {
                    for part in crate::symbols::instance_objects(self.doc, obj) {
                        self.add_object(&part, layer);
                    }
                }
                for (ring, base, height, color) in solids {
                    self.slab(
                        &ring,
                        z + base + height,
                        height,
                        color.map(rgba).or(fill).unwrap_or(HARDSCAPE_DEFAULT),
                        edge,
                        id,
                    );
                }
            }
            Shape::SiteModel { .. } | Shape::SiteModifier { .. } => {
                if let Some(ext) = self.ext {
                    let mut sink = SceneSink {
                        scene: &mut self.scene,
                        object: id,
                        z0: z,
                    };
                    ext(self.doc, obj, &mut sink);
                } else if let Shape::SiteModifier { boundary, .. } = &obj.shape {
                    let ring = curve::flatten(boundary, true, FLATTEN_TOLERANCE);
                    let pts: Vec<Vec3> = ring.into_iter().map(|p| v3(o, p, z)).collect();
                    self.scene
                        .lines
                        .polyline(&pts, true, pen.unwrap_or([0, 0, 0, 255]), id);
                }
            }
        }
    }

    /// Draws a hardscape with settings, or a landscape area that drapes, through the hardscape
    /// and landscape modules. A draped object goes to the extension, which knows the terrain.
    /// Returns false if the object has no such settings. The caller then draws it as a plain shape.
    fn styled_solid(&mut self, obj: &Object, z: f64) -> bool {
        let hardscape = matches!(obj.shape, Shape::Hardscape { .. });
        if !(crate::hardscape::wants_drape(obj)
            || (hardscape && crate::hardscape::data(obj).is_some()))
        {
            return false;
        }
        let mut sink = SceneSink {
            scene: &mut self.scene,
            object: obj.id,
            z0: z,
        };
        match self.ext {
            Some(ext) if crate::hardscape::wants_drape(obj) => ext(self.doc, obj, &mut sink),
            _ if hardscape => crate::hardscape::emit_object(self.doc, obj, None, &mut sink),
            _ => return false,
        }
        true
    }

    fn flat_face(&mut self, ring: &[DVec2], z: f64, color: [u8; 4], edge: [u8; 4], id: ObjectId) {
        let o = self.origin;
        let ring = clean_ring(ring);
        if ring.len() < 3 {
            return;
        }
        let mesh = &mut self.scene.mesh;
        let base = mesh.positions.len() as u32;
        for p in &ring {
            mesh.vertex(v3(o, *p, z), Vec3::Z, color);
        }
        for [a, b, c] in triangulate(&ring) {
            mesh.tri(base + a as u32, base + b as u32, base + c as u32, id);
        }
        let pts: Vec<Vec3> = ring.iter().map(|p| v3(o, *p, z)).collect();
        self.scene.lines.polyline(&pts, true, edge, id);
    }

    fn slab(
        &mut self,
        ring: &[DVec2],
        top: f64,
        thickness: f64,
        color: [u8; 4],
        edge: [u8; 4],
        id: ObjectId,
    ) {
        let o = self.origin;
        let ring = clean_ring(ring);
        if ring.len() < 3 {
            return;
        }
        let bottom = top - thickness;
        let tris = triangulate(&ring);
        let n = ring.len();
        let mesh = &mut self.scene.mesh;
        // Top and bottom faces.
        let top_base = mesh.positions.len() as u32;
        for p in &ring {
            mesh.vertex(v3(o, *p, top), Vec3::Z, color);
        }
        let bot_base = mesh.positions.len() as u32;
        for p in &ring {
            mesh.vertex(v3(o, *p, bottom), Vec3::NEG_Z, shade(color, 0.7));
        }
        for [a, b, c] in &tris {
            mesh.tri(
                top_base + *a as u32,
                top_base + *b as u32,
                top_base + *c as u32,
                id,
            );
            mesh.tri(
                bot_base + *a as u32,
                bot_base + *c as u32,
                bot_base + *b as u32,
                id,
            );
        }
        // Side walls: the ring is counterclockwise, so the outside is on the right of each edge.
        let side = shade(color, 0.82);
        for i in 0..n {
            let (p, q) = (ring[i], ring[(i + 1) % n]);
            let d = q - p;
            let nrm = DVec2::new(d.y, -d.x).normalize_or_zero();
            let normal = Vec3::new(nrm.x as f32, nrm.y as f32, 0.0);
            let a = mesh.vertex(v3(o, p, bottom), normal, side);
            let b = mesh.vertex(v3(o, q, bottom), normal, side);
            let c = mesh.vertex(v3(o, q, top), normal, side);
            let e = mesh.vertex(v3(o, p, top), normal, side);
            mesh.tri(a, b, c, id);
            mesh.tri(a, c, e, id);
        }
        // Edges: both outlines, and a vertical edge at each sharp corner.
        let ring_at = |z: f64| -> Vec<Vec3> { ring.iter().map(|p| v3(o, *p, z)).collect() };
        self.scene.lines.polyline(&ring_at(top), true, edge, id);
        self.scene.lines.polyline(&ring_at(bottom), true, edge, id);
        for i in 0..n {
            let (a, b, c) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            let (u, v) = ((b - a).normalize_or_zero(), (c - b).normalize_or_zero());
            if u.dot(v) < 0.94 {
                self.scene
                    .lines
                    .push(v3(o, b, bottom), v3(o, b, top), edge, id);
            }
        }
    }

    /// A plant: a trunk and a canopy whose outline depends on the form (see `PlantForm`).
    /// The canopy is the unit sphere, moved vertex by vertex into the form.
    fn plant(&mut self, p: &PlantBuild, z: f64, edge: [u8; 4], id: ObjectId) {
        let PlantBuild {
            at,
            spread,
            height,
            form,
            color,
            trunk_radius,
        } = *p;
        if !(spread > 0.0 && height > 0.0) {
            return;
        }
        let o = self.origin;
        let r = spread / 2.0;
        // Where the canopy starts (as a fraction of the height), and where the trunk ends.
        let (bottom_frac, trunk_frac) = match form {
            PlantForm::Tree => (0.3, 0.65),
            PlantForm::Columnar => (0.1, 0.3),
            PlantForm::Conical => (0.08, 0.2),
            PlantForm::Spreading => (0.5, 0.6),
            PlantForm::ShrubMound => (0.0, 0.0),
        };
        let canopy_bottom = z + height * bottom_frac;
        let canopy_h = height * (1.0 - bottom_frac);
        let center_z = canopy_bottom + canopy_h / 2.0;

        let mesh = &mut self.scene.mesh;
        if trunk_frac > 0.0 {
            // Trunk: a prism from the ground into the canopy.
            let trunk_r = trunk_radius
                .unwrap_or((spread * 0.04).clamp(15.0, 300.0))
                .min(r * 0.5);
            let trunk_top = z + height * trunk_frac;
            let base = mesh.positions.len() as u32;
            for s in 0..TRUNK_SIDES {
                let a = std::f64::consts::TAU * s as f64 / TRUNK_SIDES as f64;
                let dir = DVec2::from_angle(a);
                let n = Vec3::new(dir.x as f32, dir.y as f32, 0.0);
                mesh.vertex(v3(o, at + dir * trunk_r, z), n, TRUNK);
                mesh.vertex(v3(o, at + dir * trunk_r, trunk_top), n, TRUNK);
            }
            for s in 0..TRUNK_SIDES {
                let s2 = (s + 1) % TRUNK_SIDES;
                let (b0, t0, b1, t1) = (
                    base + 2 * s as u32,
                    base + 2 * s as u32 + 1,
                    base + 2 * s2 as u32,
                    base + 2 * s2 as u32 + 1,
                );
                mesh.tri(b0, b1, t1, id);
                mesh.tri(b0, t1, t0, id);
            }
        }
        // Canopy. A normal of an ellipsoid scales with the inverse radii. A cone has slanted sides.
        let radii = Vec3::new(r as f32, r as f32, (canopy_h / 2.0) as f32);
        let center = v3(o, at, center_z);
        let place = |p: Vec3| -> (Vec3, Vec3) {
            match form {
                PlantForm::Conical => {
                    // t runs from 0 at the base to 1 at the tip. The radius shrinks with t.
                    let t = (p.z + 1.0) / 2.0;
                    let dir = Vec3::new(p.x, p.y, 0.0).normalize_or_zero();
                    let pos =
                        center + dir * radii.x * (1.0 - t) + Vec3::new(0.0, 0.0, p.z * radii.z);
                    (
                        pos,
                        Vec3::new(dir.x * radii.z, dir.y * radii.z, radii.x).normalize_or_zero(),
                    )
                }
                PlantForm::ShrubMound => {
                    // The lower half is squashed flat onto the ground.
                    let up = p.z.max(0.0);
                    let n = (Vec3::new(p.x, p.y, up) / Vec3::new(radii.x, radii.y, radii.z * 2.0))
                        .normalize_or_zero();
                    (
                        center
                            + Vec3::new(p.x * radii.x, p.y * radii.y, (up * 2.0 - 1.0) * radii.z),
                        n,
                    )
                }
                _ => (center + p * radii, (p / radii).normalize_or_zero()),
            }
        };
        let base = mesh.positions.len() as u32;
        for p in &self.sphere.verts {
            let (pos, n) = place(*p);
            mesh.vertex(pos, n, color);
        }
        for [a, b, c] in &self.sphere.tris {
            mesh.tri(base + a, base + b, base + c, id);
        }
        // Edges: the canopy at its widest.
        let (segments, rings) = (self.sphere.segments, self.sphere.rings);
        let ring_index = if form == PlantForm::Conical {
            1
        } else {
            rings / 2
        };
        let ring: Vec<Vec3> = (0..segments)
            .map(|s| place(self.sphere.verts[ring_index * segments + s]).0)
            .collect();
        self.scene.lines.polyline(&ring, true, edge, id);
    }
}

/// What the mesh builder needs to know about a plant.
#[derive(Clone, Copy)]
struct PlantBuild {
    at: DVec2,
    spread: f64,
    height: f64,
    form: PlantForm,
    color: [u8; 4],
    /// Trunk radius, or `None` for a size that follows the spread.
    trunk_radius: Option<f64>,
}

/// A canopy color from the species name, so that species look different.
fn species_color(species: &str) -> [u8; 4] {
    let h = species
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    [
        (52 + h % 40) as u8,
        (118 + (h >> 4) % 50) as u8,
        (48 + (h >> 9) % 30) as u8,
        255,
    ]
}

/// The visible design-layer objects with their layers. Objects with numbers that the document
/// does not accept (not finite, or beyond `MAX_COORD`) are left out.
fn visible_objects(doc: &Document) -> impl Iterator<Item = (&Object, &Layer)> {
    doc.objects
        .values()
        .filter(|o| doc.is_visible(o) && o.shape.is_in_range())
        .filter_map(|o| doc.layer(o.layer).map(|l| (o, l)))
        .filter(|(_, l)| l.kind == LayerKind::Design)
}

/// The scene origin for a document: the middle of the plan, with Z zero.
/// Objects with non-finite numbers do not count.
fn scene_origin(doc: &Document) -> DVec3 {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for (o, _) in visible_objects(doc) {
        let (a, b) = o.shape.bounds();
        if a.is_finite() && b.is_finite() {
            lo = lo.min(a);
            hi = hi.max(b);
        }
    }
    if lo.x <= hi.x && lo.y <= hi.y {
        ((lo + hi) / 2.0).extend(0.0)
    } else {
        DVec3::ZERO
    }
}

/// Builds the 3D scene of the visible objects, around the middle of the drawing.
pub fn build(doc: &Document) -> Scene {
    build_at(doc, scene_origin(doc))
}

/// Like `build`, with the scene origin given.
pub fn build_at(doc: &Document, origin: DVec3) -> Scene {
    build_with(doc, origin, None)
}

/// Like `build_at`, with an extension that draws site models. Without one, a site model has no
/// 3D geometry.
pub fn build_with(doc: &Document, origin: DVec3, ext: Option<SceneExt<'_>>) -> Scene {
    // Many plants get a coarser canopy, to keep the scene small.
    let plants = doc
        .objects
        .values()
        .filter(|o| matches!(o.shape, Shape::Plant { .. } | Shape::ExistingTree { .. }))
        .count();
    let detail = if plants > COARSE_PLANT_COUNT {
        SPHERE_COARSE
    } else {
        SPHERE_DETAIL
    };
    let mut b = Builder {
        ext,
        doc,
        origin,
        scene: Scene {
            origin,
            ..Scene::default()
        },
        sphere: UnitSphere::new(detail),
    };
    b.scene.ranges.reserve(doc.objects.len());
    for (obj, layer) in visible_objects(doc) {
        let (t0, s0) = (b.scene.mesh.triangle_count(), b.scene.lines.segment_count());
        b.add_object(obj, layer);
        let (t1, s1) = (b.scene.mesh.triangle_count(), b.scene.lines.segment_count());
        if t1 > t0 || s1 > s0 {
            b.scene.ranges.insert(
                obj.id,
                ObjectRange {
                    tris: t0..t1,
                    segs: s0..s1,
                },
            );
        }
    }
    let mut acc = BoundsAcc::default();
    b.scene.mesh.positions.iter().for_each(|p| acc.add(*p));
    b.scene.lines.positions.iter().for_each(|p| acc.add(*p));
    b.scene.bounds = acc.get();
    b.scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::model::{Attributes, FIRST_LAYER, NONE_CLASS, Vertex};
    use std::collections::HashMap;

    fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        ObjectId(out.created[0])
    }

    fn hardscape(pts: &[DVec2], thickness: Option<f64>, elevation: Option<f64>) -> Shape {
        Shape::Hardscape {
            boundary: curve::corners(pts),
            material: "m".into(),
            thickness,
            elevation,
        }
    }

    /// The box of an object in world coordinates.
    fn world_bounds(scene: &Scene, id: ObjectId) -> (DVec3, DVec3) {
        let (lo, hi) = scene.object_bounds(id).unwrap();
        (scene.to_world(lo), scene.to_world(hi))
    }

    fn rect_pts(w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::ZERO,
            DVec2::new(w, 0.0),
            DVec2::new(w, h),
            DVec2::new(0.0, h),
        ]
    }

    fn tri_area_sum(pts: &[DVec2], tris: &[[usize; 3]]) -> f64 {
        tris.iter()
            .map(|[a, b, c]| (pts[*b] - pts[*a]).perp_dot(pts[*c] - pts[*a]) / 2.0)
            .sum()
    }

    /// Signed volume of a closed triangle mesh (divergence theorem).
    fn volume(m: &Mesh) -> f64 {
        (0..m.triangle_count())
            .map(|t| {
                let [a, b, c] = m.triangle(t).map(|p| p.as_dvec3());
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn triangulates_convex_concave_and_both_windings() {
        let square = rect_pts(10.0, 10.0);
        let t = triangulate(&square);
        assert_eq!(t.len(), 2);
        assert!((tri_area_sum(&square, &t) - 100.0).abs() < 1e-9);

        // An L shape, clockwise and counterclockwise.
        let l = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(6.0, 0.0),
            DVec2::new(6.0, 2.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(2.0, 6.0),
            DVec2::new(0.0, 6.0),
        ];
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        assert!((tri_area_sum(&l, &t) - 20.0).abs() < 1e-9);
        let rev: Vec<DVec2> = l.iter().rev().copied().collect();
        let t = triangulate(&rev);
        assert_eq!(t.len(), 4);
        assert!(
            (tri_area_sum(&rev, &t) - 20.0).abs() < 1e-9,
            "all triangles face up"
        );

        // A comb-like polygon with many reflex corners.
        let comb = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(9.0, 0.0),
            DVec2::new(9.0, 5.0),
            DVec2::new(7.0, 5.0),
            DVec2::new(7.0, 1.0),
            DVec2::new(5.0, 1.0),
            DVec2::new(5.0, 5.0),
            DVec2::new(3.0, 5.0),
            DVec2::new(3.0, 1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, 5.0),
            DVec2::new(0.0, 5.0),
        ];
        let t = triangulate(&comb);
        assert!(t.len() >= comb.len() - 4 && t.len() <= comb.len() - 2);
        let area = crate::geom::polygon_area(&comb);
        assert!((tri_area_sum(&comb, &t) - area).abs() < 1e-9);
    }

    #[test]
    fn triangulate_skips_collinear_and_repeated_points() {
        let pts = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(5.0, 0.0),
            DVec2::new(5.0, 0.0),
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 10.0),
            DVec2::new(0.0, 10.0),
        ];
        let t = triangulate(&pts);
        assert!((tri_area_sum(&pts, &t) - 100.0).abs() < 1e-9);
        assert!(triangulate(&pts[..2]).is_empty());
        // No area at all.
        let line = vec![DVec2::ZERO, DVec2::X, DVec2::new(2.0, 0.0)];
        assert!(triangulate(&line).is_empty());
        let spike = vec![
            DVec2::ZERO,
            DVec2::new(4.0, 0.0),
            DVec2::new(4.0, 4.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(0.0, 4.0),
        ];
        assert!(!triangulate(&spike).is_empty());
    }

    #[test]
    fn triangulate_survives_a_self_crossing_polygon() {
        let bow = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(4.0, 4.0),
            DVec2::new(4.0, 0.0),
            DVec2::new(0.0, 4.0),
            DVec2::new(2.0, -1.0),
        ];
        let t = triangulate(&bow);
        assert!(t.len() <= 3);
    }

    #[test]
    fn clean_ring_fixes_orientation_and_removes_dupes() {
        let ring = clean_ring(&[
            DVec2::new(0.0, 0.0),
            DVec2::new(0.0, 1.0),
            DVec2::new(0.0, 2.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(0.0, 0.0),
        ]);
        assert_eq!(ring.len(), 4);
        assert!(crate::geom::polygon_area(&ring) > 0.0);
        let area2: f64 = (0..4).map(|i| ring[i].perp_dot(ring[(i + 1) % 4])).sum();
        assert!(area2 > 0.0, "counterclockwise");
        assert!(clean_ring(&[DVec2::ZERO, DVec2::X]).is_empty());
        assert!(clean_ring(&[DVec2::ZERO, DVec2::X, DVec2::new(2.0, 0.0)]).is_empty());
    }

    #[test]
    fn slab_volume_is_area_times_thickness_and_it_is_watertight() {
        let mut ed = Editor::default();
        let l = [
            DVec2::new(0.0, 0.0),
            DVec2::new(6000.0, 0.0),
            DVec2::new(6000.0, 2000.0),
            DVec2::new(2000.0, 2000.0),
            DVec2::new(2000.0, 6000.0),
            DVec2::new(0.0, 6000.0),
        ];
        add(&mut ed, hardscape(&l, None, None));
        let scene = build(ed.doc());
        let area = 20.0e6;
        assert!(
            (volume(&scene.mesh) - area * 150.0).abs() / (area * 150.0) < 1e-4,
            "volume {}",
            volume(&scene.mesh)
        );
        // Every undirected edge is shared by exactly two triangles, once in each direction.
        let key = |p: Vec3| {
            (
                (p.x * 8.0).round() as i64,
                (p.y * 8.0).round() as i64,
                (p.z * 8.0).round() as i64,
            )
        };
        let mut edges: HashMap<_, i32> = HashMap::new();
        for t in 0..scene.mesh.triangle_count() {
            let tri = scene.mesh.triangle(t).map(key);
            for k in 0..3 {
                *edges.entry((tri[k], tri[(k + 1) % 3])).or_default() += 1;
            }
        }
        for ((a, b), n) in &edges {
            assert_eq!(*n, 1, "directed edge used twice");
            assert_eq!(
                edges.get(&(*b, *a)),
                Some(&1),
                "edge has no opposite partner"
            );
        }
    }

    #[test]
    fn slab_uses_thickness_elevation_and_layer_elevation() {
        let mut ed = Editor::default();
        ed.execute(&Command::AddLayer {
            name: "Upper".into(),
            elevation: 1000.0,
            scale: None,
            kind: None,
        })
        .unwrap();
        let layer = ed.doc().layers.last().unwrap().id;
        let out = ed
            .execute(&Command::AddObject {
                shape: hardscape(&rect_pts(1000.0, 1000.0), Some(300.0), Some(50.0)),
                layer: Some(layer),
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let id = ObjectId(out.created[0]);
        let scene = build(ed.doc());
        let (lo, hi) = world_bounds(&scene, id);
        assert_eq!((lo.z, hi.z), (750.0, 1050.0));
        assert_eq!((lo.x, hi.x, lo.y, hi.y), (0.0, 1000.0, 0.0, 1000.0));
        assert_eq!(scene.bounds, scene.object_bounds(id));
        // 2 top + 2 bottom + 8 side triangles.
        assert_eq!(scene.mesh.triangle_count(), 12);
        assert_eq!(scene.mesh.positions.len(), scene.mesh.normals.len());
        assert_eq!(scene.mesh.positions.len(), scene.mesh.colors.len());
        assert_eq!(scene.mesh.tri_object.len(), 12);
        assert!(scene.mesh.tri_object.iter().all(|o| *o == id));
        // 4 + 4 outline segments, 4 vertical edges.
        assert_eq!(scene.lines.segment_count(), 12);
        // A bad thickness falls back to the default.
        let mut doc = ed.doc().clone();
        if let Shape::Hardscape { thickness, .. } = &mut doc.objects.get_mut(&id).unwrap().shape {
            *thickness = Some(-4.0);
        }
        let (lo, hi) = world_bounds(&build(&doc), id);
        assert_eq!(hi.z - lo.z, 150.0);
    }

    #[test]
    fn all_normals_are_unit_length_and_slab_faces_point_outward() {
        let mut ed = Editor::default();
        add(&mut ed, hardscape(&rect_pts(1000.0, 500.0), None, None));
        add(
            &mut ed,
            Shape::Plant {
                at: DVec2::new(3000.0, 0.0),
                species: "Oak".into(),
                spread: 4000.0,
                height: 8000.0,
            },
        );
        add(
            &mut ed,
            Shape::Circle {
                center: DVec2::new(0.0, 5000.0),
                radius: 700.0,
            },
        );
        let scene = build(ed.doc());
        for n in &scene.mesh.normals {
            assert!((n.length() - 1.0).abs() < 1e-4, "normal {n:?}");
        }
        // Slab triangles: geometric normal agrees with the vertex normal.
        let slab = scene
            .objects()
            .find(|id| scene.mesh.tri_object.iter().filter(|o| *o == id).count() == 12)
            .unwrap();
        for t in 0..scene.mesh.triangle_count() {
            if scene.mesh.tri_object[t] != slab {
                continue;
            }
            let [a, b, c] = scene.mesh.triangle(t);
            let geo = (b - a).cross(c - a).normalize();
            let vn = scene.mesh.normals[scene.mesh.indices[t * 3] as usize];
            assert!(geo.dot(vn) > 0.99, "triangle {t}: {geo:?} vs {vn:?}");
        }
    }

    #[test]
    fn plant_bounds_are_spread_and_height() {
        let mut ed = Editor::default();
        let id = add(
            &mut ed,
            Shape::Plant {
                at: DVec2::new(1000.0, -2000.0),
                species: "Acer".into(),
                spread: 4000.0,
                height: 8000.0,
            },
        );
        let scene = build(ed.doc());
        let (lo, hi) = world_bounds(&scene, id);
        assert!(
            (lo.x - -1000.0).abs() < 1e-2 && (hi.x - 3000.0).abs() < 1e-2,
            "{lo:?} {hi:?}"
        );
        assert!(
            (lo.y - -4000.0).abs() < 1e-2 && (hi.y - 0.0).abs() < 1e-2,
            "{lo:?} {hi:?}"
        );
        assert!((lo.z - 0.0).abs() < 1e-3 && (hi.z - 8000.0).abs() < 1e-2);
        assert!(scene.mesh.triangle_count() > 50);
        // Degenerate plants have no geometry.
        let mut doc = Document::default();
        let obj = Object::new(
            ObjectId(50),
            FIRST_LAYER,
            NONE_CLASS,
            Shape::Plant {
                at: DVec2::ZERO,
                species: "x".into(),
                spread: 0.0,
                height: 8000.0,
            },
        );
        doc.objects.insert(obj.id, obj);
        assert!(build(&doc).is_empty());
    }

    #[test]
    fn locus3d_and_polyline3d_use_their_z() {
        let mut ed = Editor::default();
        let l = add(
            &mut ed,
            Shape::Locus3d {
                p: DVec3::new(10.0, 20.0, 5.0),
            },
        );
        let open = add(
            &mut ed,
            Shape::Polyline3d {
                points: vec![
                    DVec3::new(0.0, 0.0, 1.0),
                    DVec3::new(1.0, 0.0, 7.0),
                    DVec3::new(1.0, 1.0, 3.0),
                ],
                closed: false,
            },
        );
        let closed = add(
            &mut ed,
            Shape::Polyline3d {
                points: vec![
                    DVec3::new(0.0, 0.0, 1.0),
                    DVec3::new(1.0, 0.0, 7.0),
                    DVec3::new(1.0, 1.0, 3.0),
                ],
                closed: true,
            },
        );
        let scene = build(ed.doc());
        let segs = |id: ObjectId| scene.lines.objects.iter().filter(|o| **o == id).count();
        assert_eq!((segs(l), segs(open), segs(closed)), (3, 2, 3));
        assert_eq!(scene.mesh.triangle_count(), 0);
        let (lo, hi) = world_bounds(&scene, l);
        assert!(
            (lo.z - (5.0 - crate::model::LOCUS_MARKER_HALF)).abs() < 1e-3
                && (hi.z - (5.0 + crate::model::LOCUS_MARKER_HALF)).abs() < 1e-3
        );
        assert!((lo.x - (10.0 - crate::model::LOCUS_MARKER_HALF)).abs() < 1e-3);
        let (lo, hi) = world_bounds(&scene, open);
        assert_eq!((lo.z, hi.z), (1.0, 7.0));
    }

    #[test]
    fn flat_faces_and_lines() {
        let mut ed = Editor::default();
        let r = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::new(10.0, 10.0),
                max: DVec2::ZERO,
            },
        );
        let c = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 500.0,
            },
        );
        let a = add(
            &mut ed,
            Shape::LandscapeArea {
                boundary: curve::corners(&rect_pts(100.0, 100.0)),
                name: "n".into(),
            },
        );
        let rounded = vec![
            Vertex::new(
                DVec2::new(0.0, 0.0),
                curve::VertexKind::Arc { radius: 30.0 },
            ),
            Vertex::new(
                DVec2::new(100.0, 0.0),
                curve::VertexKind::Arc { radius: 30.0 },
            ),
            Vertex::corner(DVec2::new(100.0, 100.0)),
            Vertex::corner(DVec2::new(0.0, 100.0)),
        ];
        let p = add(
            &mut ed,
            Shape::Polyline {
                points: rounded.clone(),
                closed: true,
            },
        );
        let l = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(5.0, 5.0),
            },
        );
        let arc = add(
            &mut ed,
            Shape::Arc {
                center: DVec2::ZERO,
                radius: 100.0,
                start: 0.0,
                end: std::f64::consts::FRAC_PI_2,
            },
        );
        let open = add(
            &mut ed,
            Shape::Polyline {
                points: curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::ONE]),
                closed: false,
            },
        );
        add(
            &mut ed,
            Shape::Text {
                at: DVec2::ZERO,
                text: "t".into(),
                height: 5.0,
            },
        );
        let scene = build(ed.doc());
        assert_eq!(scene.objects().count(), 7, "text has no 3D form");
        let tris = |id: ObjectId| scene.mesh.tri_object.iter().filter(|o| **o == id).count();
        let segs = |id: ObjectId| scene.lines.objects.iter().filter(|o| **o == id).count();
        assert_eq!((tris(r), segs(r)), (2, 4));
        assert!(tris(c) >= 10 && segs(c) >= 12);
        assert_eq!(tris(a), 2);
        assert!(tris(p) > 2, "rounded corners add triangles");
        assert_eq!((tris(l), segs(l)), (0, 1));
        assert!(segs(arc) >= 2 && tris(arc) == 0);
        assert_eq!((tris(open), segs(open)), (0, 2));
        // A flat face lies at the layer elevation and points up.
        let (lo, hi) = world_bounds(&scene, r);
        assert_eq!((lo.z, hi.z), (0.0, 0.0));
        // The circle's face area is close to pi r^2.
        let area: f64 = (0..scene.mesh.triangle_count())
            .filter(|t| scene.mesh.tri_object[*t] == c)
            .map(|t| {
                let [a, b, c] = scene.mesh.triangle(t);
                ((b - a).cross(c - a).z / 2.0) as f64
            })
            .sum();
        assert!(
            (area - std::f64::consts::PI * 500.0 * 500.0).abs() / (std::f64::consts::PI * 250000.0)
                < 0.01
        );
        let _ = rounded;
    }

    #[test]
    fn full_circle_arc_closes_up() {
        let pts = arc_points(DVec2::ZERO, 100.0, 0.0, std::f64::consts::TAU);
        assert!(pts.first().unwrap().distance(*pts.last().unwrap()) < 1e-9);
        let quarter = arc_points(DVec2::ZERO, 100.0, 0.0, std::f64::consts::FRAC_PI_2);
        assert!(quarter.last().unwrap().distance(DVec2::new(0.0, 100.0)) < 1e-9);
        assert_eq!(circle_segments(1e9), 256);
        assert_eq!(circle_segments(0.0), 12);
        assert_eq!(circle_segments(1.0), 12);
    }

    #[test]
    fn hidden_layers_classes_and_sheets_are_skipped() {
        let mut ed = Editor::default();
        let a = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::splat(10.0),
            },
        );
        assert!(!build(ed.doc()).is_empty());
        ed.execute(&Command::AddClass {
            name: "Hide".into(),
            attrs: Attributes::default(),
        })
        .unwrap();
        let class = ed.doc().classes.last().unwrap().id;
        ed.execute(&Command::ModifyObject {
            id: a,
            shape: None,
            layer: None,
            class: Some(class),
            attrs: None,
        })
        .unwrap();
        ed.execute(&Command::ModifyClass {
            id: class,
            name: None,
            visible: Some(false),
            attrs: None,
        })
        .unwrap();
        assert!(build(ed.doc()).is_empty(), "class hidden");
        ed.execute(&Command::ModifyClass {
            id: class,
            name: None,
            visible: Some(true),
            attrs: None,
        })
        .unwrap();
        assert!(!build(ed.doc()).is_empty());
        ed.execute(&Command::ModifyLayer {
            id: FIRST_LAYER,
            name: None,
            visible: Some(false),
            locked: None,
            elevation: None,
            scale: None,
        })
        .unwrap();
        assert!(build(ed.doc()).is_empty(), "layer hidden");
        let mut doc = ed.doc().clone();
        doc.layers[0].visible = true;
        doc.layers[0].kind = LayerKind::Sheet;
        assert!(build(&doc).is_empty(), "sheet layer");
    }

    #[test]
    fn fill_and_pen_colors_are_used() {
        let mut ed = Editor::default();
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::Rect {
                    min: DVec2::ZERO,
                    max: DVec2::splat(10.0),
                },
                layer: None,
                class: None,
                attrs: Attributes {
                    pen: Some(Color([1, 2, 3, 255])),
                    fill: Some(Color([10, 20, 30, 255])),
                    line_weight: None,
                    ..Default::default()
                },
            })
            .unwrap();
        let scene = build(ed.doc());
        assert_eq!(scene.mesh.colors[0], [10, 20, 30, 255]);
        assert_eq!(scene.lines.colors[0], [1, 2, 3, 255]);
        assert!(scene.object_bounds(ObjectId(out.created[0])).is_some());
        assert!(scene.object_bounds(ObjectId(9999)).is_none());
    }

    #[test]
    fn cache_rebuilds_only_when_the_document_changes() {
        let mut ed = Editor::default();
        let mut cache = MeshCache::default();
        let s1 = cache.get(ed.revision(), ed.doc());
        assert!(s1.is_empty());
        assert_eq!(cache.builds, 1);
        let s2 = cache.get(ed.revision(), ed.doc());
        assert!(Arc::ptr_eq(&s1, &s2));
        assert_eq!(cache.builds, 1, "no change, no rebuild");

        add(
            &mut ed,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::splat(10.0),
            },
        );
        assert!(!cache.get(ed.revision(), ed.doc()).is_empty());
        assert_eq!(cache.builds, 2);
        cache.get(ed.revision(), ed.doc());
        assert_eq!(cache.builds, 2);

        // A failed command does not change the document or the revision.
        let before = ed.revision();
        assert!(
            ed.execute(&Command::DeleteObjects {
                ids: vec![ObjectId(999)]
            })
            .is_err()
        );
        assert_eq!(ed.revision(), before);
        cache.get(ed.revision(), ed.doc());
        assert_eq!(cache.builds, 2);

        ed.undo();
        assert!(cache.get(ed.revision(), ed.doc()).is_empty());
        assert_eq!(cache.builds, 3);
        ed.redo();
        assert!(!cache.get(ed.revision(), ed.doc()).is_empty());
        assert_eq!(cache.builds, 4);

        // Another editor never shares a revision.
        let other = Editor::default();
        assert_ne!(other.revision(), ed.revision());
        assert!(cache.get(other.revision(), other.doc()).is_empty());
        assert_eq!(cache.builds, 5);

        cache.invalidate();
        assert_eq!(cache.revision(), None);
        cache.get(other.revision(), other.doc());
        assert_eq!(cache.builds, 6);
        assert_eq!(cache.revision(), Some(other.revision()));
    }

    #[test]
    fn survey_coordinates_keep_their_precision_through_the_scene_origin() {
        // A site far from the world origin: single-precision coordinates would give a 0.5 m grid.
        let (x0, y0) = (6_500_000.0, 5_900_000.0);
        let mut ed = Editor::default();
        let id = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::new(x0, y0),
                max: DVec2::new(x0 + 1234.5, y0 + 678.25),
            },
        );
        let scene = build(ed.doc());
        assert!(
            (scene.origin.x - (x0 + 617.25)).abs() < 1e-6
                && (scene.origin.y - (y0 + 339.125)).abs() < 1e-6,
            "{:?}",
            scene.origin
        );
        let (lo, hi) = scene.object_bounds(id).unwrap();
        assert!(
            ((hi.x - lo.x) - 1234.5).abs() < 1e-3 && ((hi.y - lo.y) - 678.25).abs() < 1e-3,
            "{lo:?} {hi:?}"
        );
        let (wlo, _) = world_bounds(&scene, id);
        assert!((wlo.x - x0).abs() < 1e-3 && (wlo.y - y0).abs() < 1e-3);
        // build_at puts the origin where it is asked to.
        assert_eq!(build_at(ed.doc(), DVec3::ZERO).origin, DVec3::ZERO);
        assert_eq!(build(&Document::default()).origin, DVec3::ZERO);
    }

    #[test]
    fn objects_are_listed_in_document_order() {
        let mut ed = Editor::default();
        let ids: Vec<ObjectId> = (0..30)
            .map(|i| {
                add(
                    &mut ed,
                    Shape::Circle {
                        center: DVec2::new(i as f64 * 100.0, 0.0),
                        radius: 10.0,
                    },
                )
            })
            .collect();
        let scene = build(ed.doc());
        assert_eq!(scene.objects().collect::<Vec<_>>(), ids);
    }

    #[test]
    fn non_finite_numbers_in_a_document_do_not_reach_the_bounds() {
        let mut doc = Document::default();
        for (i, shape) in [
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::splat(10.0),
            },
            Shape::Rect {
                min: DVec2::splat(f64::NAN),
                max: DVec2::splat(1e39),
            },
            Shape::Circle {
                center: DVec2::ZERO,
                radius: f64::INFINITY,
            },
        ]
        .into_iter()
        .enumerate()
        {
            let o = Object::new(ObjectId(10 + i as u64), FIRST_LAYER, NONE_CLASS, shape);
            doc.objects.insert(o.id, o);
        }
        let scene = build(&doc);
        let (lo, hi) = scene.bounds.unwrap();
        assert!(lo.is_finite() && hi.is_finite());
        assert_eq!(scene.objects().count(), 1, "only the valid rect is drawn");
    }

    #[test]
    fn scene_empty_checks() {
        let s = Scene::default();
        assert!(s.is_empty());
        assert!(s.bounds.is_none());
    }

    fn styled_plant(form: PlantForm, spread: f64, height: f64) -> (Editor, ObjectId) {
        use crate::plants::style::PlantStyle;
        use crate::resource::{ResourceBody, ResourceId};
        let mut ed = Editor::default();
        let style = PlantStyle {
            form,
            spread,
            height,
            ..PlantStyle::default()
        };
        let sid = ResourceId(
            ed.execute(&Command::AddResource {
                name: "S".into(),
                body: ResourceBody::plant_style(style),
            })
            .unwrap()
            .created[0],
        );
        let out = ed
            .execute(&Command::AddPlants {
                style: Some(sid),
                overrides: Default::default(),
                positions: vec![DVec2::new(1000.0, 2000.0)],
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
        (ed, ObjectId(out.created[0]))
    }

    #[test]
    fn every_plant_form_fits_its_spread_and_height() {
        for form in PlantForm::ALL {
            let (ed, id) = styled_plant(form, 3000.0, 6000.0);
            let scene = build(ed.doc());
            let (lo, hi) = world_bounds(&scene, id);
            assert!((hi.z - 6000.0).abs() < 1.0, "{form:?} top {}", hi.z);
            assert!(lo.z.abs() < 1.0, "{form:?} bottom {}", lo.z);
            assert!(
                hi.x - lo.x <= 3000.0 + 1.0 && hi.x - lo.x > 2500.0,
                "{form:?} width {}",
                hi.x - lo.x
            );
            assert!(
                ((lo.x + hi.x) / 2.0 - 1000.0).abs() < 1.0
                    && ((lo.y + hi.y) / 2.0 - 2000.0).abs() < 1.0
            );
            assert!(scene.mesh.triangle_count() > 10);
            for n in &scene.mesh.normals {
                assert!(
                    (n.length() - 1.0).abs() < 1e-3 || n.length() == 0.0,
                    "{form:?} normal {n:?}"
                );
            }
            assert!(scene.lines.segment_count() > 0);
        }
    }

    #[test]
    fn plant_forms_differ_in_shape() {
        let positions = |f| {
            build(styled_plant(f, 3000.0, 6000.0).0.doc())
                .mesh
                .positions
                .clone()
        };
        let tree = positions(PlantForm::Tree);
        for f in [
            PlantForm::Columnar,
            PlantForm::Conical,
            PlantForm::Spreading,
            PlantForm::ShrubMound,
        ] {
            assert_ne!(positions(f), tree, "{f:?}");
        }
        // A mound has no trunk, so it has fewer triangles than a tree.
        let tris = |f| {
            build(styled_plant(f, 3000.0, 600.0).0.doc())
                .mesh
                .triangle_count()
        };
        assert!(tris(PlantForm::ShrubMound) < tris(PlantForm::Tree));
    }

    #[test]
    fn style_fill_colors_the_canopy_and_the_object_fill_wins() {
        use crate::model::Attributes;
        use crate::plants::style::PlantStyle;
        use crate::resource::{ResourceBody, ResourceId};
        let mut ed = Editor::default();
        let style = PlantStyle {
            fill: Some(Color([200, 10, 20, 255])),
            ..PlantStyle::default()
        };
        let sid = ResourceId(
            ed.execute(&Command::AddResource {
                name: "S".into(),
                body: ResourceBody::plant_style(style),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::AddPlants {
            style: Some(sid),
            overrides: Default::default(),
            positions: vec![DVec2::ZERO],
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        let scene = build(ed.doc());
        assert!(scene.mesh.colors.contains(&[200, 10, 20, 255]));
        let id = *ed.doc().objects.keys().next().unwrap();
        ed.execute(&Command::ModifyObject {
            id,
            shape: None,
            layer: None,
            class: None,
            attrs: Some(Attributes {
                fill: Some(Color([1, 2, 3, 255])),
                ..Default::default()
            }),
        })
        .unwrap();
        let scene = build(ed.doc());
        assert!(
            scene.mesh.colors.contains(&[1, 2, 3, 255])
                && !scene.mesh.colors.contains(&[200, 10, 20, 255])
        );
    }

    #[test]
    fn existing_tree_has_a_trunk_as_thick_as_its_dbh() {
        let mut ed = Editor::default();
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::ExistingTree {
                    at: DVec2::new(0.0, 0.0),
                    species: "Quercus".into(),
                    dbh: 600.0,
                    spread: 8000.0,
                    height: 14000.0,
                    condition: Default::default(),
                    action: Default::default(),
                    rpz_factor: None,
                    rpz_cap: None,
                },
                layer: None,
                class: None,
                attrs: crate::model::Attributes::default(),
            })
            .unwrap();
        let scene = build_at(ed.doc(), DVec3::ZERO);
        let (lo, hi) = world_bounds(&scene, ObjectId(out.created[0]));
        assert!((hi.z - 14000.0).abs() < 1.0 && (hi.x - lo.x - 8000.0).abs() < 1.0);
        // The trunk vertices are 300 mm from the axis.
        let on_trunk = scene
            .mesh
            .positions
            .iter()
            .filter(|p| (p.x as f64).hypot(p.y as f64).round() == 300.0)
            .count();
        assert!(on_trunk >= 16, "{on_trunk}");
    }

    #[test]
    fn many_plants_get_a_coarser_canopy() {
        let make = |n: usize| {
            let (mut ed, _) = styled_plant(PlantForm::Tree, 3000.0, 6000.0);
            let positions = (0..n).map(|i| DVec2::new(i as f64 * 4000.0, 0.0)).collect();
            ed.execute(&Command::AddPlants {
                style: None,
                overrides: Default::default(),
                positions,
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
            build(ed.doc()).mesh.triangle_count() as f64 / (n + 1) as f64
        };
        let few = make(10);
        let many = make(COARSE_PLANT_COUNT + 10);
        assert!(
            many < few * 0.6,
            "triangles per plant: {few} for few, {many} for many"
        );
        // The coarse canopy is still a closed shape of the right size.
        let sphere = UnitSphere::new(SPHERE_COARSE);
        assert_eq!(sphere.verts.len(), 8 * 5);
        assert!(sphere.verts.iter().all(|v| (v.length() - 1.0).abs() < 1e-5));
    }

    #[test]
    fn tags_make_no_mesh() {
        let mut ed = Editor::default();
        let sid = {
            use crate::resource::ResourceBody;
            crate::resource::ResourceId(
                ed.execute(&Command::AddResource {
                    name: "S".into(),
                    body: ResourceBody::plant_style(Default::default()),
                })
                .unwrap()
                .created[0],
            )
        };
        ed.execute(&Command::AddObject {
            shape: Shape::PlantTag {
                at: DVec2::ZERO,
                anchor: DVec2::new(5.0, 5.0),
                style: sid,
                height: 400.0,
            },
            layer: None,
            class: None,
            attrs: crate::model::Attributes::default(),
        })
        .unwrap();
        assert!(build(ed.doc()).mesh.triangle_count() == 0);
    }
}
