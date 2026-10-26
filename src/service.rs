//! Services: parts of an app that run in this process or as servers of
//! their own, without the code changing.
//!
//! `service billing = "billing.qwe"` makes `billing.invoice(...)` a call to
//! billing's `server fn invoice`; `service /shop = "shop.qwe"` is the same
//! (named after the file) and serves shop's routes under /shop too.
//!
//! Where a service runs is configuration, not code:
//! - here (the default): loaded once per process and shared by every
//!   program in it that names it, so its variables and database are one;
//! - elsewhere: `--service-url billing=http://…` (or `QUILLO_SERVICE_BILLING`)
//!   sends its calls there, `POST /_quillo/service/<fn>`, signed with the
//!   shared `QUILLO_SERVICE_SECRET` and carrying the tenant; its routes, if
//!   it has a prefix, are passed on to it.
//!
//! A service's server fns are for other services: it answers no page's call
//! of them (`/_quillo/rpc`), here or run alone (`--as-service`), and Quillo's
//! own addresses under its prefix are not passed on.
//!
//! Either way what crosses is data, as JSON: a program behaves the same
//! whichever way it runs. `quillo run app.qwe --split` runs each service as
//! a process of its own, to see it so (see `split`).
//!
//! Calls elsewhere are made to hold up (see `remote`): several addresses
//! (`--service-url billing=http://a,http://b`) taken in turn; an address
//! that keeps failing left alone for a while (its circuit open); a call that
//! did not get through tried again, with the same call id, which the service
//! remembers, so a call is run once however often it is sent; how long a
//! call may take, how often it is tried and when an address is left alone,
//! set where the service is declared (`{ timeout: "2s", retries: 2 }`).
//! Each call carries the request's id (`x-request-id`), which the service's
//! own calls pass on and the JSON log shows. `billing.later.invoice(...)`
//! sends a call to be made in the background, until it is made (see
//! `later`).

use crate::ast::{FnDef, Stmt};
use crate::interp::{fail, short, Ctrl, Interp};
use crate::parser::Parser;
use crate::server::{HttpRequest, HttpResponse};
use crate::value::*;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

type R = Result<Value, Ctrl>;

/// How old a signed call may be (seconds), either way: clocks differ a little.
const WINDOW: u64 = 300;

/// The secret services sign their calls with, if set.
fn secret() -> Option<String> {
    std::env::var("QUILLO_SERVICE_SECRET").ok().filter(|s| !s.is_empty())
}

/// `QUILLO_SERVICE_BILLING` for `billing`.
pub fn env_name(name: &str) -> String {
    format!("QUILLO_SERVICE_{}", name.to_ascii_uppercase())
}

/// Where service `name` runs, when not in this process: its addresses
/// (`http://a:8081,http://b:8081`, the same service on several servers).
pub fn urls_of(shared: &crate::shared::Shared, name: &str) -> Vec<String> {
    let given = match shared.service_urls.lock().unwrap_or_else(|e| e.into_inner()).get(name) {
        Some(u) => u.clone(),
        None => std::env::var(env_name(name)).unwrap_or_default(),
    };
    given.split(',').map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty()).collect()
}

/// One address of service `name`, for passing its routes on: the next that
/// is not left alone (see `pick`).
pub fn url_of(shared: &crate::shared::Shared, name: &str) -> Option<String> {
    let urls = urls_of(shared, name);
    pick(&urls, &[], &opts(name)).or_else(|| urls.first().cloned())
}

// ---- how a service is called ----------------------------------------------------------------

/// How calls to a service are made: what its declaration's options say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opts {
    /// How long one try may take.
    pub timeout: Duration,
    /// Tries after the first, when a call did not get through.
    pub retries: u32,
    /// Failures in a row after which an address is left alone...
    pub failures: u32,
    /// ...for this long (twice as long each time after, up to 16 times).
    pub cooldown: Duration,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts { timeout: Duration::from_secs(10), retries: 2, failures: 5, cooldown: Duration::from_secs(5) }
    }
}

fn opts_table() -> &'static Mutex<HashMap<String, Opts>> {
    static T: OnceLock<Mutex<HashMap<String, Opts>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

/// How service `name` is called.
pub fn opts(name: &str) -> Opts {
    opts_table().lock().unwrap_or_else(|e| e.into_inner()).get(name).copied().unwrap_or_default()
}

/// A duration given as milliseconds (`1500`) or text (`"1.5s"`, `"500ms"`, `"2m"`).
fn duration(v: &Value) -> Option<Duration> {
    match v {
        Value::Int(n) if *n >= 0 => Some(Duration::from_millis(*n as u64)),
        Value::Float(f) if *f >= 0.0 => Some(Duration::from_secs_f64(f / 1000.0)),
        Value::Str(s) => {
            let s = s.trim();
            let (num, unit) = s.split_at(s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len()));
            let n: f64 = num.trim().parse().ok().filter(|n: &f64| *n >= 0.0)?;
            let ms = match unit {
                "ms" => n,
                "s" | "" => n * 1000.0,
                "m" => n * 60_000.0,
                _ => return None,
            };
            Some(Duration::from_secs_f64(ms / 1000.0))
        }
        _ => None,
    }
}

/// `service billing = "billing.qwe" { timeout: "2s", retries: 2, failures: 5, cooldown: "5s" }`.
fn set_opts(name: &str, given: &Value, line: u32) -> Result<(), Ctrl> {
    let Value::Map(m) = given else {
        return fail(line, format!("service {}: its options are a map, like {{ timeout: \"2s\", retries: 2 }}", name));
    };
    let mut o = Opts::default();
    for (k, v) in m.iter() {
        let bad = |what: &str| fail(line, format!("service {}: `{}` is {}, not {}", name, k.as_str(), what, short(v)));
        match k.as_str() {
            "timeout" => match duration(v).filter(|d| !d.is_zero()) {
                Some(d) => o.timeout = d,
                None => return bad("a duration (\"2s\", \"500ms\", or milliseconds)"),
            },
            "cooldown" => match duration(v) {
                Some(d) => o.cooldown = d,
                None => return bad("a duration (\"5s\", or milliseconds)"),
            },
            "retries" => match v {
                Value::Int(n) if (0..=10).contains(n) => o.retries = *n as u32,
                _ => return bad("a number of tries after the first, 0 to 10"),
            },
            "failures" => match v {
                Value::Int(n) if *n >= 1 => o.failures = *n as u32,
                _ => return bad("how many failures in a row leave an address alone, 1 or more"),
            },
            other => {
                return fail(line, format!("service {}: no option `{}` (timeout, retries, failures, cooldown)", name, other));
            }
        }
    }
    opts_table().lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), o);
    Ok(())
}

// ---- addresses that fail: left alone a while ---------------------------------------------

/// An address's recent calls: failures in a row, and until when it is left
/// alone (its circuit open). When that time comes, one call tries it again
/// (and the next try after that waits another while, unless it went well).
#[derive(Default)]
struct Health {
    fails: u32,
    until: Option<Instant>,
}

fn health() -> &'static Mutex<HashMap<String, Health>> {
    static H: OnceLock<Mutex<HashMap<String, Health>>> = OnceLock::new();
    H.get_or_init(Default::default)
}

/// Whether to call `url` now (taking its one try, when its time has come).
fn usable(url: &str, o: &Opts) -> bool {
    let mut h = health().lock().unwrap_or_else(|e| e.into_inner());
    let Some(st) = h.get_mut(url) else { return true };
    match st.until {
        None => true,
        Some(t) if Instant::now() >= t => {
            st.until = Some(Instant::now() + o.cooldown);
            true
        }
        Some(_) => false,
    }
}

fn went_well(url: &str) {
    let mut h = health().lock().unwrap_or_else(|e| e.into_inner());
    if h.get(url).map_or(false, |st| st.fails > 0 || st.until.is_some()) {
        if h.get(url).map_or(false, |st| st.until.is_some()) {
            eprintln!("quillo: {} answers again", url);
        }
        h.remove(url);
    }
}

fn went_wrong(url: &str, o: &Opts) {
    let mut h = health().lock().unwrap_or_else(|e| e.into_inner());
    let st = h.entry(url.to_string()).or_default();
    st.fails += 1;
    if st.fails >= o.failures {
        let times = 1u32 << (st.fails - o.failures).min(4);
        let wait = o.cooldown * times;
        if st.until.is_none() {
            eprintln!("quillo: {} failed {} times in a row; left alone for {:.1}s", url, st.fails, wait.as_secs_f64());
        }
        st.until = Some(Instant::now() + wait);
    }
}

/// The next of `urls` to call, in turn: one not left alone, and not among
/// `tried` when another will do.
fn pick(urls: &[String], tried: &[String], o: &Opts) -> Option<String> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    if urls.is_empty() {
        return None;
    }
    let start = NEXT.fetch_add(1, Ordering::Relaxed);
    let order: Vec<&String> = (0..urls.len()).map(|i| &urls[(start + i) % urls.len()]).collect();
    let healthy = |u: &&String| health().lock().unwrap_or_else(|e| e.into_inner()).get(u.as_str()).map_or(true, |st| st.until.map_or(true, |t| Instant::now() >= t));
    let fresh = order.iter().copied().find(|u| !tried.contains(u) && healthy(u));
    let any = || order.iter().copied().find(|u| healthy(u));
    let u = fresh.or_else(any)?;
    usable(u, o).then(|| u.clone())
}

// ---- what a service offers ---------------------------------------------------------------

/// The statements of a program and the files it `use`s.
fn statements(path: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<Stmt>) -> Result<(), String> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(key) {
        return Ok(());
    }
    let src = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let stmts = Parser::new(&src).parse_program().map_err(|e| format!("{}: {}", path.display(), e))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    for s in stmts {
        if let Stmt::Use(file, _) = &s {
            statements(&dir.join(file), seen, out)?;
        }
        out.push(s);
    }
    Ok(())
}

/// The `server fn`s a service's program offers (its own and its `use`d files').
pub fn offered(path: &Path) -> Result<Vec<Arc<FnDef>>, String> {
    let mut stmts = Vec::new();
    statements(path, &mut HashSet::new(), &mut stmts)?;
    Ok(stmts
        .into_iter()
        .filter_map(|s| match s {
            Stmt::Fn(d) if d.server => Some(d),
            _ => None,
        })
        .collect())
}

/// Every service a program and those it runs declare: name, prefix, file.
/// For `--split`, which starts each.
pub fn declared(path: &Path) -> Result<Vec<(String, Option<String>, PathBuf)>, String> {
    let mut found: Vec<(String, Option<String>, PathBuf)> = Vec::new();
    let mut todo = vec![path.to_path_buf()];
    let mut done = HashSet::new();
    while let Some(p) = todo.pop() {
        if !done.insert(p.canonicalize().unwrap_or_else(|_| p.clone())) {
            continue;
        }
        let mut stmts = Vec::new();
        statements(&p, &mut HashSet::new(), &mut stmts)?;
        let dir = p.parent().unwrap_or(Path::new(".")).to_path_buf();
        for s in stmts {
            if let Stmt::Service(name, prefix, file, _, _) = s {
                let full = dir.join(&file);
                match found.iter_mut().find(|(n, _, _)| *n == name) {
                    Some((_, pre, _)) => {
                        if pre.is_none() {
                            *pre = prefix;
                        }
                    }
                    None => found.push((name, prefix, full.clone())),
                }
                todo.push(full);
            }
        }
    }
    Ok(found)
}

/// The value a service's name stands for: a map of functions, one per
/// `server fn` it offers, each taking that function's parameters.
pub fn namespace(it: &mut Interp, name: &str, defs: &[Arc<FnDef>], line: u32) -> R {
    let mut m = Map::new();
    let mut later = Map::new();
    for d in defs {
        let params: Vec<String> = d.params.iter().map(|p| if p.default.is_some() { format!("{} = null", p.name) } else { p.name.clone() }).collect();
        let names: Vec<&str> = d.params.iter().map(|p| p.name.as_str()).collect();
        let required = d.params.iter().filter(|p| p.default.is_none()).count();
        let mut made = |builtin: &str| -> R {
            let src = format!("({}) => {}({:?}, {:?}, [{}], {})", params.join(", "), builtin, name, d.name, names.join(", "), required);
            let lambda = Parser::new(&src).parse_expr().map_err(|e| Ctrl::Error(crate::error::QuilloError::new(e.msg, line)))?;
            let g = it.globals.clone();
            it.eval(&lambda, &g)
        };
        m.insert(d.name.as_str().into(), made("__service")?);
        later.insert(d.name.as_str().into(), made("__service_later")?);
    }
    // `billing.later.invoice(...)`: made in the background (see `later`).
    if !m.contains_key("later") {
        m.insert("later".into(), Value::map(later));
    }
    Ok(Value::map(m))
}

// ---- calling ---------------------------------------------------------------------------

/// `__service(name, fn, [args], required)`: a call to a service's server fn,
/// here or where it runs. Optional arguments left out are not sent, so the
/// service's own defaults apply. A service out of reach is an error value.
pub fn call(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (name, f, sent) = call_args(&args, "__service", line)?;
    let tenant = it.tenant();
    let rid = it.request_id.clone();
    Ok(make(it, &name, &f, sent, tenant, rid, None, line).value())
}

/// The service, the function and the arguments to send, from `__service`'s.
fn call_args(args: &[Value], who: &str, line: u32) -> Result<(String, String, Vec<Value>), Ctrl> {
    let (Some(Value::Str(name)), Some(Value::Str(f)), Some(Value::List(given))) = (args.first(), args.get(1), args.get(2)) else {
        return fail(line, format!("`{}` is Quillo's own: call a service as `billing.invoice(...)`", who));
    };
    let required = match args.get(3) {
        Some(Value::Int(n)) => *n as usize,
        _ => given.len(),
    };
    let mut sent: Vec<Value> = given.iter().cloned().collect();
    while sent.len() > required && matches!(sent.last(), Some(Value::Nil)) {
        sent.pop();
    }
    Ok((name.to_string(), f.to_string(), sent))
}

/// What became of a call.
enum Outcome {
    /// The function ran: its value (an error value too).
    Ran(Value),
    /// It did not run, or may not have (no answer in time): why.
    Missed(String),
}

impl Outcome {
    fn value(self) -> Value {
        match self {
            Outcome::Ran(v) => v,
            Outcome::Missed(why) => Value::err(Value::str(why)),
        }
    }
}

/// A call, here or where the service runs, with call id `call` (made if
/// not given: the same id for each try, so the call runs once).
#[allow(clippy::too_many_arguments)]
fn make(it: &mut Interp, name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>, rid: Option<Arc<str>>, call: Option<String>, line: u32) -> Outcome {
    let urls = urls_of(&it.shared, name);
    if urls.is_empty() {
        return local(it, name, f, args, tenant, rid, line);
    }
    remote(it, &urls, name, f, args, tenant, rid, call.unwrap_or_else(|| new_id(16)))
}

/// A random id of `n` hex digits.
pub fn new_id(n: usize) -> String {
    (0..n).map(|_| char::from_digit(fastrand::u32(..16), 16).unwrap()).collect()
}

/// A request's id: the `x-request-id` it came with, if it is a plain one,
/// else a new one.
pub fn request_id(given: Option<&str>) -> Arc<str> {
    match given {
        Some(g) if !g.is_empty() && g.len() <= 64 && g.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b)) => Arc::from(g),
        _ => Arc::from(new_id(16)),
    }
}

/// A copy of a value, as the other side of a network would have it (an
/// error stays one, as the answer of a call says so).
fn crossed(v: &Value) -> Value {
    match v {
        Value::Err(e) => Value::err(crossed(e)),
        v => Value::from_json(&v.to_json()),
    }
}

fn local(it: &mut Interp, name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>, rid: Option<Arc<str>>, line: u32) -> Outcome {
    let found = it.services.loaded.lock().unwrap_or_else(|e| e.into_inner()).get(name).map(|(_, s)| s.fork());
    let Some(mut svc) = found else {
        return Outcome::Missed(format!("service {}: not running here, and no address for it ({} or --service-url)", name, env_name(name)));
    };
    let Some(Value::Func(c)) = lookup(&svc.globals, f).filter(|_| svc.server_fns.iter().any(|n| n == f)) else {
        return Outcome::Ran(Value::err(Value::str(format!("service {}: no server fn `{}`", name, f))));
    };
    let args: Vec<Value> = args.iter().map(crossed).collect();
    if let Err(msg) = crate::server::check_args(&svc.types, &c.def, &args) {
        return Outcome::Ran(Value::err(Value::str(format!("service {}: {}", name, msg))));
    }
    svc.tenant = Some(tenant.clone());
    svc.tenant_named = tenant.is_some();
    svc.request_id = rid;
    let err = |msg: String| Outcome::Ran(Value::err(Value::str(msg)));
    match svc.call_closure(&c, args.into_iter().map(Some).collect(), line) {
        Ok(v) | Err(Ctrl::Return(v)) => match v {
            Value::Response(_) => err(format!("service {}: `{}` answered with a page, not a value", name, f)),
            v => Outcome::Ran(crossed(&v)),
        },
        Err(Ctrl::Reply(_)) => err(format!("service {}: `{}` replied with a page, not a value", name, f)),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: service {}: error in `{}`: {}", name, f, e);
            err(format!("service {}: internal error in `{}`", name, f))
        }
    }
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .redirects(0)
            .user_agent(concat!("quillo/", env!("CARGO_PKG_VERSION")))
            .build()
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The signature of a call: the time, the function, the tenant and the
/// arguments, under the shared secret.
fn signature(secret: &str, t: u64, f: &str, tenant: &str, body: &[u8]) -> String {
    let mut msg = format!("{}\n{}\n{}\n", t, f, tenant).into_bytes();
    msg.extend_from_slice(body);
    crate::crypto::hex(&crate::crypto::hmac_sha256(secret.as_bytes(), &msg))
}

/// How one try went.
enum Try {
    Answer(u16, String),
    /// No connection: the call was not sent.
    NotSent(String),
    /// Sent, but no answer (in time): it may have run.
    NoAnswer(String),
}

/// A call to where the service runs: tried at its addresses in turn (one
/// left alone when it keeps failing) until one answers, as often as its
/// options say. A try that got no connection, or a busy or missing server
/// (502, 503, 504), is made again elsewhere; one sent but not answered in
/// time, again at the same address, with the same call id, which that
/// server remembers, so the function runs once.
#[allow(clippy::too_many_arguments)]
fn remote(it: &mut Interp, urls: &[String], name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>, rid: Option<Arc<str>>, call: String) -> Outcome {
    let Some(secret) = secret() else {
        return Outcome::Missed(format!("service {}: runs at {}, but QUILLO_SERVICE_SECRET is not set to sign the call", name, urls.join(",")));
    };
    let o = opts(name);
    let body = serde_json::Value::Array(args.iter().map(|a| a.to_json()).collect()).to_string().into_bytes();
    let tenant = tenant.map(|t| t.to_string()).unwrap_or_default();
    let rid = rid.map(|r| r.to_string()).unwrap_or_default();
    let caller = if it.base.is_empty() { "/".to_string() } else { it.base.clone() };
    let mut tried: Vec<String> = Vec::new();
    let mut same: Option<String> = None;
    let mut why = String::new();
    let start = Instant::now();
    for attempt in 0..=o.retries {
        let url = match same.clone().or_else(|| pick(urls, &tried, &o)) {
            Some(u) => u,
            None if why.is_empty() => return Outcome::Missed(format!("service {}: unavailable (every address failed lately: {})", name, urls.join(", "))),
            None => break,
        };
        if attempt > 0 {
            // A little longer each time, not all at once.
            let wait = Duration::from_millis(100 << (attempt - 1)).mul_f64(1.0 + fastrand::f64() / 2.0);
            it.unlocked(move || std::thread::sleep(wait));
        }
        let t = now_secs();
        let sig = signature(&secret, t, f, &tenant, &body);
        let endpoint = format!("{}/_quillo/service/{}", url, f);
        let (b, tn, r, c, cl) = (body.clone(), tenant.clone(), rid.clone(), call.clone(), caller.clone());
        let timeout = o.timeout;
        let got = it.unlocked(move || -> Try {
            let mut rq = agent()
                .post(&endpoint)
                .timeout(timeout)
                .set("content-type", "application/json")
                .set("x-quillo-service", &format!("t={},sig={}", t, sig))
                .set("x-quillo-call", &c)
                .set("x-quillo-caller", &cl);
            if !tn.is_empty() {
                rq = rq.set("x-quillo-tenant", &tn);
            }
            if !r.is_empty() {
                rq = rq.set("x-request-id", &r);
            }
            let resp = match rq.send_bytes(&b) {
                Ok(r) | Err(ureq::Error::Status(_, r)) => r,
                Err(ureq::Error::Transport(e)) if matches!(e.kind(), ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Dns) => return Try::NotSent(e.to_string()),
                Err(e) => return Try::NoAnswer(e.to_string()),
            };
            let status = resp.status();
            let mut text = String::new();
            match resp.into_reader().take(64 << 20).read_to_string(&mut text) {
                Ok(_) => Try::Answer(status, text),
                Err(e) => Try::NoAnswer(e.to_string()),
            }
        });
        match got {
            Try::Answer(status @ (502..=504), text) => {
                went_wrong(&url, &o);
                why = format!("service {}: {} at {} ({})", name, status, url, reason(&text));
                // A call still running there (503) is asked of it again.
                if status != 503 || same.is_none() {
                    tried.push(url);
                    same = None;
                }
            }
            Try::Answer(status, text) => {
                went_well(&url);
                log_call(name, f, &url, status, start, attempt + 1, &rid);
                return Outcome::Ran(match status {
                    200 => match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(j) if j.get("ok").is_some() => Value::from_json(&j["ok"]),
                        Ok(j) if j.get("err").is_some() => Value::err(Value::from_json(&j["err"])),
                        _ => Value::err(Value::str(format!("service {}: an answer that is not one", name))),
                    },
                    _ => Value::err(Value::str(format!("service {}: {} {}", name, status, reason(&text)))),
                });
            }
            Try::NotSent(e) => {
                went_wrong(&url, &o);
                why = format!("service {}: unreachable at {} ({})", name, url, e);
                tried.push(url);
            }
            Try::NoAnswer(e) => {
                went_wrong(&url, &o);
                why = format!("service {}: no answer from {} ({})", name, url, e);
                same = Some(url);
            }
        }
        if attempt < o.retries {
            eprintln!("quillo: {}; trying again{}", why, if rid.is_empty() { String::new() } else { format!(" (request {})", rid) });
        }
    }
    log_call(name, f, tried.last().map_or("", |u| u.as_str()), 0, start, o.retries + 1, &rid);
    Outcome::Missed(why)
}

/// What an error answer says.
fn reason(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text).ok().and_then(|j| j["error"].as_str().map(String::from)).unwrap_or_else(|| text.to_string()).trim().to_string()
}

/// A call to a service, in the JSON log: for following a request across services.
fn log_call(name: &str, f: &str, url: &str, status: u16, start: Instant, tries: u32, rid: &str) {
    if crate::metrics::LOG.load(Ordering::Relaxed) != crate::metrics::JSON {
        return;
    }
    crate::httpd::log_fmt(format_args!(
        "{{\"time\":\"{}\",\"service\":{},\"fn\":{},\"url\":{},\"status\":{},\"ms\":{:.2},\"tries\":{},\"rid\":{}}}\n",
        crate::httpd::iso_now(),
        serde_json::Value::from(name),
        serde_json::Value::from(f),
        serde_json::Value::from(url),
        status,
        start.elapsed().as_secs_f64() * 1000.0,
        tries,
        serde_json::Value::from(rid)
    ));
}

// ---- answering -------------------------------------------------------------------------

fn json(status: u16, j: serde_json::Value) -> HttpResponse {
    HttpResponse { status, headers: vec![("content-type".into(), "application/json".into())], body: j.to_string().into_bytes() }
}

fn refused(status: u16, msg: &str) -> HttpResponse {
    json(status, serde_json::json!({ "error": msg }))
}

/// `POST /_quillo/service/<fn>`: another service calling one of this
/// program's server fns. Only a call signed with the shared secret, made
/// in the last few minutes, is answered; it runs in the tenant it names.
pub fn answer(it: &mut Interp, f: &str, req: &HttpRequest) -> HttpResponse {
    if req.method != "POST" {
        return refused(405, "a service call is a POST");
    }
    let Some(secret) = secret() else {
        return refused(403, "service calls need QUILLO_SERVICE_SECRET, on both sides");
    };
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let sig = header("x-quillo-service").unwrap_or("");
    let t = sig.split(',').find_map(|p| p.strip_prefix("t=")).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let given = sig.split(',').find_map(|p| p.strip_prefix("sig=")).unwrap_or("");
    let tenant = header("x-quillo-tenant").unwrap_or("");
    let expected = signature(&secret, t, f, tenant, &req.body);
    if now_secs().abs_diff(t) > WINDOW || !crate::crypto::same(expected.as_bytes(), given.as_bytes()) {
        return refused(401, "not signed with this server's QUILLO_SERVICE_SECRET (or too old)");
    }
    if !tenant.is_empty() && !crate::site::tenant_name_ok(tenant) {
        return refused(400, "not a tenant's name");
    }
    // A call sent again (it got no answer in time, or is a call made later,
    // delivered twice): answered as it was, not run again.
    let call = header("x-quillo-call").filter(|c| !c.is_empty() && c.len() <= 64 && c.bytes().all(|b| b.is_ascii_alphanumeric()));
    if let Some(c) = call {
        let mut seen = seen().lock().unwrap_or_else(|e| e.into_inner());
        match seen.get(c) {
            Some((_, Some(r))) => return r.clone(),
            Some((_, None)) => return refused(503, "this call is still running"),
            None => {
                if seen.len() >= SEEN_MAX {
                    let now = Instant::now();
                    seen.retain(|_, (at, _)| now.duration_since(*at) < SEEN_FOR);
                }
                seen.insert(c.to_string(), (Instant::now(), None));
            }
        }
    }
    let r = run_call(it, f, tenant, req);
    if let Some(c) = call {
        let mut seen = seen().lock().unwrap_or_else(|e| e.into_inner());
        // A call that did not run (a busy server) may be sent again.
        if r.status >= 502 {
            seen.remove(c);
        } else {
            seen.insert(c.to_string(), (Instant::now(), Some(r.clone())));
        }
    }
    r
}

/// Calls answered lately, by call id: when, and the answer (none while it runs).
fn seen() -> &'static Mutex<HashMap<String, (Instant, Option<HttpResponse>)>> {
    static S: OnceLock<Mutex<HashMap<String, (Instant, Option<HttpResponse>)>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// How long, and how many, calls are remembered.
const SEEN_FOR: Duration = Duration::from_secs(600);
const SEEN_MAX: usize = 100_000;

fn run_call(it: &mut Interp, f: &str, tenant: &str, req: &HttpRequest) -> HttpResponse {
    let Some(Value::Func(c)) = lookup(&it.globals, f).filter(|_| it.server_fns.iter().any(|n| n == f)) else {
        return refused(404, &format!("no server fn `{}`", f));
    };
    let args = match serde_json::from_slice::<serde_json::Value>(&req.body) {
        Ok(serde_json::Value::Array(a)) => a.iter().map(Value::from_json).collect::<Vec<_>>(),
        _ => return refused(400, "the body must be a JSON array of arguments"),
    };
    if let Err(msg) = crate::server::check_args(&it.types, &c.def, &args) {
        return refused(400, &msg);
    }
    it.tenant = Some(if tenant.is_empty() { None } else { Some(Arc::from(tenant)) });
    it.tenant_named = !tenant.is_empty();
    // Called by another service: `request` is the request it came from, as
    // far as it goes with the call (its id).
    if let Some(id) = &it.request_id {
        let mut m = Map::new();
        m.insert("id".into(), Value::str(&**id));
        it.current_request = Some(Value::map(m));
    }
    match it.call_closure(&c, args.into_iter().map(Some).collect(), 0) {
        Ok(v) | Err(Ctrl::Return(v)) => match v {
            Value::Response(_) => refused(500, &format!("`{}` answered with a page, not a value", f)),
            Value::Err(e) => json(200, serde_json::json!({ "err": e.to_json() })),
            v => json(200, serde_json::json!({ "ok": v.to_json() })),
        },
        Err(Ctrl::Reply(_)) => refused(500, &format!("`{}` replied with a page, not a value", f)),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: error in server fn `{}` (called by a service): {}", f, e);
            refused(500, &format!("internal error in `{}`", f))
        }
    }
}

/// Quillo's own addresses that are not passed on to a service under its
/// prefix: calls of its functions, and what only its own server or other
/// services ask of it.
const NOT_PASSED_ON: &[&str] = &["rpc", "service", "job", "later", "channel", "udp", "wt-ticket", "announce", "metrics"];

/// A request under a service's prefix, when the service runs elsewhere:
/// passed on to it, without the prefix (as a mounted program sees it).
pub fn pass_on(it: &mut Interp, url: &str, prefix: &str, req: &HttpRequest) -> HttpResponse {
    let rest = &req.url[prefix.len()..];
    let path = rest.split('?').next().unwrap_or("");
    if let Some(inner) = path.trim_start_matches('/').strip_prefix("_quillo/") {
        if NOT_PASSED_ON.contains(&inner.split('/').next().unwrap_or("")) {
            return crate::server::plain_response(404, "not found\n");
        }
    }
    let target = format!("{}{}", url, if rest.is_empty() || rest.starts_with('?') { format!("/{}", rest) } else { rest.to_string() });
    const HOP: &[&str] = &["host", "connection", "content-length", "transfer-encoding", "keep-alive", "upgrade", "te", "trailer", "proxy-authorization"];
    let headers: Vec<(String, String)> = req.headers.iter().filter(|(k, _)| !HOP.iter().any(|h| k.eq_ignore_ascii_case(h))).cloned().collect();
    let (method, body, remote) = (req.method.clone(), req.body.clone(), req.remote.clone());
    let host = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("host")).map(|(_, v)| v.clone()).unwrap_or_default();
    let prefix = prefix.to_string();
    let res = it.unlocked(move || -> Result<HttpResponse, String> {
        let mut rq = agent().request(&method, &target);
        for (k, v) in &headers {
            rq = rq.set(k, v);
        }
        rq = rq.set("x-forwarded-for", &remote).set("x-forwarded-host", &host).set("x-forwarded-prefix", &prefix);
        let resp = match if body.is_empty() { rq.call() } else { rq.send_bytes(&body) } {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let headers = resp
            .headers_names()
            .into_iter()
            .filter(|n| !HOP.iter().any(|h| n.eq_ignore_ascii_case(h)))
            .flat_map(|n| resp.all(&n).into_iter().map(|v| (n.to_ascii_lowercase(), v.to_string())).collect::<Vec<_>>())
            .collect();
        let mut bytes = Vec::new();
        resp.into_reader().take(256 << 20).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, headers, body: bytes })
    });
    res.unwrap_or_else(|e| HttpResponse {
        status: 502,
        headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
        body: format!("this part of the site is not answering ({})\n", e).into_bytes(),
    })
}

// ---- loading -------------------------------------------------------------------------------

/// `service name [/prefix] = "file.qwe"`, run by the program declaring it:
/// the name stands for the service's functions; the service is loaded here
/// (once per process) unless it runs elsewhere; under its prefix, its
/// routes are served (here, or passed on to it).
pub fn declare(it: &mut Interp, name: &str, prefix: Option<&str>, file: &str, options: Option<&Value>, line: u32) -> R {
    if let Some(o) = options {
        set_opts(name, o, line)?;
    }
    let full = it.base_dir.join(file);
    let defs = offered(&full).map_err(|e| Ctrl::Error(crate::error::QuilloError::new(e, line)))?;
    let ns = namespace(it, name, &defs, line)?;
    if let Some(url) = url_of(&it.shared, name) {
        if let Some(p) = prefix {
            it.remote_mounts.retain(|(q, _)| q != p);
            it.remote_mounts.push((p.to_string(), url));
        }
    } else {
        let key = full.canonicalize().unwrap_or_else(|_| full.clone());
        let existing = it.services.loaded.lock().unwrap_or_else(|e| e.into_inner()).get(name).map(|(p, s)| (p.clone(), s.fork()));
        let svc = match existing {
            Some((p, s)) if p == key => Some(s),
            Some((p, _)) => return fail(line, format!("service `{}` is {} elsewhere in this app, and {} here", name, p.display(), key.display())),
            None if it.services.loading.lock().unwrap_or_else(|e| e.into_inner()).contains(name) => None,
            None => {
                it.services.loading.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string());
                let dir = full.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| it.base_dir.to_path_buf());
                let mut sub = Interp::with_shared(dir, it.shared.clone());
                sub.services = it.services.clone();
                sub.is_service = true;
                let loaded = sub.load_file(&full).map_err(|e| Ctrl::Error(e.in_file(&full.display().to_string())));
                it.services.loading.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
                loaded?;
                it.services.loaded.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), (key, sub.fork()));
                Some(sub)
            }
        };
        if let Some(svc) = svc {
            Arc::make_mut(&mut it.loaded).extend(svc.files());
            if let Some(p) = prefix {
                let mut m = svc;
                m.base = format!("{}{}", it.base, p);
                it.mounts.retain(|(q, _)| q != p);
                it.mounts.push((p.to_string(), m));
            }
        }
    }
    define(&it.globals, name, ns);
    Ok(Value::Nil)
}

// ---- calls made later ---------------------------------------------------------------------

/// A call to be made in the background (`billing.later.invoice(...)`).
#[derive(Clone)]
pub struct Later {
    pub id: String,
    pub service: String,
    pub f: String,
    /// The arguments, as JSON.
    pub args: String,
    pub tenant: String,
    pub rid: String,
    /// When to try it next (milliseconds since 1970), and the tries so far.
    pub at: i64,
    pub tries: i64,
}

/// Calls made later, when the program has no one database to keep them in.
fn waiting() -> &'static Mutex<Vec<Later>> {
    static W: OnceLock<Mutex<Vec<Later>>> = OnceLock::new();
    W.get_or_init(Default::default)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// `__service_later(name, fn, [args], required)`: the call is kept (in the
/// program's database when it has one, so it outlives a restart; in memory
/// otherwise) and made in the background, again and again until it gets
/// through; its id is the result. The service remembers the id, so the
/// function runs once.
pub fn later(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (service, f, sent) = call_args(&args, "__service_later", line)?;
    let call = Later {
        id: new_id(24),
        service,
        f,
        args: serde_json::Value::Array(sent.iter().map(|a| a.to_json()).collect()).to_string(),
        tenant: it.tenant().map(|t| t.to_string()).unwrap_or_default(),
        rid: it.request_id.as_deref().unwrap_or("").to_string(),
        at: now_ms(),
        tries: 0,
    };
    let id = call.id.clone();
    if !crate::db::later_put(it, &call) {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| eprintln!("quillo: calls made later are kept in memory (no database could keep them): a restart loses those not made yet"));
        waiting().lock().unwrap_or_else(|e| e.into_inner()).push(call);
    }
    Ok(Value::str(id))
}

/// How long until a call that did not get through is tried again: a second,
/// then twice as long each time, up to five minutes.
fn backoff(tries: i64) -> i64 {
    (1000i64 << tries.clamp(0, 9)).min(300_000)
}

/// Makes the calls that are due (asked by the server's scheduler, each
/// second): those that get through are done with; the others wait longer.
pub fn deliver(it: &mut Interp) -> HttpResponse {
    // Every half minute: transactions over several databases left part way.
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let due = {
        let mut l = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let due = l.map_or(true, |t| t.elapsed() >= Duration::from_secs(30));
        if due {
            *l = Some(Instant::now());
        }
        due
    };
    if due {
        crate::db::recover(it);
    }
    let now = now_ms();
    let due: Vec<Later> = {
        let mut w = waiting().lock().unwrap_or_else(|e| e.into_inner());
        let (due, rest): (Vec<Later>, Vec<Later>) = std::mem::take(&mut *w).into_iter().partition(|c| c.at <= now);
        *w = rest;
        due
    };
    for mut c in due {
        if let Err(why) = make_later(it, &c) {
            c.tries += 1;
            c.at = now_ms() + backoff(c.tries);
            eprintln!("quillo: {}.{} (made later, {}) not made yet: {}; again in {}s", c.service, c.f, c.id, why, backoff(c.tries) / 1000);
            waiting().lock().unwrap_or_else(|e| e.into_inner()).push(c);
        }
    }
    // Kept in the database: each taken by one server (others may share it).
    for c in crate::db::later_due(it, now, 50) {
        if !crate::db::later_take(it, &c.id, c.at, now_ms() + 60_000) {
            continue;
        }
        match make_later(it, &c) {
            Ok(()) => crate::db::later_done(it, &c.id),
            Err(why) => {
                let tries = c.tries + 1;
                eprintln!("quillo: {}.{} (made later, {}) not made yet: {}; again in {}s", c.service, c.f, c.id, why, backoff(tries) / 1000);
                crate::db::later_again(it, &c.id, now_ms() + backoff(tries), tries, &why);
            }
        }
    }
    HttpResponse { status: 200, headers: Vec::new(), body: b"ok\n".to_vec() }
}

/// One call made later: Ok when it got through (whatever the function gave).
fn make_later(it: &mut Interp, c: &Later) -> Result<(), String> {
    let args = match serde_json::from_str::<serde_json::Value>(&c.args) {
        Ok(serde_json::Value::Array(a)) => a.iter().map(Value::from_json).collect(),
        _ => Vec::new(),
    };
    let tenant = (!c.tenant.is_empty()).then(|| Arc::from(c.tenant.as_str()));
    let rid = (!c.rid.is_empty()).then(|| Arc::from(c.rid.as_str()));
    match make(it, &c.service, &c.f, args, tenant, rid, Some(c.id.clone()), 0) {
        Outcome::Ran(Value::Err(e)) => {
            eprintln!("quillo: {}.{} (made later, {}) gave an error: {}", c.service, c.f, c.id, short(&e));
            Ok(())
        }
        Outcome::Ran(_) => Ok(()),
        Outcome::Missed(why) => Err(why),
    }
}

/// Whether a program declares services (then its calls made later are
/// delivered).
pub fn uses_services(it: &Interp) -> bool {
    it.program.iter().any(|stmts| stmts.iter().any(|s| matches!(s, Stmt::Service(..))))
}

/// Asks the server at `addr`, each second, to make the calls that are due
/// (see `deliver`), as the scheduler asks it to run jobs.
pub fn start_deliveries(addr: std::net::SocketAddr) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let addr = match addr.ip() {
        ip if ip.is_unspecified() && ip.is_ipv4() => std::net::SocketAddr::from(([127, 0, 0, 1], addr.port())),
        ip if ip.is_unspecified() => std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, addr.port())),
        _ => addr,
    };
    let _ = std::thread::Builder::new().name("quillo-later".into()).spawn(move || loop {
        if crate::httpd::stopping() {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
        let asked = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
            write!(
                s,
                "POST /_quillo/later HTTP/1.1\r\nHost: localhost\r\nX-Quillo-Job: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                crate::jobs::token()
            )?;
            let mut out = Vec::new();
            s.read_to_end(&mut out)?;
            Ok(())
        })();
        let _ = asked;
    });
}

// ---- running split ---------------------------------------------------------------------

/// The processes `--split` started: stopped with it.
pub struct Split {
    children: Vec<std::process::Child>,
}

impl Drop for Split {
    fn drop(&mut self) {
        for c in self.children.iter_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// `quillo run app.qwe --split`: each service the app declares (and those
/// they declare) runs as a process of its own, on a port of its own, its
/// log lines marked with its name; the app (and each service) is told
/// where the others are, and they sign their calls with one secret.
pub fn split(path: &Path, watch: bool, static_check: bool) -> Result<Split, String> {
    let services = declared(path)?;
    if std::env::var("QUILLO_SERVICE_SECRET").map_or(true, |s| s.is_empty()) {
        let mut b = [0u8; 24];
        for x in b.iter_mut() {
            *x = fastrand::u8(..);
        }
        std::env::set_var("QUILLO_SERVICE_SECRET", crate::crypto::hex(&b));
    }
    let mut ports = Vec::new();
    for (name, _, _) in &services {
        let port = free_port().map_err(|e| e.to_string())?;
        std::env::set_var(env_name(name), format!("http://127.0.0.1:{}", port));
        ports.push(port);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut split = Split { children: Vec::new() };
    for ((name, prefix, file), port) in services.iter().zip(&ports) {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("run").arg(file).args(["--port", &port.to_string(), "--as-service"]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if let Some(p) = prefix {
            cmd.args(["--base", p]);
        }
        if watch {
            cmd.arg("--watch");
        }
        if !static_check {
            cmd.arg("--no-check");
        }
        let mut child = cmd.spawn().map_err(|e| format!("cannot start service {}: {}", name, e))?;
        for out in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
            let tag = name.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    eprintln!("[{}] {}", tag, l);
                }
            });
        }
        eprintln!("quillo: service {} ({}) on http://127.0.0.1:{}{}", name, file.display(), port, prefix.as_ref().map_or(String::new(), |p| format!(", served under {}", p)));
        split.children.push(child);
    }
    // Ready when each answers.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    for ((name, _, _), port) in services.iter().zip(&ports) {
        while std::net::TcpStream::connect(("127.0.0.1", *port)).is_err() {
            if std::time::Instant::now() > deadline {
                return Err(format!("service {} did not start (see its lines above)", name));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(split)
}
