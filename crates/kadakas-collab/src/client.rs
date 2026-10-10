//! The client state machine.
//!
//! [`Client`] holds no connection. It takes messages in ([`Client::handle`]) and gives messages
//! out ([`Client::take_outbox`]). A host (see `transport::Link`) moves the messages. This keeps
//! the client pure, easy to test and usable in the browser.
//!
//! # Two states of the document
//!
//! The client keeps two replicas:
//!
//! - `confirmed`: the server's state at the sequence number `confirmed_seq`. Only transactions in
//!   the order of the server change it.
//! - `view`: what the user sees: `confirmed` plus the *pending* transactions (made here, not yet
//!   acknowledged), applied in the order they were made.
//!
//! The invariant is `view = confirmed + pending`.
//!
//! # Optimistic apply and rebase
//!
//! A local edit goes to `view` at once (the user sees it without delay) and to the end of the
//! pending queue. When the client is online, it is sent at once. When the server answers with
//! `ack`, the transaction moves from the queue into `confirmed`. Nothing changes in `view`, since
//! `view` already had it in the right place.
//!
//! When a transaction of another user arrives, it is ordered by the server *before* all of our
//! pending transactions (the server has not seen ours yet, or has put them later). So it goes
//! into `confirmed`, and `view` is built again: `confirmed` plus the pending transactions on
//! top. This is the **rebase**: our pending operations are replayed on the new base. Because
//! operations are property-level and the rules are last-writer-wins (see `replica`), replaying is
//! always defined. An operation that has no effect on the new base is dropped by the replay: a
//! `Set` on an object that the other user deleted does nothing, as the server will also do
//! nothing with it.
//!
//! After a reconnect, the same replay runs once against the caught-up `confirmed`, and it also
//! removes the dropped operations from the queue, so they are not sent. Then the queue goes to the
//! server in order.
//!
//! # Connection states
//!
//! `Offline` (or `Failed` after a fatal error) -> `connect()` sends `hello` ->
//! `Connecting` -> `welcome` -> `Syncing` -> `entries` or `snapshot` -> `Online`.
//! A lost connection goes back to `Offline`. Edits work in every state.
//!
//! # Offline work
//!
//! Edits made offline stay in the pending queue. With an [`OpLogStore`] the queue, the undo
//! stacks and the transaction counter are saved after every change, so a restart loses nothing
//! (see `store`).

use std::collections::VecDeque;

use kadakas_core::change::ChangeSet;
use kadakas_core::model::Document;
use kadakas_core::uid::Uid;
use serde::{Deserialize, Serialize};

use crate::docsync;
use crate::op::{InvalidTxn, Op, Txn, Writer, validate_ops};
use crate::presence::{self, Outgoing, Peer, Peers, PresenceError, PresenceState};
use crate::protocol::{ClientMsg, Entry, ErrorCode, PROTOCOL_VERSION, ServerMsg};
use crate::replica::Replica;
use crate::store::{Checkpoint, Journal, OpLogStore, Slot};
use crate::undo::{MAX_STEPS, UndoEntry, UndoReport, filter_inverse};

/// Who the client is and how it signs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientConfig {
    pub user: Uid,
    pub name: String,
    /// The token for the server. The meaning is up to the server.
    pub token: String,
}

/// Where the connection is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnState {
    Offline,
    /// `hello` is sent.
    Connecting,
    /// `welcome` arrived. The client waits for the missed entries or the snapshot.
    Syncing,
    Online,
    /// The server refused the client for good (wrong token or protocol version).
    Failed,
}

/// Things a host may want to know. Read them with [`Client::take_events`].
#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    /// The sync is done and the client is online.
    Connected,
    /// The connection ended.
    Disconnected,
    /// The server refused a transaction. The edit is gone from the view.
    Rejected {
        op_id: u64,
        code: ErrorCode,
        message: String,
    },
    /// The server sent an error that is not about one transaction.
    ServerError { code: ErrorCode, message: String },
    /// The server refused the client for good.
    Failed { code: ErrorCode, message: String },
    /// A sequence number was missing. The client went offline: connect again.
    GapDetected { expected: u64, got: u64 },
    /// The store could not save.
    StoreFailed(String),
}

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error(transparent)]
    Invalid(#[from] InvalidTxn),
    /// No operation changes anything (for example, all of them work on deleted objects).
    #[error("the edit changes nothing")]
    NoEffect,
}

/// The state machine of one collaboration client.
pub struct Client {
    cfg: ClientConfig,
    state: ConnState,
    confirmed: Replica,
    confirmed_seq: u64,
    /// True once the client has a confirmed state (from a snapshot, a restore or `with_state`).
    has_state: bool,
    head_seq: u64,
    view: Replica,
    pending: VecDeque<Txn>,
    last_op: u64,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    outbox: Vec<ClientMsg>,
    events: Vec<ClientEvent>,
    out_presence: Outgoing,
    peers: Peers,
    changed: bool,
    store: Option<Box<dyn OpLogStore>>,
}

impl Client {
    /// A client without state. It gets its state from the first sync.
    pub fn new(cfg: ClientConfig) -> Client {
        Client {
            cfg,
            state: ConnState::Offline,
            confirmed: Replica::new(),
            confirmed_seq: 0,
            has_state: false,
            head_seq: 0,
            view: Replica::new(),
            pending: VecDeque::new(),
            last_op: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            outbox: Vec::new(),
            events: Vec::new(),
            out_presence: Outgoing::default(),
            peers: Peers::default(),
            changed: false,
            store: None,
        }
    }

    /// A client that starts from a known confirmed state, for example a document that the server
    /// holds at `seq`.
    pub fn with_state(cfg: ClientConfig, state: Replica, seq: u64) -> Client {
        let mut c = Client::new(cfg);
        c.view = state.clone();
        c.confirmed = state;
        c.confirmed_seq = seq;
        c.head_seq = seq;
        c.has_state = true;
        c
    }

    /// A client that continues from a store: the checkpoint gives the confirmed state, the
    /// journal gives the pending transactions, the undo stacks and the counter. A journal of
    /// another user is ignored. The store stays attached.
    pub fn restore(cfg: ClientConfig, store: Box<dyn OpLogStore>) -> Client {
        let mut c = Client::new(cfg);
        if let Some(cp) = store
            .load(Slot::Checkpoint)
            .and_then(|t| serde_json::from_str::<Checkpoint>(&t).ok())
        {
            c.confirmed = cp.state;
            c.confirmed_seq = cp.seq;
            c.head_seq = cp.seq;
            c.has_state = true;
        }
        if let Some(j) = store
            .load(Slot::Journal)
            .and_then(|t| serde_json::from_str::<Journal>(&t).ok())
            .filter(|j| j.user == c.cfg.user)
        {
            c.last_op = j.last_op;
            c.pending = j.pending.into();
            c.undo = j.undo;
            c.redo = j.redo;
        }
        c.store = Some(store);
        c.rebuild_view();
        c
    }

    /// Attaches a store. It gets the journal and the checkpoint now.
    pub fn set_store(&mut self, store: Box<dyn OpLogStore>) {
        self.store = Some(store);
        self.save_checkpoint();
        self.save_journal();
    }

    pub fn user(&self) -> Uid {
        self.cfg.user
    }

    pub fn name(&self) -> &str {
        &self.cfg.name
    }

    pub fn state(&self) -> ConnState {
        self.state
    }

    /// The last sequence number in `confirmed`.
    pub fn confirmed_seq(&self) -> u64 {
        self.confirmed_seq
    }

    /// The last sequence number the server reported.
    pub fn head_seq(&self) -> u64 {
        self.head_seq
    }

    /// What the user sees.
    pub fn view(&self) -> &Replica {
        &self.view
    }

    /// The state that the server confirmed.
    pub fn confirmed(&self) -> &Replica {
        &self.confirmed
    }

    /// The transactions that wait for the server.
    pub fn pending(&self) -> &VecDeque<Txn> {
        &self.pending
    }

    pub fn peers(&self) -> &Peers {
        &self.peers
    }

    /// True once the client has a confirmed state: from a sync, a checkpoint or `with_state`.
    pub fn has_state(&self) -> bool {
        self.has_state
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// True if the view changed since the last call by something other than an edit that the
    /// host made itself: a change of another user, a rebase, or an undo or redo. The host builds
    /// the document again then (see `docsync::materialize`). Edits made with [`Client::edit`] do
    /// not set it, because the host's document has them already.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    pub fn take_events(&mut self) -> Vec<ClientEvent> {
        std::mem::take(&mut self.events)
    }

    /// The messages to send to the server, in order.
    pub fn take_outbox(&mut self) -> Vec<ClientMsg> {
        std::mem::take(&mut self.outbox)
    }

    // ----- connection -----

    /// Starts the sign-in: queues `hello`. Does nothing unless the client is offline or failed.
    pub fn connect(&mut self) {
        if !matches!(self.state, ConnState::Offline | ConnState::Failed) {
            return;
        }
        self.state = ConnState::Connecting;
        self.outbox.clear();
        self.outbox.push(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            user: self.cfg.user,
            name: self.cfg.name.clone(),
            token: self.cfg.token.clone(),
            last_seq: self.has_state.then_some(self.confirmed_seq),
        });
    }

    /// The connection ended (the host noticed). Edits go on in the queue.
    pub fn connection_lost(&mut self) {
        self.outbox.clear();
        self.go_offline();
    }

    /// Says goodbye to the server and goes offline. `Bye` waits in the outbox.
    pub fn disconnect(&mut self) {
        let bye = self.state == ConnState::Online;
        self.outbox.clear();
        if bye {
            self.outbox.push(ClientMsg::Bye);
        }
        self.go_offline();
    }

    fn go_offline(&mut self) {
        if self.state != ConnState::Offline {
            self.events.push(ClientEvent::Disconnected);
        }
        if self.state != ConnState::Failed {
            self.state = ConnState::Offline;
        }
        self.peers.clear();
        self.save_checkpoint();
    }

    // ----- edits -----

    /// Applies a local edit: the view changes at once, the transaction goes into the queue and,
    /// when online, to the server. Operations without effect are dropped from the transaction.
    /// Returns the transaction number.
    pub fn edit(&mut self, ops: Vec<Op>) -> Result<u64, EditError> {
        let (op_id, inverse) = self.commit(ops)?;
        self.undo.push(UndoEntry {
            of_op: op_id,
            inverse,
        });
        trim(&mut self.undo);
        self.redo.clear();
        Ok(op_id)
    }

    /// Turns a change set of the local document into an edit (see `docsync::ops_from_change`).
    /// `Ok(None)` if the change set has no shared effect.
    pub fn edit_from_change(
        &mut self,
        doc: &Document,
        cs: &ChangeSet,
    ) -> Result<Option<u64>, EditError> {
        let ops = docsync::ops_from_change(&self.view, doc, cs);
        self.edit_or_none(ops)
    }

    /// Like [`Client::edit_from_change`], comparing the whole document.
    pub fn edit_from_diff(&mut self, doc: &Document) -> Result<Option<u64>, EditError> {
        let ops = docsync::ops_from_diff(&self.view, doc);
        self.edit_or_none(ops)
    }

    fn edit_or_none(&mut self, ops: Vec<Op>) -> Result<Option<u64>, EditError> {
        if ops.is_empty() {
            return Ok(None);
        }
        match self.edit(ops) {
            Ok(id) => Ok(Some(id)),
            Err(EditError::NoEffect) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Applies the operations to the view, queues the transaction, and returns its number and
    /// the inverse operations (in the order to apply them).
    fn commit(&mut self, ops: Vec<Op>) -> Result<(u64, Vec<Op>), EditError> {
        validate_ops(&ops)?;
        let op_id = self.last_op + 1;
        let w = Writer {
            user: self.cfg.user,
            op: op_id,
        };
        let mut applied = Vec::new();
        let mut inverse = Vec::new();
        for op in ops {
            let (out, inv) = self.view.apply_inverse(w, &op);
            if out.is_applied() {
                applied.push(op);
                inverse.extend(inv);
            }
        }
        if applied.is_empty() {
            return Err(EditError::NoEffect);
        }
        inverse.reverse();
        self.last_op = op_id;
        if self.state == ConnState::Online {
            self.outbox.push(ClientMsg::Submit {
                op_id,
                ops: applied.clone(),
            });
        }
        self.pending.push_back(Txn {
            op_id,
            ops: applied,
        });
        self.save_journal();
        Ok((op_id, inverse))
    }

    /// Takes back your last edit (see `undo` for the rules). Other users' work stays.
    pub fn undo(&mut self) -> UndoReport {
        let Some(entry) = self.undo.pop() else {
            return UndoReport::default();
        };
        let (report, redo) = self.step(entry);
        if let Some(r) = redo {
            self.redo.push(r);
            trim(&mut self.redo);
        }
        self.save_journal();
        report
    }

    /// Does again what the last undo took back.
    pub fn redo(&mut self) -> UndoReport {
        let Some(entry) = self.redo.pop() else {
            return UndoReport::default();
        };
        let (report, undo) = self.step(entry);
        if let Some(u) = undo {
            self.undo.push(u);
            trim(&mut self.undo);
        }
        self.save_journal();
        report
    }

    /// Sends the inverse operations of a step that are still safe. Returns the report and the
    /// step that undoes this step.
    fn step(&mut self, entry: UndoEntry) -> (UndoReport, Option<UndoEntry>) {
        let (keep, skipped) = filter_inverse(&self.view, self.cfg.user, &entry.inverse);
        let mut report = UndoReport {
            done: true,
            skipped,
            ..Default::default()
        };
        if keep.is_empty() {
            return (report, None);
        }
        match self.commit(keep) {
            Ok((op_id, inverse)) => {
                // The host's document does not have this change yet.
                self.changed = true;
                report.op_id = Some(op_id);
                report.applied = self.pending.back().map_or(0, |t| t.ops.len());
                (
                    report,
                    Some(UndoEntry {
                        of_op: op_id,
                        inverse,
                    }),
                )
            }
            // The kept operations changed nothing now (for example, the object was deleted
            // since the check). The step is used up.
            Err(_) => (report, None),
        }
    }

    // ----- presence -----

    /// Sets the presence to show. It goes out at the next [`Client::tick`].
    pub fn set_presence(&mut self, state: PresenceState) -> Result<(), PresenceError> {
        self.out_presence.set(state)
    }

    /// The presence that this client shows.
    pub fn presence(&self) -> &PresenceState {
        self.out_presence.state()
    }

    /// Sends the presence if it is due and forgets users who went quiet. Call it often (every
    /// frame is fine). `now_ms` is any steady clock.
    pub fn tick(&mut self, now_ms: u64) {
        if self.state == ConnState::Online
            && let Some(state) = self.out_presence.poll(now_ms)
        {
            self.outbox.push(ClientMsg::Presence { state });
        }
        self.peers.expire(now_ms);
    }

    // ----- messages from the server -----

    /// Handles one message from the server. `now_ms` stamps presence news.
    pub fn handle(&mut self, msg: ServerMsg, now_ms: u64) {
        match msg {
            ServerMsg::Welcome {
                protocol,
                head_seq,
                your_last_op,
                peers,
            } => self.on_welcome(protocol, head_seq, your_last_op, peers, now_ms),
            ServerMsg::Snapshot { seq, state } => {
                if self.state == ConnState::Syncing {
                    self.confirmed = state;
                    self.confirmed_seq = seq;
                    self.has_state = true;
                    self.finish_sync();
                }
            }
            ServerMsg::Entries { entries } => {
                if self.state == ConnState::Syncing && self.apply_entries(&entries) {
                    self.finish_sync();
                }
            }
            ServerMsg::Ack { op_id, seq } => {
                if self.state == ConnState::Online {
                    self.on_ack(op_id, seq);
                }
            }
            ServerMsg::Broadcast { entry } => {
                if self.state == ConnState::Online {
                    self.apply_entries(std::slice::from_ref(&entry));
                }
            }
            ServerMsg::Presence { user, name, state } => {
                if user != self.cfg.user && state.check().is_ok() {
                    let name = presence::clean_name(&name);
                    self.peers.update(Peer { user, name, state }, now_ms);
                }
            }
            ServerMsg::PresenceLeave { user } => {
                self.peers.remove(user);
            }
            ServerMsg::Error {
                code,
                message,
                op_id,
            } => self.on_error(code, message, op_id),
        }
    }

    fn on_welcome(
        &mut self,
        protocol: u32,
        head_seq: u64,
        your_last_op: u64,
        peers: Vec<Peer>,
        now_ms: u64,
    ) {
        if self.state != ConnState::Connecting {
            return;
        }
        if protocol != PROTOCOL_VERSION {
            self.fail(
                ErrorCode::UnsupportedProtocol,
                format!("the server speaks protocol {protocol}, this client {PROTOCOL_VERSION}"),
            );
            return;
        }
        self.head_seq = head_seq;
        self.pending.retain(|t| t.op_id > your_last_op);
        self.peers.clear();
        for p in peers {
            if p.user != self.cfg.user && p.state.check().is_ok() {
                self.peers.update(
                    Peer {
                        name: presence::clean_name(&p.name),
                        ..p
                    },
                    now_ms,
                );
            }
        }
        self.state = ConnState::Syncing;
    }

    /// Applies entries to `confirmed` in order. Returns false if there was a gap (the client
    /// went offline then).
    fn apply_entries(&mut self, entries: &[Entry]) -> bool {
        let mut rebuild = false;
        for e in entries {
            if e.seq <= self.confirmed_seq {
                continue;
            }
            if e.seq != self.confirmed_seq + 1 {
                self.gap(self.confirmed_seq + 1, e.seq);
                return false;
            }
            let w = Writer {
                user: e.author,
                op: e.op_id,
            };
            self.confirmed.apply_all(w, &e.ops);
            self.confirmed_seq = e.seq;
            self.head_seq = self.head_seq.max(e.seq);
            // While syncing, `finish_sync` builds the view. Only live entries touch it here.
            let live = self.state == ConnState::Online;
            if e.author == self.cfg.user {
                self.pending.retain(|t| t.op_id != e.op_id);
                rebuild |= live;
            } else if live && self.pending.is_empty() {
                self.view.apply_all(w, &e.ops);
                self.changed = true;
            } else {
                rebuild |= live;
            }
        }
        if rebuild {
            self.rebuild_view();
        }
        self.save_journal();
        true
    }

    fn on_ack(&mut self, op_id: u64, seq: u64) {
        let Some(i) = self.pending.iter().position(|t| t.op_id == op_id) else {
            return;
        };
        if seq <= self.confirmed_seq {
            // The entry came before in another way. Only the queue needs the update.
            self.pending.remove(i);
            self.save_journal();
            return;
        }
        if seq != self.confirmed_seq + 1 {
            self.gap(self.confirmed_seq + 1, seq);
            return;
        }
        let txn = self.pending.remove(i).expect("position found above");
        let w = Writer {
            user: self.cfg.user,
            op: op_id,
        };
        self.confirmed.apply_all(w, &txn.ops);
        self.confirmed_seq = seq;
        self.head_seq = self.head_seq.max(seq);
        if i != 0 {
            // The server ordered our transactions differently than we made them.
            self.rebuild_view();
        }
        self.save_journal();
    }

    fn on_error(&mut self, code: ErrorCode, message: String, op_id: Option<u64>) {
        if let Some(op_id) = op_id {
            if self.pending.iter().any(|t| t.op_id == op_id) {
                self.pending.retain(|t| t.op_id != op_id);
                self.undo.retain(|u| u.of_op != op_id);
                self.redo.retain(|u| u.of_op != op_id);
                self.rebuild_view();
                self.save_journal();
            }
            self.events.push(ClientEvent::Rejected {
                op_id,
                code,
                message,
            });
        } else if code.is_fatal() {
            self.fail(code, message);
        } else {
            self.events.push(ClientEvent::ServerError { code, message });
        }
    }

    fn fail(&mut self, code: ErrorCode, message: String) {
        self.state = ConnState::Failed;
        self.outbox.clear();
        self.peers.clear();
        self.events.push(ClientEvent::Failed { code, message });
    }

    fn gap(&mut self, expected: u64, got: u64) {
        self.events.push(ClientEvent::GapDetected { expected, got });
        self.connection_lost();
    }

    /// The end of the sync: replay the queue on the new base, send it, and go online.
    fn finish_sync(&mut self) {
        let mut scratch = self.confirmed.clone();
        let user = self.cfg.user;
        for t in &mut self.pending {
            let w = Writer { user, op: t.op_id };
            t.ops.retain(|op| scratch.apply(w, op).is_applied());
        }
        self.pending.retain(|t| !t.ops.is_empty());
        self.view = scratch;
        self.changed = true;
        self.state = ConnState::Online;
        for t in &self.pending {
            self.outbox.push(ClientMsg::Submit {
                op_id: t.op_id,
                ops: t.ops.clone(),
            });
        }
        self.out_presence.resend();
        self.events.push(ClientEvent::Connected);
        self.save_checkpoint();
        self.save_journal();
    }

    /// `view = confirmed + pending`.
    fn rebuild_view(&mut self) {
        let mut v = self.confirmed.clone();
        for t in &self.pending {
            let w = Writer {
                user: self.cfg.user,
                op: t.op_id,
            };
            v.apply_all(w, &t.ops);
        }
        self.view = v;
        self.changed = true;
    }

    // ----- store -----

    /// The journal as it would be saved.
    pub fn journal(&self) -> Journal {
        Journal {
            user: self.cfg.user,
            last_op: self.last_op,
            pending: self.pending.iter().cloned().collect(),
            undo: self.undo.clone(),
            redo: self.redo.clone(),
        }
    }

    /// The checkpoint as it would be saved, if the client has a confirmed state.
    pub fn checkpoint(&self) -> Option<Checkpoint> {
        self.has_state.then(|| Checkpoint {
            seq: self.confirmed_seq,
            state: self.confirmed.clone(),
        })
    }

    fn save_journal(&mut self) {
        if self.store.is_none() {
            return;
        }
        let text = serde_json::to_string(&self.journal()).expect("a journal is JSON");
        self.save(Slot::Journal, &text);
    }

    /// Saves the confirmed state now.
    pub fn save_checkpoint(&mut self) {
        if self.store.is_none() {
            return;
        }
        if let Some(cp) = self.checkpoint() {
            let text = serde_json::to_string(&cp).expect("a checkpoint is JSON");
            self.save(Slot::Checkpoint, &text);
        }
    }

    fn save(&mut self, slot: Slot, text: &str) {
        if let Some(store) = &mut self.store
            && let Err(e) = store.save(slot, text)
        {
            self.events.push(ClientEvent::StoreFailed(e));
        }
    }
}

/// Keeps a stack at its limit: the oldest steps go.
fn trim(stack: &mut Vec<UndoEntry>) {
    if stack.len() > MAX_STEPS {
        stack.drain(..stack.len() - MAX_STEPS);
    }
}
