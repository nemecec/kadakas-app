//! wss: a TLS server with a certificate of our own, and the client that trusts it.

#![cfg(not(target_arch = "wasm32"))]

use std::net::TcpListener;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use kadakas_collab::protocol::ClientMsg;
use kadakas_collab::transport::Endpoint;
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

#[test]
fn wss_works_with_a_trusted_certificate_and_fails_without() {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let der = cert.cert.der().to_vec();
    let key = cert.signing_key.serialize_der();

    // A client that does not trust the certificate must refuse it.
    let (port, _rx) = tls_server(der.clone(), key.clone());
    let url = format!("wss://localhost:{port}/ws/doc");
    let err = WsEndpoint::new(&url, None)
        .unwrap()
        .connect()
        .err()
        .expect("an unknown certificate is refused")
        .to_string();
    assert!(!err.is_empty());

    // A client that trusts it connects, and its frames arrive.
    let (port, rx) = tls_server(der.clone(), key);
    let url = format!("wss://localhost:{port}/ws/doc");
    let mut ep = WsEndpoint::new(&url, Some("tok"))
        .unwrap()
        .with_root_certificate_der(der);
    let mut t = ep.connect().expect("wss connects");
    t.send(&hello()).unwrap();
    let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(got.contains("\"type\":\"hello\""), "{got}");
    t.close();
}
