//! State shared by every thread that runs a program.
//!
//! Quillo code runs under one lock (the GIL). A request handler therefore runs
//! atomically, except while it waits on I/O: builtins such as `sleep` and
//! `http.get` let go of the lock while they wait, so other requests can run.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex};
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

    /// Gives a waiting thread its turn, then takes the lock back.
    pub fn hand_over(&self) {
        self.0.hand_over();
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
/// How long a thread handing over waits for a waiter to wake and take it.
const HAND_OVER: Duration = Duration::from_millis(10);

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
        let mut g = self.m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.sleeping.fetch_add(1, SeqCst);
        // Given back between the count and the check: taken here.
        while !self.try_take() {
            g = self.cv.wait(g).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        self.sleeping.fetch_sub(1, SeqCst);
    }

    pub fn give(&self) {
        self.taken.store(false, SeqCst);
        if self.sleeping.load(SeqCst) > 0 {
            let _g = self.m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            self.cv.notify_one();
        }
    }

    /// Threads waiting for it, trying or asleep.
    pub fn waiting(&self) -> usize {
        self.spinning.load(SeqCst) + self.sleeping.load(SeqCst)
    }

    /// Gives it up to whoever waits and takes it back after them. A plain
    /// `give` then `take` is over before a sleeping thread has woken, so that
    /// thread could wait for as long as this one stays busy
    /// (FEEDBACK16.md, Q-135; branch feedback20-fixes).
    pub fn hand_over(&self) {
        self.give();
        let start = Instant::now();
        while !self.taken.load(SeqCst) && self.waiting() > 0 && start.elapsed() < HAND_OVER {
            std::thread::yield_now();
        }
        self.take();
    }
}

/// Live updates: each topic remembers the last change that touched it.
pub struct Live {
    pub seq: u64,
    versions: HashMap<String, u64>,
    /// A topic forgotten changed no later than this (see `forget`).
    floor: u64,
    /// The pages waiting, by the topics they follow: pages of one kind follow
    /// the same ones, so a change takes one call to wake them all, and none to
    /// wake those following something else (FEEDBACK24.md, Q-192; branch
    /// feedback24-fixes).
    groups: HashMap<String, (Arc<Group>, usize)>,
    by_topic: HashMap<String, Vec<Arc<Group>>>,
}

/// Pages waiting on the same topics.
pub struct Group {
    key: String,
    topics: Vec<String>,
    changed: Condvar,
}

/// Most topics remembered before those no page follows are forgotten: a topic
/// for each user or thread is the pattern the guide gives.
const MOST_TOPICS: usize = 1 << 16;

impl Live {
    fn changed_after(&self, topic: &str, since: u64) -> bool {
        self.versions.get(topic).map_or(self.floor > since, |v| *v > since)
    }

    /// The groups following `topics`, to wake once the lock is let go.
    fn following(&self, topics: &[&String]) -> Vec<Arc<Group>> {
        let mut woken: Vec<Arc<Group>> = Vec::new();
        for t in topics {
            for g in self.by_topic.get(*t).into_iter().flatten() {
                if !woken.iter().any(|w| Arc::ptr_eq(w, g)) {
                    woken.push(g.clone());
                }
            }
        }
        let pages: usize = woken.iter().map(|g| self.groups.get(&g.key).map_or(0, |(_, n)| *n)).sum();
        crate::metrics::live_woke(pages as u64);
        woken
    }

    /// With many topics, drops those no page follows, which counts them all as
    /// changed at `floor`: a page that asks gets one needless refresh.
    fn forget(&mut self) {
        if self.versions.len() <= MOST_TOPICS {
            return;
        }
        let (by_topic, seq) = (&self.by_topic, self.seq);
        self.versions.retain(|t, _| by_topic.contains_key(t));
        self.floor = seq;
    }
}

impl Default for Live {
    /// The count starts at 1: a page that saw 1 knows the state at 1, while
    /// 0 means "I know nothing yet" (asking returns the current count).
    fn default() -> Self {
        Live { seq: 1, versions: HashMap::new(), floor: 0, groups: HashMap::new(), by_topic: HashMap::new() }
    }
}

pub struct Shared {
    pub gil: Gil,
    pub live: Mutex<Live>,
    /// This server, among several serving the same app: sequence numbers
    /// are its own, so pages are told whose they are. New at each start.
    pub id: String,
    /// When running with others (`--register`), changes to tell them about.
    outbox: Mutex<Option<std::sync::mpsc::Sender<Vec<String>>>>,
    /// The other instances of the program in this process (`--instances`).
    peers: Mutex<Vec<std::sync::Weak<Shared>>>,
    /// Where services run that do not run here (`--service-url name=url`).
    pub service_urls: Mutex<std::collections::HashMap<String, String>>,
    /// Live loaders' runs shared between pages, and the values pages have,
    /// for patches (FEEDBACK2.md, Q-028; branch feedback6-fixes).
    pub live_cache: Mutex<crate::livecache::LiveCache>,
    /// Globals one of whose fields something follows (`world.scores`): only
    /// their writes are compared field by field (FEEDBACK12.md, Q-084; branch
    /// open-fixes).
    pub field_reads: Mutex<std::collections::HashSet<String>>,
}

impl Shared {
    pub fn new() -> Self {
        Shared {
            gil: Gil::new_held(),
            live: Mutex::new(Live::default()),
            id: format!("{:08x}", fastrand::u32(..)),
            outbox: Mutex::new(None),
            peers: Mutex::new(Vec::new()),
            service_urls: Mutex::new(std::collections::HashMap::new()),
            live_cache: Mutex::new(Default::default()),
            field_reads: Mutex::new(Default::default()),
        }
    }

    /// From now on, changes are also sent here, for the other servers.
    pub fn set_outbox(&self, tx: std::sync::mpsc::Sender<Vec<String>>) {
        *self.outbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tx);
    }

    /// Marks topics as changed and wakes the pages waiting on them; and when
    /// running with other servers, tells them too. Data kept in variables
    /// (`var:` topics) belongs to this server alone, so it stays here.
    /// Gives the sequence number the change was given (0 for no topics), so
    /// that a live loader can tell its own writes from others' (FEEDBACK13.md,
    /// Q-090; branch feedback13-fixes).
    pub fn publish(&self, topics: &[String]) -> u64 {
        let made = self.publish_here(topics);
        let peers = self.peers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let outbox = self.outbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if peers.is_empty() && outbox.is_none() {
            return made;
        }
        let shared: Vec<String> = topics.iter().filter(|t| !t.contains("var:")).cloned().collect();
        if shared.is_empty() {
            return made;
        }
        for p in peers.iter().filter_map(|p| p.upgrade()) {
            p.publish_here(&shared);
        }
        if let Some(tx) = &*outbox {
            let _ = tx.send(shared);
        }
        made
    }

    /// Makes instances of one program tell each other about changes to
    /// shared data (the database, `publish`), as servers in a cluster do.
    pub fn link(all: &[std::sync::Arc<Shared>]) {
        for s in all {
            let others = all.iter().filter(|o| !std::sync::Arc::ptr_eq(o, s)).map(std::sync::Arc::downgrade).collect();
            *s.peers.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = others;
        }
    }

    /// Marks topics as changed here only: for changes another server made.
    pub fn publish_here(&self, topics: &[String]) -> u64 {
        if topics.is_empty() {
            return 0;
        }
        let mut live = self.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        live.seq += 1;
        let seq = live.seq;
        for t in topics {
            live.versions.insert(t.clone(), seq);
        }
        live.forget();
        let woken = live.following(&topics.iter().collect::<Vec<_>>());
        drop(live);
        for g in woken {
            g.changed.notify_all();
        }
        seq
    }

    /// Notes that pages (or cached views) here depend on `topics`, so that
    /// other servers' changes to them are taken (see `publish_known`).
    pub fn track(&self, topics: &[String]) {
        if topics.is_empty() {
            return;
        }
        let mut live = self.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for t in topics {
            if !live.versions.contains_key(t) {
                // Version 0 is older than any a page has seen: no change yet.
                let floor = live.floor;
                live.versions.insert(t.clone(), floor);
            }
        }
    }

    /// Another server's changes: marks those of `topics` that this server
    /// already knows (changed here, or depended on), and drops the rest.
    /// Nothing here waits on a topic it has never heard of, and a message
    /// cannot make this server remember topics without end.
    pub fn publish_known(&self, topics: &[String]) {
        let mut live = self.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let known: Vec<&String> = topics.iter().filter(|t| live.versions.contains_key(*t)).collect();
        if known.is_empty() {
            return;
        }
        live.seq += 1;
        let seq = live.seq;
        for t in &known {
            if let Some(v) = live.versions.get_mut(*t) {
                *v = seq;
            }
        }
        let woken = live.following(&known);
        drop(live);
        for g in woken {
            g.changed.notify_all();
        }
    }

    /// Whether none of `topics` changed after `seq`.
    pub fn unchanged_since(&self, topics: &[String], seq: u64) -> bool {
        if topics.is_empty() {
            return true;
        }
        let live = self.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        topics.iter().all(|t| !live.changed_after(t, seq))
    }

    pub fn live_seq(&self) -> u64 {
        self.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner).seq
    }

    /// The current sequence number and which of `topics` changed after
    /// `since`, as `Follower::wait` says it, without waiting.
    pub fn live_now(&self, since: u64, server: Option<&str>, topics: &[String]) -> (u64, Vec<String>) {
        let live = self.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if since > 0 && server.map_or(false, |s| s != self.id) {
            return (live.seq, topics.to_vec());
        }
        if since == 0 || since > live.seq {
            return (live.seq, Vec::new());
        }
        let changed = topics.iter().filter(|t| live.changed_after(t, since)).cloned().collect();
        (live.seq, changed)
    }

    /// A page that will wait for changes to `topics`, for as long as this lives.
    pub fn follow(self: &Arc<Self>, topics: Vec<String>) -> Follower {
        let key = topics.join("\n");
        let mut live = self.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let live = &mut *live;
        if let Some((g, n)) = live.groups.get_mut(&key) {
            *n += 1;
            return Follower { shared: self.clone(), group: g.clone() };
        }
        let group = Arc::new(Group { key: key.clone(), topics, changed: Condvar::new() });
        for t in &group.topics {
            live.by_topic.entry(t.clone()).or_default().push(group.clone());
        }
        live.groups.insert(key, (group.clone(), 1));
        Follower { shared: self.clone(), group }
    }
}

/// A place among the pages waiting for changes.
pub struct Follower {
    shared: Arc<Shared>,
    group: Arc<Group>,
}

impl Follower {
    /// `Shared::live_now`, waiting up to `timeout` for a topic to change. `server`
    /// is the server `since` came from (`None`: this one); a number from another
    /// server (or from before a restart) says nothing about this one, so all the
    /// topics count as changed.
    pub fn wait(&self, since: u64, server: Option<&str>, timeout: std::time::Duration) -> (u64, Vec<String>) {
        let g = &self.group;
        let changed = |live: &Live| -> Vec<String> { g.topics.iter().filter(|t| live.changed_after(t, since)).cloned().collect() };
        let live = self.shared.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if since > 0 && server.map_or(false, |s| s != self.shared.id) {
            return (live.seq, g.topics.clone());
        }
        if since == 0 || since > live.seq {
            return (live.seq, Vec::new());
        }
        let (live, _) = g.changed.wait_timeout_while(live, timeout, |l| changed(l).is_empty()).unwrap_or_else(std::sync::PoisonError::into_inner);
        (live.seq, changed(&live))
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        let mut live = self.shared.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let live = &mut *live;
        let g = &self.group;
        let Some((_, n)) = live.groups.get_mut(&g.key) else { return };
        *n -= 1;
        if *n > 0 {
            return;
        }
        live.groups.remove(&g.key);
        for t in &g.topics {
            if let Some(gs) = live.by_topic.get_mut(t) {
                gs.retain(|x| !Arc::ptr_eq(x, g));
                if gs.is_empty() {
                    live.by_topic.remove(t);
                }
            }
        }
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
