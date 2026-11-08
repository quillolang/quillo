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
//! - A view that uses anything else (see `Kind`; components, the request,
//!   writes) is never cached: it is marked once and runs every time. Database
//!   reads are not cached either: a row changed outside Quillo would not be
//!   noticed.
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
        (Value::Err(x, _, _), Value::Err(y, _, _)) => Arc::ptr_eq(x, y),
        (Value::Response(x), Value::Response(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

/// What a builtin does, for what is kept of what it was called in: a view's HTML, a `live` run. It
/// is said for each builtin here, and one nobody said anything of is `Acts`: a builtin added
/// next year is not kept and not shared until someone says it can be, where this was a list of
/// those that could not, and `billing.me()` in a view, a visitor's answer, was kept for everyone
/// until the server stopped (FEEDBACK55.md, Q-878 and Q-888; branch feedback55).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Its answer is its arguments', whoever asks, whenever, and it does nothing else. A view
    /// using it can be kept. What differs by locale says so itself (`note_locale`), and a view is
    /// kept once for each; what follows a topic says so too (`watch`).
    Pure,
    /// Its answer can differ by viewer, by tenant or over time, and it changes nothing: a view
    /// using it is not kept, and a run may be shared, for as long as runs are, by what it read.
    Reads,
    /// It changes something, here or elsewhere, or makes something meant to be made once for each
    /// caller (a token, a random number): neither is kept, nor given to anyone else.
    Acts,
}

const PURE: &[&str] = &[
    // Text and numbers.
    "str", "int", "float", "dec", "format_number", "len", "type_of", "upper", "lower", "normalize", "trim", "filled", "split", "join", "contains", "starts_with",
    "ends_with", "replace", "slice", "repeat", "index_of", "cursor_index", "abs", "round", "floor", "ceil", "sqrt", "pow", "url_encode", "escape", "raw", "markdown",
    "markdown_page", "highlight", "classes", "css", "sql", "sql_name", "sql_param", "crypto.sha256", "crypto.hmac", "crypto.equal", "crypto.verify_password",
    // Lists and maps.
    "map", "filter", "find", "any", "all", "reduce", "sum", "min", "max", "sort", "sort_by", "reverse", "first", "last", "range", "enumerate", "push", "keys", "values",
    "get", "has", "set", "remove", "merge", "unique", "flatten", "zip", "take", "drop", "count", "group_by", "join_on",
    // Errors, and what a function it is given does is told as it calls.
    "err", "error", "is_err", "unwrap", "parallel", "defer", "unwatched",
    // What the parser writes, and a browser's, which do nothing here.
    "__merge", "__concat", "__css_value", "__record", "__style", "every", "run_after", "page_updated", "js", "js_new", "js_has", "waiting", "local_path",
    // Dates, which are told the moment they work on.
    "date.add_days", "date.add_months", "date.add_years", "date.at", "date.compare", "date.date_in", "date.days_between", "date.days_in_month", "date.end_of_month",
    "date.format", "date.from_ms", "date.is_leap_year", "date.months_between", "date.ms", "date.of", "date.parse", "date.parts", "date.start_of_month", "date.to_zone",
    "date.valid", "date.valid_zone", "date.weekday",
    // Maths, by name: a family's next function has to be said too, as `stats.sample` would be
    // no function of its arguments.
    "math.sqrt", "math.cbrt", "math.exp", "math.exp2", "math.expm1", "math.log", "math.log2", "math.log10", "math.log1p", "math.pow", "math.sin", "math.cos", "math.tan",
    "math.asin", "math.acos", "math.atan", "math.atan2", "math.sinh", "math.cosh", "math.tanh", "math.asinh", "math.acosh", "math.atanh", "math.degrees", "math.radians",
    "math.hypot", "math.dist", "math.floor", "math.ceil", "math.trunc", "math.fabs", "math.copysign", "math.fmod", "math.remainder", "math.modf", "math.frexp", "math.ldexp",
    "math.factorial", "math.gcd", "math.lcm", "math.comb", "math.perm", "math.isqrt", "math.isclose", "math.isfinite", "math.isinf", "math.isnan", "math.prod", "math.fsum",
    "math.sumprod", "math.erf", "math.erfc", "math.gamma", "math.lgamma", "bit.and", "bit.or", "bit.xor", "bit.not", "bit.shl", "bit.shr", "stats.mean", "stats.fmean",
    "stats.geometric_mean", "stats.harmonic_mean", "stats.median", "stats.median_low", "stats.median_high", "stats.mode", "stats.multimode", "stats.pvariance", "stats.variance",
    "stats.pstdev", "stats.stdev", "stats.quantiles", "stats.percentile", "stats.covariance", "stats.correlation", "stats.linear_regression", "stats.describe", "stats.histogram",
    "stats.zscore", "stats.normalize", "stats.rank", "stats.normal_pdf", "stats.normal_cdf", "stats.normal_inv", "vec.add", "vec.sub", "vec.mul", "vec.div", "vec.mod", "vec.pow",
    "vec.dot", "vec.norm", "vec.cumsum", "vec.cumprod", "vec.diff", "vec.argmin", "vec.argmax", "vec.argsort", "vec.clip", "vec.abs", "vec.sqrt", "vec.exp", "vec.log", "vec.round",
    "vec.linspace", "vec.arange", "vec.zeros", "vec.ones", "vec.full", "vec.rolling_mean", "vec.rolling_sum", "vec.matmul", "vec.transpose", "vec.column",
    // Money, patterns and JSON.
    "money.of", "money.parse", "money.from_minor", "money.to_minor", "money.add", "money.sub", "money.mul", "money.neg", "money.compare", "money.zero", "money.is_zero",
    "money.allocate", "money.round", "money.minor_units", "re.test", "re.valid", "re.find", "re.find_all", "re.replace", "re.split", "json.parse", "json.stringify",
    // Kept by what they say of themselves (see `Kind::Pure`).
    "t", "locale", "watch", "money.format",
];

const READS: &[&str] = &[
    // Who asks, and for whom.
    "session", "cookie", "url", "navigate", "tenant", "with_tenant", "with_locale", "live", "__answer", "mcp_tokens",
    // The clock, the system, files; and a line in the log, which changes nothing a page sees.
    "print", "now", "date.now", "date.today", "sleep", "env", "args", "platform", "file", "files", "read_file", "read_upload", "image.info", "check_program",
    // The database and the network, which say what they changed by themselves when it is a table
    // (`own_seqs`). A transaction is what is run in it.
    "db.query", "db.one", "db.all", "db.on", "db.shards", "db.tenants", "db.tenant_exists", "db.sql_name", "db.error_kind", "db.system", "db.connect", "db.transaction",
    "http.get", "later.status", "channel.peers", "media.streams",
];

/// Said so that a builtin nobody thought about is told from one that was (see the test below).
/// A service's or a site's function may write, and says so only when it has a topic to tell.
#[cfg(test)]
const ACTS: &[&str] = &[
    "exit", "stdin", "read_line", "ask", "notify", "random", "random_int", "uuid", "crypto.token", "crypto.hash_password", "write_file", "delete_file", "save_upload",
    "image.convert", "run_script", "publish", "redirect", "respond", "stream", "events", "listen", "set_cookie", "delete_cookie", "set_session", "end_session", "idempotent",
    "mcp_token", "revoke_mcp_token", "http.post", "http.put", "http.patch", "http.delete", "http.request", "http.stream", "db.exec", "db.exec_many", "db.migrate", "db.open", "db.atomic", "new_tenant",
    "__service", "__service_later", "__site_call", "__site", "__built", "__callers", "schedule", "translations", "locale_from", "tenant_from", "allow_cross_site", "site_headers",
    "accept_calls", "style_with", "ready", "channel.on", "channel.send", "channel.broadcast", "channel.open", "receive", "media.rtp", "media.close", "media.on", "udp.listen",
    "udp.send", "udp.reply",
];

/// What the builtin `name` does (see `Kind`).
pub fn kind(name: &str) -> Kind {
    static SAID: std::sync::OnceLock<HashMap<&'static str, Kind, BuildFx>> = std::sync::OnceLock::new();
    let said = SAID.get_or_init(|| PURE.iter().map(|n| (*n, Kind::Pure)).chain(READS.iter().map(|n| (*n, Kind::Reads))).collect());
    said.get(name).copied().unwrap_or(Kind::Acts)
}

/// `kind`, by where the name is: a run calling `len` a thousand times looks it up once. Each thread
/// keeps the last few (FEEDBACK55.md, Q-888; branch feedback55).
#[inline]
pub fn kind_of(name: &'static str) -> Kind {
    thread_local! {
        static LAST: std::cell::RefCell<[(usize, usize, Kind); 256]> = const { std::cell::RefCell::new([(0, 0, Kind::Acts); 256]) };
    }
    let (at, len) = (name.as_ptr() as usize, name.len());
    LAST.with(|l| {
        let mut l = l.borrow_mut();
        let slot = &mut l[(at ^ (at >> 8)) & 255];
        if (slot.0, slot.1) != (at, len) {
            *slot = (at, len, kind(name));
        }
        slot.2
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every builtin there is, as `Interp::call` is told its name.
    fn all() -> Vec<&'static str> {
        let env = crate::value::Scope::root();
        crate::builtins::install(&env);
        // Not in the scope: a site's functions, and those of what `db.shards` gives.
        let mut out = vec!["__site_call", "db.on", "db.all"];
        for (_, v) in crate::value::scope_vars(&env) {
            match v {
                Value::Builtin(n, _) => out.push(n),
                Value::Map(m) => out.extend(m.values().filter_map(|x| if let Value::Builtin(n, _) = x { Some(*n) } else { None })),
                _ => {}
            }
        }
        out
    }

    /// A builtin nobody said anything of fails here, and until then is neither kept nor shared
    /// (FEEDBACK55.md, Q-878; branch feedback55).
    #[test]
    fn every_builtin_is_said() {
        let said = |n: &&str| PURE.contains(n) || READS.contains(n) || ACTS.contains(n);
        let left: Vec<&str> = all().into_iter().filter(|n| !said(n)).collect();
        assert!(left.is_empty(), "say what these do: {:?}", left);
    }

    /// What is said of a name that is no builtin is a word for nothing.
    #[test]
    fn nothing_is_said_of_what_is_not_there() {
        let there = all();
        let gone: Vec<&&str> = PURE.iter().chain(READS).chain(ACTS).filter(|n| !there.contains(n)).collect();
        assert!(gone.is_empty(), "no such builtin: {:?}", gone);
    }

    #[test]
    fn what_nobody_said_is_neither_kept_nor_shared() {
        assert_eq!(kind("something_added_next_year"), Kind::Acts);
        assert_eq!(kind("native:redis.get"), Kind::Acts);
        assert_eq!(kind("somewhere.else"), Kind::Acts);
        assert_eq!((kind("len"), kind("math.sqrt"), kind("money.format")), (Kind::Pure, Kind::Pure, Kind::Pure));
        assert_eq!((kind("session"), kind("date.today"), kind("db.query")), (Kind::Reads, Kind::Reads, Kind::Reads));
        assert_eq!((kind("__service"), kind("__service_later"), kind("__site_call"), kind("crypto.token")), (Kind::Acts, Kind::Acts, Kind::Acts, Kind::Acts));
    }

    /// Whatever the prefetch of a page may not do is `Acts` (see `interp::CHANGES`).
    #[test]
    fn what_changes_something_acts() {
        for n in crate::interp::CHANGES {
            assert_eq!(kind(n), Kind::Acts, "{}", n);
        }
    }
}
