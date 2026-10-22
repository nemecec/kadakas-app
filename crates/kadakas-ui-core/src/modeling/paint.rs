//! The Paint Bucket (MOD-05, round B): a headless state machine that puts a material on faces and
//! objects, and the search for the faces that a fill covers.
//!
//! The bucket holds one material (or none: a click then clears the paint). A click on a face
//! paints the side that faces the viewer. The modifiers pick what a click covers:
//!
//! | Modifier | Covers |
//! |---|---|
//! | none | the one face |
//! | Ctrl | the faces connected to it through shared edges that have the same material on that side |
//! | Shift | all faces of the document with the same material on that side |
//! | Alt (Cmd on a Mac) | nothing: the material under the pointer goes into the bucket |
//!
//! These modifiers are the public habit of other modeling programs; they are not checked against
//! a manual. With whole objects selected, a click on one of them paints all the selected objects
//! that can take a material (a surface model, a group or a component instance); a click on an
//! instance with nothing selected paints the instance. Plants, hardscapes, site models and other
//! objects keep the material of their style; the tool says so and does nothing.
//!
//! Nothing here draws or touches the document: [`Paint::click`] answers with an [`Out`].

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec2;
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::present;
use kadakas_core::resource::ResourceId;
use kadakas_core::surface::{ATTR_BACK_MATERIAL, ATTR_MATERIAL, FaceId};
use kadakas_l10n::tr;

use super::exec::Action;
use super::scenes::{World, model_of};
use super::tools::Out;
use crate::infer3d::{Camera, Inferencer, TagKind};

/// What a click of the bucket covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    /// The face under the pointer.
    Single,
    /// The faces connected to it with the same material (Ctrl).
    Connected,
    /// All faces with the same material (Shift).
    SameMaterial,
}

/// The modifiers of a click of the bucket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintMods {
    pub ctrl: bool,
    pub shift: bool,
    /// Alt (Cmd on a Mac): sample the material under the pointer.
    pub sample: bool,
}

impl PaintMods {
    /// The fill of the modifiers. Ctrl wins over Shift.
    pub fn fill(self) -> Fill {
        if self.ctrl {
            Fill::Connected
        } else if self.shift {
            Fill::SameMaterial
        } else {
            Fill::Single
        }
    }
}

/// What is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintHit {
    /// A face of a surface model, and whether its front is the side the viewer sees.
    Face {
        model: ObjectId,
        face: FaceId,
        front: bool,
    },
    /// An object that is no face of a surface model (an instance, a plant, a hardscape...).
    Object(ObjectId),
    Nothing,
}

/// The bucket.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paint {
    /// The material in the bucket. `None`: the default look, a click clears the paint.
    pub bucket: Option<ResourceId>,
}

/// An inferencer that finds faces only: no snapping to points and edges, so a click near an edge
/// still paints the face under the pointer.
pub fn face_engine() -> Inferencer {
    let mut engine = Inferencer::default();
    let o = &mut engine.options;
    for v in [
        &mut o.endpoint_px,
        &mut o.center_px,
        &mut o.midpoint_px,
        &mut o.intersection_px,
        &mut o.edge_px,
    ] {
        *v = 0.01;
    }
    engine
}

/// The face under the pointer and the side the viewer sees, or [`PaintHit::Nothing`] (the caller
/// then looks for an object). `engine` should come from [`face_engine`].
pub fn hit_face(w: &World, engine: &mut Inferencer, cam: &Camera, cursor: DVec2) -> PaintHit {
    engine.reset();
    let inf = w.infer(engine, cam, cursor, None, &super::Excluded::default());
    if inf.source.kind() != TagKind::Face {
        return PaintHit::Nothing;
    }
    let Some(model) = w.scenes.object(inf.source) else {
        return PaintHit::Nothing;
    };
    let face = FaceId(inf.source.id());
    let Some(plane) = w.face_plane(inf.source) else {
        return PaintHit::Nothing;
    };
    // The front is seen when the normal points toward the eye.
    let front = cam.ray(cursor).dir.dot(plane.n) < 0.0;
    PaintHit::Face { model, face, front }
}

/// The material on one side of a face.
pub fn side_material(
    doc: &Document,
    model: ObjectId,
    face: FaceId,
    front: bool,
) -> Option<ResourceId> {
    let f = model_of(doc, model)?.face(face)?;
    let (a, b) = present::face_materials(doc, &f.attrs);
    if front { a } else { b }
}

/// The text of the attribute for a side, whether it holds anything (even a name that no material
/// has): a painted side counts as painted.
fn side_attr(doc: &Document, model: ObjectId, face: FaceId, front: bool) -> Option<String> {
    let f = model_of(doc, model)?.face(face)?;
    let key = if front {
        ATTR_MATERIAL
    } else {
        ATTR_BACK_MATERIAL
    };
    f.attrs
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The faces a fill covers, each with the side that is painted (`true`: the front). The clicked
/// face is always first.
pub fn fill_faces(
    doc: &Document,
    model: ObjectId,
    face: FaceId,
    front: bool,
    fill: Fill,
) -> Vec<(ObjectId, FaceId, bool)> {
    let Some(m) = model_of(doc, model) else {
        return Vec::new();
    };
    if m.face(face).is_none() {
        return Vec::new();
    }
    let mut out = vec![(model, face, front)];
    let want = side_attr(doc, model, face, front);
    match fill {
        Fill::Single => {}
        Fill::Connected => {
            let mut seen: BTreeSet<FaceId> = BTreeSet::from([face]);
            let mut work = vec![(face, front)];
            while let Some((cur, cur_front)) = work.pop() {
                let Some(f) = m.face(cur) else { continue };
                for (a, b) in f.uses() {
                    let Some(e) = m.edge_between(a, b) else {
                        continue;
                    };
                    for g in m.faces_of_edge(e) {
                        if seen.contains(&g) {
                            continue;
                        }
                        let Some(gf) = m.face(g) else { continue };
                        // Faces that agree on the direction of the shared edge face the same
                        // way (a side of the surface that runs on), else the sides swap.
                        let agrees = gf.uses().any(|u| u == (b, a));
                        let g_front = if agrees { cur_front } else { !cur_front };
                        if side_attr(doc, model, g, g_front) == want {
                            seen.insert(g);
                            out.push((model, g, g_front));
                            work.push((g, g_front));
                        }
                    }
                }
            }
        }
        Fill::SameMaterial => {
            for (id, obj) in &doc.objects {
                if !doc.is_visible(obj) {
                    continue;
                }
                let Shape::SurfaceModel(sm) = &obj.shape else {
                    continue;
                };
                for (fid, _) in sm.faces() {
                    if (*id, fid) != (model, face) && side_attr(doc, *id, fid, front) == want {
                        out.push((*id, fid, front));
                    }
                }
            }
        }
    }
    out
}

/// The paint action for faces with sides: grouped by model.
fn paint_action(items: Vec<(ObjectId, FaceId, bool)>, material: Option<ResourceId>) -> Action {
    let mut by_model: BTreeMap<ObjectId, (Vec<FaceId>, Vec<FaceId>)> = BTreeMap::new();
    for (m, f, front) in items {
        let e = by_model.entry(m).or_default();
        if front { &mut e.0 } else { &mut e.1 }.push(f);
    }
    Action::Paint {
        models: by_model.into_iter().map(|(m, (a, b))| (m, a, b)).collect(),
        material,
    }
}

fn name_of(doc: &Document, id: Option<ResourceId>) -> String {
    id.and_then(|r| doc.resources.get(r))
        .map_or_else(|| tr!("uic-paint-default"), |r| r.name.clone())
}

impl Paint {
    /// Puts a material in the bucket.
    pub fn set(&mut self, m: Option<ResourceId>) {
        self.bucket = m;
    }

    /// The bucket, if its material is still in the document.
    pub fn material(&self, doc: &Document) -> Option<ResourceId> {
        self.bucket.filter(|m| present::material(doc, *m).is_some())
    }

    /// A click. `selection` holds the objects that are selected as a whole.
    pub fn click(
        &mut self,
        doc: &Document,
        hit: PaintHit,
        selection: &[ObjectId],
        mods: PaintMods,
    ) -> Out {
        if mods.sample {
            return self.sample(doc, hit);
        }
        let material = self.material(doc);
        if self.bucket.is_some() && material.is_none() {
            // The material was deleted: the bucket is empty now.
            self.bucket = None;
        }
        // Whole objects: the selected ones when the click is on one of them (or in a group of
        // them), or the object that was hit when it is no face of a surface model.
        let in_selection = |o: ObjectId| {
            selection.contains(&o) || doc.ancestors(o).iter().any(|a| selection.contains(a))
        };
        let whole = match hit {
            PaintHit::Face { model, .. } if in_selection(model) => Some(selection.to_vec()),
            PaintHit::Object(o) if in_selection(o) => Some(selection.to_vec()),
            PaintHit::Object(o) => Some(vec![o]),
            _ => None,
        };
        if let Some(objects) = whole {
            return paint_objects(doc, objects, material);
        }
        match hit {
            PaintHit::Face { model, face, front } => {
                let items = fill_faces(doc, model, face, front, mods.fill());
                if items.is_empty() {
                    return Out::default();
                }
                let n = items.len();
                let mut out = Out::act(paint_action(items, material));
                out.message = Some(if n == 1 {
                    tr!("uic-paint-painted-one", name = name_of(doc, material))
                } else {
                    tr!(
                        "uic-paint-painted-many",
                        count = n,
                        name = name_of(doc, material)
                    )
                });
                out
            }
            PaintHit::Object(_) | PaintHit::Nothing => Out::default(),
        }
    }

    /// Alt: the material under the pointer goes into the bucket.
    fn sample(&mut self, doc: &Document, hit: PaintHit) -> Out {
        let taken = match hit {
            PaintHit::Face { model, face, front } => side_material(doc, model, face, front),
            PaintHit::Object(o) => present::object_material(doc, o),
            PaintHit::Nothing => return Out::default(),
        };
        self.bucket = taken;
        Out::msg(tr!("uic-paint-sampled", name = name_of(doc, taken)))
    }
}

/// Paints whole objects, or says why it cannot.
fn paint_objects(doc: &Document, objects: Vec<ObjectId>, material: Option<ResourceId>) -> Out {
    let (ok, refused): (Vec<ObjectId>, Vec<ObjectId>) = objects.into_iter().partition(|o| {
        doc.object(*o)
            .is_some_and(|obj| present::can_paint_object(&obj.shape))
    });
    if ok.is_empty() {
        return Out::msg(tr!("uic-paint-refused-style"));
    }
    let mut out = Out::act(Action::PaintObjects {
        objects: ok.clone(),
        material,
    });
    out.message = Some(if refused.is_empty() {
        tr!(
            "uic-paint-painted-objects",
            count = ok.len(),
            name = name_of(doc, material)
        )
    } else {
        tr!(
            "uic-paint-painted-some-objects",
            count = ok.len(),
            skipped = refused.len()
        )
    });
    out
}
