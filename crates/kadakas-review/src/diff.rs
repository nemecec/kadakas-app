//! The diff between two states (REV-02).
//!
//! The result groups the things by kind (objects, layers, classes, resources, and the settings of
//! the document) and lists the ones that were added, removed or changed, by [`Uid`]. A changed
//! thing lists its property paths with the old and the new value. The paths are the properties
//! of collaboration (`shape`, `attrs.pen`, `data.Soil.Depth`, `drawing.project.name`), so the
//! paths of a diff are the properties that a sync of the same two states would write.
//!
//! # The stacking order
//!
//! A state keeps the stacking order as keys (`order`) that are made relative to the version
//! before, so the same order can have other keys in two versions. The diff does not compare keys.
//! A thing that both sides have is *moved* if it is not in the longest run of things (on its layer;
//! or among the layers) that keep their relative order. A moved thing has the path `order`, with
//! its position among the things that both sides have on the layer as the old and the new value.
//! Things that were added or removed do not count as moves of the others.
//!
//! The work and the size of the answer are bounded:
//!
//! - At most `limit` things are listed (counts are complete). `truncated` says that more exist.
//! - A thing lists at most [`MAX_PATHS`] paths.
//! - A value longer than [`MAX_VALUE_CHARS`] characters of JSON is replaced by a cut preview, its
//!   length and a hash of the whole text, so a change of a big value is still visible.
//! - A value is measured as it is written, into a writer that keeps the first 160 characters and
//!   a hash, never as one big text, and the bytes written count against [`MAX_VALUE_WORK`].
//! - After [`MAX_WORK`] compared properties (or [`MAX_VALUE_WORK`] bytes of values) the diff stops; `exact` is then false and the counts
//!   cover only what was compared.
//!
//! [`Uid`]: kadakas_core::uid::Uid

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kadakas_core::uid::Uid;
use kadakas_props::op::{Kind, Target};
use kadakas_props::replica::same_json;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::state::{Props, State};

/// The most things that a diff lists when the caller gives no limit.
pub const DEFAULT_LIMIT: usize = 100;
/// The most things that a diff lists, whatever the caller asks.
pub const MAX_LIMIT: usize = 1000;
/// The most paths listed for one changed thing.
pub const MAX_PATHS: usize = 50;
/// The longest value shown whole, in characters of JSON.
pub const MAX_VALUE_CHARS: usize = 160;
/// The most properties that one diff compares.
pub const MAX_WORK: usize = 5_000_000;
/// The most bytes of values that one diff measures (to cut and to hash them).
pub const MAX_VALUE_WORK: usize = 256 * 1024 * 1024;

/// A part of the document that a diff covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Objects,
    Layers,
    Classes,
    Resources,
    Document,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::Objects,
        Group::Layers,
        Group::Classes,
        Group::Resources,
        Group::Document,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Objects => "objects",
            Group::Layers => "layers",
            Group::Classes => "classes",
            Group::Resources => "resources",
            Group::Document => "document",
        }
    }

    pub fn parse(name: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.name() == name)
    }

    pub(crate) fn of(kind: Kind) -> Group {
        match kind {
            Kind::Object => Group::Objects,
            Kind::Layer => Group::Layers,
            Kind::Class => Group::Classes,
            Kind::Resource => Group::Resources,
            // A state holds no issues (they are review data, not part of a version).
            Kind::Document | Kind::Issue | Kind::Comment => Group::Document,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DiffOptions {
    /// The parts to compare. Empty: all.
    pub kinds: Vec<Group>,
    /// The most things to list (cut to [`MAX_LIMIT`]).
    pub limit: usize,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions {
            kinds: Vec::new(),
            limit: DEFAULT_LIMIT,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.added + self.removed + self.changed
    }
}

/// A value of a diff: whole, or cut with its length and a hash of the full JSON text.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Shown {
    Whole(Value),
    Cut {
        cut: String,
        len: usize,
        hash: String,
    },
}

/// A writer for the text of a value: it hashes everything, counts the characters and the bytes,
/// and keeps only the first [`MAX_VALUE_CHARS`] characters. The value is never held as one text.
struct Gauge {
    hasher: Sha256,
    chars: usize,
    bytes: usize,
    head: Vec<u8>,
}

impl Gauge {
    fn new() -> Gauge {
        Gauge {
            hasher: Sha256::new(),
            chars: 0,
            bytes: 0,
            head: Vec::new(),
        }
    }
}

impl std::io::Write for Gauge {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(buf);
        self.bytes += buf.len();
        for &b in buf {
            // A byte that starts a character (not a continuation byte of UTF-8).
            if b & 0xC0 != 0x80 {
                self.chars += 1;
            }
            if self.chars <= MAX_VALUE_CHARS {
                self.head.push(b);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Shown {
    pub fn of(v: &Value) -> Shown {
        Shown::measure(v).0
    }

    /// The value as the diff shows it, and the bytes that were written to measure it.
    fn measure(v: &Value) -> (Shown, usize) {
        match v {
            Value::Null | Value::Bool(_) | Value::Number(_) => return (Shown::Whole(v.clone()), 8),
            Value::String(s) if s.len() <= MAX_VALUE_CHARS => {
                return (Shown::Whole(v.clone()), s.len());
            }
            _ => {}
        }
        let mut gauge = Gauge::new();
        // A text is measured as it is; other values as their compact JSON.
        let _ = match v {
            Value::String(s) => std::io::Write::write_all(&mut gauge, s.as_bytes()),
            other => serde_json::to_writer(&mut gauge, other).map_err(std::io::Error::other),
        };
        let bytes = gauge.bytes;
        if gauge.chars <= MAX_VALUE_CHARS {
            return (Shown::Whole(v.clone()), bytes);
        }
        let hash: String = gauge
            .hasher
            .clone()
            .finalize()
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect();
        (
            Shown::Cut {
                cut: String::from_utf8_lossy(&gauge.head).into_owned(),
                len: gauge.chars,
                hash,
            },
            bytes,
        )
    }
}

/// One changed property.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropChange {
    pub path: String,
    /// Missing: the property did not exist before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<Shown>,
    /// Missing: the property was removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<Shown>,
}

/// A thing that exists on one side only.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub id: Uid,
    #[serde(flatten)]
    pub info: Map<String, Value>,
}

/// A thing on both sides with different properties.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Changed {
    pub id: Uid,
    #[serde(flatten)]
    pub info: Map<String, Value>,
    pub properties: Vec<PropChange>,
    /// True if the thing has more changed properties than listed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub properties_truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Listing {
    pub added: Vec<Entry>,
    pub removed: Vec<Entry>,
    pub changed: Vec<Changed>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diff {
    /// The counts by part: "objects", "layers", "classes", "resources" and "document".
    pub counts: BTreeMap<&'static str, Counts>,
    pub totals: Counts,
    /// True if a thing is not listed because of the limit.
    pub truncated: bool,
    /// False if the work limit stopped the diff: the counts are then partial.
    pub exact: bool,
    #[serde(flatten)]
    pub groups: BTreeMap<&'static str, Listing>,
}

/// What a person needs to recognize a thing: the kind of shape and the layer, or the name.
fn info(target: &Target, props: &Props) -> Map<String, Value> {
    let mut m = Map::new();
    let mut take = |from: &str, to: &str| {
        if let Some(v) = props.get(from) {
            m.insert(to.to_string(), v.clone());
        }
    };
    match target.kind {
        Kind::Object => {
            take("layer", "layer");
            take("class", "class");
            if let Some(kind) = props
                .get("shape")
                .and_then(|s| s.get("kind"))
                .and_then(Value::as_str)
            {
                m.insert("shape".into(), Value::String(kind.to_string()));
            }
        }
        _ => take("name", "name"),
    }
    m
}

/// The index of the items that stay in the longest increasing run of `seq` (patience sorting).
fn longest_run(seq: &[usize]) -> Vec<bool> {
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; seq.len()];
    for (i, v) in seq.iter().enumerate() {
        let k = tails.partition_point(|t| seq[*t] < *v);
        prev[i] = k.checked_sub(1).map(|j| tails[j]);
        if k == tails.len() {
            tails.push(i);
        } else {
            tails[k] = i;
        }
    }
    let mut keep = vec![false; seq.len()];
    let mut at = tails.last().copied();
    while let Some(i) = at {
        keep[i] = true;
        at = prev[i];
    }
    keep
}

/// The things that moved in the stacking order, with their old and new position among the things
/// that both states have in the same list (the objects of a layer, or the layers).
pub(crate) fn moved_things(old: &State, new: &State) -> HashMap<Target, (usize, usize)> {
    let key = |p: &Props| {
        p.get("order")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    // The list of a thing: its kind and, for an object, its layer in both states.
    let list_of = |t: &Target, p: &Props| match t.kind {
        Kind::Layer => Some("layers".to_string()),
        Kind::Object => p
            .get("layer")
            .and_then(Value::as_str)
            .map(|l| format!("objects {l}")),
        _ => None,
    };
    let mut lists: BTreeMap<String, Vec<(String, String, Target)>> = BTreeMap::new();
    for (t, a) in old.things() {
        if let Some(b) = new.get(t)
            && let (Some(la), Some(lb)) = (list_of(t, a), list_of(t, b))
            && la == lb
        {
            lists.entry(la).or_default().push((key(a), key(b), *t));
        }
    }
    let mut out = HashMap::new();
    for items in lists.values_mut() {
        items.sort_by(|x, y| (&x.0, x.2).cmp(&(&y.0, y.2)));
        let mut by_new: Vec<usize> = (0..items.len()).collect();
        by_new.sort_by(|a, b| (&items[*a].1, items[*a].2).cmp(&(&items[*b].1, items[*b].2)));
        let mut new_rank = vec![0usize; items.len()];
        for (rank, i) in by_new.iter().enumerate() {
            new_rank[*i] = rank;
        }
        let keep = longest_run(&new_rank);
        for (i, (_, _, t)) in items.iter().enumerate() {
            if !keep[i] {
                out.insert(*t, (i, new_rank[i]));
            }
        }
    }
    out
}

/// True if the properties are the same, not looking at the stacking key.
pub(crate) fn same_without_order(a: &Props, b: &Props) -> bool {
    let n = |p: &Props| p.len() - usize::from(p.contains_key("order"));
    n(a) == n(b)
        && a.iter()
            .filter(|(k, _)| *k != "order")
            .all(|(k, v)| b.get(k).is_some_and(|w| same_json(v, w)))
}

/// The changed properties of a thing, at most [`MAX_PATHS`]; true if there are more. `moved` is
/// the old and new position of a thing that moved in the stacking order.
fn changes(
    old: &Props,
    new: &Props,
    moved: Option<(usize, usize)>,
) -> (Vec<PropChange>, bool, usize) {
    let paths: BTreeSet<&String> = old
        .keys()
        .chain(new.keys())
        .filter(|k| *k != "order")
        .collect();
    let mut out = Vec::new();
    let mut more = false;
    let mut written = 0;
    for p in paths {
        let (o, n) = (old.get(p), new.get(p));
        let same = match (o, n) {
            (Some(a), Some(b)) => same_json(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            continue;
        }
        if out.len() >= MAX_PATHS {
            more = true;
            break;
        }
        let (old, a) = o.map_or((None, 0), |v| {
            let (s, b) = Shown::measure(v);
            (Some(s), b)
        });
        let (new, b) = n.map_or((None, 0), |v| {
            let (s, b) = Shown::measure(v);
            (Some(s), b)
        });
        written += a + b;
        out.push(PropChange {
            path: p.clone(),
            old,
            new,
        });
    }
    if let Some((was, is)) = moved {
        if out.len() >= MAX_PATHS {
            more = true;
        } else {
            out.push(PropChange {
                path: "order".into(),
                old: Some(Shown::Whole(Value::from(was))),
                new: Some(Shown::Whole(Value::from(is))),
            });
        }
    }
    (out, more, written)
}

/// Compares two states.
pub fn diff(old: &State, new: &State, opts: &DiffOptions) -> Diff {
    diff_with_budget(old, new, opts, MAX_WORK, MAX_VALUE_WORK)
}

fn diff_with_budget(
    old: &State,
    new: &State,
    opts: &DiffOptions,
    max_work: usize,
    max_value_work: usize,
) -> Diff {
    let want = |g: Group| opts.kinds.is_empty() || opts.kinds.contains(&g);
    let limit = opts.limit.min(MAX_LIMIT);
    let mut out = Diff {
        counts: Group::ALL
            .iter()
            .filter(|g| want(**g))
            .map(|g| (g.name(), Counts::default()))
            .collect(),
        totals: Counts::default(),
        truncated: false,
        exact: true,
        groups: Group::ALL
            .iter()
            .filter(|g| want(**g))
            .map(|g| (g.name(), Listing::default()))
            .collect(),
    };
    let targets: BTreeSet<Target> = old
        .things()
        .map(|(t, _)| *t)
        .chain(new.things().map(|(t, _)| *t))
        .collect();
    let moved = moved_things(old, new);
    let mut listed = 0usize;
    let mut work = 0usize;
    let mut value_work = 0usize;
    for t in targets {
        let g = Group::of(t.kind);
        if !want(g) {
            continue;
        }
        let (a, b) = (old.get(&t), new.get(&t));
        work += a.map_or(0, Map::len) + b.map_or(0, Map::len) + 1;
        if work > max_work {
            out.exact = false;
            out.truncated = true;
            break;
        }
        let name = g.name();
        let room = listed < limit;
        match (a, b) {
            (None, Some(p)) | (Some(p), None) => {
                let added = a.is_none();
                let c = out.counts.get_mut(name).expect("a wanted group");
                if added {
                    c.added += 1;
                    out.totals.added += 1;
                } else {
                    c.removed += 1;
                    out.totals.removed += 1;
                }
                if room {
                    let e = Entry {
                        id: t.id,
                        info: info(&t, p),
                    };
                    let l = out.groups.get_mut(name).expect("a wanted group");
                    if added { &mut l.added } else { &mut l.removed }.push(e);
                    listed += 1;
                } else {
                    out.truncated = true;
                }
            }
            (Some(a), Some(b)) => {
                let moved_by = moved.get(&t).copied();
                if moved_by.is_none() && same_without_order(a, b) {
                    continue;
                }
                out.counts.get_mut(name).expect("a wanted group").changed += 1;
                out.totals.changed += 1;
                if room {
                    let (properties, properties_truncated, written) = changes(a, b, moved_by);
                    value_work += written;
                    if value_work > max_value_work {
                        out.exact = false;
                        out.truncated = true;
                        break;
                    }
                    out.groups
                        .get_mut(name)
                        .expect("a wanted group")
                        .changed
                        .push(Changed {
                            id: t.id,
                            info: info(&t, b),
                            properties,
                            properties_truncated,
                        });
                    listed += 1;
                } else {
                    out.truncated = true;
                }
            }
            (None, None) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;
