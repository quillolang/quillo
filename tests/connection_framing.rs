//! What the server does with a connection when the request or the answer says
//! something of it: Content-Length given twice, an answer that says
//! `connection: close`; and large lists a request leaves behind.

mod support;

use quillo::server::HttpRequest;
use quillo::Interp;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;
use support::Server;

fn program(name: &str, src: &str) -> (PathBuf, Server) {
    let d = support::temp_dir(&format!("framing-{}", name));
    std::fs::write(d.join("app.qwe"), src).unwrap();
    let s = support::serve(&d.join("app.qwe"), &[]);
    (d, s)
}

fn raw(port: u16, text: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(text.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn content_length_twice_with_one_value_is_taken_and_with_two_is_refused() {
    let (_d, s) = program("twice", "route POST /size = str(len(request.body))\n");
    let same = raw(s.port, "POST /size HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: 3\r\nContent-Length: 3\r\n\r\nabc");
    assert!(same.starts_with("HTTP/1.1 200"), "{}", same);
    assert!(same.ends_with("\r\n\r\n3"), "{}", same);
    let differ = raw(s.port, "POST /size HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\nabcd");
    assert!(differ.starts_with("HTTP/1.1 400"), "{}", differ);
    assert!(differ.contains("bad content-length"), "{}", differ);
}

#[test]
fn an_answer_that_says_close_closes_a_connection_that_could_stay_open() {
    let (_d, s) = program("close", "route GET /bye = respond(200, \"bye\", { connection: \"Close\" })\nroute GET /stay = respond(200, \"stay\", { connection: \"keep-alive\" })\n");
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    // Asked together: the second is not answered, since the first closes it.
    c.write_all(b"GET /bye HTTP/1.1\r\nHost: x\r\n\r\nGET /stay HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut out = Vec::new();
    c.read_to_end(&mut out).expect("closed by the server, not timed out");
    let out = String::from_utf8_lossy(&out).into_owned();
    assert!(out.starts_with("HTTP/1.1 200") && out.ends_with("bye"), "{}", out);
    assert_eq!(out.matches("HTTP/1.1").count(), 1, "{}", out);

    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c.write_all(b"GET /stay HTTP/1.1\r\nHost: x\r\n\r\nGET /bye HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut out = Vec::new();
    c.read_to_end(&mut out).expect("closed after the second");
    assert_eq!(String::from_utf8_lossy(&out).matches("HTTP/1.1").count(), 2);
}

#[test]
fn the_header_is_passed_on_as_the_program_wrote_it() {
    let d = support::temp_dir("framing-header");
    let mut it = Interp::new(&d);
    it.load_source("route GET /bye = respond(200, \"bye\", { connection: \"Close\" })\n", "app.qwe").unwrap();
    let r = it.handle(&HttpRequest { method: "GET".into(), url: "/bye".into(), headers: vec![("Host".into(), "x".into())], body: Vec::new(), remote: "127.0.0.1".into() });
    assert_eq!(r.header("connection"), Some("Close"));
}

#[test]
fn large_lists_a_request_leaves_behind_do_not_get_in_the_way_of_the_next() {
    let src = "fn total(n) {\n let xs = range(0, n)\n let ys = xs.map(x => x * 2)\n len(ys)\n}\nroute GET /big = str(total(1000))\n";
    let (_d, s) = program("large", src);
    for _ in 0..50 {
        assert_eq!(support::send(s.port, "GET", "/big", &[], "").body, "1000");
    }
}
