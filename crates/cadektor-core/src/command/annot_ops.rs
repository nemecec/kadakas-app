//! After a command, annotations follow the objects they are tied to (see `annot::sync`).

use std::collections::HashSet;

use super::*;
use crate::annot::{Annot, sync};

impl Tx<'_> {
    /// Brings the annotations that depend on changed objects up to date.
    pub(super) fn annot_after(&mut self, cmd: &Command) -> Result<(), CommandError> {
        match cmd {
            Command::AddObject { .. } => {
                if let Some(id) = self.added.last().copied() {
                    self.annot_sync(&[id].into_iter().collect(), false);
                }
            }
            Command::MoveObjects { ids, .. } => self.annot_sync(&ids.iter().copied().collect(), true),
            Command::ModifyObject { id, .. } | Command::SetVertex { id, .. } => self.annot_sync(&[*id].into_iter().collect(), false),
            Command::DeleteObjects { ids } => {
                let gone: HashSet<ObjectId> = ids.iter().copied().collect();
                // A data tag goes with its object. Locked layers keep their tags.
                let tags: Vec<ObjectId> = self
                    .doc
                    .objects
                    .values()
                    .filter(|o| matches!(&o.shape, Shape::Annotation(a) if matches!(&**a, Annot::DataTag(t) if gone.contains(&t.object))))
                    .filter(|o| self.doc.layer(o.layer).is_some_and(|l| !l.locked))
                    .map(|o| o.id)
                    .collect();
                if !tags.is_empty() {
                    self.run(&Command::DeleteObjects { ids: tags }, &mut Outcome::default())?;
                }
                // The other annotations lose their anchors and keep their places.
                self.annot_sync(&HashSet::new(), false);
            }
            _ => {}
        }
        Ok(())
    }

    /// Updates annotations that follow one of the `changed` objects, or that are in `changed`
    /// themselves (`moved`: they were moved by hand).
    fn annot_sync(&mut self, changed: &HashSet<ObjectId>, moved: bool) {
        let mut updates: Vec<(ObjectId, Annot)> = Vec::new();
        for o in self.doc.objects.values() {
            let Shape::Annotation(a) = &o.shape else { continue };
            let own = changed.contains(&o.id);
            let follows = a.depends_on().iter().any(|d| changed.contains(d));
            // With nothing changed (a delete) every annotation is looked at.
            if !(changed.is_empty() || own || follows) {
                continue;
            }
            let paper = self.doc.layer(o.layer).map_or(1.0, |l| l.scale);
            if let Some(new) = sync::resync(self.doc, a, paper, moved && own) {
                updates.push((o.id, new));
            }
        }
        for (id, new) in updates {
            self.touch(id);
            if let Some(o) = self.doc.object_mut(id) {
                o.shape = Shape::Annotation(Box::new(new));
            }
        }
    }
}
