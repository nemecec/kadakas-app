//! The edits of a surface model. Each edit works on a copy of the model (see
//! `SurfaceModel::apply`), so an error leaves the model as it was.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use super::arrange::{self, Arrangement, Region, canonical, inside, reversed};
use super::geom::{self, Plane, on_plane};
use super::index::BoxTree;
use super::sync;
use super::topo::Topo;
use super::{
    Edge, EdgeId, Face, FaceId, LENGTH_TOLERANCE, MAX_EDGES, MAX_EDIT_EDGES, MAX_FACES,
    MAX_LOOP_VERTICES, MAX_VERTICES, SurfaceError, SurfaceModel, VertexId, key,
};
use crate::model::MAX_COORD;

/// One edit of a surface model. In a command: `{"op": "edit_surface", "id": ..., "edit":
/// {"action": "add_edges", "edges": [[[0, 0, 0], [1000, 0, 0]]]}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Edit {
    /// Draws straight edges, each `[[x, y, z], [x, y, z]]` in the local coordinates of the model
    /// (mm). Ends weld to vertices and edges, crossings split, and faces are made and split.
    AddEdges { edges: Vec<[[f64; 3]; 2]> },
    /// Erases edges: coplanar faces on both sides merge, other faces of the edges go.
    EraseEdges { edges: Vec<EdgeId> },
    /// Erases faces only; their edges stay.
    EraseFaces { faces: Vec<FaceId> },
    /// Turns faces over: front and back change places.
    ReverseFaces { faces: Vec<FaceId> },
    /// Paint (MOD-05): puts a material on the front, the back or both sides of faces, or clears
    /// it (`material` null). `material` is the id of a material resource of the document (its
    /// text form); it is stored in the attribute `material` (front) or `back_material` (back) of
    /// the face. Faces turned over swap the two.
    SetFaceMaterial {
        faces: Vec<FaceId>,
        side: PaintSide,
        #[serde(default)]
        material: Option<String>,
    },
    /// Draws a chain of straight edges through the points (a closed one joins the last point to
    /// the first) and keeps them as one curve (a circle or an arc, MOD-08), so that the pieces
    /// stay one curve when other edges cut them. Welds, splits and faces work as in `AddEdges`.
    /// When one of the pieces is in a curve already, the edges are drawn and no new curve is made.
    /// With `faceted` the curve is a polygon: its edges get the mark `faceted`, and the edges
    /// that Push/Pull, Follow Me and Offset make between its sides stay creases (drawn, hard)
    /// instead of soft and smooth.
    AddCurve {
        points: Vec<[f64; 3]>,
        closed: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        faceted: bool,
    },
    /// Pushes or pulls a face along its normal by `distance` mm (positive is toward the front,
    /// negative toward the back). See the module `pushpull` for the cases. The face keeps its id
    /// and ends as the cap.
    PushPull { face: FaceId, distance: f64 },
    /// Intersect Faces (MOD-08): makes edges where faces in other planes cross, so that both
    /// faces are split there, and splits edges where they pass through a face. `model`: all faces
    /// and edges of the model. `selection`: where the given `faces` cross anything and where the
    /// given `edges` pass through a face. Faces in one plane that overlap are not handled (they
    /// are a problem of the model). See the module `intersect`.
    IntersectFaces {
        scope: IntersectScope,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        faces: Vec<FaceId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        edges: Vec<EdgeId>,
    },
    /// Move, Rotate and Scale of geometry (MOD-05): moves the given `vertices`, all vertices of the
    /// given `faces` and the ends of the given `edges` by an affine map. `matrix` has 12 numbers,
    /// the three columns of the 3x3 part and then the translation (the layout of
    /// `glam::DAffine3::to_cols_array`), in the local coordinates of the model. Every face that
    /// has a moved vertex is refitted to a plane; a face that is no longer flat is folded into
    /// triangles along new soft and smooth edges. See the module `transform`.
    TransformVertices {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        vertices: Vec<VertexId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        faces: Vec<FaceId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        edges: Vec<EdgeId>,
        matrix: [f64; 12],
    },
    /// Copy of faces and edges in the same model (Move with Ctrl, `5x`): `count` copies, the
    /// first placed by `matrix`, the next by the matrix applied twice, three times and so on
    /// (a row of copies at the same step; a rotation turns each copy further). The copies have new
    /// ids and weld to the geometry they land on. See the module `transform`.
    CopyGeometry {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        faces: Vec<FaceId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        edges: Vec<EdgeId>,
        matrix: [f64; 12],
        count: u32,
    },
    /// Hide and Unhide (MOD-05): sets (`hidden: true`) or clears the attribute `hidden` of the
    /// given faces and edges. Views, picking and the inference leave hidden items out unless
    /// Hidden Geometry shows them. Nothing else changes: a hidden edge does not merge faces.
    SetHidden {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        faces: Vec<FaceId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        edges: Vec<EdgeId>,
        hidden: bool,
    },
    /// Offset (MOD-08): draws a copy of every loop of the face `distance` mm inside the face
    /// (`outward` false, the default) or outside it, in the plane of the face, with miter
    /// corners. The face splits along the new loops; a curve gives a curve. `distance` must be
    /// larger than the length tolerance. See the module `offset`.
    OffsetFace {
        face: FaceId,
        distance: f64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        outward: bool,
    },
    /// Offset of edges (MOD-08): a chain of edges in one plane (open, or closed) drawn again
    /// `distance` mm to the side of the point `toward` (local coordinates), with miter corners;
    /// the ends of an open chain move at a right angle. A curve gives a curve.
    OffsetEdges {
        edges: Vec<EdgeId>,
        distance: f64,
        toward: [f64; 3],
    },
    /// Soft and smooth edges (MOD-08): sets (`true`) or clears (`false`) the flags of the edges;
    /// a flag that is not given stays as it is. A soft edge is not drawn, picked or inferred; the
    /// faces on the sides of a smooth edge shade as one surface. Hidden is `set_hidden`. See the
    /// module `style`.
    SetEdgeStyle {
        edges: Vec<EdgeId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        soft: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        smooth: Option<bool>,
    },
    /// Soften Edges (MOD-08): each of the edges between two faces at an angle below `angle`
    /// degrees (0 to 180) becomes soft (and smooth with `smooth`); the others lose both flags.
    /// Edges between coplanar faces are softened only with `coplanar`.
    SoftenEdges {
        edges: Vec<EdgeId>,
        angle: f64,
        #[serde(default = "yes")]
        smooth: bool,
        #[serde(default)]
        coplanar: bool,
    },
    /// Follow Me (MOD-08): sweeps the face `face` (the profile) along the chain of edges `path`
    /// (open: a solid with the profile and a new face as caps; closed: a ring without caps). The
    /// new faces get the paint of the profile; a curve of the path or of the profile gives soft
    /// and smooth edges. See the module `followme`.
    FollowMe { face: FaceId, path: Vec<EdgeId> },
    /// Solid tools (MOD-09): a boolean operation of two closed solids of the model. `first` and
    /// `second` are the faces of the two solids (see `SurfaceModel::solid_of`). A face that both
    /// solids use (two boxes through each other in one plane) may be in both lists. The result
    /// replaces the two solids. Subtract and Trim take `second` from `first`. Trim keeps `second`,
    /// split along the cut and stuck to `first`. Split makes three solids. An input that is not a
    /// closed solid, a limit, or a result that is not a closed solid refuses the edit with a
    /// reason and changes nothing. See the module `solid`.
    Solid {
        op: super::SolidOp,
        first: Vec<FaceId>,
        second: Vec<FaceId>,
    },
}

fn yes() -> bool {
    true
}

/// Which side of a face a material goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaintSide {
    Front,
    Back,
    Both,
}

/// The attribute of a face that holds the material of its front.
pub const ATTR_MATERIAL: &str = "material";
/// The attribute of a face that holds the material of its back.
pub const ATTR_BACK_MATERIAL: &str = "back_material";

/// What Intersect Faces looks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntersectScope {
    /// All faces and edges of the model.
    Model,
    /// The given faces and edges against the whole model.
    Selection,
}

mod followme;
mod hide;
mod intersect;
mod near;
pub(super) mod offset;
mod pushpull;
pub(super) mod reference;
pub(super) mod style;
mod transform;

use near::{Near, Points, Runs, Walks};

/// Runs `edit` on `m`, a copy of `before` (the model as it was, which Push/Pull compares with).
/// Returns the notes of the edit (see `merge_attrs`).
pub(super) fn run(
    m: &mut SurfaceModel,
    edit: &Edit,
    before: &SurfaceModel,
) -> Result<Vec<String>, SurfaceError> {
    let mut notes = Vec::new();
    run_noted(m, edit, before, &mut notes)?;
    Ok(notes)
}

fn run_noted(
    m: &mut SurfaceModel,
    edit: &Edit,
    before: &SurfaceModel,
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    match edit {
        Edit::AddEdges { edges } => add_edges(m, edges),
        Edit::EraseEdges { edges } => erase_edges(m, edges, notes),
        Edit::AddCurve {
            points,
            closed,
            faceted,
        } => add_curve(m, points, *closed, *faceted),
        Edit::PushPull { face, distance } => {
            pushpull::push_pull(m, before, *face, *distance, notes)
        }
        Edit::IntersectFaces {
            scope,
            faces,
            edges,
        } => intersect::intersect_faces(m, *scope, faces, edges, notes),
        Edit::TransformVertices {
            vertices,
            faces,
            edges,
            matrix,
        } => transform::transform_vertices(m, vertices, faces, edges, matrix, before, notes),
        Edit::CopyGeometry {
            faces,
            edges,
            matrix,
            count,
        } => transform::copy_geometry(m, faces, edges, matrix, *count),
        Edit::EraseFaces { faces } => {
            let ids = existing_faces(m, faces)?;
            let mut w = Work::new(m);
            for id in ids {
                w.take_face(id);
            }
            Ok(())
        }
        Edit::ReverseFaces { faces } => reverse_faces(m, faces),
        Edit::OffsetFace {
            face,
            distance,
            outward,
        } => offset::offset_face(m, *face, *distance, *outward),
        Edit::OffsetEdges {
            edges,
            distance,
            toward,
        } => offset::offset_edges(m, edges, *distance, DVec3::from(*toward)),
        Edit::FollowMe { face, path } => followme::follow_me(m, before, *face, path, notes),
        Edit::Solid { op, first, second } => super::solid::solid_edit(m, *op, first, second, notes),
        Edit::SetEdgeStyle {
            edges,
            soft,
            smooth,
        } => style::set_edge_style(m, edges, *soft, *smooth),
        Edit::SoftenEdges {
            edges,
            angle,
            smooth,
            coplanar,
        } => style::soften_edges(m, edges, *angle, *smooth, *coplanar),
        Edit::SetHidden {
            faces,
            edges,
            hidden,
        } => hide::set_hidden(m, faces, edges, *hidden),
        Edit::SetFaceMaterial {
            faces,
            side,
            material,
        } => set_face_material(m, faces, *side, material.as_deref()),
    }
}

fn set_face_material(
    m: &mut SurfaceModel,
    faces: &[FaceId],
    side: PaintSide,
    material: Option<&str>,
) -> Result<(), SurfaceError> {
    if material.is_some_and(|t| t.trim().is_empty()) {
        return Err(SurfaceError::Unsolved("the material name is empty".into()));
    }
    for id in existing_faces(m, faces)? {
        let f = m.faces.get_mut(&id).expect("checked");
        let keys: &[&str] = match side {
            PaintSide::Front => &[ATTR_MATERIAL],
            PaintSide::Back => &[ATTR_BACK_MATERIAL],
            PaintSide::Both => &[ATTR_MATERIAL, ATTR_BACK_MATERIAL],
        };
        for key in keys {
            match material {
                Some(t) => {
                    f.attrs.insert((*key).into(), serde_json::Value::from(t));
                }
                None => {
                    f.attrs.remove(*key);
                }
            }
        }
    }
    Ok(())
}

/// A face is turned over: the paint stays on the side it was on, so the material of the front
/// becomes the material of the back and the other way round.
pub(super) fn swap_paint(attrs: &mut super::Attrs) {
    let front = attrs.remove(ATTR_MATERIAL);
    let back = attrs.remove(ATTR_BACK_MATERIAL);
    if let Some(v) = back {
        attrs.insert(ATTR_MATERIAL.into(), v);
    }
    if let Some(v) = front {
        attrs.insert(ATTR_BACK_MATERIAL.into(), v);
    }
}

fn reverse_faces(m: &mut SurfaceModel, faces: &[FaceId]) -> Result<(), SurfaceError> {
    for id in existing_faces(m, faces)? {
        let f = m.faces.get_mut(&id).expect("checked");
        f.plane = f.plane.flipped();
        f.outer = reversed(&f.outer);
        let mut holes: Vec<Vec<VertexId>> = f.holes.iter().map(|h| reversed(h)).collect();
        holes.sort();
        f.holes = holes;
        swap_paint(&mut f.attrs);
    }
    Ok(())
}

fn existing_faces(m: &SurfaceModel, ids: &[FaceId]) -> Result<BTreeSet<FaceId>, SurfaceError> {
    let ids: BTreeSet<FaceId> = ids.iter().copied().collect();
    match ids.iter().find(|id| !m.faces.contains_key(id)) {
        Some(id) => Err(SurfaceError::NoFace(*id)),
        None => Ok(ids),
    }
}

/// Refuses a model over one of the limits.
pub(super) fn check_limits(m: &SurfaceModel) -> Result<(), SurfaceError> {
    let loops: usize = m.faces.values().map(Face::loop_len).sum();
    for (what, max, count) in [
        ("vertices", MAX_VERTICES, m.vertices.len()),
        ("edges", MAX_EDGES, m.edges.len()),
        ("faces", MAX_FACES, m.faces.len()),
        ("vertex uses in face loops", MAX_LOOP_VERTICES, loops),
    ] {
        if count > max * super::limit_scale() {
            return Err(SurfaceError::Limit { what, max, count });
        }
    }
    Ok(())
}

/// The model and its index, changed together.
struct Work<'a> {
    m: &'a mut SurfaceModel,
    t: Topo,
    /// The spatial index, for the edits that add edges (see [`near`]). Without it the same tests
    /// scan all vertices, edges and faces (the reference that the index is tested against).
    near: Option<Near>,
}

/// Where a point snaps to: a vertex, or a point inside an edge (that edge is split there).
enum Snap {
    Vertex(VertexId),
    Split(EdgeId, DVec3),
}

/// A point where a new edge must be split.
enum Hit {
    Vertex(VertexId),
    /// An edge that the new edge crosses, and the crossing point on that edge.
    Cross(EdgeId, DVec3),
}

impl<'a> Work<'a> {
    fn new(m: &'a mut SurfaceModel) -> Self {
        let t = Topo::of(m);
        Work { m, t, near: None }
    }

    /// A work with the spatial index, for the edits that add or erase edges (unless a test asks for
    /// the scans, see [`reference`]).
    fn indexed(m: &'a mut SurfaceModel) -> Self {
        let mut w = Work::new(m);
        if reference::mode() == reference::Mode::Index {
            w.near = Some(Near::of(w.m));
        }
        w
    }

    /// Refuses a model over one of the limits ([`check_limits`], with the loop count of the index).
    fn check_limits(&self) -> Result<(), SurfaceError> {
        let Some(n) = &self.near else {
            return check_limits(self.m);
        };
        let m = &self.m;
        for (what, max, count) in [
            ("vertices", MAX_VERTICES, m.vertices.len()),
            ("edges", MAX_EDGES, m.edges.len()),
            ("faces", MAX_FACES, m.faces.len()),
            ("vertex uses in face loops", MAX_LOOP_VERTICES, n.loops),
        ] {
            if count > max * super::limit_scale() {
                return Err(SurfaceError::Limit { what, max, count });
            }
        }
        Ok(())
    }

    fn pos(&self, v: VertexId) -> DVec3 {
        self.m.vertices[&v]
    }

    fn new_vertex(&mut self, p: DVec3) -> VertexId {
        let id = VertexId(self.m.alloc());
        self.m.vertices.insert(id, p);
        if let Some(n) = &mut self.near {
            n.add_vertex(id, p);
        }
        id
    }

    fn new_edge(&mut self, a: VertexId, b: VertexId) -> EdgeId {
        let id = EdgeId(self.m.alloc());
        let e = Edge { v: [a, b] };
        self.m.edges.insert(id, e);
        self.t.add_edge(id, e);
        if let Some(n) = &mut self.near {
            n.add_edge(id, self.m.vertices[&a], self.m.vertices[&b]);
        }
        id
    }

    fn put_face(&mut self, id: FaceId, f: Face) {
        if let Some(old) = self.m.faces.remove(&id) {
            self.t.remove_face(id, &old);
            if let Some(n) = &mut self.near {
                n.loops -= old.loop_len();
            }
        }
        self.t.add_face(id, &f);
        if let Some(n) = &mut self.near {
            n.loops += f.loop_len();
            n.add_face(id, &f, |v| self.m.vertices[&v]);
        }
        self.m.faces.insert(id, f);
    }

    fn new_face(&mut self, f: Face) -> FaceId {
        let id = FaceId(self.m.alloc());
        self.put_face(id, f);
        id
    }

    fn take_face(&mut self, id: FaceId) -> Option<Face> {
        let f = self.m.faces.remove(&id)?;
        self.t.remove_face(id, &f);
        if let Some(n) = &mut self.near {
            n.loops -= f.loop_len();
        }
        Some(f)
    }

    /// Removes an edge with its attributes and its place in a curve (an empty curve goes).
    fn remove_edge(&mut self, id: EdgeId) {
        if let Some(e) = self.m.edges.remove(&id) {
            self.t.remove_edge(id, e);
        }
        self.m.edge_attrs.remove(&id);
        self.m.curves.retain(|_, es| {
            es.retain(|e| *e != id);
            !es.is_empty()
        });
    }

    /// Splits an edge at `p` (a point on it). The edge keeps its id for the part at its first
    /// vertex; the faces that use it get the new vertex in their loops.
    fn split_edge(
        &mut self,
        e: EdgeId,
        p: DVec3,
        touched: &mut BTreeSet<(VertexId, VertexId)>,
    ) -> VertexId {
        let w = self.new_vertex(p);
        self.split_edge_at(e, w, touched);
        w
    }

    /// Splits an edge at the vertex `w`, which lies on it (within the tolerance): the edge keeps
    /// its id for the part at its first vertex; the faces that use it get `w` in their loops.
    fn split_edge_at(
        &mut self,
        e: EdgeId,
        w: VertexId,
        touched: &mut BTreeSet<(VertexId, VertexId)>,
    ) {
        let old = self.m.edges[&e];
        let [c, d] = old.v;
        let faces: Vec<FaceId> = self.t.faces_of(e).collect();
        for f in &faces {
            let face = self.m.faces[f].clone();
            self.t.remove_face(*f, &face);
        }
        self.t.remove_edge(e, old);
        let first = Edge { v: [c, w] };
        self.m.edges.insert(e, first);
        self.t.add_edge(e, first);
        if let Some(n) = &mut self.near {
            n.add_edge(e, self.m.vertices[&c], self.m.vertices[&w]);
        }
        let second = self.new_edge(w, d);
        // The new part keeps the attributes and takes its place in the curve, next to the part
        // that it touches in the chain.
        if let Some(a) = self.m.edge_attrs.get(&e).cloned() {
            self.m.edge_attrs.insert(second, a);
        }
        let edges = &self.m.edges;
        for es in self.m.curves.values_mut() {
            if let Some(i) = es.iter().position(|x| *x == e) {
                // `e` is now c–w and `second` w–d: `second` goes on the side of the chain that
                // continues at d.
                let prev_at_d = i > 0 && edges[&es[i - 1]].v.contains(&d);
                let next_at_c = es.get(i + 1).is_some_and(|n| edges[n].v.contains(&c));
                es.insert(if prev_at_d || next_at_c { i } else { i + 1 }, second);
            }
        }
        for f in &faces {
            let face = self.m.faces.get_mut(f).expect("a face of the edge");
            for l in std::iter::once(&mut face.outer).chain(face.holes.iter_mut()) {
                let n = l.len();
                if let Some(i) = (0..n).find(|&i| key(l[i], l[(i + 1) % n]) == key(c, d)) {
                    l.insert(i + 1, w);
                    if let Some(x) = &mut self.near {
                        x.loops += 1;
                    }
                }
            }
            let face = self.m.faces[f].clone();
            self.t.add_face(*f, &face);
            if let Some(x) = &mut self.near {
                x.add_face(*f, &face, |v| self.m.vertices[&v]);
            }
        }
        touched.insert(key(c, w));
        touched.insert(key(w, d));
    }

    /// The vertex for an end point: the nearest vertex within the tolerance, else a split of the
    /// nearest edge within the tolerance, else a new vertex.
    fn snap(&mut self, a: DVec3, touched: &mut BTreeSet<(VertexId, VertexId)>) -> VertexId {
        match self.snap_existing(a, touched) {
            Some(v) => v,
            None => self.new_vertex(a),
        }
    }

    /// The vertex for a point on the geometry: the nearest vertex within the tolerance, else a
    /// split of the nearest edge within the tolerance, else `None`.
    fn snap_existing(
        &mut self,
        a: DVec3,
        touched: &mut BTreeSet<(VertexId, VertexId)>,
    ) -> Option<VertexId> {
        match self.snap_target(a)? {
            Snap::Vertex(v) => Some(v),
            Snap::Split(e, x) => Some(self.split_edge(e, x, touched)),
        }
    }

    /// What [`Work::snap_existing`] does for the point, without a change: the vertex it takes, or
    /// the edge it splits and where.
    fn snap_target(&self, a: DVec3) -> Option<Snap> {
        // The index finds the vertices and edges within the largest tolerance (each tolerance below
        // is at most that; twice for rounding).
        let (lo, hi) = match &self.near {
            Some(n) => {
                let pad = DVec3::splat(2.0 * n.tol(geom::size(a)));
                (a - pad, a + pad)
            }
            None => (DVec3::NEG_INFINITY, DVec3::INFINITY),
        };
        let mut best: Option<(f64, VertexId)> = None;
        for id in self.vertices_near(lo, hi) {
            let x = self.pos(id);
            let d = a.distance(x);
            if d <= geom::tol(&[a, x]) && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, id));
            }
        }
        if let Some((_, v)) = best {
            return Some(Snap::Vertex(v));
        }
        let mut near: Option<(f64, EdgeId, DVec3)> = None;
        for id in self.edges_near(lo, hi) {
            let Some(e) = self.m.edges.get(&id) else {
                continue;
            };
            let (c, d) = (self.pos(e.v[0]), self.pos(e.v[1]));
            let (dist, s) = geom::point_segment(a, c, d);
            if dist <= geom::tol(&[a, c, d]) && near.is_none_or(|(bd, _, _)| dist < bd) {
                near = Some((dist, id, c + (d - c) * s));
            }
        }
        let (_, e, x) = near?;
        let [c, d] = self.m.edges[&e].v;
        for end in [c, d] {
            let pe = self.pos(end);
            if x.distance(pe) <= geom::tol(&[x, pe]) {
                return Some(Snap::Vertex(end));
            }
        }
        Some(Snap::Split(e, x))
    }

    /// True if [`Work::insert_segment`] of `a`–`b` would work. Nothing changes. It may refuse a
    /// segment that would work (an end that splits an edge it nearly runs along), never the other
    /// way round: the points it tests hold all the points that the insert tests.
    fn segment_fits(&self, a: DVec3, b: DVec3) -> bool {
        if a.distance(b) <= geom::tol(&[a, b]) {
            return false;
        }
        let end = |x: DVec3| match self.snap_target(x) {
            Some(Snap::Vertex(v)) => (self.pos(v), Some(v)),
            Some(Snap::Split(_, y)) => (y, None),
            None => (x, None),
        };
        let ((p, va), (q, vb)) = (end(a), end(b));
        if (va.is_some() && va == vb) || p.distance(q) <= geom::tol(&[p, q]) {
            return false;
        }
        let ends: Vec<VertexId> = [va, vb].into_iter().flatten().collect();
        let hits = self.hits_on(p, q, &ends);
        self.pieces_fit(p, q, &hits).is_ok()
    }

    /// The vertices and the crossings of edges inside the segment `p`–`q`, in order along it. The
    /// vertices `ends` and the edges at them are left out.
    fn hits_on(&self, p: DVec3, q: DVec3, ends: &[VertexId]) -> Vec<(f64, Hit)> {
        let t = geom::tol(&[p, q]);
        let len = p.distance(q);
        let (lo, hi) = (p.min(q) - DVec3::splat(t), p.max(q) + DVec3::splat(t));
        let mut hits: Vec<(f64, Hit)> = Vec::new();
        let mut on_line: BTreeSet<VertexId> = ends.iter().copied().collect();
        for id in self.vertices_near(lo, hi) {
            let x = self.pos(id);
            if ends.contains(&id) || x.cmplt(lo).any() || x.cmpgt(hi).any() {
                continue;
            }
            let tt = geom::tol(&[p, q, x]);
            let (d, s) = geom::point_segment(x, p, q);
            if d <= tt && s * len > tt && (1.0 - s) * len > tt {
                hits.push((s, Hit::Vertex(id)));
                on_line.insert(id);
            }
        }
        for id in self.edges_near(lo, hi) {
            let Some(e) = self.m.edges.get(&id) else {
                continue;
            };
            if e.v.iter().any(|v| on_line.contains(v)) {
                continue;
            }
            let (c, d) = (self.pos(e.v[0]), self.pos(e.v[1]));
            if c.max(d).cmplt(lo).any() || c.min(d).cmpgt(hi).any() {
                continue;
            }
            let Some((s, u, dist)) = geom::segment_segment(p, q, c, d) else {
                continue;
            };
            let tt = geom::tol(&[p, q, c, d]);
            let el = c.distance(d);
            if dist <= tt
                && s * len > tt
                && (1.0 - s) * len > tt
                && u * el > tt
                && (1.0 - u) * el > tt
            {
                hits.push((s, Hit::Cross(id, c + (d - c) * u)));
            }
        }
        hits.sort_by(|x, y| x.0.total_cmp(&y.0));
        hits
    }

    /// Refuses pieces of `p`–`q` between the hits that are not longer than the tolerance.
    fn pieces_fit(&self, p: DVec3, q: DVec3, hits: &[(f64, Hit)]) -> Result<(), SurfaceError> {
        let mut points = vec![p];
        for (_, h) in hits {
            points.push(match h {
                Hit::Vertex(v) => self.pos(*v),
                Hit::Cross(_, x) => *x,
            });
        }
        points.push(q);
        for w in points.windows(2) {
            let tt = geom::tol(w);
            if w[0].distance(w[1]) <= tt {
                return Err(SurfaceError::TooClose { tol: tt });
            }
        }
        Ok(())
    }

    /// Adds the edge `a`–`b`: welds its ends, splits it at vertices and crossings, and records the
    /// pieces (in the drawing direction) in `drawn`.
    fn insert_segment(
        &mut self,
        a: DVec3,
        b: DVec3,
        drawn: &mut Vec<(VertexId, VertexId)>,
        touched: &mut BTreeSet<(VertexId, VertexId)>,
    ) -> Result<(), SurfaceError> {
        let t = geom::tol(&[a, b]);
        if a.distance(b) <= t {
            return Err(SurfaceError::TooShort { tol: t });
        }
        let va = self.snap(a, touched);
        let vb = self.snap(b, touched);
        let (p, q) = (self.pos(va), self.pos(vb));
        let t = geom::tol(&[p, q]);
        if va == vb || p.distance(q) <= t {
            return Err(SurfaceError::TooShort { tol: t });
        }
        let hits = self.hits_on(p, q, &[va, vb]);
        // The pieces must not be shorter than the tolerance.
        self.pieces_fit(p, q, &hits)?;
        let mut chain = vec![va];
        for (_, h) in hits {
            chain.push(match h {
                Hit::Vertex(v) => v,
                Hit::Cross(e, x) => self.split_edge(e, x, touched),
            });
        }
        chain.push(vb);
        for w in chain.windows(2) {
            if self.t.edge(w[0], w[1]).is_none() {
                self.new_edge(w[0], w[1]);
            }
            touched.insert(key(w[0], w[1]));
            drawn.push((w[0], w[1]));
        }
        Ok(())
    }

    /// The vertices that may lie in the box `lo`–`hi`, in id order: from the index, or all.
    fn vertices_near(&self, lo: DVec3, hi: DVec3) -> Vec<VertexId> {
        match &self.near {
            Some(n) => n
                .vertices_in(self.m, lo, hi)
                .into_iter()
                .filter(|v| self.m.vertices.contains_key(v))
                .collect(),
            None => self.m.vertices.keys().copied().collect(),
        }
    }

    /// The edges that may meet the box `lo`–`hi`, in id order: from the index (some may not exist
    /// any more), or all.
    fn edges_near(&self, lo: DVec3, hi: DVec3) -> Vec<EdgeId> {
        match &self.near {
            Some(n) => n.edges_in(self.m, lo, hi),
            None => self.m.edges.keys().copied().collect(),
        }
    }

    /// True if every vertex of the face lies on the plane.
    fn face_on(&self, plane: &Plane, f: &Face) -> bool {
        f.loops().flatten().all(|v| on_plane(plane, self.pos(*v)))
    }

    /// The side of a new face of region `r` (in the axes of `plane`). It agrees with a face that
    /// shares one of its edges (the two use the edge in opposite directions). A neighbour in the
    /// same plane comes first (the face continues a flat surface, so it faces the same way); at a
    /// fold or a fin the neighbour with the lowest id decides. With no neighbour the face looks up.
    fn side_of_new(&self, r: &Region, plane: &Plane) -> Plane {
        let mut best: Option<(bool, FaceId, bool)> = None;
        for (a, b) in r.half_edges() {
            let Some(e) = self.t.edge(a, b) else { continue };
            for g in self.t.faces_of(e) {
                let gf = &self.m.faces[&g];
                let flat = self.face_on(plane, gf);
                let same = gf.uses().any(|u| u == (a, b));
                // Coplanar first, then the lowest id.
                if best.is_none_or(|(bf, bg, _)| {
                    (flat, std::cmp::Reverse(g)) > (bf, std::cmp::Reverse(bg))
                }) {
                    best = Some((flat, g, same));
                }
            }
        }
        match best {
            Some((_, _, true)) => plane.flipped(),
            Some((_, _, false)) => *plane,
            None => upward(plane),
        }
    }

    /// The vertices next to the straight run of edges through `u`–`v`: the other ends of the
    /// edges at the vertices of the run that leave the line.
    fn beside_run(&self, u: VertexId, v: VertexId) -> BTreeSet<VertexId> {
        self.straight_run(u, v).1
    }

    /// The straight run of edges through `u`–`v` (its vertices) and the vertices beside it (see
    /// [`Work::beside_run`]).
    fn straight_run(&self, u: VertexId, v: VertexId) -> (BTreeSet<VertexId>, BTreeSet<VertexId>) {
        let (pu, pv) = (self.pos(u), self.pos(v));
        let mut run: BTreeSet<VertexId> = [u, v].into_iter().collect();
        let mut todo = vec![u, v];
        let mut beside = BTreeSet::new();
        while let Some(x) = todo.pop() {
            for e in self.t.edges_at(x) {
                let w = self.m.edges[&e].other(x).expect("an edge at x");
                let pw = self.pos(w);
                if geom::point_line(pw, pu, pv) <= geom::tol(&[pu, pv, pw]) {
                    if run.insert(w) {
                        todo.push(w);
                    }
                } else {
                    beside.insert(w);
                }
            }
        }
        (run, beside)
    }

    /// Solves the faces of every plane that holds a new piece, after edges were added.
    fn solve_after_add(
        &mut self,
        drawn: &[(VertexId, VertexId)],
        touched: &BTreeSet<(VertexId, VertexId)>,
    ) -> Result<(), SurfaceError> {
        self.solve_after_add_with(drawn, touched, true)
    }

    /// [`Work::solve_after_add`]; with `fill` false a region in no face gets no new face (only
    /// faces that are there split: Intersect Faces).
    fn solve_after_add_with(
        &mut self,
        drawn: &[(VertexId, VertexId)],
        touched: &BTreeSet<(VertexId, VertexId)>,
        fill: bool,
    ) -> Result<(), SurfaceError> {
        // With the index, the walks along cycles are kept for the whole solve (the edges do not
        // change in it, only the faces).
        let (planes, on, mut walks) = if self.near.is_some() {
            let mut walks = Walks::default();
            let (planes, on) = self.planes_near(drawn, &mut walks);
            (planes, on, Some(walks))
        } else {
            let (planes, on) = self.planes_scan(drawn);
            (planes, on, None)
        };
        // The pieces that may give a region in no face a new face.
        let keys: BTreeSet<(VertexId, VertexId)> = if fill {
            drawn.iter().map(|&(a, b)| key(a, b)).collect()
        } else {
            BTreeSet::new()
        };
        for (plane, pieces) in planes.into_iter().zip(on) {
            // The area that can change: the pieces in the plane and the loops they close.
            let mut area = Area::EMPTY;
            for i in pieces {
                let (u, v) = drawn[i];
                area.add(plane.project(self.pos(u)));
                area.add(plane.project(self.pos(v)));
                for (a, b) in [(u, v), (v, u)] {
                    let closed = match &mut walks {
                        Some(w) => w.closes_on_left(self.m, &self.t, &plane, a, b),
                        None => arrange::closes_on_left(self.m, &self.t, &plane, a, b),
                    };
                    if let Some((lo, hi)) = closed {
                        area.add(lo);
                        area.add(hi);
                    }
                }
            }
            self.solve_plane(plane, area, &keys, touched)?;
        }
        Ok(())
    }

    /// The planes to solve after edges were added, and for each plane the pieces (indices into
    /// `drawn`) that lie in it, with the spatial index: the same planes in the same order as
    /// [`Work::planes_scan`], but each test only looks at the planes, faces and pieces that can
    /// pass it.
    fn planes_near(
        &self,
        drawn: &[(VertexId, VertexId)],
        walks: &mut Walks,
    ) -> (Vec<Plane>, Vec<Vec<usize>>) {
        let m = &*self.m;
        let pos = |v: VertexId| m.vertices[&v];
        // The ends of the pieces, the pieces at each end, and the planes found so far through
        // each end.
        let mut ends: Vec<VertexId> = drawn.iter().flat_map(|&(u, v)| [u, v]).collect();
        ends.sort_unstable();
        ends.dedup();
        let slot = |v: VertexId| ends.binary_search(&v).expect("an end");
        let end_pos: Vec<DVec3> = ends.iter().map(|v| pos(*v)).collect();
        let points = Points::new(&end_pos);
        let mut at: Vec<Vec<usize>> = vec![Vec::new(); ends.len()];
        for (k, &(u, v)) in drawn.iter().enumerate() {
            at[slot(u)].push(k);
            at[slot(v)].push(k);
        }
        let mut planes: Vec<Plane> = Vec::new();
        let mut through: Vec<Vec<usize>> = vec![Vec::new(); ends.len()];
        let add = |planes: &mut Vec<Plane>, through: &mut [Vec<usize>], plane: Plane| {
            for i in points.near_plane(&plane) {
                if on_plane(&plane, end_pos[i]) {
                    through[i].push(planes.len());
                }
            }
            planes.push(plane);
        };
        // The planes of faces that hold a piece. Faces with the same plane (to the bit) pass or
        // fail the test together.
        let mut groups: BTreeMap<[u64; 4], Vec<FaceId>> = BTreeMap::new();
        for (id, f) in &m.faces {
            let (n, o) = (f.plane.normal, f.plane.offset);
            let bits = [n.x, n.y, n.z, o].map(f64::to_bits);
            groups.entry(bits).or_default().push(*id);
        }
        let mut holding: Vec<FaceId> = Vec::new();
        for fs in groups.values() {
            let plane = m.faces[&fs[0]].plane;
            let holds = points.near_plane(&plane).into_iter().any(|i| {
                on_plane(&plane, end_pos[i])
                    && at[i].iter().any(|&k| {
                        let (u, v) = drawn[k];
                        on_plane(&plane, pos(u)) && on_plane(&plane, pos(v))
                    })
            });
            if holds {
                holding.extend(fs);
            }
        }
        holding.sort_unstable();
        // A face whose vertices lie on a plane found before adds no plane. Such a plane passes
        // through the first vertex of the face, so only those planes are tested.
        let first = |f: &Face| f.loops().flatten().next().map(|v| pos(*v));
        let firsts: Vec<Option<DVec3>> = holding.iter().map(|f| first(&m.faces[f])).collect();
        let first_points = Points::new(&firsts.iter().flatten().copied().collect::<Vec<_>>());
        let with_first: Vec<usize> = (0..holding.len())
            .filter(|h| firsts[*h].is_some())
            .collect();
        let mut before: Vec<Vec<usize>> = vec![Vec::new(); holding.len()];
        for (h, id) in holding.iter().enumerate() {
            let f = &m.faces[id];
            let covered = match firsts[h] {
                Some(_) => before[h].iter().any(|&j| self.face_on(&planes[j], f)),
                // A face with no vertex lies on any plane.
                None => !planes.is_empty(),
            };
            if covered {
                continue;
            }
            for g in first_points.near_plane(&f.plane) {
                let g = with_first[g];
                if g > h && on_plane(&f.plane, firsts[g].expect("a first vertex")) {
                    before[g].push(planes.len());
                }
            }
            add(&mut planes, &mut through, f.plane);
        }
        // The planes where a piece closes a loop (see `planes_scan`). The pieces of one long
        // straight run share the walk along it (see `Runs`).
        let mut runs = Runs::default();
        for &(u, v) in drawn {
            let (pu, pv) = (pos(u), pos(v));
            let k = match runs.find(u, v, pu, pv) {
                Some(k) => k,
                None => {
                    let (run, beside) = self.straight_run(u, v);
                    runs.add(pu, pv, run, beside, pos)
                }
            };
            for &z in runs.beside(k) {
                let pz = pos(z);
                if geom::point_line(pz, pu, pv) <= geom::tol(&[pu, pv, pz]) {
                    continue;
                }
                // The planes found so far through `pu`. (The scan makes the plane first; both
                // tests only skip, so the order does not change the result.)
                if through[slot(u)]
                    .iter()
                    .any(|&j| on_plane(&planes[j], pv) && on_plane(&planes[j], pz))
                {
                    continue;
                }
                let Some(plane) = Plane::new(pu, (pv - pu).cross(pz - pu)) else {
                    continue;
                };
                let plane = upward(&plane);
                if walks.closes_on_left(m, &self.t, &plane, u, v).is_some()
                    || walks.closes_on_left(m, &self.t, &plane, v, u).is_some()
                {
                    add(&mut planes, &mut through, plane);
                }
            }
        }
        // The pieces in each plane: both ends on it.
        let mut on: Vec<Vec<usize>> = vec![Vec::new(); planes.len()];
        for (k, &(u, v)) in drawn.iter().enumerate() {
            let tv = &through[slot(v)];
            for &j in &through[slot(u)] {
                if tv.binary_search(&j).is_ok() {
                    on[j].push(k);
                }
            }
        }
        (planes, on)
    }

    /// The planes to solve after edges were added, and the pieces in each plane, by scans of all
    /// faces, planes and pieces.
    fn planes_scan(&self, drawn: &[(VertexId, VertexId)]) -> (Vec<Plane>, Vec<Vec<usize>>) {
        let mut planes: Vec<Plane> = Vec::new();
        // The planes of faces that hold a piece.
        for f in self.m.faces.values() {
            let holds = drawn
                .iter()
                .any(|(u, v)| on_plane(&f.plane, self.pos(*u)) && on_plane(&f.plane, self.pos(*v)));
            if holds && !planes.iter().any(|p| self.face_on(p, f)) {
                planes.push(f.plane);
            }
        }
        // The planes where a piece closes a loop: through the piece and a vertex next to the
        // straight run of edges that holds it (so a gap closed in a straight side counts too).
        for &(u, v) in drawn {
            let (pu, pv) = (self.pos(u), self.pos(v));
            for z in self.beside_run(u, v) {
                let pz = self.pos(z);
                if geom::point_line(pz, pu, pv) <= geom::tol(&[pu, pv, pz]) {
                    continue;
                }
                let Some(plane) = Plane::new(pu, (pv - pu).cross(pz - pu)) else {
                    continue;
                };
                let plane = upward(&plane);
                if planes
                    .iter()
                    .any(|p| on_plane(p, pu) && on_plane(p, pv) && on_plane(p, pz))
                {
                    continue;
                }
                if arrange::closes_on_left(self.m, &self.t, &plane, u, v).is_some()
                    || arrange::closes_on_left(self.m, &self.t, &plane, v, u).is_some()
                {
                    planes.push(plane);
                }
            }
        }
        let on = planes
            .iter()
            .map(|plane| {
                (0..drawn.len())
                    .filter(|&k| {
                        let (u, v) = drawn[k];
                        on_plane(plane, self.pos(u)) && on_plane(plane, self.pos(v))
                    })
                    .collect()
            })
            .collect();
        (planes, on)
    }

    /// The faces in `plane` that overlap `area`, and the area grown to hold them, until no more
    /// faces overlap it. Faces that only touch it at their border are left out.
    fn faces_in(&self, plane: &Plane, mut area: Area) -> (Vec<FaceId>, Area) {
        if let Some(n) = self.near.as_ref().filter(|_| area.is_finite()) {
            // The same rounds as the scan below: each round takes every face that overlaps the
            // area as it was at the start of the round.
            let mut found: BTreeSet<FaceId> = BTreeSet::new();
            loop {
                let eps = area.tolerance();
                let mut inside = Vec::new();
                for id in n.faces_in_plane(self.m, plane, area.lo, area.hi, eps) {
                    let Some(f) = self.m.faces.get(&id) else {
                        continue;
                    };
                    if found.contains(&id) || !self.face_on(plane, f) {
                        continue;
                    }
                    let b = self.outer_area(plane, f);
                    if b.overlaps(&area, eps) {
                        inside.push((id, b));
                    }
                }
                if inside.is_empty() {
                    break;
                }
                for (id, b) in inside {
                    area.join(&b);
                    found.insert(id);
                }
            }
            return (found.into_iter().collect(), area);
        }
        let mut boxes: Vec<(FaceId, Area)> = self
            .m
            .faces
            .iter()
            .filter(|(_, f)| self.face_on(plane, f))
            .map(|(id, f)| (*id, self.outer_area(plane, f)))
            .collect();
        let mut found = Vec::new();
        loop {
            let eps = area.tolerance();
            let (inside, rest): (Vec<_>, Vec<_>) =
                boxes.into_iter().partition(|(_, b)| b.overlaps(&area, eps));
            if inside.is_empty() {
                break;
            }
            for (id, b) in inside {
                area.join(&b);
                found.push(id);
            }
            boxes = rest;
        }
        found.sort();
        (found, area)
    }

    /// The 2D box of the outer loop of a face in the axes of `plane`.
    fn outer_area(&self, plane: &Plane, f: &Face) -> Area {
        let mut b = Area::EMPTY;
        for v in &f.outer {
            b.add(plane.project(self.pos(*v)));
        }
        b
    }

    /// The edges in `plane` (as vertex pairs) that touch `area`.
    fn edges_in(&self, plane: &Plane, area: &Area) -> BTreeSet<(VertexId, VertexId)> {
        let eps = area.tolerance();
        let ids = match self.near.as_ref().filter(|_| area.is_finite()) {
            Some(n) => n.edges_in_plane(self.m, plane, area.lo, area.hi, eps),
            None => self.m.edges.keys().copied().collect(),
        };
        ids.iter()
            .filter_map(|id| self.m.edges.get(id))
            .filter(|e| {
                let (a, b) = (self.pos(e.v[0]), self.pos(e.v[1]));
                if !on_plane(plane, a) || !on_plane(plane, b) {
                    return false;
                }
                let mut bx = Area::EMPTY;
                bx.add(plane.project(a));
                bx.add(plane.project(b));
                bx.touches(area, eps)
            })
            .map(Edge::key)
            .collect()
    }

    /// Solves the faces of one plane after edges were added, inside the area that can change.
    /// Regions only split, so each region that changed lies in one old face or in no face. A region
    /// in an old face is a part of it; the largest part keeps the id. A region in no face whose
    /// outer loop has a new piece gets a new face (a loop drawn inside an empty region fills the
    /// loop, not the region around it).
    fn solve_plane(
        &mut self,
        plane: Plane,
        area: Area,
        drawn: &BTreeSet<(VertexId, VertexId)>,
        touched: &BTreeSet<(VertexId, VertexId)>,
    ) -> Result<(), SurfaceError> {
        struct Old {
            id: FaceId,
            rings: Vec<Vec<DVec2>>,
            lo: DVec2,
            hi: DVec2,
        }
        let (faces, area) = self.faces_in(&plane, area);
        let arr = Arrangement::solve(self.m, plane, &self.edges_in(&plane, &area));
        let mut olds: Vec<Old> = Vec::new();
        let mut exact: BTreeMap<(Vec<VertexId>, Vec<Vec<VertexId>>), FaceId> = BTreeMap::new();
        for id in faces {
            let loops = in_axes(&self.m.faces[&id], &plane);
            let rings: Vec<Vec<DVec2>> = std::iter::once(&loops.0)
                .chain(loops.1.iter())
                .map(|l| l.iter().map(|v| plane.project(self.pos(*v))).collect())
                .collect();
            let (lo, hi) = arrange::box_of(&rings[0]);
            exact.insert(loops, id);
            olds.push(Old { id, rings, lo, hi });
        }
        let eps = area.tolerance();
        // With many old faces and regions, a box tree finds the old faces whose box holds a
        // sample (the first in the order of `olds` is taken, as by the scan).
        let tree = (olds.len() * arr.regions.len() > 4096).then(|| {
            BoxTree::new(
                olds.iter()
                    .enumerate()
                    .map(|(i, o)| (o.lo.extend(0.0), o.hi.extend(0.0), i as u32))
                    .collect(),
            )
        });
        let mut kept: BTreeSet<FaceId> = BTreeSet::new();
        let mut parts: BTreeMap<FaceId, Vec<usize>> = BTreeMap::new();
        let mut fresh: Vec<usize> = Vec::new();
        for (i, r) in arr.regions.iter().enumerate() {
            // Edges that reach out of the area may close loops that are not regions of the whole
            // plane; every region that can change lies inside the area.
            let (lo, hi) = arrange::box_of(&arr.rings(r)[0]);
            if !area.holds(lo, hi, eps) {
                continue;
            }
            if !r.half_edges().any(|(a, b)| touched.contains(&key(a, b))) {
                if let Some(id) = exact.get(&(r.outer.clone(), r.holes.clone())) {
                    kept.insert(*id);
                }
                continue;
            }
            let s = arr.sample(r)?;
            let candidates: Vec<usize> = match &tree {
                Some(t) => {
                    let mut c = Vec::new();
                    let p = s.extend(0.0);
                    t.query(p, p, &mut |i| c.push(i as usize));
                    c.sort_unstable();
                    c
                }
                None => (0..olds.len()).collect(),
            };
            let owner = candidates
                .into_iter()
                .map(|i| &olds[i])
                .find(|o| s.cmpge(o.lo).all() && s.cmple(o.hi).all() && inside(s, &o.rings));
            let n = r.outer.len();
            let drawn_outer =
                (0..n).any(|k| drawn.contains(&key(r.outer[k], r.outer[(k + 1) % n])));
            match owner {
                Some(o) => parts.entry(o.id).or_default().push(i),
                None if drawn_outer => fresh.push(i),
                None => {}
            }
        }
        for o in &olds {
            if kept.contains(&o.id) {
                continue;
            }
            let mut ps = parts.remove(&o.id).unwrap_or_default();
            if ps.is_empty() {
                return Err(SurfaceError::Unsolved(format!(
                    "face {} has no region in its plane",
                    o.id
                )));
            }
            ps.sort_by(|&a, &b| {
                let (ra, rb) = (&arr.regions[a], &arr.regions[b]);
                rb.area.total_cmp(&ra.area).then(ra.outer.cmp(&rb.outer))
            });
            let like = self.m.faces[&o.id].clone();
            for (k, &i) in ps.iter().enumerate() {
                let face = from_region(&arr.regions[i], &plane, like.plane, Some(&like));
                if k == 0 {
                    self.put_face(o.id, face);
                } else {
                    self.new_face(face);
                }
            }
        }
        for i in fresh {
            let r = &arr.regions[i];
            let side = self.side_of_new(r, &plane);
            let face = from_region(r, &plane, side, None);
            self.new_face(face);
        }
        Ok(())
    }
}

/// A 2D box in the axes of a plane: the part of the plane that an edit can change.
#[derive(Clone, Copy, Debug)]
struct Area {
    lo: DVec2,
    hi: DVec2,
}

impl Area {
    const EMPTY: Area = Area {
        lo: DVec2::splat(f64::INFINITY),
        hi: DVec2::splat(f64::NEG_INFINITY),
    };

    fn add(&mut self, p: DVec2) {
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }

    fn is_finite(&self) -> bool {
        self.lo.is_finite() && self.hi.is_finite()
    }

    fn join(&mut self, o: &Area) {
        self.lo = self.lo.min(o.lo);
        self.hi = self.hi.max(o.hi);
    }

    /// The length tolerance at the size of the box.
    fn tolerance(&self) -> f64 {
        let size = self.lo.abs().max(self.hi.abs()).max_element();
        geom::tolerance_at(if size.is_finite() { size } else { 0.0 })
    }

    /// True if the insides overlap by more than `eps`, or one box holds the other: a box that only
    /// shares a border does not overlap.
    fn overlaps(&self, o: &Area, eps: f64) -> bool {
        (self.lo.cmplt(o.hi - eps).all() && o.lo.cmplt(self.hi - eps).all())
            || (o.lo.cmpge(self.lo - eps).all() && o.hi.cmple(self.hi + eps).all())
    }

    /// True if the boxes meet, within `eps`.
    fn touches(&self, o: &Area, eps: f64) -> bool {
        self.lo.cmple(o.hi + eps).all() && o.lo.cmple(self.hi + eps).all()
    }

    /// True if the box `lo`–`hi` lies inside, within `eps`.
    fn holds(&self, lo: DVec2, hi: DVec2, eps: f64) -> bool {
        lo.cmpge(self.lo - eps).all() && hi.cmple(self.hi + eps).all()
    }
}

/// The loops of a face in the axes of `plane` (counterclockwise outer loop when the plane faces
/// the same way as the face; else the loops are turned around), each from its smallest id, the
/// holes sorted.
fn in_axes(f: &Face, plane: &Plane) -> (Vec<VertexId>, Vec<Vec<VertexId>>) {
    let turn = |l: &Vec<VertexId>| {
        if f.plane.normal.dot(plane.normal) < 0.0 {
            reversed(l)
        } else {
            canonical(l)
        }
    };
    let mut holes: Vec<Vec<VertexId>> = f.holes.iter().map(turn).collect();
    holes.sort();
    (turn(&f.outer), holes)
}

/// A face of region `r` (found in the axes of `axes`) with the plane `side`.
/// A face of region `r` (found in the axes of `axes`) with the plane `side`, and the attributes
/// of `like` (the face that it is a part of, or the face that a merge keeps).
fn from_region(r: &Region, axes: &Plane, side: Plane, like: Option<&Face>) -> Face {
    let mut f = if side.normal.dot(axes.normal) >= 0.0 {
        Face::new(side, r.outer.clone(), r.holes.clone())
    } else {
        let mut holes: Vec<Vec<VertexId>> = r.holes.iter().map(|h| reversed(h)).collect();
        holes.sort();
        Face::new(side, reversed(&r.outer), holes)
    };
    if let Some(l) = like {
        f.attrs = l.attrs.clone();
        f.extra = l.extra.clone();
    }
    f
}

/// The plane turned to face up (+Z); a vertical plane faces +Y, then +X.
fn upward(p: &Plane) -> Plane {
    const EPS: f64 = 1e-9;
    let n = p.normal;
    let flip = if n.z.abs() > EPS {
        n.z < 0.0
    } else if n.y.abs() > EPS {
        n.y < 0.0
    } else {
        n.x < 0.0
    };
    if flip { p.flipped() } else { *p }
}

fn add_edges(m: &mut SurfaceModel, segs: &[[[f64; 3]; 2]]) -> Result<(), SurfaceError> {
    if segs.len() > MAX_EDIT_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges in one edit",
            max: MAX_EDIT_EDGES,
            count: segs.len(),
        });
    }
    add_edges_in(&mut Work::indexed(m), segs)
}

/// The body of [`add_edges`], on a work that the caller made (and may keep).
fn add_edges_in(w: &mut Work<'_>, segs: &[[[f64; 3]; 2]]) -> Result<(), SurfaceError> {
    let mut drawn = Vec::new();
    let mut touched = BTreeSet::new();
    for s in segs {
        let (a, b) = (DVec3::from(s[0]), DVec3::from(s[1]));
        if [a, b]
            .iter()
            .any(|p| !p.is_finite() || geom::size(*p) > MAX_COORD)
        {
            return Err(SurfaceError::BadPoint);
        }
        w.insert_segment(a, b, &mut drawn, &mut touched)?;
        w.check_limits()?;
    }
    w.solve_after_add(&drawn, &touched)?;
    w.check_limits()
}

/// One model that takes many edits one after another and keeps its indexes between them. Each
/// edit gives the model, the log and the revision that [`SurfaceModel::apply`] gives (the result
/// is the same to the bit, and a test compares it with the plain edits). The cost of an edit is
/// that of its own edges and not that of the whole model: `apply` copies the model and builds the
/// indexes again each time, which is what made a long run of small edits (an explode of many
/// parts) cost more than the square of its size.
///
/// The model changes only through the methods of the session, so the indexes stay true (the
/// attributes and the curves are not in an index, see [`Session::model_mut`]). After an error the
/// session is not to be used again.
pub(super) struct Session {
    m: SurfaceModel,
    t: Topo,
    near: Option<Near>,
}

impl Session {
    pub(super) fn new(m: SurfaceModel) -> Session {
        let t = Topo::of(&m);
        let near = (reference::mode() == reference::Mode::Index).then(|| Near::of(&m));
        Session { m, t, near }
    }

    pub(super) fn model(&self) -> &SurfaceModel {
        &self.m
    }

    /// The model, for a change that touches no vertex, edge or face geometry: attributes and
    /// curves.
    pub(super) fn model_mut(&mut self) -> &mut SurfaceModel {
        &mut self.m
    }

    pub(super) fn into_model(self) -> SurfaceModel {
        self.m
    }

    /// `apply(&Edit::AddEdges { edges })`, on the model of the session.
    pub(super) fn add_edges(&mut self, segs: &[[[f64; 3]; 2]]) -> Result<(), SurfaceError> {
        let intent = sync::Intent::AddEdges {
            edges: segs.to_vec(),
        };
        let base = self.m.log.is_empty().then(|| self.m.clone());
        let shape = (
            self.m.vertices.len(),
            self.m.edges.len(),
            self.m.faces.len(),
            self.m.next_id,
            self.near.as_ref().map(|n| n.loops),
        );
        let t = std::mem::take(&mut self.t);
        let mut w = Work {
            m: &mut self.m,
            t,
            near: self.near.take(),
        };
        let r = add_edges_in(&mut w, segs);
        let Work { t, near, .. } = w;
        self.t = t;
        self.near = near;
        r?;
        // `apply` records an edit that changed the geometry. An edit that allocated nothing and
        // changed no count did not (every change of an edge or a face makes a new id or changes a
        // count).
        let after = (
            self.m.vertices.len(),
            self.m.edges.len(),
            self.m.faces.len(),
            self.m.next_id,
            self.near.as_ref().map(|n| n.loops),
        );
        if after != shape {
            sync::record_in_place(&mut self.m, base.as_ref(), intent);
        }
        Ok(())
    }

    /// `apply(&Edit::ReverseFaces { faces })`, on the model of the session.
    pub(super) fn reverse_faces(&mut self, faces: Vec<FaceId>) -> Result<(), SurfaceError> {
        let intent = sync::Intent::of(
            &self.m,
            &Edit::ReverseFaces {
                faces: faces.clone(),
            },
        );
        let base = self.m.log.is_empty().then(|| self.m.clone());
        reverse_faces(&mut self.m, &faces)?;
        if !faces.is_empty() {
            sync::record_in_place(&mut self.m, base.as_ref(), intent);
        }
        Ok(())
    }

    /// The same as `sync::face_at`, found through the index.
    pub(super) fn face_at(&self, p: DVec3) -> Option<FaceId> {
        let Some(near) = &self.near else {
            return sync::face_at(&self.m, p);
        };
        let pad = DVec3::splat(4.0 * near.tol(geom::size(p)));
        near.faces_meeting(&self.m, p - pad, p + pad)
            .into_iter()
            .find(|id| {
                self.m
                    .faces
                    .get(id)
                    .is_some_and(|f| sync::face_holds(&self.m, f, p))
            })
    }

    /// The same as `sync::edge_at`, found through the index.
    pub(super) fn edge_at(&self, p: DVec3) -> Option<EdgeId> {
        let Some(near) = &self.near else {
            return sync::edge_at(&self.m, p);
        };
        let pad = DVec3::splat(4.0 * near.tol(geom::size(p)));
        let ids = near.edges_in(&self.m, p - pad, p + pad);
        sync::nearest_edge(&self.m, p, ids.into_iter())
    }
}

fn add_curve(
    m: &mut SurfaceModel,
    points: &[[f64; 3]],
    closed: bool,
    faceted: bool,
) -> Result<(), SurfaceError> {
    let n = points.len();
    let count = if closed { n } else { n.saturating_sub(1) };
    if n < 2 || count < 1 {
        return Err(SurfaceError::TooShort {
            tol: LENGTH_TOLERANCE,
        });
    }
    if count > MAX_EDIT_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges in one edit",
            max: MAX_EDIT_EDGES,
            count,
        });
    }
    let mut w = Work::indexed(m);
    let mut drawn = Vec::new();
    let mut touched = BTreeSet::new();
    // For each segment, the pieces it was drawn in (start and end vertex).
    let mut spans: Vec<(VertexId, VertexId)> = Vec::new();
    for i in 0..count {
        let (a, b) = (DVec3::from(points[i]), DVec3::from(points[(i + 1) % n]));
        if [a, b]
            .iter()
            .any(|p| !p.is_finite() || geom::size(*p) > MAX_COORD)
        {
            return Err(SurfaceError::BadPoint);
        }
        let from = drawn.len();
        w.insert_segment(a, b, &mut drawn, &mut touched)?;
        spans.push((drawn[from].0, drawn[drawn.len() - 1].1));
        w.check_limits()?;
    }
    w.solve_after_add(&drawn, &touched)?;
    // Later segments can cut the earlier pieces: find the edges that run between the span ends.
    let mut chain: Vec<EdgeId> = Vec::new();
    for (x, y) in spans {
        match w.chain_between(x, y) {
            Some(es) => chain.extend(es),
            None => return w.check_limits(),
        }
    }
    let used: BTreeSet<EdgeId> = w.m.curves.values().flatten().copied().collect();
    let unique: BTreeSet<EdgeId> = chain.iter().copied().collect();
    if unique.len() == chain.len() && chain.iter().all(|e| !used.contains(e)) {
        if faceted {
            style::mark_faceted(w.m, &chain);
        }
        let id = super::CurveId(w.m.alloc());
        w.m.curves.insert(id, chain);
    }
    w.check_limits()
}

impl Work<'_> {
    /// The edges that run along the straight line from `x` to `y`, in order, or `None` when no
    /// such chain exists.
    fn chain_between(&self, x: VertexId, y: VertexId) -> Option<Vec<EdgeId>> {
        let (p, q) = (self.pos(x), self.pos(y));
        let len = p.distance(q);
        let mut out = Vec::new();
        let mut cur = x;
        let mut at = 0.0;
        while cur != y {
            let mut best: Option<(f64, EdgeId, VertexId)> = None;
            for e in self.t.edges_at(cur) {
                let o = self.m.edges[&e].other(cur)?;
                let po = self.pos(o);
                let (d, s) = geom::point_segment(po, p, q);
                if d <= geom::tol(&[p, q, po]) && s * len > at + 1e-12 {
                    let s = s * len;
                    if best.is_none_or(|(bs, _, _)| s < bs) {
                        best = Some((s, e, o));
                    }
                }
            }
            let (s, e, o) = best?;
            out.push(e);
            at = s;
            cur = o;
        }
        Some(out)
    }
}

/// A small union-find of faces; `FaceId(0)` stands for "no face" (ids start at 1).
#[derive(Default)]
struct Groups(BTreeMap<FaceId, FaceId>);

const EMPTY: FaceId = FaceId(0);

impl Groups {
    fn root(&mut self, f: FaceId) -> FaceId {
        let p = *self.0.entry(f).or_insert(f);
        if p == f {
            return f;
        }
        let r = self.root(p);
        self.0.insert(f, r);
        r
    }

    fn join(&mut self, a: FaceId, b: FaceId) {
        let (ra, rb) = (self.root(a), self.root(b));
        // The smaller id is the root, so `EMPTY` is the root of any group that holds it.
        let (lo, hi) = if ra <= rb { (ra, rb) } else { (rb, ra) };
        self.0.insert(hi, lo);
    }
}

/// The attributes of a merged face. The id that stays is the lowest (`keep`), but the look is that
/// of the merged face with the largest area (a tie goes to the lower id): its attributes (a
/// material too) and unknown fields become those of the new face, whole. Nothing is taken from the
/// smaller faces, so a small face never changes how a large one looks (a painted bump pushed
/// flat does not paint the top). Each value of a smaller face that differs from the one kept, or
/// that the larger face does not have, is dropped with a note, so a merge never drops a material
/// silently. `taken` is sorted by id and `areas` has the area of each face in the same order.
fn merge_attrs(
    keep: FaceId,
    into: &mut Face,
    taken: &[(FaceId, Face)],
    areas: &[f64],
    notes: &mut Vec<String>,
) {
    let mut win = 0;
    for (i, a) in areas.iter().enumerate() {
        // `>` keeps the first (lowest id) of equal areas.
        if *a > areas[win] {
            win = i;
        }
    }
    let (win_id, winner) = &taken[win];
    into.attrs = winner.attrs.clone();
    into.extra = winner.extra.clone();
    for (i, (id, f)) in taken.iter().enumerate() {
        if i == win {
            continue;
        }
        for (what, mine, theirs) in [
            ("attribute", &winner.attrs, &f.attrs),
            ("field", &winner.extra, &f.extra),
        ] {
            for (k, v) in theirs.iter() {
                let own = mine.get(k);
                if own != Some(v) {
                    let kept = match own {
                        Some(own) => format!("keeps {own}"),
                        None => "has none".to_string(),
                    };
                    notes.push(format!(
                        "face {id} merged into face {keep}: its {what} {k} = {v} was dropped \
                         (the larger face {win_id} {kept})"
                    ));
                }
            }
        }
    }
}

fn erase_edges(
    m: &mut SurfaceModel,
    ids: &[EdgeId],
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    if let Some(id) = ids.iter().find(|id| !m.edges.contains_key(id)) {
        return Err(SurfaceError::NoEdge(*id));
    }
    let mut w = Work::indexed(m);
    erase_edges_in(&mut w, ids, notes)
}

/// [`erase_edges`] in a work whose index holds the edges and their faces (all edges must exist).
/// The topology index may cover only a part of the model: it must hold every edge at the ends of
/// the erased edges and at their neighbours, and every face of those edges (see
/// `transform::local_work`).
fn erase_edges_in(
    w: &mut Work<'_>,
    ids: &[EdgeId],
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    let ids: BTreeSet<EdgeId> = ids.iter().copied().collect();
    let mut groups = Groups::default();
    let mut ends: BTreeSet<VertexId> = BTreeSet::new();
    for &e in &ids {
        let edge = w.m.edges[&e];
        ends.extend(edge.v);
        let fs: Vec<FaceId> = w.t.faces_of(e).collect();
        // +1 when the face uses the edge from its first vertex to its second.
        let dir = |f: FaceId| -> f64 {
            if w.m.faces[&f].uses().any(|u| u == (edge.v[0], edge.v[1])) {
                1.0
            } else {
                -1.0
            }
        };
        for &f in &fs {
            let ff = &w.m.faces[&f];
            let partner = fs.iter().copied().find(|&g| {
                let gf = &w.m.faces[&g];
                g != f
                    && w.face_on(&ff.plane, gf)
                    && dir(g) * gf.plane.normal.dot(ff.plane.normal).signum() == -dir(f)
            });
            groups.join(f, partner.unwrap_or(EMPTY));
        }
    }
    let members: Vec<FaceId> = groups.0.keys().copied().filter(|f| *f != EMPTY).collect();
    let mut by_root: BTreeMap<FaceId, Vec<FaceId>> = BTreeMap::new();
    for f in members {
        let r = groups.root(f);
        by_root.entry(r).or_default().push(f);
    }
    // Each merge: its faces, the lowest id first.
    let mut merges: Vec<Vec<(FaceId, Face)>> = Vec::new();
    for (root, fs) in by_root {
        let faces: Vec<(FaceId, Face)> = fs
            .iter()
            .filter_map(|f| w.take_face(*f).map(|g| (*f, g)))
            .collect();
        if root != EMPTY && !faces.is_empty() {
            merges.push(faces);
        }
    }
    // The curves that lose an edge: what is left of each is cut into chains below.
    let broken: BTreeSet<super::CurveId> =
        w.m.curves
            .iter()
            .filter(|(_, es)| es.iter().any(|e| ids.contains(e)))
            .map(|(c, _)| *c)
            .collect();
    for &e in &ids {
        w.remove_edge(e);
    }
    for taken in merges {
        let keep = taken[0].0;
        let fs: Vec<FaceId> = taken.iter().map(|(id, _)| *id).collect();
        let faces: Vec<&Face> = taken.iter().map(|(_, f)| f).collect();
        let plane = faces[0].plane;
        let mut area = Area::EMPTY;
        for f in &faces {
            for v in &f.outer {
                area.add(plane.project(w.pos(*v)));
            }
        }
        let (_, area) = w.faces_in(&plane, area);
        let arr = Arrangement::solve(w.m, plane, &w.edges_in(&plane, &area));
        let mut region_of: BTreeMap<(VertexId, VertexId), usize> = BTreeMap::new();
        for (i, r) in arr.regions.iter().enumerate() {
            for h in r.half_edges() {
                region_of.insert(h, i);
            }
        }
        let found = faces.iter().find_map(|f| {
            let flip = f.plane.normal.dot(plane.normal) < 0.0;
            f.uses().find_map(|(a, b)| {
                let h = if flip { (b, a) } else { (a, b) };
                region_of.get(&h).copied()
            })
        });
        let Some(i) = found else {
            return Err(SurfaceError::Unsolved(format!(
                "the merge of faces {fs:?} has no region"
            )));
        };
        let mut face = from_region(&arr.regions[i], &plane, plane, Some(faces[0]));
        let areas: Vec<f64> = taken
            .iter()
            .map(|(_, f)| geom::face_area(f, |v| w.pos(v)))
            .collect();
        merge_attrs(keep, &mut face, &taken, &areas, notes);
        w.put_face(keep, face);
    }
    w.heal(&ends);
    rechain(w.m, &broken);
    Ok(())
}

/// Cuts each of the curves `ids` into chains: an edge that does not meet the edge before it
/// starts a new curve (the first chain keeps the id). A closed curve that lost edges is first
/// turned so that it starts after a gap, so its ends join again (a circle with one edge erased
/// stays one arc). An empty curve goes.
pub(super) fn rechain(m: &mut SurfaceModel, ids: &BTreeSet<super::CurveId>) {
    let meet = |m: &SurfaceModel, a: EdgeId, b: EdgeId| {
        let (p, q) = (m.edges[&a].v, m.edges[&b].v);
        p.iter().any(|v| q.contains(v))
    };
    for id in ids {
        let Some(mut es) = m.curves.get(id).cloned() else {
            continue;
        };
        if es.is_empty() {
            m.curves.remove(id);
            continue;
        }
        let gap = (1..es.len()).find(|&i| !meet(m, es[i - 1], es[i]));
        if let Some(i) = gap
            && es.len() > 2
            && meet(m, es[es.len() - 1], es[0])
        {
            es.rotate_left(i);
        }
        let mut pieces: Vec<Vec<EdgeId>> = vec![vec![es[0]]];
        for pair in es.windows(2) {
            if meet(m, pair[0], pair[1]) {
                pieces.last_mut().expect("one piece").push(pair[1]);
            } else {
                pieces.push(vec![pair[1]]);
            }
        }
        let mut pieces = pieces.into_iter();
        m.curves.insert(*id, pieces.next().expect("one piece"));
        for p in pieces {
            let c = super::CurveId(m.alloc());
            m.curves.insert(c, p);
        }
    }
}

impl Work<'_> {
    /// After an erase: a vertex with no edge goes, and a vertex between two edges in a straight
    /// line with the same faces is taken out (the two edges become one, with the lower id).
    fn heal(&mut self, ends: &BTreeSet<VertexId>) {
        for &v in ends {
            if !self.m.vertices.contains_key(&v) {
                continue;
            }
            match self.t.degree(v) {
                0 => {
                    self.m.vertices.remove(&v);
                }
                2 => self.heal_straight(v),
                _ => {}
            }
        }
    }

    fn heal_straight(&mut self, v: VertexId) {
        let es: Vec<EdgeId> = self.t.edges_at(v).collect();
        let (e1, e2) = (es[0], es[1]);
        let (x, y) = (
            self.m.edges[&e1].other(v).expect("an edge at v"),
            self.m.edges[&e2].other(v).expect("an edge at v"),
        );
        let (px, py, pv) = (self.pos(x), self.pos(y), self.pos(v));
        let straight = geom::point_segment(pv, px, py).0 <= geom::plane_tol(&[px, py, pv]);
        let f1: Vec<FaceId> = self.t.faces_of(e1).collect();
        let f2: Vec<FaceId> = self.t.faces_of(e2).collect();
        let short = f1.iter().any(|f| {
            self.m.faces[f]
                .loops()
                .any(|l| l.contains(&v) && l.len() <= 3)
        });
        // Two edges with other attributes or in other curves stay two.
        let same_kind = self.m.edge_attrs.get(&e1) == self.m.edge_attrs.get(&e2)
            && self.m.curve_of(e1) == self.m.curve_of(e2);
        if !straight || !same_kind || f1 != f2 || short || self.t.edge(x, y).is_some() {
            return;
        }
        let faces: Vec<Face> = f1.iter().filter_map(|f| self.take_face(*f)).collect();
        let first = self.m.edges[&e1];
        // The joined edge keeps the id, the attributes and the place in the curve of `e1`.
        self.m.edges.remove(&e1);
        self.t.remove_edge(e1, first);
        self.remove_edge(e2);
        let joined = Edge {
            v: if first.v[0] == v { [y, x] } else { [x, y] },
        };
        self.m.edges.insert(e1, joined);
        self.t.add_edge(e1, joined);
        if let Some(n) = &mut self.near {
            n.add_edge(e1, self.m.vertices[&x], self.m.vertices[&y]);
        }
        self.m.vertices.remove(&v);
        for (id, mut f) in f1.into_iter().zip(faces) {
            f.outer = canonical(
                &f.outer
                    .iter()
                    .copied()
                    .filter(|u| *u != v)
                    .collect::<Vec<_>>(),
            );
            let mut holes: Vec<Vec<VertexId>> = f
                .holes
                .iter()
                .map(|h| canonical(&h.iter().copied().filter(|u| *u != v).collect::<Vec<_>>()))
                .collect();
            holes.sort();
            f.holes = holes;
            self.put_face(id, f);
        }
    }
}
