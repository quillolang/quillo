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
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
// Only the WebTransport server keeps its certificate's hash behind one (built without it on phones; branch mobile).
#[cfg(feature = "realtime")]
use std::sync::RwLock;
use std::time::Duration;

type R = Result<Value, Ctrl>;

/// Set by `--no-wt`: pages use a WebSocket.
pub static DISABLED: AtomicBool = AtomicBool::new(false);
/// Set by `--wt-port`; 0 for the HTTP port's number.
pub static PORT: AtomicU16 = AtomicU16::new(0);
/// Set by `--tls-cert` and `--tls-key`.
pub static TLS_CERT: OnceLock<std::path::PathBuf> = OnceLock::new();
pub static TLS_KEY: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Why a certificate and a key that both read are no pair, if they are not: said as the server
/// starts, where WebTransport presented the one with the other's key and no page could connect
/// (`--tls-cert a.pem --tls-key a.key --tls-cert b.pem`: FEEDBACK55.md, Q-852; branch feedback55).
/// Files that do not read as PEM are left to WebTransport, which is then off and says why.
pub fn tls_pair_problem(cert: &std::path::Path, key: &std::path::Path) -> Option<String> {
    use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert).ok()?.collect::<Result<_, _>>().ok()?;
    let private = PrivateKeyDer::from_pem_file(key).ok()?;
    if chain.is_empty() {
        return None;
    }
    match rustls::sign::CertifiedKey::from_der(chain, private, &rustls::crypto::ring::default_provider()) {
        Err(rustls::Error::InconsistentKeys(_)) => Some(format!("--tls-cert {} and --tls-key {} are no pair: the key is not the certificate's", cert.display(), key.display())),
        _ => None,
    }
}

/// Messages a page may have waiting for it; past that, a datagram is
/// dropped, and a page that cannot take a reliable message is let go (it
/// connects again).
const PEER_QUEUE: usize = 4096;

/// How long a write to a page may wait on a page that does not read (FEEDBACK49.md, Q-594).
const WRITE_WAIT: Duration = Duration::from_secs(30);
/// Messages from pages waiting for a channel's handler.
const INBOX: usize = 10_000;
/// The largest message, as JSON.
pub const MAX_MESSAGE: usize = 1 << 20;
/// How long a page's ticket to connect is good for.
const TICKET_SECS: u64 = 120;

/// Messages dropped (queues full), for `--metrics`.
pub static DROPPED: AtomicU64 = AtomicU64::new(0);

/// The most bytes of one page's messages that may wait for their channels' handlers, and of every
/// page's together (or what `--max-memory` lets one value take, where that is less). Inboxes
/// counted messages only, 10,000 of up to 1 MiB each, so one page on one socket could have the
/// server hold gigabytes for a handler that waits on a database (FEEDBACK75.md, Q-1056; branch
/// feedback75).
const PAGE_WAITING: usize = 4 << 20;
const ALL_WAITING: usize = 256 << 20;

/// What waits in every inbox now, in all. Each page's share is its own counter (`Peer::waiting`),
/// each client's one its pages share (`Peer::client_waiting`), and each channel's its inbox's
/// (`Inbox::waiting`): one slow channel, or the pages of one address, took all the room, and every
/// other channel's messages were dropped (found in review of FEEDBACK75.md, Q-1056). A client may
/// have an eighth of all waiting, a channel a quarter (each at least a page's share). All of them
/// atomics: no lock and nothing made for a message.
static WAITING_ALL: AtomicUsize = AtomicUsize::new(0);

/// Each client's counter of what its pages have waiting, found once as a page connects.
static CLIENT_WAITING: Mutex<Option<HashMap<Arc<str>, std::sync::Weak<AtomicUsize>>>> = Mutex::new(None);

/// The counter of `client`'s pages, made with its first.
fn client_counter(client: &str) -> Arc<AtomicUsize> {
    let mut g = lock(&CLIENT_WAITING);
    let m = g.get_or_insert_with(Default::default);
    if let Some(c) = m.get(client).and_then(|w| w.upgrade()) {
        return c;
    }
    if m.len() >= 1024 {
        m.retain(|_, w| w.strong_count() > 0);
    }
    let c = Arc::new(AtomicUsize::new(0));
    m.insert(Arc::from(client), Arc::downgrade(&c));
    c
}

/// Pages waiting for room in an inbox (see `wait_for_room`), told when some is given back.
fn room() -> &'static tokio::sync::Notify {
    static ROOM: OnceLock<tokio::sync::Notify> = OnceLock::new();
    ROOM.get_or_init(tokio::sync::Notify::new)
}

/// How many pages wait for room: none, and nothing is told.
static ROOM_WAITERS: AtomicUsize = AtomicUsize::new(0);

/// Room was given back: the pages waiting for it try again.
fn room_freed() {
    if ROOM_WAITERS.load(Relaxed) > 0 {
        room().notify_waiters();
    }
}

/// What every inbox may hold together.
fn all_waiting() -> usize {
    match crate::httpd::memory_limit() {
        0 => ALL_WAITING,
        m => ALL_WAITING.min(m.min(usize::MAX as u64) as usize),
    }
}

/// How long a message a page has begun sending may take to come whole, and what all the messages
/// begun and not finished may hold, on every channel connection (WebSocket and WebTransport), or
/// what `--max-memory` lets one value take where that is less, but never less than two of the
/// largest: a page that sent most of a message and never its end held up to 1 MiB for good, on
/// each of 4,096 sockets (FEEDBACK75.md, Q-1073 and Q-1074; branch feedback75).
const MESSAGE_WITHIN: Duration = Duration::from_secs(10);
const ALL_BEGUN: usize = 64 << 20;
static BEGUN_BYTES: AtomicUsize = AtomicUsize::new(0);

fn all_begun() -> usize {
    let floor = 2 * (MAX_MESSAGE + 64);
    match crate::httpd::memory_limit() {
        0 => ALL_BEGUN,
        m => ALL_BEGUN.min(m.min(usize::MAX as u64) as usize).max(floor),
    }
}

/// What each client holds of it (see `ws::Hold::share`).
static BEGUN_CLIENTS: Mutex<Option<HashMap<Arc<str>, usize>>> = Mutex::new(None);

/// The bytes a second a message begun keeps its time with, past `MESSAGE_WITHIN` (see
/// `ws::Hold::by`): 16 KiB, so a page sending 1 MiB at 80 KB/s finishes, and one that stops is let
/// go of within 10 s of what it sent last (found in review of FEEDBACK75.md, Q-1073).
const MESSAGE_RATE: usize = 16 << 10;

/// The hold of every channel connection's unfinished messages (see `ws::Hold`).
static BEGUN: crate::ws::Hold =
    crate::ws::Hold { within: MESSAGE_WITHIN, rate: MESSAGE_RATE, held: &BEGUN_BYTES, most: all_begun, clients: &BEGUN_CLIENTS, least_share: MAX_MESSAGE + 64 };

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
    /// For a stream's channel (`media.on`): who may watch it.
    pub watch: Option<Value>,
}

// ---- rooms, by tenant ---------------------------------------------------------------

/// Channels (and streams, `media:<name>`) every tenant shares: `{ tenants: "shared" }`. A name
/// ending in `*` stands for every name it starts.
static SHARED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// `name` is shared by every tenant.
pub fn share(name: &str) {
    let mut all = lock(&SHARED);
    if !all.iter().any(|n| n == name) {
        all.push(name.to_string());
    }
}

fn shared(ch: &str) -> bool {
    lock(&SHARED).iter().any(|n| n == ch || n.strip_suffix('*').is_some_and(|p| ch.starts_with(p)))
}

/// Whether a page is in the room `key`.
pub fn in_room(key: &str) -> bool {
    lock(&hub().rooms).get(key).is_some_and(|m| !m.is_empty())
}

/// Between a tenant and a channel's name in a room's key: no name holds it.
const SEP: char = '\u{1f}';

/// The room a channel is for a tenant: each tenant's own, unless the channel is shared or there
/// is no tenant. Rooms were keyed by name, so a user on tenant `evil` heard tenant `acme`'s
/// `lobby` and wrote into it (FEEDBACK51.md, Q-644; branch feedback51-fixes).
pub fn room_key(ch: &str, tenant: Option<&str>) -> String {
    match tenant {
        Some(t) if !shared(ch) => format!("{}{}{}", t, SEP, ch),
        _ => ch.to_string(),
    }
}

/// The tenant in a room's key, if it has one.
pub fn key_tenant(key: &str) -> Option<&str> {
    key.split_once(SEP).map(|(t, _)| t)
}

/// The channel's name in a room's key.
pub fn key_channel(key: &str) -> &str {
    key.rsplit(SEP).next().unwrap_or(key)
}

/// Whose rooms the running code reaches. `None`, every tenant's: code run for no page and no
/// request (a job, a tick, the top level). Otherwise that tenant's own, and with no tenant the
/// rooms of none: a page on the bare domain, with no tenant, was taken for the program and
/// reached every tenant's (found in review of FEEDBACK51.md, Q-644; branch feedback51-fixes).
pub fn reach(it: &mut Interp) -> Option<Option<String>> {
    // `answering` as well: a stream's function or a deferred part runs after the route, with
    // the request no longer set (found in review).
    // A call on stdio has a `request` of its own (FEEDBACK63.md, Q-927) and reaches what it
    // reached without one (found in review).
    let for_someone = it.tenant_named || FOR_PAGE.with(|f| f.get()) || it.answering || (it.current_request.is_some() && !it.stdio_request) || it.pending_request.is_some();
    for_someone.then(|| it.tenant().map(|t| t.to_string()))
}

thread_local! {
    /// Set while a channel's function runs for what a page did (see `answer`).
    static FOR_PAGE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The rooms of `ch` that code reaching `reach` (see `reach`) reaches.
fn reaches(ch: &str, reach: Option<Option<&str>>) -> impl Fn(&str) -> bool {
    let own = reach.map(|t| room_key(ch, t));
    let ch = ch.to_string();
    move |key: &str| match &own {
        Some(k) => key == k,
        None => key_channel(key) == ch,
    }
}

/// What a relayed op says it reaches: `"all"`, or the tenant `"t"`, or none.
fn reach_of(op: &serde_json::Value) -> Option<Option<&str>> {
    (op["all"] != true).then(|| op["t"].as_str())
}

pub fn name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---- the hub ---------------------------------------------------------------

/// What goes to a page.
pub enum Out {
    /// `["m", channel, value]` and the like, as JSON: one text for all the pages a message goes
    /// to, where each had a copy, a burst to 256 pages 256 copies of each message (found in review
    /// of FEEDBACK55.md, Q-806; branch feedback55).
    Text { json: Arc<str>, reliable: bool },
    /// A frame of a stream it watches (see `media`), over a WebSocket.
    Media(Arc<crate::media::Frame>),
    Ping,
    /// The answer to a ping from the other end, over a WebSocket.
    Pong(Vec<u8>),
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
    /// It missed a message it was promised, and is being let go: one thread puts its close
    /// after what is queued, where one was started for each message that did not fit, and each
    /// waited on a writer stuck on a page that does not read (FEEDBACK49.md, Q-594; branch main).
    cut: AtomicBool,
    /// The channels and streams it asked to join or watch (see `MAX_ASKED`).
    asked: Mutex<HashSet<String>>,
    /// The mount point of the program whose page it is, as its ticket says: its session is that
    /// program's, and no other's (FEEDBACK75.md, Q-1061; branch feedback75).
    base: String,
    /// The bytes of its messages waiting for their channels' handlers (see `PAGE_WAITING`).
    waiting: AtomicUsize,
    /// The client (an address, or an IPv6 /64) it connected from, whose share of the messages begun
    /// its own count in (see `BEGUN`); empty for a page of this process (a phone's window).
    client: Arc<str>,
    /// What its client's pages have waiting (see `WAITING_ALL`).
    client_waiting: Arc<AtomicUsize>,
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
                if reliable && !self.cut.swap(true, Relaxed) {
                    // It would miss a message it was promised: it is let go,
                    // and joins again as new, once it has what was queued. The
                    // thread ends with the writer, which a page that does not
                    // read ends too (`WRITE_WAIT`).
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
    /// Where it is good: its tenant, and the mount point of the program that gave it out; none for
    /// the program at the root with no tenant. A ticket carried its session into every mounted
    /// program's channels (FEEDBACK75.md, Q-1061; branch feedback75).
    tenant: Option<Place>,
}

/// A ticket's tenant and its program's mount point (see `Ticket::tenant`).
#[derive(Clone, Default)]
pub struct Place {
    tenant: Option<String>,
    base: String,
}

/// A connection from a page: registered with the hub, with the queue of
/// what goes to it.
pub fn connect(t: Ticket, media_tx: Option<tokio::sync::mpsc::Sender<Arc<crate::media::Frame>>>) -> (Arc<Peer>, tokio::sync::mpsc::Receiver<Out>) {
    connect_from(t, media_tx, "")
}

/// `connect`, for a page of `client` (see `Peer::client`).
pub fn connect_from(t: Ticket, media_tx: Option<tokio::sync::mpsc::Sender<Arc<crate::media::Frame>>>, client: &str) -> (Arc<Peer>, tokio::sync::mpsc::Receiver<Out>) {
    let (tx, rx) = tokio::sync::mpsc::channel(PEER_QUEUE);
    let id = loop {
        let id = fastrand::u64(1..1 << 48);
        if !lock(&hub().peers).contains_key(&id) {
            break id;
        }
    };
    let Place { tenant, base } = t.tenant.unwrap_or_default();
    let p = Arc::new(Peer {
        id,
        session: t.session,
        tenant,
        tx,
        media_tx,
        rooms: Mutex::new(HashSet::new()),
        cut: AtomicBool::new(false),
        asked: Mutex::new(HashSet::new()),
        base,
        waiting: AtomicUsize::new(0),
        client: Arc::from(client),
        client_waiting: client_counter(client),
    });
    lock(&hub().peers).insert(id, p.clone());
    (p, rx)
}

/// The page is gone: out of its channels, whose `leave` hears of it.
pub fn disconnect(p: &Arc<Peer>) {
    if lock(&hub().peers).remove(&p.id).is_none() {
        return;
    }
    crate::media::unwatch_all(p.id);
    // A join still waiting is left once it has run, when it let the page in (Q-799).
    for ((id, _), left) in lock(join_waits()).iter_mut() {
        if *id == p.id {
            *left = true;
        }
    }
    let rooms: Vec<String> = lock(&p.rooms).drain().collect();
    for r in rooms {
        if let Some(members) = lock(&hub().rooms).get_mut(&r) {
            members.remove(&p.id);
        }
        inbox(key_channel(&r)).push(Event { kind: 'l', peer: p.clone(), value: String::new() });
        crate::media::drop_if_idle(&r);
    }
}

/// A page's message of at most this many bytes is read as JSON with no count: what it makes is at
/// most about 80 KB, gone once it is read; the count cost a message of 40 bytes 6.7% more
/// instructions (measured; FEEDBACK75.md, Q-1052; branch feedback75).
const PLAIN_JSON: usize = 1024;

/// Most channels pages may have asked of, each with an inbox and a thread: each name a page
/// sent made one, whether the program has that channel or not, and none went (found in review
/// of FEEDBACK51.md, Q-643; branch feedback51-fixes).
const MAX_INBOXES: usize = 4096;

/// Most names one page asks of, to join or watch, while it is connected.
const MAX_ASKED: usize = 256;

/// Whether `p` may ask of `ch`: an inbox there, or room for one more, and the page under
/// `MAX_ASKED`. Inboxes no page uses go after a while (see `deliver_loop`), so a page that
/// asked of 4,096 names leaves room again (found in review of FEEDBACK51.md).
fn inbox_ok(p: &Peer, ch: &str) -> bool {
    {
        let mut asked = lock(&p.asked);
        if !asked.contains(ch) {
            if asked.len() >= MAX_ASKED {
                return false;
            }
            asked.insert(ch.to_string());
        }
    }
    let all = lock(&hub().inboxes);
    all.contains_key(ch) || all.len() < MAX_INBOXES
}

/// A message from a page: `["j", channel]` to join, `["l", channel]` to
/// leave, `["m", channel, value]` for the channel's handler; `["v", stream]`
/// to watch a stream (see `media`), `["u", stream]` to stop.
pub fn from_page(p: &Arc<Peer>, text: &str) {
    // A message that cannot wait for room, a WebTransport datagram (the only caller; an app's
    // window or a phone's web view joins as a WebSocket and waits): past its shares it is dropped
    // and counted, and past the count the oldest message goes, as before.
    take_from_page(p, text, false);
}

/// `from_page`, for a page whose reader can wait for room (a WebSocket, a WebTransport stream):
/// its message past its shares waits, and the page with it (see `wait_for_room`). `window` is what
/// its connection may still be sent and hold unread while it waits, outside what the server counts
/// otherwise: none for a WebSocket (the system's buffers), a stream's window for WebTransport.
pub async fn from_page_waiting(p: &Arc<Peer>, text: &str, window: usize) {
    if let Some(pending) = take_from_page(p, text, true) {
        wait_for_room(p, pending, window).await;
    }
}

/// What a page sent, taken (see `from_page`): where the page `can_wait`, a message for which there
/// is no room now given back.
fn take_from_page(p: &Arc<Peer>, text: &str, can_wait: bool) -> Option<Pending> {
    // As deep as every reader takes (FEEDBACK55.md, Q-875's second review; branch feedback55), and
    // past `PLAIN_JSON` within what a page's JSON may make: one message of 1 MiB made 75 MB, outside
    // `--max-memory` (FEEDBACK75.md, Q-1052; branch feedback75).
    let read = if text.len() > PLAIN_JSON {
        match crate::value::json_from_page_why(text.as_bytes()) {
            Ok(j) => Ok(j),
            // JSON a page may send, past what `--max-memory` lets it make: dropped and counted, as
            // a message past a page's share is, where it was lost uncounted; bad JSON is ignored,
            // as before (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1172; branch
            // feedback75).
            Err(e) if e.memory => {
                DROPPED.fetch_add(1, Relaxed);
                return None;
            }
            Err(e) => Err(e.why),
        }
    } else {
        crate::value::json_value(text.as_bytes())
    };
    let Ok(serde_json::Value::Array(a)) = read else { return None };
    let (Some(kind), Some(ch)) = (a.first().and_then(|k| k.as_str()), a.get(1).and_then(|c| c.as_str())) else { return None };
    if !name_ok(ch) {
        return None;
    }
    let key = room_key(ch, p.tenant.as_deref());
    match kind {
        // Watching a stream is asked of its `media.on`, as joining is (FEEDBACK51.md, Q-642).
        "v" if inbox_ok(p, &format!("media:{}", ch)) => inbox(&format!("media:{}", ch)).push(Event { kind: 'w', peer: p.clone(), value: String::new() }),
        "u" => crate::media::unwatch(&room_key(&format!("media:{}", ch), p.tenant.as_deref()), p.id),
        "j" if !lock(&p.rooms).contains(&key) => {
            // Asked again while its join waits: that join is the answer, and a leave asked
            // meanwhile is taken back (Q-799).
            {
                let mut waiting = lock(join_waits());
                if let Some(left) = waiting.get_mut(&(p.id, key.clone())) {
                    *left = false;
                    return None;
                }
                // Let in meanwhile: `joined` moves a page from waiting to the room under this lock.
                if lock(&p.rooms).contains(&key) {
                    return None;
                }
                if !inbox_ok(p, ch) {
                    drop(waiting);
                    joined(ch, p, false);
                    return None;
                }
                waiting.insert((p.id, key), false);
            }
            inbox(ch).push(Event { kind: 'j', peer: p.clone(), value: String::new() });
        }
        "l" => {
            // Under the lock `joined` lets a page in with, so that one or the other sees it.
            let mut waiting = lock(join_waits());
            if lock(&p.rooms).remove(&key) {
                drop(waiting);
                if let Some(members) = lock(&hub().rooms).get_mut(&key) {
                    members.remove(&p.id);
                }
                inbox(ch).push(Event { kind: 'l', peer: p.clone(), value: String::new() });
                crate::media::drop_if_idle(&key);
            } else if let Some(left) = waiting.get_mut(&(p.id, key)) {
                // Its join has not answered: left once it has (Q-799).
                *left = true;
            }
        }
        // Only from a page the channel took.
        "m" if lock(&p.rooms).contains(&key) => {
            let v = a.get(2).cloned().unwrap_or(serde_json::Value::Null);
            let i = inbox(ch);
            let event = Event { kind: 'm', peer: p.clone(), value: v.to_string() };
            if !can_wait {
                i.push(event);
                return None;
            }
            return match i.try_push(event) {
                Ok(()) => None,
                Err((_, Room::Never)) => {
                    DROPPED.fetch_add(1, Relaxed);
                    None
                }
                Err((event, _)) => Some(Pending { inbox: i, event }),
            };
        }
        _ => {}
    }
    None
}

/// A page its stream's `watch` let in starts watching it, its tenant's.
pub fn let_watch(p: &Arc<Peer>, stream: &str) {
    crate::media::watch(&room_key(&format!("media:{}", stream), p.tenant.as_deref()), stream, Arc::new(Viewer(p.clone())));
}

/// The page took the channel's answer to its asking to join. A page that left the room, or went, while its join ran is not let in, and the channel's `leave`
/// hears of it when its `join` let it in: one `leave` for each `join` that said yes, where a page
/// gone within the join was in no room and its `leave` never ran (FEEDBACK55.md, Q-799; branch
/// feedback55). Whether it left or went is read under the lock `disconnect` marks it with, before
/// the page is put in the room, so one of the two always sees the other.
fn joined(ch: &str, p: &Arc<Peer>, yes: bool) {
    let key = room_key(ch, p.tenant.as_deref());
    let held = lock(joining()).remove(&(key.clone(), p.id)).unwrap_or_default();
    let left = {
        let mut waiting = lock(join_waits());
        let left = waiting.remove(&(p.id, key.clone())).unwrap_or(false) || !lock(&hub().peers).contains_key(&p.id);
        if yes && !left {
            lock(&p.rooms).insert(key.clone());
            lock(&hub().rooms).entry(key.clone()).or_default().insert(p.id);
        }
        left
    };
    if left {
        if yes {
            inbox(ch).push(Event { kind: 'l', peer: p.clone(), value: String::new() });
            crate::media::drop_if_idle(&key);
        }
        return;
    }
    p.put(Out::Text { json: Arc::from(serde_json::json!(["j", ch, yes]).to_string()), reliable: true });
    if yes {
        for out in held {
            p.put(out);
        }
    }
}

/// The rooms pages asked to join whose `join` has not answered yet, by page and room, each with
/// whether the page left it (or went) meanwhile: one join a room at a time, and a `leave` for each
/// `join` that let a page in (see `joined`; FEEDBACK55.md, Q-799; branch feedback55). One map for
/// all pages, not one each: a page's own cost each page's thread memory of its own (found in
/// review).
fn join_waits() -> &'static Mutex<HashMap<(u64, String), bool>> {
    static W: OnceLock<Mutex<HashMap<(u64, String), bool>>> = OnceLock::new();
    W.get_or_init(Default::default)
}

/// Pages being let into a channel: from the moment its `join` is called to
/// the moment the page hears yes, what is broadcast (or sent to them) on it
/// waits here, and goes to them after the yes. A roster broadcast from `join`
/// reaches the page that is joining too (FEEDBACK12.md, Q-086; branch
/// feedback12-fixes).
fn joining() -> &'static Mutex<HashMap<(String, u64), Vec<Out>>> {
    static J: OnceLock<Mutex<HashMap<(String, u64), Vec<Out>>>> = OnceLock::new();
    J.get_or_init(Default::default)
}

/// At most this many messages wait for a page that is joining.
const HELD: usize = 256;

/// Holds `json` for the pages joining the rooms `room` takes (all of them, or the one `only`);
/// how many it was held for.
fn hold(room: &dyn Fn(&str) -> bool, json: &str, reliable: bool, only: Option<u64>, except: Option<u64>) -> i64 {
    let mut j = lock(joining());
    let mut n = 0;
    for ((c, id), q) in j.iter_mut() {
        if room(c) && only.is_none_or(|o| o == *id) && Some(*id) != except && q.len() < HELD {
            q.push(Out::Text { json: Arc::from(json), reliable });
            n += 1;
        }
    }
    n
}

/// A message for pages: a builtin's error in it says `internal error`, as everywhere a visitor
/// reads one, where its text went to every page in the channel (FEEDBACK55.md, Q-773; branch
/// feedback55).
fn message(ch: &str, v: &Value) -> String {
    let hidden = if crate::value::hiding() { crate::server::hidden(v) } else { None };
    format!("[\"m\",{},{}]", serde_json::Value::from(ch), hidden.as_ref().unwrap_or(v).json_text())
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
    name: String,
    queue: Mutex<VecDeque<Event>>,
    arrived: Condvar,
    delivering: AtomicBool,
    /// Let go, as no page used it (see `deliver_loop`): what comes goes to a new one.
    closed: AtomicBool,
    /// The bytes of pages' messages in it (see `WAITING_ALL`).
    waiting: AtomicUsize,
}

/// How long an inbox with nothing in it, and no page in its channel's rooms, is kept.
const INBOX_IDLE: Duration = Duration::from_secs(30);

impl Inbox {
    /// Queues `e`: a join, a leave, or a message of a page that cannot wait for room (a datagram,
    /// a page of this process), which past its shares is dropped and counted, as past the count.
    fn push(self: &Arc<Self>, e: Event) {
        let mut q = lock(&self.queue);
        if self.closed.load(Relaxed) {
            drop(q);
            return inbox(&self.name).push(e);
        }
        if e.kind == 'm' && !matches!(charge(&e, self), Room::Taken) {
            DROPPED.fetch_add(1, Relaxed);
            return;
        }
        if q.len() >= INBOX {
            // Joining and leaving are kept; a message is not.
            match q.iter().position(|e| e.kind == 'm') {
                Some(i) => {
                    if let Some(old) = q.remove(i) {
                        release(&old, self);
                        room_freed();
                    }
                }
                None if e.kind == 'm' => {
                    release(&e, self);
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

    /// Queues a page's message if there is room for it, within its shares and the count: else gives
    /// it back with why, for the page to wait (see `wait_for_room`).
    fn try_push(self: &Arc<Self>, e: Event) -> Result<(), (Event, Room)> {
        let mut q = lock(&self.queue);
        if self.closed.load(Relaxed) {
            drop(q);
            return inbox(&self.name).try_push(e);
        }
        if q.len() >= INBOX {
            return Err((e, Room::Full));
        }
        match charge(&e, self) {
            Room::Taken => {}
            no => return Err((e, no)),
        }
        q.push_back(e);
        drop(q);
        self.arrived.notify_one();
        Ok(())
    }
}

/// Whether a message had room.
#[derive(Clone, Copy, PartialEq)]
enum Room {
    Taken,
    /// Not now: its page, its client, its channel or all hold their shares.
    Full,
    /// Never: larger than a page's whole share.
    Never,
}

/// A page's message is to wait for its channel's handler: its bytes counted, its page's, its
/// client's, its channel's and everyone's, or nothing counted, past any of their shares (see
/// `PAGE_WAITING` and `WAITING_ALL`; FEEDBACK75.md, Q-1056; branch feedback75).
fn charge(e: &Event, i: &Inbox) -> Room {
    let n = e.value.len();
    let all = all_waiting();
    let page = PAGE_WAITING.min(all);
    let (client, channel) = ((all / 8).max(page), (all / 4).max(page));
    if n > page {
        return Room::Never;
    }
    let counters: [(&AtomicUsize, usize); 4] = [(&e.peer.waiting, page), (&e.peer.client_waiting, client), (&i.waiting, channel), (&WAITING_ALL, all)];
    for (k, (c, most)) in counters.iter().enumerate() {
        if c.fetch_add(n, Relaxed) + n > *most {
            for (c, _) in &counters[..=k] {
                c.fetch_sub(n, Relaxed);
            }
            return Room::Full;
        }
    }
    Room::Taken
}

/// A message `charge` counted in inbox `i` no longer waits (handed to the program, or dropped); the
/// pages waiting for room are told by `room_freed`.
fn release(e: &Event, i: &Inbox) {
    if e.kind != 'm' {
        return;
    }
    let n = e.value.len();
    for c in [&e.peer.waiting, &*e.peer.client_waiting, &i.waiting, &WAITING_ALL] {
        c.fetch_sub(n, Relaxed);
    }
}

/// A page's message for which there is no room now, and its inbox.
pub struct Pending {
    inbox: Arc<Inbox>,
    event: Event,
}

/// The page waits for room for `pending`, read no further meanwhile, so that what it sends waits
/// in its connection (TCP's window, QUIC's flow control), not in the server: where a message past
/// its shares was dropped, a page bursting ahead of its handler lost some (found in review of
/// FEEDBACK75.md, Q-1056). It holds the message in its client's share of the messages begun
/// (`BEGUN`) while it waits, and the `window` its connection may still be sent unread: QUIC let a
/// waiting WebTransport stream be sent its whole 1 MiB window, which quinn held outside every
/// count, 60 waiting sessions 70 MB (found in review of FEEDBACK75.md, Q-1056). One that cannot be
/// held there has its message dropped and counted, as a datagram is, and goes on: a message of its
/// own, the rest of the stream in order. A page that goes stops waiting. Only the senders to a full
/// channel, or a full client's pages, wait.
pub async fn wait_for_room(p: &Arc<Peer>, mut pending: Pending, window: usize) {
    struct Held<'a>(usize, &'a str);
    impl Drop for Held<'_> {
        fn drop(&mut self) {
            BEGUN.give(self.0, self.1);
            ROOM_WAITERS.fetch_sub(1, Relaxed);
        }
    }
    let n = pending.event.value.len() + window;
    if !BEGUN.take(n, &p.client) {
        DROPPED.fetch_add(1, Relaxed);
        return;
    }
    ROOM_WAITERS.fetch_add(1, Relaxed);
    let _held = Held(n, &p.client);
    loop {
        let told = room().notified();
        tokio::pin!(told);
        told.as_mut().enable();
        match pending.inbox.try_push(pending.event) {
            Ok(()) => return,
            Err((_, Room::Never)) => {
                DROPPED.fetch_add(1, Relaxed);
                return;
            }
            Err((e, _)) => pending.event = e,
        }
        if !lock(&hub().peers).contains_key(&p.id) {
            return;
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), told).await;
    }
}

fn inbox(ch: &str) -> Arc<Inbox> {
    let mut all = lock(&hub().inboxes);
    if let Some(i) = all.get(ch) {
        return i.clone();
    }
    let i = Arc::new(Inbox { name: ch.to_string(), queue: Mutex::new(VecDeque::new()), arrived: Condvar::new(), delivering: AtomicBool::new(false), closed: AtomicBool::new(false), waiting: AtomicUsize::new(0) });
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

/// Takes the inbox of `ch` away if it is still empty and no page is in its channel's rooms;
/// whether it did.
fn let_go(ch: &str, i: &Arc<Inbox>) -> bool {
    let mut all = lock(&hub().inboxes);
    let q = lock(&i.queue);
    let used = !q.is_empty() || lock(&hub().rooms).iter().any(|(k, m)| !m.is_empty() && key_channel(k) == ch);
    if used {
        return false;
    }
    i.closed.store(true, Relaxed);
    drop(q);
    if all.get(ch).is_some_and(|x| Arc::ptr_eq(x, i)) {
        all.remove(ch);
    }
    true
}

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
            let idle = std::time::Instant::now();
            while q.is_empty() {
                if crate::httpd::stopping() {
                    return;
                }
                // Nothing for a while and no page in its rooms: let go, with its thread.
                if idle.elapsed() > INBOX_IDLE {
                    drop(q);
                    if let_go(&ch, &i) {
                        return;
                    }
                    q = lock(&i.queue);
                    continue;
                }
                q = i.arrived.wait_timeout(q, Duration::from_millis(500)).unwrap_or_else(std::sync::PoisonError::into_inner).0;
            }
            let (mut n, mut bytes) = (0, 0);
            while n < q.len() && n < 1024 && (n == 0 || bytes + q[n].value.len() <= 1 << 20) {
                bytes += q[n].value.len();
                n += 1;
            }
            q.drain(..n).collect()
        };
        // Each line written as it goes, where a JSON value was made of each event and then written:
        // the batch's handing over cost what the base each line came to say added, and more
        // (FEEDBACK75.md, Q-1061's review, measured; branch feedback75). The same lines: `k`, `p`,
        // `s`, `t`, `v`, and `b`, whose page, where it is not the root program's (its session is
        // that program's; Q-1061).
        let mut batch = batch;
        let mut body: Vec<u8> = Vec::with_capacity(batch.iter().map(|e| e.value.len() + e.peer.session.len() + 64).sum());
        // A batch's body within what the server reads of a request's (`--max-memory`, `--max-body`;
        // none without them): one of 1,024 lines and 1 MiB of values, each line with its page's
        // session and its value written again as a JSON string, was refused whole past either,
        // every message in it lost and none counted. The lines past it go back to the front of the
        // queue, for the next batch; a message whose line alone is past it can never be handed
        // over, and is dropped and counted, as one past a page's share is (FEEDBACK75.md follow-up
        // found by 0.8.9's coverage, Q-1172; branch feedback75).
        let most = crate::httpd::most_bytes().min(crate::httpd::body_most());
        let (mut at, mut dropped) = (0, false);
        while at < batch.len() {
            use std::io::Write as _;
            let (e, start) = (&batch[at], body.len());
            let _ = write!(body, "{{\"k\":\"{}\",\"p\":\"{:012x}\",\"s\":", e.kind, e.peer.id);
            let _ = serde_json::to_writer(&mut body, &e.peer.session);
            body.extend_from_slice(b",\"t\":");
            let _ = serde_json::to_writer(&mut body, &e.peer.tenant);
            if !e.peer.base.is_empty() {
                body.extend_from_slice(b",\"b\":");
                let _ = serde_json::to_writer(&mut body, &e.peer.base);
            }
            body.extend_from_slice(b",\"v\":");
            let _ = serde_json::to_writer(&mut body, &e.value);
            body.extend_from_slice(b"}\n");
            if body.len() > most {
                if start > 0 {
                    body.truncate(start);
                    let rest: Vec<Event> = batch.drain(at..).collect();
                    let mut q = lock(&i.queue);
                    for e in rest.into_iter().rev() {
                        q.push_front(e);
                    }
                    break;
                }
                // A join, a leave or a watch alone is handed over as it was: refused, a join is
                // answered no.
                if e.kind == 'm' {
                    body.truncate(start);
                    release(&batch.remove(at), &i);
                    DROPPED.fetch_add(1, Relaxed);
                    dropped = true;
                    continue;
                }
            }
            at += 1;
        }
        if dropped {
            room_freed();
        }
        if batch.is_empty() {
            continue;
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
            match c.post_for(&path, &body) {
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
        for e in batch.iter().filter(|e| matches!(e.kind, 'j' | 'w')) {
            let yes = v.next().unwrap_or(false);
            if e.kind == 'j' {
                joined(&ch, &e.peer, yes);
            } else if yes && lock(&hub().peers).contains_key(&e.peer.id) {
                // The prefix once: `["v", "media:x"]` asked of `media:media:x`'s rule watched `x`
                // (found in review of FEEDBACK51.md).
                let_watch(&e.peer, ch.strip_prefix("media:").unwrap_or(&ch));
            }
        }
        // The batch is done with: its messages no longer wait, and pages waiting for room are told
        // (Q-1056).
        for e in &batch {
            release(e, &i);
        }
        room_freed();
    }
}

// ---- a service's channels under --split (FEEDBACK12.md, Q-087; branch open-fixes) ----

/// In the app under `--split`: the services in processes of their own.
static SPLIT: OnceLock<Vec<String>> = OnceLock::new();

pub fn set_split(names: Vec<String>) {
    let _ = SPLIT.set(names);
}

/// What the services said their channels are (names, or starts ending `*`),
/// and when they were asked: asked again, at most every two seconds, when a
/// channel is no one's (a service started again, or reloaded).
#[derive(Default)]
struct Remote {
    names: Vec<(String, String)>,
    asked: Option<std::time::Instant>,
    /// Some service did not answer when last asked (it was starting again, or down): the names
    /// it said before are kept, and a channel no one claims is not joined meanwhile. A service
    /// that did not answer lost its names, and for the next two seconds anyone joined its channels
    /// as no one's, without its `join`, and stayed after it came back (FEEDBACK75.md, Q-1065;
    /// branch feedback75).
    missing: bool,
}

fn remote_channels() -> &'static Mutex<Remote> {
    static R: OnceLock<Mutex<Remote>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// Whether a service of this app, in a process of its own, did not answer when last asked which
/// channels are its (see `Remote::missing`).
fn remote_missing() -> bool {
    SPLIT.get().is_some_and(|s| !s.is_empty()) && lock(remote_channels()).missing
}

fn remote_for(it: &mut Interp, name: &str) -> Option<String> {
    let services = SPLIT.get().filter(|s| !s.is_empty())?;
    let matches = |n: &str| n == name || n.strip_suffix('*').is_some_and(|p| name.starts_with(p));
    {
        let r = lock(remote_channels());
        if let Some((_, s)) = r.names.iter().find(|(n, _)| matches(n)) {
            return Some(s.clone());
        }
        if r.asked.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return None;
        }
    }
    let asks: Vec<(String, Option<String>)> = services.iter().map(|s| (s.clone(), crate::service::first_url_of(&it.shared, s))).collect();
    let (found, silent) = it.unlocked(move || {
        let (mut all, mut silent) = (Vec::new(), Vec::new());
        for (s, url) in asks {
            let answer = url.ok_or(()).and_then(|u| crate::service::signed_post(&u, "/_quillo/service-channels", &s, "_channels", b"[]").map_err(|_| ()));
            match answer.and_then(|b| serde_json::from_slice::<Vec<String>>(&b).map_err(|_| ())) {
                Ok(names) => all.extend(names.into_iter().map(|n| (n, s.clone()))),
                Err(()) => silent.push(s),
            }
        }
        (all, silent)
    });
    let mut r = lock(remote_channels());
    // A service that answered says what its channels are now; one that did not keeps those it
    // said before (Q-1065).
    let kept: Vec<(String, String)> = r.names.iter().filter(|(_, s)| silent.contains(s)).cloned().collect();
    r.names = found;
    r.names.extend(kept);
    r.asked = Some(std::time::Instant::now());
    r.missing = !silent.is_empty();
    r.names.iter().find(|(n, _)| matches(n)).map(|(_, s)| s.clone())
}

/// `POST /_quillo/service-channels`, in a service: the channels its
/// `channel.on` took, for the app to hand it what pages send there.
pub fn names_here(it: &Interp) -> crate::server::HttpResponse {
    let names: Vec<&str> = it.site.channels.iter().map(|h| h.name.as_str()).collect();
    crate::server::HttpResponse { status: 200, headers: vec![("content-type".into(), "application/json".into())], body: serde_json::to_vec(&names).unwrap_or_default() }
}

/// The app hands a batch for a service's channel to the service's process,
/// and gives back its verdicts. Pages joining are held for (as `join` here
/// would), so that what the service broadcasts from its `join` reaches them.
fn forward(it: &mut Interp, service: &str, name: &str, body: &[u8]) -> crate::server::HttpResponse {
    let text = String::from_utf8_lossy(body);
    let mut joins = 0;
    // The service is this program's: a page of another program it mounts is sent with no session,
    // as that program's session is no session here (FEEDBACK75.md, Q-1061; branch feedback75).
    let mut sent = Vec::with_capacity(body.len());
    let mut messages = 0u64;
    for line in text.lines() {
        let Ok(mut e) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        messages += u64::from(e["k"] == "m");
        if e["k"] == "j" {
            joins += 1;
            if let Some(id) = e["p"].as_str().and_then(|p| u64::from_str_radix(p, 16).ok()) {
                // The tenant's room, as `joined` takes it out (found in review of FEEDBACK51.md).
                lock(joining()).insert((room_key(name, e["t"].as_str()), id), Vec::new());
            }
        }
        if e["b"].as_str().unwrap_or("") != it.base {
            e["s"] = serde_json::Value::from("{}");
            sent.extend_from_slice(e.to_string().as_bytes());
        } else {
            sent.extend_from_slice(line.as_bytes());
        }
        sent.push(b'\n');
    }
    let url = crate::service::first_url_of(&it.shared, service);
    let (to, ch) = (service.to_string(), name.to_string());
    let answer = it.unlocked(move || match url {
        // As bytes, as the batch came here: as JSON, the service read it whole first and refused
        // it past `--max-memory`; one still refused is counted, its messages dropped
        // (FEEDBACK75.md follow-up found by 0.8.9's review of Q-1172; branch feedback75).
        Some(u) => crate::service::signed_post_as(&u, &format!("/_quillo/service-channel/{}", ch), &to, &format!("_channel:{}", ch), &sent, "application/octet-stream"),
        None => Err(format!("service {} has no address", to)),
    });
    let body = match answer {
        Ok(b) => b,
        Err(e) => {
            DROPPED.fetch_add(messages, Relaxed);
            eprintln!("quillo: channel {}: service {} did not take what pages sent: {}", name, service, e);
            serde_json::to_vec(&vec![false; joins]).unwrap_or_default()
        }
    };
    crate::server::HttpResponse { status: 200, headers: vec![("content-type".into(), "application/json".into())], body }
}

/// In a service's process started by `--split`: what it sends on a channel
/// goes to the app's pages, through the app. `None` when this process's pages
/// are its own.
fn relay(it: &mut Interp, op: serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    let app = crate::service::app_url()?;
    let body = op.to_string().into_bytes();
    let out = it.unlocked(move || crate::service::signed_post(app, "/_quillo/channel-relay", "", "_channel-relay", &body));
    Some(out.and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())))
}

/// `POST /_quillo/channel-relay`, in the app: a service's `channel.broadcast`,
/// `send` or `peers`, done with the app's pages.
pub fn relay_here(body: &[u8]) -> crate::server::HttpResponse {
    let Ok(op) = crate::value::json_value(body) else {
        return crate::server::HttpResponse { status: 400, headers: Vec::new(), body: b"not JSON\n".to_vec() };
    };
    let ch = op["ch"].as_str().unwrap_or("");
    // Whose rooms the service's code reaches (FEEDBACK51.md, Q-644).
    let reach = reach_of(&op);
    let json = op["json"].as_str().unwrap_or("");
    let reliable = op["reliable"].as_bool().unwrap_or(false);
    let peer = |k: &str| op[k].as_str().and_then(|p| u64::from_str_radix(p, 16).ok());
    let out = if !name_ok(ch) || json.len() > MAX_MESSAGE {
        serde_json::Value::Null
    } else {
        match op["op"].as_str().unwrap_or("") {
            "b" => serde_json::json!(broadcast_here(ch, reach, json, reliable, peer("except"))),
            "s" => serde_json::json!(peer("to").is_some_and(|id| send_here(ch, reach, id, json, reliable))),
            "p" => serde_json::json!(peers_here(ch, reach)),
            _ => serde_json::Value::Null,
        }
    };
    crate::server::HttpResponse { status: 200, headers: vec![("content-type".into(), "application/json".into())], body: out.to_string().into_bytes() }
}

/// `POST /_quillo/channel/<name>`: a batch of what pages did, each given to
/// the channel's functions in turn. The answer is the joins' verdicts.
pub fn answer(it: &mut Interp, name: &str, body: &[u8]) -> crate::server::HttpResponse {
    let handler = find(it, name);
    // A service's channel, the service in a process of its own (Q-087).
    if handler.is_none() {
        if let Some(service) = remote_for(it, name) {
            return forward(it, &service, name, body);
        }
    }
    // A channel no reachable program claims, while a service apart did not say which are its:
    // not joined, as it may be that service's (FEEDBACK75.md, Q-1065; branch feedback75).
    let unclaimed_ok = handler.is_some() || !remote_missing();
    // In a service's process apart, what comes is what the app let through (see `forward`).
    let from_app = crate::service::app_url().is_some();
    // The stream's name (a channel's own), given to `join`, `publish` and `watch` second: a rule
    // for `media.on("call:*")` was not told which stream a page asked for, and let a page allowed
    // one call watch every call and send into anyone's (FEEDBACK75.md, Q-1050, the user's decision
    // of 2026-11-18: `(peer, name) =>`; branch feedback75). A rule of one parameter is called as it was.
    let asked = Value::str(name.strip_prefix("media:").unwrap_or(name));
    let mut verdicts: Vec<bool> = Vec::new();
    // The handler of each program pages of the batch are of, worked out once a batch (see
    // `own_handler`): once a line, it cost each message 5% more instructions (measured).
    let mut owners: Vec<(String, Option<(Arc<Handler>, Option<Vec<usize>>)>)> = Vec::new();
    let text = String::from_utf8_lossy(body);
    for line in text.lines() {
        let Ok(e) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let kind = e["k"].as_str().unwrap_or("");
        let Some((found, found_sub)) = handler.as_ref() else {
            // A channel no `channel.on` names: pages may join it (to hear
            // the program's broadcasts), and what they send goes nowhere. Not a stream's,
            // `media:<name>`, which only a `media.on` lets pages send or watch (FEEDBACK51.md,
            // Q-643; branch feedback51-fixes).
            match kind {
                "j" => verdicts.push(!name.starts_with("media:") && unclaimed_ok),
                "w" => verdicts.push(false),
                _ => {}
            }
            continue;
        };
        // The handler of the page's own program, or of a service it declares, which sees the
        // page's session; else the one found, in another mounted program, which sees none: a
        // ticket of one program carried its session into another's channels, and a session a
        // visitor sets freely in the root passed a mount's `join` (FEEDBACK75.md, Q-1061; branch
        // feedback75).
        let base = e["b"].as_str().unwrap_or("");
        let mine = match owners.iter().find(|(b, _)| b == base) {
            Some((_, o)) => o.clone(),
            None => {
                let o = own_handler(it, name, base);
                owners.push((base.to_string(), o.clone()));
                o
            }
        };
        let (h, sub, own) = match mine {
            Some((h, sub)) => (h, sub, true),
            None => (found.clone(), found_sub.clone(), from_app),
        };
        let it: &mut Interp = match &sub {
            Some(path) => mounted(&mut *it, path),
            None => &mut *it,
        };
        let session = if own { crate::value::json_value(e["s"].as_str().unwrap_or("{}").as_bytes()).map(|j| Value::from_json(&j)).unwrap_or(Value::Nil) } else { Value::Nil };
        let session = if matches!(session, Value::Map(_)) { session } else { Value::map(Map::new()) };
        let mut peer = Map::new();
        peer.insert("id".into(), Value::str(e["p"].as_str().unwrap_or("")));
        peer.insert("session".into(), session.clone());
        let peer = Value::map(peer);
        // As if the page asked: its session and tenant.
        let saved = (it.session.replace(session), it.tenant.replace(e["t"].as_str().map(Arc::from)), it.tenant_named);
        it.tenant_named = e["t"].is_string();
        let was_for_page = FOR_PAGE.with(|f| f.replace(true));
        let res = match kind {
            "j" => match &h.join {
                Some(f) => {
                    if let Some(id) = e["p"].as_str().and_then(|p| u64::from_str_radix(p, 16).ok()) {
                        lock(joining()).insert((room_key(name, e["t"].as_str()), id), Vec::new());
                    }
                    let r = it.call(f, args_for(f, vec![peer, asked.clone()]), 0);
                    let yes = matches!(&r, Ok(v) | Err(Ctrl::Return(v)) if v.truthy());
                    verdicts.push(yes);
                    r.map(|_| Value::Nil)
                }
                None => {
                    verdicts.push(true);
                    Ok(Value::Nil)
                }
            },
            // Watching a stream: only as its `watch` says; with none, no page watches
            // (FEEDBACK51.md, Q-642; branch feedback51-fixes).
            "w" => match &h.watch {
                Some(f) => {
                    let r = it.call(f, args_for(f, vec![peer, asked.clone()]), 0);
                    verdicts.push(matches!(&r, Ok(v) | Err(Ctrl::Return(v)) if v.truthy()));
                    r.map(|_| Value::Nil)
                }
                None => {
                    verdicts.push(false);
                    Ok(Value::Nil)
                }
            },
            "l" => match &h.leave {
                Some(f) => it.call(f, args_for(f, vec![peer]), 0),
                None => Ok(Value::Nil),
            },
            "m" => {
                let v = crate::value::json_value(e["v"].as_str().unwrap_or("null").as_bytes()).map(|j| Value::from_json(&j)).unwrap_or(Value::Nil);
                it.call(&h.f, args_for(&h.f, vec![v, peer]), 0)
            }
            _ => Ok(Value::Nil),
        };
        (it.session, it.tenant, it.tenant_named) = saved;
        FOR_PAGE.with(|f| f.set(was_for_page));
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

/// The handler of `name` in the program at mount point `base` (this one, or one it mounts), or in
/// a service that program declares under a prefix, which is part of it as one apart is (Q-087):
/// the programs a page of that program's session is good in (FEEDBACK75.md, Q-1061; branch
/// feedback75). None when no such program has one, or no program is at `base`.
fn own_handler(it: &Interp, name: &str, base: &str) -> Option<(Arc<Handler>, Option<Vec<usize>>)> {
    let mut path = Vec::new();
    let mut at = it;
    while at.base != base {
        let under = |m: &Interp| base.strip_prefix(m.base.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
        let (i, sub) = at.mounts.iter().enumerate().find(|(_, (_, m))| !m.base.is_empty() && under(m)).map(|(i, (_, m))| (i, m))?;
        path.push(i);
        at = sub;
    }
    let matches = |h: &Handler| h.name == name || h.name.strip_suffix('*').is_some_and(|p| name.starts_with(p));
    if let Some(h) = at.site.channels.iter().find(|h| matches(h)) {
        return Some((h.clone(), (!path.is_empty()).then_some(path)));
    }
    for (i, (_, sub)) in at.mounts.iter().enumerate() {
        if !sub.is_service || at.is_service {
            continue;
        }
        if let Some((h, more)) = find(sub, name) {
            let mut p = path.clone();
            p.push(i);
            p.extend(more.unwrap_or_default());
            return Some((h, Some(p)));
        }
    }
    None
}

/// The program mounted at `path`, as `find` gave it, from this program's own mounts.
fn mounted<'a>(it: &'a mut Interp, path: &[usize]) -> &'a mut Interp {
    let mut at = it;
    for &i in path {
        at = &mut at.mounts[i].1;
    }
    at
}

// ---- builtins ----------------------------------------------------------------

fn name_arg(args: &[Value], f: &str, line: u32) -> Result<String, Ctrl> {
    match args.first() {
        Some(Value::Str(s) | Value::Css(s)) if name_ok(s) => Ok(s.to_string()),
        Some(Value::Str(s) | Value::Css(s)) => fail(line, format!("`{}`: a channel's name is letters, digits, `_`, `-`, `.` and `:`, up to 64, got {:?}", f, &**s)).map(|_: Value| String::new()),
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
    let (mut join, mut leave, mut everyone) = (None, None, false);
    match args.get(2) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(o)) => {
            for (k, v) in o.iter() {
                match (k.as_str(), v) {
                    ("join", f @ (Value::Func(_) | Value::Builtin(..))) => join = Some(f.clone()),
                    ("leave", f @ (Value::Func(_) | Value::Builtin(..))) => leave = Some(f.clone()),
                    ("join" | "leave", _) => return fail(line, format!("`channel.on`: `{}` is a function of the page", k.as_str())),
                    ("tenants", v) => everyone = shared_opt(v, "channel.on", line)?,
                    _ => return fail(line, format!("`channel.on` has no option `{}` (it takes join, leave and tenants)", k.as_str())),
                }
            }
        }
        Some(v) => return fail(line, format!("`channel.on` expects options as a map, got {}", v.type_name())),
    }
    // What pages may send to the server: a taken file's channel, only where it was confirmed
    // (FEEDBACK83.md, Q-1301, found by the review of 4bc1ac9; branch feedback82-85).
    if !crate::builtins::site_wide(it, || format!("channel {}", name)) {
        return Ok(Value::Nil);
    }
    let site = Arc::make_mut(&mut it.site);
    if site.channels.iter().any(|h| h.name == name) {
        return fail(line, format!("`channel.on`: the program already handles channel {:?}", name));
    }
    if everyone {
        share(&name);
    }
    site.channels.push(Arc::new(Handler { name, f, join, leave, watch: None }));
    Ok(Value::Nil)
}

/// `tenants: "shared"`: one room for every tenant, where each has its own.
pub fn shared_opt(v: &Value, f: &str, line: u32) -> Result<bool, Ctrl> {
    match v {
        Value::Str(s) | Value::Css(s) if &**s == "shared" => Ok(true),
        Value::Str(s) | Value::Css(s) if &**s == "own" => Ok(false),
        other => fail(line, format!("`{}`: `tenants` is \"shared\" (one room for every tenant) or \"own\", got {}", f, other.repr())).map(|_: Value| false),
    }
}

/// The peer a value names: a peer map, or its id.
fn peer_of(v: Option<&Value>) -> Option<u64> {
    let id = match v? {
        Value::Map(m) => match m.get("id") {
            Some(Value::Str(s) | Value::Css(s)) => s.clone(),
            _ => return None,
        },
        Value::Str(s) | Value::Css(s) => s.clone(),
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
    // From a service apart, through the app's pages (Q-087).
    let reach = reach(it);
    let op = serde_json::json!({ "op": "s", "ch": name, "json": json, "reliable": reliable, "to": format!("{:x}", id), "t": reach.clone().flatten(), "all": reach.is_none() });
    if let Some(r) = relay(it, op) {
        return Ok(Value::Bool(r.map_or(false, |v| v.as_bool().unwrap_or(false))));
    }
    Ok(Value::Bool(send_here(&name, reach.as_ref().map(|t| t.as_deref()), id, &json, reliable)))
}

/// To one page in its tenant's room of `name`, if the code sending reaches that room: one
/// tenant's handler sent to another tenant's page by its id (found in review of FEEDBACK51.md,
/// Q-644).
fn send_here(name: &str, reach: Option<Option<&str>>, id: u64, json: &str, reliable: bool) -> bool {
    let p = lock(&hub().peers).get(&id).cloned();
    let Some(p) = p else { return false };
    let key = room_key(name, p.tenant.as_deref());
    if !reaches(name, reach)(&key) {
        return false;
    }
    if lock(&p.rooms).contains(&key) {
        return p.put(Out::Text { json: Arc::from(json), reliable });
    }
    // Being let in: it gets it after the yes (Q-086).
    hold(&|k: &str| k == key, json, reliable, Some(id), None) > 0
}

/// `channel.broadcast(name, value, { except, reliable })`: to every page in
/// the channel; how many it went to.
fn broadcast(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = name_arg(&args, "channel.broadcast", line)?;
    let v = args.get(1).cloned().unwrap_or(Value::Nil);
    let (reliable, except) = reliable_opt(args.get(2), "channel.broadcast", line)?;
    let json = message(&name, &v);
    too_big(&json, "channel.broadcast", line)?;
    // From a service apart, to the app's pages (Q-087).
    let reach = reach(it);
    let op = serde_json::json!({ "op": "b", "ch": name, "json": json, "reliable": reliable, "except": except.map(|e| format!("{:x}", e)), "t": reach.clone().flatten(), "all": reach.is_none() });
    if let Some(r) = relay(it, op) {
        return Ok(Value::Int(r.map_or(0, |v| v.as_i64().unwrap_or(0))));
    }
    Ok(Value::Int(broadcast_here(&name, reach.as_ref().map(|t| t.as_deref()), &json, reliable, except)))
}

/// To the pages in the rooms of `name` that code reaching `reach` reaches (see `reach`).
fn broadcast_here(name: &str, reach: Option<Option<&str>>, json: &str, reliable: bool, except: Option<u64>) -> i64 {
    let json: Arc<str> = Arc::from(json);
    let room = reaches(name, reach);
    let ids: Vec<u64> = lock(&hub().rooms).iter().filter(|(k, _)| room(k)).flat_map(|(_, m)| m.iter().copied()).collect();
    let peers: Vec<Arc<Peer>> = {
        let all = lock(&hub().peers);
        ids.iter().filter(|id| Some(**id) != except).filter_map(|id| all.get(id).cloned()).collect()
    };
    let mut n = hold(&room, &json, reliable, None, except);
    for p in peers {
        if p.put(Out::Text { json: json.clone(), reliable }) {
            n += 1;
        }
    }
    n
}

/// `channel.peers(name)`: the ids of the pages in it.
fn peers(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = name_arg(&args, "channel.peers", line)?;
    // A service apart asks the app, whose pages they are (Q-087).
    let reach = reach(it);
    if let Some(r) = relay(it, serde_json::json!({ "op": "p", "ch": name, "t": reach.clone().flatten(), "all": reach.is_none() })) {
        let ids = r.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        return Ok(Value::list(ids.iter().filter_map(|v| v.as_str()).map(Value::str).collect()));
    }
    Ok(Value::list(peers_here(&name, reach.as_ref().map(|t| t.as_deref())).into_iter().map(Value::str).collect()))
}

fn peers_here(name: &str, reach: Option<Option<&str>>) -> Vec<String> {
    let room = reaches(name, reach);
    let mut ids: Vec<u64> = lock(&hub().rooms).iter().filter(|(k, _)| room(k)).flat_map(|(_, m)| m.iter().copied()).collect();
    ids.sort();
    ids.into_iter().map(|id| format!("{:012x}", id)).collect()
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
    // Only for a program whose pages can use channels: on every program, anyone could connect and
    // send what the server reads, and start WebTransport's listener (FEEDBACK75.md, Q-1052; branch
    // feedback75). Elsewhere it is a path the program does not have.
    if !uses_channels(it) {
        return crate::server::plain(404, "not found\n");
    }
    let session = crate::session::current(it).to_json().to_string();
    let tenant = it.tenant().map(|t| t.to_string());
    // The mount point of the program giving it out: its session is good there alone (FEEDBACK75.md,
    // Q-1061; branch feedback75).
    let sealed = crate::session::seal(
        "quillo channel ticket",
        &serde_json::json!({ "s": session, "t": tenant, "b": it.base, "e": now() + TICKET_SECS }).to_string(),
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

/// Whether the program's pages may use channels or media, and so are given tickets: yes on any
/// doubt, as a wrong no breaks a program that uses them and a wrong yes leaves things as they were
/// in 0.8.8 (FEEDBACK75.md, Q-1052, the coordinator's call: fail open; branch feedback75). Yes for
/// a `channel.on` or `media.on`; a program mounted, mounting another, or with services (here or
/// apart); one that names `channel` or `media` anywhere (a channel only broadcast to, a component's
/// `channel.open`, `<quillo-media>`, a script's text), calls the page's scripts (`js`), or takes in
/// what is not a Quillo file (a plugin, a script for the page). Worked out once for each program
/// loaded, from its statements and those of the files it uses.
fn uses_channels(it: &Interp) -> bool {
    if !it.site.channels.is_empty() || !it.base.is_empty() || !it.mounts.is_empty() || !it.remote_mounts.is_empty() || SPLIT.get().is_some_and(|s| !s.is_empty()) {
        return true;
    }
    type Program = Arc<Vec<Arc<Vec<crate::ast::Stmt>>>>;
    static SEEN: Mutex<Vec<(Program, bool)>> = Mutex::new(Vec::new());
    if let Some((_, uses)) = lock(&SEEN).iter().find(|(p, _)| Arc::ptr_eq(p, &it.program)) {
        return *uses;
    }
    let uses = may_name_channels(&it.program);
    let mut seen = lock(&SEEN);
    // The programs loaded last, each kept while it is here so that its place is not another's.
    if seen.len() >= 8 {
        seen.remove(0);
    }
    seen.push((it.program.clone(), uses));
    uses
}

/// Whether statements may use channels (see `uses_channels`): a `use` of what is not a `.qwe`
/// file, a `mount` or a `service`, or `channel`, `media` or a call of `js` in their written form,
/// searched as it is made, a piece at a time, and stopped at the first.
fn may_name_channels(program: &[Arc<Vec<crate::ast::Stmt>>]) -> bool {
    use crate::ast::Stmt;
    struct Search {
        tail: Vec<u8>,
        found: bool,
    }
    const NAMES: [&[u8]; 3] = [b"channel", b"media", b"Ident(\"js\""];
    impl std::fmt::Write for Search {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.tail.extend_from_slice(s.as_bytes());
            if NAMES.iter().any(|n| memchr::memmem::find(&self.tail, n).is_some()) {
                self.found = true;
                return Err(std::fmt::Error);
            }
            let keep = self.tail.len().min(16);
            self.tail.drain(..self.tail.len() - keep);
            Ok(())
        }
    }
    let mut search = Search { tail: Vec::new(), found: false };
    for stmts in program {
        if stmts.iter().any(|s| matches!(s, Stmt::Use(path, ..) if !path.ends_with(".qwe")) || matches!(s, Stmt::Mount(..) | Stmt::Service(..))) {
            return true;
        }
        // Formatting stops only where the search stops it: any other error, and it says yes.
        if std::fmt::write(&mut search, format_args!("{:?}", stmts)).is_err() {
            return true;
        }
    }
    search.found
}

/// The ticket in a connection's `?t=`, if it is good, with the mount point of the program that
/// gave it out (FEEDBACK75.md, Q-1061; branch feedback75). One made before tickets said it (by
/// 0.8.8 or before, good for 120 s) is the root program's, as it was taken.
pub fn ticket_of(query: &str) -> Option<Ticket> {
    let t = query.split('&').find_map(|kv| kv.strip_prefix("t="))?;
    let t = crate::server::percent_decode(t, false);
    let j: serde_json::Value = serde_json::from_str(&crate::session::unseal("quillo channel ticket", &t)?).ok()?;
    if j["e"].as_u64()? < now() {
        return None;
    }
    let place = Place { tenant: j["t"].as_str().map(str::to_string), base: j["b"].as_str().unwrap_or("").to_string() };
    Some(Ticket { session: j["s"].as_str()?.to_string(), tenant: (place.tenant.is_some() || !place.base.is_empty()).then_some(place) })
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

/// On a clean stop: each page's WebTransport session told the server is going (FEEDBACK80.md,
/// Q-1265; branch feedback72-81).
pub fn close_all() {
    #[cfg(feature = "realtime")]
    quic::close_all();
}

#[cfg(feature = "realtime")]
mod quic {
    use super::*;
    use wtransport::{Endpoint, Identity, ServerConfig};

    struct Running {
        port: u16,
        hash: RwLock<Option<String>>,
        /// Its listeners: one, or on loopback, 127.0.0.1's and [::1]'s (FEEDBACK80.md, Q-1266),
        /// closed on a clean stop (Q-1265; branch feedback72-81).
        servers: Vec<Arc<Endpoint<wtransport::endpoint::endpoint_side::Server>>>,
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
        r.as_ref().map(|r| (r.port, r.hash.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()))
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

    /// The server's QUIC limits: what a session may open and have sent before it is read. With
    /// quinn's own (100 streams each way, 1.25 MB a stream, no limit a connection), one session
    /// had the server hold about 110 MB of what it sent, read or not (measured on 0.8.8: 3 sessions
    /// of 90 streams, 15 MB to 342 MB). A page opens HTTP/3's 3 streams, one per track it sends
    /// (`MOST_TRACKS`), and its first stream both ways (FEEDBACK75.md, Q-1074; branch feedback75).
    fn transport() -> wtransport::config::QuicTransportConfig {
        let mut t = wtransport::config::QuicTransportConfig::default();
        // A stream may have a whole frame (1 MiB) on its way, and the session four: with 256 KiB a
        // stream, 1 MB frames at 30 a second came at 27, and a stream at 100 ms took 21 Mbit/s at
        // most (found in review of FEEDBACK75.md, Q-1074). What a session holds unread is still
        // the connection's window at most.
        // Two streams both ways, what the protocol uses: HTTP/3's CONNECT and the page's first. With
        // four, a session opened two more that nothing reads, each holding its window for good
        // (found in review of FEEDBACK75.md, Q-1074).
        t.max_concurrent_uni_streams((3 + MOST_TRACKS as u32 + 1).into())
            .max_concurrent_bidi_streams(2u32.into())
            .stream_receive_window(STREAM_WINDOW.into())
            .receive_window((4u32 << 20).into());
        t
    }

    fn config(addr: SocketAddr, id: Identity) -> ServerConfig {
        ServerConfig::builder()
            .with_bind_address(addr)
            .with_custom_transport(id, transport())
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
            Some((cert, key)) => {
                let id = rt.block_on(Identity::load_pemfiles(cert, key)).map_err(|e| format!("cannot read the certificate: {}", e))?;
                // A file with no certificate in it reads, and wtransport then panics on it: said
                // here instead (FEEDBACK31.md, Q-381; branch feedback27-35-fixes).
                if id.certificate_chain().as_slice().is_empty() {
                    return Err(format!("cannot read the certificate: {} holds none", cert.display()));
                }
                (id, false)
            }
            None => (own_identity()?, true),
        };
        let hash = if own { hex_hash(&id) } else { None };
        let _in_rt = rt.enter();
        // On 127.0.0.1, [::1] too: a page at http://localhost dials the ticket's port at
        // `localhost`, which Chrome takes to [::1] for QUIC, with no fall back to 127.0.0.1 as TCP
        // has, so it was refused and took the WebSocket for good. Both are this machine alone, as
        // the HTTP listener is; no wider. Listening there, rather than naming 127.0.0.1 in the
        // ticket, keeps the page's own host name, which its certificate hash and its origin go
        // with, and needs nothing of the page. A machine with no IPv6 loopback keeps 127.0.0.1
        // (FEEDBACK80.md, Q-1266; branch feedback72-81).
        let mut addrs = vec![addr];
        if addr.ip() == std::net::IpAddr::from(std::net::Ipv4Addr::LOCALHOST) {
            addrs.push(SocketAddr::new(std::net::Ipv6Addr::LOCALHOST.into(), port));
        }
        let mut servers = Vec::new();
        for (i, at) in addrs.into_iter().enumerate() {
            let server = match Endpoint::server(config(at, id.clone_identity())) {
                Ok(s) => Arc::new(s),
                Err(e) if i == 0 => return Err(format!("cannot listen on UDP port {}: {}", port, e)),
                Err(_) => continue,
            };
            rt.spawn({
                let server = server.clone();
                async move {
                    loop {
                        let incoming = server.accept().await;
                        tokio::spawn(session(incoming));
                    }
                }
            });
            servers.push((at, server));
        }
        if own {
            // A new certificate before this one ends; pages connecting after
            // are told the new hash, and those connected stay.
            let servers = servers.clone();
            rt.spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(11 * 86400)).await;
                    let Ok(id) = own_identity() else { continue };
                    let hash = hex_hash(&id);
                    if servers.iter().all(|(at, server)| server.reload_config(config(*at, id.clone_identity()), false).is_ok()) {
                        if let Some(Some(r)) = RUNNING.get() {
                            *r.hash.write().unwrap_or_else(std::sync::PoisonError::into_inner) = hash;
                        }
                    }
                }
            });
        }
        eprintln!("quillo: WebTransport on UDP port {}", port);
        Ok(Running { port, hash: RwLock::new(hash), servers: servers.into_iter().map(|(_, s)| s).collect() })
    }

    /// On a clean stop: every session closed with HTTP/3's "no error" and a reason that says the
    /// server is going, so its page connects again at once, as from a WebSocket's close, where
    /// it heard nothing until the keep-alive gave up, 20 s on. Waits, briefly, for the closes to
    /// be sent (FEEDBACK80.md, Q-1265; branch feedback72-81).
    pub fn close_all() {
        let Some(Some(r)) = RUNNING.get() else { return };
        if r.servers.iter().all(|s| s.open_connections() == 0) {
            return;
        }
        for s in &r.servers {
            s.close(GOING.into(), b"server stopping");
        }
        let servers = r.servers.clone();
        let _ = crate::pg::runtime().block_on(async move {
            tokio::time::timeout(Duration::from_millis(500), async {
                for s in &servers {
                    s.wait_idle().await;
                }
            })
            .await
        });
    }

    /// H3_NO_ERROR: the connection closed with nothing wrong, a server going away (RFC 9114 §8.1).
    const GOING: u32 = 0x100;

    async fn session(incoming: wtransport::endpoint::IncomingSession) {
        let Ok(req) = incoming.await else { return };
        let path = req.path().to_string();
        let ticket = path.split_once('?').filter(|(p, _)| p.ends_with("/_quillo/wt")).and_then(|(_, q)| ticket_of(q));
        let Some(ticket) = ticket else {
            req.forbidden().await;
            return;
        };
        // A place among the pages that wait, as a WebSocket takes, held for the session: a session
        // took none, so one address could open as many as it liked (FEEDBACK75.md, Q-1074; branch
        // feedback75).
        let ip = req.remote_address().ip().to_string();
        let Some(_place) = crate::server::Waiter::take(&ip) else {
            req.too_many_requests().await;
            return;
        };
        let Ok(conn) = req.accept().await else { return };
        // The page's first stream carries what must arrive (joining, and
        // large or reliable messages), both ways.
        let Ok(Ok(opening)) = tokio::time::timeout(Duration::from_secs(10), conn.accept_bi()).await else { return };
        let (mut out, mut inn) = opening;
        let (media_tx, media_rx) = tokio::sync::mpsc::channel(240);
        let client = crate::statics::client_of(&ip);
        let (peer, mut rx) = connect_from(ticket, Some(media_tx), &client);
        let frames = tokio::spawn(send_frames(conn.clone(), media_rx));
        let reader = {
            let peer = peer.clone();
            tokio::spawn(async move {
                while let Some(buf) = read_frame(&mut inn, &peer.client).await {
                    if let Ok(text) = std::str::from_utf8(&buf) {
                        from_page_waiting(&peer, text, STREAM_WINDOW as usize).await;
                    }
                }
            })
        };
        tokio::pin!(reader);
        // Frames the page sends (`<quillo-media publish>`): a stream of its
        // own per track, each frame after its length. At most `MOST_TRACKS` at once: a stream past
        // them is let go as it comes, where each had a task holding up to 1 MiB (Q-1074).
        let sending = {
            let (conn, peer) = (conn.clone(), peer.clone());
            tokio::spawn(async move {
                let open = Arc::new(AtomicUsize::new(0));
                while let Ok(opening) = conn.accept_uni().await {
                    if open.load(Relaxed) >= MOST_TRACKS {
                        drop(opening);
                        continue;
                    }
                    open.fetch_add(1, Relaxed);
                    let (peer, open) = (peer.clone(), open.clone());
                    tokio::spawn(async move {
                        struct Ends(Arc<AtomicUsize>);
                        impl Drop for Ends {
                            fn drop(&mut self) {
                                self.0.fetch_sub(1, Relaxed);
                            }
                        }
                        let _ends = Ends(open);
                        let mut inn = opening;
                        while let Some(buf) = read_frame(&mut inn, &peer.client).await {
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
                            let _ = conn.send_datagram(json.as_bytes().to_vec());
                        } else {
                            let mut framed = Vec::with_capacity(json.len() + 4);
                            framed.extend_from_slice(&(json.len() as u32).to_be_bytes());
                            framed.extend_from_slice(json.as_bytes());
                            // A page that does not read is let go, not waited on (Q-594).
                            if !matches!(tokio::time::timeout(WRITE_WAIT, out.write_all(&framed)).await, Ok(Ok(_))) {
                                break;
                            }
                        }
                    }
                    Some(Out::Ping | Out::Pong(_) | Out::Media(_)) => {}
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

    /// The most a page's WebTransport stream may be sent before it is read (see `transport`, and
    /// `wait_for_room`, which counts it for a page that waits).
    pub const STREAM_WINDOW: u32 = 1 << 20;

    /// The most streams a page's session sends frames on at once: a track each (video and sound
    /// of a stream or two). With quinn's 100 a session, each read into a frame of up to 1 MiB with
    /// no deadline, a session held about 100 MiB (FEEDBACK75.md, Q-1074; branch feedback75).
    const MOST_TRACKS: usize = 8;

    /// The next frame on a page's stream, after its length: none when the stream ends, the length
    /// is past `MAX_MESSAGE`, the frame stops coming on (see `ws::Hold::by`), or all the messages
    /// begun, or its client's, hold what `BEGUN` allows. A frame of `ws::SMALL` bytes or less is
    /// read as it comes, with no clock or count, as a WebSocket's (Q-1073's review). A larger one
    /// is made room for as it comes, each growth counted first, where its whole length was made
    /// before any of it came (FEEDBACK75.md, Q-1074, as Q-1073 for a WebSocket; branch feedback75).
    async fn read_frame(inn: &mut wtransport::RecvStream, client: &str) -> Option<Vec<u8>> {
        struct Counted<'a>(usize, &'a str);
        impl Drop for Counted<'_> {
            fn drop(&mut self) {
                BEGUN.give(self.0, self.1);
            }
        }
        let mut len = [0u8; 4];
        inn.read_exact(&mut len).await.ok()?;
        let n = u32::from_be_bytes(len) as usize;
        if n > MAX_MESSAGE {
            return None;
        }
        if n <= crate::ws::SMALL {
            let mut buf = vec![0u8; n];
            inn.read_exact(&mut buf).await.ok()?;
            return Some(buf);
        }
        let since = std::time::Instant::now();
        let mut counted = Counted(0, client);
        let mut buf: Vec<u8> = Vec::new();
        let mut piece = vec![0u8; crate::ws::PIECE];
        while buf.len() < n {
            let want = (n - buf.len()).min(crate::ws::PIECE);
            let by = tokio::time::Instant::from_std(BEGUN.by(since, buf.len()));
            let got = tokio::time::timeout_at(by, inn.read(&mut piece[..want])).await.ok()?.ok()??;
            if buf.len() + got > buf.capacity() {
                let to = (buf.capacity() * 2).max(buf.len() + got).min(n);
                if !BEGUN.take(to - buf.capacity(), client) {
                    return None;
                }
                counted.0 += to - buf.capacity();
                buf.reserve_exact(to - buf.len());
            }
            buf.extend_from_slice(&piece[..got]);
        }
        Some(buf)
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

/// The runtime pages' WebSockets run on: a page is two tasks on it, where it was three threads,
/// each with memory of its own, half a megabyte a page, most of it kept after the page closed
/// (FEEDBACK66.md, Q-954; branch q954). Its own runtime, not the one shared connections and
/// WebTransport use, whose one thread a burst to a thousand pages would hold from the database's
/// answers. One thread: with four, each woke the others to look for work, and an answer to a page
/// cost 16% more than with a thread a page, a burst to three pages twice what one thread costs; one
/// writes a burst to a thousand pages as soon. Made when the first page connects, on a thread
/// started here, so that a thread the system will not give is an answer, where tokio's own
/// builder panicked (in the server's event loop) for want of one (found in review); `None` when
/// it cannot be made (no descriptor or thread left for it), and made again for the next page, so
/// that a server out of either for a while hangs up on the pages meanwhile and takes those after.
pub fn pages() -> Option<&'static tokio::runtime::Handle> {
    static RT: OnceLock<tokio::runtime::Handle> = OnceLock::new();
    static MAKING: Mutex<()> = Mutex::new(());
    if let Some(rt) = RT.get() {
        return Some(rt);
    }
    let _making = lock(&MAKING);
    if let Some(rt) = RT.get() {
        return Some(rt);
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
    let handle = rt.handle().clone();
    std::thread::Builder::new().name("quillo-pages".into()).spawn(move || rt.block_on(std::future::pending::<()>())).ok()?;
    Some(RT.get_or_init(|| handle))
}

/// How often a page's WebSocket is pinged, so that a page that went away is noticed.
const PING_EVERY: Duration = Duration::from_secs(20);

/// A page's connection over a WebSocket, its handshake done, as a task on `pages`: what the page
/// sends is read by a second task. What goes to the page is written in one write, up to `BATCH`
/// messages, each written from where it is, nothing copied: a write each made the writer slower
/// than one request broadcasting, and a page reading at full speed lost a third of a burst of
/// 20,000 to a queue that filled (FEEDBACK55.md, Q-806; branch feedback55). It no longer waits a
/// moment for more after a write in a burst, as a page's thread did: waking a thread cost the
/// program sending a system call a message, and waking a task costs none, so the wait (a yield to
/// the runtime, round and round) cost more than it saved, 4% more for a burst to three pages and
/// 1.5% less without it than with threads (FEEDBACK66.md, Q-954; branch q954).
pub async fn over_websocket(stream: tokio::net::TcpStream, ticket: Ticket, client: &str) {
    let (r, mut w) = stream.into_split();
    let (peer, mut rx) = connect_from(ticket, None, client);
    let reading = tokio::spawn(read_page(peer.clone(), r));
    let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + PING_EVERY, PING_EVERY);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut clean = true;
    'page: loop {
        let first = tokio::select! {
            biased;
            o = rx.recv() => o,
            _ = ping.tick() => Some(Out::Ping),
        };
        let Some(mut o) = first else { break };
        loop {
            if matches!(o, Out::Close) {
                break 'page;
            }
            // A page that does not read is let go, not waited on for good (FEEDBACK49.md, Q-594).
            let Ok(second) = rx.try_recv() else {
                // A message alone, as most are, written with nothing made for it.
                if !matches!(tokio::time::timeout(WRITE_WAIT, crate::ws::send_all_async::<1, _>(&mut w, &[frame(&o)])).await, Ok(true)) {
                    clean = false;
                    break 'page;
                }
                break;
            };
            // Made for each batch and let go after it, so that a quiet page holds none; the
            // write's own state too (see `send_batch`).
            let mut batch: Vec<Out> = Vec::with_capacity((rx.len() + 2).min(BATCH));
            batch.push(o);
            let (mut next, mut close) = (Some(second), false);
            while let Some(x) = next.take() {
                if matches!(x, Out::Close) {
                    close = true;
                    break;
                }
                batch.push(x);
                if batch.len() < BATCH {
                    next = rx.try_recv().ok();
                }
            }
            if !matches!(tokio::time::timeout(WRITE_WAIT, Box::pin(send_batch(&mut w, batch))).await, Ok(true)) {
                clean = false;
                break 'page;
            }
            if close {
                break 'page;
            }
            match next.or_else(|| rx.try_recv().ok()) {
                Some(x) => o = x,
                None => break,
            }
        }
    }
    disconnect(&peer);
    reading.abort();
    // A close frame when the last write went through (not in the middle of one that did not),
    // then the connection shut, both ways once the reader has let its half go.
    if clean {
        let mut f = Vec::with_capacity(2);
        crate::ws::frame_into(&mut f, 0x8, &[]);
        let _ = tokio::time::timeout(Duration::from_secs(2), tokio::io::AsyncWriteExt::write_all(&mut w, &f)).await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), tokio::io::AsyncWriteExt::shutdown(&mut w)).await;
}

/// What a page sends on its WebSocket, read as it comes, till it closes, hangs up or sends what a
/// page may not. However the reading ends, a panic too, the page is let go, where one left it
/// counted and ignored (FEEDBACK49.md, Q-593; branch main).
async fn read_page(peer: Arc<Peer>, r: tokio::net::tcp::OwnedReadHalf) {
    struct Ends(Arc<Peer>);
    impl Drop for Ends {
        fn drop(&mut self) {
            self.0.put(Out::Close);
        }
    }
    let _ends = Ends(peer.clone());
    // A message begun comes whole within `MESSAGE_WITHIN`, and what all those begun hold is held
    // to `BEGUN` (FEEDBACK75.md, Q-1073; branch feedback75).
    let mut reader = crate::ws::Reader::holding(tokio::io::BufReader::with_capacity(READ_AHEAD, r), MAX_MESSAGE + 64, &BEGUN, &peer.client);
    loop {
        // A message a hold counts is read by a future of its own, apart from every message's
        // (FEEDBACK75.md, Q-1073; branch feedback75).
        let next = match reader.next_or_held().await {
            Ok(crate::ws::Next::Got(m)) => Ok(m),
            Ok(crate::ws::Next::Held(f)) => Box::pin(reader.held_async(f)).await,
            Err(e) => Err(e),
        };
        match next {
            Ok(crate::ws::Message::Text(t)) => from_page_waiting(&peer, &t, 0).await,
            Ok(crate::ws::Message::Binary(b)) => frame_from_page(&peer, b),
            // RFC 6455 5.5.2 (FEEDBACK16.md, Q-132; branch feedback20-fixes).
            Ok(crate::ws::Message::Ping(data)) => {
                peer.put(Out::Pong(data));
            }
            Ok(crate::ws::Message::Close) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

/// What a page's reader takes from its connection at a time: a message of up to about this much
/// in one read, where its head, its mask and its data were a read each, and one more each message
/// found nothing come (an answer to a page cost 2% more than its thread's blocking reads did). A
/// kilobyte a page, a megabyte for a thousand (FEEDBACK66.md, Q-954; branch q954).
const READ_AHEAD: usize = 1024;

/// Writes the messages in `batch` at once: whether the page is still there.
async fn send_batch(w: &mut tokio::net::tcp::OwnedWriteHalf, batch: Vec<Out>) -> bool {
    let frames: [(u8, &[u8]); BATCH] = std::array::from_fn(|i| batch.get(i).map_or((0, &[][..]), frame));
    crate::ws::send_all_async::<BATCH, _>(w, &frames[..batch.len()]).await
}

/// A message's frame: its opcode and payload. A close is never written so (the writer ends on it,
/// with a close of its own), and is an empty close here.
fn frame(o: &Out) -> (u8, &[u8]) {
    match o {
        Out::Text { json, .. } => (0x1, json.as_bytes()),
        Out::Media(f) => (0x2, &f.wire[..]),
        Out::Ping => (0x9, &[][..]),
        Out::Pong(data) => (0xA, &data[..]),
        Out::Close => (0x8, &[][..]),
    }
}

/// The most messages a page's WebSocket is written at once (see `over_websocket`).
const BATCH: usize = crate::ws::MOST_FRAMES;

/// A frame a page sends to a stream (`<quillo-media publish>`): taken only
/// from a page that joined the stream's channel, `media:<name>` (FEEDBACK6.md,
/// Q-060; branch feedback6-fixes).
pub fn frame_from_page(p: &Arc<Peer>, wire: Vec<u8>) {
    let Some(name) = crate::media::frame_stream(&wire) else { return };
    // The tenant's own stream, as its room is (FEEDBACK51.md, Q-642; branch feedback51-fixes).
    let key = room_key(&format!("media:{}", name), p.tenant.as_deref());
    if !lock(&p.rooms).contains(&key) {
        return;
    }
    crate::media::from_page(&key, wire);
}

/// Every page leaves the room `key`, as if it had not joined: what `media.close` does to the
/// pages sending a stream.
pub fn leave_all(key: &str) {
    let members: Vec<u64> = lock(&hub().rooms).remove(key).map(|m| m.into_iter().collect()).unwrap_or_default();
    let peers = lock(&hub().peers);
    for id in members {
        if let Some(p) = peers.get(&id) {
            lock(&p.rooms).remove(key);
            p.put(Out::Text { json: Arc::from(serde_json::json!(["j", key_channel(key), false]).to_string()), reliable: true });
        }
    }
}

/// Pages connected now, for `--metrics`.
pub fn connected() -> usize {
    lock(&hub().peers).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // An inbox let go while a page pushed to it: what came goes to the inbox made in its place
    // (found in review of FEEDBACK51.md).
    #[test]
    fn what_comes_to_an_inbox_let_go_goes_to_the_new_one() {
        let (p, _rx) = connect(Ticket { session: "{}".into(), tenant: None }, None);
        // Each with its count of what waits in it (changed with FEEDBACK75.md, Q-1056's review:
        // an inbox holds its channel's count; branch feedback75).
        let gone = Arc::new(Inbox { name: "unit-let-go".into(), queue: Mutex::new(VecDeque::new()), arrived: Condvar::new(), delivering: AtomicBool::new(false), closed: AtomicBool::new(true), waiting: AtomicUsize::new(0) });
        // The one made in its place, as `inbox` makes it, with no thread taking what comes to it:
        // another test of this process serving a program (src/mobile.rs) gave each new inbox a
        // thread, which took the message before it was looked for (CI, found in the second review
        // of FEEDBACK55.md's slice 7; branch feedback55).
        let new = Arc::new(Inbox { name: "unit-let-go".into(), queue: Mutex::new(VecDeque::new()), arrived: Condvar::new(), delivering: AtomicBool::new(true), closed: AtomicBool::new(false), waiting: AtomicUsize::new(0) });
        lock(&hub().inboxes).insert("unit-let-go".into(), new.clone());
        gone.push(Event { kind: 'm', peer: p.clone(), value: "1".into() });
        assert!(lock(&gone.queue).is_empty());
        assert!(Arc::ptr_eq(&inbox("unit-let-go"), &new));
        assert_eq!(lock(&new.queue).len(), 1);
        lock(&hub().inboxes).remove("unit-let-go");
        disconnect(&p);
    }
}
