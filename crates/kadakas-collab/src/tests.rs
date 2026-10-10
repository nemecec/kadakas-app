//! Scenario tests for the client and the reference sequencer, and the convergence property test.

use std::collections::{BTreeMap, VecDeque};

use kadakas_core::model::Document;
use kadakas_core::uid::Uid;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::client::{Client, ClientConfig, ClientEvent, ConnState, EditError};
use crate::docsync::replica_of;
use crate::op::{Kind, Op, Target, Writer};
use crate::presence::{PresenceState, TTL_MS};
use crate::protocol::{ClientMsg, Entry, ErrorCode, PROTOCOL_VERSION, ServerMsg};
use crate::replica::Replica;
use crate::store::MemStore;
use crate::testkit::{ConnId, Sequencer};
use crate::undo::SkipReason;

fn obj(n: u64) -> Target {
    Target::new(Kind::Object, Uid::from_num(n))
}

fn create(n: u64, a: i64) -> Op {
    Op::Create {
        target: obj(n),
        props: [("a".to_string(), json!(a))].into(),
    }
}

fn set(n: u64, prop: &str, v: i64) -> Op {
    Op::Set {
        target: obj(n),
        prop: prop.into(),
        value: json!(v),
    }
}

fn del(n: u64) -> Op {
    Op::Delete { target: obj(n) }
}

fn cfg(user: u64) -> ClientConfig {
    ClientConfig {
        user: Uid::from_num(user),
        name: format!("user {user}"),
        token: String::new(),
    }
}

/// Clients and a sequencer, with hand-made delivery: nothing moves until a test says so.
struct Net {
    seq: Sequencer,
    clients: Vec<Client>,
    conns: Vec<Option<ConnId>>,
    inbox: Vec<VecDeque<ServerMsg>>,
    now: u64,
}

impl Net {
    fn new(n: usize) -> Net {
        let initial = replica_of(&Document::default());
        let mut net = Net {
            seq: Sequencer::new(initial),
            clients: vec![],
            conns: vec![],
            inbox: vec![],
            now: 1_000,
        };
        for i in 0..n {
            net.clients.push(Client::new(cfg(i as u64 + 1)));
            net.conns.push(None);
            net.inbox.push(VecDeque::new());
        }
        net
    }

    fn route(&mut self, outs: Vec<crate::testkit::Out>) {
        for o in outs {
            if let Some(i) = self.conns.iter().position(|c| *c == Some(o.conn)) {
                self.inbox[i].push_back(o.msg);
            }
        }
    }

    /// Sends what client `i` queued.
    fn flush(&mut self, i: usize) {
        for msg in self.clients[i].take_outbox() {
            if let Some(conn) = self.conns[i] {
                let outs = self.seq.receive(conn, msg, self.now);
                self.route(outs);
            }
        }
    }

    /// Lets client `i` handle everything that waits for it.
    fn deliver(&mut self, i: usize) {
        while let Some(m) = self.inbox[i].pop_front() {
            self.clients[i].handle(m, self.now);
        }
        self.clients[i].tick(self.now);
        self.flush(i);
    }

    fn connect(&mut self, i: usize) {
        if self.conns[i].is_some() {
            return;
        }
        self.conns[i] = Some(self.seq.open());
        self.clients[i].connect();
        self.flush(i);
    }

    fn disconnect(&mut self, i: usize) {
        if let Some(conn) = self.conns[i].take() {
            self.clients[i].connection_lost();
            let outs = self.seq.close(conn);
            self.route(outs);
        }
        self.inbox[i].clear();
    }

    fn settle(&mut self) {
        for _ in 0..50 {
            let before = (self.seq.seq(), self.total_waiting());
            for i in 0..self.clients.len() {
                self.flush(i);
                self.deliver(i);
            }
            self.now += 100;
            if self.total_waiting() == 0 && before.0 == self.seq.seq() {
                return;
            }
        }
        panic!(
            "the network did not settle: inbox {:?} pending {:?} seq {}",
            self.inbox,
            self.clients
                .iter()
                .map(|c| c.pending().len())
                .collect::<Vec<_>>(),
            self.seq.seq()
        );
    }

    fn total_waiting(&self) -> usize {
        self.inbox.iter().map(VecDeque::len).sum::<usize>()
            + self
                .clients
                .iter()
                .zip(&self.conns)
                .filter(|(_, conn)| conn.is_some())
                .map(|(c, _)| c.pending().len())
                .sum::<usize>()
    }

    fn all_equal(&self) {
        let want = self.seq.replica().canonical_text();
        for (i, c) in self.clients.iter().enumerate() {
            if self.conns[i].is_none() {
                continue;
            }
            assert_eq!(c.view().canonical_text(), want, "client {i} view");
            assert_eq!(c.confirmed().canonical_text(), want, "client {i} confirmed");
            assert!(c.pending().is_empty(), "client {i} still has pending work");
        }
    }

    fn value(&self, i: usize, n: u64, prop: &str) -> Option<Value> {
        self.clients[i]
            .view()
            .alive(obj(n))
            .and_then(|e| e.get(prop).cloned())
    }
}

#[test]
fn two_clients_sync_and_converge() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    assert_eq!(net.clients[0].state(), ConnState::Online);
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.clients[1].edit(vec![create(2, 2)]).unwrap();
    net.settle();
    net.all_equal();
    assert!(net.clients[0].view().alive(obj(2)).is_some());
    assert!(net.clients[1].view().alive(obj(1)).is_some());
    assert_eq!(net.seq.seq(), 2);
    assert_eq!(net.clients[0].head_seq(), 2);
    assert!(net.clients[0].take_changed());
    assert!(!net.clients[0].take_changed());
    assert!(
        net.clients[0]
            .take_events()
            .contains(&ClientEvent::Connected)
    );
}

#[test]
fn concurrent_edits_follow_the_server_order() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 0)]).unwrap();
    net.settle();
    // Both edit before they hear of each other: the server order decides property "a".
    net.clients[0]
        .edit(vec![set(1, "a", 10), set(1, "b", 1)])
        .unwrap();
    net.clients[1]
        .edit(vec![set(1, "a", 20), set(1, "c", 2)])
        .unwrap();
    // Each sees its own value now (optimistic apply).
    assert_eq!(net.value(0, 1, "a"), Some(json!(10)));
    assert_eq!(net.value(1, 1, "a"), Some(json!(20)));
    net.flush(1);
    net.flush(0);
    net.settle();
    net.all_equal();
    // Client 1 reached the server first, so client 0 is later and wins "a".
    assert_eq!(net.value(1, 1, "a"), Some(json!(10)));
    assert_eq!(net.value(0, 1, "b"), Some(json!(1)));
    assert_eq!(net.value(0, 1, "c"), Some(json!(2)));
}

#[test]
fn rebase_keeps_pending_edits_on_top_of_remote_ones() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 0)]).unwrap();
    net.settle();
    // Client 0 has a pending edit while client 1's edit comes first.
    net.clients[0].edit(vec![set(1, "a", 5)]).unwrap();
    net.clients[1]
        .edit(vec![set(1, "a", 7), set(1, "z", 1)])
        .unwrap();
    net.flush(1);
    net.deliver(0); // client 0 hears of client 1's edit before its own ack
    assert_eq!(
        net.value(0, 1, "a"),
        Some(json!(5)),
        "the pending edit stays on top"
    );
    assert_eq!(net.value(0, 1, "z"), Some(json!(1)));
    assert_eq!(
        net.clients[0].confirmed().alive(obj(1)).unwrap().get("a"),
        Some(&json!(7))
    );
    net.settle();
    net.all_equal();
    assert_eq!(net.value(1, 1, "a"), Some(json!(5)));
}

#[test]
fn delete_wins_and_later_edits_are_dropped() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 0)]).unwrap();
    net.settle();
    net.clients[0].edit(vec![del(1)]).unwrap();
    net.clients[1].edit(vec![set(1, "a", 9)]).unwrap();
    net.flush(0);
    net.flush(1);
    net.settle();
    net.all_equal();
    assert!(net.clients[1].view().alive(obj(1)).is_none());
    // The edit is not even sent while the client already sees the deletion.
    assert_eq!(
        net.clients[1].edit(vec![set(1, "a", 1)]),
        Err(EditError::NoEffect)
    );
}

#[test]
fn duplicate_create_is_rejected_by_the_sequencer() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    // Both make the same ID: impossible with UUIDv7, but the rule holds.
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.clients[1].edit(vec![create(1, 2)]).unwrap();
    net.flush(0);
    net.flush(1);
    net.settle();
    net.all_equal();
    assert_eq!(net.value(0, 1, "a"), Some(json!(1)));
    let events = net.clients[1].take_events();
    assert!(events.iter().any(|e| matches!(
        e,
        ClientEvent::Rejected {
            code: ErrorCode::DuplicateUid,
            ..
        }
    )));
    // A rejected edit leaves nothing in the undo stack.
    assert!(!net.clients[1].can_undo());
    assert!(net.clients[1].view().alive(obj(1)).is_some());
    // A create twice in one transaction is refused too.
    let conn = net.conns[0].unwrap();
    let outs = net.seq.receive(
        conn,
        ClientMsg::Submit {
            op_id: 50,
            ops: vec![create(5, 1), create(5, 2)],
        },
        0,
    );
    assert!(matches!(
        outs[0].msg,
        ServerMsg::Error {
            code: ErrorCode::DuplicateUid,
            op_id: Some(50),
            ..
        }
    ));
}

#[test]
fn the_replica_also_ignores_a_duplicate_create_from_the_log() {
    // A server that did not check: the clients drop the create the same way.
    let mut a = Replica::new();
    let w1 = Writer {
        user: Uid::from_num(1),
        op: 1,
    };
    let w2 = Writer {
        user: Uid::from_num(2),
        op: 1,
    };
    a.apply_all(w1, &[create(1, 1)]);
    a.apply_all(w2, &[create(1, 2)]);
    assert_eq!(a.alive(obj(1)).unwrap().get("a"), Some(&json!(1)));
}

#[test]
fn invalid_edits_are_refused_locally() {
    let mut c = Client::new(cfg(1));
    assert!(matches!(c.edit(vec![]), Err(EditError::Invalid(_))));
    assert!(matches!(
        c.edit(vec![set(1, "id", 1)]),
        Err(EditError::Invalid(_))
    ));
    assert_eq!(c.edit(vec![set(9, "a", 1)]), Err(EditError::NoEffect));
    assert!(!EditError::NoEffect.to_string().is_empty());
    assert!(c.pending().is_empty());
}

#[test]
fn offline_edits_replay_on_reconnect_and_survive_a_restart() {
    let mut net = Net::new(2);
    let store = MemStore::new();
    net.clients[0].set_store(Box::new(store.clone()));
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.settle();
    // Go offline and work.
    net.disconnect(0);
    assert_eq!(net.clients[0].state(), ConnState::Offline);
    net.clients[0]
        .edit(vec![set(1, "a", 2), create(2, 0)])
        .unwrap();
    net.clients[0].edit(vec![set(2, "b", 3)]).unwrap();
    assert_eq!(net.clients[0].pending().len(), 2);
    // Meanwhile client 1 deletes object 1 and edits something else.
    net.clients[1].edit(vec![del(1)]).unwrap();
    net.settle();
    // The program ends here. A new client starts from the store.
    let restored = Client::restore(cfg(1), Box::new(store.clone()));
    assert_eq!(restored.pending().len(), 2);
    assert!(restored.can_undo());
    assert_eq!(
        restored.view().alive(obj(2)).unwrap().get("b"),
        Some(&json!(3))
    );
    assert_eq!(restored.confirmed_seq(), 1);
    net.clients[0] = restored;
    net.connect(0);
    net.settle();
    net.all_equal();
    // Object 1 stays deleted (delete wins); the offline work on object 2 arrived.
    assert!(net.clients[0].view().alive(obj(1)).is_none());
    assert_eq!(net.value(1, 2, "b"), Some(json!(3)));
    // The set on object 1 was dropped from the queue by the rebase, not sent.
    let sent_sets = net
        .seq
        .log()
        .iter()
        .flat_map(|e| &e.ops)
        .filter(|o| matches!(o, Op::Set { target, prop, .. } if *target == obj(1) && prop == "a"))
        .count();
    assert_eq!(sent_sets, 0);
    // The counter continues: no transaction number is used twice.
    let id = net.clients[0].edit(vec![set(2, "b", 4)]).unwrap();
    assert_eq!(id, 4);
    net.settle();
    net.all_equal();
}

#[test]
fn a_journal_of_another_user_is_ignored() {
    let store = MemStore::new();
    let mut c = Client::new(cfg(1));
    c.set_store(Box::new(store.clone()));
    c.edit(vec![create(1, 1)]).unwrap();
    let other = Client::restore(cfg(2), Box::new(store));
    assert!(other.pending().is_empty());
    assert!(!other.can_undo());
}

#[test]
fn store_failures_are_reported_not_fatal() {
    let store = MemStore::new();
    let mut c = Client::new(cfg(1));
    c.set_store(Box::new(store.clone()));
    store.set_failing(true);
    c.edit(vec![create(1, 1)]).unwrap();
    assert!(
        c.take_events()
            .iter()
            .any(|e| matches!(e, ClientEvent::StoreFailed(_)))
    );
    assert_eq!(c.pending().len(), 1);
}

#[test]
fn resubmit_after_a_lost_ack_gets_the_old_sequence_number() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    // The submit reaches the server, but the connection dies before the ack arrives.
    net.flush(0);
    assert_eq!(net.seq.seq(), 1);
    net.disconnect(0);
    net.connect(0);
    net.settle();
    net.all_equal();
    assert_eq!(net.seq.seq(), 1, "the transaction was not sequenced twice");
    // The server also answers a repeated submit with the old number.
    let conn = net.seq.open();
    net.seq.receive(
        conn,
        ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            user: Uid::from_num(1),
            name: "x".into(),
            token: String::new(),
            last_seq: Some(1),
        },
        0,
    );
    let outs = net.seq.receive(
        conn,
        ClientMsg::Submit {
            op_id: 1,
            ops: vec![create(1, 1)],
        },
        0,
    );
    assert_eq!(outs[0].msg, ServerMsg::Ack { op_id: 1, seq: 1 });
}

#[test]
fn a_lost_ack_with_a_snapshot_drops_the_known_transaction() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.flush(0);
    net.disconnect(0);
    // Enough traffic that the log forgets entry 1: client 0 must take a snapshot.
    net.seq.set_log_limit(1);
    net.deliver(1);
    net.clients[1].edit(vec![set(1, "a", 2)]).unwrap();
    net.clients[1].edit(vec![set(1, "a", 3)]).unwrap();
    net.settle();
    net.connect(0);
    net.settle();
    net.all_equal();
    assert_eq!(net.value(0, 1, "a"), Some(json!(3)));
    assert_eq!(net.seq.seq(), 3);
}

#[test]
fn undo_takes_back_only_your_own_work() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 0)]).unwrap();
    net.settle();
    // A sets a=1 and b=1. B then sets a=2.
    net.clients[0]
        .edit(vec![set(1, "a", 1), set(1, "b", 1)])
        .unwrap();
    net.settle();
    net.clients[1].edit(vec![set(1, "a", 2)]).unwrap();
    net.settle();
    let report = net.clients[0].undo();
    assert!(report.done);
    assert_eq!(report.applied, 1, "only b goes back");
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].prop.as_deref(), Some("a"));
    assert_eq!(report.skipped[0].reason, SkipReason::ChangedByOther);
    net.settle();
    net.all_equal();
    assert_eq!(net.value(1, 1, "a"), Some(json!(2)), "B's value stays");
    assert_eq!(net.value(1, 1, "b"), None, "b was new, so it is removed");
    // Redo brings b back.
    let report = net.clients[0].redo();
    assert_eq!(report.applied, 1);
    net.settle();
    assert_eq!(net.value(1, 1, "b"), Some(json!(1)));
    // The undo of the create is skipped: B's edit lives in the object now.
    assert!(net.clients[0].undo().done); // b again
    let report = net.clients[0].undo(); // the create
    assert_eq!(report.applied, 0);
    assert_eq!(report.skipped[0].reason, SkipReason::ChangedByOther);
    net.settle();
    assert!(net.clients[1].view().alive(obj(1)).is_some());
    // Nothing is left to undo.
    assert!(!net.clients[0].can_undo());
    assert_eq!(net.clients[0].undo(), crate::undo::UndoReport::default());
    net.all_equal();
}

#[test]
fn undo_of_a_create_and_a_delete() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 4)]).unwrap();
    net.settle();
    // Undo the create: the object is deleted.
    assert_eq!(net.clients[0].undo().applied, 1);
    net.settle();
    assert!(net.clients[1].view().alive(obj(1)).is_none());
    // Redo: the object is restored (the same ID, with its old properties).
    assert_eq!(net.clients[0].redo().applied, 1);
    net.settle();
    assert_eq!(net.value(1, 1, "a"), Some(json!(4)));
    // Delete, then undo the delete.
    net.clients[0].edit(vec![del(1)]).unwrap();
    net.settle();
    assert!(net.clients[1].view().alive(obj(1)).is_none());
    assert_eq!(net.clients[0].undo().applied, 1);
    net.settle();
    assert_eq!(net.value(1, 1, "a"), Some(json!(4)));
    net.all_equal();
}

#[test]
fn undo_of_a_delete_is_skipped_when_somebody_else_deleted_too() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.settle();
    // Both delete. B's delete is dropped by the replicas (A's came first), and A undoes.
    net.clients[0].edit(vec![del(1)]).unwrap();
    net.flush(0);
    net.settle();
    // Now B deletes something else; A's restore still works on its own deletion only.
    assert_eq!(net.clients[0].undo().applied, 1);
    net.settle();
    assert!(net.clients[1].view().alive(obj(1)).is_some());
    // B deletes it. A's old redo entry is now about a different deletion state.
    net.clients[1].edit(vec![del(1)]).unwrap();
    net.settle();
    let report = net.clients[0].redo();
    assert_eq!(report.applied, 0);
    assert_eq!(report.skipped[0].reason, SkipReason::Gone);
    // And the undo that A has for its restore: nothing to restore.
    net.all_equal();
}

#[test]
fn a_new_edit_clears_redo() {
    let mut c = Client::new(cfg(1));
    c.edit(vec![create(1, 1)]).unwrap();
    c.edit(vec![set(1, "a", 2)]).unwrap();
    c.undo();
    assert!(c.can_redo());
    c.edit(vec![set(1, "a", 3)]).unwrap();
    assert!(!c.can_redo());
    // Offline undo works and the undo transaction waits in the queue.
    let before = c.pending().len();
    assert!(c.undo().op_id.is_some());
    assert_eq!(c.pending().len(), before + 1);
}

#[test]
fn undo_stack_is_bounded() {
    let mut c = Client::new(cfg(1));
    c.edit(vec![create(1, 0)]).unwrap();
    for i in 0..(crate::undo::MAX_STEPS as i64 + 10) {
        c.edit(vec![set(1, "a", i + 1)]).unwrap();
    }
    assert_eq!(c.journal().undo.len(), crate::undo::MAX_STEPS);
    for _ in 0..(crate::undo::MAX_STEPS + 5) {
        c.undo();
    }
    assert_eq!(c.journal().redo.len(), crate::undo::MAX_STEPS);
}

#[test]
fn a_gap_in_the_sequence_sends_the_client_offline() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    let e = |seq| Entry {
        seq,
        author: Uid::from_num(9),
        op_id: seq,
        ops: vec![create(seq + 100, 0)],
    };
    net.clients[0].handle(ServerMsg::Broadcast { entry: e(1) }, 0);
    assert_eq!(net.clients[0].confirmed_seq(), 1);
    // Old entries are ignored.
    net.clients[0].handle(ServerMsg::Broadcast { entry: e(1) }, 0);
    net.clients[0].handle(ServerMsg::Broadcast { entry: e(3) }, 0);
    assert_eq!(net.clients[0].state(), ConnState::Offline);
    assert!(
        net.clients[0]
            .take_events()
            .contains(&ClientEvent::GapDetected {
                expected: 2,
                got: 3
            })
    );
    // A gap in an ack does the same.
    net.clients[0].connect();
    net.clients[0].handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            head_seq: 1,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    net.clients[0].handle(ServerMsg::Entries { entries: vec![] }, 0);
    assert_eq!(net.clients[0].state(), ConnState::Online);
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.clients[0].handle(ServerMsg::Ack { op_id: 1, seq: 7 }, 0);
    assert_eq!(net.clients[0].state(), ConnState::Offline);
    // Entries with a gap during sync.
    net.clients[0].connect();
    net.clients[0].handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            head_seq: 9,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    net.clients[0].handle(
        ServerMsg::Entries {
            entries: vec![e(5)],
        },
        0,
    );
    assert_eq!(net.clients[0].state(), ConnState::Offline);
}

#[test]
fn acks_out_of_order_and_unknown_acks() {
    let mut c = Client::with_state(cfg(1), Replica::new(), 0);
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    c.handle(ServerMsg::Entries { entries: vec![] }, 0);
    c.edit(vec![create(1, 1)]).unwrap();
    c.edit(vec![set(1, "a", 2)]).unwrap();
    // The server orders them the other way round: the second first. The set is dropped there
    // (the object does not exist yet), and then the create comes.
    c.handle(ServerMsg::Ack { op_id: 2, seq: 1 }, 0);
    c.handle(ServerMsg::Ack { op_id: 1, seq: 2 }, 0);
    assert!(c.pending().is_empty());
    assert_eq!(
        c.confirmed().alive(obj(1)).unwrap().get("a"),
        Some(&json!(1))
    );
    assert_eq!(c.view().canonical_text(), c.confirmed().canonical_text());
    // An ack for something unknown does nothing.
    c.handle(ServerMsg::Ack { op_id: 99, seq: 3 }, 0);
    assert_eq!(c.confirmed_seq(), 2);
    // An ack whose number is old only clears the queue.
    c.edit(vec![set(1, "a", 3)]).unwrap();
    c.handle(ServerMsg::Ack { op_id: 3, seq: 1 }, 0);
    assert!(c.pending().is_empty());
}

#[test]
fn messages_in_the_wrong_state_are_ignored() {
    let mut c = Client::new(cfg(1));
    c.handle(ServerMsg::Ack { op_id: 1, seq: 1 }, 0);
    c.handle(
        ServerMsg::Broadcast {
            entry: Entry {
                seq: 1,
                author: Uid::from_num(2),
                op_id: 1,
                ops: vec![create(1, 1)],
            },
        },
        0,
    );
    c.handle(ServerMsg::Entries { entries: vec![] }, 0);
    c.handle(
        ServerMsg::Snapshot {
            seq: 3,
            state: Replica::new(),
        },
        0,
    );
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    assert_eq!(c.confirmed_seq(), 0);
    assert_eq!(c.state(), ConnState::Offline);
    // connect twice sends one hello.
    c.connect();
    c.connect();
    assert_eq!(c.take_outbox().len(), 1);
    assert_eq!(c.user(), Uid::from_num(1));
    assert_eq!(c.name(), "user 1");
}

#[test]
fn auth_and_protocol_failures_are_final() {
    let mut net = Net::new(1);
    net.seq.require_token("secret");
    net.connect(0);
    net.settle();
    assert_eq!(net.clients[0].state(), ConnState::Failed);
    assert!(net.clients[0].take_events().iter().any(|e| matches!(
        e,
        ClientEvent::Failed {
            code: ErrorCode::AuthFailed,
            ..
        }
    )));
    // A good token works from the failed state.
    net.disconnect(0);
    net.clients[0] = Client::new(ClientConfig {
        token: "secret".into(),
        ..cfg(1)
    });
    net.connect(0);
    net.settle();
    assert_eq!(net.clients[0].state(), ConnState::Online);

    // A server of another version.
    let mut c = Client::new(cfg(1));
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: 99,
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    assert_eq!(c.state(), ConnState::Failed);
    // connect works again from Failed. A non-op error that is not fatal is reported.
    c.connect();
    assert_eq!(c.state(), ConnState::Connecting);
    c.take_events();
    c.handle(
        ServerMsg::Error {
            code: ErrorCode::RateLimited,
            message: "slow down".into(),
            op_id: None,
        },
        0,
    );
    assert!(matches!(
        c.take_events()[0],
        ClientEvent::ServerError { .. }
    ));
    // A fatal one without an op ends the client.
    c.handle(
        ServerMsg::Error {
            code: ErrorCode::UnsupportedProtocol,
            message: "old".into(),
            op_id: None,
        },
        0,
    );
    assert_eq!(c.state(), ConnState::Failed);
    // connection_lost keeps Failed.
    c.connection_lost();
    assert_eq!(c.state(), ConnState::Failed);
}

#[test]
fn the_sequencer_checks_messages() {
    let mut s = Sequencer::new(Replica::new());
    let conn = s.open();
    assert_eq!(s.connections(), 1);
    // Anything before hello is refused.
    let outs = s.receive(conn, ClientMsg::Bye, 0);
    assert!(matches!(
        outs[0].msg,
        ServerMsg::Error {
            code: ErrorCode::BadMessage,
            ..
        }
    ));
    let hello = |protocol| ClientMsg::Hello {
        protocol,
        user: Uid::from_num(1),
        name: "n".repeat(200),
        token: String::new(),
        last_seq: Some(50),
    };
    let outs = s.receive(conn, hello(7), 0);
    assert!(matches!(
        outs[0].msg,
        ServerMsg::Error {
            code: ErrorCode::UnsupportedProtocol,
            ..
        }
    ));
    // A client that is ahead of the server gets a snapshot.
    let outs = s.receive(conn, hello(PROTOCOL_VERSION), 0);
    assert!(matches!(outs[1].msg, ServerMsg::Snapshot { .. }));
    let outs = s.receive(conn, hello(PROTOCOL_VERSION), 0);
    assert!(matches!(
        outs[0].msg,
        ServerMsg::Error {
            code: ErrorCode::BadMessage,
            ..
        }
    ));
    // Bad transactions.
    let outs = s.receive(
        conn,
        ClientMsg::Submit {
            op_id: 1,
            ops: vec![],
        },
        0,
    );
    assert!(matches!(
        outs[0].msg,
        ServerMsg::Error {
            code: ErrorCode::InvalidTxn,
            op_id: Some(1),
            ..
        }
    ));
    // Bad presence.
    let bad = PresenceState {
        cursor: Some([f64::NAN, 0.0]),
        ..Default::default()
    };
    let outs = s.receive(conn, ClientMsg::Presence { state: bad }, 0);
    assert!(matches!(outs[0].msg, ServerMsg::Error { .. }));
    // Unknown connections are ignored; Bye closes.
    assert!(s.receive(99, ClientMsg::Bye, 0).is_empty());
    assert!(s.close(99).is_empty());
    s.receive(conn, ClientMsg::Bye, 0);
    assert_eq!(s.connections(), 0);
    assert!(s.log().is_empty());
}

#[test]
fn presence_flows_between_clients_and_is_never_saved() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    let state = PresenceState {
        cursor: Some([10.0, 20.0]),
        selection: vec![Uid::from_num(5)],
        viewport: None,
    };
    net.clients[0].set_presence(state.clone()).unwrap();
    assert_eq!(net.clients[0].presence(), &state);
    assert!(
        net.clients[0]
            .set_presence(PresenceState {
                cursor: Some([f64::NAN, 0.0]),
                ..Default::default()
            })
            .is_err()
    );
    net.settle();
    let peer = net.clients[1].peers().get(Uid::from_num(1)).unwrap();
    assert_eq!(peer.state, state);
    assert_eq!(peer.name, "user 1");
    assert!(net.clients[0].peers().get(Uid::from_num(1)).is_none());
    // It never touches the document, the log or the checkpoint.
    assert_eq!(net.seq.seq(), 0);
    assert_eq!(
        net.seq.replica().canonical_text(),
        replica_of(&Document::default()).canonical_text()
    );
    // A late joiner hears the presence in `welcome`.
    net.clients.push(Client::new(cfg(3)));
    net.conns.push(None);
    net.inbox.push(Default::default());
    net.connect(2);
    net.settle();
    assert_eq!(net.clients[2].peers().len(), 2);
    assert_eq!(
        net.clients[2].peers().get(Uid::from_num(1)).unwrap().state,
        state
    );
    // The sender is rate-limited: a quick second update waits for the interval.
    net.now += 1_000;
    net.clients[0]
        .set_presence(PresenceState {
            cursor: Some([1.0, 1.0]),
            ..Default::default()
        })
        .unwrap();
    net.deliver(0);
    net.clients[0]
        .set_presence(PresenceState {
            cursor: Some([2.0, 2.0]),
            ..Default::default()
        })
        .unwrap();
    net.deliver(0);
    net.deliver(1);
    assert_eq!(
        net.clients[1]
            .peers()
            .get(Uid::from_num(1))
            .unwrap()
            .state
            .cursor,
        Some([1.0, 1.0])
    );
    net.now += 100;
    net.deliver(0);
    net.deliver(1);
    assert_eq!(
        net.clients[1]
            .peers()
            .get(Uid::from_num(1))
            .unwrap()
            .state
            .cursor,
        Some([2.0, 2.0])
    );
    // Leaving removes the user from the others.
    net.disconnect(0);
    net.deliver(1);
    assert!(net.clients[1].peers().get(Uid::from_num(1)).is_none());
    // A quiet peer expires.
    net.now += TTL_MS;
    net.deliver(1);
    assert!(net.clients[1].peers().get(Uid::from_num(3)).is_none());
}

#[test]
fn the_server_drops_presence_that_comes_too_fast() {
    let mut s = Sequencer::new(Replica::new());
    let a = s.open();
    let b = s.open();
    for (c, u) in [(a, 1), (b, 2)] {
        s.receive(
            c,
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                user: Uid::from_num(u),
                name: "x".into(),
                token: String::new(),
                last_seq: None,
            },
            0,
        );
    }
    let p = |x| ClientMsg::Presence {
        state: PresenceState {
            cursor: Some([x, 0.0]),
            ..Default::default()
        },
    };
    assert_eq!(s.receive(a, p(1.0), 100).len(), 1);
    assert!(s.receive(a, p(2.0), 120).is_empty());
    assert_eq!(s.receive(a, p(3.0), 150).len(), 1);
}

#[test]
fn bad_presence_from_the_server_is_ignored() {
    let mut c = Client::new(cfg(1));
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            head_seq: 0,
            your_last_op: 0,
            peers: vec![
                crate::presence::Peer {
                    user: Uid::from_num(2),
                    name: "ok".into(),
                    state: PresenceState::default(),
                },
                crate::presence::Peer {
                    user: Uid::from_num(3),
                    name: "bad".into(),
                    state: PresenceState {
                        cursor: Some([f64::INFINITY, 0.0]),
                        ..Default::default()
                    },
                },
                crate::presence::Peer {
                    user: Uid::from_num(1),
                    name: "me".into(),
                    state: PresenceState::default(),
                },
            ],
        },
        0,
    );
    assert_eq!(c.peers().len(), 1);
    let bad = PresenceState {
        cursor: Some([f64::NAN, 0.0]),
        ..Default::default()
    };
    c.handle(
        ServerMsg::Presence {
            user: Uid::from_num(4),
            name: "x".into(),
            state: bad,
        },
        0,
    );
    c.handle(
        ServerMsg::Presence {
            user: Uid::from_num(1),
            name: "me".into(),
            state: PresenceState::default(),
        },
        0,
    );
    assert_eq!(c.peers().len(), 1);
}

#[test]
fn document_changes_travel_as_operations() {
    use kadakas_core::command::{Command, Editor};
    let mut ed_a = Editor::with_sequential_ids();
    let mut ed_b = Editor::with_sequential_ids();
    let base = replica_of(ed_a.doc());
    let mut a = Client::with_state(cfg(1), base.clone(), 0);
    let mut b = Client::with_state(cfg(2), base.clone(), 0);
    let rev = ed_a.revision();
    let cmd: Command = serde_json::from_value(json!({
        "op": "add_object",
        "shape": {"kind": "rect", "min": [0.0, 0.0], "max": [10.0, 5.0]}
    }))
    .unwrap();
    ed_a.execute(&cmd).unwrap();
    let cs = ed_a
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .cloned()
        .unwrap();
    assert!(a.edit_from_change(ed_a.doc(), &cs).unwrap().is_some());
    // No change, no transaction.
    assert_eq!(a.edit_from_diff(ed_a.doc()).unwrap(), None);
    // Deliver to B by hand as a broadcast.
    let txn = a.pending().front().unwrap().clone();
    b.connect();
    b.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            head_seq: 1,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    b.handle(
        ServerMsg::Entries {
            entries: vec![Entry {
                seq: 1,
                author: a.user(),
                op_id: txn.op_id,
                ops: txn.ops,
            }],
        },
        0,
    );
    assert!(b.take_changed());
    let doc = crate::docsync::materialize(b.view(), ed_b.doc()).unwrap();
    assert_eq!(doc.objects.len(), 1);
    ed_b = Editor::new(doc);
    assert_eq!(ed_b.doc().objects.len(), 1);
    // B's full diff against its own new document finds nothing to send.
    assert_eq!(b.edit_from_diff(ed_b.doc()).unwrap(), None);
}

// ----- the convergence property test -----

#[derive(Clone, Debug)]
enum Act {
    Edit {
        c: usize,
        kind: u8,
        t: u8,
        p: u8,
        v: u8,
    },
    Undo {
        c: usize,
    },
    Redo {
        c: usize,
    },
    Flush {
        c: usize,
    },
    Deliver {
        c: usize,
    },
    Disconnect {
        c: usize,
    },
    Connect {
        c: usize,
    },
    Present {
        c: usize,
        x: u8,
    },
}

fn act(n: usize) -> impl Strategy<Value = Act> {
    prop_oneof![
        6 => (0..n, 0u8..6, 0u8..8, 0u8..3, 0u8..4)
            .prop_map(|(c, kind, t, p, v)| Act::Edit { c, kind, t, p, v }),
        2 => (0..n).prop_map(|c| Act::Undo { c }),
        1 => (0..n).prop_map(|c| Act::Redo { c }),
        4 => (0..n).prop_map(|c| Act::Flush { c }),
        4 => (0..n).prop_map(|c| Act::Deliver { c }),
        1 => (0..n).prop_map(|c| Act::Disconnect { c }),
        2 => (0..n).prop_map(|c| Act::Connect { c }),
        1 => (0..n, 0u8..255).prop_map(|(c, x)| Act::Present { c, x }),
    ]
}

/// The pool of things that all clients fight over: objects, a layer and a resource.
fn pool(t: u8) -> Target {
    match t % 8 {
        0..=4 => Target::new(Kind::Object, Uid::from_num(100 + u64::from(t))),
        5 => Target::new(Kind::Layer, Uid::from_num(200)),
        6 => Target::new(Kind::Resource, Uid::from_num(300)),
        _ => Target::new(Kind::Layer, kadakas_core::model::FIRST_LAYER.0),
    }
}

fn make_op(kind: u8, t: u8, p: u8, v: u8) -> Op {
    let target = pool(t);
    let prop = ["a", "b", "c"][usize::from(p % 3)].to_string();
    match kind {
        0 => Op::Create {
            target,
            props: [("a".to_string(), json!(v)), ("b".to_string(), json!(0))].into(),
        },
        1 => Op::Delete { target },
        2 => Op::Unset { target, prop },
        _ => Op::Set {
            target,
            prop,
            value: json!(v),
        },
    }
}

/// Every slot that another user wrote, and every deletion by another user, must be the same.
fn others_unchanged(before: &Replica, after: &Replica, me: Uid) {
    for (target, e) in before.alive_things() {
        let Some(now) = after.get(*target) else {
            panic!("a thing vanished");
        };
        for (prop, slot) in &e.props {
            if slot.writer.user != me {
                assert_eq!(now.props.get(prop), Some(slot), "{target:?}.{prop}");
            }
        }
    }
    for (target, _) in before.alive_things() {
        // Things alive before stay alive unless I made them (undo of my create).
        let was_mine = before.get(*target).is_some_and(|e| {
            e.created_by.user == me && e.props.values().all(|s| s.writer.user == me)
        });
        if !was_mine {
            assert!(
                after.alive(*target).is_some(),
                "{target:?} was deleted by an undo"
            );
        }
    }
    // A deletion by another user stays.
    for (target, e) in before.things() {
        if e.deleted_by.is_some_and(|w| w.user != me) {
            assert!(
                after.alive(*target).is_none(),
                "{target:?} was restored by an undo"
            );
        }
    }
}

fn run(n: usize, acts: Vec<Act>, order: Vec<usize>) {
    let mut net = Net::new(n);
    // Half of the clients start from the known state, the rest from a snapshot.
    let initial = net.seq.replica().clone();
    for i in (0..n).step_by(2) {
        net.clients[i] = Client::with_state(cfg(i as u64 + 1), initial.clone(), 0);
    }
    let mut stores = BTreeMap::new();
    for i in 0..n {
        let s = MemStore::new();
        net.clients[i].set_store(Box::new(s.clone()));
        stores.insert(i, s);
    }
    for a in acts {
        match a {
            Act::Edit { c, kind, t, p, v } => {
                let _ = net.clients[c].edit(vec![make_op(kind, t, p, v), make_op(3, t, p + 1, v)]);
            }
            Act::Undo { c } | Act::Redo { c } => {
                let me = net.clients[c].user();
                let before = net.clients[c].view().clone();
                if matches!(a, Act::Undo { .. }) {
                    net.clients[c].undo();
                } else {
                    net.clients[c].redo();
                }
                others_unchanged(&before, net.clients[c].view(), me);
            }
            Act::Flush { c } => net.flush(c),
            Act::Deliver { c } => net.deliver(c),
            Act::Disconnect { c } => {
                net.disconnect(c);
                // Restart the program now and then: the client comes back from its store.
                if c % 2 == 1 {
                    let s = stores[&c].clone();
                    net.clients[c] = Client::restore(cfg(c as u64 + 1), Box::new(s));
                }
            }
            Act::Connect { c } => net.connect(c),
            Act::Present { c, x } => {
                let _ = net.clients[c].set_presence(PresenceState {
                    cursor: Some([f64::from(x), 0.0]),
                    ..Default::default()
                });
                net.now += 30;
            }
        }
    }
    for i in order {
        net.connect(i % n);
    }
    for i in 0..n {
        net.connect(i);
    }
    net.settle();
    net.all_equal();
    // Presence never reached the log.
    for e in net.seq.log() {
        assert!(!e.ops.is_empty());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn clients_converge(
        n in 2usize..5,
        acts in prop::collection::vec(act(4), 1..120),
        order in Just((0usize..4).collect::<Vec<_>>()).prop_shuffle(),
    ) {
        // Actions name clients 0..4; fold them onto the n that exist.
        let acts = acts.into_iter().map(|a| match a {
            Act::Edit { c, kind, t, p, v } => Act::Edit { c: c % n, kind, t, p, v },
            Act::Undo { c } => Act::Undo { c: c % n },
            Act::Redo { c } => Act::Redo { c: c % n },
            Act::Flush { c } => Act::Flush { c: c % n },
            Act::Deliver { c } => Act::Deliver { c: c % n },
            Act::Disconnect { c } => Act::Disconnect { c: c % n },
            Act::Connect { c } => Act::Connect { c: c % n },
            Act::Present { c, x } => Act::Present { c: c % n, x },
        }).collect();
        run(n, acts, order);
    }
}

#[test]
fn links_talk_through_the_hub() {
    use crate::testkit::Hub;
    use crate::transport::{Endpoint, Link};
    let hub = Hub::new(Sequencer::new(replica_of(&Document::default())));
    let mut ep = hub.endpoint();
    let mut a = Link::new(Client::new(cfg(1)));
    let mut b = Link::new(Client::new(cfg(2)));
    a.connect(&mut ep).unwrap();
    b.connect(&mut ep).unwrap();
    let mut now = 0;
    let pump = |a: &mut Link, b: &mut Link, now: &mut u64| {
        for _ in 0..4 {
            *now += 100;
            a.pump(*now);
            b.pump(*now);
        }
    };
    pump(&mut a, &mut b, &mut now);
    assert_eq!(a.client.state(), ConnState::Online);
    assert_eq!(hub.with(|s| s.connections()), 2);
    a.client.edit(vec![create(1, 1)]).unwrap();
    b.client
        .set_presence(PresenceState {
            cursor: Some([3.0, 4.0]),
            ..Default::default()
        })
        .unwrap();
    hub.set_time(1_000);
    pump(&mut a, &mut b, &mut now);
    assert!(b.client.view().alive(obj(1)).is_some());
    assert_eq!(a.client.peers().len(), 1);
    assert_eq!(hub.with(|s| s.seq()), 1);
    // The network breaks: both go offline, edits wait, and a new connection catches up.
    hub.drop_all();
    pump(&mut a, &mut b, &mut now);
    assert_eq!(a.client.state(), ConnState::Offline);
    assert_eq!(hub.with(|s| s.connections()), 0);
    a.client.edit(vec![set(1, "a", 9)]).unwrap();
    a.connect(&mut ep).unwrap();
    b.connect(&mut ep).unwrap();
    pump(&mut a, &mut b, &mut now);
    assert_eq!(
        b.client.view().alive(obj(1)).unwrap().get("a"),
        Some(&json!(9))
    );
    // A clean goodbye closes the connection in the hub.
    a.disconnect();
    b.pump(now + 100);
    assert_eq!(hub.with(|s| s.connections()), 1);
    assert_eq!(ep.describe(), "in-memory test hub");
    // A message at a wrong time is answered with an error, not a crash.
    let mut t = ep.connect().unwrap();
    t.send(&ClientMsg::Bye).unwrap();
    assert!(matches!(t.recv().unwrap(), Some(ServerMsg::Error { .. })));
    t.close();
    assert_eq!(hub.with(|s| s.connections()), 1);
}
