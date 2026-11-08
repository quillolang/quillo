//! The HTTP server: one event loop (kqueue, epoll) on one thread, and each
//! request run as a coroutine.
//!
//! The loop thread reads and parses requests and runs the program on them.
//! A request that only computes runs start to finish without leaving the
//! thread. One that waits (the database, `http.*`, `sleep`: whatever goes
//! through `Interp::unlocked`) pauses its coroutine: the waiting is done on a
//! pool thread, and the loop answers other requests meanwhile, resuming the
//! coroutine when the wait is over. Quillo code is written as if it simply
//! waited, and all of it runs on one thread, as before under the interpreter
//! lock (which the loop holds, and lets go of while it sleeps, or when
//! another thread asks: a reload, `parallel`).
//!
//! Each coroutine has its own stack (16 MB of address space, used as
//! needed, reused from one request to the next). Deep recursion grows it
//! further (`stack::maybe_grow`), as on an ordinary thread.

use crate::server::{Answer, App, HttpRequest, HttpResponse, Worker};
use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token, Waker};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const LISTENER: Token = Token(usize::MAX);
const WAKER: Token = Token(usize::MAX - 1);
/// Largest request line and headers.
const MAX_HEAD: usize = 64 << 10;
const MAX_HEADERS: usize = 100;
/// A kept-alive connection with nothing to do is closed after this long.
const IDLE: Duration = Duration::from_secs(60);
/// A request head still arriving after this long, or a body that sends nothing for this
/// long, is given up on.
const SLOW: Duration = Duration::from_secs(30);
/// A body that keeps coming slower than this, past its first `SLOW`, is given up on: a byte
/// every 25 s held a connection for good (FEEDBACK51.md, Q-672; branch feedback51-fixes).
const MIN_BODY_RATE: u64 = 1024;
/// How long a connection being closed may keep sending (see `linger`).
const LINGER: Duration = Duration::from_secs(2);
/// A request's stack: address space, used only as needed.
const STACK: usize = 16 << 20;
/// Stacks kept for reuse.
const SPARE_STACKS: usize = 256;

/// Set to stop the server gracefully: no new connections, requests being
/// answered are finished (for up to `DRAIN`), then the loops end.
static STOPPING: AtomicBool = AtomicBool::new(false);
/// Every event loop's waker, to tell them when `STOPPING` is set.
static LOOPS: Mutex<Vec<Arc<Waker>>> = Mutex::new(Vec::new());
/// How long requests in progress get to finish when the server stops.
pub const DRAIN: Duration = Duration::from_secs(10);

/// What an event loop is running: when it last resumed a request (ms, 0 when it is waiting).
/// A watchdog reads these, so that a loop computing is seen from outside it (FEEDBACK20.md,
/// Q-162 and Q-163; branch feedback20-fixes).
#[derive(Default)]
struct Busy {
    since: AtomicU64,
    request: AtomicUsize,
    told: AtomicBool,
    over: AtomicBool,
}

/// Each event loop's `Busy`.
static LOOPS_BUSY: Mutex<Vec<Arc<Busy>>> = Mutex::new(Vec::new());
/// The longest a request may compute without letting go, in ms; 0 for no limit.
static MAX_COMPUTE: AtomicU64 = AtomicU64::new(0);
/// Whether a request somewhere has passed it: read at each step of a loop, so it stays one load.
pub static OVER_TIME: AtomicBool = AtomicBool::new(false);
/// What each request in progress is, for the log.
static RUNNING: Mutex<Option<HashMap<(usize, usize), String>>> = Mutex::new(None);
/// An instance computing for this long is logged.
const STUCK: Duration = Duration::from_secs(5);
/// An instance computing for this long gets no new connections, while another is free.
/// Short, as passing one by costs nothing, and a connection put on one that goes on
/// computing waits as long as it does.
const TOO_BUSY: Duration = Duration::from_millis(50);

thread_local! {
    static OWN_BUSY: std::cell::RefCell<Option<Arc<Busy>>> = const { std::cell::RefCell::new(None) };
    static WAITING: Cell<usize> = const { Cell::new(0) };
}

fn clock() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// Whether others are waiting behind the request starting here.
pub fn others_waiting() -> bool {
    WAITING.with(|c| c.get()) > 0
}

/// `--max-compute`.
pub fn limit_compute(ms: u64) {
    MAX_COMPUTE.store(ms, SeqCst);
}

/// The policy file the compute limit came from, when it was not `--max-compute`.
static COMPUTE_FROM: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The limit is a policy's, said in the message of a request it ends (FEEDBACK55.md, Q-849;
/// branch feedback55).
pub fn compute_limit_from(file: &std::path::Path) {
    let _ = COMPUTE_FROM.set(file.display().to_string());
}

/// How every message of a request ended for computing too long begins, wherever its limit came
/// from: what tells that stop from an error, which `??` may take and an endpoint answers 400
/// (FEEDBACK55.md, Q-849; branch feedback55).
pub const COMPUTE_STOP: &str = "this request computed for longer than ";

/// Whether `msg` is the stop of a request that computed too long (see `COMPUTE_STOP`).
pub fn is_compute_stop(msg: &str) -> bool {
    msg.contains(COMPUTE_STOP)
}

/// Why a request was ended for computing too long: the option, or the policy file, that set it.
pub fn over_compute() -> String {
    match COMPUTE_FROM.get() {
        Some(f) => format!("{}the policy {} allows (max_compute)", COMPUTE_STOP, f),
        None => format!("{}--max-compute allows", COMPUTE_STOP),
    }
}

/// `over_compute()` as a page is told it: the policy's file is the server's to know, and said in
/// its log only.
pub fn over_compute_said_to_pages() -> String {
    match COMPUTE_FROM.get() {
        Some(_) => format!("{}the server's policy allows (max_compute)", COMPUTE_STOP),
        None => over_compute(),
    }
}

/// The limit now, in milliseconds; 0 for none.
pub fn compute_limit_ms() -> u64 {
    MAX_COMPUTE.load(SeqCst)
}

/// Whether requests are limited. Compiled code has no steps to look between, so it is
/// left out then.
pub fn compute_limited() -> bool {
    MAX_COMPUTE.load(std::sync::atomic::Ordering::Relaxed) != 0
}

/// Whether the request running here has passed `--max-compute`; it asks once.
pub fn over_time() -> bool {
    let over = OWN_BUSY.with(|b| b.borrow().as_ref().is_some_and(|b| b.over.swap(false, SeqCst)));
    OVER_TIME.store(lock(&LOOPS_BUSY).iter().any(|b| b.over.load(SeqCst)), SeqCst);
    over
}

/// A request's number is its event loop's, so the two say which it is.
fn own_request() -> (usize, usize) {
    (OWN_BUSY.with(|b| b.borrow().as_ref().map_or(0, |b| Arc::as_ptr(b) as usize)), request_number())
}

/// Remembers what this request is, until `request_done`.
pub fn request_began(what: String) {
    lock(&RUNNING).get_or_insert_with(HashMap::new).insert(own_request(), what);
}

/// See `request_began`.
pub fn request_done() {
    if let Some(m) = lock(&RUNNING).as_mut() {
        m.remove(&own_request());
    }
}

/// A line of the log for an error: with the request it belongs to, so that an id a visitor
/// reports leads to it, and as an object where the log is objects (FEEDBACK21.md, Q-175;
/// branch feedback20-fixes).
pub fn error_line(text: &str) {
    error_line_with(text, &[]);
}

/// `error_line` for an error and the calls it came through. What the error says may hold what a
/// visitor sent: its line breaks and control characters are written as `\n` and `\x1b`, so it
/// cannot add lines of its own to the log or reach a terminal (FEEDBACK31.md, Q-378; branch
/// feedback27-35-fixes).
/// A line of the log for what is no error of the program's but wants a look: a request no
/// route's pattern took, and why (FEEDBACK47.md, Q-544; branch feedback40-43).
pub fn warning_line(text: &str) {
    let key = own_request();
    let what = lock(&RUNNING).as_ref().and_then(|m| m.get(&key).cloned());
    if crate::metrics::LOG.load(SeqCst) == crate::metrics::JSON {
        eprintln!("{}", serde_json::json!({ "time": iso_now(), "level": "warning", "request": what, "warning": text }));
        return;
    }
    match what {
        Some(what) => eprintln!("quillo: warning: {}: {}", log_safe(&what), log_safe(text)),
        None => eprintln!("quillo: warning: {}", log_safe(text)),
    }
}

pub fn error_line_with(text: &str, calls: &[String]) {
    let key = own_request();
    let what = lock(&RUNNING).as_ref().and_then(|m| m.get(&key).cloned());
    if crate::metrics::LOG.load(SeqCst) == crate::metrics::JSON {
        // The calls it came through, as a field and not in the text (FEEDBACK21.md, Q-174; branch
        // feedback20-fixes).
        eprintln!("{}", serde_json::json!({ "time": iso_now(), "level": "error", "request": what, "error": text, "calls": calls }));
        return;
    }
    let mut line = match what {
        Some(what) => format!("quillo: error: {}: {}", log_safe(&what), log_safe(text)),
        None => format!("quillo: error: {}", log_safe(text)),
    };
    for c in calls {
        line.push_str("\n  ");
        line.push_str(&log_safe(c));
    }
    eprintln!("{}", line);
}

/// Text for a line of the log: control characters, line breaks among them, as escapes.
pub fn log_safe(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.chars().any(|c| c.is_control() && c != '\t') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push('\t'),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// What is in progress, for a line in the log.
pub fn in_progress() -> Vec<String> {
    lock(&RUNNING).as_ref().map_or_else(Vec::new, |m| m.values().cloned().collect())
}

/// Looks at the event loops from outside: says when one has been computing for a while,
/// and ends a request past `--max-compute`, at its next step. Nothing is preempted: a
/// request holds no state another could find half done.
fn watch_loops() {
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if crate::debug::on() {
            continue;
        }
        let now = clock();
        let limit = MAX_COMPUTE.load(SeqCst);
        for b in lock(&LOOPS_BUSY).iter() {
            let since = b.since.load(SeqCst);
            if since == 0 {
                b.told.store(false, SeqCst);
                continue;
            }
            let for_ms = now.saturating_sub(since);
            if for_ms >= STUCK.as_millis() as u64 && !b.told.swap(true, SeqCst) {
                let key = (Arc::as_ptr(b) as usize, b.request.load(SeqCst));
                let what = lock(&RUNNING).as_ref().and_then(|m| m.get(&key).cloned());
                // Not `log`: that waits for its loop, and this one is not coming back.
                eprintln!("quillo: an instance has been computing for {} s without letting go: {}", for_ms / 1000, what.as_deref().unwrap_or("a request"));
            }
            if limit != 0 && for_ms >= limit {
                b.over.store(true, SeqCst);
                OVER_TIME.store(true, SeqCst);
            }
        }
    }
}

/// With several instances, connections are taken on a thread that runs no program, so a
/// request computing cannot hold them up, and each goes to the next instance that is not
/// computing, or to the next in turn if all are (FEEDBACK20.md, Q-162; branch feedback20-fixes).
fn take_connections(listener: std::net::TcpListener, inboxes: Vec<Arc<Inbox>>, busy: Vec<Arc<Busy>>) {
    let mut turn = 0usize;
    let free = |b: &Busy| {
        let since = b.since.load(SeqCst);
        since == 0 || clock().saturating_sub(since) < TOO_BUSY.as_millis() as u64
    };
    while !STOPPING.load(SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(true);
                let n = inboxes.len();
                let to = (0..n).map(|k| (turn + k) % n).find(|i| free(&busy[*i])).unwrap_or(turn % n);
                turn = to + 1;
                lock(&inboxes[to].conns).push((TcpStream::from_std(stream), peer));
                let _ = inboxes[to].waker.wake();
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(1)),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => {
                log(format!("quillo: cannot accept a connection: {}\n", e));
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Where a server's connections come from: a TCP port, or something else that makes them and
/// gives them on (a Unix socket's listener, FastCGI's), on a thread of its own.
pub enum Listener {
    Tcp(std::net::TcpListener),
    Other(Box<dyn FnOnce(Hand) + Send>),
}

/// Gives connections made elsewhere to the instances, as `take_connections` does: to the next
/// one not computing.
#[derive(Clone)]
pub struct Hand {
    inboxes: Vec<Arc<Inbox>>,
    busy: Vec<Arc<Busy>>,
    turn: Arc<AtomicUsize>,
}

impl Hand {
    /// A connection, as a `TcpStream` (a local socket's descriptor is one too: reads, writes and
    /// shutting down are the same calls), and who it is from.
    pub fn give(&self, stream: std::net::TcpStream, peer: SocketAddr) {
        let _ = stream.set_nonblocking(true);
        let n = self.inboxes.len();
        let free = |b: &Busy| {
            let since = b.since.load(SeqCst);
            since == 0 || clock().saturating_sub(since) < TOO_BUSY.as_millis() as u64
        };
        let turn = self.turn.load(SeqCst);
        let to = (0..n).map(|k| (turn + k) % n).find(|i| free(&self.busy[*i])).unwrap_or(turn % n);
        self.turn.store(to + 1, SeqCst);
        lock(&self.inboxes[to].conns).push((TcpStream::from_std(stream), peer));
        let _ = self.inboxes[to].waker.wake();
    }

    pub fn stopping(&self) -> bool {
        STOPPING.load(SeqCst)
    }
}

/// Connections on a Unix socket (`--listen unix:PATH`), each given on as it is taken
/// (FEEDBACK41.md, Q-479; branch feedback40-43).
#[cfg(unix)]
pub fn unix_connections(listener: std::os::unix::net::UnixListener) -> Box<dyn FnOnce(Hand) + Send> {
    Box::new(move |hand: Hand| {
        use std::os::fd::AsRawFd;
        let local: SocketAddr = ([127, 0, 0, 1], 0).into();
        // Waited on a tenth of a second at a time, so that once the server stops the listener
        // goes at once, as a TCP one does: a server started meanwhile finds the socket free
        // rather than handing its connections to one that is leaving.
        let mut fds = libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        while !hand.stopping() {
            // SAFETY: one live pollfd, for the listener's descriptor, which `listener` keeps open.
            let ready = unsafe { libc::poll(&mut fds, 1, 100) };
            if ready <= 0 {
                continue;
            }
            match listener.accept() {
                Ok((s, _)) => hand.give(local_as_tcp(s), local),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    log(format!("quillo: cannot accept a connection: {}\n", e));
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    })
}

/// A local socket's descriptor, as the `TcpStream` the server reads and writes.
#[cfg(unix)]
pub fn local_as_tcp(s: std::os::unix::net::UnixStream) -> std::net::TcpStream {
    use std::os::fd::{FromRawFd, IntoRawFd};
    // SAFETY: the descriptor is given up by `s`; reads, writes, shutting down and timeouts are
    // the same calls on a local socket as on a TCP one (as src/embed.rs does).
    unsafe { std::net::TcpStream::from_raw_fd(s.into_raw_fd()) }
}

/// Stops the server gracefully (see `STOPPING`).
pub fn stop() {
    STOPPING.store(true, SeqCst);
    for w in lock(&LOOPS).iter() {
        let _ = w.wake();
    }
}

/// Whether the server is stopping.
pub fn stopping() -> bool {
    STOPPING.load(SeqCst)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Work a paused request hands to the pool.
type Job = Box<dyn FnOnce() + Send>;

/// What a paused request waits for: blocking work for a pool thread, or a
/// future for the async runtime (the shared Postgres connections).
enum Wait {
    Run(Job),
    Spawn(std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>),
    /// Functions to run as coroutines of their own (`parallel`); the
    /// request carries on when all of them have ended.
    Children(Vec<Box<dyn FnOnce()>>),
}

/// How a coroutine ends.
enum End {
    Request(Done),
    Child,
}

type Task = Coroutine<(), Wait, End, DefaultStack>;

/// What a coroutine belongs to.
enum Owner {
    Conn(usize),
    Parent(usize),
}

/// A child's stack: smaller than a request's (it grows when needed).
const CHILD_STACK: usize = 512 << 10;
const SPARE_CHILD_STACKS: usize = 1024;

/// What a request's coroutine gives back when it ends.
struct Done {
    conn: Box<Conn>,
    worker: Box<Worker>,
    fate: Fate,
}

struct Running {
    task: Task,
    /// Where its stack ends now (it may have grown).
    limit: usize,
    owner: Owner,
    /// Children still running (`parallel`).
    children: usize,
    /// The request it works for (see `request_number`).
    request: usize,
}

enum Slot {
    Free,
    Idle(Box<Conn>),
    /// Being answered; `again` when something happened to it meanwhile.
    Busy { again: bool },
}

struct Conn {
    stream: TcpStream,
    peer: String,
    input: Vec<u8>,
    output: Vec<u8>,
    /// Registered for writability (an answer did not fit at once).
    writing: bool,
    /// Last time anything happened.
    active: Instant,
    /// When the request now arriving began to.
    receiving: Option<Instant>,
    /// Its head is read, and this is its line for the log: its body is arriving.
    body_of: Option<String>,
    /// When the body began to arrive, and how much had come on the connection then.
    body_from: Option<(Instant, u64)>,
    /// Bytes that came on the connection.
    got: u64,
    _place: Place,
    /// The request being answered came over HTTP/1.0, which has no chunks.
    http10: bool,
    /// `100 Continue` was sent for the request now arriving.
    continued: bool,
    /// Close once `output` is written.
    closing: bool,
    /// Written and shut for writing: what still arrives is thrown away
    /// until the client closes (see `linger`).
    lingering: bool,
    /// Requests that arrived (pipelined) but wait to be answered until the
    /// answers before them are written: a client that sends and never reads
    /// cannot have its answers pile up in memory.
    pending: Vec<Item>,
    /// Output has waited to be written since then, the client taking none
    /// of it (`None`: nothing waits). See `sweep`.
    stalled: Option<Instant>,
    /// Reading stopped at `input_cap` with more perhaps waiting: read
    /// again without waiting for a new event.
    full: bool,
    /// A chunked body being received: what is decoded so far (see `dechunk`).
    chunked: Option<Dechunk>,
}

/// Room in a connection's input past the head and the body, for chunk
/// framing and the start of a next request.
const FRAMING: usize = 64 << 10;

/// The most a connection's input holds before it stops reading (until
/// the requests there are taken out).
fn input_cap(max_body: usize) -> usize {
    MAX_HEAD.saturating_add(max_body).saturating_add(FRAMING)
}

/// Answers waiting to be written past which a connection's other requests
/// wait (see `Conn::pending`).
const OUTPUT_HIGH: usize = 256 << 10;

/// A connection given to a thread of its own, with the answers to write on
/// it before anything else (written there, never on the event loop).
pub(crate) type Detached = Box<dyn FnOnce(std::net::TcpStream, Vec<u8>) + Send>;

enum Fate {
    Keep,
    Detach(Detached),
}

/// A complete request, or one that cannot be answered.
enum Item {
    Request { req: HttpRequest, keep_alive: bool, http10: bool },
    Bad(u16, &'static str, String),
}

thread_local! {
    /// The yielder of the request coroutine running on this thread, if any.
    static YIELDER: Cell<usize> = const { Cell::new(0) };
    /// The number of the request that coroutine works for.
    static REQUEST: Cell<usize> = const { Cell::new(0) };
}

/// For `Interp::unlocked`: in a request's coroutine, runs `f` on a pool
/// thread while the server answers other requests, and gives back its
/// result. Elsewhere gives `f` back, for the caller to run.
pub fn wait_elsewhere<T: Send, F: FnOnce() -> T + Send>(f: F) -> Result<T, F> {
    let y = YIELDER.with(|c| c.get());
    // Not while a SQLite transaction is open on this thread (FEEDBACK44.md, Q-579).
    if y == 0 || held() {
        return Err(f);
    }
    // SAFETY: set by the coroutine itself, whose stack holds the yielder.
    let yielder = unsafe { &*(y as *const Yielder<(), Wait>) };
    let mut out: Option<std::thread::Result<T>> = None;
    let at = &mut out as *mut Option<std::thread::Result<T>> as usize;
    let job: Box<dyn FnOnce() + Send + '_> = Box::new(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        // SAFETY: `out` is on the paused coroutine's stack (below), which is
        // resumed only after this has run.
        unsafe { *(at as *mut Option<std::thread::Result<T>>) = Some(r) };
    });
    // SAFETY: the coroutine stays paused until the job has run, so what the
    // job borrows lives long enough.
    let job: Job = unsafe { std::mem::transmute::<Box<dyn FnOnce() + Send + '_>, Job>(job) };
    yielder.suspend(Wait::Run(job));
    YIELDER.with(|c| c.set(y));
    match out.take().expect("the job ran") {
        Ok(v) => Ok(v),
        Err(p) => std::panic::resume_unwind(p),
    }
}

/// In a request's coroutine: runs each function as a coroutine of its own,
/// so that their waits overlap, and returns when all have ended.
pub fn run_children(fs: Vec<Box<dyn FnOnce()>>) {
    let y = YIELDER.with(|c| c.get());
    assert!(y != 0, "run_children outside a request");
    // SAFETY: as in `wait_elsewhere`.
    let yielder = unsafe { &*(y as *const Yielder<(), Wait>) };
    yielder.suspend(Wait::Children(fs));
    YIELDER.with(|c| c.set(y));
}

/// Whether this is a request's coroutine.
pub fn in_task() -> bool {
    YIELDER.with(|c| c.get()) != 0
}

/// In a request's coroutine: a number for the request, the same in its
/// `parallel` calls, and different for requests running at the same time
/// (they count up). The database puts a request's queries on one
/// connection with it (`pg::Lanes`).
pub fn request_number() -> usize {
    REQUEST.with(|c| c.get())
}

thread_local! {
    /// SQLite transactions open on this thread: while one is, nothing here lets the request
    /// pause, as another would run on SQLite's one connection, inside it (FEEDBACK44.md,
    /// Q-579; branch feedback40-43).
    static HELD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A SQLite transaction opens (`true`) or ends here.
pub fn hold_in_place(open: bool) {
    HELD.with(|h| h.set(if open { h.get() + 1 } else { h.get().saturating_sub(1) }));
}

/// Whether a wait must be done in place: a SQLite transaction is open.
pub fn held() -> bool {
    HELD.with(|h| h.get() > 0)
}

/// In a request's coroutine: runs `fut` on the async runtime while the
/// server answers other requests, and gives back its output (`Err` if it
/// failed unexpectedly). `None` outside a coroutine.
pub fn await_elsewhere<T: Send + 'static>(
    fut: std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>,
) -> Option<Result<T, String>> {
    let y = YIELDER.with(|c| c.get());
    // Not while a SQLite transaction is open: the caller waits in place (Q-579).
    if y == 0 || held() {
        return None;
    }
    // SAFETY: as in `wait_elsewhere`.
    let yielder = unsafe { &*(y as *const Yielder<(), Wait>) };
    let mut out: Option<T> = None;
    let at = &mut out as *mut Option<T> as usize;
    let fut = async move {
        let v = fut.await;
        // SAFETY: `out` is on the paused coroutine's stack, resumed only
        // after this has run (or the future has been dropped).
        unsafe { *(at as *mut Option<T>) = Some(v) };
    };
    yielder.suspend(Wait::Spawn(Box::pin(fut)));
    YIELDER.with(|c| c.set(y));
    Some(out.take().ok_or_else(|| "the database call failed unexpectedly".to_string()))
}

/// Threads that do the waiting for paused requests.
struct Pool {
    jobs: Mutex<VecDeque<(usize, Job)>>,
    /// Jobs waiting, readable without the lock.
    queued: AtomicUsize,
    ready: Condvar,
    /// Idle threads asleep, and idle threads still looking for work.
    idle: AtomicUsize,
    spinning: AtomicUsize,
    threads: AtomicUsize,
    max: usize,
    done: Mutex<Vec<usize>>,
    waker: Arc<Waker>,
    /// The loop is (about to be) asleep in `poll`: wake it when a job ends.
    sleeping: AtomicBool,
    /// Jobs submitted and not yet done.
    out: AtomicUsize,
}

impl Pool {
    fn submit(self: &Arc<Self>, id: usize, job: Job) {
        self.out.fetch_add(1, SeqCst);
        lock(&self.jobs).push_back((id, job));
        self.queued.fetch_add(1, SeqCst);
        // A thread looking for work takes it without being woken (waking a
        // sleeping thread costs the loop more than many requests).
        if self.spinning.load(SeqCst) > 0 {
            return;
        }
        if self.idle.load(SeqCst) > 0 {
            self.ready.notify_one();
        } else if self.threads.fetch_add(1, SeqCst) < self.max {
            let pool = self.clone();
            let started = std::thread::Builder::new().name("quillo-wait".into()).spawn(move || {
                pool.work()
            });
            if let Err(e) = started {
                let left = self.threads.fetch_sub(1, SeqCst) - 1;
                // Said once (FEEDBACK41.md, Q-480; branch feedback40-43). With no thread at all,
                // tried again a little later, a few times: another process may have ended. Not
                // run here: the loop would wait with it, and every connection with the loop.
                static SAID: AtomicBool = AtomicBool::new(false);
                if !SAID.swap(true, SeqCst) {
                    eprintln!("quillo: cannot start a thread for waiting work ({}; the account's process limit?); requests wait for the {} threads there are", e, left);
                }
                // Tried again at most once a second, so the loop does not wait each time.
                static TRIED: AtomicU64 = AtomicU64::new(0);
                let now = clock();
                if left == 0 && now.saturating_sub(TRIED.swap(now, SeqCst)) >= 1000 {
                    for wait in [5, 20, 75] {
                        std::thread::sleep(Duration::from_millis(wait));
                        if self.threads.fetch_add(1, SeqCst) >= self.max {
                            self.threads.fetch_sub(1, SeqCst);
                            break;
                        }
                        let pool = self.clone();
                        if std::thread::Builder::new().name("quillo-wait".into()).spawn(move || pool.work()).is_ok() {
                            break;
                        }
                        self.threads.fetch_sub(1, SeqCst);
                    }
                }
            }
        } else {
            self.threads.fetch_sub(1, SeqCst);
        }
    }

    /// A job, looking for one a little while before sleeping (at most two
    /// threads look at once).
    fn next(&self) -> (usize, Job) {
        if self.spinning.fetch_add(1, SeqCst) < 2 {
            let start = Instant::now();
            let mut i = 0u32;
            loop {
                if self.queued.load(SeqCst) > 0 {
                    if let Some(j) = lock(&self.jobs).pop_front() {
                        self.queued.fetch_sub(1, SeqCst);
                        self.spinning.fetch_sub(1, SeqCst);
                        return j;
                    }
                }
                i += 1;
                if i % 64 == 0 {
                    if start.elapsed() > Duration::from_micros(200) {
                        break;
                    }
                    std::thread::yield_now();
                } else {
                    std::hint::spin_loop();
                }
            }
        }
        let mut q = lock(&self.jobs);
        // Counted as idle before it stops spinning, so a job submitted in
        // between is either seen here or wakes it.
        self.idle.fetch_add(1, SeqCst);
        self.spinning.fetch_sub(1, SeqCst);
        loop {
            if let Some(j) = q.pop_front() {
                self.queued.fetch_sub(1, SeqCst);
                self.idle.fetch_sub(1, SeqCst);
                return j;
            }
            q = self.ready.wait(q).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn work(&self) {
        loop {
            let (id, job) = self.next();
            job();
            self.finish(id);
        }
    }

    /// Task `id` can carry on.
    fn finish(&self, id: usize) {
        lock(&self.done).push(id);
        self.out.fetch_sub(1, SeqCst);
        if self.sleeping.load(SeqCst) {
            let _ = self.waker.wake();
        }
    }
}

/// Connections handed to an instance by the one that accepts them
/// (`--instances`), so each instance gets its share.
struct Inbox {
    conns: Mutex<Vec<(TcpStream, SocketAddr)>>,
    waker: Arc<Waker>,
}

/// Marks a task's future done when the future ends, even by panicking.
struct Finish(Arc<Pool>, usize);

impl Drop for Finish {
    fn drop(&mut self) {
        self.0.finish(self.1);
    }
}

/// Serves `app` on `listener`, with up to `max_waiting` requests waiting at
/// once (each wait takes a pool thread). Does not return.
pub fn serve(app: Arc<App>, listener: std::net::TcpListener, max_waiting: usize) -> Result<(), String> {
    serve_instances(vec![app], listener, max_waiting)
}

/// Serves each app (instances of one program) with an event loop of its
/// own, on a thread of its own. The first accepts the connections and
/// hands them to each in turn. Does not return.
pub fn serve_instances(apps: Vec<Arc<App>>, listener: std::net::TcpListener, max_waiting: usize) -> Result<(), String> {
    serve_on(apps, Listener::Tcp(listener), max_waiting)
}

/// `serve_instances`, with connections from `listener`.
pub fn serve_on(apps: Vec<Arc<App>>, listener: Listener, max_waiting: usize) -> Result<(), String> {
    let mut polls = Vec::new();
    let mut inboxes = Vec::new();
    for _ in &apps {
        let poll = Poll::new().map_err(|e| e.to_string())?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKER).map_err(|e| e.to_string())?);
        inboxes.push(Arc::new(Inbox { conns: Mutex::new(Vec::new()), waker }));
        polls.push(poll);
    }
    let mut apps = apps.into_iter().zip(polls).enumerate();
    let (_, (first_app, first_poll)) = apps.next().ok_or("no instances")?;
    let busy: Vec<Arc<Busy>> = inboxes.iter().map(|_| Arc::new(Busy::default())).collect();
    lock(&LOOPS_BUSY).extend(busy.iter().cloned());
    let _ = std::thread::Builder::new().name("quillo-watch".into()).spawn(watch_loops);
    let mut others = Vec::new();
    for (i, (app, poll)) in apps {
        let inbox = inboxes[i].clone();
        others.push(
            std::thread::Builder::new()
                .name("quillo-instance".into())
                .stack_size(crate::server::WORKER_STACK)
                .spawn({
                    let busy = busy[i].clone();
                    move || {
                        if let Err(e) = run_loop(app, poll, None, inbox, busy, max_waiting) {
                            eprintln!("quillo: {}", e);
                        }
                    }
                })
                .map_err(|e| format!("cannot start an instance: {}", e))?,
        );
    }
    let own = inboxes[0].clone();
    let first_busy = busy[0].clone();
    let listener = match listener {
        Listener::Tcp(listener) => {
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            if inboxes.len() > 1 {
                let (taken, all) = (busy, inboxes);
                std::thread::Builder::new().name("quillo-accept".into()).spawn(move || take_connections(listener, all, taken)).map_err(|e| e.to_string())?;
                None
            } else {
                let mut listener = TcpListener::from_std(listener);
                first_poll.registry().register(&mut listener, LISTENER, Interest::READABLE).map_err(|e| e.to_string())?;
                Some(listener)
            }
        }
        Listener::Other(take) => {
            let hand = Hand { inboxes, busy, turn: Arc::new(AtomicUsize::new(0)) };
            std::thread::Builder::new().name("quillo-accept".into()).spawn(move || take(hand)).map_err(|e| e.to_string())?;
            None
        }
    };
    let r = run_loop(first_app, first_poll, listener, own, first_busy, max_waiting);
    // Stopping: the other instances finish their requests too.
    for t in others {
        let _ = t.join();
    }
    r
}

fn run_loop(
    app: Arc<App>,
    poll: Poll,
    listener: Option<TcpListener>,
    inbox: Arc<Inbox>,
    busy: Arc<Busy>,
    max_waiting: usize,
) -> Result<(), String> {
    OWN_BUSY.with(|b| *b.borrow_mut() = Some(busy.clone()));
    let waker = inbox.waker.clone();
    lock(&LOOPS).push(waker.clone());
    let pool = Arc::new(Pool {
        jobs: Mutex::new(VecDeque::new()),
        queued: AtomicUsize::new(0),
        ready: Condvar::new(),
        idle: AtomicUsize::new(0),
        spinning: AtomicUsize::new(0),
        threads: AtomicUsize::new(0),
        max: max_waiting.max(1),
        done: Mutex::new(Vec::new()),
        waker,
        sleeping: AtomicBool::new(false),
        out: AtomicUsize::new(0),
    });
    let mut l = Loop {
        app,
        poll,
        listener,
        inbox,
        busy,
        requests: 0,
        slots: Vec::new(),
        free: Vec::new(),
        ready: VecDeque::new(),
        ended: std::collections::HashSet::new(),
        tasks: Vec::new(),
        free_tasks: Vec::new(),
        workers: Vec::new(),
        stacks: Vec::new(),
        child_stacks: Vec::new(),
        runnable: Vec::new(),
        pool,
        now: Instant::now(),
        chunk: vec![0u8; 16 << 10].into_boxed_slice(),
    };
    l.run()
}

struct Loop {
    app: Arc<App>,
    poll: Poll,
    /// Only the first instance listens.
    listener: Option<TcpListener>,
    /// Connections handed to this instance.
    inbox: Arc<Inbox>,
    /// Every instance's inbox, for the one that listens (empty when alone).
    busy: Arc<Busy>,
    /// Requests started, for their numbers.
    requests: usize,
    slots: Vec<Slot>,
    free: Vec<usize>,
    /// Connections with something to do, oldest first.
    ready: VecDeque<usize>,
    /// Connections whose client has said it sends no more (its FIN, in the event): read to the
    /// end, as with the data that came with it nothing else will say so (FEEDBACK49.md, Q-600;
    /// branch main).
    ended: std::collections::HashSet<usize>,
    tasks: Vec<Option<Running>>,
    free_tasks: Vec<usize>,
    /// Handles on the program for requests to run with, kept for reuse.
    workers: Vec<Box<Worker>>,
    stacks: Vec<DefaultStack>,
    child_stacks: Vec<DefaultStack>,
    /// Tasks whose children have all ended, to resume.
    runnable: Vec<usize>,
    pool: Arc<Pool>,
    /// The time, as of the last wait for events.
    now: Instant,
    /// Where reads land before they join a connection's input.
    chunk: Box<[u8]>,
}

impl Loop {
    fn run(&mut self) -> Result<(), String> {
        let app = self.app.clone();
        let gil = &app.shared_ref().gil;
        gil.acquire();
        let mut events = Events::with_capacity(1024);
        let mut swept = Instant::now();
        let mut draining: Option<Instant> = None;
        crate::value::defer_drops();
        loop {
            let handed = std::mem::take(&mut *lock(&self.inbox.conns));
            for (stream, peer) in handed {
                self.adopt(stream, peer);
            }
            let done = std::mem::take(&mut *lock(&self.pool.done));
            for id in done {
                self.step(id);
            }
            while !self.runnable.is_empty() {
                for id in std::mem::take(&mut self.runnable) {
                    self.step(id);
                }
            }
            let ready = self.ready.len();
            for _ in 0..ready {
                if let Some(i) = self.ready.pop_front() {
                    WAITING.with(|c| c.set(self.ready.len() + self.runnable.len()));
                    self.serve(i);
                }
            }
            WAITING.with(|c| c.set(0));
            // A reload, or threads `parallel` started, are waiting to run.
            if gil.waiting() > 0 {
                gil.hand_over();
            }
            flush_log();
            let later = crate::value::deferred_drops();
            if !later.is_empty() {
                crate::pg::runtime().spawn(async move { drop(later) });
            }
            // Sleep only when there is nothing to do.
            self.pool.sleeping.store(true, SeqCst);
            let idle = self.ready.is_empty()
                && self.runnable.is_empty()
                && lock(&self.pool.done).is_empty()
                && lock(&self.inbox.conns).is_empty();
            let r = if idle {
                gil.release();
                let wait = if draining.is_some() { Duration::from_millis(20) } else { Duration::from_secs(1) };
                let r = self.poll.poll(&mut events, Some(wait));
                gil.acquire();
                r
            } else {
                self.poll.poll(&mut events, Some(Duration::ZERO))
            };
            self.pool.sleeping.store(false, SeqCst);
            self.now = Instant::now();
            if let Err(e) = r {
                if e.kind() != ErrorKind::Interrupted {
                    eprintln!("quillo: waiting for connections failed: {}", e);
                }
                continue;
            }
            let mut accept = false;
            for ev in events.iter() {
                match ev.token() {
                    LISTENER => accept = true,
                    WAKER => {}
                    Token(i) => {
                        if ev.is_read_closed() {
                            self.ended.insert(i);
                        }
                        self.ready.push_back(i)
                    }
                }
            }
            if accept {
                self.accept();
            }
            if swept.elapsed() >= Duration::from_secs(1) {
                self.sweep();
                swept = Instant::now();
            }
            if STOPPING.load(SeqCst) {
                let since = *draining.get_or_insert(self.now);
                if let Some(mut l) = self.listener.take() {
                    let _ = self.poll.registry().deregister(&mut l);
                }
                self.close_idle();
                if self.quiet() || self.now.duration_since(since) >= DRAIN {
                    flush_log();
                    gil.release();
                    return Ok(());
                }
            }
        }
    }

    /// Stopping: closes the connections with nothing in progress (no
    /// request arriving, nothing left to write).
    fn close_idle(&mut self) {
        for i in 0..self.slots.len() {
            let Slot::Idle(c) = &self.slots[i] else { continue };
            if c.receiving.is_none() && c.output.is_empty() {
                if let Slot::Idle(c) = std::mem::replace(&mut self.slots[i], Slot::Free) {
                    self.close(i, c);
                }
            }
        }
    }

    /// Nothing being answered or written any more.
    fn quiet(&self) -> bool {
        self.tasks.iter().all(Option::is_none)
            && self.slots.iter().all(|s| match s {
                Slot::Free => true,
                Slot::Busy { .. } => false,
                Slot::Idle(c) => c.output.is_empty() && c.receiving.is_none(),
            })
    }

    fn accept(&mut self) {
        loop {
            let Some(listener) = &self.listener else { return };
            match listener.accept() {
                Ok((stream, peer)) => self.adopt(stream, peer),
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    // Out of file descriptors, for one: try again when the
                    // next connection arrives.
                    log(format!("quillo: cannot accept a connection: {}\n", e));
                    return;
                }
            }
        }
    }

    /// Takes a new connection into this loop.
    fn adopt(&mut self, mut stream: TcpStream, peer: SocketAddr) {
        let Some(place) = place_for(&peer) else { return };
        let _ = stream.set_nodelay(true);
        let i = self.free.pop().unwrap_or_else(|| {
            self.slots.push(Slot::Free);
            self.slots.len() - 1
        });
        if self.poll.registry().register(&mut stream, Token(i), Interest::READABLE).is_err() {
            self.free.push(i);
            return;
        }
        self.slots[i] = Slot::Idle(Box::new(Conn {
            stream,
            peer: ip_of(&peer),
            input: Vec::new(),
            output: Vec::new(),
            writing: false,
            active: self.now,
            receiving: None,
            body_of: None,
            body_from: None,
            got: 0,
            _place: place,
            http10: false,
            continued: false,
            closing: false,
            lingering: false,
            pending: Vec::new(),
            stalled: None,
            full: false,
            chunked: None,
        }));
        crate::metrics::CONNECTIONS.fetch_add(1, SeqCst);
        // Its first request has often arrived with it.
        self.ready.push_back(i);
    }

    /// Closes connections idle too long, too slow to send a request, or
    /// not reading their answers.
    fn sweep(&mut self) {
        // Handles on a version of the program loaded over: let go, with its
        // database connections (requests still running on it keep theirs).
        let app = self.app.clone();
        self.workers.retain(|w| !w.outdated(&app));
        let now = Instant::now();
        for i in 0..self.slots.len() {
            let Slot::Idle(c) = &self.slots[i] else { continue };
            let stale = match c.receiving {
                _ if c.lingering => now.duration_since(c.active) > LINGER,
                // A head must come whole in time; a body may take as long as it keeps coming,
                // as nginx's `client_body_timeout`: a photo from a phone on a weak link took
                // longer than 30 s and was reset (FEEDBACK36.md, Q-432; branch feedback36-39).
                Some(_) if c.body_of.is_some() => now.duration_since(c.active) > SLOW || too_slow(c, now),
                Some(since) => now.duration_since(since) > SLOW,
                // A client that takes none of its answer for this long
                // would hold the connection and the answer forever.
                None => match c.stalled {
                    Some(since) if !c.output.is_empty() => now.duration_since(since) > SLOW,
                    _ => c.output.is_empty() && now.duration_since(c.active) > IDLE,
                },
            };
            if stale {
                if let Slot::Idle(mut c) = std::mem::replace(&mut self.slots[i], Slot::Free) {
                    // Told why, with a 408, and the log says so when the request was known
                    // (FEEDBACK36.md, Q-432 and Q-446; branch feedback36-39).
                    if c.receiving.is_some() && !c.lingering {
                        if let Some(line) = c.body_of.take() {
                            let why = if too_slow(&c, now) { "the body came at less than 1 KiB/s" } else { "nothing of the body came for 30 s" };
                            log_refused(&line, 408, why);
                        }
                        write_response(&mut c.output, &crate::server::plain_response(408, "request timeout\n"), false, true);
                        if let Ok(true) = flush(&mut c) {
                            self.linger(i, c);
                            continue;
                        }
                    }
                    self.close(i, c);
                }
            }
        }
    }

    fn close(&mut self, i: usize, mut c: Box<Conn>) {
        let _ = self.poll.registry().deregister(&mut c.stream);
        self.ended.remove(&i);
        self.slots[i] = Slot::Free;
        self.free.push(i);
        crate::metrics::CONNECTIONS.fetch_sub(1, SeqCst);
    }

    /// Ends a connection whose answers are all written. Closing it at once
    /// while the client is still sending (a body refused as too large, say)
    /// makes the system reset the connection, and the client may lose the
    /// answer; so it is shut for writing and kept until the client closes
    /// too, throwing away what arrives, for `LINGER` at most.
    fn linger(&mut self, i: usize, mut c: Box<Conn>) {
        if c.stream.shutdown(std::net::Shutdown::Write).is_err() {
            return self.close(i, c);
        }
        c.lingering = true;
        c.active = self.now;
        c.input = Vec::new();
        c.chunked = None;
        c.full = false;
        if c.writing {
            c.writing = false;
            let _ = self.poll.registry().reregister(&mut c.stream, Token(i), Interest::READABLE);
        }
        self.slots[i] = Slot::Idle(c);
        // What already arrived raises no new event.
        self.ready.push_back(i);
    }

    /// Something happened on connection `i`: write, read, and start
    /// answering what has arrived.
    fn serve(&mut self, i: usize) {
        let mut c = match self.slots.get_mut(i) {
            Some(slot @ Slot::Idle(_)) => match std::mem::replace(slot, Slot::Busy { again: false }) {
                Slot::Idle(c) => c,
                _ => unreachable!(),
            },
            Some(Slot::Busy { again }) => {
                *again = true;
                return;
            }
            _ => return,
        };
        if c.lingering {
            loop {
                match c.stream.read(&mut self.chunk) {
                    Ok(0) => return self.close(i, c),
                    Ok(_) => {}
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => return self.close(i, c),
                }
            }
            self.slots[i] = Slot::Idle(c);
            return;
        }
        c.active = self.now;
        if !c.output.is_empty() {
            match flush(&mut c) {
                Err(_) => return self.close(i, c),
                // The client is not reading: read no more from it either.
                Ok(false) => return self.park(i, c),
                Ok(true) if c.closing && c.pending.is_empty() => return self.linger(i, c),
                Ok(true) => {}
            }
        }
        if !c.pending.is_empty() {
            let items = std::mem::take(&mut c.pending);
            return self.start(i, c, items);
        }
        let mut ended = false;
        let told_end = self.ended.remove(&i);
        let chunk = &mut self.chunk;
        let cap = input_cap(self.app.max_body);
        c.full = false;
        loop {
            // No more than a request can take: the rest waits in the socket
            // until the requests read so far are taken out.
            if c.input.len() >= cap {
                c.full = true;
                break;
            }
            match c.stream.read(chunk) {
                Ok(0) => {
                    ended = true;
                    break;
                }
                Ok(n) => {
                    c.got += n as u64;
                    c.input.extend_from_slice(&chunk[..n]);
                    // Less than asked for: that was all there was. (What
                    // arrives later raises a new event, so there is no need
                    // for another read just to be told so, unless the client
                    // has said it sends no more, which raises none: Q-600.)
                    if n < chunk.len() && !told_end {
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return self.close(i, c),
            }
        }
        // Stopped at the cap with the client's end not yet read: kept for the next read, as no
        // event will say it again (Q-600; found in review).
        if told_end && c.full && !ended {
            self.ended.insert(i);
        }
        // A bug in parsing costs this connection, not the server.
        let (max_body, now) = (self.app.max_body, self.now);
        let items = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| take_requests(&mut c, max_body, now))) {
            Ok(items) => items,
            Err(_) => {
                eprintln!("quillo: internal error (a bug in Quillo) while reading a request");
                return self.close(i, c);
            }
        };
        if items.is_empty() {
            // Full, and still no whole request: nothing more can arrive.
            if ended || c.input.len() >= cap {
                return self.close(i, c);
            }
            return self.finish(i, c, Fate::Keep);
        }
        if ended {
            c.closing = true;
        }
        self.start(i, c, items);
    }

    /// Answers `items` in a coroutine of their own.
    fn start(&mut self, i: usize, mut conn: Box<Conn>, items: Vec<Item>) {
        let app = self.app.clone();
        let mut worker = self.workers.pop().unwrap_or_else(|| Box::new(Worker::new(&app)));
        let stack = match self.stacks.pop() {
            Some(s) => s,
            None => match DefaultStack::new(STACK) {
                Ok(s) => s,
                Err(e) => {
                    log(format!("quillo: no stack for a request: {}\n", e));
                    return self.close(i, conn);
                }
            },
        };
        let limit = crate::stack::limit_of(&stack);
        let task = Task::with_stack(stack, move |y: &Yielder<(), Wait>, ()| {
            YIELDER.with(|c| c.set(y as *const Yielder<(), Wait> as usize));
            let fate = answer(&app, &mut worker, &mut conn, items);
            End::Request(Done { conn, worker, fate })
        });
        self.requests = self.requests.wrapping_add(1);
        let id = self.new_task(Running { task, limit, owner: Owner::Conn(i), children: 0, request: self.requests });
        self.step(id);
    }

    fn new_task(&mut self, r: Running) -> usize {
        let id = self.free_tasks.pop().unwrap_or_else(|| {
            self.tasks.push(None);
            self.tasks.len() - 1
        });
        self.tasks[id] = Some(r);
        id
    }

    /// Starts `f` as a child of task `parent`.
    fn start_child(&mut self, parent: usize, f: Box<dyn FnOnce()>) {
        let stack = match self.child_stacks.pop() {
            Some(s) => s,
            None => match DefaultStack::new(CHILD_STACK) {
                Ok(s) => s,
                Err(e) => {
                    // Nothing runs it: its result stays empty, which
                    // `parallel` reports as a failure.
                    log(format!("quillo: no stack for a parallel call: {}\n", e));
                    return self.child_ended(parent);
                }
            },
        };
        let limit = crate::stack::limit_of(&stack);
        let task = Task::with_stack(stack, move |y: &Yielder<(), Wait>, ()| {
            YIELDER.with(|c| c.set(y as *const Yielder<(), Wait> as usize));
            f();
            End::Child
        });
        let request = self.tasks[parent].as_ref().map_or(0, |p| p.request);
        let id = self.new_task(Running { task, limit, owner: Owner::Parent(parent), children: 0, request });
        self.step(id);
    }

    fn child_ended(&mut self, parent: usize) {
        if let Some(p) = self.tasks[parent].as_mut() {
            p.children -= 1;
            if p.children == 0 {
                self.runnable.push(parent);
            }
        }
    }

    /// Runs task `id` until it waits or ends.
    fn step(&mut self, id: usize) {
        let Some(r) = self.tasks[id].as_mut() else { return };
        let outer = crate::stack::switch(r.limit);
        REQUEST.with(|c| c.set(r.request));
        self.busy.request.store(r.request, SeqCst);
        self.busy.since.store(clock(), SeqCst);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.task.resume(())));
        self.busy.since.store(0, SeqCst);
        self.busy.over.store(false, SeqCst);
        r.limit = crate::stack::switch(outer);
        YIELDER.with(|c| c.set(0));
        match res {
            Ok(CoroutineResult::Yield(Wait::Run(job))) => self.pool.submit(id, job),
            Ok(CoroutineResult::Yield(Wait::Spawn(fut))) => {
                self.pool.out.fetch_add(1, SeqCst);
                let done = Finish(self.pool.clone(), id);
                crate::pg::runtime().spawn(async move {
                    let _done = done;
                    fut.await;
                });
            }
            Ok(CoroutineResult::Yield(Wait::Children(fs))) => {
                if let Some(r) = self.tasks[id].as_mut() {
                    r.children = fs.len();
                }
                if fs.is_empty() {
                    self.runnable.push(id);
                }
                for f in fs {
                    self.start_child(id, f);
                }
            }
            Ok(CoroutineResult::Return(end)) => {
                let r = self.tasks[id].take().expect("running");
                self.free_tasks.push(id);
                match (end, r.owner) {
                    (End::Request(done), Owner::Conn(slot)) => {
                        if self.stacks.len() < SPARE_STACKS {
                            self.stacks.push(r.task.into_stack());
                        }
                        self.workers.push(done.worker);
                        self.finish(slot, done.conn, done.fate);
                    }
                    (_, Owner::Parent(parent)) => {
                        if self.child_stacks.len() < SPARE_CHILD_STACKS {
                            self.child_stacks.push(r.task.into_stack());
                        }
                        self.child_ended(parent);
                    }
                    (End::Child, Owner::Conn(_)) => unreachable!("a request ends with its answer"),
                }
            }
            Err(_) => {
                // A bug in the server itself: drop the connection (or end
                // the child), keep serving.
                let r = self.tasks[id].take().expect("running");
                self.free_tasks.push(id);
                eprintln!("quillo: internal error (a bug in Quillo) while answering a request");
                match r.owner {
                    Owner::Conn(slot) => {
                        self.slots[slot] = Slot::Free;
                        self.free.push(slot);
                        crate::metrics::CONNECTIONS.fetch_sub(1, SeqCst);
                    }
                    Owner::Parent(parent) => self.child_ended(parent),
                }
            }
        }
    }

    /// After answering: write, and wait for what comes next.
    fn finish(&mut self, i: usize, mut c: Box<Conn>, fate: Fate) {
        if let Fate::Detach(f) = fate {
            let _ = self.poll.registry().deregister(&mut c.stream);
            self.slots[i] = Slot::Free;
            self.free.push(i);
            crate::metrics::CONNECTIONS.fetch_sub(1, SeqCst);
            let before = std::mem::take(&mut c.output);
            if let Some(stream) = into_std(c.stream) {
                if stream.set_nonblocking(false).is_ok() {
                    // Earlier answers on this connection go first.
                    f(stream, before);
                }
            }
            return;
        }
        match flush(&mut c) {
            Err(_) => self.close(i, c),
            Ok(false) => self.park(i, c),
            Ok(true) if c.closing && c.pending.is_empty() => self.linger(i, c),
            Ok(true) => {
                if c.writing {
                    c.writing = false;
                    let _ = self.poll.registry().reregister(&mut c.stream, Token(i), Interest::READABLE);
                }
                let again = matches!(self.slots[i], Slot::Busy { again: true });
                // Requests that arrived whole while it was answered, or
                // input left unread when there was no room for it.
                let more = !c.input.is_empty() && c.receiving.is_none() || !c.pending.is_empty() || c.full;
                self.slots[i] = Slot::Idle(c);
                if again || more {
                    self.ready.push_back(i);
                }
            }
        }
    }

    /// The answer did not fit in the socket: carry on when it can take more.
    fn park(&mut self, i: usize, mut c: Box<Conn>) {
        if !c.writing {
            c.writing = true;
            let _ = self.poll.registry().reregister(&mut c.stream, Token(i), Interest::READABLE | Interest::WRITABLE);
        }
        let again = matches!(self.slots[i], Slot::Busy { again: true });
        self.slots[i] = Slot::Idle(c);
        if again {
            self.ready.push_back(i);
        }
    }
}

/// Whether a body, past its first `SLOW`, has come slower than `MIN_BODY_RATE` (Q-672).
fn too_slow(c: &Conn, now: Instant) -> bool {
    c.body_from.is_some_and(|(at, from)| {
        let t = now.duration_since(at);
        t > SLOW && c.got - from < MIN_BODY_RATE * t.as_secs()
    })
}

/// Takes the complete requests out of the connection's input.
fn take_requests(c: &mut Conn, max_body: usize, now: Instant) -> Vec<Item> {
    let mut items = Vec::new();
    while !c.input.is_empty() {
        match take_one(&mut c.input, &mut c.chunked, &c.peer, max_body) {
            Taken::Need { expects_continue, body } => {
                c.receiving.get_or_insert(now);
                if body && c.body_of.is_none() {
                    c.body_of = Some(request_line(&c.input));
                    c.body_from = Some((now, c.got));
                }
                if expects_continue && !c.continued {
                    c.continued = true;
                    c.output.extend_from_slice(b"HTTP/1.1 100 Continue\r\n\r\n");
                }
                break;
            }
            Taken::Item(item @ Item::Bad(..)) => {
                items.push(item);
                break;
            }
            Taken::Item(item) => {
                c.receiving = None;
                c.body_of = None;
                c.body_from = None;
                c.continued = false;
                items.push(item);
            }
        }
    }
    items
}

enum Taken {
    Need { expects_continue: bool, body: bool },
    Item(Item),
}

/// A request's first line as the log writes it: at most 200 bytes, a launch token masked.
fn request_line(input: &[u8]) -> String {
    let line = input.split(|b| *b == b'\r').next().unwrap_or(&[]);
    let line = String::from_utf8_lossy(&line[..line.len().min(200)]).into_owned();
    match line.split_once(' ') {
        Some((method, rest)) => {
            let (target, version) = rest.split_once(' ').unwrap_or((rest, ""));
            let sep = if version.is_empty() { "" } else { " " };
            format!("{} {}{}{}", method, crate::server::log_url(target), sep, version)
        }
        None => line,
    }
}

/// Logs a request refused before it reached the program.
fn log_refused(line: &str, status: u16, why: &str) {
    // As it came, but for what would reach a terminal; an object where the log is
    // objects (FEEDBACK31.md, Q-378; branch feedback27-35-fixes).
    if crate::metrics::LOG.load(SeqCst) == crate::metrics::JSON {
        log(format!("{}\n", serde_json::json!({ "time": iso_now(), "status": status, "error": why, "line": line })));
    } else {
        log(format!("{} -> {} ({})\n", log_safe(line), status, why));
    }
}

/// The next request in `input`, taken out of it; or what is still needed,
/// with what is decoded so far of a chunked body kept in `chunked`; or why
/// the input is refused (then it is all dropped).
fn take_one(input: &mut Vec<u8>, chunked: &mut Option<Dechunk>, peer: &str, max_body: usize) -> Taken {
    match parse(input, peer, max_body, chunked) {
        Parsed::Need { expects_continue, body } => {
            // A chunked body's bytes already decoded are not kept twice,
            // nor read again.
            if let Some(d) = chunked {
                input.drain(d.head..d.head + d.used);
                d.used = 0;
            }
            Taken::Need { expects_continue, body }
        }
        Parsed::Bad(status, why) => {
            let line = request_line(input);
            input.clear();
            *chunked = None;
            Taken::Item(Item::Bad(status, why, line))
        }
        Parsed::Done { req, len, keep_alive, http10 } => {
            *chunked = None;
            input.drain(..len);
            Taken::Item(Item::Request { req, keep_alive, http10 })
        }
    }
}

/// For the fuzz targets (fuzz/): requests framed from bytes as they come.
#[cfg(fuzzing)]
pub mod fuzzing {
    use super::*;

    /// What a connection's input is taken as: each request (its method,
    /// URL, headers, body and whether the connection is kept), then
    /// "bad" if the rest was refused.
    fn framed(pieces: &[&[u8]], max_body: usize) -> Vec<String> {
        let (mut input, mut chunked, mut out) = (Vec::new(), None, Vec::new());
        for piece in pieces {
            input.extend_from_slice(piece);
            while !input.is_empty() {
                match take_one(&mut input, &mut chunked, "fuzz", max_body) {
                    Taken::Need { .. } => break,
                    Taken::Item(Item::Bad(..)) => {
                        out.push("bad".to_string());
                        return out;
                    }
                    Taken::Item(Item::Request { req, keep_alive, .. }) => {
                        out.push(format!("{} {} {:?} {:?} {}", req.method, req.url, req.headers, req.body, keep_alive));
                        // The answer closes a connection not kept.
                        if !keep_alive {
                            return out;
                        }
                    }
                }
            }
        }
        out
    }

    /// Frames `data` as one read, then as reads cut where `cuts` says (each
    /// byte a piece's length): a request must not be taken otherwise for
    /// coming in pieces, or the server and a proxy in front could disagree
    /// on where one request ends.
    pub fn framing(data: &[u8], cuts: &[u8], max_body: usize) {
        let whole = framed(&[data], max_body);
        let mut pieces = Vec::new();
        let mut rest = data;
        for &c in cuts.iter().cycle().take(if cuts.is_empty() { 0 } else { data.len() }) {
            if rest.is_empty() {
                break;
            }
            let (a, b) = rest.split_at((c as usize + 1).min(rest.len()));
            pieces.push(a);
            rest = b;
        }
        pieces.push(rest);
        let cut = framed(&pieces, max_body);
        assert_eq!(whole, cut, "framed otherwise in pieces of {:?}", pieces.iter().map(|p| p.len()).collect::<Vec<_>>());
    }
}

/// In the request's coroutine: answers each item, in order.
fn answer(app: &Arc<App>, worker: &mut Worker, c: &mut Conn, items: Vec<Item>) -> Fate {
    let mut items = items.into_iter();
    while let Some(item) = items.next() {
        if c.output.len() >= OUTPUT_HIGH {
            c.pending = std::iter::once(item).chain(items).collect();
            return Fate::Keep;
        }
        match item {
            Item::Bad(status, why, line) => {
                log_refused(&line, status, why);
                write_response(&mut c.output, &crate::server::plain_response(status, &format!("{}\n", why)), false, true);
                c.closing = true;
                return Fate::Keep;
            }
            Item::Request { req, keep_alive, http10 } => {
                let head = req.method == "HEAD";
                c.http10 = http10;
                match worker.answer(app, req, Some(&mut ConnSink(c))) {
                    Answer::Streamed { close } => {
                        if close || !keep_alive {
                            c.closing = true;
                            return Fate::Keep;
                        }
                    }
                    Answer::Respond(resp) => {
                        let close = !keep_alive || resp.header("connection").map_or(false, |v| v.eq_ignore_ascii_case("close"));
                        write_response(&mut c.output, &resp, head, close);
                        if close {
                            c.closing = true;
                            return Fate::Keep;
                        }
                    }
                    Answer::Head(resp, len) => {
                        write_head(&mut c.output, &resp, len, !keep_alive);
                        if !keep_alive {
                            c.closing = true;
                            return Fate::Keep;
                        }
                    }
                    Answer::Detach(f) => return Fate::Detach(f),
                }
            }
        }
    }
    Fate::Keep
}

/// A streamed answer's way onto the connection (src/stream.rs): what is
/// sent goes out at once; when the client reads slower than that, the
/// request waits (on a pool thread, as for the database) until it takes more.
struct ConnSink<'a>(&'a mut Conn);

impl crate::stream::Sink for ConnSink<'_> {
    fn chunked(&self) -> bool {
        !self.0.http10
    }

    fn write(&mut self, data: &[u8]) -> bool {
        let c = &mut *self.0;
        c.output.extend_from_slice(data);
        loop {
            match flush(c) {
                Err(_) => return false,
                Ok(true) => return true,
                Ok(false) => {}
            }
            if !writable(&c.stream) {
                return false;
            }
        }
    }
}

/// Waits until the socket takes more (for up to `SLOW`); false if it does not.
fn writable(s: &TcpStream) -> bool {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = s.as_raw_fd();
        let wait = move || {
            let mut p = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
            // SAFETY: one pollfd, of a socket the paused request keeps open.
            unsafe { libc::poll(&mut p, 1, SLOW.as_millis() as libc::c_int) > 0 }
        };
        wait_elsewhere(wait).unwrap_or_else(|f| f())
    }
    #[cfg(not(unix))]
    {
        let _ = s;
        std::thread::sleep(Duration::from_millis(5));
        true
    }
}

/// Writes as much of `output` as the socket takes; `true` when all of it.
fn flush(c: &mut Conn) -> std::io::Result<bool> {
    let mut done = 0;
    while done < c.output.len() {
        match c.stream.write(&c.output[done..]) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => done += n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    c.output.drain(..done);
    // When the client last took some of what waits (see `sweep`).
    c.stalled = match c.stalled {
        _ if c.output.is_empty() => None,
        Some(since) if done == 0 => Some(since),
        _ => Some(Instant::now()),
    };
    Ok(c.output.is_empty())
}

fn ip_of(a: &SocketAddr) -> String {
    a.ip().to_string()
}

/// `--max-connections`: most connections open at once; past it one more is closed as it
/// comes, rather than `accept` failing for everyone once descriptors run out (FEEDBACK51.md,
/// Q-672; branch feedback51-fixes). 0 until the server starts, which sets what its descriptors
/// allow, less some for files and databases.
pub static MAX_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

/// `--connections-per-client`: most one client holds open, an address or an IPv6 /64. This
/// machine and the proxies `--trust-proxy` names, which bring many clients, are not held to it.
pub static CONNECTIONS_PER_CLIENT: AtomicUsize = AtomicUsize::new(512);

/// Connections open, by client, of those held to `CONNECTIONS_PER_CLIENT`.
static PER_CLIENT: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

/// A connection's place among its client's, given back when it is dropped.
struct Place(Option<String>);

impl Drop for Place {
    fn drop(&mut self) {
        let Some(k) = &self.0 else { return };
        let mut all = PER_CLIENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // A place given is counted (see `place_for`): one fewer, and none is no entry.
        let m = all.get_or_insert_with(HashMap::new);
        match m.get(k).copied().unwrap_or(1) {
            1 => m.remove(k),
            n => m.insert(k.clone(), n - 1),
        };
    }
}

/// A place for a connection from `peer`, if the limits leave one.
fn place_for(peer: &SocketAddr) -> Option<Place> {
    let most = MAX_CONNECTIONS.load(SeqCst);
    if most > 0 && crate::metrics::CONNECTIONS.load(SeqCst) as usize >= most {
        refused_connection(&format!("{} connections are open, the most --max-connections allows", most));
        return None;
    }
    let ip = match peer.ip() {
        std::net::IpAddr::V6(a) => a.to_ipv4_mapped().map_or(peer.ip(), std::net::IpAddr::V4),
        v4 => v4,
    };
    if ip.is_loopback() || TRUSTED.get().is_some_and(|nets| trusted(ip, nets)) {
        return Some(Place(None));
    }
    let key = crate::statics::client_of(&ip.to_string());
    let mut all = PER_CLIENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let n = all.get_or_insert_with(HashMap::new).entry(key.clone()).or_insert(0);
    if *n >= CONNECTIONS_PER_CLIENT.load(SeqCst) {
        let n = *n;
        drop(all);
        refused_connection(&format!("{} holds {} connections, the most --connections-per-client allows: behind a proxy, name it with --trust-proxy", key, n));
        return None;
    }
    *n += 1;
    Some(Place(Some(key)))
}

/// Said in the log at most once a minute.
fn refused_connection(why: &str) {
    static SAID: Mutex<Option<Instant>> = Mutex::new(None);
    let mut at = SAID.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if at.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
        *at = Some(Instant::now());
        log(format!("quillo: a connection is closed as it comes: {}\n", why));
    }
}

/// Set on a request that came from a proxy `--trust-proxy` names.
pub const PROXIED: &str = "x-quillo-from-trusted-proxy";

/// `--trust-proxy`: the addresses whose `X-Forwarded-For` is believed. Without it the
/// client is the connection's peer, as anyone can send that header (FEEDBACK24.md,
/// Q-189; branch feedback24-fixes).
static TRUSTED: std::sync::OnceLock<Vec<(std::net::IpAddr, u32)>> = std::sync::OnceLock::new();

/// Whether `--trust-proxy` named any.
pub fn trusts_proxies() -> bool {
    TRUSTED.get().is_some()
}

/// Takes `10.0.0.0/8,127.0.0.1`; false if a part of it is neither.
pub fn trust_proxies(list: &str) -> bool {
    let mut nets = Vec::new();
    for part in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (ip, bits) = part.split_once('/').map_or((part, None), |(i, b)| (i, Some(b)));
        let Ok(ip) = ip.parse::<std::net::IpAddr>() else { return false };
        let most = if ip.is_ipv4() { 32 } else { 128 };
        let bits = match bits {
            None => most,
            Some(b) => match b.parse::<u32>() {
                Ok(b) if b <= most => b,
                _ => return false,
            },
        };
        nets.push((ip, bits));
    }
    !nets.is_empty() && TRUSTED.set(nets).is_ok()
}

fn trusted(ip: std::net::IpAddr, nets: &[(std::net::IpAddr, u32)]) -> bool {
    let ip = match ip {
        std::net::IpAddr::V6(a) => a.to_ipv4_mapped().map_or(ip, std::net::IpAddr::V4),
        v4 => v4,
    };
    nets.iter().any(|(net, bits)| match (ip, net) {
        (std::net::IpAddr::V4(a), std::net::IpAddr::V4(n)) => u32::from(a).checked_shr(32 - bits).unwrap_or(0) == u32::from(*n).checked_shr(32 - bits).unwrap_or(0),
        (std::net::IpAddr::V6(a), std::net::IpAddr::V6(n)) => u128::from(a).checked_shr(128 - bits).unwrap_or(0) == u128::from(*n).checked_shr(128 - bits).unwrap_or(0),
        _ => false,
    })
}

/// Who sent a request: the nearest address in `X-Forwarded-For` that is not one of
/// the proxies, as those to its left are whatever the client wrote.
fn client_of(peer: &str, headers: &[(String, String)]) -> String {
    let Some(nets) = TRUSTED.get() else { return peer.to_string() };
    if !peer.parse().is_ok_and(|ip| trusted(ip, nets)) {
        return peer.to_string();
    }
    let mut client = peer.to_string();
    for (_, v) in headers.iter().rev().filter(|(k, _)| k.eq_ignore_ascii_case("x-forwarded-for")) {
        for hop in v.rsplit(',').map(str::trim) {
            let Ok(ip) = hop.parse::<std::net::IpAddr>() else { return client };
            client = ip.to_string();
            if !trusted(ip, nets) {
                return client;
            }
        }
    }
    client
}

#[cfg(unix)]
fn into_std(s: TcpStream) -> Option<std::net::TcpStream> {
    use std::os::fd::{FromRawFd, IntoRawFd};
    // SAFETY: the descriptor comes from a stream that gives it up.
    Some(unsafe { std::net::TcpStream::from_raw_fd(s.into_raw_fd()) })
}

#[cfg(windows)]
fn into_std(s: TcpStream) -> Option<std::net::TcpStream> {
    use std::os::windows::io::{FromRawSocket, IntoRawSocket};
    // SAFETY: the socket comes from a stream that gives it up.
    Some(unsafe { std::net::TcpStream::from_raw_socket(s.into_raw_socket()) })
}

// ---- requests ------------------------------------------------------------

enum Parsed {
    /// More is needed; `body` when the head is read.
    Need { expects_continue: bool, body: bool },
    Bad(u16, &'static str),
    Done { req: HttpRequest, len: usize, keep_alive: bool, http10: bool },
}

/// `chunked_so_far` is what is decoded of a chunked body arriving in `buf`
/// (made here when it starts).
fn parse(buf: &[u8], peer: &str, max_body: usize, chunked_so_far: &mut Option<Dechunk>) -> Parsed {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut r = httparse::Request::new(&mut headers);
    let head = match r.parse(buf) {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) if buf.len() > MAX_HEAD => return too_large(buf),
        Ok(httparse::Status::Partial) => return Parsed::Need { expects_continue: false, body: false },
        Err(httparse::Error::TooManyHeaders) => return Parsed::Bad(431, "too many request headers"),
        Err(_) => return Parsed::Bad(400, "bad request"),
    };
    if head > MAX_HEAD {
        return too_large(buf);
    }
    let version = r.version.unwrap_or(1);
    let mut hs: Vec<(String, String)> = Vec::with_capacity(r.headers.len());
    let mut length: Option<usize> = None;
    let mut codings: Vec<String> = Vec::new();
    let mut te_named = false;
    let mut connection = String::new();
    let mut expects_continue = false;
    for h in r.headers.iter() {
        let value = String::from_utf8_lossy(h.value).into_owned();
        if h.name.eq_ignore_ascii_case("content-length") {
            // Digits only: a proxy in front may read `+5` or `5 5` otherwise
            // than this server does, and then the two disagree on where the
            // next request starts.
            let digits = value.trim_matches([' ', '\t']);
            match digits.parse::<usize>() {
                Ok(n) if digits.bytes().all(|b| b.is_ascii_digit()) && length.map_or(true, |m| m == n) => length = Some(n),
                _ => return Parsed::Bad(400, "bad content-length"),
            }
        } else if h.name.eq_ignore_ascii_case("transfer-encoding") {
            te_named = true;
            codings.extend(value.to_ascii_lowercase().split(',').map(|t| t.trim_matches([' ', '\t']).to_string()).filter(|t| !t.is_empty()));
        } else if h.name.eq_ignore_ascii_case("connection") {
            connection = value.to_ascii_lowercase();
        } else if h.name.eq_ignore_ascii_case("upgrade") && version == 0 {
            // HTTP/1.0 has no upgrade: ignored, as RFC 9110 §7.8 says (FEEDBACK36.md, Q-445;
            // branch feedback36-39).
            continue;
        } else if h.name.eq_ignore_ascii_case("expect") && value.eq_ignore_ascii_case("100-continue") {
            // HTTP/1.0 has no interim answers: a 1.0 client takes a `100` for the answer, so
            // the expectation is ignored, as RFC 9110 §10.1.1 says (FEEDBACK36.md, Q-434;
            // branch feedback36-39).
            expects_continue = version >= 1;
        } else if h.name.eq_ignore_ascii_case("expect") && version >= 1 {
            // An expectation this server cannot meet (FEEDBACK36.md, Q-446).
            return Parsed::Bad(417, "unknown expectation (only 100-continue is taken)");
        }
        hs.push((h.name.to_string(), value));
    }
    // A body is framed one way only, and only the way both ends of a proxy
    // read alike: `chunked` (last, and alone, since nothing else is decoded)
    // over HTTP/1.1, or a length.
    // A `Transfer-Encoding` that names nothing (`,`, empty) is no framing a proxy and this
    // server read alike: it was read by its length (FEEDBACK51.md, Q-654; branch
    // feedback51-fixes).
    if te_named && codings.is_empty() {
        return Parsed::Bad(400, "transfer-encoding names no coding");
    }
    let chunked = !codings.is_empty();
    if chunked {
        if version == 0 {
            return Parsed::Bad(400, "transfer-encoding is not HTTP/1.0");
        }
        if length.is_some() {
            return Parsed::Bad(400, "both content-length and transfer-encoding");
        }
        if codings.last().map(String::as_str) != Some("chunked") {
            return Parsed::Bad(400, "transfer-encoding does not end with chunked");
        }
        if codings.len() > 1 {
            return Parsed::Bad(501, "unsupported transfer-encoding");
        }
    }
    let keep_alive = match version {
        0 => connection.contains("keep-alive"),
        _ => !connection.contains("close"),
    };
    let (body, len) = if chunked {
        let d = chunked_so_far.get_or_insert_with(|| Dechunk::new(head));
        if d.head != head {
            *d = Dechunk::new(head);
        }
        match dechunk(d, &buf[head..], max_body) {
            Chunked::Need => return Parsed::Need { expects_continue, body: true },
            Chunked::TooLarge => return Parsed::Bad(413, "request body too large"),
            Chunked::Bad => return Parsed::Bad(400, "bad chunked body"),
            Chunked::Done(body, used) => (body, head + used),
        }
    } else {
        let n = length.unwrap_or(0);
        if n > max_body {
            return Parsed::Bad(413, "request body too large");
        }
        if buf.len() < head + n {
            return Parsed::Need { expects_continue, body: true };
        }
        (buf[head..head + n].to_vec(), head + n)
    };
    // One `Host`, a host and a port, as RFC 9112 §3.2 asks: with two, a proxy routing by the first
    // and this server answering for the last disagree about whose site it is. A request in the
    // absolute form names its host in its target (§3.2.2). Methods are case-sensitive: `get` is
    // none this server has (FEEDBACK31.md, Q-393; branch feedback27-35-fixes).
    let method = r.method.unwrap_or("GET");
    if !method.bytes().all(|b| b.is_ascii_uppercase() || b == b'-' || b == b'_') {
        return Parsed::Bad(501, "unknown method (methods are written in capitals)");
    }
    let mut url = r.path.unwrap_or("/").to_string();
    let absolute = ["http://", "https://"].iter().find_map(|s| url.get(..s.len()).filter(|p| p.eq_ignore_ascii_case(s)).map(|p| p.len()));
    if let Some(n) = absolute {
        let rest = url[n..].to_string();
        let (authority, path) = match rest.find(['/', '?']) {
            Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
            None => (rest.clone(), "/".to_string()),
        };
        hs.retain(|(k, _)| !k.eq_ignore_ascii_case("host"));
        hs.push(("host".into(), authority));
        url = if path.starts_with('?') { format!("/{}", path) } else { path };
    }
    let hosts: Vec<&str> = hs.iter().filter(|(k, _)| k.eq_ignore_ascii_case("host")).map(|(_, v)| v.as_str()).collect();
    if hosts.len() > 1 {
        return Parsed::Bad(400, "more than one host header");
    }
    if hosts.is_empty() && version == 1 {
        return Parsed::Bad(400, "no host header");
    }
    if let Some(h) = hosts.first() {
        let ok = !h.is_empty() && h.len() <= 255 && h.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_:[]%~!$&'()*+,;=".contains(&b));
        if !ok {
            return Parsed::Bad(400, "bad host header");
        }
    }
    // A body sent compressed is read as it was before: gzip decoded (with the same limit on what
    // it decodes to, so that a small body cannot grow without end), any other coding refused, as
    // RFC 9110 §8.4 asks, where the route ran on the compressed bytes (FEEDBACK36.md, Q-438;
    // branch feedback36-39).
    let mut body = body;
    if let Some(i) = hs.iter().position(|(k, _)| k.eq_ignore_ascii_case("content-encoding")) {
        let coding = hs[i].1.trim().to_ascii_lowercase();
        match coding.as_str() {
            "identity" | "" => {}
            "gzip" | "x-gzip" => {
                use std::io::Read;
                let mut out = Vec::new();
                let read = flate2::read::MultiGzDecoder::new(&body[..]).take(max_body as u64 + 1).read_to_end(&mut out);
                match read {
                    Err(_) => return Parsed::Bad(400, "the body is not gzip, as its content-encoding says"),
                    Ok(_) if out.len() > max_body => return Parsed::Bad(413, "request body too large (once decompressed)"),
                    Ok(_) => body = out,
                }
            }
            _ => return Parsed::Bad(415, "a body in a coding this server cannot read (it takes gzip)"),
        }
        hs.remove(i);
        let n = body.len().to_string();
        match hs.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("content-length")) {
            Some((_, v)) => *v = n,
            None => hs.push(("content-length".into(), n)),
        }
    }
    // Whether it came through a proxy `--trust-proxy` names: what such a proxy says of the client
    // (the certificate it checked) is believed then only. A client's own such header is dropped
    // (FEEDBACK38.md, Q-462; branch feedback36-39).
    hs.retain(|(k, _)| !k.eq_ignore_ascii_case(PROXIED));
    if TRUSTED.get().is_some_and(|nets| peer.parse().is_ok_and(|ip| trusted(ip, nets))) {
        hs.push((PROXIED.into(), "1".into()));
    }
    let remote = client_of(peer, &hs);
    Parsed::Done {
        req: HttpRequest {
            method: method.to_string(),
            url,
            headers: hs,
            body,
            remote,
        },
        len,
        keep_alive,
        http10: version == 0,
    }
}

/// A head over `MAX_HEAD`: 414 when the request line alone is that long, as RFC 9110 §15.5.15
/// asks, else 431 (FEEDBACK36.md, Q-446; branch feedback36-39).
fn too_large(buf: &[u8]) -> Parsed {
    match buf.windows(2).position(|w| w == b"\r\n") {
        Some(end) if end <= MAX_HEAD => Parsed::Bad(431, "request headers too large"),
        _ => Parsed::Bad(414, "request target too long"),
    }
}

enum Chunked {
    Need,
    TooLarge,
    Bad,
    Done(Vec<u8>, usize),
}

/// Longest chunk-size line.
const MAX_CHUNK_LINE: usize = 1024;
/// Most bytes of chunk extensions and trailers in one body, together.
const MAX_CHUNK_EXTRA: usize = 8 << 10;

/// A chunked body as it arrives: what is decoded so far, and where the
/// decoding is. Kept with the connection between reads, so each byte is
/// looked at once.
struct Dechunk {
    /// Where the body starts in the connection's input (the head's length).
    head: usize,
    /// The body so far.
    body: Vec<u8>,
    /// Bytes after the head already decoded. They are dropped from the
    /// input when it waits for more (see `take_requests`).
    used: usize,
    step: Step,
    /// Bytes of chunk extensions and trailers so far.
    extra: usize,
}

enum Step {
    /// A chunk-size line.
    Size,
    /// The data of a chunk: this many bytes still to come.
    Data(usize),
    /// The line end after a chunk's data.
    DataEnd,
    /// After the last chunk: trailer lines, then an empty line.
    Trailers,
}

impl Dechunk {
    fn new(head: usize) -> Self {
        Dechunk { head, body: Vec::new(), used: 0, step: Step::Size, extra: 0 }
    }
}

/// Goes on decoding a `transfer-encoding: chunked` body from `buf` (all of
/// the input after the head). Done: the data, and how many bytes of `buf` it
/// took.
fn dechunk(d: &mut Dechunk, buf: &[u8], max_body: usize) -> Chunked {
    loop {
        let Some(rest) = buf.get(d.used..) else { return Chunked::Bad };
        match d.step {
            Step::Size => {
                let Some(eol) = find_crlf(rest) else {
                    return if rest.len() > MAX_CHUNK_LINE { Chunked::Bad } else { Chunked::Need };
                };
                let line = &rest[..eol];
                // No control character in it: a proxy that ends the line at a bare LF (or
                // reads a CR or NUL otherwise) saw one request where this server saw two
                // (FEEDBACK51.md, Q-654; branch feedback51-fixes).
                if line.iter().any(|&b| is_ctl(b)) {
                    return Chunked::Bad;
                }
                let size_text = line.split(|b| *b == b';').next().unwrap_or(&[]);
                d.extra += line.len() - size_text.len();
                if eol > MAX_CHUNK_LINE || d.extra > MAX_CHUNK_EXTRA {
                    return Chunked::Bad;
                }
                // Hex digits only (spaces or tabs only before a `;`), at
                // most 16: a size that fits, however large, and that a proxy
                // in front reads as this server does.
                let size_text = if size_text.len() < line.len() {
                    let end = size_text.iter().rposition(|b| !matches!(b, b' ' | b'\t')).map_or(0, |i| i + 1);
                    &size_text[..end]
                } else {
                    size_text
                };
                if size_text.is_empty() || size_text.len() > 16 || !size_text.iter().all(u8::is_ascii_hexdigit) {
                    return Chunked::Bad;
                }
                let size_text = std::str::from_utf8(size_text).unwrap_or("");
                let Ok(size) = usize::from_str_radix(size_text, 16) else { return Chunked::Bad };
                d.used += eol + 2;
                d.step = if size == 0 {
                    Step::Trailers
                } else if size > max_body.saturating_sub(d.body.len()) {
                    return Chunked::TooLarge;
                } else {
                    Step::Data(size)
                };
            }
            Step::Data(left) => {
                let n = left.min(rest.len());
                d.body.extend_from_slice(&rest[..n]);
                d.used += n;
                if n < left {
                    d.step = Step::Data(left - n);
                    return Chunked::Need;
                }
                d.step = Step::DataEnd;
            }
            Step::DataEnd => match rest.get(..2) {
                None => return Chunked::Need,
                Some(b"\r\n") => {
                    d.used += 2;
                    d.step = Step::Size;
                }
                Some(_) => return Chunked::Bad,
            },
            Step::Trailers => {
                let Some(eol) = find_crlf(rest) else {
                    return if d.extra + rest.len() > MAX_CHUNK_EXTRA { Chunked::Bad } else { Chunked::Need };
                };
                if rest[..eol].iter().any(|&b| is_ctl(b)) {
                    return Chunked::Bad;
                }
                d.used += eol + 2;
                if eol == 0 {
                    return Chunked::Done(std::mem::take(&mut d.body), d.used);
                }
                d.extra += eol + 2;
                if d.extra > MAX_CHUNK_EXTRA {
                    return Chunked::Bad;
                }
            }
        }
    }
}

/// A control character, which a chunk's line or a trailer may not hold (but a tab).
fn is_ctl(b: u8) -> bool {
    b < 0x20 && b != b'\t' || b == 0x7f
}

fn find_crlf(b: &[u8]) -> Option<usize> {
    b.windows(2).position(|w| w == b"\r\n")
}

// ---- answers -------------------------------------------------------------

/// The answer as HTTP/1.1 bytes, appended to `out`. The program's own
/// `content-length`, `transfer-encoding`, `connection` and `date` are
/// replaced by the server's.
pub(crate) fn write_response(out: &mut Vec<u8>, resp: &HttpResponse, head: bool, close: bool) {
    let bodyless = write_head(out, resp, resp.body.len() as u64, close);
    if !head && !bodyless {
        out.extend_from_slice(&resp.body);
    }
}

/// The status line and headers for a body `len` long (written by the
/// caller, if at all); whether the status has no body.
pub(crate) fn write_head(out: &mut Vec<u8>, resp: &HttpResponse, len: u64, close: bool) -> bool {
    write_head_as(out, resp, Some(len), close)
}

/// The head of an answer sent as it is made: in chunks, or, to an HTTP/1.0 client, which
/// cannot read chunks, with no length and the connection closed at its end, as 1.0 frames
/// it (FEEDBACK36.md, Q-434; branch feedback36-39).
pub(crate) fn write_stream_head(out: &mut Vec<u8>, resp: &HttpResponse, chunked: bool) {
    write_head_as(out, resp, if chunked { None } else { Some(u64::MAX) }, !chunked);
}

/// With `len` None, the body is chunked; `u64::MAX`, it is as long as the connection.
fn write_head_as(out: &mut Vec<u8>, resp: &HttpResponse, len: Option<u64>, close: bool) -> bool {
    out.extend_from_slice(b"HTTP/1.1 ");
    push_num(out, resp.status as u64);
    out.push(b' ');
    out.extend_from_slice(reason(resp.status).as_bytes());
    out.extend_from_slice(b"\r\n");
    let mut server = false;
    for (k, v) in &resp.headers {
        let skip = ["content-length", "transfer-encoding", "connection", "date"].iter().any(|h| k.eq_ignore_ascii_case(h));
        // Nor one the program named `null`, `server: null` taking Quillo's away too (FEEDBACK55.md,
        // Q-710; branch feedback55).
        server |= k.eq_ignore_ascii_case("server");
        if skip || v == crate::server::DROPPED {
            continue;
        }
        if !crate::server::valid_header(k, v) {
            log(format!("quillo: header `{}` not sent: its name or value has characters headers cannot have\n", k.escape_debug()));
            continue;
        }
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(v.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    let bodyless = resp.status < 200 || resp.status == 204 || resp.status == 304;
    // A 205 "must not generate content" (RFC 9110 §15.3.6): it says so, and its body is not
    // sent (FEEDBACK36.md, Q-446; branch feedback36-39).
    if resp.status == 205 {
        out.extend_from_slice(b"content-length: 0\r\n");
    } else if !bodyless {
        match len {
            Some(u64::MAX) => {}
            Some(len) => {
                out.extend_from_slice(b"content-length: ");
                push_num(out, len);
                out.extend_from_slice(b"\r\n");
            }
            None => out.extend_from_slice(b"transfer-encoding: chunked\r\n"),
        }
    }
    if !server {
        out.extend_from_slice(b"server: Quillo\r\n");
    }
    out.extend_from_slice(b"date: ");
    DATE.with(|d| {
        let mut d = d.borrow_mut();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        if d.0 != now {
            *d = (now, http_date(now));
        }
        out.extend_from_slice(d.1.as_bytes());
    });
    out.extend_from_slice(b"\r\n");
    if close {
        out.extend_from_slice(b"connection: close\r\n");
    }
    out.extend_from_slice(b"\r\n");
    bodyless || resp.status == 205
}

thread_local! {
    static DATE: RefCell<(u64, String)> = const { RefCell::new((u64::MAX, String::new())) };
}

fn push_num(out: &mut Vec<u8>, mut n: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&buf[i..]);
}

/// `Thu, 23 Oct 2026 08:44:04 GMT`.
/// The time now as ISO 8601 in UTC, to the millisecond (for `--log json`).
pub(crate) fn iso_now() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = t.as_secs();
    let (year, month, day) = civil((secs / 86400) as i64);
    let (h, m, s) = ((secs % 86400) / 3600, (secs % 3600) / 60, secs % 60);
    format!("{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", year, month, day, h, m, s, t.subsec_millis())
}

/// Year, month and day of a day counted from 1970-01-01 (Howard Hinnant's
/// algorithm).
pub(crate) fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if month <= 2 { 1 } else { 0 }, month, day)
}

pub(crate) fn http_date(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let (h, m, s) = ((secs % 86400) / 3600, (secs % 3600) / 60, secs % 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{}, {:02} {} {} {:02}:{:02}:{:02} GMT", weekday, day, months[(month - 1) as usize], year, h, m, s)
}

/// A status's words, as the status line says them; the MCP door says them for a `reply` with no
/// body (FEEDBACK56.md, A.1; branch feedback56).
pub(crate) fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        402 => "Payment Required",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        511 => "Network Authentication Required",
        _ => "",
    }
}

// ---- the request log -----------------------------------------------------

static LOG: Mutex<String> = Mutex::new(String::new());

/// Adds a line to the request log, written out in batches (each line a
/// separate write would cost more than many requests).
pub(crate) fn log(line: String) {
    log_fmt(format_args!("{}", line));
}

/// `log`, writing straight into the log without a string of its own.
pub(crate) fn log_fmt(line: std::fmt::Arguments) {
    use std::fmt::Write as _;
    let mut l = lock(&LOG);
    let _ = l.write_fmt(line);
    if l.len() > 32 << 10 {
        let _ = std::io::stderr().write_all(l.as_bytes());
        l.clear();
    }
}

pub(crate) fn flush_log() {
    let mut l = lock(&LOG);
    if !l.is_empty() {
        let _ = std::io::stderr().write_all(l.as_bytes());
        l.clear();
    }
}

#[cfg(test)]
mod tests {
    // Q-672: a client holds so many connections, all clients so many, this machine and the
    // proxies --trust-proxy names no limit of the first.
    #[test]
    fn connections_have_places() {
        use std::sync::atomic::Ordering::SeqCst;
        let at = |a: &str| a.parse::<std::net::SocketAddr>().unwrap();
        let before = super::CONNECTIONS_PER_CLIENT.swap(2, SeqCst);
        let (a, b) = (super::place_for(&at("203.0.113.7:1000")), super::place_for(&at("203.0.113.7:1001")));
        assert!(a.is_some() && b.is_some());
        assert!(super::place_for(&at("203.0.113.7:1002")).is_none());
        // One of an IPv6 /64 is the same client as another of it.
        let v6 = [super::place_for(&at("[2001:db8:1:2::1]:1")), super::place_for(&at("[2001:db8:1:2::9]:1"))];
        assert!(v6.iter().all(Option::is_some) && super::place_for(&at("[2001:db8:1:2::5]:1")).is_none());
        // A place given back is another's.
        drop(a);
        assert!(super::place_for(&at("203.0.113.7:1003")).is_some());
        for _ in 0..5 {
            assert!(super::place_for(&at("127.0.0.1:1")).is_some());
        }
        super::CONNECTIONS_PER_CLIENT.store(before, SeqCst);
        // All of them: past --max-connections, none.
        let most = super::MAX_CONNECTIONS.swap(1, SeqCst);
        crate::metrics::CONNECTIONS.fetch_add(1, SeqCst);
        assert!(super::place_for(&at("127.0.0.1:2")).is_none());
        crate::metrics::CONNECTIONS.fetch_sub(1, SeqCst);
        super::MAX_CONNECTIONS.store(most, SeqCst);
        drop((b, v6));
    }

    #[test]
    fn dates() {
        assert_eq!(super::http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(super::http_date(1792716244), "Fri, 23 Oct 2026 00:44:04 GMT");
    }

    use super::{dechunk, Chunked, Dechunk};

    fn whole(b: &[u8], max: usize) -> Chunked {
        dechunk(&mut Dechunk::new(0), b, max)
    }

    #[test]
    fn chunked_bodies() {
        let b = b"4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\nX-T: 1\r\n\r\nNEXT";
        match whole(b, 100) {
            Chunked::Done(body, used) => {
                assert_eq!(body, b"Wikipedia");
                assert_eq!(&b[used..], b"NEXT");
            }
            _ => panic!("not parsed"),
        }
        assert!(matches!(whole(b"4\r\nWi", 100), Chunked::Need));
        assert!(matches!(whole(b"ff\r\n", 10), Chunked::TooLarge));
        assert!(matches!(whole(b"zz\r\n", 100), Chunked::Bad));
        // Sizes that would overflow, or are longer than any size can be.
        assert!(matches!(whole(b"1\r\nA\r\nffffffffffffffff\r\nBB", 100), Chunked::TooLarge));
        assert!(matches!(whole(b"00000000000000001\r\nA\r\n", 100), Chunked::Bad));
        // Sizes as the review sent them: a sign, or spaces around.
        for b in [&b"+5\r\nhello\r\n0\r\n\r\n"[..], b" 5 \r\nhello\r\n0\r\n\r\n", b"5 \r\nhello\r\n0\r\n\r\n", b"0x5\r\nhello\r\n0\r\n\r\n"] {
            assert!(matches!(whole(b, 100), Chunked::Bad), "{}", String::from_utf8_lossy(b));
        }
        assert!(matches!(whole(b"5 ;x=1\r\nhello\r\n0\r\n\r\n", 100), Chunked::Done(..)));
    }

    #[test]
    fn chunked_bodies_arriving_a_byte_at_a_time() {
        let b = b"4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\nX-T: 1\r\n\r\nNEXT";
        let mut d = Dechunk::new(0);
        for n in 1..=b.len() {
            match dechunk(&mut d, &b[..n], 100) {
                Chunked::Need => {}
                Chunked::Done(body, used) => {
                    assert_eq!(body, b"Wikipedia");
                    assert_eq!(used, b.len() - 4);
                    return;
                }
                _ => panic!("refused at {}", n),
            }
        }
        panic!("never done");
    }

    #[test]
    fn chunk_extensions_and_trailers_are_limited() {
        let mut long = b"1\r\nA\r\n0\r\n".to_vec();
        for _ in 0..100 {
            long.extend_from_slice(format!("X-T: {}\r\n", "y".repeat(200)).as_bytes());
        }
        assert!(matches!(whole(&long, 100), Chunked::Bad));
        let mut ext = Vec::new();
        for _ in 0..100 {
            ext.extend_from_slice(format!("1;{}\r\nA\r\n", "e".repeat(200)).as_bytes());
        }
        assert!(matches!(whole(&ext, 1000), Chunked::Bad));
    }
}
