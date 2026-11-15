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
        } else if data.len() <= 0xffff {
            f.push(0x80 | 126);
            f.extend_from_slice(&(data.len() as u16).to_be_bytes());
        } else {
            f.push(0x80 | 127);
            f.extend_from_slice(&(data.len() as u64).to_be_bytes());
        }
        let mask = [3, 1, 4, 1];
        f.extend_from_slice(&mask);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.0.write_all(&f).unwrap();
    }

    /// `open`, once the server has a place for it: a page that was let go gives its place back
    /// once its task has ended, which may be just after its connection closed.
    fn open_when_free(port: u16) -> Page {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
            s.write_all(upgrade_request(&ticket(port), "13", "Upgrade").as_bytes()).unwrap();
            let h = head(&mut s);
            if h.starts_with(b"HTTP/1.1 101") {
                return Page(s);
            }
            assert!(h.starts_with(b"HTTP/1.1 503") && Instant::now() < end, "{}", String::from_utf8_lossy(&h));
            std::thread::sleep(Duration::from_millis(20));
        }
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
    let mut a = Page::open_when_free(s.port);
    let mut b = Page::open_when_free(s.port);
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
    let mut c = Page::open_when_free(s.port);
    let mut e = Page::open_when_free(s.port);
    c.join();
    e.join();
    assert_eq!(peers(s.port), 2);
}

// ---- pages with live data (Q-954's second part) ----

const LIVE: &str = "route GET / = \"hi\"\nroute GET /bump/:t {\n    publish(t)\n    \"ok\"\n}\n";

fn seq(port: u16) -> u64 {
    let a = support::send(port, "GET", "/_quillo/live?since=0&t=x", &[], "");
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap_or_else(|e| panic!("{}: {}", e, a.body));
    j["seq"].as_u64().unwrap()
}

/// A page waiting for changes to `topic` on its socket.
fn live_page(port: u16, since: u64, topic: &str) -> Page {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    write!(s, "GET /_quillo/live?since={}&t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", since, topic).unwrap();
    let h = head(&mut s);
    assert!(h.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&h));
    Page(s)
}

fn changed(m: &str) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(m).unwrap()["changed"].clone()
}

// Q-954: 300 pages with live data start no thread each, where each was one, 240 KB a page, and cost
// a few kilobytes each; when they close, they are let go.
#[cfg(unix)]
#[test]
fn pages_with_live_data_take_no_thread_of_their_own_and_little_memory() {
    let d = dir("live-memory", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &["--live-per-client", "1000"]);
    let at = seq(s.port);
    // The pages' runtime is made by the first page: counted from there.
    let _first = live_page(s.port, at, "first");
    std::thread::sleep(Duration::from_millis(200));
    let (threads_before, rss_before) = (threads(s.pid()), rss_kib(s.pid()));
    let pages: Vec<Page> = (0..300).map(|i| live_page(s.port, at, &format!("t{}", i))).collect();
    std::thread::sleep(Duration::from_millis(300));
    let (t, rss) = (threads(s.pid()), rss_kib(s.pid()));
    assert!(t <= threads_before + 2, "threads: {} before, {} with 300 pages more", threads_before, t);
    assert!(rss < rss_before + 300 * 32, "memory: {} KiB before, {} KiB with 300 pages more", rss_before, rss);
    drop(pages);
    std::thread::sleep(Duration::from_millis(500));
    assert!(threads(s.pid()) <= threads_before + 2);
}

// Q-954: a change reaches the pages that follow it and no other, each at once, many pages at once;
// the first change goes at once and what follows closely is gathered into one message.
#[test]
fn a_change_reaches_the_pages_that_follow_it_and_no_other() {
    let d = dir("live-reach", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &["--live-per-client", "1000"]);
    let at = seq(s.port);
    let mut pages: Vec<Page> = (0..100).map(|i| live_page(s.port, at, &format!("t{}", i % 10))).collect();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(support::send(s.port, "GET", "/bump/t3", &[], "").body, "ok");
    for (i, p) in pages.iter_mut().enumerate() {
        if i % 10 == 3 {
            assert_eq!(changed(&p.text().unwrap()), serde_json::json!(["t3"]), "page {}", i);
        }
    }
    // The others heard nothing: a change of their own is the next they are told.
    assert_eq!(support::send(s.port, "GET", "/bump/t4", &[], "").body, "ok");
    for (i, p) in pages.iter_mut().enumerate() {
        if i % 10 == 4 {
            assert_eq!(changed(&p.text().unwrap()), serde_json::json!(["t4"]), "page {}", i);
        }
    }
    // Two changes close together reach a page as one.
    let mut p = live_page(s.port, seq(s.port), "a,b");
    std::thread::sleep(Duration::from_millis(200));
    support::send(s.port, "GET", "/bump/a", &[], "");
    assert_eq!(changed(&p.text().unwrap()), serde_json::json!(["a"]));
    support::send(s.port, "GET", "/bump/b", &[], "");
    support::send(s.port, "GET", "/bump/a", &[], "");
    let m = p.text().unwrap();
    assert_eq!(changed(&m).as_array().map(|a| a.len()), Some(2), "{}", m);
}

// Q-954: a long poll waits as a task too: answered with the change within its 25 s, after the
// answers before it on its connection, both asked at once.
#[test]
fn a_long_poll_is_answered_after_what_came_before_it_on_its_connection() {
    let d = dir("live-poll", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    let at = seq(s.port);
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    write!(c, "GET / HTTP/1.1\r\nHost: localhost\r\n\r\nGET /_quillo/live?since={}&t=poll HTTP/1.1\r\nHost: localhost\r\n\r\n", at).unwrap();
    let asked = Instant::now();
    let first = String::from_utf8_lossy(&head(&mut c)).into_owned();
    assert!(first.starts_with("HTTP/1.1 200"), "{}", first);
    assert!(asked.elapsed() < Duration::from_secs(5), "the first answer waited {:?} for the long poll", asked.elapsed());
    let mut hi = [0u8; 2];
    c.read_exact(&mut hi).unwrap();
    assert_eq!(&hi, b"hi");
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    assert_eq!(support::send(s.port, "GET", "/bump/poll", &[], "").body, "ok");
    let h = String::from_utf8_lossy(&head(&mut c)).into_owned();
    assert!(h.starts_with("HTTP/1.1 200") && h.contains("no-store"), "{}", h);
    let len: usize = h.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").map(|n| n.trim().parse().unwrap())).unwrap();
    let mut body = vec![0u8; len];
    c.read_exact(&mut body).unwrap();
    assert_eq!(changed(&String::from_utf8(body).unwrap()), serde_json::json!(["poll"]));
    assert!(t.elapsed() < Duration::from_secs(5), "answered after {:?}", t.elapsed());
}

// Q-954: a quiet page with live data is pinged every 20 s, as its thread pinged it, and one that
// answers is kept and told of changes after.
#[test]
fn a_quiet_live_page_is_pinged_and_kept() {
    let d = dir("live-ping", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    let mut p = live_page(s.port, seq(s.port), "quiet");
    let t = Instant::now();
    assert_eq!(p.frame(), Some((0x9, Vec::new())), "after {:?}", t.elapsed());
    assert!(t.elapsed() >= Duration::from_secs(15), "pinged after {:?}", t.elapsed());
    p.send(0xA, b"");
    support::send(s.port, "GET", "/bump/quiet", &[], "");
    assert_eq!(changed(&p.text().unwrap()), serde_json::json!(["quiet"]));
}

// Q-954: an upgrade to a live socket the server cannot take is told why and gives its place back:
// with room for one page, a refused one leaves room for it.
#[test]
fn a_refused_live_upgrade_gives_its_place_back() {
    let d = dir("live-refused", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &["--live-per-client", "1"]);
    let at = seq(s.port);
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(c, "GET /_quillo/live?since={}&t=x HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: k\r\nSec-WebSocket-Version: 12\r\n\r\n", at).unwrap();
    assert!(head(&mut c).starts_with(b"HTTP/1.1 426"));
    let mut b = [0u8; 1];
    assert_eq!(c.read(&mut b).unwrap_or(0), 0);
    // Its place is given back once its task has ended, which may be just after the close.
    let mut p = None;
    assert!(until(10, || {
        let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        write!(c, "GET /_quillo/live?since={}&t=x HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", at).unwrap();
        let ok = head(&mut c).starts_with(b"HTTP/1.1 101");
        if ok {
            p = Some(Page(c));
        }
        ok
    }), "the refused page's place was not given back");
    let mut p = p.unwrap();
    std::thread::sleep(Duration::from_millis(200));
    support::send(s.port, "GET", "/bump/x", &[], "");
    assert_eq!(changed(&p.text().unwrap()), serde_json::json!(["x"]));
}

/// A connection that takes 4 KiB unread at most, so that what the server writes to it stops part
/// way when its page does not read.
#[cfg(unix)]
fn small_window(port: u16) -> TcpStream {
    use std::os::fd::FromRawFd;
    // SAFETY: a socket made, set and connected here, and handed whole to the TcpStream.
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        assert!(fd >= 0);
        let n: libc::c_int = 4096;
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVBUF, &n as *const libc::c_int as *const libc::c_void, std::mem::size_of::<libc::c_int>() as libc::socklen_t);
        let mut addr: libc::sockaddr_in = std::mem::zeroed();
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = port.to_be();
        addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
        {
            addr.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
        }
        let r = libc::connect(fd, &addr as *const libc::sockaddr_in as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t);
        assert_eq!(r, 0, "connect: {}", std::io::Error::last_os_error());
        TcpStream::from_raw_fd(fd)
    }
}

// Q-954: a change too large to be written at once, to a page that stops reading for a while, is
// written as far as it goes and the rest after, each whole and in order once the page reads: twenty
// of 63 KB each, to a connection that holds 4 KiB unread.
#[cfg(unix)]
#[test]
fn changes_too_large_to_write_at_once_reach_a_page_whole() {
    let src = "route GET /all {\n    for i in 0..256 {\n        publish(\"t\" + str(i) + \"-\" + \"x\".repeat(240))\n    }\n    \"ok\"\n}\n";
    let d = dir("live-large", src);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    let topics: Vec<String> = (0..256).map(|i| format!("t{}-{}", i, "x".repeat(240))).collect();
    let mut s2 = small_window(s.port);
    s2.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let at: u64 = {
        let a = support::send(s.port, "GET", "/_quillo/live?since=0&t=x", &[], "");
        serde_json::from_str::<serde_json::Value>(&a.body).unwrap()["seq"].as_u64().unwrap()
    };
    write!(s2, "GET /_quillo/live?since={}&tm=1 HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", at).unwrap();
    assert!(head(&mut s2).starts_with(b"HTTP/1.1 101"));
    let mut p = Page(s2);
    p.send(0x1, topics.join(",").as_bytes());
    std::thread::sleep(Duration::from_millis(300));
    // Twenty changes to every topic, a gathering apart, none read.
    for _ in 0..20 {
        assert_eq!(support::send(s.port, "GET", "/all", &[], "").body, "ok");
        std::thread::sleep(Duration::from_millis(250));
    }
    // Read now: every message whole, of the page's own topics, till the last change; the first
    // change of each burst goes at once, and what follows it is gathered, the rest in a message
    // (fewer of them while a write waits for the page to read).
    let last = seq(s.port);
    let (mut whole, mut at_last, mut lens) = (0, 0, Vec::new());
    while at_last < last {
        let m = p.text().expect("a change");
        let j: serde_json::Value = serde_json::from_str(&m).unwrap_or_else(|e| panic!("{}: {}", e, &m[..m.len().min(200)]));
        let c = j["changed"].as_array().unwrap();
        assert!(c.iter().all(|t| topics.iter().any(|x| Some(x.as_str()) == t.as_str())), "{}", &m[..m.len().min(200)]);
        if c.len() >= 250 {
            whole += 1;
        }
        lens.push(c.len());
        at_last = j["seq"].as_u64().unwrap();
    }
    assert!(whole >= 5, "{} messages of 250 topics or more: {:?}", whole, lens);
}

// Q-954: a live page's ping is answered with its data, at once, and the page is kept.
#[test]
fn a_live_pages_ping_is_answered() {
    let d = dir("live-pong", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    let mut p = live_page(s.port, seq(s.port), "pong");
    p.send(0x9, b"are you there");
    assert_eq!(p.frame(), Some((0xA, b"are you there".to_vec())));
    support::send(s.port, "GET", "/bump/pong", &[], "");
    assert_eq!(changed(&p.text().unwrap()), serde_json::json!(["pong"]));
}

// Q-954: a live page that hangs up in the middle of a frame is let go, and its place given back:
// with room for one page, the next one gets in and is told of a change.
#[test]
fn a_live_page_gone_in_the_middle_of_a_frame_gives_its_place_back() {
    let d = dir("live-cut", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &["--live-per-client", "1"]);
    let at = seq(s.port);
    let mut p = live_page(s.port, at, "cut");
    // A text frame's head for 100 bytes, its mask, and 3 of them; then it hangs up.
    p.0.write_all(&[0x81, 0x80 | 100, 1, 2, 3, 4, b'a', b'b', b'c']).unwrap();
    p.0.shutdown(std::net::Shutdown::Write).unwrap();
    assert!(until(10, || {
        let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(c, "GET /_quillo/live?since={}&t=cut HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", at).unwrap();
        head(&mut c).starts_with(b"HTTP/1.1 101")
    }), "the place was not given back");
}

// Q-954: a long poll from a page whose number is ahead of the server's (from before a restart) is
// told the server's number at once, with nothing changed, as a thread told it.
#[test]
fn a_long_poll_ahead_of_the_server_is_told_its_number_at_once() {
    let d = dir("live-ahead", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    let now = seq(s.port);
    let t = Instant::now();
    let a = support::send(s.port, "GET", "/_quillo/live?since=999999999&t=x", &[], "");
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    assert_eq!((j["seq"].as_u64(), j["changed"].clone()), (Some(now), serde_json::json!([])));
    assert!(t.elapsed() < Duration::from_secs(5), "after {:?}", t.elapsed());
}

// Q-954: the pages that wait for `--watch`'s reloads still have a thread each, within the same
// places: past --live-per-client, one is told to try later.
#[test]
fn a_watch_page_past_its_places_is_told_to_try_later() {
    let d = dir("watch-places", LIVE);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch", "--live-per-client", "1"]);
    let open = |v: &str| {
        let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(c, "GET /_quillo/events?v={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", v).unwrap();
        (String::from_utf8_lossy(&head(&mut c)).into_owned(), c)
    };
    // A page with no version is told the program's and let go; one that has it is kept.
    let (first, c) = open("0");
    assert!(first.starts_with("HTTP/1.1 101"), "{}", first);
    let version = Page(c).text().expect("the version");
    // Its place is given back once its thread has closed it: asked again till then.
    let mut kept = None;
    assert!(until(10, || {
        let (h, c) = open(&version);
        let ok = h.starts_with("HTTP/1.1 101");
        if ok {
            kept = Some(c);
        }
        ok
    }), "the first page's place was not given back");
    let (second, _) = open(&version);
    assert!(second.starts_with("HTTP/1.1 503") && second.contains("retry-after: 5"), "{}", second);
}
