//! Stable hashes of DXF content, and the one way to write handles and names.
//!
//! The hashes use FNV-1a with 64 bits (`Hasher`, from `kadakas_core::provenance`). They are not a
//! security check: a file that makes two entities collide only gets a wrong diff. They do not
//! depend on the platform or the build, so an importer can save them in the document and read
//! them again later. The parser does not use this module for anything but handles.

use std::collections::HashMap;

pub use kadakas_core::provenance::{Hasher, hex};

use super::parse::{self, Dxf, Entity};

/// Group codes whose values are numbers in the hash. Other values are hashed as trimmed text.
fn numeric_code(c: i32) -> bool {
    matches!(c, 10..=59 | 110..=149 | 210..=239 | 460..=469 | 1010..=1059)
}

/// Group codes that name other objects or give the writer's own bookkeeping: handles (5, 105),
/// owners and pointers (330 to 369, 390, 480, 481, 1005) and the markers of subclasses and groups
/// (100, 102). They change when a program writes the same drawing again, so the content hash
/// leaves them out.
fn bookkeeping_code(c: i32) -> bool {
    matches!(c, 5 | 100 | 102 | 105 | 330..=369 | 390 | 480 | 481 | 1005)
}

/// True for the name of an anonymous block (`*U12`, `*D3`, `*X1`): a program makes a new name at
/// each save. The names of model space and paper space are not anonymous blocks.
pub fn anonymous(name: &str) -> bool {
    name.starts_with('*') && !name.to_ascii_lowercase().contains("space")
}

pub(crate) fn hash_value(h: &mut Hasher, code: i32, value: &str) {
    h.u64(code as u64);
    match numeric_code(code)
        .then(|| parse::parse_f64(value))
        .flatten()
    {
        Some(v) => {
            h.u64(1);
            h.f64(v);
        }
        // The name of an anonymous block is not content.
        None if code == 2 && anonymous(value.trim()) => {
            h.u64(2);
        }
        None => {
            h.u64(0);
            h.str(value.trim());
        }
    }
}

/// The hash of the content of an entity: its type, all its values and the content of its
/// children (the vertices of a POLYLINE). It leaves out the handle and the bookkeeping codes.
pub fn entity_hash(e: &Entity) -> u64 {
    let mut h = Hasher::new();
    h.str(&e.kind);
    for (c, v) in e.items.iter().filter(|(c, _)| !bookkeeping_code(*c)) {
        hash_value(&mut h, *c, v);
    }
    h.u64(e.children.len() as u64);
    for ch in &e.children {
        h.u64(entity_hash(ch));
    }
    h.finish()
}

/// The hash of a file header: all variables, in order of name.
pub fn header_hash(dxf: &Dxf) -> u64 {
    let mut names: Vec<&String> = dxf.header.keys().collect();
    names.sort();
    let mut h = Hasher::new();
    h.u64(names.len() as u64);
    for n in names {
        h.str(n);
        let vals = &dxf.header[n];
        h.u64(vals.len() as u64);
        for (c, v) in vals {
            hash_value(&mut h, *c, v);
        }
    }
    h.finish()
}

/// The hash of all the entities of a file (the model and the blocks), in any order. It names a
/// file that has no header.
pub fn content_hash(dxf: &Dxf) -> u64 {
    let mut all: Vec<u64> = dxf.entities.iter().map(entity_hash).collect();
    all.extend(
        dxf.blocks
            .values()
            .flat_map(|b| b.entities.iter().map(entity_hash)),
    );
    all.sort_unstable();
    let mut h = Hasher::new();
    h.u64(all.len() as u64);
    all.into_iter().for_each(|x| h.u64(x));
    h.finish()
}

/// A hash of two texts, for the name of a stored resource.
pub fn content_hash_of(a: &str, b: &str) -> u64 {
    let mut h = Hasher::new();
    h.str(a);
    h.str(b);
    h.finish()
}

/// The normal form of a handle: trimmed, upper case. None for an empty one, one of more than 32
/// characters, or one with a character that is not a letter or a digit. Tags and records both use
/// this function.
pub fn normal_handle(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty() && t.len() <= 32 && t.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| t.to_ascii_uppercase())
}

/// Text for a tag or a key: control characters become spaces, and it is cut to `max` characters.
/// Empty text becomes `empty`. Tags and records both use this function for block names.
pub fn clean_name(s: &str, max: usize, empty: &str) -> String {
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

/// A header variable that holds a GUID, cleaned. None if the file has none or it is all zeros.
pub fn header_guid(dxf: &Dxf, var: &str) -> Option<String> {
    let g = clean_name(dxf.header_value(var)?, 200, "");
    (g.chars().any(|c| c.is_ascii_hexdigit() && c != '0')).then_some(g)
}

/// The key of each block definition, by block name. The key is what a record and a tag use to name
/// the definition: the BLOCK_RECORD handle, else the name, and for an anonymous block a hash of its
/// content (the name changes at each save).
pub fn block_keys(dxf: &Dxf) -> HashMap<String, String> {
    dxf.blocks
        .iter()
        .map(|(name, b)| {
            let key = if anonymous(name) {
                let mut h = Hasher::new();
                h.u64(b.entities.len() as u64);
                b.entities.iter().for_each(|e| h.u64(entity_hash(e)));
                format!("anon:{}", hex(h.finish()))
            } else if let Some(r) = &b.record {
                format!("rec:{r}")
            } else {
                format!("name:{}", clean_name(name, 200, "unnamed"))
            };
            (name.clone(), key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(kind: &str, items: &[(i32, &str)]) -> Entity {
        Entity {
            kind: kind.into(),
            items: items.iter().map(|(c, v)| (*c, v.to_string())).collect(),
            children: vec![],
        }
    }

    #[test]
    fn the_hash_ignores_handles_pointers_and_number_formats() {
        let a = ent(
            "LINE",
            &[
                (5, "1A"),
                (8, "W"),
                (10, "1.0"),
                (20, "2"),
                (330, "7"),
                (390, "3"),
                (1005, "9"),
            ],
        );
        let b = ent(
            "LINE",
            &[
                (5, "2B"),
                (8, "W"),
                (10, "1"),
                (20, "2.0000"),
                (330, "9"),
                (480, "1"),
            ],
        );
        assert_eq!(entity_hash(&a), entity_hash(&b));
        let c = ent("LINE", &[(5, "1A"), (8, "V"), (10, "1"), (20, "2")]);
        assert_ne!(entity_hash(&a), entity_hash(&c));
        let z = ent("LINE", &[(10, "-0.0")]);
        assert_eq!(entity_hash(&z), entity_hash(&ent("LINE", &[(10, "0")])));
        assert_eq!(
            entity_hash(&ent("TEXT", &[(1, " Oak ")])),
            entity_hash(&ent("TEXT", &[(1, "Oak")]))
        );
        assert_ne!(
            entity_hash(&ent("TEXT", &[(1, "oak")])),
            entity_hash(&ent("TEXT", &[(1, "Oak")]))
        );
        assert_ne!(
            entity_hash(&ent("T", &[(1, "ab"), (3, "c")])),
            entity_hash(&ent("T", &[(1, "a"), (3, "bc")]))
        );
        // The line weight (370) is content, not bookkeeping.
        assert_ne!(
            entity_hash(&ent("LINE", &[(370, "25")])),
            entity_hash(&ent("LINE", &[(370, "50")]))
        );
    }

    #[test]
    fn anonymous_block_names_are_not_content() {
        assert!(anonymous("*U12") && anonymous("*D3") && !anonymous("TREE"));
        assert!(!anonymous("*Model_Space") && !anonymous("*Paper_Space0"));
        let a = ent("INSERT", &[(2, "*U12"), (10, "1"), (20, "2")]);
        let b = ent("INSERT", &[(2, "*U99"), (10, "1"), (20, "2")]);
        assert_eq!(entity_hash(&a), entity_hash(&b));
        let named = ent("INSERT", &[(2, "TREE"), (10, "1"), (20, "2")]);
        assert_ne!(entity_hash(&a), entity_hash(&named));
        assert_ne!(
            entity_hash(&ent("INSERT", &[(2, "A")])),
            entity_hash(&ent("INSERT", &[(2, "B")]))
        );
    }

    #[test]
    fn children_count_in_the_hash() {
        let mut p = ent("POLYLINE", &[(70, "0")]);
        let h0 = entity_hash(&p);
        p.children.push(ent("VERTEX", &[(10, "1"), (20, "2")]));
        let h1 = entity_hash(&p);
        p.children[0].items[0].1 = "3".into();
        assert!(h0 != h1 && h1 != entity_hash(&p));
        assert_eq!(hex(0xab), "00000000000000ab");
    }

    #[test]
    fn handles_and_names_have_one_normal_form() {
        assert_eq!(normal_handle(" 2f ").as_deref(), Some("2F"));
        for bad in ["", "  ", "a b", "x!", &"F".repeat(33)] {
            assert_eq!(normal_handle(bad), None, "{bad:?}");
        }
        assert_eq!(clean_name("a\nb\u{7}c", 10, "x"), "a b c");
        assert_eq!(clean_name("  ", 10, "x"), "x");
        assert_eq!(clean_name("abcdef", 3, "x"), "abc");
    }

    fn file(header: &str, blocks: &str) -> Dxf {
        parse::parse(
            format!("0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n1\n0\nENDSEC\n0\nEOF\n")
                .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn header_hash_guids_and_content_hash() {
        let a = file("9\n$A\n70\n1\n", "");
        let b = file("9\n$B\n70\n1\n", "");
        assert_ne!(header_hash(&a), header_hash(&b));
        assert_eq!(header_hash(&a), header_hash(&file("9\n$A\n70\n1\n", "")));
        assert_eq!(
            content_hash(&a),
            content_hash(&b),
            "the content is the same"
        );
        let g = file(
            "9\n$FINGERPRINTGUID\n2\n{AB-12}\n9\n$VERSIONGUID\n2\n{000-0}\n",
            "",
        );
        assert_eq!(
            header_guid(&g, "$FINGERPRINTGUID").as_deref(),
            Some("{AB-12}")
        );
        assert_eq!(
            header_guid(&g, "$VERSIONGUID"),
            None,
            "all zeros is no GUID"
        );
        assert_eq!(header_guid(&g, "$NOPE"), None);
    }

    #[test]
    fn blocks_are_keyed_by_record_handle_name_or_content() {
        let blocks = "0\nBLOCK\n5\n20\n330\n1f\n2\nTREE\n0\nLINE\n10\n0\n0\nENDBLK\n\
0\nBLOCK\n5\n21\n2\nPLAIN\n0\nENDBLK\n\
0\nBLOCK\n5\n22\n2\n*U7\n0\nLINE\n10\n5\n0\nENDBLK\n\
0\nBLOCK\n5\n23\n2\n*U8\n0\nLINE\n10\n5\n0\nENDBLK\n";
        let keys = block_keys(&file("", blocks));
        assert_eq!(keys["TREE"], "rec:1F");
        assert_eq!(keys["PLAIN"], "name:PLAIN");
        assert!(keys["*U7"].starts_with("anon:"));
        assert_eq!(keys["*U7"], keys["*U8"], "the same content, another name");
    }
}
