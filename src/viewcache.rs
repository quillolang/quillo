//! Reusing what views rendered.
//!
//! A view's HTML depends on its arguments and on the global variables it
//! reads, nothing else, unless it does something else: reads the clock or
//! the database, calls the network, renders a component. So when a view is
//! called again with the same arguments, and none of the globals it read has
//! been assigned since, the HTML it gave last time is the HTML it would give
//! now, and is reused instead of running the view again.
//!
//! - Arguments are compared by value for numbers and text, and by identity
//!   for lists and maps (values never change, so the same list is the same
//!   data). The cache holds on to them, so an address cannot be reused by
//!   different data while an entry refers to it.
//! - Every assignment to a global is announced (`var:name`, the same
//!   announcements live data uses), with a sequence number. An entry is
//!   valid while none of the globals it read was announced after it was
//!   made.
//! - A view that uses anything else (see `impure_builtin`; components, the
//!   request, writes) is never cached: it is marked once and runs every time.
//!   Database reads are not cached either: a row changed outside Quillo
//!   would not be noticed.
//! - Views containing views: a cached inner view passes what it read on to
//!   the view around it, so a whole page can be cached and still be
//!   invalidated by the global its innermost view read.
//! - A view that translates (`t`, `locale`) is kept once per locale.

use crate::ast::FnDef;
use crate::shared::Shared;
use crate::value::{BuildFx, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Set by `quillo run --no-view-cache` (for measuring, and for debugging).
pub static DISABLED: AtomicBool = AtomicBool::new(false);

/// Kept per view: renders for different arguments, oldest dropped first.
const PER_VIEW: usize = 32;

#[derive(Default)]
pub struct ViewCache {
    views: HashMap<usize, View, BuildFx>,
    pub hits: u64,
    pub misses: u64,
}

struct View {
    /// Keeps the definition alive while its address is the key.
    _def: Arc<FnDef>,
    uncacheable: bool,
    entries: VecDeque<Arc<Entry>>,
}

pub struct Entry {
    args: Vec<Option<Value>>,
    pub html: Arc<str>,
    /// The topics (`var:name`, `watch`ed ones) the render read.
    pub deps: Vec<String>,
    /// The sequence number when the render started.
    seq: u64,
    /// The locale it was rendered in, when it used one.
    pub locale: Option<Arc<str>>,
}

/// What a lookup found.
pub enum Found {
    Hit(Arc<Entry>),
    Miss,
    Uncacheable,
}

impl ViewCache {
    pub fn lookup(&mut self, def: &Arc<FnDef>, args: &[Option<Value>], shared: &Shared, locale: Option<&Arc<str>>) -> Found {
        let Some(view) = self.views.get_mut(&key(def)) else {
            self.misses += 1;
            return Found::Miss;
        };
        if view.uncacheable {
            return Found::Uncacheable;
        }
        let Some(i) = view.entries.iter().position(|e| same_args(&e.args, args) && e.locale.as_ref().map_or(true, |l| Some(l) == locale)) else {
            self.misses += 1;
            return Found::Miss;
        };
        let entry = view.entries[i].clone();
        if shared.unchanged_since(&entry.deps, entry.seq) {
            self.hits += 1;
            Found::Hit(entry)
        } else {
            view.entries.remove(i);
            self.misses += 1;
            Found::Miss
        }
    }

    pub fn store(&mut self, def: &Arc<FnDef>, args: Vec<Option<Value>>, html: &str, mut deps: Vec<String>, seq: u64, locale: Option<Arc<str>>) {
        deps.sort();
        deps.dedup();
        let view = self.view(def);
        if view.uncacheable {
            return;
        }
        if view.entries.len() >= PER_VIEW {
            view.entries.pop_front();
        }
        view.entries.push_back(Arc::new(Entry { args, html: html.into(), deps, seq, locale }));
    }

    /// The view did something that cannot be cached; it runs every time.
    pub fn forbid(&mut self, def: &Arc<FnDef>) {
        let view = self.view(def);
        view.uncacheable = true;
        view.entries.clear();
    }

    pub fn clear(&mut self) {
        self.views.clear();
    }

    fn view(&mut self, def: &Arc<FnDef>) -> &mut View {
        self.views
            .entry(key(def))
            .or_insert_with(|| View { _def: def.clone(), uncacheable: false, entries: VecDeque::new() })
    }
}

fn key(def: &Arc<FnDef>) -> usize {
    Arc::as_ptr(def) as usize
}

fn same_args(a: &[Option<Value>], b: &[Option<Value>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| match (x, y) {
            (None, None) => true,
            (Some(x), Some(y)) => same(x, y),
            _ => false,
        })
}

/// Whether two values are certainly the same: equal scalars and text, or
/// the very same list, map or function.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        // The same digits: 12.5 and 12.50 show differently.
        (Value::Dec(x), Value::Dec(y)) => x.mantissa() == y.mantissa() && x.scale() == y.scale(),
        (Value::Str(x), Value::Str(y)) | (Value::Html(x), Value::Html(y)) => Arc::ptr_eq(x, y) || x == y,
        (Value::List(x), Value::List(y)) => Arc::ptr_eq(x, y),
        (Value::Map(x), Value::Map(y)) => Arc::ptr_eq(x, y),
        (Value::Func(x), Value::Func(y)) => Arc::ptr_eq(x, y),
        (Value::Builtin(x, _), Value::Builtin(y, _)) => x == y,
        (Value::Err(x), Value::Err(y)) => Arc::ptr_eq(x, y),
        (Value::Response(x), Value::Response(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

/// Builtins whose result is not determined by their arguments (the clock,
/// randomness, files, the environment, the network, the database, the
/// request), or that do something besides returning it.
pub fn impure_builtin(name: &str) -> bool {
    name.starts_with("db.")
        || name.starts_with("http.")
        // Rust may do anything.
        || name.starts_with("native:")
        || matches!(
            name,
            "print"
                | "env"
                | "now"
                | "sleep"
                | "file"
                | "read_file"
                | "write_file"
                | "delete_file"
                | "check_program"
                | "random"
                | "random_int"
                | "uuid"
                | "crypto.token"
                | "image.info"
                | "image.convert"
                | "crypto.hash_password"
                | "live"
                | "url"
                | "navigate"
                | "publish"
                | "redirect"
                | "respond"
                | "tenant"
                | "with_tenant"
                | "tenant_from"
                | "allow_cross_site"
                | "translations"
                | "schedule"
                | "locale_from"
                | "with_locale"
                | "set_cookie"
                | "delete_cookie"
                | "session"
                | "set_session"
                | "end_session"
        )
}

/// What a render being cached has read so far.
#[derive(Default)]
pub struct Track {
    pub deps: Vec<String>,
    pub impure: bool,
    pub locale: bool,
}

pub fn enabled() -> bool {
    !DISABLED.load(Ordering::Relaxed)
}
