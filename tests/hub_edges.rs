//! The channels' hub (src/wt.rs), in this process: pages are `wt::connect`ed
//! peers with the test reading their queues, and a small server in the test
//! takes what the hub delivers to `wt::answer`, as the real one does, and can
//! hold it back or answer badly.

mod support;

use quillo::media;
use quillo::server::{HttpRequest, HttpResponse};
use quillo::wt::{self, Out, Peer};
use quillo::Interp;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Receiver;

const PROGRAM: &str = r#"
let heard = []
fn log(x) {
    heard = heard + [x]
}
let jam_count = 0
let jam_second = null

channel.on("chat", (m, peer) => {
    log(`${peer.session.name} says ${m.text}`)
}, { leave: (p) => log(`${p.session.name} left`) })

channel.on("vip", (m) => log(m), { join: (p) => p.session.name == "ana" })
channel.on("nobody", (m) => null, { join: (p) => 1 / len("") })

channel.on("roster", () => null, { join: (p) => {
    channel.broadcast("roster", "someone joins")
    channel.broadcast("roster", "not to them", { except: p })
    channel.send("roster", p, "welcome", { reliable: true })
    true
} })

channel.on("broken", (m) => 1 / len(""))

fn jammed(m) {
    jam_count = jam_count + 1
    if jam_count == 2 {
        jam_second = m
    }
}
channel.on("jam", jammed)
channel.on("jam2", (m) => null)

channel.on("full", (m) => null)
channel.on("gone", (m) => null)
channel.on("plain", () => log("no arguments"))
media.on("rooms:*")
media.on("cam", { publish: (p) => p.session.name == "ana" })
media.on("open")

route GET /heard = heard
"#;

/// How the test's server treats a channel's batches.
#[derive(Clone)]
enum Mode {
    /// Not answered until the flag is cleared.
    Hold(Arc<AtomicBool>),
    /// The connection is dropped, unanswered.
    Drop,
    /// What is not a list of verdicts.
    Garbage,
    /// As many verdicts as the given number, whatever was asked.
    Verdicts(usize),
}

struct Hub {
    it: Mutex<Interp>,
    modes: Mutex<HashMap<String, Mode>>,
    token_seen: AtomicBool,
    dropped: AtomicUsize,
    /// Batches held back so far.
    held: AtomicUsize,
}

fn read_request(s: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 8192]);
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
    while buf.len() < end + len {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    Some((head, buf[end..end + len].to_vec()))
}

fn serve_connection(mut s: TcpStream, hub: &Hub) {
    while let Some((head, body)) = read_request(&mut s) {
        let path = head.split(' ').nth(1).unwrap_or("");
        let name = path.strip_prefix("/_quillo/channel/").unwrap_or("").to_string();
        if head.contains(&format!("X-Quillo-Job: {}\r\n", quillo::jobs::token())) {
            hub.token_seen.store(true, Ordering::SeqCst);
        }
        let mode = hub.modes.lock().unwrap().get(&name).cloned();
        let answer = match mode {
            Some(Mode::Hold(on)) => {
                hub.held.fetch_add(1, Ordering::SeqCst);
                while on.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                None
            }
            Some(Mode::Drop) => {
                hub.dropped.fetch_add(1, Ordering::SeqCst);
                return;
            }
            Some(Mode::Garbage) => Some(b"not a list".to_vec()),
            Some(Mode::Verdicts(n)) => Some(serde_json::to_vec(&vec![true; n]).unwrap()),
            None => None,
        };
        let answer = answer.unwrap_or_else(|| wt::answer(&mut hub.it.lock().unwrap(), &name, &body).body);
        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", answer.len());
        let _ = s.write_all(&answer);
    }
}

/// The hub, served once for this process.
fn hub() -> &'static Hub {
    static HUB: OnceLock<Hub> = OnceLock::new();
    HUB.get_or_init(|| {
        // A page that asks before the server listens: its asking waits.
        let (early, rx) = peer("{\"name\":\"early\"}");
        wt::from_page(&early, "[\"j\",\"chat\"]");
        std::mem::forget(early);
        EARLY_PEER.lock().unwrap().replace(rx);

        let d = std::env::temp_dir().join(format!("quillo-hub-edges-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut it = Interp::new(&d);
        it.load_source(PROGRAM, "main.qwe").unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        wt::serving(addr);
        // Only the first counts.
        wt::serving("127.0.0.1:9".parse().unwrap());
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(s) = s else { return };
                std::thread::spawn(move || serve_connection(s, hub()));
            }
        });
        Hub { it: Mutex::new(it), modes: Default::default(), token_seen: AtomicBool::new(false), dropped: AtomicUsize::new(0), held: AtomicUsize::new(0) }
    })
}

static EARLY_PEER: Mutex<Option<Receiver<Out>>> = Mutex::new(None);

fn ticket(session: &str) -> wt::Ticket {
    let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 100;
    let sealed = quillo::session::seal("quillo channel ticket", &serde_json::json!({ "s": session, "t": null, "e": exp }).to_string());
    wt::ticket_of(&format!("t={}", sealed)).unwrap()
}

fn peer(session: &str) -> (Arc<Peer>, Receiver<Out>) {
    wt::connect(ticket(session), None)
}

fn joined_peer(session: &str, ch: &str) -> (Arc<Peer>, Receiver<Out>) {
    hub();
    let (p, mut rx) = peer(session);
    wt::from_page(&p, &serde_json::json!(["j", ch]).to_string());
    assert_eq!(text(&mut rx), serde_json::json!(["j", ch, true]).to_string());
    (p, rx)
}

fn next(rx: &mut Receiver<Out>) -> Out {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(o) = rx.try_recv() {
            return o;
        }
        assert!(Instant::now() < deadline, "nothing came");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn text(rx: &mut Receiver<Out>) -> String {
    match next(rx) {
        Out::Text { json, .. } => json,
        Out::Media(_) => panic!("a frame came"),
        Out::Ping | Out::Pong(_) => panic!("a ping came"),
        Out::Close => panic!("it was closed"),
    }
}

fn ev(src: &str) -> String {
    let h = hub();
    let mut it = h.it.lock().unwrap();
    match it.load_source(src, "after.qwe") {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn heard(what: &str) {
    eventually(10, &format!("{:?} was not heard", what), || ev("heard").contains(what).then_some(()));
}

fn fails(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
}

/// One test at a time counts what is dropped.
static COUNTING: Mutex<()> = Mutex::new(());

#[test]
fn the_builtins_say_what_they_take() {
    let f = "fn on(m) = null\n";
    assert!(fails("channel.on()").contains("`channel.on` takes a channel's name first, got nothing"));
    assert!(fails("channel.on(1)").contains("got int"));
    assert!(fails(&format!("{}channel.on(\"a b\", on)", f)).contains("`channel.on`: a channel's name is letters, digits"));
    assert!(fails(&format!("{}channel.on(\"{}\", on)", f, "x".repeat(65))).contains("a channel's name is letters"));
    assert!(fails("channel.on(\"a\", 1)").contains("needs a function to call with each message, got int"));
    assert!(fails("channel.on(\"a\")").contains("got nothing"));
    assert!(fails(&format!("{}channel.on(\"a\", on, {{ join: 1 }})", f)).contains("`join` is a function of the page"));
    assert!(fails(&format!("{}channel.on(\"a\", on, {{ leave: 1 }})", f)).contains("`leave` is a function of the page"));
    assert!(fails(&format!("{}channel.on(\"a\", on, {{ later: 1 }})", f)).contains("no option `later` (it takes join and leave)"));
    assert!(fails(&format!("{}channel.on(\"a\", on, 5)", f)).contains("expects options as a map, got int"));
    assert!(fails(&format!("{}channel.on(\"a\", on)\nchannel.on(\"a\", on)", f)).contains("already handles channel \"a\""));
    assert!(fails("channel.send(\"a\", 5, 1)").contains("takes a peer (as the channel's function gets it, or its id) second"));
    assert!(fails("channel.send(\"a\", { id: 5 }, 1)").contains("takes a peer"));
    assert!(fails("channel.send(\"a\", {}, 1)").contains("takes a peer"));
    assert!(fails("channel.send(\"a\", \"not hex\", 1)").contains("takes a peer"));
    assert!(fails("channel.send(\"a\", \"1\", 1, { except: 2 })").contains("`channel.send` has no option `except`"));
    assert!(fails("channel.broadcast(\"a\", 1, { to: 2 })").contains("`channel.broadcast` has no option `to`"));
    assert!(fails("channel.broadcast(\"a\", 1, 5)").contains("`channel.broadcast` expects options as a map, got int"));
    assert!(fails("channel.broadcast(\"a\", \"x\".repeat(1100000))").contains("`channel.broadcast`: the message is 1100012 bytes as JSON; at most 1048576 go to a page"));
    assert!(fails("channel.send(\"a\", \"1\", \"x\".repeat(1100000))").contains("`channel.send`: the message is"));
    assert!(fails("channel.peers(1)").contains("`channel.peers` takes a channel's name first, got int"));
    assert!(fails("channel.open(\"a b\", (m) => m)").contains("`channel.open`: a channel's name"));

    // Opened while a page is made on the server: nothing to send on.
    assert_eq!(quillo::eval_str("channel.open(\"a\", (m) => m).send(1)").unwrap().repr(), "false");
    // Builtins for functions, and options that say nothing.
    let mut it = Interp::new(std::env::temp_dir());
    it.load_source("channel.on(\"a\", len, { join: str, leave: len })\nchannel.on(\"b\", len, null)\nchannel.on(\"c\", len, {})", "c.qwe").unwrap();
    assert!(it.site.channels[0].join.is_some() && it.site.channels[0].leave.is_some() && it.site.channels[1].join.is_none());
}

#[test]
fn a_page_is_let_in_or_refused_and_only_those_let_in_are_heard() {
    let (ana, mut ana_rx) = joined_peer("{\"name\":\"ana\"}", "vip");
    let (bo, mut bo_rx) = peer("{\"name\":\"bo\"}");
    wt::from_page(&bo, "[\"j\",\"vip\"]");
    assert_eq!(text(&mut bo_rx), "[\"j\",\"vip\",false]");
    // A message from a page not let in goes nowhere; leaving where it never was, too.
    wt::from_page(&bo, "[\"m\",\"vip\",\"from bo\"]");
    wt::from_page(&bo, "[\"l\",\"vip\"]");
    wt::from_page(&ana, "[\"j\",\"vip\"]");
    wt::from_page(&ana, "[\"m\",\"vip\",\"from ana\"]");
    heard("from ana");
    assert!(!ev("heard").contains("from bo"));
    // A message with no value, for a function that takes one.
    let (p, _rx) = joined_peer("{}", "plain");
    wt::from_page(&p, "[\"m\",\"plain\"]");
    heard("no arguments");
    // What makes no sense is passed over.
    for bad in ["not json", "{}", "[]", "[1]", "[\"j\"]", "[\"j\",\"no spaces\"]", "[\"x\",\"vip\"]", "[1,\"vip\"]"] {
        wt::from_page(&ana, bad);
    }
    assert_eq!(ev("channel.peers(\"vip\")").matches(',').count(), 0);
    assert_eq!(ev("channel.broadcast(\"vip\", \"to all\")"), "1");
    assert_eq!(text(&mut ana_rx), "[\"m\",\"vip\",\"to all\"]");
    assert!(bo_rx.try_recv().is_err());
    wt::disconnect(&ana);
    wt::disconnect(&bo);
    // A join that fails is a no.
    let (c, mut c_rx) = peer("{}");
    wt::from_page(&c, "[\"j\",\"nobody\"]");
    assert_eq!(text(&mut c_rx), "[\"j\",\"nobody\",false]");
    wt::disconnect(&c);
}

#[test]
fn what_is_said_while_a_page_is_let_in_reaches_it_after() {
    hub();
    let (a, mut a_rx) = peer("{}");
    wt::from_page(&a, "[\"j\",\"roster\"]");
    assert_eq!(text(&mut a_rx), "[\"j\",\"roster\",true]");
    assert_eq!(text(&mut a_rx), "[\"m\",\"roster\",\"someone joins\"]");
    assert_eq!(text(&mut a_rx), "[\"m\",\"roster\",\"welcome\"]");
    assert!(a_rx.try_recv().is_err(), "held back from the page it was not for");
    // One in the room, and the next one coming.
    let (b, mut b_rx) = peer("{}");
    wt::from_page(&b, "[\"j\",\"roster\"]");
    assert_eq!(text(&mut b_rx), "[\"j\",\"roster\",true]");
    assert_eq!(text(&mut b_rx), "[\"m\",\"roster\",\"someone joins\"]");
    assert_eq!(text(&mut b_rx), "[\"m\",\"roster\",\"welcome\"]");
    assert_eq!(text(&mut a_rx), "[\"m\",\"roster\",\"someone joins\"]");
    assert_eq!(text(&mut a_rx), "[\"m\",\"roster\",\"not to them\"]");
    // A page connected, in no channel and not coming in: nothing is held for it.
    let (c, _c_rx) = peer("{}");
    let id = format!("{:012x}", c.id);
    assert_eq!(ev(&format!("channel.send(\"roster\", {:?}, 1)", id)), "false");
    assert_eq!(ev(&format!("channel.send(\"roster\", {{ id: {:?} }}, 1, null)", format!("{:012x}", a.id))), "true");
    assert_eq!(text(&mut a_rx), "[\"m\",\"roster\",1]");
    assert_eq!(ev("channel.send(\"roster\", \"ff\", 1)"), "false");
    for p in [&a, &b, &c] {
        wt::disconnect(p);
    }
}

#[test]
fn a_page_that_leaves_or_goes_is_out_of_its_channels() {
    let (a, mut a_rx) = joined_peer("{\"name\":\"mia\"}", "chat");
    let (b, mut b_rx) = joined_peer("{\"name\":\"noa\"}", "chat");
    let ids = ev("channel.peers(\"chat\")");
    assert!(ids.contains(&format!("{:012x}", a.id)) && ids.contains(&format!("{:012x}", b.id)), "{}", ids);
    wt::from_page(&a, "[\"m\",\"chat\",{\"text\":\"hello\"}]");
    heard("mia says hello");
    wt::from_page(&a, "[\"l\",\"chat\"]");
    heard("mia left");
    assert!(!ev("channel.peers(\"chat\")").contains(&format!("{:012x}", a.id)));
    assert!(a_rx.try_recv().is_err());
    // Its connection ends: as if it left, once.
    wt::disconnect(&b);
    wt::disconnect(&b);
    heard("noa left");
    assert_eq!(ev("heard").matches("noa left").count(), 1);
    assert!(!ev("channel.peers(\"chat\")").contains(&format!("{:012x}", b.id)));
    assert!(b_rx.try_recv().is_err());
    // The queue is gone: nothing can be sent to it.
    drop(a_rx);
    wt::from_page(&a, "[\"j\",\"gone\"]");
    eventually(10, "it did not get in", || ev("channel.peers(\"gone\")").contains(&format!("{:012x}", a.id)).then_some(()));
    assert_eq!(ev("channel.broadcast(\"gone\", 1)"), "0");
    wt::disconnect(&a);
    // A name ending in `*` takes every stream it starts.
    let (p, mut rx) = joined_peer("{}", "media:rooms:one");
    wt::from_page(&p, "[\"m\",\"media:rooms:one\",\"not for anyone\"]");
    wt::from_page(&p, "[\"l\",\"media:rooms:one\"]");
    wt::from_page(&p, "[\"j\",\"media:rooms:two\"]");
    assert_eq!(text(&mut rx), "[\"j\",\"media:rooms:two\",true]");
    wt::disconnect(&p);
}

#[test]
fn a_handler_that_fails_does_not_stop_what_comes_after() {
    let (p, mut rx) = joined_peer("{}", "broken");
    wt::from_page(&p, "[\"m\",\"broken\",1]");
    wt::from_page(&p, "[\"j\",\"chat\"]");
    assert_eq!(text(&mut rx), "[\"j\",\"chat\",true]");
    wt::disconnect(&p);
}

#[test]
fn a_page_that_does_not_keep_up_loses_messages_and_then_its_connection() {
    let _one = COUNTING.lock().unwrap_or_else(|e| e.into_inner());
    let (p, mut rx) = joined_peer("{}", "full");
    let dropped = wt::DROPPED.load(Ordering::SeqCst);
    // The queue holds 4096.
    let sent = ev("let n = 0\nfor i in 0..4196 {\n n = n + channel.broadcast(\"full\", i)\n}\nn");
    assert_eq!(sent, "4096");
    assert_eq!(wt::DROPPED.load(Ordering::SeqCst) - dropped, 100);
    // A message it was promised cannot wait: it is let go.
    assert_eq!(ev("channel.broadcast(\"full\", \"promised\", { reliable: true })"), "0");
    let mut n = 0;
    loop {
        match next(&mut rx) {
            Out::Text { json, reliable } => {
                assert!(!reliable && !json.contains("promised"), "{}", json);
                n += 1;
            }
            Out::Close => break,
            _ => panic!("what is this"),
        }
    }
    assert_eq!(n, 4096);
    wt::disconnect(&p);
}

/// A frame as a page or `media` sends it.
fn wire(stream: &str, kind: u8, key: bool, data: &[u8]) -> Vec<u8> {
    let mut w = vec![stream.len() as u8];
    w.extend_from_slice(stream.as_bytes());
    w.extend_from_slice(&[kind, key as u8, 0, 0, 0, 7]);
    w.extend_from_slice(data);
    w
}

#[test]
fn a_page_watches_a_stream_and_one_that_falls_behind_waits_for_a_keyframe() {
    let name = "watched";
    let (p, mut rx) = peer("{}");
    wt::from_page(&p, &format!("[\"v\",\"{}\"]", name));
    for _ in 0..4200 {
        media::from_page(wire(name, media::VIDEO, true, b"k"));
    }
    let mut frames = 0;
    while let Ok(o) = rx.try_recv() {
        assert!(matches!(o, Out::Media(_)));
        frames += 1;
    }
    assert_eq!(frames, 4096, "the rest were not taken");
    media::from_page(wire(name, media::VIDEO, false, b"d"));
    assert!(rx.try_recv().is_err(), "behind: only a keyframe will do");
    media::from_page(wire(name, media::VIDEO, true, b"k"));
    assert!(matches!(next(&mut rx), Out::Media(f) if f.key));
    // Stopping, it gets no more.
    wt::from_page(&p, &format!("[\"u\",\"{}\"]", name));
    media::from_page(wire(name, media::VIDEO, true, b"k"));
    assert!(rx.try_recv().is_err());
    // Its connection ends: it watches nothing.
    wt::from_page(&p, &format!("[\"v\",\"{}\"]", name));
    assert!(matches!(next(&mut rx), Out::Media(_)));
    wt::disconnect(&p);
    media::from_page(wire(name, media::VIDEO, true, b"k"));
    assert!(rx.try_recv().is_err());
    assert_eq!(ev(&format!("media.close({:?})", name)), "true");
}

#[test]
fn a_page_on_webtransport_gets_frames_on_their_own_queue() {
    let name = "own-queue";
    let (tx, mut frames) = tokio::sync::mpsc::channel(4);
    let (p, mut rx) = wt::connect(ticket("{}"), Some(tx));
    wt::from_page(&p, &format!("[\"v\",\"{}\"]", name));
    media::from_page(wire(name, media::VIDEO, true, b"k"));
    assert!(frames.try_recv().unwrap().key);
    assert!(rx.try_recv().is_err());
    assert_eq!(ev(&format!("media.close({:?})", name)), "true");
    wt::disconnect(&p);
}

#[test]
fn only_a_page_let_into_a_streams_channel_sends_it() {
    let (w, mut w_rx) = peer("{}");
    wt::from_page(&w, "[\"v\",\"cam\"]");
    let (ana, mut ana_rx) = peer("{\"name\":\"ana\"}");
    let (bo, mut bo_rx) = peer("{\"name\":\"bo\"}");
    // Nothing there to say which stream, or not text.
    wt::frame_from_page(&ana, Vec::new());
    wt::frame_from_page(&ana, vec![1, 0xff, 1, 1, 0, 0, 0, 0]);
    // Not let in yet.
    wt::frame_from_page(&ana, wire("cam", media::VIDEO, true, b"early"));
    wt::from_page(&ana, "[\"j\",\"media:cam\"]");
    assert_eq!(text(&mut ana_rx), "[\"j\",\"media:cam\",true]");
    wt::from_page(&bo, "[\"j\",\"media:cam\"]");
    assert_eq!(text(&mut bo_rx), "[\"j\",\"media:cam\",false]");
    wt::frame_from_page(&bo, wire("cam", media::VIDEO, true, b"bo"));
    wt::frame_from_page(&ana, wire("cam", media::VIDEO, true, b"ana"));
    match next(&mut w_rx) {
        Out::Media(f) => assert!(f.wire.ends_with(b"ana")),
        _ => panic!("no frame"),
    }
    assert!(w_rx.try_recv().is_err());
    // What a page sends as a message on the stream's channel goes nowhere.
    wt::from_page(&ana, "[\"m\",\"media:cam\",1]");
    // The stream ends: those sending it are out, and must ask again.
    assert_eq!(ev("media.close(\"cam\")"), "true");
    assert_eq!(text(&mut ana_rx), "[\"j\",\"media:cam\",false]");
    assert!(matches!(next(&mut w_rx), Out::Media(f) if f.kind == media::END));
    wt::frame_from_page(&ana, wire("cam", media::VIDEO, true, b"after"));
    assert!(w_rx.try_recv().is_err());
    for p in [&w, &ana, &bo] {
        wt::disconnect(p);
    }
}

#[test]
fn without_an_answer_no_one_is_let_in() {
    let h = hub();
    // The server drops the connection, each of 50 tries.
    h.modes.lock().unwrap().insert("dropped".into(), Mode::Drop);
    let (p, mut rx) = peer("{}");
    let before = h.dropped.load(Ordering::SeqCst);
    wt::from_page(&p, "[\"j\",\"dropped\"]");
    assert_eq!(text(&mut rx), "[\"j\",\"dropped\",false]");
    assert_eq!(h.dropped.load(Ordering::SeqCst) - before, 50);
    // It answers what is no list; or fewer than were asked.
    h.modes.lock().unwrap().insert("garbage".into(), Mode::Garbage);
    wt::from_page(&p, "[\"j\",\"garbage\"]");
    assert_eq!(text(&mut rx), "[\"j\",\"garbage\",false]");
    h.modes.lock().unwrap().insert("few".into(), Mode::Verdicts(0));
    wt::from_page(&p, "[\"j\",\"few\"]");
    assert_eq!(text(&mut rx), "[\"j\",\"few\",false]");
    assert!(h.token_seen.load(Ordering::SeqCst));
    wt::disconnect(&p);
}

#[test]
fn a_full_inbox_lets_go_of_messages_before_joins() {
    let _one = COUNTING.lock().unwrap_or_else(|e| e.into_inner());
    let h = hub();
    let (p, _rx) = joined_peer("{}", "jam");
    let hold = Arc::new(AtomicBool::new(true));
    h.modes.lock().unwrap().insert("jam".into(), Mode::Hold(hold.clone()));
    // This one is taken, and held by the server; the rest wait in the inbox.
    let held = h.held.load(Ordering::SeqCst);
    wt::from_page(&p, "[\"m\",\"jam\",0]");
    eventually(10, "the server was not asked", || (h.held.load(Ordering::SeqCst) > held).then_some(()));
    let dropped = wt::DROPPED.load(Ordering::SeqCst);
    for n in 1..=10_001 {
        wt::from_page(&p, &format!("[\"m\",\"jam\",{}]", n));
    }
    assert_eq!(wt::DROPPED.load(Ordering::SeqCst) - dropped, 1);
    h.modes.lock().unwrap().remove("jam");
    hold.store(false, Ordering::SeqCst);
    // The oldest went: 0 was with the server, 1 was let go, 2 comes next.
    eventually(30, "the messages did not come", || (ev("jam_count") == "10001").then_some(()));
    assert_eq!(ev("jam_second"), "2");

    // Nothing but joins waiting: they are kept, and a message is not.
    let (p, _rx) = joined_peer("{}", "jam2");
    let hold = Arc::new(AtomicBool::new(true));
    h.modes.lock().unwrap().insert("jam2".into(), Mode::Hold(hold.clone()));
    let held = h.held.load(Ordering::SeqCst);
    wt::from_page(&p, "[\"m\",\"jam2\",0]");
    eventually(10, "the server was not asked", || (h.held.load(Ordering::SeqCst) > held).then_some(()));
    let (q, q_rx) = peer("{}");
    drop(q_rx);
    let dropped = wt::DROPPED.load(Ordering::SeqCst);
    for _ in 0..10_000 {
        wt::from_page(&q, "[\"j\",\"jam2\"]");
    }
    wt::from_page(&p, "[\"m\",\"jam2\",\"lost\"]");
    assert_eq!(wt::DROPPED.load(Ordering::SeqCst) - dropped, 1);
    wt::from_page(&q, "[\"j\",\"jam2\"]");
    assert_eq!(wt::DROPPED.load(Ordering::SeqCst) - dropped, 2);
    h.modes.lock().unwrap().remove("jam2");
    hold.store(false, Ordering::SeqCst);
    eventually(30, "the joins did not come", || ev("channel.peers(\"jam2\")").contains(&format!("{:012x}", q.id)).then_some(()));
    wt::disconnect(&p);
    wt::disconnect(&q);
}

fn line(kind: &str, peer: &str, value: &str) -> String {
    format!("{}\n", serde_json::json!({ "k": kind, "p": peer, "s": "{\"name\":\"x\"}", "t": null, "v": value }))
}

#[test]
fn a_batch_is_taken_a_line_at_a_time() {
    let d = std::env::temp_dir().join(format!("quillo-hub-batch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut it = Interp::new(&d);
    it.load_source("let got = []\nchannel.on(\"known\", (m) => { got = got + [m] })\nroute GET /got = got\n", "main.qwe").unwrap();
    let answer = |it: &mut Interp, ch: &str, body: &str| {
        let r = wt::answer(it, ch, body.as_bytes());
        (r.status, String::from_utf8_lossy(&r.body).into_owned())
    };
    // A channel no one handles: pages may join, and what they send goes nowhere.
    let body = format!("{}{}", line("j", "1", ""), line("m", "1", "1"));
    assert_eq!(answer(&mut it, "unknown", &body), (200, "[true]".to_string()));
    // Lines that make no sense, of a kind it does not know; a value and a
    // session that are no JSON, with no peer at all.
    let body = format!("nonsense\n{}{{\"k\":\"m\",\"s\":\"[1]\",\"v\":\"not json\"}}\n{{\"k\":\"m\",\"s\":\"nope\"}}\n{}", line("z", "1", ""), line("j", "not hex", ""));
    assert_eq!(answer(&mut it, "known", &body), (200, "[true]".to_string()));
    assert_eq!(it.load_source("got", "x.qwe").unwrap().repr(), "[null, null]");
}

#[test]
fn a_channel_of_a_mounted_program_is_handled_there() {
    let d = std::env::temp_dir().join(format!("quillo-hub-mount-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("inner.qwe"), "channel.on(\"deep\", (m, p) => { deep = deep + [m] }, { join: (p) => true })\nchannel.on(\"shut\", (m) => null, { join: (p) => false })\nlet deep = []\nroute GET /deep = deep\n").unwrap();
    std::fs::write(d.join("sub.qwe"), "mount /inner = \"inner.qwe\"\nroute GET / = \"sub\"\n").unwrap();
    std::fs::write(d.join("main.qwe"), "mount /sub = \"sub.qwe\"\nroute GET / = \"main\"\n").unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    let body = format!("{}{}", line("j", "a", ""), line("m", "a", "\"from a page\""));
    assert_eq!(wt::answer(&mut it, "deep", body.as_bytes()).body, b"[true]");
    assert_eq!(wt::answer(&mut it, "shut", line("j", "b", "").as_bytes()).body, b"[false]");
    assert_eq!(wt::answer(&mut it, "no-one-handles-this", line("j", "b", "").as_bytes()).body, b"[true]");
    let r: HttpResponse = it.handle(&HttpRequest { method: "GET".into(), url: "/sub/inner/deep".into(), headers: vec![("Host".into(), "localhost".into())], body: Vec::new(), remote: "127.0.0.1".into() });
    assert_eq!(String::from_utf8_lossy(&r.body), "[\"from a page\"]");
}

#[test]
fn what_a_service_apart_asks_of_the_apps_pages_is_done_here() {
    let (p, mut rx) = joined_peer("{}", "relayed");
    let relay = |j: serde_json::Value| {
        let r = wt::relay_here(j.to_string().as_bytes());
        (r.status, String::from_utf8_lossy(&r.body).into_owned())
    };
    let id = format!("{:x}", p.id);
    let msg = "[\"m\",\"relayed\",\"relayed\"]";
    assert_eq!(wt::relay_here(b"not json").status, 400);
    assert_eq!(relay(serde_json::json!({ "op": "b", "ch": "no spaces", "json": msg })).1, "null");
    assert_eq!(relay(serde_json::json!({ "op": "b", "ch": "relayed", "json": "x".repeat(wt::MAX_MESSAGE + 1) })).1, "null");
    assert_eq!(relay(serde_json::json!({ "op": "z", "ch": "relayed", "json": msg })).1, "null");
    assert_eq!(relay(serde_json::json!({ "op": "s", "ch": "relayed", "json": msg, "to": "not hex" })).1, "false");
    assert_eq!(relay(serde_json::json!({ "op": "s", "ch": "relayed", "json": msg, "to": id })).1, "true");
    assert_eq!(text(&mut rx), msg);
    assert!(relay(serde_json::json!({ "op": "b", "ch": "relayed", "json": msg, "reliable": true })).1.parse::<i64>().unwrap() >= 1);
    assert_eq!(text(&mut rx), msg);
    assert_eq!(relay(serde_json::json!({ "op": "b", "ch": "relayed", "json": msg, "except": id })).0, 200);
    assert!(rx.try_recv().is_err());
    assert!(relay(serde_json::json!({ "op": "p", "ch": "relayed" })).1.contains(&format!("{:012x}", p.id)));
    wt::disconnect(&p);
}

#[test]
fn a_ticket_is_good_until_it_is_not() {
    hub();
    let seal = |j: serde_json::Value| quillo::session::seal("quillo channel ticket", &j.to_string());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert!(wt::ticket_of("").is_none());
    assert!(wt::ticket_of("x=1").is_none());
    assert!(wt::ticket_of("t=forged").is_none());
    assert!(wt::ticket_of(&format!("t={}", quillo::session::seal("something else", "{}"))).is_none());
    assert!(wt::ticket_of(&format!("t={}", seal(serde_json::json!({ "s": "{}", "e": now - 5 })))).is_none(), "too old");
    assert!(wt::ticket_of(&format!("t={}", seal(serde_json::json!({ "s": "{}" })))).is_none(), "no end");
    assert!(wt::ticket_of(&format!("t={}", seal(serde_json::json!({ "e": now + 5 })))).is_none(), "no session");
    assert!(wt::ticket_of(&format!("a=1&t={}", seal(serde_json::json!({ "s": "{}", "t": "acme", "e": now + 5 })))).is_some());
    let (p, _rx) = peer("{}");
    assert!(wt::connected() >= 1);
    wt::disconnect(&p);

    // What a page is given: a ticket, and where to connect, once it is served.
    let r = wt::ticket(&mut hub().it.lock().unwrap());
    let j: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert!(wt::ticket_of(&format!("t={}", j["ticket"].as_str().unwrap())).is_some());
    assert_eq!(r.status, 200);
    assert!(r.headers.iter().any(|(k, v)| k == "cache-control" && v == "no-store"));
    assert_eq!(j["hash"].as_str().map(str::len), Some(64), "{}", j);
}

#[test]
fn a_page_that_asked_before_the_server_listened_is_answered() {
    hub();
    let mut rx = EARLY_PEER.lock().unwrap().take().unwrap();
    assert_eq!(text(&mut rx), "[\"j\",\"chat\",true]");
}
