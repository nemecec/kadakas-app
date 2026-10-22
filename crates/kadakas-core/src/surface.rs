//! Surface models (MOD-06): vertices, edges and faces with holes. This is the geometry of the
//! 3D-first modeler (section 4.23 of `docs/feature-spec.md`).
//!
//! # What a surface model is
//!
//! A surface model is one object (`Shape::SurfaceModel`). It holds **vertices** (points), straight
//! **edges** between two vertices, and planar **faces**. A face has one outer loop and any number
//! of inner loops (holes). The coordinates are local to the model (mm); `transform` places the
//! model in the drawing (the `z` of the result is measured from the elevation of the layer, as for
//! extrudes). All geometry in one model "sticks together": edges that meet share a vertex, an edge
//! that crosses another splits both, and edges in the plane of a face split it. Geometry in two
//! models never interacts, so a group or a component (MOD-07) is a model of its own.
//!
//! # Topology: edges with face uses, not half-edges
//!
//! A half-edge structure (each edge is two twin half-edges, each with exactly one face) can only
//! hold 2-manifold surfaces. A model of this kind is often not manifold: edges with no face (wire
//! edges, guides of a sketch), edges with one face (the border of an open surface), edges with
//! three or more faces (a fin on a box), faces that touch at one vertex, and holes. So the model
//! uses the structure of a radial-edge kernel, kept small:
//!
//! - an edge names its two vertices; there is at most one edge between two vertices;
//! - a face names its loops as cycles of vertex ids; each pair of neighbours in a loop is an edge,
//!   used in the direction of the loop (a *use* or co-edge);
//! - the index (`topo`) is derived: the edge of a vertex pair, the edges at a vertex, and the faces
//!   that use an edge (the radial list). It is built when an edit starts and kept in step during
//!   the edit. It is never saved, so the file has one source of truth.
//!
//! An edge is manifold when exactly two faces use it, in opposite directions. [`SurfaceModel::check`]
//! reports wire edges (no face), border edges (one face), non-manifold edges (three or more faces)
//! and reversed edges (two faces that disagree about front and back), so the solid tools (MOD-09)
//! can ask for a closed solid and show the edges that make it open.
//!
//! # Faces, planes and sides
//!
//! A face keeps its plane (`normal` and `offset`). The normal points to the **front**; the outer
//! loop runs counterclockwise seen from the front and the holes run clockwise. Every vertex of a
//! face lies within the plane tolerance of the plane. The plane is kept, not derived, so that a
//! face that is split or merged keeps exactly its plane and nothing drifts. A new face takes its
//! side from a neighbour face that shares an edge (so the two agree, as in a closed solid): a
//! neighbour in the same plane first, else (at a fold or a fin) the neighbour with the lowest id.
//! With no neighbour it faces up (+Z), and a vertical face faces +Y, then +X.
//!
//! Inside one plane the faces never overlap. The edges in a plane form a planar graph; its regions
//! (with holes) are found by walking the graph (the "sharpest left turn" walk), and a face is a
//! region that is filled. An edge with the same region on both sides (a stray edge on a face) is no
//! part of any loop.
//!
//! # Behavior
//!
//! - **Add edges** ([`Edit::AddEdges`]): an end point within the length tolerance of a vertex
//!   becomes that vertex (weld); an end point on an edge splits the edge. The new edge is split at
//!   every vertex on it and at every edge that it crosses (the crossed edge is split too). Then
//!   each plane that holds a new piece is solved again: a face that a piece crosses splits in two
//!   (the larger part keeps the id), a closed loop drawn inside a face makes an inner face and a
//!   hole in the outer face, and a closed loop of edges in one plane that has no face gets one.
//!   Drawing an edge that exists again restores the faces next to it. A loop drawn inside an empty
//!   region (a loop whose face was erased) fills only the new loop. Only the part of the plane that
//!   the new pieces and the loops they close can reach is solved again, so the region work does
//!   not grow with the faces elsewhere in the plane. Welds, splits, crossings and the planes to
//!   solve are found with a spatial index of the edit (`edit::near`), not by scans of the model.
//! - **Erase edges** ([`Edit::EraseEdges`]): two coplanar faces on the two sides of an erased edge
//!   merge (the lower id stays). Every other face that uses an erased edge goes away. A vertex left
//!   with no edge goes away, and a vertex left between two edges in a straight line with the same
//!   faces is healed (the two edges become one again).
//! - **Erase faces** ([`Edit::EraseFaces`]): only the faces go; their edges stay.
//! - **Reverse faces** ([`Edit::ReverseFaces`]): front and back change places.
//! - **Set face material** ([`Edit::SetFaceMaterial`]): Paint Bucket (MOD-05); the material of the
//!   front is the attribute `material`, of the back `back_material`; Reverse swaps them.
//! - **Add curve** ([`Edit::AddCurve`]): a chain of edges through points that stays one curve (a
//!   circle or an arc), with the faces that it closes.
//! - **Push/Pull** ([`Edit::PushPull`]): a face moves along its normal (the walls follow), or is
//!   extruded into a bump, a pocket or a closed solid. The hole through a slab, when a push reaches
//!   the opposite face, is a later step and is refused.
//! - **Intersect Faces** ([`Edit::IntersectFaces`]): drawing does not split faces in other planes
//!   that cross, nor an edge that passes through a face in another plane (as in other modelers of
//!   this kind; the check warns, [`Problem::NotIntersected`]). This edit makes the edges where
//!   faces cross, so both faces split, and splits the edges where they pass through a face; for
//!   the whole model or for chosen faces and edges.
//! - **Transform vertices** ([`Edit::TransformVertices`]): Move, Rotate and Scale of vertices, faces
//!   and edges by an affine map. Every face that has a moved vertex is refitted to a plane; a face
//!   that is not flat any more is folded into triangles along new `soft` and `smooth` edges (the
//!   largest piece keeps the id of the face), vertices that land on others weld, and faces that
//!   become coplanar merge. A map of all vertices of a face never folds it. See the module
//!   `edit::transform`.
//! - **Copy geometry** ([`Edit::CopyGeometry`]): copies of faces and edges in the same model, with
//!   new ids, welded to the geometry they land on.
//! - Erasing edges of a curve leaves the rest as chains: a curve that is no longer one chain is
//!   cut into curves.
//!
//! # Tolerances
//!
//! Rounding errors grow with the size of the coordinates, so the tolerances do too. For points
//! whose largest coordinate (in absolute value, local) is `s` mm:
//!
//! - **length tolerance** `max(LENGTH_TOLERANCE, RELATIVE_TOLERANCE * s)`: 0.01 mm up to 10 km from
//!   the local origin, then 1 part in 10^9 (1 mm at the largest coordinate, 1000 km). Points closer
//!   than this are one vertex, and no edge is shorter;
//! - **plane tolerance**: a quarter of the length tolerance. A point this close to a plane is on
//!   it. It is smaller than the length tolerance, so two edges that are farther apart than the
//!   length tolerance never cross when they are seen in the plane.
//!
//! The size `s` comes from the points that are compared, not from the whole model: an edit far away
//! never changes the result of a test here. The validity check uses the same plane test as the
//! edits, so a face is valid exactly when the edits see it in its plane.
//!
//! # Room to grow and collaboration
//!
//! Edges and faces have free attribute maps ([`Attrs`]: soft, smooth, hidden, material in later
//! rows), and fields that this version does not know are kept. Curves ([`CurveId`]) are chains of
//! edges that stay one arc or circle when they are cut. `rev` counts the edits and a short log
//! holds them as replayable intents, so that two users who edit one model at once both keep their
//! work (see [`sync`]).
//!
//! # Determinism and ids
//!
//! Vertices, edges and faces have ids from one counter of the model (`next_id`), so an id names one
//! element of any kind (for selection, undo, collaboration and agents). Ids are never reused. All
//! maps are ordered (`BTreeMap`); no result depends on hash order or on randomness. The same model
//! and the same edit always give the same ids and the same geometry. The saved loops start at their
//! smallest vertex id, so equal models have equal text.
//!
//! # Limits
//!
//! [`MAX_VERTICES`], [`MAX_EDGES`], [`MAX_FACES`], [`MAX_LOOP_VERTICES`] and [`MAX_EDIT_EDGES`]. An
//! edit that would pass one fails with [`SurfaceError::Limit`] and changes nothing; a file over a
//! limit does not load. The numbers come from the measurement in `tests::perf_limits` (see the
//! constants).

use std::collections::BTreeMap;

use glam::{DAffine3, DVec2, DVec3};
use serde::{Deserialize, Serialize};

mod arrange;
mod check;
mod clash;
mod edit;
mod geom;
mod index;
mod part;
mod serial;
pub mod sync;
mod topo;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_absorb;
#[cfg(test)]
mod tests_followme;
#[cfg(test)]
mod tests_hide;
#[cfg(test)]
mod tests_index;
#[cfg(test)]
mod tests_intersect;
#[cfg(test)]
mod tests_offset;
#[cfg(test)]
mod tests_pushpull;
#[cfg(test)]
mod tests_review;
#[cfg(test)]
mod tests_style;
#[cfg(test)]
mod tests_transform;

pub use check::{At, Problem, Report};
pub use edit::style::{
    ATTR_FACETED, ATTR_SMOOTH, ATTR_SOFT, SoftSurfaces, dihedral_of, flagged, soften_plan,
};
pub use edit::{ATTR_BACK_MATERIAL, ATTR_MATERIAL, Edit, IntersectScope, PaintSide};
pub use geom::{Plane, on_plane, plane_tolerance_at, tolerance_at};
pub use part::{Absorber, Selection};

/// The version of the saved form of a surface model (the field `v`). A file with a higher number
/// is from a newer program: it is kept as it is and cannot be edited (`Shape::Unknown`).
pub const FORMAT: u32 = 1;
/// Smallest length tolerance in mm (see the module documentation).
pub const LENGTH_TOLERANCE: f64 = 0.01;
/// Length tolerance as a share of the size of the coordinates.
pub const RELATIVE_TOLERANCE: f64 = 1e-9;
/// The plane tolerance is this share of the length tolerance.
pub const PLANE_TOLERANCE_FACTOR: f64 = 0.25;

/// The factor for the four count limits below: always 1, unless the build has the cfg `kadakas_measure`
/// (set by `RUSTFLAGS` only, for the measurement of the limits) and a measurement sets another ([`set_limit_scale`]).
#[cfg(kadakas_measure)]
static LIMIT_SCALE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

/// Multiplies the limits of vertices, edges, faces and loop vertices by `n` for the whole
/// process, to measure what raised limits would cost. Only with the cfg `kadakas_measure`.
#[cfg(kadakas_measure)]
pub fn set_limit_scale(n: usize) {
    LIMIT_SCALE.store(n.max(1), std::sync::atomic::Ordering::Relaxed);
}

#[cfg(kadakas_measure)]
pub(crate) fn limit_scale() -> usize {
    LIMIT_SCALE.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(not(kadakas_measure))]
#[inline(always)]
pub(crate) const fn limit_scale() -> usize {
    1
}

/// Most vertices of one model.
///
/// Measured in `tests::perf_limits` (release build). A 300 x 300 grid of faces in one plane
/// (90,601 vertices, 180,600 edges) took 1.0 s for an edit in that plane and 1.1 s for the check of
/// each command, so the limits are about half of that. At a 200 x 200 grid (40,401 vertices,
/// 80,400 edges, 40,000 faces): an edit in the plane 0.26 s, an erase 0.10 s, the check of each
/// command 0.11 s, the full check 0.55 s, and 6.3 MB of text (156 bytes per vertex), far below
/// 32 MiB, the largest property that collaboration sends. Adding one box to 800 separate boxes
/// takes about 30 ms.
///
/// Raised on 2026-10-22 (from 50,000 vertices, 100,000 edges, 50,000 faces and 200,000 loop
/// vertices) after a new measurement, see `docs/modeler-limits.md`. A terrain at 99,500 faces:
/// the check of each command 0.18 s, the full check 1.26 s, 18.3 MB of text and one whole-model
/// update of 18.5 MB (the limit of one collaboration value is 32 MiB). The costs grow in line with
/// the size. Spiky meshes grow faster than that (the full check took 13.9 s at 100,000 faces).
pub const MAX_VERTICES: usize = 55_000;
/// Most edges of one model.
pub const MAX_EDGES: usize = 150_000;
/// Most faces of one model.
pub const MAX_FACES: usize = 100_000;
/// Most vertex uses in the loops of all faces together.
pub const MAX_LOOP_VERTICES: usize = 300_000;
/// Most edges that one edit may add.
pub const MAX_EDIT_EDGES: usize = 10_000;

macro_rules! id_type {
    ($name:ident, $what:literal) => {
        #[doc = concat!("The id of a ", $what, " in its surface model.")]
        #[derive(
            Clone,
            Copy,
            Debug,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
            schemars::JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(VertexId, "vertex");
id_type!(EdgeId, "edge");
id_type!(FaceId, "face");
id_type!(CurveId, "curve");

/// Free attributes of an edge or a face: soft, smooth, hidden, a material and so on (later rows).
/// The saved form keeps every key, also the keys that this version does not know, and writes them
/// back unchanged.
pub type Attrs = BTreeMap<String, serde_json::Value>;

/// A straight edge between two vertices. The order of the two is the order it was drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub v: [VertexId; 2],
}

impl Edge {
    /// The vertex pair in ascending order: the key of the edge.
    pub fn key(&self) -> (VertexId, VertexId) {
        key(self.v[0], self.v[1])
    }

    /// The other end, or `None` when `v` is not an end of this edge.
    pub fn other(&self, v: VertexId) -> Option<VertexId> {
        if self.v[0] == v {
            Some(self.v[1])
        } else if self.v[1] == v {
            Some(self.v[0])
        } else {
            None
        }
    }
}

pub(crate) fn key(a: VertexId, b: VertexId) -> (VertexId, VertexId) {
    if a <= b { (a, b) } else { (b, a) }
}

/// A planar face: the outer loop counterclockwise and the holes clockwise, seen from the front
/// (the side the normal of `plane` points to).
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub plane: Plane,
    pub outer: Vec<VertexId>,
    pub holes: Vec<Vec<VertexId>>,
    /// Free attributes (see [`Attrs`]). A part of a split face keeps them; a merged face takes
    /// the attributes of the face with the lower id.
    pub attrs: Attrs,
    /// Fields of the saved face that this version does not know. They are written back.
    pub extra: Attrs,
}

impl Face {
    /// A face with no attributes.
    pub fn new(plane: Plane, outer: Vec<VertexId>, holes: Vec<Vec<VertexId>>) -> Face {
        Face {
            plane,
            outer,
            holes,
            attrs: Attrs::new(),
            extra: Attrs::new(),
        }
    }

    /// The outer loop and then the holes.
    pub fn loops(&self) -> impl Iterator<Item = &Vec<VertexId>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }

    /// Every directed use of an edge, as (from, to), in loop order.
    pub fn uses(&self) -> impl Iterator<Item = (VertexId, VertexId)> + '_ {
        self.loops()
            .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
    }

    /// The number of vertex uses in all loops.
    pub fn loop_len(&self) -> usize {
        self.loops().map(Vec::len).sum()
    }
}

/// Why an edit failed. The model is unchanged after an error.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SurfaceError {
    #[error("a point is not finite or out of range")]
    BadPoint,
    #[error("an edge is shorter than the length tolerance ({tol} mm)")]
    TooShort { tol: f64 },
    #[error(
        "an edge passes closer than the length tolerance ({tol} mm) to other geometry without meeting it"
    )]
    TooClose { tol: f64 },
    #[error("edge {0} does not exist")]
    NoEdge(EdgeId),
    #[error("face {0} does not exist")]
    NoFace(FaceId),
    #[error("vertex {0} does not exist")]
    NoVertex(VertexId),
    #[error("a surface model has at most {max} {what}; the edit needs {count}")]
    Limit {
        what: &'static str,
        max: usize,
        count: usize,
    },
    #[error("the faces of a plane could not be solved: {0}")]
    Unsolved(String),
    /// An edit that would make a face collapse, a loop cross itself or the model invalid: the
    /// reason is the message (Move, Rotate and Scale).
    #[error("{0}")]
    Refused(String),
}

/// A surface model: vertices, edges and faces in local coordinates, placed by `transform`.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceModel {
    /// An optional name.
    pub name: String,
    /// From local coordinates to the drawing (mm; `z` from the layer elevation).
    pub transform: DAffine3,
    vertices: BTreeMap<VertexId, DVec3>,
    edges: BTreeMap<EdgeId, Edge>,
    faces: BTreeMap<FaceId, Face>,
    /// The next free id. Ids are never reused.
    next_id: u32,
    /// Attributes of edges (see [`Attrs`]); an edge without attributes has no entry.
    edge_attrs: BTreeMap<EdgeId, Attrs>,
    /// Curves: chains of edges that stay one arc or circle after they are cut (MOD-08). An edge is
    /// in one curve at most; a split keeps both parts in the curve, a heal joins them again.
    curves: BTreeMap<CurveId, Vec<EdgeId>>,
    /// The number of edits so far (each edit adds one).
    rev: u64,
    /// The recent edits as replayable intents (see [`sync`]).
    log: Vec<sync::Entry>,
    /// Fields of the saved model that this version does not know. They are written back.
    extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for SurfaceModel {
    fn default() -> Self {
        SurfaceModel::new()
    }
}

/// Triangles of one face, for drawing. Positions are local; the triangles run counterclockwise
/// seen from the front.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceTriangles {
    pub face: FaceId,
    /// The front normal.
    pub normal: DVec3,
    /// The vertices of the loops, outer loop first, then the holes.
    pub vertices: Vec<VertexId>,
    pub positions: Vec<DVec3>,
    /// Indices into `positions`.
    pub triangles: Vec<[u32; 3]>,
}

/// A triangle mesh of a whole model in local coordinates: one position per vertex of the model,
/// and for each triangle the face it comes from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceMesh {
    pub positions: Vec<DVec3>,
    /// The vertex id of each position.
    pub vertices: Vec<VertexId>,
    pub triangles: Vec<[u32; 3]>,
    pub faces: Vec<FaceId>,
}

impl SurfaceModel {
    /// An empty model with the identity transform.
    pub fn new() -> Self {
        SurfaceModel {
            name: String::new(),
            transform: DAffine3::IDENTITY,
            vertices: BTreeMap::new(),
            edges: BTreeMap::new(),
            faces: BTreeMap::new(),
            next_id: 1,
            edge_attrs: BTreeMap::new(),
            curves: BTreeMap::new(),
            rev: 0,
            log: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }

    /// Runs one edit. On an error the model is unchanged. An edit that changes the geometry adds
    /// one to the revision and goes into the log as an intent that can be replayed (see [`sync`]).
    pub fn apply(&mut self, edit: &Edit) -> Result<(), SurfaceError> {
        self.apply_with_notes(edit).map(|_| ())
    }

    /// Runs one edit like [`SurfaceModel::apply`] and returns its notes: what the edit changed
    /// that the person did not ask for, such as the attributes of a face that merged into
    /// another one (see the module `edit`). Empty for most edits.
    pub fn apply_with_notes(&mut self, edit: &Edit) -> Result<Vec<String>, SurfaceError> {
        let intent = sync::Intent::of(self, edit);
        let mut work = self.clone();
        let notes = edit::run(&mut work, edit, self)?;
        // An edit that changes nothing leaves the model as it was (also its id counter), so it
        // is no revision and no undo step.
        if work.same_geometry(self) {
            return Ok(notes);
        }
        sync::record(&mut work, self, intent);
        *self = work;
        Ok(notes)
    }

    /// True if the vertices, edges, faces, curves and attributes are the same.
    fn same_geometry(&self, o: &SurfaceModel) -> bool {
        self.vertices == o.vertices
            && self.edges == o.edges
            && self.faces == o.faces
            && self.edge_attrs == o.edge_attrs
            && self.curves == o.curves
    }

    /// The number of edits so far.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// The recent edits, oldest first.
    pub fn log(&self) -> &[sync::Entry] {
        &self.log
    }

    /// The attributes of an edge (empty when it has none).
    pub fn edge_attrs(&self, id: EdgeId) -> Option<&Attrs> {
        self.edge_attrs.get(&id)
    }

    /// True if the face is hidden (Hide, MOD-05).
    pub fn face_hidden(&self, id: FaceId) -> bool {
        self.faces
            .get(&id)
            .is_some_and(|f| f.attrs.get("hidden") == Some(&serde_json::Value::Bool(true)))
    }

    /// True if the edge is hidden (Hide, MOD-05).
    pub fn edge_hidden(&self, id: EdgeId) -> bool {
        self.edge_attrs
            .get(&id)
            .is_some_and(|a| a.get("hidden") == Some(&serde_json::Value::Bool(true)))
    }

    /// True if the edge is soft (MOD-08): not drawn, picked or inferred.
    pub fn edge_soft(&self, id: EdgeId) -> bool {
        flagged(self.edge_attrs.get(&id), ATTR_SOFT)
    }

    /// True if the edge is smooth (MOD-08): the faces on its sides shade as one surface.
    pub fn edge_smooth(&self, id: EdgeId) -> bool {
        flagged(self.edge_attrs.get(&id), ATTR_SMOOTH)
    }

    /// True if the edge is drawn, picked and inferred: neither hidden nor soft (View > Hidden
    /// Geometry shows the others ghosted).
    pub fn edge_shown(&self, id: EdgeId) -> bool {
        !self.edge_hidden(id) && !self.edge_soft(id)
    }

    /// The hidden faces, in the order of their ids.
    pub fn hidden_faces(&self) -> Vec<FaceId> {
        self.faces
            .keys()
            .copied()
            .filter(|id| self.face_hidden(*id))
            .collect()
    }

    /// The hidden edges, in the order of their ids.
    pub fn hidden_edges(&self) -> Vec<EdgeId> {
        self.edge_attrs
            .keys()
            .copied()
            .filter(|id| self.edge_hidden(*id))
            .collect()
    }

    /// The curves, each a chain of edges in order.
    pub fn curves(&self) -> impl Iterator<Item = (CurveId, &Vec<EdgeId>)> {
        self.curves.iter().map(|(k, v)| (*k, v))
    }

    /// The curve of an edge.
    pub fn curve_of(&self, e: EdgeId) -> Option<CurveId> {
        self.curves
            .iter()
            .find(|(_, es)| es.contains(&e))
            .map(|(id, _)| *id)
    }

    /// Makes a curve of a chain of edges (each one meets the next), for the tools that draw arcs
    /// and circles (MOD-08). Fails when an edge does not exist, is in a curve already, or the
    /// edges are not a chain.
    pub fn add_curve(&mut self, edges: Vec<EdgeId>) -> Result<CurveId, SurfaceError> {
        let mut work = self.clone();
        let id = CurveId(work.alloc());
        work.curves.insert(id, edges);
        if let Some(p) = check::curves(&work) {
            return Err(SurfaceError::Unsolved(p.to_string()));
        }
        sync::record(&mut work, self, sync::Intent::Unlogged);
        *self = work;
        Ok(id)
    }

    pub fn vertex(&self, id: VertexId) -> Option<DVec3> {
        self.vertices.get(&id).copied()
    }

    pub fn edge(&self, id: EdgeId) -> Option<Edge> {
        self.edges.get(&id).copied()
    }

    pub fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(&id)
    }

    /// The vertices in id order.
    pub fn vertices(&self) -> impl Iterator<Item = (VertexId, DVec3)> + '_ {
        self.vertices.iter().map(|(k, v)| (*k, *v))
    }

    /// The edges in id order.
    pub fn edges(&self) -> impl Iterator<Item = (EdgeId, Edge)> + '_ {
        self.edges.iter().map(|(k, v)| (*k, *v))
    }

    /// The faces in id order.
    pub fn faces(&self) -> impl Iterator<Item = (FaceId, &Face)> {
        self.faces.iter().map(|(k, v)| (*k, v))
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// The next id that an edit will give.
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// The edge between two vertices, if there is one.
    pub fn edge_between(&self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        let k = key(a, b);
        self.edges
            .iter()
            .find(|(_, e)| e.key() == k)
            .map(|(id, _)| *id)
    }

    /// The faces that use an edge, in id order.
    pub fn faces_of_edge(&self, id: EdgeId) -> Vec<FaceId> {
        let Some(e) = self.edges.get(&id) else {
            return Vec::new();
        };
        let k = e.key();
        self.faces
            .iter()
            .filter(|(_, f)| f.uses().any(|(a, b)| key(a, b) == k))
            .map(|(id, _)| *id)
            .collect()
    }

    /// The vertex positions of a loop.
    pub fn loop_points(&self, l: &[VertexId]) -> Vec<DVec3> {
        l.iter()
            .map(|v| self.vertices.get(v).copied().unwrap_or(DVec3::NAN))
            .collect()
    }

    /// Area of a face in mm² (the outer loop less the holes). `None` for a face that does not exist.
    pub fn face_area(&self, id: FaceId) -> Option<f64> {
        let f = self.faces.get(&id)?;
        Some(geom::face_area(f, |v| self.vertices[&v]))
    }

    /// The front normal of a face.
    pub fn face_normal(&self, id: FaceId) -> Option<DVec3> {
        self.faces.get(&id).map(|f| f.plane.normal)
    }

    /// The total area of all faces in mm².
    pub fn area(&self) -> f64 {
        self.faces
            .values()
            .map(|f| geom::face_area(f, |v| self.vertices[&v]))
            .sum()
    }

    /// The box around the vertices in local coordinates, or `None` for an empty model.
    pub fn local_bounds(&self) -> Option<(DVec3, DVec3)> {
        geom::bounds(self.vertices.values().copied())
    }

    /// The box around the vertices in drawing coordinates, or `None` for an empty model.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        geom::bounds(
            self.vertices
                .values()
                .map(|p| self.transform.transform_point3(*p)),
        )
    }

    /// The box of a face in local coordinates.
    pub fn face_bounds(&self, id: FaceId) -> Option<(DVec3, DVec3)> {
        let f = self.faces.get(&id)?;
        geom::bounds(f.outer.iter().map(|v| self.vertices[v]))
    }

    /// A point of the drawing in local coordinates. `None` when the transform cannot be inverted.
    pub fn to_local(&self, p: DVec3) -> Option<DVec3> {
        (self.transform.matrix3.determinant().abs() > 1e-12)
            .then(|| self.transform.inverse().transform_point3(p))
    }

    /// The loops that an Offset of `face` by `distance` would draw (outer loop first, local
    /// coordinates), for a preview; the error is the reason the edit would give (MOD-08).
    pub fn offset_outline(
        &self,
        face: FaceId,
        distance: f64,
        outward: bool,
    ) -> Result<Vec<Vec<DVec3>>, SurfaceError> {
        edit::offset::outline(self, face, distance, outward, true)
    }

    /// The line that an Offset of the chain `edges` by `distance` toward `toward` would draw
    /// (local coordinates) and whether it is closed, for a preview (MOD-08).
    pub fn offset_chain(
        &self,
        edges: &[EdgeId],
        distance: f64,
        toward: DVec3,
    ) -> Result<(Vec<DVec3>, bool), SurfaceError> {
        edit::offset::chain_line(self, edges, distance, toward, true).map(|(_, c, l)| (l, c))
    }

    /// The plane that an Offset of the chain `edges` toward `toward` works in (local), MOD-08.
    pub fn offset_chain_plane(
        &self,
        edges: &[EdgeId],
        toward: DVec3,
    ) -> Result<Plane, SurfaceError> {
        let (chain, _) = edit::offset::chain_of(self, edges)?;
        let pts = self.loop_points(&chain);
        edit::offset::chain_plane(self, edges, &pts, toward)
    }

    /// The triangles of one face (holes are left open).
    pub fn triangulate_face(&self, id: FaceId) -> Result<FaceTriangles, SurfaceError> {
        let f = self.faces.get(&id).ok_or(SurfaceError::NoFace(id))?;
        geom::triangulate_face(id, f, |v| self.vertices[&v]).ok_or_else(|| {
            SurfaceError::Unsolved(format!("face {id} cannot be cut into triangles"))
        })
    }

    /// The triangles of all faces, in face id order. Vertices are shared by id.
    pub fn mesh(&self) -> Result<SurfaceMesh, SurfaceError> {
        let index: BTreeMap<VertexId, u32> = self
            .vertices
            .keys()
            .enumerate()
            .map(|(i, v)| (*v, i as u32))
            .collect();
        let mut out = SurfaceMesh {
            positions: self.vertices.values().copied().collect(),
            vertices: self.vertices.keys().copied().collect(),
            ..SurfaceMesh::default()
        };
        for id in self.faces.keys() {
            let t = self.triangulate_face(*id)?;
            for tri in t.triangles {
                out.triangles
                    .push(tri.map(|i| index[&t.vertices[i as usize]]));
                out.faces.push(*id);
            }
        }
        Ok(out)
    }

    /// The full validity check: structure, planes, loop sides, distances, edges that pass through
    /// a face, coplanar faces that overlap, and the manifold state of every edge. It takes
    /// `O(n log n)` time (box trees for the faces).
    pub fn check(&self) -> Report {
        check::full(self)
    }

    /// The first problem that makes the model unusable, or `None`. The checks that run on every
    /// load and every change: ids, references, limits, finite numbers, planes and loop sides
    /// (linear time). [`SurfaceModel::check`] also looks at distances.
    pub fn problem(&self) -> Option<String> {
        check::quick(self).map(|p| p.to_string())
    }

    /// True if every number is finite.
    pub fn is_finite(&self) -> bool {
        self.transform.is_finite()
            && self.vertices.values().all(|p| p.is_finite())
            && self
                .faces
                .values()
                .all(|f| f.plane.normal.is_finite() && f.plane.offset.is_finite())
    }

    /// Calls `f` with every coordinate and the numbers of the transform.
    pub fn for_each_number(&self, f: &mut dyn FnMut(f64)) {
        self.transform.to_cols_array().into_iter().for_each(&mut *f);
        for p in self.vertices.values() {
            p.to_array().into_iter().for_each(&mut *f);
        }
        if let Some((lo, hi)) = self.bounds() {
            lo.to_array()
                .into_iter()
                .chain(hi.to_array())
                .for_each(&mut *f);
        }
    }

    /// Moves the model in the drawing, in 3D.
    pub fn translate(&mut self, d: DVec3) {
        if d != DVec3::ZERO {
            self.place(DAffine3::from_translation(d) * self.transform);
        }
    }

    /// Places the model with a new transform. The change goes into the log, so that a concurrent
    /// edit of the geometry by another user can be replayed with it (see [`sync`]).
    pub fn place(&mut self, transform: DAffine3) {
        if transform != self.transform {
            let before = self.clone();
            self.transform = transform;
            sync::record(
                self,
                &before,
                sync::Intent::Place {
                    transform: transform.to_cols_array(),
                },
            );
        }
    }

    /// Places the model again after a change of the map (a CRS change): its origin goes to `origin`,
    /// the plan axes x and y become `dx` and `dy` (in drawing units per local unit), and heights are
    /// scaled by `z_scale`.
    pub fn remap_plan(&mut self, origin: DVec3, dx: DVec2, dy: DVec2, z_scale: f64) {
        let map = glam::DMat3::from_cols(dx.extend(0.0), dy.extend(0.0), DVec3::Z * z_scale);
        self.place(DAffine3 {
            matrix3: map * self.transform.matrix3,
            translation: origin,
        });
    }

    /// A rough size in memory, for the bytes of the undo history.
    pub fn approx_bytes(&self) -> u64 {
        (self.vertices.len() * 64 + self.edges.len() * 48) as u64
            + self
                .faces
                .values()
                .map(|f| 96 + 4 * f.loop_len() as u64)
                .sum::<u64>()
    }

    /// Builds a model from its parts, as a file has them. Nothing is checked; call
    /// [`SurfaceModel::problem`] or [`SurfaceModel::check`].
    pub fn from_parts(
        vertices: BTreeMap<VertexId, DVec3>,
        edges: BTreeMap<EdgeId, Edge>,
        faces: BTreeMap<FaceId, Face>,
        next_id: u32,
    ) -> Self {
        SurfaceModel {
            name: String::new(),
            transform: DAffine3::IDENTITY,
            vertices,
            edges,
            faces,
            next_id,
            edge_attrs: BTreeMap::new(),
            curves: BTreeMap::new(),
            rev: 0,
            log: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }

    fn alloc(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// True if `raw` is the JSON of a surface model from a newer format (a higher `v`). Such a shape is
/// kept as it is (`Shape::Unknown`).
pub fn is_newer(raw: &serde_json::Value) -> bool {
    raw.get("kind").and_then(serde_json::Value::as_str) == Some("surface_model")
        && raw
            .get("v")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|v| v > u64::from(FORMAT))
}

/// Projects 3D points into the 2D basis of a plane (for callers that draw a face in its plane).
pub fn project(plane: &Plane, p: DVec3) -> DVec2 {
    plane.project(p)
}
