//! Per-user undo.
//!
//! In a shared document, "undo" must take back your own work and nothing else. The client
//! therefore keeps its own undo stack. For each transaction that you make, it records the
//! *inverse* operations: the operations that bring the properties back to the values they had
//! before. Undo sends these inverse operations as a new transaction, like any other edit. It
//! never rewinds the shared log, so other users see an ordinary change. Redo works the same way
//! with the inverse of the undo.
//!
//! # What happens when another user changed the same thing
//!
//! Before an inverse operation is sent, the client checks it against what it sees now:
//!
//! - **Set or Unset of a property:** the operation is *skipped* if another user wrote that
//!   property after you. Their value stays. If you wrote it last, the old value comes back.
//! - **Undo of a create (a Delete):** the operation is skipped if another user wrote any
//!   property of the object. It holds work of somebody else now, so it stays.
//! - **Undo of a delete (a Restore):** the object comes back with the properties it had when you
//!   deleted it. It is skipped if somebody else deleted the object in the meantime (a restore
//!   works only for the deletion that you made).
//! - **The object is deleted:** all operations on it are skipped. Delete wins.
//!
//! The rest of the inverse transaction still goes out. An undo step that is skipped in full
//! changes nothing, and it is still used up. The result lists what was skipped and why
//! ([`Skipped`]), so a user interface can say "2 properties changed by Ann were kept".
//!
//! The check uses what the client sees. A change by another user that the client has not
//! received yet is not seen. If it is ordered before the undo by the server, the undo overwrites
//! it, like any later edit would (last writer wins).
//!
//! The stacks hold at most [`MAX_STEPS`] steps. They are saved with the offline log, so undo
//! also works after a restart.

use kadakas_core::uid::Uid;
use serde::{Deserialize, Serialize};

use crate::op::{Op, Target};
use crate::replica::Replica;

/// The most steps an undo stack keeps.
pub const MAX_STEPS: usize = 1_000;

/// One undo (or redo) step: the inverse of one transaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UndoEntry {
    /// The transaction that this step takes back.
    pub of_op: u64,
    pub inverse: Vec<Op>,
}

/// Why an inverse operation was skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Another user changed it after you.
    ChangedByOther,
    /// The object is gone (deleted), or somebody else deleted it.
    Gone,
}

/// An inverse operation that was not sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skipped {
    pub target: Target,
    /// The property, if the operation was about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop: Option<String>,
    pub reason: SkipReason,
}

/// What an undo or redo did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UndoReport {
    /// False if there was nothing to undo or redo.
    pub done: bool,
    /// The transaction that was sent, if any operation was left.
    pub op_id: Option<u64>,
    /// How many operations were sent.
    pub applied: usize,
    pub skipped: Vec<Skipped>,
}

/// Keeps the inverse operations that are still safe to send. `me` is the user who undoes.
pub fn filter_inverse(view: &Replica, me: Uid, inverse: &[Op]) -> (Vec<Op>, Vec<Skipped>) {
    let mut keep = Vec::new();
    let mut skipped = Vec::new();
    for op in inverse {
        match check(view, me, op) {
            None => keep.push(op.clone()),
            Some(reason) => skipped.push(Skipped {
                target: op.target(),
                prop: op.prop().map(str::to_string),
                reason,
            }),
        }
    }
    (keep, skipped)
}

fn check(view: &Replica, me: Uid, op: &Op) -> Option<SkipReason> {
    match op {
        Op::Set { target, prop, .. } | Op::Unset { target, prop } => {
            let Some(e) = view.alive(*target) else {
                return Some(SkipReason::Gone);
            };
            match e.props.get(prop) {
                Some(slot) if slot.writer.user != me => Some(SkipReason::ChangedByOther),
                _ => None,
            }
        }
        Op::Delete { target } => {
            let Some(e) = view.alive(*target) else {
                return Some(SkipReason::Gone);
            };
            e.props
                .values()
                .any(|s| s.writer.user != me)
                .then_some(SkipReason::ChangedByOther)
        }
        Op::Restore { target, deleted_by } => match view.get(*target) {
            Some(e) if e.deleted_by == Some(*deleted_by) => None,
            _ => Some(SkipReason::Gone),
        },
        // Nothing records a create as an inverse. If one is here, the data is damaged.
        Op::Create { .. } => Some(SkipReason::Gone),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{Kind, Writer};
    use serde_json::json;

    fn obj(n: u64) -> Target {
        Target::new(Kind::Object, Uid::from_num(n))
    }

    fn w(user: u64, op: u64) -> Writer {
        Writer {
            user: Uid::from_num(user),
            op,
        }
    }

    fn setup() -> Replica {
        let mut r = Replica::new();
        r.apply(
            w(1, 1),
            &Op::Create {
                target: obj(1),
                props: [("a".to_string(), json!(1))].into(),
            },
        );
        r
    }

    fn set(prop: &str, v: i64) -> Op {
        Op::Set {
            target: obj(1),
            prop: prop.into(),
            value: json!(v),
        }
    }

    #[test]
    fn own_property_is_undone_foreign_is_kept() {
        let mut r = setup();
        r.apply(w(1, 2), &set("a", 2));
        r.apply(w(2, 1), &set("b", 5));
        let me = Uid::from_num(1);
        // "a" was last written by me; "b" by user 2.
        let (keep, skipped) = filter_inverse(&r, me, &[set("a", 1), set("b", 0)]);
        assert_eq!(keep, vec![set("a", 1)]);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].prop.as_deref(), Some("b"));
        assert_eq!(skipped[0].reason, SkipReason::ChangedByOther);
        // A property that nobody wrote has no owner: the inverse goes.
        let unset = Op::Unset {
            target: obj(1),
            prop: "never".into(),
        };
        assert_eq!(filter_inverse(&r, me, &[unset]).0.len(), 1);
    }

    #[test]
    fn undo_of_create_waits_for_foreign_edits() {
        let mut r = setup();
        let me = Uid::from_num(1);
        let del = Op::Delete { target: obj(1) };
        assert_eq!(
            filter_inverse(&r, me, std::slice::from_ref(&del)).0.len(),
            1
        );
        r.apply(w(2, 1), &set("a", 9));
        let (keep, skipped) = filter_inverse(&r, me, std::slice::from_ref(&del));
        assert!(keep.is_empty());
        assert_eq!(skipped[0].reason, SkipReason::ChangedByOther);
        assert_eq!(skipped[0].prop, None);
    }

    #[test]
    fn deleted_objects_skip_everything() {
        let mut r = setup();
        r.apply(w(2, 1), &Op::Delete { target: obj(1) });
        let me = Uid::from_num(1);
        let del = Op::Delete { target: obj(1) };
        let unset = Op::Unset {
            target: obj(1),
            prop: "a".into(),
        };
        let (keep, skipped) = filter_inverse(&r, me, &[set("a", 1), del, unset]);
        assert!(keep.is_empty());
        assert!(skipped.iter().all(|s| s.reason == SkipReason::Gone));
        assert_eq!(skipped.len(), 3);
    }

    #[test]
    fn restore_only_for_my_deletion() {
        let mut r = setup();
        r.apply(w(1, 2), &Op::Delete { target: obj(1) });
        let me = Uid::from_num(1);
        let mine = Op::Restore {
            target: obj(1),
            deleted_by: w(1, 2),
        };
        assert_eq!(
            filter_inverse(&r, me, std::slice::from_ref(&mine)).0.len(),
            1
        );
        let other = Op::Restore {
            target: obj(1),
            deleted_by: w(2, 7),
        };
        assert_eq!(filter_inverse(&r, me, &[other]).1.len(), 1);
        let missing = Op::Restore {
            target: obj(9),
            deleted_by: w(1, 2),
        };
        assert_eq!(filter_inverse(&r, me, &[missing]).1.len(), 1);
        let create = Op::Create {
            target: obj(9),
            props: Default::default(),
        };
        assert_eq!(filter_inverse(&r, me, &[create]).1.len(), 1);
    }

    #[test]
    fn report_json() {
        let rep = UndoReport {
            done: true,
            op_id: Some(3),
            applied: 1,
            skipped: vec![Skipped {
                target: obj(1),
                prop: Some("a".into()),
                reason: SkipReason::ChangedByOther,
            }],
        };
        let back: UndoReport = serde_json::from_str(&serde_json::to_string(&rep).unwrap()).unwrap();
        assert_eq!(back, rep);
        let e = UndoEntry {
            of_op: 1,
            inverse: vec![],
        };
        let back: UndoEntry = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
        assert_eq!(UndoReport::default().applied, 0);
    }
}
