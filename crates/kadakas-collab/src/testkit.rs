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
//! - Checks `hello` (protocol version, optional token) and answers `welcome` and then `entries`
//!   or `snapshot`.
//! - Gives each valid transaction the next sequence number, applies it to its own replica, sends
//!   `ack` to the author and `broadcast` to the others.
//! - Answers a repeated transaction (same user and `op_id`) with the old sequence number.
//! - Refuses `create` of an ID that exists or existed, and malformed transactions.
//! - Relays presence to the others, at most once per user per `MIN_INTERVAL_MS`.
//! - Can forget old log entries, so clients that are far behind get a `snapshot`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use kadakas_core::uid::Uid;

use crate::op::{Op, Target, Writer, validate_ops};
use crate::presence::{self, Peer, PresenceState, Throttle};
use crate::protocol::{ClientMsg, Entry, ErrorCode, PROTOCOL_VERSION, ServerMsg};
use crate::replica::Replica;
use crate::transport::{Endpoint, MemPeer, MemTransport, Transport, TransportError, mem_pair};

/// A connection number, made by [`Sequencer::open`].
pub type ConnId = u64;

/// A message for a connection.
#[derive(Clone, Debug, PartialEq)]
pub struct Out {
    pub conn: ConnId,
    pub msg: ServerMsg,
}

struct Conn {
    /// Set after a good `hello`.
    user: Option<Uid>,
    name: String,
    presence: PresenceState,
    throttle: Throttle,
}

/// The reference sequencer.
pub struct Sequencer {
    replica: Replica,
    seq: u64,
    log: Vec<Entry>,
    /// The sequence number before the first entry of `log`.
    log_start: u64,
    log_limit: usize,
    done: BTreeMap<(Uid, u64), u64>,
    last_op: BTreeMap<Uid, u64>,
    conns: BTreeMap<ConnId, Conn>,
    next_conn: ConnId,
    token: Option<String>,
}

impl Sequencer {
    /// A sequencer for a document in this state, at sequence number 0.
    pub fn new(initial: Replica) -> Sequencer {
        Sequencer {
            replica: initial,
            seq: 0,
            log: Vec::new(),
            log_start: 0,
            log_limit: usize::MAX,
            done: BTreeMap::new(),
            last_op: BTreeMap::new(),
            conns: BTreeMap::new(),
            next_conn: 1,
            token: None,
        }
    }

    /// Clients must send this token in `hello`.
    pub fn require_token(&mut self, token: &str) {
        self.token = Some(token.to_string());
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
            self.log_start += cut as u64;
        }
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

    /// Opens a connection. The user is not known until `hello`.
    pub fn open(&mut self) -> ConnId {
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(
            id,
            Conn {
                user: None,
                name: String::new(),
                presence: PresenceState::default(),
                throttle: Throttle::new(presence::MIN_INTERVAL_MS),
            },
        );
        id
    }

    /// The connection ended. The others hear that the user left.
    pub fn close(&mut self, conn: ConnId) -> Vec<Out> {
        match self.conns.remove(&conn) {
            Some(Conn {
                user: Some(user), ..
            }) => self.to_others(conn, ServerMsg::PresenceLeave { user }),
            _ => Vec::new(),
        }
    }

    fn joined(&self) -> impl Iterator<Item = (&ConnId, &Conn)> {
        self.conns.iter().filter(|(_, c)| c.user.is_some())
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

    /// Handles one message of a connection. `now_ms` is for the presence rate limit.
    pub fn receive(&mut self, conn: ConnId, msg: ClientMsg, now_ms: u64) -> Vec<Out> {
        if !self.conns.contains_key(&conn) {
            return Vec::new();
        }
        let user = self.conns[&conn].user;
        let reply = |msg| vec![Out { conn, msg }];
        match (msg, user) {
            (
                ClientMsg::Hello {
                    protocol,
                    user: who,
                    name,
                    token,
                    last_seq,
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
                if self.token.as_ref().is_some_and(|t| *t != token) {
                    return reply(error(ErrorCode::AuthFailed, "wrong token", None));
                }
                self.join(conn, who, &name, last_seq)
            }
            (ClientMsg::Hello { .. }, Some(_)) => {
                reply(error(ErrorCode::BadMessage, "hello twice", None))
            }
            (_, None) => reply(error(ErrorCode::BadMessage, "say hello first", None)),
            (ClientMsg::Submit { op_id, ops }, Some(user)) => self.submit(conn, user, op_id, ops),
            (ClientMsg::Presence { state }, Some(user)) => {
                if state.check().is_err() {
                    return reply(error(ErrorCode::BadMessage, "bad presence", None));
                }
                let c = self.conns.get_mut(&conn).expect("checked above");
                if !c.throttle.allow(now_ms) {
                    return Vec::new();
                }
                c.presence = state.clone();
                let name = c.name.clone();
                self.to_others(conn, ServerMsg::Presence { user, name, state })
            }
            (ClientMsg::Bye, Some(_)) => self.close(conn),
        }
    }

    fn join(&mut self, conn: ConnId, user: Uid, name: &str, last_seq: Option<u64>) -> Vec<Out> {
        let name = presence::clean_name(name);
        {
            let c = self.conns.get_mut(&conn).expect("open connection");
            c.user = Some(user);
            c.name = name;
        }
        let peers = self
            .joined()
            .filter(|(id, _)| **id != conn)
            .map(|(_, c)| Peer {
                user: c.user.expect("joined"),
                name: c.name.clone(),
                state: c.presence.clone(),
            })
            .collect();
        let mut out = vec![Out {
            conn,
            msg: ServerMsg::Welcome {
                protocol: PROTOCOL_VERSION,
                head_seq: self.seq,
                your_last_op: self.last_op.get(&user).copied().unwrap_or(0),
                peers,
            },
        }];
        let can_resume = last_seq.is_some_and(|n| n >= self.log_start && n <= self.seq);
        out.push(Out {
            conn,
            msg: if can_resume {
                let n = last_seq.expect("checked");
                ServerMsg::Entries {
                    entries: self.log[(n - self.log_start) as usize..].to_vec(),
                }
            } else {
                ServerMsg::Snapshot {
                    seq: self.seq,
                    state: self.replica.clone(),
                }
            },
        });
        out
    }

    fn submit(&mut self, conn: ConnId, user: Uid, op_id: u64, ops: Vec<Op>) -> Vec<Out> {
        let reply = |msg| vec![Out { conn, msg }];
        if let Some(seq) = self.done.get(&(user, op_id)) {
            return reply(ServerMsg::Ack { op_id, seq: *seq });
        }
        if let Err(e) = validate_ops(&ops) {
            return reply(error(ErrorCode::InvalidTxn, &e.to_string(), Some(op_id)));
        }
        let mut made: Vec<Target> = Vec::new();
        for op in &ops {
            if let Op::Create { target, .. } = op {
                if self.replica.contains(*target) || made.contains(target) {
                    return reply(error(
                        ErrorCode::DuplicateUid,
                        &format!("the ID {} is used", target.id),
                        Some(op_id),
                    ));
                }
                made.push(*target);
            }
        }
        self.seq += 1;
        self.replica.apply_all(Writer { user, op: op_id }, &ops);
        let entry = Entry {
            seq: self.seq,
            author: user,
            op_id,
            ops,
        };
        self.log.push(entry.clone());
        self.trim();
        self.done.insert((user, op_id), self.seq);
        let last = self.last_op.entry(user).or_insert(0);
        *last = (*last).max(op_id);
        let mut out = reply(ServerMsg::Ack {
            op_id,
            seq: self.seq,
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
