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
            // Data changes make no geometry. In a batch the annotations that follow the object are
            // brought up to date once, when the batch ends (its `annot_after` looks at everything
            // that the batch touched). This keeps a batch of many `SetData` linear.
            Command::SetData { .. } if self.batch_depth > 0 => {}
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
                self.annot_sync(&ids, false, false);
            }
            Command::MoveObjects { ids, .. } => {
                self.annot_sync(&ids.iter().copied().collect(), false, true)
            }
            Command::ModifyObject { id, .. } | Command::SetVertex { id, .. } => {
                self.annot_sync(&[*id].into_iter().collect(), false, false)
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
                self.annot_sync(&HashSet::new(), false, false);
            }
            // Any other command (rotate, scale, mirror, copy, array, a new landscape area or
            // hardscape, a group move...): annotations follow every object the command changed.
            _ => {
                // Everything the transaction changed so far, looked up in the sets that it keeps
                // (a copy for each command would cost a pass over all added objects, and a batch
                // of thousands of commands would be quadratic).
                if !(self.before.is_empty() && self.added_set.is_empty()) {
                    self.annot_sync(&HashSet::new(), true, false);
                }
            }
        }
        Ok(())
    }

    /// Updates annotations that follow one of the `changed` objects, or that are in `changed`
    /// themselves (`moved`: they were moved by hand). With `all`, every object that the
    /// transaction changed so far (`before` and `added_set`) counts as changed too.
    pub(super) fn annot_sync(&mut self, changed: &HashSet<ObjectId>, all: bool, moved: bool) {
        // The objects the command touched count too: a group move changes its members, not only
        // the ids it names.
        let with_touched = all || !changed.is_empty();
        let (before, added) = (&self.before, &self.added_set);
        let touched =
            |id: &ObjectId| with_touched && (before.contains_key(id) || added.contains(id));
        let mut updates: Vec<(ObjectId, Annot)> = Vec::new();
        for o in self.doc.objects.values() {
            let Shape::Annotation(a) = &o.shape else {
                continue;
            };
            let own = changed.contains(&o.id) || (all && touched(&o.id));
            let follows = a
                .depends_on()
                .iter()
                .any(|d| changed.contains(d) || touched(d));
            // With nothing changed (a delete) every annotation is looked at.
            if !(changed.is_empty() && !all || own || follows) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annot::DataTag;
    use crate::annot::style::{AnnotResource, TagStyle};
    use crate::resource::ResourceBody;
    use glam::DVec2;

    /// An object, a data tag on it, and the data of the object, in one batch or one by one.
    fn world(batch: bool) -> Document {
        let mut ed = Editor::new(Document::with_sequential_ids());
        ed.execute(&Command::AddResource {
            name: "Tags".into(),
            body: ResourceBody::Annot(Box::new(AnnotResource::TagStyle(TagStyle::default()))),
        })
        .unwrap();
        let style = ed.doc().resources.iter().next().unwrap().id;
        let circle = ObjectId(ed.doc().id_plan().take());
        let add = |shape| Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        };
        let tag = DataTag {
            style,
            object: circle,
            offset: DVec2::new(1.0, 1.0),
            at: DVec2::ZERO,
            anchor: DVec2::ZERO,
        };
        let cmds = vec![
            add(Shape::Circle {
                center: DVec2::new(100.0, 50.0),
                radius: 5.0,
            }),
            add(Shape::Annotation(Box::new(Annot::DataTag(tag)))),
            Command::SetData {
                id: circle,
                set: [("note".to_string(), serde_json::json!("x"))].into(),
                remove: vec![],
            },
            Command::MoveObjects {
                ids: vec![circle],
                delta: DVec2::new(10.0, 0.0),
            },
        ];
        if batch {
            ed.execute(&Command::Batch { commands: cmds }).unwrap();
        } else {
            for c in &cmds {
                ed.execute(c).unwrap();
            }
        }
        ed.doc().clone()
    }

    #[test]
    fn a_batch_syncs_annotations_like_the_same_commands_one_by_one() {
        let (batch, single) = (world(true), world(false));
        assert_eq!(batch.objects, single.objects);
        let tag = batch
            .objects
            .values()
            .find_map(|o| match &o.shape {
                Shape::Annotation(a) => match &**a {
                    Annot::DataTag(t) => Some(t.clone()),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        // The tag follows the object that was added, given data and moved in the same batch.
        assert_eq!(tag.anchor, DVec2::new(110.0, 50.0));
        assert_ne!(tag.at, tag.anchor);
    }

    #[test]
    fn many_commands_in_a_batch_do_not_cost_a_pass_over_the_added_objects_each() {
        // 3000 objects with data in one batch. The work grows with the number of commands, not with
        // its square: the sets of the transaction are looked up, not copied for each command.
        let mut ed = Editor::new(Document::with_sequential_ids());
        let mut plan = ed.doc().id_plan();
        let mut cmds = Vec::new();
        for k in 0..3000 {
            let id = ObjectId(plan.take());
            cmds.push(Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::new(k as f64, 0.0),
                    radius: 1.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            });
            cmds.push(Command::SetData {
                id,
                set: [("k".to_string(), serde_json::json!(k))].into(),
                remove: vec![],
            });
        }
        ed.execute(&Command::Batch { commands: cmds }).unwrap();
        assert_eq!(ed.doc().objects.len(), 3000);
        assert!(ed.doc().objects.values().all(|o| o.data.contains_key("k")));
    }
}
