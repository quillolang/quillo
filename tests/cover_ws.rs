//! A live page's WebSocket while it waits (src/ws.rs, `Ws::gone`): what the page sends then
//! is read to see whether it has gone, and frames that are not a close leave it there.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;
use support::send;

const LIVE: &str = r#"
db.open("live.db")
db.exec("create table if not exists rows (id integer primary key, title text)")
db.exec("insert into rows (title) values ('one')")
server fn rows() = db.query("select * from rows order by id")
route GET /write = db.exec("update rows set title = ? where id = 1", str(random_int(0, 1000000)))
"#;

fn folder(name: &str) -> PathBuf {
    let d = support::temp_dir(&format!("cover-ws-{}", name));
    std::fs::write(d.join("main.qwe"), LIVE).unwrap();
    d
}

fn seq(port: u16) -> u64 {
    let a = send(port, "POST", "/_quillo/rpc/rows", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]");
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap_or_else(|_| panic!("{} {}", a.status, a.body));
    j["seq"].as_u64().unwrap()
}

/// A page waiting for changes to `db:rows`.
fn open(port: u16, since: u64) -> TcpStream {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "GET /_quillo/live?since={}&s=&t=db:rows HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", since).unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut b).unwrap();
        head.push(b[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
    s
}

/// A frame as a page sends it: masked.
fn masked(opcode: u8, data: &[u8]) -> Vec<u8> {
    let mut f = vec![0x80 | opcode];
    if data.len() < 126 {
        f.push(0x80 | data.len() as u8);
    } else {
        f.push(0x80 | 126);
        f.extend_from_slice(&(data.len() as u16).to_be_bytes());
    }
    let mask = [7, 1, 3, 9];
    f.extend_from_slice(&mask);
    f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// The next text frame, skipping pings; None if none comes in time.
fn text(s: &mut TcpStream) -> Option<String> {
    loop {
        let mut h = [0u8; 2];
        s.read_exact(&mut h).ok()?;
        let mut len = (h[1] & 0x7f) as usize;
        if len == 126 {
            let mut l = [0u8; 2];
            s.read_exact(&mut l).ok()?;
            len = u16::from_be_bytes(l) as usize;
        }
        let mut data = vec![0u8; len];
        s.read_exact(&mut data).ok()?;
        if h[0] & 0x0f == 1 {
            return Some(String::from_utf8_lossy(&data).into_owned());
        }
    }
}

/// Pongs, short frames and long ones a waiting page sends are not a close: once the server has
/// looked at them (every five seconds), the page is still there, and hears the next change.
#[test]
fn a_waiting_page_that_sends_frames_other_than_a_close_is_kept() {
    let s = support::serve(&folder("kept").join("main.qwe"), &[]);
    let since = seq(s.port);
    // Two pongs: all of what came is looked through, and none is a close.
    let mut pongs = open(s.port, since);
    let mut two = masked(0xA, b"");
    two.extend(masked(0xA, b"ok"));
    pongs.write_all(&two).unwrap();
    // A pong, then a frame too long to be one a waiting page sends: it is not looked into.
    let mut long = open(s.port, since);
    let mut both = masked(0xA, b"");
    both.extend(masked(0x1, &[b'z'; 200]));
    long.write_all(&both).unwrap();
    // Past the first look.
    std::thread::sleep(Duration::from_millis(6500));
    assert_eq!(send(s.port, "GET", "/write", &[], "").status, 200);
    for (name, page) in [("pongs", &mut pongs), ("long", &mut long)] {
        let t = text(page).unwrap_or_else(|| panic!("{}: the page was let go", name));
        assert!(t.contains("db:rows"), "{}: {}", name, t);
    }
}

/// A close among what a waiting page sent ends it at the next look.
#[test]
fn a_waiting_page_that_sends_a_close_after_a_pong_is_let_go() {
    let s = support::serve(&folder("closed").join("main.qwe"), &[]);
    let since = seq(s.port);
    let mut page = open(s.port, since);
    let mut both = masked(0xA, b"");
    both.extend(masked(0x8, b""));
    page.write_all(&both).unwrap();
    page.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    // Let go at the next look: the server closes the connection.
    let mut rest = Vec::new();
    let began = std::time::Instant::now();
    let _ = page.read_to_end(&mut rest);
    assert!(began.elapsed() < Duration::from_secs(14), "the page was kept");
    assert!(!String::from_utf8_lossy(&rest).contains("db:rows"), "{:?}", rest);
}

/// The same, on a socket made here: what a page sent is looked through without waiting, and
/// only a close, or the page hanging up, is its going.
#[test]
fn what_a_page_sent_is_looked_through_for_a_close() {
    use quillo::server::HttpRequest;
    use quillo::ws::{is_upgrade, Socket};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let req = HttpRequest {
        method: "GET".into(),
        url: "/_quillo/live".into(),
        headers: [("Host", "x"), ("Upgrade", "websocket"), ("Connection", "Upgrade"), ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="), ("Sec-WebSocket-Version", "13")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: Vec::new(),
        remote: "127.0.0.1".into(),
    };
    assert!(is_upgrade(&req));
    let page = |frames: &[u8]| -> (TcpStream, Socket) {
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let socket = Socket::accept(&req, server).expect("upgraded");
        let mut head = [0u8; 12];
        client.read_exact(&mut head).unwrap();
        assert_eq!(&head, b"HTTP/1.1 101");
        client.write_all(frames).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        (client, socket)
    };
    // Nothing sent: still there.
    let (_c, mut s) = page(b"");
    assert!(!s.gone());
    // Pongs only, read whole.
    let mut two = masked(0xA, b"");
    two.extend(masked(0xA, b"pong"));
    let (_c, mut s) = page(&two);
    assert!(!s.gone());
    assert!(s.send("still writable"), "the socket is blocking again");
    // A long frame stops the looking: no close is taken from inside it.
    let mut long = masked(0xA, b"");
    long.extend(masked(0x1, &[b'z'; 300]));
    let (_c, mut s) = page(&long);
    assert!(!s.gone());
    // A close after a pong.
    let mut closing = masked(0xA, b"");
    closing.extend(masked(0x8, b""));
    let (_c, mut s) = page(&closing);
    assert!(s.gone());
    // Hung up.
    let (c, mut s) = page(b"");
    drop(c);
    std::thread::sleep(Duration::from_millis(100));
    assert!(s.gone());
}

// ---- a channel's WebSocket (src/wt.rs) ----------------------------------------------------

/// A pong a page sends on a channel's WebSocket (unasked, as RFC 6455 §5.5.3 lets it) is taken
/// and dropped: the page stays in its channels and is answered as before.
#[test]
fn a_pong_on_a_channels_websocket_is_dropped_and_the_page_stays() {
    let d = support::temp_dir("cover-ws-channel");
    std::fs::write(d.join("main.qwe"), "channel.on(\"chat\", (m, p) => channel.broadcast(\"chat\", m))\nroute GET /peers = len(channel.peers(\"chat\"))\n").unwrap();
    let s = support::serve_with(&d.join("main.qwe"), &["--no-wt"]);
    let t = send(s.port, "POST", "/_quillo/wt-ticket", &[], "");
    assert_eq!(t.status, 200, "{}", t.body);
    let ticket: serde_json::Value = serde_json::from_str(&t.body).unwrap();
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(c, "GET /_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", ticket["ticket"].as_str().unwrap()).unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        c.read_exact(&mut b).unwrap();
        head.push(b[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
    c.write_all(&masked(0x1, b"[\"j\",\"chat\"]")).unwrap();
    assert_eq!(text(&mut c).as_deref(), Some("[\"j\",\"chat\",true]"));
    c.write_all(&masked(0xA, b"unasked")).unwrap();
    c.write_all(&masked(0x1, b"[\"m\",\"chat\",\"after the pong\"]")).unwrap();
    assert_eq!(text(&mut c).as_deref(), Some("[\"m\",\"chat\",\"after the pong\"]"));
    assert_eq!(send(s.port, "GET", "/peers", &[], "").body, "1");
}
