//! The derived index of a surface model: the edge of each vertex pair, the edges at each vertex
//! and the faces that use each edge (the radial list). It is built when an edit starts and kept in
//! step with the model during the edit; it is never saved.

use std::collections::{BTreeMap, BTreeSet};

use super::{Edge, EdgeId, Face, FaceId, SurfaceModel, VertexId, key};

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Topo {
    pair: BTreeMap<(VertexId, VertexId), EdgeId>,
    at: BTreeMap<VertexId, BTreeSet<EdgeId>>,
    uses: BTreeMap<EdgeId, BTreeSet<FaceId>>,
}

impl Topo {
    pub fn of(m: &SurfaceModel) -> Topo {
        let mut t = Topo::default();
        for (id, e) in &m.edges {
            t.add_edge(*id, *e);
        }
        for (id, f) in &m.faces {
            t.add_face(*id, f);
        }
        t
    }

    pub fn add_edge(&mut self, id: EdgeId, e: Edge) {
        self.pair.insert(e.key(), id);
        for v in e.v {
            self.at.entry(v).or_default().insert(id);
        }
    }

    pub fn remove_edge(&mut self, id: EdgeId, e: Edge) {
        self.pair.remove(&e.key());
        for v in e.v {
            if let Some(s) = self.at.get_mut(&v) {
                s.remove(&id);
                if s.is_empty() {
                    self.at.remove(&v);
                }
            }
        }
        self.uses.remove(&id);
    }

    /// Records the uses of a face. Every pair of neighbours of its loops must be an edge.
    pub fn add_face(&mut self, id: FaceId, f: &Face) {
        for (a, b) in f.uses() {
            if let Some(e) = self.edge(a, b) {
                self.uses.entry(e).or_default().insert(id);
            }
        }
    }

    pub fn remove_face(&mut self, id: FaceId, f: &Face) {
        for (a, b) in f.uses() {
            if let Some(e) = self.edge(a, b)
                && let Some(s) = self.uses.get_mut(&e)
            {
                s.remove(&id);
                if s.is_empty() {
                    self.uses.remove(&e);
                }
            }
        }
    }

    /// The edge between two vertices.
    pub fn edge(&self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        self.pair.get(&key(a, b)).copied()
    }

    /// The edges at a vertex, in id order.
    pub fn edges_at(&self, v: VertexId) -> impl Iterator<Item = EdgeId> + '_ {
        self.at.get(&v).into_iter().flatten().copied()
    }

    pub fn degree(&self, v: VertexId) -> usize {
        self.at.get(&v).map_or(0, BTreeSet::len)
    }

    /// The faces that use an edge, in id order.
    pub fn faces_of(&self, e: EdgeId) -> impl Iterator<Item = FaceId> + '_ {
        self.uses.get(&e).into_iter().flatten().copied()
    }
}
