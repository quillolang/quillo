//! `quillo fcgi`'s corners that a web server rarely sends: a port of its own, a listening socket
//! given on descriptor 0 (Unix or TCP, or one that takes no connections), management records,
//! a second request before the first ends, a role it does not play, an abort, parameters and
//! bodies over their limits, answers whose head or body is large, and more connections at once
//! than it takes.

#![cfg(unix)]

mod support;

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROGRAM: &str = "route GET / = \"hi\"\nroute GET /big = repeat(\"b\", 100000)\nroute GET /tall = respond(200, \"under\", { \"x-tall\": repeat(\"t\", 1100000) })\nroute GET /wide = respond(200, \"fits\", { \"x-wide\": repeat(\"w\", 1000000) })\nroute POST /echo = `got ${request.body}`\n";

fn folder(name: &str) -> PathBuf {
    let d = support::temp_dir(&format!("cf-{}", name));
    std::fs::write(d.join("main.qwe"), PROGRAM).unwrap();
    d
}

/// `quillo fcgi main.qwe` with `args` in `dir`, its descriptor 0 `stdin`, its output in fcgi.log.
fn start(dir: &Path, args: &[&str], stdin: Stdio) -> support::Guard {
    let log = std::fs::File::create(dir.join("fcgi.log")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("fcgi")
        .arg("main.qwe")
        .args(args)
        .current_dir(dir)
        .stdin(stdin)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    support::Guard::new(child)
}

fn said(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("fcgi.log")).unwrap_or_default()
}

/// Connects with `connect` until it works, for at most 20 s.
fn until_connected<S>(dir: &Path, mut connect: impl FnMut() -> std::io::Result<S>) -> S {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(s) = connect() {
            return s;
        }
        assert!(Instant::now() < deadline, "never listened: {}", said(dir));
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Stops the server with SIGINT, as a process manager does, and waits at most 10 s.
fn interrupt(g: &mut support::Guard) -> Option<i32> {
    let _ = Command::new("kill").args(["-INT", &g.0.id().to_string()]).status();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(s) = g.0.try_wait().unwrap() {
            return s.code();
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn rec(kind: u8, id: u16, content: &[u8]) -> Vec<u8> {
    let pad = (8 - content.len() % 8) % 8;
    let mut r = vec![1, kind];
    r.extend_from_slice(&id.to_be_bytes());
    r.extend_from_slice(&(content.len() as u16).to_be_bytes());
    r.extend_from_slice(&[pad as u8, 0]);
    r.extend_from_slice(content);
    r.resize(r.len() + pad, 0);
    r
}

fn pair(k: &str, v: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for n in [k.len(), v.len()] {
        if n < 128 {
            out.push(n as u8);
        } else {
            out.extend_from_slice(&((n as u32) | 0x8000_0000).to_be_bytes());
        }
    }
    out.extend_from_slice(k.as_bytes());
    out.extend_from_slice(v.as_bytes());
    out
}

fn begin(id: u16, role: u16, keep: bool) -> Vec<u8> {
    let r = role.to_be_bytes();
    rec(1, id, &[r[0], r[1], keep as u8, 0, 0, 0, 0, 0])
}

/// The records of one request: BEGIN_REQUEST, its parameters, an empty PARAMS, its body, and an
/// empty STDIN.
fn request(id: u16, params: &[(&str, &str)], body: &[u8], keep: bool) -> Vec<u8> {
    let mut p = Vec::new();
    for (k, v) in params {
        p.extend(pair(k, v));
    }
    let mut m = begin(id, 1, keep);
    m.extend(rec(4, id, &p));
    m.extend(rec(4, id, &[]));
    if !body.is_empty() {
        m.extend(rec(5, id, body));
    }
    m.extend(rec(5, id, &[]));
    m
}

/// One record: its type, id and content; None at the connection's end.
fn read_rec(s: &mut impl Read) -> Option<(u8, u16, Vec<u8>)> {
    let mut h = [0u8; 8];
    s.read_exact(&mut h).ok()?;
    let n = u16::from_be_bytes([h[4], h[5]]) as usize;
    let mut c = vec![0u8; n + h[6] as usize];
    s.read_exact(&mut c).ok()?;
    c.truncate(n);
    Some((h[1], u16::from_be_bytes([h[2], h[3]]), c))
}

/// What request `id` was answered: its STDOUT, and END_REQUEST's application and protocol
/// statuses.
fn answer(s: &mut impl Read, id: u16) -> (String, (u32, u8)) {
    let mut out = Vec::new();
    loop {
        let (kind, got, c) = read_rec(s).expect("the connection ended before END_REQUEST");
        assert_eq!(got, id);
        match kind {
            6 => out.extend_from_slice(&c),
            3 => return (String::from_utf8_lossy(&out).into_owned(), (u32::from_be_bytes([c[0], c[1], c[2], c[3]]), c[4])),
            k => panic!("record {}", k),
        }
    }
}

fn ask(s: &mut (impl Read + Write), id: u16, uri: &str, keep: bool) -> (String, (u32, u8)) {
    s.write_all(&request(id, &[("REQUEST_METHOD", "GET"), ("REQUEST_URI", uri)], b"", keep)).unwrap();
    answer(s, id)
}

fn unix(dir: &Path, sock: &Path) -> UnixStream {
    let s = until_connected(dir, || UnixStream::connect(sock));
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s
}

#[test]
fn a_request_made_from_a_script_name_alone_has_no_query() {
    let p = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    let (req, peer) = quillo::fcgi::http_request(&p(&[("SCRIPT_NAME", "app"), ("PATH_INFO", "/x"), ("QUERY_STRING", "")]), b"");
    assert_eq!(String::from_utf8(req).unwrap(), "GET /app/x HTTP/1.0\r\nhost: localhost\r\ncontent-length: 0\r\n\r\n");
    assert_eq!(peer, "127.0.0.1:0".parse().unwrap());
}

#[test]
fn quillo_fcgi_on_a_port_answers_over_tcp_with_large_heads_and_bodies() {
    let d = folder("port");
    let port = support::free_port();
    let mut g = start(&d, &["--port", &port.to_string()], Stdio::null());
    let connect = || {
        let s = std::net::TcpStream::connect(("127.0.0.1", port))?;
        s.set_read_timeout(Some(Duration::from_secs(20)))?;
        Ok(s)
    };
    let mut s = until_connected(&d, connect);
    let (out, end) = ask(&mut s, 1, "/", true);
    assert!(out.starts_with("Status: 200 OK\r\n") && out.ends_with("\r\n\r\nhi"), "{}", out);
    assert_eq!(end, (0, 0));
    // A body larger than one read comes in more than one piece, all of it.
    let (out, _) = ask(&mut s, 2, "/big", true);
    assert!(out.starts_with("Status: 200 OK\r\n"), "{}", &out[..200.min(out.len())]);
    assert!(out.ends_with(&format!("\r\n\r\n{}", "b".repeat(100000))), "{}", out.len());
    // A head over 1 MB is no answer the web server can take: a 502 that says so, not the head
    // cut at 1 MB with its rest, `content-length` and all, sent as the body.
    let (out, end) = ask(&mut s, 3, "/tall", false);
    assert_eq!(out, "Status: 502 Bad Gateway\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nthe program's answer has a head over 1 MB\n");
    assert_eq!(end, (0, 0));
    // Its edge: a head just under 1 MB is the answer's own, whole, and its body after it.
    assert!(read_rec(&mut s).is_none(), "not kept: the connection ends");
    let mut s = connect().unwrap();
    let (out, end) = ask(&mut s, 4, "/wide", false);
    assert!(out.starts_with("Status: 200 OK\r\nx-wide: www"), "{}", &out[..200.min(out.len())]);
    assert!(out.contains(&format!("x-wide: {}\r\n", "w".repeat(1000000))), "the whole header");
    assert!(out.ends_with("\r\n\r\nfits"), "{:?}", &out[out.len() - 60..]);
    assert_eq!(end, (0, 0));
    assert!(read_rec(&mut s).is_none(), "not kept: the connection ends");
    assert_eq!(interrupt(&mut g), Some(0), "{}", said(&d));
}

#[test]
fn a_listening_unix_socket_given_on_descriptor_0_is_taken() {
    let d = folder("fd0-unix");
    let sock = d.join("pm.sock");
    let l = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let mut g = start(&d, &[], Stdio::from(std::os::fd::OwnedFd::from(l)));
    let mut s = unix(&d, &sock);
    let (out, end) = ask(&mut s, 1, "/", false);
    assert!(out.ends_with("\r\n\r\nhi"), "{}", out);
    assert_eq!(end, (0, 0));
    assert_eq!(interrupt(&mut g), Some(0), "{}", said(&d));
}

#[test]
fn a_listening_tcp_socket_given_on_descriptor_0_is_taken() {
    let d = folder("fd0-tcp");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let mut g = start(&d, &[], Stdio::from(std::os::fd::OwnedFd::from(l)));
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let (out, _) = ask(&mut s, 7, "/", false);
    assert!(out.ends_with("\r\n\r\nhi"), "{}", out);
    assert!(said(&d).contains("FastCGI at 127.0.0.1:"), "{}", said(&d));
    assert_eq!(interrupt(&mut g), Some(0), "{}", said(&d));
}

#[test]
fn a_socket_on_descriptor_0_that_takes_no_connections_is_said_and_tried_again() {
    let d = folder("fd0-stream");
    // A connected socket, not a listening one: accepting on it fails, again and again.
    let (ours, theirs) = UnixStream::pair().unwrap();
    let mut g = start(&d, &[], Stdio::from(std::os::fd::OwnedFd::from(theirs)));
    let deadline = Instant::now() + Duration::from_secs(20);
    while said(&d).matches("cannot accept a FastCGI connection").count() < 2 {
        assert!(Instant::now() < deadline, "{}", said(&d));
        assert!(g.0.try_wait().unwrap().is_none(), "it keeps trying: {}", said(&d));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(interrupt(&mut g), Some(0), "{}", said(&d));
    drop(ours);
}

#[test]
fn management_records_and_requests_out_of_turn_are_answered_as_the_specification_says() {
    let d = folder("records");
    let mut g = start(&d, &["--listen", "unix:f.sock", "--max-body", "100"], Stdio::null());
    let sock = d.join("f.sock");

    // FCGI_GET_VALUES: what it knows, and nothing for a name it does not.
    let mut s = unix(&d, &sock);
    let mut q = pair("FCGI_MPXS_CONNS", "");
    q.extend(pair("FCGI_MAX_CONNS", ""));
    q.extend(pair("FCGI_MAX_REQS", ""));
    q.extend(pair("SOMETHING_ELSE", ""));
    s.write_all(&rec(9, 0, &q)).unwrap();
    let (kind, id, c) = read_rec(&mut s).unwrap();
    assert_eq!((kind, id), (10, 0));
    let mut want = pair("FCGI_MPXS_CONNS", "0");
    want.extend(pair("FCGI_MAX_CONNS", "64"));
    want.extend(pair("FCGI_MAX_REQS", "64"));
    assert_eq!(c, want);
    assert_eq!(quillo::fcgi::pairs(&c).len(), 3);
    // A management record of a kind it does not know: FCGI_UNKNOWN_TYPE, naming it.
    s.write_all(&rec(99, 0, &[])).unwrap();
    assert_eq!(read_rec(&mut s).unwrap(), (11, 0, vec![99, 0, 0, 0, 0, 0, 0, 0]));
    // A record of a request that is not there, of a kind it does not know, is dropped.
    s.write_all(&rec(99, 5, b"x")).unwrap();
    s.write_all(&rec(4, 5, b"x")).unwrap();
    // A role it does not play (the authorizer), on a connection kept: refused, and the next
    // request on it is answered.
    s.write_all(&begin(2, 2, true)).unwrap();
    let (kind, id, c) = read_rec(&mut s).unwrap();
    assert_eq!((kind, id, c[4]), (3, 2, 3), "FCGI_UNKNOWN_ROLE");
    let (out, _) = ask(&mut s, 3, "/", true);
    assert!(out.ends_with("hi"), "{}", out);
    // A second request before the first ends: FCGI_CANT_MPX_CONN; the first goes on.
    s.write_all(&begin(4, 1, true)).unwrap();
    s.write_all(&begin(5, 1, true)).unwrap();
    let (kind, id, c) = read_rec(&mut s).unwrap();
    assert_eq!((kind, id, c[4]), (3, 5, 1), "FCGI_CANT_MPX_CONN");
    // An abort of another request is nothing; of this one, it ends, and the connection, kept,
    // takes the next.
    s.write_all(&rec(2, 9, &[])).unwrap();
    s.write_all(&rec(2, 4, &[])).unwrap();
    let (kind, id, c) = read_rec(&mut s).unwrap();
    assert_eq!((kind, id, c[4]), (3, 4, 0), "FCGI_REQUEST_COMPLETE");
    let (out, _) = ask(&mut s, 6, "/", true);
    assert!(out.ends_with("hi"), "{}", out);
    // A version other than 1 ends the connection.
    let mut bad = rec(9, 0, &[]);
    bad[0] = 2;
    s.write_all(&bad).unwrap();
    assert!(read_rec(&mut s).is_none());

    // A role it does not play, with the connection not kept: refused, and closed.
    let mut s = unix(&d, &sock);
    s.write_all(&begin(1, 3, false)).unwrap();
    assert_eq!(read_rec(&mut s).map(|r| (r.0, r.2[4])), Some((3, 3)));
    assert!(read_rec(&mut s).is_none());
    // An abort, the connection not kept: complete, and closed.
    let mut s = unix(&d, &sock);
    s.write_all(&begin(1, 1, false)).unwrap();
    s.write_all(&rec(2, 1, &[])).unwrap();
    assert_eq!(read_rec(&mut s).map(|r| (r.0, r.2[4])), Some((3, 0)));
    assert!(read_rec(&mut s).is_none());

    // A body over --max-body: ended with an application status of 1, and closed.
    let mut s = unix(&d, &sock);
    s.write_all(&request(1, &[("REQUEST_METHOD", "POST"), ("REQUEST_URI", "/echo")], &[b'x'; 101], true)).unwrap();
    let (kind, id, c) = read_rec(&mut s).unwrap();
    assert_eq!((kind, id, u32::from_be_bytes([c[0], c[1], c[2], c[3]]), c[4]), (3, 1, 1, 0));
    assert!(read_rec(&mut s).is_none());
    // One within it is answered.
    let mut s = unix(&d, &sock);
    s.write_all(&request(1, &[("REQUEST_METHOD", "POST"), ("REQUEST_URI", "/echo")], &[b'x'; 100], false)).unwrap();
    let (out, _) = answer(&mut s, 1);
    assert!(out.ends_with(&format!("got {}", "x".repeat(100))), "{}", out);

    // Parameters over 1 MB, beside the body: the same.
    let mut s = unix(&d, &sock);
    s.write_all(&begin(1, 1, true)).unwrap();
    let chunk = vec![b'p'; 65535];
    for _ in 0..17 {
        if s.write_all(&rec(4, 1, &chunk)).is_err() {
            break;
        }
    }
    let (kind, id, c) = read_rec(&mut s).unwrap();
    assert_eq!((kind, id, u32::from_be_bytes([c[0], c[1], c[2], c[3]])), (3, 1, 1));

    assert_eq!(interrupt(&mut g), Some(0), "{}", said(&d));
    assert!(!sock.exists());
}

/// Raises this process's limit on open files, for the connections a test holds.
fn more_files(n: u64) {
    let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `r` is live for the calls to read and write.
    unsafe {
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) == 0 && r.rlim_cur < n {
            r.rlim_cur = n.min(r.rlim_max);
            libc::setrlimit(libc::RLIMIT_NOFILE, &r);
        }
    }
}

#[test]
fn a_connection_over_the_most_at_once_is_closed_as_it_comes() {
    more_files(2048);
    let d = folder("many");
    let mut g = start(&d, &["--listen", "unix:f.sock"], Stdio::null());
    let sock = d.join("f.sock");
    // 256 idle connections, each holding a thread; a request on one shows all are taken.
    let mut held: Vec<UnixStream> = (0..256).map(|_| unix(&d, &sock)).collect();
    let (out, _) = ask(held.last_mut().unwrap(), 1, "/", true);
    assert!(out.ends_with("hi"), "{}", out);
    // The next is closed at once, unread.
    let mut over = unix(&d, &sock);
    let mut b = [0u8; 8];
    assert!(matches!(over.read(&mut b), Ok(0) | Err(_)), "closed");
    // The ones held are still served.
    let (out, _) = ask(&mut held[0], 2, "/", true);
    assert!(out.ends_with("hi"), "{}", out);
    // One let go makes room again.
    held.truncate(255);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let mut s = unix(&d, &sock);
        if s.write_all(&request(1, &[("REQUEST_METHOD", "GET"), ("REQUEST_URI", "/")], b"", false)).is_ok() {
            if let Some((6, 1, c)) = read_rec(&mut s) {
                assert!(String::from_utf8_lossy(&c).starts_with("Status: 200"));
                break;
            }
        }
        assert!(Instant::now() < deadline, "no room was made");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(held);
    assert_eq!(interrupt(&mut g), Some(0), "{}", said(&d));
}
