// FEEDBACK66.md: qwertime's rooms. Q-954: a page on a channel cost the server half a megabyte,
// three threads each with memory of its own, and most of it stayed after the page closed. A
// page's WebSocket is now two tasks on one runtime (branch q954).

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f66-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

const ROOM: &str = "channel.on(\"c\", (msg, peer) => null, { join: (peer) => true })\nroute GET / = \"hi\"\nroute GET /peers = len(channel.peers(\"c\"))\nroute GET /say/:n(int) {\n    let sent = 0\n    for i in 0..n {\n        sent = sent + channel.broadcast(\"c\", i)\n    }\n    sent\n}\n";

fn until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

fn ticket(port: u16) -> String {
    let a = support::send(port, "POST", "/_quillo/wt-ticket", &[], "");
    let v: serde_json::Value = serde_json::from_str(&a.body).unwrap_or_else(|e| panic!("{}: {}", e, a.body));
    v["ticket"].as_str().unwrap().to_string()
}

fn upgrade_request(ticket: &str, version: &str, connection: &str) -> String {
    format!("GET /_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: {}\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: {}\r\n\r\n", ticket, connection, version)
}

/// What the server wrote up to the end of a head, or to the end of the connection.
fn head(s: &mut TcpStream) -> Vec<u8> {
    let mut got = Vec::new();
    let mut b = [0u8; 1];
    while !got.ends_with(b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(1) => got.push(b[0]),
            _ => break,
        }
    }
    got
}

/// A page on the channels' WebSocket.
struct Page(TcpStream);

impl Page {
    fn open(port: u16) -> Page {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        s.write_all(upgrade_request(&ticket(port), "13", "Upgrade").as_bytes()).unwrap();
        let h = head(&mut s);
        assert!(h.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&h));
        Page(s)
    }

    /// A frame as a page sends it, masked.
    fn send(&mut self, op: u8, data: &[u8]) {
        let mut f = vec![0x80 | op];
        if data.len() < 126 {
            f.push(0x80 | data.len() as u8);
        } else {
            f.push(0x80 | 126);
            f.extend_from_slice(&(data.len() as u16).to_be_bytes());
        }
        let mask = [3, 1, 4, 1];
        f.extend_from_slice(&mask);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.0.write_all(&f).unwrap();
    }

    fn join(&mut self) {
        self.send(0x1, b"[\"j\",\"c\"]");
        assert_eq!(self.text().as_deref(), Some("[\"j\",\"c\",true]"));
    }

    /// The next frame: its opcode and payload; None once the connection ends.
    fn frame(&mut self) -> Option<(u8, Vec<u8>)> {
        let mut h = [0u8; 2];
        self.0.read_exact(&mut h).ok()?;
        let mut n = (h[1] & 0x7f) as usize;
        if n == 126 {
            let mut l = [0u8; 2];
            self.0.read_exact(&mut l).ok()?;
            n = u16::from_be_bytes(l) as usize;
        } else if n == 127 {
            let mut l = [0u8; 8];
            self.0.read_exact(&mut l).ok()?;
            n = u64::from_be_bytes(l) as usize;
        }
        let mut data = vec![0u8; n];
        self.0.read_exact(&mut data).ok()?;
        Some((h[0] & 0x0f, data))
    }

    /// The next text message, pings and pongs passed over.
    fn text(&mut self) -> Option<String> {
        loop {
            match self.frame()? {
                (0x1, d) => return Some(String::from_utf8(d).unwrap()),
                (0x8, _) => return None,
                _ => {}
            }
        }
    }
}

fn peers(port: u16) -> usize {
    support::send(port, "GET", "/peers", &[], "").body.parse().unwrap()
}

/// The server's threads, from the system.
#[cfg(unix)]
fn threads(pid: u32) -> usize {
    if let Ok(d) = std::fs::read_dir(format!("/proc/{}/task", pid)) {
        return d.count();
    }
    let o = std::process::Command::new("ps").args(["-M", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).lines().count() - 1
}

#[cfg(unix)]
fn rss_kib(pid: u32) -> u64 {
    let o = std::process::Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().parse().unwrap()
}

// Q-954: 300 pages on a channel start no thread each, where each was three, and cost the server
// a few kilobytes each, where each was half a megabyte; when they close, every one is let go.
#[cfg(unix)]
#[test]
fn pages_on_a_channel_take_no_thread_of_their_own_and_little_memory() {
    let d = dir("memory", ROOM);
    let s = support::serve_with(&d.join("main.qwe"), &["--no-wt", "--live-per-client", "1000"]);
    // The pages' runtime is made by the first page: counted from there.
    let mut first = Page::open(s.port);
    first.join();
    let (threads_before, rss_before) = (threads(s.pid()), rss_kib(s.pid()));
    let mut pages: Vec<Page> = (0..300).map(|_| Page::open(s.port)).collect();
    for p in &mut pages {
        p.join();
    }
    assert_eq!(peers(s.port), 301);
    let (t, rss) = (threads(s.pid()), rss_kib(s.pid()));
    assert!(t <= threads_before + 4, "threads: {} before, {} with 300 pages more", threads_before, t);
    // Three threads a page took 556 KB, 160 MB for 300; 48 KiB each is ample.
    assert!(rss < rss_before + 300 * 48, "memory: {} KiB before, {} KiB with 300 pages more", rss_before, rss);
    drop(pages);
    assert!(until(10, || peers(s.port) == 1), "{} pages still in", peers(s.port));
    assert!(threads(s.pid()) <= threads_before + 4);
}

// Q-954: a broadcast to many pages reaches each whole and in order, a burst of them too.
#[test]
fn a_burst_to_many_pages_reaches_each_whole_and_in_order() {
    let d = dir("burst", ROOM);
    let s = support::serve_with(&d.join("main.qwe"), &["--no-wt", "--live-per-client", "1000"]);
    let mut pages: Vec<Page> = (0..200).map(|_| Page::open(s.port)).collect();
    for p in &mut pages {
        p.join();
    }
    let readers: Vec<_> = pages
        .into_iter()
        .map(|mut p| {
            std::thread::spawn(move || {
                for want in 0..300 {
                    assert_eq!(p.text(), Some(format!("[\"m\",\"c\",{}]", want)));
                }
            })
        })
        .collect();
    assert_eq!(support::send(s.port, "GET", "/say/300", &[], "").body, "60000");
    for r in readers {
        r.join().unwrap();
    }
}

// Q-954: a quiet page is pinged, as its own thread pinged it every 20 s; one that answers stays,
// and messages reach it after.
#[test]
fn a_quiet_page_is_pinged_and_one_that_answers_stays() {
    let d = dir("ping", ROOM);
    let s = support::serve_with(&d.join("main.qwe"), &["--no-wt"]);
    let mut p = Page::open(s.port);
    p.join();
    let t = Instant::now();
    let f = p.frame();
    assert_eq!(f, Some((0x9, Vec::new())), "after {:?}", t.elapsed());
    assert!(t.elapsed() >= Duration::from_secs(15), "pinged after {:?}", t.elapsed());
    p.send(0xA, b"");
    // A ping from the page is answered with its data.
    p.send(0x9, b"are you there");
    assert_eq!(p.frame(), Some((0xA, b"are you there".to_vec())));
    assert_eq!(support::send(s.port, "GET", "/say/1", &[], "").body, "1");
    assert_eq!(p.text().as_deref(), Some("[\"m\",\"c\",0]"));
}

// Q-954: an upgrade the server cannot take is answered with its refusal, the connection closed,
// and its place given back: with room for two pages, three refused leave room for two.
#[test]
fn a_refused_upgrade_is_told_why_and_gives_its_place_back() {
    let d = dir("refused", ROOM);
    let s = support::serve_with(&d.join("main.qwe"), &["--no-wt", "--live-per-client", "2"]);
    let refused = |version: &str, connection: &str| {
        let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        c.write_all(upgrade_request(&ticket(s.port), version, connection).as_bytes()).unwrap();
        let h = String::from_utf8_lossy(&head(&mut c)).into_owned();
        let mut b = [0u8; 1];
        assert_eq!(c.read(&mut b).unwrap_or(0), 0, "kept after {}", h);
        h
    };
    let h = refused("12", "Upgrade");
    assert!(h.starts_with("HTTP/1.1 426") && h.contains("sec-websocket-version: 13"), "{}", h);
    assert!(refused("13", "keep-alive").starts_with("HTTP/1.1 400"));
    assert!(refused("8", "keep-alive, Upgrade").starts_with("HTTP/1.1 426"));
    let mut a = Page::open(s.port);
    let mut b = Page::open(s.port);
    a.join();
    b.join();
    // A third page is past the limit.
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c.write_all(upgrade_request(&ticket(s.port), "13", "Upgrade").as_bytes()).unwrap();
    assert!(head(&mut c).starts_with(b"HTTP/1.1 503"));
}

// Q-954: an answer on the connection before its upgrade goes first, then the 101, then the page's
// messages: both requests sent at once.
#[test]
fn an_answer_before_the_upgrade_on_its_connection_goes_first() {
    let d = dir("before", ROOM);
    let s = support::serve_with(&d.join("main.qwe"), &["--no-wt"]);
    let t = ticket(s.port);
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c.write_all(format!("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n{}", upgrade_request(&t, "13", "Upgrade")).as_bytes()).unwrap();
    let first = String::from_utf8_lossy(&head(&mut c)).into_owned();
    assert!(first.starts_with("HTTP/1.1 200"), "{}", first);
    let mut body = [0u8; 2];
    c.read_exact(&mut body).unwrap();
    assert_eq!(&body, b"hi");
    let second = String::from_utf8_lossy(&head(&mut c)).into_owned();
    assert!(second.starts_with("HTTP/1.1 101"), "{}", second);
    let mut p = Page(c);
    p.join();
    assert_eq!(support::send(s.port, "GET", "/say/1", &[], "").body, "1");
    assert_eq!(p.text().as_deref(), Some("[\"m\",\"c\",0]"));
}

// Q-954: a page that closes is answered with a close and let go, and one that hangs up is let go;
// their places are given back for the next pages.
#[test]
fn a_page_that_closes_or_hangs_up_is_let_go_and_its_place_given_back() {
    let d = dir("gone", ROOM);
    let s = support::serve_with(&d.join("main.qwe"), &["--no-wt", "--live-per-client", "2"]);
    let mut a = Page::open(s.port);
    let mut b = Page::open(s.port);
    a.join();
    b.join();
    assert_eq!(peers(s.port), 2);
    a.send(0x8, &1000u16.to_be_bytes());
    assert_eq!(a.frame().map(|f| f.0), Some(0x8));
    assert_eq!(a.frame(), None);
    drop(b);
    assert!(until(10, || peers(s.port) == 0), "{} pages still in", peers(s.port));
    // Both places given back.
    let mut c = Page::open(s.port);
    let mut e = Page::open(s.port);
    c.join();
    e.join();
    assert_eq!(peers(s.port), 2);
}
