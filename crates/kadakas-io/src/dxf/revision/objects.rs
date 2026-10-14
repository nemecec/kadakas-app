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

/// The objects that `rec` made, among the tagged objects. The result is sorted by ID.
pub fn objects_of<'a>(
    rec: &Rec,
    drawing: &str,
    tagged: impl IntoIterator<Item = (ObjectId, &'a ImportTag)>,
) -> Vec<ObjectId> {
    let mut out: Vec<ObjectId> = Vec::new();
    for (id, tag) in tagged.into_iter().filter(|(_, t)| t.drawing == drawing) {
        let hit = if rec.kind == "INSERT" {
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
        };
        if hit {
            out.push(id);
        }
    }
    out.sort_by_key(|o| o.0);
    out.dedup();
    out
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
}
