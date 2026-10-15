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
}

impl Shared {
    pub fn new() -> Self {
        Shared { gil: Gil::new_held(), live: Mutex::new(Live::default()), live_changed: Condvar::new() }
    }

    /// Marks topics as changed and wakes the pages waiting on them.
    pub fn publish(&self, topics: &[String]) {
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

    pub fn live_seq(&self) -> u64 {
        self.live.lock().unwrap_or_else(|e| e.into_inner()).seq
    }

    /// The current sequence number and which of `topics` changed after
    /// `since`, waiting up to `timeout` for one to change.
    pub fn wait_live(&self, since: u64, topics: &[String], timeout: std::time::Duration) -> (u64, Vec<String>) {
        let changed = |live: &Live| -> Vec<String> {
            topics.iter().filter(|t| live.versions.get(*t).map_or(false, |v| *v > since)).cloned().collect()
        };
        let live = self.live.lock().unwrap_or_else(|e| e.into_inner());
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
