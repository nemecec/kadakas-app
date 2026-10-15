//! From a record of the base file to the objects that it made.
//!
//! An update diffs the stored records of the old revision with the new file (`diff`), and then
//! asks which objects each old record stands for. The answer comes from the tags
//! (`kadakas_core::provenance`):
//!
//! - an entity in the model made the objects with its handle (or, without handles, its hash),
//!   its type, and no block;
//! - an entity inside a block definition made one object for each INSERT that placed the block,
//!   and for each copy of an array INSERT. A change of the definition fans out to all of them:
//!   every object whose `block.key` is the definition and whose entity matches;
//! - an INSERT made no object itself. A change of the INSERT (moved, scaled, another block)
//!   concerns every object that has it in `via`.
//!
//! `part` tells the objects of one entity apart (the loops of a hatch). The caller maps all of
//! them.
//!
//! The tags of one document can come from several drawings: `drawing` limits the answer to the
//! objects of the drawing whose records are asked about.

use std::collections::HashMap;

use kadakas_core::model::ObjectId;
use kadakas_core::provenance::ImportTag;

use super::rec::{PAPER, Rec};
use crate::dxf::hash::hex;

fn same_entity(rec: &Rec, tag: &ImportTag) -> bool {
    tag.kind == rec.kind
        && match (&rec.handle, &tag.handle) {
            (Some(a), Some(b)) => a == b,
            _ => tag.hash == hex(rec.hash),
        }
}

/// True if the tag is of an object that `rec` made.
fn hits(rec: &Rec, tag: &ImportTag) -> bool {
    if rec.kind == "INSERT" {
        // The INSERT is one of the references on the way to the object.
        tag.via.iter().any(|v| match (&rec.handle, &v.handle) {
            (Some(a), Some(b)) => a == b,
            _ => v.hash == hex(rec.hash),
        })
    } else if rec.scope == PAPER {
        false
    } else if let Some(key) = rec.scope.strip_prefix("block:") {
        tag.block.as_ref().is_some_and(|b| b.key == key) && same_entity(rec, tag)
    } else {
        tag.block.is_none() && same_entity(rec, tag)
    }
}

/// The objects that `rec` made, among the tagged objects. The result is sorted by ID.
///
/// The cost is linear in the number of tagged objects. For many records use a [`TagIndex`].
pub fn objects_of<'a>(
    rec: &Rec,
    drawing: &str,
    tagged: impl IntoIterator<Item = (ObjectId, &'a ImportTag)>,
) -> Vec<ObjectId> {
    let mut out: Vec<ObjectId> = tagged
        .into_iter()
        .filter(|(_, t)| t.drawing == drawing && hits(rec, t))
        .map(|(id, _)| id)
        .collect();
    out.sort_by_key(|o| o.0);
    out.dedup();
    out
}

/// The tagged objects of one drawing, indexed, so that the objects of many records are found
/// without a pass over all objects for each record. It gives the same answer as [`objects_of`].
pub struct TagIndex<'a> {
    tags: Vec<(ObjectId, &'a ImportTag)>,
    by_handle: HashMap<(&'a str, &'a str), Vec<u32>>,
    by_hash: HashMap<(&'a str, &'a str), Vec<u32>>,
    by_insert_handle: HashMap<&'a str, Vec<u32>>,
    by_insert_hash: HashMap<&'a str, Vec<u32>>,
}

impl<'a> TagIndex<'a> {
    /// The index of the objects whose tag is of `drawing`.
    pub fn new(
        drawing: &str,
        tagged: impl IntoIterator<Item = (ObjectId, &'a ImportTag)>,
    ) -> TagIndex<'a> {
        let mut ix = TagIndex {
            tags: Vec::new(),
            by_handle: HashMap::new(),
            by_hash: HashMap::new(),
            by_insert_handle: HashMap::new(),
            by_insert_hash: HashMap::new(),
        };
        for (id, tag) in tagged.into_iter().filter(|(_, t)| t.drawing == drawing) {
            let n = ix.tags.len() as u32;
            ix.tags.push((id, tag));
            if let Some(h) = &tag.handle {
                ix.by_handle.entry((&tag.kind, h)).or_default().push(n);
            }
            ix.by_hash
                .entry((&tag.kind, &tag.hash))
                .or_default()
                .push(n);
            for v in &tag.via {
                if let Some(h) = &v.handle {
                    ix.by_insert_handle.entry(h).or_default().push(n);
                }
                ix.by_insert_hash.entry(&v.hash).or_default().push(n);
            }
        }
        ix
    }

    /// How many objects the index holds.
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// The objects that `rec` made, sorted by ID (see [`objects_of`]).
    pub fn objects_of(&self, rec: &Rec) -> Vec<ObjectId> {
        let hash = hex(rec.hash);
        let mut found: Vec<u32> = Vec::new();
        let mut take = |list: Option<&Vec<u32>>| {
            found.extend(list.into_iter().flatten().copied());
        };
        if rec.kind == "INSERT" {
            if let Some(h) = &rec.handle {
                take(self.by_insert_handle.get(h.as_str()));
            }
            take(self.by_insert_hash.get(hash.as_str()));
        } else {
            if let Some(h) = &rec.handle {
                take(self.by_handle.get(&(rec.kind.as_str(), h.as_str())));
            }
            take(self.by_hash.get(&(rec.kind.as_str(), hash.as_str())));
        }
        found.sort_unstable();
        found.dedup();
        let mut out: Vec<ObjectId> = found
            .into_iter()
            .map(|n| self.tags[n as usize])
            .filter(|(_, t)| hits(rec, t))
            .map(|(id, _)| id)
            .collect();
        out.sort_by_key(|o| o.0);
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::revision::MODEL;
    use kadakas_core::provenance::{BlockRef, InsertRef};
    use kadakas_core::uid::Uid;

    fn tag(handle: Option<&str>, kind: &str, hash: u64) -> ImportTag {
        ImportTag {
            drawing: "d".into(),
            revision: "r".into(),
            handle: handle.map(str::to_string),
            kind: kind.into(),
            hash: hex(hash),
            obj: hex(0),
            block: None,
            via: vec![],
            part: None,
        }
    }

    fn rec(handle: Option<&str>, kind: &str, scope: &str, hash: u64) -> Rec {
        Rec {
            handle: handle.map(str::to_string),
            kind: kind.into(),
            scope: scope.into(),
            hash,
            ..Rec::default()
        }
    }

    fn id(n: u64) -> ObjectId {
        ObjectId(Uid::from_num(n))
    }

    #[test]
    fn a_changed_block_definition_fans_out_to_every_instance() {
        let mut tags: Vec<(ObjectId, ImportTag)> = Vec::new();
        // Model entity, and a block entity placed three times (one INSERT with 2 copies, one more).
        tags.push((id(1), tag(Some("A"), "LINE", 1)));
        for (n, (insert, copy)) in [("I1", Some(0)), ("I1", Some(1)), ("I2", None)]
            .into_iter()
            .enumerate()
        {
            let mut t = tag(Some("B2"), "LINE", 2);
            t.block = Some(BlockRef {
                name: "TREE".into(),
                key: "rec:B1".into(),
                handle: None,
            });
            t.via = vec![InsertRef {
                handle: Some(insert.into()),
                hash: hex(9),
                copy,
            }];
            tags.push((id(10 + n as u64), t));
        }
        let mut other = tag(Some("B2"), "LINE", 2);
        other.block = Some(BlockRef {
            name: "OAK".into(),
            key: "rec:C1".into(),
            handle: None,
        });
        tags.push((id(20), other));
        let view = || tags.iter().map(|(i, t)| (*i, t));

        assert_eq!(
            objects_of(&rec(Some("A"), "LINE", MODEL, 1), "d", view()),
            vec![id(1)]
        );
        // The block entity: all three instances, none of the other block with the same handle.
        let def = rec(Some("B2"), "LINE", "block:rec:B1", 2);
        assert_eq!(objects_of(&def, "d", view()), vec![id(10), id(11), id(12)]);
        // The INSERT I1: its two copies. It made no object itself.
        let ins = rec(Some("I1"), "INSERT", MODEL, 7);
        assert_eq!(objects_of(&ins, "d", view()), vec![id(10), id(11)]);
        // A record without a handle is found by its hash; another type or scope does not match.
        let anon = rec(None, "LINE", MODEL, 1);
        assert_eq!(objects_of(&anon, "d", view()), vec![id(1)]);
        assert_eq!(
            objects_of(&rec(None, "LINE", MODEL, 99), "d", view()),
            vec![]
        );
        assert_eq!(
            objects_of(&rec(Some("A"), "CIRCLE", MODEL, 1), "d", view()),
            vec![]
        );
        assert_eq!(
            objects_of(&rec(Some("A"), "LINE", PAPER, 1), "d", view()),
            vec![]
        );
        let mut no_handle = tags.clone();
        no_handle[0].1.handle = None;
        assert_eq!(
            objects_of(&anon, "d", no_handle.iter().map(|(i, t)| (*i, t))),
            vec![id(1)]
        );
        // Only the objects of the drawing that is asked about.
        assert_eq!(objects_of(&def, "other", view()), vec![]);
        // An INSERT without a handle is found by its hash.
        let mut by_hash = tags.clone();
        by_hash[1].1.via[0].handle = None;
        by_hash[1].1.via[0].hash = hex(5);
        let ins = rec(None, "INSERT", MODEL, 5);
        assert_eq!(
            objects_of(&ins, "d", by_hash.iter().map(|(i, t)| (*i, t))),
            vec![id(10)]
        );
    }

    #[test]
    fn the_index_gives_the_answer_of_objects_of() {
        let mut tags: Vec<(ObjectId, ImportTag)> = Vec::new();
        tags.push((id(1), tag(Some("A"), "LINE", 1)));
        tags.push((id(2), tag(None, "LINE", 2)));
        tags.push((id(3), tag(Some("A"), "CIRCLE", 1)));
        for (n, (insert, hash)) in [(Some("I1"), 9u64), (None, 5), (Some("I2"), 9)]
            .into_iter()
            .enumerate()
        {
            let mut t = tag(Some("B2"), "LINE", 2);
            t.block = Some(BlockRef {
                name: "TREE".into(),
                key: "rec:B1".into(),
                handle: None,
            });
            t.via = vec![InsertRef {
                handle: insert.map(str::to_string),
                hash: hex(hash),
                copy: None,
            }];
            tags.push((id(10 + n as u64), t));
        }
        let mut other = tag(Some("A"), "LINE", 1);
        other.drawing = "other".into();
        tags.push((id(20), other));
        let view = || tags.iter().map(|(i, t)| (*i, t));
        let ix = TagIndex::new("d", view());
        assert_eq!(ix.len(), 6);
        assert!(!ix.is_empty());
        assert!(TagIndex::new("none", view()).is_empty());
        let recs = [
            rec(Some("A"), "LINE", MODEL, 1),
            rec(Some("A"), "LINE", MODEL, 99),
            rec(None, "LINE", MODEL, 2),
            rec(Some("A"), "CIRCLE", MODEL, 7),
            rec(Some("Z"), "LINE", MODEL, 1),
            rec(Some("B2"), "LINE", "block:rec:B1", 2),
            rec(Some("B2"), "LINE", "block:rec:other", 2),
            rec(Some("A"), "LINE", PAPER, 1),
            rec(Some("I1"), "INSERT", MODEL, 7),
            rec(None, "INSERT", MODEL, 5),
            rec(None, "INSERT", MODEL, 9),
            rec(Some("I9"), "INSERT", MODEL, 1),
        ];
        for r in &recs {
            assert_eq!(ix.objects_of(r), objects_of(r, "d", view()), "{r:?}");
        }
    }
}
