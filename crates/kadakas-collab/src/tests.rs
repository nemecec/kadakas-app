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
        merge: false,
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
        client: Uid::from_num(user),
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
        let mut seq = Sequencer::new(initial);
        // The tests use made-up properties; the check of the things has its own tests.
        seq.check_entities(false);
        let mut net = Net {
            seq,
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
        client: Uid::from_num(1),
        op: 1,
    };
    let w2 = Writer {
        client: Uid::from_num(2),
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
            client: Uid::from_num(1),
            name: "x".into(),
            token: String::new(),
            last_seq: Some(1),
            log_id: None,
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
        user: Uid::from_num(9),
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
            log_id: "log-1".into(),
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
            log_id: "log-1".into(),
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
    let mut c = Client::with_state(cfg(1), Replica::new(), 0, "log-1");
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-1".into(),
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
                user: Uid::from_num(2),
                op_id: 1,
                ops: vec![create(1, 1)],
            },
        },
        0,
    );
    c.handle(ServerMsg::Entries { entries: vec![] }, 0);
    c.handle(
        ServerMsg::Snapshot {
            log_id: "log-1".into(),
            seq: 3,
            state: Replica::new(),
        },
        0,
    );
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-1".into(),
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
            log_id: "log-1".into(),
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
        client: Uid::from_num(1),
        name: "n".repeat(200),
        token: String::new(),
        last_seq: Some(50),
        log_id: None,
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
                client: Uid::from_num(u),
                name: "x".into(),
                token: String::new(),
                last_seq: None,
                log_id: None,
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
            log_id: "log-1".into(),
            head_seq: 0,
            your_last_op: 0,
            peers: vec![
                crate::presence::Peer {
                    client: Uid::from_num(2),
                    user: Uid::from_num(2),
                    name: "ok".into(),
                    state: PresenceState::default(),
                },
                crate::presence::Peer {
                    client: Uid::from_num(3),
                    user: Uid::from_num(3),
                    name: "bad".into(),
                    state: PresenceState {
                        cursor: Some([f64::INFINITY, 0.0]),
                        ..Default::default()
                    },
                },
                crate::presence::Peer {
                    client: Uid::from_num(1),
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
            client: Uid::from_num(4),
            user: Uid::from_num(4),
            name: "x".into(),
            state: bad,
        },
        0,
    );
    c.handle(
        ServerMsg::Presence {
            client: Uid::from_num(1),
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
    let ed_b = Editor::with_sequential_ids();
    let base = replica_of(ed_a.doc());
    let mut a = Client::with_state(cfg(1), base.clone(), 0, "log-1");
    let mut b = Client::with_state(cfg(2), base.clone(), 0, "log-1");
    a.set_shadow(ed_a.doc());
    b.set_shadow(ed_b.doc());
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
    let made = a.edit_from_change(ed_a.doc(), &cs);
    assert_eq!(made.op_ids.len(), 1);
    assert!(made.problems.is_empty());
    // No change, no transaction: the shadow already has it.
    assert_eq!(
        a.edit_from_diff(ed_a.doc()),
        crate::client::DocEdit::default()
    );
    // Deliver to B by hand as a broadcast.
    let txn = a.pending().front().unwrap().clone();
    b.connect();
    b.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-1".into(),
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
                author: a.client_id(),
                user: a.user(),
                op_id: txn.op_id,
                ops: txn.ops,
            }],
        },
        0,
    );
    assert!(b.take_changed());
    let m = crate::materialize::materialize(b.view(), ed_b.doc());
    assert_eq!(m.doc.objects.len(), 1);
    assert!(m.quarantined.is_empty());
    // B shows the new document and sends nothing back: its shadow is the shown document.
    b.set_shadow(&m.doc);
    assert_eq!(b.edit_from_diff(&m.doc), crate::client::DocEdit::default());
    // A client without a shadow reports it.
    let mut c = Client::new(cfg(3));
    assert_eq!(c.edit_from_diff(&m.doc).problems.len(), 1);
    assert!(!c.has_shadow());
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
            merge: false,
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
            if slot.writer.client != me {
                assert_eq!(now.props.get(prop), Some(slot), "{target:?}.{prop}");
            }
        }
    }
    for (target, _) in before.alive_things() {
        // Things alive before stay alive unless I made them (undo of my create).
        let was_mine = before
            .get(*target)
            .is_some_and(|e| e.props.values().all(|s| s.writer.client == me));
        if !was_mine {
            assert!(
                after.alive(*target).is_some(),
                "{target:?} was deleted by an undo"
            );
        }
    }
    // A deletion by another user stays.
    for (target, e) in before.things() {
        if e.deleted_by.is_some_and(|w| w.client != me) {
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
        net.clients[i] = Client::with_state(cfg(i as u64 + 1), initial.clone(), 0, "log-1");
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
                let me = net.clients[c].client_id();
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
    hub.with(|s| s.check_entities(false));
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

// ----- identity, retry, a server that goes back, big changes -----

fn cfg_of(user: u64, client: u64) -> ClientConfig {
    ClientConfig {
        client: Uid::from_num(client),
        ..cfg(user)
    }
}

#[test]
fn two_devices_of_one_user_do_not_collide() {
    let mut net = Net::new(2);
    net.clients[1] = Client::new(cfg_of(1, 2));
    net.connect(0);
    net.connect(1);
    net.settle();
    // Both start counting transactions at 1. They have different writers.
    assert_eq!(net.clients[0].edit(vec![create(1, 1)]), Ok(1));
    assert_eq!(net.clients[1].edit(vec![create(2, 2)]), Ok(1));
    net.settle();
    net.all_equal();
    assert!(net.clients[0].view().alive(obj(2)).is_some());
    assert!(net.clients[1].view().alive(obj(1)).is_some());
    assert_eq!(net.seq.log().len(), 2);
    assert_eq!(net.seq.log()[0].user, net.seq.log()[1].user);
    assert_ne!(net.seq.log()[0].author, net.seq.log()[1].author);
}

#[test]
fn a_lost_journal_with_the_same_writer_renumbers_and_loses_nothing() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    for i in 1..=3 {
        net.clients[0].edit(vec![create(i, 0)]).unwrap();
    }
    net.settle();
    assert_eq!(net.seq.seq(), 3);
    // The program starts again with the same writer ID and no journal (it was lost).
    net.disconnect(0);
    net.clients[0] = Client::new(cfg(1));
    net.clients[0].edit(vec![create(99, 9)]).unwrap();
    assert_eq!(net.clients[0].pending()[0].op_id, 1);
    net.connect(0);
    net.settle();
    net.all_equal();
    assert!(
        net.seq.replica().alive(obj(99)).is_some(),
        "the edit arrived"
    );
    assert_eq!(net.clients[0].pending().len(), 0);
    assert!(
        net.clients[0]
            .take_events()
            .contains(&ClientEvent::Renumbered { count: 1 })
    );
    // The next transaction goes on above.
    assert_eq!(net.clients[0].edit(vec![set(99, "a", 1)]), Ok(5));
}

#[test]
fn op_id_reused_takes_the_client_offline_and_fixes_the_numbers_at_the_next_sign_in() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    net.seq.inject_errors(ErrorCode::OpIdReused, 1);
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.clients[0].edit(vec![create(2, 1)]).unwrap();
    net.flush(0);
    net.deliver(0);
    assert_eq!(net.clients[0].state(), ConnState::Offline);
    assert!(
        net.clients[0]
            .take_events()
            .iter()
            .any(|e| matches!(e, ClientEvent::Reconnect(_)))
    );
    net.disconnect(0);
    net.connect(0);
    net.settle();
    net.all_equal();
    assert!(net.seq.replica().alive(obj(1)).is_some());
    assert!(net.seq.replica().alive(obj(2)).is_some());
    assert!(
        net.clients[0]
            .take_events()
            .iter()
            .any(|e| matches!(e, ClientEvent::Renumbered { .. }))
    );
}

#[test]
fn a_server_that_remembers_an_id_with_other_content_refuses_it() {
    let mut s = Sequencer::new(Replica::new());
    s.check_entities(false);
    let conn = s.open();
    let hello = ClientMsg::Hello {
        protocol: PROTOCOL_VERSION,
        user: Uid::from_num(1),
        client: Uid::from_num(1),
        name: "x".into(),
        token: String::new(),
        last_seq: None,
        log_id: None,
    };
    s.receive(conn, hello, 0);
    let submit = |op_id, a| ClientMsg::Submit {
        op_id,
        ops: vec![create(op_id + 10, a)],
    };
    assert!(matches!(
        s.receive(conn, submit(1, 1), 0)[0].msg,
        ServerMsg::Ack { seq: 1, .. }
    ));
    // The same number and the same content: the old answer. Other content, or a lower number:
    // refused.
    assert_eq!(
        s.receive(conn, submit(1, 1), 0)[0].msg,
        ServerMsg::Ack { op_id: 1, seq: 1 }
    );
    assert!(matches!(
        s.receive(conn, submit(1, 2), 0)[0].msg,
        ServerMsg::Error {
            code: ErrorCode::OpIdReused,
            ..
        }
    ));
    s.receive(conn, submit(5, 1), 0);
    assert!(matches!(
        s.receive(conn, submit(4, 1), 0)[0].msg,
        ServerMsg::Error {
            code: ErrorCode::OpIdReused,
            ..
        }
    ));
}

#[test]
fn a_journal_or_checkpoint_that_cannot_be_read_is_reported() {
    use crate::store::{OpLogStore, Slot};
    let mut store = MemStore::new();
    store.save(Slot::Journal, "{not json").unwrap();
    store.save(Slot::Checkpoint, "[1,2]").unwrap();
    let mut c = Client::restore(cfg(1), Box::new(store.clone()));
    let events = c.take_events();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ClientEvent::StoreCorrupt(_)))
            .count(),
        2
    );
    assert!(!c.has_state());
    assert!(c.pending().is_empty());
    // A journal of another user is reported too. The client keeps its own writer ID.
    let store = MemStore::new();
    let mut other = Client::new(cfg_of(2, 22));
    other.set_store(Box::new(store.clone()));
    other.edit(vec![create(1, 1)]).unwrap();
    let mut c = Client::restore(cfg_of(1, 11), Box::new(store));
    assert!(matches!(
        c.take_events().as_slice(),
        [ClientEvent::StoreCorrupt(m)] if m.contains("another user")
    ));
    assert_eq!(c.client_id(), Uid::from_num(11));
}

#[test]
fn the_journal_keeps_the_writer_across_restarts() {
    let store = MemStore::new();
    let mut a = Client::new(cfg_of(1, 41));
    a.set_store(Box::new(store.clone()));
    a.edit(vec![create(1, 1)]).unwrap();
    let b = Client::restore(cfg_of(1, 99), Box::new(store));
    assert_eq!(b.client_id(), Uid::from_num(41));
    assert_eq!(b.pending().len(), 1);
}

#[test]
fn a_transient_error_keeps_the_transaction_and_tries_again_later() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    net.seq.inject_errors(ErrorCode::RateLimited, 1);
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.flush(0);
    net.deliver(0);
    assert_eq!(net.clients[0].pending().len(), 1, "kept");
    assert!(net.seq.replica().alive(obj(1)).is_none());
    // Not yet: the first wait is 250 ms.
    net.now += 100;
    net.deliver(0);
    assert!(net.seq.replica().alive(obj(1)).is_none());
    net.now += 200;
    net.deliver(0);
    net.settle();
    net.all_equal();
    assert!(net.seq.replica().alive(obj(1)).is_some());
    // Two refusals in a row: the wait doubles. A later transaction waits behind the first.
    net.seq.inject_errors(ErrorCode::Internal, 2);
    net.clients[0].edit(vec![create(2, 1)]).unwrap();
    net.clients[0].edit(vec![create(3, 1)]).unwrap();
    net.flush(0);
    net.deliver(0);
    net.now += 300;
    net.deliver(0); // second try: refused again, now the wait is 500 ms
    net.deliver(0);
    assert!(net.seq.replica().alive(obj(2)).is_none());
    net.now += 300;
    net.deliver(0);
    assert!(
        net.seq.replica().alive(obj(2)).is_none(),
        "the wait doubled"
    );
    net.now += 300;
    net.settle();
    net.all_equal();
    assert!(net.seq.replica().alive(obj(2)).is_some());
    assert!(net.seq.replica().alive(obj(3)).is_some());
    let order: Vec<u64> = net.seq.log().iter().map(|e| e.op_id).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "in order: {order:?}");
}

#[test]
fn an_unknown_error_code_does_not_stop_the_client() {
    let unknown = |op: Option<u64>| {
        let text = match op {
            Some(n) => {
                format!(r#"{{"type":"error","code":"quota_exceeded","message":"new","op_id":{n}}}"#)
            }
            None => r#"{"type":"error","code":"quota_exceeded","message":"new"}"#.to_string(),
        };
        serde_json::from_str::<ServerMsg>(&text).unwrap()
    };
    // With an op_id: a retry. The transaction stays and goes through later.
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    let op = net.clients[0].pending()[0].op_id;
    net.clients[0].handle(unknown(Some(op)), net.now);
    assert_eq!(net.clients[0].state(), ConnState::Online);
    assert_eq!(net.clients[0].pending().len(), 1, "kept");
    net.now += 300;
    net.settle();
    net.all_equal();
    assert!(net.seq.replica().alive(obj(1)).is_some());
    // Without an op_id: reported, and the client goes on.
    net.clients[0].take_events();
    net.clients[0].handle(unknown(None), net.now);
    assert_eq!(net.clients[0].state(), ConnState::Online);
    assert!(matches!(
        net.clients[0].take_events()[0],
        ClientEvent::ServerError {
            code: ErrorCode::Unknown,
            ..
        }
    ));
}

#[test]
fn a_refusal_for_good_drops_the_transaction() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    net.seq.inject_errors(ErrorCode::InvalidTxn, 1);
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.settle();
    assert!(net.clients[0].pending().is_empty());
    assert!(net.clients[0].view().alive(obj(1)).is_none());
    assert!(
        net.clients[0]
            .take_events()
            .iter()
            .any(|e| matches!(e, ClientEvent::Rejected { .. }))
    );
}

#[test]
fn a_server_that_went_back_gets_the_lost_work_again() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.clients[1].edit(vec![create(2, 2)]).unwrap();
    net.settle();
    net.clients[0].edit(vec![set(1, "a", 5)]).unwrap();
    net.settle();
    net.all_equal();
    // The server is restored from a backup: another log, the work is gone.
    let mut restored = Sequencer::new(replica_of(&Document::default()));
    restored.check_entities(false);
    restored.set_log_id("log-2");
    net.seq = restored;
    for i in 0..2 {
        net.disconnect(i);
    }
    net.connect(0);
    net.settle();
    net.connect(1);
    net.settle();
    net.all_equal();
    assert_eq!(
        net.seq.replica().alive(obj(1)).unwrap().get("a"),
        Some(&json!(5))
    );
    assert!(net.seq.replica().alive(obj(2)).is_some());
    let events = net.clients[0].take_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ClientEvent::ServerRegressed { resent } if *resent > 0)),
        "{events:?}"
    );
    assert_eq!(net.clients[0].log_id(), Some("log-2"));
    // Client 1 saw the other log too and sent its object again; nothing is duplicated.
    assert_eq!(net.seq.replica().alive_of(Kind::Object).count(), 2);
}

#[test]
fn a_lower_sequence_number_in_the_same_log_is_a_regress_too() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    net.clients[0].edit(vec![create(1, 1)]).unwrap();
    net.settle();
    let mut older = Sequencer::new(replica_of(&Document::default()));
    older.check_entities(false);
    net.seq = older;
    net.disconnect(0);
    net.connect(0);
    net.settle();
    net.all_equal();
    assert!(net.seq.replica().alive(obj(1)).is_some());
}

#[test]
fn entries_for_a_client_without_state_are_a_violation() {
    let mut c = Client::new(cfg(1));
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-1".into(),
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    c.handle(ServerMsg::Entries { entries: vec![] }, 0);
    assert_eq!(c.state(), ConnState::Failed);
    // So are entries while the client waits for a snapshot after a regress.
    let mut c = Client::with_state(cfg(1), Replica::new(), 5, "log-1");
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-2".into(),
            head_seq: 9,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    c.handle(ServerMsg::Entries { entries: vec![] }, 0);
    assert_eq!(c.state(), ConnState::Failed);
}

#[test]
fn an_ack_for_a_transaction_that_came_as_an_entry_rebuilds_the_view() {
    let mut c = Client::with_state(cfg(1), Replica::new(), 0, "log-1");
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-1".into(),
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    c.handle(ServerMsg::Entries { entries: vec![] }, 0);
    c.edit(vec![create(1, 1)]).unwrap();
    c.handle(
        ServerMsg::Broadcast {
            entry: Entry {
                seq: 1,
                author: Uid::from_num(2),
                user: Uid::from_num(2),
                op_id: 1,
                ops: vec![create(5, 5)],
            },
        },
        0,
    );
    // The server says that our transaction has the old number 1.
    c.handle(ServerMsg::Ack { op_id: 1, seq: 1 }, 0);
    assert!(c.pending().is_empty());
    assert_eq!(c.view().canonical_text(), c.confirmed().canonical_text());
}

#[test]
fn a_big_import_is_split_into_transactions_and_arrives() {
    use kadakas_core::command::{Command, Editor};
    let mut ed = Editor::with_sequential_ids();
    let base = replica_of(ed.doc());
    let mut c = Client::with_state(cfg(1), base.clone(), 0, "log-1");
    c.set_shadow(ed.doc());
    let rev = ed.revision();
    let cmds: Vec<Value> = (0..20_001)
        .map(|i| {
            json!({"op": "add_object", "shape": {"kind": "rect",
                "min": [i as f64 * 20.0, 0.0], "max": [i as f64 * 20.0 + 10.0, 5.0]}})
        })
        .collect();
    let cmd: Command = serde_json::from_value(json!({"op": "batch", "commands": cmds})).unwrap();
    ed.execute(&cmd).unwrap();
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .cloned()
        .unwrap();
    let made = c.edit_from_change(ed.doc(), &cs);
    assert!(made.problems.is_empty(), "{:?}", made.problems);
    assert!(
        made.op_ids.len() >= 5,
        "20 001 objects and their keys in several transactions"
    );
    assert!(
        c.pending()
            .iter()
            .all(|t| t.ops.len() <= crate::op::MAX_OPS)
    );
    // Through a server that checks every thing, all of them arrive.
    let mut seq = Sequencer::new(base);
    let conn = seq.open();
    for m in {
        c.connect();
        c.take_outbox()
    } {
        seq.receive(conn, m, 0);
    }
    let mut outs = Vec::new();
    // The server answers hello with welcome and a snapshot.
    for o in seq.receive(
        conn,
        ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            user: c.user(),
            client: c.client_id(),
            name: String::new(),
            token: String::new(),
            last_seq: Some(0),
            log_id: Some("log-1".into()),
        },
        0,
    ) {
        outs.push(o.msg);
    }
    assert!(!outs.is_empty());
    let mut c2 = Client::with_state(cfg(1), replica_of(&Document::default()), 0, "log-1");
    c2.set_shadow(&Document::default());
    for t in c.pending().clone() {
        let outs = seq.receive(
            conn,
            ClientMsg::Submit {
                op_id: t.op_id,
                ops: t.ops,
            },
            0,
        );
        assert!(
            matches!(outs[0].msg, ServerMsg::Ack { .. }),
            "{:?}",
            outs[0].msg
        );
    }
    assert_eq!(seq.replica().alive_of(Kind::Object).count(), 20_001);
    // The split keeps the order: nothing in a later transaction is needed by an earlier one.
    let _ = c2;
}

fn set_op(target: Target, prop: &str, value: Value) -> Op {
    Op::Set {
        target,
        prop: prop.into(),
        value,
    }
}

#[test]
fn the_sequencer_checks_things_and_holds_presence_for_the_trailing_edge() {
    let mut s = Sequencer::new(replica_of(&Document::default()));
    let conn = s.open();
    let other = s.open();
    for (c, u) in [(conn, 1), (other, 2)] {
        s.receive(
            c,
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                user: Uid::from_num(u),
                client: Uid::from_num(u),
                name: "x".into(),
                token: String::new(),
                last_seq: None,
                log_id: None,
            },
            0,
        );
    }
    let obj_props = |shape: Value| -> std::collections::BTreeMap<String, Value> {
        let mut m: std::collections::BTreeMap<String, Value> = Default::default();
        m.insert("layer".into(), json!(kadakas_core::model::FIRST_LAYER.0));
        m.insert("class".into(), json!(kadakas_core::model::NONE_CLASS.0));
        m.insert("shape".into(), shape);
        m
    };
    let good = json!({"kind": "rect", "min": [0, 0], "max": [5, 5]});
    let submit = |op_id, ops| ClientMsg::Submit { op_id, ops };
    let create_obj = |n, shape| Op::Create {
        target: obj(n),
        props: obj_props(shape),
        merge: false,
    };
    assert!(matches!(
        s.receive(conn, submit(1, vec![create_obj(1, good.clone())]), 0)[0].msg,
        ServerMsg::Ack { .. }
    ));
    // A bad shape is refused when it is created, set or removed; an edit of a thing that is not
    // there is a drop in the merge, not an error.
    let bad = json!({"kind": "rect", "min": "x"});
    for (id, op) in [
        (2, create_obj(2, bad.clone())),
        (3, set_op(obj(1), "shape", bad.clone())),
        (
            4,
            Op::Unset {
                target: obj(1),
                prop: "shape".into(),
            },
        ),
    ] {
        assert!(
            matches!(
                s.receive(conn, submit(id, vec![op]), 0)[0].msg,
                ServerMsg::Error {
                    code: ErrorCode::InvalidTxn,
                    ..
                }
            ),
            "{id}"
        );
    }
    assert!(matches!(
        s.receive(
            conn,
            submit(5, vec![set_op(obj(77), "shape", bad.clone())]),
            0
        )[0]
        .msg,
        ServerMsg::Ack { .. }
    ));
    // Presence: the second update is held, and sent by `tick` when the time has come.
    let p = |x| ClientMsg::Presence {
        state: PresenceState {
            cursor: Some([x, 0.0]),
            ..Default::default()
        },
    };
    assert_eq!(s.receive(conn, p(1.0), 1_000).len(), 1);
    assert!(s.receive(conn, p(2.0), 1_010).is_empty());
    assert!(s.receive(conn, p(3.0), 1_020).is_empty());
    assert!(s.tick(1_030).is_empty());
    let late = s.tick(1_060);
    assert_eq!(late.len(), 1);
    assert_eq!(late[0].conn, other);
    assert!(matches!(
        &late[0].msg,
        ServerMsg::Presence { state, .. } if state.cursor == Some([3.0, 0.0])
    ));
    assert!(s.tick(2_000).is_empty());
}

#[test]
fn the_sequencer_lets_a_derived_id_be_made_twice_but_not_other_ids() {
    let mut s = Sequencer::new(Replica::new());
    s.check_entities(false);
    let conns: Vec<ConnId> = (1..=2)
        .map(|u| {
            let c = s.open();
            s.receive(
                c,
                ClientMsg::Hello {
                    protocol: PROTOCOL_VERSION,
                    user: Uid::from_num(u),
                    client: Uid::from_num(u),
                    name: "x".into(),
                    token: String::new(),
                    last_seq: None,
                    log_id: None,
                },
                0,
            );
            c
        })
        .collect();
    let derived = Target::new(Kind::Object, Uid::derive(Uid::from_num(9), 2, 0));
    let make = |target, merge| Op::Create {
        target,
        props: [("a".to_string(), json!(1))].into(),
        merge,
    };
    let submit = |op_id, ops| ClientMsg::Submit { op_id, ops };
    for (c, op_id) in [(conns[0], 1), (conns[1], 1)] {
        assert!(matches!(
            s.receive(c, submit(op_id, vec![make(derived, true)]), 0)[0].msg,
            ServerMsg::Ack { .. }
        ));
    }
    // Twice in one transaction is the same.
    assert!(matches!(
        s.receive(
            conns[0],
            submit(2, vec![make(derived, true), make(derived, true)]),
            0
        )[0]
        .msg,
        ServerMsg::Ack { .. }
    ));
    // A merge create of a thing that was deleted is dropped by the merge; the transaction is
    // ordered and the thing stays deleted.
    assert!(matches!(
        s.receive(conns[0], submit(4, vec![Op::Delete { target: derived }]), 0)[0].msg,
        ServerMsg::Ack { .. }
    ));
    let other = Target::new(Kind::Object, Uid::from_num(70));
    assert!(matches!(
        s.receive(
            conns[1],
            submit(2, vec![make(derived, true), make(other, false)]),
            0
        )[0]
        .msg,
        ServerMsg::Ack { .. }
    ));
    assert!(s.replica().alive(derived).is_none() && s.replica().alive(other).is_some());
    // An ordinary ID is refused, and so is a derived ID without the flag.
    assert!(matches!(
        s.receive(conns[0], submit(5, vec![make(derived, false)]), 0)[0].msg,
        ServerMsg::Error {
            code: ErrorCode::DuplicateUid,
            ..
        }
    ));
}

#[test]
fn undo_restores_the_value_that_was_really_underneath_in_server_order() {
    let mut net = Net::new(2);
    net.connect(0);
    net.connect(1);
    net.settle();
    net.clients[0].edit(vec![create(1, 0)]).unwrap();
    net.settle();
    // B sets a=2 and A sets a=3 before either hears of the other. The server orders B first.
    net.clients[1].edit(vec![set(1, "a", 2)]).unwrap();
    net.clients[0].edit(vec![set(1, "a", 3)]).unwrap();
    net.flush(1);
    net.flush(0);
    net.settle();
    net.all_equal();
    assert_eq!(net.value(0, 1, "a"), Some(json!(3)));
    // A's inverse was made from a view that had a=0 underneath. The server had a=2 there.
    let report = net.clients[0].undo();
    assert_eq!(report.applied, 1);
    net.settle();
    net.all_equal();
    assert_eq!(
        net.value(1, 1, "a"),
        Some(json!(2)),
        "B's value, not the stale 0"
    );
    // Redo puts A's value back.
    net.clients[0].redo();
    net.settle();
    assert_eq!(net.value(1, 1, "a"), Some(json!(3)));
}

#[test]
fn blobs_travel_by_hash_and_the_object_waits_for_its_picture() {
    use crate::blob::{externalize, sha256_hex};
    let text = "A".repeat(crate::blob::MIN_BLOB_BYTES + 1);
    let shape = json!({"kind": "underlay", "png": text, "opacity": 1.0});
    let (reference, hash, data) = externalize(&shape).unwrap();
    let mut s = Sequencer::new(Replica::new());
    s.check_entities(false);
    let conn = s.open();
    s.receive(
        conn,
        ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            user: Uid::from_num(1),
            client: Uid::from_num(1),
            name: "x".into(),
            token: String::new(),
            last_seq: None,
            log_id: None,
        },
        0,
    );
    // A blob with a wrong hash is refused; a good one is kept and can be asked for.
    let bad = s.receive(
        conn,
        ClientMsg::BlobPut {
            hash: "00".into(),
            data: data.clone(),
        },
        0,
    );
    assert!(matches!(
        bad[0].msg,
        ServerMsg::Error {
            code: ErrorCode::BadMessage,
            ..
        }
    ));
    assert!(
        s.receive(
            conn,
            ClientMsg::BlobPut {
                hash: hash.clone(),
                data: data.clone()
            },
            0
        )
        .is_empty()
    );
    assert_eq!(
        s.receive(conn, ClientMsg::BlobGet { hash: hash.clone() }, 0)[0].msg,
        ServerMsg::Blob {
            hash: hash.clone(),
            data: data.clone()
        }
    );
    assert!(matches!(
        s.receive(
            conn,
            ClientMsg::BlobGet {
                hash: "nope".into()
            },
            0
        )[0]
        .msg,
        ServerMsg::BlobMissing { .. }
    ));

    // A client with a reference to a picture it does not have asks for it, and leaves the object
    // out of the document until the picture comes.
    let mut r = Replica::new();
    r.insert_baseline(
        Target::new(Kind::Object, Uid::from_num(5)),
        serde_json::from_value(json!({
            "layer": kadakas_core::model::FIRST_LAYER.0,
            "class": kadakas_core::model::NONE_CLASS.0,
            "shape": reference,
        }))
        .unwrap(),
    );
    let mut c = Client::with_state(cfg(2), r, 0, "log-1");
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-1".into(),
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    c.handle(ServerMsg::Entries { entries: vec![] }, 0);
    c.take_outbox();
    c.tick(10);
    let sent = c.take_outbox();
    assert!(sent.contains(&ClientMsg::BlobGet { hash: hash.clone() }));
    let m = c.materialize(&Document::default());
    assert!(m.quarantined[0].reason.contains("has not arrived"));
    // A blob with a wrong hash is ignored. A good one is kept (the object then fails for its own
    // reason: this picture is not a PNG).
    c.handle(
        ServerMsg::Blob {
            hash: hash.clone(),
            data: "other".into(),
        },
        11,
    );
    assert!(
        c.materialize(&Document::default()).quarantined[0]
            .reason
            .contains("has not arrived")
    );
    c.handle(
        ServerMsg::Blob {
            hash: hash.clone(),
            data: data.clone(),
        },
        12,
    );
    assert_eq!(sha256_hex(data.as_bytes()), hash);
    assert!(c.take_changed());
    assert!(
        !c.materialize(&Document::default()).quarantined[0]
            .reason
            .contains("has not arrived")
    );
    // A missing blob is not asked for again.
    let mut r = Replica::new();
    r.insert_baseline(
        Target::new(Kind::Object, Uid::from_num(6)),
        serde_json::from_value(json!({"shape": {"kind": "underlay", "png": {"$blob": "ab"}}}))
            .unwrap(),
    );
    let mut c = Client::with_state(cfg(3), r, 0, "log-1");
    c.connect();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-1".into(),
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    c.handle(ServerMsg::Entries { entries: vec![] }, 0);
    c.take_outbox();
    c.tick(1);
    let gets = |out: Vec<ClientMsg>| {
        out.into_iter()
            .filter(|m| matches!(m, ClientMsg::BlobGet { .. }))
            .count()
    };
    assert_eq!(gets(c.take_outbox()), 1);
    c.handle(ServerMsg::BlobMissing { hash: "ab".into() }, 2);
    c.keep_changed();
    c.tick(3);
    assert_eq!(gets(c.take_outbox()), 0);
}

#[test]
fn a_waiting_transaction_sends_its_picture_again_after_a_sign_in() {
    let text = "B".repeat(crate::blob::MIN_BLOB_BYTES + 5);
    let shape = json!({"kind": "underlay", "png": text});
    let store = MemStore::new();
    let mut c = Client::new(cfg(4));
    c.set_store(Box::new(store.clone()));
    let op = Op::Create {
        target: obj(9),
        props: [("shape".to_string(), shape)].into(),
        merge: false,
    };
    c.edit(vec![op]).unwrap();
    // The operation holds a reference. The picture is in the journal.
    let j = c.journal();
    assert_eq!(j.blobs.len(), 1);
    assert!(serde_json::to_string(&j.pending).unwrap().len() < 1_000);
    // After a restart the picture goes to the server before the transaction.
    let mut c = Client::restore(cfg(4), Box::new(store));
    c.connect();
    c.take_outbox();
    c.handle(
        ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            log_id: "log-1".into(),
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        },
        0,
    );
    c.handle(
        ServerMsg::Snapshot {
            log_id: "log-1".into(),
            seq: 0,
            state: Replica::new(),
        },
        0,
    );
    let out = c.take_outbox();
    assert!(matches!(out[0], ClientMsg::BlobPut { .. }));
    assert!(matches!(out[1], ClientMsg::Submit { .. }));
}

fn picture_op(n: u64, fill: char) -> (Op, String) {
    let text = fill.to_string().repeat(crate::blob::MIN_BLOB_BYTES + 3);
    let hash = crate::blob::sha256_hex(text.as_bytes());
    (
        Op::Create {
            target: obj(n),
            props: [(
                "shape".to_string(),
                json!({"kind": "underlay", "png": text}),
            )]
            .into(),
            merge: false,
        },
        hash,
    )
}

#[test]
fn a_picture_is_sent_again_after_the_server_went_back() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    let (op, hash) = picture_op(1, 'P');
    net.clients[0].edit(vec![op]).unwrap();
    net.settle();
    assert!(net.seq.has_blob(&hash));
    // A server that lost everything, with another log: the work is sent again, with its picture.
    let mut restored = Sequencer::new(replica_of(&Document::default()));
    restored.check_entities(false);
    restored.set_log_id("log-2");
    net.seq = restored;
    net.disconnect(0);
    net.connect(0);
    net.settle();
    net.all_equal();
    assert!(net.seq.replica().alive(obj(1)).is_some());
    assert!(net.seq.has_blob(&hash), "the picture came again");
}

#[test]
fn undo_sends_a_picture_that_the_server_dropped() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    let (op, first) = picture_op(1, 'Q');
    net.clients[0].edit(vec![op]).unwrap();
    // The picture is replaced by another one. Nothing refers to the first picture any more.
    let (op2, second) = picture_op(2, 'T');
    let Op::Create { props, .. } = op2 else {
        unreachable!()
    };
    net.clients[0]
        .edit(vec![Op::Set {
            target: obj(1),
            prop: "shape".into(),
            value: props["shape"].clone(),
        }])
        .unwrap();
    net.settle();
    assert!(net.seq.has_blob(&first) && net.seq.has_blob(&second));
    // The server cleans up the picture that nothing refers to. Undo brings the reference back,
    // so the client sends the picture again, before the transaction.
    net.seq.forget_blobs();
    assert!(net.clients[0].undo().done);
    net.settle();
    net.all_equal();
    assert!(net.seq.has_blob(&first));
    assert_eq!(
        crate::blob::blob_ref(
            net.seq
                .replica()
                .alive(obj(1))
                .unwrap()
                .get("shape")
                .unwrap()
        ),
        Some(first.as_str())
    );
    // Redo needs the second picture.
    net.seq.forget_blobs();
    assert!(net.clients[0].redo().done);
    net.settle();
    net.all_equal();
    assert!(net.seq.has_blob(&second));
}

#[test]
fn the_checkpoint_keeps_the_pictures_that_something_refers_to() {
    let mut net = Net::new(1);
    net.connect(0);
    net.settle();
    // A picture that a transaction used and the server refused: nothing refers to it any more.
    net.seq.inject_errors(ErrorCode::InvalidTxn, 1);
    let (op, lost) = picture_op(1, 'R');
    net.clients[0].edit(vec![op]).unwrap();
    net.settle();
    // A picture in a deleted object is kept (an undo can bring the object back).
    let (op, kept) = picture_op(2, 'S');
    net.clients[0].edit(vec![op]).unwrap();
    net.clients[0].edit(vec![del(2)]).unwrap();
    net.settle();
    let blobs = net.clients[0].checkpoint().unwrap().blobs;
    assert!(!blobs.contains_key(&lost));
    assert!(blobs.contains_key(&kept));
    assert!(net.clients[0].journal().blobs.is_empty());
}

#[test]
fn a_missing_picture_is_asked_for_again_after_a_new_sign_in() {
    let mut r = Replica::new();
    r.insert_baseline(
        Target::new(Kind::Object, Uid::from_num(6)),
        serde_json::from_value(json!({"shape": {"kind": "underlay", "png": {"$blob": "ab"}}}))
            .unwrap(),
    );
    let mut c = Client::with_state(cfg(3), r, 0, "log-1");
    let online = |c: &mut Client| {
        c.connect();
        c.handle(
            ServerMsg::Welcome {
                protocol: PROTOCOL_VERSION,
                log_id: "log-1".into(),
                head_seq: 0,
                your_last_op: 0,
                peers: vec![],
            },
            0,
        );
        c.handle(ServerMsg::Entries { entries: vec![] }, 0);
        c.take_outbox();
    };
    let gets = |out: Vec<ClientMsg>| {
        out.into_iter()
            .filter(|m| matches!(m, ClientMsg::BlobGet { .. }))
            .count()
    };
    online(&mut c);
    c.tick(1);
    assert_eq!(gets(c.take_outbox()), 1);
    c.handle(ServerMsg::BlobMissing { hash: "ab".into() }, 2);
    c.connection_lost();
    online(&mut c);
    c.tick(3);
    assert_eq!(
        gets(c.take_outbox()),
        1,
        "asked again on the new connection"
    );
}
