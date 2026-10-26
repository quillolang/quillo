//! State shared by every thread that runs a program.
//!
//! Quillo code runs under one lock (the GIL). A request handler therefore runs
//! atomically, except while it waits on I/O: builtins such as `sleep` and
//! `http.get` let go of the lock while they wait, so other requests can run.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// A lock that is not tied to a guard's lifetime, so a builtin can release
/// it in the middle of a call and take it back afterwards.
pub struct Gil(Baton);

impl Gil {
    /// A new lock, already held by the thread that creates it.
    pub fn new_held() -> Self {
        Gil(Baton::new(true))
    }

    pub fn acquire(&self) {
        self.0.take();
    }

    pub fn release(&self) {
        self.0.give();
    }

    /// Threads waiting for the lock.
    pub fn waiting(&self) -> usize {
        self.0.waiting()
    }
}

/// Something one thread holds at a time (the interpreter lock, the lead of
/// the server's connections), usually given back within microseconds. A
/// thread that wants it tries for a short while first, since waking a
/// sleeping thread takes far longer than that, and only then sleeps. At most
/// a couple of threads try at once, so idle threads do not spin for nothing.
pub struct Baton {
    taken: AtomicBool,
    spinning: AtomicUsize,
    sleeping: AtomicUsize,
    m: Mutex<()>,
    cv: Condvar,
}

/// How long a thread tries before sleeping.
const SPIN: Duration = Duration::from_micros(100);
/// How many threads may try at once.
const SPINNERS: usize = 2;

impl Baton {
    pub const fn new(taken: bool) -> Self {
        Baton {
            taken: AtomicBool::new(taken),
            spinning: AtomicUsize::new(0),
            sleeping: AtomicUsize::new(0),
            m: Mutex::new(()),
            cv: Condvar::new(),
        }
    }

    fn try_take(&self) -> bool {
        !self.taken.load(SeqCst) && self.taken.compare_exchange(false, true, SeqCst, SeqCst).is_ok()
    }

    pub fn take(&self) {
        if self.try_take() {
            return;
        }
        if self.spinning.fetch_add(1, SeqCst) < SPINNERS {
            let start = Instant::now();
            let mut i = 0u32;
            loop {
                if self.try_take() {
                    self.spinning.fetch_sub(1, SeqCst);
                    return;
                }
                i += 1;
                if i % 64 == 0 {
                    if start.elapsed() > SPIN {
                        break;
                    }
                    std::thread::yield_now();
                } else {
                    std::hint::spin_loop();
                }
            }
        }
        self.spinning.fetch_sub(1, SeqCst);
        let mut g = self.m.lock().unwrap_or_else(|e| e.into_inner());
        self.sleeping.fetch_add(1, SeqCst);
        // Given back between the count and the check: taken here.
        while !self.try_take() {
            g = self.cv.wait(g).unwrap_or_else(|e| e.into_inner());
        }
        self.sleeping.fetch_sub(1, SeqCst);
    }

    pub fn give(&self) {
        self.taken.store(false, SeqCst);
        if self.sleeping.load(SeqCst) > 0 {
            let _g = self.m.lock().unwrap_or_else(|e| e.into_inner());
            self.cv.notify_one();
        }
    }

    /// Threads waiting for it, trying or asleep.
    pub fn waiting(&self) -> usize {
        self.spinning.load(SeqCst) + self.sleeping.load(SeqCst)
    }
}

/// Live updates: each topic remembers the last change that touched it.
pub struct Live {
    pub seq: u64,
    versions: HashMap<String, u64>,
}

impl Default for Live {
    /// The count starts at 1: a page that saw 1 knows the state at 1, while
    /// 0 means "I know nothing yet" (asking returns the current count).
    fn default() -> Self {
        Live { seq: 1, versions: HashMap::new() }
    }
}

pub struct Shared {
    pub gil: Gil,
    pub live: Mutex<Live>,
    live_changed: Condvar,
    /// This server, among several serving the same app: sequence numbers
    /// are its own, so pages are told whose they are. New at each start.
    pub id: String,
    /// When running with others (`--register`), changes to tell them about.
    outbox: Mutex<Option<std::sync::mpsc::Sender<Vec<String>>>>,
    /// The other instances of the program in this process (`--instances`).
    peers: Mutex<Vec<std::sync::Weak<Shared>>>,
    /// Where services run that do not run here (`--service-url name=url`).
    pub service_urls: Mutex<std::collections::HashMap<String, String>>,
}

impl Shared {
    pub fn new() -> Self {
        Shared {
            gil: Gil::new_held(),
            live: Mutex::new(Live::default()),
            live_changed: Condvar::new(),
            id: format!("{:08x}", fastrand::u32(..)),
            outbox: Mutex::new(None),
            peers: Mutex::new(Vec::new()),
            service_urls: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// From now on, changes are also sent here, for the other servers.
    pub fn set_outbox(&self, tx: std::sync::mpsc::Sender<Vec<String>>) {
        *self.outbox.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    }

    /// Marks topics as changed and wakes the pages waiting on them; and when
    /// running with other servers, tells them too. Data kept in variables
    /// (`var:` topics) belongs to this server alone, so it stays here.
    pub fn publish(&self, topics: &[String]) {
        self.publish_here(topics);
        let peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        let outbox = self.outbox.lock().unwrap_or_else(|e| e.into_inner());
        if peers.is_empty() && outbox.is_none() {
            return;
        }
        let shared: Vec<String> = topics.iter().filter(|t| !t.contains("var:")).cloned().collect();
        if shared.is_empty() {
            return;
        }
        for p in peers.iter().filter_map(|p| p.upgrade()) {
            p.publish_here(&shared);
        }
        if let Some(tx) = &*outbox {
            let _ = tx.send(shared);
        }
    }

    /// Makes instances of one program tell each other about changes to
    /// shared data (the database, `publish`), as servers in a cluster do.
    pub fn link(all: &[std::sync::Arc<Shared>]) {
        for s in all {
            let others = all.iter().filter(|o| !std::sync::Arc::ptr_eq(o, s)).map(std::sync::Arc::downgrade).collect();
            *s.peers.lock().unwrap_or_else(|e| e.into_inner()) = others;
        }
    }

    /// Marks topics as changed here only: for changes another server made.
    pub fn publish_here(&self, topics: &[String]) {
        if topics.is_empty() {
            return;
        }
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        live.seq += 1;
        let seq = live.seq;
        for t in topics {
            live.versions.insert(t.clone(), seq);
        }
        drop(live);
        self.live_changed.notify_all();
    }

    /// Whether none of `topics` changed after `seq`.
    pub fn unchanged_since(&self, topics: &[String], seq: u64) -> bool {
        if topics.is_empty() {
            return true;
        }
        let live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        topics.iter().all(|t| live.versions.get(t).map_or(true, |v| *v <= seq))
    }

    pub fn live_seq(&self) -> u64 {
        self.live.lock().unwrap_or_else(|e| e.into_inner()).seq
    }

    /// The current sequence number and which of `topics` changed after
    /// `since`, waiting up to `timeout` for one to change. `server` is the
    /// server `since` came from (`None`: this one); a number from another
    /// server (or from before a restart) says nothing about this one, so all
    /// the topics count as changed.
    pub fn wait_live(
        &self,
        since: u64,
        server: Option<&str>,
        topics: &[String],
        timeout: std::time::Duration,
    ) -> (u64, Vec<String>) {
        let changed = |live: &Live| -> Vec<String> {
            topics.iter().filter(|t| live.versions.get(*t).map_or(false, |v| *v > since)).cloned().collect()
        };
        let live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if since > 0 && server.map_or(false, |s| s != self.id) {
            return (live.seq, topics.to_vec());
        }
        if since == 0 || since > live.seq {
            return (live.seq, Vec::new());
        }
        let (live, _) = self
            .live_changed
            .wait_timeout_while(live, timeout, |l| changed(l).is_empty())
            .unwrap_or_else(|e| e.into_inner());
        (live.seq, changed(&live))
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

/// The services a version of the program loaded in this process
/// (src/service.rs), shared by it, its mounted programs and its services:
/// one copy of each. A version loaded again has a table of its own, so
/// requests still running on the old version call the old services.
#[derive(Default)]
pub struct Services {
    /// By name: the file, and the program, which callers fork (its variables
    /// and database are the one copy's).
    pub loaded: Mutex<std::collections::HashMap<String, (std::path::PathBuf, crate::interp::Interp)>>,
    /// Being loaded: a service using one being loaded calls it when it is
    /// there, without loading it twice.
    pub loading: Mutex<std::collections::HashSet<String>>,
}
