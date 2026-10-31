//! Tree-walking evaluator: expressions, patterns and HTML rendering.

use crate::ast::*;
use crate::decimal::Dec;
use crate::error::QuilloError;
use crate::parser::Parser;
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

/// Builtins that change something: a page prefetched does not call them
/// (see `Interp::no_writes`).
const CHANGES: &[&str] = &[
    "db.exec",
    "db.migrate",
    "db.atomic",
    "idempotent",
    "set_cookie",
    "delete_cookie",
    "set_session",
    "end_session",
    "write_file",
    "delete_file",
    "save_upload",
    "publish",
    "http.post",
    "http.put",
    "http.patch",
    "http.delete",
    "image.convert",
    "__service",
    "__service_later",
];

/// A run-time error, told the file it happened in (as the program was
/// given it: relative to the folder it runs in, when it is inside it)
/// unless it knows already.
pub(crate) fn in_file<T>(r: Result<T, Ctrl>, file: &Option<Arc<str>>) -> Result<T, Ctrl> {
    match (r, file) {
        (Err(Ctrl::Error(e)), Some(f)) if e.wants_file() => Err(Ctrl::Error(e.in_file(&crate::error::shown_path(f)))),
        (r, _) => r,
    }
}

pub fn fail<T>(line: u32, msg: impl Into<String>) -> Result<T, Ctrl> {
    Err(Ctrl::Error(QuilloError::new(msg, line)))
}

fn undefined(name: &str, line: u32) -> Ctrl {
    Ctrl::Error(QuilloError::new(format!("unknown name `{}`{}", name, crate::error::removed_note(name)), line))
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
    pub(crate) loaded: Arc<HashSet<PathBuf>>,
    /// What each file's names are called in the program (src/modules.rs).
    links: Arc<crate::modules::Links>,
    /// Files of programs this one mounts or calls as services: watched
    /// with its own (`--watch`), but not its own, so a file both `use`
    /// is still run here (FEEDBACK4.md, Q-040; branch bank-fixes).
    pub(crate) watched: Arc<HashSet<PathBuf>>,
    pub shared: Arc<Shared>,
    /// The services this version of the program loaded (see `shared::Services`).
    pub(crate) services: Arc<crate::shared::Services>,
    /// Calls in progress on this thread, to stop runaway recursion cleanly.
    depth: usize,
    /// The file of the function running, and of the one that called it:
    /// a run-time error is told where it happened (see `enter_call`).
    pub(crate) cur_file: Option<Arc<str>>,
    caller_file: Option<Arc<str>>,
    /// Calls and loop steps left before a sandboxed script is stopped
    /// (`run_script`); `u64::MAX` everywhere else.
    pub(crate) fuel: u64,
    /// Effects the running code may use, and the `with` function that limited them.
    pub(crate) allowed: u8,
    /// While a sandboxed script runs: its outermost scope, and the effects
    /// the program had (see `script_mask`).
    pub(crate) script: Option<(Env, u8)>,
    pub(crate) restrictor: Option<Arc<FnDef>>,
    /// Names of `server fn`s, callable from the browser.
    pub server_fns: Arc<Vec<String>>,
    /// What the program set about its site (see `site::Site`).
    pub site: Arc<crate::site::Site>,
    /// The current request's tenant, once worked out (`None` until then).
    pub tenant: Option<Option<Arc<str>>>,
    /// The current request's id (`x-request-id`, given or made), which calls
    /// to services pass on (src/service.rs).
    pub request_id: Option<Arc<str>>,
    /// When the service call being answered must be done by: its caller's
    /// time left, which calls it makes in turn may wait no longer than
    /// (FEEDBACK6.md, Q-056; branch feedback6-fixes).
    pub deadline: Option<std::time::Instant>,
    /// Whether the program named the tenant (`with_tenant`), rather than a
    /// request's host: only then may its database be made.
    pub tenant_named: bool,
    /// This request asked a tenant's database that is not there (see `tenant_backend`).
    pub no_such_tenant: bool,
    /// The current request's body when it carries files (see `upload`).
    pub upload_body: Option<Arc<[u8]>>,
    /// `Set-Cookie` lines for the response being made (see `session`).
    pub set_cookies: Vec<String>,
    /// The request's session, once read or set.
    pub session: Option<Value>,
    /// The locale the code runs in, once worked out (see `i18n`).
    pub locale: Option<Arc<str>>,
    /// Every statement the program has loaded, for the browser compiler.
    pub program: Arc<Vec<Arc<Vec<Stmt>>>>,
    /// The browser bundle (`/_quillo/quillo.js`), built after loading; in
    /// development, made when first asked for (see `client_js`).
    pub client_js: Arc<String>,
    client_lazy: Arc<std::sync::OnceLock<Arc<String>>>,
    /// Whether the program names `db.atomic` (then transactions it left
    /// prepared are looked after: see `db::recover`). Noted as the files are
    /// read: it was found by printing the whole program as text and searching
    /// that, 17 ms of every reload (FEEDBACK15.md, Q-098; branch dev-maps-once).
    pub uses_atomic: bool,
    /// The request being handled, visible as `request` inside `server fn`s.
    pub current_request: Option<Value>,
    /// The request being handled, when its map has not been made yet (see
    /// `RouteDef::uses_request`): made on first use by `request_value`.
    pub(crate) pending_request: Option<crate::server::PendingRequest>,
    /// State of the components being rendered, innermost last, with the
    /// `live(...)` subscriptions of each.
    islands: Vec<(Map, Map)>,
    /// While the server renders a component: its code also runs in the
    /// browser, where `http.*` calls are made (see src/fetch.rs). A
    /// `server fn` it calls runs on the server, and clears it.
    pub(crate) in_component: bool,
    /// While a `live` loader runs: the topics (tables, globals) it reads.
    pub reads: Option<std::collections::HashSet<String>>,
    /// The code read the request (its session, cookies, headers): what it
    /// made is this viewer's (FEEDBACK2.md, Q-028; branch feedback6-fixes).
    pub read_request: bool,
    /// The subscription the last `live(...)` call set up, for `state`.
    pub last_live: Option<Value>,
    /// Open `db.transaction`s, and changes to announce when they commit.
    pub tx_depth: usize,
    pub pending: Vec<String>,
    /// The same topics as `pending`, as the program wrote them (before the
    /// tenant and mount point are added): what a service sends back.
    pub(crate) pending_raw: Vec<String>,
    /// While answering another service's call: the topics announced, sent
    /// back with the answer for the caller to announce to its own pages
    /// (FEEDBACK4.md, Q-038; branch bank-fixes).
    pub(crate) published: Option<Vec<String>>,
    /// Open SQLite transactions. SQLite has one connection, shared by every
    /// request, so while one is open the lock is kept (see `unlocked`).
    pub sqlite_tx: usize,
    /// The Postgres connection this request's `db.transaction` holds.
    pub pg_tx: std::sync::Mutex<Vec<crate::db::TxConn>>,
    /// This program's database (SQLite or a Postgres pool), opened on first use.
    pub db: Arc<std::sync::Mutex<Option<crate::db::DbSlot>>>,
    /// Databases besides `db` (`db.connect`): each with its label for live
    /// updates, shared by the program's forks.
    pub(crate) db_conns: Arc<std::sync::Mutex<Vec<(String, crate::db::DbSlot)>>>,
    /// The connection a `db` function was just called on (`analytics.query`),
    /// taken by the function as it starts.
    pub(crate) db_on: Option<usize>,
    /// The request (its number) that last wrote to a database: its reads go
    /// to the primary, not a replica, so it sees what it wrote.
    pub(crate) wrote_req: Option<usize>,
    /// While a page is prefetched (a link pointed at, before it is clicked):
    /// set when the code tried to change something, which stopped it (see
    /// `no_writes`). Shared with what it runs in parallel.
    pub(crate) prefetching: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Tenant databases whose function (`db.open((tenant) => ...)`) is
    /// running, which may not use them itself.
    pub(crate) db_resolving: Vec<usize>,
    /// Why the function picking a tenant's database failed, when it was by
    /// using that database (said even if the function went on regardless).
    pub(crate) db_loop: Option<String>,
    /// Where this program is mounted ("" at the root, "/notes" under `mount`).
    /// Root-relative links, redirects and browser URLs get it as a prefix.
    pub base: String,
    /// Programs mounted under a path prefix: `mount /notes = "notes.qwe"`.
    pub mounts: Vec<(String, Interp)>,
    /// Services with a prefix that run elsewhere: their routes are passed
    /// on to them (`service /shop = "shop.qwe"`, src/service.rs).
    pub remote_mounts: Vec<(String, String)>,
    /// Loaded as a service (src/service.rs): its server fns are called by
    /// other services, signed, never by browsers (`/_quillo/rpc`).
    pub is_service: bool,
    /// Set when a reload has replaced this version of the program (shared by
    /// its forks): a job of it still running ends at its next `sleep`, so the
    /// new version's starts (FEEDBACK12.md, Q-082; branch feedback12-fixes).
    pub(crate) retired: Arc<std::sync::atomic::AtomicBool>,
    /// The sequence numbers the running live loader's own writes were given:
    /// what it wrote and also read does not wake it again (FEEDBACK13.md,
    /// Q-090; branch feedback13-fixes).
    pub(crate) own_seqs: Vec<u64>,
    /// Running a scheduled job or a tick, not a request: its `sleep` ends at a
    /// stop or a reload (FEEDBACK12.md, Q-081 and Q-082; branch feedback12-sleep).
    pub(crate) in_job: bool,
    /// A service's own `.env`, beside its file (FEEDBACK9.md, Q-071; branch
    /// feedback9-fixes). The app's is in the process's environment.
    pub env_file: Arc<std::collections::HashMap<String, String>>,
    /// The services the program declares, and the checks it adds with
    /// `ready`, for `/_quillo/ready` (FEEDBACK10.md, Q-076; branch
    /// feedback10-fixes).
    pub declared_services: Arc<Vec<String>>,
    pub ready_checks: Arc<Vec<(String, Value)>>,
    /// The command components' styles go through (`style_with`; FEEDBACK11.md,
    /// Q-079; branch feedback11-fixes).
    pub style_with: Option<String>,
    /// The stream `send` writes to, while a `stream`'s function runs.
    pub(crate) stream_out: Option<crate::stream::Out>,
    /// The function of the streamed answer `handle` just made, for the
    /// server to run once it has sent the head (see `streams_direct`).
    pub(crate) pending_stream: Option<crate::stream::Pending>,
    /// Set by the server: `handle` leaves a streamed answer's function to it,
    /// rather than make the whole answer first.
    pub(crate) streams_direct: bool,
    /// The program's own types (`type User = { ... }`: src/record.rs).
    pub(crate) types: Arc<crate::record::Types>,
    /// Browser libraries, by the name the program calls each: `client
    /// "https://.../chart.umd.min.js" use Chart` (branch js-libraries).
    pub(crate) js_libs: Arc<std::collections::BTreeMap<String, crate::client::JsLib>>,
    /// Whether the response being made has a component island, and so
    /// needs the browser bundle.
    pub rendered_island: bool,
    /// The components this request's page was rendered with: the browser
    /// loads a bundle of those (and what they use) alone.
    pub island_names: Vec<String>,
    /// The page's waiting components, rendered again after it (src/defer.rs;
    /// branch defer), the deferred states of the island rendering now, and the
    /// states a second render takes as they were.
    pub(crate) deferred: Vec<crate::defer::Deferred>,
    island_defers: Vec<String>,
    state_preset: Option<Map>,
    /// Bundles for sets of components, by their names: (hash, script).
    /// Shared by every handle on the program, made again when it loads.
    pub bundles: Arc<std::sync::Mutex<crate::client::Bundles>>,
    /// Components' and views' own CSS, and the pages' sheets made of it.
    pub styles: Arc<std::sync::Mutex<crate::styles::Styles>>,
    /// A buffer lent to the renderer for building attribute values.
    spare: String,
    /// What views rendered, shared by the workers (see `viewcache`).
    pub view_cache: Option<Arc<Mutex<crate::viewcache::ViewCache>>>,
    /// The renders being cached, innermost last: what each has read.
    tracks: Vec<crate::viewcache::Track>,
    /// The calls in progress, while a debugger is attached (src/debug.rs).
    pub(crate) trace: crate::debug::Trace,
}

/// Deepest call nesting allowed before a "too much recursion" error.
pub const MAX_DEPTH: usize = 10_000;

/// What stops a prefetched page that would change something; the server
/// answers it "not prefetched" and does not log it.
pub const PREFETCH_REFUSED: &str = "not while the page is prefetched";

/// A source file's absolute path, as the parser records it on what it
/// parses (see `FnDef::file`).
/// Programs parsed, by file, with a hash of the text each was parsed from: a
/// reload under --watch parses only the files that changed, not a 4 MB kit
/// file again (FEEDBACK15.md, Q-098; branch dev-maps-once). One parse is kept
/// per file: a file's new text replaces its old parse.
static PARSED: Mutex<Vec<(Arc<str>, u64, Arc<Vec<Stmt>>)>> = Mutex::new(Vec::new());

/// A file parsed, its names as `names` says they are called in the program (src/modules.rs).
fn parse_cached(src: &str, abs: Option<Arc<str>>, file: &str, names: Option<&std::collections::HashMap<String, String>>) -> Result<Arc<Vec<Stmt>>, QuilloError> {
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        src.hash(&mut h);
        let mut renamed: Vec<_> = names.iter().flat_map(|m| m.iter()).collect();
        renamed.sort();
        renamed.hash(&mut h);
        h.finish()
    };
    if let Some(abs) = &abs {
        let kept = PARSED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, _, stmts)) = kept.iter().find(|(p, h, _)| p == abs && *h == hash) {
            return Ok(stmts.clone());
        }
    }
    let mut stmts = Parser::new(src).in_file(abs.clone()).parse_program().map_err(|e| e.in_file(file))?;
    if let Some(names) = names {
        crate::privacy::rename_as(&mut stmts, names, "a name another file has too").map_err(|e| e.in_file(file))?;
    }
    let stmts = Arc::new(stmts);
    if let Some(abs) = abs {
        let mut kept = PARSED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.retain(|(p, _, _)| *p != abs);
        kept.push((abs, hash, stmts.clone()));
    }
    Ok(stmts)
}

fn source_path(file: &Path) -> Option<Arc<str>> {
    let p = std::fs::canonicalize(file).ok()?;
    Some(Arc::from(p.display().to_string()))
}

pub fn lit_value(l: &Lit) -> Value {
    match l {
        Lit::Nil => Value::Nil,
        Lit::Bool(b) => Value::Bool(*b),
        Lit::Int(n) => Value::Int(*n),
        Lit::Float(f) => Value::Float(*f),
        Lit::Dec(d) => Value::Dec(*d),
        Lit::Str(s) => Value::Str(s.clone()),
    }
}

pub fn type_name_matches(t: &str, v: &Value) -> bool {
    match t {
        "any" => true,
        "int" => matches!(v, Value::Int(_)),
        "float" => matches!(v, Value::Float(_)),
        "dec" => matches!(v, Value::Dec(_)),
        "num" => matches!(v, Value::Int(_) | Value::Float(_)),
        "str" => matches!(v, Value::Str(_)),
        // Text that is a date, not a date-time (FEEDBACK13.md, Q-093; branch open-fixes).
        "date" => matches!(v, Value::Str(s) if crate::dates::parse_date(s).is_some()),
        "bool" => matches!(v, Value::Bool(_)),
        "list" => matches!(v, Value::List(_)),
        "map" => matches!(v, Value::Map(_)),
        "html" => matches!(v, Value::Html(_)),
        "fn" => matches!(v, Value::Func(_) | Value::Builtin(..)),
        "response" => matches!(v, Value::Response(_)),
        _ => false,
    }
}

/// Whether `v` is of type `ty`, as a builtin type: the program's own types
/// are known to `Interp::fits`.
pub fn type_matches(ty: &Ty, v: &Value) -> bool {
    (ty.optional && matches!(v, Value::Nil)) || ty.names.iter().any(|n| type_name_matches(n, v))
}

/// " (`email` is missing)": what a record lacks, after a mismatch.
pub(crate) fn because(why: &crate::record::Why) -> String {
    why.text().map_or(String::new(), |t| format!(" ({})", t))
}

impl Interp {
    /// Whether `v` is of type `ty`, records (src/record.rs) included.
    pub(crate) fn fits(&self, ty: &Ty, v: &Value) -> bool {
        crate::record::fits(&self.types, ty, v, false).is_ok()
    }
}

fn ty_text(ty: &Ty) -> String {
    ty.text()
}

/// A value as a decimal, for a `dec` pattern or record field: a decimal,
/// an int, numeric text ("12.50", from a query, a form or JSON) or a float
/// (a JSON number, through its shortest text).
pub fn as_dec(v: &Value) -> Option<Dec> {
    match v {
        Value::Dec(d) => Some(*d),
        Value::Int(n) => Some(Dec::from_i64(*n)),
        Value::Float(f) => Dec::from_f64(*f),
        Value::Str(s) => Dec::parse(s.trim()),
        _ => None,
    }
}

/// Converts request text to the type a pattern asks for.
pub fn coerce(t: &str, s: &str) -> Option<Value> {
    let s = s.trim();
    match t {
        "int" => s.parse::<i64>().ok().map(Value::Int),
        "float" => s.parse::<f64>().ok().map(Value::Float),
        "num" => s.parse::<i64>().ok().map(Value::Int).or_else(|| s.parse::<f64>().ok().map(Value::Float)),
        "dec" => Dec::parse(s).map(Value::Dec),
        "date" => crate::dates::parse_date(s).map(|_| Value::str(s)),
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
    fn entity(b: u8) -> Option<&'static str> {
        Some(match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&#39;",
            _ => return None,
        })
    }
    // Eight bytes at a time: whether any is one of the five (a zero byte
    // after xor with it, found without a branch per byte).
    const LO: u64 = 0x0101_0101_0101_0101;
    const HI: u64 = 0x8080_8080_8080_8080;
    let has = |x: u64, c: u8| {
        let y = x ^ (LO * c as u64);
        y.wrapping_sub(LO) & !y & HI
    };
    let b = s.as_bytes();
    let (mut done, mut i) = (0, 0);
    while i + 8 <= b.len() {
        let x = u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        if has(x, b'&') | has(x, b'<') | has(x, b'>') | has(x, b'"') | has(x, b'\'') != 0 {
            for j in i..i + 8 {
                if let Some(e) = entity(b[j]) {
                    out.push_str(&s[done..j]);
                    out.push_str(e);
                    done = j + 1;
                }
            }
        }
        i += 8;
    }
    for j in i..b.len() {
        if let Some(e) = entity(b[j]) {
            out.push_str(&s[done..j]);
            out.push_str(e);
            done = j + 1;
        }
    }
    out.push_str(&s[done..]);
}

/// Attributes whose value the browser may load or run as a URL.
pub const URL_ATTRS: &[&str] =
    &["href", "src", "action", "formaction", "poster", "xlink:href", "cite", "background", "data", "codebase"];

/// Whether an attribute is one of `URL_ATTRS`, however it is written
/// (HTML attribute names ignore case: `HREF` is `href`).
pub fn url_attr(name: &str) -> bool {
    URL_ATTRS.iter().any(|a| a.eq_ignore_ascii_case(name))
}

/// What a `javascript:` URL in one of `URL_ATTRS` becomes.
pub const BLOCKED_URL: &str = "about:blank#blocked";

/// Whether an attribute of an SVG animation gives its value to another
/// attribute (`<set attributeName="href" to={u}>`), which may be a URL one:
/// `to`, `from`, `by`, and `values`, a list split by `;`. Their values are
/// checked as URLs whatever the attribute animated, since a `javascript:`
/// value means nothing anywhere else.
pub fn anim_value_attr(tag: &str, name: &str) -> bool {
    (tag.eq_ignore_ascii_case("set") || tag.eq_ignore_ascii_case("animate"))
        && ["to", "from", "by", "values"].iter().any(|a| a.eq_ignore_ascii_case(name))
}

/// Whether any of an animation's `;`-separated values is a script URL.
fn script_url_in_list(text: &str) -> bool {
    text.split(';').any(script_url)
}

/// Like `shows_script_url`, for an animation's values.
fn shows_script_url_in_list(v: &Value) -> bool {
    match v {
        Value::Str(s) => script_url_in_list(s),
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::List(_) | Value::Map(_) => false,
        other => script_url_in_list(&other.display()),
    }
}

/// Whether a value written in a URL attribute would be a `javascript:` URL:
/// its text is what the attribute says, whatever its type (an `html`
/// value from `markdown(u, { inline: true })` too).
fn shows_script_url(v: &Value) -> bool {
    match v {
        Value::Str(s) => script_url(s),
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::List(_) | Value::Map(_) => false,
        other => script_url(&other.display()),
    }
}

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

/// An attribute's name as the browser compares it: lower case, without the
/// `bind:` of `bind:value`.
fn attr_key(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.strip_prefix("bind:") {
        Some(rest) => rest.to_string(),
        None => lower,
    }
}

/// Whether a key of a `{...map}` spread may become an attribute. The name
/// that would be written is checked: `bind:onclick` is `onclick`. Names
/// with `:` (but `xlink:href`) and `@` handlers come only from the program.
fn spread_attr_allowed(name: &str) -> bool {
    let key = attr_key(name);
    attr_name_ok(name)
        && !code_attr(&key)
        && !name.starts_with("__")
        && !name.starts_with('@')
        && (!name.contains(':') || key == "xlink:href")
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
        other => escape_into(&crate::server::hidden(other).as_ref().unwrap_or(other).display(), out),
    }
}

/// A value placed in a `<script>` (see `render_tag_into`).
fn script_value(v: &Value, out: &mut String) {
    match v {
        Value::Nil | Value::Bool(false) => {}
        Value::Html(h) => out.push_str(&h.replace("</", "<\\/").replace("<!--", "<\\!--")),
        other => out.push_str(&script_json(&crate::server::hidden(other).as_ref().unwrap_or(other).to_json())),
    }
}

/// JSON that is safe inside `<script>`: no `</script>`, `<!--` or line
/// separators (which end a string in older JavaScript).
pub fn script_json(j: &serde_json::Value) -> String {
    serde_json::to_string(j).unwrap_or_default().replace('<', "\\u003c").replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")
}

/// JSON for an island's props and state, and for what server fns take and
/// give: like `to_json`, but an error value becomes `{"$quillo_err":
/// payload}`, a decimal `{"$dec": "12.50"}` and an int past what a JS number
/// holds whole `{"$int": "9007199254740993"}`, which the browser turns back
/// into an error value, its decimal and a BigInt.
pub fn island_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Err(e, _, _) => serde_json::json!({ "$quillo_err": island_json(e) }),
        Value::Dec(d) => serde_json::json!({ "$dec": d.to_string() }),
        Value::Int(n) if n.unsigned_abs() > JS_SAFE => serde_json::json!({ "$int": n.to_string() }),
        Value::List(l) => serde_json::Value::Array(l.iter().map(island_json).collect()),
        Value::Map(m) => serde_json::Value::Object(m.iter().map(|(k, x)| (k.to_string(), island_json(x))).collect()),
        other => other.to_json(),
    }
}

/// The largest int a JS number holds whole: 2^53 - 1.
const JS_SAFE: u64 = (1 << 53) - 1;

/// `island_json` read back: what the browser sent a server fn.
pub fn from_island_json(j: &serde_json::Value) -> Value {
    use serde_json::Value as J;
    match j {
        J::Array(a) => Value::list(a.iter().map(from_island_json).collect()),
        J::Object(o) if o.len() == 1 => {
            let (k, x) = o.iter().next().unwrap();
            match (k.as_str(), x) {
                ("$dec", J::String(s)) => Dec::parse(s).map_or_else(|| Value::str(s), Value::Dec),
                ("$int", J::String(s)) => s.parse::<i64>().map_or_else(|_| Value::str(s), Value::Int),
                ("$quillo_err", x) => Value::err(from_island_json(x)),
                _ => Value::map(o.iter().map(|(k, v)| (k.into(), from_island_json(v))).collect()),
            }
        }
        J::Object(o) => Value::map(o.iter().map(|(k, v)| (k.into(), from_island_json(v))).collect()),
        other => Value::from_json(other),
    }
}

/// Why a value cannot travel to the browser as JSON, if it cannot.
pub fn not_sendable(v: &Value) -> Option<&'static str> {
    match v {
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Dec(_) | Value::Str(_) => None,
        Value::List(l) => l.iter().find_map(not_sendable),
        Value::Map(m) => m.values().find_map(not_sendable),
        Value::Func(_) | Value::Builtin(..) => Some("a function"),
        Value::Html(_) => Some("html"),
        Value::Response(_) => Some("a response"),
        Value::Err(e, _, _) => not_sendable(e),
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

pub(crate) fn short(v: &Value) -> String {
    let r = v.repr();
    if r.chars().count() > 40 {
        format!("{}…", r.chars().take(40).collect::<String>())
    } else {
        r
    }
}

fn verb(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "subtract",
        BinOp::Mul => "multiply",
        BinOp::Div => "divide",
        _ => "take the remainder of",
    }
}

/// Said after "expects a number, got dec" from what works on floats.
pub const FLOAT_ONLY: &str = " (this works on floats: convert a dec with float(x))";

/// What to do about a decimal met with a float, said after the error.
pub(crate) fn dec_hint(a: &Value, b: &Value) -> &'static str {
    match (a, b) {
        (Value::Dec(_), Value::Float(_)) | (Value::Float(_), Value::Dec(_)) => {
            ": a dec is exact and a float is not, so convert one with dec(x) or float(x)"
        }
        _ => "",
    }
}

/// Arithmetic with a decimal: with another, or an int, it is exact and
/// gives a decimal; with a float it is an error, never a silent float.
fn dec_op(op: BinOp, a: &Value, b: &Value, line: u32) -> Result<Value, Ctrl> {
    let (x, y) = match (a, b) {
        (Value::Dec(x), Value::Dec(y)) => (*x, *y),
        (Value::Dec(x), Value::Int(y)) => (*x, Dec::from_i64(*y)),
        (Value::Int(x), Value::Dec(y)) => (Dec::from_i64(*x), *y),
        _ => return fail(line, format!("cannot {} {} and {}{}", verb(op), a.type_name(), b.type_name(), dec_hint(a, b))),
    };
    let r = match op {
        BinOp::Add => x.add(y),
        BinOp::Sub => x.sub(y),
        BinOp::Mul => x.mul(y),
        BinOp::Div => x.div(y),
        _ => x.rem(y),
    };
    r.map(Value::Dec).map_err(|e| Ctrl::Error(QuilloError::new(e.message(), line)))
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
                // Made to size at once (not grown as it is filled).
                let mut v = match Arc::try_unwrap(x) {
                    std::result::Result::Ok(mut v) => {
                        v.reserve_exact(y.len());
                        v
                    }
                    std::result::Result::Err(x) => {
                        let mut v = Vec::with_capacity(x.len() + y.len());
                        v.extend(x.iter().cloned());
                        v
                    }
                };
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
            (x @ Dec(_), y) | (x, y @ Dec(_)) => dec_op(op, &x, &y, line),
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
                    // Nothing is left over from dividing the smallest int by -1,
                    // which `checked_rem` calls an overflow (FEEDBACK16.md, Q-146;
                    // branch feedback20-fixes).
                    _ => Some(x.wrapping_rem(y)),
                };
                return r.map(Int).ok_or_else(overflow);
            }
            if matches!(a, Dec(_)) || matches!(b, Dec(_)) {
                return dec_op(op, &a, &b, line);
            }
            let verb = verb(op);
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
                return fail(line, format!("cannot compare {} with {}{}", a.type_name(), b.type_name(), dec_hint(&a, &b)));
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
            links: Default::default(),
            watched: Arc::new(HashSet::new()),
            shared,
            services: Default::default(),
            depth: 0,
            cur_file: None,
            caller_file: None,
            fuel: u64::MAX,
            allowed: ALL_EFFECTS,
            script: None,
            restrictor: None,
            server_fns: Arc::new(Vec::new()),
            site: Arc::new(crate::site::Site::default()),
            tenant: None,
            tenant_named: false,
            no_such_tenant: false,
            request_id: None,
            deadline: None,
            upload_body: None,
            set_cookies: Vec::new(),
            session: None,
            locale: None,
            program: Arc::new(Vec::new()),
            client_js: Arc::new(String::new()),
            client_lazy: Default::default(),
            uses_atomic: false,
            bundles: Default::default(),
            styles: Default::default(),
            current_request: None,
            pending_request: None,
            islands: Vec::new(),
            in_component: false,
            pg_tx: std::sync::Mutex::new(Vec::new()),
            reads: None,
            read_request: false,
            last_live: None,
            tx_depth: 0,
            pending: Vec::new(),
            pending_raw: Vec::new(),
            published: None,
            sqlite_tx: 0,
            db: Arc::new(std::sync::Mutex::new(None)),
            db_conns: Arc::new(std::sync::Mutex::new(Vec::new())),
            db_on: None,
            wrote_req: None,
            prefetching: None,
            db_resolving: Vec::new(),
            db_loop: None,
            base: String::new(),
            mounts: Vec::new(),
            remote_mounts: Vec::new(),
            is_service: false,
            retired: Default::default(),
            own_seqs: Vec::new(),
            in_job: false,
            env_file: Default::default(),
            declared_services: Default::default(),
            ready_checks: Default::default(),
            style_with: None,
            types: Arc::new(Default::default()),
            js_libs: Arc::new(Default::default()),
            stream_out: None,
            pending_stream: None,
            streams_direct: false,
            rendered_island: false,
            island_names: Vec::new(),
            deferred: Vec::new(),
            island_defers: Vec::new(),
            state_preset: None,
            spare: String::new(),
            view_cache: crate::viewcache::enabled().then(Default::default),
            tracks: Vec::new(),
            trace: Default::default(),
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
            links: self.links.clone(),
            watched: self.watched.clone(),
            shared: self.shared.clone(),
            services: self.services.clone(),
            depth: 0,
            cur_file: None,
            caller_file: None,
            fuel: u64::MAX,
            allowed: ALL_EFFECTS,
            script: None,
            restrictor: None,
            server_fns: self.server_fns.clone(),
            site: self.site.clone(),
            tenant: None,
            tenant_named: false,
            no_such_tenant: false,
            request_id: None,
            deadline: None,
            upload_body: None,
            set_cookies: Vec::new(),
            session: None,
            locale: None,
            program: self.program.clone(),
            client_js: self.client_js.clone(),
            client_lazy: self.client_lazy.clone(),
            uses_atomic: self.uses_atomic,
            bundles: self.bundles.clone(),
            styles: self.styles.clone(),
            current_request: None,
            pending_request: None,
            islands: Vec::new(),
            in_component: false,
            pg_tx: std::sync::Mutex::new(Vec::new()),
            reads: None,
            read_request: false,
            last_live: None,
            tx_depth: 0,
            pending: Vec::new(),
            pending_raw: Vec::new(),
            published: None,
            sqlite_tx: 0,
            db: self.db.clone(),
            db_conns: self.db_conns.clone(),
            db_on: None,
            wrote_req: None,
            prefetching: None,
            db_resolving: Vec::new(),
            db_loop: None,
            base: self.base.clone(),
            mounts: self.mounts.iter().map(|(p, m)| (p.clone(), m.fork())).collect(),
            remote_mounts: self.remote_mounts.clone(),
            is_service: self.is_service,
            retired: self.retired.clone(),
            own_seqs: Vec::new(),
            in_job: self.in_job,
            env_file: self.env_file.clone(),
            declared_services: self.declared_services.clone(),
            ready_checks: self.ready_checks.clone(),
            style_with: self.style_with.clone(),
            types: self.types.clone(),
            js_libs: self.js_libs.clone(),
            stream_out: None,
            pending_stream: None,
            streams_direct: false,
            rendered_island: false,
            island_names: Vec::new(),
            deferred: Vec::new(),
            island_defers: Vec::new(),
            state_preset: None,
            spare: String::new(),
            view_cache: self.view_cache.clone(),
            tracks: Vec::new(),
            trace: Default::default(),
        }
    }

    /// The source files this program has loaded (the main file and `use`d
    /// files), and those of the programs it mounts or calls as services.
    pub fn files(&self) -> Vec<PathBuf> {
        self.loaded.iter().chain(self.watched.iter()).cloned().collect()
    }

    /// Announces changes to `topics`, after the current transaction commits.
    pub fn publish(&mut self, topics: Vec<String>) {
        self.mark_impure();
        // Each program has its own topics (two mounted apps may both have
        // a `notes` table).
        let raw = topics.clone();
        let topics: Vec<String> = topics.into_iter().map(|t| self.scope_topic(t)).collect();
        if self.tx_depth > 0 {
            self.pending.extend(topics);
            self.pending_raw.extend(raw);
        } else {
            let seq = self.shared.publish(&topics);
            if self.reads.is_some() {
                self.own_seqs.push(seq);
            }
            self.note_published(raw);
        }
    }

    /// While answering a service call, keeps the topics just announced
    /// that the caller's pages may watch: those the program names itself,
    /// not its own tables (`db:`) or variables (`var:`), which the caller
    /// does not share (FEEDBACK4.md, Q-038; branch bank-fixes).
    pub(crate) fn note_published(&mut self, raw: Vec<String>) {
        if let Some(out) = &mut self.published {
            for t in raw {
                if !(t.starts_with("db:") || t.starts_with("db@") || t.starts_with("var:")) && !out.contains(&t) {
                    out.push(t);
                }
            }
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

    /// The renders being cached used the locale: they are kept per locale.
    pub fn note_locale(&mut self) {
        for t in &mut self.tracks {
            t.locale = true;
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
                let c = c.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
            let module = matches!(&v, Value::Map(m) if is_module_map(m));
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
        self.read_request = true;
        if self.current_request.is_none() {
            if let Some(p) = &self.pending_request {
                let mut r = p.make();
                // `request.id`: the id calls to services pass on.
                if let (Value::Map(m), Some(id)) = (&mut r, &self.request_id) {
                    Arc::make_mut(m).insert("id".into(), Value::str(&**id));
                }
                self.current_request = Some(r);
            }
        }
        self.current_request.clone()
    }

    /// While a page is prefetched, the code may read but not change
    /// anything (a database, a global, a cookie, a file, another server):
    /// what would stops it here, and the prefetch is answered "not
    /// prefetched" (the page is made when the link is followed).
    pub(crate) fn no_writes(&mut self, what: &str, line: u32) -> Result<(), Ctrl> {
        match &self.prefetching {
            Some(flag) => {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                fail(line, format!("{}: {}", PREFETCH_REFUSED, what))
            }
            None => Ok(()),
        }
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
            || !self.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty();
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
        it.in_component = self.in_component;
        it.current_request = self.current_request.clone().or_else(|| self.pending_request.as_ref().map(|p| p.make()));
        it.tenant = self.tenant.clone();
        it.tenant_named = self.tenant_named;
        it.request_id = self.request_id.clone();
        it.prefetching = self.prefetching.clone();
        it.upload_body = self.upload_body.clone();
        it.locale = self.locale.clone();
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
        let abs = source_path(Path::new(file));
        self.uses_atomic |= src.contains(".atomic");
        let mut stmts = parse_cached(src, abs.clone(), file, None)?;
        // What each file sees, before any of them runs (src/modules.rs).
        if stmts.iter().any(|s| matches!(s, Stmt::Use(p, ..) if !crate::ast::is_script(p))) {
            let links = crate::modules::link(Path::new(file), stmts.clone(), &self.base_dir, &mut |_, full| {
                let src = std::fs::read_to_string(full).ok()?;
                parse_cached(&src, source_path(full), &full.display().to_string(), None).ok()
            });
            if let Some(p) = links.problems.first() {
                return Err(QuilloError::new(p.message.clone(), p.line).in_file(&p.file.display().to_string()));
            }
            if let Some(names) = links.renames(&crate::modules::key(Path::new(file))) {
                stmts = parse_cached(src, abs.clone(), file, Some(names))?;
            }
            self.links = Arc::new(links);
        }
        Arc::make_mut(&mut self.program).push(stmts.clone());
        let env = self.globals.clone();
        let mut last = Value::Nil;
        let debugging = crate::debug::on();
        if debugging {
            crate::debug::enter(self, "<main>", abs, 1);
        }
        let r = self.load_stmts(&stmts, &env, file, &mut last);
        if debugging {
            crate::debug::leave(self);
        }
        r?;
        self.build_client().map_err(|e| e.in_file(file))?;
        Ok(last)
    }

    fn load_stmts(&mut self, stmts: &[Stmt], env: &Env, file: &str, last: &mut Value) -> Result<(), QuilloError> {
        for s in stmts.iter() {
            match self.exec_stmt(s, env) {
                Ok(v) => *last = v,
                Err(Ctrl::Error(e)) => return Err(e.in_file(file)),
                Err(Ctrl::Return(v)) => {
                    *last = v;
                    break;
                }
                Err(Ctrl::Reply(_)) => return Err(QuilloError::new("`reply` used outside of a route", 0).in_file(file)),
            }
        }
        Ok(())
    }

    /// Compiles the program's components (and what they use) for the browser.
    pub fn build_client(&mut self) -> Result<(), QuilloError> {
        // In development the whole program's bundle is made when first asked
        // for: each page compiles its own components, and a reload is a
        // third quicker without it (FEEDBACK15.md, Q-098; branch dev-maps-once).
        let runtime = if crate::client::DEV.load(std::sync::atomic::Ordering::Relaxed) {
            self.client_js = Arc::new(String::new());
            self.client_lazy = Default::default();
            None
        } else {
            let (js, runtime) = crate::client::bundle_and_runtime(self)?;
            self.client_js = Arc::new(js);
            runtime.map(Arc::new)
        };
        let mut bundles = crate::client::Bundles::default();
        bundles.runtime = runtime;
        self.bundles = Arc::new(std::sync::Mutex::new(bundles));
        self.styles = Arc::new(std::sync::Mutex::new(crate::styles::Styles::collect(self)?));
        Ok(())
    }

    /// The whole program's bundle: `/_quillo/quillo.js`, and a page's bundle
    /// no longer kept. In development it is made here, the first time; a
    /// component that cannot run in the browser then says so in the
    /// browser's console instead of at load.
    pub fn client_js(&self) -> Arc<String> {
        if !self.client_js.is_empty() {
            return self.client_js.clone();
        }
        self.client_lazy
            .get_or_init(|| {
                Arc::new(crate::client::bundle(self).unwrap_or_else(|e| {
                    eprintln!("quillo: {}", e);
                    format!("console.error({});\n", serde_json::json!(e.to_string()))
                }))
            })
            .clone()
    }

    /// The bundle for a page with these components: its address's hash,
    /// whether it takes the runtime file, and the styles it can show. None
    /// when it cannot be made (the full bundle serves).
    pub fn bundle_for_page(&self, names: &[String]) -> Option<crate::client::PageRef> {
        let mut sorted = names.to_vec();
        sorted.sort();
        sorted.dedup();
        let key = sorted.join(",");
        let runtime = {
            let mut b = self.bundles.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(hash) = b.hash_for_roots(&key) {
                return Some(hash);
            }
            b.runtime.clone()
        };
        let made = crate::client::bundle_for(self, &sorted, runtime.as_deref()).ok()?;
        Some(self.bundles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).keep(key, made))
    }

    pub fn bundle_by_hash(&self, hash: &str) -> Option<Arc<String>> {
        self.bundles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).by_hash(hash)
    }

    /// The sheet of the styles a page shows (FEEDBACK8.md, Q-068; branch
    /// feedback8-fixes): its hash, or none when none of them has a style.
    pub fn sheet_for_page(&self, ids: &[String]) -> Option<String> {
        self.styles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).sheet_for(ids)
    }

    /// A page's sheet by its hash, or, for one no longer kept, every style.
    pub fn sheet_by_hash(&self, hash: &str) -> (Arc<String>, bool) {
        let s = self.styles.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match s.by_hash(hash) {
            Some(css) => (css, true),
            None => (Arc::new(s.all()), false),
        }
    }

    /// The runtime file pages' bundles share, when there is one: its hash
    /// and its code.
    pub fn runtime_file(&self) -> Option<(String, Arc<String>)> {
        self.bundles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).runtime.as_ref().map(|r| (r.hash.clone(), r.js.clone()))
    }

    pub fn load_file(&mut self, path: &Path) -> Result<Value, QuilloError> {
        let src = std::fs::read_to_string(path)
            .map_err(|e| QuilloError::new(format!("cannot read {}: {}", path.display(), e), 0))?;
        if let Ok(p) = path.canonicalize() {
            Arc::make_mut(&mut self.loaded).insert(p);
        }
        self.load_source(&src, &path.display().to_string())
    }

    #[inline]
    pub fn exec_stmt(&mut self, s: &Stmt, env: &Env) -> Result<Value, Ctrl> {
        if crate::debug::on() {
            return crate::debug::stmt(self, s, env);
        }
        self.run_stmt(s, env)
    }

    pub(crate) fn run_stmt(&mut self, s: &Stmt, env: &Env) -> Result<Value, Ctrl> {
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
            // The server has none of the page's objects: a map's fields are
            // made with `set` or `{ ...m, name: v }`, never changed in place.
            Stmt::SetField(o, name, _, e, line) => {
                let o = self.eval(o, env)?;
                self.eval(e, env)?;
                let why = match o {
                    Value::Map(_) => format!("a map's fields are not set in place: write `m = set(m, \"{}\", …)`", name),
                    _ => format!("{} has no field `{}` to set: only the page's objects (an element, an event) do, in a component", o.type_name(), name),
                };
                fail(*line, why)
            }
            Stmt::Assign(name, op, e, line) => {
                if self.prefetching.is_some() && lookup_global(env, name).map_or(false, |(_, global)| global) {
                    self.no_writes(&format!("assigning `{}`", name), *line)?;
                }
                let rhs = match self.in_place(name, op, e, env)? {
                    Some(v) => v,
                    None => self.eval(e, env)?,
                };
                let v = match op {
                    AssignOp::Set => rhs,
                    AssignOp::Add | AssignOp::Sub => {
                        let old = lookup(env, name).ok_or_else(|| undefined(name, *line))?;
                        let bop = if matches!(op, AssignOp::Add) { BinOp::Add } else { BinOp::Sub };
                        binop(bop, old, rhs, *line)?
                    }
                };
                let function = matches!(v, Value::Func(_)) || matches!(lookup(env, name), Some(Value::Func(_)));
                // Which fields of a global map changed, for what follows
                // one field (FEEDBACK12.md, Q-084; branch open-fixes).
                let followed = self.shared.field_reads.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(name.as_str());
                let fields = match lookup_global(env, name) {
                    Some((old, true)) if followed => changed_fields(&old, &v),
                    _ => Vec::new(),
                };
                match assign_global(env, name, v) {
                    None => fail(
                        *line,
                        format!("cannot assign to `{}`: it was never declared (use `let {} = …`)", name, name),
                    ),
                    Some(global) => {
                        if global {
                            let mut topics = vec![format!("var:{}", name)];
                            topics.extend(fields.into_iter().map(|k| format!("var:{}.{}", name, k)));
                            self.publish(topics);
                            // Cached renders do not record the functions
                            // they called, so a new one starts afresh.
                            if function {
                                if let Some(c) = &self.view_cache {
                                    c.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
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
            // A browser library: its names, for components' code; on the server
            // they give null, as `js(...)` does (branch js-libraries).
            Stmt::Use(path, Some(names), _, options) if crate::ast::is_script(path) => {
                for (global, name) in names {
                    let libs = Arc::make_mut(&mut self.js_libs);
                    let order = libs.get(name).map_or(libs.len(), |l| l.order);
                    // A module's exports go by the program's name for them, in the
                    // runtime (branch library-hints).
                    let (page_name, export) = if options.module { (name.clone(), Some(global.clone())) } else { (global.clone(), None) };
                    libs.insert(name.clone(), crate::client::JsLib { global: page_name, src: path.clone(), order, hint: options.hint, export });
                    define(env, name, Value::Builtin("__jslib", crate::builtins::js_lib));
                }
                Ok(Value::Nil)
            }
            Stmt::Use(path, names, line, _) => {
                let path = &crate::modules::resolve(&self.base_dir, path);
                let full = self.base_dir.join(path);
                let key = full.canonicalize().unwrap_or_else(|_| full.clone());
                // A `.qwer` file's names are defined for each `use` that takes some
                // (its library is built and opened once); anything else runs once.
                if !Arc::make_mut(&mut self.loaded).insert(key.clone()) && (names.is_none() || !path.ends_with(".qwer")) {
                    return Ok(Value::Nil);
                }
                // Rust: built, loaded, and its functions defined (see `native`).
                if path.ends_with(".qwer") {
                    let fname = full.display().to_string();
                    return match crate::native::use_file(self, &full, names.as_deref(), *line) {
                        Err(Ctrl::Error(e)) => Err(Ctrl::Error(e.in_file(&fname))),
                        other => other.map(|_| Value::Nil),
                    };
                }
                let src = std::fs::read_to_string(&full)
                    .map_err(|e| QuilloError::new(format!("cannot read {}: {}", full.display(), e), *line))?;
                let fname = full.display().to_string();
                let abs = source_path(&full);
                self.uses_atomic |= src.contains(".atomic");
                let links = self.links.clone();
                let stmts = parse_cached(&src, abs.clone(), &fname, links.renames(&key))?;
                Arc::make_mut(&mut self.program).push(stmts.clone());
                let g = self.globals.clone();
                let debugging = crate::debug::on();
                if debugging {
                    crate::debug::enter(self, path, abs, 1);
                }
                let mut r = Ok(Value::Nil);
                for s in stmts.iter() {
                    match self.exec_stmt(s, &g) {
                        Ok(_) => {}
                        // `return` at a file's top level leaves that file, not the one
                        // that used it (FEEDBACK16.md, Q-109; branch feedback20-fixes).
                        Err(Ctrl::Return(_)) => break,
                        Err(e) => {
                            r = Err(match e {
                                Ctrl::Error(e) => Ctrl::Error(e.in_file(&fname)),
                                other => other,
                            });
                            break;
                        }
                    }
                }
                if debugging {
                    crate::debug::leave(self);
                }
                r
            }
            Stmt::Type(td) => {
                if !Arc::ptr_eq(env, &self.globals) {
                    return fail(td.line, "`type` must be at the top level");
                }
                Arc::make_mut(&mut self.types).insert(td.name.clone(), td.clone());
                // `User(fields)` makes one (`__record`, src/record.rs).
                let line = td.line;
                let def = Arc::new(FnDef {
                    name: td.name.clone(),
                    params: vec![Param { name: "fields".into(), ty: None, default: Some(Expr::Lit(Lit::Nil)) }],
                    ret: None,
                    err: None,
                    effects: None,
                    server: false,
                    component: false,
                    line,
                    body: Expr::Call(
                        Box::new(Expr::Ident("__record".into(), line)),
                        vec![Expr::Lit(Lit::Str(td.name.as_str().into())), Expr::Ident("fields".into(), line)],
                        line,
                    ),
                    nesting: 0,
                    file: None,
                    hot: Default::default(),
                });
                define(env, &td.name, Value::Func(Arc::new(Closure { def, env: env.clone() })));
                Ok(Value::Nil)
            }
            Stmt::Service(name, prefix, file, options, line) => {
                if !Arc::ptr_eq(env, &self.globals) {
                    return fail(*line, "`service` must be at the top level");
                }
                let options = match options {
                    Some(e) => Some(self.eval(e, env)?),
                    None => None,
                };
                crate::service::declare(self, name, prefix.as_deref(), file, options.as_ref(), *line)
            }
            Stmt::Mount(prefix, file, line) => {
                if !Arc::ptr_eq(env, &self.globals) {
                    return fail(*line, "`mount` must be at the top level");
                }
                let full = self.base_dir.join(file);
                let dir = full.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| self.base_dir.to_path_buf());
                let mut sub = Interp::with_shared(dir, self.shared.clone());
                sub.services = self.services.clone();
                sub.base = format!("{}{}", self.base, prefix);
                // What a service mounts is part of the service.
                sub.is_service = self.is_service;
                sub.load_file(&full).map_err(|e| Ctrl::Error(e.in_file(&full.display().to_string())))?;
                // Watch the mounted program's files too: watched, not loaded,
                // so a file both `use` still runs here (FEEDBACK4.md, Q-040;
                // branch bank-fixes).
                Arc::make_mut(&mut self.watched).extend(sub.files());
                self.mounts.retain(|(p, _)| p != prefix);
                self.mounts.push((prefix.clone(), sub));
                Ok(Value::Nil)
            }
            Stmt::State(name, e, _) => {
                self.last_live = None;
                // A component placed in the page (the only island rendering) defers
                // `defer(...)`: null now, worked out after the page. Its second
                // render takes the other states as they were (src/defer.rs; branch defer).
                let top = self.islands.len() == 1;
                let v = match self.state_preset.as_ref().filter(|_| top).and_then(|m| m.get(name.as_str()).cloned()) {
                    Some(v) => v,
                    None if top && self.state_preset.is_none() && defer_call(e, env) => {
                        self.island_defers.push(name.clone());
                        Value::Nil
                    }
                    None => self.eval(e, env)?,
                };
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
            Stmt::Return(e, _) => {
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
                if self.fuel != u64::MAX {
                    return self.script_binop(*op, x, y, *line);
                }
                binop(*op, x, y, *line)
            }
            Expr::Coalesce(a, b) => {
                let v = self.eval(a, env)?;
                if matches!(v, Value::Nil | Value::Err(..)) {
                    self.eval(b, env)
                } else {
                    Ok(v)
                }
            }
            Expr::Pipe(lhs, rhs, line) => self.eval_pipe(lhs, rhs, env, *line),
            Expr::Call(callee, args, line) => self.eval_call(callee, args, None, env, *line),
            Expr::Field(obj, name, line) => {
                // A local map's field, without a copy of the map.
                if let Expr::Ident(var, _) = &**obj {
                    let found = with_local(env, var, |v| match v {
                        Value::Map(m) if m.len() <= 8 => m.iter().find(|(k, _)| k.as_str() == name).map(|(_, x)| x.clone()),
                        Value::Map(m) => m.get(name.as_str()).cloned(),
                        _ => None,
                    });
                    if let Some(Some(x)) = found {
                        return Ok(x);
                    }
                }
                // A field of a global map, read by a live loader or a cached
                // view: what it follows is that field (`var:world.scores`),
                // not every write to the map (FEEDBACK12.md, Q-084; branch
                // open-fixes). A name the map lacks may be a method
                // (`world.len`), which reads all of it.
                if let Expr::Ident(var, _) = &**obj {
                    if self.reads.is_some() || !self.tracks.is_empty() {
                        if let Some((Value::Map(m), true)) = lookup_global(env, var) {
                            if m.contains_key(name.as_str()) && !is_module_map(&m) {
                                {
                                    let mut f = self.shared.field_reads.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                                    if !f.contains(var.as_str()) {
                                        f.insert(var.to_string());
                                    }
                                }
                                self.watch(format!("var:{}.{}", var, name));
                                return self.get_field(Value::Map(m), name, env, *line);
                            }
                        }
                    }
                }
                let v = self.eval(obj, env)?;
                self.get_field(v, name, env, *line)
            }
            Expr::Try(x) => {
                let v = self.eval(x, env)?;
                if let Value::Err(..) = v {
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
            Expr::For(pat, iter, body, line) => self.eval_for(pat, iter, body, env, *line, true),
            Expr::While(cond, body, line) => {
                while self.eval(cond, env)?.truthy() {
                    self.within_limit(*line)?;
                    if self.fuel != u64::MAX {
                        self.burn(*line)?;
                    }
                    self.eval(body, env)?;
                }
                Ok(Value::Nil)
            }
            Expr::Reply(status, body, line) => self.eval_reply(status, body.as_deref(), env, *line),
            Expr::Html(el, _) => self.eval_html(el, env),
        }
    }

    #[inline(never)]
    fn eval_pipe(&mut self, lhs: &Expr, rhs: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let v = self.eval(lhs, env)?;
        match rhs {
            Expr::Call(callee, args, l) => self.eval_call(callee, args, Some(v), env, *l),
            // `sql | analytics.one`: on that connection.
            Expr::Field(obj, name, l) => {
                let recv = self.eval(obj, env)?;
                let db_on = crate::db::conn_of(&recv, name);
                let f = self.get_field(recv, name, env, *l)?;
                self.db_on = db_on;
                self.call(&f, vec![v], line)
            }
            other => {
                let f = self.eval(other, env)?;
                self.call(&f, vec![v], line)
            }
        }
    }

    #[inline(never)]
    fn eval_block(&mut self, stmts: &[Stmt], env: &Env) -> Result<Value, Ctrl> {
        // A block that declares nothing runs in the scope around it: the
        // same names mean the same, with no scope to make (a loop's body).
        let declares = !stmts.iter().all(|s| matches!(s, Stmt::Expr(_) | Stmt::Assign(..) | Stmt::Return(..)));
        let scope = if declares { Scope::child(env) } else { env.clone() };
        let mut last = Value::Nil;
        let n = stmts.len();
        for (i, s) in stmts.iter().enumerate() {
            last = match s {
                // A loop whose list nobody takes makes none.
                Stmt::Expr(Expr::For(pat, iter, body, line)) if i + 1 < n && !crate::debug::on() => self.eval_for(pat, iter, body, &scope, *line, false)?,
                s => self.exec_stmt(s, &scope)?,
            };
        }
        if declares {
            Scope::done(scope);
        }
        Ok(last)
    }

    #[inline(never)]
    fn eval_template(&mut self, parts: &[TPart], env: &Env) -> Result<Value, Ctrl> {
        let mut s = String::new();
        self.template_into(parts, env, &mut s)?;
        self.script_made(Value::str(s), 0)
    }

    #[inline(never)]
    fn eval_list(&mut self, items: &[Expr], env: &Env) -> Result<Value, Ctrl> {
        let mut v = Vec::with_capacity(items.len());
        for i in items {
            v.push(self.eval(i, env)?);
        }
        self.script_made(Value::list(v), 0)
    }

    #[inline(never)]
    fn eval_map(&mut self, entries: &[(String, Expr)], env: &Env) -> Result<Value, Ctrl> {
        let mut m = Map::new();
        for (k, x) in entries {
            let v = self.eval(x, env)?;
            m.insert(k.into(), v);
        }
        self.script_made(Value::map(m), 0)
    }

    /// A step of a loop or a call: `--max-compute` ends a request here, between statements,
    /// so nothing is left half done (FEEDBACK20.md, Q-163; branch feedback20-fixes).
    #[inline]
    fn within_limit(&mut self, line: u32) -> Result<(), Ctrl> {
        if crate::httpd::OVER_TIME.load(std::sync::atomic::Ordering::Relaxed) && crate::httpd::over_time() {
            return fail(line, "this request computed for longer than --max-compute allows");
        }
        Ok(())
    }

    /// `v`, unless a sandboxed script made it too big; a script also pays
    /// for what it makes, a step for each 1 KB (src/script.rs).
    #[inline]
    fn script_made(&mut self, v: Value, line: u32) -> Result<Value, Ctrl> {
        if self.fuel != u64::MAX {
            self.pay_for(&v, line)?;
        }
        Ok(v)
    }

    #[cold]
    fn pay_for(&mut self, v: &Value, line: u32) -> Result<(), Ctrl> {
        let n = crate::script::too_big(v).or_else(|why| fail(line, why))?;
        let cost = (n / 1024) as u64;
        if cost > self.fuel {
            return fail(line, "the script took too many steps");
        }
        self.fuel -= cost;
        Ok(())
    }

    /// An operator in a sandboxed script: a range it may not make is
    /// refused before it is made, and the result is paid for.
    #[cold]
    fn script_binop(&mut self, op: BinOp, x: Value, y: Value, line: u32) -> Result<Value, Ctrl> {
        if let (BinOp::Range, Value::Int(a), Value::Int(b)) = (op, &x, &y) {
            if b.saturating_sub(*a).saturating_mul(8) > crate::script::SCRIPT_BYTES as i64 {
                return fail(line, "the script made a value larger than 1 MB");
            }
        }
        let v = binop(op, x, y, line)?;
        self.script_made(v, line)
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
                Value::Dec(d) => Ok(Value::Dec(d.neg())),
                o => fail(line, format!("cannot negate {}", o.type_name())),
            },
        }
    }

    /// `m = set(m, k, v)` (or `push`, `merge`, `remove`), as in a loop that
    /// builds a map: the variable lets go of the value while the builtin
    /// runs, which then changes it rather than copying it, so the loop
    /// costs its length and not its square. Not when the other arguments
    /// read the variable, nor for a global, which other requests read.
    fn in_place(&mut self, name: &str, op: &AssignOp, e: &Expr, env: &Env) -> Result<Option<Value>, Ctrl> {
        match op {
            AssignOp::Set => self.in_place_call(Some(name), e, env),
            _ => Ok(None),
        }
    }

    /// The same for a call that is a function's whole body, on one of its
    /// own parameters, as in `reduce(xs, {}, (m, x) => set(m, x.id, x))`
    /// (`name` is None: the caller has checked it is a parameter).
    fn in_place_call(&mut self, name: Option<&str>, e: &Expr, env: &Env) -> Result<Option<Value>, Ctrl> {
        let Expr::Call(f, args, line) = e else { return Ok(None) };
        let (Expr::Ident(fname, _), Some(Expr::Ident(first, _))) = (&**f, args.first()) else { return Ok(None) };
        if name.is_some_and(|n| n != first) || !matches!(fname.as_str(), "set" | "push" | "merge" | "remove") {
            return Ok(None);
        }
        let name = first.as_str();
        let Some(b @ Value::Builtin(bname, _)) = lookup(env, fname) else { return Ok(None) };
        if bname != fname.as_str() {
            return Ok(None);
        }
        let mut reads = false;
        for a in &args[1..] {
            crate::client::each_expr(a, &mut |x| reads |= matches!(x, Expr::Ident(n, _) if n == name));
        }
        if reads || lookup_global(env, name).map_or(true, |(_, global)| global) {
            return Ok(None);
        }
        let mut vals = Vec::with_capacity(args.len());
        vals.push(Value::Nil);
        for a in &args[1..] {
            vals.push(self.eval(a, env)?);
        }
        let Some(old) = take_local(env, name) else { return Ok(None) };
        vals[0] = old;
        self.call(&b, vals, *line).map(Some)
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
            // Keys are text: a number is the key it is written as, as for
            // `set` and `get`.
            (Value::Map(m), Value::Int(n)) => Ok(m.get(n.to_string().as_str()).cloned().unwrap_or(Value::Nil)),
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
    /// `for`: the list of what the body gave for each item (`keep`), or
    /// nothing when no one takes it.
    fn eval_for(&mut self, pat: &Pattern, iter: &Expr, body: &Expr, env: &Env, line: u32, keep: bool) -> Result<Value, Ctrl> {
        let mut out = Vec::new();
        let mut scope = None;
        // `range(a, b)` and `a..b`: counted, without making the list.
        if let Some((a, b)) = self.counted(iter, env, line)? {
            if keep {
                // A script stops after its steps: no room for more.
                out.reserve(((b - a).max(0) as u64).min(self.fuel) as usize);
            }
            for i in a..b {
                let v = self.for_step(pat, Value::Int(i), body, env, &mut scope, line)?;
                if keep {
                    out.push(v);
                }
            }
        } else {
            let it = self.eval(iter, env)?;
            let made: Vec<Value>;
            let items: &[Value] = match &it {
                Value::List(l) => l,
                Value::Map(m) => {
                    made = m.iter().map(|(k, v)| Value::list(vec![Value::str(k), v.clone()])).collect();
                    &made
                }
                Value::Str(s) => {
                    made = s.chars().map(|c| Value::str(c.to_string())).collect();
                    &made
                }
                Value::Nil => &[],
                other => return fail(line, format!("cannot loop over {}", other.type_name())),
            };
            if keep {
                out.reserve(items.len());
            }
            for item in items {
                let v = self.for_step(pat, item.clone(), body, env, &mut scope, line)?;
                if keep {
                    out.push(v);
                }
            }
        }
        if let Some(s) = scope {
            Scope::done(s);
        }
        if keep {
            self.script_made(Value::list(out), line)
        } else {
            Ok(Value::Nil)
        }
    }

    /// One step of a loop: `item` bound by `pat` in a scope of its own (the
    /// last step's again, when nothing kept it), and the body's value.
    fn for_step(&mut self, pat: &Pattern, item: Value, body: &Expr, env: &Env, scope: &mut Option<Env>, line: u32) -> Result<Value, Ctrl> {
        self.within_limit(line)?;
        if self.fuel != u64::MAX {
            self.burn(line)?;
        }
        if !scope.as_mut().map_or(false, Scope::reuse) {
            *scope = Some(Scope::child(env));
        }
        let sc = scope.as_ref().expect("set above");
        if let Pattern::Bind(n) = pat {
            define(sc, n, item);
        } else {
            let mut binds = Vec::new();
            if self.match_pat(pat, &item, Mode::Strict, env, &mut binds)?.is_none() {
                return fail(line, format!("`for` pattern does not match {}", short(&item)));
            }
            for (n, x) in binds {
                define(sc, &n, x);
            }
        }
        self.eval(body, sc)
    }

    /// The bounds of `range(a, b)`, `range(n)` or `a..b` (the builtin, not
    /// a variable of that name), checked as those check them; None for
    /// anything else.
    fn counted(&mut self, iter: &Expr, env: &Env, line: u32) -> Result<Option<(i64, i64)>, Ctrl> {
        let (a, b, f) = match iter {
            Expr::Binary(BinOp::Range, a, b, _) => (self.eval(a, env)?, self.eval(b, env)?, None),
            Expr::Call(callee, args, _) if (args.len() == 1 || args.len() == 2) && matches!(&**callee, Expr::Ident(n, _) if n == "range") => {
                let Some(f @ Value::Builtin("range", _)) = lookup(env, "range") else { return Ok(None) };
                if args.len() == 1 {
                    (Value::Int(0), self.eval(&args[0], env)?, Some(f))
                } else {
                    (self.eval(&args[0], env)?, self.eval(&args[1], env)?, Some(f))
                }
            }
            _ => return Ok(None),
        };
        match (&a, &b) {
            (Value::Int(x), Value::Int(y)) if y.saturating_sub(*x) <= 10_000_000 => Ok(Some((*x, *y))),
            // Anything else fails as it would have.
            _ => match f {
                Some(f) => {
                    let args = if matches!(iter, Expr::Call(_, args, _) if args.len() == 1) { vec![b] } else { vec![a, b] };
                    self.call(&f, args, line).map(|_| None)
                }
                None => binop(BinOp::Range, a, b, line).map(|_| None),
            },
        }
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
        // `analytics.query(...)` on a connection from `db.connect`: which one.
        let mut db_on = None;
        let f = if let Expr::Field(obj, name, _) = callee {
            // `Chart.new(...)`, `L.control.zoom(...)`: the browser's, null here
            // (branch js-libraries).
            if !self.js_libs.is_empty() && lib_root(obj, env) {
                return Ok(Value::Nil);
            }
            let recv = self.eval(obj, env)?;
            // `json.stringify(...)` where a variable named `json` hides the
            // module (FEEDBACK9.md, Q-069; branch feedback9-fixes).
            if let Expr::Ident(m, _) = &**obj {
                if crate::check::MODULES.contains(&m.as_str()) && !matches!(recv, Value::Map(_)) {
                    return fail(line, format!("`{}.{}`: `{}` is a variable here ({}), which hides the module `{}`: rename the variable", m, name, m, recv.type_name(), m));
                }
            }
            let method = match &recv {
                Value::Map(m) => {
                    db_on = crate::db::conn_of(&recv, name);
                    m.get(name.as_str()).cloned()
                }
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
        // `raw(t(key, vars))`: the translation's markup, but the values put
        // in it are text, escaped (see `i18n::t_html`).
        if let (Value::Builtin("raw", _), None, [Expr::Call(inner, targs, l)]) = (&f, &pre, args) {
            if matches!(&**inner, Expr::Ident(n, _) if n == "t") && matches!(self.eval(inner, env)?, Value::Builtin("t", _)) {
                let mut tv = Vec::with_capacity(targs.len());
                for a in targs {
                    tv.push(self.eval(a, env)?);
                }
                return crate::i18n::t_html(self, tv, *l);
            }
        }
        if let Some(p) = pre {
            argv.push(p);
        }
        for a in args {
            argv.push(self.eval(a, env)?);
        }
        self.db_on = db_on;
        self.call(&f, argv, line)
    }

    /// `v.name`, read where `env` is the scope of the code that reads it.
    fn get_field(&mut self, v: Value, name: &str, env: &Env, line: u32) -> Result<Value, Ctrl> {
        match &v {
            Value::Nil => return Ok(Value::Nil),
            // `lib.version`: the browser's, null here (branch js-libraries).
            Value::Builtin("__jslib", _) => return Ok(Value::Nil),
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
                        Body::Stream { .. } => Value::Nil,
                    })
                }
                _ => {}
            },
            Value::Err(e, _, _) if name == "error" => return Ok((**e).clone()),
            _ => {}
        }
        // `xs.len` is `len(xs)`. On maps only `len`, `keys` and `values` work
        // this way, so any other missing key on a record is simply null. The
        // function is the one in scope where the field is read: a script's
        // scope holds only what it was given, not the program's globals.
        if let Some(f) = lookup_fn(env, name) {
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
        if self.fuel != u64::MAX {
            self.burn(line)?;
        }
        match f {
            Value::Builtin(name, func) => {
                if self.allowed != ALL_EFFECTS {
                    self.check_builtin_effect(name, line)?;
                }
                if self.prefetching.is_some() && CHANGES.contains(name) {
                    self.no_writes(name, line)?;
                }
                if !self.tracks.is_empty() && crate::viewcache::impure_builtin(name) {
                    self.mark_impure();
                }
                if name.starts_with("native:") {
                    return crate::native::call(self, name, args, line);
                }
                if self.fuel != u64::MAX {
                    return self.script_builtin(name, *func, args, line);
                }
                match func(self, args, line) {
                    // All an error value is made by: `err` and the builtins that fail
                    // (FEEDBACK21.md, Q-174; branch feedback20-fixes).
                    Ok(Value::Err(e, own, Place::NONE)) => Ok(Value::Err(e, own, Place::new(self.cur_file.as_ref(), line))),
                    r => r,
                }
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
        self.within_limit(line)?;
        if self.fuel != u64::MAX {
            self.burn(line)?;
        }
        // Numbers in, a number out: in machine code once it is called again.
        #[cfg(feature = "jit")]
        if self.fuel == u64::MAX && self.depth < MAX_DEPTH && !crate::debug::on() && !crate::httpd::compute_limited() {
            if let Some(r) = crate::jit::call(c, &args, self.depth) {
                return in_file(r, &c.def.file);
            }
        }
        // `(x) => x.name`, as given to `sort_by`, `map` and `filter`: the
        // field, without a scope for the call.
        if let (Some((field, at)), [Some(_)], u64::MAX) = (projection(&c.def), args.as_slice(), self.fuel) {
            let v = args.into_iter().next().flatten().unwrap_or(Value::Nil);
            return in_file(self.get_field(v, field, &c.env, at), &c.def.file);
        }
        let n = args.len();
        self.call_closure_with(c, args, n, line)
    }

    /// `f(x)`, as `call` but without making a list of the one argument: for
    /// `map`, `filter` and the like, which call f for each item.
    pub fn call1(&mut self, f: &Value, x: Value, line: u32) -> Result<Value, Ctrl> {
        let Value::Func(c) = f else {
            return self.call(f, vec![x], line);
        };
        if self.fuel != u64::MAX {
            self.burn(line)?;
            self.burn(line)?;
        }
        if let (Some((field, at)), u64::MAX) = (projection(&c.def), self.fuel) {
            return in_file(self.get_field(x, field, &c.env, at), &c.def.file);
        }
        let c = c.clone();
        self.call_closure_with(&c, std::iter::once(Some(x)), 1, line)
    }

    fn call_closure_with(&mut self, c: &Closure, args: impl IntoIterator<Item = Option<Value>>, nargs: usize, line: u32) -> Result<Value, Ctrl> {
        let def = c.def.clone();
        let mask = if self.fuel != u64::MAX { self.script_mask(c) } else { None };
        let outer = mask.map(|m| std::mem::replace(&mut self.allowed, m));
        let r = self.enter_call(&def, nargs, line, |this| this.call_closure_inner(c, &def, args, line));
        if let Some(a) = outer {
            self.allowed = a;
        }
        let r = r?;
        if let (Some(ety), Value::Err(payload, _, _)) = (&def.err, &r) {
            if !self.fits(ety, payload) {
                return in_file(fail(
                    def.line,
                    format!(
                        "`{}` declares errors as `! {}`, but returned err({})",
                        def.name,
                        ety.text(),
                        short(payload)
                    ),
                ), &def.file);
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
        let caller = std::mem::replace(&mut self.cur_file, def.file.clone());
        let outer_caller = std::mem::replace(&mut self.caller_file, caller.clone());
        let was_component = def.server && std::mem::replace(&mut self.in_component, false);
        let debugging = crate::debug::on();
        if debugging {
            crate::debug::enter(self, &def.name, def.file.clone(), def.line);
        }
        // Grow the stack on demand, so deep recursion works on any thread,
        // and so that a body that nests deeply has room for it.
        let r = crate::stack::for_nesting(def.nesting, || body(self));
        if debugging {
            crate::debug::leave(self);
        }
        self.depth -= 1;
        let r = match in_file(r, &def.file) {
            // On the way out: a call that succeeds pays nothing (FEEDBACK21.md, Q-174; branch
            // feedback20-fixes).
            Err(Ctrl::Error(e)) => Err(Ctrl::Error(e.called(&def.name, caller.as_ref(), line))),
            r => r,
        };
        self.cur_file = caller;
        self.caller_file = outer_caller;
        if was_component {
            self.in_component = true;
        }
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

    /// In a sandboxed script, the effects function `c` may use: none for the
    /// script's own, those the program had for a helper it passed in.
    #[cold]
    fn script_mask(&self, c: &Closure) -> Option<u8> {
        let (root, outer) = self.script.as_ref()?;
        Some(if within(&c.env, root) { 0 } else { *outer })
    }

    /// One step of a sandboxed script's budget.
    #[cold]
    fn burn(&mut self, line: u32) -> Result<(), Ctrl> {
        if self.fuel == 0 {
            return fail(line, "the script took too many steps");
        }
        self.fuel -= 1;
        Ok(())
    }

    /// A builtin called by a sandboxed script: one that would make a value
    /// over the limit is refused before it runs.
    #[cold]
    fn script_builtin(&mut self, name: &str, func: BuiltinFn, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
        if crate::script::would_be_too_big(name, &args) {
            return fail(line, "the script made a value larger than 1 MB");
        }
        let v = func(self, args, line)?;
        self.script_made(v, line)
    }

    #[cold]
    fn check_builtin_effect(&self, name: &str, line: u32) -> Result<(), Ctrl> {
        let need = builtin_effect(name);
        if need & !self.allowed != 0 {
            return fail(line, self.effect_denied(name, need));
        }
        Ok(())
    }

    fn call_closure_inner(&mut self, c: &Closure, def: &FnDef, args: impl IntoIterator<Item = Option<Value>>, line: u32) -> Result<Value, Ctrl> {
        let scope = self.bind_args(c, def, args, line).or_else(|e| in_file(Err(e), &self.caller_file))?;
        if crate::debug::on() {
            crate::debug::body(self, &def.body, &scope);
        }
        let own_param = matches!(&def.body, Expr::Call(_, a, _) if matches!(a.first(), Some(Expr::Ident(n, _)) if def.params.iter().any(|p| p.name == *n)));
        let in_place = if own_param { self.in_place_call(None, &def.body, &scope)? } else { None };
        let r = match in_place.map(Ok).unwrap_or_else(|| self.eval(&def.body, &scope)) {
            Ok(v) | Err(Ctrl::Return(v)) => v,
            Err(Ctrl::Error(e)) if crate::debug::on() => {
                crate::debug::failed(self, &e, &scope);
                return Err(Ctrl::Error(e));
            }
            Err(e) => return Err(e),
        };
        Scope::done(scope);
        if let Some(ty) = &def.ret {
            if !matches!(r, Value::Err(..)) {
                if let Err(why) = crate::record::fits(&self.types, ty, &r, false) {
                    return fail(
                        def.line,
                        format!("`{}` must return {}, but returned {} {}{}", def.name, ty_text(ty), r.type_name(), short(&r), because(&why)),
                    );
                }
            }
        }
        Ok(r)
    }

    /// A scope for a call of `c` with its parameters set from `args` (and
    /// defaults), checked against their types.
    fn bind_args(&mut self, c: &Closure, def: &FnDef, args: impl IntoIterator<Item = Option<Value>>, line: u32) -> Result<Env, Ctrl> {
        let scope = Scope::child(&c.env);
        if def.server {
            let was = self.read_request;
            if let Some(req) = self.request_value() {
                define(&scope, "request", req.clone());
                self.mark_impure();
            }
            // `request` given to a server fn is read only if its code names
            // it: a live loader that does not is shared between viewers
            // (FEEDBACK2.md, Q-028; branch feedback6-fixes).
            if self.reads.is_some() && !was && !crate::parser::names_request(&def.body) {
                self.read_request = false;
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
                if let Err(why) = crate::record::fits(&self.types, ty, &v, false) {
                    return fail(
                        line,
                        format!(
                            "argument `{}` of `{}` must be {}, got {} {}{}",
                            p.name,
                            def.name,
                            ty_text(ty),
                            v.type_name(),
                            short(&v),
                            because(&why)
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
        let locale = if self.site.i18n.is_some() { self.locale() } else { None };
        let found = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).lookup(&c.def, &args, &self.shared, locale.as_ref());
        match found {
            Found::Hit(entry) => {
                out.push_str(&entry.html);
                if entry.locale.is_some() {
                    self.note_locale();
                }
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
                    outer.locale |= track.locale;
                }
                r?;
                if !track.impure {
                    self.shared.track(&track.deps);
                }
                let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if track.impure {
                    cache.forbid(&c.def);
                } else {
                    cache.store(&c.def, args, &out[start..], track.deps, seq, if track.locale { locale } else { None });
                }
                Ok(())
            }
        }
    }

    fn render_call_plain(&mut self, c: &Closure, args: Vec<Option<Value>>, line: u32, out: &mut String) -> Result<(), Ctrl> {
        let def = c.def.clone();
        self.enter_call(&def, args.len(), line, |this| {
            let scope = this.bind_args(c, &def, args, line).or_else(|e| in_file(Err(e), &this.caller_file))?;
            if crate::debug::on() {
                crate::debug::body(this, &def.body, &scope);
            }
            let start = out.len();
            match this.render_expr_into(&def.body, &scope, out) {
                Ok(()) => Ok(()),
                Err(Ctrl::Error(e)) if crate::debug::on() => {
                    crate::debug::failed(this, &e, &scope);
                    Err(Ctrl::Error(e))
                }
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
                Some((end @ Stmt::Expr(last), rest)) => {
                    let scope = Scope::child(env);
                    for s in rest {
                        self.exec_stmt(s, &scope)?;
                    }
                    if crate::debug::on() {
                        return crate::debug::around(self, end, &scope, |it| it.render_expr_into(last, &scope, out));
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
        // One scope for the rows, emptied for each, unless a row kept it.
        let mut scope: Option<Env> = None;
        let mut each = |this: &mut Self, item: &Value| -> Result<(), Ctrl> {
            if !scope.as_mut().map_or(false, Scope::reuse) {
                scope = Some(Scope::child(env));
            }
            let scope = scope.as_ref().expect("set above");
            // `for p in items`: nothing to match.
            if let Pattern::Bind(n) = pat {
                define(scope, n, item.clone());
                return this.render_expr_into(body, scope, out);
            }
            let mut binds = Vec::new();
            if this.match_pat(pat, item, Mode::Strict, env, &mut binds)?.is_none() {
                return fail(line, format!("`for` pattern does not match {}", short(item)));
            }
            for (n, x) in binds {
                define(scope, &n, x);
            }
            this.render_expr_into(body, scope, out)
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
        if let Some(s) = scope {
            Scope::done(s);
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
            // A record: the value made as `User(v)` makes it.
            Pattern::Type(t) if self.types.contains_key(t) => {
                let ty = Ty { names: vec![t.clone()], optional: false, elem: None, fields: None };
                crate::record::make(self, &ty, v, lenient)?.ok()
            }
            // A decimal from an int, numeric text or a JSON number, in
            // any pattern: what it is given is converted.
            Pattern::Type(t) if t == "dec" => as_dec(v).map(Value::Dec),
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
                            // The same, under a name of its own (see `privacy`).
                            if let Pattern::As(q, n) = &f.pat {
                                if q.is_type_test() {
                                    binds.push((n.clone(), d));
                                    continue;
                                }
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
                // Refused when the program is loaded; never written all the same.
                AttrVal::Expr(_) if code_from_data(&el.tag, &el.attrs, name, val) => {}
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
        // Room for what it made last time, at once.
        use std::sync::atomic::Ordering::Relaxed;
        let mut out = String::with_capacity(el.facts.last_len.load(Relaxed));
        self.render_el_into(el, env, &mut out)?;
        el.facts.last_len.store(out.len(), Relaxed);
        Ok(out)
    }

    fn render_el_into(&mut self, el: &HtmlEl, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        let f = &el.facts;
        if f.component {
            return self.render_component_into(el, env, out);
        }
        // An element whose HTML never changes is made once.
        if f.fixed {
            if let Some(h) = f.html_once.get() {
                out.push_str(h);
                return Ok(());
            }
            let start = out.len();
            self.render_tag_into(el, env, out)?;
            let _ = f.html_once.set(Arc::from(&out[start..]));
            return Ok(());
        }
        // Tags that never change: their HTML is in pieces, made once.
        if f.tags_fixed {
            if f.pieces.get().is_none() {
                let mut pieces = Vec::new();
                let mut html = String::new();
                self.pieces_into(el, env, &mut pieces, &mut html)?;
                if !html.is_empty() {
                    pieces.push(Piece::Html(html.into()));
                }
                let _ = f.pieces.set(pieces);
            }
            for p in f.pieces.get().expect("set above") {
                match p {
                    Piece::Html(h) => out.push_str(h),
                    Piece::Expr(e) => self.render_expr_into(e, env, out)?,
                    Piece::El(e) => self.render_el_into(e, env, out)?,
                }
            }
            return Ok(());
        }
        self.render_tag_into(el, env, out)
    }

    /// `el` as pieces (see `ElFacts::pieces`): its tags and those of the
    /// elements in it like it as HTML, the rest as it is, to render each time.
    fn pieces_into(&mut self, el: &HtmlEl, env: &Env, pieces: &mut Vec<Piece>, html: &mut String) -> Result<(), Ctrl> {
        html.push('<');
        html.push_str(&el.tag);
        self.render_attrs_fast(el, env, html)?;
        html.push('>');
        if el.facts.void {
            return Ok(());
        }
        for c in &el.children {
            match c {
                HtmlChild::El(e) if e.facts.tags_fixed => self.pieces_into(e, env, pieces, html)?,
                HtmlChild::Expr(e @ Expr::Lit(Lit::Str(_))) => self.render_expr_into(e, env, html)?,
                _ => {
                    if !html.is_empty() {
                        pieces.push(Piece::Html(std::mem::take(html).into()));
                    }
                    pieces.push(match c {
                        HtmlChild::El(e) => Piece::El(e.clone()),
                        HtmlChild::Expr(e) => Piece::Expr(e.clone()),
                    });
                }
            }
        }
        html.push_str("</");
        html.push_str(&el.tag);
        html.push('>');
        Ok(())
    }

    fn render_tag_into(&mut self, el: &HtmlEl, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        let f = &el.facts;
        if f.html {
            out.push_str("<!doctype html>");
        }
        out.push('<');
        out.push_str(&el.tag);
        // Most elements have no spread, no `class=` and are no <textarea>:
        // their attributes are written as they are evaluated.
        let text_value = if f.plain_attrs { self.render_attrs_fast(el, env, out)?; None } else { self.render_attrs(el, env, out)? };
        out.push('>');
        if f.void {
            return Ok(());
        }
        match text_value {
            Some(t) => out.push_str(&escape(&t)),
            // A script is code to the browser, where HTML escaping means
            // nothing: a value goes in as JSON (a JS value, `<` escaped), and
            // HTML (`raw(...)`) as it is, but without `</` or `<!--`, so
            // neither can end the element early.
            None if el.tag == "script" => {
                for c in &el.children {
                    match c {
                        HtmlChild::Expr(e) if !matches!(e, Expr::Lit(Lit::Str(_))) => {
                            let v = self.eval(e, env)?;
                            script_value(&v, out);
                        }
                        c => self.render_child(c, env, out)?,
                    }
                }
            }
            // A style sheet is raw text to the browser: `.a > b` must stay
            // as written. Only `</` is changed, so the text cannot end the
            // element early.
            None if f.style => {
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
        // Names are compared as the browser does (`ACTION` is `action`), so
        // a spread key cannot stand in for a written attribute.
        let spread: Vec<(String, Value)> = spread.into_iter().filter(|(n, _)| spread_attr_allowed(n)).collect();
        let mut written: HashSet<String> = explicit.iter().map(|(n, _)| attr_key(n)).collect();
        // Written classes first, then the spread's.
        let (spread_class, spread): (Vec<_>, Vec<_>) = spread.into_iter().partition(|(n, _)| attr_key(n) == "class");
        let mut entries: Vec<(String, Value)> = Vec::new();
        for (n, v) in spread {
            // Once each: the first of `href` and `HREF` in a spread.
            if !(attr_key(&n) == "id" && el.id.is_some()) && written.insert(attr_key(&n)) {
                entries.push((n, v));
            }
        }
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
                _ if url_attr(name) && shows_script_url(&v) => Value::str(BLOCKED_URL),
                _ if anim_value_attr(&el.tag, name) && shows_script_url_in_list(&v) => Value::str(BLOCKED_URL),
                (Value::Str(s), "href" | "src" | "action" | "formaction" | "poster") if !self.base.is_empty() => {
                    Value::str(self.with_base(s))
                }
                _ => v,
            };
            if name == "value" && el.tag == "textarea" {
                text_value = Some(if matches!(v, Value::Nil) { String::new() } else { v.display() });
                continue;
            }
            if name.eq_ignore_ascii_case("class") {
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
            if code_from_data(&el.tag, &el.attrs, name, val) {
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
                AttrVal::Expr(Expr::Template(parts)) if !url_attr(name) && !anim_value_attr(&el.tag, name) => {
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
                        self.write_url_attr(&el.tag, name, &url, out);
                    }
                    self.spare = url;
                    r?;
                }
                AttrVal::Expr(e) => {
                    let v = self.eval(e, env)?;
                    self.write_attr(&el.tag, name, v, out);
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
            if self.fuel != u64::MAX && out.len() > crate::script::SCRIPT_BYTES {
                return fail(0, "the script made a value larger than 1 MB");
            }
        }
        Ok(())
    }

    /// ` name="url"` for a URL attribute of a `tag` (see `write_attr`).
    fn write_url_attr(&self, tag: &str, name: &str, url: &str, out: &mut String) {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        if script_url(url) || (anim_value_attr(tag, name) && script_url_in_list(url)) {
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
    fn write_attr(&self, tag: &str, name: &str, v: Value, out: &mut String) {
        let v = match (&v, name) {
            _ if url_attr(name) && shows_script_url(&v) => Value::str(BLOCKED_URL),
            _ if anim_value_attr(tag, name) && shows_script_url_in_list(&v) => Value::str(BLOCKED_URL),
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
        if !self.island_names.contains(&el.tag) {
            self.island_names.push(el.tag.clone());
        }
        self.mark_impure();
        self.islands.push((Map::new(), Map::new()));
        // A component placed in the page may defer: what renders it again (branch defer).
        let again = (!nested).then(|| args.clone());
        let mut body = String::new();
        let was = std::mem::replace(&mut self.in_component, true);
        let r = crate::value::js_numbers(true, || self.render_call_into(&c, args, line, &mut body));
        self.in_component = was;
        let (state, live) = self.islands.pop().unwrap_or_default();
        let defers = if nested { Vec::new() } else { std::mem::take(&mut self.island_defers) };
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
        // Waiting: registered, and marked for the browser to leave alone until it comes.
        let mut extra = String::new();
        if !defers.is_empty() {
            let id = self.deferred.len();
            let mut kept = state.clone();
            for n in &defers {
                kept.shift_remove(n.as_str());
            }
            self.deferred.push(crate::defer::Deferred {
                id, c: c.clone(), args: again.unwrap_or_default(), tag: el.tag.clone(), props: props.clone(), state: kept, live: live.clone(), line,
            });
            extra = format!(" data-qd=\"{}\"", id);
        }
        let eager = crate::island::runs_by_itself(&c.def.body);
        self.island_wrap(&el.tag, props, state, live, eager, &extra, &body, out);
        Ok(())
    }

    /// A component's island: its HTML in a `<quillo-island>` carrying its props and
    /// state (or, inside a table or a select, its own root element carrying them).
    #[allow(clippy::too_many_arguments)]
    fn island_wrap(&self, tag: &str, props: Map, state: Map, live: Map, eager: bool, extra: &str, body: &str, out: &mut String) {
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
        // A component that runs by itself (timers, live data, `@mount`, the
        // page's JavaScript) is started as soon as the page is idle; others
        // when someone reaches for them (see the loader in server.rs).
        let data = format!(
            " data-c=\"{}\" data-p=\"{}\" data-s=\"{}\"{}{}{}",
            escape(tag),
            escape(&island_json(&crate::server::for_visitor(Value::map(props))).to_string()),
            escape(&island_json(&crate::server::for_visitor(Value::map(state))).to_string()),
            live_attrs,
            if eager { " data-e" } else { "" },
            extra
        );
        // Inside a table or a select the browser's parser moves any other
        // element out, island and all: there the component's own root
        // element is its island.
        if let Some(at) = crate::island::own_root(body) {
            out.push_str(&body[..at]);
            out.push_str(" data-qi");
            out.push_str(&data);
            out.push_str(&body[at..]);
            return;
        }
        out.push_str("<quillo-island style=\"display:contents\"");
        out.push_str(&data);
        out.push('>');
        out.push_str(body);
        out.push_str("</quillo-island>");
    }

    /// A waiting component rendered again (src/defer.rs; branch defer): the states it
    /// had as they were, its deferred ones worked out now.
    pub(crate) fn render_deferred(&mut self, d: &crate::defer::Deferred) -> Result<String, Ctrl> {
        self.state_preset = Some(d.state.clone());
        self.islands.push((Map::new(), Map::new()));
        let mut body = String::new();
        let was = std::mem::replace(&mut self.in_component, true);
        let r = crate::value::js_numbers(true, || self.render_call_into(&d.c, d.args.clone(), d.line, &mut body));
        self.in_component = was;
        let (state, mut live) = self.islands.pop().unwrap_or_default();
        self.state_preset = None;
        r?;
        // What it followed the first time, it follows still.
        for (k, v) in d.live.iter() {
            if !live.contains_key(k) {
                live.insert(k.clone(), v.clone());
            }
        }
        for (k, x) in state.iter() {
            if let Some(bad) = not_sendable(x) {
                return fail(d.line, format!("state `{}` of <{}> is {}, which cannot be sent to the browser", k, d.tag, bad));
            }
        }
        let mut out = String::new();
        self.island_wrap(&d.tag, d.props.clone(), state, live, crate::island::runs_by_itself(&d.c.def.body), "", &body, &mut out);
        Ok(out)
    }
}

/// A module (`date`, `json`): a map of builtins, which no write changes.
fn is_module_map(m: &Map) -> bool {
    !m.is_empty() && m.values().all(|x| matches!(x, Value::Builtin(..)))
}

/// The top-level keys whose values differ between a global's old value and
/// its new one, when either is a map: what a loader that read one field of it
/// follows. Values the new map shares with the old (`{ ...world, tick: n }`
/// keeps `scores` as it was) are the same at once; others are compared when
/// small, and taken as changed when large (FEEDBACK12.md, Q-084; branch
/// open-fixes).
fn changed_fields(old: &Value, new: &Value) -> Vec<String> {
    fn same(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Map(x), Value::Map(y)) => Arc::ptr_eq(x, y) || (x.len() <= 32 && crate::value::equal(a, b)),
            (Value::List(x), Value::List(y)) => Arc::ptr_eq(x, y) || (x.len() <= 32 && crate::value::equal(a, b)),
            (Value::Str(x), Value::Str(y)) => Arc::ptr_eq(x, y) || x == y,
            (Value::Func(x), Value::Func(y)) => Arc::ptr_eq(x, y),
            _ => crate::value::equal(a, b),
        }
    }
    let empty = Map::new();
    let (o, n) = match (old, new) {
        (Value::Map(o), Value::Map(n)) => (&**o, &**n),
        (Value::Map(o), _) => (&**o, &empty),
        (_, Value::Map(n)) => (&empty, &**n),
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for (k, v) in n {
        if o.get(k).map_or(true, |w| !same(w, v)) {
            out.push(k.to_string());
        }
    }
    for k in o.keys() {
        if !n.contains_key(k) {
            out.push(k.to_string());
        }
    }
    out
}

/// `defer(x)`, the builtin (not a function of the program's own of that name).
fn defer_call(e: &Expr, env: &Env) -> bool {
    matches!(e, Expr::Call(f, args, _) if args.len() == 1
        && matches!(&**f, Expr::Ident(n, _) if n == "defer" && matches!(lookup(env, "defer"), Some(Value::Builtin("defer", _)))))
}

/// Whether `Chart`, `L.control` or the like starts at a browser library's name
/// (branch js-libraries).
fn lib_root(e: &Expr, env: &Env) -> bool {
    match e {
        Expr::Ident(n, _) => matches!(lookup(env, n), Some(Value::Builtin("__jslib", _))),
        Expr::Field(o, _, _) => lib_root(o, env),
        _ => false,
    }
}
