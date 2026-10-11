//! A small WebSocket server for tests (feature `test-server`). **This is not a server.**
//!
//! It runs the in-memory reference sequencer of `kadakas_collab::testkit` behind real WebSocket
//! connections on `127.0.0.1`, so tests of the transport, of the API and of the program can
//! run a full collaboration with no Docker. It has no accounts and no storage: the sequencer
//! can check one token (`require_token`). The real server is the `kadakas-server` repository.
//!
//! One thread accepts connections, and one thread serves each connection.

use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kadakas_collab::protocol::ServerMsg;
use kadakas_collab::replica::Replica;
use kadakas_collab::testkit::{ConnId, Sequencer};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{HeaderValue, StatusCode};
use tungstenite::{Message, WebSocket};

/// What a connection showed in its upgrade request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub path: String,
    /// The value of the `Authorization` header, if there was one.
    pub authorization: Option<String>,
    /// The value of `Sec-WebSocket-Protocol`, if there was one.
    pub protocols: Option<String>,
}

struct State {
    seq: Sequencer,
    queues: HashMap<ConnId, VecDeque<ServerMsg>>,
    seen: Vec<Seen>,
    /// Answer the upgrade with 400 when it has an `Authorization` header (a proxy that removes it).
    refuse_authorization: bool,
    /// Answer the upgrade with this status.
    refuse_all: Option<u16>,
    /// The token that the upgrade must carry, if any.
    upgrade_token: Option<String>,
    clock: Instant,
}

/// A WebSocket server for one test.
pub struct TestServer {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    /// Counts up when the server drops all connections.
    generation: Arc<AtomicU64>,
    listening: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl TestServer {
    /// Starts on a free port with an empty document.
    pub fn start() -> TestServer {
        TestServer::start_with(Sequencer::new(Replica::default()))
    }

    pub fn start_with(seq: Sequencer) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let addr = listener.local_addr().expect("an address");
        let mut s = TestServer {
            addr,
            state: Arc::new(Mutex::new(State {
                seq,
                queues: HashMap::new(),
                seen: Vec::new(),
                refuse_authorization: false,
                refuse_all: None,
                upgrade_token: None,
                clock: Instant::now(),
            })),
            generation: Arc::new(AtomicU64::new(0)),
            listening: Arc::new(AtomicBool::new(true)),
            accept: None,
        };
        s.spawn_accept(listener);
        s
    }

    fn spawn_accept(&mut self, listener: TcpListener) {
        listener.set_nonblocking(true).expect("non-blocking");
        let state = self.state.clone();
        let generation = self.generation.clone();
        let listening = self.listening.clone();
        listening.store(true, Ordering::SeqCst);
        self.accept = Some(std::thread::spawn(move || {
            while listening.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (state, generation, listening) =
                            (state.clone(), generation.clone(), listening.clone());
                        std::thread::spawn(move || serve(stream, state, generation, listening));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        }));
    }

    /// `ws://127.0.0.1:<port>`
    pub fn base_url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    /// `ws://127.0.0.1:<port>/ws/<doc>`
    pub fn url(&self, doc: &str) -> String {
        format!("ws://{}/ws/{doc}", self.addr)
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Runs `f` with the sequencer (to read the state of the document or to set it up).
    pub fn with<R>(&self, f: impl FnOnce(&mut Sequencer) -> R) -> R {
        f(&mut self.state.lock().unwrap().seq)
    }

    /// Hello must carry this token, and the upgrade request must carry it too (as
    /// `Authorization: Bearer` or as the sub-protocol), or the server answers 401.
    pub fn require_token(&self, token: &str, user: kadakas_core::uid::Uid) {
        let mut st = self.state.lock().unwrap();
        st.seq.require_token(token);
        st.seq.bind_token(token, user);
        st.upgrade_token = Some(token.to_string());
    }

    /// The upgrade requests that came in, oldest first.
    pub fn seen(&self) -> Vec<Seen> {
        self.state.lock().unwrap().seen.clone()
    }

    /// Answer an upgrade that has an `Authorization` header with 400 (like a proxy that does not
    /// pass it), so a client must use the sub-protocol.
    pub fn refuse_authorization_header(&self, on: bool) {
        self.state.lock().unwrap().refuse_authorization = on;
    }

    /// Answer every upgrade with this HTTP status (`None`: do not).
    pub fn refuse_with_status(&self, status: Option<u16>) {
        self.state.lock().unwrap().refuse_all = status;
    }

    /// Closes all connections at once, like a restart or a lost network. The document stays.
    pub fn drop_connections(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Stops listening and closes all connections (a server that is down).
    pub fn stop(&mut self) {
        self.listening.store(false, Ordering::SeqCst);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
        self.drop_connections();
    }

    /// Starts again on the same port, with the same document (a restart).
    pub fn restart(&mut self) {
        self.stop();
        // The port may need a moment to be free again.
        let end = Instant::now() + Duration::from_secs(5);
        let listener = loop {
            match TcpListener::bind(self.addr) {
                Ok(l) => break l,
                Err(e) => {
                    assert!(Instant::now() < end, "cannot bind again: {e}");
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        self.spawn_accept(listener);
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(
    stream: TcpStream,
    state: Arc<Mutex<State>>,
    generation: Arc<AtomicU64>,
    listening: Arc<AtomicBool>,
) {
    let my_generation = generation.load(Ordering::SeqCst);
    let _ = stream.set_nodelay(true);
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    #[allow(clippy::result_large_err)] // the type is fixed by tungstenite's callback trait
    let callback = |req: &Request, mut resp: Response| -> Result<Response, ErrorResponse> {
        let header = |n: &str| {
            req.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let seen = Seen {
            path: req.uri().path().to_string(),
            authorization: header("authorization"),
            protocols: header("sec-websocket-protocol"),
        };
        let mut st = state.lock().unwrap();
        st.seen.push(seen.clone());
        let status = |code: u16| -> ErrorResponse {
            let mut r = ErrorResponse::new(None);
            *r.status_mut() = StatusCode::from_u16(code).unwrap();
            r
        };
        if let Some(code) = st.refuse_all {
            return Err(status(code));
        }
        if st.refuse_authorization && seen.authorization.is_some() {
            return Err(status(400));
        }
        if let Some(want) = &st.upgrade_token {
            let by_header = seen.authorization.as_deref() == Some(&format!("Bearer {want}"));
            let by_protocol = seen.protocols.as_deref().is_some_and(|p| {
                p.split(',')
                    .any(|x| x.trim() == format!("kadakas.token.{want}"))
            });
            if !by_header && !by_protocol {
                return Err(status(401));
            }
        }
        if seen
            .protocols
            .as_deref()
            .is_some_and(|p| p.split(',').any(|x| x.trim() == "kadakas.v1"))
        {
            resp.headers_mut().insert(
                "sec-websocket-protocol",
                HeaderValue::from_static("kadakas.v1"),
            );
        }
        Ok(resp)
    };
    let Ok(mut ws) = tungstenite::accept_hdr(stream, callback) else {
        return;
    };
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(3)));
    let conn = {
        let mut st = state.lock().unwrap();
        let c = st.seq.open();
        st.queues.insert(c, VecDeque::new());
        c
    };
    run_connection(
        &mut ws,
        conn,
        &state,
        &generation,
        my_generation,
        &listening,
    );
    let mut st = state.lock().unwrap();
    st.queues.remove(&conn);
    let outs = st.seq.close(conn);
    route(&mut st, outs);
    let _ = ws.close(None);
    let _ = ws.flush();
}

fn route(st: &mut State, outs: Vec<kadakas_collab::testkit::Out>) {
    for o in outs {
        if let Some(q) = st.queues.get_mut(&o.conn) {
            q.push_back(o.msg);
        }
    }
}

fn run_connection(
    ws: &mut WebSocket<TcpStream>,
    conn: ConnId,
    state: &Arc<Mutex<State>>,
    generation: &AtomicU64,
    my_generation: u64,
    listening: &AtomicBool,
) {
    let _ = listening;
    loop {
        if generation.load(Ordering::SeqCst) != my_generation {
            return;
        }
        // Timers of the sequencer (held presence updates), and what waits for this connection.
        let to_send: Vec<ServerMsg> = {
            let mut st = state.lock().unwrap();
            let now = st.clock.elapsed().as_millis() as u64;
            let outs = st.seq.tick(now);
            route(&mut st, outs);
            st.queues
                .get_mut(&conn)
                .map(|q| q.drain(..).collect())
                .unwrap_or_default()
        };
        for m in to_send {
            let text = serde_json::to_string(&m).expect("JSON");
            if ws.send(Message::text(text)).is_err() {
                return;
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                let mut st = state.lock().unwrap();
                let now = st.clock.elapsed().as_millis() as u64;
                let outs = match serde_json::from_str(t.as_str()) {
                    Ok(msg) => st.seq.receive(conn, msg, now),
                    Err(_) => return,
                };
                route(&mut st, outs);
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                let _ = ws.flush();
            }
            Err(_) => return,
        }
    }
}
