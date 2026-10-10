//! The native WebSocket: a blocking client and one thread per connection.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, OnceLock};
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

/// The time to open the TCP connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The time for the upgrade request (and the TLS handshake with it).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the I/O thread waits for a frame before it looks at the commands again.
const POLL: Duration = Duration::from_millis(5);
/// A ping this often keeps proxies from closing a quiet connection.
const PING_EVERY: Duration = Duration::from_secs(20);
/// A connection that has been silent this long (the server answers pings) is dead.
const DEAD_AFTER: Duration = Duration::from_secs(75);

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
enum OpenError {
    /// The server answered the upgrade with an HTTP status (not 101).
    Http(u16),
    Other(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Http(401 | 403) => f.write_str(
                "the server refused the token (HTTP 401): check the token and the document",
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

fn tcp_connect(url: &CollabUrl) -> Result<TcpStream, String> {
    let host = url.bare_host();
    let addrs = (host, url.port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot find {host}: {e}"))?;
    let mut last = format!("no address for {host}");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last = format!("cannot connect to {}:{}: {e}", host, url.port),
        }
    }
    Err(last)
}

/// Opens the socket and does the upgrade. `mode` is `Header` or `Subprotocol` here.
fn open(
    url: &CollabUrl,
    token: Option<&str>,
    mode: TokenMode,
    roots: &[Vec<u8>],
) -> Result<Socket, OpenError> {
    let other = OpenError::Other;
    let tcp = tcp_connect(url).map_err(other)?;
    let _ = tcp.set_nodelay(true);
    tcp.set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .and_then(|()| tcp.set_write_timeout(Some(Duration::from_secs(30))))
        .map_err(|e| OpenError::Other(e.to_string()))?;
    let stream = if url.scheme == "wss" {
        let cfg = tls_config(roots).map_err(OpenError::Other)?;
        let name = ServerName::try_from(url.bare_host().to_string())
            .map_err(|e| OpenError::Other(format!("bad host name: {e}")))?;
        let conn = rustls::ClientConnection::new(cfg, name)
            .map_err(|e| OpenError::Other(format!("TLS: {e}")))?;
        Stream::Tls(Box::new(rustls::StreamOwned::new(conn, tcp)))
    } else {
        Stream::Plain(tcp)
    };
    // The address is rebuilt from its parts: nothing but scheme, host, port and path.
    let target = format!("{}://{}:{}{}", url.scheme, url.host, url.port, url.path);
    let mut request = target
        .as_str()
        .into_client_request()
        .map_err(|e| OpenError::Other(e.to_string()))?;
    let bad_token =
        || OpenError::Other("the token has characters that a header cannot carry".into());
    if let Some(t) = token.filter(|t| !t.is_empty()) {
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
    let _ = ws.get_ref().tcp().set_read_timeout(Some(POLL));
    Ok(ws)
}

fn open_with_fallback(
    url: &CollabUrl,
    token: Option<&str>,
    mode: TokenMode,
    roots: &[Vec<u8>],
) -> Result<Socket, OpenError> {
    let usable = token.filter(|t| !t.is_empty());
    match (mode, usable) {
        (TokenMode::Auto, Some(t)) => match open(url, token, TokenMode::Header, roots) {
            // A proxy may remove the header: try the sub-protocol once.
            Err(OpenError::Http(_)) if crate::token_fits_subprotocol(t) => {
                open(url, token, TokenMode::Subprotocol, roots)
            }
            other => other,
        },
        (m, _) => open(url, token, m, roots),
    }
}

enum Cmd {
    Send(String),
    Close,
}

/// One WebSocket connection. A thread moves the frames; the trait calls never wait.
pub struct WsTransport {
    tx: Sender<Cmd>,
    rx: Receiver<String>,
    ended: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WsTransport {
    /// Connects now (this call waits for the upgrade). The `url` is the full address of the
    /// document, for example `ws://127.0.0.1:8787/ws/<document id>`.
    pub fn connect(url: &str, token: Option<&str>) -> Result<WsTransport, TransportError> {
        let ep = WsEndpoint::new(url, token).map_err(|e| TransportError::Failed(e.to_string()))?;
        ep.connect_now()
    }

    fn start(ws: Socket) -> WsTransport {
        let (tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (frame_tx, rx) = mpsc::channel::<String>();
        let ended = Arc::new(AtomicBool::new(false));
        let flag = ended.clone();
        let thread = std::thread::spawn(move || {
            io_loop(ws, cmd_rx, frame_tx);
            flag.store(true, Ordering::SeqCst);
        });
        WsTransport {
            tx,
            rx,
            ended,
            thread: Some(thread),
        }
    }
}

fn io_loop(mut ws: Socket, cmds: Receiver<Cmd>, frames: Sender<String>) {
    let mut last_heard = Instant::now();
    let mut last_ping = Instant::now();
    'run: loop {
        loop {
            match cmds.try_recv() {
                Ok(Cmd::Send(text)) => {
                    if ws.send(Message::text(text)).is_err() {
                        break 'run;
                    }
                }
                Ok(Cmd::Close) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    break 'run;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break 'run,
            }
        }
        if last_ping.elapsed() >= PING_EVERY {
            last_ping = Instant::now();
            if ws.send(Message::Ping(Default::default())).is_err() {
                break;
            }
        }
        if last_heard.elapsed() >= DEAD_AFTER {
            break;
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                last_heard = Instant::now();
                if frames.send(t.as_str().to_string()).is_err() {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => last_heard = Instant::now(),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // Sends the pong that the read queued.
                let _ = ws.flush();
            }
            Err(_) => break,
        }
    }
}

impl Transport for WsTransport {
    fn send(&mut self, msg: &ClientMsg) -> Result<(), TransportError> {
        if self.ended.load(Ordering::SeqCst) {
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
            .map_err(|_| TransportError::Closed)
    }

    fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError> {
        match self.rx.try_recv() {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| TransportError::BadMessage(e.to_string())),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(TransportError::Closed),
        }
    }

    fn close(&mut self) {
        let _ = self.tx.send(Cmd::Close);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WsTransport {
    fn drop(&mut self) {
        // The thread ends when the command channel is gone; do not wait for it.
        let _ = self.tx.send(Cmd::Close);
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
    /// upgrade request (see [`TokenMode`]).
    pub fn new(url: &str, token: Option<&str>) -> Result<WsEndpoint, UrlError> {
        Ok(WsEndpoint {
            url: CollabUrl::parse(url)?,
            token: token.filter(|t| !t.is_empty()).map(str::to_string),
            mode: TokenMode::Auto,
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

    fn connect_now(&self) -> Result<WsTransport, TransportError> {
        let ws = open_with_fallback(&self.url, self.token.as_deref(), self.mode, &self.roots)
            .map_err(|e| TransportError::Failed(format!("{}: {e}", self.url.display())))?;
        Ok(WsTransport::start(ws))
    }
}

impl Endpoint for WsEndpoint {
    fn describe(&self) -> String {
        self.url.display()
    }

    fn connect(&mut self) -> Result<Box<dyn Transport>, TransportError> {
        Ok(Box::new(self.connect_now()?))
    }
}
