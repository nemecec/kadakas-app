//! The diff between two states (REV-02).
//!
//! The result groups the things by kind (objects, layers, classes, resources, and the settings of
//! the document) and lists the ones that were added, removed or changed, by [`Uid`]. A changed
//! thing lists its property paths with the old and the new value. The paths are the properties
//! of collaboration (`shape`, `attrs.pen`, `data.Soil.Depth`, `drawing.project.name`), so the
//! paths of a diff are the properties that a sync of the same two states would write.
//!
//! The work and the size of the answer are bounded:
//!
//! - At most `limit` things are listed (counts are complete). `truncated` says that more exist.
//! - A thing lists at most [`MAX_PATHS`] paths.
//! - A value longer than [`MAX_VALUE_CHARS`] characters of JSON is replaced by a cut preview, its
//!   length and a hash of the whole text, so a change of a big value is still visible.
//! - After [`MAX_WORK`] compared properties the diff stops; `exact` is then false and the counts
//!   cover only what was compared.
//!
//! [`Uid`]: kadakas_core::uid::Uid

use std::collections::{BTreeMap, BTreeSet};

use kadakas_core::uid::Uid;
use kadakas_props::blob::sha256_hex;
use kadakas_props::op::{Kind, Target};
use kadakas_props::replica::same_json;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::state::{Props, State, same_props};

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

    fn of(kind: Kind) -> Group {
        match kind {
            Kind::Object => Group::Objects,
            Kind::Layer => Group::Layers,
            Kind::Class => Group::Classes,
            Kind::Resource => Group::Resources,
            Kind::Document => Group::Document,
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

impl Shown {
    pub fn of(v: &Value) -> Shown {
        // Short scalars and small values are the common case: measure without a full text first.
        let text = match v {
            Value::String(s) if s.len() <= MAX_VALUE_CHARS => return Shown::Whole(v.clone()),
            Value::Null | Value::Bool(_) | Value::Number(_) => return Shown::Whole(v.clone()),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let len = text.chars().count();
        if len <= MAX_VALUE_CHARS {
            return Shown::Whole(v.clone());
        }
        Shown::Cut {
            cut: text.chars().take(MAX_VALUE_CHARS).collect(),
            len,
            hash: sha256_hex(text.as_bytes())[..16].to_string(),
        }
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

/// The changed properties of a thing, at most [`MAX_PATHS`]; true if there are more.
fn changes(old: &Props, new: &Props) -> (Vec<PropChange>, bool) {
    let paths: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    let mut out = Vec::new();
    let mut more = false;
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
        out.push(PropChange {
            path: p.clone(),
            old: o.map(Shown::of),
            new: n.map(Shown::of),
        });
    }
    (out, more)
}

/// Compares two states.
pub fn diff(old: &State, new: &State, opts: &DiffOptions) -> Diff {
    diff_with_budget(old, new, opts, MAX_WORK)
}

fn diff_with_budget(old: &State, new: &State, opts: &DiffOptions, max_work: usize) -> Diff {
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
    let mut listed = 0usize;
    let mut work = 0usize;
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
                if same_props(a, b) {
                    continue;
                }
                out.counts.get_mut(name).expect("a wanted group").changed += 1;
                out.totals.changed += 1;
                if room {
                    let (properties, properties_truncated) = changes(a, b);
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
