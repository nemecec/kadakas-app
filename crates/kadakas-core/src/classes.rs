//! Every place that names a class (MOD-05, Tags). Deleting a class (a tag in the 3D modeler) must
//! leave no dangling id and must not change what a person sees without a word. This module counts
//! the references, so the window can ask with the real numbers, and moves them all to another
//! class (Untagged by default).
//!
//! References to a class:
//!
//! - the class of an object;
//! - the class of a part, a component or the insert class of a symbol definition (a resource);
//! - the class overrides and the annotations of a viewport;
//! - the classes and the active class of a saved view;
//! - the material slots of a class.
//!
//! A material slot of a class is not moved to the other class: that would paint every object of it.
//! The objects that moved get a slot of their own, so they look the same as before.

use crate::graphics::GraphicResource;
use crate::model::{ClassId, Document, ObjectId, Shape};
use crate::present::SlotTarget;
use crate::resource::{ResourceBody, Resources};
use crate::sheets::Drawing;

/// How many places name a class, by kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassUses {
    /// Objects of the class, guides not counted.
    pub objects: usize,
    /// Guides of the class.
    pub guides: usize,
    /// Parts, 2D components and insert classes of symbol and component definitions.
    pub definition_parts: usize,
    /// Class overrides of viewports.
    pub viewport_overrides: usize,
    /// Annotations of viewports.
    pub annotations: usize,
    /// Saved views that name the class.
    pub saved_views: usize,
    /// Material slots of the class.
    pub material_slots: usize,
}

impl ClassUses {
    /// Everything but the objects: the references the old tag text did not count.
    pub fn others(&self) -> usize {
        self.guides
            + self.definition_parts
            + self.viewport_overrides
            + self.annotations
            + self.saved_views
            + self.material_slots
    }

    pub fn total(&self) -> usize {
        self.objects + self.others()
    }
}

/// Counts the references to the class.
pub fn uses(doc: &Document, id: ClassId) -> ClassUses {
    let mut n = ClassUses::default();
    for o in doc.objects.values() {
        if o.class == id {
            if matches!(o.shape, Shape::Guide(_)) {
                n.guides += 1;
            } else {
                n.objects += 1;
            }
        }
        if let Shape::Viewport(v) = &o.shape {
            n.viewport_overrides += usize::from(v.class_overrides.contains_key(&id));
            n.annotations += v.annotations.iter().filter(|a| a.class == id).count();
        }
    }
    for r in doc.resources.iter() {
        if let ResourceBody::Graphic(g) = &r.body {
            n.definition_parts += graphic_uses(g, id);
        }
    }
    n.saved_views = doc
        .drawing
        .saved_views
        .iter()
        .filter(|v| v.classes.contains_key(&id) || v.active_class == Some(id))
        .count();
    n.material_slots = doc
        .drawing
        .presentation
        .slots
        .iter()
        .filter(|e| matches!(&e.target, SlotTarget::Class { class } if *class == id))
        .count();
    n
}

fn graphic_uses(g: &GraphicResource, id: ClassId) -> usize {
    g.classes().iter().filter(|c| **c == id).count()
}

/// Moves the viewport references of an object from `from` to `to`. Returns how many it moved.
/// An override that `to` has already stays, and the one of `from` goes.
pub fn remap_object(o: &mut crate::model::Object, from: ClassId, to: ClassId) -> usize {
    let Shape::Viewport(v) = &mut o.shape else {
        return 0;
    };
    let mut n = 0;
    if let Some(ov) = v.class_overrides.remove(&from) {
        v.class_overrides.entry(to).or_insert(ov);
        n += 1;
    }
    for a in &mut v.annotations {
        if a.class == from {
            a.class = to;
            n += 1;
        }
    }
    n
}

/// Moves the references of the definitions in `resources`. Returns how many it moved.
pub fn remap_resources(resources: &mut Resources, from: ClassId, to: ClassId) -> usize {
    let mut n = 0;
    for r in resources.iter_mut() {
        if let ResourceBody::Graphic(g) = &mut r.body {
            n += graphic_uses(g, from);
            g.remap_classes(&|c| Some(if c == from { to } else { c }));
        }
    }
    n
}

/// What `remap_drawing` did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawingMoved {
    pub saved_views: usize,
    pub material_slots: usize,
}

/// Moves the references of the saved views and the material slots. `moved` are the objects that
/// leave the class: the slot of the class becomes a slot of each of them (unless the object has one
/// already).
pub fn remap_drawing(
    drawing: &mut Drawing,
    from: ClassId,
    to: ClassId,
    moved: &[ObjectId],
) -> DrawingMoved {
    let mut out = DrawingMoved::default();
    for v in &mut drawing.saved_views {
        let mut hit = false;
        if let Some(on) = v.classes.remove(&from) {
            v.classes.entry(to).or_insert(on);
            hit = true;
        }
        if v.active_class == Some(from) {
            v.active_class = Some(to);
            hit = true;
        }
        out.saved_views += usize::from(hit);
    }
    let class_slot = SlotTarget::Class { class: from };
    if let Some(material) = drawing.presentation.slot(&class_slot) {
        for object in moved {
            let target = SlotTarget::Object {
                object: *object,
                slot: None,
            };
            if drawing.presentation.slot(&target).is_none() {
                drawing.presentation.set_slot(target, Some(material));
            }
        }
        drawing.presentation.set_slot(class_slot, None);
        out.material_slots += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_of_nothing_are_zero() {
        let doc = Document::default();
        assert_eq!(uses(&doc, ClassId::from_num(7)), ClassUses::default());
        assert_eq!(ClassUses::default().total(), 0);
    }
}
