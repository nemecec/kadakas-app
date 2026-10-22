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
    /// Slots of objects, styles and classes, the textures of site models and the textures of
    /// texture beds.
    pub slots: usize,
    /// Faces of surface models, in the document and in the definitions of components (a face with
    /// the material on both sides counts twice).
    pub faces: usize,
    /// The surface models of the document that have faces with the material, with the faces that
    /// hold it on the front and on the back.
    pub models: Vec<(
        ObjectId,
        Vec<crate::surface::FaceId>,
        Vec<crate::surface::FaceId>,
    )>,
    /// The same for the parts of component definitions: the definition, the number of the part,
    /// the faces on the front and on the back.
    pub parts: Vec<(
        ResourceId,
        usize,
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

/// How the text of a face attribute becomes a material, with the lookups made once for a whole
/// document: the ids of the materials and their names (the first material of a name wins; the
/// Paint Bucket stores ids, so a name is only for files made by hand).
pub struct PaintResolver {
    ids: std::collections::HashSet<ResourceId>,
    names: std::collections::HashMap<String, ResourceId>,
}

impl PaintResolver {
    pub fn new(doc: &Document) -> PaintResolver {
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashMap::new();
        for r in doc.resources.iter() {
            if material(doc, r.id).is_some() {
                ids.insert(r.id);
                names.entry(r.name.to_lowercase()).or_insert(r.id);
            }
        }
        PaintResolver { ids, names }
    }

    pub fn resolve(&self, text: &str) -> Option<ResourceId> {
        if let Some(uid) = crate::uid::Uid::parse(text) {
            let id = ResourceId(uid);
            if self.ids.contains(&id) {
                return Some(id);
            }
        }
        self.names.get(&text.to_lowercase()).copied()
    }

    pub fn faces(&self, attrs: &Attrs) -> (Option<ResourceId>, Option<ResourceId>) {
        let get = |k: &str| {
            attrs
                .get(k)
                .and_then(|v| v.as_str())
                .and_then(|t| self.resolve(t))
        };
        (get(ATTR_MATERIAL), get(ATTR_BACK_MATERIAL))
    }
}

/// Who uses each material, in one pass over the document: the slots, the textures of site models
/// and texture beds, the faces of surface models, and the faces of the parts of component
/// definitions. Only materials that are used have an entry. (Styles of plants and hardscapes
/// name their materials as text, not as resources, so they hold no use.)
pub fn material_use_map(doc: &Document) -> std::collections::HashMap<ResourceId, MaterialUses> {
    use std::collections::HashMap;
    let mut map: HashMap<ResourceId, MaterialUses> = HashMap::new();
    for e in &doc.drawing.presentation.slots {
        map.entry(e.material).or_default().slots += 1;
    }
    let res = PaintResolver::new(doc);
    let sides = |m: &crate::surface::SurfaceModel| {
        // The faces by material, front and back.
        let mut by: HashMap<
            ResourceId,
            (Vec<crate::surface::FaceId>, Vec<crate::surface::FaceId>),
        > = HashMap::new();
        for (fid, f) in m.faces() {
            let (a, b) = res.faces(&f.attrs);
            if let Some(a) = a {
                by.entry(a).or_default().0.push(fid);
            }
            if let Some(b) = b {
                by.entry(b).or_default().1.push(fid);
            }
        }
        by
    };
    for obj in doc.objects.values() {
        if let Some(t) = crate::texture_bed::texture_of(&obj.shape) {
            map.entry(t).or_default().slots += 1;
        }
        let Shape::SurfaceModel(m) = &obj.shape else {
            continue;
        };
        for (mat, (front, back)) in sides(m) {
            let u = map.entry(mat).or_default();
            u.faces += front.len() + back.len();
            u.models.push((obj.id, front, back));
        }
    }
    for r in doc.resources.iter() {
        let crate::resource::ResourceBody::Graphic(g) = &r.body else {
            continue;
        };
        let crate::graphics::GraphicResource::Symbol(def) = g.as_ref() else {
            continue;
        };
        for (i, part) in def.parts.iter().enumerate() {
            let Shape::SurfaceModel(m) = &part.shape else {
                continue;
            };
            for (mat, (front, back)) in sides(m) {
                let u = map.entry(mat).or_default();
                u.faces += front.len() + back.len();
                u.parts.push((r.id, i, front, back));
            }
        }
    }
    map
}

/// Who uses the material `id` (see [`material_use_map`]; ask the map when you need many).
pub fn material_uses(doc: &Document, id: ResourceId) -> MaterialUses {
    material_use_map(doc).remove(&id).unwrap_or_default()
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

/// True if an object can be edited now: its layer is not locked or grey and its class is not grey.
/// The same test as the commands make.
pub fn can_edit(doc: &Document, obj: &crate::model::Object) -> bool {
    doc.layer(obj.layer).is_some_and(|l| !l.locked)
        && !doc.drawing.grey.layers.contains(&obj.layer)
        && !doc.drawing.grey.classes.contains(&obj.class)
}

/// Gives every side of the faces of `model` that has no material the material `m`. Used where a
/// group or an instance with a material is exploded: the material stays on what it was on.
pub fn bake_material(
    model: &mut crate::surface::SurfaceModel,
    m: ResourceId,
) -> Result<(), crate::surface::SurfaceError> {
    use crate::surface::{Edit, PaintSide};
    let (mut front, mut back) = (Vec::new(), Vec::new());
    for (fid, f) in model.faces() {
        if !f.attrs.contains_key(ATTR_MATERIAL) {
            front.push(fid);
        }
        if !f.attrs.contains_key(ATTR_BACK_MATERIAL) {
            back.push(fid);
        }
    }
    for (faces, side) in [(front, PaintSide::Front), (back, PaintSide::Back)] {
        if !faces.is_empty() {
            model.apply(&Edit::SetFaceMaterial {
                faces,
                side,
                material: Some(m.to_string()),
            })?;
        }
    }
    Ok(())
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
