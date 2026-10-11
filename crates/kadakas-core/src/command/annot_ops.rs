//! After a command, annotations follow the objects they are tied to (see `annot::sync`).

use std::collections::{HashMap, HashSet};

use super::*;
use crate::annot::{Anchor, Annot, sync};

impl Tx<'_> {
    /// Fixes the links of a copy of an annotation. `map` has the new ID of every object that was
    /// copied in the same command.
    ///
    /// - A link to an object that was copied too goes to the copy.
    /// - A dimension or callout anchor to an object that was not copied is dropped: the copy keeps
    ///   its points and does not jump back to the original.
    /// - A data tag copied alone keeps tagging the original object, and its label stays where the
    ///   copy put it (the offset is calculated again).
    pub(super) fn remap_copy_refs(&self, o: &mut Object, map: &HashMap<ObjectId, ObjectId>) {
        let layer = o.layer;
        let Shape::Annotation(a) = &mut o.shape else {
            return;
        };
        let remap = |anchor: &mut Option<Anchor>| {
            if let Some(x) = anchor {
                match map.get(&x.object) {
                    Some(new) => x.object = *new,
                    None => *anchor = None,
                }
            }
        };
        match a.as_mut() {
            Annot::DataTag(t) => {
                if let Some(new) = map.get(&t.object) {
                    t.object = *new;
                } else if let Some(p) = self
                    .doc
                    .object(t.object)
                    .and_then(|x| crate::annot::ref_point(&x.shape))
                {
                    let paper = self.doc.layer(layer).map_or(1.0, |l| l.scale);
                    if paper > 0.0 {
                        t.offset = (t.at - p) / paper;
                    }
                }
            }
            Annot::Dimension(d) => {
                d.anchors.iter_mut().for_each(remap);
                while d.anchors.last().is_some_and(Option::is_none) {
                    d.anchors.pop();
                }
            }
            Annot::Callout(c) => c.leaders.iter_mut().for_each(|l| remap(&mut l.anchor)),
            _ => {}
        }
    }

    /// Brings the annotations that depend on changed objects up to date.
    pub(super) fn annot_after(&mut self, cmd: &Command) -> Result<(), CommandError> {
        match cmd {
            // A new annotation only needs its own anchors applied. This stays cheap in a batch.
            Command::AddObject { .. } => {
                if let Some(id) = self.added.last().copied()
                    && let Some(o) = self.doc.object(id)
                    && let Shape::Annotation(a) = &o.shape
                {
                    let paper = self.doc.layer(o.layer).map_or(1.0, |l| l.scale);
                    if let Some(new) = sync::resync(self.doc, a, paper, false) {
                        self.doc.object_mut(id).expect("exists").shape =
                            Shape::Annotation(Box::new(new));
                    }
                }
            }
            // Label positions of data tags are paper offsets, so a new scale moves them.
            Command::ModifyLayer {
                id, scale: Some(_), ..
            } => {
                let ids: HashSet<ObjectId> = self
                    .doc
                    .objects
                    .values()
                    .filter(|o| o.layer == *id && matches!(o.shape, Shape::Annotation(_)))
                    .map(|o| o.id)
                    .collect();
                self.annot_sync(&ids, false);
            }
            Command::MoveObjects { ids, .. } => {
                self.annot_sync(&ids.iter().copied().collect(), true)
            }
            Command::ModifyObject { id, .. } | Command::SetVertex { id, .. } => {
                self.annot_sync(&[*id].into_iter().collect(), false)
            }
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
                    self.run(
                        &Command::DeleteObjects { ids: tags },
                        &mut Outcome::default(),
                    )?;
                }
                // The other annotations lose their anchors and keep their places.
                self.annot_sync(&HashSet::new(), false);
            }
            // Any other command (rotate, scale, mirror, copy, array, a new landscape area or
            // hardscape, a group move...): annotations follow every object the command changed.
            _ => {
                let changed: HashSet<ObjectId> = self
                    .before
                    .keys()
                    .copied()
                    .chain(self.added_set.iter().copied())
                    .collect();
                if !changed.is_empty() {
                    self.annot_sync(&changed, false);
                }
            }
        }
        Ok(())
    }

    /// Updates annotations that follow one of the `changed` objects, or that are in `changed`
    /// themselves (`moved`: they were moved by hand).
    fn annot_sync(&mut self, changed: &HashSet<ObjectId>, moved: bool) {
        // The objects the command touched count too: a group move changes its members, not only
        // the ids it names.
        let touched: HashSet<ObjectId> = if changed.is_empty() {
            HashSet::new()
        } else {
            self.before
                .keys()
                .copied()
                .chain(self.added_set.iter().copied())
                .collect()
        };
        let mut updates: Vec<(ObjectId, Annot)> = Vec::new();
        for o in self.doc.objects.values() {
            let Shape::Annotation(a) = &o.shape else {
                continue;
            };
            let own = changed.contains(&o.id);
            let follows = a
                .depends_on()
                .iter()
                .any(|d| changed.contains(d) || touched.contains(d));
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
