//! Paint (MOD-05): the materials of the faces of surface models and how to find who uses a
//! material.
//!
//! A face holds the material of its front in the attribute `material` and the material of its back
//! in `back_material` (see `crate::surface::Edit::SetFaceMaterial`). The value is the text form of
//! the id of a material resource. A value that is the exact name of a material (any case) works
//! too, so a file made by hand with `"material": "Brick"` shows the brick. The face has priority
//! over the material of the object (the slot `object`), which has priority over the style and the
//! class (see [`super::SlotIndex::resolve`]).

use super::{SlotTarget, material};
use crate::model::{Document, ObjectId, Shape};
use crate::resource::ResourceId;
use crate::surface::{ATTR_BACK_MATERIAL, ATTR_MATERIAL, Attrs};

/// The material resource that a face attribute value names: the text form of its id, else its
/// name. `None` if the document has no such material.
pub fn material_ref(doc: &Document, text: &str) -> Option<ResourceId> {
    if let Some(uid) = crate::uid::Uid::parse(text) {
        let id = ResourceId(uid);
        if material(doc, id).is_some() {
            return Some(id);
        }
    }
    doc.resources
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(text) && material(doc, r.id).is_some())
        .map(|r| r.id)
}

/// The materials of the front and the back of a face, from its attributes.
pub fn face_materials(doc: &Document, attrs: &Attrs) -> (Option<ResourceId>, Option<ResourceId>) {
    let get = |k: &str| {
        attrs
            .get(k)
            .and_then(|v| v.as_str())
            .and_then(|t| material_ref(doc, t))
    };
    (get(ATTR_MATERIAL), get(ATTR_BACK_MATERIAL))
}

/// Where a material is used.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterialUses {
    /// Slots of objects, styles and classes.
    pub slots: usize,
    /// Faces of surface models (a face with the material on both sides counts twice).
    pub faces: usize,
    /// The surface models that have faces with the material, with the faces that hold it on the
    /// front and on the back.
    pub models: Vec<(
        ObjectId,
        Vec<crate::surface::FaceId>,
        Vec<crate::surface::FaceId>,
    )>,
}

impl MaterialUses {
    /// True if nothing uses the material.
    pub fn is_unused(&self) -> bool {
        self.slots == 0 && self.faces == 0
    }

    /// Slots and face sides together.
    pub fn total(&self) -> usize {
        self.slots + self.faces
    }
}

/// Who uses the material `id`: the slots of the document and the faces of its surface models.
/// (Faces inside the definition of a component are not counted: an edit in place changes them.)
pub fn material_uses(doc: &Document, id: ResourceId) -> MaterialUses {
    let mut uses = MaterialUses {
        slots: doc
            .drawing
            .presentation
            .slots
            .iter()
            .filter(|e| e.material == id)
            .count(),
        ..MaterialUses::default()
    };
    // The texture of a site model and its beds also hold a material.
    for obj in doc.objects.values() {
        if let Shape::SiteModel { settings, .. } = &obj.shape
            && settings.texture == Some(id)
        {
            uses.slots += 1;
        }
        let Shape::SurfaceModel(m) = &obj.shape else {
            continue;
        };
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for (fid, f) in m.faces() {
            let (a, b) = face_materials(doc, &f.attrs);
            if a == Some(id) {
                front.push(fid);
            }
            if b == Some(id) {
                back.push(fid);
            }
        }
        if !front.is_empty() || !back.is_empty() {
            uses.faces += front.len() + back.len();
            uses.models.push((obj.id, front, back));
        }
    }
    uses
}

/// True if `target` is a slot of the object `object` as a whole (no part).
pub fn is_whole_object_slot(target: &SlotTarget, object: ObjectId) -> bool {
    matches!(target, SlotTarget::Object { object: o, slot: None } if *o == object)
}

/// The material that an object has as a whole (the slot of the object, no part), if any.
pub fn object_material(doc: &Document, object: ObjectId) -> Option<ResourceId> {
    doc.drawing
        .presentation
        .slots
        .iter()
        .rev()
        .find(|e| is_whole_object_slot(&e.target, object) && material(doc, e.material).is_some())
        .map(|e| e.material)
}

/// True if the Paint Bucket may put a material on the object as a whole: a surface model, a
/// group or a component instance. Plants, hardscapes, site models and the rest keep the material
/// of their style; the reason is in [`paint_object_refusal`].
pub fn can_paint_object(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::SurfaceModel(_) | Shape::Group { .. } | Shape::Component { .. }
    )
}

/// Why an object cannot take a material from the Paint Bucket (an English text for logs and
/// tests; the windows have their own words). `None` if it can.
pub fn paint_object_refusal(shape: &Shape) -> Option<&'static str> {
    if can_paint_object(shape) {
        None
    } else {
        Some(
            "this kind of object keeps the material of its style; change the style in the Kadakas app",
        )
    }
}
