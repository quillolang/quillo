//! Tree-walking evaluator: expressions, patterns and HTML rendering.

use crate::ast::*;
use crate::decimal::Dec;
use crate::error::QuilloError;
use crate::parser::Parser;
use crate::shared::Shared;
use crate::value::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Non-local exits: runtime errors, `return` (and `?`), and `reply`. An error is boxed: every
/// expression's result is a `Result<Value, Ctrl>`, and it is a third smaller so, where an
/// error is rare (FEEDBACK29.md; branch feedback29-fixes).
pub enum Ctrl {
    Error(Box<QuilloError>),
    Return(Value),
    Reply(Arc<Response>),
}

/// What a call changes as it starts, kept to be put back after one that panicked
/// (`Interp::entered`).
pub(crate) struct Entered {
    depth: usize,
    cur_file: Option<Arc<str>>,
    caller_file: Option<Arc<str>>,
    in_component: bool,
    allowed: u8,
    restrictor: Option<Arc<FnDef>>,
}

impl From<QuilloError> for Ctrl {
    fn from(e: QuilloError) -> Self {
        Ctrl::Error(Box::new(e))
    }
}

impl From<Box<QuilloError>> for Ctrl {
    fn from(e: Box<QuilloError>) -> Self {
        Ctrl::Error(e)
    }
}

static NEXT_CODE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Builtins that change something: a page prefetched does not call them
/// (see `Interp::no_writes`).
pub(crate) const CHANGES: &[&str] = &[
    "db.exec",
    "db.migrate",
    "db.atomic",
    "idempotent",
    "set_cookie",
    "delete_cookie",
    "set_session",
    "end_session",
    // A token kept, or taken away (FEEDBACK56.md, Q-859; branch feedback56).
    "mcp_token",
    "revoke_mcp_token",
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
        (Err(Ctrl::Error(e)), Some(f)) if e.wants_file() => Err(Ctrl::Error(Box::new(e.in_file(&crate::error::shown_path(f))))),
        (r, _) => r,
    }
}

pub fn fail<T>(line: u32, msg: impl Into<String>) -> Result<T, Ctrl> {
    Err(Ctrl::Error(Box::new(QuilloError::new(msg, line))))
}

/// Whether a builtin's refusal is of an error value it was given.
fn refused_an_error(msg: &str) -> bool {
    msg.contains("got error") || msg.contains("convert error") || msg.contains("got an error")
}

/// What an error given to a builtin was: `it failed before: read_file: cannot read x.json`.
fn earlier((inner, at): &(Arc<Value>, Place)) -> String {
    let text = crate::value::told(|| inner.display());
    format!("it failed before this call, {}", at.before(&text))
}

fn undefined(name: &str, line: u32) -> Ctrl {
    Ctrl::Error(Box::new(QuilloError::new(format!("unknown name `{}`{}", name, crate::error::removed_note(name)), line)))
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
    /// What `use`, `mount` and `service` paths count from, for a main file that is a link: the
    /// folder of the file it leads to; empty, `base_dir` (see `src_dir()`).
    src_dir: Arc<Path>,
    pub(crate) loaded: Arc<HashSet<PathBuf>>,
    /// The Rust files a `use` has taken every name of, where the names are the program's
    /// (FEEDBACK55.md, Q-816; branch feedback55).
    rust_whole: Arc<HashSet<PathBuf>>,
    /// What each file's names are called in the program (src/modules.rs).
    links: Arc<crate::modules::Links>,
    /// Files of programs this one mounts or calls as services: watched
    /// with its own (`--watch`), but not its own, so a file both `use`
    /// is still run here (FEEDBACK4.md, Q-040; branch bank-fixes).
    pub(crate) watched: Arc<HashSet<PathBuf>>,
    /// The program's files as written, where a link is on their way (see `note_link`).
    pub(crate) via_links: Arc<HashSet<PathBuf>>,
    pub shared: Arc<Shared>,
    /// The services this version of the program loaded (see `shared::Services`).
    pub(crate) services: Arc<crate::shared::Services>,
    /// Calls in progress on this thread, to stop runaway recursion cleanly.
    depth: usize,
    /// The quotient of two ints a pipe is passing on, for `floor` to make in ints (`eval_pipe`):
    /// for the pipe's own call to take as it begins (`pipe_div`), then for the call itself, set
    /// just before it and taken by it (`exact_div`), so that no argument, inner or later call
    /// sees it (found in review).
    pipe_div: Option<Box<ExactDiv>>,
    exact_div: Option<Box<ExactDiv>>,
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
    /// The running request's route's `with`, for what its answer runs after the body: a
    /// stream's function, deferred parts (branch feedback40-43).
    pub(crate) route_limit: Option<Arc<FnDef>>,
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
    /// Its hidden id, once read or made; `Some(None)` once it ended (see `session::session_id`).
    pub(crate) session_id: Option<Option<Arc<str>>>,
    /// When the session set in this request ends, where the program gave it a `max_age`; None
    /// until one is set here (see `session::given_life`; FEEDBACK56.md, Q-859; branch feedback56).
    pub(crate) session_ends: Option<Option<u64>>,
    /// The answer read who the visitor is, from a cookie: one visitor's, for caches to keep
    /// apart (FEEDBACK31.md, Q-391; branch feedback27-35-fixes).
    pub visitor_read: bool,
    /// The answer was made in the visitor's language: it varies by what chose it
    /// (FEEDBACK55.md, Q-886; branch feedback55).
    pub locale_read: bool,
    /// A request is being answered, the parts made after its route (a stream, a deferred
    /// part) included: code here runs for someone (see `wt::reach`).
    pub answering: bool,
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
    /// The request being answered, and whose a kept value of it is once that was asked (see
    /// `Asked`, `Interp::who`).
    pub(crate) asked: Option<crate::server::Asked>,
    pub(crate) who: Option<u64>,
    /// State of the components being rendered, innermost last, with the
    /// `live(...)` subscriptions of each.
    islands: Vec<(Map, Map)>,
    /// The value of the `<select>` whose start tag is being written, then what its options are
    /// rendered against (see `render_option_into`; FEEDBACK55.md, Q-758; branch feedback55).
    select_pending: Option<Value>,
    select_want: Option<SelectWant>,
    /// Where each component being rendered keeps its states, innermost last, to read them as its
    /// first render leaves them; and the states a second render of one starts with, with how deep
    /// it is (see `render_again`; FEEDBACK55.md, Q-770; branch feedback55).
    island_scopes: Vec<(usize, Env, Option<Value>)>,
    island_scopes_at: Vec<usize>,
    /// Rendering inside `<svg>` (and not in a `<foreignObject>` there): a component's root is its
    /// own island (see `island_wrap`; FEEDBACK55.md, Q-763; branch feedback55).
    in_svg: bool,
    /// Components marked by comments so far (see `island_wrap`), and those of them that run by
    /// themselves: what an island around them says (FEEDBACK61.md, Q-892; branch feedback61).
    marked: (usize, usize),
    state_again: Option<(usize, Map)>,
    /// While the server renders a component: its code also runs in the
    /// browser, where `http.*` calls are made (see src/fetch.rs). A
    /// `server fn` it calls runs on the server, and clears it.
    pub(crate) in_component: bool,
    /// While a `live` loader runs: the topics (tables, globals) it reads.
    pub reads: Option<std::collections::HashSet<String>>,
    /// The code read the request (its session, cookies, headers): what it
    /// made is this viewer's (FEEDBACK2.md, Q-028; branch feedback6-fixes).
    pub read_request: bool,
    /// A `live` run is under way, in `unwatched` too; and it acted (see `viewcache::Kind::Acts`):
    /// what it made goes to no one else (FEEDBACK55.md, Q-888; branch feedback55).
    pub in_run: bool,
    pub acted: bool,
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
    /// Transactions running here that take an error a statement drops: the first such
    /// error, which rolls the innermost back (FEEDBACK27.md, Q-332; branch feedback27-35-fixes).
    pub catching_drops: usize,
    pub dropped: Option<Value>,
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
    /// Set when a request that may change something does: the guard's own list of what
    /// changes things, so that a page kept for a click can be let go (FEEDBACK22.md, Q-177;
    /// branch feedback22-fixes).
    pub(crate) changed: Option<Arc<std::sync::atomic::AtomicBool>>,
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
    /// The files of those services, for the program's API to describe their routes
    /// (FEEDBACK32.md, Q-397; branch feedback27-35-fixes).
    pub remote_sources: Vec<(String, PathBuf)>,
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
    /// In a template string written straight in `print(...)`.
    printing: bool,
    /// What `in_place_call` gave back is already where it goes.
    stored: bool,
    /// This program's, for compiled loops (src/jitloop.rs): new for each one loaded, the
    /// same in a fork, so code made for one program's loop is never another's.
    pub(crate) code_id: u64,
    /// Whether a program's last statement gives its value, as `load_source` returns it.
    keep_last: bool,
    /// The long text last read by index, and what a short one needed.
    text: Option<TextIndex>,
    text_small: Option<Vec<u32>>,
    /// Running a scheduled job or a tick, not a request: its `sleep` ends at a
    /// stop or a reload (FEEDBACK12.md, Q-081 and Q-082; branch feedback12-sleep).
    pub(crate) in_job: bool,
    /// The `parallel` call this runs a function of, once another of its functions failed: this
    /// one stops at its next step (see `Stop`).
    pub(crate) stop: Option<Arc<Stop>>,
    /// A service's own `.env`, beside its file (FEEDBACK9.md, Q-071; branch
    /// feedback9-fixes). The app's is in the process's environment.
    pub env_file: Arc<std::collections::HashMap<String, String>>,
    /// The services the program declares, and the checks it adds with
    /// `ready`, for `/_quillo/ready` (FEEDBACK10.md, Q-076; branch
    /// feedback10-fixes).
    pub declared_services: Arc<Vec<String>>,
    /// How calls from other services are taken (`accept_calls`), and which callers each
    /// `server fn ... for [...]` takes (FEEDBACK38.md, Q-458 and Q-460; branch feedback36-39).
    pub accept: Option<crate::svcauth::Shared>,
    pub fn_callers: Arc<std::collections::HashMap<String, Vec<String>>>,
    /// The program's MCP door over HTTP, `/_quillo/mcp`, when it has tools and is not run as a
    /// service: its tools as the checker read them, shared by its forks (FEEDBACK56.md, C.2;
    /// branch feedback56).
    pub(crate) mcp: Option<Arc<crate::mcp::Door>>,
    /// A tool's call at the MCP door that may ask its user (`confirm`, `about`), while it runs:
    /// what was answered, and what it changed, so that a question after a change is refused
    /// (FEEDBACK56.md, Part E; branch feedback56). None everywhere else.
    pub(crate) asking: Option<Box<crate::mcp::Asking>>,
    /// A tool's call runs (`mcp::answer_tool`), here or in a fork of it. With no `asking`, a
    /// question its code reaches anyway, through a function `quillo check` cannot follow (a
    /// `locale_from` rule), is refused: it had been answered as outside any tool's call, `confirm`
    /// true, a yes nobody gave (FEEDBACK56.md, found in the coverage review; branch feedback56).
    pub(crate) in_tool: bool,
    /// Run as a service (`--as-service`): its tools, which its app's door lists as its own and
    /// passes the calls of on to it, signed (FEEDBACK56.md, Part F; branch feedback56). None
    /// everywhere else, and for a service with none.
    pub(crate) offered_tools: Option<Arc<Vec<crate::mcp::Tool>>>,
    pub ready_checks: Arc<Vec<(String, Value)>>,
    /// The command components' styles go through (`style_with`; FEEDBACK11.md,
    /// Q-079; branch feedback11-fixes).
    pub style_with: Option<String>,
    /// Or the function they go through (FEEDBACK37.md, Q-455; branch feedback36-39).
    pub style_with_fn: Option<Value>,
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
    /// Styles that ran with no element to carry their mark (see `style_noop`).
    pub styles_used: Vec<String>,
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

/// What a `parallel` function is stopped with once another has failed (see `Stop`).
pub const PARALLEL_STOPPED: &str = "stopped: another function given to `parallel` failed";

/// A `parallel` call, for its functions: once one fails, the others stop at their next step (a
/// call, a loop's round, the end of a `sleep`), as the guide says, where they all ran to their
/// end and kept what they wrote (FEEDBACK55.md, Q-804; branch feedback55). Inside another
/// `parallel`'s function, that call's stop is this one's too.
pub(crate) struct Stop {
    now: std::sync::atomic::AtomicBool,
    up: Option<Arc<Stop>>,
}

impl Stop {
    pub(crate) fn stopped(&self) -> bool {
        self.now.load(std::sync::atomic::Ordering::Relaxed) || self.up.as_ref().is_some_and(|u| u.stopped())
    }
}

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
        // Money every `money` function takes (FEEDBACK50.md, Q-621; branch feedback50-fixes).
        "money" => crate::money::is_money(v),
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
/// Whether `??` may take a stop: not one that means the request must end, the compute limit's
/// whether `--max-compute` or a policy set it (FEEDBACK55.md, Q-849; branch feedback55).
fn guardable(msg: &str) -> bool {
    // Nor a tool's stop to ask its user, which `x ?? y` would take for null and go on
    // (FEEDBACK56.md, Part E; branch feedback56).
    !msg.contains(PREFETCH_REFUSED) && !crate::httpd::is_compute_stop(msg) && !msg.contains("the script ") && msg != PARALLEL_STOPPED && msg != crate::mcp::ASKING
}

/// What a `parallel` function did to the answer being made (see `Interp::back`).
#[derive(Default)]
struct Back {
    cookies: Vec<String>,
    session: Option<Value>,
    visitor_read: bool,
    locale_read: bool,
    read_request: bool,
    acted: bool,
    /// What it announced while answering another service's call, and whether it wrote to the
    /// database (FEEDBACK55.md, Q-796, found in review; branch feedback55).
    published: Option<Vec<String>>,
    wrote_req: Option<usize>,
    /// A question refused in a tool's call with no asking (`Interp::in_tool`): the call stops,
    /// though the function that reached it went on (FEEDBACK56.md, the coverage review's security
    /// review; branch feedback56).
    refused: Option<Box<crate::mcp::Asking>>,
}

/// A text shorter than this is looked at each time it is read by index, not kept.
const TEXT_SMALL: usize = 64;

/// What is known of a text read by index.
struct TextIndex {
    text: Arc<str>,
    chars: usize,
    /// Where each character starts, unless each is a byte.
    offsets: Option<Vec<u32>>,
}

/// A character as text, shared for the ASCII ones: a loop over a text makes one each
/// round.
fn one_char(c: char) -> Value {
    static ASCII: OnceLock<Vec<Value>> = OnceLock::new();
    if c.is_ascii() {
        let all = ASCII.get_or_init(|| (0u8..128).map(|b| Value::str((b as char).to_string())).collect());
        return all[c as usize].clone();
    }
    Value::str(c.to_string())
}

/// Text from a URL, a query or a form as the type a pattern names, written one way: an int is
/// an optional `-` and digits with no leading `0`, a float or a decimal also with a fraction and
/// an exponent and finite, and nothing is trimmed. `/n/05`, `/n/+5` and `/n/%205` were 5 (one
/// page at many addresses), and `NaN`, `inf` and `1e999` were floats a JSON answer wrote as
/// null (FEEDBACK47.md, Q-542; branch feedback40-43). `bool` reads any case (Q-543).
pub fn coerce(t: &str, s: &str) -> Option<Value> {
    match t {
        "int" => plain_int(s).then(|| s.parse::<i64>().ok().map(Value::Int)).flatten(),
        "float" => plain_number(s).then(|| s.parse::<f64>().ok().filter(|f| f.is_finite()).map(Value::Float)).flatten(),
        "num" => coerce("int", s).or_else(|| coerce("float", s)),
        "dec" => plain_number(s).then(|| Dec::parse(s).map(Value::Dec)).flatten(),
        "date" => crate::dates::parse_date(s).map(|_| Value::str(s)),
        "bool" => match s.to_ascii_lowercase().as_str() {
            "true" | "on" | "1" | "yes" => Some(Value::Bool(true)),
            "false" | "off" | "0" | "no" => Some(Value::Bool(false)),
            _ => None,
        },
        "str" | "any" => Some(Value::str(s)),
        _ => None,
    }
}

/// `-12`, `0`, `7`: not `05`, `+5`, ` 5` or `-0`.
fn plain_int(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) && (digits == "0" || !digits.starts_with('0')) && s != "-0"
}

/// `-1.5`, `2e3`, `0.25`: an int as above, then a fraction and an exponent, each optional.
fn plain_number(s: &str) -> bool {
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (whole, frac) = match mantissa.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (mantissa, None),
    };
    // `-0.5` has a whole part of `-0`; `-0` alone is no number of its own.
    let whole_ok = plain_int(whole) || (whole == "-0" && (frac.is_some() || exp.is_some()));
    let frac_ok = frac.map_or(true, |f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()));
    let exp_ok = exp.map_or(true, |e| {
        let d = e.strip_prefix(['-', '+']).unwrap_or(e);
        !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
    });
    whole_ok && frac_ok && exp_ok
}

/// What `int(s)` and `float(s)` take, as Python's do: around spaces, a `+`, and for a float
/// `nan` and `inf` too.
pub fn coerce_loose(t: &str, s: &str) -> Option<Value> {
    let s = s.trim();
    match t {
        "int" => s.parse::<i64>().ok().map(Value::Int),
        "float" => s.parse::<f64>().ok().map(Value::Float),
        _ => coerce(t, s),
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
/// copied whole. The HTML parser makes a CR a newline and drops a NUL from text, so the page
/// said otherwise than the browser's own text, and it made the component again on every load: a
/// CR is `&#13;`, which the parser keeps, and a NUL U+FFFD, as the parser makes it in an attribute
/// and the browser writes it (FEEDBACK55.md, Q-772; branch feedback55). In an attribute too, as
/// Q-312 had it there (FEEDBACK25.md; branch feedback26-fixes).
pub fn escape_into(s: &str, out: &mut String) {
    fn entity(b: u8) -> Option<&'static str> {
        Some(match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&#39;",
            b'\r' => "&#13;",
            0 => "\u{FFFD}",
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
        if has(x, b'&') | has(x, b'<') | has(x, b'>') | has(x, b'"') | has(x, b'\'') | has(x, b'\r') | has(x, 0) != 0 {
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

/// A `<style>` child's text: a list's items a line each; nothing for null and false.
fn sheet_text(v: &Value) -> String {
    match v {
        Value::Nil | Value::Bool(false) => String::new(),
        Value::List(items) => items.iter().map(|x| x.display()).collect::<Vec<_>>().join("\n"),
        other => other.display(),
    }
}

/// The builtins that make a quotient of two ints whole in ints (`floor(a / b)`; FEEDBACK55.md,
/// Q-788; branch feedback55).
const ROUNDERS: &[&str] = &["floor", "ceil", "round", "math.floor", "math.ceil", "math.trunc"];

/// A quotient of two ints a pipe passes on as a float (`eval_pipe`): its bits, the ints, and its
/// line.
#[derive(Clone, Copy)]
pub struct ExactDiv {
    bits: u64,
    a: i64,
    b: i64,
    line: u32,
}

/// Whether a URL would run code when followed: `javascript:` or `vbscript:`,
/// however browsers would read it (they skip leading spaces and control
/// characters and ignore tabs and line breaks inside).
pub fn script_url(url: &str) -> bool {
    script_scheme(url).is_some()
}

/// The scheme of a URL that would run code when followed, as `script_url` reads it:
/// `javascript:` or `vbscript:`, for a refusal to name the one given (`redirect("vbscript:...")`
/// was told of javascript:; FEEDBACK55.md; branch feedback55).
pub fn script_scheme(url: &str) -> Option<&'static str> {
    // Paths and fragments, the usual case, have no scheme.
    if matches!(url.as_bytes().first(), Some(b'/' | b'#' | b'?')) {
        return None;
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
    if scheme.starts_with(b"javascript:") {
        Some("javascript:")
    } else if scheme.starts_with(b"vbscript:") {
        Some("vbscript:")
    } else {
        None
    }
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
        && !runtime_attr(&key)
        && !name.starts_with("__")
        && !name.starts_with('@')
        && (!name.contains(':') || key == "xlink:href")
}

/// Whether `key`, an attribute's name in lower case, is one the runtime or the page's loader
/// reads to take an element up as a component (see `island_attr`), or Quillo's own
/// `data-q-<style>` and `data-quillo-…`. Visitor JSON in a spread wrote them, and the element became a component of
/// the visitor's choosing, with a string `children` put in as HTML (FEEDBACK55.md, Q-700;
/// branch feedback55).
fn runtime_attr(key: &str) -> bool {
    island_attr(key) || key.starts_with("data-q-") || key.starts_with("data-quillo-")
}

/// Whether `key`, an attribute's name in lower case, is one the server writes on a component's
/// island: its mark, `data-qd` while it waits and `data-qt` on what is swapped in for it
/// (src/defer.rs), its component, props, state, live data and whether it starts by itself.
pub(crate) fn island_attr(key: &str) -> bool {
    // `data-qm`: an island holding some marked by comments (FEEDBACK61.md, Q-892; branch feedback61).
    matches!(key, "data-qi" | "data-qd" | "data-qt" | "data-c" | "data-p" | "data-s" | "data-l" | "data-v" | "data-e" | "data-qm")
}

/// A list's or map's JSON as an attribute's value: a builtin's error in it says `internal error`
/// where the server hides them, as `json.stringify` does (FEEDBACK55.md, Q-773; branch
/// feedback55).
fn attr_json(v: &Value) -> String {
    let hidden = if crate::value::hiding() { crate::server::hidden(v) } else { None };
    hidden.as_ref().unwrap_or(v).json_text()
}

/// Whether `lower`, an attribute's name in lower case, is React's handler form, `onclick`
/// (see `react_event` in the parser).
fn is_react_handler(lower: &str) -> bool {
    lower.len() > 2 && lower.starts_with("on") && lower[2..].bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'_')
}

fn render_value(v: &Value, out: &mut String) {
    match v {
        Value::Html(h) => out.push_str(h),
        // `true` shows nothing, as `false` and null do, as in React; a function too, where it
        // showed `<fn <fn>>` (FEEDBACK48.md, Q-564 and Q-571; branch main).
        Value::Nil | Value::Bool(_) | Value::Func(_) | Value::Builtin(..) => {}
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
/// A map of CSS properties as a `style` attribute's text: `fontSize` and `font_size` are
/// `font-size`, a custom property (`--gap`) stays, and a null or false value is left out.
/// `style={{ color: c, font_size: "2rem" }}`, React's way: a property for each key, `_` and
/// camelCase written as `-` (FEEDBACK35.md, Q-429; branch feedback27-35-fixes), on every path
/// that writes attributes (Q-531).
fn style_value(name: &str, v: Value) -> Value {
    match &v {
        Value::Map(m) if name.eq_ignore_ascii_case("style") => Value::str(style_text(m)),
        _ => v,
    }
}

pub fn style_text(m: &Map) -> String {
    let mut out = String::new();
    for (k, v) in m.iter() {
        if matches!(v, Value::Nil | Value::Bool(false)) {
            continue;
        }
        // A property's name, or a custom property's: a key from data with `;`, `:` or
        // `{` in it wrote declarations of its own (FEEDBACK51.md, Q-668; branch
        // feedback51-fixes).
        let named = match k.strip_prefix("--") {
            Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
            None => !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        };
        if !named {
            continue;
        }
        if !out.is_empty() {
            out.push_str("; ");
        }
        let start = out.len();
        if k.starts_with("--") {
            out.push_str(k);
        } else {
            for c in k.chars() {
                match c {
                    '_' => out.push('-'),
                    c if c.is_ascii_uppercase() => {
                        out.push('-');
                        out.push(c.to_ascii_lowercase());
                    }
                    c => out.push(c),
                }
            }
            // `msFlex` is `-ms-flex`, as `WebkitFlex` is `-webkit-flex`, React's way
            // (FEEDBACK48.md, Q-565; branch main).
            if out[start..].starts_with("ms-") {
                out.insert(start, '-');
            }
        }
        let end = out.len();
        out.push_str(": ");
        // A number is in pixels, as React has it (`fontSize: 12` is `12px`, where the browser
        // dropped `font-size: 12`), but 0, a custom property and React's unitless properties
        // (`opacity`, `zIndex`, `lineHeight`, ...) (FEEDBACK48.md, Q-565; branch main).
        let nonzero = match v {
            Value::Int(n) => {
                push_int(&mut out, *n);
                *n != 0
            }
            Value::Float(f) => {
                out.push_str(&v.display());
                *f != 0.0 && f.is_finite()
            }
            Value::Dec(d) => {
                out.push_str(&v.display());
                !d.is_zero()
            }
            other => {
                out.push_str(&crate::builtins::css_value(&other.display()));
                false
            }
        };
        if nonzero && takes_px(&out[start..end]) {
            out.push_str("px");
        }
    }
    out
}

/// React's unitless CSS properties (`isUnitlessNumber`), as the attribute names them
/// (FEEDBACK48.md, Q-565; branch main).
const UNITLESS: &[&str] = &[
    "animation-iteration-count",
    "aspect-ratio",
    "border-image-outset",
    "border-image-slice",
    "border-image-width",
    "box-flex",
    "box-flex-group",
    "box-ordinal-group",
    "column-count",
    "columns",
    "fill-opacity",
    "flex",
    "flex-grow",
    "flex-negative",
    "flex-order",
    "flex-positive",
    "flex-shrink",
    "flood-opacity",
    "font-weight",
    "grid-area",
    "grid-column",
    "grid-column-end",
    "grid-column-span",
    "grid-column-start",
    "grid-row",
    "grid-row-end",
    "grid-row-span",
    "grid-row-start",
    "line-clamp",
    "line-height",
    "opacity",
    "order",
    "orphans",
    "scale",
    "stop-opacity",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "tab-size",
    "widows",
    "z-index",
    "zoom",
];

/// Whether a number for CSS property `name` (`font-size`) is in pixels: not for a custom
/// property or a unitless one, with or without a vendor prefix (`-webkit-flex`).
fn takes_px(name: &str) -> bool {
    if name.starts_with("--") {
        return false;
    }
    let base = ["-webkit-", "-moz-", "-ms-", "-o-"].iter().find_map(|p| name.strip_prefix(p)).unwrap_or(name);
    !UNITLESS.contains(&base)
}

/// The text a bool is for an attribute whose values are words: "true" and "false" for ARIA's,
/// `data-` ones and the enumerated `draggable`, `spellcheck` and `contenteditable`, "yes" and
/// "no" for `translate`, as React writes them. None for the rest, HTML's boolean attributes
/// (`disabled`), which are there (bare) or not (FEEDBACK48.md, Q-564; branch main).
pub fn bool_attr_text(name: &str, b: bool) -> Option<&'static str> {
    let n = name.as_bytes();
    let has = |p: &str| n.len() > p.len() && n[..p.len()].eq_ignore_ascii_case(p.as_bytes());
    if has("aria-") || has("data-") || ["draggable", "spellcheck", "contenteditable"].iter().any(|w| name.eq_ignore_ascii_case(w)) {
        Some(if b { "true" } else { "false" })
    } else if name.eq_ignore_ascii_case("translate") {
        Some(if b { "yes" } else { "no" })
    } else {
        None
    }
}

/// ` name="true"` for a bool attribute whose values are words (see `bool_attr_text`), else
/// ` name` for true and nothing for false.
fn write_bool_attr(name: &str, b: bool, out: &mut String) {
    match bool_attr_text(name, b) {
        Some(t) => {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            out.push_str(t);
            out.push('"');
        }
        None if b => {
            out.push(' ');
            out.push_str(name);
        }
        None => {}
    }
}

/// The classes `class={...}` gives, as clsx has them: a map's keys whose values are truthy, a
/// list's items (lists in it flattened, null, false and true skipped, a map in it as above),
/// joined with spaces; text as it is. A map or list was written as its Quillo text
/// (FEEDBACK48.md, Q-566; branch main).
fn class_words(v: &Value, out: &mut String) {
    let word = |w: &str, out: &mut String| {
        if !w.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(w);
        }
    };
    match v {
        Value::Nil | Value::Bool(_) | Value::Func(_) | Value::Builtin(..) => {}
        Value::Str(s) => word(s, out),
        Value::List(items) => {
            for i in items.iter() {
                class_words(i, out);
            }
        }
        Value::Map(m) => {
            for (k, x) in m.iter() {
                if x.truthy() {
                    word(k, out);
                }
            }
        }
        other => word(&other.display(), out),
    }
}

/// Whether the browser drops a newline right after the start tag of `tag`: <pre>, <textarea>
/// and <listing>, whose text keeps its layout (FEEDBACK48.md, Q-572; branch main).
fn drops_first_newline(tag: &str) -> bool {
    matches!(tag, "pre" | "textarea" | "listing")
}

/// A `<script>`'s own text as the page carries it: as written, but that `</` and `<!--` cannot
/// end it early (FEEDBACK35.md, Q-428; branch feedback27-35-fixes).
pub fn script_text(s: &str) -> String {
    s.replace("</", "<\\/").replace("<!--", "<\\!--")
}

fn script_value(v: &Value, out: &mut String) {
    match v {
        // As JSON, where they were left out as children are, and `const a = {null};` was
        // `const a = ;` (FEEDBACK53.md, Q-682; branch main).
        Value::Nil => out.push_str("null"),
        Value::Bool(false) => out.push_str("false"),
        Value::Html(h) => out.push_str(&h.replace("</", "<\\/").replace("<!--", "<\\!--")),
        other => out.push_str(&script_json(&crate::server::hidden(other).as_ref().unwrap_or(other).to_json())),
    }
}

/// JSON that is safe inside `<script>`: no `</script>`, `<!--` or line
/// separators (which end a string in older JavaScript), and no `&`, which inside `<svg>` or
/// `<math>` the browser decodes: `&quot;` ended a string there (FEEDBACK51.md, Q-646; branch
/// feedback51-fixes).
pub fn script_json(j: &serde_json::Value) -> String {
    serde_json::to_string(j).unwrap_or_default().replace('<', "\\u003c").replace('&', "\\u0026").replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")
}

/// JSON for an island's props and state, and for what server fns take and
/// give: like `to_json`, but an error value becomes `{"$quillo_err":
/// payload}`, a decimal `{"$dec": "12.50"}` and an int past what a JS number
/// holds whole `{"$int": "9007199254740993"}`, which the browser turns back
/// into an error value, its decimal and a BigInt. A map of one key that starts
/// with `$` is `{"$": [key, value]}`, read back as that map: a visitor's key
/// (`set({}, k, "boom")`) made an error value, an int or a decimal of it in the
/// browser, and on the server it was sent (FEEDBACK55.md, Q-875's review; branch
/// feedback55).
pub fn island_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Err(e, _, _) => serde_json::json!({ "$quillo_err": island_json(e) }),
        Value::Dec(d) => serde_json::json!({ "$dec": d.to_string() }),
        Value::Int(n) if n.unsigned_abs() > JS_SAFE => serde_json::json!({ "$int": n.to_string() }),
        Value::List(l) => serde_json::Value::Array(l.iter().map(island_json).collect()),
        Value::Map(m) => match m.iter().next() {
            Some((k, x)) if m.len() == 1 && k.starts_with('$') => serde_json::json!({ "$": [k.to_string(), island_json(x)] }),
            _ => serde_json::Value::Object(m.iter().map(|(k, x)| (k.to_string(), island_json(x))).collect()),
        },
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
                // A map of one key that starts with `$` (see `island_json`).
                ("$", J::Array(a)) if a.len() == 2 && a[0].is_string() => {
                    let mut m = Map::new();
                    m.insert(a[0].as_str().unwrap_or_default().into(), from_island_json(&a[1]));
                    Value::map(m)
                }
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

fn same_file(a: &Option<Arc<str>>, b: &Option<Arc<str>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

/// `(_i) => true`, as `map` is given to fill a list: always the same, so it is not called.
fn constant_lambda(def: &FnDef) -> Option<&Lit> {
    let [p] = def.params.as_slice() else { return None };
    if p.ty.is_some() || p.default.is_some() || def.ret.is_some() || def.err.is_some() || def.effects.is_some() || def.server || def.component {
        return None;
    }
    match &def.body {
        Expr::Lit(l) => Some(l),
        _ => None,
    }
}

/// `(x) => x`, as `count(flags, (f) => f)` and `filter` are given: what it is given is what
/// it gives, with no scope to make (FEEDBACK29.md; branch feedback29-fixes).
fn identity(def: &FnDef) -> bool {
    let [p] = def.params.as_slice() else { return false };
    p.ty.is_none() && p.default.is_none() && def.ret.is_none() && def.err.is_none() && def.effects.is_none() && !def.server && !def.component
        && matches!(&def.body, Expr::Ident(n, _) if *n == p.name)
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

/// A component called as a function, by another name than its own (`let make = Dead`, then
/// `make()`) or given to `map`: its HTML would come with nothing in the page to run it, so
/// its clicks would do nothing, with no word (FEEDBACK28.md, Q-345; branch feedback29-fixes).
fn component_called(name: &str) -> String {
    format!("`{}` is a component, which the page runs from its tag: write <{} ... />, as a function it would show and never answer", name, name)
}

/// What `children` is in a view or component: null for a tag given none, as the browser has
/// it, so `children == null` is the same on both sides and the page takes up the server's
/// HTML (FEEDBACK28.md, Q-345; branch feedback29-fixes).
fn children_of(el: &HtmlEl, kids: String) -> Value {
    if el.children.is_empty() {
        Value::Nil
    } else {
        Value::html(kids)
    }
}

/// The names of the builtins an assignment can do where it is.
fn update_names() -> (Sym, Sym, Sym, Sym) {
    static NAMES: OnceLock<(Sym, Sym, Sym, Sym)> = OnceLock::new();
    *NAMES.get_or_init(|| (Sym::new("set"), Sym::new("push"), Sym::new("merge"), Sym::new("remove")))
}

/// `f(x, a)`, `x | f(a)` or `x.f(a)`: the function's name, `x`, the rest and the line.
pub(crate) fn call_on(e: &Expr) -> Option<(Sym, &Expr, &[Expr], u32)> {
    match e {
        Expr::Call(f, args, line) => match (&**f, args.split_first()) {
            (Expr::Ident(f, _), Some((first, rest))) => Some((*f, first, rest, *line)),
            (Expr::Field(o, f, _), _) => Some((Sym::new(f), o, args, *line)),
            _ => None,
        },
        Expr::Pipe(lhs, rhs, _) => match &**rhs {
            Expr::Call(f, args, line) => match &**f {
                Expr::Ident(f, _) => Some((*f, lhs, args, *line)),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// An expression that calls nothing, so it does the same worked out twice, or earlier.
pub(crate) fn quiet(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) | Expr::Ident(..) => true,
        Expr::Unary(_, x, _) | Expr::Field(x, _, _) => quiet(x),
        Expr::Binary(_, a, b, _) | Expr::Index(a, b, _) => quiet(a) && quiet(b),
        Expr::List(xs) => xs.iter().all(quiet),
        Expr::Map(xs) => xs.iter().all(|(_, x)| quiet(x)),
        _ => false,
    }
}

/// An expression that cannot fail, whatever is in scope.
fn constant(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) => true,
        Expr::List(xs) => xs.iter().all(constant),
        Expr::Map(xs) => xs.iter().all(|(_, x)| constant(x)),
        _ => false,
    }
}

/// An int's text, as `str` and a map's key have it, written into `buf` with nothing made
/// (FEEDBACK55.md, string speed; branch feedback55).
pub(crate) fn int_text(i: i64, buf: &mut [u8; 20]) -> &str {
    let mut n = i.unsigned_abs();
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if i < 0 {
        at -= 1;
        buf[at] = b'-';
    }
    // SAFETY: ASCII digits and a sign, written just above.
    unsafe { std::str::from_utf8_unchecked(&buf[at..]) }
}

/// Whether two quiet expressions are written the same.
pub(crate) fn same_expr(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x, _), Expr::Ident(y, _)) => x == y,
        (Expr::Lit(Lit::Int(x)), Expr::Lit(Lit::Int(y))) => x == y,
        (Expr::Lit(Lit::Str(x)), Expr::Lit(Lit::Str(y))) => x == y,
        (Expr::Field(x, f, _), Expr::Field(y, g, _)) => f == g && same_expr(x, y),
        (Expr::Index(x, i, _), Expr::Index(y, j, _)) => same_expr(x, y) && same_expr(i, j),
        (Expr::Binary(p, x, i, _), Expr::Binary(q, y, j, _)) => p == q && same_expr(x, y) && same_expr(i, j),
        (Expr::Call(f, xs, _), Expr::Call(g, ys, _)) => {
            same_expr(f, g) && xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| same_expr(x, y))
        }
        _ => false,
    }
}

/// Builtins that only compute from what they are given: called twice, the same.
const PURE: &[&str] = &["str", "int", "float", "lower", "normalize", "upper", "trim", "len", "abs", "round", "floor", "ceil"];

/// `quiet`, or calls of those builtins, as they are in `env`: `set(m, str(i % 10), get(m,
/// str(i % 10), []) | push(i))` works its key out once, where it is written twice, and that
/// is the same (FEEDBACK29.md, Q-352; branch feedback29-fixes).
fn pure(e: &Expr, env: &Env) -> bool {
    match e {
        Expr::Call(f, args, _) => {
            let Expr::Ident(n, _) = &**f else { return false };
            PURE.contains(&n.as_str())
                && matches!(crate::value::builtin_named(env, *n), Some((b, _)) if b == n.as_str())
                && args.iter().all(|a| pure(a, env))
        }
        Expr::Lit(_) | Expr::Ident(..) => true,
        Expr::Unary(_, x, _) | Expr::Field(x, _, _) => pure(x, env),
        Expr::Binary(_, a, b, _) | Expr::Index(a, b, _) => pure(a, env) && pure(b, env),
        Expr::List(xs) => xs.iter().all(|x| pure(x, env)),
        Expr::Map(xs) => xs.iter().all(|(_, x)| pure(x, env)),
        _ => false,
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

/// `dec_hint`, and for `<` and the rest, `i < 0` after `index_of`, as from JavaScript
/// (FEEDBACK28.md, Q-347; branch feedback29-fixes).
fn compare_hint(a: &Value, b: &Value) -> &'static str {
    match (a, b) {
        (Value::Nil, b) if !matches!(b, Value::Nil) => NIL_COMPARED,
        (a, Value::Nil) if !matches!(a, Value::Nil) => NIL_COMPARED,
        _ => dec_hint(a, b),
    }
}

const NIL_COMPARED: &str = ": what is not found is null, not -1, from `index_of`, `find` and `get` too, so ask `x == null` first";

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
    r.map(Value::Dec).map_err(|e| Ctrl::Error(Box::new(QuilloError::new(e.message(), line))))
}

/// Two ints, for the operators where nothing can go otherwise: None for all else, which
/// `binop` works out or refuses. Inside `eval`, it is a third of what a loop's sum costs.
#[inline(always)]
fn int_op(op: BinOp, x: i64, y: i64) -> Option<Value> {
    Some(match op {
        BinOp::Add => Value::Int(x.checked_add(y)?),
        BinOp::Sub => Value::Int(x.checked_sub(y)?),
        BinOp::Mul => Value::Int(x.checked_mul(y)?),
        BinOp::Mod if x >= 0 && y > 0 => Value::Int(x % y),
        BinOp::Lt => Value::Bool(x < y),
        BinOp::Le => Value::Bool(x <= y),
        BinOp::Gt => Value::Bool(x > y),
        BinOp::Ge => Value::Bool(x >= y),
        BinOp::Eq => Value::Bool(x == y),
        BinOp::Ne => Value::Bool(x != y),
        _ => return None,
    })
}

pub fn binop(op: BinOp, a: Value, b: Value, line: u32) -> Result<Value, Ctrl> {
    use Value::*;
    let overflow = || Ctrl::Error(Box::new(QuilloError::new("integer overflow", line)));
    // Two money values add up, and subtract (FEEDBACK50.md, Q-620; branch feedback50-fixes).
    if matches!((op, &a, &b), (BinOp::Add | BinOp::Sub, Map(_), Map(_))) {
        if let Some(r) = crate::money::op(&a, &b, op == BinOp::Sub, line) {
            return r;
        }
    }
    match op {
        BinOp::Add => match (a, b) {
            (Int(x), Int(y)) => x.checked_add(y).map(Int).ok_or_else(overflow),
            (Str(x), Str(y)) => {
                let mut s = String::with_capacity(x.len() + y.len());
                s.push_str(&x);
                s.push_str(&y);
                Ok(Value::str(s))
            }
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
                return fail(line, format!("cannot compare {} with {}{}", a.type_name(), b.type_name(), compare_hint(&a, &b)));
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
            src_dir: Arc::from(Path::new("")),
            loaded: Arc::new(HashSet::new()),
            rust_whole: Arc::new(HashSet::new()),
            links: Default::default(),
            watched: Arc::new(HashSet::new()),
            via_links: Arc::new(HashSet::new()),
            shared,
            services: Default::default(),
            depth: 0,
            pipe_div: None,
            exact_div: None,
            cur_file: None,
            caller_file: None,
            fuel: u64::MAX,
            allowed: crate::policy::ceiling(),
            script: None,
            restrictor: None,
            route_limit: None,
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
            session_id: None,
            session_ends: None,
            visitor_read: false,
            locale_read: false,
            answering: false,
            locale: None,
            program: Arc::new(Vec::new()),
            client_js: Arc::new(String::new()),
            client_lazy: Default::default(),
            uses_atomic: false,
            bundles: Default::default(),
            styles: Default::default(),
            current_request: None,
            pending_request: None,
            asked: None,
            who: None,
            islands: Vec::new(),
            select_pending: None,
            select_want: None,
            island_scopes: Vec::new(),
            island_scopes_at: Vec::new(),
            in_svg: false,
            marked: (0, 0),
            state_again: None,
            in_component: false,
            pg_tx: std::sync::Mutex::new(Vec::new()),
            reads: None,
            read_request: false,
            in_run: false,
            acted: false,
            last_live: None,
            tx_depth: 0,
            pending: Vec::new(),
            pending_raw: Vec::new(),
            published: None,
            sqlite_tx: 0,
            catching_drops: 0,
            dropped: None,
            db: Arc::new(std::sync::Mutex::new(None)),
            db_conns: Arc::new(std::sync::Mutex::new(Vec::new())),
            db_on: None,
            wrote_req: None,
            prefetching: None,
            changed: None,
            db_resolving: Vec::new(),
            db_loop: None,
            base: String::new(),
            mounts: Vec::new(),
            remote_mounts: Vec::new(),
            remote_sources: Vec::new(),
            is_service: false,
            retired: Default::default(),
            own_seqs: Vec::new(),
            printing: false,
            stored: false,
            keep_last: true,
            code_id: NEXT_CODE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            text: None,
            text_small: None,
            in_job: false,
            stop: None,
            env_file: Default::default(),
            declared_services: Default::default(),
            accept: None,
            fn_callers: Default::default(),
            mcp: None,
            asking: None,
            in_tool: false,
            offered_tools: None,
            ready_checks: Default::default(),
            style_with: None,
            style_with_fn: None,
            types: Arc::new(Default::default()),
            js_libs: Arc::new(Default::default()),
            stream_out: None,
            pending_stream: None,
            streams_direct: false,
            rendered_island: false,
            island_names: Vec::new(),
            styles_used: Vec::new(),
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
            src_dir: self.src_dir.clone(),
            loaded: self.loaded.clone(),
            rust_whole: self.rust_whole.clone(),
            links: self.links.clone(),
            watched: self.watched.clone(),
            via_links: self.via_links.clone(),
            shared: self.shared.clone(),
            services: self.services.clone(),
            depth: 0,
            pipe_div: None,
            exact_div: None,
            cur_file: None,
            caller_file: None,
            fuel: u64::MAX,
            allowed: crate::policy::ceiling(),
            script: None,
            restrictor: None,
            route_limit: None,
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
            session_id: None,
            session_ends: None,
            visitor_read: false,
            locale_read: false,
            answering: false,
            locale: None,
            program: self.program.clone(),
            client_js: self.client_js.clone(),
            client_lazy: self.client_lazy.clone(),
            uses_atomic: self.uses_atomic,
            bundles: self.bundles.clone(),
            styles: self.styles.clone(),
            current_request: None,
            pending_request: None,
            asked: None,
            who: None,
            islands: Vec::new(),
            select_pending: None,
            select_want: None,
            island_scopes: Vec::new(),
            island_scopes_at: Vec::new(),
            in_svg: false,
            marked: (0, 0),
            state_again: None,
            in_component: false,
            pg_tx: std::sync::Mutex::new(Vec::new()),
            reads: None,
            read_request: false,
            in_run: false,
            acted: false,
            last_live: None,
            tx_depth: 0,
            pending: Vec::new(),
            pending_raw: Vec::new(),
            published: None,
            sqlite_tx: 0,
            catching_drops: 0,
            dropped: None,
            db: self.db.clone(),
            db_conns: self.db_conns.clone(),
            db_on: None,
            wrote_req: None,
            prefetching: None,
            changed: None,
            db_resolving: Vec::new(),
            db_loop: None,
            base: self.base.clone(),
            mounts: self.mounts.iter().map(|(p, m)| (p.clone(), m.fork())).collect(),
            remote_mounts: self.remote_mounts.clone(),
            remote_sources: self.remote_sources.clone(),
            is_service: self.is_service,
            retired: self.retired.clone(),
            own_seqs: Vec::new(),
            printing: false,
            stored: false,
            keep_last: true,
            code_id: self.code_id,
            text: None,
            text_small: None,
            in_job: self.in_job,
            stop: None,
            env_file: self.env_file.clone(),
            declared_services: self.declared_services.clone(),
            accept: self.accept.clone(),
            fn_callers: self.fn_callers.clone(),
            mcp: self.mcp.clone(),
            asking: None,
            in_tool: self.in_tool,
            offered_tools: self.offered_tools.clone(),
            ready_checks: self.ready_checks.clone(),
            style_with: self.style_with.clone(),
            style_with_fn: self.style_with_fn.clone(),
            types: self.types.clone(),
            js_libs: self.js_libs.clone(),
            stream_out: None,
            pending_stream: None,
            streams_direct: false,
            rendered_island: false,
            island_names: Vec::new(),
            styles_used: Vec::new(),
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
        self.loaded.iter().chain(self.watched.iter()).chain(self.via_links.iter()).cloned().collect()
    }

    /// A file reached through a symbolic link, of its own or of a folder on its way, is watched
    /// by that way too, so a link pointed elsewhere (`ln -sfn`, a `current` release) is seen
    /// (FEEDBACK31.md, Q-383; branch feedback27-35-fixes).
    /// What `use`, `mount` and `service` paths count from: the program's folder, or for a main
    /// file that is a link, the folder of the file it leads to, where they were looked for
    /// beside the link (FEEDBACK55.md, Q-817; branch feedback55). The policy, `.env` and data
    /// count from `base_dir`, as given.
    pub(crate) fn src_dir(&self) -> &Path {
        if self.src_dir.as_os_str().is_empty() {
            &self.base_dir
        } else {
            &self.src_dir
        }
    }

    fn note_link(&mut self, path: &Path) {
        let abs = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf()) };
        if abs.ancestors().any(|a| a.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink())) {
            Arc::make_mut(&mut self.via_links).insert(abs);
        }
    }

    /// Announces changes to `topics`, after the current transaction commits.
    pub fn publish(&mut self, topics: Vec<String>) {
        self.mark_impure();
        // Each program has its own topics (two mounted apps may both have
        // a `notes` table).
        let raw = topics.clone();
        let topics: Vec<String> = topics.into_iter().map(|t| self.scope_topic(t)).collect();
        if self.tx_depth > 0 {
            // Once each: 100,000 inserts in a transaction are one change to their table, not
            // 100,000 to announce when it commits. The two lists stay the same length, as a
            // rollback cuts both at one place.
            for (t, r) in topics.into_iter().zip(raw) {
                if !self.pending.iter().rev().take(64).any(|p| *p == t) {
                    self.pending.push(t);
                    self.pending_raw.push(r);
                }
            }
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
        self.locale_read = true;
        // A rule of the program's can read anything of the visitor's (FEEDBACK55.md, Q-886; branch
        // feedback55).
        if self.site.locale_rule.is_some() {
            self.visitor_read = true;
        }
        for t in &mut self.tracks {
            t.locale = true;
        }
    }

    /// The renders being cached did something that cannot be.
    pub(crate) fn mark_impure(&mut self) {
        for t in &mut self.tracks {
            t.impure = true;
        }
    }

    /// What a builtin called in a render being kept or a `live` run does to it (see
    /// `viewcache::Kind`).
    fn note_builtin(&mut self, name: &'static str) {
        use crate::viewcache::Kind;
        match crate::viewcache::kind_of(name) {
            Kind::Pure => {}
            Kind::Reads => self.mark_impure(),
            Kind::Acts => {
                self.mark_impure();
                self.acted = true;
            }
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
        // The thread's hold too: a service's code runs on an interpreter of its own, here
        // (FEEDBACK44.md, Q-579; found in review).
        if self.sqlite_tx > 0 || crate::httpd::held() {
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
            if let (Some(p), Some(req)) = (&self.pending_request, self.asked()) {
                let mut r = p.make(req);
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
        if let Some(flag) = &self.changed {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        match &self.prefetching {
            Some(flag) => {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                fail(line, format!("{}: {}", PREFETCH_REFUSED, what))
            }
            None => Ok(()),
        }
    }

    /// Whether what changes something matters to this request (see `no_writes`).
    #[inline]
    pub(crate) fn writes_noted(&self) -> bool {
        self.prefetching.is_some() || self.changed.is_some()
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
        // And in a tool's call that may ask its user: its questions come in the order its code
        // asks them, on this handle (FEEDBACK56.md, Part E; branch feedback56).
        if fs.len() < 2 || in_tx || self.reads.is_some() || !self.tracks.is_empty() || !self.islands.is_empty() || self.asking.is_some() {
            let mut out = Vec::with_capacity(fs.len());
            for f in &fs {
                out.push(self.call(f, Vec::new(), line)?);
            }
            return Ok(Value::list(out));
        }
        // In the server: each function as a coroutine on the event loop, so
        // their waits (queries, `http.*`) overlap without threads.
        let stop = Arc::new(Stop { now: Default::default(), up: self.stop.clone() });
        if crate::httpd::in_task() {
            let mut results: Vec<Option<(Result<Value, Ctrl>, Back)>> = (0..fs.len()).map(|_| None).collect();
            let base = results.as_mut_ptr() as usize;
            let children: Vec<Box<dyn FnOnce()>> = fs
                .into_iter()
                .enumerate()
                .map(|(i, f)| {
                    let mut it = self.task_fork();
                    it.stop = Some(stop.clone());
                    Box::new(move || {
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.call(&f, Vec::new(), line)))
                            .unwrap_or_else(|_| fail(line, "a function given to `parallel` failed unexpectedly"));
                        it.failed(&r);
                        let back = it.back();
                        // SAFETY: `results` is not touched (or moved) while
                        // the children run; the request waits for them.
                        unsafe { *(base as *mut Option<(Result<Value, Ctrl>, Back)>).add(i) = Some((r, back)) };
                    }) as Box<dyn FnOnce()>
                })
                .collect();
            crate::httpd::run_children(children);
            return self.parallel_results(results.into_iter().map(|r| r.unwrap_or_else(|| (fail(line, "a function given to `parallel` failed unexpectedly"), Back::default()))));
        }
        let tasks: Vec<(Interp, Value)> = fs
            .into_iter()
            .map(|f| {
                let mut it = self.task_fork();
                it.stop = Some(stop.clone());
                (it, f)
            })
            .collect();
        let shared = self.shared.clone();
        let results: Vec<(Result<Value, Ctrl>, Back)> = self.unlocked(|| {
            std::thread::scope(|s| {
                let running: Vec<_> = tasks
                    .into_iter()
                    .map(|(mut it, f)| {
                        let shared = &shared;
                        std::thread::Builder::new().name("quillo-parallel".into()).stack_size(8 << 20).spawn_scoped(s, move || {
                            shared.gil.acquire();
                            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.call(&f, Vec::new(), line)))
                                .unwrap_or_else(|_| fail(line, "a function given to `parallel` failed unexpectedly"));
                            it.failed(&r);
                            let back = it.back();
                            shared.gil.release();
                            (r, back)
                        })
                    })
                    .collect();
                running
                    .into_iter()
                    .map(|t| match t {
                        Ok(h) => h.join().unwrap_or_else(|_| (fail(line, "a function given to `parallel` failed unexpectedly"), Back::default())),
                        Err(e) => (fail(line, format!("`parallel` could not start a thread: {}", e)), Back::default()),
                    })
                    .collect()
            })
        });
        self.parallel_results(results.into_iter())
    }

    /// The functions' results in order, the first failure being the call's; what each did to the
    /// answer is the request's, in their order, as if they had run one after another
    /// (FEEDBACK55.md, Q-796; branch feedback55).
    /// A function stopped because another failed is not the failure (Q-804), unless that is all
    /// there is: a `parallel` around this one was stopped.
    fn parallel_results(&mut self, results: impl Iterator<Item = (Result<Value, Ctrl>, Back)>) -> Result<Value, Ctrl> {
        let mut out = Vec::with_capacity(results.size_hint().0);
        let (mut first, mut stopped): (Option<Ctrl>, Option<Ctrl>) = (None, None);
        for (r, back) in results {
            self.take_back(back);
            match r {
                Ok(v) => out.push(v),
                Err(Ctrl::Error(e)) if e.msg == PARALLEL_STOPPED => {
                    stopped.get_or_insert(Ctrl::Error(e));
                }
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        match first.or(stopped) {
            Some(e) => Err(e),
            None => Ok(Value::list(out)),
        }
    }

    /// A `parallel` function that failed stops the others (Q-804).
    fn failed(&self, r: &Result<Value, Ctrl>) {
        if let (Err(_), Some(s)) = (r, &self.stop) {
            s.now.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// What a `parallel` function did to the answer being made: the cookies it queued and the
    /// session it set, which a function's own handle on the program dropped (a logout with
    /// `end_session()` in one left the visitor signed in), and whether it read who the visitor
    /// is, which makes the answer theirs (FEEDBACK55.md, Q-796; branch feedback55).
    /// What it announced while answering another service's call goes back with that answer, and
    /// a write makes the request's reads go to the primary, after `parallel` as in it (found in
    /// review).
    fn back(&mut self) -> Back {
        Back {
            cookies: std::mem::take(&mut self.set_cookies),
            session: self.session.take(),
            visitor_read: self.visitor_read,
            locale_read: self.locale_read,
            read_request: self.read_request,
            acted: self.acted,
            published: self.published.take(),
            wrote_req: self.wrote_req,
            refused: self.asking.take(),
        }
    }

    fn take_back(&mut self, b: Back) {
        if self.asking.is_none() {
            self.asking = b.refused;
        }
        self.visitor_read |= b.visitor_read;
        self.locale_read |= b.locale_read;
        self.read_request |= b.read_request;
        self.acted |= b.acted;
        if let Some(p) = b.published {
            self.note_published(p);
        }
        if b.wrote_req.is_some() {
            self.wrote_req = b.wrote_req;
        }
        if !b.cookies.is_empty() || self.session.is_none() {
            crate::session::take_back(self, b.cookies, b.session);
        }
    }

    /// A handle for a `parallel` task: the program, and what the caller may
    /// do (effects, the request being handled).
    fn task_fork(&self) -> Interp {
        let mut it = self.fork();
        it.depth = self.depth;
        it.allowed = self.allowed;
        it.restrictor = self.restrictor.clone();
        it.in_component = self.in_component;
        it.current_request = self.current_request.clone().or_else(|| self.pending_request.as_ref().zip(self.asked()).map(|(p, req)| p.make(req)));
        it.in_run = self.in_run;
        it.tenant = self.tenant.clone();
        it.tenant_named = self.tenant_named;
        it.request_id = self.request_id.clone();
        it.prefetching = self.prefetching.clone();
        it.changed = self.changed.clone();
        it.upload_body = self.upload_body.clone();
        it.locale = self.locale.clone();
        // The session as the request has it now: one it set before is the one a function reads
        // (FEEDBACK55.md, Q-796; branch feedback55). What it announces for a calling service is
        // gathered as the request's is, and a write the request made before sends its reads to
        // the primary (found in review).
        it.session = self.session.clone();
        it.published = self.published.as_ref().map(|_| Vec::new());
        it.wrote_req = self.wrote_req;
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
        // Told by the program's own rule, from anything in the request, the page is the visitor's.
        // A run is kept by the tenant in force, whoever told it (see `run_scope`; FEEDBACK55.md,
        // Q-879; branch feedback55).
        self.visitor_read |= matches!(rule, crate::site::TenantRule::Function(_));
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
            let src_dir = self.src_dir().to_path_buf();
            let links = crate::modules::link(Path::new(file), stmts.clone(), &src_dir, &mut |_, full| {
                let src = crate::error::read_source(full).ok()?;
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
        let loading = crate::value::set_loading(true);
        let r = self.load_stmts(&stmts, &env, file, &mut last);
        crate::value::set_loading(loading);
        if debugging {
            crate::debug::leave(self);
        }
        r?;
        self.build_client().map_err(|e| e.in_file(file))?;
        Ok(last)
    }

    fn load_stmts(&mut self, stmts: &[Stmt], env: &Env, file: &str, last: &mut Value) -> Result<(), QuilloError> {
        // Top-level code is in its file, so an error it makes or drops names it (FEEDBACK27.md,
        // Q-332 and Q-333; branch feedback27-35-fixes).
        let was = self.cur_file.replace(Arc::from(file));
        let r = self.load_stmts_in(stmts, env, file, last);
        self.cur_file = was;
        r
    }

    fn load_stmts_in(&mut self, stmts: &[Stmt], env: &Env, file: &str, last: &mut Value) -> Result<(), QuilloError> {
        let n = stmts.len();
        for (i, s) in stmts.iter().enumerate() {
            // What gives a value no one takes: a loop in it makes no list. The last statement
            // gives the program's value, which only a program loaded from text is asked for
            // (FEEDBACK29.md, Q-356; branch feedback29-fixes).
            let r = match s {
                Stmt::Expr(e) if (i + 1 < n || !self.keep_last) && !crate::debug::on() => self.eval_unused(e, env),
                s => self.exec_stmt(s, env),
            };
            match r {
                Ok(v) => *last = v,
                Err(Ctrl::Error(e)) => return Err(e.in_file(file)),
                // An error that reaches the top has no one left to hand it to: the
                // program failed, and says so (FEEDBACK29.md, Q-350; branch
                // feedback29-fixes).
                Err(Ctrl::Return(Value::Err(e, _, at))) => {
                    let text = crate::value::told(|| e.display());
                    return Err(QuilloError::new(at.before(&text), 0).in_file(file));
                }
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
        let src = crate::error::read_source(path)
            .map_err(|e| QuilloError::new(format!("cannot read {}: {}", path.display(), e), 0))?;
        if let Ok(p) = path.canonicalize() {
            // Its private names' tags count from its folder, wherever it was started from
            // (FEEDBACK55.md, Q-807; branch feedback55).
            crate::privacy::set_root(p.parent().unwrap_or(Path::new("/")));
            // Known by identity too, as a second hard link to it is it (FEEDBACK55.md, Q-701;
            // branch feedback55).
            crate::builtins::protect(&p, crate::builtins::OWN_SOURCE);
            Arc::make_mut(&mut self.loaded).insert(p);
        }
        self.note_link(path);
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            self.src_dir = Arc::from(crate::modules::source_dir(path).as_path());
        } else {
            self.src_dir = Arc::from(Path::new(""));
        }
        // Nothing asks a program run from its file for its value.
        let keep = std::mem::replace(&mut self.keep_last, false);
        let r = self.load_source(&src, &path.display().to_string());
        self.keep_last = keep;
        r
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
            Stmt::Let(Pattern::Bind(n), e, _) => {
                let v = self.eval(e, env)?;
                define(env, *n, v);
                Ok(Value::Nil)
            }
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
            // `i += 1` on a local number, a loop's commonest statement, here and at once.
            Stmt::Assign(name, op @ (AssignOp::Add | AssignOp::Sub), e, line) if !self.writes_noted() => {
                let rhs = self.eval(e, env)?;
                if crate::value::bump_local(env, *name, &rhs, matches!(op, AssignOp::Sub)) {
                    return Ok(Value::Nil);
                }
                self.exec_assign(name, op, e, line, env, Some(rhs))
            }
            Stmt::Assign(name, op, e, line) => self.exec_assign(name, op, e, line, env, None),
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
            Stmt::Use(path, Some(names), line, options) if crate::ast::is_script(path) => {
                for (global, name) in names {
                    // A module's exports go by the program's name for them, in the
                    // runtime (branch library-hints).
                    let (page_name, export) = if options.module { (name.clone(), Some(global.clone())) } else { (global.clone(), None) };
                    // One name from two scripts: refused, as `check` refuses it, where the last
                    // was every file's (FEEDBACK55.md, Q-808; branch feedback55).
                    if let Some(l) = self.js_libs.get(name).filter(|l| l.src != *path || l.global != page_name || l.export != export) {
                        return fail(*line, format!("`{}` is taken from {} already, and here from {}: a page has one `{}`, so take one of them under another name (`use {} as ...`)", name, l.src, path, name, global));
                    }
                    let libs = Arc::make_mut(&mut self.js_libs);
                    let order = libs.get(name).map_or(libs.len(), |l| l.order);
                    libs.insert(name.clone(), crate::client::JsLib { global: page_name, src: path.clone(), order, hint: options.hint, export });
                    define(env, name, Value::Builtin("__jslib", crate::builtins::js_lib));
                }
                Ok(Value::Nil)
            }
            // Out of line: inlined, its lookups made every statement's frame save registers,
            // and a loop of `t += x` was 4% slower (found in review of FEEDBACK55.md, string
            // speed; branch feedback55).
            Stmt::Use(path, names, line, options) => self.run_use(path, names, line, options),
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
                    mcp: false,
                    component: false,
                    line,
                    body: Expr::Call(
                        Box::new(Expr::Ident("__record".into(), line)),
                        vec![Expr::Lit(Lit::Str(td.name.as_str().into())), Expr::Ident("fields".into(), line)],
                        line,
                    ),
                    nesting: 0,
                    file: None,
                    doc: None,
                    hot: Default::default(),
                    request_use: Default::default(),
                });
                define(env, &td.name, Value::Func(Arc::new(Closure { def, env: env.clone() })));
                Ok(Value::Nil)
            }
            Stmt::Service(name, prefix, file, options, line, mcp) => {
                if !Arc::ptr_eq(env, &self.globals) {
                    return fail(*line, "`service` must be at the top level");
                }
                // `service mcp crm = "https://..."`: kept for later, said as `quillo check` says it
                // (FEEDBACK56.md, Q-853; branch feedback56). `service mcp /billing` runs as
                // `service /billing` does.
                if *mcp && prefix.is_none() {
                    return fail(*line, &crate::service::mcp_calls_later(name));
                }
                let options = match options {
                    Some(e) => Some(self.eval(e, env)?),
                    None => None,
                };
                crate::service::declare(self, name, prefix.as_deref(), file, options.as_ref(), *line)?;
                // Its tools' calls are signed both ways (FEEDBACK56.md, Part F, security review).
                if let Some(how) = mcp.then(|| crate::service::unsigned_auth(name)).flatten() {
                    return fail(*line, &crate::service::tools_unsigned(name, how));
                }
                Ok(Value::Nil)
            }
            Stmt::Mount(prefix, file, line) => {
                if !Arc::ptr_eq(env, &self.globals) {
                    return fail(*line, "`mount` must be at the top level");
                }
                let full = self.src_dir().join(file);
                let dir = full.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| self.base_dir.to_path_buf());
                let mut sub = Interp::with_shared(dir, self.shared.clone());
                sub.services = self.services.clone();
                sub.base = format!("{}{}", self.base, prefix);
                // What a service mounts is part of the service.
                sub.is_service = self.is_service;
                sub.load_file(&full).map_err(|e| Ctrl::Error(Box::new(e.in_file(&full.display().to_string()))))?;
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
                let given = self.state_preset.as_ref().filter(|_| top).and_then(|m| m.get(name.as_str()).cloned());
                let again = || self.state_again.as_ref().filter(|(d, _)| *d == self.islands.len()).and_then(|(_, m)| m.get(name.as_str()).cloned());
                let v = match given.or_else(again) {
                    Some(v) => v,
                    None if top && self.state_preset.is_none() && defer_call(e, env) => {
                        self.island_defers.push(name.clone());
                        Value::Nil
                    }
                    None => self.eval(e, env)?,
                };
                let live = self.last_live.take();
                if let Some((state, subs)) = self.islands.last_mut() {
                    let (at, _) = state.insert_full(name.into(), v.clone());
                    if let Some(l) = live {
                        subs.insert(name.into(), l);
                    }
                    self.island_scopes.push((at, env.clone(), None));
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

    /// `use`: a file's statements run, or a Rust file built and its functions defined, once
    /// (see `run_stmt`).
    #[cold]
    #[inline(never)]
    fn run_use(&mut self, path: &str, names: &Option<Vec<(String, String)>>, line: &u32, options: &LibOptions) -> Result<Value, Ctrl> {
        let src_dir = self.src_dir().to_path_buf();
        let from = self.cur_file.as_deref().map_or_else(|| src_dir.join("main.qwe"), PathBuf::from);
        // A path without its extension that two files could be: said, not one taken
        // (FEEDBACK55.md, Q-819; branch feedback55).
        if options.implied {
            let could = crate::modules::implied_files(&src_dir, &from, path);
            if could.len() > 1 {
                return fail(*line, crate::modules::two_files(&could));
            }
        }
        let path = &crate::modules::resolve_use(&src_dir, &from, path, options.implied);
        let full = src_dir.join(path);
        let key = full.canonicalize().unwrap_or_else(|_| full.clone());
        self.note_link(&full);
        // The file's names for a Rust file's functions, its own (FEEDBACK55.md, Q-816;
        // branch feedback55).
        let links = self.links.clone();
        let renames = if crate::native::is_rust(path) { links.rust_names(&crate::modules::key(&from)) } else { None };
        // A `.qwer` file's names are defined for each `use` that takes some, or that
        // calls them its own way, or the first that takes them all, after one that took
        // some (its library is built and opened once); anything else runs once. A file
        // taken whole after another took names of it had none (Q-816).
        let first = Arc::make_mut(&mut self.loaded).insert(key.clone());
        if crate::native::is_rust(path) {
            if names.is_none() && renames.is_none() && !Arc::make_mut(&mut self.rust_whole).insert(key.clone()) {
                return Ok(Value::Nil);
            }
        } else if !first {
            return Ok(Value::Nil);
        }
        // By identity too (FEEDBACK55.md, Q-701; branch feedback55).
        crate::builtins::protect(&key, crate::builtins::OWN_SOURCE);
        // Rust: built, loaded, and its functions defined (see `native`).
        if crate::native::is_rust(&path) {
            let fname = full.display().to_string();
            return match crate::native::use_file(self, &full, names.as_deref(), renames, *line) {
                Err(Ctrl::Error(e)) => Err(Ctrl::Error(Box::new(e.in_file(&fname)))),
                other => other.map(|_| Value::Nil),
            };
        }
        let src = crate::error::read_source(&full)
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
        // Its statements are in its file: a `use ./x.qwe` in it counts from there
        // (FEEDBACK25.md, Q-325; branch feedback27-35-fixes).
        let was_file = self.cur_file.replace(Arc::from(fname.as_str()));
        for s in stmts.iter() {
            // A used file's statements give nothing to anyone.
            let ran = match s {
                Stmt::Expr(node @ Expr::For(pat, iter, body, line)) if !crate::debug::on() => self.eval_for(node, pat, iter, body, &g, *line, false),
                s => self.exec_stmt(s, &g),
            };
            match ran {
                Ok(_) => {}
                // An error is the program's failure from a used file too, and
                // the file it came from says so (FEEDBACK29.md, Q-350; branch
                // feedback29-fixes).
                Err(Ctrl::Return(Value::Err(e, _, at))) => {
                    let text = crate::value::told(|| e.display());
                    r = Err(Ctrl::Error(Box::new(QuilloError::new(at.before(&text), 0).in_file(&fname))));
                    break;
                }
                // `return` at a file's top level leaves that file, not the one
                // that used it (FEEDBACK16.md, Q-109; branch feedback20-fixes).
                Err(Ctrl::Return(_)) => break,
                Err(e) => {
                    r = Err(match e {
                        Ctrl::Error(e) => Ctrl::Error(Box::new(e.in_file(&fname))),
                        other => other,
                    });
                    break;
                }
            }
        }
        self.cur_file = was_file;
        if debugging {
            crate::debug::leave(self);
        }
        r
    }

    /// `name = e`, `+=` and `-=`: out of `run_stmt`, whose frame every statement pays for and
    /// deeply nested code stacks (FEEDBACK29.md; branch feedback29-fixes).
    #[inline(never)]
    fn exec_assign(&mut self, name: &Sym, op: &AssignOp, e: &Expr, line: &u32, env: &Env, given: Option<Value>) -> Result<Value, Ctrl> {
        if self.writes_noted() && lookup_global(env, name).map_or(false, |(_, global)| global) {
            self.no_writes(&format!("assigning `{}`", name), *line)?;
        }
        let name = *name;
        let v = match op {
            AssignOp::Set => {
                // `x = x + e` as `x += e`: a number in place, a list or text grown where
                // it is. `e` first, as `+=` has it, when that cannot change `x`.
                let grown = match e {
                    Expr::Binary(bop @ (BinOp::Add | BinOp::Sub), a, b, _) if self.fuel == u64::MAX && matches!(&**a, Expr::Ident(n, _) if *n == name) => {
                        let sub = *bop == BinOp::Sub;
                        let list_or_text = !sub && crate::value::growable(env, name);
                        if list_or_text || quiet(b) {
                            let more = self.eval(b, env)?;
                            if crate::value::bump_local(env, name, &more, sub) {
                                return Ok(Value::Nil);
                            }
                            if list_or_text {
                                match self.grown(name, more, env, *line)? {
                                    Some(v) => Some(v),
                                    None => return Ok(Value::Nil),
                                }
                            } else {
                                let old = lookup(env, name).ok_or_else(|| undefined(&name, *line))?;
                                Some(binop(*bop, old, more, *line)?)
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                match grown {
                    Some(v) => v,
                    None => match self.in_place_call(Some(name), e, env)? {
                        Some(_) if std::mem::take(&mut self.stored) => return Ok(Value::Nil),
                        Some(v) => v,
                        None => self.eval(e, env)?,
                    },
                }
            }
            AssignOp::Add | AssignOp::Sub => {
                // `run_stmt` may have worked it out already.
                let rhs = match given {
                    Some(v) => v,
                    None => self.eval(e, env)?,
                };
                let sub = matches!(op, AssignOp::Sub);
                if crate::value::bump_local(env, name, &rhs, sub) {
                    return Ok(Value::Nil);
                }
                if !sub && self.fuel == u64::MAX && crate::value::growable(env, name) {
                    match self.grown(name, rhs, env, *line)? {
                        Some(v) => v,
                        None => return Ok(Value::Nil),
                    }
                } else {
                    let old = lookup(env, name).ok_or_else(|| undefined(&name, *line))?;
                    // A script's limit on what it makes holds here as for `+`: `a += a`
                    // went past it, as far as memory goes (FEEDBACK29.md; branch
                    // feedback29-fixes).
                    let op = if sub { BinOp::Sub } else { BinOp::Add };
                    if self.fuel != u64::MAX {
                        self.script_binop(op, old, rhs, *line)?
                    } else {
                        binop(op, old, rhs, *line)?
                    }
                }
            }
        };
        // A local has nothing following it, and nor has a global while the program
        // loads: a statement should not cost five looks at the same name
        // (FEEDBACK29.md; branch feedback29-fixes).
        let v = match crate::value::assign_local(env, name, v) {
            Ok(()) => return Ok(Value::Nil),
            Err(v) => v,
        };
        if crate::value::loading() && assign(env, name, v.clone()) {
            return Ok(Value::Nil);
        }
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
                    // A change a tool's question must come before (FEEDBACK56.md, Part E; branch
                    // feedback56).
                    if self.asking.is_some() {
                        crate::mcp::assigned(self, name.as_str());
                    }
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
                lookup(env, *n).ok_or_else(|| undefined(n, *line))
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
                if let (Value::Int(p), Value::Int(q)) = (&x, &y) {
                    if let Some(v) = int_op(*op, *p, *q) {
                        // Ints have nothing to let go of.
                        std::mem::forget((x, y));
                        return Ok(v);
                    }
                }
                if self.fuel != u64::MAX {
                    return self.script_binop(*op, x, y, *line);
                }
                binop(*op, x, y, *line)
            }
            Expr::Coalesce(a, b) => {
                // What the left side could not work out is a value to guard against
                // too: bad input stops a decimal's `/`, `date.of` or a pattern, and
                // there is no `try` (FEEDBACK25.md, Q-318; branch feedback26-fixes).
                // Nor any error once a tool's call stopped at a question or refused one: what
                // follows the `??` would act on a yes nobody gave (FEEDBACK56.md, Part E, found in
                // review; branch feedback56).
                let v = match self.eval(a, env) {
                    Err(Ctrl::Error(e)) if guardable(&e.msg) && !self.asking.as_ref().is_some_and(|a| a.stopped()) => Value::Nil,
                    other => other?,
                };
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
            Expr::Block(stmts) => self.eval_block(stmts, env, true),
            Expr::Lambda(def) => Ok(Value::Func(Arc::new(Closure { def: def.clone(), env: env.clone() }))),
            Expr::Template(parts) => self.eval_template(parts, env),
            Expr::List(items) => self.eval_list(items, env),
            Expr::Map(entries) => self.eval_map(entries, env),
            Expr::Unary(op, x, line) => self.eval_unary(*op, x, env, *line),
            Expr::Index(obj, idx, line) => self.eval_index(obj, idx, env, *line),
            Expr::Match(subj, arms, line) => self.eval_match(subj, arms, env, *line),
            Expr::For(pat, iter, body, line) => self.eval_for(e, pat, iter, body, env, *line, true),
            Expr::While(cond, body, line) => {
                // A body that declares has one scope for all its rounds, unless one of them
                // leaves something holding it, as a `for` has.
                let stmts = match &**body {
                    Expr::Block(stmts) if !stmts.iter().all(|s| matches!(s, Stmt::Expr(_) | Stmt::Assign(..) | Stmt::Return(..))) => Some(stmts),
                    _ => None,
                };
                let mut scope: Option<Env> = None;
                let mut rounds = 0u64;
                loop {
                    // A long loop goes on in machine code from here (src/jitloop.rs).
                    #[cfg(feature = "jit")]
                    if rounds == crate::jitloop::WARM && self.loops_compile() {
                        if let Some(s) = scope.take() {
                            Scope::done(s);
                        }
                        match crate::jitloop::run(self, e, env, None) {
                            Some(crate::jitloop::Ran::Done) => break,
                            Some(crate::jitloop::Ran::Failed(err)) => return Err(err),
                            Some(crate::jitloop::Ran::Resume(_)) | None => {}
                        }
                    }
                    rounds += 1;
                    if !self.eval(cond, env)?.truthy() {
                        break;
                    }
                    self.within_limit(*line)?;
                    if self.fuel != u64::MAX {
                        self.burn(*line)?;
                    }
                    match stmts {
                        Some(stmts) => {
                            if !scope.as_mut().map_or(false, Scope::reuse) {
                                scope = Some(Scope::child(env));
                            }
                            self.block_in(stmts, scope.as_ref().expect("set above"), false)?;
                        }
                        None => {
                            self.eval_unused(body, env)?;
                        }
                    }
                }
                if let Some(s) = scope {
                    Scope::done(s);
                }
                Ok(Value::Nil)
            }
            Expr::Reply(status, body, line) => self.eval_reply(status, body.as_deref(), env, *line),
            Expr::Html(el, _) => self.eval_html(el, env),
        }
    }

    #[inline(never)]
    fn eval_pipe(&mut self, lhs: &Expr, rhs: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        // `a / b | floor` of two ints, in ints, as `floor(a / b)` is, however the function is
        // reached (`m.floor`, `[floor][0]`, `floor()`): the quotient goes on as the float it is,
        // and `floor` given that float alone, here, makes it from the ints (FEEDBACK55.md,
        // Q-788; found in review).
        let v = match lhs {
            Expr::Binary(BinOp::Div, a, b, at) => {
                let x = self.eval(a, env)?;
                let y = self.eval(b, env)?;
                let ints = match (&x, &y) {
                    (Value::Int(p), Value::Int(q)) if *q != 0 && p.checked_rem(*q) != Some(0) => Some((*p, *q)),
                    _ => None,
                };
                let v = if self.fuel != u64::MAX { self.script_binop(BinOp::Div, x, y, *at)? } else { binop(BinOp::Div, x, y, *at)? };
                // Boxed: the interpreter carries a pointer's room for it, not its own (a call does
                // not notice either).
                let pending = match (ints, &v) {
                    (Some((p, q)), Value::Float(f)) => Some(Box::new(ExactDiv { bits: f.to_bits(), a: p, b: q, line: *at })),
                    _ => None,
                };
                return self.pipe_into(v, rhs, env, line, pending);
            }
            _ => self.eval(lhs, env)?,
        };
        self.pipe_into(v, rhs, env, line, None)
    }

    /// `v | rhs`. `pending`, a quotient of two ints `v` is, goes to the call the pipe makes with
    /// `v` alone, and to no other.
    fn pipe_into(&mut self, v: Value, rhs: &Expr, env: &Env, line: u32, pending: Option<Box<ExactDiv>>) -> Result<Value, Ctrl> {
        match rhs {
            Expr::Call(callee, args, l) => {
                if args.is_empty() {
                    self.pipe_div = pending;
                }
                let r = self.eval_call(callee, args, Some(v), env, *l);
                self.pipe_div = None;
                r
            }
            // `sql | analytics.one`: on that connection.
            Expr::Field(obj, name, l) => {
                let recv = self.eval(obj, env)?;
                let db_on = crate::db::conn_of(&recv, name);
                let f = self.get_field(recv, name, env, *l)?;
                self.db_on = db_on;
                self.exact_div = pending;
                self.piped_site(rhs);
                self.call(&f, vec![v], line)
            }
            other => {
                let f = self.eval(other, env)?;
                self.exact_div = pending;
                self.piped_site(rhs);
                self.call(&f, vec![v], line)
            }
        }
    }

    /// `x | confirm`: where a tool's question is, this pipe's right side, as a call's is its
    /// callee (`eval_call`; FEEDBACK56.md, Part E, second review; branch feedback56).
    #[inline]
    fn piped_site(&mut self, rhs: &Expr) {
        if let Some(a) = self.asking.as_mut() {
            a.site = rhs as *const Expr as usize;
        }
    }

    /// A block, and its value if `keep`. One frame for each block nested, as deeply nested
    /// source is run on the stack `stack::for_nesting` makes room for.
    #[inline(never)]
    fn eval_block(&mut self, stmts: &[Stmt], env: &Env, keep: bool) -> Result<Value, Ctrl> {
        // One statement that declares nothing, as `if c { t += 1 }` has: that statement.
        if let ([s @ (Stmt::Expr(_) | Stmt::Assign(..))], false) = (stmts, crate::debug::on()) {
            return match s {
                Stmt::Expr(e) if keep => self.eval(e, env),
                Stmt::Expr(e) => self.eval_unused(e, env),
                s => self.run_stmt(s, env),
            };
        }
        // A block that declares nothing runs in the scope around it: the
        // same names mean the same, with no scope to make (a loop's body).
        if stmts.iter().all(|s| matches!(s, Stmt::Expr(_) | Stmt::Assign(..) | Stmt::Return(..))) {
            return self.block_in(stmts, env, keep);
        }
        let scope = Scope::child(env);
        let last = self.block_in(stmts, &scope, keep)?;
        Scope::done(scope);
        Ok(last)
    }

    /// `e` for what it does, its value not used: a loop in it, in an `if`'s branch or a
    /// block, makes no list of what each round gave, and so may go to machine code
    /// (FEEDBACK29.md; branch feedback29-fixes).
    fn eval_unused(&mut self, e: &Expr, env: &Env) -> Result<Value, Ctrl> {
        match e {
            Expr::For(pat, iter, body, line) => self.eval_for(e, pat, iter, body, env, *line, false),
            Expr::If(c, a, b) => {
                if self.eval(c, env)?.truthy() {
                    self.eval_unused(a, env)?;
                } else if let Some(b) = b {
                    self.eval_unused(b, env)?;
                }
                Ok(Value::Nil)
            }
            Expr::Block(stmts) => self.eval_block(stmts, env, false),
            e => {
                let v = self.eval(e, env)?;
                if let Value::Err(err, ..) = &v {
                    // A `db.open` that failed as the program starts, its error taken by nothing
                    // (alone on a line, or given back by a function called so): the program
                    // stops, where it ran on a database in memory, answered writes, and lost
                    // them at the next start (FEEDBACK49.md, Q-602; branch main). Taken (`let r
                    // = ...`, `| is_err`, `??`), it is a value, as before.
                    if crate::value::loading() && crate::db::open_failed(err) {
                        let why = crate::value::told(|| err.display());
                        return Err(Ctrl::Error(Box::new(QuilloError::new(
                            format!("db.open: {}: the program's database cannot be opened, so the program does not start (take the error, `let r = db.open(...)`, to go on without it)", why.trim_start_matches("db: ")),
                            crate::debug::expr_line(e),
                        ))));
                    }
                    self.dropped_error(&v, crate::debug::expr_line(e));
                }
                Ok(v)
            }
        }
    }

    /// An error a statement gave and no one took: inside a transaction, it rolls that back and
    /// is its value; anywhere else it goes to the log, with its place and the request, so a
    /// failed write is not silent (FEEDBACK27.md, Q-332; branch feedback27-35-fixes). `let _ =`
    /// takes a value, so it is the way to say an error is not wanted.
    #[cold]
    #[inline(never)]
    fn dropped_error(&mut self, v: &Value, line: u32) {
        if self.catching_drops > 0 {
            if self.dropped.is_none() {
                self.dropped = Some(v.clone());
            }
            return;
        }
        if let Value::Err(e, _, at) = v {
            let text = crate::value::told(|| e.display());
            let here = Place::new(self.cur_file.as_ref(), line);
            let made = if at.to_string().is_empty() || at.to_string() == here.to_string() { String::new() } else { format!(", made at {}", at) };
            crate::httpd::error_line(&here.before(&format!("an error no statement took{}: {} (pass it on with `?`, give a value with `??`, or take it with `let _ =`)", made, text)));
        }
    }

    #[inline]
    /// The statements of a block in `scope`, and the last one's value, if `keep`: a loop's
    /// body has none that is used, unless its loop's list is (FEEDBACK29.md, Q-356; branch
    /// feedback29-fixes).
    fn block_in(&mut self, stmts: &[Stmt], scope: &Env, keep: bool) -> Result<Value, Ctrl> {
        let mut last = Value::Nil;
        let n = stmts.len();
        for (i, s) in stmts.iter().enumerate() {
            last = match s {
                // What nobody takes the value of: a loop in it makes no list.
                Stmt::Expr(e) if (i + 1 < n || !keep) && !crate::debug::on() => self.eval_unused(e, scope)?,
                // The block's value, with no statement's frame between: a block in a block
                // nests one frame deeper, not three.
                Stmt::Expr(e) if !crate::debug::on() => self.eval(e, scope)?,
                s => self.exec_stmt(s, scope)?,
            };
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

    /// Whether a loop may go on in machine code: nothing watches what it does there.
    #[cfg(feature = "jit")]
    fn loops_compile(&self) -> bool {
        self.fuel == u64::MAX && self.reads.is_none() && self.tracks.is_empty() && !crate::debug::on() && !crate::httpd::compute_limited() && crate::jit::enabled()
    }

    /// A step of a loop or a call: `--max-compute` ends a request here, between statements,
    /// so nothing is left half done (FEEDBACK20.md, Q-163; branch feedback20-fixes). Always in
    /// place: with the stop's branch it was no longer inlined, and calls took 4% longer (found
    /// in review of FEEDBACK55.md, Q-804; branch feedback55).
    #[inline(always)]
    fn within_limit(&mut self, line: u32) -> Result<(), Ctrl> {
        if crate::httpd::OVER_TIME.load(std::sync::atomic::Ordering::Relaxed) && crate::httpd::over_time() {
            // Naming the policy file when its limit is the one (FEEDBACK55.md, Q-849; branch
            // feedback55).
            return fail(line, crate::httpd::over_compute());
        }
        // Another function of its `parallel` failed (FEEDBACK55.md, Q-804; branch feedback55).
        if self.stop.is_some() {
            return self.stopped_here(line);
        }
        Ok(())
    }

    /// In a `parallel` function: stopped, once another has failed (see `Stop`). Out of the way of
    /// every other step, which a check in place slowed by a tenth.
    #[cold]
    #[inline(never)]
    fn stopped_here(&self, line: u32) -> Result<(), Ctrl> {
        match &self.stop {
            Some(s) if s.stopped() => fail(line, PARALLEL_STOPPED),
            _ => Ok(()),
        }
    }

    /// `s[n]`: a character, from the end when negative. A long text is indexed once
    /// and kept, so a loop over it is not quadratic (FEEDBACK29.md, Q-355; branch
    /// feedback29-fixes).
    fn char_at(&mut self, s: &Arc<str>, n: i64) -> Value {
        let (chars, offsets) = self.text_index(s);
        let n = if n < 0 { chars as i64 + n } else { n };
        if n < 0 || n >= chars as i64 {
            return Value::Nil;
        }
        let n = n as usize;
        match offsets {
            None => one_char(s.as_bytes()[n] as char),
            Some(o) => {
                let at = o[n] as usize;
                one_char(s[at..].chars().next().expect("the offset of a character"))
            }
        }
    }

    /// How many characters `s` has, and where each is unless each is a byte.
    fn text_index(&mut self, s: &Arc<str>) -> (usize, Option<&[u32]>) {
        const SMALL: usize = TEXT_SMALL;
        let kept = matches!(&self.text, Some(t) if Arc::ptr_eq(&t.text, s));
        if !kept {
            let plain = s.is_ascii();
            if s.len() < SMALL {
                // Too short to be worth keeping: looked at each time.
                let offsets = (!plain).then(|| s.char_indices().map(|(i, _)| i as u32).collect::<Vec<u32>>());
                self.text_small = offsets;
                let chars = self.text_small.as_ref().map_or(s.len(), Vec::len);
                return (chars, self.text_small.as_deref());
            }
            let offsets = (!plain && s.len() <= u32::MAX as usize).then(|| s.char_indices().map(|(i, _)| i as u32).collect::<Vec<u32>>());
            let chars = offsets.as_ref().map_or(s.len(), Vec::len);
            self.text = Some(TextIndex { text: s.clone(), chars, offsets });
        }
        let t = self.text.as_ref().expect("kept above");
        (t.chars, t.offsets.as_deref())
    }

    /// `len(s)`, with what `s[i]` keeps: `while i < len(s)` asks each round.
    pub(crate) fn text_len(&mut self, s: &Arc<str>) -> usize {
        self.text_index(s).0
    }

    /// What `text_index` knows of `s`, for a compiled loop that reads it or adds to it
    /// (src/jitloop.rs): what was kept of it is taken, as holding it would make the loop's
    /// first `+=` a copy; worked out when nothing was and `want` says to (FEEDBACK55.md,
    /// string speed; branch feedback55).
    pub(crate) fn text_index_take(&mut self, s: &Arc<str>, want: bool) -> Option<(usize, Option<Vec<u32>>)> {
        if let Some(t) = self.text.take_if(|t| Arc::ptr_eq(&t.text, s)) {
            return Some((t.chars, t.offsets));
        }
        if !want {
            None
        } else if s.is_ascii() {
            Some((s.len(), None))
        } else {
            let o = s.char_indices().map(|(i, _)| i as u32).collect::<Vec<u32>>();
            Some((o.len(), Some(o)))
        }
    }

    /// What `text_index_take` took, back after the loop, so the interpreter reading on does not
    /// work it out again.
    pub(crate) fn text_index_back(&mut self, s: Arc<str>, chars: usize, offsets: Option<Vec<u32>>) {
        if s.len() >= TEXT_SMALL {
            self.text = Some(TextIndex { text: s, chars, offsets });
        }
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
                    n.checked_neg().map(Value::Int).ok_or_else(|| Ctrl::Error(Box::new(QuilloError::new("integer overflow", line))))
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
    /// `xs += ys` and `xs = xs + ys` on a list, which grow it as `push` does (FEEDBACK29.md,
    /// Q-352; branch feedback29-fixes), and `s += t` on a text, added to where it is when
    /// nothing else holds it (Q-354): None when that is done, or else the value to assign,
    /// as `+` has it.
    fn grown(&mut self, name: Sym, more: Value, env: &Env, line: u32) -> Result<Option<Value>, Ctrl> {
        if let Value::Str(t) = &more {
            if self.text_added(name, t, env) {
                return Ok(None);
            }
        }
        let mut more = Some(more);
        let done = crate::value::with_slot(env, name, |slot| match (slot, more.as_ref()) {
            (Value::List(l), Some(Value::List(_))) => {
                let Some(Value::List(items)) = more.take() else { unreachable!("looked at above") };
                let l = Arc::make_mut(l);
                match Arc::try_unwrap(items) {
                    Ok(items) => l.extend(items),
                    Err(shared) => l.extend_from_slice(&shared),
                }
                true
            }
            _ => false,
        });
        if done == Some(true) {
            return Ok(None);
        }
        let old = lookup(env, name).ok_or_else(|| undefined(&name, line))?;
        binop(BinOp::Add, old, more.unwrap_or(Value::Nil), line).map(Some)
    }

    /// `t` added to the text a variable holds where it is, if nothing else holds it. What
    /// `s[i]` keeps of it stays right, so `while len(s) < n { s += "x" }` stays linear.
    fn text_added(&mut self, name: Sym, t: &str, env: &Env) -> bool {
        let mut kept = self.text.take();
        let mut index = None;
        let added = crate::value::with_slot(env, name, |slot| {
            let Value::Str(s) = slot else { return false };
            let mut own = kept.take_if(|k| Arc::ptr_eq(&k.text, s));
            if let Some(k) = &mut own {
                k.text = Arc::from("");
            }
            if !crate::value::append_text(s, t) {
                return false;
            }
            // Letters of one byte each so far, by no list of where they start: the first
            // that is longer starts one.
            if let Some(mut k) = own.filter(|k| k.offsets.is_some() || t.is_ascii()) {
                let start = s.len() - t.len();
                k.chars += t.chars().count();
                if let Some(o) = &mut k.offsets {
                    o.extend(t.char_indices().map(|(i, _)| (start + i) as u32));
                }
                k.text = s.clone();
                index = Some(k);
            }
            true
        });
        self.text = index.or(kept);
        added == Some(true)
    }

    /// The same for a call that is a function's whole body, on one of its
    /// own parameters, as in `reduce(xs, {}, (m, x) => set(m, x.id, x))`
    /// (`name` is None: the caller has checked it is a parameter).
    fn in_place_call(&mut self, name: Option<Sym>, e: &Expr, env: &Env) -> Result<Option<Value>, Ctrl> {
        // The same call, however it is written: `set(m, k, v)`, `m | set(k, v)` and
        // `m.set(k, v)` (FEEDBACK29.md, Q-352; branch feedback29-fixes).
        let Some((fname, Expr::Ident(first, _), args, line)) = call_on(e) else { return Ok(None) };
        let first = *first;
        let (set, push, merge, remove) = update_names();
        if name.is_some_and(|n| n != first) || !(fname == set || fname == push || fname == merge || fname == remove) {
            return Ok(None);
        }
        let stored = name.is_some();
        let name = first;
        // `m.set(...)` is a function a map holds, if it does.
        if matches!(e, Expr::Call(f, ..) if matches!(&**f, Expr::Field(..))) && !matches!(lookup(env, name), Some(Value::List(_))) && !matches!(lookup(env, name), Some(Value::Map(m)) if !m.contains_key(fname.as_str())) {
            return Ok(None);
        }
        // The builtin of that name, unless something nearer has it; no copy of it is made
        // to ask.
        let Some((bname, bf)) = crate::value::builtin_named(env, fname) else { return Ok(None) };
        if bname != fname.as_str() {
            return Ok(None);
        }
        let b = Value::Builtin(bname, bf);
        // The other arguments are worked out first, so they may read it: what they
        // keep of it holds it, and it is then copied (FEEDBACK25.md, Q-314; branch
        // feedback26-fixes). A global too while nothing is being served, as nothing
        // else can read it: a script's top level is where most of it is (FEEDBACK29.md,
        // Q-353; branch feedback29-fixes).
        if !crate::value::takable(env, name) {
            return Ok(None);
        }
        if fname == set && self.fuel == u64::MAX {
            if let [key, Expr::Binary(bop @ (BinOp::Add | BinOp::Sub | BinOp::Mul), read, more, line)] = args {
                if let Some(v) = self.in_place_counted(name, key, *bop, read, more, *line, env, stored)? {
                    return Ok(Some(v));
                }
            } else if let Some(v) = self.in_place_nested(name, args, env)? {
                return Ok(Some(v));
            }
        }
        // Where it is, with no call to make, when that is all it takes.
        if self.fuel == u64::MAX && (fname == push && args.len() == 1 || fname == set && args.len() == 2) {
            let mut few = [Value::Nil, Value::Nil, Value::Nil];
            for (i, a) in args.iter().enumerate() {
                few[i + 1] = self.eval(a, env)?;
            }
            let few = &mut few[..=args.len()];
            let done = crate::value::with_slot(env, name, |slot| crate::value::update(slot, few));
            if done == Some(true) {
                // An assignment has nothing left to do.
                self.stored = stored;
                return Ok(if stored { Some(Value::Nil) } else { take_local(env, name) });
            }
            // What `update` does not change where it is, `set` or `push` refuses, or has
            // nothing to copy (null, an error for a key): the builtin is given the value, and the
            // variable keeps it, as the long way keeps it. Taken, a refusal caught by `??` left
            // the variable null (found in review of FEEDBACK55.md, string speed; branch
            // feedback55). The arguments are not worked out again.
            let Some(old) = lookup(env, name) else { return Ok(None) };
            few[0] = old;
            return self.call(&b, few.iter_mut().map(|v| std::mem::replace(v, Value::Nil)).collect(), line).map(Some);
        }
        let mut vals = Vec::with_capacity(args.len() + 1);
        vals.push(Value::Nil);
        for a in args {
            vals.push(self.eval(a, env)?);
        }
        // Taken, to be changed where it is, only where the builtin cannot refuse it (a map
        // merged with a map, a key removed from a list or a map); anything else is given the
        // value, which the variable keeps if the call fails (the same review).
        let sure = self.fuel == u64::MAX
            && vals.len() == 2
            && crate::value::with_var(env, name, |v| {
                if fname == merge {
                    matches!(v, Value::Map(_) | Value::Nil) && matches!(vals[1], Value::Map(_) | Value::Nil)
                } else {
                    fname == remove && matches!(v, Value::List(_) | Value::Map(_) | Value::Nil)
                }
            }) == Some(true);
        let Some(old) = (if sure { take_local(env, name) } else { lookup(env, name) }) else { return Ok(None) };
        vals[0] = old;
        self.call(&b, vals, line).map(Some)
    }

    /// `m = set(m, k, get(m, k, []) | push(x))` and `g = set(g, y, set(g[y], x, v))`, which
    /// is what a list of lists or a map of lists is written with: what is in `m` is read,
    /// changed and put back, and read out of `m` it has two owners, so each round copied
    /// all of it (FEEDBACK29.md, Q-352; branch feedback29-fixes). Here it is changed where
    /// it is. The value, or None to do it the long way: anything that could fail or
    /// surprise is left to that, which is why all that is worked out here has no effect
    /// and may be worked out again.
    fn in_place_nested(&mut self, name: Sym, args: &[Expr], env: &Env) -> Result<Option<Value>, Ctrl> {
        let [key, inner] = args else { return Ok(None) };
        let Some((op, recv, rest, _)) = call_on(inner) else { return Ok(None) };
        let fits = match (op.as_str(), rest.len()) {
            ("push", 1) | ("set", 2) => true,
            _ => false,
        };
        if !fits || !pure(key, env) || !rest.iter().all(|x| pure(x, env)) {
            return Ok(None);
        }
        // What was read: `m[k]`, or `get(m, k)` with what to start from.
        let (same_key, start) = match recv {
            Expr::Index(o, k, _) if matches!(&**o, Expr::Ident(n, _) if *n == name) => (&**k, None),
            r => match call_on(r) {
                Some((f, Expr::Ident(n, _), [k, more @ ..], _)) if f == "get" && *n == name && more.len() < 2 && more.iter().all(constant) => {
                    if !matches!(lookup(env, f), Some(Value::Builtin("get", _))) {
                        return Ok(None);
                    }
                    (k, more.first())
                }
                _ => return Ok(None),
            },
        };
        if !same_expr(key, same_key) || !matches!(lookup(env, op), Some(Value::Builtin(b, _)) if b == op.as_str()) {
            return Ok(None);
        }
        let key = self.eval(key, env)?;
        let start = match start {
            Some(d) => Some(self.eval(d, env)?),
            None => None,
        };
        let mut vals = Vec::with_capacity(2);
        for a in rest {
            vals.push(self.eval(a, env)?);
        }
        let done = crate::value::with_slot(env, name, |whole| crate::value::update_inside(whole, &key, start, vals));
        Ok(if done == Some(true) { take_local(env, name) } else { None })
    }

    /// `m = set(m, k, get(m, k, d) + e)` (or `-`, `*`), a count kept in a map, as word
    /// frequencies and histograms are written: the key is looked for once, where `get` and
    /// then `set` looked for it twice, and with no call made (FEEDBACK55.md, string speed; branch
    /// feedback55). All that can send it the long way is asked before anything is worked out;
    /// then the key, what to start from and `e` are each worked out once, in the long way's
    /// order (the key is pure, so `get`'s, written again, is the same), and what fails fails as
    /// the long way fails, with the same values at the same line. A key is read as `get` and
    /// `set` read it: a text as it is, anything else as its text, an int with no text made; an
    /// error is no key, and both give it back (found in review: int keys went the long way, after
    /// their key had been worked out, and a histogram was 5% slower). `stored`: an assignment's,
    /// whose map stays where it is, as `in_place_call` leaves one.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn in_place_counted(&mut self, name: Sym, key: &Expr, op: BinOp, read: &Expr, more: &Expr, line: u32, env: &Env, stored: bool) -> Result<Option<Value>, Ctrl> {
        static GET: OnceLock<Sym> = OnceLock::new();
        let Some((f, Expr::Ident(n, _), [k, start @ ..], _)) = call_on(read) else { return Ok(None) };
        if f != *GET.get_or_init(|| Sym::new("get")) || *n != name || start.len() > 1 || !start.iter().all(constant) || !same_expr(key, k) || !pure(key, env) || !pure(more, env) {
            return Ok(None);
        }
        // `m.get(k)` is a function the map holds, if it does.
        let field = matches!(read, Expr::Call(g, ..) if matches!(&**g, Expr::Field(..)));
        if !matches!(lookup(env, f), Some(Value::Builtin("get", _))) || crate::value::with_var(env, name, |v| matches!(v, Value::Map(m) if !(field && m.contains_key("get")))) != Some(true) {
            return Ok(None);
        }
        let kv = self.eval(key, env)?;
        let start = match start.first() {
            Some(d) => self.eval(d, env)?,
            None => Value::Nil,
        };
        let more = self.eval(more, env)?;
        let mut digits = [0u8; 20];
        let owned;
        let k: &str = match &kv {
            Value::Str(s) => s,
            Value::Int(i) => int_text(*i, &mut digits),
            // `get` gives the error back, and `+` of it fails, or else `set` gives it back too,
            // which the variable then holds (builtins.rs: `get`, `set_of`).
            Value::Err(..) => {
                binop(op, kv.clone(), more, line)?;
                return Ok(Some(kv));
            }
            v => {
                owned = v.key_text();
                &owned
            }
        };
        let done = crate::value::with_slot(env, name, |whole| -> Result<bool, Ctrl> {
            let Value::Map(m) = whole else { return Ok(false) };
            let at = m.get_index_of(k);
            let old = match at {
                Some(i) => m[i].clone(),
                None => start,
            };
            let now = binop(op, old, more, line)?;
            let m = Arc::make_mut(m);
            match at {
                Some(i) => m[i] = now,
                None => {
                    m.insert(crate::value::Key::new(k), now);
                }
            }
            Ok(true)
        });
        match done {
            // An assignment has nothing left to do: the map is where it goes.
            Some(Ok(true)) if stored => {
                self.stored = true;
                Ok(Some(Value::Nil))
            }
            Some(Ok(true)) => Ok(take_local(env, name)),
            Some(Err(e)) => Err(e),
            // Asked above: it is a map.
            _ => Ok(None),
        }
    }

    #[inline(never)]
    fn eval_index(&mut self, obj: &Expr, idx: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        // `xs[i]` of a variable reads the item where the list is, without a hold on all of
        // it to let go of after; the index first, which reads and calls nothing, so the order
        // changes nothing (FEEDBACK29.md; branch feedback29-fixes).
        if let Expr::Ident(var, _) = obj {
            if self.reads.is_none() && self.tracks.is_empty() && quiet(idx) {
                let i = self.eval(idx, env)?;
                if let Value::Int(n) = i {
                    let found = crate::value::with_var(env, *var, |v| match v {
                        Value::List(l) => {
                            let at = if n < 0 { l.len() as i64 + n } else { n };
                            Some(if at >= 0 { l.get(at as usize).cloned().unwrap_or(Value::Nil) } else { Value::Nil })
                        }
                        Value::Str(s) => Some(self.char_at(s, n)),
                        _ => None,
                    });
                    if let Some(Some(v)) = found {
                        return Ok(v);
                    }
                }
                let o = self.eval(obj, env)?;
                return self.index_value(o, i, line);
            }
        }
        let o = self.eval(obj, env)?;
        let i = self.eval(idx, env)?;
        self.index_value(o, i, line)
    }

    fn index_value(&mut self, o: Value, i: Value, line: u32) -> Result<Value, Ctrl> {
        match (&o, &i) {
            (Value::List(l), Value::Int(n)) => {
                let n = if *n < 0 { l.len() as i64 + n } else { *n };
                Ok(if n >= 0 { l.get(n as usize).cloned().unwrap_or(Value::Nil) } else { Value::Nil })
            }
            (Value::Map(m), Value::Str(k)) => Ok(m.get(&**k).cloned().unwrap_or(Value::Nil)),
            // Keys are text: a number is the key it is written as, as for
            // `set` and `get`.
            (Value::Map(m), Value::Int(n)) => Ok(m.get(n.to_string().as_str()).cloned().unwrap_or(Value::Nil)),
            (Value::Str(s), Value::Int(n)) => Ok(self.char_at(s, *n)),
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
    #[allow(clippy::too_many_arguments)]
    fn eval_for(&mut self, node: &Expr, pat: &Pattern, iter: &Expr, body: &Expr, env: &Env, line: u32, keep: bool) -> Result<Value, Ctrl> {
        let mut out = Vec::new();
        let mut scope = None;
        // `range(a, b)` and `a..b`: counted, without making the list.
        if let Some((a, b, step)) = self.counted(iter, env, line, keep)? {
            if keep {
                // A script stops after its steps: no room for more.
                out.reserve(crate::builtins::range_count(a, b, step).min(self.fuel) as usize);
            }
            let mut i = a;
            let mut rounds = 0u64;
            while if step > 0 { i < b } else { i > b } {
                // A long loop goes on in machine code from here (src/jitloop.rs).
                #[cfg(feature = "jit")]
                if !keep && rounds == crate::jitloop::WARM && self.loops_compile() {
                    if let Some(s) = scope.take() {
                        Scope::done(s);
                    }
                    match crate::jitloop::run(self, node, env, Some((i, b, step))) {
                        Some(crate::jitloop::Ran::Done) => break,
                        Some(crate::jitloop::Ran::Resume(k)) => {
                            i = k;
                            rounds += 1;
                            continue;
                        }
                        Some(crate::jitloop::Ran::Failed(e)) => return Err(e),
                        None => {}
                    }
                }
                rounds += 1;
                let v = self.for_step(pat, Value::Int(i), body, env, &mut scope, line, keep)?;
                if keep {
                    out.push(v);
                }
                // Past the last int there is nothing to count to.
                let Some(next) = i.checked_add(step) else { break };
                i = next;
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
                let v = self.for_step(pat, item.clone(), body, env, &mut scope, line, keep)?;
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
    #[allow(clippy::too_many_arguments)]
    fn for_step(&mut self, pat: &Pattern, item: Value, body: &Expr, env: &Env, scope: &mut Option<Env>, line: u32, keep: bool) -> Result<Value, Ctrl> {
        self.within_limit(line)?;
        if self.fuel != u64::MAX {
            self.burn(line)?;
        }
        // The loop's variable given its next value where it is, when the last round left
        // nothing else in its scope (FEEDBACK29.md; branch feedback29-fixes).
        let rebound = match (pat, scope.as_mut()) {
            (Pattern::Bind(n), Some(sc)) => Scope::rebind(sc, *n, item),
            _ => Err(item),
        };
        if let Err(item) = rebound {
            if !scope.as_mut().map_or(false, Scope::reuse) {
                *scope = Some(Scope::child(env));
            }
            let sc = scope.as_ref().expect("set above");
            if let Pattern::Bind(n) = pat {
                define(sc, *n, item);
            } else {
                let mut binds = Vec::new();
                if self.match_pat(pat, &item, Mode::Strict, env, &mut binds)?.is_none() {
                    return fail(line, format!("`for` pattern does not match {}", short(&item)));
                }
                for (n, x) in binds {
                    define(sc, &n, x);
                }
            }
        }
        let sc = scope.as_ref().expect("set above");
        // The body's own variables can go in this scope: it is new each round.
        match body {
            Expr::Block(stmts) => self.block_in(stmts, sc, keep),
            body if keep => self.eval(body, sc),
            body => self.eval_unused(body, sc),
        }
    }

    /// The bounds of `range(a, b)`, `range(a, b, step)`, `range(n)` or `a..b` (the
    /// builtin, not a variable of that name), checked as those check them; None for
    /// anything else.
    fn counted(&mut self, iter: &Expr, env: &Env, line: u32, keep: bool) -> Result<Option<(i64, i64, i64)>, Ctrl> {
        let (a, b, step, f) = match iter {
            Expr::Binary(BinOp::Range, a, b, _) => (self.eval(a, env)?, self.eval(b, env)?, Value::Int(1), None),
            Expr::Call(callee, args, _) if (1..=3).contains(&args.len()) && matches!(&**callee, Expr::Ident(n, _) if n == "range") => {
                let Some(f @ Value::Builtin("range", _)) = lookup(env, "range") else { return Ok(None) };
                match args.as_slice() {
                    [b] => (Value::Int(0), self.eval(b, env)?, Value::Int(1), Some(f)),
                    [a, b] => (self.eval(a, env)?, self.eval(b, env)?, Value::Int(1), Some(f)),
                    [a, b, c] => (self.eval(a, env)?, self.eval(b, env)?, self.eval(c, env)?, Some(f)),
                    _ => unreachable!("one to three"),
                }
            }
            _ => return Ok(None),
        };
        match (&a, &b, &step) {
            // A loop that keeps nothing makes no list, so a list's limit means nothing
            // here (FEEDBACK29.md, Q-356; branch feedback29-fixes).
            (Value::Int(x), Value::Int(y), Value::Int(s)) if *s != 0 && (!keep || crate::builtins::range_count(*x, *y, *s) <= 10_000_000) => Ok(Some((*x, *y, *s))),
            // Anything else fails as it would have.
            _ => match f {
                Some(f) => {
                    let args = match iter {
                        Expr::Call(_, args, _) if args.len() == 1 => vec![b],
                        Expr::Call(_, args, _) if args.len() == 2 => vec![a, b],
                        _ => vec![a, b, step],
                    };
                    self.call(&f, args, line).map(|_| None)
                }
                None => binop(BinOp::Range, a, b, line).map(|_| None),
            },
        }
    }

    #[inline(never)]
    fn eval_reply(&mut self, status: &Expr, body: Option<&Expr>, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let code = match self.eval(status, env)? {
            // Not a 1xx, which is no answer (FEEDBACK36.md, Q-433; branch feedback36-39).
            Value::Int(n) if (200..=599).contains(&n) => n as u16,
            other => return fail(line, format!("reply status must be an HTTP status code (200-599), got {}", short(&other))),
        };
        let body = match body {
            Some(b) => self.eval(b, env)?,
            None => Value::Nil,
        };
        let resp = match body {
            // A stream's function would send a body after a head that says there is none
            // (FEEDBACK55.md, Q-709; branch feedback55).
            Value::Response(r) if matches!(r.body, Body::Stream { .. }) && crate::stream::bodyless(code as i64) => {
                return fail(line, crate::stream::no_body(if matches!(r.body, Body::Stream { events: true, .. }) { "events" } else { "stream" }, code as i64));
            }
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
        // The pipe's quotient of two ints, for this call alone (see `eval_pipe`).
        let piped = if pre.is_some() && self.pipe_div.is_some() { self.pipe_div.take() } else { None };
        // `(a / b).floor()`, as `floor(a / b)` (Q-788).
        if let (Expr::Field(obj, name, _), [], None) = (callee, args, &pre) {
            if let Expr::Binary(BinOp::Div, a, b, at) = &**obj {
                // What a number's method is: the function of that name.
                if let Some(f @ Value::Builtin(n, _)) = lookup(env, name).filter(|f| matches!(f, Value::Builtin(n, _) if ROUNDERS.contains(n))) {
                    return match self.div_of(a, b, Some(n), env, *at)? {
                        Ok(exact) => Ok(exact),
                        Err(v) => self.call(&f, vec![v], line),
                    };
                }
            }
        }
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
            // `shop.notes()` on a `site`: the site's server fn of that name (FEEDBACK33.md,
            // Q-412; branch feedback27-35-fixes).
            // Only a value a `site` declaration made, not any map with its keys (FEEDBACK51.md,
            // Q-640; branch feedback51-fixes).
            let site = method.is_none() && crate::sites::is_site(&recv);
            match method {
                Some(f) => f,
                None if site => {
                    argv.push(recv);
                    argv.push(Value::str(name.as_str()));
                    Value::Builtin("__site_call", crate::sites::call)
                }
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
        // `floor(a / b)` of two ints, `ceil` and `round` of it too, and math's: worked out in ints,
        // the guide's integer division; through the float `a / b` it was off past 2^53,
        // `floor(9007199254740995 / 2)` ...498 (FEEDBACK55.md, Q-788; branch feedback55).
        // However the builtin is reached: `fl(a / b)` with `fl` holding it, `m.floor(a / b)` with
        // the module in a variable (found in review).
        // Not where the builtin is a value's method, `name(recv, a / b)`: `math.round(7 / 2)`,
        // which `math` has not, went as `round(7 / 2)` and gave 4, where the browser and
        // `math.round(3.5)` refuse it (FEEDBACK55.md, Q-788; branch feedback55).
        if let (None, [Expr::Binary(BinOp::Div, a, b, at)], Value::Builtin(name, _), true) = (&pre, args, &f, argv.is_empty()) {
            if ROUNDERS.contains(name) {
                return match self.div_of(a, b, Some(name), env, *at)? {
                    Ok(exact) => Ok(exact),
                    Err(v) => self.call(&f, vec![v], line),
                };
            }
        }
        if let Some(p) = pre {
            argv.push(p);
        }
        // `print(`it failed: ${r}`)` is the program's own log (FEEDBACK26.md, Q-197;
        // branch feedback26-fixes).
        let printing = matches!(f, Value::Builtin("print", _));
        for a in args {
            self.printing = printing && matches!(a, Expr::Template(_));
            let v = self.eval(a, env);
            self.printing = false;
            argv.push(v?);
        }
        self.db_on = db_on;
        if let Value::Func(c) = &f {
            if c.def.component {
                return fail(line, component_called(&c.def.name));
            }
        }
        if let (Some(p), true) = (piped, args.is_empty()) {
            self.exact_div = Some(p);
        }
        // Where a tool's question is: this call, by its callee (FEEDBACK56.md, Part E, found in
        // review; branch feedback56).
        if let Some(a) = self.asking.as_mut() {
            a.site = callee as *const Expr as usize;
        }
        self.call(&f, argv, line)
    }

    /// The pipe's quotient of two ints given alone to `name`, one of `floor`'s kind: in ints.
    /// The record is taken whatever `name` is, so that no later call sees it.
    #[cold]
    #[inline(never)]
    fn piped_rounder(&mut self, name: &str, args: &[Value]) -> Option<Result<Value, Ctrl>> {
        let d = self.exact_div.take()?;
        match args {
            [Value::Float(f)] if f.to_bits() == d.bits && ROUNDERS.contains(&name) => Some(crate::builtins::int_div_rounded(name, d.a, d.b, d.line)),
            _ => None,
        }
    }

    /// `a / b` for `rounder`, `floor` or one of its kind, when there is one: Ok with its answer in
    /// ints when both are ints, else Err with the quotient, for the call to go on with.
    fn div_of(&mut self, a: &Expr, b: &Expr, rounder: Option<&str>, env: &Env, at: u32) -> Result<Result<Value, Value>, Ctrl> {
        let x = self.eval(a, env)?;
        let y = self.eval(b, env)?;
        if let (Some(name), Value::Int(p), Value::Int(q)) = (rounder, &x, &y) {
            if self.fuel != u64::MAX {
                self.burn(at)?;
            }
            return crate::builtins::int_div_rounded(name, *p, *q, at).map(Ok);
        }
        Ok(Err(if self.fuel != u64::MAX { self.script_binop(BinOp::Div, x, y, at)? } else { binop(BinOp::Div, x, y, at)? }))
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
                    // As `respond` takes them back: a header named `null` is null, and `set-cookie`
                    // sent more than once a list, where each went for the last (FEEDBACK55.md,
                    // Q-710; branch feedback55).
                    let mut m = Map::with_capacity(r.headers.len());
                    for (k, v) in r.headers.iter() {
                        let v = if v == crate::server::DROPPED { Value::Nil } else { Value::str(v) };
                        let cookie = k.eq_ignore_ascii_case("set-cookie");
                        match m.get_mut(k.as_str()) {
                            Some(was) if cookie => {
                                *was = match std::mem::replace(was, Value::Nil) {
                                    Value::List(l) => {
                                        let mut l = (*l).clone();
                                        l.push(v);
                                        Value::list(l)
                                    }
                                    Value::Nil => v,
                                    first => Value::list(vec![first, v]),
                                };
                            }
                            Some(was) => *was = v,
                            None => {
                                m.insert(k.as_str().into(), v);
                            }
                        }
                    }
                    return Ok(Value::map(m));
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
            Value::Err(e, _, _) if name == "error" || name == "message" => return Ok((**e).clone()),
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
        // A value that did not fail has no error, whatever it is, as for a map: a value
        // whose type nothing checked must not stop a route for asking (FEEDBACK26.md,
        // Q-199; branch feedback26-fixes).
        if matches!(name, "error" | "message") && !matches!(v, Value::Err(..)) {
            return Ok(Value::Nil);
        }
        let hint = if name == "length" && matches!(v, Value::List(_) | Value::Str(_)) { ": `len(x)` is how many it holds" } else { "" };
        fail(line, format!("{} has no field `{}`{}", v.type_name(), name, hint))
    }

    pub fn call(&mut self, f: &Value, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
        if self.fuel != u64::MAX {
            self.burn(line)?;
        }
        match f {
            Value::Builtin(name, func) => {
                // More than it takes: said, as `check` says it where it knows the function; the rest
                // were dropped, and `xs.sort((a, b) => b - a)` sorted up (FEEDBACK55.md, Q-786;
                // branch feedback55).
                if args.len() > crate::builtins::most_args(name) {
                    // `ask` given choices and a default in a tool's call, where `quillo check`
                    // could not see it: told of `about`, and the call stops there, as at the
                    // page's `ask` (FEEDBACK56.md, the user's decision 2026-11-09; branch
                    // feedback56).
                    if *name == "ask" && (self.asking.is_some() || self.in_tool) {
                        return crate::mcp::ask_in_a_tool(self, args.len(), line);
                    }
                    return fail(line, crate::check::too_many_args(name, args.len()));
                }
                // The quotient a pipe made of two ints, given by the pipe alone to `floor` or its
                // kind: in ints (see `eval_pipe`). Set for this call alone, and taken by it.
                if self.exact_div.is_some() {
                    if let Some(r) = self.piped_rounder(name, &args) {
                        return r;
                    }
                }
                if self.allowed != ALL_EFFECTS {
                    self.check_builtin_effect(name, &args, line)?;
                }
                if self.writes_noted() && CHANGES.contains(name) {
                    self.no_writes(name, line)?;
                }
                // Another function of its `parallel` failed: not a write more (FEEDBACK55.md,
                // Q-804; branch feedback55).
                if self.stop.is_some() {
                    self.stopped_here(line)?;
                }
                if self.in_run || !self.tracks.is_empty() {
                    self.note_builtin(name);
                }
                // A tool's call that may ask its user: what changes something is noted, and
                // nothing runs once it stopped to ask (FEEDBACK56.md, Part E; branch feedback56).
                if self.asking.is_some() {
                    crate::mcp::noted(self, name, line)?;
                }
                if name.starts_with("native:") {
                    return crate::native::call(self, name, args, line);
                }
                if self.fuel != u64::MAX {
                    return self.script_builtin(name, *func, args, line);
                }
                // An error given to a builtin that cannot take one: what it was is said, as
                // the builtin's own refusal names only its type (FEEDBACK27.md, Q-333; branch
                // feedback27-35-fixes).
                let given = args.iter().find_map(|a| if let Value::Err(e, _, at) = a { Some((e.clone(), *at)) } else { None });
                // The usual call, given no error, takes the path it always took.
                let Some(given) = given else {
                    return match func(self, args, line) {
                        // All an error value is made by: `err` and the builtins that fail
                        // (FEEDBACK21.md, Q-174; branch feedback20-fixes).
                        Ok(Value::Err(e, own, Place::NONE)) => Ok(Value::Err(e, own, Place::new(self.cur_file.as_ref(), line))),
                        r => r,
                    };
                };
                let given = Some(given);
                let r = match func(self, args, line) {
                    Ok(Value::Err(e, own, Place::NONE)) => Ok(Value::Err(e, own, Place::new(self.cur_file.as_ref(), line))),
                    r => r,
                };
                match (given, r) {
                    (Some(g), Err(Ctrl::Error(mut e))) if refused_an_error(&e.msg) => {
                        e.msg = format!("{} ({})", e.msg, earlier(&g));
                        Err(Ctrl::Error(e))
                    }
                    (Some(g), Ok(Value::Err(e, own, at))) if matches!(&*e, Value::Str(m) if refused_an_error(m)) => {
                        let m = format!("{} ({})", e.display(), earlier(&g));
                        Ok(Value::Err(Arc::new(Value::str(m)), own, at))
                    }
                    (_, r) => r,
                }
            }
            Value::Func(c) => {
                // The pipe's quotient is no function's to take (see `eval_pipe`).
                if self.exact_div.is_some() {
                    self.exact_div = None;
                }
                let c = c.clone();
                self.call_closure_given(&c, args, line)
            }
            other => {
                self.exact_div = None;
                fail(line, format!("cannot call {}: it is not a function", other.type_name()))
            }
        }
    }

    /// `args[i] == None` means "not given": the parameter's default is used.
    pub fn call_closure(&mut self, c: &Closure, args: Vec<Option<Value>>, line: u32) -> Result<Value, Ctrl> {
        self.within_limit(line)?;
        if self.fuel != u64::MAX {
            self.burn(line)?;
        }
        // Numbers in, a number out: in machine code once it is called again. Interpreted, a call
        // whose machine code went too deep takes the floor it set away as it ends, by any way out
        // (`jit::Floored`; FEEDBACK52.md, Q-945 review 5; branch feedback52).
        #[cfg(feature = "jit")]
        let _floored = if self.fuel == u64::MAX && self.depth < MAX_DEPTH && !crate::debug::on() && !crate::httpd::compute_limited() {
            if let Some(r) = crate::jit::call(c, &args, self.depth) {
                return in_file(r, &c.def.file);
            }
            crate::jit::Floored::after(self.depth)
        } else {
            None
        };
        // `(x) => x.name`, as given to `sort_by`, `map` and `filter`: the
        // field, without a scope for the call.
        if let (Some((field, at)), [Some(_)], u64::MAX) = (projection(&c.def), args.as_slice(), self.fuel) {
            let v = args.into_iter().next().flatten().unwrap_or(Value::Nil);
            return in_file(self.get_field(v, field, &c.env, at), &c.def.file);
        }
        let n = args.len();
        self.call_closure_with(c, args, n, line)
    }

    /// `call_closure` with every argument given: no list of them to make again.
    fn call_closure_given(&mut self, c: &Closure, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
        self.within_limit(line)?;
        if self.fuel != u64::MAX {
            self.burn(line)?;
        }
        #[cfg(feature = "jit")]
        let _floored = if self.fuel == u64::MAX && self.depth < MAX_DEPTH && !crate::debug::on() && !crate::httpd::compute_limited() {
            if let Some(r) = crate::jit::call(c, &args, self.depth) {
                return in_file(r, &c.def.file);
            }
            crate::jit::Floored::after(self.depth)
        } else {
            None
        };
        if let (Some((field, at)), 1, u64::MAX) = (projection(&c.def), args.len(), self.fuel) {
            let v = args.into_iter().next().unwrap_or(Value::Nil);
            return in_file(self.get_field(v, field, &c.env, at), &c.def.file);
        }
        let n = args.len();
        self.call_closure_with(c, args.into_iter().map(Some), n, line)
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
        if self.fuel == u64::MAX {
            if identity(&c.def) {
                return Ok(x);
            }
            if let Some(l) = constant_lambda(&c.def) {
                return Ok(lit_value(l));
            }
            if let Some((field, at)) = projection(&c.def) {
                return in_file(self.get_field(x, field, &c.env, at), &c.def.file);
            }
        }
        if c.def.component {
            return fail(line, component_called(&c.def.name));
        }
        // A number for a number, as most of these functions are (FEEDBACK29.md; branch
        // feedback29-fixes).
        #[cfg(feature = "jit")]
        let _floored = if matches!(x, Value::Int(_) | Value::Float(_) | Value::Bool(_)) && self.fuel == u64::MAX && self.depth < MAX_DEPTH && !crate::debug::on() && !crate::httpd::compute_limited() {
            if let Some(r) = crate::jit::call(c, std::slice::from_ref(&x), self.depth) {
                return in_file(r, &c.def.file);
            }
            crate::jit::Floored::after(self.depth)
        } else {
            None
        };
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
        // A call within its file, as most are, has nothing to change here (FEEDBACK29.md;
        // branch feedback29-fixes).
        let same = same_file(&self.cur_file, &def.file) && same_file(&self.caller_file, &def.file);
        let swapped = (!same).then(|| {
            let caller = std::mem::replace(&mut self.cur_file, def.file.clone());
            let outer = std::mem::replace(&mut self.caller_file, caller.clone());
            (caller, outer)
        });
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
            Err(Ctrl::Error(e)) => {
                let caller = swapped.as_ref().map_or(&self.cur_file, |(c, _)| c);
                Err(Ctrl::Error(Box::new(e.called(&def.name, caller.as_ref(), line))))
            }
            r => r,
        };
        if let Some((caller, outer)) = swapped {
            self.cur_file = caller;
            self.caller_file = outer;
        }
        if was_component {
            self.in_component = true;
        }
        if let Some((mask, who)) = saved {
            self.allowed = mask;
            self.restrictor = who;
        }
        r
    }

    /// What `enter_call` changes on the way in and puts back on the way out, as it is now: a call
    /// that panics never comes back to put it back, and the MCP door, which answers the next
    /// message after one, puts it back itself (`put_back`; FEEDBACK56.md, C.1, found in review;
    /// branch feedback56). Left as it was, a `with db` narrowed every tool after it.
    pub(crate) fn entered(&self) -> Entered {
        Entered {
            depth: self.depth,
            cur_file: self.cur_file.clone(),
            caller_file: self.caller_file.clone(),
            in_component: self.in_component,
            allowed: self.allowed,
            restrictor: self.restrictor.clone(),
        }
    }

    pub(crate) fn put_back(&mut self, e: Entered) {
        self.depth = e.depth;
        self.cur_file = e.cur_file;
        self.caller_file = e.caller_file;
        self.in_component = e.in_component;
        self.allowed = e.allowed;
        self.restrictor = e.restrictor;
    }

    /// "`http.get` needs the http effect, but `load` is declared `with db`".
    /// `f` under the route's `with`, as its body ran: narrowed, never widened (branch
    /// feedback40-43).
    pub(crate) fn under_route<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let Some(def) = self.route_limit.clone() else { return f(self) };
        let mask = effect_mask(def.effects.as_deref().unwrap_or_default()) & self.allowed;
        let allowed = std::mem::replace(&mut self.allowed, mask);
        let who = std::mem::replace(&mut self.restrictor, Some(def));
        let r = f(self);
        self.allowed = allowed;
        self.restrictor = who;
        r
    }

    fn effect_denied(&self, what: &str, missing: u8) -> String {
        let names = effect_names(missing);
        let need = format!("the {} effect{}", names.join(", "), if names.len() == 1 { "" } else { "s" });
        // An effect the `with` names but the policy took away (a route's, narrowed by the
        // policy rather than refused) is the policy's to explain (branch feedback40-43).
        match self.restrictor.as_ref().filter(|f| effect_mask(f.effects.as_deref().unwrap_or_default()) & missing == 0) {
            Some(f) => format!(
                "`{}` needs {}, but `{}` is declared `with {}`",
                what,
                need,
                f.name,
                f.effects.as_deref().unwrap_or_default().join(", ")
            ),
            None if crate::policy::ceiling() != ALL_EFFECTS => format!("`{}` needs {}, which {} does not allow", what, need, crate::policy::get().effects_source()),
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
    fn check_builtin_effect(&self, name: &str, args: &[Value], line: u32) -> Result<(), Ctrl> {
        let need = builtin_effect(name);
        if need & !self.allowed != 0 {
            if need == ALL_EFFECTS && name.starts_with("native:") {
                return fail(line, crate::native::undeclared(crate::native::display_name(name), &crate::native::file_of(name), self.restrictor.as_ref().map(|f| f.name.as_str())));
            }
            // A site's function as it is written, `shop.notes` (FEEDBACK51.md, Q-640; branch
            // feedback51-fixes).
            if let ("__site_call", [Value::Map(m), Value::Str(f), ..]) = (name, args) {
                return fail(line, self.effect_denied(&format!("{}.{}", m.get("$site").map_or(String::new(), |s| s.display()), f), need));
            }
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
            // (FEEDBACK2.md, Q-028; branch feedback6-fixes). Its code is all of it, the
            // defaults of its parameters too (FEEDBACK55.md, Q-880; branch feedback55).
            let (names, visitor) = def.request_use();
            if self.reads.is_some() && !was && !names {
                self.read_request = false;
            }
            self.visitor_read |= visitor;
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
            define(&scope, p.name, v);
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
                    // A component's body that declared its states here: its markup starts.
                    if self.island_scopes.last().is_some_and(|(_, e, _)| Arc::ptr_eq(e, &scope)) {
                        self.markup_starts();
                    }
                    if crate::debug::on() {
                        return crate::debug::around(self, end, &scope, |it| it.render_expr_into(last, &scope, out));
                    }
                    self.render_expr_into(last, &scope, out)
                }
                _ => {
                    let v = self.eval_block(stmts, env, true)?;
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
    /// Why `v` does not match `p`, a route's query or request pattern: the first field missing
    /// or of another kind, as the 400 under `--watch` and the log say (FEEDBACK47.md, Q-544;
    /// branch feedback40-43). `place` is what the fields are under: `query`, or none for a
    /// request pattern, whose fields name `form`, `json` or `headers` themselves.
    pub fn why_no_match(&mut self, p: &Pattern, v: &Value, place: &str, mode: Mode) -> String {
        // Neither a value sent (a token in a header, a password) nor a pattern's own literal
        // (a webhook's secret) is said: this goes to the log, and under `--watch` to whoever
        // sent the request. Nothing is evaluated either: a default is code of the program's
        // (found in review).
        let Pattern::Record(fields) = p else { return "the request does not match its pattern".to_string() };
        for f in fields {
            let name = if place.is_empty() { f.path.join(".") } else { format!("{}.{}", place, f.path.join(".")) };
            let sub = match mode {
                Mode::Request if f.path[0] == "json" => Mode::Strict,
                Mode::Request => Mode::Lenient,
                m => m,
            };
            let article = |t: &str| if t.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
            let wanted = match &f.pat {
                Pattern::Type(t) => format!("{} {}", article(t), t),
                Pattern::List(items, None) if items.len() == 1 => match &items[0] {
                    Pattern::Type(t) => format!("a list of {}", t),
                    _ => "a list".to_string(),
                },
                Pattern::Lit(_) => "the value the route asks for".to_string(),
                _ => "what its pattern takes".to_string(),
            };
            match get_path(v, &f.path) {
                None | Some(Value::Nil) if f.default.is_none() => {
                    let checkbox = name.starts_with("form.") && matches!(&f.pat, Pattern::Type(t) if t == "bool");
                    let hint = if checkbox {
                        format!(" (an unchecked checkbox sends nothing: give it a default, `{}: bool = false`)", f.path.last().map_or("", |s| s.as_str()))
                    } else {
                        String::new()
                    };
                    return format!("`{}` is missing, and must be {}{}", name, wanted, hint);
                }
                // `{ form: { agree: bool } }`: the field inside, by its whole name.
                Some(x @ Value::Map(_)) if matches!(f.pat, Pattern::Record(_)) => {
                    let inner = self.why_no_match(&f.pat, &x, &name, sub);
                    if inner != "the request does not match its pattern" {
                        return inner;
                    }
                }
                // Money says which of `money.of`'s rules it fails (FEEDBACK50.md, Q-621; branch
                // feedback50-fixes).
                Some(x) if matches!(&f.pat, Pattern::Type(t) if t == "money") && !matches!(x, Value::Nil) => {
                    if let Err(why) = crate::money::from_data(&x) {
                        return format!("`{}` {}", name, why);
                    }
                }
                Some(x) if !matches!(x, Value::Nil) && !self.pattern_fits(&f.pat, &x, sub == Mode::Lenient) => {
                    let kind = match &x {
                        Value::Str(_) => "text".to_string(),
                        other => format!("{} {}", article(other.type_name()), other.type_name()),
                    };
                    return format!("`{}` is {}, not {}", name, kind, wanted);
                }
                _ => {}
            }
        }
        "the request does not match its pattern".to_string()
    }

    /// Whether `x` could match `p`, by its type and shape only: what `why_no_match` reads.
    fn pattern_fits(&self, p: &Pattern, x: &Value, lenient: bool) -> bool {
        match p {
            Pattern::Type(t) if self.types.contains_key(t) => true,
            Pattern::Type(t) if t == "dec" => as_dec(x).is_some(),
            Pattern::Type(t) if t == "money" => crate::money::from_data(x).is_ok(),
            Pattern::Type(t) => type_name_matches(t, x) || (lenient && matches!(x, Value::Str(s) if coerce(t, s).is_some())),
            Pattern::Lit(l) => {
                let lv = lit_value(l);
                equal(&lv, x) || (lenient && matches!(x, Value::Str(s) if coerce(lv.type_name(), s).is_some_and(|c| equal(&c, &lv))))
            }
            Pattern::Alt(ps) => ps.iter().any(|q| self.pattern_fits(q, x, lenient)),
            Pattern::As(q, _) => self.pattern_fits(q, x, lenient),
            Pattern::List(items, None) if items.len() == 1 => matches!(x, Value::List(l) if l.iter().all(|y| self.pattern_fits(&items[0], y, lenient))),
            _ => true,
        }
    }

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
                binds.push((n.to_string(), v.clone()));
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
            // Money from data, held to `money.of`'s rules (FEEDBACK50.md, Q-621; branch
            // feedback50-fixes).
            Pattern::Type(t) if t == "money" => crate::money::from_data(v).ok(),
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
            // `{ tag: [str] }` in a query or a form: every value of that name
            // (FEEDBACK26.md, Q-200; branch feedback26-fixes).
            Pattern::List(items, None) if lenient && items.len() == 1 => {
                let Value::List(l) = v else { return Ok(None) };
                let mut out = Vec::with_capacity(l.len());
                for x in l.iter() {
                    let Some(y) = self.match_pat(&items[0], x, mode, env, binds)? else { return Ok(None) };
                    out.push(y);
                }
                Some(Value::list(out))
            }
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
                    // A blank field of a query or a form, as a search form sends one left empty,
                    // is one not sent when there is a default: `?page=` was a 400 (FEEDBACK47.md,
                    // Q-543; branch feedback40-43).
                    // Not a `str` field: there blank text is text, a password of spaces included
                    // (found in review).
                    let text_field = matches!(&f.pat, Pattern::Type(t) if t == "str" || t == "any");
                    let blank = sub_mode == Mode::Lenient && f.default.is_some() && !text_field && matches!(&found, Some(Value::Str(s)) if s.trim().is_empty());
                    let val = match (found, &f.default) {
                        (Some(x), _) if !matches!(x, Value::Nil) && !blank => x,
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
        if self.select_want.is_some() && (el.tag == "option" || el.tag == "optgroup") {
            return self.render_option_into(el, env, out);
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
        // Tags that never change: their HTML is in pieces, made once. Not a <pre>, whose text,
        // known only as it is written, may need a newline before it (FEEDBACK48.md, Q-572;
        // branch main).
        if f.tags_fixed && !drops_first_newline(&el.tag) {
            if f.pieces.get().is_none() {
                let mut pieces = Vec::new();
                let mut html = String::new();
                self.pieces_into(el, env, &mut pieces, &mut html)?;
                if !html.is_empty() {
                    pieces.push(Piece::Html(html.into()));
                }
                let _ = f.pieces.set(pieces);
            }
            // In an `<svg>` (or out of it, in a `<foreignObject>`): see `in_svg`.
            let outer = f.svg.map(|v| std::mem::replace(&mut self.in_svg, v));
            let r = (|| {
                for p in f.pieces.get().expect("set above") {
                    match p {
                        Piece::Html(h) => out.push_str(h),
                        Piece::Expr(e) => self.render_expr_into(e, env, out)?,
                        Piece::El(e) => self.render_el_into(e, env, out)?,
                    }
                }
                Ok(())
            })();
            if let Some(v) = outer {
                self.in_svg = v;
            }
            return r;
        }
        self.render_tag_into(el, env, out)
    }

    /// An option (or a group of them) of a `<select value={...}>`, made each time: ` selected` on
    /// the first the value names, as the browser shows a select given that value; for a list
    /// (`<select multiple>`), on each it names. An option's value is its `value`, else its text,
    /// spaces trimmed and run together, as the browser reads it; one marked `selected` already
    /// stays as written (FEEDBACK55.md, Q-758 and Q-761; branch feedback55).
    fn render_option_into(&mut self, el: &HtmlEl, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        let start = out.len();
        let option = el.tag == "option";
        // An option whose HTML never changes is made once, as elsewhere, and marked here.
        match el.facts.html_once.get() {
            Some(h) if option && el.facts.fixed => out.push_str(h),
            _ => {
                self.render_tag_into(el, env, out)?;
                if option && el.facts.fixed {
                    let _ = el.facts.html_once.set(Arc::from(&out[start..]));
                }
            }
        }
        let html = &out[start..];
        let (Some(want), Some(end), true) = (self.select_want.as_mut(), html.find('>'), option) else { return Ok(()) };
        let tag = &html[..end];
        if want.done || start_tag_attr(tag, "selected").is_some() {
            return Ok(());
        }
        let named = match start_tag_attr(tag, "value") {
            Some(v) => want.values.iter().any(|w| w == v),
            None => {
                let text = html[end + 1..html.len() - "</option>".len()].split_ascii_whitespace().collect::<Vec<_>>().join(" ");
                want.values.iter().any(|w| *w == text)
            }
        };
        if named {
            want.done = !want.many;
            out.insert_str(start + "<option".len(), " selected");
        }
        Ok(())
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
                // Not an `<svg>` or a `<foreignObject>`, which change what is in them (`in_svg`).
                HtmlChild::El(e) if e.facts.tags_fixed && !drops_first_newline(&e.tag) && e.facts.svg.is_none() => self.pieces_into(e, env, pieces, html)?,
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
        let select = el.tag == "select";
        if select {
            // Only its own value (one left by a select whose attributes failed is not).
            self.select_pending = None;
        }
        let text_value = if f.plain_attrs { self.render_attrs_fast(el, env, out)?; None } else { self.render_attrs(el, env, out)? };
        out.push('>');
        if f.void {
            return Ok(());
        }
        if select {
            // Its options, against its value (see `render_option_into`).
            let want = self.select_pending.take().map(|v| SelectWant::of(&v));
            let outer = std::mem::replace(&mut self.select_want, want);
            let r = self.render_contents(el, env, text_value, out);
            self.select_want = outer;
            r?;
        } else if let Some(v) = f.svg {
            let outer = std::mem::replace(&mut self.in_svg, v);
            let r = self.render_contents(el, env, text_value, out);
            self.in_svg = outer;
            r?;
        } else {
            self.render_contents(el, env, text_value, out)?;
        }
        out.push_str("</");
        out.push_str(&el.tag);
        out.push('>');
        Ok(())
    }

    /// An element's contents.
    fn render_contents(&mut self, el: &HtmlEl, env: &Env, text_value: Option<String>, out: &mut String) -> Result<(), Ctrl> {
        let f = &el.facts;
        match text_value {
            Some(t) => {
                // The browser drops a first newline here: one more keeps the text's own
                // (FEEDBACK48.md, Q-572; branch main).
                if t.starts_with('\n') {
                    out.push('\n');
                }
                out.push_str(&escape(&t))
            }
            // A script is code to the browser, where HTML escaping means
            // nothing: a value goes in as JSON (a JS value, `<` escaped), and
            // HTML (`raw(...)`) as it is, but without `</` or `<!--`, so
            // neither can end the element early.
            None if el.tag == "script" => {
                for c in &el.children {
                    match c {
                        // Its own text is code, which the browser does not unescape: written as it
                        // is, but for `</` and `<!--` (FEEDBACK35.md, Q-428; branch feedback27-35-fixes).
                        HtmlChild::Expr(Expr::Lit(Lit::Str(s))) => out.push_str(&script_text(s)),
                        HtmlChild::Expr(e) => {
                            let v = self.eval(e, env)?;
                            // `{dev && raw(x)}`, `{if dev { raw(x) }}`: what puts something in or
                            // not, which nothing then, where `false` or `null` was left in the
                            // script (FEEDBACK53.md, Q-682; found in review).
                            let maybe = matches!(e, Expr::Binary(BinOp::And, ..) | Expr::If(_, _, None));
                            if !(maybe && matches!(v, Value::Nil | Value::Bool(false))) {
                                script_value(&v, out);
                            }
                        }
                        c => self.render_child(c, env, out)?,
                    }
                }
            }
            // A style sheet is raw text to the browser: `.a > b` must stay as written. Each `<` is
            // CSS's `\3c `, the same character to CSS: in HTML the text cannot end the element
            // early, and inside `<svg>` or `<math>`, where it is markup, it cannot start an
            // element, wherever the `<style>` was made (FEEDBACK51.md, Q-646; branch
            // feedback51-fixes).
            None if f.style => {
                for c in &el.children {
                    if let HtmlChild::Expr(e) = c {
                        let v = self.eval(e, env)?;
                        out.push_str(&sheet_text(&v).replace('<', "\\3c "));
                    }
                }
            }
            None => {
                let start = out.len();
                for c in &el.children {
                    self.render_child(c, env, out)?;
                }
                // A <pre>, <textarea> or <listing> whose text starts with a newline: the browser
                // drops the first one after the start tag, so one more is written, and the text
                // the page shows is the one written (FEEDBACK48.md, Q-572; branch main).
                if out.as_bytes().get(start) == Some(&b'\n') && drops_first_newline(&el.tag) {
                    out.insert(start, '\n');
                }
            }
        }
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
        // A spread's keys follow the rules of the same ones written out: `className` is `class`
        // and `htmlFor` is `for`, as React's, and `onClick: f` a handler, which only the browser
        // has (its `…Capture` refused, as written out), where they were written as they were or
        // dropped (FEEDBACK48.md, Q-567; branch main).
        let mut kept = Vec::with_capacity(spread.len());
        for (n, v) in spread {
            let lower = n.to_ascii_lowercase();
            if matches!(v, Value::Func(_) | Value::Builtin(..)) && is_react_handler(&lower) {
                if let Err(m) = crate::parser::react_event(&el.tag, &n, &lower[2..]) {
                    return fail(el.line, m);
                }
                continue;
            }
            let n = match lower.as_str() {
                "classname" => "class".to_string(),
                "htmlfor" => "for".to_string(),
                _ => n,
            };
            if spread_attr_allowed(&n) {
                kept.push((n, v));
            }
        }
        let spread = kept;
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
            // Here too: an element with `class=`, a spread or a textarea (FEEDBACK46.md, Q-531;
            // branch feedback40-43).
            let v = style_value(name, v);
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
            // Browsers ignore `value` on a select: its options show it (FEEDBACK55.md, Q-758;
            // branch feedback55).
            if name == "value" && el.tag == "select" {
                self.select_pending = Some(v);
                continue;
            }
            if name.eq_ignore_ascii_case("class") {
                match &v {
                    Value::Nil | Value::Bool(false) | Value::Func(_) | Value::Builtin(..) => {}
                    Value::Str(s) => classes.push(s.to_string()),
                    // A map's keys that are true and a list's words, as clsx gives them
                    // (FEEDBACK48.md, Q-566; branch main).
                    Value::List(_) | Value::Map(_) | Value::Bool(true) => {
                        let mut words = String::new();
                        class_words(&v, &mut words);
                        if !words.is_empty() {
                            classes.push(words);
                        }
                    }
                    other => classes.push(other.display()),
                }
                continue;
            }
            match v {
                Value::Bool(b) => write_bool_attr(name, b, &mut rest),
                // A function, as in `write_attr` (found in review).
                Value::Nil | Value::Func(_) | Value::Builtin(..) => {}
                // A builtin's error in it says `internal error`, as `json.stringify` has it: its
                // text went out in the attribute (FEEDBACK55.md, Q-773; branch feedback55).
                Value::List(_) | Value::Map(_) => rest.push_str(&format!(" {}=\"{}\"", name, escape(&attr_json(&v)))),
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
            // As in `render_attrs` (FEEDBACK55.md, Q-758; branch feedback55).
            if el.facts.select_value && name == "value" {
                self.select_pending = Some(match val {
                    AttrVal::Expr(Expr::Template(parts)) => {
                        let mut t = String::new();
                        self.template_into(parts, env, &mut t)?;
                        Value::str(&t)
                    }
                    AttrVal::Expr(e) => self.eval(e, env)?,
                    _ => Value::Bool(true),
                });
                continue;
            }
            match val {
                // `<div aria-hidden>` is `aria-hidden="true"`, as `aria-hidden={true}` is
                // (FEEDBACK48.md, Q-564; branch main).
                AttrVal::Bool => write_bool_attr(name, true, out),
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
                    v if self.printing => out.push_str(&crate::value::told(|| v.display())),
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
        let v = style_value(name, v);
        let v = match (&v, name) {
            _ if url_attr(name) && shows_script_url(&v) => Value::str(BLOCKED_URL),
            _ if anim_value_attr(tag, name) && shows_script_url_in_list(&v) => Value::str(BLOCKED_URL),
            (Value::Str(s), "href" | "src" | "action" | "formaction" | "poster") if !self.base.is_empty() => {
                Value::str(self.with_base(s))
            }
            _ => v,
        };
        match &v {
            // `aria-expanded={false}` is `aria-expanded="false"`, `disabled={false}` nothing
            // (FEEDBACK48.md, Q-564; branch main).
            Value::Bool(b) => write_bool_attr(name, *b, out),
            // A function, as a child is: nothing, where it was `<fn <fn>>` here and the browser
            // called it as the page loaded (FEEDBACK48.md, Q-571; found in review).
            Value::Nil | Value::Func(_) | Value::Builtin(..) => {}
            Value::List(_) | Value::Map(_) => {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                // As in `render_attrs` (FEEDBACK55.md, Q-773; branch feedback55).
                escape_into(&attr_json(&v), out);
                out.push('"');
            }
            // The HTML parser folds a CR, so `value={s}` would read otherwise here than
            // after an update (FEEDBACK25.md, Q-312; branch feedback26-fixes): `escape_into`
            // writes it `&#13;` (FEEDBACK55.md, Q-772; branch feedback55).
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
                    args.push(Some(children_of(el, std::mem::take(&mut kids))));
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
                    props.insert(p.name.as_str().into(), if el.children.is_empty() { Value::Nil } else { Value::str(&kids) });
                }
                args.push(Some(children_of(el, std::mem::take(&mut kids))));
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
        self.island_scopes_at.push(self.island_scopes.len());
        // A component placed in the page may defer: what renders it again (branch defer).
        let again = (!nested).then(|| args.clone());
        // What renders it once more (see `render_again`), but inside another's second render, and
        // the options of a select it is in, as they were before it.
        let twice = self.state_again.is_none().then(|| (args.clone(), self.select_want.clone()));
        let mut body = String::new();
        let was = std::mem::replace(&mut self.in_component, true);
        let marked = self.marked;
        let r = crate::value::js_numbers(true, || self.render_call_into(&c, args, line, &mut body));
        self.in_component = was;
        let (mut state, mut live) = self.islands.pop().unwrap_or_default();
        let now = self.states_now(&state);
        let defers = if nested { Vec::new() } else { std::mem::take(&mut self.island_defers) };
        r?;
        if let (Some(now), Some((args, options))) = (now, twice) {
            let after = std::mem::replace(&mut self.select_want, options);
            match self.render_again(&c, args, line, now) {
                Some((b, s, l)) => {
                    body = b;
                    state = s;
                    for (k, v) in l {
                        live.insert(k, v);
                    }
                }
                None => self.select_want = after,
            }
        }
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
                svg: self.in_svg,
            });
            extra = format!(" data-qd=\"{}\"", id);
        }
        // One marked by comments inside it is found only where this one says so (`data-qm`), and
        // one of those that runs by itself starts this one with the page (FEEDBACK61.md, Q-892;
        // branch feedback61).
        if self.marked.0 > marked.0 {
            extra.push_str(" data-qm");
        }
        let eager = crate::island::runs_by_itself(&c.def.body) || self.marked.1 > marked.1;
        let by_itself = eager || !live.is_empty();
        if self.island_wrap(&el.tag, props, state, live, eager, &extra, &body, self.in_svg, nested, out) {
            self.marked.0 += 1;
            if by_itself {
                self.marked.1 += 1;
            }
        }
        Ok(())
    }

    /// A component whose state changed while its markup was rendered, after what was rendered
    /// before showed it (a child calling back `on_ready(5)` as it starts): rendered again, its
    /// states as declared but those, which start as the first render left them, so that its page
    /// shows one value, and its island carries it, as the browser ends up showing. What its own
    /// statements set before the markup (`user = "ana"`, `n += 1`) is no reason: the browser runs
    /// them as the server did. Kept only when the second render's markup changes nothing again
    /// (`count += n` from the child does, and keeps the first, as before); at most once each, and
    /// not inside another's second render, so that what is below is not rendered over and over
    /// (FEEDBACK55.md, Q-770; branch feedback55).
    fn render_again(&mut self, c: &Closure, args: Vec<Option<Value>>, line: u32, now: Map) -> Option<(String, Map, Map)> {
        self.islands.push((Map::new(), Map::new()));
        self.island_scopes_at.push(self.island_scopes.len());
        let outer = self.state_again.replace((self.islands.len(), now));
        let mut body = String::new();
        let was = std::mem::replace(&mut self.in_component, true);
        let r = crate::value::js_numbers(true, || self.render_call_into(c, args, line, &mut body));
        self.in_component = was;
        self.state_again = outer;
        let (state, live) = self.islands.pop().unwrap_or_default();
        let again = self.states_now(&state);
        (r.is_ok() && again.is_none()).then_some((body, state, live))
    }

    /// The states of the component just rendered (its island's `state`, as declared), those that
    /// changed while its markup was rendered as the render left them, when there are any, else
    /// none; their places are let go of either way (see `render_again`).
    fn states_now(&mut self, state: &Map) -> Option<Map> {
        let from = self.island_scopes_at.pop().unwrap_or(0).min(self.island_scopes.len());
        let mut now: Option<Map> = None;
        for (at, env, seen) in self.island_scopes.drain(from..) {
            let (Some(was), Some((name, _))) = (seen, state.get_index(at)) else { continue };
            let Some(v) = lookup(&env, name.as_str()) else { continue };
            let same = match (&was, &v) {
                (Value::List(a), Value::List(b)) => Arc::ptr_eq(a, b) || crate::value::equal(&was, &v),
                (Value::Map(a), Value::Map(b)) => Arc::ptr_eq(a, b) || crate::value::equal(&was, &v),
                _ => crate::value::equal(&was, &v),
            };
            if !same {
                now.get_or_insert_with(|| state.clone()).insert(name.clone(), v);
            }
        }
        now
    }

    /// A component's markup starts (its body's statements are done): its states as they are now,
    /// which a change while the markup renders is told by (see `states_now`).
    fn markup_starts(&mut self) {
        let from = self.island_scopes_at.last().copied().unwrap_or(0).min(self.island_scopes.len());
        let Some((state, _)) = self.islands.last() else { return };
        for (at, env, seen) in &mut self.island_scopes[from..] {
            if let Some((name, _)) = state.get_index(*at) {
                *seen = lookup(env, name.as_str());
            }
        }
    }

    /// A component's island: its HTML in a `<quillo-island>` carrying its props and
    /// state (or, inside a table or a select, its own root element carrying them, or comments
    /// around it). Whether it was marked by comments.
    #[allow(clippy::too_many_arguments)]
    fn island_wrap(&self, tag: &str, props: Map, mut state: Map, live: Map, eager: bool, extra: &str, body: &str, svg: bool, nested: bool, out: &mut String) -> bool {
        // A state that still holds its very prop, as `state items = initial` does, is written as
        // a reference to it, not as a second copy of the list (FEEDBACK30.md, Q-371; branch
        // feedback27-35-fixes).
        // Written into the state's JSON as it is, after `island_json`, which writes a map the
        // program made of that shape as one, not as the mark (FEEDBACK55.md, Q-875's review;
        // branch feedback55).
        let mut marks: Vec<(String, String)> = Vec::new();
        for (k, v) in state.iter_mut() {
            let same = |p: &Value| match (p, &*v) {
                (Value::List(a), Value::List(b)) => Arc::ptr_eq(a, b) && !a.is_empty(),
                (Value::Map(a), Value::Map(b)) => Arc::ptr_eq(a, b) && !a.is_empty(),
                (Value::Str(a), Value::Str(b)) => Arc::ptr_eq(a, b) && a.len() > 64,
                _ => false,
            };
            if let Some((name, _)) = props.iter().find(|(_, p)| same(p)) {
                marks.push((k.to_string(), name.to_string()));
                *v = Value::Nil;
            }
        }
        let mut state_json = island_json(&crate::server::for_visitor(Value::map(state)));
        if let serde_json::Value::Object(o) = &mut state_json {
            for (k, name) in marks {
                o.insert(k, serde_json::json!({ "$quillo_prop": name }));
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
        // A component that runs by itself (timers, live data, `@mount`, the
        // page's JavaScript) is started as soon as the page is idle; others
        // when someone reaches for them (see the loader in server.rs).
        let data = format!(
            " data-c=\"{}\" data-p=\"{}\" data-s=\"{}\"{}{}{}",
            escape(tag),
            escape(&island_json(&crate::server::for_visitor(Value::map(props))).to_string()),
            escape(&state_json.to_string()),
            live_attrs,
            if eager { " data-e" } else { "" },
            extra
        );
        // Inside a table or a select the browser's parser moves any other
        // element out, island and all: there the component's own root
        // element is its island. So it is inside an `<svg>` (`svg`), whatever
        // it is, as SVG draws no `<quillo-island>` (FEEDBACK55.md, Q-763; branch
        // feedback55).
        if let Some(at) = crate::island::own_root_in(body, svg) {
            out.push_str(&body[..at]);
            out.push_str(" data-qi");
            out.push_str(&data);
            crate::island::root_without_island_attrs(&body[at..], out);
            return false;
        }
        // Not one element, inside another component: in an `<svg>`, or of a table's or a select's
        // parts only (or nothing, which may stand there), between two comments carrying what the
        // island would, which the parser leaves where they are, and which the browser takes up and
        // takes away. In a `<quillo-island>` there, the parser moved it out of the table, and the
        // browser made the whole page again (FEEDBACK61.md, Q-892 and Q-936; branch feedback61).
        if nested && (svg || crate::island::parts_only(body)) {
            out.push_str("<!--q");
            out.push_str(&data);
            out.push_str("-->");
            out.push_str(body);
            out.push_str("<!--/q-->");
            return true;
        }
        out.push_str("<quillo-island style=\"display:contents\"");
        out.push_str(&data);
        out.push('>');
        out.push_str(body);
        out.push_str("</quillo-island>");
        false
    }

    /// A waiting component rendered again (src/defer.rs; branch defer): the states it
    /// had as they were, its deferred ones worked out now.
    pub(crate) fn render_deferred(&mut self, d: &crate::defer::Deferred) -> Result<String, Ctrl> {
        self.state_preset = Some(d.state.clone());
        self.islands.push((Map::new(), Map::new()));
        self.island_scopes_at.push(self.island_scopes.len());
        let mut body = String::new();
        let was = std::mem::replace(&mut self.in_component, true);
        // Where it stands, in an `<svg>` or not (see `in_svg`).
        let outer = std::mem::replace(&mut self.in_svg, d.svg);
        let marked = self.marked;
        let r = crate::value::js_numbers(true, || self.render_call_into(&d.c, d.args.clone(), d.line, &mut body));
        self.in_component = was;
        let (state, mut live) = self.islands.pop().unwrap_or_default();
        let _ = self.states_now(&state);
        self.in_svg = outer;
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
        // As a component placed in the page says it (see `render_component_into`; FEEDBACK61.md, Q-892;
        // branch feedback61).
        let extra = if self.marked.0 > marked.0 { " data-qm" } else { "" };
        let eager = crate::island::runs_by_itself(&d.c.def.body) || self.marked.1 > marked.1;
        self.island_wrap(&d.tag, d.props.clone(), state, live, eager, extra, &body, d.svg, false, &mut out);
        Ok(out)
    }
}

/// Attribute `name`'s value (as written, escaped) in a start tag the server wrote (`<option a="x"
/// b`, its `>` left out): `name="..."`, or a bare `name`.
fn start_tag_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag.split_once(' ')?.1;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        // (At least the `=` of one written without a name, so each turn moves on.)
        let n = rest.find(['=', ' ']).unwrap_or(rest.len()).max(1);
        let (here, after) = rest.split_at(n);
        let (value, after) = match after.strip_prefix("=\"") {
            Some(v) => v.split_once('"').unwrap_or((v, "")),
            None => ("", after),
        };
        if here == name {
            return Some(value);
        }
        rest = after;
    }
}

/// What a `<select>`'s options are rendered against: the texts its value names (escaped, as its
/// options' are written), all of them for a list, else the first (see `render_option_into`).
#[derive(Clone)]
struct SelectWant {
    values: Vec<String>,
    many: bool,
    done: bool,
}

impl SelectWant {
    fn of(v: &Value) -> SelectWant {
        let text = |v: &Value| if matches!(v, Value::Nil) { String::new() } else { escape(&v.display()) };
        match v {
            Value::List(xs) => SelectWant { values: xs.iter().map(text).collect(), many: true, done: false },
            v => SelectWant { values: vec![text(v)], many: false, done: false },
        }
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
