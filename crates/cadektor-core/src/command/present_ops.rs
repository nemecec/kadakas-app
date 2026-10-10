//! Commands for the sun, material slots and the render style (see `crate::present`).

use super::*;
use crate::present::SlotTarget;

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    pub(super) fn run_present(&mut self, cmd: &Command) -> Result<(), CommandError> {
        match cmd {
            Command::SetSun { sun } => {
                if let Some(s) = sun {
                    s.validate().map_err(invalid)?;
                }
                self.doc.drawing.presentation.sun = *sun;
            }
            Command::SetMaterialSlot { target, material } => {
                if let Some(m) = material
                    && crate::present::material(self.doc, *m).is_none()
                {
                    return Err(match self.doc.resources.get(*m) {
                        Some(_) => invalid(format!("resource {m} is not a material")),
                        None => CommandError::NoResource(*m),
                    });
                }
                match target {
                    SlotTarget::Object { object, slot } => {
                        self.doc
                            .object(*object)
                            .ok_or(CommandError::NoObject(*object))?;
                        if slot.as_deref().is_some_and(|s| s.trim().is_empty()) {
                            return Err(invalid("a slot name cannot be empty"));
                        }
                    }
                    SlotTarget::Style { style, slot } => {
                        self.doc
                            .resources
                            .get(*style)
                            .ok_or(CommandError::NoResource(*style))?;
                        if slot.trim().is_empty() {
                            return Err(invalid("a slot name cannot be empty"));
                        }
                    }
                    SlotTarget::Class { class } => self.check_class(*class)?,
                }
                self.doc
                    .drawing
                    .presentation
                    .set_slot(target.clone(), *material);
            }
            Command::SetRenderStyle { style } => {
                if let Some(s) = style
                    && crate::present::render_style(self.doc, *s).is_none()
                {
                    return Err(match self.doc.resources.get(*s) {
                        Some(_) => invalid(format!("resource {s} is not a render style")),
                        None => CommandError::NoResource(*s),
                    });
                }
                self.doc.drawing.presentation.active_style = *style;
            }
            _ => unreachable!("not a presentation command"),
        }
        Ok(())
    }

    /// Drops the slots of objects that no longer exist. An undo brings them back with the objects,
    /// because the slots are part of the document state that undo restores.
    pub(super) fn present_prune(&mut self) {
        let p = &self.doc.drawing.presentation;
        if p.slots.iter().any(|e| {
            matches!(&e.target, SlotTarget::Object { object, .. } if self.doc.object(*object).is_none())
        }) {
            let doc = &*self.doc;
            let keep: Vec<_> = doc
                .drawing
                .presentation
                .slots
                .iter()
                .filter(|e| {
                    !matches!(&e.target, SlotTarget::Object { object, .. } if doc.object(*object).is_none())
                })
                .cloned()
                .collect();
            self.doc.drawing.presentation.slots = keep;
        }
    }

    /// Gives the copies of objects the material slots of the originals.
    pub(super) fn present_copy_slots(
        &mut self,
        map: &std::collections::HashMap<ObjectId, ObjectId>,
    ) {
        let copies: Vec<_> = self
            .doc
            .drawing
            .presentation
            .slots
            .iter()
            .filter_map(|e| match &e.target {
                SlotTarget::Object { object, slot } => map.get(object).map(|n| {
                    (
                        SlotTarget::Object {
                            object: *n,
                            slot: slot.clone(),
                        },
                        e.material,
                    )
                }),
                _ => None,
            })
            .collect();
        for (target, m) in copies {
            self.doc.drawing.presentation.set_slot(target, Some(m));
        }
    }

    /// Deletes a material or a render style and removes it from the slots. Returns false if the
    /// resource is of another kind (the caller handles it).
    pub(super) fn present_delete(&mut self, id: ResourceId) -> Result<bool, CommandError> {
        let is_ours = matches!(
            self.doc.resources.get(id).map(|r| &r.body),
            Some(ResourceBody::Render(_))
        );
        if !is_ours {
            return Ok(false);
        }
        self.doc.resources.remove(id);
        self.doc.drawing.presentation.forget(id);
        Ok(true)
    }
}
