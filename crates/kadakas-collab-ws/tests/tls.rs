//! wss: a TLS server with a certificate of our own, and the client that trusts it.

#![cfg(not(target_arch = "wasm32"))]

use std::net::TcpListener;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use kadakas_collab::protocol::ClientMsg;
use kadakas_collab::transport::{Endpoint, Transport};
use kadakas_collab_ws::WsEndpoint;
use kadakas_core::uid::Uid;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tungstenite::Message;

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A server that accepts one TLS WebSocket and sends what the client's first frame was to `out`.
fn tls_server(cert_der: Vec<u8>, key_der: Vec<u8>) -> (u16, mpsc::Receiver<String>) {
    let cfg = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert_der.into()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
        )
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let conn = rustls::ServerConnection::new(Arc::new(cfg)).unwrap();
        let stream = rustls::StreamOwned::new(conn, tcp);
        let mut ws = tungstenite::accept(stream).unwrap();
        if let Ok(Message::Text(t)) = ws.read() {
            let _ = tx.send(t.as_str().to_string());
        }
        let _ = ws.close(None);
        let _ = ws.flush();
    });
    (port, rx)
}

fn hello() -> ClientMsg {
    ClientMsg::Hello {
        protocol: 1,
        user: Uid::from_num(1),
        client: Uid::from_num(2),
        name: "Ann".into(),
        token: String::new(),
        last_seq: None,
        log_id: None,
        last_hash: None,
    }
}

/// Reads until the transport ends. Returns the reason.
fn failure_of(t: &mut dyn Transport) -> String {
    let end = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if t.recv().is_err() {
            return t.failure().unwrap_or_default();
        }
        assert!(std::time::Instant::now() < end, "the transport never ended");
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn make_cert() -> (Vec<u8>, Vec<u8>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    (cert.cert.der().to_vec(), cert.signing_key.serialize_der())
}

#[test]
fn wss_works_with_a_trusted_certificate_and_fails_without() {
    let (der, key) = make_cert();

    // A client that does not trust the certificate must refuse it.
    let (port, _rx) = tls_server(der.clone(), key.clone());
    let url = format!("wss://localhost:{port}/ws/doc");
    let mut t = WsEndpoint::new(&url, None).unwrap().connect().unwrap();
    let why = failure_of(&mut *t);
    assert!(
        why.to_lowercase().contains("cert") || why.contains("TLS"),
        "{why}"
    );

    // A client that trusts it connects, and its frames arrive.
    let (port, rx) = tls_server(der.clone(), key);
    let url = format!("wss://localhost:{port}/ws/doc");
    let mut ep = WsEndpoint::new(&url, Some("tok"))
        .unwrap()
        .with_root_certificate_der(der);
    let mut t = ep.connect().expect("wss starts");
    t.send(&hello()).unwrap();
    let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(got.contains("\"type\":\"hello\""), "{got}");
    t.close();
}

#[test]
fn a_certificate_for_another_host_name_is_refused() {
    // The certificate is for "localhost" and the client trusts it, but it connects to the IP
    // address 127.0.0.1: the name does not match, so the connection must fail.
    let (der, key) = make_cert();
    let (port, rx) = tls_server(der.clone(), key);
    let url = format!("wss://127.0.0.1:{port}/ws/doc");
    let mut t = WsEndpoint::new(&url, Some("tok"))
        .unwrap()
        .with_root_certificate_der(der)
        .connect()
        .unwrap();
    let _ = t.send(&hello());
    let why = failure_of(&mut *t);
    assert!(!why.is_empty());
    // Nothing reached the server.
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
}
