//! One DXF entity as the matching engine sees it: identity (handle, scope, type), the properties
//! that a diff reports, a geometry summary for the nearest match, and stable hashes.
//!
//! The hashes are in `dxf::hash`. A record can be saved (`to_value`, `from_value`) as the base
//! side of a later update.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dxf::hash::{Hasher, anonymous, block_keys, entity_hash, hash_value};
use crate::dxf::parse::{self, Dxf, Entity};

/// Most points that a record keeps for the nearest match. Longer entities are summarized by the
/// first points; their hashes still cover everything.
pub const MAX_POINTS: usize = 4096;
/// Most size values (group codes 40 to 48) that a record keeps.
const MAX_SIZES: usize = 64;

/// Most points that a stored summary keeps (the same as the core check).
pub const MAX_STORED_POINTS: usize = kadakas_core::provenance::MAX_BASE_POINTS;

/// Group codes of the geometry: coordinates, sizes, angles and the extrusion direction.
fn geometry_code(c: i32) -> bool {
    matches!(c, 10..=59 | 210..=239)
}

/// Types whose flags and counts (70 to 73) are part of the shape (closed polylines, hatch loops).
fn shape_flags(kind: &str) -> bool {
    matches!(kind, "LWPOLYLINE" | "POLYLINE" | "SPLINE" | "HATCH")
}

/// A hash in JSON: 16 hexadecimal digits, because JSON numbers lose bits above 2^53.
mod hex_u64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&kadakas_core::provenance::hex(*v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let t = String::deserialize(d)?;
        u64::from_str_radix(&t, 16).map_err(serde::de::Error::custom)
    }
}

/// The scope of entities in the model.
pub const MODEL: &str = "";
/// The scope of entities in paper space.
pub const PAPER: &str = "*paper";

/// One entity, summarized.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rec {
    /// DXF handle (normal form), if the entity has one.
    pub handle: Option<String>,
    /// `MODEL`, `PAPER`, or `block:<key>` for an entity inside a block definition (the key is
    /// `hash::block_keys`: the BLOCK_RECORD handle, the name, or a content hash for an anonymous
    /// block). Matching never crosses scopes.
    pub scope: String,
    /// The name of the block definition for an entity in a named block (empty elsewhere, and for an
    /// anonymous block). A file that was saved again may give the definition a new BLOCK_RECORD
    /// handle, so `diff` pairs the definitions of two files by this name when the keys differ.
    /// Not stored: a stored summary (the base of an update) has no block names, so only two files
    /// that are read whole (a comparison) are paired this way.
    #[serde(skip)]
    pub block: String,
    /// The entity type, for example `LINE`.
    pub kind: String,
    pub layer: String,
    /// Color: the values of group codes 62, 420 and 430.
    pub color: String,
    pub linetype: String,
    /// Line weight (370) and linetype scale (48).
    pub lineweight: String,
    /// The text of TEXT, MTEXT and ATTRIB entities, and the name of an INSERT.
    pub text: String,
    /// Hash of all content, without the handle.
    #[serde(with = "hex_u64")]
    pub hash: u64,
    /// Hash of the type and the geometry only: it stays equal when the layer, color or text change.
    #[serde(with = "hex_u64")]
    pub geo_hash: u64,
    /// Points in drawing units (the first `MAX_POINTS`).
    pub points: Vec<[f64; 2]>,
    /// Size values (radius, height) in drawing units.
    pub sizes: Vec<f64>,
    /// Center and diagonal of the box around the points. Set by [`Rec::finish`].
    pub center: Option<[f64; 2]>,
    pub diag: f64,
}

impl Rec {
    /// A record of an entity in `scope`.
    pub fn from_entity(e: &Entity, scope: &str) -> Rec {
        let mut r = Rec {
            handle: e.handle(),
            scope: scope.to_string(),
            kind: e.kind.clone(),
            layer: e.layer().to_string(),
            hash: entity_hash(e),
            ..Rec::default()
        };
        let join = |codes: &[i32]| -> String {
            codes
                .iter()
                .filter_map(|c| e.get(*c).map(|v| format!("{c}={}", v.trim())))
                .collect::<Vec<_>>()
                .join(";")
        };
        r.color = join(&[62, 420, 430]);
        r.linetype = e.get(6).unwrap_or("").trim().to_string();
        r.lineweight = join(&[370, 48]);
        r.text = match e.kind.as_str() {
            "TEXT" | "MTEXT" | "ATTRIB" | "ATTDEF" => e
                .items
                .iter()
                .filter(|(c, _)| matches!(c, 1 | 3))
                .map(|(_, v)| v.as_str())
                .collect(),
            // The name of an anonymous block is not content.
            "INSERT" => Some(e.get(2).unwrap_or("").trim())
                .filter(|n| !anonymous(n))
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        };
        let mut g = Hasher::new();
        g.str(&e.kind);
        r.add_geometry(e, &mut g, true);
        for ch in &e.children {
            g.str(&ch.kind);
            r.add_geometry(ch, &mut g, false);
        }
        r.geo_hash = g.finish();
        r.finish();
        r
    }

    /// Hashes the geometry values of `e`, and keeps its points and sizes. The point of a POLYLINE
    /// header is an elevation, not a place, so only its vertices give points.
    fn add_geometry(&mut self, e: &Entity, g: &mut Hasher, top: bool) {
        let flags = shape_flags(&e.kind);
        let places = e.kind != "POLYLINE";
        let mut x: [Option<f64>; 9] = [None; 9];
        for (c, v) in &e.items {
            if !(geometry_code(*c) || (flags && matches!(c, 70..=73))) {
                continue;
            }
            hash_value(g, *c, v);
            let Some(n) = parse::parse_f64(v) else {
                continue;
            };
            match c {
                10..=18 => x[(c - 10) as usize] = Some(n),
                20..=28 => {
                    if let Some(px) = x[(c - 20) as usize].take()
                        && places
                        && self.points.len() < MAX_POINTS
                    {
                        self.points.push([px, n]);
                    }
                }
                40..=48 if top && self.sizes.len() < MAX_SIZES => self.sizes.push(n),
                _ => {}
            }
        }
    }

    /// Sets `center` and `diag` from the points. Call it after you set `points` by hand.
    pub fn finish(&mut self) {
        let pts: Vec<&[f64; 2]> = self
            .points
            .iter()
            .filter(|p| p[0].is_finite() && p[1].is_finite())
            .collect();
        if pts.is_empty() {
            self.center = None;
            self.diag = 0.0;
            return;
        }
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        let mut sum = [0.0; 2];
        for p in &pts {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
                sum[k] += p[k];
            }
        }
        let n = pts.len() as f64;
        let c = [sum[0] / n, sum[1] / n];
        self.center = (c[0].is_finite() && c[1].is_finite()).then_some(c);
        let d = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2)).sqrt();
        self.diag = if d.is_finite() { d } else { 0.0 };
    }

    /// The size of the entity: the diagonal of its box, or twice its largest size value (a radius,
    /// a text height).
    pub fn size(&self) -> f64 {
        let s = self
            .sizes
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(0.0, f64::max);
        self.diag.max(2.0 * s)
    }

    /// Cuts the points to what a stored summary keeps. The center and diagonal stay.
    pub fn compact(&mut self) {
        self.points.truncate(MAX_STORED_POINTS);
        self.block.clear();
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("a record is plain data")
    }

    /// Reads a stored summary. None if it is not one, or a number in it is not finite.
    pub fn from_value(v: &Value) -> Option<Rec> {
        let mut r: Rec = serde_json::from_value(v.clone()).ok()?;
        r.points.truncate(MAX_STORED_POINTS);
        r.sizes.truncate(MAX_SIZES);
        let finite = r
            .points
            .iter()
            .flatten()
            .chain(&r.sizes)
            .all(|v| v.is_finite())
            && r.diag.is_finite()
            && r.center.is_none_or(|c| c.iter().all(|v| v.is_finite()));
        finite.then_some(r)
    }

    /// The order key: the same for the same entity in any order of the input.
    pub(super) fn key(&self) -> (&str, Option<&str>, u64, &str, &str) {
        (
            &self.scope,
            self.handle.as_deref(),
            self.hash,
            &self.kind,
            &self.layer,
        )
    }
}

/// The records of a parsed file: the model, paper space and the entities of block definitions.
/// Stops at `max` records and says so with the second value.
pub fn records(dxf: &Dxf, max: usize) -> (Vec<Rec>, bool) {
    let keys = block_keys(dxf);
    let mut out = Vec::new();
    let mut names: Vec<&String> = dxf.blocks.keys().collect();
    names.sort();
    let all = dxf
        .entities
        .iter()
        .map(|e| (None, e))
        .chain(names.into_iter().flat_map(|n| {
            dxf.blocks[n]
                .entities
                .iter()
                .map(move |e| (Some(n.as_str()), e))
        }));
    for (block, e) in all {
        if out.len() >= max {
            return (out, true);
        }
        let scope = match block {
            Some(n) => format!("block:{}", keys[n]),
            None if e.int(67) == Some(1) => PAPER.to_string(),
            None => MODEL.to_string(),
        };
        let mut rec = Rec::from_entity(e, &scope);
        if let Some(n) = block.filter(|n| !anonymous(n)) {
            rec.block = n.to_string();
        }
        out.push(rec);
    }
    (out, false)
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
    fn records_split_properties_from_geometry() {
        let e = ent(
            "CIRCLE",
            &[
                (5, " 2f "),
                (8, "Trees"),
                (62, "3"),
                (6, "DASHED"),
                (370, "25"),
                (10, "10"),
                (20, "20"),
                (40, "5"),
            ],
        );
        let r = Rec::from_entity(&e, MODEL);
        assert_eq!(r.handle.as_deref(), Some("2F"));
        assert_eq!((r.layer.as_str(), r.color.as_str()), ("Trees", "62=3"));
        assert_eq!(
            (r.linetype.as_str(), r.lineweight.as_str()),
            ("DASHED", "370=25")
        );
        assert_eq!(r.points, vec![[10.0, 20.0]]);
        assert_eq!(r.sizes, vec![5.0]);
        assert_eq!(r.center, Some([10.0, 20.0]));
        // A layer change leaves the geometry hash alone.
        let mut e2 = e.clone();
        e2.items[1].1 = "Shrubs".into();
        let r2 = Rec::from_entity(&e2, MODEL);
        assert_eq!(r.geo_hash, r2.geo_hash);
        assert_ne!(r.hash, r2.hash);
        // A move changes it.
        e2.items[5].1 = "11".into();
        assert_ne!(r.geo_hash, Rec::from_entity(&e2, MODEL).geo_hash);
    }

    #[test]
    fn text_and_insert_names_are_kept() {
        let t = ent(
            "MTEXT",
            &[(3, "Hello "), (1, "world"), (10, "0"), (20, "0")],
        );
        assert_eq!(Rec::from_entity(&t, MODEL).text, "Hello world");
        let i = ent("INSERT", &[(2, "TREE"), (10, "4"), (20, "5")]);
        assert_eq!(Rec::from_entity(&i, MODEL).text, "TREE");
        assert_eq!(Rec::from_entity(&ent("LINE", &[]), MODEL).text, "");
    }

    #[test]
    fn polyline_vertices_give_the_points() {
        let mut p = ent("POLYLINE", &[(10, "0"), (20, "0"), (30, "5"), (70, "1")]);
        p.children.push(ent("VERTEX", &[(10, "10"), (20, "10")]));
        p.children.push(ent("VERTEX", &[(10, "20"), (20, "10")]));
        let r = Rec::from_entity(&p, MODEL);
        assert_eq!(r.points, vec![[10.0, 10.0], [20.0, 10.0]]);
    }

    #[test]
    fn long_entities_keep_a_bounded_summary() {
        let mut items = vec![(70, "0".to_string())];
        for i in 0..(MAX_POINTS + 10) {
            items.push((10, i.to_string()));
            items.push((20, "0".to_string()));
        }
        let e = Entity {
            kind: "LWPOLYLINE".into(),
            items,
            children: vec![],
        };
        let r = Rec::from_entity(&e, MODEL);
        assert_eq!(r.points.len(), MAX_POINTS);
        // The hash still sees the tail.
        let mut e2 = e.clone();
        e2.items.last_mut().unwrap().1 = "9".into();
        assert_ne!(r.geo_hash, Rec::from_entity(&e2, MODEL).geo_hash);
    }

    #[test]
    fn finish_ignores_points_that_are_not_finite() {
        let mut r = Rec {
            points: vec![[f64::NAN, 1.0], [f64::INFINITY, 0.0]],
            ..Rec::default()
        };
        r.finish();
        assert_eq!(r.center, None);
        r.points = vec![[1e308, 0.0], [-1e308, 0.0], [1e308, 1e308]];
        r.finish();
        assert_eq!(r.diag, 0.0, "an overflowing box is ignored");
        r.points = vec![[0.0, 0.0], [3.0, 4.0]];
        r.finish();
        assert_eq!((r.center, r.diag), (Some([1.5, 2.0]), 5.0));
    }

    #[test]
    fn records_of_a_file_cover_model_paper_and_blocks() {
        let text = "0\nSECTION\n2\nHEADER\n9\n$A\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n0\nLINE\n5\n9\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n1\n0\nTEXT\n5\n2\n67\n1\n1\nx\n0\nENDSEC\n0\nEOF\n";
        let d = parse::parse(text.as_bytes()).unwrap();
        let (recs, cut) = records(&d, 100);
        assert!(!cut);
        let scopes: Vec<&str> = recs.iter().map(|r| r.scope.as_str()).collect();
        assert_eq!(scopes, vec![MODEL, PAPER, "block:name:B"]);
        let (recs, cut) = records(&d, 2);
        assert!(cut && recs.len() == 2);
        // The header hash does not depend on the map order, and it sees the variables.
    }
}
