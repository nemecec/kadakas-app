//! Groups: queries over the tree that `Object::parent` makes, and the selection rules.
//!
//! A group is an object with `Shape::Group`. Its members name it as their `parent`, and groups
//! can be members of other groups. The group draws nothing: its members draw themselves, so
//! renderers need no group code. Commands (see `command`) move, rotate, copy, delete and
//! ungroup a group as one unit.
//!
//! Selection follows Vectorworks. A click selects the group at the top (outside any group you are
//! in). After you enter a group, a click selects the members of that group, and a member that is
//! a group again is selected as a whole.

use std::collections::HashSet;

use crate::model::{Document, Object, ObjectId, Shape};

/// Groups inside groups are followed this deep.
pub const MAX_DEPTH: usize = 64;

impl Document {
    /// True if the object exists and is a group.
    pub fn is_group(&self, id: ObjectId) -> bool {
        self.object(id).is_some_and(|o| matches!(o.shape, Shape::Group { .. }))
    }

    /// The direct members of a group, in z-order.
    pub fn children(&self, group: ObjectId) -> Vec<ObjectId> {
        self.objects.values().filter(|o| o.parent == Some(group)).map(|o| o.id).collect()
    }

    /// All members of a group at any depth (the groups too), in z-order.
    pub fn descendants(&self, group: ObjectId) -> Vec<ObjectId> {
        let mut inside: HashSet<ObjectId> = HashSet::from([group]);
        let mut out = Vec::new();
        // The list is in z-order, but a member can come before its group. Repeat until nothing is added.
        loop {
            let before = inside.len();
            for o in self.objects.values() {
                if o.parent.is_some_and(|p| inside.contains(&p)) && inside.insert(o.id) {
                    continue;
                }
            }
            if inside.len() == before {
                break;
            }
        }
        for o in self.objects.values() {
            if o.id != group && inside.contains(&o.id) {
                out.push(o.id);
            }
        }
        out
    }

    /// The groups that contain the object, nearest first.
    pub fn ancestors(&self, id: ObjectId) -> Vec<ObjectId> {
        let mut out = Vec::new();
        let mut cur = self.object(id).and_then(|o| o.parent);
        while let Some(p) = cur {
            if out.len() >= MAX_DEPTH || out.contains(&p) {
                break;
            }
            out.push(p);
            cur = self.object(p).and_then(|o| o.parent);
        }
        out
    }

    /// The outermost group that contains the object, or the object itself if it is in no group.
    pub fn top_level(&self, id: ObjectId) -> ObjectId {
        self.ancestors(id).last().copied().unwrap_or(id)
    }

    /// What a click on `id` selects when you are in the group `context` (`None`: in no group).
    /// It is the object, or the group that holds it, that is a direct member of the context.
    /// `None` if the object is not inside the context (it cannot be selected from there).
    pub fn selection_target(&self, id: ObjectId, context: Option<ObjectId>) -> Option<ObjectId> {
        self.object(id)?;
        let mut chain = vec![id];
        chain.extend(self.ancestors(id));
        match context {
            None => chain.last().copied(),
            Some(g) => {
                let i = chain.iter().position(|c| *c == g)?;
                i.checked_sub(1).map(|j| chain[j])
            }
        }
    }

    /// The objects with no group in them: every non-group object in the given objects and in the
    /// groups among them. Each appears once, in z-order.
    pub fn leaves(&self, ids: &[ObjectId]) -> Vec<ObjectId> {
        let mut want: HashSet<ObjectId> = HashSet::new();
        for &id in ids {
            if self.is_group(id) {
                want.extend(self.descendants(id));
            } else {
                want.insert(id);
            }
        }
        self.objects.values().filter(|o| want.contains(&o.id) && !matches!(o.shape, Shape::Group { .. })).map(|o| o.id).collect()
    }

    /// The given objects with all members of the groups among them (and the groups), each once, in z-order.
    pub fn with_members(&self, ids: &[ObjectId]) -> Vec<ObjectId> {
        let mut want: HashSet<ObjectId> = HashSet::new();
        for &id in ids {
            want.insert(id);
            if self.is_group(id) {
                want.extend(self.descendants(id));
            }
        }
        self.objects.values().filter(|o| want.contains(&o.id)).map(|o| o.id).collect()
    }

    /// True if `ancestor` is a group that holds `id`, directly or through other groups.
    pub fn is_inside(&self, id: ObjectId, ancestor: ObjectId) -> bool {
        self.ancestors(id).contains(&ancestor)
    }
}

/// Checks the groups of a document: a parent exists, is a group, is on the same layer and does not
/// lead back to the object.
pub fn check_tree(doc: &Document) -> Result<(), String> {
    for o in doc.objects.values() {
        if let Some(p) = o.parent {
            let parent = doc.object(p).ok_or_else(|| format!("object {} has a group {p} that does not exist", o.id))?;
            if !matches!(parent.shape, Shape::Group { .. }) {
                return Err(format!("object {} has a parent {p} that is not a group", o.id));
            }
            if parent.layer != o.layer {
                return Err(format!("object {} is not on the layer of its group {p}", o.id));
            }
            if doc.ancestors(o.id).contains(&o.id) || doc.ancestors(o.id).len() >= MAX_DEPTH {
                return Err(format!("object {} is its own ancestor", o.id));
            }
        }
    }
    Ok(())
}

/// True if the object is a group member that draws nothing by itself (a group).
pub fn is_group_object(o: &Object) -> bool {
    matches!(o.shape, Shape::Group { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::model::Attributes;
    use glam::DVec2;

    fn add(ed: &mut Editor, x: f64) -> ObjectId {
        let shape = Shape::Circle { center: DVec2::new(x, 0.0), radius: 1.0 };
        ObjectId(ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0])
    }

    fn group(ed: &mut Editor, ids: &[ObjectId]) -> ObjectId {
        ObjectId(ed.execute(&Command::Group { ids: ids.to_vec(), name: None }).unwrap().created[0])
    }

    #[test]
    fn tree_queries() {
        let mut ed = Editor::default();
        let (a, b, c, d) = (add(&mut ed, 0.0), add(&mut ed, 1.0), add(&mut ed, 2.0), add(&mut ed, 3.0));
        let inner = group(&mut ed, &[a, b]);
        let outer = group(&mut ed, &[inner, c]);
        let doc = ed.doc();
        assert!(doc.is_group(inner) && !doc.is_group(a) && !doc.is_group(ObjectId(999)));
        assert_eq!(doc.children(outer), vec![c, inner]);
        assert_eq!(doc.children(inner), vec![a, b]);
        let mut all = doc.descendants(outer);
        all.sort();
        let mut want = vec![a, b, c, inner];
        want.sort();
        assert_eq!(all, want);
        assert_eq!(doc.ancestors(a), vec![inner, outer]);
        assert_eq!(doc.top_level(a), outer);
        assert_eq!(doc.top_level(d), d);
        assert!(doc.is_inside(a, outer) && !doc.is_inside(d, outer));
        assert_eq!(doc.leaves(&[outer, d]), vec![a, b, c, d]);
        assert_eq!(doc.leaves(&[a]), vec![a]);
        let mut with = doc.with_members(&[inner]);
        with.sort();
        let mut want = vec![a, b, inner];
        want.sort();
        assert_eq!(with, want);
        assert!(check_tree(doc).is_ok());
        assert!(is_group_object(doc.object(outer).unwrap()) && !is_group_object(doc.object(a).unwrap()));
    }

    #[test]
    fn a_click_selects_the_top_group_and_entering_selects_members() {
        let mut ed = Editor::default();
        let (a, b, c) = (add(&mut ed, 0.0), add(&mut ed, 1.0), add(&mut ed, 2.0));
        let inner = group(&mut ed, &[a, b]);
        let outer = group(&mut ed, &[inner, c]);
        let doc = ed.doc();
        // Outside any group: the outermost group.
        assert_eq!(doc.selection_target(a, None), Some(outer));
        assert_eq!(doc.selection_target(c, None), Some(outer));
        assert_eq!(doc.selection_target(outer, None), Some(outer));
        // Inside the outer group: its direct members. A member group is selected as a whole.
        assert_eq!(doc.selection_target(a, Some(outer)), Some(inner));
        assert_eq!(doc.selection_target(c, Some(outer)), Some(c));
        // Inside the inner group: the leaves.
        assert_eq!(doc.selection_target(a, Some(inner)), Some(a));
        // Objects outside the context cannot be selected from it, and the context itself is not a member.
        assert_eq!(doc.selection_target(c, Some(inner)), None);
        assert_eq!(doc.selection_target(inner, Some(inner)), None);
        assert_eq!(doc.selection_target(ObjectId(999), None), None);
    }

    #[test]
    fn a_broken_tree_is_found() {
        let mut ed = Editor::default();
        let a = add(&mut ed, 0.0);
        let b = add(&mut ed, 1.0);
        let g = group(&mut ed, &[a, b]);
        let mut doc = ed.doc().clone();
        doc.object_mut(a).unwrap().parent = Some(ObjectId(999));
        assert!(check_tree(&doc).unwrap_err().contains("does not exist"));
        doc.object_mut(a).unwrap().parent = Some(b);
        assert!(check_tree(&doc).unwrap_err().contains("not a group"));
        doc.object_mut(a).unwrap().parent = Some(g);
        let layer = doc.layers[0].id;
        doc.layers.push(crate::model::Layer { id: crate::model::LayerId(900), name: "x".into(), kind: Default::default(), visible: true, locked: false, elevation: 0.0, scale: 100.0 });
        doc.object_mut(a).unwrap().layer = crate::model::LayerId(900);
        assert!(check_tree(&doc).unwrap_err().contains("layer"));
        doc.object_mut(a).unwrap().layer = layer;
        // A group inside itself.
        doc.object_mut(g).unwrap().parent = Some(g);
        assert!(check_tree(&doc).unwrap_err().contains("ancestor"));
        assert!(doc.ancestors(g).len() <= 1);
    }
}
