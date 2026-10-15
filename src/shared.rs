//! State shared by every thread that runs a program.
//!
//! Quillo code runs under one lock (the GIL). A request handler therefore runs
//! atomically, except while it waits on I/O: builtins such as `sleep` and
//! `http.get` let go of the lock while they wait, so other requests can run.

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

pub struct Shared {
    pub gil: Gil,
    /// The program's database (SQLite or a Postgres pool), opened on first use.
    pub db: Mutex<Option<std::sync::Arc<crate::db::Backend>>>,
}

impl Shared {
    pub fn new() -> Self {
        Shared { gil: Gil::new_held(), db: Mutex::new(None) }
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}
