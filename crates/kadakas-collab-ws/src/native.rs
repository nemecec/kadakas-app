//! The native WebSocket: a blocking client and one thread per connection.
//!
//! `connect` returns at once. The thread of the connection does everything that can take time:
//! the name lookup, the TCP connection, the TLS handshake and the upgrade, all inside one total
//! deadline ([`CONNECT_DEADLINE`]). `send` queues a message (it waits in the queue until the
//! socket is open), and `recv` gives `None` until a message comes. If the connection cannot be
//! opened, `recv` gives `Closed` (or `Refused` for HTTP 401 and 403), and `failure()` says why.
//!
//! After the upgrade the socket does not block. On Unix the thread waits in `poll(2)` for the
//! socket or for a wake-up byte (a `send` or a `close` writes it), with a limit of 250 ms for the
//! timers (ping and dead-peer check). Elsewhere it looks every 5 ms.

use std::fmt;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kadakas_collab::op::{MAX_FRAME_BYTES, MAX_SNAPSHOT_BYTES};
use kadakas_collab::protocol::{ClientMsg, ServerMsg};
use kadakas_collab::transport::{Endpoint, Transport, TransportError};
use rustls::pki_types::{CertificateDer, ServerName};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::{CollabUrl, SUBPROTOCOL, TOKEN_SUBPROTOCOL_PREFIX, TokenMode, UrlError};

/// One total limit for the name lookup, the TCP connection, the TLS handshake and the upgrade.
pub const CONNECT_DEADLINE: Duration = Duration::from_secs(10);
/// The longest wait of the thread between two looks at its timers.
const TICK: Duration = Duration::from_millis(250);
/// A ping this often keeps proxies from closing a quiet connection.
// Short in the unit tests of this file, so that they can see a ping and a dead peer.
const PING_EVERY: Duration = if cfg!(test) {
    Duration::from_millis(100)
} else {
    Duration::from_secs(20)
};
/// A connection that has been silent this long (the server answers pings) is dead.
const DEAD_AFTER: Duration = if cfg!(test) {
    Duration::from_millis(700)
} else {
    Duration::from_secs(75)
};
/// How many frames may wait for the reader. The thread stops reading when the queue is full.
const FRAME_QUEUE: usize = 4096;
/// How long `close` waits for the thread (to send `bye` and the close frame).
const CLOSE_WAIT: Duration = Duration::from_secs(1);

enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Stream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Stream::Plain(s) => s,
            Stream::Tls(s) => &s.sock,
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

type Socket = WebSocket<Stream>;

fn ws_config() -> WebSocketConfig {
    // Snapshots are the biggest frames. The frame limit for everything else is checked on send.
    // The server is the one that must hold the limit for what it receives.
    WebSocketConfig::default()
        .max_message_size(Some(MAX_SNAPSHOT_BYTES + 1024))
        .max_frame_size(None)
}

/// Why one try to open the socket failed.
#[derive(Debug)]
enum OpenError {
    /// The server answered the upgrade with an HTTP status (not 101).
    Http(u16),
    Other(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Http(c @ (401 | 403)) => write!(
                f,
                "the server refused the token or the access (HTTP {c}): check the token and the document"
            ),
            OpenError::Http(404) => f.write_str("the server has no such document (HTTP 404)"),
            OpenError::Http(c) => write!(f, "the server refused the connection (HTTP {c})"),
            OpenError::Other(m) => f.write_str(m),
        }
    }
}

fn tls_config(extra_roots: &[Vec<u8>]) -> Result<Arc<rustls::ClientConfig>, String> {
    static DEFAULT: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    let build = |extra: &[Vec<u8>]| -> Result<Arc<rustls::ClientConfig>, String> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        #[cfg(feature = "native-roots")]
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        for der in extra {
            roots
                .add(CertificateDer::from(der.clone()))
                .map_err(|e| format!("bad root certificate: {e}"))?;
        }
        let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Ok(Arc::new(cfg))
    };
    if extra_roots.is_empty() {
        if let Some(c) = DEFAULT.get() {
            return Ok(c.clone());
        }
        let c = build(&[])?;
        return Ok(DEFAULT.get_or_init(|| c).clone());
    }
    build(extra_roots)
}

/// What the thread needs to open a socket.
struct Target {
    url: CollabUrl,
    token: Option<String>,
    mode: TokenMode,
    roots: Vec<Vec<u8>>,
}

/// Shared between the thread and the transport.
#[derive(Default)]
struct Shared {
    ended: AtomicBool,
    /// The upgrade is done.
    connected: AtomicBool,
    /// The transport was closed or dropped: stop at the next step.
    stop: AtomicBool,
    failure: Mutex<Option<OpenFailure>>,
}

enum OpenFailure {
    Refused(String),
    Other(String),
}

impl Shared {
    fn set_failure(&self, f: OpenFailure) {
        let mut slot = self.failure.lock().unwrap();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// The time left until the deadline, or an error text.
fn remaining(deadline: Instant) -> Result<Duration, OpenError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| {
            OpenError::Other(format!(
                "no connection in {} seconds",
                CONNECT_DEADLINE.as_secs()
            ))
        })
}

fn resolve(url: &CollabUrl, shared: &Shared) -> Result<Vec<std::net::SocketAddr>, OpenError> {
    let host = url.bare_host();
    let addrs: Vec<_> = (host, url.port)
        .to_socket_addrs()
        .map_err(|e| OpenError::Other(format!("cannot find {host}: {e}")))?
        .collect();
    if shared.stop.load(Ordering::SeqCst) {
        return Err(OpenError::Other("closed".into()));
    }
    if addrs.is_empty() {
        return Err(OpenError::Other(format!("no address for {host}")));
    }
    Ok(addrs)
}

/// A token must not cross the network as plain text: with `ws://` it goes only to this computer,
/// and a name like `localhost` must resolve to addresses of this computer only.
fn check_token_destination(
    url: &CollabUrl,
    has_token: bool,
    addrs: &[std::net::SocketAddr],
) -> Result<(), OpenError> {
    if url.scheme != "ws" || !has_token {
        return Ok(());
    }
    let all_loopback = addrs.iter().all(|a| ip_is_loopback(a.ip()));
    if !url.is_loopback() || !all_loopback {
        return Err(OpenError::Other(
            "the token is not sent over ws:// to a server that is not on this computer: use wss://"
                .into(),
        ));
    }
    Ok(())
}

fn ip_is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => a.is_loopback(),
        IpAddr::V6(a) => a.is_loopback() || a.to_ipv4_mapped().is_some_and(|v| v.is_loopback()),
    }
}

fn tcp_connect(
    addrs: &[std::net::SocketAddr],
    deadline: Instant,
    shared: &Shared,
) -> Result<TcpStream, OpenError> {
    let mut last = String::from("no address");
    for addr in addrs {
        if shared.stop.load(Ordering::SeqCst) {
            return Err(OpenError::Other("closed".into()));
        }
        let left = remaining(deadline)?;
        match TcpStream::connect_timeout(addr, left.min(Duration::from_secs(5))) {
            Ok(s) => return Ok(s),
            Err(e) => last = format!("cannot connect to {}:{}: {e}", addr.ip(), addr.port()),
        }
    }
    Err(OpenError::Other(last))
}

/// Opens the socket and does the upgrade. `mode` is `Header` or `Subprotocol` here.
fn open(
    target: &Target,
    mode: TokenMode,
    deadline: Instant,
    shared: &Shared,
) -> Result<Socket, OpenError> {
    let url = &target.url;
    let other = OpenError::Other;
    let addrs = resolve(url, shared)?;
    check_token_destination(url, target.token.is_some(), &addrs)?;
    let tcp = tcp_connect(&addrs, deadline, shared)?;
    let _ = tcp.set_nodelay(true);
    let left = remaining(deadline)?;
    tcp.set_read_timeout(Some(left))
        .and_then(|()| tcp.set_write_timeout(Some(left)))
        .map_err(|e| other(e.to_string()))?;
    let stream = if url.scheme == "wss" {
        let cfg = tls_config(&target.roots).map_err(other)?;
        let name = ServerName::try_from(url.bare_host().to_string())
            .map_err(|e| OpenError::Other(format!("bad host name: {e}")))?;
        let conn = rustls::ClientConnection::new(cfg, name)
            .map_err(|e| OpenError::Other(format!("TLS: {e}")))?;
        Stream::Tls(Box::new(rustls::StreamOwned::new(conn, tcp)))
    } else {
        Stream::Plain(tcp)
    };
    // The address is rebuilt from its parts: nothing but scheme, host, port and path.
    let target_url = format!("{}://{}:{}{}", url.scheme, url.host, url.port, url.path);
    let mut request = target_url
        .as_str()
        .into_client_request()
        .map_err(|e| other(e.to_string()))?;
    let bad_token =
        || OpenError::Other("the token has characters that a header cannot carry".into());
    if let Some(t) = target.token.as_deref().filter(|t| !t.is_empty()) {
        match mode {
            TokenMode::Subprotocol => {
                let value = format!("{SUBPROTOCOL}, {TOKEN_SUBPROTOCOL_PREFIX}{t}");
                request.headers_mut().insert(
                    HeaderName::from_static("sec-websocket-protocol"),
                    HeaderValue::from_str(&value).map_err(|_| bad_token())?,
                );
            }
            _ => {
                let mut v =
                    HeaderValue::from_str(&format!("Bearer {t}")).map_err(|_| bad_token())?;
                v.set_sensitive(true);
                request
                    .headers_mut()
                    .insert(HeaderName::from_static("authorization"), v);
            }
        }
    }
    let (ws, _response) =
        tungstenite::client::client_with_config(request, stream, Some(ws_config())).map_err(
            |e| match e {
                tungstenite::HandshakeError::Failure(tungstenite::Error::Http(r)) => {
                    OpenError::Http(r.status().as_u16())
                }
                // Never print the error with the request in it: only its text.
                tungstenite::HandshakeError::Failure(e) => OpenError::Other(e.to_string()),
                tungstenite::HandshakeError::Interrupted(_) => {
                    OpenError::Other("the handshake did not finish".into())
                }
            },
        )?;
    // From here on the socket does not block.
    ws.get_ref()
        .tcp()
        .set_nonblocking(true)
        .map_err(|e| other(e.to_string()))?;
    Ok(ws)
}

fn open_with_fallback(
    target: &Target,
    deadline: Instant,
    shared: &Shared,
) -> Result<Socket, OpenError> {
    let usable = target.token.as_deref().filter(|t| !t.is_empty());
    match (target.mode, usable) {
        (TokenMode::Auto, Some(t)) => match open(target, TokenMode::Header, deadline, shared) {
            // Only a server or a proxy that did not understand the request gets the second try
            // (400 bad request, 426 upgrade required, 431 headers too large). A 401 or 403
            // stays a refusal.
            Err(OpenError::Http(400 | 426 | 431)) if crate::token_fits_subprotocol(t) => {
                open(target, TokenMode::Subprotocol, deadline, shared)
            }
            other => other,
        },
        (TokenMode::Subprotocol, _) => open(target, TokenMode::Subprotocol, deadline, shared),
        _ => open(target, TokenMode::Header, deadline, shared),
    }
}

enum Cmd {
    Send(String),
    Close,
}

#[cfg(unix)]
mod wake {
    use std::os::fd::RawFd;

    /// A pipe: a byte in it wakes the thread that waits in `poll`.
    pub struct Waker {
        read: RawFd,
        write: RawFd,
    }

    impl Waker {
        pub fn new() -> Waker {
            let mut fds = [0 as RawFd; 2];
            // SAFETY: `fds` has room for the two descriptors that pipe fills in.
            let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
            assert_eq!(rc, 0, "cannot make a pipe");
            for fd in fds {
                // SAFETY: plain flag calls on descriptors that we own.
                unsafe {
                    let fl = libc::fcntl(fd, libc::F_GETFL);
                    libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
                    libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                }
            }
            Waker {
                read: fds[0],
                write: fds[1],
            }
        }

        pub fn wake(&self) {
            // SAFETY: writes one byte from a valid buffer. A full pipe (EAGAIN) means a wake-up
            // is already waiting.
            unsafe {
                libc::write(self.write, [1u8].as_ptr().cast(), 1);
            }
        }

        fn drain(&self) {
            let mut buf = [0u8; 64];
            // SAFETY: reads into a valid buffer until the pipe is empty.
            while unsafe { libc::read(self.read, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
        }

        /// Waits until the socket can be read (or written, if `want_out`), a wake-up comes, or
        /// the time is over.
        pub fn wait(&self, socket: RawFd, want_out: bool, ms: i32) {
            let mut fds = [
                libc::pollfd {
                    fd: socket,
                    events: libc::POLLIN | if want_out { libc::POLLOUT } else { 0 },
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.read,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: `fds` is a valid array of two entries.
            unsafe {
                libc::poll(fds.as_mut_ptr(), 2, ms);
            }
            if fds[1].revents & libc::POLLIN != 0 {
                self.drain();
            }
        }
    }

    impl Drop for Waker {
        fn drop(&mut self) {
            // SAFETY: closes the two descriptors that `new` made.
            unsafe {
                libc::close(self.read);
                libc::close(self.write);
            }
        }
    }
}

#[cfg(not(unix))]
mod wake {
    pub struct Waker;

    impl Waker {
        pub fn new() -> Waker {
            Waker
        }

        pub fn wake(&self) {}

        pub fn wait(&self, _socket: (), _want_out: bool, _ms: i32) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

use wake::Waker;

#[cfg(unix)]
fn socket_id(ws: &Socket) -> std::os::fd::RawFd {
    use std::os::fd::AsRawFd;
    ws.get_ref().tcp().as_raw_fd()
}

#[cfg(not(unix))]
fn socket_id(_ws: &Socket) {}

/// One WebSocket connection. A thread moves the frames; the trait calls never wait.
pub struct WsTransport {
    tx: Sender<Cmd>,
    rx: Receiver<String>,
    shared: Arc<Shared>,
    waker: Arc<Waker>,
    thread: Option<JoinHandle<()>>,
}

impl WsTransport {
    /// Starts to connect and returns at once. The full address of the document is
    /// `ws://127.0.0.1:8787/ws/<document id>`.
    pub fn connect(url: &str, token: Option<&str>) -> Result<WsTransport, TransportError> {
        let ep = WsEndpoint::new(url, token).map_err(|e| TransportError::Failed(e.to_string()))?;
        Ok(ep.connect_now())
    }

    fn start(target: Target) -> WsTransport {
        let (tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (frame_tx, rx) = mpsc::sync_channel::<String>(FRAME_QUEUE);
        let shared = Arc::new(Shared::default());
        let waker = Arc::new(Waker::new());
        let (s, w) = (shared.clone(), waker.clone());
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + CONNECT_DEADLINE;
            match open_with_fallback(&target, deadline, &s) {
                Ok(ws) => {
                    s.connected.store(true, Ordering::SeqCst);
                    io_loop(ws, &cmd_rx, &frame_tx, &s, &w);
                }
                Err(OpenError::Http(c @ (401 | 403))) => {
                    s.set_failure(OpenFailure::Refused(OpenError::Http(c).to_string()));
                }
                Err(e) => s.set_failure(OpenFailure::Other(e.to_string())),
            }
            s.ended.store(true, Ordering::SeqCst);
        });
        WsTransport {
            tx,
            rx,
            shared,
            waker,
            thread: Some(thread),
        }
    }

    fn end_error(&self) -> TransportError {
        match &*self.shared.failure.lock().unwrap() {
            Some(OpenFailure::Refused(m)) => TransportError::Refused(m.clone()),
            _ => TransportError::Closed,
        }
    }
}

/// Puts a frame in the queue. Waits while the queue is full (the reader is slow), but not after a
/// stop. False if the transport is gone.
fn push_frame(frames: &SyncSender<String>, shared: &Shared, mut text: String) -> bool {
    loop {
        match frames.try_send(text) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(t)) => {
                if shared.stop.load(Ordering::SeqCst) {
                    return false;
                }
                text = t;
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

fn is_would_block(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(e)
        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

fn io_loop(
    mut ws: Socket,
    cmds: &Receiver<Cmd>,
    frames: &SyncSender<String>,
    shared: &Shared,
    waker: &Waker,
) {
    let socket = socket_id(&ws);
    let mut last_heard = Instant::now();
    let mut last_ping = Instant::now();
    'run: loop {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        // Commands: messages to send, or the end.
        loop {
            match cmds.try_recv() {
                Ok(Cmd::Send(text)) => match ws.send(Message::text(text)) {
                    Ok(()) => {}
                    Err(e) if is_would_block(&e) => {}
                    Err(_) => break 'run,
                },
                Ok(Cmd::Close) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    break 'run;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break 'run,
            }
        }
        // Frames from the server, until the socket has nothing more.
        loop {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    last_heard = Instant::now();
                    if !push_frame(frames, shared, t.as_str().to_string()) {
                        break 'run;
                    }
                }
                Ok(Message::Close(_)) => break 'run,
                Ok(_) => last_heard = Instant::now(),
                Err(e) if is_would_block(&e) => break,
                Err(_) => break 'run,
            }
        }
        // Pongs and half-written frames.
        let write_pending = match ws.flush() {
            Ok(()) => false,
            Err(e) if is_would_block(&e) => true,
            Err(_) => break,
        };
        if last_ping.elapsed() >= PING_EVERY {
            last_ping = Instant::now();
            match ws.send(Message::Ping(Default::default())) {
                Ok(()) => {}
                Err(e) if is_would_block(&e) => {}
                Err(_) => break,
            }
        }
        if last_heard.elapsed() >= DEAD_AFTER {
            break;
        }
        waker.wait(socket, write_pending, TICK.as_millis() as i32);
    }
}

impl Transport for WsTransport {
    fn send(&mut self, msg: &ClientMsg) -> Result<(), TransportError> {
        if self.shared.ended.load(Ordering::SeqCst) {
            return Err(TransportError::Closed);
        }
        let text = serde_json::to_string(msg).expect("a message is JSON");
        if text.len() > MAX_FRAME_BYTES {
            return Err(TransportError::Failed(format!(
                "a frame of {} bytes is above the limit of {MAX_FRAME_BYTES}",
                text.len()
            )));
        }
        self.tx
            .send(Cmd::Send(text))
            .map_err(|_| TransportError::Closed)?;
        self.waker.wake();
        Ok(())
    }

    fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError> {
        let next = match self.rx.try_recv() {
            Ok(t) => Some(t),
            Err(_) if self.shared.ended.load(Ordering::SeqCst) => {
                // A frame may have come between the two looks.
                match self.rx.try_recv() {
                    Ok(t) => Some(t),
                    Err(_) => return Err(self.end_error()),
                }
            }
            Err(_) => None,
        };
        match next {
            Some(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| TransportError::BadMessage(e.to_string())),
            None => Ok(None),
        }
    }

    fn close(&mut self) {
        let _ = self.tx.send(Cmd::Close);
        self.waker.wake();
        // Give the thread a moment to send `bye` and the close frame. A thread that is still in
        // the name lookup is left alone: it stops at its next step.
        let end = Instant::now() + CLOSE_WAIT;
        while self.shared.connected.load(Ordering::SeqCst)
            && !self.shared.ended.load(Ordering::SeqCst)
            && Instant::now() < end
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        self.shared.stop.store(true, Ordering::SeqCst);
        if self.shared.ended.load(Ordering::SeqCst) {
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        } else {
            self.thread.take();
        }
    }

    fn failure(&self) -> Option<String> {
        match &*self.shared.failure.lock().unwrap() {
            Some(OpenFailure::Refused(m) | OpenFailure::Other(m)) => Some(m.clone()),
            None => None,
        }
    }
}

impl Drop for WsTransport {
    fn drop(&mut self) {
        // The thread ends at its next step; do not wait for it.
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::Close);
        self.waker.wake();
    }
}

/// Makes WebSocket connections to one document.
#[derive(Clone)]
pub struct WsEndpoint {
    url: CollabUrl,
    token: Option<String>,
    mode: TokenMode,
    roots: Vec<Vec<u8>>,
}

impl fmt::Debug for WsEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WsEndpoint")
            .field("url", &self.url.display())
            .field("token", &self.token.as_ref().map(|_| "<hidden>"))
            .field("mode", &self.mode)
            .finish()
    }
}

impl WsEndpoint {
    /// `url` is the full address (`ws://host:port/ws/<document id>`). `token` is sent with the
    /// upgrade request (see [`TokenMode`]). A token never goes over `ws://` to a server that is
    /// not on this computer.
    pub fn new(url: &str, token: Option<&str>) -> Result<WsEndpoint, UrlError> {
        let url = CollabUrl::parse(url)?;
        let token = token.filter(|t| !t.is_empty()).map(str::to_string);
        if token.is_some() && url.scheme == "ws" && !url.is_loopback() {
            return Err(UrlError(
                "the token is not sent over ws:// to a server that is not on this computer: use wss://"
                    .into(),
            ));
        }
        Ok(WsEndpoint {
            url,
            token,
            mode: TokenMode::default(),
            roots: Vec::new(),
        })
    }

    pub fn with_token_mode(mut self, mode: TokenMode) -> WsEndpoint {
        self.mode = mode;
        self
    }

    /// Also trusts this root certificate (DER), for a server with a private CA.
    pub fn with_root_certificate_der(mut self, der: Vec<u8>) -> WsEndpoint {
        self.roots.push(der);
        self
    }

    pub fn url(&self) -> &CollabUrl {
        &self.url
    }

    fn connect_now(&self) -> WsTransport {
        WsTransport::start(Target {
            url: self.url.clone(),
            token: self.token.clone(),
            mode: self.mode,
            roots: self.roots.clone(),
        })
    }
}

impl Endpoint for WsEndpoint {
    fn describe(&self) -> String {
        self.url.display()
    }

    fn connect(&mut self) -> Result<Box<dyn Transport>, TransportError> {
        Ok(Box::new(self.connect_now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A WebSocket server for the first connection: `script` gets the open socket.
    fn ws_server(script: impl FnOnce(WebSocket<TcpStream>) + Send + 'static) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/ws/x", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            if let Ok(ws) = tungstenite::accept(s) {
                script(ws);
            }
        });
        url
    }

    /// A TCP server for the first connection, before any WebSocket handshake. Gives `host:port`.
    fn tcp_server(script: impl FnOnce(TcpStream) + Send + 'static) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            script(s);
        });
        format!("{addr}")
    }

    /// Reads until the transport ends and gives the end.
    fn end_of(t: &mut WsTransport) -> TransportError {
        let stop = Instant::now() + Duration::from_secs(10);
        loop {
            match t.recv() {
                Err(e) => return e,
                Ok(_) => assert!(Instant::now() < stop, "the transport never ended"),
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn ack(n: u64) -> Message {
        Message::text(format!(r#"{{"type":"ack","op_id":{n},"seq":{n}}}"#))
    }

    #[test]
    fn a_passed_deadline_is_an_error() {
        let e = remaining(Instant::now()).unwrap_err();
        assert_eq!(e.to_string(), "no connection in 10 seconds");
        assert!(remaining(Instant::now() + Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn open_errors_have_clear_texts() {
        assert!(
            OpenError::Http(404)
                .to_string()
                .contains("no such document")
        );
        assert!(OpenError::Http(500).to_string().contains("HTTP 500"));
        assert!(
            OpenError::Http(403)
                .to_string()
                .contains("refused the token")
        );
    }

    #[test]
    fn a_name_that_does_not_exist_and_a_stop_end_the_lookup() {
        let shared = Shared::default();
        let url = CollabUrl::parse("ws://no-such-host.invalid:9/ws/x").unwrap();
        let e = resolve(&url, &shared).unwrap_err();
        assert!(e.to_string().contains("cannot find"), "{e}");
        let url = CollabUrl::parse("ws://127.0.0.1:9/ws/x").unwrap();
        assert_eq!(resolve(&url, &shared).unwrap().len(), 1);
        shared.stop.store(true, Ordering::SeqCst);
        assert_eq!(resolve(&url, &shared).unwrap_err().to_string(), "closed");
    }

    #[test]
    fn tcp_connect_needs_an_address_and_obeys_a_stop() {
        let shared = Shared::default();
        let soon = Instant::now() + Duration::from_secs(5);
        let e = tcp_connect(&[], soon, &shared).unwrap_err();
        assert_eq!(e.to_string(), "no address");
        let addr: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
        shared.stop.store(true, Ordering::SeqCst);
        assert_eq!(
            tcp_connect(&[addr], soon, &shared).unwrap_err().to_string(),
            "closed"
        );
        let shared = Shared::default();
        let e = tcp_connect(&[addr], Instant::now(), &shared).unwrap_err();
        assert!(e.to_string().contains("no connection in"), "{e}");
    }

    #[test]
    fn a_token_goes_only_to_addresses_of_this_computer() {
        let ws = CollabUrl::parse("ws://localhost:9/ws/x").unwrap();
        let far: std::net::SocketAddr = "8.8.8.8:9".parse().unwrap();
        let near: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
        assert!(check_token_destination(&ws, true, &[near]).is_ok());
        assert!(check_token_destination(&ws, true, &[near, far]).is_err());
        assert!(check_token_destination(&ws, false, &[far]).is_ok());
        let wss = CollabUrl::parse("wss://localhost:9/ws/x").unwrap();
        assert!(check_token_destination(&wss, true, &[far]).is_ok());
        assert!(ip_is_loopback("::1".parse().unwrap()));
        assert!(ip_is_loopback("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!ip_is_loopback("::ffff:8.8.8.8".parse().unwrap()));
        assert!(!ip_is_loopback("2001:db8::1".parse().unwrap()));
        assert!(!ip_is_loopback("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn a_token_that_a_header_cannot_carry_is_not_sent() {
        for mode in [TokenMode::Header, TokenMode::Subprotocol] {
            let addr = tcp_server(|s| {
                std::thread::sleep(Duration::from_millis(300));
                drop(s);
            });
            let ep = WsEndpoint::new(&format!("ws://{addr}/ws/x"), Some("a\nb"))
                .unwrap()
                .with_token_mode(mode);
            let mut t = ep.connect_now();
            assert!(matches!(end_of(&mut t), TransportError::Closed));
            let why = t.failure().unwrap();
            assert!(why.contains("header cannot carry"), "{why}");
            assert!(!why.contains("a\nb"));
        }
    }

    #[test]
    fn a_bad_root_certificate_is_an_error_and_the_default_roots_are_kept() {
        let addr = tcp_server(drop);
        let ep = WsEndpoint::new(&format!("wss://{addr}/ws/x"), None)
            .unwrap()
            .with_root_certificate_der(vec![1, 2, 3]);
        assert_eq!(ep.url().scheme, "wss");
        let mut t = ep.connect_now();
        assert!(matches!(end_of(&mut t), TransportError::Closed));
        assert!(t.failure().unwrap().contains("bad root certificate"));
        // Without extra roots, the shared default configuration is built once and used again.
        for _ in 0..2 {
            let addr = tcp_server(drop);
            let mut t = WsEndpoint::new(&format!("wss://{addr}/ws/x"), None)
                .unwrap()
                .connect_now();
            assert!(matches!(end_of(&mut t), TransportError::Closed));
            assert!(t.failure().is_some());
        }
    }

    #[test]
    fn a_server_that_does_not_speak_websocket_is_an_error() {
        let addr = tcp_server(|mut s| {
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        });
        let mut t = WsTransport::connect(&format!("ws://{addr}/ws/x"), None).unwrap();
        assert!(matches!(end_of(&mut t), TransportError::Closed));
        assert!(t.failure().is_some());
    }

    #[test]
    fn frames_of_other_kinds_are_skipped_and_a_bad_text_is_reported() {
        let url = ws_server(|mut ws| {
            ws.send(Message::binary(vec![1, 2, 3])).unwrap();
            ws.send(Message::Ping(Default::default())).unwrap();
            ws.send(Message::text("not json")).unwrap();
            ws.send(ack(7)).unwrap();
            ws.send(Message::Close(None)).unwrap();
            let _ = ws.flush();
            // Wait for the close handshake.
            while ws.read().is_ok() {}
        });
        let mut t = WsTransport::connect(&url, None).unwrap();
        let mut got = Vec::new();
        let stop = Instant::now() + Duration::from_secs(10);
        let end = loop {
            match t.recv() {
                Ok(Some(m)) => got.push(Ok(m)),
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(TransportError::BadMessage(m)) => got.push(Err(m)),
                Err(e) => break e,
            }
            assert!(Instant::now() < stop, "the transport never ended");
        };
        assert!(matches!(end, TransportError::Closed));
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].is_err());
        assert!(matches!(&got[1], Ok(ServerMsg::Ack { op_id: 7, .. })));
        // A send after the end is refused.
        assert!(matches!(
            t.send(&ClientMsg::Bye),
            Err(TransportError::Closed)
        ));
    }

    #[test]
    fn a_slow_reader_gets_every_frame_of_a_flood() {
        const N: u64 = 30_000;
        let url = ws_server(|mut ws| {
            for n in 0..N {
                if ws.send(ack(n)).is_err() {
                    return;
                }
            }
            let _ = ws.close(None);
            let _ = ws.flush();
            while ws.read().is_ok() {}
        });
        let mut t = WsTransport::connect(&url, None).unwrap();
        // The queue holds FRAME_QUEUE frames. The thread waits when it is full.
        std::thread::sleep(Duration::from_millis(500));
        let mut next = 0;
        let stop = Instant::now() + Duration::from_secs(60);
        loop {
            match t.recv() {
                Ok(Some(ServerMsg::Ack { op_id, .. })) => {
                    assert_eq!(op_id, next, "frames stay in order");
                    next += 1;
                }
                Ok(Some(m)) => panic!("{m:?}"),
                Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                Err(_) => break,
            }
            assert!(Instant::now() < stop, "stuck at {next}");
        }
        assert_eq!(next, N);
    }

    #[test]
    fn a_server_that_vanishes_ends_the_transport() {
        let url = ws_server(drop);
        let mut t = WsTransport::connect(&url, None).unwrap();
        assert!(matches!(end_of(&mut t), TransportError::Closed));
        assert!(matches!(
            t.send(&ClientMsg::Bye),
            Err(TransportError::Closed)
        ));
    }

    #[test]
    fn a_quiet_connection_is_pinged_and_a_silent_one_is_dropped() {
        // The server reads, so it sees the pings (and answers them).
        let (tx, rx) = mpsc::channel();
        let url = ws_server(move |mut ws| {
            let _ = ws
                .get_ref()
                .set_read_timeout(Some(Duration::from_millis(20)));
            let stop = Instant::now() + Duration::from_secs(5);
            while Instant::now() < stop {
                match ws.read() {
                    Ok(Message::Ping(_)) => {
                        let _ = ws.flush();
                        let _ = tx.send(());
                        break;
                    }
                    Ok(_) | Err(tungstenite::Error::Io(_)) => {}
                    Err(_) => break,
                }
            }
            std::thread::sleep(Duration::from_millis(300));
        });
        let mut t = WsTransport::connect(&url, None).unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("a ping came");
        t.close();
        // The server never reads: it answers no ping, and the client gives up.
        let url = ws_server(|ws| {
            std::thread::sleep(Duration::from_secs(3));
            drop(ws);
        });
        let mut t = WsTransport::connect(&url, None).unwrap();
        assert!(matches!(end_of(&mut t), TransportError::Closed));
    }

    #[test]
    fn close_tells_the_server_and_the_endpoint_describes_itself() {
        let (tx, rx) = mpsc::channel();
        let url = ws_server(move |mut ws| {
            let mut texts = Vec::new();
            while let Ok(m) = ws.read() {
                if let Message::Text(t) = m {
                    texts.push(t.as_str().to_string());
                }
            }
            let _ = tx.send(texts);
        });
        let ep = WsEndpoint::new(&url, None).unwrap();
        assert_eq!(ep.describe(), url);
        let mut t = ep.connect_now();
        t.send(&ClientMsg::Bye).unwrap();
        // Wait for the socket to open, so the close goes out after `bye`.
        let stop = Instant::now() + Duration::from_secs(10);
        while !t.shared.connected.load(Ordering::SeqCst) {
            assert!(Instant::now() < stop);
            std::thread::sleep(Duration::from_millis(2));
        }
        t.close();
        let texts = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(texts.len(), 1, "{texts:?}");
        assert!(texts[0].contains("bye"));
    }
}
