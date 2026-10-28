//! Channels: messages between the server and its pages, as they happen.
//!
//! ```text
//! channel.on("game", (msg, peer) => {                 // each message from a page
//!     channel.broadcast("game", { id: peer.id, pos: msg.pos }, { except: peer })
//! })
//!
//! component Game() {
//!     let game = channel.open("game", (m) => { ... })  // in the browser
//!     <canvas @pointermove={(e) => game.send({ pos: [e.clientX, e.clientY] })}/>
//! }
//! ```
//!
//! A page opens one connection, shared by its channels: WebTransport (HTTP/3
//! over QUIC, so over UDP) where the browser has it, which carries small
//! messages as datagrams, unreliable and unordered, and larger ones (or
//! those asked to be reliable) on a stream; else a WebSocket, which carries
//! all of them reliably. The server listens for WebTransport on the UDP port
//! with the HTTP port's number (`--wt-port` for another), from when the
//! first page asks to, with a certificate of its own the page is told the
//! hash of (or `--tls-cert` and `--tls-key`'s).
//!
//! The connection is the hub's, in Rust; Quillo code meets it in two
//! places. What pages send waits in a queue per channel, and the server
//! is asked to run the channel's handler for it (`POST /_quillo/channel/<name>`,
//! with this process's job secret), a batch at a time, as UDP packets are
//! (see `udp`): in order, like a request, with `session()` the page's. What
//! the program sends goes into the page's queue, without waiting.
//!
//! A page joins a channel by asking, and its messages are taken once the
//! answer is yes: the channel's `join` function's, or yes when it has none.
//! Anyone who can open a page can ask, so a channel whose broadcasts are not
//! for everyone has a `join` that says who.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::time::Duration;

type R = Result<Value, Ctrl>;

/// Set by `--no-wt`: pages use a WebSocket.
pub static DISABLED: AtomicBool = AtomicBool::new(false);
/// Set by `--wt-port`; 0 for the HTTP port's number.
pub static PORT: AtomicU16 = AtomicU16::new(0);
/// Set by `--tls-cert` and `--tls-key`.
pub static TLS_CERT: OnceLock<std::path::PathBuf> = OnceLock::new();
pub static TLS_KEY: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Messages a page may have waiting for it; past that, a datagram is
/// dropped, and a page that cannot take a reliable message is let go (it
/// connects again).
const PEER_QUEUE: usize = 4096;
/// Messages from pages waiting for a channel's handler.
const INBOX: usize = 10_000;
/// The largest message, as JSON.
pub const MAX_MESSAGE: usize = 1 << 20;
/// How long a page's ticket to connect is good for.
const TICKET_SECS: u64 = 120;

/// Messages dropped (queues full), for `--metrics`.
pub static DROPPED: AtomicU64 = AtomicU64::new(0);

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("channel.on", on),
        ("channel.send", send),
        ("channel.broadcast", broadcast),
        ("channel.peers", peers),
        ("channel.open", open),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("channel.").into(), Value::Builtin(name, *f));
    }
}

/// What `channel.on` registered.
pub struct Handler {
    pub name: String,
    pub f: Value,
    pub join: Option<Value>,
    pub leave: Option<Value>,
}

pub fn name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- the hub ---------------------------------------------------------------

/// What goes to a page.
pub enum Out {
    /// `["m", channel, value]` and the like, as JSON.
    Text { json: String, reliable: bool },
    /// A frame of a stream it watches (see `media`), over a WebSocket.
    Media(Arc<crate::media::Frame>),
    Ping,
    Close,
}

pub struct Peer {
    pub id: u64,
    /// The page's session when it connected, as JSON, and its tenant.
    session: String,
    tenant: Option<String>,
    tx: tokio::sync::mpsc::Sender<Out>,
    /// Over WebTransport, frames go to a task of their own (streams of
    /// their own); over a WebSocket, with the rest.
    media_tx: Option<tokio::sync::mpsc::Sender<Arc<crate::media::Frame>>>,
    rooms: Mutex<HashSet<String>>,
}

/// A page, as a stream's viewer.
struct Viewer(Arc<Peer>);

impl crate::media::Sink for Viewer {
    fn take(&self, f: &Arc<crate::media::Frame>) -> bool {
        match &self.0.media_tx {
            Some(tx) => tx.try_send(f.clone()).is_ok(),
            None => self.0.put(Out::Media(f.clone())),
        }
    }

    fn id(&self) -> u64 {
        self.0.id
    }
}

impl Peer {
    fn id_text(&self) -> String {
        format!("{:012x}", self.id)
    }

    /// Queues a message; false when the page is gone or not keeping up.
    fn put(&self, out: Out) -> bool {
        let reliable = matches!(out, Out::Text { reliable: true, .. } | Out::Close);
        let frame = matches!(out, Out::Media(_));
        match self.tx.try_send(out) {
            Ok(()) => true,
            // A frame not taken makes its viewer skip to the next keyframe.
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) if frame => false,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                DROPPED.fetch_add(1, Relaxed);
                if reliable {
                    // It would miss a message it was promised: it is let go,
                    // and joins again as new.
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.blocking_send(Out::Close);
                    });
                }
                false
            }
            Err(_) => false,
        }
    }
}

#[derive(Default)]
struct Hub {
    peers: Mutex<HashMap<u64, Arc<Peer>>>,
    /// Who is in each channel.
    rooms: Mutex<HashMap<String, HashSet<u64>>>,
    inboxes: Mutex<HashMap<String, Arc<Inbox>>>,
}

fn hub() -> &'static Hub {
    static H: OnceLock<Hub> = OnceLock::new();
    H.get_or_init(Default::default)
}

/// A page's ticket: who it is, given out by `/_quillo/wt-ticket` and shown
/// when it connects.
#[derive(Clone)]
pub struct Ticket {
    session: String,
    tenant: Option<String>,
}

/// A connection from a page: registered with the hub, with the queue of
/// what goes to it.
pub fn connect(t: Ticket, media_tx: Option<tokio::sync::mpsc::Sender<Arc<crate::media::Frame>>>) -> (Arc<Peer>, tokio::sync::mpsc::Receiver<Out>) {
    let (tx, rx) = tokio::sync::mpsc::channel(PEER_QUEUE);
    let id = loop {
        let id = fastrand::u64(1..1 << 48);
        if !lock(&hub().peers).contains_key(&id) {
            break id;
        }
    };
    let p = Arc::new(Peer { id, session: t.session, tenant: t.tenant, tx, media_tx, rooms: Mutex::new(HashSet::new()) });
    lock(&hub().peers).insert(id, p.clone());
    (p, rx)
}

/// The page is gone: out of its channels, whose `leave` hears of it.
pub fn disconnect(p: &Arc<Peer>) {
    if lock(&hub().peers).remove(&p.id).is_none() {
        return;
    }
    crate::media::unwatch_all(p.id);
    let rooms: Vec<String> = lock(&p.rooms).drain().collect();
    for r in rooms {
        if let Some(members) = lock(&hub().rooms).get_mut(&r) {
            members.remove(&p.id);
        }
        inbox(&r).push(Event { kind: 'l', peer: p.clone(), value: String::new() });
    }
}

/// A message from a page: `["j", channel]` to join, `["l", channel]` to
/// leave, `["m", channel, value]` for the channel's handler; `["v", stream]`
/// to watch a stream (see `media`), `["u", stream]` to stop.
pub fn from_page(p: &Arc<Peer>, text: &str) {
    let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(text) else { return };
    let (Some(kind), Some(ch)) = (a.first().and_then(|k| k.as_str()), a.get(1).and_then(|c| c.as_str())) else { return };
    if !name_ok(ch) {
        return;
    }
    match kind {
        "v" => crate::media::watch(ch, Arc::new(Viewer(p.clone()))),
        "u" => crate::media::unwatch(ch, p.id),
        "j" if !lock(&p.rooms).contains(ch) => inbox(ch).push(Event { kind: 'j', peer: p.clone(), value: String::new() }),
        "l" => {
            if lock(&p.rooms).remove(ch) {
                if let Some(members) = lock(&hub().rooms).get_mut(ch) {
                    members.remove(&p.id);
                }
                inbox(ch).push(Event { kind: 'l', peer: p.clone(), value: String::new() });
            }
        }
        // Only from a page the channel took.
        "m" if lock(&p.rooms).contains(ch) => {
            let v = a.get(2).cloned().unwrap_or(serde_json::Value::Null);
            inbox(ch).push(Event { kind: 'm', peer: p.clone(), value: v.to_string() });
        }
        _ => {}
    }
}

/// The page took the channel's answer to its asking to join.
fn joined(ch: &str, p: &Arc<Peer>, yes: bool) {
    if yes && lock(&hub().peers).contains_key(&p.id) {
        lock(&p.rooms).insert(ch.to_string());
        lock(&hub().rooms).entry(ch.to_string()).or_default().insert(p.id);
    }
    p.put(Out::Text { json: serde_json::json!(["j", ch, yes]).to_string(), reliable: true });
}

fn message(ch: &str, v: &Value) -> String {
    format!("[\"m\",{},{}]", serde_json::Value::from(ch), v.json_text())
}

// ---- what pages send, to the program -----------------------------------------

struct Event {
    /// 'j'oin, 'l'eave or 'm'essage.
    kind: char,
    peer: Arc<Peer>,
    /// The message, as JSON.
    value: String,
}

struct Inbox {
    queue: Mutex<VecDeque<Event>>,
    arrived: Condvar,
    delivering: AtomicBool,
}

impl Inbox {
    fn push(self: &Arc<Self>, e: Event) {
        let mut q = lock(&self.queue);
        if q.len() >= INBOX {
            // Joining and leaving are kept; a message is not.
            match q.iter().position(|e| e.kind == 'm') {
                Some(i) => {
                    q.remove(i);
                }
                None if e.kind == 'm' => {
                    DROPPED.fetch_add(1, Relaxed);
                    return;
                }
                None => {}
            }
            DROPPED.fetch_add(1, Relaxed);
        }
        q.push_back(e);
        drop(q);
        self.arrived.notify_one();
    }
}

fn inbox(ch: &str) -> Arc<Inbox> {
    let mut all = lock(&hub().inboxes);
    if let Some(i) = all.get(ch) {
        return i.clone();
    }
    let i = Arc::new(Inbox { queue: Mutex::new(VecDeque::new()), arrived: Condvar::new(), delivering: AtomicBool::new(false) });
    all.insert(ch.to_string(), i.clone());
    if let Some(addr) = ADDR.get() {
        start_inbox(*addr, ch.to_string(), i.clone());
    }
    i
}

/// Where the server listens (its HTTP address).
static ADDR: OnceLock<SocketAddr> = OnceLock::new();

/// The server listens at `addr`: messages already waiting go to it.
pub fn serving(addr: SocketAddr) {
    if ADDR.set(crate::jobs::loopback(addr)).is_err() {
        return;
    }
    let all: Vec<(String, Arc<Inbox>)> = lock(&hub().inboxes).iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (ch, i) in all {
        start_inbox(addr, ch, i);
    }
    let _ = HTTP_PORT.set(addr.port());
}

static HTTP_PORT: OnceLock<u16> = OnceLock::new();

fn start_inbox(addr: SocketAddr, ch: String, i: Arc<Inbox>) {
    if i.delivering.swap(true, Relaxed) {
        return;
    }
    let addr = crate::jobs::loopback(addr);
    let _ = std::thread::Builder::new().name(format!("quillo-ch-{}", ch)).spawn(move || deliver_loop(addr, ch, i));
}

/// Hands a channel's messages to the program, a batch at a time, and
/// carries out its answers to pages asking to join.
fn deliver_loop(addr: SocketAddr, ch: String, i: Arc<Inbox>) {
    let path = format!("/_quillo/channel/{}", ch);
    let mut conn: Option<crate::udp::Loopback> = None;
    loop {
        let batch: Vec<Event> = {
            let mut q = lock(&i.queue);
            while q.is_empty() {
                if crate::httpd::stopping() {
                    return;
                }
                q = i.arrived.wait_timeout(q, Duration::from_millis(500)).unwrap_or_else(|e| e.into_inner()).0;
            }
            let (mut n, mut bytes) = (0, 0);
            while n < q.len() && n < 1024 && (n == 0 || bytes + q[n].value.len() <= 1 << 20) {
                bytes += q[n].value.len();
                n += 1;
            }
            q.drain(..n).collect()
        };
        let mut body = String::new();
        for e in &batch {
            let line = serde_json::json!({
                "k": e.kind.to_string(),
                "p": e.peer.id_text(),
                "s": e.peer.session,
                "t": e.peer.tenant,
                "v": e.value,
            });
            body.push_str(&line.to_string());
            body.push('\n');
        }
        let mut answer: Option<Vec<u8>> = None;
        for _ in 0..50 {
            let c = match conn.take() {
                Some(c) => c,
                None => match crate::udp::Loopback::connect(addr) {
                    Ok(c) => c,
                    Err(_) => {
                        if crate::httpd::stopping() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                },
            };
            match c.post_for(&path, body.as_bytes()) {
                Ok((c, _, out)) => {
                    conn = Some(c);
                    answer = Some(out);
                    break;
                }
                Err(_) if crate::httpd::stopping() => return,
                Err(_) => {}
            }
        }
        // Who was let in: the answer lists, in order, a yes or no for each
        // asking to join. Without an answer, no one was.
        let verdicts: Vec<bool> = answer.and_then(|a| serde_json::from_slice(&a).ok()).unwrap_or_default();
        let mut v = verdicts.into_iter();
        for e in batch.iter().filter(|e| e.kind == 'j') {
            joined(&ch, &e.peer, v.next().unwrap_or(false));
        }
    }
}

/// `POST /_quillo/channel/<name>`: a batch of what pages did, each given to
/// the channel's functions in turn. The answer is the joins' verdicts.
pub fn answer(it: &mut Interp, name: &str, body: &[u8]) -> crate::server::HttpResponse {
    let handler = find(it, name);
    let mut verdicts: Vec<bool> = Vec::new();
    let text = String::from_utf8_lossy(body);
    for line in text.lines() {
        let Ok(e) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let kind = e["k"].as_str().unwrap_or("");
        let Some((h, sub)) = handler.as_ref() else {
            // A channel no `channel.on` names: pages may join it (to hear
            // the program's broadcasts), and what they send goes nowhere.
            if kind == "j" {
                verdicts.push(true);
            }
            continue;
        };
        let it: &mut Interp = match sub {
            Some(path) => match mounted(&mut *it, path) {
                Some(s) => s,
                None => {
                    if kind == "j" {
                        verdicts.push(false);
                    }
                    continue;
                }
            },
            None => &mut *it,
        };
        let session = serde_json::from_str::<serde_json::Value>(e["s"].as_str().unwrap_or("{}")).map(|j| Value::from_json(&j)).unwrap_or(Value::Nil);
        let session = if matches!(session, Value::Map(_)) { session } else { Value::map(Map::new()) };
        let mut peer = Map::new();
        peer.insert("id".into(), Value::str(e["p"].as_str().unwrap_or("")));
        peer.insert("session".into(), session.clone());
        let peer = Value::map(peer);
        // As if the page asked: its session and tenant.
        let saved = (it.session.replace(session), it.tenant.replace(e["t"].as_str().map(Arc::from)), it.tenant_named);
        it.tenant_named = e["t"].is_string();
        let res = match kind {
            "j" => match &h.join {
                Some(f) => {
                    let r = it.call(f, args_for(f, vec![peer]), 0);
                    let yes = matches!(&r, Ok(v) | Err(Ctrl::Return(v)) if v.truthy());
                    verdicts.push(yes);
                    r.map(|_| Value::Nil)
                }
                None => {
                    verdicts.push(true);
                    Ok(Value::Nil)
                }
            },
            "l" => match &h.leave {
                Some(f) => it.call(f, args_for(f, vec![peer]), 0),
                None => Ok(Value::Nil),
            },
            "m" => {
                let v = serde_json::from_str::<serde_json::Value>(e["v"].as_str().unwrap_or("null")).map(|j| Value::from_json(&j)).unwrap_or(Value::Nil);
                it.call(&h.f, args_for(&h.f, vec![v, peer]), 0)
            }
            _ => Ok(Value::Nil),
        };
        (it.session, it.tenant, it.tenant_named) = saved;
        if let Err(Ctrl::Error(e)) = res {
            eprintln!("quillo: error in channel {}: {}", h.name, e);
        }
    }
    crate::server::HttpResponse {
        status: 200,
        headers: vec![("content-type".into(), "application/json".into())],
        body: serde_json::to_vec(&verdicts).unwrap_or_default(),
    }
}

/// As many of `args` as `f` takes.
fn args_for(f: &Value, mut args: Vec<Value>) -> Vec<Value> {
    if let Value::Func(c) = f {
        args.truncate(c.def.params.len());
    }
    args
}

/// The channel's handler: in the program, or in one it mounts (with the
/// mounts' indexes to reach it).
fn find(it: &Interp, name: &str) -> Option<(Arc<Handler>, Option<Vec<usize>>)> {
    // `media.on("call:*")` handles every `media:call:...` (a channel's own
    // name cannot hold a `*`).
    let matches = |h: &Handler| h.name == name || h.name.strip_suffix('*').is_some_and(|p| name.starts_with(p));
    if let Some(h) = it.site.channels.iter().find(|h| matches(h)) {
        return Some((h.clone(), None));
    }
    for (i, (_, sub)) in it.mounts.iter().enumerate() {
        if let Some((h, path)) = find(sub, name) {
            let mut p = vec![i];
            p.extend(path.unwrap_or_default());
            return Some((h, Some(p)));
        }
    }
    None
}

fn mounted<'a>(it: &'a mut Interp, path: &[usize]) -> Option<&'a mut Interp> {
    let mut at = it;
    for &i in path {
        at = &mut at.mounts.get_mut(i)?.1;
    }
    Some(at)
}

// ---- builtins ----------------------------------------------------------------

fn name_arg(args: &[Value], f: &str, line: u32) -> Result<String, Ctrl> {
    match args.first() {
        Some(Value::Str(s)) if name_ok(s) => Ok(s.to_string()),
        Some(Value::Str(s)) => fail(line, format!("`{}`: a channel's name is letters, digits, `_`, `-`, `.` and `:`, up to 64, got {:?}", f, &**s)).map(|_: Value| String::new()),
        other => fail(line, format!("`{}` takes a channel's name first, got {}", f, other.map_or("nothing", |v| v.type_name()))).map(|_: Value| String::new()),
    }
}

/// `channel.on(name, f, { join, leave })`, at the top of a program.
fn on(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = name_arg(&args, "channel.on", line)?;
    let f = match args.get(1) {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        other => return fail(line, format!("`channel.on` needs a function to call with each message, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let (mut join, mut leave) = (None, None);
    match args.get(2) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(o)) => {
            for (k, v) in o.iter() {
                match (k.as_str(), v) {
                    ("join", f @ (Value::Func(_) | Value::Builtin(..))) => join = Some(f.clone()),
                    ("leave", f @ (Value::Func(_) | Value::Builtin(..))) => leave = Some(f.clone()),
                    ("join" | "leave", _) => return fail(line, format!("`channel.on`: `{}` is a function of the page", k.as_str())),
                    _ => return fail(line, format!("`channel.on` has no option `{}` (it takes join and leave)", k.as_str())),
                }
            }
        }
        Some(v) => return fail(line, format!("`channel.on` expects options as a map, got {}", v.type_name())),
    }
    let site = Arc::make_mut(&mut it.site);
    if site.channels.iter().any(|h| h.name == name) {
        return fail(line, format!("`channel.on`: the program already handles channel {:?}", name));
    }
    site.channels.push(Arc::new(Handler { name, f, join, leave }));
    Ok(Value::Nil)
}

/// The peer a value names: a peer map, or its id.
fn peer_of(v: Option<&Value>) -> Option<u64> {
    let id = match v? {
        Value::Map(m) => match m.get("id") {
            Some(Value::Str(s)) => s.clone(),
            _ => return None,
        },
        Value::Str(s) => s.clone(),
        _ => return None,
    };
    u64::from_str_radix(&id, 16).ok()
}

fn reliable_opt(o: Option<&Value>, f: &str, line: u32) -> Result<(bool, Option<u64>), Ctrl> {
    let mut out = (false, None);
    match o {
        None | Some(Value::Nil) => {}
        Some(Value::Map(o)) => {
            for (k, v) in o.iter() {
                match k.as_str() {
                    "reliable" => out.0 = v.truthy(),
                    "except" if f == "channel.broadcast" => out.1 = peer_of(Some(v)),
                    _ => return fail(line, format!("`{}` has no option `{}`", f, k.as_str())).map(|_: Value| out),
                }
            }
        }
        Some(v) => return fail(line, format!("`{}` expects options as a map, got {}", f, v.type_name())).map(|_: Value| out),
    }
    Ok(out)
}

fn too_big(json: &str, f: &str, line: u32) -> Result<(), Ctrl> {
    if json.len() > MAX_MESSAGE {
        return fail(line, format!("`{}`: the message is {} bytes as JSON; at most {} go to a page", f, json.len(), MAX_MESSAGE)).map(|_: Value| ());
    }
    Ok(())
}

/// `channel.send(name, peer, value, { reliable })`: to one page in the
/// channel; false when it is not (any more) there.
fn send(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let _ = it;
    let name = name_arg(&args, "channel.send", line)?;
    let Some(id) = peer_of(args.get(1)) else {
        return fail(line, "`channel.send` takes a peer (as the channel's function gets it, or its id) second".to_string());
    };
    let v = args.get(2).cloned().unwrap_or(Value::Nil);
    let (reliable, _) = reliable_opt(args.get(3), "channel.send", line)?;
    let json = message(&name, &v);
    too_big(&json, "channel.send", line)?;
    let p = lock(&hub().peers).get(&id).cloned();
    Ok(Value::Bool(match p {
        Some(p) if lock(&p.rooms).contains(&name) => p.put(Out::Text { json, reliable }),
        _ => false,
    }))
}

/// `channel.broadcast(name, value, { except, reliable })`: to every page in
/// the channel; how many it went to.
fn broadcast(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = name_arg(&args, "channel.broadcast", line)?;
    let v = args.get(1).cloned().unwrap_or(Value::Nil);
    let (reliable, except) = reliable_opt(args.get(2), "channel.broadcast", line)?;
    let json = message(&name, &v);
    too_big(&json, "channel.broadcast", line)?;
    let ids: Vec<u64> = lock(&hub().rooms).get(&name).map(|m| m.iter().copied().collect()).unwrap_or_default();
    let peers: Vec<Arc<Peer>> = {
        let all = lock(&hub().peers);
        ids.iter().filter(|id| Some(**id) != except).filter_map(|id| all.get(id).cloned()).collect()
    };
    let mut n = 0;
    for p in peers {
        if p.put(Out::Text { json: json.clone(), reliable }) {
            n += 1;
        }
    }
    Ok(Value::Int(n))
}

/// `channel.peers(name)`: the ids of the pages in it.
fn peers(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = name_arg(&args, "channel.peers", line)?;
    let mut ids: Vec<u64> = lock(&hub().rooms).get(&name).map(|m| m.iter().copied().collect()).unwrap_or_default();
    ids.sort();
    Ok(Value::list(ids.into_iter().map(|id| Value::str(format!("{:012x}", id))).collect()))
}

/// `channel.open(name, f)` is the browser's (src/client/runtime.js); while a
/// page is rendered on the server it opens nothing.
fn open(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    name_arg(&args, "channel.open", line)?;
    fn nothing(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
        Ok(Value::Bool(false))
    }
    let mut m = Map::new();
    m.insert("send".into(), Value::Builtin("channel.open.send", nothing));
    Ok(Value::map(m))
}

// ---- tickets ------------------------------------------------------------------

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `POST /_quillo/wt-ticket`, from the program's own page: what it needs to
/// connect (a ticket, and where WebTransport listens, with the hash of its
/// certificate when it has one of its own).
pub fn ticket(it: &mut Interp) -> crate::server::HttpResponse {
    let session = crate::session::current(it).to_json().to_string();
    let tenant = it.tenant().map(|t| t.to_string());
    let sealed = crate::session::seal(
        "quillo channel ticket",
        &serde_json::json!({ "s": session, "t": tenant, "e": now() + TICKET_SECS }).to_string(),
    );
    let mut out = serde_json::json!({ "ticket": sealed });
    if let Some((port, hash)) = endpoint() {
        out["port"] = port.into();
        if let Some(h) = hash {
            out["hash"] = h.into();
        }
    }
    let mut r = crate::server::HttpResponse {
        status: 200,
        headers: vec![("content-type".into(), "application/json".into())],
        body: out.to_string().into_bytes(),
    };
    r.headers.push(("cache-control".into(), "no-store".into()));
    r
}

/// The ticket in a connection's `?t=`, if it is good.
pub fn ticket_of(query: &str) -> Option<Ticket> {
    let t = query.split('&').find_map(|kv| kv.strip_prefix("t="))?;
    let t = crate::server::percent_decode(t, false);
    let j: serde_json::Value = serde_json::from_str(&crate::session::unseal("quillo channel ticket", &t)?).ok()?;
    if j["e"].as_u64()? < now() {
        return None;
    }
    Some(Ticket { session: j["s"].as_str()?.to_string(), tenant: j["t"].as_str().map(str::to_string) })
}

// ---- WebTransport ----------------------------------------------------------------

/// Where WebTransport listens and the hash of its certificate (none for one
/// given with `--tls-cert`), started the first time a page asks.
fn endpoint() -> Option<(u16, Option<String>)> {
    #[cfg(feature = "realtime")]
    {
        quic::endpoint()
    }
    #[cfg(not(feature = "realtime"))]
    {
        None
    }
}

#[cfg(feature = "realtime")]
mod quic {
    use super::*;
    use wtransport::{Endpoint, Identity, ServerConfig};

    struct Running {
        port: u16,
        hash: RwLock<Option<String>>,
    }

    static RUNNING: OnceLock<Option<Running>> = OnceLock::new();

    pub fn endpoint() -> Option<(u16, Option<String>)> {
        if DISABLED.load(Relaxed) {
            return None;
        }
        let http = *HTTP_PORT.get()?;
        let r = RUNNING.get_or_init(|| match start(http) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("quillo: WebTransport is off ({}); pages use a WebSocket", e);
                None
            }
        });
        r.as_ref().map(|r| (r.port, r.hash.read().unwrap_or_else(|e| e.into_inner()).clone()))
    }

    fn hex_hash(id: &Identity) -> Option<String> {
        let cert = id.certificate_chain().as_slice().first()?;
        Some(cert.hash().fmt(wtransport::tls::Sha256DigestFmt::DottedHex).replace(':', ""))
    }

    /// Ours, for browsers to take by its hash: such a certificate may be
    /// good for two weeks at most.
    fn own_identity() -> Result<Identity, String> {
        Identity::self_signed_builder()
            .subject_alt_names(["localhost", "127.0.0.1", "::1"])
            .from_now_utc()
            .validity_days(13)
            .build()
            .map_err(|e| e.to_string())
    }

    fn config(addr: SocketAddr, id: Identity) -> ServerConfig {
        ServerConfig::builder()
            .with_bind_address(addr)
            .with_identity(id)
            .keep_alive_interval(Some(Duration::from_secs(5)))
            .max_idle_timeout(Some(Duration::from_secs(20)))
            .map(|b| b.build())
            .unwrap_or_else(|_| unreachable!())
    }

    fn start(http: u16) -> Result<Running, String> {
        let rt = crate::pg::runtime();
        let port = match PORT.load(Relaxed) {
            0 => http,
            p => p,
        };
        let ip = ADDR.get().map(|a| a.ip()).unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
        // Listening on every address: on every IPv6 one too (with IPv4).
        let ip = if ip.is_loopback() { ip } else { std::net::IpAddr::from(std::net::Ipv6Addr::UNSPECIFIED) };
        let addr = SocketAddr::new(ip, port);
        let (id, own) = match TLS_CERT.get().zip(TLS_KEY.get()) {
            Some((cert, key)) => (rt.block_on(Identity::load_pemfiles(cert, key)).map_err(|e| format!("cannot read the certificate: {}", e))?, false),
            None => (own_identity()?, true),
        };
        let hash = if own { hex_hash(&id) } else { None };
        let _in_rt = rt.enter();
        let server = Endpoint::server(config(addr, id)).map_err(|e| format!("cannot listen on UDP port {}: {}", port, e))?;
        let server = Arc::new(server);
        rt.spawn({
            let server = server.clone();
            async move {
                loop {
                    let incoming = server.accept().await;
                    tokio::spawn(session(incoming));
                }
            }
        });
        if own {
            // A new certificate before this one ends; pages connecting after
            // are told the new hash, and those connected stay.
            let server = server.clone();
            rt.spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(11 * 86400)).await;
                    let Ok(id) = own_identity() else { continue };
                    let hash = hex_hash(&id);
                    if server.reload_config(config(addr, id), false).is_ok() {
                        if let Some(Some(r)) = RUNNING.get() {
                            *r.hash.write().unwrap_or_else(|e| e.into_inner()) = hash;
                        }
                    }
                }
            });
        }
        eprintln!("quillo: WebTransport on UDP port {}", port);
        Ok(Running { port, hash: RwLock::new(hash) })
    }

    async fn session(incoming: wtransport::endpoint::IncomingSession) {
        let Ok(req) = incoming.await else { return };
        let path = req.path().to_string();
        let ticket = path.split_once('?').filter(|(p, _)| p.ends_with("/_quillo/wt")).and_then(|(_, q)| ticket_of(q));
        let Some(ticket) = ticket else {
            req.forbidden().await;
            return;
        };
        let Ok(conn) = req.accept().await else { return };
        // The page's first stream carries what must arrive (joining, and
        // large or reliable messages), both ways.
        let Ok(Ok(opening)) = tokio::time::timeout(Duration::from_secs(10), conn.accept_bi()).await else { return };
        let (mut out, mut inn) = opening;
        let (media_tx, media_rx) = tokio::sync::mpsc::channel(240);
        let (peer, mut rx) = connect(ticket, Some(media_tx));
        let frames = tokio::spawn(send_frames(conn.clone(), media_rx));
        let reader = {
            let peer = peer.clone();
            tokio::spawn(async move {
                let mut len = [0u8; 4];
                loop {
                    if inn.read_exact(&mut len).await.is_err() {
                        return;
                    }
                    let n = u32::from_be_bytes(len) as usize;
                    if n > MAX_MESSAGE {
                        return;
                    }
                    let mut buf = vec![0u8; n];
                    if inn.read_exact(&mut buf).await.is_err() {
                        return;
                    }
                    if let Ok(text) = std::str::from_utf8(&buf) {
                        from_page(&peer, text);
                    }
                }
            })
        };
        tokio::pin!(reader);
        // Frames the page sends (`<quillo-media publish>`): a stream of its
        // own per track, each frame after its length.
        let sending = {
            let (conn, peer) = (conn.clone(), peer.clone());
            tokio::spawn(async move {
                while let Ok(opening) = conn.accept_uni().await {
                    let peer = peer.clone();
                    tokio::spawn(async move {
                        let mut inn = opening;
                        let mut len = [0u8; 4];
                        loop {
                            if inn.read_exact(&mut len).await.is_err() {
                                return;
                            }
                            let n = u32::from_be_bytes(len) as usize;
                            if n > MAX_MESSAGE {
                                return;
                            }
                            let mut buf = vec![0u8; n];
                            if inn.read_exact(&mut buf).await.is_err() {
                                return;
                            }
                            frame_from_page(&peer, buf);
                        }
                    });
                }
            })
        };
        loop {
            tokio::select! {
                d = conn.receive_datagram() => match d {
                    Ok(d) => if let Ok(text) = std::str::from_utf8(&d.payload()) { from_page(&peer, text) },
                    Err(_) => break,
                },
                o = rx.recv() => match o {
                    Some(Out::Text { json, reliable }) => {
                        let fits = conn.max_datagram_size().map_or(false, |m| json.len() <= m);
                        if !reliable && fits {
                            let _ = conn.send_datagram(json.into_bytes());
                        } else {
                            let mut framed = Vec::with_capacity(json.len() + 4);
                            framed.extend_from_slice(&(json.len() as u32).to_be_bytes());
                            framed.extend_from_slice(json.as_bytes());
                            if out.write_all(&framed).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Out::Ping | Out::Media(_)) => {}
                    Some(Out::Close) | None => break,
                },
                _ = &mut reader => break,
            }
        }
        disconnect(&peer);
        frames.abort();
        sending.abort();
        conn.close(0u32.into(), b"");
    }

    /// A page's frames: each track on a stream of its own (opened with its
    /// first frame), each frame after its length.
    async fn send_frames(conn: wtransport::Connection, mut rx: tokio::sync::mpsc::Receiver<Arc<crate::media::Frame>>) {
        let mut streams: HashMap<Vec<u8>, wtransport::SendStream> = HashMap::new();
        while let Some(f) = rx.recv().await {
            // The stream's name and the frame's kind say which track.
            let key = f.wire[..f.wire[0] as usize + 2].to_vec();
            if !streams.contains_key(&key) {
                let Ok(opening) = conn.open_uni().await else { return };
                let Ok(s) = opening.await else { return };
                streams.insert(key.clone(), s);
            }
            let Some(s) = streams.get_mut(&key) else { return };
            let mut head = (f.wire.len() as u32).to_be_bytes().to_vec();
            head.extend_from_slice(&f.wire);
            if s.write_all(&head).await.is_err() {
                return;
            }
        }
    }
}

// ---- a WebSocket, where WebTransport cannot be had ----------------------------------

/// A page's connection over a WebSocket, on the waiting thread that owns
/// it: what the page sends is read on a second thread.
pub fn over_websocket(mut socket: crate::ws::Socket, ticket: Ticket) {
    let (peer, mut rx) = connect(ticket, None);
    let Ok(mut reader) = socket.reader(MAX_MESSAGE + 64) else {
        disconnect(&peer);
        return;
    };
    let reading = {
        let peer = peer.clone();
        std::thread::Builder::new().name("quillo-ch-ws".into()).stack_size(256 << 10).spawn(move || {
            loop {
                match reader.next() {
                    Ok(crate::ws::Message::Text(t)) => from_page(&peer, &t),
                    Ok(crate::ws::Message::Binary(b)) => frame_from_page(&peer, b),
                    Ok(crate::ws::Message::Close) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            peer.put(Out::Close);
        })
    };
    if reading.is_err() {
        disconnect(&peer);
        return;
    }
    // Something every 20 seconds, so a page that went away is noticed.
    let ticker = peer.tx.clone();
    let _ = std::thread::Builder::new().name("quillo-ch-ping".into()).stack_size(64 << 10).spawn(move || loop {
        std::thread::sleep(Duration::from_secs(20));
        if ticker.blocking_send(Out::Ping).is_err() {
            return;
        }
    });
    while let Some(o) = rx.blocking_recv() {
        let alive = match o {
            Out::Text { json, .. } => socket.send(&json),
            Out::Media(f) => socket.send_binary(&f.wire),
            Out::Ping => socket.ping(),
            Out::Close => break,
        };
        if !alive {
            break;
        }
    }
    disconnect(&peer);
    socket.close();
}

/// A frame a page sends to a stream (`<quillo-media publish>`): taken only
/// from a page that joined the stream's channel, `media:<name>` (FEEDBACK6.md,
/// Q-060; branch feedback6-fixes).
pub fn frame_from_page(p: &Arc<Peer>, wire: Vec<u8>) {
    let Some(name) = crate::media::frame_stream(&wire) else { return };
    if !lock(&p.rooms).contains(&format!("media:{}", name)) {
        return;
    }
    crate::media::from_page(wire);
}

/// Every page leaves `channel`, as if it had not joined: what `media.close`
/// does to the pages sending a stream.
pub fn leave_all(channel: &str) {
    let members: Vec<u64> = lock(&hub().rooms).remove(channel).map(|m| m.into_iter().collect()).unwrap_or_default();
    let peers = lock(&hub().peers);
    for id in members {
        if let Some(p) = peers.get(&id) {
            lock(&p.rooms).remove(channel);
            p.put(Out::Text { json: serde_json::json!(["j", channel, false]).to_string(), reliable: true });
        }
    }
}

/// Pages connected now, for `--metrics`.
pub fn connected() -> usize {
    lock(&hub().peers).len()
}
