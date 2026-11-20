//! Coverage of 0.8.9's realtime lines no test ran (FEEDBACK75.md, slice A): the hold of messages
//! begun (Q-1073, Q-1074) as a reader that blocks and a task read it, its total and each client's
//! share, frames that end a message begun, inboxes held by bytes (Q-1056), the counters of many
//! clients, `/_quillo/wt-ticket` for programs that may use channels (Q-1052) and a mounted page's
//! session kept from a split service's channel (Q-1061). Coverage of 0.8.9; branch feedback75.

mod support;

use quillo::ws::{Hold, Message, Next, Reader};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---- the hold, read by the library ----------------------------------------------------

/// A hold of its own (statics are shared by the tests of this file, which run at once): `most`
/// in all, a client's share at least `least`.
fn hold(most: fn() -> usize, least: usize) -> &'static Hold {
    let held: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));
    let clients: &'static Mutex<Option<HashMap<Arc<str>, usize>>> = Box::leak(Box::new(Mutex::new(None)));
    Box::leak(Box::new(Hold { within: Duration::from_secs(600), rate: 16 << 10, held, most, clients, least_share: least }))
}

fn held(h: &Hold) -> usize {
    h.held.load(Ordering::SeqCst)
}

/// A page's frame: `fin`, its opcode, its data, masked as a page's are.
fn frame(fin: bool, opcode: u8, data: &[u8]) -> Vec<u8> {
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
    let mask = [0x37, 0xfa, 0x21, 0x3d];
    f.extend_from_slice(&mask);
    f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// A reader that blocks on its connection, given a hold: a message of one frame past `SMALL`
/// (3,000 `é`s, 6,000 bytes) is read through the hold and given back whole; one in parts with a
/// ping between them answers the ping with its first part still held (5,000 bytes), then, asked
/// again, goes on with the rest; a frame whose data stops short ends in UnexpectedEof, its bytes
/// held until the reader goes (src/ws.rs `Reading::fill_by`/`some_by`, `Reader::next`'s held
/// path, FEEDBACK75.md Q-1073; coverage of 0.8.9; branch feedback75)
#[test]
fn a_blocking_reader_with_a_hold_reads_messages_begun() {
    let h = hold(|| 1 << 20, 64 << 10);
    let big = "é".repeat(3000);
    let mut wire = frame(true, 0x1, big.as_bytes());
    wire.extend(frame(false, 0x1, &[b'a'; 5000]));
    wire.extend(frame(true, 0x9, b"p"));
    wire.extend(frame(true, 0x0, "bç".as_bytes()));
    wire.extend(frame(true, 0x1, b"small"));
    let mut r = Reader::holding(&wire[..], 1 << 20, h, "198.51.100.1");
    assert_eq!(r.next().unwrap(), Message::Text(big));
    assert_eq!(held(h), 0, "a whole message is given back");
    assert_eq!(r.next().unwrap(), Message::Ping(b"p".to_vec()));
    assert_eq!(held(h), 5000, "the first part is held while the ping is answered");
    assert_eq!(r.next().unwrap(), Message::Text(format!("{}bç", "a".repeat(5000))));
    assert_eq!(held(h), 0);
    assert_eq!(r.next().unwrap(), Message::Text("small".into()));
    assert_eq!(r.next().unwrap_err().kind(), ErrorKind::UnexpectedEof);
    // Its data cut 100 bytes short: what came is held until the reader goes.
    let mut cut = frame(true, 0x2, &[1u8; 10_000]);
    cut.truncate(cut.len() - 100);
    let mut r = Reader::holding(&cut[..], 1 << 20, h, "198.51.100.1");
    assert_eq!(r.next().unwrap_err().kind(), ErrorKind::UnexpectedEof);
    assert!(held(h) >= 9_900, "{}", held(h));
    drop(r);
    assert_eq!(held(h), 0, "a reader gone gives back what it held");
}

/// A task's reader with a hold (`next_async`): a message of one frame of 4,096 bytes (`SMALL`) is
/// read with no count, though the client's share is 4,096; one of 4,097 is counted, past that
/// share, and refused with nothing left held; a message in a few bytes is read as always (src/ws.rs
/// `Reader::next_async`, FEEDBACK75.md Q-1073; coverage of 0.8.9; branch feedback75)
#[test]
fn a_tasks_reader_reads_through_next_async() {
    let h = hold(|| 4096 * 8, 4096);
    assert_eq!(h.share(), 4096);
    let mut wire = frame(true, 0x1, "zoë".as_bytes());
    wire.extend(frame(true, 0x2, &[8u8; 4096]));
    wire.extend(frame(true, 0x2, &[9u8; 4097]));
    rt().block_on(async {
        let mut r = Reader::holding(&wire[..], 1 << 20, h, "198.51.100.2");
        assert_eq!(r.next_async().await.unwrap(), Message::Text("zoë".into()));
        assert_eq!(r.next_async().await.unwrap(), Message::Binary(vec![8u8; 4096]));
        let e = r.next_async().await.unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfMemory, "{}", e);
        assert!(e.to_string().contains("hold all they may"), "{}", e);
        assert_eq!(held(h), 0);
    });
    // Within the share, the same 4,097 bytes are taken.
    let h = hold(|| 1 << 20, 64 << 10);
    let wire = frame(true, 0x2, &[9u8; 4097]);
    rt().block_on(async {
        let mut r = Reader::holding(&wire[..], 1 << 20, h, "198.51.100.2");
        assert_eq!(r.next_async().await.unwrap(), Message::Binary(vec![9u8; 4097]));
        assert_eq!(held(h), 0);
    });
}

/// A hold refuses a client past what all may hold even where its own share has room, counting
/// nothing for the refusal; a client given back part of what it holds keeps the rest counted in
/// its share (1,500 of 2,000: 501 more refused, 500 taken), where all of it is given back only when
/// all is (src/ws.rs `Hold::take`'s total, `Hold::give` of part, FEEDBACK75.md Q-1073; coverage of
/// 0.8.9; branch feedback75)
#[test]
fn a_hold_refuses_past_its_total_and_keeps_a_clients_rest() {
    let h = hold(|| 3000, 2000);
    assert_eq!(h.share(), 2000);
    assert!(h.take(2000, "a"));
    assert!(!h.take(1, "a"), "past a's share");
    assert!(h.take(1000, "b"));
    assert_eq!(held(h), 3000);
    // c holds nothing, but all is held: refused, and nothing counted for it.
    assert!(!h.take(1, "c"));
    assert_eq!(held(h), 3000);
    // a gives back 500 and keeps 1,500.
    h.give(500, "a");
    assert_eq!(held(h), 2500);
    assert!(h.take(500, "c"), "the room given back is another's");
    assert!(!h.take(1, "a"), "within a's share, past the total");
    h.give(1000, "b");
    h.give(500, "c");
    assert_eq!(held(h), 1500);
    assert!(!h.take(501, "a"), "a's 1,500 still count in its share");
    assert!(h.take(500, "a"));
    // Given back all, by a client and then another that holds nothing: nothing is left.
    h.give(2000, "a");
    h.give(0, "a");
    assert_eq!(held(h), 0);
    assert!(h.take(2000, "a") && h.take(1000, "ü-client"));
    h.give(2000, "a");
    h.give(1000, "ü-client");
    assert_eq!(held(h), 0);
}

/// A hold gives back no more than a client holds: given back for a client that holds nothing, or
/// more than it holds, the total keeps what the others hold (1,000 kept after 400 "given back" by
/// a client of none, where it fell to 600); a fresh hold given back 5 bytes stays at 0, where it
/// wrapped to 18,446,744,073,709,551,611 and then took 1,000 bytes past its total; giving back
/// twice gives back once (src/ws.rs `Hold::give`, FEEDBACK75.md follow-up found by 0.8.9's
/// coverage, Q-1173; coverage of 0.8.9; branch feedback75)
#[test]
fn a_hold_gives_back_only_what_a_client_holds() {
    let h = hold(|| 3000, 2000);
    assert!(h.take(1000, "a"));
    h.give(400, "nobody");
    assert_eq!(held(h), 1000, "given back for a client that holds nothing");
    assert!(h.take(2000, "b"));
    assert!(!h.take(1, "c"), "all is held");
    // a gives back more than it holds: its 1,000 alone.
    h.give(1500, "a");
    assert_eq!(held(h), 2000);
    assert!(!h.take(1001, "a") && h.take(1000, "a"), "b's 2,000 still count in the total");
    h.give(1000, "a");
    h.give(1000, "a");
    assert_eq!(held(h), 2000, "given back twice");
    h.give(2000, "b");
    assert_eq!(held(h), 0);
    // A fresh hold: nothing to give back, no wrap.
    let h = hold(|| 3000, 2000);
    h.give(5, "x");
    assert_eq!(held(h), 0);
    assert!(!h.take(2001, "y"), "past y's share");
    assert!(h.take(2000, "y") && h.take(1000, "z"));
    assert!(!h.take(1, "w"), "a take past the total passed");
    assert_eq!(held(h), 3000);
    h.give(usize::MAX, "y");
    h.give(usize::MAX, "z");
    assert_eq!(held(h), 0);
}

/// After a message the reader refused (a continuation of nothing, 5,000 bytes, counted as it
/// came), a control frame ends what that message held: the bytes are given back at the frame, a
/// close or a pong, not only when the reader goes (src/ws.rs `took`: `st.done()` for a control
/// frame between messages, its Close and Pong, FEEDBACK75.md Q-1073; coverage of 0.8.9; branch
/// feedback75)
#[test]
fn a_control_frame_after_a_refused_message_gives_back_what_it_held() {
    let h = hold(|| 1 << 20, 64 << 10);
    let mut wire = frame(true, 0x0, &[b'x'; 5000]);
    wire.extend(frame(true, 0xA, b""));
    wire.extend(frame(true, 0x0, &[b'y'; 6000]));
    wire.extend(frame(true, 0x8, b""));
    let mut r = Reader::holding(&wire[..], 1 << 20, h, "198.51.100.3");
    let e = r.next().unwrap_err();
    assert!(e.to_string().contains("a continuation of nothing"), "{}", e);
    assert_eq!(held(h), 5000);
    assert_eq!(r.next().unwrap(), Message::Pong);
    assert_eq!(held(h), 0, "the pong gave back what the refused message held");
    assert!(r.next().unwrap_err().to_string().contains("a continuation of nothing"));
    assert_eq!(held(h), 6000);
    assert_eq!(r.next().unwrap(), Message::Close);
    assert_eq!(held(h), 0, "the close gave back what the refused message held");
    // A task's reader the same.
    rt().block_on(async {
        let mut r = Reader::holding(&wire[..], 1 << 20, h, "198.51.100.3");
        assert!(r.next_async().await.is_err());
        assert_eq!(r.next_async().await.unwrap(), Message::Pong);
        assert_eq!(held(h), 0);
    });
}

/// A frame a hold counts, begun on a reader with a hold and read on by one without (its `Started`
/// handed over): read whole, counted in no hold (src/ws.rs `State::charge` with no hold,
/// FEEDBACK75.md Q-1073; coverage of 0.8.9; branch feedback75)
#[test]
fn a_reader_without_a_hold_reads_a_counted_frame_uncounted() {
    let h = hold(|| 1 << 20, 64 << 10);
    let text = "ž".repeat(3000);
    let wire = frame(true, 0x1, text.as_bytes());
    // The head of a frame of 6,000 bytes: two bytes and its 16-bit length.
    let (head, rest) = wire.split_at(4);
    rt().block_on(async {
        let mut a = Reader::holding(head, 1 << 20, h, "198.51.100.4");
        let Next::Held(started) = a.next_or_held().await.unwrap() else { panic!("a frame of 6,000 bytes is counted") };
        let mut b = Reader::new(rest, 1 << 20);
        assert_eq!(b.held_async(started).await.unwrap(), Message::Text(text));
    });
    assert_eq!(held(h), 0);
}

// ---- servers --------------------------------------------------------------------------

const SECRET: &str = "a-test-secret-for-services-long-enough";

/// How much longer a debug build (CI's, unoptimised) may take: every deadline here is so many
/// seconds times this, never a time a test means (as tests/cover089_server.rs has it).
const SLOWER: u64 = if cfg!(debug_assertions) { 10 } else { 1 };

/// A deadline of `n` seconds, in a release build.
fn secs(n: u64) -> Duration {
    Duration::from_secs(n * SLOWER)
}

/// A folder of its own, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, files: &[(&str, &str)]) -> Dir {
        let d = support::temp_dir(&format!("c089rt-{}", name));
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
    let deadline = Instant::now() + secs(120);
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
    let deadline = Instant::now() + Duration::from_secs(secs * SLOWER);
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

    /// A line of `--metrics`, its number.
    fn metric(&self, name: &str) -> i64 {
        let m = self.get("/_quillo/metrics");
        m.lines().find_map(|l| l.strip_prefix(name).and_then(|v| v.trim().parse().ok())).unwrap_or_else(|| panic!("no {} in:\n{}", name, m))
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
    r.port = support::port_in_log(&mut r.child, &r.log, secs(60));
    let deadline = Instant::now() + secs(30);
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
    Pong(Vec<u8>),
}

impl Ws {
    /// A page connected with `ticket` at `path`'s `/_quillo/wt`, through a proxy that says it is
    /// `xff` when one is given (a server run with `--trust-proxy 127.0.0.1` takes it).
    fn connect(r: &Run, ticket: &str, path: &str, xff: Option<&str>) -> Ws {
        let mut s = TcpStream::connect(("127.0.0.1", r.port)).unwrap();
        s.set_read_timeout(Some(secs(60))).unwrap();
        let fwd = xff.map(|a| format!("X-Forwarded-For: {}\r\n", a)).unwrap_or_default();
        write!(
            s,
            "GET {}/_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\n{}Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            path, ticket, fwd
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

    /// A page of the program at `base` connected with `cookie`'s session, at `path`'s `/_quillo/wt`.
    fn open_at(r: &Run, base: &str, cookie: &str, path: &str) -> Ws {
        let t = r.ticket(base, cookie);
        Ws::connect(r, t["ticket"].as_str().unwrap(), path, None)
    }

    fn open(r: &Run) -> Ws {
        Ws::open_at(r, "", "", "")
    }

    /// A frame: `fin`, its opcode, its data (masked, as a page's are).
    fn frame(&mut self, fin: bool, opcode: u8, data: &[u8]) -> std::io::Result<()> {
        self.0.write_all(&frame(fin, opcode, data))
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
            10 => Frame::Pong(data),
            o => panic!("opcode {}", o),
        })
    }

    /// The next text frame, pings passed over.
    fn text_frame(&mut self) -> String {
        loop {
            match self.read() {
                Frame::Text(t) => return t,
                Frame::Ping | Frame::Pong(_) | Frame::Binary(_) => {}
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

    /// A ping, and its pong: the server has read all that was sent before it. False when the
    /// connection ended instead.
    fn read_up_to_here(&mut self) -> bool {
        if self.frame(true, 9, b"here").is_err() {
            return false;
        }
        loop {
            match self.try_read() {
                Ok(Frame::Pong(d)) if d == b"here" => return true,
                Ok(Frame::Close) | Err(_) => return false,
                Ok(_) => {}
            }
        }
    }

    /// Whether the server closed the connection (a close frame or the end), within `within`.
    fn closed_within(&mut self, within: Duration) -> bool {
        let _ = self.0.set_read_timeout(Some(within));
        let start = Instant::now();
        loop {
            match self.try_read() {
                Ok(Frame::Close) => return true,
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => return false,
                Err(_) => return true,
            }
            if start.elapsed() > within {
                return false;
            }
        }
    }
}

const ECHO: &str = r#"
let heard = []
channel.on("chat", (m, p) => {
    heard = heard + [len(str(m))]
}, { join: (peer) => true })
route GET /heard = heard
"#;

// ---- frames that end a message begun --------------------------------------------------

/// Inside a message begun (its first part, a frame the hold counts, read), a frame a page may not
/// send ends the page and the message is never handled: one with a reserved bit, one unmasked, a
/// ping of 126 bytes, a ping in parts, a new message before the last ended, opcodes 3 and 0xB, and
/// a close; a pong and a ping of exactly 125 bytes between the parts leave the message to end, and
/// it is handled (src/ws.rs `head_ok`, `len_ok`, `took`'s Close, Pong, new message and unknown
/// opcode, FEEDBACK75.md Q-1073; coverage of 0.8.9; branch feedback75)
#[test]
fn a_frame_a_page_may_not_send_inside_a_message_begun_ends_the_page() {
    let d = Dir::new("mid", &[("main.qwe", ECHO)]);
    let r = run(&d.file("main.qwe"), &["--no-wt"], &[]);
    let begun = |ws: &mut Ws| {
        assert!(ws.join("chat"));
        ws.frame(false, 1, format!("[\"m\",\"chat\",\"{}", "ü".repeat(300)).as_bytes()).unwrap();
    };
    let mask = |data: &[u8]| -> Vec<u8> { data.iter().enumerate().map(|(i, b)| b ^ [1u8, 2, 3, 4][i % 4]).collect() };
    let mut rsv = vec![0x80 | 0x40, 0x80 | 2, 1, 2, 3, 4];
    rsv.extend(mask(b"\"]"));
    let bad: Vec<(&str, Vec<u8>)> = vec![
        ("a reserved bit", rsv),
        ("unmasked", vec![0x80, 2, b'"', b']']),
        ("a ping of 126 bytes", frame(true, 9, &[7u8; 126])),
        ("a ping in parts", frame(false, 9, b"p")),
        ("a new message", frame(true, 1, b"[\"m\",\"chat\",1]")),
        ("opcode 3", frame(true, 3, b"\"]")),
        ("opcode 0xB", frame(true, 0xB, b"")),
        ("a close", frame(true, 8, b"")),
    ];
    for (what, f) in &bad {
        let mut ws = Ws::open(&r);
        begun(&mut ws);
        ws.0.write_all(f).unwrap();
        assert!(ws.closed_within(secs(30)), "{}: the page was not let go", what);
    }
    // A pong between the parts, then a ping of exactly 125 bytes: answered, and the message ends.
    let mut ws = Ws::open(&r);
    begun(&mut ws);
    ws.frame(true, 0xA, b"").unwrap();
    ws.frame(true, 9, &[5u8; 125]).unwrap();
    loop {
        match ws.read() {
            Frame::Pong(d) => {
                assert_eq!(d, vec![5u8; 125]);
                break;
            }
            Frame::Ping => {}
            f => panic!("{:?}", f),
        }
    }
    ws.frame(true, 0, "é\"]".as_bytes()).unwrap();
    eventually(30, "the message with a pong and a ping between its parts was not handled", || (r.get("/heard") == "[301]").then_some(()));
    // None of the others was: the next page's message comes after the one above alone.
    let mut next = Ws::open(&r);
    assert!(next.join("chat"));
    next.text("[\"m\",\"chat\",\"ok\"]");
    eventually(30, "the next message was not handled", || (r.get("/heard") == "[301,2]").then_some(()));
}

// ---- the hold of messages begun, in all and by client --------------------------------------

/// Under `--max-memory 64KB` the messages begun hold 2,097,280 bytes in all, a client's share
/// 1,048,640: three clients (by a trusted proxy's `X-Forwarded-For`) holding 1,048,640, 1,043,640
/// and 5,000 bytes hold all of it, and a fourth client's first byte is refused (its page let go)
/// though its own share is empty; once a page goes, its bytes are another's (src/ws.rs
/// `Hold::take`'s total, FEEDBACK75.md Q-1073; coverage of 0.8.9; branch feedback75)
#[test]
fn messages_begun_are_held_to_what_all_may_hold() {
    let d = Dir::new("begun-total", &[("main.qwe", ECHO)]);
    let r = run(&d.file("main.qwe"), &["--no-wt", "--max-memory", "64KB", "--trust-proxy", "127.0.0.1"], &[]);
    let t = r.ticket("", "")["ticket"].as_str().unwrap().to_string();
    let begin = |who: &str, n: usize| -> Ws {
        let mut ws = Ws::connect(&r, &t, "", Some(who));
        ws.frame(false, 1, &vec![b'x'; n]).unwrap();
        ws
    };
    let mut a = begin("10.0.0.1", 1_048_640);
    assert!(a.read_up_to_here(), "a client's whole share was refused");
    let mut b = begin("10.0.0.2", 1_043_640);
    assert!(b.read_up_to_here());
    let mut c = begin("10.0.0.3", 5_000);
    assert!(c.read_up_to_here(), "the last 5,000 bytes of the hold were refused");
    // All is held: a fourth client's first byte is refused.
    let mut d4 = begin("10.0.0.4", 1);
    assert!(!d4.read_up_to_here(), "a byte past all the hold was taken");
    assert!(d4.closed_within(secs(30)));
    assert!(b.read_up_to_here() && c.read_up_to_here(), "the pages within the hold were let go");
    // The first page goes: its bytes are given back, and the fourth client's byte is taken.
    drop(a);
    eventually(30, "the bytes of a page gone were not given back", || {
        let mut e = begin("10.0.0.4", 1);
        e.read_up_to_here().then_some(())
    });
    drop((b, c));
}

// ---- inboxes held by bytes ---------------------------------------------------------------

/// A page message's value as JSON, of `escapes` control characters (each `\u0001`, six bytes) and
/// `plain` letters: what an inbox counts is that JSON, made again.
fn escaped(escapes: usize, plain: usize) -> String {
    format!("[\"m\",\"chat\",\"{}{}\"]", "\\u0001".repeat(escapes), "a".repeat(plain))
}

/// Under `--max-memory 64KB` a page's share of what waits for handlers is 65,536 bytes, counted as
/// the value's JSON made again: a message of 1,000 `\u0001`s (six bytes each) and 50,000 letters
/// is handled; one of 10,000 `\u0001`s and 5,535 letters, 65,537 bytes as JSON though it holds
/// 15,535 characters, can never fit and is dropped and counted; the one at exactly 65,536 is
/// dropped and counted too, its line in the channel's batch being past `--max-memory`; the page
/// goes on with its next message (src/wt.rs `take_from_page`'s `Room::Never`, `charge`'s
/// `Room::Never`, FEEDBACK75.md Q-1056 and Q-1172; coverage of 0.8.9; branch feedback75)
#[test]
fn a_message_larger_than_a_pages_whole_share_is_dropped() {
    let d = Dir::new("never", &[("main.qwe", ECHO)]);
    let r = run(&d.file("main.qwe"), &["--no-wt", "--max-memory", "64KB", "--metrics"], &[]);
    let mut ws = Ws::open(&r);
    assert!(ws.join("chat"));
    assert_eq!(r.metric("quillo_channel_dropped_total"), 0);
    ws.text(&escaped(1_000, 50_000));
    eventually(30, "a message within a page's share was not handled", || (r.get("/heard") == "[51000]").then_some(()));
    ws.text(&escaped(10_000, 5_534));
    ws.text(&escaped(10_000, 5_535));
    ws.text("[\"m\",\"chat\",\"after\"]");
    let heard: Vec<i64> = eventually(30, "the message after was not handled", || {
        let h: Vec<i64> = serde_json::from_str(&r.get("/heard")).unwrap();
        (h.last() == Some(&5)).then_some(h)
    });
    assert!(!heard.contains(&15_535), "a message past a page's share was handled: {:?}", heard);
    // Changed with Q-1172: the message at exactly the share was lost uncounted (its batch refused
    // whole); it is now dropped and counted, as it can never be handed over.
    assert!(!heard.contains(&15_534), "{:?}", heard);
    assert_eq!(r.metric("quillo_channel_dropped_total"), 2, "the messages at and past the share are dropped and counted");
}

const SLOW_IN_ORDER: &str = r#"
let go = false
let got = []
channel.on("chat", (m, p) => {
    // Stands in for a database that does not answer yet.
    while !go {
        sleep(20)
    }
    got = got + [m.i]
}, { join: (peer) => true })
route writes GET /go {
    go = true
    "going"
}
route GET /got = got
"#;

/// A page message of exactly 1,000 bytes as JSON: `{"i":<i>,"p":"…"}`.
fn numbered(i: usize) -> String {
    let head = format!("{{\"i\":{},\"p\":\"", i);
    let v = format!("{}{}\"}}", head, "x".repeat(1000 - head.len() - 2));
    assert_eq!(v.len(), 1000);
    format!("[\"m\",\"chat\",{}]", v)
}

/// What waited behind a slow handler is handed over in batches within what the server reads of a
/// request's body: under `--max-memory 1MB`, 1,040 messages of 1,000 bytes as JSON (each line
/// about 1,060), and under `--max-body 100KB`, 300 of them, are all handled, in the order sent,
/// none dropped, where a batch past the limit was refused whole and 1,024 were lost (16 handled, 27
/// in 0.8.8; 2 of 300 under `--max-body`); without a limit, as before (src/wt.rs `deliver_loop`,
/// src/httpd.rs `serve_on`, FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1172; coverage of
/// 0.8.9; branch feedback75)
#[test]
fn messages_behind_a_slow_handler_are_handed_over_within_the_servers_limits() {
    let d = Dir::new("batches", &[("main.qwe", SLOW_IN_ORDER)]);
    for (extra, n) in [(&["--max-memory", "1MB"][..], 1040), (&["--max-body", "100KB"][..], 300), (&[][..], 1040)] {
        let mut args = vec!["--no-wt", "--metrics"];
        args.extend_from_slice(extra);
        let r = run(&d.file("main.qwe"), &args, &[]);
        let mut ws = Ws::open(&r);
        assert!(ws.join("chat"));
        for i in 0..n {
            ws.text(&numbered(i));
        }
        assert!(ws.read_up_to_here(), "{:?}: the messages were not all taken", extra);
        assert_eq!(r.get("/go"), "going");
        let want: Vec<usize> = (0..n).collect();
        let got: Vec<usize> = eventually(120, &format!("{:?}: the messages were not all handled: {}", extra, r.get("/got").len()), || {
            let g: Vec<usize> = serde_json::from_str(&r.get("/got")).unwrap();
            (g.len() >= n).then_some(g)
        });
        assert_eq!(got, want, "{:?}", extra);
        assert_eq!(r.metric("quillo_channel_dropped_total"), 0, "{:?}", extra);
        assert!(!r.log().contains("POST /_quillo/channel/chat (rid"), "{:?}: {}", extra, r.log());
    }
}

/// A page message `["m","chat","a…"]` of `n` letters.
fn letters(n: usize) -> String {
    format!("[\"m\",\"chat\",\"{}\"]", "a".repeat(n))
}

/// A message whose line in the channel's batch alone is past what the server reads of a request's
/// body can never be handed over, and is dropped and counted, the page going on; one that fits is
/// handled. A line is the value's JSON written again as a JSON string (each backslash and quote
/// twice), the page's id, its empty session and no tenant: 56 bytes more than the value's JSON and
/// its backslashes. Under `--max-memory 64KB`: 9,000 `\u0001`s and 2,478 letters make a line of
/// exactly 65,536 and are handled, one letter more is dropped; 10,000 `\u0001`s and 5,533, 5,534
/// letters (65,535 and 65,536 bytes as JSON, within a page's share) are dropped and counted where
/// they were lost uncounted, 5,535 (past the share) as before, and 10,000 alone (60,002 bytes as
/// JSON) too. Under `--max-body 100KB`: 102,342 letters (a line of 102,400) are handled, 102,343
/// dropped (src/wt.rs `deliver_loop`, FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1172;
/// coverage of 0.8.9; branch feedback75)
#[test]
fn a_message_too_large_to_hand_over_is_dropped_and_counted() {
    let d = Dir::new("too-large", &[("main.qwe", ECHO)]);
    let cases: [(&[&str], (String, usize), Vec<String>); 2] = [
        (
            &["--max-memory", "64KB"],
            (escaped(9_000, 2_478), 11_478),
            vec![escaped(9_000, 2_479), escaped(10_000, 5_533), escaped(10_000, 5_534), escaped(10_000, 5_535), escaped(10_000, 0)],
        ),
        (&["--max-body", "100KB"], (letters(102_342), 102_342), vec![letters(102_343)]),
    ];
    for (extra, (fits, len), tries) in cases {
        let mut args = vec!["--no-wt", "--metrics"];
        args.extend_from_slice(extra);
        let r = run(&d.file("main.qwe"), &args, &[]);
        let mut ws = Ws::open(&r);
        assert!(ws.join("chat"));
        let heard = |want: usize| -> Vec<usize> {
            eventually(60, &format!("{:?}: no message of {} was handled", extra, want), || {
                let h: Vec<usize> = serde_json::from_str(&r.get("/heard")).unwrap();
                (h.last() == Some(&want)).then_some(h)
            })
        };
        ws.text(&fits);
        assert_eq!(heard(len), vec![len], "{:?}", extra);
        for t in &tries {
            ws.text(t);
        }
        ws.text("[\"m\",\"chat\",\"after\"]");
        assert_eq!(heard(5), vec![len, 5], "{:?}", extra);
        assert_eq!(r.metric("quillo_channel_dropped_total"), tries.len() as i64, "{:?}", extra);
        assert!(!r.log().contains("POST /_quillo/channel/chat (rid"), "{:?}: {}", extra, r.log());
    }
}

/// A page message whose JSON the server cannot read within `--max-memory` is dropped and counted,
/// where it was ignored uncounted, as bad JSON still is: under `--max-memory 64KB`, 65,243 letters
/// are the most a page's JSON reader makes (the letters, and the room of the list and its texts),
/// so 65,242 and 65,243 are handled and 65,244 dropped and counted; a message cut off, one nested
/// past what a page may send, and a map in place of a list, each over 1 KB, are ignored and not
/// counted (src/value.rs `json_from_page_why`, src/wt.rs `take_from_page`, FEEDBACK75.md
/// follow-up found by 0.8.9's coverage, Q-1172; coverage of 0.8.9; branch feedback75)
#[test]
fn a_page_message_too_large_to_read_is_dropped_and_counted() {
    let d = Dir::new("unread", &[("main.qwe", ECHO)]);
    let r = run(&d.file("main.qwe"), &["--no-wt", "--metrics", "--max-memory", "64KB"], &[]);
    let mut ws = Ws::open(&r);
    assert!(ws.join("chat"));
    let heard = |want: usize| -> Vec<usize> {
        eventually(60, &format!("no message of {} was handled", want), || {
            let h: Vec<usize> = serde_json::from_str(&r.get("/heard")).unwrap();
            (h.last() == Some(&want)).then_some(h)
        })
    };
    ws.text(&letters(65_242));
    ws.text(&letters(65_243));
    assert_eq!(heard(65_243), vec![65_242, 65_243]);
    ws.text(&letters(65_244));
    ws.text("[\"m\",\"chat\",\"after\"]");
    assert_eq!(heard(5), vec![65_242, 65_243, 5]);
    assert_eq!(r.metric("quillo_channel_dropped_total"), 1, "the message too large to read was not counted");
    // Bad JSON, as before: ignored, not counted.
    for bad in [format!("[\"m\",\"chat\",\"{}", "b".repeat(2000)), "[".repeat(1100), format!("{{\"m\":\"{}\"}}", "c".repeat(2000))] {
        ws.text(&bad);
    }
    ws.text("[\"m\",\"chat\",\"again!\"]");
    assert_eq!(heard(6), vec![65_242, 65_243, 5, 6]);
    assert_eq!(r.metric("quillo_channel_dropped_total"), 1, "bad JSON was counted as a drop");
}

/// A program inside an app (no socket: its channel's batches go to it through
/// `jobs::post_self`) under `--max-memory 64KB` takes a batch as the server's own delivery does,
/// its body never read as JSON: the largest message a page's JSON reader makes (65,243 letters, a
/// line of 65,301 bytes) and one a letter shorter are handled, where reading the batch's one line
/// as JSON made more than the limit (the text and the map's room) and the batch was refused, the
/// message lost uncounted; a message whose line is exactly 65,536 bytes (9,000 `\u0001`s and
/// 2,478 letters) is handled and one a byte longer dropped and counted (src/jobs.rs `post_self`'s
/// Content-Type, FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1172; coverage of 0.8.9;
/// branch feedback75)
#[test]
fn an_apps_program_takes_a_batch_as_the_server_does() {
    use quillo::embed::{request_of, Frame, Program};
    let root = support::temp_dir("c089rt-embed");
    std::fs::create_dir_all(root.join("bundle")).unwrap();
    let src = "channel.on(\"chat\", (m, peer) => channel.send(\"chat\", peer, len(str(m))))\nroute GET / = \"hi\"\n";
    std::fs::write(root.join("bundle/main.qwe"), src).unwrap();
    checks(&root.join("bundle/main.qwe"));
    // One program for this process, and the limit for all of it while it runs: nothing else here
    // reads values past 64 KB in this process. Put back, and the program stopped, however the
    // test ends.
    struct Limit(u64);
    impl Drop for Limit {
        fn drop(&mut self) {
            quillo::httpd::limit_memory(self.0);
        }
    }
    let _limit = Limit(quillo::httpd::memory_limit());
    quillo::httpd::limit_memory(64 << 10);
    struct Running(Program, PathBuf);
    impl Drop for Running {
        fn drop(&mut self) {
            self.0.stop();
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }
    let running = Running(Program::open(&root.join("bundle"), &root.join("data"), "", "{}", "app").unwrap(), root.clone());
    let p = &running.0;
    let own = || vec![("origin".to_string(), "quillo://app".to_string())];
    let t = p.request_whole(request_of("POST", "quillo://app/_quillo/wt-ticket", own(), vec![]));
    let ticket: serde_json::Value = serde_json::from_slice(&t.body).unwrap();
    let url = format!("quillo://app/_quillo/wt?t={}", ticket["ticket"].as_str().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let conn = p.connect(request_of("GET", &url, own(), vec![]), Box::new(move |f| tx.send(f).is_ok())).unwrap();
    let next = || loop {
        match rx.recv_timeout(secs(60)).expect("no answer") {
            Frame::Text(t) => return t,
            _ => {}
        }
    };
    assert!(p.send(conn, Frame::Text("[\"j\",\"chat\"]".into())));
    assert_eq!(next(), "[\"j\",\"chat\",true]");
    let dropped = || quillo::wt::DROPPED.load(Ordering::SeqCst);
    for (m, want) in [(letters(65_243), 65_243), (letters(65_242), 65_242), (escaped(9_000, 2_478), 11_478)] {
        assert!(p.send(conn, Frame::Text(m)));
        assert_eq!(next(), format!("[\"m\",\"chat\",{}]", want));
    }
    let before = dropped();
    assert!(p.send(conn, Frame::Text(escaped(9_000, 2_479))));
    assert!(p.send(conn, Frame::Text("[\"m\",\"chat\",\"after\"]".into())));
    assert_eq!(next(), "[\"m\",\"chat\",5]");
    assert_eq!(dropped(), before + 1);
    p.close(conn);
}

// ---- each client's counter of what waits, past 1,024 clients ------------------------------

const TWO_SLOW: &str = r#"
let go = false
let fast = []
channel.on("slow", (m, p) => {
    // A handler that does not keep up.
    while !go {
        sleep(20)
    }
}, { join: (peer) => true })
channel.on("slow2", (m, p) => {
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

/// The counters of what each client's pages have waiting are let go past 1,024 clients, those of
/// clients still connected kept: under `--max-memory 32MB` (a client's share 4 MiB), a page of
/// 10.1.0.1 holds 4,000,000 bytes waiting behind a slow handler; 1,100 other clients (by a trusted
/// proxy's `X-Forwarded-For`) connect and go; a second page of 10.1.0.1, connected after, shares
/// the first's count, so its message of 300,000 bytes to another slow channel waits (its next
/// message, to `fast`, not read), while a page of 10.1.0.2 doing the same has its `fast` message
/// heard at once; all are handled once the handlers go on (src/wt.rs `client_counter`'s
/// `retain`, FEEDBACK75.md Q-1056; coverage of 0.8.9; branch feedback75)
#[test]
fn client_counters_past_1024_clients_keep_those_still_connected() {
    let d = Dir::new("counters", &[("main.qwe", TWO_SLOW)]);
    let r = run(&d.file("main.qwe"), &["--no-wt", "--max-memory", "32MB", "--trust-proxy", "127.0.0.1"], &[]);
    let t = r.ticket("", "")["ticket"].as_str().unwrap().to_string();
    let mut first = Ws::connect(&r, &t, "", Some("10.1.0.1"));
    assert!(first.join("slow"));
    let big = format!("[\"m\",\"slow\",\"{}\"]", "x".repeat(999_998));
    for _ in 0..4 {
        first.text(&big);
    }
    assert!(first.read_up_to_here(), "the four messages were not all taken");
    // 1,100 clients come and go: past 1,024, the counters of those gone are let go.
    let mut t = t;
    for i in 0..1100u32 {
        // A ticket is good for two minutes: a fresh one now and then, for a slow build.
        if i % 100 == 99 {
            t = r.ticket("", "")["ticket"].as_str().unwrap().to_string();
        }
        let ws = Ws::connect(&r, &t, "", Some(&format!("10.2.{}.{}", i / 250, i % 250 + 1)));
        drop(ws);
    }
    let t = r.ticket("", "")["ticket"].as_str().unwrap().to_string();
    let mut same = Ws::connect(&r, &t, "", Some("10.1.0.1"));
    let mut other = Ws::connect(&r, &t, "", Some("10.1.0.2"));
    for ws in [&mut same, &mut other] {
        assert!(ws.join("slow2") && ws.join("fast"));
    }
    let mid = format!("[\"m\",\"slow2\",\"{}\"]", "y".repeat(299_998));
    same.text(&mid);
    same.text(&format!("[\"m\",\"fast\",\"{}\"]", "s".repeat(22)));
    other.text(&mid);
    other.text(&format!("[\"m\",\"fast\",\"{}\"]", "o".repeat(33)));
    eventually(60, "another client's message was not heard", || (r.get("/fast") == "[33]").then_some(()));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(r.get("/fast"), "[33]", "the second page of 10.1.0.1 did not share its client's count");
    assert_eq!(r.get("/go"), "going");
    eventually(60, "the waiting page's message was not heard once the handlers went on", || (r.get("/fast") == "[33,22]").then_some(()));
    drop((first, same, other));
}

// ---- a mounted page and a split service's channel ----------------------------------------

const SPLIT_SERVICE: &str = r#"
channel.on("staff", (m, p) => channel.send("staff", p, { seen: p.session }), { join: (peer) => true })
channel.on("bosses", (m, p) => channel.send("bosses", p, { seen: p.session }), { join: (peer) => peer.session.user == "boss" })
"#;

const SPLIT_APP: &str = r#"
service chat = "chat.qwe"
mount /b = "b.qwe"
route writes GET /login {
    set_session({ user: "boss" })
    "the root's boss"
}
route GET / = "app"
"#;

const SPLIT_MOUNT: &str = r#"
route writes GET /login {
    set_session({ user: "boss" })
    "b's boss"
}
route GET / = "b"
"#;

/// Under `--split`, a page of a program the app mounts, joining a channel of the app's service in
/// its own process, is sent with no session: B's own `boss` does not pass the service's `join`,
/// and its handler sees `{}`; the root's own page, with the same session in the root, is let in
/// and seen as it is (src/wt.rs `forward`, another program's page, FEEDBACK75.md Q-1061; coverage
/// of 0.8.9; branch feedback75)
#[test]
fn a_mounted_pages_session_is_not_sent_to_a_split_services_channel() {
    let d = Dir::new("split-mount", &[("app.qwe", SPLIT_APP), ("b.qwe", SPLIT_MOUNT), ("chat.qwe", SPLIT_SERVICE)]);
    checks(&d.file("chat.qwe"));
    let r = run(&d.file("app.qwe"), &["--split", "--no-wt"], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    let root = session_cookie(&r.get_as("/login", ""));
    let b = session_cookie(&r.get_as("/b/login", ""));
    assert_eq!(r.get_as("/b/", &b).body, "b");
    let mut mine = Ws::open_at(&r, "", &root, "");
    eventually(60, "the service did not let the root's boss in", || mine.join("bosses").then_some(()));
    mine.text("[\"m\",\"bosses\",1]");
    assert_eq!(mine.text_frame(), "[\"m\",\"bosses\",{\"seen\":{\"user\":\"boss\"}}]");
    assert!(mine.join("staff"));
    mine.text("[\"m\",\"staff\",1]");
    assert_eq!(mine.text_frame(), "[\"m\",\"staff\",{\"seen\":{\"user\":\"boss\"}}]");
    // B's page, with B's own boss: no session at the service.
    let mut theirs = Ws::open_at(&r, "/b", &b, "");
    assert!(!theirs.join("bosses"), "a mounted program's session passed the service's join");
    assert!(theirs.join("staff"));
    theirs.text("[\"m\",\"staff\",\"é\"]");
    assert_eq!(theirs.text_frame(), "[\"m\",\"staff\",{\"seen\":{}}]");
    assert!(!r.log().contains("did not take what pages sent"), "{}", r.log());
}

// ---- which programs are given tickets ---------------------------------------------------

/// The program in `file`, loaded in this process.
fn program(dir: &Path, file: &str) -> quillo::Interp {
    let mut it = quillo::Interp::new(dir.to_str().unwrap());
    it.load_file(&dir.join(file)).unwrap_or_else(|e| panic!("{}: {}", file, e));
    it
}

/// What `/_quillo/wt-ticket` answers a page of the program `it`.
fn ticket_status(it: &mut quillo::Interp) -> u16 {
    let req = quillo::server::HttpRequest {
        method: "POST".into(),
        url: "/_quillo/wt-ticket".into(),
        headers: vec![("Host".into(), "localhost:8080".into()), ("Sec-Fetch-Site".into(), "same-origin".into())],
        body: Vec::new(),
        remote: "127.0.0.1".into(),
    };
    let a = it.handle(&req);
    if a.status == 200 {
        assert!(a.text().contains("\"ticket\""), "{}", a.text());
    } else {
        assert_eq!(a.text(), "not found\n");
    }
    a.status
}

/// A program that takes in a script for its pages (`client "chart.js" use Chart`), and nothing
/// else of channels, is given a ticket, as one that names `channel` is; a program of routes alone
/// is told 404; and the verdicts stay right past the eight programs remembered, the first ones
/// worked out again once let go (src/wt.rs `may_name_channels`' `use` of what is not Quillo,
/// `uses_channels`' oldest let go, FEEDBACK75.md Q-1052; coverage of 0.8.9; branch feedback75)
#[test]
fn a_ticket_is_given_to_a_program_with_a_script_and_verdicts_outlast_eight_programs() {
    let mut files: Vec<(String, String, u16)> = Vec::new();
    files.push(("script.qwe".into(), "client \"chart.js\" use Chart\nroute GET / = \"hi\"\n".into(), 200));
    for i in 0..5 {
        files.push((format!("plain{}.qwe", i), format!("route GET / = \"plain {}\"\n", i), 404));
        files.push((format!("named{}.qwe", i), format!("route GET / = \"hi\"\nroute GET /n = str(channel.broadcast(\"news{}\", 1))\n", i), 200));
    }
    let mut all: Vec<(&str, &str)> = files.iter().map(|(f, s, _)| (f.as_str(), s.as_str())).collect();
    all.push(("chart.js", "var Chart = 1;\n"));
    let d = Dir::new("tickets", &all);
    for (f, _, _) in &files {
        checks(&d.file(f));
    }
    // Eleven programs loaded at once, each asked: the first three are let go of what is kept,
    // and asked again are worked out again; the last is kept.
    let mut its: Vec<quillo::Interp> = files.iter().map(|(f, _, _)| program(&d.0, f)).collect();
    let order: Vec<usize> = (0..files.len()).chain([0, 1, 2, files.len() - 1, files.len() - 1]).collect();
    for i in order {
        assert_eq!(ticket_status(&mut its[i]), files[i].2, "{}", files[i].0);
    }
}

// ---- WebTransport ------------------------------------------------------------------------

#[cfg(feature = "realtime")]
mod webtransport {
    use super::*;
    use wtransport::{ClientConfig, Connection, Endpoint};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
    }

    /// A session to the server's WebTransport, asked again only when wtransport's own driver
    /// dropped the attempt before Quillo was asked (FEEDBACK75.md, Q-1124; see feedback75a.rs's
    /// `connect_at`). At most five tries.
    async fn connect(r: &Run) -> Result<Connection, String> {
        let mut last = String::new();
        for _ in 0..5 {
            match tokio::time::timeout(secs(30), connect_once(r)).await {
                Ok(Err(e)) if dropped_by_wtransport(&e) => last = e,
                Ok(answer) => return answer,
                Err(_) => last = format!("no answer within {:?}", secs(30)),
            }
        }
        Err(last)
    }

    fn dropped_by_wtransport(e: &str) -> bool {
        e.strip_prefix("connection closed by peer: ").and_then(|c| c.split_whitespace().next()).and_then(|c| c.parse::<u64>().ok()).is_some_and(|c| (0x100..0x110).contains(&c))
    }

    async fn connect_once(r: &Run) -> Result<Connection, String> {
        let t = r.ticket("", "");
        let port = t["port"].as_u64().expect("no port");
        let hex = t["hash"].as_str().expect("no hash");
        let bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
        let hash = wtransport::tls::Sha256Digest::new(bytes.try_into().unwrap());
        let c = ClientConfig::builder().with_bind_default().with_server_certificate_hashes([hash]).build();
        let url = format!("https://127.0.0.1:{}/_quillo/wt?t={}", port, t["ticket"].as_str().unwrap());
        Endpoint::client(c).unwrap().connect(url).await.map_err(|e| e.to_string())
    }

    fn framed(t: &[u8]) -> Vec<u8> {
        let mut f = (t.len() as u32).to_be_bytes().to_vec();
        f.extend_from_slice(t);
        f
    }

    async fn read_framed(s: &mut wtransport::RecvStream) -> String {
        let mut len = [0u8; 4];
        s.read_exact(&mut len).await.unwrap();
        let mut b = vec![0u8; u32::from_be_bytes(len) as usize];
        s.read_exact(&mut b).await.unwrap();
        String::from_utf8(b).unwrap()
    }

    /// A session that joined `ch`, and its first stream.
    async fn joined(r: &Run, ch: &str) -> (Connection, wtransport::SendStream, wtransport::RecvStream) {
        let conn = connect(r).await.unwrap();
        let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
        out.write_all(&framed(serde_json::json!(["j", ch]).to_string().as_bytes())).await.unwrap();
        let answer = tokio::time::timeout(secs(30), read_framed(&mut inn)).await.expect("the join was not answered");
        assert_eq!(answer, serde_json::json!(["j", ch, true]).to_string());
        (conn, out, inn)
    }

    /// Whether the server let the stream go (a stop) within `within`.
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

    /// Datagrams, which cannot wait for room, past a page's share of what waits (65,536 bytes
    /// under `--max-memory 64KB`) are dropped and counted: of 120 messages of 902 bytes as JSON
    /// sent while the handler waits, at most 72 are handled once it goes on, the rest counted as
    /// dropped (src/wt.rs `Inbox::push` past a share, FEEDBACK75.md Q-1056; coverage of 0.8.9;
    /// branch feedback75)
    #[test]
    fn datagrams_past_a_pages_share_are_dropped_and_counted() {
        let d = Dir::new("wt-datagrams", &[("main.qwe", SLOW_CHAT)]);
        let r = run(&d.file("main.qwe"), &["--max-memory", "64KB", "--metrics"], &[]);
        let sent = 120;
        rt().block_on(async {
            let (conn, _out, _inn) = joined(&r, "chat").await;
            let msg = format!("[\"m\",\"chat\",\"{}\"]", "x".repeat(900));
            let most = conn.max_datagram_size().expect("datagrams are not taken");
            assert!(msg.len() <= most, "{} > {}", msg.len(), most);
            for i in 0..sent {
                conn.send_datagram(msg.as_bytes()).unwrap();
                if i % 10 == 9 {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
            // The drops counted: all past the 72 that fit, or (a datagram lost on the way) none
            // more for 3 s.
            let (mut last, mut since) = (-1, Instant::now());
            let deadline = Instant::now() + secs(60);
            loop {
                let now = r.metric("quillo_channel_dropped_total");
                if now >= sent - 72 {
                    break;
                }
                if now != last {
                    (last, since) = (now, Instant::now());
                } else if now > 0 && since.elapsed() > secs(3) {
                    break;
                }
                assert!(Instant::now() < deadline, "no datagram was dropped ({})", now);
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            assert_eq!(r.get("/go"), "going");
            let got: Vec<i64> = eventually(60, "the datagrams that waited were not handled", || {
                let a: Vec<i64> = serde_json::from_str(&r.get("/got")).unwrap();
                let b: Vec<i64> = {
                    std::thread::sleep(Duration::from_millis(500));
                    serde_json::from_str(&r.get("/got")).unwrap()
                };
                (!a.is_empty() && a == b).then_some(b)
            });
            let dropped = r.metric("quillo_channel_dropped_total");
            assert!(got.iter().all(|n| *n == 900), "{:?}", got);
            assert!(got.len() <= 72, "{} datagrams of 902 bytes waited within 65,536", got.len());
            assert!(dropped >= 1 && got.len() as i64 + dropped <= sent, "{} handled, {} dropped of {}", got.len(), dropped, sent);
            drop(conn);
        });
    }

    /// A media frame of the page's.
    fn media_frame(len: usize) -> Vec<u8> {
        let mut wire = vec![3u8];
        wire.extend_from_slice(b"cam");
        wire.extend_from_slice(&[1, 1]);
        wire.extend_from_slice(&7u32.to_be_bytes());
        wire.resize(len, 7);
        framed(&wire)
    }

    /// A WebTransport stream's frame past `SMALL` (5,000 bytes) is let go, never taken, while its
    /// client holds its whole share of the messages begun (a WebSocket of 127.0.0.1 with 1,048,640
    /// bytes begun, under `--max-memory 64KB`); once that page goes, the same frame on a new stream
    /// is taken (src/wt.rs `read_frame`'s refused room, FEEDBACK75.md Q-1074; coverage of 0.8.9;
    /// branch feedback75)
    #[test]
    fn a_webtransport_frame_past_its_clients_share_is_let_go() {
        let src = "media.on(\"cam\", { publish: (p) => true, watch: (p) => true })\nroute GET /streams = media.streams()\n";
        let d = Dir::new("wt-share", &[("main.qwe", src)]);
        let r = run(&d.file("main.qwe"), &["--max-memory", "64KB"], &[]);
        let frames = || -> i64 {
            let v: serde_json::Value = serde_json::from_str(&r.get("/streams")).unwrap();
            v.as_array().and_then(|a| a.first()).map_or(0, |s| s["video"]["frames"].as_i64().unwrap_or(0))
        };
        let mut ws = Ws::open(&r);
        ws.frame(false, 1, &vec![b'x'; 1_048_640]).unwrap();
        assert!(ws.read_up_to_here(), "the WebSocket's message begun was refused");
        rt().block_on(async {
            let (conn, _out, _inn) = joined(&r, "media:cam").await;
            let mut s = conn.open_uni().await.unwrap().await.unwrap();
            s.write_all(&media_frame(5000)).await.unwrap();
            assert!(stopped(&mut s, secs(30)).await, "a frame past its client's share was read on");
            assert_eq!(frames(), 0, "a frame past its client's share was taken");
            // The WebSocket goes: its share is given back, and the frame is taken.
            drop(ws);
            let deadline = Instant::now() + secs(60);
            while frames() == 0 {
                assert!(Instant::now() < deadline, "the frame was never taken once the share was given back");
                let mut s = conn.open_uni().await.unwrap().await.unwrap();
                s.write_all(&media_frame(5000)).await.unwrap();
                for _ in 0..20 {
                    if frames() > 0 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            assert_eq!(frames(), 1);
        });
    }
}
