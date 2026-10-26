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
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const LISTENER: Token = Token(usize::MAX);
const WAKER: Token = Token(usize::MAX - 1);
/// Largest request line and headers.
const MAX_HEAD: usize = 64 << 10;
const MAX_HEADERS: usize = 100;
/// A kept-alive connection with nothing to do is closed after this long.
const IDLE: Duration = Duration::from_secs(60);
/// A request still arriving after this long is given up on.
const SLOW: Duration = Duration::from_secs(30);
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
const DRAIN: Duration = Duration::from_secs(10);

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
    m.lock().unwrap_or_else(|e| e.into_inner())
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
    Request { req: HttpRequest, keep_alive: bool },
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
    if y == 0 {
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

/// In a request's coroutine: runs `fut` on the async runtime while the
/// server answers other requests, and gives back its output (`Err` if it
/// failed unexpectedly). `None` outside a coroutine.
pub fn await_elsewhere<T: Send + 'static>(
    fut: std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>,
) -> Option<Result<T, String>> {
    let y = YIELDER.with(|c| c.get());
    if y == 0 {
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
                crate::alloc::waiter_thread();
                pool.work()
            });
            if started.is_err() {
                self.threads.fetch_sub(1, SeqCst);
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
            q = self.ready.wait(q).unwrap_or_else(|e| e.into_inner());
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
    let mut others = Vec::new();
    for (i, (app, poll)) in apps {
        let inbox = inboxes[i].clone();
        others.push(
            std::thread::Builder::new()
                .name("quillo-instance".into())
                .stack_size(crate::server::WORKER_STACK)
                .spawn(move || {
                    if let Err(e) = run_loop(app, poll, None, inbox, Vec::new(), i, max_waiting) {
                        eprintln!("quillo: {}", e);
                    }
                })
                .map_err(|e| format!("cannot start an instance: {}", e))?,
        );
    }
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut listener = TcpListener::from_std(listener);
    first_poll.registry().register(&mut listener, LISTENER, Interest::READABLE).map_err(|e| e.to_string())?;
    let own = inboxes[0].clone();
    let all = if inboxes.len() > 1 { inboxes } else { Vec::new() };
    let r = run_loop(first_app, first_poll, Some(listener), own, all, 0, max_waiting);
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
    instances: Vec<Arc<Inbox>>,
    index: usize,
    max_waiting: usize,
) -> Result<(), String> {
    // mimalloc here too (see src/alloc.rs).
    crate::alloc::waiter_thread();
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
        instances,
        index,
        turn: 0,
        requests: 0,
        slots: Vec::new(),
        free: Vec::new(),
        ready: VecDeque::new(),
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
    instances: Vec<Arc<Inbox>>,
    index: usize,
    /// The instance the next connection goes to.
    turn: usize,
    /// Requests started, for their numbers.
    requests: usize,
    slots: Vec<Slot>,
    free: Vec<usize>,
    /// Connections with something to do, oldest first.
    ready: VecDeque<usize>,
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
                    self.serve(i);
                }
            }
            // A reload, or threads `parallel` started, are waiting to run.
            if gil.waiting() > 0 {
                gil.release();
                gil.acquire();
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
                    Token(i) => self.ready.push_back(i),
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
                Ok((stream, peer)) => {
                    // With several instances, each gets the next one in turn.
                    if !self.instances.is_empty() {
                        let to = self.turn % self.instances.len();
                        self.turn += 1;
                        if to != self.index {
                            let inbox = &self.instances[to];
                            lock(&inbox.conns).push((stream, peer));
                            let _ = inbox.waker.wake();
                            continue;
                        }
                    }
                    self.adopt(stream, peer);
                }
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
                Some(since) => now.duration_since(since) > SLOW,
                // A client that takes none of its answer for this long
                // would hold the connection and the answer forever.
                None => match c.stalled {
                    Some(since) if !c.output.is_empty() => now.duration_since(since) > SLOW,
                    _ => c.output.is_empty() && now.duration_since(c.active) > IDLE,
                },
            };
            if stale {
                if let Slot::Idle(c) = std::mem::replace(&mut self.slots[i], Slot::Free) {
                    self.close(i, c);
                }
            }
        }
    }

    fn close(&mut self, i: usize, mut c: Box<Conn>) {
        let _ = self.poll.registry().deregister(&mut c.stream);
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
                    c.input.extend_from_slice(&chunk[..n]);
                    // Less than asked for: that was all there was. (What
                    // arrives later raises a new event, so there is no need
                    // for another read just to be told so.)
                    if n < chunk.len() {
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return self.close(i, c),
            }
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
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.task.resume(())));
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

/// Takes the complete requests out of the connection's input.
fn take_requests(c: &mut Conn, max_body: usize, now: Instant) -> Vec<Item> {
    let mut items = Vec::new();
    while !c.input.is_empty() {
        match parse(&c.input, &c.peer, max_body, &mut c.chunked) {
            Parsed::Need { expects_continue } => {
                c.receiving.get_or_insert(now);
                if expects_continue && !c.continued {
                    c.continued = true;
                    c.output.extend_from_slice(b"HTTP/1.1 100 Continue\r\n\r\n");
                }
                // A chunked body's bytes already decoded are not kept twice,
                // nor read again.
                if let Some(d) = &mut c.chunked {
                    c.input.drain(d.head..d.head + d.used);
                    d.used = 0;
                }
                break;
            }
            Parsed::Bad(status, why) => {
                let line = c.input.split(|b| *b == b'\r').next().unwrap_or(&[]);
                let line = String::from_utf8_lossy(&line[..line.len().min(200)]).into_owned();
                c.input.clear();
                c.chunked = None;
                items.push(Item::Bad(status, why, line));
                break;
            }
            Parsed::Done { req, len, keep_alive } => {
                c.chunked = None;
                c.input.drain(..len);
                c.receiving = None;
                c.continued = false;
                items.push(Item::Request { req, keep_alive });
            }
        }
    }
    items
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
                log(format!("{} -> {} ({})\n", line, status, why));
                write_response(&mut c.output, &crate::server::plain_response(status, &format!("{}\n", why)), false, true);
                c.closing = true;
                return Fate::Keep;
            }
            Item::Request { req, keep_alive } => {
                let head = req.method == "HEAD";
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
    Need { expects_continue: bool },
    Bad(u16, &'static str),
    Done { req: HttpRequest, len: usize, keep_alive: bool },
}

/// `chunked_so_far` is what is decoded of a chunked body arriving in `buf`
/// (made here when it starts).
fn parse(buf: &[u8], peer: &str, max_body: usize, chunked_so_far: &mut Option<Dechunk>) -> Parsed {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut r = httparse::Request::new(&mut headers);
    let head = match r.parse(buf) {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) if buf.len() > MAX_HEAD => return Parsed::Bad(431, "request headers too large"),
        Ok(httparse::Status::Partial) => return Parsed::Need { expects_continue: false },
        Err(httparse::Error::TooManyHeaders) => return Parsed::Bad(431, "too many request headers"),
        Err(_) => return Parsed::Bad(400, "bad request"),
    };
    if head > MAX_HEAD {
        return Parsed::Bad(431, "request headers too large");
    }
    let version = r.version.unwrap_or(1);
    let mut hs: Vec<(String, String)> = Vec::with_capacity(r.headers.len());
    let mut length: Option<usize> = None;
    let mut codings: Vec<String> = Vec::new();
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
            codings.extend(value.to_ascii_lowercase().split(',').map(|t| t.trim_matches([' ', '\t']).to_string()).filter(|t| !t.is_empty()));
        } else if h.name.eq_ignore_ascii_case("connection") {
            connection = value.to_ascii_lowercase();
        } else if h.name.eq_ignore_ascii_case("expect") && value.eq_ignore_ascii_case("100-continue") {
            expects_continue = true;
        }
        hs.push((h.name.to_string(), value));
    }
    // A body is framed one way only, and only the way both ends of a proxy
    // read alike: `chunked` (last, and alone, since nothing else is decoded)
    // over HTTP/1.1, or a length.
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
            Chunked::Need => return Parsed::Need { expects_continue },
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
            return Parsed::Need { expects_continue };
        }
        (buf[head..head + n].to_vec(), head + n)
    };
    Parsed::Done {
        req: HttpRequest {
            method: r.method.unwrap_or("GET").to_ascii_uppercase(),
            url: r.path.unwrap_or("/").to_string(),
            headers: hs,
            body,
            remote: peer.to_string(),
        },
        len,
        keep_alive,
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

/// The head of an answer sent in chunks, as it is made.
pub(crate) fn write_stream_head(out: &mut Vec<u8>, resp: &HttpResponse) {
    write_head_as(out, resp, None, false);
}

/// With `len` None, the body is chunked.
fn write_head_as(out: &mut Vec<u8>, resp: &HttpResponse, len: Option<u64>, close: bool) -> bool {
    out.extend_from_slice(b"HTTP/1.1 ");
    push_num(out, resp.status as u64);
    out.push(b' ');
    out.extend_from_slice(reason(resp.status).as_bytes());
    out.extend_from_slice(b"\r\n");
    let mut server = false;
    for (k, v) in &resp.headers {
        let skip = ["content-length", "transfer-encoding", "connection", "date"].iter().any(|h| k.eq_ignore_ascii_case(h));
        if skip {
            continue;
        }
        if !crate::server::valid_header(k, v) {
            log(format!("quillo: header `{}` not sent: its name or value has characters headers cannot have\n", k.escape_debug()));
            continue;
        }
        server |= k.eq_ignore_ascii_case("server");
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(v.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    let bodyless = resp.status < 200 || resp.status == 204 || resp.status == 304;
    if !bodyless {
        match len {
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
    bodyless
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

fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
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
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
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
    drop(l);
    crate::metrics::flush();
}

#[cfg(test)]
mod tests {
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
