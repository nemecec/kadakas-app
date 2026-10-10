//! Operations: the units of change that clients send and the server orders.
//!
//! An operation works on one thing, named by a [`Target`]: a kind and a [`Uid`]. There are five
//! operations. `Create` makes a thing with all its properties. `Delete` removes it for good.
//! `Restore` brings back a thing that the same user deleted (this is how undo of a delete works).
//! `Set` and `Unset` change or remove one property of a thing.
//!
//! A property is a top-level field of the thing in its JSON form, for example `shape`, `attrs`,
//! `layer` or `name`. A property is the smallest unit that can win or lose a conflict.
//!
//! A [`Txn`] groups the operations of one user action (one command). The server gives a whole
//! transaction one sequence number, so other users never see half of a command.

use std::collections::BTreeMap;

use kadakas_core::model::{FIRST_LAYER, NONE_CLASS};
use kadakas_core::uid::Uid;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn is_false(b: &bool) -> bool {
    !*b
}

/// The most operations in one transaction.
pub const MAX_OPS: usize = 20_000;
/// The longest property name.
pub const MAX_PROP_LEN: usize = 256;
/// The deepest nesting of a property value (arrays and objects). `serde_json` stops reading at
/// 128 levels; a value must stay far below.
pub const MAX_JSON_DEPTH: usize = 64;
/// The largest value of one property, in bytes of JSON. Pictures (an underlay) are the reason
/// for the size.
pub const MAX_VALUE_BYTES: usize = 32 * 1024 * 1024;
/// The largest transaction, in bytes of JSON. A client splits bigger changes into several
/// transactions (see `docsync::split`).
pub const MAX_TXN_BYTES: usize = 48 * 1024 * 1024;
/// The largest message (one frame) that a server must accept or send, except a snapshot.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
/// The largest snapshot frame.
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024 * 1024;

/// What kind of thing an operation works on. Each kind has its own space of IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Object,
    Layer,
    Class,
    Resource,
    /// The settings of the document itself (georeference, drawing data). There is exactly one,
    /// with the nil ID. It always exists and nobody can delete it.
    Document,
}

impl Kind {
    /// All kinds, in the order that a document is built: things that others refer to come first.
    pub const ALL: [Kind; 5] = [
        Kind::Layer,
        Kind::Class,
        Kind::Resource,
        Kind::Object,
        Kind::Document,
    ];
}

/// A thing in the document: its kind and its ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Target {
    pub kind: Kind,
    pub id: Uid,
}

impl Target {
    pub const fn new(kind: Kind, id: Uid) -> Target {
        Target { kind, id }
    }

    /// The one settings thing of the document.
    pub const DOCUMENT: Target = Target::new(Kind::Document, Uid::NIL);

    /// The things that nobody can delete: the document settings, the first layer and the "None"
    /// class. Every document has them, with the same IDs, so they cannot be made twice.
    pub fn is_protected(&self) -> bool {
        match self.kind {
            Kind::Document => true,
            Kind::Layer => self.id == FIRST_LAYER.0,
            Kind::Class => self.id == NONE_CLASS.0,
            _ => false,
        }
    }
}

/// Who wrote something: a client instance and the number of its transaction.
///
/// The `client` is a UUID that a client instance makes for itself. It is not the user: one user
/// can work on two devices, in two sessions, or start again with a lost journal, and each of them
/// has its own client ID, so their transaction numbers never meet. Transaction numbers count up
/// from 1 for each client ID. They are strictly increasing, and the client saves its counter with
/// its offline log. The pair (nil, 0) is the "baseline" writer of the first content of a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Writer {
    pub client: Uid,
    pub op: u64,
}

impl Writer {
    /// The writer of the content that a document had before collaboration began.
    pub const BASELINE: Writer = Writer {
        client: Uid::NIL,
        op: 0,
    };
}

/// One operation. In JSON it has an `op` field with the snake_case name of the variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Makes a thing with these properties. It fails if the ID was ever used (also by a thing
    /// that was deleted).
    Create {
        target: Target,
        props: BTreeMap<String, Value>,
        /// For things with a derived ID (`Uid::derive`), which a program makes again and again
        /// from one source: if the thing exists already, the properties are written as `set`
        /// operations (last writer wins) instead of the create being refused. Two users who make
        /// the same thing at the same time then end with one thing, with the properties of the
        /// writer who came last. For all other IDs the flag is false and a repeat is refused.
        #[serde(default, skip_serializing_if = "is_false")]
        merge: bool,
    },
    /// Deletes a thing. Edits that come after it are dropped.
    Delete { target: Target },
    /// Brings back a thing, if it is deleted and `deleted_by` is the writer of that deletion.
    Restore { target: Target, deleted_by: Writer },
    /// Sets one property.
    Set {
        target: Target,
        prop: String,
        value: Value,
    },
    /// Removes one property.
    Unset { target: Target, prop: String },
}

impl Op {
    pub fn target(&self) -> Target {
        match self {
            Op::Create { target, .. }
            | Op::Delete { target }
            | Op::Restore { target, .. }
            | Op::Set { target, .. }
            | Op::Unset { target, .. } => *target,
        }
    }

    /// The property this operation changes, if it changes only one.
    pub fn prop(&self) -> Option<&str> {
        match self {
            Op::Set { prop, .. } | Op::Unset { prop, .. } => Some(prop),
            _ => None,
        }
    }
}

/// A transaction: the operations of one user action, in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Txn {
    /// The number of this transaction for its user (see [`Writer`]).
    pub op_id: u64,
    pub ops: Vec<Op>,
}

/// Why a list of operations is not well formed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTxn {
    #[error("a transaction needs at least one operation")]
    Empty,
    #[error("a transaction can hold at most {MAX_OPS} operations")]
    TooMany,
    #[error("property names are 1 to {MAX_PROP_LEN} characters and never `id`: {0:?}")]
    BadProp(String),
    #[error("the settings of the document cannot be created")]
    CreateDocument,
    #[error("`merge` is only for objects with a derived ID: {0}")]
    BadMerge(Uid),
    #[error("a value of {0:?} is nested deeper than {MAX_JSON_DEPTH} levels")]
    TooDeep(String),
    #[error(
        "a value of {0:?} is larger than {MAX_VALUE_BYTES} bytes, or the transaction is larger than {MAX_TXN_BYTES} bytes"
    )]
    TooLarge(String),
    #[error("a value of {0:?} holds a number that is not finite")]
    NotFinite(String),
}

/// Walks a JSON value: its depth, its size in bytes (an estimate that is never below half of the
/// text) and whether all numbers are finite. Stops early when a limit is crossed.
fn scan(v: &Value, depth: usize, size: &mut usize) -> Result<(), &'static str> {
    if depth > MAX_JSON_DEPTH {
        return Err("deep");
    }
    *size += 8;
    if *size > MAX_TXN_BYTES {
        return Err("large");
    }
    match v {
        Value::Number(n) => {
            if n.as_f64().is_none_or(|f| !f.is_finite()) {
                return Err("finite");
            }
        }
        Value::String(s) => *size += s.len(),
        Value::Array(a) => {
            for x in a {
                scan(x, depth + 1, size)?;
            }
        }
        Value::Object(m) => {
            for (k, x) in m {
                *size += k.len();
                scan(x, depth + 1, size)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_value(prop: &str, v: &Value, total: &mut usize) -> Result<(), InvalidTxn> {
    let mut size = 0;
    scan(v, 0, &mut size).map_err(|why| match why {
        "deep" => InvalidTxn::TooDeep(prop.to_string()),
        "finite" => InvalidTxn::NotFinite(prop.to_string()),
        _ => InvalidTxn::TooLarge(prop.to_string()),
    })?;
    if size > MAX_VALUE_BYTES {
        return Err(InvalidTxn::TooLarge(prop.to_string()));
    }
    *total += size;
    if *total > MAX_TXN_BYTES {
        return Err(InvalidTxn::TooLarge(prop.to_string()));
    }
    Ok(())
}

/// The size of an operation in bytes of JSON, estimated (never far below the real size).
pub fn op_size(op: &Op) -> usize {
    let mut size = 32;
    match op {
        Op::Create { props, .. } => {
            for (k, v) in props {
                size += k.len();
                let _ = scan(v, 0, &mut size);
            }
        }
        Op::Set { prop, value, .. } => {
            size += prop.len();
            let _ = scan(value, 0, &mut size);
        }
        Op::Unset { prop, .. } => size += prop.len(),
        _ => {}
    }
    size
}

fn check_prop(prop: &str) -> Result<(), InvalidTxn> {
    if prop.is_empty() || prop.len() > MAX_PROP_LEN || prop == "id" {
        return Err(InvalidTxn::BadProp(prop.to_string()));
    }
    Ok(())
}

/// Checks the form of a list of operations. The server runs this on every submit. It does not
/// look at the document: conflicts are not errors (see the module `replica`).
pub fn validate_ops(ops: &[Op]) -> Result<(), InvalidTxn> {
    if ops.is_empty() {
        return Err(InvalidTxn::Empty);
    }
    if ops.len() > MAX_OPS {
        return Err(InvalidTxn::TooMany);
    }
    let mut total = 0;
    for op in ops {
        match op {
            Op::Create {
                target,
                props,
                merge,
            } => {
                if target.kind == Kind::Document {
                    return Err(InvalidTxn::CreateDocument);
                }
                if *merge && (target.kind != Kind::Object || !target.id.is_derived()) {
                    return Err(InvalidTxn::BadMerge(target.id));
                }
                for (k, v) in props {
                    check_prop(k)?;
                    check_value(k, v, &mut total)?;
                }
            }
            Op::Set { prop, value, .. } => {
                check_prop(prop)?;
                check_value(prop, value, &mut total)?;
            }
            Op::Unset { prop, .. } => check_prop(prop)?,
            Op::Delete { .. } | Op::Restore { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(n: u64) -> Target {
        Target::new(Kind::Object, Uid::from_num(n))
    }

    #[test]
    fn json_form_is_stable() {
        let op = Op::Set {
            target: t(7),
            prop: "name".into(),
            value: json!("a"),
        };
        let text = serde_json::to_string(&op).unwrap();
        assert!(text.contains(r#""op":"set""#));
        assert_eq!(serde_json::from_str::<Op>(&text).unwrap(), op);
        let op = Op::Restore {
            target: t(7),
            deleted_by: Writer::BASELINE,
        };
        let back: Op = serde_json::from_str(&serde_json::to_string(&op).unwrap()).unwrap();
        assert_eq!(back, op);
    }

    #[test]
    fn accessors() {
        let set = Op::Set {
            target: t(1),
            prop: "p".into(),
            value: json!(1),
        };
        assert_eq!(set.target(), t(1));
        assert_eq!(set.prop(), Some("p"));
        assert_eq!(Op::Delete { target: t(2) }.prop(), None);
        assert_eq!(Op::Delete { target: t(2) }.target(), t(2));
    }

    #[test]
    fn protected_things() {
        assert!(Target::DOCUMENT.is_protected());
        assert!(Target::new(Kind::Layer, FIRST_LAYER.0).is_protected());
        assert!(Target::new(Kind::Class, NONE_CLASS.0).is_protected());
        assert!(!t(1).is_protected());
        assert!(!Target::new(Kind::Layer, Uid::from_num(9)).is_protected());
        assert!(!Target::new(Kind::Resource, Uid::from_num(9)).is_protected());
    }

    #[test]
    fn validation() {
        let del = Op::Delete { target: t(1) };
        assert_eq!(validate_ops(&[]), Err(InvalidTxn::Empty));
        assert_eq!(validate_ops(std::slice::from_ref(&del)), Ok(()));
        assert_eq!(
            validate_ops(&vec![del.clone(); MAX_OPS + 1]),
            Err(InvalidTxn::TooMany)
        );
        let bad = Op::Set {
            target: t(1),
            prop: "id".into(),
            value: json!(1),
        };
        assert!(matches!(validate_ops(&[bad]), Err(InvalidTxn::BadProp(_))));
        let bad = Op::Unset {
            target: t(1),
            prop: "x".repeat(MAX_PROP_LEN + 1),
        };
        assert!(matches!(validate_ops(&[bad]), Err(InvalidTxn::BadProp(_))));
        let bad = Op::Create {
            target: t(1),
            props: [("".to_string(), json!(1))].into(),
            merge: false,
        };
        assert!(matches!(validate_ops(&[bad]), Err(InvalidTxn::BadProp(_))));
        let doc = Op::Create {
            target: Target::DOCUMENT,
            props: BTreeMap::new(),
            merge: false,
        };
        assert_eq!(validate_ops(&[doc]), Err(InvalidTxn::CreateDocument));
        // `merge` is for objects with a derived ID only.
        let derived = Uid::derive(Uid::from_num(1), 2, 0);
        let make = |target, merge| Op::Create {
            target,
            props: BTreeMap::new(),
            merge,
        };
        assert_eq!(
            validate_ops(&[make(Target::new(Kind::Object, derived), true)]),
            Ok(())
        );
        assert_eq!(
            validate_ops(&[make(t(1), true)]),
            Err(InvalidTxn::BadMerge(Uid::from_num(1)))
        );
        assert!(matches!(
            validate_ops(&[make(Target::new(Kind::Layer, derived), true)]),
            Err(InvalidTxn::BadMerge(_))
        ));
        assert!(!InvalidTxn::BadMerge(derived).to_string().is_empty());
        let ok = Op::Create {
            target: t(1),
            props: [("shape".to_string(), json!({}))].into(),
            merge: false,
        };
        assert_eq!(validate_ops(&[ok]), Ok(()));
        // Depth and size.
        let mut deep = json!(1);
        for _ in 0..(MAX_JSON_DEPTH + 2) {
            deep = json!([deep]);
        }
        let set_deep = Op::Set {
            target: t(1),
            prop: "a".into(),
            value: deep,
        };
        assert!(matches!(
            validate_ops(&[set_deep]),
            Err(InvalidTxn::TooDeep(_))
        ));
        let big = Op::Set {
            target: t(1),
            prop: "a".into(),
            value: json!("x".repeat(MAX_VALUE_BYTES + 1)),
        };
        assert!(matches!(validate_ops(&[big]), Err(InvalidTxn::TooLarge(_))));
        let many = Op::Set {
            target: t(1),
            prop: "a".into(),
            value: json!("x".repeat(MAX_VALUE_BYTES - 100)),
        };
        assert!(matches!(
            validate_ops(&vec![many; 3]),
            Err(InvalidTxn::TooLarge(_))
        ));
        let huge: Value = serde_json::from_str("[1e308]").unwrap();
        assert!(
            validate_ops(&[Op::Set {
                target: t(1),
                prop: "a".into(),
                value: huge
            }])
            .is_ok()
        );
        for e in [
            InvalidTxn::TooDeep("a".into()),
            InvalidTxn::TooLarge("a".into()),
            InvalidTxn::NotFinite("a".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
        assert!(!InvalidTxn::Empty.to_string().is_empty());
    }
}
