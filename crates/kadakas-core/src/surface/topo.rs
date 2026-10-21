//! The derived index of a surface model: the edge of each vertex pair, the edges at each vertex
//! and the faces that use each edge (the radial list). It is built when an edit starts and kept in
//! step with the model during the edit; it is never saved.
//!
//! The lists at a vertex and at an edge are short, so they are sorted vectors (a set each, in id
//! order); the whole index is built from sorted lists at once, which is several times faster than
//! inserting one item at a time.

use std::collections::BTreeMap;

use super::{Edge, EdgeId, Face, FaceId, SurfaceModel, VertexId, key};

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Topo {
    pair: BTreeMap<(VertexId, VertexId), EdgeId>,
    at: BTreeMap<VertexId, Vec<EdgeId>>,
    uses: BTreeMap<EdgeId, Vec<FaceId>>,
}

/// Puts `x` into the sorted list (once).
fn put<T: Ord>(list: &mut Vec<T>, x: T) {
    if let Err(i) = list.binary_search(&x) {
        list.insert(i, x);
    }
}

/// Takes `x` out of the sorted list; true if the list is empty then.
fn take<T: Ord>(list: &mut Vec<T>, x: &T) -> bool {
    if let Ok(i) = list.binary_search(x) {
        list.remove(i);
    }
    list.is_empty()
}

/// The map of sorted lists of sorted pairs (each pair once).
fn grouped<K: Ord + Copy, V: Ord + Copy>(mut pairs: Vec<(K, V)>) -> BTreeMap<K, Vec<V>> {
    pairs.sort_unstable();
    pairs.dedup();
    pairs
        .chunk_by(|a, b| a.0 == b.0)
        .map(|run| (run[0].0, run.iter().map(|p| p.1).collect()))
        .collect()
}

impl Topo {
    pub fn of(m: &SurfaceModel) -> Topo {
        // Of two edges with the same pair (not in a valid model) the later one counts, as when
        // they are added one at a time.
        let pair: BTreeMap<(VertexId, VertexId), EdgeId> =
            m.edges.iter().map(|(id, e)| (e.key(), *id)).collect();
        let at = grouped(
            m.edges
                .iter()
                .flat_map(|(id, e)| e.v.map(|v| (v, *id)))
                .collect(),
        );
        let mut uses = Vec::new();
        for (id, f) in &m.faces {
            for (a, b) in f.uses() {
                if let Some(e) = pair.get(&key(a, b)) {
                    uses.push((*e, *id));
                }
            }
        }
        Topo {
            pair,
            at,
            uses: grouped(uses),
        }
    }

    /// The index built one edge and one face at a time ([`Topo::of`] must give the same).
    #[cfg(test)]
    pub fn of_one_by_one(m: &SurfaceModel) -> Topo {
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
            put(self.at.entry(v).or_default(), id);
        }
    }

    pub fn remove_edge(&mut self, id: EdgeId, e: Edge) {
        self.pair.remove(&e.key());
        for v in e.v {
            if let Some(s) = self.at.get_mut(&v)
                && take(s, &id)
            {
                self.at.remove(&v);
            }
        }
        self.uses.remove(&id);
    }

    /// Records the uses of a face. Every pair of neighbours of its loops must be an edge.
    pub fn add_face(&mut self, id: FaceId, f: &Face) {
        for (a, b) in f.uses() {
            if let Some(e) = self.edge(a, b) {
                put(self.uses.entry(e).or_default(), id);
            }
        }
    }

    pub fn remove_face(&mut self, id: FaceId, f: &Face) {
        for (a, b) in f.uses() {
            if let Some(e) = self.edge(a, b)
                && let Some(s) = self.uses.get_mut(&e)
                && take(s, &id)
            {
                self.uses.remove(&e);
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
        self.at.get(&v).map_or(0, Vec::len)
    }

    /// The faces that use an edge, in id order.
    pub fn faces_of(&self, e: EdgeId) -> impl Iterator<Item = FaceId> + '_ {
        self.uses.get(&e).into_iter().flatten().copied()
    }
}
