//! References to layers, classes, sheets and objects in the parameters of a method.
//!
//! A [`LayerRef`], [`ClassRef`] or [`SheetRef`] takes an ID or the unique name of a layer, class or
//! sheet layer. When serde reads it, it looks the name up in the session (the document that
//! [`with_names`] set for the call) and keeps the ID, so the method gets IDs only. The rules:
//!
//! - A text that is an ID stays as it is (the method says if no such layer exists).
//! - A name is matched exactly first, then without regard to the case. One match is the answer;
//!   no match, or several, is an error (-32602) with the candidates in `details.candidates`.
//!
//! The execute command takes names too: [`resolve_command`] does it for one command, against the
//! document as it is when the command runs (inside a batch, after the commands before it).

use std::cell::RefCell;
use std::rc::Rc;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use kadakas_core::model::{Document, LayerKind};
use kadakas_core::uid::Uid;

use crate::RpcError;

/// The names of the layers, sheets and classes of the session, as the parameters see them.
#[derive(Clone, Debug, Default)]
pub struct Names {
    layers: Vec<(Uid, String)>,
    sheets: Vec<(Uid, String)>,
    classes: Vec<(Uid, String)>,
    /// A name that matches nothing stays as it is (the tests of the schemas use it).
    lenient: bool,
}

impl Names {
    pub fn of(doc: &Document) -> Names {
        Names {
            layers: doc
                .layers
                .iter()
                .map(|l| (l.id.0, l.name.clone()))
                .collect(),
            sheets: doc
                .layers
                .iter()
                .filter(|l| l.kind == LayerKind::Sheet)
                .map(|l| (l.id.0, l.name.clone()))
                .collect(),
            classes: doc
                .classes
                .iter()
                .map(|c| (c.id.0, c.name.clone()))
                .collect(),
            lenient: false,
        }
    }

    /// Names that are never looked up: a name stays a name. For tests of parameters without a
    /// document.
    pub fn lenient() -> Names {
        Names {
            lenient: true,
            ..Names::default()
        }
    }
}

thread_local! {
    static NAMES: RefCell<Option<Rc<Names>>> = const { RefCell::new(None) };
    static FAILURE: RefCell<Option<RpcError>> = const { RefCell::new(None) };
}

/// Runs `f` with the names that references resolve against.
pub fn with_names<R>(names: Names, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Rc<Names>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            NAMES.with(|n| *n.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(NAMES.with(|n| n.borrow_mut().replace(Rc::new(names))));
    f()
}

/// The error that a reference left (a name that is unknown or ambiguous), if there is one.
pub(super) fn take_failure() -> Option<RpcError> {
    FAILURE.with(|f| f.borrow_mut().take())
}

/// What kind of thing a reference names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Layer,
    Class,
    Sheet,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Layer => "layer",
            Kind::Class => "class",
            Kind::Sheet => "sheet layer",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Names that are near.

/// The distance between two names, counting inserted, removed and changed letters and swapped
/// neighbours.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// The known name that is nearest to `name`, if one is near enough to be a typing mistake.
pub fn nearest<'a>(name: &str, known: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let lower = name.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for k in known {
        let kl = k.to_ascii_lowercase();
        let mut d = distance(&lower, &kl);
        // `layers` for `layer`, `ids` for `id`, `site_model` for `site`: a name that contains the
        // other counts as a near miss.
        if kl.len() >= 3 && lower.len() >= 3 && (lower.contains(&kl) || kl.contains(&lower)) {
            d = d.min(1);
        }
        let limit = (kl.chars().count().max(lower.chars().count()) / 3).clamp(1, 3);
        if d <= limit && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| k)
}

fn candidates<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let mut v: Vec<String> = items.map(|n| format!("\"{n}\"")).take(13).collect();
    if v.len() == 13 {
        v.truncate(12);
        v.push("...".into());
    }
    v.join(", ")
}

// ---------------------------------------------------------------------------------------------
// The lookup.

/// A layer, class or sheet from an ID or a name. `Ok(None)` is an ID, which stays as it is.
fn look_up(names: &Names, kind: Kind, text: &str) -> Result<Option<Uid>, RpcError> {
    let t = text.trim();
    if Uid::parse(t).is_some() {
        return Ok(None);
    }
    let items = match kind {
        Kind::Layer => &names.layers,
        Kind::Class => &names.classes,
        Kind::Sheet => &names.sheets,
    };
    let label = kind.label();
    // The exact name wins; else the name without regard to the case. Several matches are an error.
    for same in [
        (|a: &str, b: &str| a == b) as fn(&str, &str) -> bool,
        |a: &str, b: &str| a.eq_ignore_ascii_case(b),
    ] {
        let hits: Vec<&(Uid, String)> = items.iter().filter(|(_, n)| same(n, t)).collect();
        match hits.as_slice() {
            [] => {}
            [(id, _)] => return Ok(Some(*id)),
            many => {
                let listed = many
                    .iter()
                    .map(|(id, n)| format!("\"{n}\" ({id})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(RpcError::invalid_params(format!(
                    "the name \"{t}\" matches {} {label}s: {listed}. Give the ID of the one you mean",
                    many.len()
                ))
                .with_details(json!({
                    "name": t,
                    "candidates": many.iter().map(|(id, n)| json!({"id": id, "name": n})).collect::<Vec<_>>(),
                })));
            }
        }
    }
    let near = nearest(t, items.iter().map(|(_, n)| n.as_str()));
    let hint = near
        .map(|n| format!(" Did you mean \"{n}\"?"))
        .unwrap_or_default();
    Err(RpcError::invalid_params(format!(
        "no {label} is named \"{t}\".{hint} The {label}s are: {}",
        if items.is_empty() {
            "none".to_string()
        } else {
            candidates(items.iter().map(|(_, n)| n.as_str()))
        }
    ))
    .with_details(json!({
        "name": t,
        "candidates": items.iter().map(|(id, n)| json!({"id": id, "name": n})).collect::<Vec<_>>(),
    })))
}

/// The text of the ID for `text`, or the error. Without names (outside a call) a name is an error,
/// unless the names are lenient.
fn resolve(kind: Kind, text: &str) -> Result<String, RpcError> {
    NAMES.with(|n| match n.borrow().as_deref() {
        Some(names) => match look_up(names, kind, text) {
            Ok(Some(id)) => Ok(id.to_string()),
            Ok(None) => Ok(text.to_string()),
            Err(_) if names.lenient => Ok(text.to_string()),
            Err(e) => Err(e),
        },
        None => Ok(text.to_string()),
    })
}

fn read<'de, D: Deserializer<'de>>(kind: Kind, d: D) -> Result<String, D::Error> {
    let text = String::deserialize(d)?;
    resolve(kind, &text).map_err(|e| {
        let message = e.message.clone();
        FAILURE.with(|f| *f.borrow_mut() = Some(e));
        D::Error::custom(message)
    })
}

macro_rules! reference {
    ($(#[$doc:meta])* $name:ident, $kind:expr, $label:literal, $what:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                read($kind, d).map($name)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "x-kadakas-ref": $label,
                    "description": $what
                })
            }
            fn inline_schema() -> bool {
                true
            }
        }
    };
}

reference!(
    /// A design layer: its ID or its unique name.
    LayerRef,
    Kind::Layer,
    "layer",
    "A design layer: its ID (UUID text) or its unique name"
);
reference!(
    /// A class: its ID or its unique name.
    ClassRef,
    Kind::Class,
    "class",
    "A class: its ID (UUID text) or its unique name"
);
reference!(
    /// A sheet layer: its ID or its unique name.
    SheetRef,
    Kind::Sheet,
    "sheet",
    "A sheet layer: its ID (UUID text) or its unique name"
);

// ---------------------------------------------------------------------------------------------
// The commands of execute.

/// Replaces the names in the `layer` and `class` of one command by IDs, against `names`. The
/// commands inside a `batch` are left to their turn (see [`crate::Session`]'s `execute`).
pub fn resolve_command(names: &Names, c: &mut Map<String, Value>) -> Result<(), RpcError> {
    for (key, kind) in [("layer", Kind::Layer), ("class", Kind::Class)] {
        let Some(Value::String(s)) = c.get_mut(key) else {
            continue;
        };
        if let Some(id) = look_up(names, kind, s)? {
            *s = id.to_string();
        }
    }
    Ok(())
}

/// True when the command has a `layer` or `class` that is not an ID: a name to look up.
pub fn has_names(c: &Map<String, Value>) -> bool {
    ["layer", "class"]
        .iter()
        .any(|k| matches!(c.get(*k), Some(Value::String(s)) if Uid::parse(s.trim()).is_none()))
}
