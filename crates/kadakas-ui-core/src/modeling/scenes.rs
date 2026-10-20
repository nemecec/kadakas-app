//! The geometry the tools work on: the inference scene of the surface models of the document, and
//! the lookup from a tag of the scene to a model object of the document.
//!
//! The scene is rebuilt only when the document revision changes ([`SceneCache::get`]), never every
//! frame. `Scenes::scene` is the one place that gives the scene to the inference. When the
//! inference takes one scene per model, this holder keeps one per object id and returns the list.

use std::collections::BTreeSet;

use glam::{DAffine3, DVec3};
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::surface::{CurveId, EdgeId, FaceId, SurfaceModel};

use crate::infer3d::{Scene, SceneBuilder, Tag};

/// The elevation of the layer of an object (mm). Surface models are measured from it.
pub fn elevation(doc: &Document, id: ObjectId) -> f64 {
    doc.object(id)
        .and_then(|o| doc.layer(o.layer))
        .map_or(0.0, |l| l.elevation)
}

/// The model of an object, if it is a surface model.
pub fn model_of(doc: &Document, id: ObjectId) -> Option<&SurfaceModel> {
    match &doc.object(id)?.shape {
        Shape::SurfaceModel(m) => Some(m),
        _ => None,
    }
}

/// From the local coordinates of a model to the world (drawing) coordinates, with the elevation
/// of its layer.
pub fn frame_of(doc: &Document, id: ObjectId) -> DAffine3 {
    let t = model_of(doc, id).map_or(DAffine3::IDENTITY, |m| m.transform);
    DAffine3::from_translation(DVec3::new(0.0, 0.0, elevation(doc, id))) * t
}

/// The inference scene of a document and the numbers it uses for the models.
pub struct Scenes {
    ids: Vec<ObjectId>,
    scene: Scene,
    /// The document revision the scene was built from.
    pub rev: u64,
}

impl Scenes {
    /// Builds the scene of all visible surface models.
    pub fn build(doc: &Document, rev: u64) -> Scenes {
        let mut b = SceneBuilder::new();
        let mut ids = Vec::new();
        for o in doc.objects.values() {
            let Shape::SurfaceModel(m) = &o.shape else {
                continue;
            };
            if !doc.is_visible(o) {
                continue;
            }
            let number = ids.len() as u32;
            let elev = elevation(doc, o.id);
            if elev == 0.0 {
                b.add_surface_model(m, number);
            } else {
                let mut moved = (**m).clone();
                moved.transform =
                    DAffine3::from_translation(DVec3::new(0.0, 0.0, elev)) * moved.transform;
                b.add_surface_model(&moved, number);
            }
            ids.push(o.id);
        }
        Scenes {
            ids,
            scene: b.finish(),
            rev,
        }
    }

    /// The scene for the inference.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The object of a tag of the scene.
    pub fn object(&self, tag: Tag) -> Option<ObjectId> {
        if tag == Tag::NONE {
            return None;
        }
        self.ids.get(tag.model() as usize).copied()
    }

    /// The objects in the scene, in the order of their numbers.
    pub fn objects(&self) -> &[ObjectId] {
        &self.ids
    }
}

/// Keeps the scene of the last revision.
#[derive(Default)]
pub struct SceneCache {
    scenes: Option<Scenes>,
    /// How many times the scene was built.
    pub builds: u64,
}

impl SceneCache {
    /// The scene for the revision `rev` of the document: the one that is kept, or a new one when
    /// the revision changed.
    pub fn get(&mut self, doc: &Document, rev: u64) -> &Scenes {
        if self.scenes.as_ref().is_none_or(|s| s.rev != rev) {
            self.scenes = Some(Scenes::build(doc, rev));
            self.builds += 1;
        }
        self.scenes.as_ref().expect("just built")
    }

    /// Forgets the scene (a new document has other revisions that can repeat).
    pub fn clear(&mut self) {
        self.scenes = None;
    }
}

/// What the tools ask about the geometry. The document and the scene of the same revision.
#[derive(Clone, Copy)]
pub struct World<'a> {
    pub doc: &'a Document,
    pub scenes: &'a Scenes,
}

impl World<'_> {
    /// The model object and the model of a tag.
    pub fn model(&self, tag: Tag) -> Option<(ObjectId, &SurfaceModel)> {
        let id = self.scenes.object(tag)?;
        Some((id, model_of(self.doc, id)?))
    }

    /// The height of the ground plane for new geometry: the elevation of the active layer.
    pub fn ground_z(&self) -> f64 {
        self.doc
            .layer(self.doc.active_layer)
            .map_or(0.0, |l| l.elevation)
    }

    /// The plane of a face tag in world coordinates, with its front normal.
    pub fn face_plane(&self, tag: Tag) -> Option<super::Plane3> {
        let (id, m) = self.model(tag)?;
        let f = m.face(FaceId(tag.id()))?;
        let fr = frame_of(self.doc, id);
        let p0 = fr.transform_point3(m.vertex(*f.outer.first()?)?);
        let n = fr.matrix3.inverse().transpose() * f.plane.normal;
        super::Plane3::through(p0, n)
    }

    /// The loops of a face in world coordinates (outer first).
    pub fn face_loops(&self, id: ObjectId, face: FaceId) -> Vec<Vec<DVec3>> {
        let Some(m) = model_of(self.doc, id) else {
            return Vec::new();
        };
        let fr = frame_of(self.doc, id);
        m.face(face).map_or_else(Vec::new, |f| {
            f.loops()
                .map(|l| {
                    l.iter()
                        .filter_map(|v| m.vertex(*v).map(|p| fr.transform_point3(p)))
                        .collect()
                })
                .collect()
        })
    }

    /// The ends of an edge in world coordinates.
    pub fn edge_ends(&self, id: ObjectId, edge: EdgeId) -> Option<[DVec3; 2]> {
        let m = model_of(self.doc, id)?;
        let e = m.edge(edge)?;
        let fr = frame_of(self.doc, id);
        Some([
            fr.transform_point3(m.vertex(e.v[0])?),
            fr.transform_point3(m.vertex(e.v[1])?),
        ])
    }

    /// The edges that go with an edge when it is erased: the whole curve if the edge is in one.
    pub fn with_curve(&self, id: ObjectId, edge: EdgeId) -> BTreeSet<EdgeId> {
        let mut out = BTreeSet::from([edge]);
        if let Some(m) = model_of(self.doc, id)
            && let Some(c) = m.curve_of(edge)
            && let Some((_, es)) = m.curves().find(|(k, _): &(CurveId, _)| *k == c)
        {
            out.extend(es.iter().copied());
        }
        out
    }

    /// A point of the world in the local coordinates of a model.
    pub fn to_local(&self, id: ObjectId, p: DVec3) -> DVec3 {
        let fr = frame_of(self.doc, id);
        if fr.matrix3.determinant().abs() > 1e-12 {
            fr.inverse().transform_point3(p)
        } else {
            p
        }
    }
}
