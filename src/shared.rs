//! State shared by every thread that runs a program.
//!
//! Quillo code runs under one lock (the GIL). A request handler therefore runs
//! atomically, except while it waits on I/O: builtins such as `sleep` and
//! `http.get` let go of the lock while they wait, so other requests can run.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};

/// A lock that is not tied to a guard's lifetime, so a builtin can release
/// it in the middle of a call and take it back afterwards.
pub struct Gil {
    held: Mutex<bool>,
    cv: Condvar,
}

impl Gil {
    /// A new lock, already held by the thread that creates it.
    pub fn new_held() -> Self {
        Gil { held: Mutex::new(true), cv: Condvar::new() }
    }

    pub fn acquire(&self) {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        while *held {
            held = self.cv.wait(held).unwrap_or_else(|e| e.into_inner());
        }
        *held = true;
    }

    pub fn release(&self) {
        *self.held.lock().unwrap_or_else(|e| e.into_inner()) = false;
        self.cv.notify_one();
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
}

impl Shared {
    pub fn new() -> Self {
        Shared {
            gil: Gil::new_held(),
            live: Mutex::new(Live::default()),
            live_changed: Condvar::new(),
            id: format!("{:08x}", fastrand::u32(..)),
            outbox: Mutex::new(None),
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
        if let Some(tx) = &*self.outbox.lock().unwrap_or_else(|e| e.into_inner()) {
            let shared: Vec<String> = topics.iter().filter(|t| !t.contains("var:")).cloned().collect();
            if !shared.is_empty() {
                let _ = tx.send(shared);
            }
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
