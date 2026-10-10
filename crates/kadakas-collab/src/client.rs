//! The client state machine.
//!
//! [`Client`] holds no connection. It takes messages in ([`Client::handle`]) and gives messages
//! out ([`Client::take_outbox`]). A host (see `transport::Link`) moves the messages. This keeps
//! the client pure, easy to test and usable in the browser.
//!
//! # Identity
//!
//! A **user** is a person. A **client** is one running instance of the program, with its own UUID
//! (`ClientConfig::client`). The client ID is the *writer* of the transactions: the pair (client,
//! `op_id`) names a transaction. A second device, a second session, or a program that lost its
//! journal gets a new client ID, so its transaction numbers never meet the numbers of another
//! instance. The user is metadata: the server binds it to the token and puts it in the entries.
//! A client that restores its journal keeps the client ID of the journal.
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
//! When a transaction of another client arrives, it is ordered by the server *before* all of our
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
//! # The document and the shadow
//!
//! The host has a `Document`. The client does not see it change; the host tells the client about
//! local changes with [`Client::edit_from_change`] or [`Client::edit_from_diff`]. These compare
//! the document with the **shadow**: a replica that equals the last document that the host
//! built from the view (or the last local state that was already turned into edits). They do not
//! compare with the view, because the view can already hold remote changes that the document does
//! not show yet, and a diff against the view would undo them. After the host rebuilds its document
//! from the view, it calls [`Client::set_shadow`].
//!
//! # Connection states
//!
//! `Offline` (or `Failed` after a fatal error) -> `connect()` sends `hello` ->
//! `Connecting` -> `welcome` -> `Syncing` -> `entries` or `snapshot` -> `Online`.
//! A lost connection goes back to `Offline`. Edits work in every state.
//!
//! # Errors of the server
//!
//! Each error code has a class (see `protocol::ErrorClass`). A *retry* error (`rate_limited`,
//! `internal`) keeps the transaction and sends it, and the ones after it, again after a time that
//! doubles each time (250 ms, up to 30 s). A *reject* error drops the transaction and its undo
//! step. A *renumber* error (`op_id_reused`) takes the client offline with a `Reconnect` event; at
//! the next sign-in the waiting transactions get new numbers above everything the server knows.
//! A *fatal* error ends the client until the host connects again.
//!
//! # A server that goes back
//!
//! `welcome` carries the `log_id` of the log. If it differs from the one in the client's state,
//! or `head_seq` is below the client's sequence number, the server lost entries or is another
//! log (a restore from a backup). The client then waits for the `snapshot`, throws away its queue
//! and undo steps, and compares the snapshot with the state it had. What it had and the snapshot
//! lacks (acknowledged work that the server lost) goes out again as new transactions.
//!
//! # Offline work
//!
//! Edits made offline stay in the pending queue. With an [`OpLogStore`] the queue, the undo
//! stacks, the client ID and the transaction counter are saved when they change, so a restart
//! loses nothing (see `store`). A journal that cannot be read is reported as an event, never
//! ignored.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use kadakas_core::change::ChangeSet;
use kadakas_core::model::Document;
use kadakas_core::uid::Uid;
use serde::{Deserialize, Serialize};

use serde_json::Value;

use crate::blob;
use crate::docsync;
use crate::op::{InvalidTxn, Op, Txn, Writer, validate_ops};
use crate::presence::{self, Outgoing, Peer, Peers, PresenceError, PresenceState};
use crate::protocol::{ClientMsg, Entry, ErrorClass, ErrorCode, PROTOCOL_VERSION, ServerMsg};
use crate::replica::Replica;
use crate::store::{Checkpoint, Journal, MAX_JOURNAL_BYTES, OpLogStore, Slot};
use crate::undo::{MAX_STEPS, UndoEntry, UndoReport, filter_inverse};

/// The first wait before a transaction is sent again after a retry error, in milliseconds.
pub const RETRY_BASE_MS: u64 = 250;
/// The longest wait.
pub const RETRY_MAX_MS: u64 = 30_000;

/// Who the client is and how it signs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientConfig {
    /// The person.
    pub user: Uid,
    /// This instance of the program: a fresh UUID. A client that restores a journal uses the
    /// client ID of the journal instead.
    pub client: Uid,
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
    /// The server refused the client for good (wrong token or protocol version), or sent
    /// something that cannot be read.
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
    /// The journal or the checkpoint in the store could not be read, or belongs to another
    /// user. The client starts without it. The text says why.
    StoreCorrupt(String),
    /// The server went back (another `log_id`, or a lower sequence number). The client took the
    /// snapshot and sent `resent` transactions with work that the server had lost.
    ServerRegressed { resent: usize },
    /// The pending transactions got new numbers (the server knew a higher number for this client).
    Renumbered { count: usize },
    /// The client went offline on purpose and the host should connect again (for example after
    /// `op_id_reused`: the numbers are fixed at the next sign-in).
    Reconnect(String),
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

/// The result of turning a document change into transactions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocEdit {
    /// The numbers of the transactions that were made (a big change is split into several).
    pub op_ids: Vec<u64>,
    /// What could not be shared, as texts. The local document has these changes anyway.
    pub problems: Vec<String>,
}

/// A transaction that waits to be sent again.
#[derive(Clone, Copy, Debug)]
struct Retry {
    /// The first transaction that was refused. It and all later ones are sent again.
    from_op: u64,
    /// When to send, or `None` if they were sent and an answer is awaited.
    at: Option<u64>,
    attempts: u32,
}

/// The state machine of one collaboration client.
pub struct Client {
    cfg: ClientConfig,
    /// The writer ID of this instance (`cfg.client`, or the one of the restored journal).
    client: Uid,
    state: ConnState,
    confirmed: Replica,
    confirmed_seq: u64,
    log_id: Option<String>,
    /// True once the client has a confirmed state (from a snapshot, a restore or `with_state`).
    has_state: bool,
    /// The server is not the one that the state came from: only a snapshot is acceptable.
    regress: bool,
    head_seq: u64,
    view: Replica,
    shadow: Option<Replica>,
    pending: VecDeque<Txn>,
    last_op: u64,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    outbox: Vec<ClientMsg>,
    events: Vec<ClientEvent>,
    out_presence: Outgoing,
    peers: Peers,
    changed: bool,
    retry: Option<Retry>,
    /// The blobs (pictures) that this client has, by hash.
    blobs: BTreeMap<String, String>,
    /// The blobs that were sent on this connection. A blob is sent again on a new connection.
    put_on_conn: BTreeSet<String>,
    /// Blobs that were asked for, and blobs that the server does not have.
    asked: BTreeSet<String>,
    missing: BTreeSet<String>,
    /// The view changed: look for references to blobs that are not here.
    blob_scan: bool,
    /// At the next sign-in, give the waiting transactions new numbers, whatever the server says.
    force_renumber: bool,
    store: Option<Box<dyn OpLogStore>>,
    saved_journal: String,
}

impl Client {
    /// A client without state. It gets its state from the first sync.
    pub fn new(cfg: ClientConfig) -> Client {
        Client {
            client: cfg.client,
            cfg,
            state: ConnState::Offline,
            confirmed: Replica::new(),
            confirmed_seq: 0,
            log_id: None,
            has_state: false,
            regress: false,
            head_seq: 0,
            view: Replica::new(),
            shadow: None,
            pending: VecDeque::new(),
            last_op: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            outbox: Vec::new(),
            events: Vec::new(),
            out_presence: Outgoing::default(),
            peers: Peers::default(),
            changed: false,
            retry: None,
            blobs: BTreeMap::new(),
            put_on_conn: BTreeSet::new(),
            asked: BTreeSet::new(),
            missing: BTreeSet::new(),
            blob_scan: false,
            force_renumber: false,
            store: None,
            saved_journal: String::new(),
        }
    }

    /// A client that starts from a known confirmed state, for example a document that the server
    /// holds at `seq` in the log `log_id`.
    pub fn with_state(cfg: ClientConfig, state: Replica, seq: u64, log_id: &str) -> Client {
        let mut c = Client::new(cfg);
        c.view = state.clone();
        c.confirmed = state;
        c.confirmed_seq = seq;
        c.head_seq = seq;
        c.log_id = Some(log_id.to_string());
        c.has_state = true;
        c
    }

    /// A client that continues from a store: the checkpoint gives the confirmed state, the
    /// journal gives the client ID, the pending transactions, the undo stacks and the counter.
    /// The store stays attached.
    ///
    /// A checkpoint or journal that cannot be read, or a journal of another user, is not
    /// silently dropped: the client reports [`ClientEvent::StoreCorrupt`] and starts without it
    /// (with the client ID of `cfg`, so its numbers do not meet the numbers of the lost journal).
    pub fn restore(cfg: ClientConfig, store: Box<dyn OpLogStore>) -> Client {
        let mut c = Client::new(cfg);
        if let Some(text) = store.load(Slot::Checkpoint) {
            match serde_json::from_str::<Checkpoint>(&text) {
                Ok(cp) => {
                    c.confirmed = cp.state;
                    c.confirmed_seq = cp.seq;
                    c.head_seq = cp.seq;
                    c.log_id = Some(cp.log_id).filter(|l| !l.is_empty());
                    c.blobs.extend(cp.blobs);
                    c.has_state = true;
                }
                Err(e) => c
                    .events
                    .push(ClientEvent::StoreCorrupt(format!("the checkpoint: {e}"))),
            }
        }
        if let Some(text) = store.load(Slot::Journal) {
            match serde_json::from_str::<Journal>(&text) {
                Ok(j) if j.user == c.cfg.user => {
                    c.client = j.client;
                    c.last_op = j.last_op;
                    c.pending = j.pending.into();
                    c.undo = j.undo;
                    c.redo = j.redo;
                    c.blobs.extend(j.blobs);
                    c.saved_journal = text;
                }
                Ok(_) => c.events.push(ClientEvent::StoreCorrupt(
                    "the journal belongs to another user".into(),
                )),
                Err(e) => c
                    .events
                    .push(ClientEvent::StoreCorrupt(format!("the journal: {e}"))),
            }
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

    /// The writer ID of this instance.
    pub fn client_id(&self) -> Uid {
        self.client
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

    /// The name of the log that the state comes from.
    pub fn log_id(&self) -> Option<&str> {
        self.log_id.as_deref()
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

    /// True once the host has told the client what its document shows (see [`Client::set_shadow`]).
    pub fn has_shadow(&self) -> bool {
        self.shadow.is_some()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// True if the view changed since the last call by something other than an edit that the
    /// host made itself: a change of another user, a rebase, or an undo or redo. The host builds
    /// the document again then (see `materialize::materialize`). Edits made with
    /// [`Client::edit`] do not set it, because the host's document has them already.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Sets the flag again. A host calls it when it could not use the change yet.
    pub fn keep_changed(&mut self) {
        self.changed = true;
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
        self.regress = false;
        self.put_on_conn.clear();
        self.outbox.clear();
        self.outbox.push(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            user: self.cfg.user,
            client: self.client,
            name: self.cfg.name.clone(),
            token: self.cfg.token.clone(),
            last_seq: self.has_state.then_some(self.confirmed_seq),
            log_id: self.log_id.clone(),
        });
    }

    /// The connection ended (the host noticed). Edits go on in the queue.
    pub fn connection_lost(&mut self) {
        self.outbox.clear();
        self.go_offline();
    }

    /// The server sent a frame that cannot be read (not JSON, too deep, not a message). Going
    /// back to the same frame by reconnecting would loop, so the client stops: it is `Failed`
    /// until the host connects again on purpose.
    pub fn bad_frame(&mut self, why: &str) {
        self.fail(
            ErrorCode::BadMessage,
            format!("the server sent a message that cannot be read: {why}"),
        );
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
        self.retry = None;
        self.put_on_conn.clear();
        self.save_checkpoint();
    }

    // ----- edits -----

    /// Applies a local edit: the view changes at once, the transaction goes into the queue and,
    /// when online, to the server. Operations without effect are dropped from the transaction.
    /// Returns the transaction number.
    pub fn edit(&mut self, ops: Vec<Op>) -> Result<u64, EditError> {
        let (op_id, inverse) = self.commit(ops, false)?;
        self.undo.push(UndoEntry {
            of_op: op_id,
            inverse,
        });
        trim(&mut self.undo);
        self.redo.clear();
        self.save_journal();
        Ok(op_id)
    }

    /// Tells the client what the host's document shows. Call it after the host built its document
    /// from the view, and when the client starts with a document that equals the state.
    pub fn set_shadow(&mut self, doc: &Document) {
        self.shadow = Some(docsync::replica_of(doc));
    }

    /// Turns a change set of the local document into edits (see `docsync::ops_from_change`). Big
    /// changes become several transactions.
    pub fn edit_from_change(&mut self, doc: &Document, cs: &ChangeSet) -> DocEdit {
        match &self.shadow {
            Some(shadow) => {
                let ops = docsync::ops_from_change(shadow, doc, cs);
                self.edit_doc_ops(ops)
            }
            None => no_shadow(),
        }
    }

    /// Like [`Client::edit_from_change`], comparing the whole document with the shadow.
    pub fn edit_from_diff(&mut self, doc: &Document) -> DocEdit {
        match &self.shadow {
            Some(shadow) => {
                let ops = docsync::ops_from_diff(shadow, doc);
                self.edit_doc_ops(ops)
            }
            None => no_shadow(),
        }
    }

    fn edit_doc_ops(&mut self, ops: Vec<Op>) -> DocEdit {
        let mut out = DocEdit::default();
        if ops.is_empty() {
            return out;
        }
        if let Some(shadow) = &mut self.shadow {
            shadow.apply_all(Writer::BASELINE, &ops);
        }
        for chunk in docsync::split(ops) {
            match self.edit(chunk) {
                Ok(id) => out.op_ids.push(id),
                Err(EditError::NoEffect) => {}
                Err(e) => out.problems.push(e.to_string()),
            }
        }
        out
    }

    /// Applies the operations to the view, queues the transaction, and returns its number and
    /// the inverse operations (in the order to apply them).
    fn commit(&mut self, ops: Vec<Op>, force_put: bool) -> Result<(u64, Vec<Op>), EditError> {
        validate_ops(&ops)?;
        let ops = self.externalize(ops);
        let op_id = self.last_op + 1;
        let w = Writer {
            client: self.client,
            op: op_id,
        };
        let total = ops.len();
        let (applied, inverse) = apply_with_inverse(&mut self.view, w, ops);
        if applied.len() < total {
            // The merge rules dropped some operations (a delete of a thing that cannot go, an
            // edit of a deleted thing). The host's document has them, so it must be built again
            // from the view.
            self.changed = true;
        }
        if applied.is_empty() {
            return Err(EditError::NoEffect);
        }
        self.last_op = op_id;
        if self.state == ConnState::Online {
            self.put_blobs(&applied, force_put);
            self.outbox.push(ClientMsg::Submit {
                op_id,
                ops: applied.clone(),
            });
        }
        self.pending.push_back(Txn {
            op_id,
            ops: applied,
        });
        Ok((op_id, inverse))
    }

    /// Replaces big pictures in the operations by references (see `blob`) and keeps the pictures.
    /// They go to the server with `put_blobs`, before the transaction that refers to them.
    fn externalize(&mut self, mut ops: Vec<Op>) -> Vec<Op> {
        let put = |this: &mut Client, v: &mut Value| {
            if let Some((r, hash, data)) = blob::externalize(v) {
                *v = r;
                this.blobs.insert(hash, data);
            }
        };
        for op in &mut ops {
            match op {
                Op::Create { props, .. } => {
                    if let Some(v) = props.get_mut("shape") {
                        put(self, v);
                    }
                }
                Op::Set { prop, value, .. } if prop == "shape" => put(self, value),
                _ => {}
            }
        }
        ops
    }

    /// Queues a `blob_put` for every picture that these operations refer to and that this
    /// connection has not sent yet. It covers new edits, undo and redo, a connection that starts
    /// again, a server that went back, and a put that failed.
    /// With `force`, also the ones that this connection sent before: the server may have dropped
    /// a picture that nothing referred to (an undo refers to it again).
    fn put_blobs(&mut self, ops: &[Op], force: bool) {
        for op in ops {
            for h in op_blob_refs(op) {
                if let Some(data) = self.blobs.get(&h)
                    && (self.put_on_conn.insert(h.clone()) || force)
                {
                    self.outbox.push(ClientMsg::BlobPut {
                        hash: h,
                        data: data.clone(),
                    });
                }
            }
        }
    }

    /// The document that the view describes, with the pictures it refers to (see
    /// `materialize::materialize_with`).
    pub fn materialize(&self, base: &Document) -> crate::materialize::Materialized {
        crate::materialize::materialize_with(&self.view, base, &|h| self.blobs.get(h).cloned())
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

    /// Does again what the last undo took back. Edits of other users do not clear the redo
    /// steps; a new edit of your own does.
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
        let (keep, skipped) = filter_inverse(&self.view, self.client, &entry.inverse);
        let mut report = UndoReport {
            done: true,
            skipped,
            ..Default::default()
        };
        if keep.is_empty() {
            return (report, None);
        }
        match self.commit(keep, true) {
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

    /// Sends the presence if it is due, sends refused transactions again when their time has
    /// come, and forgets clients that went quiet. Call it often (every frame is fine). `now_ms`
    /// is any steady clock.
    pub fn tick(&mut self, now_ms: u64) {
        if self.state == ConnState::Online {
            if let Some(state) = self.out_presence.poll(now_ms) {
                self.outbox.push(ClientMsg::Presence { state });
            }
            if self.blob_scan {
                self.blob_scan = false;
                self.ask_for_blobs();
            }
            if let Some(r) = self.retry
                && r.at.is_some_and(|at| now_ms >= at)
            {
                self.retry = Some(Retry { at: None, ..r });
                self.resend_from(r.from_op);
            }
        }
        self.peers.expire(now_ms);
    }

    /// Asks the server for the blobs that the state refers to and that this client does not have.
    fn ask_for_blobs(&mut self) {
        let mut wanted: Vec<String> = Vec::new();
        for (_, e) in self.view.alive_of(crate::op::Kind::Object) {
            if let Some(h) = e.get("shape").and_then(blob::blob_ref)
                && !self.blobs.contains_key(h)
                && !self.asked.contains(h)
                && !self.missing.contains(h)
            {
                wanted.push(h.to_string());
            }
        }
        for h in wanted {
            self.asked.insert(h.clone());
            self.outbox.push(ClientMsg::BlobGet { hash: h });
        }
    }

    fn resend_from(&mut self, from_op: u64) {
        let txns: Vec<Txn> = self
            .pending
            .iter()
            .filter(|t| t.op_id >= from_op)
            .cloned()
            .collect();
        for t in txns {
            self.put_blobs(&t.ops, false);
            self.outbox.push(ClientMsg::Submit {
                op_id: t.op_id,
                ops: t.ops,
            });
        }
    }

    // ----- messages from the server -----

    /// Handles one message from the server. `now_ms` stamps presence news and sets the time of a
    /// new try after a retry error.
    pub fn handle(&mut self, msg: ServerMsg, now_ms: u64) {
        match msg {
            ServerMsg::Welcome {
                protocol,
                log_id,
                head_seq,
                your_last_op,
                peers,
            } => self.on_welcome(protocol, log_id, head_seq, your_last_op, peers, now_ms),
            ServerMsg::Snapshot { log_id, seq, state } => {
                if self.state == ConnState::Syncing {
                    self.on_snapshot(log_id, seq, state);
                }
            }
            ServerMsg::Entries { entries } => {
                if self.state != ConnState::Syncing {
                    return;
                }
                if !self.has_state || self.regress {
                    self.fail(
                        ErrorCode::BadMessage,
                        "the server sent entries to a client that needs a snapshot".into(),
                    );
                } else if self.apply_entries(&entries) {
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
            ServerMsg::Presence {
                client,
                user,
                name,
                state,
            } => {
                if client != self.client && state.check().is_ok() {
                    let name = presence::clean_name(&name);
                    self.peers.update(
                        Peer {
                            client,
                            user,
                            name,
                            state,
                        },
                        now_ms,
                    );
                }
            }
            ServerMsg::PresenceLeave { client } => {
                self.peers.remove(client);
            }
            ServerMsg::Blob { hash, data } => {
                if blob::sha256_hex(data.as_bytes()) == hash && data.len() <= blob::MAX_BLOB_BYTES {
                    self.asked.remove(&hash);
                    self.blobs.insert(hash, data);
                    self.changed = true;
                }
            }
            ServerMsg::BlobMissing { hash } => {
                self.missing.insert(hash);
            }
            ServerMsg::Error {
                code,
                message,
                op_id,
            } => self.on_error(code, message, op_id, now_ms),
        }
    }

    fn on_welcome(
        &mut self,
        protocol: u32,
        log_id: String,
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
        if self.has_state
            && (self.log_id.as_ref().is_some_and(|l| *l != log_id) || head_seq < self.confirmed_seq)
        {
            self.regress = true;
        }
        self.head_seq = head_seq;
        if your_last_op > self.last_op || self.force_renumber {
            // The journal is behind what the server knows of this client: numbers up to
            // `your_last_op` are used, maybe by other content. Our waiting transactions get
            // numbers above, so the server cannot take them for old ones.
            self.force_renumber = false;
            self.last_op = self.last_op.max(your_last_op);
            let count = self.renumber_from(0);
            if count > 0 {
                self.events.push(ClientEvent::Renumbered { count });
            }
        } else {
            self.pending.retain(|t| t.op_id > your_last_op);
        }
        self.peers.clear();
        for p in peers {
            if p.client != self.client && p.state.check().is_ok() {
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

    fn on_snapshot(&mut self, log_id: String, seq: u64, state: Replica) {
        let regress =
            self.has_state && (self.regress || self.log_id.as_ref().is_some_and(|l| *l != log_id));
        let old_view = std::mem::take(&mut self.view);
        self.confirmed = state;
        self.confirmed_seq = seq;
        self.log_id = Some(log_id);
        self.has_state = true;
        if regress {
            // The server lost entries, or this is another log. What we had and the snapshot
            // lacks goes out again. The old queue and undo steps do not fit the new state.
            self.pending.clear();
            self.undo.clear();
            self.redo.clear();
            let lost = docsync::diff_replicas(&self.confirmed, &old_view);
            self.finish_sync();
            let mut resent = 0;
            for chunk in docsync::split(lost) {
                if self.commit(chunk, false).is_ok() {
                    resent += 1;
                }
            }
            self.regress = false;
            self.changed = true;
            self.events.push(ClientEvent::ServerRegressed { resent });
            self.save_journal();
        } else {
            self.finish_sync();
        }
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
                client: e.author,
                op: e.op_id,
            };
            if e.author == self.client {
                self.confirm_own(w, e.ops.clone());
            } else {
                self.confirmed.apply_all(w, &e.ops);
            }
            self.confirmed_seq = e.seq;
            self.head_seq = self.head_seq.max(e.seq);
            // While syncing, `finish_sync` builds the view. Only live entries touch it here.
            let live = self.state == ConnState::Online;
            if e.author == self.client {
                self.pending.retain(|t| t.op_id != e.op_id);
                rebuild |= live;
            } else if live && self.pending.is_empty() {
                self.view.apply_all(w, &e.ops);
                self.changed = true;
                self.blob_scan = true;
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
        if self.retry.is_some_and(|r| r.at.is_none()) {
            self.retry = None;
        }
        if seq <= self.confirmed_seq {
            // The entry came before in another way (the server sent it as an entry). The
            // transaction is in `confirmed` already, so the view is built again without it.
            self.pending.remove(i);
            self.rebuild_view();
            self.save_journal();
            return;
        }
        if seq != self.confirmed_seq + 1 {
            self.gap(self.confirmed_seq + 1, seq);
            return;
        }
        let txn = self.pending.remove(i).expect("position found above");
        let w = Writer {
            client: self.client,
            op: op_id,
        };
        self.confirm_own(w, txn.ops);
        self.confirmed_seq = seq;
        self.head_seq = self.head_seq.max(seq);
        if i != 0 {
            // The server ordered our transactions differently than we made them.
            self.rebuild_view();
        }
        self.save_journal();
    }

    /// Puts a transaction of this client into `confirmed`, and makes its undo step again from the
    /// state that was *really* underneath it in the server order. The inverse that was made when
    /// the edit was shown to the user came from the view, which may not have had a write of another
    /// user that the server ordered before this transaction. Undo then restores the value that
    /// was underneath, not a stale one.
    fn confirm_own(&mut self, w: Writer, ops: Vec<Op>) {
        let (_, inverse) = apply_with_inverse(&mut self.confirmed, w, ops);
        for u in self
            .undo
            .iter_mut()
            .chain(self.redo.iter_mut())
            .filter(|u| u.of_op == w.op)
        {
            u.inverse = inverse.clone();
        }
    }

    fn on_error(&mut self, code: ErrorCode, message: String, op_id: Option<u64>, now_ms: u64) {
        let class = code.class();
        match (op_id, class) {
            (_, ErrorClass::Fatal) => self.fail(code, message),
            (Some(op_id), ErrorClass::Retry) => {
                // An error for a later transaction of the same round does not make the wait longer.
                if self
                    .retry
                    .is_some_and(|r| r.at.is_some() && op_id >= r.from_op)
                {
                    return;
                }
                if self.pending.iter().any(|t| t.op_id == op_id) {
                    let prev = self.retry;
                    let attempts = prev.map_or(0, |r| r.attempts);
                    let wait = RETRY_BASE_MS
                        .saturating_mul(1u64 << attempts.min(20))
                        .min(RETRY_MAX_MS);
                    self.retry = Some(Retry {
                        from_op: prev.map_or(op_id, |r| r.from_op.min(op_id)),
                        at: Some(now_ms + wait),
                        attempts: attempts + 1,
                    });
                }
            }
            (Some(_), ErrorClass::Renumber) => {
                // The server knows this number with other content. Later transactions may have
                // been taken or not, so the client stops here and fixes the numbers at the next
                // sign-in, when the server tells the highest number it has.
                self.force_renumber = true;
                self.events.push(ClientEvent::Reconnect(message));
                self.connection_lost();
            }
            (Some(op_id), ErrorClass::Reject) => {
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
            }
            (None, _) => {
                // The error may be about a blob_put that failed: send the pictures again.
                self.put_on_conn.clear();
                self.events.push(ClientEvent::ServerError { code, message });
            }
        }
    }

    /// Gives the waiting transactions from `from_op` on new numbers above `last_op`, in order.
    /// Returns how many. The view is built again, because the writer of their slots changes.
    fn renumber_from(&mut self, from_op: u64) -> usize {
        let mut count = 0;
        let mut map: Vec<(u64, u64)> = Vec::new();
        for t in self.pending.iter_mut().filter(|t| t.op_id >= from_op) {
            self.last_op += 1;
            map.push((t.op_id, self.last_op));
            t.op_id = self.last_op;
            count += 1;
        }
        for u in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            if let Some((_, new)) = map.iter().find(|(old, _)| *old == u.of_op) {
                u.of_op = *new;
            }
        }
        if count > 0 {
            self.rebuild_view();
            self.save_journal();
        }
        count
    }

    fn fail(&mut self, code: ErrorCode, message: String) {
        self.state = ConnState::Failed;
        self.outbox.clear();
        self.peers.clear();
        self.retry = None;
        self.events.push(ClientEvent::Failed { code, message });
    }

    fn gap(&mut self, expected: u64, got: u64) {
        self.events.push(ClientEvent::GapDetected { expected, got });
        self.connection_lost();
    }

    /// The end of the sync: replay the queue on the new base, send it, and go online.
    fn finish_sync(&mut self) {
        let mut scratch = self.confirmed.clone();
        let client = self.client;
        for t in &mut self.pending {
            let w = Writer {
                client,
                op: t.op_id,
            };
            t.ops.retain(|op| scratch.apply(w, op).is_applied());
        }
        self.pending.retain(|t| !t.ops.is_empty());
        self.view = scratch;
        self.changed = true;
        self.blob_scan = true;
        self.asked.clear();
        self.state = ConnState::Online;
        // The pictures that waiting transactions refer to go first. The server may be another
        // one (a restore), so the pictures that were asked for or missing are tried again too.
        self.put_on_conn.clear();
        self.missing.clear();
        let txns: Vec<Txn> = self.pending.iter().cloned().collect();
        for t in txns {
            self.put_blobs(&t.ops, false);
            self.outbox.push(ClientMsg::Submit {
                op_id: t.op_id,
                ops: t.ops,
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
                client: self.client,
                op: t.op_id,
            };
            v.apply_all(w, &t.ops);
        }
        self.view = v;
        self.changed = true;
        self.blob_scan = true;
    }

    // ----- store -----

    /// The journal as it would be saved.
    pub fn journal(&self) -> Journal {
        Journal {
            user: self.cfg.user,
            client: self.client,
            last_op: self.last_op,
            pending: self.pending.iter().cloned().collect(),
            undo: self.undo.clone(),
            redo: self.redo.clone(),
            blobs: self.journal_blobs(),
        }
    }

    /// The blobs that the waiting transactions and the undo and redo steps refer to.
    fn journal_blobs(&self) -> BTreeMap<String, String> {
        let mut want = BTreeSet::new();
        for t in &self.pending {
            want.extend(t.ops.iter().flat_map(op_blob_refs));
        }
        for u in self.undo.iter().chain(&self.redo) {
            want.extend(u.inverse.iter().flat_map(op_blob_refs));
        }
        self.blobs
            .iter()
            .filter(|(h, _)| want.contains(*h))
            .map(|(h, d)| (h.clone(), d.clone()))
            .collect()
    }

    /// The blobs that the checkpoint keeps: the ones that the confirmed state refers to (deleted
    /// things too, because an undo can bring them back), and the ones of the journal.
    fn checkpoint_blobs(&self) -> BTreeMap<String, String> {
        let mut want: BTreeSet<String> = self.journal_blobs().into_keys().collect();
        for (_, e) in self.confirmed.things() {
            want.extend(
                e.props
                    .values()
                    .filter(|s| !s.removed)
                    .filter_map(|s| blob::blob_ref(&s.value).map(str::to_string)),
            );
        }
        self.blobs
            .iter()
            .filter(|(h, _)| want.contains(*h))
            .map(|(h, d)| (h.clone(), d.clone()))
            .collect()
    }

    /// The checkpoint as it would be saved, if the client has a confirmed state.
    pub fn checkpoint(&self) -> Option<Checkpoint> {
        self.has_state.then(|| Checkpoint {
            log_id: self.log_id.clone().unwrap_or_default(),
            seq: self.confirmed_seq,
            state: self.confirmed.clone(),
            blobs: self.checkpoint_blobs(),
        })
    }

    /// Saves the journal when it changed. It is bounded: when it is too large, the oldest undo
    /// and redo steps go first. If the waiting transactions alone are too large, the journal is
    /// not saved and the client reports it.
    fn save_journal(&mut self) {
        if self.store.is_none() {
            return;
        }
        let mut j = self.journal();
        let mut text = serde_json::to_string(&j).expect("a journal is JSON");
        while text.len() > MAX_JOURNAL_BYTES && (!j.undo.is_empty() || !j.redo.is_empty()) {
            let n = j.undo.len().div_ceil(2);
            j.undo.drain(..n);
            let n = j.redo.len().div_ceil(2);
            j.redo.drain(..n);
            text = serde_json::to_string(&j).expect("a journal is JSON");
        }
        if text.len() > MAX_JOURNAL_BYTES {
            self.events.push(ClientEvent::StoreFailed(
                "the waiting edits are too large for the journal".into(),
            ));
            return;
        }
        if text != self.saved_journal && self.save(Slot::Journal, &text) {
            self.saved_journal = text;
        }
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

    fn save(&mut self, slot: Slot, text: &str) -> bool {
        if let Some(store) = &mut self.store
            && let Err(e) = store.save(slot, text)
        {
            self.events.push(ClientEvent::StoreFailed(e));
            return false;
        }
        true
    }
}

fn no_shadow() -> DocEdit {
    DocEdit {
        op_ids: Vec::new(),
        problems: vec!["the client does not know the document yet (call set_shadow)".into()],
    }
}

/// Keeps a stack at its limit: the oldest steps go.
fn trim(stack: &mut Vec<UndoEntry>) {
    if stack.len() > MAX_STEPS {
        stack.drain(..stack.len() - MAX_STEPS);
    }
}

/// Applies operations to a replica and returns the ones that had an effect, and the inverse
/// operations in the order to apply them. The key of a new thing is part of its creation: undo
/// deletes the thing, and the key goes with it.
fn apply_with_inverse(r: &mut Replica, w: Writer, ops: Vec<Op>) -> (Vec<Op>, Vec<Op>) {
    let mut applied = Vec::new();
    let mut inverse = Vec::new();
    let mut created: Vec<crate::op::Target> = Vec::new();
    for op in ops {
        let (out, inv) = r.apply_inverse(w, &op);
        if out.is_applied() {
            let own_key = op.prop() == Some("order") && created.contains(&op.target());
            if let Op::Create { target, .. } = &op {
                created.push(*target);
            }
            applied.push(op);
            if !own_key {
                inverse.extend(inv);
            }
        }
    }
    inverse.reverse();
    (applied, inverse)
}

/// The blobs that one operation refers to.
fn op_blob_refs(op: &Op) -> Vec<String> {
    match op {
        Op::Create { props, .. } => blob::refs_in(props),
        Op::Set { prop, value, .. } if prop == "shape" => blob::blob_ref(value)
            .map(str::to_string)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}
