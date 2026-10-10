//! A small HTTP server for the tests. It listens on 127.0.0.1 on a free port and answers each
//! request from a closure, so no test needs the real network.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum Reply {
    /// Status 200 with a content type and a body.
    Ok(&'static str, Vec<u8>),
    /// Any status with a text body.
    Status(u16, &'static str),
    /// A redirect to the given address.
    Redirect(String),
    /// Wait, then answer 200 with "late".
    Slow(Duration),
    /// Close the connection without an answer.
    Hang,
}

pub struct Mock {
    pub base: String,
    pub hits: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Mock {
    pub fn start<F>(handler: F) -> Mock
    where
        F: Fn(&str) -> Reply + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);
        let (h2, s2) = (hits.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (handler, hits) = (handler.clone(), h2.clone());
                        std::thread::spawn(move || serve(stream, &*handler, &hits));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Mock {
            base: format!("http://127.0.0.1:{port}"),
            hits,
            stop,
        }
    }

    pub fn hit_count(&self) -> usize {
        self.hits.lock().unwrap().len()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn serve(stream: TcpStream, handler: &dyn Fn(&str) -> Reply, hits: &Mutex<Vec<String>>) {
    let _ = stream.set_nonblocking(false);
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let target = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    // Read the headers.
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
    }
    hits.lock().unwrap().push(target.clone());
    let mut out = stream;
    let (status, ctype, body, extra): (u16, &str, Vec<u8>, String) = match handler(&target) {
        Reply::Ok(c, b) => (200, c, b, String::new()),
        Reply::Status(s, b) => (s, "text/plain", b.as_bytes().to_vec(), String::new()),
        Reply::Redirect(to) => (302, "text/plain", Vec::new(), format!("Location: {to}\r\n")),
        Reply::Slow(d) => {
            std::thread::sleep(d);
            (200, "text/plain", b"late".to_vec(), String::new())
        }
        Reply::Hang => return,
    };
    let _ = write!(
        out,
        "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    );
    let _ = out.write_all(&body);
    let _ = out.flush();
}

/// A PNG of one color.
pub fn png(w: u32, h: u32, c: [u8; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    for _ in 0..w * h {
        data.extend_from_slice(&c);
    }
    cadektor_core::underlay::Rgba {
        width: w,
        height: h,
        data,
    }
    .to_png()
}
