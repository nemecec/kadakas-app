//! The scene the inference works on: points, straight edges and planar faces in world
//! coordinates, with a spatial index. A surface model (MOD-06) is added with
//! [`SceneBuilder::add_surface_model`]; other 3D objects add their points, edges and faces with the
//! plain `add_*` calls. Build the scene again when the geometry changes.

use glam::{DAffine3, DVec3};
use kadakas_core::surface::{CurveId, EdgeId, FaceId, SurfaceModel};

use super::bvh::{Aabb, Bvh};
use super::math::{circumcenter, newell, world_eps};

/// What a caller knows about a piece of the scene. The inference gives it back in the result,
/// so a tool can tell which edge or face was hit. The tags of a surface model say the model (the
/// number given to `add_surface_model`), the kind and the id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(pub u64);

/// The kind part of a [`Tag`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagKind {
    Edge,
    Face,
    Curve,
    /// A guide (MOD-05): the number says which one of the guides given to the scene.
    Guide,
    Other,
}

impl Tag {
    /// For geometry that has no tag.
    pub const NONE: Tag = Tag(u64::MAX);

    fn pack(kind: u64, model: u32, id: u32) -> Tag {
        Tag(kind << 62 | u64::from(model & 0x3fff_ffff) << 32 | u64::from(id))
    }

    pub fn edge(model: u32, id: EdgeId) -> Tag {
        Tag::pack(0, model, id.0)
    }

    pub fn face(model: u32, id: FaceId) -> Tag {
        Tag::pack(1, model, id.0)
    }

    pub fn curve(model: u32, id: CurveId) -> Tag {
        Tag::pack(2, model, id.0)
    }

    /// The tag of the guide with this number in the list of guides of the scene.
    pub fn guide(index: u32) -> Tag {
        Tag::pack(3, 0, index)
    }

    pub fn kind(self) -> TagKind {
        if self == Tag::NONE {
            return TagKind::Other;
        }
        match self.0 >> 62 {
            0 => TagKind::Edge,
            1 => TagKind::Face,
            2 => TagKind::Curve,
            _ => TagKind::Guide,
        }
    }

    /// The model number of a surface model tag.
    pub fn model(self) -> u32 {
        ((self.0 >> 32) & 0x3fff_ffff) as u32
    }

    /// The id inside the model (an edge, face or curve id).
    pub fn id(self) -> u32 {
        self.0 as u32
    }

    /// The same tag with `add` added to the model number (for the models of an instance). A tag
    /// that has no model is returned as it is.
    pub(crate) fn shifted(self, add: u32) -> Tag {
        if self == Tag::NONE || matches!(self.kind(), TagKind::Other | TagKind::Guide) {
            return self;
        }
        Tag::pack(self.0 >> 62, self.model().wrapping_add(add), self.id())
    }
}

/// What an explicit point is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    /// A vertex or an end.
    Endpoint,
    /// A guide point (MOD-05).
    Guide,
    /// The center of a circle, an arc or a regular polygon.
    Center,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PointItem {
    pub p: DVec3,
    pub kind: PointKind,
    pub tag: Tag,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EdgeItem {
    pub a: DVec3,
    pub b: DVec3,
    pub tag: Tag,
}

#[derive(Clone, Debug)]
pub(crate) struct FaceItem {
    /// The outer loop first, then the holes.
    pub loops: Vec<Vec<DVec3>>,
    pub normal: DVec3,
    /// The axis to leave out when the polygon is flattened for the inside test.
    pub drop: usize,
    pub tag: Tag,
}

impl FaceItem {
    pub fn origin(&self) -> DVec3 {
        self.loops[0][0]
    }
}

/// A scene under construction. It owns its data and is `Send`, so a worker thread can build it.
#[derive(Clone, Debug, Default)]
pub struct SceneBuilder {
    points: Vec<PointItem>,
    edges: Vec<EdgeItem>,
    faces: Vec<FaceItem>,
    lines: Vec<LineItem>,
    reach: f64,
}

/// Edges of a surface model that carry this attribute with the value `true` are not offered.
pub const HIDDEN_ATTR: &str = "hidden";

/// How far a guide line without ends reaches for the inference, on each side of its point (mm),
/// when the scene is small: 2 km. A scene sets a larger reach from its size
/// ([`SceneBuilder::set_reach`]). Beyond the reach the pointer does not snap to the line.
pub const GUIDE_REACH: f64 = 2.0e6;

/// How far the window draws a line without ends. The line is cut to the view before it is drawn, so
/// this only has to be larger than any scene.
pub const GUIDE_DRAW_REACH: f64 = 1.0e9;

/// The reach of a scene of the given extent (the largest coordinate): ten times the extent, but at
/// least [`GUIDE_REACH`].
pub fn reach_for(extent: f64) -> f64 {
    GUIDE_REACH.max(10.0 * extent.abs())
}

/// A guide line without ends: kept apart from the index, in a short list.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LineItem {
    pub origin: DVec3,
    pub dir: DVec3,
    pub reach: f64,
    pub tag: Tag,
}

impl LineItem {
    /// The part of the line the inference looks at, as an edge.
    pub(crate) fn as_edge(&self) -> EdgeItem {
        EdgeItem {
            a: self.origin - self.dir * self.reach,
            b: self.origin + self.dir * self.reach,
            tag: self.tag,
        }
    }
}

impl SceneBuilder {
    pub fn new() -> SceneBuilder {
        SceneBuilder::default()
    }

    /// How far the guide lines without ends that are added after this call reach on each side of
    /// their point. See [`reach_for`].
    pub fn set_reach(&mut self, reach: f64) -> &mut Self {
        if reach.is_finite() && reach > 0.0 {
            self.reach = reach;
        }
        self
    }

    /// A point to snap to. A point or a center that is not finite is left out.
    pub fn add_point(&mut self, p: DVec3, kind: PointKind, tag: Tag) -> &mut Self {
        if p.is_finite() {
            self.points.push(PointItem { p, kind, tag });
        }
        self
    }

    /// A straight edge. Its two ends are endpoints and its middle is a midpoint. An edge that is
    /// not finite or has no length is left out.
    pub fn add_edge(&mut self, a: DVec3, b: DVec3, tag: Tag) -> &mut Self {
        if a.is_finite() && b.is_finite() && a != b {
            self.edges.push(EdgeItem { a, b, tag });
        }
        self
    }

    /// A guide of the document (MOD-05). `z0` is the elevation of its layer, `index` the number
    /// that goes into its tag ([`Tag::guide`]). A guide point is a point to snap to (a guide
    /// point). A guide line is a long edge that only offers "on guide" and its crossings. A line
    /// without ends reaches [`GUIDE_REACH`] on each side. A guide that is not finite is left out.
    pub fn add_guide(&mut self, g: &kadakas_core::guide::Guide, z0: f64, index: u32) -> &mut Self {
        let at = g.at + DVec3::new(0.0, 0.0, z0);
        let tag = Tag::guide(index);
        match g.unit_dir() {
            None if g.dir.is_none() => {
                self.add_point(at, PointKind::Guide, tag);
            }
            None => {}
            Some(d) => match g.length {
                Some(l) => {
                    self.add_edge(at, at + d * l, tag);
                }
                None if at.is_finite() => self.lines.push(LineItem {
                    origin: at,
                    dir: d,
                    reach: if self.reach > 0.0 {
                        self.reach
                    } else {
                        GUIDE_REACH
                    },
                    tag,
                }),
                None => {}
            },
        }
        self
    }

    /// A planar face from its outer loop and its holes. The face is a hit target for "on face"
    /// and for the intersections of edges with it. A face with fewer than three points or no area
    /// is left out. It gives no center point: a rectangle or a square has none. For the face of
    /// a circle or a regular polygon use [`SceneBuilder::add_polygon_face`].
    pub fn add_face(&mut self, outer: Vec<DVec3>, holes: Vec<Vec<DVec3>>, tag: Tag) -> &mut Self {
        self.push_face(outer, holes, tag, false)
    }

    /// A face that is the inside of a circle or a regular polygon (a curve). Like
    /// [`SceneBuilder::add_face`], and when all corners lie on a circle with equal sides (and
    /// there are no holes) the center is a point to snap to. A surface model gets the centers of
    /// its curves from [`SceneBuilder::add_surface_model`].
    pub fn add_polygon_face(
        &mut self,
        outer: Vec<DVec3>,
        holes: Vec<Vec<DVec3>>,
        tag: Tag,
    ) -> &mut Self {
        self.push_face(outer, holes, tag, true)
    }

    fn push_face(
        &mut self,
        outer: Vec<DVec3>,
        holes: Vec<Vec<DVec3>>,
        tag: Tag,
        curve: bool,
    ) -> &mut Self {
        if outer.len() < 3 || !outer.iter().all(|p| p.is_finite()) {
            return self;
        }
        let n = newell(&outer);
        let len = n.length();
        let size = outer
            .iter()
            .fold(0.0f64, |m, p| m.max(p.abs().max_element()));
        if len <= 1e-12 * (1.0 + size * size) {
            return self;
        }
        let normal = n / len;
        let drop = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() {
            0
        } else if normal.y.abs() >= normal.z.abs() {
            1
        } else {
            2
        };
        if curve
            && holes.is_empty()
            && let Some(c) = regular_center(&outer)
        {
            self.points.push(PointItem {
                p: c,
                kind: PointKind::Center,
                tag,
            });
        }
        let mut loops = vec![outer];
        loops.extend(
            holes
                .into_iter()
                .filter(|h| h.len() >= 3 && h.iter().all(|p| p.is_finite())),
        );
        self.faces.push(FaceItem {
            loops,
            normal,
            drop,
            tag,
        });
        self
    }

    /// A surface model: its edges (not the hidden ones), its faces and the centers of its curves
    /// that are circles or arcs. `model` is the number that goes into the tags.
    pub fn add_surface_model(&mut self, m: &SurfaceModel, model: u32) -> &mut Self {
        self.add_surface_model_placed(m, &m.transform, model)
    }

    /// Like [`SceneBuilder::add_surface_model`], with the placement `xf` (from the local
    /// coordinates of the model to the scene) instead of the transform of the model. A model of a
    /// component definition is placed by the instance and the transform of the model inside it.
    pub fn add_surface_model_placed(
        &mut self,
        m: &SurfaceModel,
        xf: &DAffine3,
        model: u32,
    ) -> &mut Self {
        self.add_surface_model_placed_with(m, xf, model, false)
    }

    /// Like [`SceneBuilder::add_surface_model_placed`]. With `show_hidden` the hidden edges and
    /// faces (Hide, MOD-05) are offered too: View > Hidden Geometry makes them selectable.
    pub fn add_surface_model_placed_with(
        &mut self,
        m: &SurfaceModel,
        xf: &DAffine3,
        model: u32,
        show_hidden: bool,
    ) -> &mut Self {
        let w = |p: DVec3| xf.transform_point3(p);
        let hidden = |attrs: Option<&kadakas_core::surface::Attrs>| {
            !show_hidden
                && attrs
                    .and_then(|a| a.get(HIDDEN_ATTR))
                    .is_some_and(|v| v == &serde_json::Value::Bool(true))
        };
        for (id, e) in m.edges() {
            if hidden(m.edge_attrs(id)) {
                continue;
            }
            if let (Some(a), Some(b)) = (m.vertex(e.v[0]), m.vertex(e.v[1])) {
                self.add_edge(w(a), w(b), Tag::edge(model, id));
            }
        }
        for (id, f) in m.faces() {
            if hidden(Some(&f.attrs)) {
                continue;
            }
            let loops: Vec<Vec<DVec3>> = f
                .loops()
                .map(|l| {
                    l.iter()
                        .filter_map(|v| m.vertex(*v).map(w))
                        .collect::<Vec<_>>()
                })
                .collect();
            let mut it = loops.into_iter();
            if let Some(outer) = it.next() {
                self.add_face(outer, it.collect(), Tag::face(model, id));
            }
        }
        for (cid, edges) in m.curves() {
            if let Some(c) = curve_center(m, edges) {
                self.add_point(w(c), PointKind::Center, Tag::curve(model, cid));
            }
        }
        self
    }

    /// Adds everything of `other` placed by `xf`, with `shift` added to the model number of every
    /// tag. This is how the instances of a component share the data of the definition: `other`
    /// is built once, in the coordinates of the definition ([`super::DefinitionScene`]), and each
    /// instance adds a placed copy of the points, edges and faces, which is all the index needs.
    /// The definition itself, its surface models and their ids, are not copied.
    pub fn add_placed(&mut self, other: &SceneBuilder, xf: &DAffine3, shift: u32) -> &mut Self {
        let w = |p: DVec3| xf.transform_point3(p);
        self.points.reserve(other.points.len());
        for p in &other.points {
            self.add_point(w(p.p), p.kind, p.tag.shifted(shift));
        }
        self.edges.reserve(other.edges.len());
        for e in &other.edges {
            self.add_edge(w(e.a), w(e.b), e.tag.shifted(shift));
        }
        self.faces.reserve(other.faces.len());
        for f in &other.faces {
            let mut loops = f
                .loops
                .iter()
                .map(|l| l.iter().map(|p| w(*p)).collect::<Vec<_>>());
            if let Some(outer) = loops.next() {
                self.push_face(outer, loops.collect(), f.tag.shifted(shift), false);
            }
        }
        self
    }

    /// Builds the spatial index.
    pub fn finish(self) -> Scene {
        let pad = DVec3::ZERO;
        let pb: Vec<Aabb> = self
            .points
            .iter()
            .map(|p| Aabb {
                min: p.p - pad,
                max: p.p + pad,
            })
            .collect();
        let eb: Vec<Aabb> = self
            .edges
            .iter()
            .map(|e| Aabb::of_points(&[e.a, e.b]))
            .collect();
        let fb: Vec<Aabb> = self
            .faces
            .iter()
            .map(|f| f.loops[0].iter().fold(Aabb::EMPTY, |b, p| b.grow(*p)))
            .collect();
        let mut scale = 0.0f64;
        for b in pb.iter().chain(&eb).chain(&fb) {
            scale = scale
                .max(b.min.abs().max_element())
                .max(b.max.abs().max_element());
        }
        Scene {
            bvh_points: Bvh::build(&pb),
            bvh_edges: Bvh::build(&eb),
            bvh_faces: Bvh::build(&fb),
            points: self.points,
            edges: self.edges,
            faces: self.faces,
            lines: self.lines,
            scale,
        }
    }
}

/// The center of a regular polygon (all corners at the same distance from the middle, all sides
/// equal), or `None`.
fn regular_center(outer: &[DVec3]) -> Option<DVec3> {
    let n = outer.len();
    let c = outer.iter().copied().sum::<DVec3>() / n as f64;
    let r = outer.iter().map(|p| p.distance(c)).sum::<f64>() / n as f64;
    if r <= 0.0 {
        return None;
    }
    let tol = 1e-6 * r;
    let side = outer[0].distance(outer[1]);
    let ok = (0..n).all(|i| {
        (outer[i].distance(c) - r).abs() <= tol
            && (outer[i].distance(outer[(i + 1) % n]) - side).abs() <= tol
    });
    ok.then_some(c)
}

/// The center of a curve (a chain of edges) that is a circle or an arc, or `None` for another
/// curve. The corners must lie on one circle.
fn curve_center(m: &SurfaceModel, edges: &[EdgeId]) -> Option<DVec3> {
    // Walk the chain from the first edge.
    let first = m.edge(*edges.first()?)?;
    let second = edges.get(1).and_then(|e| m.edge(*e));
    let mut verts = Vec::with_capacity(edges.len() + 1);
    match second {
        Some(s) if s.v.contains(&first.v[1]) => verts.extend([first.v[0], first.v[1]]),
        Some(_) => verts.extend([first.v[1], first.v[0]]),
        None => verts.extend([first.v[0], first.v[1]]),
    }
    for id in &edges[1..] {
        let e = m.edge(*id)?;
        let last = *verts.last()?;
        verts.push(e.other(last)?);
    }
    let closed = verts.len() > 2 && verts.first() == verts.last();
    if closed {
        verts.pop();
    }
    let pts: Vec<DVec3> = verts.iter().map(|v| m.vertex(*v)).collect::<Option<_>>()?;
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let c = circumcenter(pts[0], pts[n / 2], pts[n - 1])
        .or_else(|| circumcenter(pts[0], pts[n / 3], pts[2 * n / 3]))?;
    let r = c.distance(pts[0]);
    let tol = 1e-6 * r.max(1e-9);
    // All corners on the circle and in its plane.
    let normal = (pts[n / 2] - pts[0])
        .cross(pts[n - 1] - pts[0])
        .normalize_or_zero();
    let on_circle = pts
        .iter()
        .all(|p| (p.distance(c) - r).abs() <= tol && (*p - c).dot(normal).abs() <= tol);
    on_circle.then_some(c)
}

/// The geometry of a scene with its index.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub(crate) points: Vec<PointItem>,
    pub(crate) edges: Vec<EdgeItem>,
    pub(crate) faces: Vec<FaceItem>,
    /// Guide lines without ends.
    pub(crate) lines: Vec<LineItem>,
    pub(crate) bvh_points: Bvh,
    pub(crate) bvh_edges: Bvh,
    pub(crate) bvh_faces: Bvh,
    scale: f64,
}

impl Scene {
    /// Builds the scene of one surface model from owned data. Call it on a worker thread (the
    /// model is moved in, the scene is `Send`) and hand the scene to the UI thread.
    pub fn from_surface_model(m: SurfaceModel, model: u32) -> Scene {
        Scene::from_surface_model_with(m, model, false)
    }

    /// Like [`Scene::from_surface_model`]; with `show_hidden` the hidden edges and faces are in it.
    pub fn from_surface_model_with(m: SurfaceModel, model: u32, show_hidden: bool) -> Scene {
        let mut b = SceneBuilder::new();
        b.add_surface_model_placed_with(&m, &m.transform, model, show_hidden);
        b.finish()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
            && self.edges.is_empty()
            && self.faces.is_empty()
            && self.lines.is_empty()
    }

    /// The number of guide lines without ends.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The largest coordinate of the points, edges and faces: how large the scene is.
    pub fn extent(&self) -> f64 {
        self.scale
    }

    /// The tolerance for "the same point" in this scene, in world units.
    pub(crate) fn eps(&self) -> f64 {
        world_eps(self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::surface::Edit;

    #[test]
    fn a_guide_gives_a_point_or_a_long_edge() {
        use kadakas_core::guide::Guide;
        let mut b = SceneBuilder::new();
        b.add_guide(&Guide::point(DVec3::new(1.0, 2.0, 3.0)), 10.0, 0);
        b.add_guide(&Guide::line(DVec3::ZERO, DVec3::new(0.0, 5.0, 0.0)), 0.0, 1);
        b.add_guide(&Guide::segment(DVec3::ZERO, DVec3::X, 100.0), 5.0, 2);
        // A direction without length and a point that is not finite give nothing.
        b.add_guide(&Guide::line(DVec3::ZERO, DVec3::ZERO), 0.0, 3);
        b.add_guide(&Guide::point(DVec3::NAN), 0.0, 4);
        let s = b.finish();
        // The line without ends is in the short list, not in the index.
        assert_eq!((s.point_count(), s.edge_count(), s.line_count()), (1, 1, 1));
        assert_eq!(
            s.points[0].p,
            DVec3::new(1.0, 2.0, 13.0),
            "the layer elevation is added"
        );
        assert_eq!(s.points[0].kind, PointKind::Guide);
        assert!(s.points[0].tag == Tag::guide(0) && s.points[0].tag.kind() == TagKind::Guide);
        let long = s
            .lines
            .iter()
            .find(|l| l.tag == Tag::guide(1))
            .unwrap()
            .as_edge();
        assert_eq!(long.a, DVec3::new(0.0, -GUIDE_REACH, 0.0));
        assert_eq!(long.b, DVec3::new(0.0, GUIDE_REACH, 0.0));
        assert!(!s.is_empty());
        let short = s.edges.iter().find(|e| e.tag == Tag::guide(2)).unwrap();
        assert_eq!(
            (short.a, short.b),
            (DVec3::new(0.0, 0.0, 5.0), DVec3::new(100.0, 0.0, 5.0))
        );
        assert_eq!(Tag::guide(7).id(), 7);
        assert_eq!(Tag::guide(7).shifted(5), Tag::guide(7), "no model to shift");
    }

    #[test]
    fn the_reach_of_a_line_follows_the_size_of_the_scene() {
        use kadakas_core::guide::Guide;
        assert_eq!(reach_for(0.0), GUIDE_REACH);
        assert_eq!(reach_for(1.0e5), GUIDE_REACH, "a small site: 2 km");
        assert_eq!(reach_for(5.0e6), 5.0e7, "a large site: ten times its size");
        let mut b = SceneBuilder::new();
        b.set_reach(5.0e7).set_reach(f64::NAN).set_reach(-1.0);
        b.add_guide(&Guide::line(DVec3::ZERO, DVec3::X), 0.0, 0);
        let s = b.finish();
        assert_eq!(s.lines[0].reach, 5.0e7, "a bad reach is ignored");
        assert_eq!(
            s.extent(),
            0.0,
            "a line without ends does not make the scene large"
        );
    }

    #[test]
    fn tags_round_trip() {
        let t = Tag::edge(7, EdgeId(42));
        assert_eq!((t.kind(), t.model(), t.id()), (TagKind::Edge, 7, 42));
        let t = Tag::face(3, FaceId(9));
        assert_eq!((t.kind(), t.model(), t.id()), (TagKind::Face, 3, 9));
        let t = Tag::curve(1, CurveId(5));
        assert_eq!((t.kind(), t.model(), t.id()), (TagKind::Curve, 1, 5));
        assert_eq!(Tag::NONE.kind(), TagKind::Other);
        assert_eq!(Tag(3 << 62 | 5).kind(), TagKind::Guide);
    }

    #[test]
    fn bad_input_is_left_out() {
        let mut b = SceneBuilder::new();
        b.add_point(DVec3::NAN, PointKind::Endpoint, Tag::NONE);
        b.add_edge(DVec3::ZERO, DVec3::ZERO, Tag::NONE);
        b.add_edge(DVec3::ZERO, DVec3::INFINITY, Tag::NONE);
        b.add_face(vec![DVec3::ZERO, DVec3::X], vec![], Tag::NONE);
        b.add_face(
            vec![DVec3::ZERO, DVec3::X, DVec3::X * 2.0],
            vec![],
            Tag::NONE,
        );
        b.add_face(vec![DVec3::ZERO, DVec3::X, DVec3::NAN], vec![], Tag::NONE);
        let s = b.finish();
        assert!(s.is_empty());
        assert_eq!((s.edge_count(), s.face_count(), s.point_count()), (0, 0, 0));
    }

    #[test]
    fn only_a_polygon_face_has_a_center() {
        let mut b = SceneBuilder::new();
        let sq = |w: f64, h: f64| {
            vec![
                DVec3::ZERO,
                DVec3::new(w, 0.0, 0.0),
                DVec3::new(w, h, 0.0),
                DVec3::new(0.0, h, 0.0),
            ]
        };
        // A plain face (a square from the rectangle tool) has no center.
        b.add_face(sq(10.0, 10.0), vec![], Tag(3));
        b.add_polygon_face(sq(10.0, 10.0), vec![], Tag(1));
        b.add_polygon_face(sq(10.0, 20.0), vec![], Tag(2));
        let s = b.finish();
        assert_eq!(s.face_count(), 3);
        assert_eq!(s.point_count(), 1);
        assert!(s.points[0].p.distance(DVec3::new(5.0, 5.0, 0.0)) < 1e-9);
        assert_eq!(s.points[0].kind, PointKind::Center);
        assert!(s.eps() > 0.0);
    }

    #[test]
    fn a_surface_model_gives_edges_faces_and_curve_centers() {
        let mut m = SurfaceModel::new();
        m.transform = glam::DAffine3::from_translation(DVec3::new(100.0, 0.0, 0.0));
        // A hexagon-like circle of 12 sides: a closed curve.
        let n = 12;
        let pts: Vec<[f64; 3]> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                [500.0 * a.cos(), 500.0 * a.sin(), 0.0]
            })
            .collect();
        m.apply(&Edit::AddEdges {
            edges: (0..n).map(|i| [pts[i], pts[(i + 1) % n]]).collect(),
        })
        .unwrap();
        let ids: Vec<EdgeId> = m.edges().map(|(id, _)| id).collect();
        m.add_curve(ids).unwrap();
        let mut b = SceneBuilder::new();
        b.add_surface_model(&m, 4);
        let s = b.finish();
        assert_eq!(s.edge_count(), 12);
        assert_eq!(s.face_count(), 1);
        // The curve center only: the face adds none.
        assert_eq!(s.point_count(), 1);
        for p in &s.points {
            assert!(p.p.distance(DVec3::new(100.0, 0.0, 0.0)) < 1e-6);
        }
        assert!(
            s.points
                .iter()
                .any(|p| p.tag.kind() == TagKind::Curve && p.tag.model() == 4)
        );
        assert!(s.edges.iter().all(|e| e.tag.kind() == TagKind::Edge));
    }

    #[test]
    fn a_hidden_edge_and_an_open_polyline_curve() {
        let mut m = SurfaceModel::new();
        m.apply(&Edit::AddEdges {
            edges: vec![
                [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
                [[100.0, 0.0, 0.0], [100.0, 100.0, 0.0]],
                [[100.0, 100.0, 0.0], [300.0, 100.0, 0.0]],
            ],
        })
        .unwrap();
        let ids: Vec<EdgeId> = m.edges().map(|(id, _)| id).collect();
        m.add_curve(ids.clone()).unwrap();
        let mut b = SceneBuilder::new();
        b.add_surface_model(&m, 0);
        let s = b.finish();
        // Not a circle: no center.
        assert_eq!(s.point_count(), 0);
        assert_eq!(s.edge_count(), 3);
        assert!(curve_center(&m, &[]).is_none());
        assert!(curve_center(&m, &ids[..1]).is_none());
        assert!(curve_center(&m, &[EdgeId(999)]).is_none());
    }

    #[test]
    fn an_open_arc_has_a_center() {
        let mut m = SurfaceModel::new();
        let pts: Vec<[f64; 3]> = (0..=6)
            .map(|i| {
                let a = 0.5 * std::f64::consts::PI * i as f64 / 6.0;
                [300.0 * a.cos(), 300.0 * a.sin(), 50.0]
            })
            .collect();
        m.apply(&Edit::AddEdges {
            edges: (0..6).map(|i| [pts[i], pts[i + 1]]).collect(),
        })
        .unwrap();
        let mut ids: Vec<(EdgeId, kadakas_core::surface::Edge)> = m.edges().collect();
        // Give the chain in a shuffled start: the walker follows the shared ends.
        ids.sort_by_key(|(id, _)| id.0);
        let chain: Vec<EdgeId> = ids.iter().map(|(id, _)| *id).collect();
        let c = curve_center(&m, &chain).unwrap();
        assert!(c.distance(DVec3::new(0.0, 0.0, 50.0)) < 1e-6);
        // Edges that do not form a chain give nothing.
        let broken = [chain[0], chain[2]];
        assert!(curve_center(&m, &broken).is_none());
    }

    #[test]
    fn hidden_edges_and_faces_are_skipped() {
        let mut m = SurfaceModel::new();
        m.apply(&Edit::AddEdges {
            edges: vec![
                [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
                [[100.0, 0.0, 0.0], [100.0, 100.0, 0.0]],
                [[100.0, 100.0, 0.0], [0.0, 100.0, 0.0]],
                [[0.0, 100.0, 0.0], [0.0, 0.0, 0.0]],
            ],
        })
        .unwrap();
        let count = |m: &SurfaceModel| {
            let mut b = SceneBuilder::new();
            b.add_surface_model(m, 0);
            let s = b.finish();
            (s.edge_count(), s.face_count())
        };
        assert_eq!(count(&m), (4, 1));
        let mut v = serde_json::to_value(&m).unwrap();
        v["edges"][0]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"hidden": true}));
        v["faces"][0]["attrs"] = serde_json::json!({"hidden": true});
        let hidden: SurfaceModel = serde_json::from_value(v).unwrap();
        assert_eq!(count(&hidden), (3, 0));
    }
}
