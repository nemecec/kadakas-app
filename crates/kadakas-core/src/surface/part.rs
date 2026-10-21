//! Taking a part out of a surface model and putting a model into another one (groups and
//! components, MOD-07). The work is done with the edits of the model, so the rules of MOD-06
//! hold: a part that is put back welds, splits and makes faces as a drawing of its edges would.
//!
//! - [`SurfaceModel::select`] turns faces and edges into a [`Selection`]: the edges of the faces
//!   go with them, and a curve goes whole or not at all.
//! - [`SurfaceModel::extract`] copies the selection into a new model with the same ids and the
//!   same placement. [`SurfaceModel::remove`] takes it out of the model: the faces go, and so do
//!   the edges that no face outside the selection uses. An edge that a remaining face uses stays,
//!   so the rest of the model keeps its faces (the edge is then in both models).
//! - [`SurfaceModel::absorb`] draws all edges of another model into this one (the other model
//!   placed by `xf`), then gives the faces and edges that formed the attributes and the side of
//!   the faces they came from. When several faces land on one face, the one with the larger area
//!   decides (equal areas: the lower id), for the side and for the attributes it sets. A closed loop of coplanar edges gets a face, as always in MOD-06,
//!   also when the other model had erased that face. Curves are kept when all their edges are
//!   found again as whole edges; an edge that the geometry it met has cut takes its curve
//!   identity no longer (the curve tools are MOD-08).

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine3, DVec3};

use super::edit::Session;
use super::geom::tol;
use super::sync::{self, Intent};
use super::{
    Attrs, Edge, EdgeId, Edit, FaceId, MAX_EDIT_EDGES, SurfaceError, SurfaceModel, VertexId,
};

/// Faces and edges chosen in a model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub faces: BTreeSet<FaceId>,
    /// The chosen edges, the edges of the chosen faces, and the edges of the curves among them.
    pub edges: BTreeSet<EdgeId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty() && self.edges.is_empty()
    }
}

impl SurfaceModel {
    /// Checks the ids and adds what goes with them (see the module documentation).
    pub fn select(&self, faces: &[FaceId], edges: &[EdgeId]) -> Result<Selection, SurfaceError> {
        let mut sel = Selection::default();
        for &f in faces {
            let face = self.faces.get(&f).ok_or(SurfaceError::NoFace(f))?;
            sel.faces.insert(f);
            for (a, b) in face.uses() {
                if let Some(e) = self.edge_between(a, b) {
                    sel.edges.insert(e);
                }
            }
        }
        for &e in edges {
            if !self.edges.contains_key(&e) {
                return Err(SurfaceError::NoEdge(e));
            }
            sel.edges.insert(e);
        }
        for es in self.curves.values() {
            if es.iter().any(|e| sel.edges.contains(e)) {
                sel.edges.extend(es.iter().copied());
            }
        }
        Ok(sel)
    }

    /// True if the selection holds every face and every edge of the model.
    pub fn is_whole(&self, sel: &Selection) -> bool {
        sel.faces.len() == self.faces.len() && sel.edges.len() == self.edges.len()
    }

    /// A copy of the selected part: the same ids, attributes, curves (those with all edges chosen)
    /// and placement; `next_id` of the model, so an id is never used twice by the two models. The
    /// revision and the log start again.
    pub fn extract(&self, sel: &Selection) -> SurfaceModel {
        let edges: BTreeMap<EdgeId, Edge> = sel
            .edges
            .iter()
            .filter_map(|e| self.edges.get(e).map(|x| (*e, *x)))
            .collect();
        let used: BTreeSet<VertexId> = edges.values().flat_map(|e| e.v).collect();
        let mut m = SurfaceModel::from_parts(
            self.vertices
                .iter()
                .filter(|(v, _)| used.contains(v))
                .map(|(v, p)| (*v, *p))
                .collect(),
            edges,
            self.faces
                .iter()
                .filter(|(f, _)| sel.faces.contains(f))
                .map(|(f, x)| (*f, x.clone()))
                .collect(),
            self.next_id,
        );
        m.name = self.name.clone();
        m.transform = self.transform;
        m.edge_attrs = self
            .edge_attrs
            .iter()
            .filter(|(e, _)| sel.edges.contains(e))
            .map(|(e, a)| (*e, a.clone()))
            .collect();
        m.curves = self
            .curves
            .iter()
            .filter(|(_, es)| es.iter().all(|e| sel.edges.contains(e)))
            .map(|(c, es)| (*c, es.clone()))
            .collect();
        m
    }

    /// Takes the selection out: the faces go, and the chosen edges that no other face uses.
    /// Up to two edits of the model (see [`SurfaceModel::apply`]); on an error the model is
    /// unchanged.
    pub fn remove(&mut self, sel: &Selection) -> Result<(), SurfaceError> {
        let mut work = self.clone();
        if !sel.faces.is_empty() {
            work.apply(&Edit::EraseFaces {
                faces: sel.faces.iter().copied().collect(),
            })?;
        }
        let gone: Vec<EdgeId> = sel
            .edges
            .iter()
            .copied()
            .filter(|e| work.edges.contains_key(e) && work.faces_of_edge(*e).is_empty())
            .collect();
        if !gone.is_empty() {
            work.apply(&Edit::EraseEdges { edges: gone })?;
        }
        *self = work;
        Ok(())
    }

    /// Draws all edges of `other` into this model, `other` placed by `xf` (in the local
    /// coordinates of this model; `None`: as they are). See the module documentation. On an error
    /// the model is unchanged.
    pub fn absorb(
        &mut self,
        other: &SurfaceModel,
        xf: Option<DAffine3>,
    ) -> Result<(), SurfaceError> {
        let mut a = Absorber::new(self.clone());
        a.absorb(other, xf)?;
        *self = a.finish();
        Ok(())
    }

    /// The plain version of [`SurfaceModel::absorb`], one `apply` per step, that the version with
    /// a session is compared with. Draws all edges of `other` into this model, `other` placed by `xf` (in the local
    /// coordinates of this model; `None`: as they are). See the module documentation. On an error
    /// the model is unchanged.
    #[cfg(test)]
    pub(super) fn absorb_reference(
        &mut self,
        other: &SurfaceModel,
        xf: Option<DAffine3>,
    ) -> Result<(), SurfaceError> {
        let map = |p: DVec3| xf.map_or(p, |x| x.transform_point3(p));
        let ends = |e: &Edge| -> Option<[DVec3; 2]> {
            Some([
                map(*other.vertices.get(&e.v[0])?),
                map(*other.vertices.get(&e.v[1])?),
            ])
        };
        let edges: Vec<[[f64; 3]; 2]> = other
            .edges
            .values()
            .filter_map(ends)
            .map(|[a, b]| [a.to_array(), b.to_array()])
            .collect();
        let mut work = self.clone();
        for chunk in edges.chunks(MAX_EDIT_EDGES) {
            work.apply(&Edit::AddEdges {
                edges: chunk.to_vec(),
            })?;
        }
        // The side of the faces that formed, and the attributes of faces and edges.
        let normal_of = |n: DVec3| xf.map_or(n, |x| x.matrix3.inverse().transpose() * n);
        // Several faces of `other` can land on one face of `work` (they were split or merged by
        // the geometry they met). The face that decides the side and wins the attributes of the
        // result is the one with the larger area, and with equal areas the one with the lower id
        // (`decide`). The rule does not depend on the order of the faces. A face of `other` that
        // is much smaller than the face it lands on, when that face was already in this model,
        // changes neither its side nor its attributes.
        let scale = xf.map_or(1.0, |x| x.matrix3.determinant().abs().powf(2.0 / 3.0));
        let mut landed: BTreeMap<FaceId, Vec<Landed>> = BTreeMap::new();
        for (id, f) in &other.faces {
            let Some(nf) = sync::face_point(other, *id).and_then(|p| sync::face_at(&work, map(p)))
            else {
                continue;
            };
            landed.entry(nf).or_default().push(Landed {
                id: *id,
                area: other.face_area(*id).unwrap_or(0.0) * scale,
                flip: work.faces[&nf].plane.normal.dot(normal_of(f.plane.normal)) < 0.0,
                attrs: &f.attrs,
            });
        }
        let mut reverse: Vec<FaceId> = Vec::new();
        let mut face_attrs: Vec<(FaceId, Vec<&Attrs>)> = Vec::new();
        for (nf, sources) in landed {
            let existing = self.faces.contains_key(&nf);
            let area = work.face_area(nf).unwrap_or(0.0);
            let ordered = decide(sources, existing.then_some(area));
            let Some(winner) = ordered.last() else {
                continue;
            };
            if winner.flip {
                reverse.push(nf);
            }
            let list: Vec<&Attrs> = ordered
                .iter()
                .map(|s| s.attrs)
                .filter(|a| !a.is_empty())
                .collect();
            if !list.is_empty() {
                face_attrs.push((nf, list));
            }
        }
        if !reverse.is_empty() {
            work.apply(&Edit::ReverseFaces { faces: reverse })?;
        }
        // The edge of `work` that is the whole of an edge of `other`.
        let find = |m: &SurfaceModel, e: EdgeId| -> Option<EdgeId> {
            let [a, b] = ends(other.edges.get(&e)?)?;
            let id = sync::edge_at(m, (a + b) / 2.0)?;
            let found = m.edges[&id];
            let (pa, pb) = (m.vertices[&found.v[0]], m.vertices[&found.v[1]]);
            let near = |p: DVec3, q: DVec3| p.distance(q) <= tol(&[p, q]);
            ((near(pa, a) && near(pb, b)) || (near(pa, b) && near(pb, a))).then_some(id)
        };
        let before = work.clone();
        let mut changed = false;
        for (nf, list) in face_attrs {
            let f = work.faces.get_mut(&nf).expect("found above");
            for attrs in list {
                for (k, v) in attrs.iter() {
                    changed |= f.attrs.insert(k.clone(), v.clone()).as_ref() != Some(v);
                }
            }
        }
        for (e, attrs) in &other.edge_attrs {
            if let Some(id) = find(&work, *e) {
                let slot = work.edge_attrs.entry(id).or_default();
                for (k, v) in attrs {
                    changed |= slot.insert(k.clone(), v.clone()).as_ref() != Some(v);
                }
            }
        }
        if changed {
            sync::record(&mut work, &before, Intent::Unlogged);
        }
        for es in other.curves.values() {
            let mapped: Option<Vec<EdgeId>> = es.iter().map(|e| find(&work, *e)).collect();
            if let Some(list) = mapped {
                // A chain that is not a chain any more (the edge was cut) is not a curve.
                let _ = work.add_curve(list);
            }
        }
        *self = work;
        Ok(())
    }
}

/// A model that takes many models one after another ([`SurfaceModel::absorb`]) and keeps its
/// indexes between them. The result is the same to the bit as that of one `absorb` after the
/// other (a test compares the two); the cost of each is that of the model that is taken in and
/// not that of the whole result. The explode of a component with thousands of parts uses it.
/// After an error it is not to be used again.
pub struct Absorber {
    s: Session,
}

impl Absorber {
    pub fn new(m: SurfaceModel) -> Absorber {
        Absorber { s: Session::new(m) }
    }

    pub fn model(&self) -> &SurfaceModel {
        self.s.model()
    }

    pub fn finish(self) -> SurfaceModel {
        self.s.into_model()
    }

    /// Like [`SurfaceModel::absorb`] on the model of this absorber.
    pub fn absorb(
        &mut self,
        other: &SurfaceModel,
        xf: Option<DAffine3>,
    ) -> Result<(), SurfaceError> {
        let map = |p: DVec3| xf.map_or(p, |x| x.transform_point3(p));
        let ends = |e: &Edge| -> Option<[DVec3; 2]> {
            Some([
                map(*other.vertices.get(&e.v[0])?),
                map(*other.vertices.get(&e.v[1])?),
            ])
        };
        let edges: Vec<[[f64; 3]; 2]> = other
            .edges
            .values()
            .filter_map(ends)
            .map(|[a, b]| [a.to_array(), b.to_array()])
            .collect();
        // The faces that were in the model before are those with an id below this one.
        let first_new = self.s.model().next_id;
        for chunk in edges.chunks(MAX_EDIT_EDGES) {
            self.s.add_edges(chunk)?;
        }
        let normal_of = |n: DVec3| xf.map_or(n, |x| x.matrix3.inverse().transpose() * n);
        // Several faces of `other` can land on one face of the result (see `absorb_reference`).
        let scale = xf.map_or(1.0, |x| x.matrix3.determinant().abs().powf(2.0 / 3.0));
        let mut landed: BTreeMap<FaceId, Vec<Landed>> = BTreeMap::new();
        for (id, f) in &other.faces {
            let Some(nf) = sync::face_point(other, *id).and_then(|p| self.s.face_at(map(p))) else {
                continue;
            };
            landed.entry(nf).or_default().push(Landed {
                id: *id,
                area: other.face_area(*id).unwrap_or(0.0) * scale,
                flip: self.s.model().faces[&nf]
                    .plane
                    .normal
                    .dot(normal_of(f.plane.normal))
                    < 0.0,
                attrs: &f.attrs,
            });
        }
        let mut reverse: Vec<FaceId> = Vec::new();
        let mut face_attrs: Vec<(FaceId, Vec<&Attrs>)> = Vec::new();
        for (nf, sources) in landed {
            let existing = nf.0 < first_new;
            let area = self.s.model().face_area(nf).unwrap_or(0.0);
            let ordered = decide(sources, existing.then_some(area));
            let Some(winner) = ordered.last() else {
                continue;
            };
            if winner.flip {
                reverse.push(nf);
            }
            let list: Vec<&Attrs> = ordered
                .iter()
                .map(|s| s.attrs)
                .filter(|a| !a.is_empty())
                .collect();
            if !list.is_empty() {
                face_attrs.push((nf, list));
            }
        }
        if !reverse.is_empty() {
            self.s.reverse_faces(reverse)?;
        }
        // The edge of the result that is the whole of an edge of `other`.
        let find = |s: &Session, e: EdgeId| -> Option<EdgeId> {
            let [a, b] = ends(other.edges.get(&e)?)?;
            let id = s.edge_at((a + b) / 2.0)?;
            let m = s.model();
            let found = m.edges[&id];
            let (pa, pb) = (m.vertices[&found.v[0]], m.vertices[&found.v[1]]);
            let near = |p: DVec3, q: DVec3| p.distance(q) <= tol(&[p, q]);
            ((near(pa, a) && near(pb, b)) || (near(pa, b) && near(pb, a))).then_some(id)
        };
        // The attributes change no geometry. The log gets one entry for them, as `absorb` does.
        let base = self
            .s
            .model()
            .log
            .is_empty()
            .then(|| self.s.model().clone());
        let mut changed = false;
        for (nf, list) in face_attrs {
            let f = self.s.model_mut().faces.get_mut(&nf).expect("found above");
            for attrs in list {
                for (k, v) in attrs.iter() {
                    changed |= f.attrs.insert(k.clone(), v.clone()).as_ref() != Some(v);
                }
            }
        }
        for (e, attrs) in &other.edge_attrs {
            if let Some(id) = find(&self.s, *e) {
                let slot = self.s.model_mut().edge_attrs.entry(id).or_default();
                for (k, v) in attrs {
                    changed |= slot.insert(k.clone(), v.clone()).as_ref() != Some(v);
                }
            }
        }
        if changed {
            sync::record_in_place(self.s.model_mut(), base.as_ref(), Intent::Unlogged);
        }
        for es in other.curves.values() {
            let mapped: Option<Vec<EdgeId>> = es.iter().map(|e| find(&self.s, *e)).collect();
            if let Some(list) = mapped {
                // A chain that is not a chain any more (the edge was cut) is not a curve.
                let _ = self.s.model_mut().add_curve(list);
            }
        }
        Ok(())
    }
}

/// A face of the absorbed model that lands on a face of the result.
struct Landed<'a> {
    id: FaceId,
    /// In the units of the result.
    area: f64,
    /// Its side is the other way round from the side of the face it landed on.
    flip: bool,
    attrs: &'a Attrs,
}

/// Which faces that landed on one face count, in the order of their weight: the last one decides
/// the side and its attributes win. Larger area first wins, equal areas: the lower id.
/// `existing` is the area of the face that was already in the model, if it was: a face less than
/// half as large as that does not count at all.
fn decide(mut sources: Vec<Landed>, existing: Option<f64>) -> Vec<Landed> {
    if let Some(area) = existing {
        sources.retain(|s| s.area >= 0.5 * area);
    }
    sources.sort_by(|a, b| a.area.total_cmp(&b.area).then(b.id.cmp(&a.id)));
    sources
}

#[cfg(test)]
mod decide_tests {
    use super::*;

    static NONE: std::sync::LazyLock<Attrs> = std::sync::LazyLock::new(Attrs::default);

    fn l(id: u32, area: f64, flip: bool) -> Landed<'static> {
        Landed {
            id: FaceId(id),
            area,
            flip,
            attrs: &NONE,
        }
    }

    fn winner(v: Vec<Landed>, existing: Option<f64>) -> Option<(u32, bool)> {
        decide(v, existing).last().map(|s| (s.id.0, s.flip))
    }

    #[test]
    fn the_larger_face_decides_in_any_order() {
        let a = || l(1, 10.0, true);
        let b = || l(2, 90.0, false);
        assert_eq!(winner(vec![a(), b()], None), Some((2, false)));
        assert_eq!(winner(vec![b(), a()], None), Some((2, false)));
        // Equal areas: the lower id.
        let c = || l(7, 5.0, true);
        let d = || l(3, 5.0, false);
        assert_eq!(winner(vec![c(), d()], None), Some((3, false)));
        assert_eq!(winner(vec![d(), c()], None), Some((3, false)));
    }

    #[test]
    fn a_small_face_does_not_change_a_face_that_was_in_the_model() {
        // The face of the model has area 100. A face of 10 that lands on it is ignored; one of
        // 60 counts, and the larger of two still wins.
        assert_eq!(winner(vec![l(1, 10.0, true)], Some(100.0)), None);
        assert_eq!(winner(vec![l(1, 60.0, true)], Some(100.0)), Some((1, true)));
        assert_eq!(
            winner(vec![l(1, 60.0, true), l(2, 10.0, false)], Some(100.0)),
            Some((1, true))
        );
        assert_eq!(
            winner(vec![l(2, 10.0, false), l(1, 60.0, true)], Some(100.0)),
            Some((1, true))
        );
        // A face that is new in the result takes any size.
        assert_eq!(winner(vec![l(1, 10.0, true)], None), Some((1, true)));
    }
}
