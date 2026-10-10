//! A reference sequencer for tests. **This is not a server.**
//!
//! It shows what the server side of the protocol must do, in the smallest form, and it lets the
//! tests of this crate and of the API crate run a full collaboration in memory. It has no
//! network, no accounts, no storage, no permissions and no fairness. It runs in one thread and
//! it is "sans-IO": a caller gives it a message and gets the answers.
//!
//! The real server is in the separate repository `kadakas-server`. It may use this crate for the
//! merge rules ([`crate::replica::Replica`]) or follow `docs/collaboration.md` and write its own.
//!
//! What it does:
//!
//! - Checks `hello` (protocol version, optional token that is bound to a user) and answers
//!   `welcome` and then `entries` or `snapshot`. A client without state, with another `log_id`, or
//!   ahead of the log gets a snapshot.
//! - Gives each valid transaction the next sequence number, applies it to its own replica, sends
//!   `ack` to the author and `broadcast` to the others.
//! - Keeps for each client only the highest `op_id`, its sequence number and a hash of its content.
//!   A repeat with the same content gets the old sequence number. The same `op_id` with other
//!   content, or a lower one, is `op_id_reused`.
//! - Refuses `create` of an ID that exists or existed, malformed transactions, and things that
//!   fail `validate::validate_entity`.
//! - Relays presence to the others, at most once per client per `MIN_INTERVAL_MS`. The latest
//!   update that came too soon is held and sent later ([`Sequencer::tick`]).
//! - Can forget old log entries, so clients that are far behind get a `snapshot`.
//! - Can answer the next submits with an error of your choice (for tests of retry).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use kadakas_core::uid::Uid;

use crate::chain::{self, CHAIN_START};
use crate::op::{Op, Writer, ops_digest, validate_ops};
use crate::presence::{self, Peer, PresenceState, Throttle};
use crate::protocol::{ClientMsg, Entry, ErrorCode, PROTOCOL_VERSION, ServerMsg};
use crate::replica::Replica;
use crate::transport::{Endpoint, MemPeer, MemTransport, Transport, TransportError, mem_pair};
use crate::validate::{CheckError, check_duplicates, check_transaction};

/// A connection number, made by [`Sequencer::open`].
pub type ConnId = u64;

/// A message for a connection.
#[derive(Clone, Debug, PartialEq)]
pub struct Out {
    pub conn: ConnId,
    pub msg: ServerMsg,
}

struct Conn {
    /// Set after a good `hello`: (client, user).
    who: Option<(Uid, Uid)>,
    name: String,
    presence: PresenceState,
    throttle: Throttle,
    /// The latest presence that came too soon.
    held: Option<PresenceState>,
}

/// What the server keeps for one client: not a set of all transactions, only the last one.
#[derive(Clone, Copy)]
struct WriterRecord {
    high: u64,
    seq: u64,
    hash: [u8; 32],
}

/// The reference sequencer.
pub struct Sequencer {
    replica: Replica,
    log_id: String,
    seq: u64,
    log: Vec<Entry>,
    /// The sequence number before the first entry of `log`.
    log_start: u64,
    log_limit: usize,
    writers: BTreeMap<Uid, WriterRecord>,
    conns: BTreeMap<ConnId, Conn>,
    next_conn: ConnId,
    /// token -> the user it belongs to (`None`: any user).
    tokens: BTreeMap<String, Option<Uid>>,
    require_token: bool,
    inject: Option<(ErrorCode, usize)>,
    check_entities: bool,
    /// Per connection: after a refusal that the client must retry, every later submit gets the
    /// same error, until the refused number comes again.
    blocked: BTreeMap<ConnId, (ErrorCode, u64)>,
    /// Blobs by hash.
    blobs: BTreeMap<String, String>,
    /// The chain hash of each entry of `log`, and the chain hash before the first of them.
    chains: Vec<[u8; 32]>,
    base_chain: [u8; 32],
    /// False: act as a server that does not keep the chain (the old protocol).
    use_chain: bool,
    epoch: u32,
    /// client ID -> the first user who signed in with it.
    owners: BTreeMap<Uid, Uid>,
    /// The next blob_put calls fail with this error (nothing is kept).
    inject_blob: Option<(ErrorCode, usize)>,
    /// Per connection: a blob_put was refused with a retry error. Every later submit gets the
    /// same error, until that blob is put again.
    blob_blocked: BTreeMap<ConnId, (ErrorCode, String)>,
}

impl Sequencer {
    /// A sequencer for a document in this state, at sequence number 0. A new shared document
    /// starts from `docsync::replica_of(&Document::default())`: it has the protected first layer,
    /// the class "None" and the document settings. An empty `Replica::new()` has none of them.
    pub fn new(initial: Replica) -> Sequencer {
        Sequencer {
            replica: initial,
            log_id: "log-1".into(),
            seq: 0,
            log: Vec::new(),
            log_start: 0,
            log_limit: usize::MAX,
            writers: BTreeMap::new(),
            conns: BTreeMap::new(),
            next_conn: 1,
            tokens: BTreeMap::new(),
            require_token: false,
            inject: None,
            check_entities: true,
            blocked: BTreeMap::new(),
            blobs: BTreeMap::new(),
            chains: Vec::new(),
            base_chain: CHAIN_START,
            use_chain: true,
            epoch: 0,
            owners: BTreeMap::new(),
            inject_blob: None,
            blob_blocked: BTreeMap::new(),
        }
    }

    /// Clients must send this token in `hello`. Any user may use it.
    pub fn require_token(&mut self, token: &str) {
        self.require_token = true;
        self.tokens.insert(token.to_string(), None);
    }

    /// Clients must send a token, and this token belongs to this user: a `hello` with another
    /// user is refused.
    pub fn bind_token(&mut self, token: &str, user: Uid) {
        self.require_token = true;
        self.tokens.insert(token.to_string(), Some(user));
    }

    /// Turns the check of the things on or off (it is on by default). Tests of the merge rules use
    /// made-up properties that are not valid objects, and switch it off.
    pub fn check_entities(&mut self, on: bool) {
        self.check_entities = on;
    }

    /// Names the log. A new name tells clients that the log started again.
    pub fn set_log_id(&mut self, id: &str) {
        self.log_id = id.to_string();
    }

    /// Gives the log a new name, as a server does after a restore (section 4.9). Clients that
    /// come back with the old name get a snapshot. The sequencer does it by itself when a client
    /// says (`last_hash`) that its log has other entries at the same sequence number.
    pub fn new_epoch(&mut self) {
        self.epoch += 1;
        self.log_id = format!("log-e{}", self.epoch);
    }

    /// Switches the hash chain off or on (it is on by default). Off, the sequencer sends no
    /// `chain` and ignores `last_hash`, like a server of the first version of the protocol.
    pub fn keep_chain(&mut self, on: bool) {
        self.use_chain = on;
    }

    /// The chain hash after the entry `seq`, if the log still holds it (or `seq` is just before
    /// its first entry).
    pub fn chain_at(&self, seq: u64) -> Option<[u8; 32]> {
        if seq < self.log_start || seq > self.seq {
            None
        } else if seq == self.log_start {
            Some(self.base_chain)
        } else {
            Some(self.chains[(seq - self.log_start - 1) as usize])
        }
    }

    fn chain_text(&self, seq: u64) -> Option<String> {
        self.chain_at(seq)
            .filter(|_| self.use_chain)
            .map(|c| chain::to_hex(&c))
    }

    /// Makes the next `count` submits fail with this error (nothing is ordered).
    pub fn inject_errors(&mut self, code: ErrorCode, count: usize) {
        self.inject = Some((code, count));
    }

    /// Makes the next `count` `blob_put` messages fail with this error. The error names the blob
    /// (`hash`). With a retry class, the submits of the connection fail with the same code until
    /// the blob is put again (rule 7).
    pub fn inject_blob_errors(&mut self, code: ErrorCode, count: usize) {
        self.inject_blob = Some((code, count));
    }

    /// Keeps only the last `n` log entries. A client that is further behind gets a snapshot.
    pub fn set_log_limit(&mut self, n: usize) {
        self.log_limit = n;
        self.trim();
    }

    fn trim(&mut self) {
        if self.log.len() > self.log_limit {
            let cut = self.log.len() - self.log_limit;
            self.log.drain(..cut);
            self.base_chain = self.chains[cut - 1];
            self.chains.drain(..cut);
            self.log_start += cut as u64;
        }
    }

    /// True if the sequencer has this blob.
    pub fn has_blob(&self, hash: &str) -> bool {
        self.blobs.contains_key(hash)
    }

    /// Forgets all blobs, as a clean-up of the server does.
    pub fn forget_blobs(&mut self) {
        self.blobs.clear();
    }

    pub fn replica(&self) -> &Replica {
        &self.replica
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn log(&self) -> &[Entry] {
        &self.log
    }

    /// How many connections are open.
    pub fn connections(&self) -> usize {
        self.conns.len()
    }

    /// Opens a connection. The client is not known until `hello`.
    pub fn open(&mut self) -> ConnId {
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(
            id,
            Conn {
                who: None,
                name: String::new(),
                presence: PresenceState::default(),
                throttle: Throttle::new(presence::MIN_INTERVAL_MS),
                held: None,
            },
        );
        id
    }

    /// The connection ended. The others hear that the client left.
    pub fn close(&mut self, conn: ConnId) -> Vec<Out> {
        self.blocked.remove(&conn);
        self.blob_blocked.remove(&conn);
        match self.conns.remove(&conn) {
            Some(Conn {
                who: Some((client, _)),
                ..
            }) => self.to_others(conn, ServerMsg::PresenceLeave { client }),
            _ => Vec::new(),
        }
    }

    fn joined(&self) -> impl Iterator<Item = (&ConnId, &Conn)> {
        self.conns.iter().filter(|(_, c)| c.who.is_some())
    }

    fn to_others(&self, from: ConnId, msg: ServerMsg) -> Vec<Out> {
        self.joined()
            .filter(|(id, _)| **id != from)
            .map(|(id, _)| Out {
                conn: *id,
                msg: msg.clone(),
            })
            .collect()
    }

    /// Sends the presence that was held back, if its time has come.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Out> {
        let due: Vec<ConnId> = self
            .conns
            .iter()
            .filter(|(_, c)| c.held.is_some() && c.throttle.ready(now_ms))
            .map(|(id, _)| *id)
            .collect();
        let mut out = Vec::new();
        for conn in due {
            let c = self.conns.get_mut(&conn).expect("listed");
            let state = c.held.take().expect("checked");
            c.throttle.allow(now_ms);
            c.presence = state.clone();
            let (client, user) = c.who.expect("only joined connections hold presence");
            let name = c.name.clone();
            out.extend(self.to_others(
                conn,
                ServerMsg::Presence {
                    client,
                    user,
                    name,
                    state,
                },
            ));
        }
        out
    }

    /// Handles one message of a connection. `now_ms` is for the presence rate limit.
    pub fn receive(&mut self, conn: ConnId, msg: ClientMsg, now_ms: u64) -> Vec<Out> {
        if !self.conns.contains_key(&conn) {
            return Vec::new();
        }
        let who = self.conns[&conn].who;
        let reply = |msg| vec![Out { conn, msg }];
        match (msg, who) {
            (
                ClientMsg::Hello {
                    protocol,
                    user,
                    client,
                    name,
                    token,
                    last_seq,
                    log_id,
                    last_hash,
                },
                None,
            ) => {
                if protocol != PROTOCOL_VERSION {
                    return reply(error(
                        ErrorCode::UnsupportedProtocol,
                        "this sequencer speaks protocol 1",
                        None,
                    ));
                }
                if self.require_token {
                    match self.tokens.get(&token) {
                        Some(bound) if bound.is_none_or(|u| u == user) => {}
                        _ => return reply(error(ErrorCode::AuthFailed, "wrong token", None)),
                    }
                }
                // A client ID belongs to the first user who signs in with it, also when that
                // client is offline later.
                if *self.owners.entry(client).or_insert(user) != user {
                    return reply(error(
                        ErrorCode::AuthFailed,
                        "this client ID belongs to another user",
                        None,
                    ));
                }
                // The client says which history it has. If this log has another one at that
                // number, the log is not the one the client knew: it gets a new name.
                if self.use_chain
                    && log_id.as_deref() == Some(self.log_id.as_str())
                    && let (Some(n), Some(h)) = (last_seq, last_hash.as_deref())
                    && let Some(mine) = self.chain_text(n)
                    && mine != h
                {
                    self.new_epoch();
                }
                self.join(conn, user, client, &name, last_seq, log_id)
            }
            (ClientMsg::Hello { .. }, Some(_)) => {
                reply(error(ErrorCode::BadMessage, "hello twice", None))
            }
            (_, None) => reply(error(ErrorCode::BadMessage, "say hello first", None)),
            (ClientMsg::Submit { op_id, ops }, Some((client, user))) => {
                self.submit(conn, client, user, op_id, ops)
            }
            (ClientMsg::Presence { state }, Some((client, user))) => {
                if state.check().is_err() {
                    return reply(error(ErrorCode::BadMessage, "bad presence", None));
                }
                let c = self.conns.get_mut(&conn).expect("checked above");
                if !c.throttle.allow(now_ms) {
                    c.held = Some(state);
                    return Vec::new();
                }
                c.held = None;
                c.presence = state.clone();
                let name = c.name.clone();
                self.to_others(
                    conn,
                    ServerMsg::Presence {
                        client,
                        user,
                        name,
                        state,
                    },
                )
            }
            (ClientMsg::BlobPut { hash, data }, Some(_)) => {
                if let Some((code, n)) = self.inject_blob {
                    self.inject_blob = (n > 1).then_some((code, n - 1));
                    if code.class() == crate::protocol::ErrorClass::Retry {
                        self.blob_blocked.insert(conn, (code, hash.clone()));
                    }
                    return reply(ServerMsg::Error {
                        code,
                        message: "injected by the test".into(),
                        op_id: None,
                        hash: Some(hash),
                    });
                }
                if data.len() > crate::blob::MAX_BLOB_BYTES
                    || crate::blob::sha256_hex(data.as_bytes()) != hash
                {
                    return reply(error(
                        ErrorCode::BadMessage,
                        "the hash is not the hash of the data",
                        None,
                    ));
                }
                if self
                    .blob_blocked
                    .get(&conn)
                    .is_some_and(|(_, h)| *h == hash)
                {
                    self.blob_blocked.remove(&conn);
                }
                self.blobs.insert(hash, data);
                Vec::new()
            }
            (ClientMsg::BlobGet { hash }, Some(_)) => match self.blobs.get(&hash) {
                Some(data) => reply(ServerMsg::Blob {
                    data: data.clone(),
                    hash,
                }),
                None => reply(ServerMsg::BlobMissing { hash }),
            },
            (ClientMsg::Bye, Some(_)) => self.close(conn),
        }
    }

    fn join(
        &mut self,
        conn: ConnId,
        user: Uid,
        client: Uid,
        name: &str,
        last_seq: Option<u64>,
        log_id: Option<String>,
    ) -> Vec<Out> {
        let name = presence::clean_name(name);
        {
            let c = self.conns.get_mut(&conn).expect("open connection");
            c.who = Some((client, user));
            c.name = name.clone();
        }
        let announce = ServerMsg::Presence {
            client,
            user,
            name,
            state: PresenceState::default(),
        };
        let peers = self
            .joined()
            .filter(|(id, _)| **id != conn)
            .map(|(_, c)| {
                let (client, user) = c.who.expect("joined");
                Peer {
                    client,
                    user,
                    name: c.name.clone(),
                    state: c.presence.clone(),
                }
            })
            .collect();
        let mut out = self.to_others(conn, announce);
        out.push(Out {
            conn,
            msg: ServerMsg::Welcome {
                protocol: PROTOCOL_VERSION,
                log_id: self.log_id.clone(),
                head_seq: self.seq,
                your_last_op: self.writers.get(&client).map_or(0, |w| w.high),
                peers,
            },
        });
        let same_log = log_id.as_deref() == Some(self.log_id.as_str());
        let can_resume = same_log && last_seq.is_some_and(|n| n >= self.log_start && n <= self.seq);
        out.push(Out {
            conn,
            msg: if can_resume {
                let n = last_seq.expect("checked");
                ServerMsg::Entries {
                    entries: self.log[(n - self.log_start) as usize..].to_vec(),
                }
            } else {
                ServerMsg::Snapshot {
                    log_id: self.log_id.clone(),
                    seq: self.seq,
                    state: self.replica.clone(),
                    chain: self.chain_text(self.seq),
                }
            },
        });
        out
    }

    fn submit(
        &mut self,
        conn: ConnId,
        client: Uid,
        user: Uid,
        op_id: u64,
        ops: Vec<Op>,
    ) -> Vec<Out> {
        let reply = |msg| vec![Out { conn, msg }];
        if let Some((code, from)) = self.blocked.get(&conn).copied() {
            if op_id > from {
                return reply(error(
                    code,
                    "an earlier transaction was refused",
                    Some(op_id),
                ));
            }
            self.blocked.remove(&conn);
        }
        if let Some((code, hash)) = self.blob_blocked.get(&conn).cloned() {
            return reply(ServerMsg::Error {
                code,
                message: "a blob was refused".into(),
                op_id: Some(op_id),
                hash: Some(hash),
            });
        }
        if let Some((code, n)) = self.inject {
            self.inject = (n > 1).then_some((code, n - 1));
            if code.class() == crate::protocol::ErrorClass::Retry {
                self.blocked.insert(conn, (code, op_id));
            }
            return reply(error(code, "injected by the test", Some(op_id)));
        }
        let hash = ops_digest(&ops);
        if let Some(w) = self.writers.get(&client) {
            if op_id == w.high && hash == w.hash {
                return reply(ServerMsg::Ack {
                    op_id,
                    seq: w.seq,
                    chain: self.chain_text(w.seq),
                });
            }
            if op_id <= w.high {
                return reply(error(
                    ErrorCode::OpIdReused,
                    "the number is used or too low",
                    Some(op_id),
                ));
            }
        }
        if let Err(e) = validate_ops(&ops) {
            return reply(error(ErrorCode::InvalidTxn, &e.to_string(), Some(op_id)));
        }
        let blobs = |h: &str| self.blobs.get(h).cloned();
        let checked = if self.check_entities {
            check_transaction(&self.replica, &ops, &blobs)
        } else {
            check_duplicates(&self.replica, &ops)
        };
        if let Err(e) = checked {
            let code = match e {
                CheckError::DuplicateUid(_) => ErrorCode::DuplicateUid,
                _ => ErrorCode::InvalidTxn,
            };
            return reply(error(code, &e.to_string(), Some(op_id)));
        }
        self.seq += 1;
        self.replica.apply_all(Writer { client, op: op_id }, &ops);
        let prev = self.chain_at(self.seq - 1).expect("the log holds its head");
        let link = chain::chain_next(&prev, self.seq, client, op_id, &ops);
        let entry = Entry {
            seq: self.seq,
            author: client,
            user,
            op_id,
            ops,
            chain: self.use_chain.then(|| chain::to_hex(&link)),
        };
        self.chains.push(link);
        self.log.push(entry.clone());
        self.trim();
        self.writers.insert(
            client,
            WriterRecord {
                high: op_id,
                seq: self.seq,
                hash,
            },
        );
        let mut out = reply(ServerMsg::Ack {
            op_id,
            seq: self.seq,
            chain: entry.chain.clone(),
        });
        out.extend(self.to_others(conn, ServerMsg::Broadcast { entry }));
        out
    }
}

fn error(code: ErrorCode, message: &str, op_id: Option<u64>) -> ServerMsg {
    ServerMsg::Error {
        code,
        message: message.to_string(),
        op_id,
        hash: None,
    }
}

// ----- a hub that connects in-memory transports to a sequencer -----

struct HubInner {
    seq: Sequencer,
    peers: BTreeMap<ConnId, MemPeer>,
    now_ms: u64,
}

/// A sequencer with in-memory connections: an [`Endpoint`] for tests and examples. Clones share
/// the same sequencer.
#[derive(Clone)]
pub struct Hub {
    inner: Arc<Mutex<HubInner>>,
}

impl Hub {
    pub fn new(seq: Sequencer) -> Hub {
        Hub {
            inner: Arc::new(Mutex::new(HubInner {
                seq,
                peers: BTreeMap::new(),
                now_ms: 0,
            })),
        }
    }

    /// Runs a function on the sequencer.
    pub fn with<R>(&self, f: impl FnOnce(&mut Sequencer) -> R) -> R {
        f(&mut self.inner.lock().unwrap().seq)
    }

    /// Sets the time that the sequencer uses for the presence rate limit.
    pub fn set_time(&self, now_ms: u64) {
        self.inner.lock().unwrap().now_ms = now_ms;
    }

    /// An endpoint that opens a connection to this hub.
    pub fn endpoint(&self) -> HubEndpoint {
        HubEndpoint { hub: self.clone() }
    }

    /// Moves all messages: reads what clients sent, lets the sequencer answer, and delivers the
    /// answers. Connections that the client closed are closed in the sequencer too.
    pub fn step(&self) {
        let mut g = self.inner.lock().unwrap();
        let inner = &mut *g;
        let now = inner.now_ms;
        for o in inner.seq.tick(now) {
            if let Some(p) = inner.peers.get_mut(&o.conn) {
                p.send(&o.msg);
            }
        }
        let mut work = true;
        while work {
            work = false;
            let ids: Vec<ConnId> = inner.peers.keys().copied().collect();
            for id in ids {
                let mut outs = Vec::new();
                loop {
                    match inner.peers.get_mut(&id).expect("listed").recv() {
                        Ok(Some(msg)) => outs.extend(inner.seq.receive(id, msg, now)),
                        Ok(None) => break,
                        Err(_) => {
                            outs.push(Out {
                                conn: id,
                                msg: error(ErrorCode::BadMessage, "not a message", None),
                            });
                        }
                    }
                }
                if inner.peers[&id].is_closed() {
                    inner.peers.remove(&id);
                    outs.extend(inner.seq.close(id));
                }
                work |= !outs.is_empty();
                for o in outs {
                    if let Some(p) = inner.peers.get_mut(&o.conn) {
                        p.send(&o.msg);
                    }
                }
            }
        }
    }

    /// Closes all connections from the server side, as a network fault does.
    pub fn drop_all(&self) {
        let mut g = self.inner.lock().unwrap();
        let inner = &mut *g;
        for (id, mut p) in std::mem::take(&mut inner.peers) {
            p.close();
            inner.seq.close(id);
        }
    }
}

/// Makes connections to a [`Hub`].
pub struct HubEndpoint {
    hub: Hub,
}

impl Endpoint for HubEndpoint {
    fn describe(&self) -> String {
        "in-memory test hub".into()
    }

    fn connect(&mut self) -> Result<Box<dyn Transport>, TransportError> {
        let (client_end, peer) = mem_pair();
        let mut g = self.hub.inner.lock().unwrap();
        let id = g.seq.open();
        g.peers.insert(id, peer);
        Ok(Box::new(HubTransport {
            inner: client_end,
            hub: self.hub.clone(),
        }))
    }
}

/// The client end of a hub connection. Every send also runs the hub, so the answer is ready
/// at the next `recv`.
pub struct HubTransport {
    inner: MemTransport,
    hub: Hub,
}

impl Transport for HubTransport {
    fn send(&mut self, msg: &ClientMsg) -> Result<(), TransportError> {
        self.inner.send(msg)?;
        self.hub.step();
        Ok(())
    }

    fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError> {
        self.hub.step();
        self.inner.recv()
    }

    fn close(&mut self) {
        self.inner.close();
        self.hub.step();
    }
}
