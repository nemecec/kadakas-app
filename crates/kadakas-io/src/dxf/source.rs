//! Source tracking for the DXF importer (IOP-22): the source id of a file and the tags of the
//! objects that it makes. The shape of the tags and their checks are in `kadakas_core::provenance`;
//! the hashes are in `revision`, so an update can compute the same hashes for a new file.

use kadakas_core::provenance::{self, BlockRef, ImportTag, InsertRef, LayerSource};
use serde_json::Value;

use super::parse::{Dxf, Entity};
use super::revision::{entity_hash, header_hash, hex};

/// Longest file name in a source id. The hash and the `#` need 17 characters of the 256.
const MAX_NAME: usize = 200;

/// Text for a tag: control characters become spaces, and it is cut to `max` characters. Empty text
/// becomes `empty`.
pub fn clean(s: &str, max: usize, empty: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect();
    let t = t.trim();
    if t.is_empty() {
        empty.to_string()
    } else {
        t.to_string()
    }
}

/// The source of one import.
#[derive(Clone, Debug)]
pub struct Tracking {
    /// File name, `#`, and the hash of the file header.
    pub id: String,
    /// The file name alone.
    pub name: String,
}

impl Tracking {
    pub fn new(name: &str, dxf: &Dxf) -> Tracking {
        let name = clean(name, MAX_NAME, "unnamed");
        Tracking {
            id: format!("{name}#{}", hex(header_hash(dxf))),
            name,
        }
    }

    /// The source of a layer that holds a DXF layer (or, with an empty name, the whole file).
    pub fn layer_source(&self, dxf_layer: &str) -> LayerSource {
        LayerSource {
            source: self.id.clone(),
            dxf_layer: clean(dxf_layer, provenance::MAX_TEXT, ""),
        }
    }

    /// The tag of the object that `e` makes, or None if the tag would not be valid.
    pub fn tag(&self, e: &Entity, path: &Path, part: Option<u32>) -> Option<Value> {
        let t = ImportTag {
            source: self.id.clone(),
            handle: e.handle(),
            kind: clean(&e.kind, 32, "?"),
            hash: hex(entity_hash(e)),
            block: path.block.clone(),
            via: path.via.clone(),
            part,
        };
        t.problem().is_none().then(|| t.to_value())
    }
}

/// How an entity was reached: the block it sits in and the INSERTs on the way.
#[derive(Clone, Debug, Default)]
pub struct Path {
    block: Option<BlockRef>,
    via: Vec<InsertRef>,
}

impl Path {
    /// The path of the content of a block that `insert` places. `copy` is the number of the copy
    /// of an array INSERT.
    pub fn enter(
        &self,
        insert: &Entity,
        block_name: &str,
        block_handle: Option<&str>,
        copy: Option<u32>,
    ) -> Path {
        let mut via = self.via.clone();
        via.push(InsertRef {
            handle: insert.handle(),
            hash: hex(entity_hash(insert)),
            copy,
        });
        Path {
            block: Some(BlockRef {
                name: clean(block_name, provenance::MAX_TEXT, "unnamed"),
                handle: block_handle.map(str::to_string),
            }),
            via,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::parse;

    fn dxf(header: &str) -> Dxf {
        parse::parse(
            format!("0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n")
                .as_bytes(),
        )
        .unwrap()
    }

    fn entity(handle: &str) -> Entity {
        Entity {
            kind: "LINE".into(),
            items: vec![(5, handle.into()), (10, "1".into())],
            children: vec![],
        }
    }

    #[test]
    fn the_source_id_has_the_name_and_the_header_hash() {
        let a = Tracking::new("plan.dxf", &dxf("9\n$ACADVER\n1\nAC1027\n"));
        let b = Tracking::new("plan.dxf", &dxf("9\n$ACADVER\n1\nAC1032\n"));
        assert!(a.id.starts_with("plan.dxf#") && a.id.len() == "plan.dxf#".len() + 16);
        assert_ne!(a.id, b.id, "two files with the same name differ");
        assert_eq!(a.layer_source("Walls").dxf_layer, "Walls");
        assert!(a.layer_source("Walls").problem().is_none());
    }

    #[test]
    fn names_are_cleaned_and_cut() {
        assert_eq!(clean("a\nb\u{7}c", 10, "x"), "a b c");
        assert_eq!(clean("  ", 10, "x"), "x");
        assert_eq!(clean("abcdef", 3, "x"), "abc");
        let t = Tracking::new(&"n".repeat(1000), &dxf(""));
        assert!(t.layer_source("").problem().is_none());
        assert!(t.id.chars().count() <= provenance::MAX_TEXT);
    }

    #[test]
    fn tags_carry_the_path_through_blocks() {
        let t = Tracking::new("p", &dxf(""));
        let root = Path::default();
        let tag = t.tag(&entity(" 2f "), &root, None).unwrap();
        let tag = ImportTag::of(&tag).unwrap();
        assert_eq!(tag.handle.as_deref(), Some("2F"));
        assert!(tag.block.is_none() && tag.via.is_empty());
        let inner = root.enter(&entity("A1"), "TREE", Some("B1"), Some(3));
        let deep = inner.enter(&entity("A2"), "LEAF\n", None, None);
        let tag = ImportTag::of(&t.tag(&entity("C"), &deep, Some(1)).unwrap()).unwrap();
        assert_eq!(tag.block.unwrap().name, "LEAF");
        assert_eq!(tag.via.len(), 2);
        assert_eq!(tag.via[0].copy, Some(3));
        assert_eq!(tag.part, Some(1));
        // A handle that is not a word is left out, and the tag stays valid.
        let tag = ImportTag::of(&t.tag(&entity("not valid!"), &root, None).unwrap()).unwrap();
        assert_eq!(tag.handle, None);
    }

    #[test]
    fn a_path_that_is_too_deep_gives_no_tag() {
        let t = Tracking::new("p", &dxf(""));
        let mut path = Path::default();
        for _ in 0..=provenance::MAX_DEPTH {
            path = path.enter(&entity("1"), "B", None, None);
        }
        assert!(t.tag(&entity("2"), &path, None).is_none());
    }
}
