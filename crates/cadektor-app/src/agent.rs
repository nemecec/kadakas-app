//! TCP server for AI agents. Each line is a JSON-RPC request. The socket
//! threads never touch the document: they send the line to the UI thread
//! and wait for the response, so GUI and agent edits share one `Session`.
//!
//! Security: the server listens on 127.0.0.1 only, but any local program or
//! web page can reach that. So every connection must prove it knows a random
//! 256-bit token, which the app writes to a runtime file that only the current
//! user can read (see `cadektor_sys::agent::agent_info_path`). The first line
//! on a connection must be `{"jsonrpc":"2.0","id":..,"method":"auth","params":{"token":".."}}`.
//! A connection whose first byte is not `{` (for example an HTTP request from a
//! browser) is closed at once, and nothing it sent is run.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cadektor_sys::agent::{self as sys_agent, AgentInfo};
use serde_json::{Value, json};

/// Time that the UI thread spends on agent requests in one frame.
pub const FRAME_BUDGET: Duration = Duration::from_millis(8);

/// Limits that protect the app from a client that misbehaves.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Longest request line in bytes. A longer line closes the connection.
    pub max_line: usize,
    /// Number of clients at the same time. More are refused.
    pub max_clients: usize,
    /// Time that a new client has to send a valid `auth` line.
    pub auth_timeout: Duration,
    /// Time to wait for the UI thread to answer one request.
    pub reply_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_line: 16 * 1024 * 1024,
            max_clients: 4,
            auth_timeout: Duration::from_secs(10),
            reply_timeout: Duration::from_secs(30),
        }
    }
}

/// The first line must be small: it only holds a token.
const MAX_AUTH_LINE: usize = 4096;

/// What a request handler does with a request.
pub enum Reply {
    /// Answer now (`None`: no answer, for a notification).
    Now(Option<String>),
    /// The handler keeps the sender and answers in a later frame.
    Later,
}

/// Lines that wait for the writer of one connection. A client that does not read is dropped when
/// this is full, so a slow client cannot hold memory or the UI thread.
const OUTBOX_LINES: usize = 4096;

/// The writers of the open connections, by connection ID.
type Outboxes = Arc<Mutex<HashMap<u64, SyncSender<String>>>>;

pub struct Request {
    /// The connection that sent it (see [`AgentServer::push`]).
    pub conn: u64,
    pub line: String,
    pub reply: Sender<Option<String>>,
    /// Set by the socket thread when it gave up waiting. The request is then not run.
    pub cancelled: Arc<AtomicBool>,
}

/// Reads from a socket, and fails with `TimedOut` once one overall deadline has passed.
struct DeadlineStream {
    stream: TcpStream,
    deadline: Option<Instant>,
}

impl Read for DeadlineStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(d) = self.deadline {
            let left = d.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            self.stream.set_read_timeout(Some(left))?;
        }
        self.stream.read(buf)
    }
}

pub struct AgentServer {
    pub addr: SocketAddr,
    /// The secret that clients send in `auth`.
    pub token: String,
    rx: Receiver<Request>,
    /// A request that was taken from the queue when the time budget ran out. It runs first in the next poll.
    carry: std::sync::Mutex<Option<Request>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    info_path: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    outboxes: Outboxes,
}

impl AgentServer {
    /// Sends a line (a notification) to a connection, from any thread, without waiting. False
    /// if the connection is gone or does not read fast enough: the caller forgets it then.
    pub fn push(&self, conn: u64, line: String) -> bool {
        let mut boxes = self.outboxes.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tx) = boxes.get(&conn) else {
            return false;
        };
        if tx.try_send(line).is_ok() {
            return true;
        }
        boxes.remove(&conn); // the writer ends and closes the connection
        false
    }

    /// Answers waiting requests for at most `FRAME_BUDGET`, with `handle`
    /// turning each request line into a response line.
    /// Returns how many it handled.
    #[cfg(test)]
    pub fn poll(&self, handle: &mut dyn FnMut(&str) -> Option<String>) -> usize {
        self.poll_for(FRAME_BUDGET, handle)
    }

    /// Like `poll`, with an explicit time budget. When the budget runs out
    /// the UI is asked to repaint, so the rest is answered in the next frame.
    #[cfg(test)]
    pub fn poll_for(
        &self,
        budget: Duration,
        handle: &mut dyn FnMut(&str) -> Option<String>,
    ) -> usize {
        self.poll_ext_for(budget, &mut |line, _, _| Reply::Now(handle(line)))
    }

    /// Like `poll`, for a handler that can answer later: it gets the sender of the reply and
    /// returns `Reply::Later` to keep it. The client waits (up to `Limits::reply_timeout`) until
    /// the handler sends the answer line, or `None` for no answer.
    pub fn poll_ext(
        &self,
        handle: &mut dyn FnMut(&str, &Sender<Option<String>>, u64) -> Reply,
    ) -> usize {
        self.poll_ext_for(FRAME_BUDGET, handle)
    }

    pub fn poll_ext_for(
        &self,
        budget: Duration,
        handle: &mut dyn FnMut(&str, &Sender<Option<String>>, u64) -> Reply,
    ) -> usize {
        let start = Instant::now();
        let mut n = 0;
        let mut next = self
            .carry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .or_else(|| self.rx.try_recv().ok());
        while let Some(req) = next.take() {
            next = self.rx.try_recv().ok();
            if req.cancelled.load(Ordering::SeqCst) {
                continue; // the client was told that this request was cancelled
            }
            match handle(&req.line, &req.reply, req.conn) {
                Reply::Now(answer) => {
                    let _ = req.reply.send(answer);
                }
                Reply::Later => {}
            }
            n += 1;
            if start.elapsed() >= budget {
                // Ask for another frame only if a request is really waiting. The wake of a client
                // that is not waiting would be a repaint for nothing, and it made the tests flaky.
                if next.is_some() {
                    *self.carry.lock().unwrap_or_else(|e| e.into_inner()) = next;
                    (self.wake)();
                }
                break;
            }
        }
        n
    }
}

impl Drop for AgentServer {
    fn drop(&mut self) {
        if let Some(path) = &self.info_path {
            // Delete the runtime file, unless another app instance has replaced it.
            if sys_agent::read_agent_info(path).is_ok_and(|i| i.token == self.token) {
                let _ = std::fs::remove_file(path);
            }
        }
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr); // wakes the accept loop so that it can end
    }
}

fn random_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| std::io::Error::other(format!("no random numbers: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Listens on 127.0.0.1 only. Port 0 picks a free port.
/// `wake` runs after each request arrives, to repaint the UI.
/// With `info_path`, the port and token are written there (owner only) and
/// the file is deleted when the server is dropped.
pub fn start(
    port: u16,
    wake: Arc<dyn Fn() + Send + Sync>,
    info_path: Option<PathBuf>,
) -> std::io::Result<AgentServer> {
    start_with(port, wake, info_path, Limits::default())
}

pub fn start_with(
    port: u16,
    wake: Arc<dyn Fn() + Send + Sync>,
    info_path: Option<PathBuf>,
    limits: Limits,
) -> std::io::Result<AgentServer> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let addr = listener.local_addr()?;
    let token = random_token()?;
    if let Some(path) = &info_path {
        sys_agent::write_agent_info(
            path,
            &AgentInfo {
                port: addr.port(),
                token: token.clone(),
            },
        )?;
    }
    let (tx, rx) = mpsc::channel::<Request>();
    let stop = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let outboxes: Outboxes = Arc::default();
    {
        let (token, wake, stop) = (token.clone(), wake.clone(), stop.clone());
        let outboxes = outboxes.clone();
        let next_conn = AtomicU64::new(1);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                if active.fetch_add(1, Ordering::SeqCst) >= limits.max_clients {
                    active.fetch_sub(1, Ordering::SeqCst);
                    continue; // dropping the stream refuses the client
                }
                let guard = ClientGuard(active.clone());
                let (tx, wake, token) = (tx.clone(), wake.clone(), token.clone());
                let conn = next_conn.fetch_add(1, Ordering::SeqCst);
                let outboxes = outboxes.clone();
                thread::spawn(move || {
                    let _guard = guard;
                    serve_client(stream, &token, limits, conn, &outboxes, tx, wake);
                    outboxes
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&conn);
                });
            }
        });
    }
    Ok(AgentServer {
        addr,
        token,
        rx,
        carry: std::sync::Mutex::new(None),
        wake,
        info_path,
        stop,
        outboxes,
    })
}

/// Counts a connected client until its thread ends.
struct ClientGuard(Arc<AtomicUsize>);

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Compares in time that does not depend on where the first difference is.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Reads one line of at most `max` bytes (without the line end).
/// `Ok(None)` is the end of the stream. A longer line is an error.
fn read_line_capped<R: BufRead>(r: &mut R, max: usize) -> std::io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(max as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    } else if buf.len() > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "line too long",
        ));
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "not UTF-8"))
}

fn response(id: &Value, body: Value) -> String {
    let mut r = json!({"jsonrpc": "2.0", "id": id});
    r.as_object_mut()
        .expect("object")
        .extend(body.as_object().expect("object").clone());
    r.to_string()
}

fn send(out: &mut TcpStream, line: &str) -> bool {
    writeln!(out, "{line}").and_then(|_| out.flush()).is_ok()
}

/// Reads and checks the `auth` line. True if the client may continue.
fn authenticate<R: BufRead>(reader: &mut R, out: &mut TcpStream, token: &str) -> bool {
    // A web page can send bytes to this port, but not a line that starts with `{`.
    if reader
        .fill_buf()
        .map(|b| b.first() != Some(&b'{'))
        .unwrap_or(true)
    {
        return false;
    }
    let Ok(Some(line)) = read_line_capped(reader, MAX_AUTH_LINE) else {
        return false;
    };
    let Ok(req) = serde_json::from_str::<Value>(&line) else {
        return false;
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let sent = req["params"]["token"].as_str().unwrap_or("");
    if req["method"] == "auth" && constant_time_eq(sent.as_bytes(), token.as_bytes()) {
        send(out, &response(&id, json!({"result": {"ok": true}})))
    } else {
        send(
            out,
            &response(
                &id,
                json!({"error": {"code": -32001, "message": "authentication failed"}}),
            ),
        );
        false
    }
}

fn serve_client(
    stream: TcpStream,
    token: &str,
    limits: Limits,
    conn: u64,
    outboxes: &Outboxes,
    tx: Sender<Request>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let _ = out.set_write_timeout(Some(limits.reply_timeout));
    let mut reader = BufReader::new(DeadlineStream {
        stream,
        deadline: Some(Instant::now() + limits.auth_timeout),
    });
    if !authenticate(&mut reader, &mut out, token) {
        return;
    }
    reader.get_mut().deadline = None;
    let _ = reader.get_ref().stream.set_read_timeout(None);
    // One thread writes replies and pushed notifications, in the order they were queued.
    let (outbox, lines) = mpsc::sync_channel::<String>(OUTBOX_LINES);
    outboxes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(conn, outbox.clone());
    thread::spawn(move || {
        for line in lines {
            if !send(&mut out, &line) {
                let _ = out.shutdown(std::net::Shutdown::Both);
                return;
            }
        }
    });
    while let Ok(Some(line)) = read_line_capped(&mut reader, limits.max_line) {
        if line.trim().is_empty() {
            continue;
        }
        // The id is only needed to answer when the UI thread is too slow.
        let id = serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|v| v.get("id").cloned());
        let (reply, answer) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        if tx
            .send(Request {
                conn,
                line,
                reply,
                cancelled: cancelled.clone(),
            })
            .is_err()
        {
            return;
        }
        wake();
        let resp = match answer.recv_timeout(limits.reply_timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::SeqCst);
                id.map(|id| {
                    response(&id, json!({"error": {"code": -32000, "message": "timed out waiting for the app; the request was cancelled and was not run"}}))
                })
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if let Some(resp) = resp
            && outbox.send(resp).is_err()
        {
            return;
        }
    }
}

/// Folders from `--allow-dir DIR` (or `--allow-dir=DIR`), which agents may read and write. Default: the current folder.
pub fn parse_allow_dirs(args: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--allow-dir" {
            if let Some(v) = it.next() {
                dirs.push(PathBuf::from(v));
            }
        } else if let Some(v) = a.strip_prefix("--allow-dir=") {
            dirs.push(PathBuf::from(v));
        }
    }
    if dirs.is_empty() {
        dirs.push(PathBuf::from("."));
    }
    dirs
}

/// Addresses from `--allow-url URL` (or `--allow-url=URL`), which agents may fetch for map services.
/// Default: none.
pub fn parse_allow_urls(args: &[String]) -> Vec<String> {
    let mut urls = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--allow-url" {
            if let Some(v) = it.next() {
                urls.push(v.clone());
            }
        } else if let Some(v) = a.strip_prefix("--allow-url=") {
            urls.push(v.to_string());
        }
    }
    urls
}

/// Port from `--agent-port N` (or `--agent-port=N`), else from the env value.
pub fn parse_agent_port(args: &[String], env: Option<&str>) -> Option<u16> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--agent-port" {
            return it.next().and_then(|v| v.parse().ok());
        }
        if let Some(v) = a.strip_prefix("--agent-port=") {
            return v.parse().ok();
        }
    }
    env.and_then(|v| v.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_api::Session;
    use std::sync::atomic::AtomicUsize;

    fn wait() -> Arc<AtomicUsize> {
        Arc::new(AtomicUsize::new(0))
    }

    fn counter_wake(c: &Arc<AtomicUsize>) -> Arc<dyn Fn() + Send + Sync> {
        let c = c.clone();
        Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })
    }

    /// Plays the UI thread on its own thread until `finish` is called.
    struct Ui {
        stop: Arc<AtomicBool>,
        handled: Arc<AtomicUsize>,
        thread: Option<thread::JoinHandle<AgentServer>>,
    }

    impl Ui {
        fn start(server: AgentServer) -> Ui {
            let stop = Arc::new(AtomicBool::new(false));
            let handled = wait();
            let (s, h) = (stop.clone(), handled.clone());
            let thread = thread::spawn(move || {
                let mut session = Session::default();
                while !s.load(Ordering::SeqCst) {
                    let n = server.poll(&mut |line| session.handle_line(line));
                    h.fetch_add(n, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(2));
                }
                server
            });
            Ui {
                stop,
                handled,
                thread: Some(thread),
            }
        }

        fn handled(&self) -> usize {
            self.handled.load(Ordering::SeqCst)
        }

        fn finish(&mut self) -> AgentServer {
            self.stop.store(true, Ordering::SeqCst);
            self.thread.take().unwrap().join().unwrap()
        }
    }

    struct Client {
        out: TcpStream,
        reader: BufReader<TcpStream>,
    }

    impl Client {
        fn connect(addr: SocketAddr) -> Client {
            let out = TcpStream::connect(addr).unwrap();
            out.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let reader = BufReader::new(out.try_clone().unwrap());
            Client { out, reader }
        }

        fn send_raw(&mut self, bytes: &[u8]) -> bool {
            self.out
                .write_all(bytes)
                .and_then(|_| self.out.flush())
                .is_ok()
        }

        fn send(&mut self, line: &str) -> bool {
            self.send_raw(format!("{line}\n").as_bytes())
        }

        fn read(&mut self) -> Option<Value> {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(n) if n > 0 => serde_json::from_str(&line).ok(),
                _ => None,
            }
        }

        /// True if the server closed the connection (end of stream or reset).
        fn is_closed(&mut self) -> bool {
            let mut buf = [0u8; 64];
            match self.reader.read(&mut buf) {
                Ok(0) => true,
                Ok(_) => false,
                Err(e) => !matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ),
            }
        }

        /// Reads and ignores any reply, then reports whether the server closed the connection.
        fn drains_to_close(&mut self) -> bool {
            let mut buf = [0u8; 256];
            loop {
                match self.reader.read(&mut buf) {
                    Ok(0) => return true,
                    Ok(_) => {}
                    Err(e) => {
                        return !matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        );
                    }
                }
            }
        }

        fn auth(&mut self, token: &str) -> Option<Value> {
            self.send(
                &json!({"jsonrpc": "2.0", "id": 0, "method": "auth", "params": {"token": token}})
                    .to_string(),
            );
            self.read()
        }

        fn call(&mut self, id: u64, method: &str) -> Value {
            self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method}).to_string());
            self.read().expect("response")
        }
    }

    fn small_limits() -> Limits {
        Limits {
            max_line: 1024,
            max_clients: 4,
            auth_timeout: Duration::from_millis(300),
            reply_timeout: Duration::from_secs(5),
        }
    }

    fn started(limits: Limits) -> (Ui, SocketAddr, String) {
        let server = start_with(0, Arc::new(|| {}), None, limits).unwrap();
        let (addr, token) = (server.addr, server.token.clone());
        (Ui::start(server), addr, token)
    }

    #[test]
    fn allow_urls_from_arguments() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(parse_allow_urls(&a(&["app"])).is_empty());
        assert_eq!(
            parse_allow_urls(&a(&[
                "app",
                "--allow-url",
                "maps.example.org",
                "--allow-url=*.gov.fi",
                "--allow-url"
            ])),
            vec!["maps.example.org".to_string(), "*.gov.fi".to_string()]
        );
    }

    #[test]
    fn allow_dirs_from_arguments() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_allow_dirs(&a(&["app"])), vec![PathBuf::from(".")]);
        assert_eq!(
            parse_allow_dirs(&a(&[
                "app",
                "--allow-dir",
                "/a",
                "--allow-dir=/b",
                "--allow-dir"
            ])),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn port_parsing() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            parse_agent_port(&a(&["app", "--agent-port", "7001"]), Some("9")),
            Some(7001)
        );
        assert_eq!(
            parse_agent_port(&a(&["app", "--agent-port=7002"]), None),
            Some(7002)
        );
        assert_eq!(parse_agent_port(&a(&["app"]), Some("7003")), Some(7003));
        assert_eq!(parse_agent_port(&a(&["app"]), Some("x")), None);
    }

    #[test]
    fn round_trip_over_localhost_after_auth() {
        let wakes = wait();
        let server = start(0, counter_wake(&wakes), None).unwrap();
        assert!(server.addr.ip().is_loopback());
        assert_eq!(server.token.len(), 64);
        assert!(server.token.bytes().all(|b| b.is_ascii_hexdigit()));
        let (addr, token) = (server.addr, server.token.clone());

        let client = thread::spawn(move || {
            let mut c = Client::connect(addr);
            assert_eq!(c.auth(&token).unwrap()["result"]["ok"], true);
            let mut resp = Vec::new();
            resp.push(c.call(1, "describe"));
            // A notification gets no response.
            c.send(r#"{"method":"undo"}"#);
            resp.push(c.call(2, "document.summary"));
            // Blank lines are ignored.
            c.send("");
            resp.push(c.call(3, "selection.get"));
            resp
        });

        let mut session = Session::default();
        let mut handled = 0;
        while handled < 4 {
            handled += server.poll(&mut |line| session.handle_line(line));
            thread::sleep(Duration::from_millis(5));
        }
        let resp = client.join().unwrap();
        assert_eq!(resp[0]["id"], 1);
        assert!(resp[0]["result"]["methods"].is_array());
        assert_eq!(resp[1]["result"]["object_count"], 0);
        assert_eq!(resp[2]["result"]["ids"], json!([]));
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            4,
            "the auth line does not wake the UI"
        );
    }

    #[test]
    fn change_events_are_pushed_to_the_subscriber_only() {
        let server = start(0, Arc::new(|| {}), None).unwrap();
        let (addr, token) = (server.addr, server.token.clone());
        let watcher = thread::spawn({
            let token = token.clone();
            move || {
                let mut c = Client::connect(addr);
                c.auth(&token).unwrap();
                let sub = c.call(1, "events.subscribe");
                assert!(sub["result"]["revision"].is_u64());
                // The other client changes the document. A notification arrives, with no request.
                let note = c.read().unwrap();
                assert_eq!(note["method"], "events.change");
                assert!(note.get("id").is_none());
                assert_eq!(note["params"]["origin"], "agent");
                note
            }
        });
        let editor = thread::spawn(move || {
            let mut c = Client::connect(addr);
            c.auth(&token).unwrap();
            thread::sleep(Duration::from_millis(150));
            let r = c.call(1, "execute");
            r
        });
        let mut session = Session::with_sequential_ids();
        let mut sent = false;
        let mut handled = 0;
        let started = Instant::now();
        while !(sent && handled >= 2) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the test hangs"
            );
            handled += server.poll_ext(&mut |line, _, conn| {
                let line = if line.contains(r#""execute""#) {
                    json!({"jsonrpc": "2.0", "id": 1, "method": "execute", "params": {"command": {"op": "add_layer", "name": "L", "elevation": 0}}}).to_string()
                } else {
                    line.to_string()
                };
                Reply::Now(session.handle_line_as(conn, &line))
            });
            for conn in session.subscribers() {
                for note in session.take_notifications(conn) {
                    assert!(server.push(conn, note.to_string()));
                    sent = true;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        let note = watcher.join().unwrap();
        assert_eq!(note["params"]["op"], "add_layer");
        assert!(editor.join().unwrap()["result"]["created"].is_array());
        assert!(!server.push(9999, "x".into()), "an unknown connection");
    }

    #[test]
    fn requests_before_auth_are_never_run() {
        let (mut ui, addr, _) = started(small_limits());
        let mut c = Client::connect(addr);
        c.send(r#"{"jsonrpc":"2.0","id":1,"method":"describe"}"#);
        let reply = c.read().expect("an error is sent before the close");
        assert_eq!(reply["error"]["code"], -32001);
        assert!(reply.get("result").is_none());
        assert!(c.is_closed());
        // A request that tries to slip in behind the first line is not run either.
        let mut c = Client::connect(addr);
        c.send_raw(b"{\"id\":1,\"method\":\"execute\"}\n{\"id\":2,\"method\":\"describe\"}\n");
        assert!(c.drains_to_close());
        assert_eq!(ui.handled(), 0);
        ui.finish();
    }

    #[test]
    fn wrong_token_is_refused_and_closed() {
        let (mut ui, addr, token) = started(small_limits());
        for bad in [
            "nope".to_string(),
            String::new(),
            format!("{token}0"),
            token[..63].to_string(),
            token.to_uppercase(),
        ] {
            let mut c = Client::connect(addr);
            let r = c.auth(&bad).expect("an error is sent");
            assert_eq!(r["error"]["message"], "authentication failed", "{bad}");
            assert!(r.get("result").is_none());
            assert!(c.is_closed());
        }
        // A token that is not a string, a missing token and a wrong method are refused too.
        for line in [
            r#"{"id":1,"method":"auth","params":{"token":5}}"#,
            r#"{"id":1,"method":"auth"}"#,
            r#"{"id":1,"method":"describe","params":{"token":"x"}}"#,
        ] {
            let mut c = Client::connect(addr);
            c.send(line);
            assert!(c.read().unwrap().get("error").is_some());
            assert!(c.is_closed());
        }
        // Garbage that starts with `{` is closed without a reply.
        let mut c = Client::connect(addr);
        c.send("{not json");
        assert!(c.is_closed());
        // The right token still works afterwards.
        let mut c = Client::connect(addr);
        assert_eq!(c.auth(&token).unwrap()["result"]["ok"], true);
        assert_eq!(ui.handled(), 0);
        ui.finish();
    }

    #[test]
    fn first_byte_that_is_not_an_open_brace_closes_the_connection() {
        let (mut ui, addr, _) = started(small_limits());
        let mut c = Client::connect(addr);
        // What a browser sends in a cross-protocol POST.
        c.send_raw(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n{}");
        assert!(c.is_closed(), "closed with no reply");
        let mut c = Client::connect(addr);
        c.send_raw(b" {\"id\":1}\n");
        assert!(c.is_closed());
        let mut c = Client::connect(addr);
        c.send_raw(&[0xff, 0xfe, b'\n']);
        assert!(c.is_closed());
        assert_eq!(ui.handled(), 0);
        ui.finish();
    }

    #[test]
    fn oversized_lines_close_the_connection() {
        let (mut ui, addr, token) = started(small_limits());
        // The auth line itself is limited to a few KiB.
        let mut c = Client::connect(addr);
        c.send_raw(format!("{{\"id\":1,\"pad\":\"{}", "a".repeat(2 * MAX_AUTH_LINE)).as_bytes());
        assert!(c.is_closed());

        // After auth, a line without an end that is longer than the limit closes the connection.
        let mut c = Client::connect(addr);
        c.auth(&token).unwrap();
        c.send_raw(format!("{{\"pad\":\"{}", "a".repeat(2000)).as_bytes());
        assert!(c.is_closed());

        // So does a long line that ends.
        let mut c = Client::connect(addr);
        c.auth(&token).unwrap();
        c.send(&format!(
            "{{\"id\":1,\"method\":\"describe\",\"pad\":\"{}\"}}",
            "a".repeat(2000)
        ));
        assert!(c.is_closed());
        assert_eq!(ui.handled(), 0);

        // A line at the limit is fine.
        let mut c = Client::connect(addr);
        c.auth(&token).unwrap();
        let head = r#"{"jsonrpc":"2.0","id":7,"method":"describe","pad":""#;
        let pad = "a".repeat(1024 - head.len() - 2);
        c.send(&format!("{head}{pad}\"}}"));
        assert_eq!(c.read().unwrap()["id"], 7);
        ui.finish();
    }

    #[test]
    fn a_fifth_client_is_refused_until_one_leaves() {
        let (mut ui, addr, token) = started(small_limits());
        let mut clients: Vec<Client> = (0..4).map(|_| Client::connect(addr)).collect();
        for c in &mut clients {
            assert_eq!(c.auth(&token).unwrap()["result"]["ok"], true);
        }
        let mut fifth = Client::connect(addr);
        fifth.send(
            &json!({"jsonrpc": "2.0", "id": 0, "method": "auth", "params": {"token": token}})
                .to_string(),
        );
        assert!(fifth.is_closed(), "refused without an answer");
        // The four others still work.
        assert_eq!(clients[0].call(1, "selection.get")["id"], 1);

        // When one client leaves, a new one is accepted.
        drop(clients.pop());
        let mut accepted = false;
        for _ in 0..100 {
            let mut c = Client::connect(addr);
            if c.auth(&token).is_some() {
                accepted = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(accepted);
        ui.finish();
    }

    #[test]
    fn a_client_that_never_authenticates_is_dropped() {
        let (mut ui, addr, _) = started(small_limits());
        let mut c = Client::connect(addr);
        // The auth timeout is 300 ms; the client sends nothing.
        assert!(c.is_closed());
        ui.finish();
    }

    #[test]
    fn slow_ui_gets_a_timeout_error_and_the_client_can_go_on() {
        let limits = Limits {
            reply_timeout: Duration::from_millis(150),
            ..small_limits()
        };
        let server = start_with(0, Arc::new(|| {}), None, limits).unwrap();
        let (addr, token) = (server.addr, server.token.clone());
        let mut c = Client::connect(addr);
        c.auth(&token).unwrap();
        // Nobody polls, so the request times out.
        let r = c.call(5, "describe");
        assert_eq!(r["id"], 5);
        assert_eq!(r["error"]["code"], -32000);
        // A notification that times out gets no reply at all. Use a real request to see that we are in sync.
        c.send(r#"{"method":"undo"}"#);
        thread::sleep(Duration::from_millis(400)); // let the notification time out too
        let mut ui = Ui::start(server);
        // The two requests above were cancelled, so the UI skips them.
        let r = c.call(6, "selection.get");
        assert_eq!(r["id"], 6);
        assert_eq!(ui.handled(), 1, "timed-out requests are not run");
        ui.finish();
    }

    #[test]
    fn timeout_message_says_cancelled() {
        let limits = Limits {
            reply_timeout: Duration::from_millis(100),
            ..small_limits()
        };
        let server = start_with(0, Arc::new(|| {}), None, limits).unwrap();
        let mut c = Client::connect(server.addr);
        c.auth(&server.token.clone()).unwrap();
        let r = c.call(1, "describe");
        assert!(
            r["error"]["message"]
                .as_str()
                .unwrap()
                .contains("cancelled")
        );
    }

    #[test]
    fn trickling_client_hits_the_overall_auth_deadline() {
        let (mut ui, addr, _) = started(small_limits());
        let mut c = Client::connect(addr);
        c.send_raw(b"{");
        let start = Instant::now();
        // One byte every 100 ms would reset a per-read timeout forever.
        while start.elapsed() < Duration::from_secs(2) {
            if !c.send_raw(b" ") {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(c.drains_to_close());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "closed near the 300 ms deadline"
        );
        ui.finish();
    }

    #[test]
    fn a_slow_request_does_not_ask_for_a_repaint_when_nothing_waits() {
        // A budget of zero is "the handler took too long". With one request the queue is empty
        // afterwards, so there is nothing to come back for, and the wake count stays exact.
        let wakes = wait();
        let server = start(0, counter_wake(&wakes), None).unwrap();
        let (addr, token) = (server.addr, server.token.clone());
        let client = thread::spawn(move || {
            let mut c = Client::connect(addr);
            c.auth(&token).unwrap();
            c.call(1, "selection.get")
        });
        while wakes.load(Ordering::SeqCst) < 1 {
            thread::sleep(Duration::from_millis(2));
        }
        let mut session = Session::default();
        assert_eq!(
            server.poll_for(Duration::ZERO, &mut |line| session.handle_line(line)),
            1
        );
        assert!(client.join().unwrap()["result"].is_object());
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn poll_stops_at_the_time_budget_and_asks_for_a_repaint() {
        let wakes = wait();
        let server = start_with(0, counter_wake(&wakes), None, small_limits()).unwrap();
        let (addr, token) = (server.addr, server.token.clone());
        let clients: Vec<_> = (0..3)
            .map(|i| {
                let token = token.clone();
                thread::spawn(move || {
                    let mut c = Client::connect(addr);
                    c.auth(&token).unwrap();
                    c.call(i, "selection.get")
                })
            })
            .collect();
        // Wait until all three requests are queued: each one wakes the UI.
        while wakes.load(Ordering::SeqCst) < 3 {
            thread::sleep(Duration::from_millis(5));
        }
        let mut session = Session::default();
        let before = wakes.load(Ordering::SeqCst);
        let mut handler = |line: &str| session.handle_line(line);
        assert_eq!(
            server.poll_for(Duration::ZERO, &mut handler),
            1,
            "one request per call with no budget"
        );
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            before + 1,
            "the UI is asked to come back"
        );
        assert_eq!(server.poll_for(Duration::from_secs(5), &mut handler), 2);
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            before + 1,
            "no repaint request when everything was answered"
        );
        for c in clients {
            assert!(c.join().unwrap()["result"].is_object());
        }
    }

    #[test]
    fn runtime_file_is_written_private_and_removed_on_drop() {
        let dir = std::env::temp_dir().join(format!("cadektor-agent-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("cadektor").join("agent.json");
        let server = start(0, Arc::new(|| {}), Some(path.clone())).unwrap();
        let info = sys_agent::read_agent_info(&path).unwrap();
        assert_eq!(
            info,
            AgentInfo {
                port: server.addr.port(),
                token: server.token.clone()
            }
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let addr = server.addr;
        drop(server);
        assert!(!path.exists(), "the file is deleted on exit");

        // A file that another instance wrote is left alone.
        let server = start(0, Arc::new(|| {}), Some(path.clone())).unwrap();
        sys_agent::write_agent_info(
            &path,
            &AgentInfo {
                port: 1,
                token: "other".into(),
            },
        )
        .unwrap();
        drop(server);
        assert_eq!(sys_agent::read_agent_info(&path).unwrap().token, "other");
        // After the server is dropped the port stops accepting real clients.
        thread::sleep(Duration::from_millis(100));
        let mut c = TcpStream::connect(addr).ok().inspect(|s| {
            s.set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
        });
        if let Some(s) = c.as_mut() {
            let _ = writeln!(s, "{{}}");
            let mut buf = [0u8; 8];
            assert!(!matches!(s.read(&mut buf), Ok(n) if n > 0));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_runtime_file_that_cannot_be_written_fails_the_start() {
        let dir = std::env::temp_dir().join(format!("cadektor-agent-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("file");
        std::fs::write(&blocker, "x").unwrap();
        assert!(
            start(
                0,
                Arc::new(|| {}),
                Some(blocker.join("sub").join("agent.json"))
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn capped_line_reader() {
        let read = |data: &[u8], max| read_line_capped(&mut BufReader::new(data), max);
        assert_eq!(read(b"abc\nrest", 10).unwrap().as_deref(), Some("abc"));
        assert_eq!(read(b"abc\r\n", 10).unwrap().as_deref(), Some("abc"));
        assert_eq!(
            read(b"abc", 10).unwrap().as_deref(),
            Some("abc"),
            "last line without a line end"
        );
        assert_eq!(read(b"", 10).unwrap(), None);
        assert_eq!(
            read(b"abcde\n", 5).unwrap().as_deref(),
            Some("abcde"),
            "exactly at the limit"
        );
        assert_eq!(
            read(b"abcdef\n", 5).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            read(b"abcdef", 5).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            read(&[0xff, b'\n'], 5).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
