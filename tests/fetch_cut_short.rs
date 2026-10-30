//! `http.get` when the server hangs up partway through what it said it would
//! send: an error value, not a short body taken for the whole.

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;

/// Answers one request with `reply`, then closes the connection.
fn answers_once(reply: &'static [u8]) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = s.read(&mut buf);
        let _ = s.write_all(reply);
    });
    port
}

fn get(port: u16) -> String {
    match quillo::eval_str(&format!("http.get(\"http://127.0.0.1:{}/\")", port)) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}", e),
    }
}

#[test]
fn a_body_shorter_than_its_length_is_an_error() {
    let port = answers_once(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 100\r\nConnection: close\r\n\r\nstart");
    let got = get(port);
    assert!(got.starts_with("err(\"http: "), "{}", got);
    assert!(!got.contains("start"), "{}", got);
}

#[test]
fn a_body_of_its_length_is_the_answer() {
    let port = answers_once(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: close\r\n\r\nstart");
    assert!(get(port).contains("body: \"start\""));
}
