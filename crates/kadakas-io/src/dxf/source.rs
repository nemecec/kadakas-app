//! Source tracking for the DXF importer (IOP-22): the identity of a file and the tags of the
//! objects that it makes. The shape of the tags and their checks are in `kadakas_core::provenance`;
//! the hashes and the way to write handles and names are in `hash`, so an update computes the same
//! values for a new file.

use std::cell::RefCell;
use std::collections::HashMap;

use kadakas_core::provenance::{self, BlockRef, ImportTag, InsertRef, LayerSource};
use serde_json::Value;

use super::hash::{
    block_keys, clean_name, content_hash, entity_hash, header_guid, header_hash, hex,
};
use super::parse::{Block, Dxf, Entity};

/// Longest file name in an identity. The rest of the 256 characters is for a hash.
const MAX_NAME: usize = 200;

/// The identity and the helpers of one import.
///
/// `drawing` is the lineage: `$FINGERPRINTGUID`, else the file name. A file without a header gets
/// its name and a hash of its entities, so two such files with the same name are two drawings.
/// `revision` is `$VERSIONGUID`, else a hash of the header (for a file without a header, of the
/// entities).
pub struct Tracking {
    pub drawing: String,
    pub revision: String,
    /// The file name, for layer names and notes.
    pub name: String,
    /// The file has no header, so nothing says which drawing it is.
    pub headerless: bool,
    keys: HashMap<String, String>,
    /// The hash of each entity of this import, by address: an entity in a block is visited once
    /// for each INSERT.
    hashes: RefCell<HashMap<usize, u64>>,
}

impl Tracking {
    pub fn new(name: &str, dxf: &Dxf) -> Tracking {
        let name = clean_name(name, MAX_NAME, "unnamed");
        let headerless = dxf.header.is_empty();
        let drawing = match header_guid(dxf, "$FINGERPRINTGUID") {
            Some(g) => format!("guid:{g}"),
            None if headerless => format!("{name}#{}", hex(content_hash(dxf))),
            None => name.clone(),
        };
        let revision = match header_guid(dxf, "$VERSIONGUID") {
            Some(g) => format!("guid:{g}"),
            None if headerless => hex(content_hash(dxf)),
            None => hex(header_hash(dxf)),
        };
        Tracking {
            drawing,
            revision,
            name,
            headerless,
            keys: block_keys(dxf),
            hashes: RefCell::default(),
        }
    }

    fn hash_of(&self, e: &Entity) -> u64 {
        *self
            .hashes
            .borrow_mut()
            .entry(e as *const Entity as usize)
            .or_insert_with(|| entity_hash(e))
    }

    /// The source of a layer that holds a DXF layer (or, with an empty name, the whole file), with
    /// the transform of the import: `scale` mm per unit and the `shift` in mm.
    pub fn layer_source(&self, dxf_layer: &str, scale: f64, shift: [f64; 2]) -> LayerSource {
        LayerSource {
            drawing: self.drawing.clone(),
            revision: self.revision.clone(),
            dxf_layer: clean_name(dxf_layer, provenance::MAX_TEXT, ""),
            unit_scale: scale,
            shift,
        }
    }

    /// The tag of the object that `e` makes, or None if the tag would not be valid.
    pub fn tag(&self, e: &Entity, path: &Path, part: Option<u32>) -> Option<Value> {
        let t = ImportTag {
            drawing: self.drawing.clone(),
            revision: self.revision.clone(),
            handle: e.handle(),
            kind: clean_name(&e.kind, 32, "?"),
            hash: hex(self.hash_of(e)),
            obj: provenance::PENDING_OBJ.into(),
            block: path.block.clone(),
            via: path.via.clone(),
            part,
        };
        t.problem().is_none().then(|| t.to_value())
    }

    /// The path of the content of `block` that `insert` places. `copy` is the number of the copy
    /// of an array INSERT.
    pub fn enter(
        &self,
        path: &Path,
        insert: &Entity,
        block_name: &str,
        block: &Block,
        copy: Option<u32>,
    ) -> Path {
        let mut via = path.via.clone();
        via.push(InsertRef {
            handle: insert.handle(),
            hash: hex(self.hash_of(insert)),
            copy,
        });
        let key = self
            .keys
            .get(block_name)
            .cloned()
            .unwrap_or_else(|| format!("name:{}", clean_name(block_name, MAX_NAME, "unnamed")));
        Path {
            block: Some(BlockRef {
                name: clean_name(block_name, provenance::MAX_TEXT, "unnamed"),
                key: clean_name(&key, provenance::MAX_TEXT, "unnamed"),
                handle: block.handle.clone(),
            }),
            via,
        }
    }
}

/// How an entity was reached: the block it sits in and the INSERTs on the way.
#[derive(Clone, Debug, Default)]
pub struct Path {
    block: Option<BlockRef>,
    via: Vec<InsertRef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::parse;

    fn dxf(header: &str) -> Dxf {
        parse::parse(
            format!("0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n1\n0\nENDSEC\n0\nEOF\n")
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

    const FP: &str = "9\n$FINGERPRINTGUID\n2\n{AAA-111}\n";

    #[test]
    fn a_fingerprint_names_the_drawing_and_a_version_guid_the_revision() {
        let a = Tracking::new(
            "plan.dxf",
            &dxf(&format!("{FP}9\n$VERSIONGUID\n2\n{{BBB-1}}\n")),
        );
        let renamed = Tracking::new(
            "plan_v2.dxf",
            &dxf(&format!("{FP}9\n$VERSIONGUID\n2\n{{BBB-2}}\n")),
        );
        assert_eq!(a.drawing, "guid:{AAA-111}");
        assert_eq!(a.drawing, renamed.drawing, "a rename keeps the drawing");
        assert_eq!(a.revision, "guid:{BBB-1}");
        assert_ne!(a.revision, renamed.revision, "a save is a new revision");
        assert_eq!(
            a.layer_source("Walls", 1000.0, [0.0, 5.0]).dxf_layer,
            "Walls"
        );
        assert!(
            a.layer_source("Walls", 1000.0, [0.0, 5.0])
                .problem()
                .is_none()
        );
    }

    #[test]
    fn without_a_fingerprint_the_name_is_the_drawing() {
        let a = Tracking::new("plan.dxf", &dxf("9\n$ACADVER\n1\nAC1027\n"));
        let b = Tracking::new("plan.dxf", &dxf("9\n$ACADVER\n1\nAC1032\n"));
        assert_eq!(a.drawing, "plan.dxf");
        assert_eq!(a.drawing, b.drawing);
        assert_ne!(a.revision, b.revision, "the header hash is the revision");
        assert!(!a.headerless);
    }

    #[test]
    fn files_without_a_header_with_the_same_name_are_two_drawings() {
        let one = Tracking::new("old.dxf", &dxf(""));
        let other =
            parse::parse(b"0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n1\n40\n3\n0\nENDSEC\n0\nEOF\n")
                .unwrap();
        let two = Tracking::new("old.dxf", &other);
        assert!(one.headerless && two.headerless);
        assert_ne!(one.drawing, two.drawing);
        assert!(one.drawing.starts_with("old.dxf#"));
        // The same content under the same name is the same drawing and revision.
        let again = Tracking::new("old.dxf", &dxf(""));
        assert_eq!(
            (&one.drawing, &one.revision),
            (&again.drawing, &again.revision)
        );
    }

    #[test]
    fn names_are_cleaned_and_cut() {
        let t = Tracking::new(&format!("n\n{}", "n".repeat(1000)), &dxf(""));
        assert!(t.layer_source("", 1.0, [0.0; 2]).problem().is_none());
        assert!(t.drawing.chars().count() <= provenance::MAX_TEXT);
        assert!(!t.name.contains('\n'));
    }

    #[test]
    fn tags_carry_the_path_through_blocks() {
        let t = Tracking::new("p", &dxf(""));
        let root = Path::default();
        let tag = ImportTag::of(&t.tag(&entity(" 2f "), &root, None).unwrap()).unwrap();
        assert_eq!(tag.handle.as_deref(), Some("2F"));
        assert!(tag.block.is_none() && tag.via.is_empty());
        assert_eq!(tag.obj, provenance::PENDING_OBJ);
        let block = Block {
            handle: Some("B1".into()),
            ..Block::default()
        };
        let inner = t.enter(&root, &entity("A1"), "TREE", &block, Some(3));
        let deep = t.enter(&inner, &entity("A2"), "LEAF\n", &Block::default(), None);
        let tag = ImportTag::of(&t.tag(&entity("C"), &deep, Some(1)).unwrap()).unwrap();
        let b = tag.block.unwrap();
        assert_eq!((b.name.as_str(), b.key.as_str()), ("LEAF", "name:LEAF"));
        assert_eq!(tag.via.len(), 2);
        assert_eq!(tag.via[0].copy, Some(3));
        assert_eq!(tag.part, Some(1));
        let tag = ImportTag::of(&t.tag(&entity("not valid!"), &root, None).unwrap()).unwrap();
        assert_eq!(tag.handle, None);
        // Hashes are cached by address and stay the same.
        let e = entity("D");
        assert_eq!(t.hash_of(&e), t.hash_of(&e));
    }

    #[test]
    fn a_path_that_is_too_deep_gives_no_tag() {
        let t = Tracking::new("p", &dxf(""));
        let mut path = Path::default();
        for _ in 0..=provenance::MAX_DEPTH {
            path = t.enter(&path, &entity("1"), "B", &Block::default(), None);
        }
        assert!(t.tag(&entity("2"), &path, None).is_none());
    }
}
