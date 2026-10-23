//! Tree-walking evaluator: expressions, patterns and HTML rendering.

use crate::ast::*;
use crate::error::QuilloError;
use crate::parser::{Parser, VOID_TAGS};
use crate::shared::Shared;
use crate::value::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Non-local exits: runtime errors, `return` (and `?`), and `reply`.
pub enum Ctrl {
    Error(QuilloError),
    Return(Value),
    Reply(Arc<Response>),
}

impl From<QuilloError> for Ctrl {
    fn from(e: QuilloError) -> Self {
        Ctrl::Error(e)
    }
}

pub fn fail<T>(line: u32, msg: impl Into<String>) -> Result<T, Ctrl> {
    Err(Ctrl::Error(QuilloError::new(msg, line)))
}

fn undefined(name: &str, line: u32) -> Ctrl {
    Ctrl::Error(QuilloError::new(format!("unknown name `{}`", name), line))
}

/// How strictly a pattern compares values. Query strings, form fields and
/// headers arrive as text, so `Lenient` lets `int` match "42" (binding 42).
/// `Request` is lenient for every request field except `json`.
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Strict,
    Lenient,
    Request,
}

pub struct Interp {
    pub globals: Env,
    // Set while the program loads, then only read: shared by every handle
    // on the program (`fork`), which is then a few reference counts.
    pub routes: Arc<Vec<Arc<RouteDef>>>,
    pub base_dir: Arc<Path>,
    loaded: Arc<HashSet<PathBuf>>,
    pub shared: Arc<Shared>,
    /// Calls in progress on this thread, to stop runaway recursion cleanly.
    depth: usize,
    /// Effects the running code may use, and the `with` function that limited them.
    allowed: u8,
    restrictor: Option<Arc<FnDef>>,
    /// Names of `server fn`s, callable from the browser.
    pub server_fns: Arc<Vec<String>>,
    /// What the program set about its site (see `site::Site`).
    pub site: Arc<crate::site::Site>,
    /// The current request's tenant, once worked out (`None` until then).
    pub tenant: Option<Option<Arc<str>>>,
    /// The current request's body when it carries files (see `upload`).
    pub upload_body: Option<Arc<[u8]>>,
    /// `Set-Cookie` lines for the response being made (see `session`).
    pub set_cookies: Vec<String>,
    /// The request's session, once read or set.
    pub session: Option<Value>,
    /// Every statement the program has loaded, for the browser compiler.
    pub program: Arc<Vec<Arc<Vec<Stmt>>>>,
    /// The browser bundle (`/_quillo/quillo.js`), built after loading.
    pub client_js: Arc<String>,
    /// The request being handled, visible as `request` inside `server fn`s.
    pub current_request: Option<Value>,
    /// The request being handled, when its map has not been made yet (see
    /// `RouteDef::uses_request`): made on first use by `request_value`.
    pub(crate) pending_request: Option<crate::server::PendingRequest>,
    /// State of the components being rendered, innermost last, with the
    /// `live(...)` subscriptions of each.
    islands: Vec<(Map, Map)>,
    /// While a `live` loader runs: the topics (tables, globals) it reads.
    pub reads: Option<std::collections::HashSet<String>>,
    /// The subscription the last `live(...)` call set up, for `~state`.
    pub last_live: Option<Value>,
    /// Open `db.transaction`s, and changes to announce when they commit.
    pub tx_depth: usize,
    pub pending: Vec<String>,
    /// Open SQLite transactions. SQLite has one connection, shared by every
    /// request, so while one is open the lock is kept (see `unlocked`).
    pub sqlite_tx: usize,
    /// The Postgres connection this request's `db.transaction` holds.
    pub pg_tx: std::sync::Mutex<Option<crate::db::PgTx>>,
    /// This program's database (SQLite or a Postgres pool), opened on first use.
    pub db: Arc<std::sync::Mutex<Option<crate::db::DbSlot>>>,
    /// Where this program is mounted ("" at the root, "/notes" under `mount`).
    /// Root-relative links, redirects and browser URLs get it as a prefix.
    pub base: String,
    /// Programs mounted under a path prefix: `mount /notes = "notes.qwe"`.
    pub mounts: Vec<(String, Interp)>,
    /// Whether the response being made has a component island, and so
    /// needs the browser bundle.
    pub rendered_island: bool,
    /// A buffer lent to the renderer for building attribute values.
    spare: String,
    /// What views rendered, shared by the workers (see `viewcache`).
    pub view_cache: Option<Arc<Mutex<crate::viewcache::ViewCache>>>,
    /// The renders being cached, innermost last: what each has read.
    tracks: Vec<crate::viewcache::Track>,
}

/// Deepest call nesting allowed before a "too much recursion" error.
pub const MAX_DEPTH: usize = 10_000;

pub fn lit_value(l: &Lit) -> Value {
    match l {
        Lit::Nil => Value::Nil,
        Lit::Bool(b) => Value::Bool(*b),
        Lit::Int(n) => Value::Int(*n),
        Lit::Float(f) => Value::Float(*f),
        Lit::Str(s) => Value::Str(s.clone()),
    }
}

pub fn type_name_matches(t: &str, v: &Value) -> bool {
    match t {
        "any" => true,
        "int" => matches!(v, Value::Int(_)),
        "float" => matches!(v, Value::Float(_)),
        "num" => matches!(v, Value::Int(_) | Value::Float(_)),
        "str" => matches!(v, Value::Str(_)),
        "bool" => matches!(v, Value::Bool(_)),
        "list" => matches!(v, Value::List(_)),
        "map" => matches!(v, Value::Map(_)),
        "html" => matches!(v, Value::Html(_)),
        "fn" => matches!(v, Value::Func(_) | Value::Builtin(..)),
        "response" => matches!(v, Value::Response(_)),
        _ => false,
    }
}

pub fn type_matches(ty: &Ty, v: &Value) -> bool {
    (ty.optional && matches!(v, Value::Nil)) || ty.names.iter().any(|n| type_name_matches(n, v))
}

fn ty_text(ty: &Ty) -> String {
    ty.text()
}

/// Converts request text to the type a pattern asks for.
pub fn coerce(t: &str, s: &str) -> Option<Value> {
    let s = s.trim();
    match t {
        "int" => s.parse::<i64>().ok().map(Value::Int),
        "float" => s.parse::<f64>().ok().map(Value::Float),
        "num" => s.parse::<i64>().ok().map(Value::Int).or_else(|| s.parse::<f64>().ok().map(Value::Float)),
        "bool" => match s {
            "true" | "on" | "1" | "yes" => Some(Value::Bool(true)),
            "false" | "off" | "0" | "no" => Some(Value::Bool(false)),
            _ => None,
        },
        "str" | "any" => Some(Value::str(s)),
        _ => None,
    }
}

fn get_path(v: &Value, path: &[String]) -> Option<Value> {
    let mut cur = v.clone();
    for k in path {
        cur = match &cur {
            Value::Map(m) => m.get(k.as_str())?.clone(),
            _ => return None,
        };
    }
    Some(cur)
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape_into(s, &mut out);
    out
}

/// `escape`, appending to `out`: runs without special characters are
/// copied whole.
pub fn escape_into(s: &str, out: &mut String) {
    let mut done = 0;
    for (i, b) in s.bytes().enumerate() {
        let entity = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&#39;",
            _ => continue,
        };
        out.push_str(&s[done..i]);
        out.push_str(entity);
        done = i + 1;
    }
    out.push_str(&s[done..]);
}

/// Attributes whose value the browser may load or run as a URL.
pub const URL_ATTRS: &[&str] =
    &["href", "src", "action", "formaction", "poster", "xlink:href", "cite", "background", "data", "codebase"];

/// What a `javascript:` URL in one of `URL_ATTRS` becomes.
pub const BLOCKED_URL: &str = "about:blank#blocked";

/// Whether a URL would run code when followed: `javascript:` or `vbscript:`,
/// however browsers would read it (they skip leading spaces and control
/// characters and ignore tabs and line breaks inside).
pub fn script_url(url: &str) -> bool {
    // Paths and fragments, the usual case, have no scheme.
    if matches!(url.as_bytes().first(), Some(b'/' | b'#' | b'?')) {
        return false;
    }
    let mut scheme = [0u8; 11];
    let mut n = 0;
    for c in url.trim_start_matches(|c: char| c <= ' ').chars() {
        if matches!(c, '\t' | '\n' | '\r') {
            continue;
        }
        // Neither scheme has other characters in its first 11.
        if n == scheme.len() || !c.is_ascii() {
            break;
        }
        scheme[n] = c.to_ascii_lowercase() as u8;
        n += 1;
    }
    let scheme = &scheme[..n];
    scheme.starts_with(b"javascript:") || scheme.starts_with(b"vbscript:")
}

/// An integer's digits, appended to `out` (without `fmt`'s machinery).
fn push_int(out: &mut String, n: i64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut m = n.unsigned_abs();
    loop {
        i -= 1;
        buf[i] = b'0' + (m % 10) as u8;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    if n < 0 {
        out.push('-');
    }
    // SAFETY: ASCII digits.
    out.push_str(unsafe { std::str::from_utf8_unchecked(&buf[i..]) });
}

/// A value's text, escaped, appended to `out`.
fn display_escaped(v: &Value, out: &mut String) {
    match v {
        Value::Str(s) => escape_into(s, out),
        Value::Int(n) => push_int(out, *n),
        other => escape_into(&other.display(), out),
    }
}

/// Whether a name can be written as an HTML attribute as it is.
pub fn attr_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| !c.is_control() && !matches!(c, ' ' | '"' | '\'' | '>' | '<' | '/' | '=' | '`' | '{' | '}'))
}

/// Whether a key of a `{...map}` spread may become an attribute.
fn spread_attr_allowed(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    attr_name_ok(name) && !lower.starts_with("on") && lower != "srcdoc" && !name.starts_with("__")
}

fn render_value(v: &Value, out: &mut String) {
    match v {
        Value::Html(h) => out.push_str(h),
        Value::Nil | Value::Bool(false) => {}
        Value::List(items) => {
            for i in items.iter() {
                render_value(i, out);
            }
        }
        Value::Str(s) => escape_into(s, out),
        Value::Int(n) => push_int(out, *n),
        other => escape_into(&other.display(), out),
    }
}

/// JSON for an island's props and state: like `to_json`, but an error value
/// becomes `{"$quillo_err": payload}`, which the browser turns back into one.
pub fn island_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Err(e) => serde_json::json!({ "$quillo_err": island_json(e) }),
        Value::List(l) => serde_json::Value::Array(l.iter().map(island_json).collect()),
        Value::Map(m) => serde_json::Value::Object(m.iter().map(|(k, x)| (k.to_string(), island_json(x))).collect()),
        other => other.to_json(),
    }
}

/// Why a value cannot travel to the browser as JSON, if it cannot.
pub fn not_sendable(v: &Value) -> Option<&'static str> {
    match v {
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_) => None,
        Value::List(l) => l.iter().find_map(not_sendable),
        Value::Map(m) => m.values().find_map(not_sendable),
        Value::Func(_) | Value::Builtin(..) => Some("a function"),
        Value::Html(_) => Some("html"),
        Value::Response(_) => Some("a response"),
        Value::Err(e) => not_sendable(e),
    }
}

/// The field a function only reads from its one parameter, as in
/// `(x) => x.name`, when calling it can do nothing else (no types,
/// defaults or effects to check).
fn projection(def: &FnDef) -> Option<(&str, u32)> {
    let [p] = def.params.as_slice() else { return None };
    if p.ty.is_some() || p.default.is_some() || def.ret.is_some() || def.err.is_some() || def.effects.is_some() || def.server
    {
        return None;
    }
    match &def.body {
        Expr::Field(obj, field, at) => match &**obj {
            Expr::Ident(n, _) if *n == p.name => Some((field, *at)),
            _ => None,
        },
        _ => None,
    }
}

fn short(v: &Value) -> String {
    let r = v.repr();
    if r.chars().count() > 40 {
        format!("{}…", r.chars().take(40).collect::<String>())
    } else {
        r
    }
}

pub fn binop(op: BinOp, a: Value, b: Value, line: u32) -> Result<Value, Ctrl> {
    use Value::*;
    let overflow = || Ctrl::Error(QuilloError::new("integer overflow", line));
    match op {
        BinOp::Add => match (a, b) {
            (Int(x), Int(y)) => x.checked_add(y).map(Int).ok_or_else(overflow),
            (Str(x), y) => Ok(Value::str(format!("{}{}", x, y.display()))),
            (x, Str(y)) => Ok(Value::str(format!("{}{}", x.display(), y))),
            (Html(x), Html(y)) => Ok(Value::html(format!("{}{}", x, y))),
            (List(x), List(y)) => {
                let mut v = Arc::unwrap_or_clone(x);
                v.extend(y.iter().cloned());
                Ok(Value::list(v))
            }
            (Map(x), Map(y)) => {
                let mut m = Arc::unwrap_or_clone(x);
                for (k, v) in y.iter() {
                    m.insert(k.clone(), v.clone());
                }
                Ok(Value::map(m))
            }
            (x, y) => match (x.as_f64(), y.as_f64()) {
                (Some(p), Some(q)) => Ok(Float(p + q)),
                _ => fail(line, format!("cannot add {} and {}", x.type_name(), y.type_name())),
            },
        },
        BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
            if let (Int(x), Int(y)) = (&a, &b) {
                let (x, y) = (*x, *y);
                if y == 0 && matches!(op, BinOp::Div | BinOp::Mod) {
                    return fail(line, "division by zero");
                }
                // `/` divides exactly, as in JavaScript: 6 / 2 is 3, 7 / 2 is 3.5.
                // (The browser cannot tell 2 from 2.0, so this is also the
                // only rule that gives the same answer on both sides.)
                if op == BinOp::Div && x.checked_rem(y) != Some(0) {
                    return Ok(Float(x as f64 / y as f64));
                }
                let r = match op {
                    BinOp::Sub => x.checked_sub(y),
                    BinOp::Mul => x.checked_mul(y),
                    BinOp::Div => x.checked_div(y),
                    _ => x.checked_rem(y),
                };
                return r.map(Int).ok_or_else(overflow);
            }
            let verb = match op {
                BinOp::Sub => "subtract",
                BinOp::Mul => "multiply",
                BinOp::Div => "divide",
                _ => "take the remainder of",
            };
            match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => Ok(Float(match op {
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => x / y,
                    _ => x % y,
                })),
                _ => fail(line, format!("cannot {} {} and {}", verb, a.type_name(), b.type_name())),
            }
        }
        BinOp::Eq => Ok(Bool(equal(&a, &b))),
        BinOp::Ne => Ok(Bool(!equal(&a, &b))),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let Some(ord) = compare(&a, &b) else {
                return fail(line, format!("cannot compare {} with {}", a.type_name(), b.type_name()));
            };
            Ok(Bool(match op {
                BinOp::Lt => ord.is_lt(),
                BinOp::Le => ord.is_le(),
                BinOp::Gt => ord.is_gt(),
                _ => ord.is_ge(),
            }))
        }
        BinOp::Range => match (a, b) {
            (Int(x), Int(y)) if y.saturating_sub(x) <= 10_000_000 => Ok(Value::list((x..y).map(Int).collect())),
            (Int(_), Int(_)) => fail(line, "range is too large"),
            _ => fail(line, "`..` needs two ints"),
        },
        BinOp::And | BinOp::Or => unreachable!("handled in eval"),
    }
}

impl Interp {
    /// A new interpreter. The calling thread holds its lock (see `shared`).
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self::with_shared(base_dir, Arc::new(Shared::new()))
    }

    /// A new interpreter using an existing lock, for reloading a program
    /// while other threads keep serving. The caller must hold the lock.
    pub fn with_shared(base_dir: impl Into<PathBuf>, shared: Arc<Shared>) -> Self {
        let globals = Scope::root();
        crate::builtins::install(&globals);
        Interp {
            globals,
            routes: Arc::new(Vec::new()),
            base_dir: Arc::from(base_dir.into().as_path()),
            loaded: Arc::new(HashSet::new()),
            shared,
            depth: 0,
            allowed: ALL_EFFECTS,
            restrictor: None,
            server_fns: Arc::new(Vec::new()),
            site: Arc::new(crate::site::Site::default()),
            tenant: None,
            upload_body: None,
            set_cookies: Vec::new(),
            session: None,
            program: Arc::new(Vec::new()),
            client_js: Arc::new(String::new()),
            current_request: None,
            pending_request: None,
            islands: Vec::new(),
            pg_tx: std::sync::Mutex::new(None),
            reads: None,
            last_live: None,
            tx_depth: 0,
            pending: Vec::new(),
            sqlite_tx: 0,
            db: Arc::new(std::sync::Mutex::new(None)),
            base: String::new(),
            mounts: Vec::new(),
            rendered_island: false,
            spare: String::new(),
            view_cache: crate::viewcache::enabled().then(Default::default),
            tracks: Vec::new(),
        }
    }

    /// Another handle on the same program for a worker thread: globals,
    /// routes and the lock are shared.
    pub fn fork(&self) -> Interp {
        Interp {
            globals: self.globals.clone(),
            routes: self.routes.clone(),
            base_dir: self.base_dir.clone(),
            loaded: self.loaded.clone(),
            shared: self.shared.clone(),
            depth: 0,
            allowed: ALL_EFFECTS,
            restrictor: None,
            server_fns: self.server_fns.clone(),
            site: self.site.clone(),
            tenant: None,
            upload_body: None,
            set_cookies: Vec::new(),
            session: None,
            program: self.program.clone(),
            client_js: self.client_js.clone(),
            current_request: None,
            pending_request: None,
            islands: Vec::new(),
            pg_tx: std::sync::Mutex::new(None),
            reads: None,
            last_live: None,
            tx_depth: 0,
            pending: Vec::new(),
            sqlite_tx: 0,
            db: self.db.clone(),
            base: self.base.clone(),
            mounts: self.mounts.iter().map(|(p, m)| (p.clone(), m.fork())).collect(),
            rendered_island: false,
            spare: String::new(),
            view_cache: self.view_cache.clone(),
            tracks: Vec::new(),
        }
    }

    /// The source files this program has loaded (the main file and `use`d files).
    pub fn files(&self) -> Vec<PathBuf> {
        self.loaded.iter().cloned().collect()
    }

    /// Announces changes to `topics`, after the current transaction commits.
    pub fn publish(&mut self, topics: Vec<String>) {
        self.mark_impure();
        // Each program has its own topics (two mounted apps may both have
        // a `notes` table).
        let topics: Vec<String> = topics.into_iter().map(|t| self.scope_topic(t)).collect();
        if self.tx_depth > 0 {
            self.pending.extend(topics);
        } else {
            self.shared.publish(&topics);
        }
    }

    /// Notes that the running `live` loader depends on `topic`.
    pub fn watch(&mut self, topic: String) {
        let topic = self.scope_topic(topic);
        self.note_dep(topic);
    }

    /// Records a topic (already prefixed) for the running `live` loader and
    /// the renders being cached.
    fn note_dep(&mut self, topic: String) {
        if let Some(t) = self.tracks.last_mut() {
            t.deps.push(topic.clone());
        }
        if let Some(r) = &mut self.reads {
            r.insert(topic);
        }
    }

    /// The renders being cached did something that cannot be.
    fn mark_impure(&mut self) {
        for t in &mut self.tracks {
            t.impure = true;
        }
    }

    /// Hits and misses of the view cache so far.
    pub fn view_cache_stats(&self) -> (u64, u64) {
        match &self.view_cache {
            Some(c) => {
                let c = c.lock().unwrap_or_else(|e| e.into_inner());
                (c.hits, c.misses)
            }
            None => (0, 0),
        }
    }

    /// A root-relative URL ("/x") as seen from outside this program.
    pub fn with_base(&self, url: &str) -> String {
        if !self.base.is_empty() && url.starts_with('/') && !url.starts_with("//") {
            format!("{}{}", self.base, url)
        } else {
            url.to_string()
        }
    }

    #[cold]
    fn note_global_read(&mut self, env: &Env, name: &str) {
        if let Some((v, true)) = lookup_global(env, name) {
            let module = matches!(&v, Value::Map(m) if !m.is_empty() && m.values().all(|x| matches!(x, Value::Builtin(..))));
            if !module && !matches!(v, Value::Func(_) | Value::Builtin(..)) {
                self.watch(format!("var:{}", name));
            }
        }
    }

    /// Runs `f` without holding the interpreter lock, so other requests can
    /// run while this one waits. `f` must not touch Quillo values.
    pub fn unlocked<T: Send>(&mut self, f: impl FnOnce() -> T + Send) -> T {
        // Inside a SQLite transaction, another request would run its SQL in
        // this one's transaction: committed or rolled back with it, and
        // seeing what it has not committed. So the wait holds everyone up.
        if self.sqlite_tx > 0 {
            return f();
        }
        // A request in the server is a coroutine: it pauses, `f` runs on
        // another thread, and the server answers other requests meanwhile.
        let f = match crate::httpd::wait_elsewhere(f) {
            Ok(v) => return v,
            Err(f) => f,
        };
        // Takes the lock back even if `f` panics, so the worker that
        // catches the panic gives back a lock it holds.
        struct Relock<'a>(&'a crate::shared::Gil);
        impl Drop for Relock<'_> {
            fn drop(&mut self) {
                self.0.acquire();
            }
        }
        self.shared.gil.release();
        let _relock = Relock(&self.shared.gil);
        f()
    }

    /// The request being handled, as the `request` map (made now if needed).
    pub fn request_value(&mut self) -> Option<Value> {
        if self.current_request.is_none() {
            if let Some(p) = &self.pending_request {
                self.current_request = Some(p.make());
            }
        }
        self.current_request.clone()
    }

    /// `parallel(f, g, ...)`: calls the functions at once and gives back
    /// their results, in order. Each runs on a thread of its own with its own
    /// handle on the program; since waiting (for the database, HTTP, `sleep`)
    /// lets other Quillo code run, their waits overlap. The first failure,
    /// in order, is the result; error values are results like any other.
    ///
    /// Where other threads would change what the code means, the functions
    /// run one after another instead: inside a transaction (their SQL would
    /// run outside it), and while what is read is being recorded (a `live`
    /// loader, a cached view, a component's state).
    pub fn run_parallel(&mut self, fs: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
        for (i, f) in fs.iter().enumerate() {
            if !matches!(f, Value::Func(_) | Value::Builtin(..)) {
                return fail(
                    line,
                    format!("`parallel` takes functions to call, like parallel(() => a(), () => b()); argument {} is {}", i + 1, f.type_name()),
                );
            }
        }
        let in_tx = self.tx_depth > 0
            || self.sqlite_tx > 0
            || self.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        if fs.len() < 2 || in_tx || self.reads.is_some() || !self.tracks.is_empty() || !self.islands.is_empty() {
            let mut out = Vec::with_capacity(fs.len());
            for f in &fs {
                out.push(self.call(f, Vec::new(), line)?);
            }
            return Ok(Value::list(out));
        }
        // In the server: each function as a coroutine on the event loop, so
        // their waits (queries, `http.*`) overlap without threads.
        if crate::httpd::in_task() {
            let mut results: Vec<Option<Result<Value, Ctrl>>> = (0..fs.len()).map(|_| None).collect();
            let base = results.as_mut_ptr() as usize;
            let children: Vec<Box<dyn FnOnce()>> = fs
                .into_iter()
                .enumerate()
                .map(|(i, f)| {
                    let mut it = self.task_fork();
                    Box::new(move || {
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.call(&f, Vec::new(), line)))
                            .unwrap_or_else(|_| fail(line, "a function given to `parallel` failed unexpectedly"));
                        // SAFETY: `results` is not touched (or moved) while
                        // the children run; the request waits for them.
                        unsafe { *(base as *mut Option<Result<Value, Ctrl>>).add(i) = Some(r) };
                    }) as Box<dyn FnOnce()>
                })
                .collect();
            crate::httpd::run_children(children);
            let mut out = Vec::with_capacity(results.len());
            for r in results {
                out.push(r.unwrap_or_else(|| fail(line, "a function given to `parallel` failed unexpectedly"))?);
            }
            return Ok(Value::list(out));
        }
        let tasks: Vec<(Interp, Value)> = fs.into_iter().map(|f| (self.task_fork(), f)).collect();
        let shared = self.shared.clone();
        let results: Vec<Result<Value, Ctrl>> = self.unlocked(|| {
            std::thread::scope(|s| {
                let running: Vec<_> = tasks
                    .into_iter()
                    .map(|(mut it, f)| {
                        let shared = &shared;
                        std::thread::Builder::new().name("quillo-parallel".into()).stack_size(8 << 20).spawn_scoped(s, move || {
                            shared.gil.acquire();
                            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.call(&f, Vec::new(), line)));
                            shared.gil.release();
                            r.unwrap_or_else(|_| fail(line, "a function given to `parallel` failed unexpectedly"))
                        })
                    })
                    .collect();
                running
                    .into_iter()
                    .map(|t| match t {
                        Ok(h) => h.join().unwrap_or_else(|_| fail(line, "a function given to `parallel` failed unexpectedly")),
                        Err(e) => fail(line, format!("`parallel` could not start a thread: {}", e)),
                    })
                    .collect()
            })
        });
        let mut out = Vec::with_capacity(results.len());
        for r in results {
            out.push(r?);
        }
        Ok(Value::list(out))
    }

    /// A handle for a `parallel` task: the program, and what the caller may
    /// do (effects, the request being handled).
    fn task_fork(&self) -> Interp {
        let mut it = self.fork();
        it.depth = self.depth;
        it.allowed = self.allowed;
        it.restrictor = self.restrictor.clone();
        it.current_request = self.current_request.clone().or_else(|| self.pending_request.as_ref().map(|p| p.make()));
        it.tenant = self.tenant.clone();
        it.upload_body = self.upload_body.clone();
        it
    }

    /// The tenant the code is running for: the request's (see `tenant_from`),
    /// or the one `with_tenant` set; none outside a request.
    pub fn tenant(&mut self) -> Option<Arc<str>> {
        if let Some(t) = &self.tenant {
            return t.clone();
        }
        let Some(rule) = self.site.tenant_rule.clone() else {
            self.tenant = Some(None);
            return None;
        };
        let Some(Value::Map(req)) = self.request_value() else { return None };
        let found = match rule {
            crate::site::TenantRule::Subdomain(domain) => {
                let host = match req.get("headers") {
                    Some(Value::Map(h)) => match h.get("host") {
                        Some(Value::Str(s)) => s.to_ascii_lowercase(),
                        _ => String::new(),
                    },
                    _ => String::new(),
                };
                let host = host.rsplit_once(':').filter(|(_, p)| p.bytes().all(|b| b.is_ascii_digit())).map_or(host.as_str(), |(h, _)| h);
                host.strip_suffix(domain.as_str()).and_then(|s| s.strip_suffix('.')).map(str::to_string)
            }
            crate::site::TenantRule::Function(f) => match self.call(&f, vec![Value::Map(req)], 0) {
                Ok(Value::Str(s)) => Some(s.to_string()),
                _ => None,
            },
        };
        // A name that could not be a tenant's is no tenant.
        let t: Option<Arc<str>> = found.filter(|n| crate::site::tenant_name_ok(n)).map(Arc::from);
        self.tenant = Some(t.clone());
        t
    }

    /// A topic as this program and tenant see it: a tenant's own unless it
    /// is a global variable's (`var:`), which all tenants share.
    fn scope_topic(&mut self, topic: String) -> String {
        let topic = if topic.starts_with("var:") || self.site.tenant_rule.is_none() && self.tenant.is_none() {
            topic
        } else {
            match self.tenant() {
                Some(t) => format!("{}@{}", t, topic),
                None => topic,
            }
        };
        if self.base.is_empty() { topic } else { format!("{}|{}", self.base, topic) }
    }

    /// Parses and runs a program's top level, registering its routes.
    pub fn load_source(&mut self, src: &str, file: &str) -> Result<Value, QuilloError> {
        let stmts = Arc::new(Parser::new(src).parse_program().map_err(|e| e.in_file(file))?);
        Arc::make_mut(&mut self.program).push(stmts.clone());
        let env = self.globals.clone();
        let mut last = Value::Nil;
        for s in stmts.iter() {
            match self.exec_stmt(s, &env) {
                Ok(v) => last = v,
                Err(Ctrl::Error(e)) => return Err(e.in_file(file)),
                Err(Ctrl::Return(v)) => {
                    last = v;
                    break;
                }
                Err(Ctrl::Reply(_)) => return Err(QuilloError::new("`reply` used outside of a route", 0).in_file(file)),
            }
        }
        self.build_client().map_err(|e| e.in_file(file))?;
        Ok(last)
    }

    /// Compiles the program's components (and what they use) for the browser.
    pub fn build_client(&mut self) -> Result<(), QuilloError> {
        self.client_js = Arc::new(crate::client::bundle(self)?);
        Ok(())
    }

    pub fn load_file(&mut self, path: &Path) -> Result<Value, QuilloError> {
        let src = std::fs::read_to_string(path)
            .map_err(|e| QuilloError::new(format!("cannot read {}: {}", path.display(), e), 0))?;
        if let Ok(p) = path.canonicalize() {
            Arc::make_mut(&mut self.loaded).insert(p);
        }
        self.load_source(&src, &path.display().to_string())
    }

    pub fn exec_stmt(&mut self, s: &Stmt, env: &Env) -> Result<Value, Ctrl> {
        match s {
            Stmt::Expr(e) => self.eval(e, env),
            Stmt::Let(p, e, line) => {
                let v = self.eval(e, env)?;
                let mut binds = Vec::new();
                if self.match_pat(p, &v, Mode::Strict, env, &mut binds)?.is_none() {
                    return fail(*line, format!("`let` pattern does not match {}", short(&v)));
                }
                for (n, v) in binds {
                    define(env, &n, v);
                }
                Ok(Value::Nil)
            }
            Stmt::Assign(name, op, e, line) => {
                let rhs = self.eval(e, env)?;
                let v = match op {
                    AssignOp::Set => rhs,
                    AssignOp::Add | AssignOp::Sub => {
                        let old = lookup(env, name).ok_or_else(|| undefined(name, *line))?;
                        let bop = if matches!(op, AssignOp::Add) { BinOp::Add } else { BinOp::Sub };
                        binop(bop, old, rhs, *line)?
                    }
                };
                let function = matches!(v, Value::Func(_)) || matches!(lookup(env, name), Some(Value::Func(_)));
                match assign_global(env, name, v) {
                    None => fail(
                        *line,
                        format!("cannot assign to `{}`: it was never declared (use `let {} = …`)", name, name),
                    ),
                    Some(global) => {
                        if global {
                            self.publish(vec![format!("var:{}", name)]);
                            // Cached renders do not record the functions
                            // they called, so a new one starts afresh.
                            if function {
                                if let Some(c) = &self.view_cache {
                                    c.lock().unwrap_or_else(|e| e.into_inner()).clear();
                                }
                            }
                        }
                        Ok(Value::Nil)
                    }
                }
            }
            Stmt::Fn(def) => {
                define(env, &def.name, Value::Func(Arc::new(Closure { def: def.clone(), env: env.clone() })));
                if def.server && !self.server_fns.contains(&def.name) {
                    Arc::make_mut(&mut self.server_fns).push(def.name.clone());
                }
                Ok(Value::Nil)
            }
            Stmt::Route(r) => {
                Arc::make_mut(&mut self.routes).push(r.clone());
                Ok(Value::Nil)
            }
            Stmt::Use(path, line) => {
                let full = self.base_dir.join(path);
                let key = full.canonicalize().unwrap_or_else(|_| full.clone());
                if !Arc::make_mut(&mut self.loaded).insert(key) {
                    return Ok(Value::Nil);
                }
                let src = std::fs::read_to_string(&full)
                    .map_err(|e| QuilloError::new(format!("cannot read {}: {}", full.display(), e), *line))?;
                let fname = full.display().to_string();
                let stmts = Arc::new(Parser::new(&src).parse_program().map_err(|e| e.in_file(&fname))?);
                Arc::make_mut(&mut self.program).push(stmts.clone());
                let g = self.globals.clone();
                for s in stmts.iter() {
                    match self.exec_stmt(s, &g) {
                        Err(Ctrl::Error(e)) => return Err(Ctrl::Error(e.in_file(&fname))),
                        other => {
                            other?;
                        }
                    }
                }
                Ok(Value::Nil)
            }
            Stmt::Mount(prefix, file, line) => {
                if !Arc::ptr_eq(env, &self.globals) {
                    return fail(*line, "`mount` must be at the top level");
                }
                let full = self.base_dir.join(file);
                let dir = full.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| self.base_dir.to_path_buf());
                let mut sub = Interp::with_shared(dir, self.shared.clone());
                sub.base = format!("{}{}", self.base, prefix);
                sub.load_file(&full).map_err(|e| Ctrl::Error(e.in_file(&full.display().to_string())))?;
                // Watch the mounted program's files too.
                Arc::make_mut(&mut self.loaded).extend(sub.files());
                self.mounts.retain(|(p, _)| p != prefix);
                self.mounts.push((prefix.clone(), sub));
                Ok(Value::Nil)
            }
            Stmt::State(name, e, _) => {
                self.last_live = None;
                let v = self.eval(e, env)?;
                let live = self.last_live.take();
                if let Some((state, subs)) = self.islands.last_mut() {
                    state.insert(name.into(), v.clone());
                    if let Some(l) = live {
                        subs.insert(name.into(), l);
                    }
                }
                define(env, name, v);
                Ok(Value::Nil)
            }
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Nil,
                };
                Err(Ctrl::Return(v))
            }
        }
    }

    /// Evaluates an expression. Only the cheap cases are handled inline; the
    /// rest live in separate functions so that each nested call uses little
    /// stack (deep recursion in Quillo is deep recursion here).
    pub fn eval(&mut self, e: &Expr, env: &Env) -> Result<Value, Ctrl> {
        match e {
            Expr::Lit(l) => Ok(lit_value(l)),
            Expr::Ident(n, line) => {
                if self.reads.is_some() || !self.tracks.is_empty() {
                    self.note_global_read(env, n);
                }
                lookup(env, n).ok_or_else(|| undefined(n, *line))
            }
            // As in JavaScript: the operand that decided the result.
            Expr::Binary(BinOp::And, a, b, _) => {
                let x = self.eval(a, env)?;
                if !x.truthy() {
                    return Ok(x);
                }
                self.eval(b, env)
            }
            Expr::Binary(BinOp::Or, a, b, _) => {
                let x = self.eval(a, env)?;
                if x.truthy() {
                    return Ok(x);
                }
                self.eval(b, env)
            }
            Expr::Binary(op, a, b, line) => {
                let x = self.eval(a, env)?;
                let y = self.eval(b, env)?;
                binop(*op, x, y, *line)
            }
            Expr::Coalesce(a, b) => {
                let v = self.eval(a, env)?;
                if matches!(v, Value::Nil | Value::Err(_)) {
                    self.eval(b, env)
                } else {
                    Ok(v)
                }
            }
            Expr::Pipe(lhs, rhs, line) => self.eval_pipe(lhs, rhs, env, *line),
            Expr::Call(callee, args, line) => self.eval_call(callee, args, None, env, *line),
            Expr::Field(obj, name, line) => {
                let v = self.eval(obj, env)?;
                self.get_field(v, name, *line)
            }
            Expr::Try(x) => {
                let v = self.eval(x, env)?;
                if let Value::Err(_) = v {
                    Err(Ctrl::Return(v))
                } else {
                    Ok(v)
                }
            }
            Expr::If(c, a, b) => {
                if self.eval(c, env)?.truthy() {
                    self.eval(a, env)
                } else if let Some(b) = b {
                    self.eval(b, env)
                } else {
                    Ok(Value::Nil)
                }
            }
            Expr::Block(stmts) => self.eval_block(stmts, env),
            Expr::Lambda(def) => Ok(Value::Func(Arc::new(Closure { def: def.clone(), env: env.clone() }))),
            Expr::Template(parts) => self.eval_template(parts, env),
            Expr::List(items) => self.eval_list(items, env),
            Expr::Map(entries) => self.eval_map(entries, env),
            Expr::Unary(op, x, line) => self.eval_unary(*op, x, env, *line),
            Expr::Index(obj, idx, line) => self.eval_index(obj, idx, env, *line),
            Expr::Match(subj, arms, line) => self.eval_match(subj, arms, env, *line),
            Expr::For(pat, iter, body, line) => self.eval_for(pat, iter, body, env, *line),
            Expr::Reply(status, body, line) => self.eval_reply(status, body.as_deref(), env, *line),
            Expr::Html(el, _) => self.eval_html(el, env),
        }
    }

    #[inline(never)]
    fn eval_pipe(&mut self, lhs: &Expr, rhs: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let v = self.eval(lhs, env)?;
        match rhs {
            Expr::Call(callee, args, l) => self.eval_call(callee, args, Some(v), env, *l),
            other => {
                let f = self.eval(other, env)?;
                self.call(&f, vec![v], line)
            }
        }
    }

    #[inline(never)]
    fn eval_block(&mut self, stmts: &[Stmt], env: &Env) -> Result<Value, Ctrl> {
        let scope = Scope::child(env);
        let mut last = Value::Nil;
        for s in stmts {
            last = self.exec_stmt(s, &scope)?;
        }
        Scope::done(scope);
        Ok(last)
    }

    #[inline(never)]
    fn eval_template(&mut self, parts: &[TPart], env: &Env) -> Result<Value, Ctrl> {
        let mut s = String::new();
        self.template_into(parts, env, &mut s)?;
        Ok(Value::str(s))
    }

    #[inline(never)]
    fn eval_list(&mut self, items: &[Expr], env: &Env) -> Result<Value, Ctrl> {
        let mut v = Vec::with_capacity(items.len());
        for i in items {
            v.push(self.eval(i, env)?);
        }
        Ok(Value::list(v))
    }

    #[inline(never)]
    fn eval_map(&mut self, entries: &[(String, Expr)], env: &Env) -> Result<Value, Ctrl> {
        let mut m = Map::new();
        for (k, x) in entries {
            let v = self.eval(x, env)?;
            m.insert(k.into(), v);
        }
        Ok(Value::map(m))
    }

    #[inline(never)]
    fn eval_unary(&mut self, op: UnOp, x: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let v = self.eval(x, env)?;
        match op {
            UnOp::Not => Ok(Value::Bool(!v.truthy())),
            UnOp::Neg => match v {
                Value::Int(n) => {
                    n.checked_neg().map(Value::Int).ok_or_else(|| Ctrl::Error(QuilloError::new("integer overflow", line)))
                }
                Value::Float(f) => Ok(Value::Float(-f)),
                o => fail(line, format!("cannot negate {}", o.type_name())),
            },
        }
    }

    #[inline(never)]
    fn eval_index(&mut self, obj: &Expr, idx: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let o = self.eval(obj, env)?;
        let i = self.eval(idx, env)?;
        match (&o, &i) {
            (Value::List(l), Value::Int(n)) => {
                let n = if *n < 0 { l.len() as i64 + n } else { *n };
                Ok(if n >= 0 { l.get(n as usize).cloned().unwrap_or(Value::Nil) } else { Value::Nil })
            }
            (Value::Map(m), Value::Str(k)) => Ok(m.get(&**k).cloned().unwrap_or(Value::Nil)),
            (Value::Str(s), Value::Int(n)) => {
                let len = s.chars().count() as i64;
                let n = if *n < 0 { len + n } else { *n };
                Ok(if n >= 0 {
                    s.chars().nth(n as usize).map(|c| Value::str(c.to_string())).unwrap_or(Value::Nil)
                } else {
                    Value::Nil
                })
            }
            (Value::Nil, _) => Ok(Value::Nil),
            _ => fail(line, format!("cannot index {} with {}", o.type_name(), i.type_name())),
        }
    }

    #[inline(never)]
    fn eval_match(&mut self, subj: &Expr, arms: &[Arm], env: &Env, line: u32) -> Result<Value, Ctrl> {
        let v = self.eval(subj, env)?;
        for arm in arms {
            let mut binds = Vec::new();
            if self.match_pat(&arm.pat, &v, Mode::Strict, env, &mut binds)?.is_none() {
                continue;
            }
            let scope = Scope::child(env);
            for (n, x) in binds {
                define(&scope, &n, x);
            }
            if let Some(g) = &arm.guard {
                if !self.eval(g, &scope)?.truthy() {
                    continue;
                }
            }
            return self.eval(&arm.body, &scope);
        }
        fail(line, format!("no `match` arm matches {}", short(&v)))
    }

    #[inline(never)]
    fn eval_for(&mut self, pat: &Pattern, iter: &Expr, body: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let it = self.eval(iter, env)?;
        let items: Vec<Value> = match &it {
            Value::List(l) => l.to_vec(),
            Value::Map(m) => m.iter().map(|(k, v)| Value::list(vec![Value::str(k), v.clone()])).collect(),
            Value::Str(s) => s.chars().map(|c| Value::str(c.to_string())).collect(),
            Value::Nil => Vec::new(),
            other => return fail(line, format!("cannot loop over {}", other.type_name())),
        };
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let scope = Scope::child(env);
            if let Pattern::Bind(n) = pat {
                define(&scope, n, item);
                out.push(self.eval(body, &scope)?);
                Scope::done(scope);
                continue;
            }
            let mut binds = Vec::new();
            if self.match_pat(pat, &item, Mode::Strict, env, &mut binds)?.is_none() {
                return fail(line, format!("`for` pattern does not match {}", short(&item)));
            }
            for (n, x) in binds {
                define(&scope, &n, x);
            }
            out.push(self.eval(body, &scope)?);
        }
        Ok(Value::list(out))
    }

    #[inline(never)]
    fn eval_reply(&mut self, status: &Expr, body: Option<&Expr>, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let code = match self.eval(status, env)? {
            Value::Int(n) if (100..=599).contains(&n) => n as u16,
            other => return fail(line, format!("reply status must be an HTTP status code, got {}", short(&other))),
        };
        let body = match body {
            Some(b) => self.eval(b, env)?,
            None => Value::Nil,
        };
        let resp = match body {
            Value::Response(r) => Response { status: code, ..(*r).clone() },
            b => Response { status: code, headers: Vec::new(), body: Body::Value(b) },
        };
        Err(Ctrl::Reply(Arc::new(resp)))
    }

    #[inline(never)]
    fn eval_html(&mut self, el: &HtmlEl, env: &Env) -> Result<Value, Ctrl> {
        Ok(Value::html(self.render_el(el, env)?))
    }

    /// Calls `callee(args)`, with `pre` (the left side of `|`) as the first
    /// argument. `x.name(args)` calls a function stored in map `x` under
    /// `name`, or else the function `name` with `x` first: `xs.sort()`.
    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        pre: Option<Value>,
        env: &Env,
        line: u32,
    ) -> Result<Value, Ctrl> {
        let mut argv = Vec::with_capacity(args.len() + 2);
        let f = if let Expr::Field(obj, name, _) = callee {
            let recv = self.eval(obj, env)?;
            let method = match &recv {
                Value::Map(m) => m.get(name.as_str()).cloned(),
                _ => None,
            };
            match method {
                Some(f) => f,
                None => match lookup(env, name).filter(|f| matches!(f, Value::Func(_) | Value::Builtin(..))) {
                    Some(f) => {
                        argv.push(recv);
                        f
                    }
                    None => return fail(line, format!("{} has no method `{}`", recv.type_name(), name)),
                },
            }
        } else {
            self.eval(callee, env)?
        };
        if let Some(p) = pre {
            argv.push(p);
        }
        for a in args {
            argv.push(self.eval(a, env)?);
        }
        self.call(&f, argv, line)
    }

    fn get_field(&mut self, v: Value, name: &str, line: u32) -> Result<Value, Ctrl> {
        match &v {
            Value::Nil => return Ok(Value::Nil),
            Value::Map(m) => {
                // A record's few fields are found faster in order than hashed.
                let found = if m.len() <= 8 { m.iter().find(|(k, _)| k.as_str() == name).map(|(_, x)| x) } else { m.get(name) };
                if let Some(x) = found {
                    return Ok(x.clone());
                }
            }
            Value::Response(r) => match name {
                "status" => return Ok(Value::Int(r.status as i64)),
                "headers" => {
                    return Ok(Value::map(r.headers.iter().map(|(k, v)| (k.into(), Value::str(v))).collect()))
                }
                "body" => {
                    return Ok(match &r.body {
                        Body::Value(b) => b.clone(),
                        Body::Bytes(b) => Value::str(String::from_utf8_lossy(b)),
                    })
                }
                _ => {}
            },
            Value::Err(e) if name == "error" => return Ok((**e).clone()),
            _ => {}
        }
        // `xs.len` is `len(xs)`. On maps only `len`, `keys` and `values` work
        // this way, so any other missing key on a record is simply null.
        if let Some(f) = lookup(&self.globals, name) {
            let usable = match (&v, &f) {
                (Value::Map(_), Value::Builtin(..)) => matches!(name, "len" | "keys" | "values"),
                (Value::Map(_), _) => false,
                (_, Value::Func(_) | Value::Builtin(..)) => true,
                _ => false,
            };
            if usable {
                return self.call(&f, vec![v], line);
            }
        }
        if matches!(v, Value::Map(_)) {
            return Ok(Value::Nil);
        }
        fail(line, format!("{} has no field `{}`", v.type_name(), name))
    }

    pub fn call(&mut self, f: &Value, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
        match f {
            Value::Builtin(name, func) => {
                if self.allowed != ALL_EFFECTS {
                    self.check_builtin_effect(name, line)?;
                }
                if !self.tracks.is_empty() && crate::viewcache::impure_builtin(name) {
                    self.mark_impure();
                }
                func(self, args, line)
            }
            Value::Func(c) => {
                let c = c.clone();
                self.call_closure(&c, args.into_iter().map(Some).collect(), line)
            }
            other => fail(line, format!("cannot call {}: it is not a function", other.type_name())),
        }
    }

    /// `args[i] == None` means "not given": the parameter's default is used.
    pub fn call_closure(&mut self, c: &Closure, args: Vec<Option<Value>>, line: u32) -> Result<Value, Ctrl> {
        // `(x) => x.name`, as given to `sort_by`, `map` and `filter`: the
        // field, without a scope for the call.
        if let (Some((field, at)), [Some(_)]) = (projection(&c.def), args.as_slice()) {
            let v = args.into_iter().next().flatten().unwrap_or(Value::Nil);
            return self.get_field(v, field, at);
        }
        let def = c.def.clone();
        let r = self.enter_call(&def, args.len(), line, |this| this.call_closure_inner(c, &def, args, line))?;
        if let (Some(ety), Value::Err(payload)) = (&def.err, &r) {
            if !type_matches(ety, payload) {
                return fail(
                    def.line,
                    format!(
                        "`{}` declares errors as `! {}`, but returned err({})",
                        def.name,
                        ety.text(),
                        short(payload)
                    ),
                );
            }
        }
        Ok(r)
    }

    /// What every call of a Quillo function goes through: the argument
    /// count, the recursion limit, `with` effects and stack growth, around
    /// `body`.
    fn enter_call<R>(
        &mut self,
        def: &Arc<FnDef>,
        nargs: usize,
        line: u32,
        body: impl FnOnce(&mut Self) -> Result<R, Ctrl>,
    ) -> Result<R, Ctrl> {
        if nargs > def.params.len() {
            return fail(
                line,
                format!(
                    "`{}` takes {} argument{}, got {}",
                    def.name,
                    def.params.len(),
                    if def.params.len() == 1 { "" } else { "s" },
                    nargs
                ),
            );
        }
        if self.depth >= MAX_DEPTH {
            return fail(line, format!("too much recursion (more than {} nested calls) in `{}`", MAX_DEPTH, def.name));
        }
        // `with` narrows what the call may do; it cannot widen it.
        let saved = match &def.effects {
            Some(effs) => {
                let mask = effect_mask(effs);
                if mask & !self.allowed != 0 {
                    return fail(line, self.effect_denied(&def.name, mask & !self.allowed));
                }
                let prev_mask = std::mem::replace(&mut self.allowed, mask);
                Some((prev_mask, std::mem::replace(&mut self.restrictor, Some(def.clone()))))
            }
            None => None,
        };
        self.depth += 1;
        // Grow the stack on demand, so deep recursion works on any thread.
        let r = crate::stack::maybe_grow(128 << 10, 4 << 20, || body(self));
        self.depth -= 1;
        if let Some((mask, who)) = saved {
            self.allowed = mask;
            self.restrictor = who;
        }
        r
    }

    /// "`http.get` needs the http effect, but `load` is declared `with db`".
    fn effect_denied(&self, what: &str, missing: u8) -> String {
        let names = effect_names(missing);
        let need = format!("the {} effect{}", names.join(", "), if names.len() == 1 { "" } else { "s" });
        match &self.restrictor {
            Some(f) => format!(
                "`{}` needs {}, but `{}` is declared `with {}`",
                what,
                need,
                f.name,
                f.effects.as_deref().unwrap_or_default().join(", ")
            ),
            None => format!("`{}` needs {}, which is not allowed here", what, need),
        }
    }

    #[cold]
    fn check_builtin_effect(&self, name: &str, line: u32) -> Result<(), Ctrl> {
        let need = builtin_effect(name);
        if need & !self.allowed != 0 {
            return fail(line, self.effect_denied(name, need));
        }
        Ok(())
    }

    fn call_closure_inner(&mut self, c: &Closure, def: &FnDef, args: Vec<Option<Value>>, line: u32) -> Result<Value, Ctrl> {
        let scope = self.bind_args(c, def, args, line)?;
        let r = match self.eval(&def.body, &scope) {
            Ok(v) | Err(Ctrl::Return(v)) => v,
            Err(e) => return Err(e),
        };
        Scope::done(scope);
        if let Some(ty) = &def.ret {
            if !matches!(r, Value::Err(_)) && !type_matches(ty, &r) {
                return fail(
                    def.line,
                    format!("`{}` must return {}, but returned {} {}", def.name, ty_text(ty), r.type_name(), short(&r)),
                );
            }
        }
        Ok(r)
    }

    /// A scope for a call of `c` with its parameters set from `args` (and
    /// defaults), checked against their types.
    fn bind_args(&mut self, c: &Closure, def: &FnDef, args: Vec<Option<Value>>, line: u32) -> Result<Env, Ctrl> {
        let scope = Scope::child(&c.env);
        if def.server {
            if let Some(req) = self.request_value() {
                define(&scope, "request", req.clone());
                self.mark_impure();
            }
        }
        let mut it = args.into_iter();
        for p in &def.params {
            let v = match it.next().flatten() {
                Some(v) => v,
                None => match &p.default {
                    Some(d) => self.eval(d, &scope)?,
                    None => return fail(line, format!("missing argument `{}` in call to `{}`", p.name, def.name)),
                },
            };
            if let Some(ty) = &p.ty {
                if !type_matches(ty, &v) {
                    return fail(
                        line,
                        format!(
                            "argument `{}` of `{}` must be {}, got {} {}",
                            p.name,
                            def.name,
                            ty_text(ty),
                            v.type_name(),
                            short(&v)
                        ),
                    );
                }
            }
            define(&scope, &p.name, v);
        }
        Ok(scope)
    }

    /// Calls `c` and renders what it returns into `out`, writing its HTML
    /// as it goes (see `render_expr_into`).
    fn render_call_into(&mut self, c: &Closure, args: Vec<Option<Value>>, line: u32, out: &mut String) -> Result<(), Ctrl> {
        // A declared result type is checked on the value.
        if c.def.ret.is_some() || c.def.err.is_some() {
            let v = self.call_closure(c, args, line)?;
            render_value(&v, out);
            return Ok(());
        }
        // Views defined at the top level are cached (a nested one would
        // depend on the variables around it too).
        if let (Some(cache), false, true) = (&self.view_cache, c.def.component, Arc::ptr_eq(&c.env, &self.globals)) {
            return self.render_cached(cache.clone(), c, args, line, out);
        }
        self.render_call_plain(c, args, line, out)
    }

    /// A view call through the cache (see `viewcache`).
    fn render_cached(
        &mut self,
        cache: Arc<Mutex<crate::viewcache::ViewCache>>,
        c: &Closure,
        args: Vec<Option<Value>>,
        line: u32,
        out: &mut String,
    ) -> Result<(), Ctrl> {
        use crate::viewcache::Found;
        let found = cache.lock().unwrap_or_else(|e| e.into_inner()).lookup(&c.def, &args, &self.shared);
        match found {
            Found::Hit(entry) => {
                out.push_str(&entry.html);
                if self.reads.is_some() || !self.tracks.is_empty() {
                    for d in &entry.deps {
                        self.note_dep(d.clone());
                    }
                }
                Ok(())
            }
            Found::Uncacheable => self.render_call_plain(c, args, line, out),
            Found::Miss => {
                let seq = self.shared.live_seq();
                self.tracks.push(Default::default());
                let start = out.len();
                let r = self.render_call_plain(c, args.clone(), line, out);
                let track = self.tracks.pop().unwrap_or_default();
                if let Some(outer) = self.tracks.last_mut() {
                    outer.deps.extend(track.deps.iter().cloned());
                    outer.impure |= track.impure;
                }
                r?;
                let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
                if track.impure {
                    cache.forbid(&c.def);
                } else {
                    cache.store(&c.def, args, &out[start..], track.deps, seq);
                }
                Ok(())
            }
        }
    }

    fn render_call_plain(&mut self, c: &Closure, args: Vec<Option<Value>>, line: u32, out: &mut String) -> Result<(), Ctrl> {
        let def = c.def.clone();
        self.enter_call(&def, args.len(), line, |this| {
            let scope = this.bind_args(c, &def, args, line)?;
            let start = out.len();
            match this.render_expr_into(&def.body, &scope, out) {
                Ok(()) => Ok(()),
                // `return` (or `?`) part-way: the result is that value alone.
                Err(Ctrl::Return(v)) => {
                    out.truncate(start);
                    render_value(&v, out);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        })
    }

    // ---- rendering straight into the page -----------------------------
    //
    // `render_expr_into(e)` writes what `render_value(eval(e))` would, without
    // building the value first: elements, loops, conditionals and view calls
    // write their HTML into one buffer as they go, instead of each becoming
    // a string that is copied into its parent's.

    fn render_expr_into(&mut self, e: &Expr, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        match e {
            Expr::Html(el, _) => self.render_el_into(el, env, out),
            Expr::If(c, a, b) => {
                if self.eval(c, env)?.truthy() {
                    self.render_expr_into(a, env, out)
                } else if let Some(b) = b {
                    self.render_expr_into(b, env, out)
                } else {
                    Ok(())
                }
            }
            Expr::Binary(BinOp::And, a, b, _) => {
                let x = self.eval(a, env)?;
                if !x.truthy() {
                    render_value(&x, out);
                    return Ok(());
                }
                self.render_expr_into(b, env, out)
            }
            Expr::Binary(BinOp::Or, a, b, _) => {
                let x = self.eval(a, env)?;
                if x.truthy() {
                    render_value(&x, out);
                    return Ok(());
                }
                self.render_expr_into(b, env, out)
            }
            Expr::For(pat, iter, body, line) => self.render_for_into(pat, iter, body, env, *line, out),
            Expr::Template(parts) => {
                for p in parts {
                    match p {
                        TPart::Lit(t) => escape_into(t, out),
                        TPart::Expr(x) => {
                            let v = self.eval(x, env)?;
                            display_escaped(&v, out);
                        }
                    }
                }
                Ok(())
            }
            Expr::Match(subj, arms, line) => {
                let v = self.eval(subj, env)?;
                for arm in arms {
                    let mut binds = Vec::new();
                    if self.match_pat(&arm.pat, &v, Mode::Strict, env, &mut binds)?.is_none() {
                        continue;
                    }
                    let scope = Scope::child(env);
                    for (n, x) in binds {
                        define(&scope, &n, x);
                    }
                    if let Some(g) = &arm.guard {
                        if !self.eval(g, &scope)?.truthy() {
                            continue;
                        }
                    }
                    return self.render_expr_into(&arm.body, &scope, out);
                }
                fail(*line, format!("no `match` arm matches {}", short(&v)))
            }
            Expr::Block(stmts) => match stmts.split_last() {
                Some((Stmt::Expr(last), rest)) => {
                    let scope = Scope::child(env);
                    for s in rest {
                        self.exec_stmt(s, &scope)?;
                    }
                    self.render_expr_into(last, &scope, out)
                }
                _ => {
                    let v = self.eval_block(stmts, env)?;
                    render_value(&v, out);
                    Ok(())
                }
            },
            _ => {
                let v = self.eval(e, env)?;
                render_value(&v, out);
                Ok(())
            }
        }
    }

    fn render_for_into(
        &mut self,
        pat: &Pattern,
        iter: &Expr,
        body: &Expr,
        env: &Env,
        line: u32,
        out: &mut String,
    ) -> Result<(), Ctrl> {
        let it = self.eval(iter, env)?;
        let mut each = |this: &mut Self, item: &Value| -> Result<(), Ctrl> {
            let scope = Scope::child(env);
            // `for p in items`: nothing to match.
            if let Pattern::Bind(n) = pat {
                define(&scope, n, item.clone());
                return this.render_expr_into(body, &scope, out);
            }
            let mut binds = Vec::new();
            if this.match_pat(pat, item, Mode::Strict, env, &mut binds)?.is_none() {
                return fail(line, format!("`for` pattern does not match {}", short(item)));
            }
            for (n, x) in binds {
                define(&scope, &n, x);
            }
            this.render_expr_into(body, &scope, out)
        };
        match &it {
            Value::List(l) => {
                for item in l.iter() {
                    each(self, item)?;
                }
            }
            Value::Map(m) => {
                for (k, v) in m.iter() {
                    each(self, &Value::list(vec![Value::str(k), v.clone()]))?;
                }
            }
            Value::Str(s) => {
                for c in s.chars() {
                    each(self, &Value::str(c.to_string()))?;
                }
            }
            Value::Nil => {}
            other => return fail(line, format!("cannot loop over {}", other.type_name())),
        }
        Ok(())
    }

    // ---- patterns ------------------------------------------------------

    /// Returns the matched (possibly coerced) value, or `None` on no match.
    /// Bindings are appended to `binds`.
    pub fn match_pat(
        &mut self,
        p: &Pattern,
        v: &Value,
        mode: Mode,
        env: &Env,
        binds: &mut Vec<(String, Value)>,
    ) -> Result<Option<Value>, Ctrl> {
        let lenient = mode == Mode::Lenient;
        Ok(match p {
            Pattern::Wild => Some(v.clone()),
            Pattern::Bind(n) => {
                binds.push((n.clone(), v.clone()));
                Some(v.clone())
            }
            Pattern::Lit(l) => {
                let lv = lit_value(l);
                if equal(&lv, v) {
                    Some(v.clone())
                } else if let (true, Value::Str(s)) = (lenient, v) {
                    coerce(lv.type_name(), s).filter(|c| equal(c, &lv))
                } else {
                    None
                }
            }
            Pattern::Type(t) => {
                if type_name_matches(t, v) {
                    Some(v.clone())
                } else if let (true, Value::Str(s)) = (lenient, v) {
                    coerce(t, s)
                } else {
                    None
                }
            }
            Pattern::Alt(ps) => {
                let mut found = None;
                for q in ps {
                    let mark = binds.len();
                    if let Some(x) = self.match_pat(q, v, mode, env, binds)? {
                        found = Some(x);
                        break;
                    }
                    binds.truncate(mark);
                }
                found
            }
            Pattern::As(q, n) => match self.match_pat(q, v, mode, env, binds)? {
                Some(x) => {
                    binds.push((n.clone(), x.clone()));
                    Some(x)
                }
                None => None,
            },
            Pattern::List(items, rest) => {
                let Value::List(l) = v else { return Ok(None) };
                if l.len() < items.len() || (rest.is_none() && l.len() != items.len()) {
                    return Ok(None);
                }
                for (q, x) in items.iter().zip(l.iter()) {
                    if self.match_pat(q, x, mode, env, binds)?.is_none() {
                        return Ok(None);
                    }
                }
                if let Some(Some(n)) = rest {
                    binds.push((n.clone(), Value::list(l[items.len()..].to_vec())));
                }
                Some(v.clone())
            }
            Pattern::Record(fields) => {
                if !matches!(v, Value::Map(_)) {
                    return Ok(None);
                }
                for f in fields {
                    let sub_mode = match mode {
                        Mode::Request if f.path[0] == "json" => Mode::Strict,
                        Mode::Request => Mode::Lenient,
                        m => m,
                    };
                    let found = get_path(v, &f.path);
                    let val = match (found, &f.default) {
                        (Some(x), _) if !matches!(x, Value::Nil) => x,
                        (_, Some(d)) => {
                            // A default is taken as written: `limit: int = null`
                            // binds null when the field is missing.
                            let d = self.eval(d, env)?;
                            if f.pat.is_type_test() {
                                binds.push((f.path.last().unwrap().clone(), d));
                                continue;
                            }
                            d
                        }
                        (Some(x), None) => x,
                        (None, None) => return Ok(None),
                    };
                    let mark = binds.len();
                    let Some(x) = self.match_pat(&f.pat, &val, sub_mode, env, binds)? else {
                        return Ok(None);
                    };
                    // `{ limit: int }` binds `limit`.
                    if binds.len() == mark && f.pat.is_type_test() {
                        binds.push((f.path.last().unwrap().clone(), x));
                    }
                }
                Some(v.clone())
            }
        })
    }

    // ---- HTML ----------------------------------------------------------

    /// A tag's attributes: what `{...map}` spreads, and the ones written out.
    fn eval_attrs(&mut self, el: &HtmlEl, env: &Env) -> Result<(Vec<(String, Value)>, Vec<(String, Value)>), Ctrl> {
        let mut spread = Vec::new();
        let mut explicit = Vec::new();
        for (name, val) in &el.attrs {
            match val {
                AttrVal::Bool => explicit.push((name.clone(), Value::Bool(true))),
                AttrVal::Expr(e) => explicit.push((name.clone(), self.eval(e, env)?)),
                AttrVal::Spread(e) => match self.eval(e, env)? {
                    Value::Map(m) => spread.extend(m.iter().map(|(k, v)| (k.to_string(), v.clone()))),
                    Value::Nil => {}
                    other => {
                        return fail(el.line, format!("`{{...}}` in <{}> needs a map, got {}", el.tag, other.type_name()))
                    }
                },
            }
        }
        Ok((spread, explicit))
    }

    fn render_el(&mut self, el: &HtmlEl, env: &Env) -> Result<String, Ctrl> {
        let mut out = String::new();
        self.render_el_into(el, env, &mut out)?;
        Ok(out)
    }

    fn render_el_into(&mut self, el: &HtmlEl, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        if el.tag.chars().next().map_or(false, |c| c.is_uppercase()) {
            return self.render_component_into(el, env, out);
        }
        if el.tag == "html" {
            out.push_str("<!doctype html>");
        }
        out.push('<');
        out.push_str(&el.tag);
        // Most elements have no spread, no `class=` and are no <textarea>:
        // their attributes are written as they are evaluated.
        let fast = el.tag != "textarea" && el.attrs.iter().all(|(n, a)| !matches!(a, AttrVal::Spread(_)) && n != "class");
        let text_value = if fast { self.render_attrs_fast(el, env, out)?; None } else { self.render_attrs(el, env, out)? };
        out.push('>');
        if VOID_TAGS.contains(&el.tag.as_str()) {
            return Ok(());
        }
        match text_value {
            Some(t) => out.push_str(&escape(&t)),
            // A style sheet is raw text to the browser: `.a > b` must stay
            // as written. Only `</` is changed, so the text cannot end the
            // element early.
            None if el.tag == "style" => {
                for c in &el.children {
                    if let HtmlChild::Expr(e) = c {
                        let v = self.eval(e, env)?;
                        let text = match &v {
                            Value::Nil | Value::Bool(false) => String::new(),
                            Value::List(items) => items.iter().map(|x| x.display()).collect::<Vec<_>>().join("\n"),
                            other => other.display(),
                        };
                        out.push_str(&text.replace("</", "<\\/"));
                    }
                }
            }
            None => {
                for c in &el.children {
                    self.render_child(c, env, out)?;
                }
            }
        }
        out.push_str("</");
        out.push_str(&el.tag);
        out.push('>');
        Ok(())
    }

    /// Attributes in general: spread maps, `class=` merged with `.classes`,
    /// a <textarea>'s value as its text (returned).
    fn render_attrs(&mut self, el: &HtmlEl, env: &Env, out: &mut String) -> Result<Option<String>, Ctrl> {
        if let Some(id) = &el.id {
            let v = self.eval(id, env)?;
            out.push_str(&format!(" id=\"{}\"", escape(&v.display())));
        }
        let mut classes = Vec::new();
        for c in &el.classes {
            classes.push(self.eval(c, env)?.display());
        }
        let mut rest = String::new();
        // Written-out attributes win over spread ones, except `class`,
        // where both count.
        let (spread, explicit) = self.eval_attrs(el, env)?;
        // Spread keys are data, often from outside: only plain attribute
        // names, and no event handlers (`onclick`) or `srcdoc`, which run
        // the value as code or as a page.
        let spread: Vec<(String, Value)> = spread.into_iter().filter(|(n, _)| spread_attr_allowed(n)).collect();
        let written: HashSet<&str> =
            explicit.iter().map(|(n, _)| n.strip_prefix("bind:").unwrap_or(n.as_str())).collect();
        // Written classes first, then the spread's.
        let (spread_class, spread): (Vec<_>, Vec<_>) = spread.into_iter().partition(|(n, _)| n == "class");
        let mut entries: Vec<(String, Value)> = spread
            .into_iter()
            .filter(|(n, _)| !written.contains(n.as_str()) && !(n == "id" && el.id.is_some()))
            .collect();
        entries.extend(explicit.iter().cloned());
        entries.extend(spread_class);
        // A textarea's value is its text, not an attribute.
        let mut text_value: Option<String> = None;
        for (name, v) in entries {
            // `@click` handlers only exist in the browser; `bind:value={x}`
            // renders as `value`.
            if name.starts_with('@') || name == "key" {
                continue;
            }
            let name = name.strip_prefix("bind:").unwrap_or(&name).to_string();
            let name = name.as_str();
            if !attr_name_ok(name) {
                continue;
            }
            let v = match (&v, name) {
                (Value::Str(s), _) if URL_ATTRS.contains(&name) && script_url(s) => Value::str(BLOCKED_URL),
                (Value::Str(s), "href" | "src" | "action" | "formaction" | "poster") if !self.base.is_empty() => {
                    Value::str(self.with_base(s))
                }
                _ => v,
            };
            if name == "value" && el.tag == "textarea" {
                text_value = Some(if matches!(v, Value::Nil) { String::new() } else { v.display() });
                continue;
            }
            if name == "class" {
                if !matches!(v, Value::Nil | Value::Bool(false)) {
                    classes.push(v.display());
                }
                continue;
            }
            match v {
                Value::Bool(true) => {
                    rest.push(' ');
                    rest.push_str(name);
                }
                Value::Bool(false) | Value::Nil => {}
                Value::List(_) | Value::Map(_) => {
                    rest.push_str(&format!(" {}=\"{}\"", name, escape(&v.json_text())))
                }
                other => rest.push_str(&format!(" {}=\"{}\"", name, escape(&other.display()))),
            }
        }
        if !classes.is_empty() {
            out.push_str(&format!(" class=\"{}\"", escape(&classes.join(" "))));
        }
        out.push_str(&rest);
        Ok(text_value)
    }

    /// Attributes of an element without spreads or `class=`, written into
    /// `out` as they are evaluated.
    fn render_attrs_fast(&mut self, el: &HtmlEl, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        if let Some(id) = &el.id {
            let v = self.eval(id, env)?;
            out.push_str(" id=\"");
            display_escaped(&v, out);
            out.push('"');
        }
        if !el.classes.is_empty() {
            out.push_str(" class=\"");
            for (i, c) in el.classes.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let v = self.eval(c, env)?;
                display_escaped(&v, out);
            }
            out.push('"');
        }
        for (name, val) in &el.attrs {
            // `@click` handlers only exist in the browser, `key` only matters
            // there; `bind:value={x}` renders as `value`.
            if name.starts_with('@') || name == "key" {
                continue;
            }
            let name = name.strip_prefix("bind:").unwrap_or(name);
            if !attr_name_ok(name) {
                continue;
            }
            match val {
                AttrVal::Bool => {
                    out.push(' ');
                    out.push_str(name);
                }
                // A template outside URL attributes goes straight in, part by part.
                AttrVal::Expr(Expr::Template(parts)) if !URL_ATTRS.contains(&name) => {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    for p in parts {
                        match p {
                            TPart::Lit(t) => escape_into(t, out),
                            TPart::Expr(x) => {
                                let v = self.eval(x, env)?;
                                display_escaped(&v, out);
                            }
                        }
                    }
                    out.push('"');
                }
                // In a URL attribute, the whole value is checked first.
                AttrVal::Expr(Expr::Template(parts)) => {
                    let mut url = std::mem::take(&mut self.spare);
                    url.clear();
                    let r = self.template_into(parts, env, &mut url);
                    if r.is_ok() {
                        self.write_url_attr(name, &url, out);
                    }
                    self.spare = url;
                    r?;
                }
                AttrVal::Expr(e) => {
                    let v = self.eval(e, env)?;
                    self.write_attr(name, v, out);
                }
                AttrVal::Spread(_) => unreachable!("render_attrs handles spreads"),
            }
        }
        Ok(())
    }

    /// A template's text, appended to `out` (unescaped).
    fn template_into(&mut self, parts: &[TPart], env: &Env, out: &mut String) -> Result<(), Ctrl> {
        for p in parts {
            match p {
                TPart::Lit(t) => out.push_str(t),
                TPart::Expr(x) => match self.eval(x, env)? {
                    Value::Str(t) => out.push_str(&t),
                    Value::Int(n) => push_int(out, n),
                    v => out.push_str(&v.display()),
                },
            }
        }
        Ok(())
    }

    /// ` name="url"` for a URL attribute (see `write_attr`).
    fn write_url_attr(&self, name: &str, url: &str, out: &mut String) {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        if script_url(url) {
            out.push_str(BLOCKED_URL);
        } else if !self.base.is_empty() && matches!(name, "href" | "src" | "action" | "formaction" | "poster") {
            escape_into(&self.with_base(url), out);
        } else {
            escape_into(url, out);
        }
        out.push('"');
    }

    /// ` name="value"`, or ` name` for true, nothing for false and null.
    /// URLs that would run script are blocked; site paths get the mount's
    /// prefix.
    fn write_attr(&self, name: &str, v: Value, out: &mut String) {
        let v = match (&v, name) {
            (Value::Str(s), _) if URL_ATTRS.contains(&name) && script_url(s) => Value::str(BLOCKED_URL),
            (Value::Str(s), "href" | "src" | "action" | "formaction" | "poster") if !self.base.is_empty() => {
                Value::str(self.with_base(s))
            }
            _ => v,
        };
        match &v {
            Value::Bool(true) => {
                out.push(' ');
                out.push_str(name);
            }
            Value::Bool(false) | Value::Nil => {}
            Value::List(_) | Value::Map(_) => {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                escape_into(&v.json_text(), out);
                out.push('"');
            }
            other => {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                display_escaped(other, out);
                out.push('"');
            }
        }
    }

    fn render_child(&mut self, c: &HtmlChild, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        match c {
            HtmlChild::El(el) => self.render_el_into(el, env, out),
            HtmlChild::Expr(e) => self.render_expr_into(e, env, out),
        }
    }

    /// `<Card user={u}>"kids"</Card>` calls view `Card` with its parameters
    /// taken from attributes by name; a `children` parameter gets the body.
    fn render_component_into(&mut self, el: &HtmlEl, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        let line = el.line;
        let Some(f) = lookup(env, &el.tag) else {
            return fail(line, format!("unknown view <{}>", el.tag));
        };
        let Value::Func(c) = f else {
            return fail(line, format!("<{}> is not a view", el.tag));
        };
        // The body is only rendered for a view that takes it.
        let takes_children = c.def.params.iter().any(|p| p.name == "children");
        let mut kids = String::new();
        if takes_children {
            for ch in &el.children {
                self.render_child(ch, env, &mut kids)?;
            }
        }
        // A view called without spreads takes its arguments straight from
        // the attributes, in parameter order.
        if !c.def.component && !el.attrs.iter().any(|(_, a)| matches!(a, AttrVal::Spread(_))) {
            let params = &c.def.params;
            if let Some((n, _)) =
                el.attrs.iter().find(|(n, _)| n != "key" && (n == "children" || !params.iter().any(|p| p.name == *n)))
            {
                return fail(line, format!("<{}> has no parameter `{}`", el.tag, n));
            }
            let mut args = Vec::with_capacity(params.len());
            for p in params {
                if p.name == "children" {
                    args.push(Some(Value::html(std::mem::take(&mut kids))));
                    continue;
                }
                args.push(match el.attrs.iter().find(|(n, _)| *n == p.name) {
                    Some((_, AttrVal::Bool)) => Some(Value::Bool(true)),
                    Some((_, AttrVal::Expr(e))) => Some(self.eval(e, env)?),
                    _ => None,
                });
            }
            return self.render_call_into(&c, args, line, out);
        }
        let (spread, explicit) = self.eval_attrs(el, env)?;
        let mut given: Vec<(String, Value)> = explicit.into_iter().filter(|(n, _)| n != "key").collect();
        // Spread values fill parameters not written out; other spread keys
        // (extra fields of the data) are ignored.
        for (k, v) in spread {
            if k != "key" && k != "children" && !given.iter().any(|(n, _)| *n == k) && c.def.params.iter().any(|p| p.name == k) {
                given.push((k, v));
            }
        }
        let component = c.def.component;
        let mut args = Vec::with_capacity(c.def.params.len());
        // Props travel to the browser with a component; a view needs none.
        let mut props = Map::new();
        for p in &c.def.params {
            if p.name == "children" {
                if component {
                    props.insert(p.name.as_str().into(), Value::str(&kids));
                }
                args.push(Some(Value::html(std::mem::take(&mut kids))));
                continue;
            }
            let pos = given.iter().position(|(n, _)| n == &p.name);
            let v = pos.map(|i| given.remove(i).1);
            if let (true, Some(v)) = (component, &v) {
                props.insert(p.name.as_str().into(), v.clone());
            }
            args.push(v);
        }
        if let Some((n, _)) = given.first() {
            return fail(line, format!("<{}> has no parameter `{}`", el.tag, n));
        }
        if !component {
            return self.render_call_into(&c, args, line, out);
        }
        // A component renders inside an island that carries its props and
        // state to the browser, which takes over from there. A component
        // inside another one gets its props from its parent in the browser
        // too, so only its state travels: its props may be anything,
        // callbacks and HTML included.
        let nested = !self.islands.is_empty();
        self.rendered_island = true;
        self.mark_impure();
        self.islands.push((Map::new(), Map::new()));
        let mut body = String::new();
        let r = self.render_call_into(&c, args, line, &mut body);
        let (state, live) = self.islands.pop().unwrap_or_default();
        r?;
        let props = if nested { Map::new() } else { props };
        for (what, m) in [("prop", &props), ("state", &state)] {
            for (k, x) in m.iter() {
                if let Some(bad) = not_sendable(x) {
                    return fail(
                        line,
                        if what == "prop" {
                            format!(
                                "prop `{}` of <{}> is {}, which cannot be sent to the browser; \
                                 pass it from inside another component, or use a server fn",
                                k, el.tag, bad
                            )
                        } else {
                            format!("{} `{}` of <{}> is {}, which cannot be sent to the browser", what, k, el.tag, bad)
                        },
                    );
                }
            }
        }
        let live_attrs = if live.is_empty() {
            String::new()
        } else {
            format!(
                " data-l=\"{}\" data-v=\"{}:{}\"",
                escape(&Value::map(live).json_text()),
                self.shared.live_seq(),
                self.shared.id
            )
        };
        out.push_str(&format!(
            "<quillo-island style=\"display:contents\" data-c=\"{}\" data-p=\"{}\" data-s=\"{}\"{}>",
            escape(&el.tag),
            escape(&island_json(&Value::map(props)).to_string()),
            escape(&island_json(&Value::map(state)).to_string()),
            live_attrs
        ));
        out.push_str(&body);
        out.push_str("</quillo-island>");
        Ok(())
    }
}
