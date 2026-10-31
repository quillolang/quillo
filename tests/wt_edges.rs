//! Channels over real connections (src/wt.rs): a WebSocket written by hand,
//! WebTransport with its own client, and a service's channels under `--split`.
#![cfg(feature = "realtime")]

mod support;

use std::io::{Read, Write};
use std::net::{TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use wtransport::{ClientConfig, Connection, Endpoint};

const SECRET: &str = "a-test-secret-for-services-long-enough";

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-wt-edges-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// A running server, its log in a file.
struct Run {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        // As Ctrl-C does: a server started with --split stops its services too.
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Run {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn get(&self, path: &str) -> String {
        support::send(self.port, "GET", path, &[], "").body
    }

    /// What a page is given to connect.
    fn ticket(&self) -> serde_json::Value {
        let a = support::send(self.port, "POST", "/_quillo/wt-ticket", &[], "");
        assert_eq!(a.status, 200, "{}", a.body);
        serde_json::from_str(&a.body).unwrap()
    }
}

fn run(file: &Path, extra: &[&str], env: &[(&str, &str)]) -> Run {
    static N: AtomicUsize = AtomicUsize::new(0);
    let port = support::free_port();
    let log = file.with_extension(format!("log{}", N.fetch_add(1, Ordering::SeqCst)));
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .env("QUILLO_SECRET", "a test secret for the channels, long enough")
        .env_remove("QUILLO_SERVICE_SECRET")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let mut r = Run { child, port, log };
    let deadline = Instant::now() + Duration::from_secs(25);
    while !support::listening(port) {
        if let Ok(Some(status)) = r.child.try_wait() {
            panic!("quillo stopped ({}):\n{}", status, r.log());
        }
        assert!(Instant::now() < deadline, "server did not start:\n{}", r.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    answers(port);
    r
}

/// Waits for a whole answer: a server may drop a first connection as it starts.
fn answers(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut out = Vec::new();
        let asked = TcpStream::connect(("127.0.0.1", port)).and_then(|mut s| {
            s.write_all(b"GET /_quillo/ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
            s.read_to_end(&mut out)
        });
        if asked.is_ok() && out.starts_with(b"HTTP/1.1") {
            return;
        }
        assert!(Instant::now() < deadline, "the server does not answer");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A UDP port below those the system hands out, so that no other socket of
/// these tests can be given it.
fn free_udp_port() -> u16 {
    loop {
        let p = support::free_port();
        if UdpSocket::bind(("0.0.0.0", p)).is_ok() {
            return p;
        }
    }
}

// ---- a WebSocket by hand ------------------------------------------------------------

struct Ws(TcpStream);

#[derive(Debug, PartialEq)]
enum Frame {
    Text(String),
    Binary(Vec<u8>),
    Ping,
    Close,
    Pong,
}

impl Ws {
    fn open(r: &Run) -> Ws {
        let t = r.ticket();
        let mut s = TcpStream::connect(("127.0.0.1", r.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        write!(
            s,
            "GET /_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            t["ticket"].as_str().unwrap()
        )
        .unwrap();
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            head.push(b[0]);
        }
        assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
        Ws(s)
    }

    fn send(&mut self, opcode: u8, data: &[u8]) {
        let mut f = vec![0x80 | opcode];
        if data.len() < 126 {
            f.push(0x80 | data.len() as u8);
        } else {
            f.push(0x80 | 126);
            f.extend_from_slice(&(data.len() as u16).to_be_bytes());
        }
        let mask = [1, 2, 3, 4];
        f.extend_from_slice(&mask);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.0.write_all(&f).unwrap();
    }

    fn text(&mut self, t: &str) {
        self.send(1, t.as_bytes());
    }

    fn read(&mut self) -> Frame {
        let mut h = [0u8; 2];
        self.0.read_exact(&mut h).unwrap();
        let mut n = (h[1] & 0x7f) as usize;
        if n == 126 {
            let mut l = [0u8; 2];
            self.0.read_exact(&mut l).unwrap();
            n = u16::from_be_bytes(l) as usize;
        }
        let mut data = vec![0u8; n];
        self.0.read_exact(&mut data).unwrap();
        match h[0] & 0x0f {
            1 => Frame::Text(String::from_utf8(data).unwrap()),
            2 => Frame::Binary(data),
            8 => Frame::Close,
            9 => Frame::Ping,
            10 => Frame::Pong,
            o => panic!("opcode {}", o),
        }
    }
}

/// Routes that send a page more than it is reading.
const FLOOD: &str = r#"route GET /flood {
    let big = "x".repeat(1000000)
    let sent = 0
    for i in 0..40 {
        sent = sent + channel.broadcast("chat", big)
    }
    sent
}
route GET /many {
    let sent = 0
    for i in 0..4200 {
        sent = sent + channel.broadcast("chat", i, { reliable: true })
    }
    sent
}
route GET /peers"#;

const CHANNELS: &str = r#"
let heard = []
channel.on("chat", (m, p) => {
    heard = heard + [m]
    channel.broadcast("chat", m, { except: p })
})
media.on("cam")
route GET /heard = heard
route GET /say/:n(int) {
    let sent = 0
    for i in 0..n {
        sent = channel.broadcast("chat", `line ${i}`, { reliable: true })
    }
    sent
}
route GET /peers = len(channel.peers("chat"))
route GET /end = str(media.close("cam"))
route GET /pages = str(channel.peers("chat"))
"#;

fn wire(stream: &str, kind: u8, key: bool, data: &[u8]) -> Vec<u8> {
    let mut w = vec![stream.len() as u8];
    w.extend_from_slice(stream.as_bytes());
    w.extend_from_slice(&[kind, key as u8, 0, 0, 0, 7]);
    w.extend_from_slice(data);
    w
}

#[test]
fn frames_and_messages_go_over_a_websocket_both_ways() {
    let d = dir("ws");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    let r = run(&d.join("main.qwe"), &["--no-wt"], &[]);
    assert!(r.ticket().get("port").is_none(), "--no-wt: no WebTransport");
    let (mut a, mut b) = (Ws::open(&r), Ws::open(&r));
    a.text("[\"j\",\"chat\"]");
    assert_eq!(a.read(), Frame::Text("[\"j\",\"chat\",true]".into()));
    b.text("[\"j\",\"chat\"]");
    assert_eq!(b.read(), Frame::Text("[\"j\",\"chat\",true]".into()));
    a.text("[\"m\",\"chat\",\"hello\"]");
    assert_eq!(b.read(), Frame::Text("[\"m\",\"chat\",\"hello\"]".into()));
    // A ping from the page gets its pong (FEEDBACK16.md, Q-132), and changes nothing else.
    a.send(9, b"are you there");
    assert_eq!(a.read(), Frame::Pong);
    a.text("[\"m\",\"chat\",\"still here\"]");
    assert_eq!(b.read(), Frame::Text("[\"m\",\"chat\",\"still here\"]".into()));

    // What a page sends a stream reaches those watching it, as a binary message.
    b.text("[\"v\",\"cam\"]");
    a.text("[\"j\",\"media:cam\"]");
    assert_eq!(a.read(), Frame::Text("[\"j\",\"media:cam\",true]".into()));
    let frame = wire("cam", 1, true, b"a keyframe");
    a.send(2, &frame);
    assert_eq!(b.read(), Frame::Binary(frame));
    assert_eq!(r.get("/end"), "true");
    assert_eq!(a.read(), Frame::Text("[\"j\",\"media:cam\",false]".into()));
    assert_eq!(b.read(), Frame::Binary(vec![3, b'c', b'a', b'm', 0, 0, 0, 0, 0, 0]));

    // Closing: out of its channels.
    a.send(8, b"");
    assert_eq!(a.read(), Frame::Close);
    eventually(5, "the page did not leave", || (r.get("/peers") == "1").then_some(()));
}

#[test]
fn a_ping_on_a_channels_websocket_is_answered() {
    let d = dir("ws-pong");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    let r = run(&d.join("main.qwe"), &["--no-wt"], &[]);
    let mut a = Ws::open(&r);
    a.send(9, b"are you there");
    assert_eq!(a.read(), Frame::Pong);
    // Still in use: the answer did not end it.
    a.text("[\"j\",\"chat\"]");
    assert_eq!(a.read(), Frame::Text("[\"j\",\"chat\",true]".into()));
}

#[test]
fn a_page_that_went_away_is_noticed_when_it_is_written_to() {
    let d = dir("ws-gone");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    let r = run(&d.join("main.qwe"), &["--no-wt"], &[]);
    let mut a = Ws::open(&r);
    a.text("[\"j\",\"chat\"]");
    assert_eq!(a.read(), Frame::Text("[\"j\",\"chat\",true]".into()));
    drop(a);
    eventually(10, "the page was not let go", || {
        r.get("/say/20");
        (r.get("/peers") == "0").then_some(())
    });
}

#[test]
fn a_page_is_pinged_every_twenty_seconds() {
    let d = dir("ws-ping");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    let r = run(&d.join("main.qwe"), &["--no-wt"], &[]);
    let mut stays = Ws::open(&r);
    // This one's ticker finds it gone at its next ping.
    let leaves = Ws::open(&r);
    drop(leaves);
    let t = Instant::now();
    assert_eq!(stays.read(), Frame::Ping);
    assert!(t.elapsed() > Duration::from_secs(15), "{:?}", t.elapsed());
}

// ---- WebTransport with a client of its own -----------------------------------------------

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

async fn connect(r: &Run, path: Option<&str>) -> Result<Connection, String> {
    let t = r.ticket();
    let port = t["port"].as_u64().expect("no port");
    let hex = t["hash"].as_str().expect("no hash");
    let bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
    let hash = wtransport::tls::Sha256Digest::new(bytes.try_into().unwrap());
    let c = ClientConfig::builder().with_bind_default().with_server_certificate_hashes([hash]).build();
    let url = match path {
        Some(p) => format!("https://127.0.0.1:{}{}", port, p),
        None => format!("https://127.0.0.1:{}/_quillo/wt?t={}", port, t["ticket"].as_str().unwrap()),
    };
    Endpoint::client(c).unwrap().connect(url).await.map_err(|e| e.to_string())
}

fn framed(t: &str) -> Vec<u8> {
    let mut f = (t.len() as u32).to_be_bytes().to_vec();
    f.extend_from_slice(t.as_bytes());
    f
}

async fn read_framed(s: &mut wtransport::RecvStream) -> String {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await.unwrap();
    let mut b = vec![0u8; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut b).await.unwrap();
    String::from_utf8(b).unwrap()
}

/// A page connected, its first stream open, in channel `chat`.
async fn joined(r: &Run) -> (Connection, wtransport::SendStream, wtransport::RecvStream) {
    let conn = connect(r, None).await.unwrap();
    let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
    out.write_all(&framed("[\"j\",\"chat\"]")).await.unwrap();
    assert_eq!(read_framed(&mut inn).await, "[\"j\",\"chat\",true]");
    (conn, out, inn)
}

#[test]
fn over_webtransport_small_messages_are_datagrams_and_large_ones_a_stream() {
    let d = dir("wt");
    std::fs::write(d.join("main.qwe"), CHANNELS.replace("route GET /peers", "route GET /big = channel.broadcast(\"chat\", \"x\".repeat(5000))\nroute GET /small = channel.broadcast(\"chat\", \"small\")\nroute GET /peers")).unwrap();
    let r = run(&d.join("main.qwe"), &[], &[]);
    rt().block_on(async {
        let (conn, mut out, mut inn) = joined(&r).await;
        // From the page, as a datagram.
        conn.send_datagram(b"[\"m\",\"chat\",\"by datagram\"]".to_vec()).unwrap();
        conn.send_datagram(vec![0xff, 0xfe]).unwrap();
        eventually(5, "the datagram was not heard", || r.get("/heard").contains("by datagram").then_some(()));
        // To the page.
        assert_eq!(r.get("/small"), "1");
        let d = conn.receive_datagram().await.unwrap();
        assert_eq!(&d.payload()[..], b"[\"m\",\"chat\",\"small\"]");
        assert_eq!(r.get("/big"), "1");
        assert!(read_framed(&mut inn).await.len() > 5000);
        // Not UTF-8, on the stream: passed over.
        let mut bad = 2u32.to_be_bytes().to_vec();
        bad.extend_from_slice(&[0xff, 0xfe]);
        out.write_all(&bad).await.unwrap();
        out.write_all(&framed("[\"m\",\"chat\",\"after that\"]")).await.unwrap();
        eventually(5, "the stream stopped", || r.get("/heard").contains("after that").then_some(()));
    });
}

#[test]
fn a_connection_without_a_ticket_is_refused() {
    let d = dir("wt-refused");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    let r = run(&d.join("main.qwe"), &[], &[]);
    rt().block_on(async {
        for path in ["/_quillo/wt", "/_quillo/wt?t=forged", "/elsewhere?x=1"] {
            let e = connect(&r, Some(path)).await.err().unwrap_or_else(|| panic!("{} was taken", path));
            assert!(e.contains("403") || e.to_lowercase().contains("reject"), "{}: {}", path, e);
        }
    });
}

#[test]
fn a_page_that_opens_no_stream_is_let_go() {
    let d = dir("wt-silent");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    let r = run(&d.join("main.qwe"), &[], &[]);
    rt().block_on(async {
        let conn = connect(&r, None).await.unwrap();
        let t = Instant::now();
        tokio::time::timeout(Duration::from_secs(20), conn.closed()).await.expect("it stayed");
        assert!(t.elapsed() > Duration::from_secs(8), "{:?}", t.elapsed());
    });
}

#[test]
fn a_message_too_long_or_cut_short_ends_the_connection() {
    let d = dir("wt-long");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    let r = run(&d.join("main.qwe"), &[], &[]);
    rt().block_on(async {
        // Its length says more than a message may hold.
        let (conn, mut out, _inn) = joined(&r).await;
        out.write_all(&(2u32 << 20).to_be_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), conn.closed()).await.expect("it stayed");
        // Its stream ends part way through one.
        let (conn, mut out, _inn) = joined(&r).await;
        out.write_all(&100u32.to_be_bytes()).await.unwrap();
        out.write_all(b"short").await.unwrap();
        out.finish().await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), conn.closed()).await.expect("it stayed");
        // Its stream ends at a message's end.
        let (conn, mut out, _inn) = joined(&r).await;
        out.finish().await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), conn.closed()).await.expect("it stayed");
        eventually(5, "the pages did not leave", || (r.get("/peers") == "0").then_some(()));
    });
}

#[test]
fn frames_from_a_page_go_on_streams_of_their_own() {
    let d = dir("wt-frames");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    let r = run(&d.join("main.qwe"), &[], &[]);
    rt().block_on(async {
        // A page watching, and one sending.
        let viewer = connect(&r, None).await.unwrap();
        let (mut vo, _vi) = viewer.open_bi().await.unwrap().await.unwrap();
        vo.write_all(&framed("[\"v\",\"cam\"]")).await.unwrap();
        let conn = connect(&r, None).await.unwrap();
        let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
        out.write_all(&framed("[\"j\",\"media:cam\"]")).await.unwrap();
        assert_eq!(read_framed(&mut inn).await, "[\"j\",\"media:cam\",true]");

        let frame = wire("cam", 1, true, b"key");
        let mut track = conn.open_uni().await.unwrap().await.unwrap();
        let mut head = (frame.len() as u32).to_be_bytes().to_vec();
        head.extend_from_slice(&frame);
        // It may be some moments before the watching page's `v` is taken.
        tokio::time::sleep(Duration::from_millis(300)).await;
        track.write_all(&head).await.unwrap();
        let mut got = viewer.accept_uni().await.unwrap();
        assert_eq!(read_framed_bytes(&mut got).await, frame);
        // A track that says too much is dropped; so is one cut short; the others go on.
        let mut long = conn.open_uni().await.unwrap().await.unwrap();
        long.write_all(&(2u32 << 20).to_be_bytes()).await.unwrap();
        long.finish().await.unwrap();
        let mut cut = conn.open_uni().await.unwrap().await.unwrap();
        cut.write_all(&100u32.to_be_bytes()).await.unwrap();
        cut.write_all(b"cut").await.unwrap();
        cut.finish().await.unwrap();
        let next = wire("cam", 1, false, b"delta");
        let mut head = (next.len() as u32).to_be_bytes().to_vec();
        head.extend_from_slice(&next);
        track.write_all(&head).await.unwrap();
        assert_eq!(read_framed_bytes(&mut got).await, next);
        // Audio is a track of its own, on a stream of its own.
        let audio = wire("cam", 2, true, b"tone");
        let mut head = (audio.len() as u32).to_be_bytes().to_vec();
        head.extend_from_slice(&audio);
        track.write_all(&head).await.unwrap();
        let mut got2 = viewer.accept_uni().await.unwrap();
        assert_eq!(read_framed_bytes(&mut got2).await, audio);
        track.finish().await.unwrap();
        // The page stops reading: the stream stops being written to.
        got.stop(0u32.into());
        tokio::time::sleep(Duration::from_millis(200)).await;
        let last = wire("cam", 1, true, b"key2");
        let mut t2 = conn.open_uni().await.unwrap().await.unwrap();
        for _ in 0..3 {
            let mut head = (last.len() as u32).to_be_bytes().to_vec();
            head.extend_from_slice(&last);
            t2.write_all(&head).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // The connection itself stays.
        assert_eq!(r.get("/peers"), "0");
    });
}

async fn read_framed_bytes(s: &mut wtransport::RecvStream) -> Vec<u8> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await.unwrap();
    let mut b = vec![0u8; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut b).await.unwrap();
    b
}

#[test]
fn a_page_that_stops_reading_and_goes_is_let_go() {
    let d = dir("wt-stuck");
    std::fs::write(d.join("main.qwe"), CHANNELS.replace("route GET /peers", FLOOD)).unwrap();
    let r = run(&d.join("main.qwe"), &[], &[]);
    rt().block_on(async {
        let (conn, _out, _inn) = joined(&r).await;
        // More than flow control lets through while the page reads nothing.
        assert_eq!(r.get("/flood"), "40");
        eventually(5, "the page is not there", || (r.get("/peers") == "1").then_some(()));
        conn.close(0u32.into(), b"");
        drop(conn);
        eventually(30, "the page was not let go", || (r.get("/peers") == "0").then_some(()));
    });
}

#[test]
fn a_page_on_webtransport_that_falls_too_far_behind_is_let_go() {
    let d = dir("wt-behind");
    std::fs::write(d.join("main.qwe"), CHANNELS.replace("route GET /peers", FLOOD)).unwrap();
    let r = run(&d.join("main.qwe"), &[], &[]);
    rt().block_on(async {
        let (conn, _out, mut inn) = joined(&r).await;
        // Its stream is full, and then its queue.
        assert_eq!(r.get("/flood"), "40");
        let sent: i64 = r.get("/many").parse().unwrap();
        assert!(sent < 4200, "{}", sent);
        // Reading again, it gets what was queued, and then the connection ends.
        let mut buf = vec![0u8; 1 << 16];
        let mut total = 0;
        while let Ok(Some(n)) = tokio::time::timeout(Duration::from_secs(20), inn.read(&mut buf)).await.expect("it stayed") {
            total += n;
        }
        assert!(total >= 39_000_000, "{}", total);
        tokio::time::timeout(Duration::from_secs(10), conn.closed()).await.expect("it stayed");
        eventually(5, "the page was not let go", || (r.get("/peers") == "0").then_some(()));
    });
}

#[test]
fn webtransport_listens_where_it_is_told_or_says_why_it_cannot() {
    let d = dir("wt-port");
    std::fs::write(d.join("main.qwe"), CHANNELS).unwrap();
    // Another port.
    let wt_port = free_udp_port();
    let r = run(&d.join("main.qwe"), &["--wt-port", &wt_port.to_string()], &[]);
    assert_eq!(r.ticket()["port"].as_u64(), Some(wt_port as u64));
    assert!(r.log().contains(&format!("WebTransport on UDP port {}", wt_port)), "{}", r.log());
    drop(r);

    // A port someone has: pages use a WebSocket.
    let taken = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let r = run(&d.join("main.qwe"), &["--host", "127.0.0.1", "--wt-port", &port.to_string()], &[]);
    let t = r.ticket();
    assert!(t.get("port").is_none() && t["ticket"].is_string(), "{}", t);
    assert!(r.log().contains(&format!("WebTransport is off (cannot listen on UDP port {}", port)), "{}", r.log());
    // Asked again, it does not try again.
    assert!(r.ticket().get("port").is_none());
    assert_eq!(r.log().matches("WebTransport is off").count(), 1);
    drop(r);

    // A certificate given: no hash for pages to take it by.
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    std::fs::write(d.join("cert.pem"), cert.cert.pem()).unwrap();
    std::fs::write(d.join("key.pem"), cert.signing_key.serialize_pem()).unwrap();
    let (c, k) = (d.join("cert.pem").display().to_string(), d.join("key.pem").display().to_string());
    let r = run(&d.join("main.qwe"), &["--tls-cert", &c, "--tls-key", &k], &[]);
    let t = r.ticket();
    assert_eq!(t["port"].as_u64(), Some(r.port as u64), "{}", t);
    assert!(t.get("hash").is_none(), "{}", t);
    drop(r);

    // One that cannot be read.
    let r = run(&d.join("main.qwe"), &["--tls-cert", &d.join("none.pem").display().to_string(), "--tls-key", &k], &[]);
    assert!(r.ticket().get("port").is_none());
    assert!(r.log().contains("WebTransport is off (cannot read the certificate"), "{}", r.log());
}

// ---- a service's channels under --split -------------------------------------------------------

const SPLIT_SERVICE: &str = r#"
let said = []
channel.on("room", (m, p) => {
    said = said + [m]
    channel.send("room", p, "only to you")
    channel.broadcast("room", m, { except: p })
}, {
    join: (p) => {
        channel.broadcast("room", "someone comes")
        p.session.banned != true
    },
    leave: (p) => channel.broadcast("room", "someone goes"),
})
route GET /said = said
route GET /in-room = channel.peers("room")
route GET /to/:who = str(channel.send("room", who, "by id"))
route GET /all = str(channel.broadcast("room", "to all", { reliable: true }))
"#;

const SPLIT_APP: &str = r#"
service /talk = "talk.qwe"
route GET / = "app"
route GET /peers = channel.peers("room")
route GET /plain = str(channel.broadcast("nobodys", "x"))
"#;

#[test]
fn a_services_channels_reach_the_apps_pages_when_it_runs_apart() {
    let d = dir("split");
    std::fs::write(d.join("talk.qwe"), SPLIT_SERVICE).unwrap();
    std::fs::write(d.join("app.qwe"), SPLIT_APP).unwrap();
    let r = run(&d.join("app.qwe"), &["--split", "--no-wt"], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    eventually(20, "the service did not answer", || (r.get("/talk/said") == "[]").then_some(()));

    let (mut a, mut b) = (Ws::open(&r), Ws::open(&r));
    a.text("[\"j\",\"room\"]");
    assert_eq!(a.read(), Frame::Text("[\"j\",\"room\",true]".into()));
    // What its `join` broadcast reaches the page coming in, after the yes.
    assert_eq!(a.read(), Frame::Text("[\"m\",\"room\",\"someone comes\"]".into()));
    b.text("[\"j\",\"room\"]");
    assert_eq!(b.read(), Frame::Text("[\"j\",\"room\",true]".into()));
    assert_eq!(b.read(), Frame::Text("[\"m\",\"room\",\"someone comes\"]".into()));
    assert_eq!(a.read(), Frame::Text("[\"m\",\"room\",\"someone comes\"]".into()));

    // A page's message reaches the service, and what it sends comes back.
    a.text("[\"m\",\"room\",\"hello\"]");
    assert_eq!(a.read(), Frame::Text("[\"m\",\"room\",\"only to you\"]".into()));
    assert_eq!(b.read(), Frame::Text("[\"m\",\"room\",\"hello\"]".into()));
    assert_eq!(r.get("/talk/said"), "[\"hello\"]");

    // The service sees the app's pages, and can reach each.
    let peers: Vec<String> = serde_json::from_str(&r.get("/talk/in-room")).unwrap();
    assert_eq!(peers.len(), 2);
    assert_eq!(serde_json::from_str::<Vec<String>>(&r.get("/peers")).unwrap(), peers);
    for id in &peers {
        assert_eq!(r.get(&format!("/talk/to/{}", id)), "true");
    }
    assert_eq!(r.get("/talk/to/ffffff"), "false");
    assert_eq!(r.get("/talk/all"), "2");
    for page in [&mut a, &mut b] {
        assert_eq!(page.read(), Frame::Text("[\"m\",\"room\",\"by id\"]".into()));
        assert_eq!(page.read(), Frame::Text("[\"m\",\"room\",\"to all\"]".into()));
    }

    // Leaving is heard there too.
    a.text("[\"l\",\"room\"]");
    assert_eq!(b.read(), Frame::Text("[\"m\",\"room\",\"someone goes\"]".into()));

    // A channel that is no one's: asked once in a while, and pages may join it.
    let mut c = Ws::open(&r);
    c.text("[\"j\",\"nobodys\"]");
    assert_eq!(c.read(), Frame::Text("[\"j\",\"nobodys\",true]".into()));
    c.text("[\"j\",\"nobodys2\"]");
    assert_eq!(c.read(), Frame::Text("[\"j\",\"nobodys2\",true]".into()));
    assert_eq!(r.get("/plain"), "1");
    // The service gone: a page asking to join hears no. It comes back within
    // a second or so, so this is done at once.
    std::thread::sleep(Duration::from_millis(2100));
    let talk = d.join("talk.qwe").display().to_string();
    let pids = String::from_utf8(Command::new("pgrep").args(["-f", &talk]).output().unwrap().stdout).unwrap();
    assert!(!pids.trim().is_empty());
    for pid in pids.split_whitespace() {
        let _ = Command::new("kill").args(["-9", pid]).status();
    }
    let mut e = Ws::open(&r);
    e.text("[\"j\",\"room\"]");
    assert_eq!(e.read(), Frame::Text("[\"j\",\"room\",false]".into()));
    eventually(5, "the app did not say so", || r.log().contains("channel room: service talk did not take what pages sent").then_some(()));
    // Asked which channels are its, it does not answer: they are no one's.
    e.text("[\"j\",\"asked-again\"]");
    assert_eq!(e.read(), Frame::Text("[\"j\",\"asked-again\",true]".into()));
}

#[test]
fn a_service_whose_app_cannot_be_reached_sends_to_no_one() {
    let d = dir("no-app");
    std::fs::write(d.join("talk.qwe"), "route GET /b = str(channel.broadcast(\"room\", 1))\nroute GET /s = str(channel.send(\"room\", \"ff\", 1))\nroute GET /p = str(channel.peers(\"room\"))\n").unwrap();
    // No secret to sign with; then an app that is not there.
    for env in [vec![], vec![("QUILLO_SERVICE_SECRET", SECRET)]] {
        let mut e = vec![("QUILLO_APP_URL", "http://127.0.0.1:1")];
        e.extend(env);
        let r = run(&d.join("talk.qwe"), &[], &e);
        assert_eq!(r.get("/b"), "0");
        assert_eq!(r.get("/s"), "false");
        assert_eq!(r.get("/p"), "[]");
    }
}

// ---- a server stopping ----------------------------------------------------------------------

#[test]
fn a_server_taking_rtp_stops_when_asked() {
    let d = dir("stop");
    let udp = free_udp_port();
    std::fs::write(d.join("main.qwe"), format!("media.rtp(\"cam\", {{ video: {}, host: \"127.0.0.1\" }})\nroute GET / = str(len(media.streams()))\nroute GET /slow {{\n sleep(500)\n \"finished\"\n}}\n", udp)).unwrap();
    let mut r = run(&d.join("main.qwe"), &[], &[]);
    assert_eq!(r.get("/"), "1");
    // A request in progress is answered before it stops.
    let port = r.port;
    let slow = std::thread::spawn(move || support::send(port, "GET", "/slow", &[], "").body);
    std::thread::sleep(Duration::from_millis(150));
    let _ = Command::new("kill").args(["-INT", &r.child.id().to_string()]).status();
    assert_eq!(slow.join().unwrap(), "finished");
    let status = eventually(10, "it did not stop", || r.child.try_wait().unwrap());
    assert!(status.success(), "{}: {}", status, r.log());
}
