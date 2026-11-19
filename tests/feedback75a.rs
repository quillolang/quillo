//! FEEDBACK75.md, slice A (realtime): channels, media, sockets, sessions and UDP. A stream's name
//! for a `*` rule (Q-1050), inboxes held by bytes (Q-1056), tickets bound to their program
//! (Q-1061), a split service's channels while it is down (Q-1065), RTP on this machine and from
//! one sender (Q-1066), messages begun and never finished (Q-1073, Q-1074), mounted programs'
//! session cookies (Q-1077) and UDP fairness by packets (Q-1094). Branch feedback75.

mod support;

use std::io::{Read, Write};
use std::net::{TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const SECRET: &str = "a-test-secret-for-services-long-enough";

/// A folder of its own, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, files: &[(&str, &str)]) -> Dir {
        let d = support::temp_dir(&format!("f75a-{}", name));
        for (f, src) in files {
            std::fs::write(d.join(f), src).unwrap();
        }
        Dir(d)
    }

    fn file(&self, f: &str) -> PathBuf {
        self.0.join(f)
    }
}

/// `quillo check` takes the program (every snippet here passes it).
fn checks(file: &Path) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(file).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(s) = c.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            let _ = c.wait();
            panic!("quillo check {} took too long", file.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut out = String::new();
    let _ = c.stdout.take().unwrap().read_to_string(&mut out);
    let _ = c.stderr.take().unwrap().read_to_string(&mut out);
    assert!(status.success(), "quillo check {}: {}", file.display(), out);
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

/// A running server, its log in a file, stopped when dropped (and when the tests are gone).
struct Run {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        // As Ctrl-C does: a server started with --split stops its services too.
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
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

    fn get_as(&self, path: &str, cookie: &str) -> support::Answer {
        support::send(self.port, "GET", path, &[("Cookie", cookie)], "")
    }

    /// What a page of the program at `base` is given to connect, with `cookie`.
    fn ticket(&self, base: &str, cookie: &str) -> serde_json::Value {
        let path = format!("{}/_quillo/wt-ticket", base);
        let mut h = vec![("Sec-Fetch-Site", "same-origin")];
        if !cookie.is_empty() {
            h.push(("Cookie", cookie));
        }
        let a = support::send(self.port, "POST", &path, &h, "");
        assert_eq!(a.status, 200, "{}", a.body);
        serde_json::from_str(&a.body).unwrap()
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }
}

/// `quillo run file --port 0` with `extra`, its log in a file.
fn run(file: &Path, extra: &[&str], env: &[(&str, &str)]) -> Run {
    static N: AtomicUsize = AtomicUsize::new(0);
    checks(file);
    let log = file.with_extension(format!("log{}", N.fetch_add(1, Ordering::SeqCst)));
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", "0"])
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
    let mut r = Run { child, port: 0, log };
    r.port = support::port_in_log(&mut r.child, &r.log, Duration::from_secs(25));
    let deadline = Instant::now() + Duration::from_secs(10);
    while support::send(r.port, "GET", "/_quillo/ready", &[], "").status == 0 {
        assert!(Instant::now() < deadline, "the server does not answer");
        std::thread::sleep(Duration::from_millis(20));
    }
    r
}

/// The session cookie an answer sets, as a `Cookie` header's pair.
fn session_cookie(a: &support::Answer) -> String {
    a.headers.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session")).map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap_or_else(|| panic!("no session cookie: {:?}", a.headers))
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
    /// A page of the program at `base` connected with `cookie`'s session, at `path`'s `/_quillo/wt`.
    fn open_at(r: &Run, base: &str, cookie: &str, path: &str) -> Ws {
        let t = r.ticket(base, cookie);
        let mut s = TcpStream::connect(("127.0.0.1", r.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        write!(
            s,
            "GET {}/_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            path,
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

    fn open(r: &Run) -> Ws {
        Ws::open_at(r, "", "", "")
    }

    /// A page connected from `ip` (`::1`, another client than 127.0.0.1), of a server on `::`.
    fn open_from(r: &Run, ip: &str) -> Ws {
        let t = r.ticket("", "");
        let mut s = TcpStream::connect((ip, r.port)).unwrap();
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

    /// The head of a frame that says `len` bytes, masked, with none of them.
    fn head_only(&mut self, fin: bool, opcode: u8, len: u64) -> std::io::Result<()> {
        let mut f = vec![if fin { 0x80 } else { 0 } | opcode, 0x80 | 127];
        f.extend_from_slice(&len.to_be_bytes());
        f.extend_from_slice(&[1, 2, 3, 4]);
        self.0.write_all(&f)
    }

    /// A frame: `fin`, its opcode, its data (masked, as a page's are).
    fn frame(&mut self, fin: bool, opcode: u8, data: &[u8]) -> std::io::Result<()> {
        let mut f = vec![if fin { 0x80 } else { 0 } | opcode];
        if data.len() < 126 {
            f.push(0x80 | data.len() as u8);
        } else if data.len() < 65536 {
            f.push(0x80 | 126);
            f.extend_from_slice(&(data.len() as u16).to_be_bytes());
        } else {
            f.push(0x80 | 127);
            f.extend_from_slice(&(data.len() as u64).to_be_bytes());
        }
        let mask = [1, 2, 3, 4];
        f.extend_from_slice(&mask);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.0.write_all(&f)
    }

    fn text(&mut self, t: &str) {
        self.frame(true, 1, t.as_bytes()).unwrap();
    }

    fn read(&mut self) -> Frame {
        self.try_read().unwrap()
    }

    fn try_read(&mut self) -> std::io::Result<Frame> {
        let mut h = [0u8; 2];
        self.0.read_exact(&mut h)?;
        let mut n = (h[1] & 0x7f) as usize;
        if n == 126 {
            let mut l = [0u8; 2];
            self.0.read_exact(&mut l)?;
            n = u16::from_be_bytes(l) as usize;
        } else if n == 127 {
            let mut l = [0u8; 8];
            self.0.read_exact(&mut l)?;
            n = u64::from_be_bytes(l) as usize;
        }
        let mut data = vec![0u8; n];
        self.0.read_exact(&mut data)?;
        Ok(match h[0] & 0x0f {
            1 => Frame::Text(String::from_utf8(data).unwrap()),
            2 => Frame::Binary(data),
            8 => Frame::Close,
            9 => Frame::Ping,
            10 => Frame::Pong,
            o => panic!("opcode {}", o),
        })
    }

    /// The next text frame, pings and a stream's frames passed over.
    fn text_frame(&mut self) -> String {
        loop {
            match self.read() {
                Frame::Text(t) => return t,
                Frame::Ping | Frame::Pong | Frame::Binary(_) => {}
                f => panic!("{:?}", f),
            }
        }
    }

    /// Asks to join `ch` and gives the answer.
    fn join(&mut self, ch: &str) -> bool {
        self.text(&serde_json::json!(["j", ch]).to_string());
        let t = self.text_frame();
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        assert_eq!((v[0].as_str(), v[1].as_str()), (Some("j"), Some(ch)), "{}", t);
        v[2].as_bool().unwrap()
    }

    /// Whether the server closed the connection (a close frame or the end), within `within`.
    fn closed_within(&mut self, within: Duration) -> bool {
        let _ = self.0.set_read_timeout(Some(within));
        let start = Instant::now();
        loop {
            match self.try_read() {
                Ok(Frame::Close) => return true,
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => return false,
                Err(_) => return true,
            }
            if start.elapsed() > within {
                return false;
            }
        }
    }
}

// ---- Q-1050: a `*` rule is told which stream ------------------------------------------

const CALLS: &str = r#"
let watched = []
route writes GET /login ? { user: str } {
    set_session({ user: user })
    "in"
}
// A stream per person: each sends only their own, and watches only their room's.
media.on("call:*", {
    publish: (peer, name) => session().user != null && name.ends_with(":" + session().user),
    watch: (peer, name) => {
        watched = watched + [name]
        session().user != null && name.starts_with("call:42:")
    },
})
// The rules of one parameter, and of none, as they were.
media.on("cam", { publish: (peer) => peer.session.user == "ana", watch: () => true })
channel.on("room", (m, p) => channel.broadcast("room", m), { join: (peer, name) => name == "room" && peer.session.user != null })
channel.on("hall", (m, p) => channel.broadcast("hall", m), { join: (peer) => true })
route GET /watched = watched
route GET /viewers = media.streams()
"#;

/// The guide's call with one stream per person: a rule for `call:*` is given the stream's name
/// second, so bob can no longer send into ana's stream, nor watch a room he is not in; the stream
/// a watch asked for reaches `watch`, a channel's `join` is given its name too, and rules of one
/// parameter, or none, are called as they were (FEEDBACK75.md, Q-1050; branch feedback75)
#[test]
fn a_star_streams_rules_are_told_which_stream_a_page_asks_for() {
    let d = Dir::new("calls", &[("main.qwe", CALLS)]);
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    let ana = session_cookie(&r.get_as("/login?user=ana", ""));
    let bob = session_cookie(&r.get_as("/login?user=bob", ""));
    let mut a = Ws::open_at(&r, "", &ana, "");
    let mut b = Ws::open_at(&r, "", &bob, "");
    let mut nobody = Ws::open(&r);
    // Each sends their own stream, and no one else's.
    assert!(a.join("media:call:42:ana"));
    let mut frame = vec![11u8];
    frame.extend_from_slice(b"call:42:ana");
    frame.extend_from_slice(&[1, 1, 0, 0, 0, 7]);
    frame.extend_from_slice(b"key");
    a.frame(true, 2, &frame).unwrap();
    assert!(!b.join("media:call:42:ana"), "bob sent into ana's stream");
    assert!(b.join("media:call:42:bob"));
    assert!(!nobody.join("media:call:42:"), "no one signed in, and an empty name after the room");
    // The one at the limit: a name that ends with the user, as the rule says, and one byte more.
    assert!(!a.join("media:call:42:anax"));
    // What a watch asked for is what `watch` is told: the room's, and another's, refused.
    b.text("[\"v\",\"call:42:ana\"]");
    b.text("[\"v\",\"call:7:ana\"]");
    eventually(10, "the watches were not asked", || (r.get("/watched") == "[\"call:42:ana\",\"call:7:ana\"]").then_some(()));
    eventually(10, "bob does not watch ana's call", || r.get("/viewers").contains("\"viewers\":1").then_some(()));
    // Rules of one parameter and of none.
    assert!(a.join("media:cam"));
    assert!(!b.join("media:cam"));
    // A channel's `join` is told its name; one of one parameter is called as before.
    assert!(a.join("room"));
    assert!(!nobody.join("room"));
    assert!(nobody.join("hall"));
}

// ---- Q-1056: an inbox is held by bytes ------------------------------------------------

const SLOW_CHAT: &str = r#"
let go = false
let got = []
channel.on("chat", (m, p) => {
    // Stands in for a database that does not answer yet.
    while !go {
        sleep(20)
    }
    got = got + [len(str(m))]
}, { join: (peer) => true })
route writes GET /go {
    go = true
    "going"
}
route GET /got = got
"#;

/// Sends `n` messages of `chars` copies of `c` on channel `chat`, then a small one marking the end.
fn send_many(ws: &mut Ws, n: usize, c: &str, chars: usize) {
    let text = serde_json::json!(["m", "chat", c.repeat(chars)]).to_string();
    for _ in 0..n {
        ws.text(&text);
    }
}

/// The handler's lengths once it has taken all it will: none comes for a second.
fn settled(r: &Run) -> Vec<usize> {
    let mut last = String::new();
    let mut since = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let now = r.get("/got");
        if now != last {
            last = now;
            since = Instant::now();
        } else if since.elapsed() > Duration::from_secs(1) {
            return serde_json::from_str(&last).unwrap();
        }
        assert!(Instant::now() < deadline, "the handler never settled");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Sends `msgs` on `ws` from a thread of its own, as a page does: the server may stop reading it.
fn send_apart(mut ws: Ws, msgs: Vec<String>) -> std::thread::JoinHandle<Ws> {
    std::thread::spawn(move || {
        for m in &msgs {
            if ws.frame(true, 1, m.as_bytes()).is_err() {
                break;
            }
        }
        ws
    })
}

/// Waits for `h` to finish, for at most `secs`: whether it did.
fn finished<T>(h: &std::thread::JoinHandle<T>, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !h.is_finished() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// One page on one socket can no longer have the server hold more than its share of bytes for a
/// handler that waits, and loses nothing for it: under `--max-memory 1000000`, 64 messages of
/// exactly 250,000 bytes as JSON (four fit), and the same in two-byte `ü`s (bytes, not
/// characters), are not all read while the handler waits (the page's writes stop, its socket no
/// longer read), and all 64 are handled once it goes on, then the next (FEEDBACK75.md, Q-1056
/// and its review: wait, not drop; branch feedback75)
#[test]
fn a_pages_messages_wait_within_their_bytes_held_to_max_memory() {
    for (c, chars) in [("x", 249_998), ("ü", 124_999)] {
        let d = Dir::new(&format!("inbox-{}-{}", chars, c.len()), &[("main.qwe", SLOW_CHAT)]);
        let r = run(&d.file("main.qwe"), &["--no-wt", "--max-memory", "1000000", "--metrics"], &[]);
        let mut ws = Ws::open(&r);
        assert!(ws.join("chat"));
        let msg = serde_json::json!(["m", "chat", c.repeat(chars)]).to_string();
        // 16 MB: more than the connection's buffers hold, so a server that stops reading stops it.
        let sending = send_apart(ws, vec![msg; 64]);
        // The server stops reading the page: what it sends waits in its connection.
        assert!(!finished(&sending, 2), "the page's messages were all read while the handler waited");
        assert_eq!(r.get("/go"), "going");
        assert!(finished(&sending, 30), "the page never got to send");
        let deadline = Instant::now() + Duration::from_secs(30);
        let got = loop {
            let g = settled(&r);
            if g.len() == 64 {
                break g;
            }
            let metrics = r.get("/_quillo/metrics");
            let dropped = metrics.lines().find(|l| l.starts_with("quillo_channel_dropped_total")).unwrap_or("");
            assert!(Instant::now() < deadline, "{} of 64 handled ({}); the log ends:\n{}", g.len(), dropped, r.log().lines().rev().take(20).collect::<Vec<_>>().join("\n"));
        };
        assert!(got.iter().all(|n| *n == chars), "{:?}", got);
        // The page goes on: its next message is taken.
        let mut ws = sending.join().unwrap();
        ws.text("[\"m\",\"chat\",\"after\"]");
        eventually(10, "the next message was not taken", || (settled(&r).len() == 65).then_some(()));
    }
}

/// Without `--max-memory`, a page's share (4 MiB) is its own: a page that sent twenty messages of
/// 1 MB waits once four are waiting (twenty, as the system's buffers on loopback may take 8 MiB
/// of what is not read), and another page's messages, an empty one and a short one, still go in
/// at once; all twenty-two are handled once the handler goes on, none lost; small messages go as
/// they did (FEEDBACK75.md, Q-1056 and its review; branch feedback75)
#[test]
fn one_pages_messages_do_not_take_another_pages_place() {
    let d = Dir::new("inbox-share", &[("main.qwe", SLOW_CHAT)]);
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    let (mut hog, mut other) = (Ws::open(&r), Ws::open(&r));
    assert!(hog.join("chat"));
    assert!(other.join("chat"));
    let big = serde_json::json!(["m", "chat", "x".repeat(1_000_000)]).to_string();
    let sending = send_apart(hog, vec![big; 20]);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!sending.is_finished(), "all twenty were read while the handler waited");
    other.text("[\"m\",\"chat\"]");
    other.text("[\"m\",\"chat\",\"hi\"]");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(r.get("/go"), "going");
    assert!(finished(&sending, 30));
    let got = eventually(30, "the messages were not all handled", || {
        let g = settled(&r);
        (g.len() == 22).then_some(g)
    });
    assert_eq!(got.iter().filter(|n| **n == 1_000_000).count(), 20, "{:?}", got);
    // `null` and `hi`: the other page's.
    assert_eq!(got.iter().filter(|n| **n < 10).count(), 2, "{:?}", got);
    // The ordinary form: a burst of small messages, all taken.
    send_many(&mut other, 200, "y", 10);
    eventually(20, "the small messages were not all taken", || (settled(&r).len() == 222).then_some(()));
}

/// A page bursting 5,000 messages of 2 KB ahead of its channel's handler loses none: it waits for
/// room, its socket no longer read, while another channel's pages, of its own address and of
/// another, go on at once (FEEDBACK75.md, Q-1056's review: one page lost 52 of 40,000; branch
/// feedback75)
#[test]
fn a_burst_ahead_of_its_handler_waits_and_loses_nothing() {
    let src = TWO_CHANNELS.replace("while !go {", "slow = slow + 1\n    while !go {").replace("let fast = []", "let fast = []\nlet slow = 0").replace("route GET /fast = fast", "route GET /fast = fast\nroute GET /slow = slow");
    let d = Dir::new("inbox-burst", &[("main.qwe", &src)]);
    let v6 = has_v6();
    let r = run(&d.file("main.qwe"), if v6 { &["--no-wt", "--host", "::"][..] } else { &["--no-wt"][..] }, &[]);
    let (mut burst, mut same, mut other) = (Ws::open(&r), Ws::open(&r), if v6 { Ws::open_from(&r, "::1") } else { Ws::open(&r) });
    assert!(burst.join("slow") && same.join("fast") && other.join("fast"));
    let msg = format!("[\"m\",\"slow\",\"{}\"]", "b".repeat(2000));
    let sending = send_apart(burst, vec![msg; 5000]);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!sending.is_finished(), "the burst was all read while its handler waited");
    for ws in [&mut same, &mut other] {
        for _ in 0..10 {
            ws.text("[\"m\",\"fast\",\"f\"]");
        }
    }
    eventually(10, "the other channel stalled with the slow one", || (r.get("/fast").matches(',').count() == 19).then_some(()));
    assert_eq!(r.get("/go"), "going");
    assert!(finished(&sending, 60), "the burst never got to send");
    eventually(60, "the burst lost messages", || (r.get("/slow") == "5000").then_some(()));
}

const TWO_CHANNELS: &str = r#"
let go = false
let fast = []
channel.on("slow", (m, p) => {
    // A handler that does not keep up.
    while !go {
        sleep(20)
    }
}, { join: (peer) => true })
channel.on("fast", (m, p) => {
    fast = fast + [len(str(m))]
}, { join: (peer) => true })
route writes GET /go {
    go = true
    "going"
}
route GET /fast = fast
"#;

/// One address's pages keeping a slow channel full have no more than their client's share waiting
/// (an eighth of all; past it they wait), and a channel no more than a quarter: under
/// `--max-memory 64MB`, 20 pages of 127.0.0.1 each sending 4 messages of 1 MB to `slow`, a page of
/// ::1 still has all its 20 messages of 50 KB to `fast` heard, where it had none (found in review of FEEDBACK75.md, Q-1056; branch
/// feedback75)
#[test]
fn one_address_or_one_channel_does_not_take_everyones_room() {
    if !has_v6() {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    }
    let d = Dir::new("inbox-shares", &[("main.qwe", TWO_CHANNELS)]);
    let r = run(&d.file("main.qwe"), &["--no-wt", "--max-memory", "64MB", "--host", "::"], &[]);
    // All in first: the slow channel's joins wait behind its handler once it has a message. Each
    // sends from a thread of its own: once their client's share is full, they wait.
    let mut hogs: Vec<Ws> = (0..20).map(|_| Ws::open(&r)).collect();
    for ws in hogs.iter_mut() {
        assert!(ws.join("slow"));
    }
    let big = format!("[\"m\",\"slow\",\"{}\"]", "x".repeat(1_000_000));
    let _sending: Vec<_> = hogs.into_iter().map(|ws| send_apart(ws, vec![big.clone(); 4])).collect();
    std::thread::sleep(Duration::from_millis(2000));
    let mut other = Ws::open_from(&r, "::1");
    assert!(other.join("fast"));
    let msg = format!("[\"m\",\"fast\",\"{}\"]", "y".repeat(50_000));
    for _ in 0..20 {
        other.text(&msg);
    }
    let heard = eventually(20, "the other client's messages were not all heard", || {
        let h: Vec<i64> = serde_json::from_str(&r.get("/fast")).unwrap();
        (h.len() == 20).then_some(h)
    });
    assert!(heard.iter().all(|n| *n == 50_000), "{:?}", heard);
    assert_eq!(r.get("/go"), "going");
}

// ---- Q-1061: a ticket is its own program's ---------------------------------------------

const MOUNTS_ROOT: &str = r#"
mount /a = "a.qwe"
mount /b = "b.qwe"
service /talk = "talk.qwe"
// A guest picks the name shown beside posts: anyone sets it.
route writes GET /name ? { n: str } {
    set_session({ user: n })
    "named"
}
channel.on("lobby", (m, p) => channel.send("lobby", p, { seen: p.session }), { join: (peer) => peer.session.user != null })
"#;

const MOUNT_A: &str = r#"
channel.on("a-room", (m, p) => null)
// Anyone signs up, and is user 1 here.
route writes GET /login {
    set_session({ user: 1 })
    "signed in to A as user 1"
}
route GET /me = session()
"#;

const MOUNT_B: &str = r#"
route writes GET /login {
    set_session({ user: 1, admin: true })
    "B's own administrator"
}
route GET /me = session()
channel.on("admin", (msg, peer) => {
    channel.send("admin", peer, { secret: "B admin data", seen: peer.session, also: session() })
}, { join: (peer) => peer.session.user == 1 })
channel.on("ops", (m, p) => channel.broadcast("ops", m), { join: (peer) => peer.session.user == "admin" })
"#;

const TALK: &str = r#"
channel.on("talk", (m, p) => channel.send("talk", p, { seen: p.session }), { join: (peer) => peer.session.user != null })
"#;

/// A ticket from mount A is no session in mount B's channels, at B's own address or the root's;
/// a session set freely in the root does not pass B's `join`; B's own ticket does, and so does the
/// root's own session in its own channel and in a service it declares under a prefix (part of
/// it, as one apart is) (FEEDBACK75.md, Q-1061; branch feedback75)
#[test]
fn a_ticket_carries_its_session_only_into_its_own_programs_channels() {
    let d = Dir::new("mounts", &[("main.qwe", MOUNTS_ROOT), ("a.qwe", MOUNT_A), ("b.qwe", MOUNT_B), ("talk.qwe", TALK)]);
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    let a_cookie = session_cookie(&r.get_as("/a/login", ""));
    // B sees no session of A's, as promised.
    assert_eq!(r.get_as("/b/me", &a_cookie).body, "{}");
    // A's ticket, at B's address and at the root's: B's `join` sees no session.
    for path in ["/b", "", "/a"] {
        let mut ws = Ws::open_at(&r, "/a", &a_cookie, path);
        assert!(!ws.join("admin"), "A's session passed B's join at {:?}", path);
    }
    // The root's session, which a guest picks freely: B's `join` sees none.
    let root = session_cookie(&r.get_as("/name?n=admin", ""));
    let mut ws = Ws::open_at(&r, "", &root, "");
    assert!(!ws.join("ops"), "the root's session passed B's join");
    // The root's own channel and its service's see the root's session, as before.
    assert!(ws.join("lobby"));
    ws.text("[\"m\",\"lobby\",1]");
    assert_eq!(ws.text_frame(), "[\"m\",\"lobby\",{\"seen\":{\"user\":\"admin\"}}]");
    assert!(ws.join("talk"));
    ws.text("[\"m\",\"talk\",1]");
    assert_eq!(ws.text_frame(), "[\"m\",\"talk\",{\"seen\":{\"user\":\"admin\"}}]");
    // B's own page, with B's own session: let in, and its handler sees it.
    let b_cookie = session_cookie(&r.get_as("/b/login", ""));
    let mut own = Ws::open_at(&r, "/b", &b_cookie, "/b");
    assert!(own.join("admin"));
    own.text("[\"m\",\"admin\",{}]");
    assert_eq!(own.text_frame(), "[\"m\",\"admin\",{\"secret\":\"B admin data\",\"seen\":{\"user\":1,\"admin\":true},\"also\":{\"user\":1,\"admin\":true}}]");
    // A page of the root with no session in B's open channel... B's `ops` wants its admin: none.
    let mut anon = Ws::open(&r);
    assert!(!anon.join("ops"));
}

/// A ticket says the mount point of the program that gave it out (`""` at the root, `/b` for a
/// program mounted there, `/b/c` nested), with the session it seals; a forged one is refused as
/// before, and one made before tickets said it (as 0.8.8 made them) is taken as the root program's
/// (FEEDBACK75.md, Q-1061; branch feedback75)
#[test]
fn a_ticket_says_which_program_gave_it_out() {
    let d = Dir::new(
        "ticket-base",
        &[
            ("main.qwe", "mount /b = \"b.qwe\"\nroute GET / = \"root\"\nroute GET /n = len(channel.peers(\"x\"))\n"),
            ("b.qwe", "mount /c = \"c.qwe\"\nroute GET / = \"b\"\nroute GET /n = len(channel.peers(\"x\"))\n"),
            ("c.qwe", MOUNT_B),
        ],
    );
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    for (base, want) in [("", ""), ("/b", "/b"), ("/b/c", "/b/c")] {
        let t = r.ticket(base, "");
        let sealed = t["ticket"].as_str().unwrap();
        let (body, _) = sealed.split_once('.').unwrap();
        let j: serde_json::Value = serde_json::from_str(&String::from_utf8(base64url(body)).unwrap()).unwrap();
        assert_eq!(j["b"], want, "{}", j);
        // Forged: refused; as given: taken.
        for (ticket, ok) in [(format!("{}.x", body), false), (sealed.to_string(), true)] {
            let mut s = TcpStream::connect(("127.0.0.1", r.port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            write!(s, "GET /_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", ticket).unwrap();
            let mut head = [0u8; 12];
            s.read_exact(&mut head).unwrap();
            assert_eq!(head.starts_with(b"HTTP/1.1 101"), ok, "{}", String::from_utf8_lossy(&head));
        }
    }
    // A nested program's own page, with its own session: its channel lets it in.
    let cookie = session_cookie(&r.get_as("/b/c/login", ""));
    let mut ws = Ws::open_at(&r, "/b/c", &cookie, "");
    assert!(ws.join("admin"));
    // One made as 0.8.8 made them, with no mount point: the root's, as it was.
    let old = quillo::session::seal("quillo channel ticket", &serde_json::json!({ "s": "{}", "t": null, "e": 4_000_000_000u64 }).to_string());
    assert!(quillo::wt::ticket_of(&format!("t={}", old)).is_some());
}

fn base64url(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'-' => 62,
        _ => 63,
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes().filter(|c| *c != b'=') {
        acc = (acc << 6) | val(c) as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

// ---- Q-1065: a split service down -------------------------------------------------------

const STAFF_SERVICE: &str = r#"
channel.on("staff", (m, p) => channel.broadcast("staff", m), { join: (peer) => peer.session.user == "boss" })
server fn announce(t: str) {
    return channel.broadcast("staff", { secret: t })
}
"#;

const STAFF_APP: &str = r#"
service chat = "chat.qwe"
route GET /announce ? { t: str } {
    return chat.announce(t)
}
route GET / = "app"
route GET /in-staff = len(channel.peers("staff"))
"#;

/// Under `--split`, while a service is down, a join to its channel (and to any name no program
/// claims) is refused, and the names it said are kept: an anonymous page no longer joins its
/// `staff` channel without its `join` and hears it after it comes back; once it is back, a name no
/// one claims is joined again, and its own `join` still says who (FEEDBACK75.md, Q-1065; branch
/// feedback75)
#[test]
fn a_split_services_channels_are_not_joined_while_it_is_down() {
    let d = Dir::new("split", &[("chat.qwe", STAFF_SERVICE), ("app.qwe", STAFF_APP)]);
    let r = run(&d.file("app.qwe"), &["--split", "--no-wt"], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    let mut anon = Ws::open(&r);
    eventually(20, "the service did not answer", || (!anon.join("staff") && r.get("/announce?t=x") == "0").then_some(()));
    // Past the two seconds the services are asked again after, then down, as in a crash or a
    // deploy: only the service's own process, by its file. It starts again within a second or so,
    // so the pages ask at once.
    std::thread::sleep(Duration::from_millis(2100));
    let chat = d.file("chat.qwe").display().to_string();
    let pids = String::from_utf8(Command::new("pgrep").args(["-f", &chat]).output().unwrap().stdout).unwrap();
    let pids: Vec<&str> = pids.split_whitespace().filter(|p| *p != r.pid().to_string()).collect();
    assert!(!pids.is_empty());
    let mut down = Ws::open(&r);
    for pid in &pids {
        let _ = Command::new("kill").args(["-9", pid]).status();
    }
    // A name no one claims asks the services again, which do not answer: refused, and the
    // service's own channel too.
    assert!(!down.join("no-such-channel"));
    assert!(!down.join("staff"), "joined the service's channel while it was down");
    eventually(10, "the service was not down", || r.log().contains("service chat stopped").then_some(()));
    // Back: still refused by its `join`, and nothing it broadcasts reaches the page.
    eventually(20, "the service did not come back", || (r.get("/announce?t=payroll") == "0").then_some(()));
    assert!(!down.join("staff"));
    // A name no one claims is joined again once the services answer.
    std::thread::sleep(Duration::from_millis(2100));
    eventually(10, "a name no one claims stayed refused", || down.join(&format!("nobodys-{}", fastrand::u32(..))).then_some(()));
}

// ---- Q-1066: RTP on this machine, from one sender ----------------------------------------

fn free_udp_port() -> u16 {
    loop {
        let p = support::free_port();
        if UdpSocket::bind(("0.0.0.0", p)).is_ok() {
            return p;
        }
    }
}

fn rtp(seq: u16, ts: u32, ssrc: u32, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![0x80, 0x80 | 96];
    b.extend_from_slice(&seq.to_be_bytes());
    b.extend_from_slice(&ts.to_be_bytes());
    b.extend_from_slice(&ssrc.to_be_bytes());
    b.extend_from_slice(payload);
    b
}

/// Sends `n` Opus packets (a frame each) from `s` to `port`, from sequence `from`.
fn opus(s: &UdpSocket, to: (&str, u16), ssrc: u32, from: u16, n: u16) {
    for i in from..from + n {
        s.send_to(&rtp(i, i as u32 * 960, ssrc, b"opus"), to).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn frames(r: &Run) -> i64 {
    let v: serde_json::Value = serde_json::from_str(&r.get("/streams")).unwrap();
    v[0]["audio"]["frames"].as_i64().unwrap()
}

/// This machine's address on another interface than loopback, if it has one.
fn outside_address() -> Option<std::net::IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// `media.rtp` listens on this machine alone unless `host` says otherwise, and takes one sender
/// (its address and SSRC) at a time: another's packets are dropped while it sends, and taken once it
/// has been quiet for 2 s; with `host: "0.0.0.0"` packets to another address arrive, as before
/// (FEEDBACK75.md, Q-1066; branch feedback75)
#[test]
fn rtp_is_taken_on_this_machine_and_from_one_sender_at_a_time() {
    let (port, open) = (free_udp_port(), free_udp_port());
    let src = format!("media.rtp(\"cam\", {{ audio: {} }})\nmedia.rtp(\"open\", {{ audio: {}, host: \"0.0.0.0\" }})\nmedia.on(\"cam\", {{ watch: (peer) => true }})\nroute GET /streams = media.streams()\n", port, open);
    let d = Dir::new("rtp", &[("main.qwe", &src)]);
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    let cam = UdpSocket::bind("127.0.0.1:0").unwrap();
    let intruder = UdpSocket::bind("127.0.0.1:0").unwrap();
    opus(&cam, ("127.0.0.1", port), 7, 1, 5);
    eventually(10, "the camera's packets were not taken", || (frames(&r) == 5).then_some(()));
    // Another sender while the camera sends: dropped, the same SSRC from another socket too.
    for ssrc in [7, 8] {
        opus(&intruder, ("127.0.0.1", port), ssrc, 6, 5);
        opus(&cam, ("127.0.0.1", port), 7, 6, 1);
    }
    std::thread::sleep(Duration::from_millis(300));
    let v: serde_json::Value = serde_json::from_str(&r.get("/streams")).unwrap();
    let cam_frames = v.as_array().unwrap().iter().find(|s| s["name"] == "cam").unwrap()["audio"]["frames"].as_i64().unwrap();
    assert_eq!(cam_frames, 6, "{}", v);
    // The camera quiet for 2 s: the next sender is taken, from its own first packet.
    std::thread::sleep(Duration::from_millis(2200));
    opus(&intruder, ("127.0.0.1", port), 9, 500, 3);
    eventually(10, "the new sender was not taken", || {
        let v: serde_json::Value = serde_json::from_str(&r.get("/streams")).unwrap();
        (v.as_array().unwrap().iter().find(|s| s["name"] == "cam").unwrap()["audio"]["frames"] == 9).then_some(())
    });
    // From another interface: not taken by the default, taken where `host` names every one.
    if let Some(ip) = outside_address() {
        let s = UdpSocket::bind((ip, 0)).unwrap();
        for (p, want) in [(port, 9), (open, 2)] {
            opus(&s, (&ip.to_string(), p), 11, 1, 2);
            std::thread::sleep(Duration::from_millis(300));
            let v: serde_json::Value = serde_json::from_str(&r.get("/streams")).unwrap();
            let name = if p == port { "cam" } else { "open" };
            assert_eq!(v.as_array().unwrap().iter().find(|s| s["name"] == name).unwrap()["audio"]["frames"], want, "{}", v);
        }
    }
}

// ---- Q-1073: a message begun and never finished -----------------------------------------

const ECHO: &str = r#"
let heard = []
channel.on("chat", (m, p) => {
    heard = heard + [len(str(m))]
}, { join: (peer) => true })
route GET /heard = heard
"#;

/// A channel socket's message begun and never finished is let go 10 s after it began (its parts
/// trickled in, a ping between them), where it was held for good; one sent in parts that ends in
/// time is taken, as before (FEEDBACK75.md, Q-1073; branch feedback75)
#[test]
fn a_message_begun_on_a_channel_socket_must_end_within_its_time() {
    let d = Dir::new("begun", &[("main.qwe", ECHO)]);
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    // In parts, ended at once: taken.
    let mut ok = Ws::open(&r);
    assert!(ok.join("chat"));
    ok.frame(false, 1, b"[\"m\",\"chat\",\"").unwrap();
    ok.frame(true, 9, b"").unwrap();
    ok.frame(false, 0, "ü".repeat(1000).as_bytes()).unwrap();
    ok.frame(true, 0, b"\"]").unwrap();
    eventually(10, "the message in parts was not taken", || (r.get("/heard") == "[1000]").then_some(()));
    // Begun, its parts trickling in, never ended.
    let mut slow = Ws::open(&r);
    assert!(slow.join("chat"));
    let t = Instant::now();
    slow.frame(false, 1, &[b'x'; 64 * 1024]).unwrap();
    let mut closed = false;
    while t.elapsed() < Duration::from_secs(25) {
        std::thread::sleep(Duration::from_millis(1000));
        if slow.frame(false, 0, b"x").is_err() || slow.frame(true, 9, b"").is_err() {
            closed = true;
            break;
        }
        if slow.closed_within(Duration::from_millis(10)) {
            closed = true;
            break;
        }
    }
    assert!(closed, "the message was held past its time");
    assert!(t.elapsed() > Duration::from_secs(8), "let go too soon: {:?}", t.elapsed());
    // A page quiet between messages for longer than that is not let go.
    std::thread::sleep(Duration::from_secs(11));
    ok.text("[\"m\",\"chat\",1]");
    eventually(10, "a quiet page was let go", || (r.get("/heard") == "[1000,1]").then_some(()));
}

/// Whether this machine has IPv6's loopback, for a second client beside 127.0.0.1.
fn has_v6() -> bool {
    std::net::TcpListener::bind("[::1]:0").is_ok()
}

/// One client holds only its share of the messages begun (an eighth of the hold, at least the
/// largest message): with the default hold, 64 MiB, the eight sockets of 127.0.0.1 each holding a
/// begun message of exactly 1 MiB are its 8 MiB, the ninth and tenth are let go, and another
/// client's message of 40 KB is taken; under `--max-memory 64KB` the share is one largest message
/// (1,048,640 bytes) (found in review of FEEDBACK75.md, Q-1073; branch feedback75)
#[test]
fn a_client_holds_only_its_share_of_messages_begun() {
    if !has_v6() {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    }
    let d = Dir::new("begun-share", &[("main.qwe", ECHO)]);
    let r = run(&d.file("main.qwe"), &["--no-wt", "--host", "::"], &[]);
    let mut pages: Vec<Ws> = (0..10).map(|_| Ws::open(&r)).collect();
    for ws in pages.iter_mut() {
        assert!(ws.join("chat"));
        let _ = ws.frame(false, 1, &vec![b'x'; 1 << 20]);
        std::thread::sleep(Duration::from_millis(300));
    }
    let open: Vec<bool> = pages.iter_mut().map(|ws| !ws.closed_within(Duration::from_millis(300))).collect();
    assert_eq!(open.iter().filter(|o| **o).count(), 8, "{:?}", open);
    let mut other = Ws::open_from(&r, "::1");
    assert!(other.join("chat"));
    other.text(&format!("[\"m\",\"chat\",\"{}\"]", "y".repeat(40_000)));
    eventually(10, "another client's message was cut", || (r.get("/heard") == "[40000]").then_some(()));
    drop(pages);
    drop(r);
    // Under --max-memory 64KB: one largest message a client, its next byte refused.
    let r = run(&d.file("main.qwe"), &["--no-wt", "--max-memory", "64KB"], &[]);
    let (mut one, mut two) = (Ws::open(&r), Ws::open(&r));
    assert!(one.join("chat") && two.join("chat"));
    one.frame(false, 1, &vec![b'x'; 1_048_640]).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let _ = two.frame(false, 1, &vec![b'x'; 5000]);
    assert!(two.closed_within(Duration::from_secs(3)), "a client held past its share");
    assert!(!one.closed_within(Duration::from_millis(100)));
    // Given back when it goes: the next is taken.
    drop((one, two));
    std::thread::sleep(Duration::from_millis(500));
    let mut ws = Ws::open(&r);
    assert!(ws.join("chat"));
    ws.text(&format!("[\"m\",\"chat\",\"{}\"]", "z".repeat(5000)));
    eventually(10, "the message was not taken", || (r.get("/heard") == "[5000]").then_some(()));
}

/// Messages begun with no data hold nothing, as room is made only as bytes come: under
/// `--max-memory 16MB`, 256 sockets of 127.0.0.1 each having sent only the 14-byte head of a frame
/// of 1 MiB, a page of ::1 still has its 5 KB message taken, and a WebTransport session of ::1 its
/// join answered, where a 64 KiB piece counted before any data took the hold and cut both (found in
/// review of FEEDBACK75.md, Q-1073 and Q-1074; branch feedback75)
#[test]
fn messages_begun_with_no_data_hold_nothing() {
    if !has_v6() {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    }
    let d = Dir::new("begun-heads", &[("main.qwe", ECHO)]);
    let r = run(&d.file("main.qwe"), &["--max-memory", "16MB", "--host", "::"], &[]);
    let mut heads: Vec<Ws> = Vec::new();
    for _ in 0..256 {
        let mut ws = Ws::open(&r);
        ws.head_only(true, 1, 1 << 20).unwrap();
        heads.push(ws);
    }
    std::thread::sleep(Duration::from_millis(500));
    let mut other = Ws::open_from(&r, "::1");
    assert!(other.join("chat"));
    other.text(&format!("[\"m\",\"chat\",\"{}\"]", "y".repeat(5000)));
    eventually(10, "the other client's message was cut", || (r.get("/heard") == "[5000]").then_some(()));
    #[cfg(feature = "realtime")]
    webtransport::join_from(&r, "[::1]");
    drop(heads);
}

/// A message that keeps coming keeps its time: 1 MiB sent in one frame at about 80 KB/s, 13 s in
/// all, is taken, where it was cut at 10 s; one that stops is still let go of (see
/// `a_message_begun_on_a_channel_socket_must_end_within_its_time`) (found in review of
/// FEEDBACK75.md, Q-1073; branch feedback75)
#[test]
fn a_slow_honest_message_is_taken() {
    let d = Dir::new("begun-slow", &[("main.qwe", ECHO)]);
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    let mut ws = Ws::open(&r);
    assert!(ws.join("chat"));
    let text = format!("[\"m\",\"chat\",\"{}\"]", "s".repeat((1 << 20) - 20));
    let len = text.len();
    let mut f = vec![0x81u8, 0x80 | 127];
    f.extend_from_slice(&(len as u64).to_be_bytes());
    f.extend_from_slice(&[1, 2, 3, 4]);
    f.extend(text.bytes().enumerate().map(|(i, b)| b ^ [1u8, 2, 3, 4][i % 4]));
    let t = Instant::now();
    for chunk in f.chunks(8192) {
        ws.0.write_all(chunk).unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(t.elapsed() > Duration::from_secs(12), "{:?}", t.elapsed());
    eventually(10, "the slow message was cut", || (r.get("/heard") == format!("[{}]", (1 << 20) - 20)).then_some(()));
}

// ---- Q-1052: a page's message read within what its JSON may make -----------------------

/// A page message of about 1 MiB whose JSON is `[[0,0,…]]` (520,000 zeros), or as 13 lists of
/// 40,000.
fn heavy(nested: bool) -> String {
    let inner = if nested { (0..13).map(|_| format!("[{}]", vec!["0"; 40_000].join(","))).collect::<Vec<_>>().join(",") } else { format!("[{}]", vec!["0"; 520_000].join(",")) };
    format!("[\"m\",\"chat\",[{}]]", inner)
}

/// The heap a server may take, at most: one channel message of these made 75 to 94 MB, which a
/// server held to this cannot have made and lived.
const SMALL_HEAP: [(&str, &str); 3] = [("MIMALLOC_RESERVE_OS_MEMORY", "64MiB"), ("MIMALLOC_ARENA_RESERVE", "64MiB"), ("MIMALLOC_DISALLOW_OS_ALLOC", "1")];

/// Under `--max-memory 8MB`, a page message of about 1 MiB of nested lists (`[[0,…]]`, and
/// `[[0×40000]×13]`) is refused, not made, in a server whose whole heap is 64 MiB, where one such
/// message made 75 to 94 MB; the page and the server go on, and the next message is taken
/// (FEEDBACK75.md, Q-1052; branch feedback75)
#[test]
fn a_page_message_is_read_within_what_its_json_may_make() {
    for nested in [false, true] {
        let d = Dir::new(&format!("page-json-{}", nested), &[("main.qwe", ECHO)]);
        let r = run(&d.file("main.qwe"), &["--no-wt", "--max-memory", "8MB"], &SMALL_HEAP);
        let mut ws = Ws::open(&r);
        assert!(ws.join("chat"));
        let msg = heavy(nested);
        assert!(msg.len() > 1_000_000 && msg.len() < 1 << 20, "{}", msg.len());
        ws.text(&msg);
        ws.text("[\"m\",\"chat\",\"after\"]");
        eventually(20, "the next message was not taken", || (r.get("/heard") == "[5]").then_some(()));
        assert!(!r.log().contains("memory allocation"), "{}", r.log());
    }
}

/// A page's ordinary messages are taken as they were, with a limit and without: text, a list, a
/// map, unicode, an empty list, and messages of 1,024 bytes (read with no count) and 1,025 (with
/// it) (FEEDBACK75.md, Q-1052; branch feedback75)
#[test]
fn a_pages_ordinary_messages_are_taken_with_or_without_a_limit() {
    for extra in [&["--no-wt"][..], &["--no-wt", "--max-memory", "8MB"][..]] {
        let d = Dir::new(&format!("page-json-ok-{}", extra.len()), &[("main.qwe", ECHO)]);
        let r = run(&d.file("main.qwe"), extra, &[]);
        let mut ws = Ws::open(&r);
        assert!(ws.join("chat"));
        for m in ["\"hi\"", "[1,2,3]", "{\"a\":{\"b\":[true,null]}}", "\"zoë 🙂\"", "[]"] {
            ws.text(&format!("[\"m\",\"chat\",{}]", m));
        }
        // 1,024 bytes as sent, read with no count, and one byte more, with it.
        for n in [1024, 1025] {
            let text = format!("[\"m\",\"chat\",\"{}\"]", "x".repeat(n - 15));
            assert_eq!(text.len(), n);
            ws.text(&text);
        }
        let heard: Vec<i64> = eventually(10, "the messages were not all taken", || {
            let h: Vec<i64> = serde_json::from_str(&r.get("/heard")).unwrap();
            (h.len() == 7).then_some(h)
        });
        assert_eq!((heard[0], heard[3], heard[5], heard[6]), (2, 5, 1009, 1010), "{:?}", heard);
    }
}

/// `/_quillo/wt-ticket` is told 404, as for a path the program does not have, only where it is sure
/// the program's pages use no channels: a program of one route is; a program with a `channel.on`,
/// one that only broadcasts, one whose page has `<quillo-media>`, one whose channels are in a file
/// it uses, one whose page calls its own scripts with `js` (which may open them), and a program
/// mounted under one with channels all get a ticket, as in 0.8.8 (FEEDBACK75.md, Q-1052, the
/// coordinator's call: fail open on doubt; branch feedback75)
#[test]
fn a_ticket_is_given_unless_the_program_surely_has_no_channels() {
    let none = "route GET / = \"hi\"\n";
    let broadcast = "route GET / = \"hi\"\nroute GET /b = str(channel.broadcast(\"news\", 1))\n";
    let media = "route GET / = <html><body><quillo-media stream=\"cam\"/></body></html>\n";
    let used = "use lib.qwe\nroute GET / = str(news())\n";
    let lib = "fn news() {\n    return channel.broadcast(\"news\", 1)\n}\n";
    let script = "component Go() {\n    <button @click={() => js(\"window.openRoom\", \"lobby\")}>go</button>\n}\nroute GET / = <html><body><Go/></body></html>\n";
    let d = Dir::new(
        "ticket-door",
        &[
            ("none.qwe", none),
            ("broadcast.qwe", broadcast),
            ("media.qwe", media),
            ("used.qwe", used),
            ("lib.qwe", lib),
            ("script.qwe", script),
            ("echo.qwe", ECHO),
            ("parent.qwe", &format!("mount /quiet = \"none.qwe\"\n{}", ECHO)),
        ],
    );
    let ask = |r: &Run, base: &str| support::send(r.port, "POST", &format!("{}/_quillo/wt-ticket", base), &[("Sec-Fetch-Site", "same-origin")], "");
    let cases = [
        ("none.qwe", "", 404),
        ("broadcast.qwe", "", 200),
        ("media.qwe", "", 200),
        ("used.qwe", "", 200),
        ("script.qwe", "", 200),
        ("echo.qwe", "", 200),
        ("parent.qwe", "", 200),
        ("parent.qwe", "/quiet", 200),
    ];
    for (file, base, want) in cases {
        let r = run(&d.file(file), &["--no-wt"], &[]);
        let a = ask(&r, base);
        assert_eq!(a.status, want, "{} {}: {}", file, base, a.body);
        if want == 200 {
            assert!(a.body.contains("\"ticket\""), "{}", a.body);
        } else {
            assert_eq!(a.body, "not found\n");
        }
    }
}

// ---- Q-1074: WebTransport sessions are held as WebSockets are ---------------------------

#[cfg(feature = "realtime")]
mod webtransport {
    use super::*;
    use wtransport::{ClientConfig, Connection, Endpoint};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
    }

    pub async fn connect(r: &Run) -> Result<Connection, String> {
        connect_at(r, "127.0.0.1").await
    }

    /// A session to the server's WebTransport at `host` (`127.0.0.1`, `[::1]`), asked again only
    /// when wtransport's own driver dropped the attempt before Quillo was asked: wtransport 0.7.2
    /// reads a page's SETTINGS frame inside a `select!` it rebuilds whenever another stream comes,
    /// so a frame split across packets is read again from its middle and the server closes the
    /// connection at the HTTP/3 level (0x105, 0x107) or never answers, on 0.8.8 as on this branch
    /// (FEEDBACK75.md, Q-1124; branch feedback75). At most five tries; any other error, refusal or
    /// session is returned as it is.
    pub async fn connect_at(r: &Run, host: &str) -> Result<Connection, String> {
        let mut last = String::new();
        for _ in 0..5 {
            match tokio::time::timeout(Duration::from_secs(10), connect_once(r, host)).await {
                Ok(Err(e)) if dropped_by_wtransport(&e) => last = e,
                Ok(answer) => return answer,
                Err(_) => last = "no answer within 10 s".to_string(),
            }
        }
        Err(last)
    }

    /// Whether the server closed a connection at the HTTP/3 level (0x100 to 0x10f), as
    /// wtransport's driver does on the race `connect_at` asks again for (Q-1124).
    fn dropped_by_wtransport(e: &str) -> bool {
        e.strip_prefix("connection closed by peer: ").and_then(|c| c.split_whitespace().next()).and_then(|c| c.parse::<u64>().ok()).is_some_and(|c| (0x100..0x110).contains(&c))
    }

    async fn connect_once(r: &Run, host: &str) -> Result<Connection, String> {
        let t = r.ticket("", "");
        let port = t["port"].as_u64().expect("no port");
        let hex = t["hash"].as_str().expect("no hash");
        let bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
        let hash = wtransport::tls::Sha256Digest::new(bytes.try_into().unwrap());
        let c = ClientConfig::builder().with_bind_default().with_server_certificate_hashes([hash]).build();
        let url = format!("https://{}:{}/_quillo/wt?t={}", host, port, t["ticket"].as_str().unwrap());
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

    /// Whether the server let the stream go (a stop) within `within`, for what was written on it.
    async fn stopped(s: &mut wtransport::SendStream, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if tokio::time::timeout(Duration::from_millis(200), s.write_all(b"")).await.is_ok_and(|r| r.is_err()) {
                return true;
            }
            if tokio::time::timeout(Duration::from_millis(50), s.write_all(&[0])).await.is_ok_and(|r| r.is_err()) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    /// A page of `host` joins `chat` over WebTransport, and is answered.
    pub fn join_from(r: &Run, host: &str) {
        rt().block_on(async {
            let conn = connect_at(r, host).await.unwrap();
            let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
            out.write_all(&framed("[\"j\",\"chat\"]")).await.unwrap();
            let answer = tokio::time::timeout(Duration::from_secs(10), read_framed(&mut inn)).await.expect("the join was not answered");
            assert_eq!(answer, "[\"j\",\"chat\",true]");
        });
    }

    /// A WebTransport session takes a place among the pages that wait from the same count as a
    /// WebSocket, where it had a count of its own: with `--live-per-client 2`, a page on a
    /// WebSocket and a session leave no place for a second session (429) or a second socket (503),
    /// and one more is taken once one has gone (FEEDBACK75.md, Q-1074 and its review; branch
    /// feedback75)
    #[test]
    fn webtransport_sessions_count_against_the_per_client_limit() {
        let d = Dir::new("wt-places", &[("main.qwe", ECHO)]);
        let r = run(&d.file("main.qwe"), &["--live-per-client", "2"], &[]);
        rt().block_on(async {
            let page = Ws::open(&r);
            let one = connect(&r).await.unwrap();
            let refused = connect(&r).await.err().expect("a second session was taken");
            assert!(refused.contains("429") || refused.to_lowercase().contains("reject"), "{}", refused);
            let t = r.ticket("", "");
            let mut s = TcpStream::connect(("127.0.0.1", r.port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            write!(s, "GET /_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", t["ticket"].as_str().unwrap()).unwrap();
            let mut head = [0u8; 12];
            s.read_exact(&mut head).unwrap();
            assert_eq!(&head, b"HTTP/1.1 503", "a second socket was taken");
            drop(page);
            one.close(0u32.into(), b"");
            drop(one);
            eventually(10, "the place was not given back", || {
                std::thread::sleep(Duration::from_millis(100));
                Some(())
            });
            let mut taken = false;
            for _ in 0..50 {
                if connect(&r).await.is_ok() {
                    taken = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            assert!(taken, "the place was not given back");
        });
    }

    /// A page's session sends frames on at most 8 streams at once (a stream past them is let go as
    /// it comes), a frame begun past 4 KiB that stops coming on is let go of about 10 s after its
    /// length, and what all frames begun hold is held as a WebSocket's messages are; the page's
    /// ordinary message on its first stream is taken (FEEDBACK75.md, Q-1074; branch feedback75)
    #[test]
    fn a_webtransport_sessions_streams_and_frames_are_held() {
        let d = Dir::new("wt-frames", &[("main.qwe", ECHO)]);
        let r = run(&d.file("main.qwe"), &[], &[]);
        rt().block_on(async {
            let conn = connect(&r).await.unwrap();
            let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
            out.write_all(&framed("[\"j\",\"chat\"]")).await.unwrap();
            assert_eq!(read_framed(&mut inn).await, "[\"j\",\"chat\",true]");
            out.write_all(&framed("[\"m\",\"chat\",\"ü\"]")).await.unwrap();
            eventually(10, "the message was not taken", || (r.get("/heard") == "[1]").then_some(()));
            // Eight streams, each with a frame begun: kept; the ninth is let go.
            let mut kept = Vec::new();
            for _ in 0..8 {
                let mut s = conn.open_uni().await.unwrap().await.unwrap();
                s.write_all(&100_000u32.to_be_bytes()).await.unwrap();
                s.write_all(&[0; 10]).await.unwrap();
                kept.push(s);
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            let mut ninth = conn.open_uni().await.unwrap().await.unwrap();
            ninth.write_all(&10u32.to_be_bytes()).await.unwrap();
            assert!(stopped(&mut ninth, Duration::from_secs(5)).await, "a ninth stream was taken");
            // The first eight's frames never come whole: let go after 10 s.
            let t = Instant::now();
            assert!(!stopped(&mut kept[0], Duration::from_secs(5)).await, "let go too soon");
            assert!(stopped(&mut kept[0], Duration::from_secs(15)).await, "a frame begun was held for good");
            assert!(t.elapsed() > Duration::from_secs(8), "{:?}", t.elapsed());
        });
    }

    /// A session opens no stream both ways past HTTP/3's CONNECT and its first: a second, which
    /// nothing reads and which held its 1 MiB window for good, cannot be opened, and 20 sessions
    /// that try leave the server's memory as it was, where they took 50 MB (found in review of
    /// FEEDBACK75.md, Q-1074; branch feedback75)
    #[test]
    fn a_session_opens_no_stream_both_ways_past_its_first() {
        let d = Dir::new("wt-bidi", &[("main.qwe", ECHO)]);
        let r = run(&d.file("main.qwe"), &[], &[]);
        let rss = || -> u64 { String::from_utf8(Command::new("ps").args(["-o", "rss=", "-p", &r.pid().to_string()]).output().unwrap().stdout).unwrap().trim().parse().unwrap_or(0) };
        rt().block_on(async {
            let before = rss();
            let mut kept = Vec::new();
            for i in 0..20 {
                let conn = connect(&r).await.unwrap();
                let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
                out.write_all(&framed("[\"j\",\"chat\"]")).await.unwrap();
                assert_eq!(read_framed(&mut inn).await, "[\"j\",\"chat\",true]");
                // A second stream both ways: not given.
                let opened = tokio::time::timeout(Duration::from_millis(if i == 0 { 2000 } else { 300 }), async { conn.open_bi().await.ok()?.await.ok() }).await;
                assert!(!matches!(opened, Ok(Some(_))), "a second stream both ways was opened");
                kept.push((conn, out, inn));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            let grew = rss().saturating_sub(before);
            assert!(grew < 30_000, "20 sessions took {} KB", grew);
        });
    }

    /// A page sending 1 MB frames 30 a second over WebTransport keeps 30 a second: 90 frames are
    /// all taken within 3.6 s (in a release build), where a stream's window of 256 KiB held it to
    /// 27 (found in review of FEEDBACK75.md, Q-1074; branch feedback75)
    #[test]
    fn a_page_sending_large_frames_keeps_its_rate() {
        let src = "media.on(\"cam\", { publish: (p) => true, watch: (p) => true })\nroute GET /streams = media.streams()\n";
        let d = Dir::new("wt-rate", &[("main.qwe", src)]);
        let r = run(&d.file("main.qwe"), &[], &[]);
        let frames = || -> i64 {
            let v: serde_json::Value = serde_json::from_str(&r.get("/streams")).unwrap();
            v.as_array().and_then(|a| a.first()).map_or(0, |s| s["video"]["frames"].as_i64().unwrap_or(0))
        };
        rt().block_on(async {
            let conn = connect(&r).await.unwrap();
            let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
            out.write_all(&framed("[\"j\",\"media:cam\"]")).await.unwrap();
            assert_eq!(read_framed(&mut inn).await, "[\"j\",\"media:cam\",true]");
            let mut s = conn.open_uni().await.unwrap().await.unwrap();
            let start = Instant::now();
            for i in 0..90u32 {
                let mut wire = vec![3u8];
                wire.extend_from_slice(b"cam");
                wire.extend_from_slice(&[1, (i % 30 == 0) as u8]);
                wire.extend_from_slice(&(i * 3000).to_be_bytes());
                wire.resize(1_000_000, 7);
                let mut f = (wire.len() as u32).to_be_bytes().to_vec();
                f.extend_from_slice(&wire);
                s.write_all(&f).await.unwrap();
                let next = start + Duration::from_millis(33 * (i as u64 + 1));
                tokio::time::sleep_until(tokio::time::Instant::from_std(next)).await;
            }
            let all_by = if cfg!(debug_assertions) { 60 } else { 4 };
            let took = loop {
                if frames() >= 90 {
                    break start.elapsed();
                }
                assert!(start.elapsed() < Duration::from_secs(all_by), "{} of 90 frames in {:?}", frames(), start.elapsed());
                tokio::time::sleep(Duration::from_millis(20)).await;
            };
            if !cfg!(debug_assertions) {
                assert!(took < Duration::from_millis(3600), "90 frames took {:?}", took);
            }
        });
    }

    /// A program whose channel `slow` waits two minutes on its first message, and whose channel
    /// `fast` notes each message's length.
    const WAITING: &str = r#"
let got = []
channel.on("slow", (m, p) => {
    // Stands in for a database that does not answer.
    sleep(120000)
}, { join: (peer) => true })
channel.on("fast", (m, p) => {
    got = got + [len(str(m))]
}, { join: (peer) => true })
route GET /got = got
"#;

    /// What process `pid` holds, in KB: on macOS its footprint, which counts what the system has
    /// compressed or paged out (a busy Mac pages out what waits, and the resident size fell to
    /// nothing while the server held 100 MB), elsewhere its resident size.
    fn held_kb(pid: u32) -> u64 {
        let pid = pid.to_string();
        if cfg!(target_os = "macos") {
            if let Ok(o) = Command::new("footprint").args(["-p", &pid]).output() {
                let text = String::from_utf8_lossy(&o.stdout);
                let size = text.split("Footprint: ").nth(1).and_then(|t| {
                    let mut w = t.split_whitespace();
                    let n: f64 = w.next()?.parse().ok()?;
                    let unit = match w.next()? {
                        "B" => 1.0 / 1024.0,
                        "KB" => 1.0,
                        "MB" => 1024.0,
                        "GB" => 1024.0 * 1024.0,
                        _ => return None,
                    };
                    Some((n * unit) as u64)
                });
                if let Some(kb) = size {
                    return kb;
                }
            }
        }
        String::from_utf8(Command::new("ps").args(["-o", "rss=", "-p", &pid]).output().unwrap().stdout).unwrap().trim().parse().unwrap_or(0)
    }

    /// A page waiting for room on WebTransport holds its stream's window (1 MiB, what QUIC lets
    /// its page still send, which quinn keeps unread) in its client's share of the messages begun,
    /// as well as its message: one page of ::1 fills `slow`'s inbox (10,050 messages behind a
    /// handler that sleeps), then 60 more each send a message and 1.06 MiB behind it; 7 in all
    /// wait within the client's 8 MiB, the rest have their messages dropped and are read to their
    /// end, so the server holds about 6 MB more (its footprint 22 MB to 28 MB) where the 60 kept
    /// about 60 MB (12 MB to 83 MB from its start); a page of 127.0.0.1 has its messages taken
    /// meanwhile, and once the sessions end ::1's share is given back (a message of 900 KB taken)
    /// (found in review of FEEDBACK75.md, Q-1056; branch feedback75)
    #[test]
    fn a_webtransport_page_waiting_for_room_holds_its_window_in_its_share() {
        if !has_v6() {
            eprintln!("no IPv6 loopback here: skipped");
            return;
        }
        let d = Dir::new("wt-waiting", &[("main.qwe", WAITING)]);
        // On `::` WebTransport listens on [::1]: every session is of ::1.
        let r = run(&d.file("main.qwe"), &["--host", "::"], &[]);
        let size = || held_kb(r.pid());
        let mut other = Ws::open(&r);
        assert!(other.join("fast"));
        rt().block_on(async {
            // Every page joins before the handler is busy: joins wait behind it too.
            let mut pages = Vec::new();
            for _ in 0..61 {
                let conn = connect_at(&r, "[::1]").await.unwrap();
                let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
                out.write_all(&framed("[\"j\",\"slow\"]")).await.unwrap();
                let answer = tokio::time::timeout(Duration::from_secs(10), read_framed(&mut inn)).await.expect("the join was not answered");
                assert_eq!(answer, "[\"j\",\"slow\",true]");
                pages.push((conn, out, inn));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            let before = size();
            let mut fill = Vec::new();
            for i in 0..10_050 {
                fill.extend(framed(&format!("[\"m\",\"slow\",{}]", i)));
            }
            let _ = tokio::time::timeout(Duration::from_secs(10), pages[0].1.write_all(&fill)).await;
            // Past the stream's window: a page whose message waits cannot send it all. Letters
            // that do not repeat, so that macOS does not compress what the server holds out of
            // its resident size.
            let mut seed = 0x2545_f491_4f6c_dd1du64;
            let mut letters = |n: usize| -> String {
                (0..n)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"[(seed % 62) as usize] as char
                    })
                    .collect()
            };
            let mut more = Vec::new();
            while more.len() < (1 << 20) + (64 << 10) {
                more.extend(framed(&format!("[\"m\",\"slow\",\"{}\"]", letters(4000))));
            }
            let writers: Vec<_> = pages
                .drain(1..)
                .map(|(conn, mut out, inn)| {
                    let more = more.clone();
                    tokio::spawn(async move {
                        let sent = tokio::time::timeout(Duration::from_secs(10), async { out.write_all(&framed("[\"m\",\"slow\",1]")).await.is_ok() && out.write_all(&more).await.is_ok() }).await;
                        ((conn, out, inn), sent)
                    })
                })
                .collect();
            let (mut waiting, mut cut) = (0, 0);
            for w in writers {
                let (page, sent) = w.await.unwrap();
                match sent {
                    Err(_) => waiting += 1,
                    Ok(false) => cut += 1,
                    Ok(true) => {}
                }
                pages.push(page);
            }
            // The client's 8 MiB hold about 7 windows and messages, the filling page's among them;
            // the rest are read to their end.
            eprintln!("of 60 pages, {} wait and {} were cut", waiting, cut);
            // What stays while the handler sleeps: the waiting pages' windows. Pages that dropped
            // read theirs, which takes a while on a busy machine; the waiting ones hold theirs for
            // two minutes.
            let start = Instant::now();
            let mut held = size().saturating_sub(before);
            while held >= 30_000 && start.elapsed() < Duration::from_secs(30) {
                tokio::time::sleep(Duration::from_millis(250)).await;
                held = size().saturating_sub(before);
            }
            eprintln!("60 pages waiting with 1 MiB behind: the server holds {} KB more after {:?}", held, start.elapsed());
            assert!((1..=7).contains(&waiting) && cut == 0, "of 60 pages {} wait and {} were cut", waiting, cut);
            assert!(held < 30_000, "60 waiting pages had the server hold {} KB for 30 s", held);
            // Another client's messages are taken meanwhile.
            for n in [10, 20, 30] {
                other.text(&format!("[\"m\",\"fast\",\"{}\"]", "y".repeat(n)));
            }
            eventually(10, "another client's messages were not taken", || (r.get("/got") == "[10,20,30]").then_some(()));
            // The sessions end: what they held is given back.
            for (conn, _, _) in &pages {
                conn.close(0u32.into(), b"");
            }
            drop(pages);
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let mut again = Ws::open_from(&r, "::1");
        assert!(again.join("fast"));
        again.text(&format!("[\"m\",\"fast\",\"{}\"]", "z".repeat(900_000)));
        eventually(10, "::1's share was not given back", || (r.get("/got") == "[10,20,30,900000]").then_some(()));
    }
}

// ---- Q-1077: a mounted program's session cookie ---------------------------------------

const SHOP: &str = r#"
route writes GET /in ? { who: str } {
    set_session({ user: who })
    "ok"
}
route GET /who = session()
route writes GET /out {
    end_session()
    "out"
}
"#;

/// A mounted program's session cookie is `__Host-` with the mount in its name and `Path=/`
/// (`/shop`: `__Host-shop.quillo_session`; nested and dotted paths each a name of their own), so a
/// sibling subdomain cannot plant one; the old `quillo_session` planted for the path is no session;
/// signing out deletes the cookie by that name; over `--plain-http` it stays `quillo_session` with
/// the mount's path (FEEDBACK75.md, Q-1077; branch feedback75)
#[test]
fn a_mounted_programs_session_cookie_is_host_only() {
    let root = "mount /shop = \"shop.qwe\"\nmount /a/b = \"shop.qwe\"\nmount /a.b = \"shop.qwe\"\nroute GET / = \"root\"\n";
    let d = Dir::new("cookie-name", &[("main.qwe", root), ("shop.qwe", SHOP)]);
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    let line = |path: &str| {
        let a = r.get_as(path, "");
        a.headers.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session")).map(|(_, v)| v.clone()).unwrap_or_else(|| panic!("{:?}", a.headers))
    };
    let shop = line("/shop/in?who=ana");
    assert!(shop.starts_with("__Host-shop.quillo_session=") && shop.contains("; Path=/;") && shop.contains("; Secure") && !shop.contains("Domain"), "{}", shop);
    assert!(line("/a/b/in?who=ana").starts_with("__Host-a.b.quillo_session="));
    assert!(line("/a.b/in?who=ana").starts_with("__Host-a%2Eb.quillo_session="));
    let pair = shop.split(';').next().unwrap().to_string();
    assert_eq!(r.get_as("/shop/who", &pair).body, "{\"user\":\"ana\"}");
    // The old name, as a sibling subdomain would plant it for the path: no session.
    let value = pair.split_once('=').unwrap().1;
    assert_eq!(r.get_as("/shop/who", &format!("quillo_session={}", value)).body, "{}");
    // Another mount's cookie is no session here.
    let ab = line("/a/b/in?who=bob").split(';').next().unwrap().to_string();
    assert_eq!(r.get_as("/shop/who", &ab).body, "{}");
    // Signing out deletes it by its name.
    let out = r.get_as("/shop/out", &pair);
    assert!(out.headers.iter().any(|(k, v)| k == "set-cookie" && v.starts_with("__Host-shop.quillo_session=;") && v.contains("Max-Age=0")), "{:?}", out.headers);
    drop(r);
    // Over plain HTTP, on purpose: `quillo_session`, its path the mount's, as before.
    let plain = run(&d.file("main.qwe"), &["--no-wt", "--plain-http"], &[]);
    let a = plain.get_as("/shop/in?who=ana", "");
    let c = a.headers.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session")).map(|(_, v)| v.clone()).unwrap();
    assert!(c.starts_with("quillo_session=") && c.contains("Path=/shop;") && !c.contains("Secure"), "{}", c);
}

/// Under `--base` the site's other paths may be other programs' behind the same proxy, so every
/// program of the process keeps 0.8.8's cookie: the root's `quillo_session` with `Path=/shop`, and
/// a program it mounts its own with `Path=/shop/m`, each a session (a name past ASCII in it) and no
/// session in the other (FEEDBACK75.md, Q-1077, the coordinator's decision: mounts only; branch
/// feedback75)
#[test]
fn under_base_every_programs_session_cookie_is_as_before() {
    let root = format!("mount /m = \"shop.qwe\"\n{}", SHOP);
    let d = Dir::new("cookie-base", &[("main.qwe", &root), ("shop.qwe", SHOP)]);
    let r = run(&d.file("main.qwe"), &["--no-wt", "--base", "/shop"], &[]);
    // The proxy in front takes the base off the path, as for every path under --base.
    let line = |path: &str| {
        let a = r.get_as(path, "");
        a.headers.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session")).map(|(_, v)| v.clone()).unwrap_or_else(|| panic!("{} {:?}", a.status, a.headers))
    };
    let root_line = line("/in?who=zo%C3%AB");
    assert!(root_line.starts_with("quillo_session=") && root_line.contains("; Path=/shop;"), "{}", root_line);
    let mount_line = line("/m/in?who=ana");
    assert!(mount_line.starts_with("quillo_session=") && mount_line.contains("; Path=/shop/m;"), "{}", mount_line);
    let (root_pair, mount_pair) = (root_line.split(';').next().unwrap(), mount_line.split(';').next().unwrap());
    assert_eq!(r.get_as("/who", root_pair).body, "{\"user\":\"zoë\"}");
    assert_eq!(r.get_as("/m/who", mount_pair).body, "{\"user\":\"ana\"}");
    assert_eq!(r.get_as("/m/who", root_pair).body, "{}");
    assert_eq!(r.get_as("/who", mount_pair).body, "{}");
}

// ---- Q-1094: UDP fairness by packets ----------------------------------------------------

const SENSOR: &str = r#"
let got = 0
udp.listen(PORT, (p) => {
    // A database write per reading.
    sleep(2)
    if p.text != null && p.text != "" {
        got = got + 1
    }
}, { host: "::", queue: 1000 })
route GET /got = got
"#;

/// One sender of empty packets fills a port's queue by packets and is held back as one filling
/// its bytes is: a sensor sending at the same time on another address is heard for most of its
/// readings, where it was heard for 2 of 135 (FEEDBACK75.md, Q-1094; branch feedback75)
#[test]
fn a_sender_of_empty_packets_does_not_crowd_out_the_others() {
    let Ok(sensor) = UdpSocket::bind("[::1]:0") else {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    };
    let port = free_udp_port();
    let src = SENSOR.replace("PORT", &port.to_string());
    let d = Dir::new("udp-fair", &[("main.qwe", &src)]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    let attacker = UdpSocket::bind("127.0.0.1:0").unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flood = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut n = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let _ = attacker.send_to(b"", ("127.0.0.1", port));
                n += 1;
            }
            n
        })
    };
    let mut sent = 0;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if sensor.send_to(b"reading", ("::1", port)).is_ok() {
            sent += 1;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    stop.store(true, Ordering::Relaxed);
    let flooded = flood.join().unwrap();
    // What is queued is handled at the handler's pace (2 ms a packet, the flood's empty ones
    // included, a batch of up to 1,000 at a time, its count seen when the batch is done): read once
    // every reading is heard, or nothing more comes for 8 s.
    let (mut got, mut since) = (0u64, Instant::now());
    let deadline = Instant::now() + Duration::from_secs(60);
    while got < sent && since.elapsed() < Duration::from_secs(8) && Instant::now() < deadline {
        let now: u64 = r.get("/got").parse().unwrap();
        if now != got {
            got = now;
            since = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(flooded > 10 * sent, "the flood was too slow to fill the queue: {} packets", flooded);
    assert!(got * 10 >= sent * 8, "the sensor was heard for {} of {} readings", got, sent);
}
