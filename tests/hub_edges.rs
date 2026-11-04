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
media.on("rooms:*", { publish: (p) => true, watch: (p) => true })
media.on("cam", { publish: (p) => p.session.name == "ana", watch: (p) => true })
media.on("open", { publish: (p) => true, watch: (p) => true })
channel.on("lobby", (m) => channel.broadcast("lobby", m))
channel.on("hall", (m) => channel.broadcast("hall", m), { tenants: "shared" })
media.on("tv", { publish: (p) => true })
media.on("radio", { watch: (p) => true })
channel.on("who", (m) => channel.broadcast("who", channel.peers("who")))
channel.on("dm", (m) => channel.send("dm", m.to, "a word"))
media.on("cam2", { publish: (p) => true, watch: (p) => true })

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

/// A page's ticket on a tenant's host.
fn ticket_in(tenant: Option<&str>) -> wt::Ticket {
    let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 100;
    let sealed = quillo::session::seal("quillo channel ticket", &serde_json::json!({ "s": "{}", "t": tenant, "e": exp }).to_string());
    wt::ticket_of(&format!("t={}", sealed)).unwrap()
}

/// A page on `tenant` let into `ch`.
fn joined_in(tenant: Option<&str>, ch: &str) -> (Arc<Peer>, Receiver<Out>) {
    hub();
    let (p, mut rx) = wt::connect(ticket_in(tenant), None);
    wt::from_page(&p, &serde_json::json!(["j", ch]).to_string());
    assert_eq!(text(&mut rx), serde_json::json!(["j", ch, true]).to_string());
    (p, rx)
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
    assert!(fails(&format!("{}channel.on(\"a\", on, {{ later: 1 }})", f)).contains("no option `later` (it takes join, leave and tenants)"));
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
    wt::let_watch(&p, name);
    for _ in 0..4200 {
        media::from_page(&media::key(name, None), wire(name, media::VIDEO, true, b"k"));
    }
    let mut frames = 0;
    while let Ok(o) = rx.try_recv() {
        assert!(matches!(o, Out::Media(_)));
        frames += 1;
    }
    assert_eq!(frames, 4096, "the rest were not taken");
    media::from_page(&media::key(name, None), wire(name, media::VIDEO, false, b"d"));
    assert!(rx.try_recv().is_err(), "behind: only a keyframe will do");
    media::from_page(&media::key(name, None), wire(name, media::VIDEO, true, b"k"));
    assert!(matches!(next(&mut rx), Out::Media(f) if f.key));
    // Stopping, it gets no more.
    wt::from_page(&p, &format!("[\"u\",\"{}\"]", name));
    media::from_page(&media::key(name, None), wire(name, media::VIDEO, true, b"k"));
    assert!(rx.try_recv().is_err());
    // Its connection ends: it watches nothing.
    wt::let_watch(&p, name);
    assert!(matches!(next(&mut rx), Out::Media(_)));
    wt::disconnect(&p);
    media::from_page(&media::key(name, None), wire(name, media::VIDEO, true, b"k"));
    assert!(rx.try_recv().is_err());
    assert_eq!(ev(&format!("media.close({:?})", name)), "true");
}

#[test]
fn a_page_on_webtransport_gets_frames_on_their_own_queue() {
    let name = "own-queue";
    let (tx, mut frames) = tokio::sync::mpsc::channel(4);
    let (p, mut rx) = wt::connect(ticket("{}"), Some(tx));
    wt::let_watch(&p, name);
    media::from_page(&media::key(name, None), wire(name, media::VIDEO, true, b"k"));
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

// Q-644: rooms were keyed by the channel's name alone, so a page on tenant `evil` heard tenant
// `acme`'s `lobby` and wrote into it. Each tenant has its own room now; `tenants: "shared"` makes
// one for all, and the program with no tenant (a job, a script) reaches every tenant's.
#[test]
fn each_tenant_has_its_own_room_unless_the_channel_is_shared() {
    let (a1, mut a1_rx) = joined_in(Some("acme"), "lobby");
    let (a2, mut a2_rx) = joined_in(Some("acme"), "lobby");
    let (e1, mut e1_rx) = joined_in(Some("evil"), "lobby");
    wt::from_page(&e1, "[\"m\",\"lobby\",\"from evil\"]");
    assert_eq!(text(&mut e1_rx), "[\"m\",\"lobby\",\"from evil\"]");
    wt::from_page(&a1, "[\"m\",\"lobby\",\"from acme\"]");
    // The first each of acme's gets is acme's: evil's went to evil's room only.
    assert_eq!(text(&mut a1_rx), "[\"m\",\"lobby\",\"from acme\"]");
    assert_eq!(text(&mut a2_rx), "[\"m\",\"lobby\",\"from acme\"]");
    assert!(e1_rx.try_recv().is_err(), "acme's reached evil");
    // A page asking to send to a page of another tenant in its room: there is none.
    assert_eq!(ev(&format!("channel.send(\"lobby\", {:?}, 1)", format!("{:012x}", e1.id))), "true");
    assert_eq!(text(&mut e1_rx), "[\"m\",\"lobby\",1]");
    // From no tenant: every tenant's room.
    assert_eq!(ev("channel.broadcast(\"lobby\", \"to all\")"), "3");
    assert_eq!(ev("len(channel.peers(\"lobby\"))"), "3");
    for rx in [&mut a1_rx, &mut a2_rx, &mut e1_rx] {
        assert_eq!(text(rx), "[\"m\",\"lobby\",\"to all\"]");
    }
    // A tenant's own, as when acme's page is answered: acme's room alone.
    assert_eq!(ev("with_tenant(\"acme\", () => channel.broadcast(\"lobby\", \"acme only\"))"), "2");
    assert_eq!(ev("with_tenant(\"evil\", () => len(channel.peers(\"lobby\")))"), "1");
    assert_eq!(text(&mut a1_rx), "[\"m\",\"lobby\",\"acme only\"]");
    assert!(e1_rx.try_recv().is_err());
    // Shared: one room.
    let (h1, mut h1_rx) = joined_in(Some("acme"), "hall");
    let (h2, mut h2_rx) = joined_in(Some("evil"), "hall");
    let (h3, mut h3_rx) = joined_in(None, "hall");
    wt::from_page(&h2, "[\"m\",\"hall\",\"to everyone\"]");
    for rx in [&mut h1_rx, &mut h2_rx, &mut h3_rx] {
        assert_eq!(text(rx), "[\"m\",\"hall\",\"to everyone\"]");
    }
    assert_eq!(ev("with_tenant(\"acme\", () => len(channel.peers(\"hall\")))"), "3");
    // Leaving is leaving the tenant's room.
    wt::from_page(&a2, "[\"l\",\"lobby\"]");
    eventually(10, "acme's page did not leave", || (ev("with_tenant(\"acme\", () => len(channel.peers(\"lobby\")))") == "1").then_some(()));
    for p in [&a1, &a2, &e1, &h1, &h2, &h3] {
        wt::disconnect(p);
    }
}

// Q-642 and Q-643: with no `watch` rule any page watched any stream, and streams were one per
// name for every tenant. Watching is asked of `media.on` as joining is, and each tenant's
// stream of a name is its own.
#[test]
fn a_stream_is_watched_and_sent_only_as_its_rules_say_and_per_tenant() {
    hub();
    // `tv` has no watch rule: the page asking is refused, and gets nothing of what is sent.
    let (w, mut w_rx) = peer("{}");
    wt::from_page(&w, "[\"v\",\"tv\"]");
    let (s, mut s_rx) = peer("{}");
    wt::from_page(&s, "[\"j\",\"media:tv\"]");
    assert_eq!(text(&mut s_rx), "[\"j\",\"media:tv\",true]");
    wt::frame_from_page(&s, wire("tv", media::VIDEO, true, b"tv"));
    // `radio` has no publish rule: no page sends it.
    let (r, mut r_rx) = peer("{}");
    wt::from_page(&r, "[\"j\",\"media:radio\"]");
    assert_eq!(text(&mut r_rx), "[\"j\",\"media:radio\",false]");
    std::thread::sleep(Duration::from_millis(100));
    assert!(w_rx.try_recv().is_err(), "a page watched what no rule lets it");
    // `open`, per tenant: acme's page sees acme's frames, not evil's, nor those a page with
    // no tenant sends.
    let (none, mut none_rx) = peer("{}");
    wt::from_page(&none, "[\"j\",\"media:open\"]");
    assert_eq!(text(&mut none_rx), "[\"j\",\"media:open\",true]");
    wt::frame_from_page(&none, wire("open", media::VIDEO, true, b"none first"));
    let (aw, mut aw_rx) = wt::connect(ticket_in(Some("acme")), None);
    wt::from_page(&aw, "[\"v\",\"open\"]");
    let (evil, mut evil_rx) = joined_in(Some("evil"), "media:open");
    let (acme, mut acme_rx) = joined_in(Some("acme"), "media:open");
    wt::frame_from_page(&none, wire("open", media::VIDEO, true, b"none"));
    wt::frame_from_page(&evil, wire("open", media::VIDEO, true, b"evil"));
    wt::frame_from_page(&acme, wire("open", media::VIDEO, true, b"acme"));
    match next(&mut aw_rx) {
        Out::Media(f) => assert!(f.wire.ends_with(b"acme"), "{:?}", String::from_utf8_lossy(&f.wire)),
        _ => panic!("no frame"),
    }
    assert!(aw_rx.try_recv().is_err());
    // Its streams are its own and the program's: not another tenant's.
    let names = ev("with_tenant(\"evil\", () => media.streams().filter((s) => s.name == \"open\").map((s) => s.viewers))");
    assert_eq!(names, "[0]");
    assert_eq!(ev("with_tenant(\"acme\", () => media.streams().filter((s) => s.name == \"open\").map((s) => s.viewers))"), "[1]");
    let _ = (&mut evil_rx, &mut acme_rx, &mut none_rx, &mut r_rx);
    for p in [&w, &s, &r, &none, &aw, &evil, &acme] {
        wt::disconnect(p);
    }
}

// Found in review of Q-644: a page with no tenant (the bare domain) was taken for the program,
// which reaches every tenant's rooms; and `channel.send` reached a page of another tenant by its
// id. A page with no tenant has the rooms of no tenant.
#[test]
fn a_page_with_no_tenant_reaches_no_tenants_rooms() {
    let (acme, mut acme_rx) = joined_in(Some("acme"), "who");
    let (apex, mut apex_rx) = joined_in(None, "who");
    wt::from_page(&apex, "[\"m\",\"who\",1]");
    // It hears its own room's peers, and acme hears nothing.
    assert_eq!(text(&mut apex_rx), serde_json::json!(["m", "who", [format!("{:012x}", apex.id)]]).to_string());
    assert!(acme_rx.try_recv().is_err());
    wt::from_page(&acme, "[\"m\",\"who\",1]");
    assert_eq!(text(&mut acme_rx), serde_json::json!(["m", "who", [format!("{:012x}", acme.id)]]).to_string());
    assert!(apex_rx.try_recv().is_err());
    // A tenant's page sends to another tenant's by its id: it is not there.
    let (a, mut a_rx) = joined_in(Some("acme"), "dm");
    let (a2, mut a2_rx) = joined_in(Some("acme"), "dm");
    let (e, mut e_rx) = joined_in(Some("evil"), "dm");
    wt::from_page(&e, &serde_json::json!(["m", "dm", { "to": format!("{:012x}", a.id) }]).to_string());
    wt::from_page(&a2, &serde_json::json!(["m", "dm", { "to": format!("{:012x}", a.id) }]).to_string());
    // The first thing acme's page gets is acme's word: evil's went nowhere.
    assert_eq!(text(&mut a_rx), "[\"m\",\"dm\",\"a word\"]");
    std::thread::sleep(Duration::from_millis(100));
    assert!(a_rx.try_recv().is_err());
    let _ = (&mut a2_rx, &mut e_rx);
    for p in [&acme, &apex, &a, &a2, &e] {
        wt::disconnect(p);
    }
}

// Found in review of Q-642: a page with no tenant asking to watch made a stream that acme's
// viewer then joined for want of acme's own, and the page's frames reached acme. Only the
// program's own (RTP) stands in for a tenant's. And `media.close` from a job ends every
// tenant's stream of the name.
#[test]
fn a_tenants_viewer_joins_no_stream_a_page_made_and_a_job_ends_all() {
    hub();
    let (apex, mut apex_rx) = joined_in(None, "media:cam2");
    wt::from_page(&apex, "[\"v\",\"cam2\"]");
    let (aw, mut aw_rx) = wt::connect(ticket_in(Some("acme")), None);
    wt::from_page(&aw, "[\"v\",\"cam2\"]");
    // In the inbox behind both: the yes for acme's sender means both were answered.
    let (acme, mut acme_rx) = joined_in(Some("acme"), "media:cam2");
    wt::frame_from_page(&apex, wire("cam2", media::VIDEO, true, b"apex"));
    wt::frame_from_page(&acme, wire("cam2", media::VIDEO, true, b"acme"));
    match next(&mut aw_rx) {
        Out::Media(f) => assert!(f.wire.ends_with(b"acme"), "{:?}", String::from_utf8_lossy(&f.wire)),
        _ => panic!("no frame"),
    }
    assert!(aw_rx.try_recv().is_err());
    // The page with no tenant watches its own.
    match next(&mut apex_rx) {
        Out::Media(f) => assert!(f.wire.ends_with(b"apex")),
        _ => panic!("no frame"),
    }
    // From the program with no tenant, each tenant's `cam2` ends.
    assert_eq!(ev("media.close(\"cam2\")"), "true");
    assert!(matches!(next(&mut aw_rx), Out::Media(f) if f.kind == media::END));
    // Out of the room it sent in, then told the stream it watched ended.
    assert_eq!(text(&mut apex_rx), "[\"j\",\"media:cam2\",false]");
    assert!(matches!(next(&mut apex_rx), Out::Media(f) if f.kind == media::END));
    assert_eq!(text(&mut acme_rx), "[\"j\",\"media:cam2\",false]");
    for p in [&apex, &aw, &acme] {
        wt::disconnect(p);
    }
}
